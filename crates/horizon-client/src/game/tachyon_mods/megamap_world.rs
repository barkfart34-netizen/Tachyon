//! World, client-service, and 3D primitive adapter for Tachyon's built-in Megamap
//! (`Teiwazik/teiwaziks-all-in-one-map` v0.3.11, MIT).
//!
//! Provides zero-copy `Send + Sync` chunk/biome snapshots for Megamap's background
//! baking workers, client entity/state/texture queries, the `MegamapHost` adapter
//! (normalizing 24-bit RGB text colors to ARGB for Horizon's `native_text`), and
//! 3D waypoint beam/label/trail tessellation into `WorldGeometry`.

use super::super::{Game, Screen};
use horizon_client::{
    biome::{BiomeColors, Biomes},
    block_entity_render::{BeamShape, beam},
    block_states::BlockStates,
    chunk::Chunk,
    client_resources::{ClientResources, TexturePixels},
    gizmos::{self, Gizmos, Line},
    hud::HudBatch,
    mesher::Vertex,
    net::FRIEND,
    options_file::MAX_RENDER_DISTANCE,
    text::Text,
    world::World,
    world_geometry::WorldGeometry,
    world_primitives::{MAX_BEAM_HEIGHT, PrimitiveShape, WorldPrimitives, text_unit, vertex_color},
};
use horizon_core::geometry::Vec3;
use horizon_world::registry::BlockStateId;
use std::{borrow::Cow, ops::Range, path::PathBuf, sync::Arc};

const MAX_CHUNKS: i64 = (2 * MAX_RENDER_DISTANCE as i64 + 1).pow(2);
const CHUNK_SHIFT: i32 = 4;
const CHUNK_MASK: i32 = 15;
const SECTION_HEIGHT: i32 = 16;
const SKIN_SIDE: u32 = 64;

/// A biome by the client's own id (vanilla biomes first, then data packs').
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct NativeBiomeId(pub u16);

/// Zero-copy `Send + Sync` snapshot of loaded client chunks in a rectangular area.
#[derive(Clone)]
pub struct NativeClientChunks {
    origin: [i32; 2],
    width: i32,
    chunks: Vec<Option<Arc<Chunk>>>,
    air: BlockStateId,
    height: Range<i32>,
}

impl NativeClientChunks {
    fn of(world: &World, from: [i32; 2], to: [i32; 2]) -> Result<Self, String> {
        let origin = [from[0].min(to[0]), from[1].min(to[1])];
        let span = |axis: usize| i64::from(from[axis].max(to[axis])) - i64::from(origin[axis]) + 1;
        let (width, depth) = (span(0), span(1));
        let count = width.saturating_mul(depth);
        if count > MAX_CHUNKS {
            return Err(format!("{count} chunks, more than {MAX_CHUNKS} a snapshot"));
        }
        let chunks = (0..depth)
            .flat_map(|dz| (0..width).map(move |dx| (dx, dz)))
            .map(|(dx, dz)| {
                let key = (origin[0] + dx as i32, origin[1] + dz as i32);
                world.chunks.get(&key).cloned()
            })
            .collect();
        Ok(Self {
            origin,
            width: width as i32,
            chunks,
            air: world.air,
            height: world.shape.min_y..world.shape.max_y(),
        })
    }

    #[inline]
    fn chunk(&self, x: i32, z: i32) -> Option<&Chunk> {
        let width = i64::from(self.width);
        let dx = i64::from(x >> CHUNK_SHIFT) - i64::from(self.origin[0]);
        let dz = i64::from(z >> CHUNK_SHIFT) - i64::from(self.origin[1]);
        if !(0..width).contains(&dx) {
            return None;
        }
        self.chunks
            .get(usize::try_from(dz * width + dx).ok()?)?
            .as_deref()
    }

    #[inline]
    pub fn block(&self, [x, y, z]: [i32; 3]) -> BlockStateId {
        match self.chunk(x, z) {
            Some(chunk) if self.height.contains(&y) => {
                chunk.block(x & CHUNK_MASK, y, z & CHUNK_MASK)
            }
            _ => self.air,
        }
    }

    #[inline]
    pub fn biome(&self, [x, y, z]: [i32; 3]) -> Option<NativeBiomeId> {
        let y = y.clamp(self.height.start, self.height.end - 1);
        self.chunk(x, z)
            .map(|chunk| NativeBiomeId(chunk.biome(x, y, z)))
    }

    #[inline]
    pub fn loaded(&self, [x, z]: [i32; 2]) -> bool {
        self.chunk(x, z).is_some()
    }

    pub fn section_top(&self, [x, z]: [i32; 2]) -> i32 {
        let bottom = self.height.start;
        let Some(chunk) = self.chunk(x, z) else {
            return bottom;
        };
        chunk
            .sections
            .iter()
            .rposition(|section| !section.is_uniform(self.air.0))
            .map_or(bottom, |s| chunk.min_y() + (s as i32 + 1) * SECTION_HEIGHT)
    }

    pub fn height(&self) -> Range<i32> {
        self.height.clone()
    }

    fn changed_in(&self, world: &World) -> Vec<[i32; 2]> {
        let width = self.width as usize;
        self.chunks
            .iter()
            .enumerate()
            .filter_map(|(i, held)| {
                let pos = [
                    self.origin[0] + (i % width) as i32,
                    self.origin[1] + (i / width) as i32,
                ];
                let same = match (held, world.chunks.get(&(pos[0], pos[1]))) {
                    (Some(held), Some(now)) => Arc::ptr_eq(held, now),
                    (None, None) => true,
                    _ => false,
                };
                (!same).then_some(pos)
            })
            .collect()
    }
}

/// Zero-copy `Send + Sync` client biome color table for worker threads.
#[derive(Clone)]
pub struct NativeClientBiomes(Arc<Biomes>);

impl NativeClientBiomes {
    #[allow(dead_code)]
    pub fn name(&self, biome: NativeBiomeId) -> &str {
        &self.0.get(biome.0).name
    }

    pub fn colors(&self, biome: NativeBiomeId, [x, z]: [i32; 2]) -> BiomeColors {
        let info = self.0.get(biome.0);
        BiomeColors {
            grass: self.0.grass(biome.0, f64::from(x), f64::from(z)),
            foliage: info.foliage,
            dry_foliage: info.dry_foliage,
            water: info.water,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum NativeScreenKind {
    #[default]
    Playing,
    Chat,
    Death,
    Other,
}

impl NativeScreenKind {
    fn of(screen: Screen, covered: bool) -> Self {
        match screen {
            _ if covered => Self::Other,
            Screen::Playing => Self::Playing,
            Screen::Chat => Self::Chat,
            Screen::Dead => Self::Death,
            _ => Self::Other,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum NativeWorldId {
    Singleplayer(String),
    Server(String),
}

impl NativeWorldId {
    fn of(save: Option<&str>, address: &str) -> Self {
        match (save, address.strip_prefix(FRIEND)) {
            (Some(folder), _) => Self::Singleplayer(folder.to_owned()),
            (None, Some(code)) => Self::Server(format!("{FRIEND}{}", invite_public_part(code))),
            (None, None) => Self::Server(address.to_owned()),
        }
    }
}

fn invite_public_part(code: &str) -> String {
    horizon_hooks::engine::tunnel().map_or_else(String::new, |r| r.public_part(code))
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct NativeClientState {
    pub gui_hidden: bool,
    pub gui_size: [f32; 2],
    pub world_partial_tick: f64,
    pub screen: NativeScreenKind,
    pub debug_screen: bool,
    pub world: Option<NativeWorldId>,
}

#[derive(Clone, Debug)]
pub struct MegamapPlayer<'a> {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub yaw: f32,
    pub dimension: &'a str,
}

#[derive(Clone, Debug, Default)]
#[allow(dead_code)]
pub struct NativeClientEntity {
    pub id: u32,
    pub player_uuid: Option<u128>,
    pub player_name: Option<String>,
    pub position: [f64; 3],
    pub previous_position: [f64; 3],
    pub alive: bool,
    pub invisible: bool,
    pub kind: String,
    pub yaw: f32,
    pub baby: bool,
    pub spectator: bool,
    pub texture: Option<String>,
}

impl NativeClientEntity {
    #[allow(dead_code)]
    pub fn position_at(&self, partial_tick: f64) -> [f64; 3] {
        let vec = |[x, y, z]: [f64; 3]| Vec3::new(x, y, z);
        let at = vec(self.previous_position).lerp(vec(self.position), partial_tick);
        [at.x, at.y, at.z]
    }
}

/// Thin host trait matching the exact 24 methods `megamap/src/glue.rs` expects from `Game`.
pub(super) trait MegamapHost {
    fn biome(&self, pos: [i32; 3]) -> Option<&str>;
    fn block_map_color(&self, state: BlockStateId) -> Option<(u8, u32)>;
    fn client_biomes(&self) -> Option<NativeClientBiomes>;
    fn client_biomes_changed(&self, biomes: &NativeClientBiomes) -> bool;
    fn client_chunks(&self, from: [i32; 2], to: [i32; 2]) -> Result<NativeClientChunks, String>;
    fn client_chunks_changed(&self, snapshot: &NativeClientChunks) -> Vec<[i32; 2]>;
    fn client_entities(&self) -> Vec<NativeClientEntity>;
    fn client_state(&self) -> Option<NativeClientState>;
    fn data_dir(&self, mod_id: &str) -> PathBuf;
    fn day_time(&self) -> i64;
    fn fill(&self, batch: &mut HudBatch, rect: [f32; 4], argb: u32);
    fn light(&self, pos: [i32; 3]) -> (u8, u8);
    fn mod_image(&mut self, width: u32, height: u32, rgba: &[u8]) -> bool;
    fn mod_key(&self, mod_id: &str, name: &str, default: &str) -> String;
    fn mod_screen(&self) -> Option<&str>;
    fn mod_screen_close(&mut self, mod_id: &str);
    fn mod_screen_open(&mut self, mod_id: &str) -> bool;
    fn open_chat(&mut self, text: &str) -> Result<(), String>;
    fn player(&self) -> Option<MegamapPlayer<'_>>;
    fn quad(&self, batch: &mut HudBatch, points: [[f32; 2]; 4], argb: u32);
    fn send_chat(&mut self, message: &str) -> Result<(), String>;
    fn sprite(&self, batch: &mut HudBatch, name: &str, src: [f32; 4], dst: [f32; 4]) -> bool;
    fn text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], color: u32, shadow: bool);
    fn text_width(&self, text: &str) -> f32;
}

impl MegamapHost for Game<'_> {
    fn biome(&self, pos: [i32; 3]) -> Option<&str> {
        let chunk = self.world.chunks.get(&(pos[0] >> 4, pos[2] >> 4))?;
        let y = pos[1].clamp(
            self.world.shape.min_y,
            self.world.shape.max_y().saturating_sub(1),
        );
        Some(world_biomes!(self).name(chunk.biome(pos[0], y, pos[2])))
    }

    fn block_map_color(&self, state: BlockStateId) -> Option<(u8, u32)> {
        let id = self.block_state_map_color(state)?;
        let rgb = horizon_items::blocks::MAP_COLORS
            .get(id as usize)
            .copied()
            .unwrap_or(0);
        Some((id, rgb))
    }

    fn client_biomes(&self) -> Option<NativeClientBiomes> {
        self.native_window()
            .then(|| NativeClientBiomes(self.worker_context().biomes.clone()))
    }

    fn client_biomes_changed(&self, biomes: &NativeClientBiomes) -> bool {
        !Arc::ptr_eq(&biomes.0, &self.worker_context().biomes)
    }

    fn client_chunks(&self, from: [i32; 2], to: [i32; 2]) -> Result<NativeClientChunks, String> {
        if !self.native_window() {
            return Err("a server has no client world".into());
        }
        NativeClientChunks::of(&self.world, from, to)
    }

    fn client_chunks_changed(&self, snapshot: &NativeClientChunks) -> Vec<[i32; 2]> {
        snapshot.changed_in(&self.world)
    }

    fn client_entities(&self) -> Vec<NativeClientEntity> {
        if !self.native_window() || self.remote.is_none() {
            return Vec::new();
        }
        self.mobs
            .iter()
            .filter(|m| !m.removed)
            .map(|m| NativeClientEntity {
                id: m.id,
                player_uuid: self.remote_player_uuid(m.id),
                player_name: self.remote_player_name(m.id).map(str::to_owned),
                position: [m.position.x, m.position.y, m.position.z],
                previous_position: [m.previous.x, m.previous.y, m.previous.z],
                alive: m.alive(),
                invisible: m.invisible || m.has_effect("minecraft:invisibility"),
                kind: m.kind.id.to_string(),
                yaw: m.yaw,
                baby: m.is_baby(),
                spectator: m.extra.spectator,
                texture: Some(horizon_client::mobs::render::body(m).1)
                    .filter(|t| !t.is_empty())
                    .map(Cow::into_owned),
            })
            .collect()
    }

    fn client_state(&self) -> Option<NativeClientState> {
        if !self.native_window() {
            return None;
        }
        let (w, h) = self.gui_size();
        Some(NativeClientState {
            gui_hidden: self.ui.hide_gui,
            gui_size: [w, h],
            world_partial_tick: self.clock.fraction(),
            screen: NativeScreenKind::of(
                self.screen,
                self.title.is_some()
                    || self.screen_over_playing()
                    || self.remote_screen_up()
                    || self.pack_reload.is_some(),
            ),
            debug_screen: self.debug,
            world: self
                .remote
                .as_ref()
                .filter(|r| r.logged_in)
                .map(|r| {
                    NativeWorldId::of(
                        self.integrated.as_ref().map(|s| s.save_folder.as_str()),
                        &r.session.address,
                    )
                }),
        })
    }

    fn data_dir(&self, mod_id: &str) -> PathBuf {
        self.native_data_dir(mod_id)
    }

    fn day_time(&self) -> i64 {
        self.world.day_time
    }

    fn fill(&self, batch: &mut HudBatch, rect: [f32; 4], argb: u32) {
        self.native_fill(batch, rect, argb);
    }

    fn light(&self, pos: [i32; 3]) -> (u8, u8) {
        let packed = self.world.light(pos[0], pos[1], pos[2]);
        (packed & 0x0F, packed >> 4)
    }

    fn mod_image(&mut self, width: u32, height: u32, rgba: &[u8]) -> bool {
        Game::mod_image(self, width, height, rgba)
    }

    fn mod_key(&self, mod_id: &str, name: &str, default: &str) -> String {
        self.tachyon_mod_key(mod_id, name, default)
    }

    fn mod_screen(&self) -> Option<&str> {
        Game::mod_screen(self)
    }

    fn mod_screen_close(&mut self, mod_id: &str) {
        Game::mod_screen_close(self, mod_id);
    }

    fn mod_screen_open(&mut self, mod_id: &str) -> bool {
        Game::mod_screen_open(self, mod_id)
    }

    fn open_chat(&mut self, text: &str) -> Result<(), String> {
        Game::open_chat(self, text)
    }

    fn player(&self) -> Option<MegamapPlayer<'_>> {
        let p = self.player.as_ref()?;
        Some(MegamapPlayer {
            x: p.position.x,
            y: p.position.y,
            z: p.position.z,
            yaw: self.yaw,
            dimension: self.dim.id(),
        })
    }

    fn quad(&self, batch: &mut HudBatch, points: [[f32; 2]; 4], argb: u32) {
        self.native_quad(batch, points, argb);
    }

    fn send_chat(&mut self, message: &str) -> Result<(), String> {
        Game::send_chat(self, message)
    }

    fn sprite(&self, batch: &mut HudBatch, name: &str, src: [f32; 4], dst: [f32; 4]) -> bool {
        self.native_sprite(batch, name, src, dst, None)
    }

    fn text(&self, batch: &mut HudBatch, text: &str, at: [f32; 2], color: u32, shadow: bool) {
        // Megamap passes 24-bit RGB (0xRRGGBB) colors; Horizon's `native_text` expects ARGB
        // and skips drawing when alpha == 0, so promote opaque 24-bit colors to 0xFF_RRGGBB.
        let argb = if color & 0xFF00_0000 == 0 {
            color | 0xFF00_0000
        } else {
            color
        };
        self.native_text(batch, text, at, argb, shadow);
    }

    fn text_width(&self, text: &str) -> f32 {
        let scratch = HudBatch::scratch([1, 1]);
        self.native_text_width(&scratch, text)
    }
}

impl Game<'_> {
    pub(super) fn megamap_block_sprites_version(&self) -> u64 {
        self.block_sprites_version()
    }

    pub(super) fn megamap_block_states(&self) -> BlockStates {
        let split = self
            .renderer
            .as_ref()
            .map_or(u32::MAX, |r| r.sprite_split());
        BlockStates::new(
            self.res.facts(),
            self.res.collision(),
            self.pack.context.clone(),
            split,
        )
    }

    pub(super) fn megamap_client_resources(&self) -> ClientResources {
        ClientResources::new(self.options.resources.clone())
    }

    pub(super) fn megamap_client_texture(&self, texture: &str) -> Option<TexturePixels> {
        if !self.native_window() {
            return None;
        }
        if let Some(rgba) = self.skin_downloads.borrow().image(texture) {
            return Some(TexturePixels {
                width: SKIN_SIDE,
                height: SKIN_SIDE,
                rgba: rgba.to_vec(),
            });
        }
        self.megamap_client_resources().texture(texture)
    }

    /// Tessellates Megamap's 3D waypoint beams, billboard labels, and breadcrumb trail
    /// directly into the frame's `WorldGeometry` and queues glyph patches into `map_patches`.
    pub(in crate::game) fn submit_megamap_primitives(
        &mut self,
        alpha: f64,
        draw: &mut WorldGeometry,
    ) {
        if !self.native_window() || !self.tachyon_mod_on(super::megamap_glue::ID) {
            return;
        }
        let camera_arr = draw.camera();
        let mut primitives = WorldPrimitives::default();
        super::megamap_glue::primitives(camera_arr, &mut primitives);
        if primitives.is_empty() {
            return;
        }
        let camera = glam::DVec3::from_array(camera_arr);
        let view = draw.view();
        let atlas = &self.gui.atlas;
        let white = horizon_client::gfx::lightning::white_uv(&atlas.font.white());
        let beam_sprite = atlas.sprites.get("entity/beacon/beacon_beam").copied();
        let time = self.world.game_time.rem_euclid(40) as f32 + alpha as f32;
        let relative = |p: [f64; 3]| (glam::DVec3::from_array(p) - camera).as_vec3();

        let mut quads: [Vec<Vertex>; 2] = Default::default();
        let mut lines: [Vec<Line>; 2] = Default::default();
        let mut texts = Vec::new();

        for primitive in primitives.iter() {
            let through = usize::from(primitive.through_walls);
            match &primitive.shape {
                PrimitiveShape::Line {
                    from,
                    to,
                    color,
                    width,
                } if finite(from) && finite(to) && width.is_finite() => {
                    lines[through].push(Line {
                        start: *from,
                        end: *to,
                        color: *color,
                        width: *width,
                    });
                }
                PrimitiveShape::BoxOutline {
                    min,
                    max,
                    color,
                    width,
                } if finite(min) && finite(max) && width.is_finite() => {
                    let mut edges = Gizmos::default();
                    edges.cuboid(*min, *max, *color, *width);
                    lines[through].extend(edges.lines);
                }
                PrimitiveShape::FilledBox { min, max, color }
                    if finite(min) && finite(max) =>
                {
                    let color = vertex_color(*color, 255);
                    for quad in gizmos::cuboid_fill_quads(*min, *max) {
                        quads[through]
                            .extend(quad.map(|corner| plain(relative(corner), white, color)));
                    }
                }
                PrimitiveShape::Beam {
                    base,
                    height,
                    color,
                } if finite(base) => {
                    let Some(sprite) = beam_sprite else {
                        continue;
                    };
                    let corner = relative(*base) - glam::Vec3::new(0.5, 0.0, 0.5);
                    let start = quads[through].len();
                    beam(
                        glam::Mat4::from_translation(corner),
                        BeamShape {
                            scale: 1.0,
                            time,
                            start: 0,
                            height: (*height).min(MAX_BEAM_HEIGHT) as i32,
                            color: color & 0xFF_FFFF,
                            solid_radius: 0.2,
                            glow_radius: 0.25,
                        },
                        sprite,
                        &mut quads[through],
                    );
                    for v in &mut quads[through][start..] {
                        let a = (v.color >> 24) * (color >> 24) / 255;
                        v.color = a << 24 | v.color & 0xFF_FFFF;
                    }
                }
                PrimitiveShape::Text {
                    at,
                    text,
                    color,
                    scale,
                    scale_with_distance,
                    background,
                } if finite(at) => {
                    let o = glam::DVec3::from_array(*at) - camera;
                    let unit = text_unit(*scale, *scale_with_distance, o.length());
                    if unit > 0.0 {
                        texts.push((through, o, text.clone(), *color, unit, *background));
                    }
                }
                _ => {}
            }
        }

        for (through, o, text, color, unit, background) in texts {
            let line = [(
                Text::colored(text, color & 0xFF_FFFF),
                f64::from(unit * 4.5),
            )];
            let patches = self.name_tag_quads(
                o,
                &line,
                color >> 24,
                background,
                0,
                0,
                unit,
                &mut quads[through],
            );
            self.map_patches.extend(patches);
        }

        for through in 0..2 {
            if !quads[through].is_empty() {
                draw.push_raw_quads(through == 1, &quads[through]);
            }
            if !lines[through].is_empty() {
                let raw = gizmos::camera_relative(&lines[through], camera_arr, view);
                draw.push_raw_lines(through == 1, &raw);
            }
        }
    }
}

fn finite(p: &[f64; 3]) -> bool {
    p.iter().all(|v| v.is_finite())
}

fn plain(pos: glam::Vec3, white: [f32; 2], color: u32) -> Vertex {
    Vertex {
        pos: pos.to_array(),
        uv: white,
        layer: 0,
        color,
        light: 0,
        normal: [0; 4],
        glint_uv: [0.0, 0.0],
    }
}
