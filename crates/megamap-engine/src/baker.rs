//! Chunk baker: turns a 16x16 column of blocks into map pixels and heights. It runs on the worker thread over a
//! snapshot (`ChunkSource`), never touching the game, so a slow bake can not stall a frame.

use super::color;

/// The blocks of a client snapshot (`NativeClientChunks` in the glue, a fake world in the tests).
pub trait ChunkSource {
    /// Block state id at a block position; air (0) outside the snapshot.
    fn block(&self, pos: [i32; 3]) -> u32;
    /// The biome id at a block position, `None` outside the snapshot.
    fn biome(&self, pos: [i32; 3]) -> Option<u16>;
    /// One above the highest non-air section of the column (block coordinates).
    fn section_top(&self, xz: [i32; 2]) -> i32;
    /// Whether the chunk holding this block column is in the snapshot.
    fn loaded(&self, xz: [i32; 2]) -> bool;
    fn min_y(&self) -> i32;
}

/// Biome tints (0xRRGGBB): grass, foliage, water.
pub trait Tints {
    fn tint(&self, biome: u16, x: i32, z: i32) -> (u32, u32, u32);
}

/// Map colour ids (`MapColor.id`) the baker treats specially.
pub const MC_NONE: u8 = 0;
pub const MC_GRASS: u8 = 1;
pub const MC_PLANT: u8 = 7;
pub const MC_WATER: u8 = 12;

/// Block state id -> `(map colour id << 24) | rgb`, built on the main thread (where `block_map_color` lives) a
/// slice per frame and shared read-only with the worker.
#[derive(Clone, Default)]
pub struct ColorTable {
    entries: Vec<u32>,
    /// Block light a state gives off (0..=15, `BlockStateFacts::emission`), empty when none is known.
    emit: Vec<u8>,
}

impl ColorTable {
    pub const MISSING: u32 = u32::MAX;

    pub fn with_len(n: usize) -> Self {
        Self { entries: vec![Self::MISSING; n], emit: Vec::new() }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    /// Names the block registry the state ids belong to (its size, nonzero), for saved block ids
    /// (`region::set_block_stamp`): a game or mod change that adds blocks renumbers them.
    pub fn fingerprint(&self) -> u32 {
        (self.entries.len() as u32).max(1)
    }
    pub fn set(&mut self, id: u32, map_color: u8, rgb: u32) {
        let i = id as usize;
        if i >= self.entries.len() {
            self.entries.resize(i + 1, Self::MISSING);
        }
        self.entries[i] = (map_color as u32) << 24 | (rgb & 0xFF_FFFF);
    }
    /// Sets the block light state `id` gives off.
    pub fn set_emit(&mut self, id: u32, level: u8) {
        let i = id as usize;
        if level == 0 && i >= self.emit.len() {
            return;
        }
        if i >= self.emit.len() {
            self.emit.resize(i + 1, 0);
        }
        self.emit[i] = level.min(15);
    }
    #[inline]
    pub fn emit(&self, id: u32) -> u8 {
        self.emit.get(id as usize).copied().unwrap_or(0)
    }
    /// Whether any state gives off light (the baker skips the light pass otherwise).
    pub fn has_emitters(&self) -> bool {
        !self.emit.is_empty()
    }
    /// The emission table of `other`, kept when the colours are replaced (the resource pack palette).
    pub fn copy_emit(&mut self, other: &ColorTable) {
        self.emit = other.emit.clone();
    }
    /// `(map colour id, rgb)`; `None` for air, glass-like (`NONE`) and unknown states.
    #[inline]
    pub fn get(&self, id: u32) -> Option<(u8, u32)> {
        let e = *self.entries.get(id as usize)?;
        if e == Self::MISSING || e >> 24 == MC_NONE as u32 {
            return None;
        }
        Some(((e >> 24) as u8, e & 0xFF_FFFF))
    }
}

/// One baked chunk: 256 ARGB pixels (x fastest) and surface heights.
#[derive(Clone)]
pub struct BakedChunk {
    pub cx: i32,
    pub cz: i32,
    pub color: [u32; 256],
    pub height: [i16; 256],
    /// The shown block's state id per column (0 unknown), saved for block textures where no chunk is loaded.
    pub block: [u32; 256],
}

/// What a column shows: its top height, the albedo and how deep water over it is.
#[derive(Clone, Copy)]
struct Column {
    y: i32,
    rgb: u32,
    water: u8,
}

#[derive(Clone, Copy)]
pub enum Layer {
    /// Highest visible block.
    Surface,
    /// Cave slice `(top, floor)`: start at `top` (blocks above are the cut ceiling), skip the rock down to air and
    /// show the first floor, looking no deeper than `floor` (`layers::cave_window`).
    Cave(i32, i32),
    /// Indoors: the highest visible block at or under the cut `top` (the ceiling and all above it cut off), so a
    /// room shows its floor, walls and furniture.
    Indoor(i32),
}

/// Offsets of the biome blend: the column plus four samples two blocks away, so tints fade over a 5 block band
/// instead of switching at the biome border.
const BLEND: [(i32, i32); 5] = [(0, 0), (-2, 0), (2, 0), (0, -2), (0, 2)];

/// Tint (`which`: 0 grass, 1 foliage, 2 water) averaged over `BLEND`.
fn blended(src: &dyn ChunkSource, tints: &dyn Tints, x: i32, y: i32, z: i32, which: u8) -> u32 {
    let own = src.biome([x, y, z]).unwrap_or(0);
    let (mut r, mut g, mut b) = (0u32, 0u32, 0u32);
    for (dx, dz) in BLEND {
        let biome = if dx == 0 && dz == 0 { own } else { src.biome([x + dx, y, z + dz]).unwrap_or(own) };
        let t = tints.tint(biome, x + dx, z + dz);
        let c = match which {
            0 => t.0,
            1 => t.1,
            _ => t.2,
        };
        r += (c >> 16) & 0xFF;
        g += (c >> 8) & 0xFF;
        b += c & 0xFF;
    }
    (r / 5) << 16 | (g / 5) << 8 | (b / 5)
}

fn column(src: &dyn ChunkSource, tints: &dyn Tints, table: &ColorTable, x: i32, z: i32, layer: Layer) -> Option<Column> {
    let top = src.section_top([x, z]);
    let min = match layer {
        Layer::Cave(_, floor) => src.min_y().max(floor),
        Layer::Surface | Layer::Indoor(_) => src.min_y(),
    };
    let mut y = match layer {
        Layer::Surface => top - 1,
        Layer::Indoor(c) => c.min(top - 1),
        Layer::Cave(c, _) => {
            // Skip the rock above until air, so the slice shows the floor of the cave the player is in.
            let mut y = c.min(top - 1);
            while y >= min && table.get(src.block([x, y, z])).is_some() {
                y -= 1;
            }
            y
        }
    };
    while y >= min {
        let state = src.block([x, y, z]);
        if let Some((mc, rgb)) = table.get(state) {
            if mc == MC_WATER {
                let water = color::multiply(color::OPAQUE | rgb, blended(src, tints, x, y, z, 2));
                // Walk down to the bottom: its colour shows through the shallows.
                let mut depth = 1u8;
                let mut d = y - 1;
                let mut bottom = None;
                while d >= min && depth < 15 {
                    match table.get(src.block([x, d, z])) {
                        Some((MC_WATER, _)) => depth += 1,
                        Some((bmc, brgb)) => {
                            bottom = Some(tint(src, tints, x, d, z, bmc, brgb));
                            break;
                        }
                        None => {}
                    }
                    d -= 1;
                }
                let rgb = match bottom {
                    // Clear in the first blocks, the water colour taking over by about 5 deep.
                    Some(b) => color::mix(color::OPAQUE | b, water, (120 + depth as u32 * 28).min(256)),
                    None => water,
                };
                return Some(Column { y, rgb: rgb & 0xFF_FFFF, water: depth });
            }
            return Some(Column { y, rgb: tint(src, tints, x, y, z, mc, rgb), water: 0 });
        }
        y -= 1;
    }
    None
}

#[inline]
fn tint(src: &dyn ChunkSource, tints: &dyn Tints, x: i32, y: i32, z: i32, mc: u8, rgb: u32) -> u32 {
    match mc {
        MC_GRASS => color::multiply(color::OPAQUE | rgb, blended(src, tints, x, y, z, 0)) & 0xFF_FFFF,
        MC_PLANT => color::multiply(color::OPAQUE | rgb, blended(src, tints, x, y, z, 1)) & 0xFF_FFFF,
        _ => rgb,
    }
}

/// Bakes chunk (`cx`, `cz`); `None` when it is not in the snapshot. The result is albedo plus heights: relief
/// (hillshade, shadows, ambient occlusion) and the water depth tone are applied by the view at compose time, so
/// they stay right across chunk borders and at every zoom. Land gets a small per-block brightness jitter; water
/// keeps its depth in the alpha byte (see `color::water`).
pub fn bake_chunk(
    src: &dyn ChunkSource,
    tints: &dyn Tints,
    table: &ColorTable,
    cx: i32,
    cz: i32,
    layer: Layer,
) -> Option<BakedChunk> {
    let (bx, bz) = (cx * 16, cz * 16);
    if !src.loaded([bx, bz]) {
        return None;
    }
    let mut out = BakedChunk { cx, cz, color: [color::UNKNOWN; 256], height: [i16::MIN; 256], block: [0; 256] };
    for lz in 0..16 {
        for lx in 0..16 {
            let i = lz * 16 + lx;
            let (x, z) = (bx + lx as i32, bz + lz as i32);
            let Some(c) = column(src, tints, table, x, z, layer) else {
                continue;
            };
            out.color[i] = if c.water > 0 {
                color::water(c.rgb, c.water as u32)
            } else {
                color::shade(color::OPAQUE | c.rgb, color::jitter(x, z, 10))
            };
            out.height[i] = c.y.clamp(i16::MIN as i32 + 1, i16::MAX as i32) as i16;
            // What the live texture lookup reads at the shown height (`Lookup::detail`); water has none drawn.
            out.block[i] = if c.water > 0 { 0 } else { src.block([x, out.height[i] as i32, z]) };
        }
    }
    if let Some(light) = block_light(src, table, bx, bz, &out.height) {
        for (c, l) in out.color.iter_mut().zip(light) {
            if l > 0 && *c >> 24 != 0 && color::water_depth(*c) == 0 {
                *c = color::land(*c, l as u32);
            }
        }
    }
    Some(out)
}

/// How far block light reaches: it falls one level a block (taxicab), so 14 blocks from the brightest emitter.
const LIGHT_REACH: i32 = 14;

/// Block light (0..=15) over the shown surface of chunk (`bx`, `bz`) (block coordinates of its corner), from
/// the emitters (torches, lanterns, lava, glowstone...) within `LIGHT_REACH` around it, near the height the map
/// shows: one level less a block of taxicab distance, measured to the air over the shown block. Walls do not
/// stop it (the map has no light propagation), which reads as the glow around a lit base. `None` when no
/// emitter is near (or the table knows none).
fn block_light(src: &dyn ChunkSource, table: &ColorTable, bx: i32, bz: i32, ys: &[i16; 256]) -> Option<[u8; 256]> {
    if !table.has_emitters() {
        return None;
    }
    let mut emitters: Vec<(i32, i32, i32, i32)> = Vec::new();
    for z in bz - LIGHT_REACH..bz + 16 + LIGHT_REACH {
        for x in bx - LIGHT_REACH..bx + 16 + LIGHT_REACH {
            let (lx, lz) = ((x - bx).clamp(0, 15), (z - bz).clamp(0, 15));
            // Distance from the chunk: an emitter this far can not light any of its columns.
            let out = (x - bx - lx).abs() + (z - bz - lz).abs();
            if out >= LIGHT_REACH || !src.loaded([x, z]) {
                continue;
            }
            // The height the nearest column of the chunk shows is where the light matters.
            let ry = ys[lz as usize * 16 + lx as usize];
            if ry == i16::MIN {
                continue;
            }
            let ry = ry as i32;
            let top = src.section_top([x, z]) - 1;
            for y in ry - 3..=(ry + 6).min(top) {
                let e = table.emit(src.block([x, y, z])) as i32;
                if e > out {
                    emitters.push((x, y, z, e));
                }
            }
        }
    }
    if emitters.is_empty() {
        return None;
    }
    let mut light = [0u8; 256];
    for (ex, ey, ez, e) in emitters {
        for lz in 0..16 {
            let dz = (bz + lz - ez).abs();
            if dz >= e {
                continue;
            }
            for lx in 0..16 {
                let i = (lz * 16 + lx) as usize;
                if ys[i] == i16::MIN {
                    continue;
                }
                let d = dz + (bx + lx - ex).abs() + (ey - (ys[i] as i32 + 1)).abs();
                let l = e - d;
                if l > light[i] as i32 {
                    light[i] = l as u8;
                }
            }
        }
    }
    Some(light)
}
