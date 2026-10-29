//! The launchers' renderer: rounded plates, rings, gradients, images and text
//! in the design system's fonts, drawn with wgpu into the window. Text and
//! images share one atlas. Coordinates are dp (the launcher's logical pixels).
use ab_glyph::{Font as _, FontRef, GlyphId, PxScale, ScaleFont as _, point};
use std::collections::HashMap;
use std::sync::Arc;
use wgpu::util::DeviceExt;
use winit::window::Window;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

const ATLAS: u32 = 2048;

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct Vertex {
    /// Physical pixels.
    pos: [f32; 2],
    /// From the shape's centre, in physical pixels (the rounded box's distance).
    local: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
    /// Half width, half height, corner radius, and the ring's width (0 fills,
    /// below 0 samples the atlas).
    shape: [f32; 4],
}

const SHADER: &str = r#"
struct Screen { size: vec2f, pad: vec2f }
@group(0) @binding(0) var<uniform> screen: Screen;
@group(0) @binding(1) var atlas: texture_2d<f32>;
@group(0) @binding(2) var atlas_sampler: sampler;

struct Out {
    @builtin(position) pos: vec4f,
    @location(0) local: vec2f,
    @location(1) uv: vec2f,
    @location(2) color: vec4f,
    @location(3) shape: vec4f,
}

@vertex
fn vs(@location(0) pos: vec2f, @location(1) local: vec2f, @location(2) uv: vec2f,
      @location(3) color: vec4f, @location(4) shape: vec4f) -> Out {
    var out: Out;
    out.pos = vec4f(pos.x / screen.size.x * 2.0 - 1.0, 1.0 - pos.y / screen.size.y * 2.0, 0.0, 1.0);
    out.local = local;
    out.uv = uv;
    out.color = color;
    out.shape = shape;
    return out;
}

@fragment
fn fs(in: Out) -> @location(0) vec4f {
    if (in.shape.w < 0.0) {
        return in.color * textureSample(atlas, atlas_sampler, in.uv);
    }
    let r = min(in.shape.z, min(in.shape.x, in.shape.y));
    let q = abs(in.local) - in.shape.xy + vec2f(r);
    let d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - r;
    var a = clamp(0.5 - d, 0.0, 1.0);
    if (in.shape.w > 0.0) {
        a = a * clamp(0.5 + d + in.shape.w, 0.0, 1.0);
    }
    return vec4f(in.color.rgb, in.color.a * a);
}
"#;

/// A colour from a palette token (`0xRRGGBB`) and an opacity.
pub fn hex(c: u32, alpha: f32) -> [f32; 4] {
    let f = |shift: u32| ((c >> shift) & 0xff) as f32 / 255.0;
    [f(16), f(8), f(0), alpha]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Face {
    Regular,
    Medium,
    Mono,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }
    pub fn contains(&self, (x, y): (f32, f32)) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.w && y < self.y + self.h
    }
    pub fn right(&self) -> f32 {
        self.x + self.w
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }
}

#[derive(Clone, Copy)]
struct Glyph {
    /// Atlas pixels; none for a blank glyph.
    at: Option<[u32; 4]>,
    left: f32,
    top: f32,
}

/// Rasterized glyphs in the atlas, by face, glyph and pixel size.
struct Glyphs {
    faces: [FontRef<'static>; 3],
    entries: HashMap<(Face, u16, u32), Glyph>,
    shelf: (u32, u32, u32),
    uploads: Vec<([u32; 4], Vec<u8>)>,
}

impl Glyphs {
    fn new() -> Self {
        let f = |b: &'static [u8]| FontRef::try_from_slice(b).expect("bundled font");
        Self {
            faces: [
                f(horizon_theme::fonts::GEIST_REGULAR),
                f(horizon_theme::fonts::GEIST_MEDIUM),
                f(horizon_theme::fonts::GEIST_MONO),
            ],
            entries: HashMap::new(),
            // The first row holds a white texel for images of a flat colour.
            shelf: (4, 0, 4),
            uploads: vec![([0, 0, 2, 2], vec![255; 16])],
        }
    }
    fn face(&self, face: Face) -> &FontRef<'static> {
        &self.faces[face as usize]
    }
    fn scale(&self, face: Face, px: f32) -> PxScale {
        let font = self.face(face);
        let upem = font.units_per_em().unwrap_or(1000.0);
        PxScale::from(px * font.height_unscaled() / upem)
    }
    fn place(&mut self, w: u32, h: u32) -> Option<[u32; 2]> {
        let (mut x, mut y, mut row) = self.shelf;
        if x + w + 1 > ATLAS {
            x = 0;
            y += row + 1;
            row = 0;
        }
        if y + h + 1 > ATLAS {
            return None;
        }
        self.shelf = (x + w + 1, y, row.max(h));
        Some([x, y])
    }
    fn glyph(&mut self, face: Face, id: GlyphId, px: u32) -> Glyph {
        let key = (face, id.0, px);
        if let Some(g) = self.entries.get(&key) {
            return *g;
        }
        let scale = self.scale(face, px as f32);
        let outlined = self
            .face(face)
            .outline_glyph(id.with_scale_and_position(scale, point(0.0, 0.0)));
        let glyph = match outlined {
            None => Glyph {
                at: None,
                left: 0.0,
                top: 0.0,
            },
            Some(outline) => {
                let bounds = outline.px_bounds();
                let (w, h) = (bounds.width() as u32, bounds.height() as u32);
                let mut rgba = vec![0u8; (w * h * 4) as usize];
                outline.draw(|x, y, a| {
                    let i = ((y * w + x) * 4) as usize;
                    if i + 3 < rgba.len() {
                        rgba[i..i + 3].fill(255);
                        rgba[i + 3] = (a.clamp(0.0, 1.0) * 255.0).round() as u8;
                    }
                });
                match self.place(w, h) {
                    Some([x, y]) if w > 0 && h > 0 => {
                        self.uploads.push(([x, y, w, h], rgba));
                        Glyph {
                            at: Some([x, y, w, h]),
                            left: bounds.min.x,
                            top: bounds.min.y,
                        }
                    }
                    _ => Glyph {
                        at: None,
                        left: 0.0,
                        top: 0.0,
                    },
                }
            }
        };
        self.entries.insert(key, glyph);
        glyph
    }
}

/// An image placed in the atlas (`Renderer::image`).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Image {
    /// Atlas pixels: x, y, width, height.
    at: [u32; 4],
}

impl Image {
    pub fn width(&self) -> u32 {
        self.at[2]
    }
    pub fn height(&self) -> u32 {
        self.at[3]
    }
}

/// An RGBA image from PNG bytes.
pub fn decode_png(bytes: &[u8]) -> Option<(Vec<u8>, u32, u32)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    let (w, h) = (info.width, info.height);
    let pixels = &buf[..info.buffer_size()];
    let rgba = match info.color_type.samples() {
        4 => pixels.to_vec(),
        3 => pixels
            .chunks_exact(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        2 => pixels
            .chunks_exact(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        1 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        _ => return None,
    };
    Some((rgba, w, h))
}

/// `rgba` (`w` by `h`) scaled to `nw` by `nh`, by nearest texel.
pub fn resize_nearest(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (nw * nh * 4) as usize];
    for y in 0..nh {
        for x in 0..nw {
            let (sx, sy) = (x * w / nw, y * h / nh);
            let s = ((sy * w + sx) * 4) as usize;
            let d = ((y * nw + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&rgba[s..s + 4]);
        }
    }
    out
}

/// One frame's drawing, in dp.
pub struct Canvas<'a> {
    glyphs: &'a mut Glyphs,
    vertices: Vec<Vertex>,
    /// Physical pixels per dp.
    pub scale: f32,
    pub width: f32,
    pub height: f32,
}

impl Canvas<'_> {
    fn quad(&mut self, r: Rect, uv: [f32; 4], color: [f32; 4], shape: [f32; 4]) {
        let s = self.scale;
        let (x0, y0, x1, y1) = (r.x * s, r.y * s, (r.x + r.w) * s, (r.y + r.h) * s);
        let (hw, hh) = ((x1 - x0) / 2.0, (y1 - y0) / 2.0);
        let corners = [
            ([x0, y0], [-hw, -hh], [uv[0], uv[1]]),
            ([x1, y0], [hw, -hh], [uv[2], uv[1]]),
            ([x1, y1], [hw, hh], [uv[2], uv[3]]),
            ([x0, y1], [-hw, hh], [uv[0], uv[3]]),
        ];
        for i in [0, 1, 2, 0, 2, 3] {
            let (pos, local, uv) = corners[i];
            self.vertices.push(Vertex {
                pos,
                local,
                uv,
                color,
                shape: [hw, hh, shape[2], shape[3]],
            });
        }
    }
    /// A filled plate with rounded corners (`radius` in dp).
    pub fn rect(&mut self, r: Rect, radius: f32, color: [f32; 4]) {
        if color[3] <= 0.0 {
            return;
        }
        self.quad(r, [0.0; 4], color, [0.0, 0.0, radius * self.scale, 0.0]);
    }
    /// A part of `image` (`src`: left, top, right, bottom from 0 to 1) in `r`,
    /// multiplied by `color`.
    pub fn image(&mut self, image: Image, r: Rect, src: [f32; 4], color: [f32; 4]) {
        let [x, y, w, h] = image.at.map(|v| v as f32);
        let a = ATLAS as f32;
        let uv = [
            (x + src[0] * w) / a,
            (y + src[1] * h) / a,
            (x + src[2] * w) / a,
            (y + src[3] * h) / a,
        ];
        self.quad(r, uv, color, [0.0, 0.0, 0.0, -1.0]);
    }
    /// A ring `width` dp wide just inside `r`'s rounded edge.
    pub fn ring(&mut self, r: Rect, radius: f32, width: f32, color: [f32; 4]) {
        let s = self.scale;
        self.quad(r, [0.0; 4], color, [0.0, 0.0, radius * s, width * s]);
    }
    /// A plate shaded from `top` to `bottom`, its corners rounded by `radius` dp.
    pub fn gradient(&mut self, r: Rect, radius: f32, top: [f32; 4], bottom: [f32; 4]) {
        self.shaded(r, radius, [top, top, bottom, bottom]);
    }
    /// A plate shaded from `left` to `right`.
    pub fn gradient_across(&mut self, r: Rect, radius: f32, left: [f32; 4], right: [f32; 4]) {
        self.shaded(r, radius, [left, right, right, left]);
    }
    /// A plate with a colour at each corner: top left, top right, bottom
    /// right, bottom left.
    fn shaded(&mut self, r: Rect, radius: f32, colors: [[f32; 4]; 4]) {
        let first = self.vertices.len();
        self.quad(r, [0.0; 4], colors[0], [0.0, 0.0, radius * self.scale, 0.0]);
        // `quad`'s two triangles, by corner.
        for (v, corner) in self.vertices[first..].iter_mut().zip([0, 1, 2, 0, 2, 3]) {
            v.color = colors[corner];
        }
    }
    /// A one-pixel rule.
    pub fn rule(&mut self, x: f32, y: f32, w: f32, color: [f32; 4]) {
        self.rect(Rect::new(x, y, w, 1.0 / self.scale), 0.0, color);
    }
    fn px(&self, size: f32) -> u32 {
        (size * self.scale).round().max(1.0) as u32
    }
    /// The width of `text` at `size` dp.
    pub fn measure(&mut self, text: &str, face: Face, size: f32) -> f32 {
        let px = self.px(size);
        let scale = self.glyphs.scale(face, px as f32);
        let font = self.glyphs.face(face).as_scaled(scale);
        let mut width = 0.0;
        let mut last = None;
        for c in text.chars() {
            let id = font.glyph_id(c);
            if let Some(prev) = last {
                width += font.kern(prev, id);
            }
            width += font.h_advance(id);
            last = Some(id);
        }
        width / self.scale
    }
    /// Text with its top at `y`; returns its width in dp.
    pub fn text(
        &mut self,
        x: f32,
        y: f32,
        text: &str,
        face: Face,
        size: f32,
        color: [f32; 4],
    ) -> f32 {
        let px = self.px(size);
        let scale = self.glyphs.scale(face, px as f32);
        let (ascent, ids) = {
            let font = self.glyphs.face(face).as_scaled(scale);
            let ids: Vec<_> = text
                .chars()
                .map(|c| {
                    let id = font.glyph_id(c);
                    (id, font.h_advance(id))
                })
                .collect();
            let kerns: Vec<f32> = ids.windows(2).map(|w| font.kern(w[0].0, w[1].0)).collect();
            (
                font.ascent(),
                ids.into_iter()
                    .enumerate()
                    .map(|(i, (id, advance))| (id, advance + kerns.get(i).copied().unwrap_or(0.0)))
                    .collect::<Vec<_>>(),
            )
        };
        let mut pen = (x * self.scale).round();
        let baseline = (y * self.scale + ascent).round();
        let start = pen;
        for (id, advance) in ids {
            let glyph = self.glyphs.glyph(face, id, px);
            if let Some([gx, gy, gw, gh]) = glyph.at {
                let a = ATLAS as f32;
                let s = self.scale;
                let r = Rect::new(
                    (pen + glyph.left).round() / s,
                    (baseline + glyph.top) / s,
                    gw as f32 / s,
                    gh as f32 / s,
                );
                self.quad(
                    r,
                    [
                        gx as f32 / a,
                        gy as f32 / a,
                        (gx + gw) as f32 / a,
                        (gy + gh) as f32 / a,
                    ],
                    color,
                    [0.0, 0.0, 0.0, -1.0],
                );
            }
            pen += advance;
        }
        (pen - start) / self.scale
    }
    /// The line height a text of `size` takes.
    pub fn line(&self, face: Face, size: f32) -> f32 {
        let px = (size * self.scale).round().max(1.0);
        let scale = self.glyphs.scale(face, px);
        let font = self.glyphs.face(face).as_scaled(scale);
        (font.ascent() - font.descent()) / self.scale
    }
    /// `text` cut to fit `width`, with an ellipsis.
    pub fn fit(&mut self, text: &str, face: Face, size: f32, width: f32) -> String {
        if self.measure(text, face, size) <= width {
            return text.to_owned();
        }
        let mut chars: Vec<char> = text.chars().collect();
        while !chars.is_empty() {
            chars.pop();
            let cut: String = chars.iter().collect::<String>().trim_end().to_owned() + "…";
            if self.measure(&cut, face, size) <= width {
                return cut;
            }
        }
        String::new()
    }
    /// Words of `text` broken into lines no wider than `width`.
    pub fn wrap(&mut self, text: &str, face: Face, size: f32, width: f32) -> Vec<String> {
        let mut lines = Vec::new();
        for paragraph in text.split('\n') {
            let mut line = String::new();
            for word in paragraph.split(' ') {
                let candidate = if line.is_empty() {
                    word.to_owned()
                } else {
                    format!("{line} {word}")
                };
                if !line.is_empty() && self.measure(&candidate, face, size) > width {
                    lines.push(std::mem::take(&mut line));
                    line = word.to_owned();
                } else {
                    line = candidate;
                }
            }
            lines.push(line);
        }
        lines
    }
}

/// The window's surface and the launcher's pipeline.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
    bind_group: wgpu::BindGroup,
    screen: wgpu::Buffer,
    atlas: wgpu::Texture,
    glyphs: Glyphs,
    /// Colours are written as they are into an sRGB target's linear values.
    srgb: bool,
    /// The window's frames can be copied out (`capture`).
    copyable: bool,
    /// Where the next frame is saved as a PNG.
    capture: Option<std::path::PathBuf>,
    pub scale: f32,
}

impl Renderer {
    pub fn new(window: Arc<Window>) -> Result<Self> {
        pollster::block_on(Self::open(window))
    }

    async fn open(window: Arc<Window>) -> Result<Self> {
        let mut descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
        descriptor.backends = if cfg!(windows) {
            wgpu::Backends::VULKAN | wgpu::Backends::DX12 | wgpu::Backends::GL
        } else {
            wgpu::Backends::VULKAN | wgpu::Backends::GL
        };
        let instance = wgpu::Instance::new(descriptor);
        let scale = window.scale_factor() as f32;
        let size = window.inner_size();
        let surface = instance.create_surface(window)?;
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                ..Default::default()
            })
            .await?;
        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("Horizon launcher"),
                ..Default::default()
            })
            .await?;
        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .unwrap_or(caps.formats[0]);
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::Fifo,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            color_space: wgpu::SurfaceColorSpace::default(),
        };
        surface.configure(&device, &config);
        let atlas = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("launcher glyphs"),
            size: wgpu::Extent3d {
                width: ATLAS,
                height: ATLAS,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let screen = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("launcher screen"),
            contents: bytemuck::cast_slice(&[0f32; 4]),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("launcher"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let view = atlas.create_view(&Default::default());
        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("launcher"),
            layout: &layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: screen.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("launcher"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("launcher"),
            bind_group_layouts: &[Some(&layout)],
            ..Default::default()
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("launcher"),
            layout: Some(&pipeline_layout),
            vertex: wgpu::VertexState {
                module: &module,
                entry_point: Some("vs"),
                compilation_options: Default::default(),
                buffers: &[Some(wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &wgpu::vertex_attr_array![
                        0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4
                    ],
                })],
            },
            fragment: Some(wgpu::FragmentState {
                module: &module,
                entry_point: Some("fs"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });
        Ok(Self {
            surface,
            device,
            queue,
            config,
            pipeline,
            bind_group,
            screen,
            atlas,
            glyphs: Glyphs::new(),
            srgb: format.is_srgb(),
            copyable: caps.usages.contains(wgpu::TextureUsages::COPY_SRC),
            capture: None,
            scale,
        })
    }

    /// Places an RGBA image in the atlas, for `Canvas::image`; none when the
    /// atlas has no room left.
    pub fn image(&mut self, rgba: &[u8], width: u32, height: u32) -> Option<Image> {
        let [x, y] = self.glyphs.place(width, height)?;
        self.glyphs
            .uploads
            .push(([x, y, width, height], rgba.to_vec()));
        Some(Image {
            at: [x, y, width, height],
        })
    }

    /// Saves the next frame as a PNG at `path` (for pictures of the launcher
    /// in CI); false where the window's frames cannot be copied out.
    pub fn capture(&mut self, path: std::path::PathBuf) -> bool {
        if !self.copyable {
            return false;
        }
        if !self.config.usage.contains(wgpu::TextureUsages::COPY_SRC) {
            self.config.usage |= wgpu::TextureUsages::COPY_SRC;
            self.surface.configure(&self.device, &self.config);
        }
        self.capture = Some(path);
        true
    }

    /// Copies `texture` (the frame) out to `path` as a PNG.
    fn save(
        &self,
        encoder: wgpu::CommandEncoder,
        texture: &wgpu::Texture,
        path: &std::path::Path,
    ) -> Result<()> {
        let mut encoder = encoder;
        let (w, h) = (self.config.width, self.config.height);
        let row = (w * 4).div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT)
            * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("launcher capture"),
            size: u64::from(row * h),
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            wgpu::TexelCopyTextureInfo {
                texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(row),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.device.poll(wgpu::PollType::wait_indefinitely())?;
        let data = buffer.slice(..).get_mapped_range()?;
        let bgra = matches!(
            self.config.format,
            wgpu::TextureFormat::Bgra8Unorm | wgpu::TextureFormat::Bgra8UnormSrgb
        );
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let line = &data[(y * row) as usize..][..(w * 4) as usize];
            for p in line.chunks_exact(4) {
                rgba.extend(if bgra {
                    [p[2], p[1], p[0], 255]
                } else {
                    [p[0], p[1], p[2], 255]
                });
            }
        }
        let file = std::io::BufWriter::new(std::fs::File::create(path)?);
        let mut png = png::Encoder::new(file, w, h);
        png.set_color(png::ColorType::Rgba);
        png.set_depth(png::BitDepth::Eight);
        png.write_header()?.write_image_data(&rgba)?;
        Ok(())
    }

    pub fn resize(&mut self, width: u32, height: u32, scale: f32) {
        self.scale = scale;
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Draws one frame built by `build`, which gets a canvas the window's size in dp.
    pub fn frame(&mut self, background: [f32; 4], build: impl FnOnce(&mut Canvas)) -> Result<()> {
        let mut canvas = Canvas {
            width: self.config.width as f32 / self.scale,
            height: self.config.height as f32 / self.scale,
            scale: self.scale,
            glyphs: &mut self.glyphs,
            vertices: Vec::new(),
        };
        build(&mut canvas);
        let mut vertices = canvas.vertices;
        if self.srgb {
            for v in &mut vertices {
                for c in &mut v.color[..3] {
                    *c = c.powf(2.2);
                }
            }
        }
        for ([x, y, w, h], rgba) in self.glyphs.uploads.drain(..) {
            self.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.atlas,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(w * 4),
                    rows_per_image: Some(h),
                },
                wgpu::Extent3d {
                    width: w,
                    height: h,
                    depth_or_array_layers: 1,
                },
            );
        }
        self.queue.write_buffer(
            &self.screen,
            0,
            bytemuck::cast_slice(&[
                self.config.width as f32,
                self.config.height as f32,
                0.0,
                0.0,
            ]),
        );
        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            _ => return Ok(()),
        };
        let view = frame.texture.create_view(&Default::default());
        let buffer = (!vertices.is_empty()).then(|| {
            self.device
                .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("launcher vertices"),
                    contents: bytemuck::cast_slice(&vertices),
                    usage: wgpu::BufferUsages::VERTEX,
                })
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut bg = background;
            if self.srgb {
                for c in &mut bg[..3] {
                    *c = c.powf(2.2);
                }
            }
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("launcher"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64,
                            g: bg[1] as f64,
                            b: bg[2] as f64,
                            a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            if let Some(buffer) = &buffer {
                pass.set_pipeline(&self.pipeline);
                pass.set_bind_group(0, &self.bind_group, &[]);
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..vertices.len() as u32, 0..1);
            }
        }
        match self.capture.take() {
            Some(path) => {
                if let Err(e) = self.save(encoder, &frame.texture, &path) {
                    log::warn!("Tachyon launcher: {} not saved: {e}", path.display());
                }
            }
            None => {
                self.queue.submit([encoder.finish()]);
            }
        }
        self.queue.present(frame);
        Ok(())
    }
}
