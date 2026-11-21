//! Entity portraits for the radar, the map and the entities window: flat face icons (8 x 8, cut from the skin or
//! from the mob's head UV) and small 3D renders (a CPU rasteriser over the game's own posed mob geometry).
//!
//! Everything here is pure (pixels in, pixels out) so it is tested without the game; the glue feeds it the
//! decoded textures and the vertices `mobs::render::render_posed` produces, and caches the results.
//!
//! Colours in the returned face arrays are ARGB (`0xAARRGGBB`), ready for `Host::fill`; 3D renders are RGBA8.

/// A decoded texture (RGBA8, row-major).
#[derive(Clone, Debug, Default)]
pub struct Tex {
    pub w: usize,
    pub h: usize,
    pub rgba: Vec<u8>,
}

impl Tex {
    fn texel(&self, x: usize, y: usize) -> [u8; 4] {
        let i = (y.min(self.h - 1) * self.w + x.min(self.w - 1)) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2], self.rgba[i + 3]]
    }
    fn ok(&self) -> bool {
        self.w > 0 && self.h > 0 && self.rgba.len() >= self.w * self.h * 4
    }
}

fn argb(p: [u8; 4]) -> u32 {
    (p[3] as u32) << 24 | (p[0] as u32) << 16 | (p[1] as u32) << 8 | p[2] as u32
}

/// An 8 x 8 face, ARGB, row-major.
pub type Face = [u32; 64];

/// Cuts a texture rect (normalised UV `[u0, v0, u1, v1]`; u0 > u1 means mirrored) to an 8 x 8 face, nearest
/// sampling. None if the rect is degenerate or the result is fully transparent.
pub fn crop_face(t: &Tex, uv: [f32; 4]) -> Option<Face> {
    if !t.ok() {
        return None;
    }
    let [u0, v0, u1, v1] = uv;
    if (u1 - u0).abs() * t.w as f32 <= 0.5 || (v1 - v0).abs() * t.h as f32 <= 0.5 {
        return None;
    }
    let mut f = [0u32; 64];
    let mut any = false;
    for y in 0..8 {
        for x in 0..8 {
            let u = u0 + (u1 - u0) * ((x as f32 + 0.5) / 8.0);
            let v = v0 + (v1 - v0) * ((y as f32 + 0.5) / 8.0);
            let p = t.texel((u * t.w as f32).max(0.0) as usize, (v * t.h as f32).max(0.0) as usize);
            any |= p[3] > 0;
            f[y * 8 + x] = argb(p);
        }
    }
    any.then_some(f)
}

/// Lays `over` on `base` where `over` is not transparent (the hat layer).
pub fn overlay_face(base: &mut Face, over: &Face) {
    for (b, o) in base.iter_mut().zip(over) {
        if o >> 24 >= 0x80 {
            *b = *o | 0xFF00_0000;
        }
    }
}

/// A player face from a skin: the face at (8, 8) and the hat layer at (40, 8), both 8 x 8 in 64-wide units (an HD
/// skin scales; a legacy 64 x 32 skin has both in the same place). The face is made opaque.
pub fn player_face(skin: &Tex) -> Option<Face> {
    if !skin.ok() || skin.w < 64 || skin.h < 32 {
        return None;
    }
    // Skin height is 64 or 32 units; v in units of the 64-wide grid.
    let units_h = if skin.h * 2 == skin.w { 32.0 } else { 64.0 };
    let rect = |x: f32, y: f32| [x / 64.0, y / units_h, (x + 8.0) / 64.0, (y + 8.0) / units_h];
    let mut f = crop_face(skin, rect(8.0, 8.0))?;
    for p in f.iter_mut() {
        *p |= 0xFF00_0000;
    }
    if let Some(hat) = crop_face(skin, rect(40.0, 8.0)) {
        overlay_face(&mut f, &hat);
    }
    Some(f)
}

/// Horizontal runs of equal opaque colour in an image (`get(x, y)` ARGB, alpha >= 0x80 counts), for drawing with
/// a handful of fills instead of one per pixel: `(x, y, len, argb)`.
pub fn runs(w: usize, h: usize, get: impl Fn(usize, usize) -> u32) -> Vec<(u16, u16, u16, u32)> {
    let mut out = Vec::new();
    for y in 0..h {
        let mut x = 0;
        while x < w {
            let c = get(x, y);
            if c >> 24 < 0x80 {
                x += 1;
                continue;
            }
            let c = c | 0xFF00_0000;
            let mut e = x + 1;
            while e < w && (get(e, y) | 0xFF00_0000) == c && get(e, y) >> 24 >= 0x80 {
                e += 1;
            }
            out.push((x as u16, y as u16, (e - x) as u16, c));
            x = e;
        }
    }
    out
}

/// Runs of an RGBA8 image.
pub fn image_runs(img: &[u8], n: usize) -> Vec<(u16, u16, u16, u32)> {
    runs(n, n, |x, y| {
        let i = (y * n + x) * 4;
        argb([img[i], img[i + 1], img[i + 2], img[i + 3]])
    })
}

/// A model vertex for the rasteriser: position in blocks, UV in "virtual atlas" space (texture `i` spans
/// u in `[i, i + 1]`, v in `[0, 1]`), RGBA8 tint (red in the low byte), normal (w = 0: unlit / baked).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RVert {
    pub pos: [f32; 3],
    pub uv: [f32; 2],
    pub color: u32,
    pub normal: [i8; 4],
}

/// The UV rect texture `i` of `n` gets in the virtual atlas.
pub fn virtual_rect(i: usize) -> [f32; 4] {
    [i as f32, 0.0, i as f32 + 1.0, 1.0]
}

fn rot(p: [f32; 3], yaw: f32, pitch: f32) -> [f32; 3] {
    let (sy, cy) = yaw.sin_cos();
    let (sp, cp) = pitch.sin_cos();
    let x = p[0] * cy + p[2] * sy;
    let z = -p[0] * sy + p[2] * cy;
    let y = p[1] * cp - z * sp;
    let z2 = p[1] * sp + z * cp;
    [x, y, z2]
}

/// Rasterises quads (`verts.len()` a multiple of 4) into an `n` x `n` RGBA8 image, orthographic, the model
/// turned by `yaw_deg` about Y and tilted by `pitch_deg` (positive looks down on it), fitted to the image with a
/// small margin, 2x2 supersampled. The camera sits on +Z looking at -Z. Cut-out alpha (texels under half alpha
/// are holes). Returns the image and the number of pixels covered.
pub fn raster(verts: &[RVert], texs: &[Tex], n: usize, yaw_deg: f32, pitch_deg: f32) -> (Vec<u8>, usize) {
    raster_fit(verts, texs, n, yaw_deg, pitch_deg, None)
}

/// `raster` with a fixed frame (`zoom`: `Some`): the model's bounding sphere fills the image at zoom 1 whatever
/// the angle, so a turning model keeps its size and place instead of being refitted to its outline every frame;
/// a larger zoom magnifies about the sphere's centre.
pub fn raster_fit(verts: &[RVert], texs: &[Tex], n: usize, yaw_deg: f32, pitch_deg: f32, zoom: Option<f32>) -> (Vec<u8>, usize) {
    const SS: usize = 2;
    let m = n * SS;
    let mut img = vec![0u8; n * n * 4];
    if verts.len() < 4 || n == 0 {
        return (img, 0);
    }
    let (yaw, pitch) = (yaw_deg.to_radians(), pitch_deg.to_radians());
    let tv: Vec<[f32; 3]> = verts.iter().map(|v| rot(v.pos, yaw, pitch)).collect();
    let (mut x0, mut x1, mut y0, mut y1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
    for p in &tv {
        x0 = x0.min(p[0]);
        x1 = x1.max(p[0]);
        y0 = y0.min(p[1]);
        y1 = y1.max(p[1]);
    }
    let mut span = (x1 - x0).max(y1 - y0).max(1e-4);
    let (mut cx, mut cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    if let Some(z) = zoom {
        // The box of the unturned model: its centre, and the farthest vertex from it.
        let (mut lo, mut hi) = ([f32::MAX; 3], [f32::MIN; 3]);
        for v in verts {
            for k in 0..3 {
                lo[k] = lo[k].min(v.pos[k]);
                hi[k] = hi[k].max(v.pos[k]);
            }
        }
        let c = [(lo[0] + hi[0]) / 2.0, (lo[1] + hi[1]) / 2.0, (lo[2] + hi[2]) / 2.0];
        let r = verts.iter().map(|v| (0..3).map(|k| (v.pos[k] - c[k]).powi(2)).sum::<f32>()).fold(0.0f32, f32::max).sqrt();
        let rc = rot(c, yaw, pitch);
        (cx, cy) = (rc[0], rc[1]);
        span = (2.0 * r).max(1e-4) / z.clamp(0.25, 8.0);
    }
    let scale = m as f32 * 0.88 / span;
    let scr = |p: [f32; 3]| [(p[0] - cx) * scale + m as f32 / 2.0, -(p[1] - cy) * scale + m as f32 / 2.0, p[2]];
    let mut color = vec![[0f32; 4]; m * m];
    let mut depth = vec![f32::MIN; m * m];
    let light = {
        let l = [-0.35f32, 0.75, 0.55];
        let k = (l[0] * l[0] + l[1] * l[1] + l[2] * l[2]).sqrt();
        [l[0] / k, l[1] / k, l[2] / k]
    };
    for (qi, q) in verts.chunks_exact(4).enumerate() {
        let cu = q.iter().map(|v| v.uv[0]).sum::<f32>() / 4.0;
        let ti = cu.floor().max(0.0) as usize;
        let Some(tex) = texs.get(ti).filter(|t| t.ok()) else { continue };
        let nm = q[0].normal;
        let shade = if nm[3] == 0 {
            1.0
        } else {
            let nn = rot([nm[0] as f32 / 127.0, nm[1] as f32 / 127.0, nm[2] as f32 / 127.0], yaw, pitch);
            let d = nn[0] * light[0] + nn[1] * light[1] + nn[2] * light[2];
            // Two-sided: a face seen from behind is lit by its flipped normal.
            let d = if nn[2] < 0.0 { -d } else { d };
            0.55 + 0.45 * d.max(0.0)
        };
        let tint = q[0].color;
        let tint = [
            (tint & 0xFF) as f32 / 255.0,
            (tint >> 8 & 0xFF) as f32 / 255.0,
            (tint >> 16 & 0xFF) as f32 / 255.0,
        ];
        let p: Vec<[f32; 3]> = (0..4).map(|k| scr(tv[qi * 4 + k])).collect();
        for tri in [[0usize, 1, 2], [0, 2, 3]] {
            let [a, b, c] = [p[tri[0]], p[tri[1]], p[tri[2]]];
            let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
            if area.abs() < 1e-6 {
                continue;
            }
            let (ua, ub, uc) = (q[tri[0]].uv, q[tri[1]].uv, q[tri[2]].uv);
            let bx0 = a[0].min(b[0]).min(c[0]).floor().max(0.0) as usize;
            let bx1 = (a[0].max(b[0]).max(c[0]).ceil() as usize).min(m);
            let by0 = a[1].min(b[1]).min(c[1]).floor().max(0.0) as usize;
            let by1 = (a[1].max(b[1]).max(c[1]).ceil() as usize).min(m);
            for y in by0..by1 {
                for x in bx0..bx1 {
                    let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                    let w0 = ((b[0] - px) * (c[1] - py) - (b[1] - py) * (c[0] - px)) / area;
                    let w1 = ((c[0] - px) * (a[1] - py) - (c[1] - py) * (a[0] - px)) / area;
                    let w2 = 1.0 - w0 - w1;
                    if w0 < -1e-4 || w1 < -1e-4 || w2 < -1e-4 {
                        continue;
                    }
                    let z = a[2] * w0 + b[2] * w1 + c[2] * w2;
                    let i = y * m + x;
                    if z <= depth[i] {
                        continue;
                    }
                    let u = (ua[0] * w0 + ub[0] * w1 + uc[0] * w2 - ti as f32).clamp(0.0, 0.99999);
                    let v = (ua[1] * w0 + ub[1] * w1 + uc[1] * w2).clamp(0.0, 0.99999);
                    let t = tex.texel((u * tex.w as f32) as usize, (v * tex.h as f32) as usize);
                    if t[3] < 128 {
                        continue;
                    }
                    depth[i] = z;
                    color[i] = [
                        t[0] as f32 / 255.0 * tint[0] * shade,
                        t[1] as f32 / 255.0 * tint[1] * shade,
                        t[2] as f32 / 255.0 * tint[2] * shade,
                        1.0,
                    ];
                }
            }
        }
    }
    let mut covered = 0;
    for y in 0..n {
        for x in 0..n {
            let mut s = [0f32; 4];
            for dy in 0..SS {
                for dx in 0..SS {
                    let c = color[(y * SS + dy) * m + x * SS + dx];
                    s[0] += c[0];
                    s[1] += c[1];
                    s[2] += c[2];
                    s[3] += c[3];
                }
            }
            let k = (SS * SS) as f32;
            let i = (y * n + x) * 4;
            if s[3] > 0.0 {
                img[i] = (s[0] / s[3] * 255.0).min(255.0) as u8;
                img[i + 1] = (s[1] / s[3] * 255.0).min(255.0) as u8;
                img[i + 2] = (s[2] / s[3] * 255.0).min(255.0) as u8;
                img[i + 3] = (s[3] / k * 255.0) as u8;
                covered += (img[i + 3] >= 128) as usize;
            }
        }
    }
    (img, covered)
}

/// The six faces of a Minecraft model box (`ModelPart.Cube`): origin and size in model pixels (y down, as the
/// model files), texture origin `(u, v)` on a `tw` x `th` texture; pushes quads into texture slot `slot`.
#[allow(clippy::too_many_arguments)]
pub fn push_box(out: &mut Vec<RVert>, o: [f32; 3], s: [f32; 3], u: f32, v: f32, tw: f32, th: f32, slot: usize, grow: f32) {
    let (x0, y0, z0) = (o[0] - grow, o[1] - grow, o[2] - grow);
    let (x1, y1, z1) = (o[0] + s[0] + grow, o[1] + s[1] + grow, o[2] + s[2] + grow);
    let (w, h, d) = (s[0], s[1], s[2]);
    // Model space (y down, front = -Z) to blocks as the entity renderer leaves it for yaw 0: (x, -y, -z), so the
    // front faces the camera on +Z and the model's right (-x) is on screen left.
    let p = |x: f32, y: f32, z: f32| [x / 16.0, -y / 16.0, -z / 16.0];
    let uvr = |a: f32, b: f32, c: f32, e: f32| [a / tw, b / th, c / tw, e / th];
    let sl = slot as f32;
    // Corners in order top-left, top-right, bottom-right, bottom-left of the texture rect.
    let mut face = |c: [[f32; 3]; 4], r: [f32; 4], n: [i8; 3]| {
        let uv = [[r[0], r[1]], [r[2], r[1]], [r[2], r[3]], [r[0], r[3]]];
        for k in 0..4 {
            out.push(RVert {
                pos: c[k],
                uv: [sl + uv[k][0].clamp(0.0, 1.0), uv[k][1]],
                color: 0xFFFF_FFFF,
                normal: [n[0], n[1], n[2], -127],
            });
        }
    };
    // Front (model -Z): texture (u+d, v+d) w x h, its left edge on screen left (model x0).
    face([p(x0, y0, z0), p(x1, y0, z0), p(x1, y1, z0), p(x0, y1, z0)], uvr(u + d, v + d, u + d + w, v + d + h), [0, 0, 127]);
    // Back: seen from behind, its left is model x1.
    face([p(x1, y0, z1), p(x0, y0, z1), p(x0, y1, z1), p(x1, y1, z1)], uvr(u + 2.0 * d + w, v + d, u + 2.0 * d + 2.0 * w, v + d + h), [0, 0, -127]);
    // The model's right side (-x), then its left (+x); each with the front edge toward the front rect.
    face([p(x0, y0, z1), p(x0, y0, z0), p(x0, y1, z0), p(x0, y1, z1)], uvr(u, v + d, u + d, v + d + h), [-127, 0, 0]);
    face([p(x1, y0, z0), p(x1, y0, z1), p(x1, y1, z1), p(x1, y1, z0)], uvr(u + d + w, v + d, u + 2.0 * d + w, v + d + h), [127, 0, 0]);
    // Top (model -y): the rect's bottom edge is the front edge. Bottom likewise.
    face([p(x0, y0, z1), p(x1, y0, z1), p(x1, y0, z0), p(x0, y0, z0)], uvr(u + d, v, u + d + w, v + d), [0, 127, 0]);
    face([p(x0, y1, z1), p(x1, y1, z1), p(x1, y1, z0), p(x0, y1, z0)], uvr(u + d + w, v, u + d + 2.0 * w, v + d), [0, -127, 0]);
}

/// The player model as boxes on a 64 x 64 skin (texture slot 0), standing, arms down: head, body, arms, legs
/// and their outer layers (hat, jacket, sleeves, trousers). `slim` = 3-pixel arms. A legacy 64 x 32 skin
/// (`legacy`) has no left-limb or outer-body layers; its left limbs mirror the right ones' texture.
pub fn player_model(slim: bool, legacy: bool) -> Vec<RVert> {
    let mut v = Vec::with_capacity(6 * 4 * 12);
    let th = if legacy { 32.0 } else { 64.0 };
    let aw = if slim { 3.0 } else { 4.0 };
    let b = |v: &mut Vec<RVert>, o: [f32; 3], s: [f32; 3], u: f32, vv: f32, g: f32| push_box(v, o, s, u, vv, 64.0, th, 0, g);
    // Head and hat.
    b(&mut v, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 0.0, 0.0, 0.0);
    b(&mut v, [-4.0, -8.0, -4.0], [8.0, 8.0, 8.0], 32.0, 0.0, 0.5);
    // Body.
    b(&mut v, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 16.0, 16.0, 0.0);
    // Right arm (model -x, screen left), left arm.
    b(&mut v, [-4.0 - aw, 0.0, -2.0], [aw, 12.0, 4.0], 40.0, 16.0, 0.0);
    b(&mut v, [4.0, 0.0, -2.0], [aw, 12.0, 4.0], if legacy { 40.0 } else { 32.0 }, if legacy { 16.0 } else { 48.0 }, 0.0);
    // Legs.
    b(&mut v, [-4.0, 12.0, -2.0], [4.0, 12.0, 4.0], 0.0, 16.0, 0.0);
    b(&mut v, [0.0, 12.0, -2.0], [4.0, 12.0, 4.0], if legacy { 0.0 } else { 16.0 }, if legacy { 16.0 } else { 48.0 }, 0.0);
    if !legacy {
        b(&mut v, [-4.0, 0.0, -2.0], [8.0, 12.0, 4.0], 16.0, 32.0, 0.25);
        b(&mut v, [-4.0 - aw, 0.0, -2.0], [aw, 12.0, 4.0], 40.0, 32.0, 0.25);
        b(&mut v, [4.0, 0.0, -2.0], [aw, 12.0, 4.0], 48.0, 48.0, 0.25);
        b(&mut v, [-4.0, 12.0, -2.0], [4.0, 12.0, 4.0], 0.0, 32.0, 0.25);
        b(&mut v, [0.0, 12.0, -2.0], [4.0, 12.0, 4.0], 0.0, 48.0, 0.25);
    }
    v
}

/// A block-model element (`from` / `to` in block pixels, 0..16, y up) centred on the block's bottom middle, each
/// face textured from texture slot `slots[face]` the way a block model's default UVs are (from the face's position
/// in the block), tinted `color` (0xAABBGGRR). Faces: north (-z), south, west (-x), east, up, down.
pub fn block_box(out: &mut Vec<RVert>, from: [f32; 3], to: [f32; 3], slots: [usize; 6], color: u32) {
    let [x0, y0, z0] = from;
    let [x1, y1, z1] = to;
    let p = |x: f32, y: f32, z: f32| [(x - 8.0) / 16.0, y / 16.0, (z - 8.0) / 16.0];
    let mut face = |slot: usize, c: [[f32; 3]; 4], uv: [[f32; 2]; 4], n: [i8; 3]| {
        for k in 0..4 {
            out.push(RVert { pos: c[k], uv: [slot as f32 + uv[k][0] / 16.0, uv[k][1] / 16.0], color, normal: [n[0], n[1], n[2], -127] });
        }
    };
    // Corners listed top-left, top-right, bottom-right, bottom-left as seen from outside the face.
    face(slots[0], [p(x1, y1, z0), p(x0, y1, z0), p(x0, y0, z0), p(x1, y0, z0)], [[16.0 - x1, 16.0 - y1], [16.0 - x0, 16.0 - y1], [16.0 - x0, 16.0 - y0], [16.0 - x1, 16.0 - y0]], [0, 0, -127]);
    face(slots[1], [p(x0, y1, z1), p(x1, y1, z1), p(x1, y0, z1), p(x0, y0, z1)], [[x0, 16.0 - y1], [x1, 16.0 - y1], [x1, 16.0 - y0], [x0, 16.0 - y0]], [0, 0, 127]);
    face(slots[2], [p(x0, y1, z0), p(x0, y1, z1), p(x0, y0, z1), p(x0, y0, z0)], [[z0, 16.0 - y1], [z1, 16.0 - y1], [z1, 16.0 - y0], [z0, 16.0 - y0]], [-127, 0, 0]);
    face(slots[3], [p(x1, y1, z1), p(x1, y1, z0), p(x1, y0, z0), p(x1, y0, z1)], [[16.0 - z1, 16.0 - y1], [16.0 - z0, 16.0 - y1], [16.0 - z0, 16.0 - y0], [16.0 - z1, 16.0 - y0]], [127, 0, 0]);
    face(slots[4], [p(x0, y1, z0), p(x1, y1, z0), p(x1, y1, z1), p(x0, y1, z1)], [[x0, z0], [x1, z0], [x1, z1], [x0, z1]], [0, 127, 0]);
    face(slots[5], [p(x0, y0, z1), p(x1, y0, z1), p(x1, y0, z0), p(x0, y0, z0)], [[x0, 16.0 - z1], [x1, 16.0 - z1], [x1, 16.0 - z0], [x0, 16.0 - z0]], [0, -127, 0]);
}

/// A flat item-like card: one quad facing the camera with the whole of texture slot 0 (items and other
/// entities without a model are shown flat).
pub fn flat_card() -> Vec<RVert> {
    let n = [0, 0, 127, 0];
    let c = 0xFFFF_FFFF;
    vec![
        RVert { pos: [-0.5, 0.5, 0.0], uv: [0.0, 0.0], color: c, normal: n },
        RVert { pos: [0.5, 0.5, 0.0], uv: [1.0, 0.0], color: c, normal: n },
        RVert { pos: [0.5, -0.5, 0.0], uv: [1.0, 1.0], color: c, normal: n },
        RVert { pos: [-0.5, -0.5, 0.0], uv: [0.0, 1.0], color: c, normal: n },
    ]
}

/// A per-frame work limiter for caches: `take()` says whether one more expensive job may run this frame.
#[derive(Clone, Copy, Debug)]
pub struct Budget {
    left: u32,
}

impl Budget {
    pub fn new(per_frame: u32) -> Self {
        Budget { left: per_frame }
    }
    pub fn take(&mut self) -> bool {
        if self.left == 0 {
            return false;
        }
        self.left -= 1;
        true
    }
}
