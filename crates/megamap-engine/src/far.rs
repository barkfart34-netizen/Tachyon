//! Far terrain beyond the loaded chunks, from whatever coarse source the game can offer: Horizon's LOD source
//! (in memory, read through `LodSource::sampler`) or its `horizon_far_terrain` disk cache (`hlod`). Nothing here
//! loads chunks: a `FarSource` only reads data the game already has, and the grid fills a budget at a time on
//! the far thread.

use super::color;

/// Coarse ground: height and an ARGB colour, `None` where the source knows nothing.
pub trait FarSource {
    fn surface(&mut self, x: i32, z: i32) -> Option<(i32, u32)>;
}

/// No source (the default build): the map falls back to fog past the explored area.
pub struct NoFar;
impl FarSource for NoFar {
    fn surface(&mut self, _x: i32, _z: i32) -> Option<(i32, u32)> {
        None
    }
}

/// A square grid of `cells` x `cells` samples, `step` blocks apart, centred near the player. Cells fill in row
/// order, so the west and north neighbours of a cell are known when it is shaded: relief costs one pass.
#[derive(Clone)]
pub struct FarGrid {
    pub step: i32,
    pub cells: usize,
    origin: [i32; 2],
    data: Vec<u32>,
    heights: Vec<i32>,
    cursor: usize,
    pub generation: u64,
}

impl FarGrid {
    pub fn new(step: i32, cells: usize) -> Self {
        FarGrid {
            step,
            cells,
            origin: [0, 0],
            data: vec![0; cells * cells],
            heights: vec![i32::MIN; cells * cells],
            cursor: 0,
            generation: 0,
        }
    }

    pub fn origin(&self) -> [i32; 2] {
        self.origin
    }

    /// Moves the grid when the player left its middle half; the moved-in cells start unknown. Returns whether
    /// it moved.
    pub fn recenter(&mut self, x: i32, z: i32) -> bool {
        let span = self.step * self.cells as i32;
        let mid = [self.origin[0] + span / 2, self.origin[1] + span / 2];
        if (x - mid[0]).abs() < span / 4 && (z - mid[1]).abs() < span / 4 && self.generation != 0 {
            return false;
        }
        let snap = |v: i32| (v - span / 2).div_euclid(self.step) * self.step;
        self.origin = [snap(x), snap(z)];
        self.data.fill(0);
        self.heights.fill(i32::MIN);
        self.cursor = 0;
        self.generation += 1;
        true
    }

    /// Starts over at the same place (the source got new data).
    pub fn restart(&mut self) {
        self.cursor = 0;
    }

    /// Samples up to `budget` cells; returns how many changed (0 once the grid is complete).
    pub fn fill(&mut self, src: &mut dyn FarSource, budget: usize) -> usize {
        let total = self.cells * self.cells;
        let n = self.cells;
        let mut changed = 0;
        for _ in 0..budget {
            if self.cursor >= total {
                break;
            }
            let i = self.cursor;
            self.cursor += 1;
            let (cx, cz) = (i % n, i / n);
            let (x, z) = (self.origin[0] + cx as i32 * self.step, self.origin[1] + cz as i32 * self.step);
            let Some((h, c)) = src.surface(x, z) else {
                continue;
            };
            let at = |j: Option<usize>| j.map(|j| self.heights[j]).filter(|&v| v != i32::MIN).unwrap_or(h);
            let w = at((cx > 0).then(|| i - 1));
            let no = at((cz > 0).then(|| i - n));
            let nw = at((cx > 1 && cz > 1).then(|| i - 2 * n - 2));
            let px = if color::water_depth(c) > 0 {
                color::shade(color::OPAQUE | c, 268 - color::water_depth(c) * 9)
            } else {
                let s = self.step as f32;
                let slope = ((h - w) as f32 + (h - no) as f32) / s;
                let shadow = ((nw - h) as f32 - 2.0 * s * 0.85).max(0.0) * 0.04;
                let f = (1.0 + (slope * 0.45).clamp(-0.5, 0.4) - shadow.min(0.25)).clamp(0.4, 1.5);
                color::shade(color::OPAQUE | c, (f * 256.0) as u32)
            };
            self.heights[i] = h;
            if self.data[i] != px {
                self.data[i] = px;
                changed += 1;
            }
        }
        if changed > 0 {
            self.generation += 1;
        }
        changed
    }

    #[inline]
    pub fn sample(&self, x: i32, z: i32) -> u32 {
        let (gx, gz) = ((x - self.origin[0]).div_euclid(self.step), (z - self.origin[1]).div_euclid(self.step));
        if gx < 0 || gz < 0 || gx as usize >= self.cells || gz as usize >= self.cells {
            return 0;
        }
        self.data[gz as usize * self.cells + gx as usize]
    }

    pub fn known(&self) -> usize {
        self.data.iter().filter(|&&c| c >> 24 != 0).count()
    }

    pub fn complete(&self) -> bool {
        self.cursor >= self.cells * self.cells
    }
}

/// One far column as Horizon's level of detail keeps it (`lod::Surface`): the top solid face and the fluid face
/// above it, in absolute block coordinates of the face (one above the block), and the biome index.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Surface {
    pub height: i32,
    pub state: u32,
    pub biome: u16,
    pub water: Option<(i32, u32)>,
}

/// Biome tints by biome index (grass, foliage, water as 0xRRGGBB), copied on the main thread for the far thread.
#[derive(Clone, Default)]
pub struct TintTable {
    pub tints: Vec<(u32, u32, u32)>,
}

impl TintTable {
    #[inline]
    pub fn get(&self, biome: u16) -> (u32, u32, u32) {
        self.tints.get(biome as usize).copied().unwrap_or((0x7FB2_38, 0x59AE_30, 0x3F76_E4))
    }
}

impl Surface {
    /// The map pixel (albedo with the water material, see `color::water`) and the height of the column's top block.
    pub fn pixel(&self, x: i32, z: i32, table: &super::baker::ColorTable, tints: &TintTable) -> Option<(i32, u32)> {
        use super::baker::{MC_GRASS, MC_PLANT};
        let (g, f, w) = tints.get(self.biome);
        let ground = table.get(self.state).map(|(mc, rgb)| match mc {
            MC_GRASS => color::multiply(color::OPAQUE | rgb, g),
            MC_PLANT => color::multiply(color::OPAQUE | rgb, f),
            _ => color::OPAQUE | rgb,
        });
        match self.water {
            Some((wh, ws)) if wh > self.height => {
                let base = table.get(ws).map(|(_, rgb)| rgb).unwrap_or(0x3F76E4);
                let water = color::multiply(color::OPAQUE | base, w);
                let depth = (wh - self.height) as u32;
                let rgb = match ground {
                    Some(b) => color::mix(b, water, (120 + depth.min(15) * 28).min(256)),
                    None => water,
                };
                Some((wh - 1, color::water(rgb, depth)))
            }
            _ => ground.map(|c| (self.height - 1, color::shade(c, color::jitter(x, z, 10)))),
        }
    }
}

/// Bakes the 256 far columns of chunk (`cx`, `cz`) (index z * 16 + x) into the same form a live chunk bakes to,
/// so the view shades both alike. `None` when no column is known.
pub fn bake_surfaces(
    cx: i32,
    cz: i32,
    columns: &[Option<Surface>],
    table: &super::baker::ColorTable,
    tints: &TintTable,
) -> Option<super::baker::BakedChunk> {
    let mut out = super::baker::BakedChunk { cx, cz, color: [color::UNKNOWN; 256], height: [i16::MIN; 256], block: [0; 256] };
    let mut any = false;
    for (i, s) in columns.iter().enumerate().take(256) {
        let Some(s) = s else { continue };
        let (x, z) = (cx * 16 + (i % 16) as i32, cz * 16 + (i / 16) as i32);
        if let Some((h, c)) = s.pixel(x, z, table, tints) {
            out.color[i] = c;
            out.height[i] = h.clamp(i16::MIN as i32 + 1, i16::MAX as i32) as i16;
            // The top block for its texture (water columns show none).
            out.block[i] = if color::water_depth(c) > 0 { 0 } else { s.state };
            any = true;
        }
    }
    any.then_some(out)
}

/// Chunks in square rings around a centre, nearest ring first, without allocating: the order the far filler walks.
#[derive(Clone, Copy, Debug)]
pub struct Spiral {
    pub center: [i32; 2],
    pub radius: i32,
    ring: i32,
    i: i32,
}

impl Spiral {
    pub fn new(center: [i32; 2], radius: i32) -> Self {
        Spiral { center, radius, ring: 0, i: 0 }
    }
    /// Starts over from the centre (a new centre, or another pass over the same rings).
    pub fn restart(&mut self, center: [i32; 2]) {
        *self = Spiral::new(center, self.radius);
    }
    pub fn finished(&self) -> bool {
        self.ring > self.radius
    }
}

impl Iterator for Spiral {
    type Item = [i32; 2];
    fn next(&mut self) -> Option<[i32; 2]> {
        if self.ring > self.radius {
            return None;
        }
        let r = self.ring;
        let [cx, cz] = self.center;
        if r == 0 {
            self.ring = 1;
            self.i = 0;
            return Some([cx, cz]);
        }
        // 8r cells on ring r: top row left to right, right column, bottom row right to left, left column.
        let i = self.i;
        let side = 2 * r;
        let p = match i / side {
            0 => [cx - r + i, cz - r],
            1 => [cx + r, cz - r + (i - side)],
            2 => [cx + r - (i - 2 * side), cz + r],
            _ => [cx - r, cz + r - (i - 3 * side)],
        };
        self.i += 1;
        if self.i == 8 * r {
            self.ring += 1;
            self.i = 0;
        }
        Some(p)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Planning the far thread's work from what is on disk, without reading any chunk.

/// The chunks a Minecraft-style region file (`r.X.Z.mca`, also Horizon's server-terrain folder) holds, from its
/// 4 KiB location table alone: slot `x + z * 32` is present when its entry is non-zero.
pub fn mca_present(header: &[u8]) -> [bool; 1024] {
    let mut out = [false; 1024];
    if header.len() < 4096 {
        return out;
    }
    for (i, p) in out.iter_mut().enumerate() {
        let e = &header[i * 4..i * 4 + 4];
        // Offset (3 bytes, in 4 KiB sectors past the two header sectors) and a sector count.
        let offset = u32::from_be_bytes([0, e[0], e[1], e[2]]);
        *p = offset >= 2 && e[3] != 0;
    }
    out
}

/// `mca_present` of the file at `path`; `None` when there is no such file.
pub fn read_mca_present(path: &std::path::Path) -> Option<[bool; 1024]> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut header = vec![0u8; 4096];
    f.read_exact(&mut header).ok()?;
    Some(mca_present(&header))
}

/// Regions (32 x 32 chunks) touching the square of `radius` chunks around `focus`, nearest first.
pub fn regions_around(focus: [i32; 2], radius: i32) -> Vec<(i32, i32)> {
    let (x0, x1) = ((focus[0] - radius) >> 5, (focus[0] + radius) >> 5);
    let (z0, z1) = ((focus[1] - radius) >> 5, (focus[1] + radius) >> 5);
    let mut out = Vec::with_capacity(((x1 - x0 + 1) * (z1 - z0 + 1)) as usize);
    for rz in z0..=z1 {
        for rx in x0..=x1 {
            out.push((rx, rz));
        }
    }
    let mid = |r: i32| r * 32 + 16;
    out.sort_unstable_by_key(|&(rx, rz)| (mid(rx) - focus[0]).pow(2) + (mid(rz) - focus[1]).pow(2));
    out
}

/// One unit of far work: a chunk and which sources have it (bits of `SRC_*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FarTask {
    pub chunk: [i32; 2],
    pub sources: u8,
}

pub const SRC_HLOD: u8 = 1;
pub const SRC_TERRAIN: u8 = 2;
pub const SRC_SAVE: u8 = 4;

/// Squared chunk distance.
#[inline]
pub fn dist2(a: [i32; 2], b: [i32; 2]) -> i64 {
    let (dx, dz) = ((a[0] - b[0]) as i64, (a[1] - b[1]) as i64);
    dx * dx + dz * dz
}

/// Collects the work of one region: every chunk some source has (`present`, one mask per source bit), within
/// the circle of `radius` chunks around `focus`, that is neither `skip`ped (explored in a saved map region, or
/// baked already) nor done.
pub fn plan_region(
    key: (i32, i32),
    present: &[(u8, [bool; 1024])],
    skip: &[u64; 16],
    focus: [i32; 2],
    radius: i32,
    done: &impl Fn([i32; 2]) -> bool,
    out: &mut Vec<FarTask>,
) {
    let r2 = radius as i64 * radius as i64;
    for i in 0..1024usize {
        let mut sources = 0u8;
        for (bit, p) in present {
            if p[i] {
                sources |= bit;
            }
        }
        if sources == 0 || skip[i / 64] >> (i % 64) & 1 != 0 {
            continue;
        }
        let c = [key.0 * 32 + (i % 32) as i32, key.1 * 32 + (i / 32) as i32];
        if dist2(c, focus) > r2 || done(c) {
            continue;
        }
        out.push(FarTask { chunk: c, sources });
    }
}

/// Sorts tasks nearest `focus` first (what the view looks at fills in first).
pub fn sort_tasks(tasks: &mut [FarTask], focus: [i32; 2]) {
    tasks.sort_unstable_by_key(|t| dist2(t.chunk, focus));
}

/// Spreads a coarse grid (`side` x `side` columns, one every `stride` blocks from block (`x0`, `z0`)) over the
/// 256 columns of chunk (`cx`, `cz`): each column takes the cell it falls in. True when any column is known.
pub fn spread_grid(
    cx: i32,
    cz: i32,
    x0: i32,
    z0: i32,
    stride: i32,
    side: usize,
    grid: &[Option<Surface>],
    out: &mut [Option<Surface>; 256],
) -> bool {
    let mut any = false;
    for lz in 0..16 {
        for lx in 0..16 {
            let (x, z) = (cx * 16 + lx, cz * 16 + lz);
            let (ix, iz) = ((x - x0).div_euclid(stride), (z - z0).div_euclid(stride));
            let s = if ix >= 0 && iz >= 0 && (ix as usize) < side && (iz as usize) < side {
                grid.get(iz as usize * side + ix as usize).copied().flatten()
            } else {
                None
            };
            any |= s.is_some();
            out[(lz * 16 + lx) as usize] = s;
        }
    }
    any
}
