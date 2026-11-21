//! Composes cached region pixels into an RGBA image for the minimap or world map. Rotation, zoom and the round
//! mask are a transform at sampling time: turning the player never re-bakes a chunk, it only resamples.

use super::color;
use super::region::{Region, MIP_LEVELS, SIZE};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    /// World block coordinates (x, z) at the image centre.
    pub center: [f64; 2],
    /// Blocks per image pixel.
    pub bpp: f64,
    /// Radians; 0 is north up. The minimap in rotate mode passes the player's yaw so their heading is up.
    pub rotation: f64,
    pub round: bool,
    /// Hatch unexplored ground (the exploration fog feature) instead of leaving it plain dark.
    pub fog: bool,
    pub style: Style,
}

/// How the composed map is lit and decorated. All of it is applied while sampling, so changing a setting never
/// re-bakes a chunk.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Style {
    /// Relief strength: 0 flat, 1 soft, 2 normal, 3 strong (hillshade + cast shadow + ambient occlusion).
    pub relief: u8,
    /// Contour line interval in blocks, 0 = off.
    pub contours: u8,
    /// Grid lines: bit 0 chunks, bit 1 regions.
    pub grid: u8,
    /// Global light, 256 = full day. Below that the map is tinted towards `night`.
    pub light: u32,
    pub night: u32,
    /// Unknown ground comes out with alpha 0 (its colour still the fog tone), so a later pass (the cloud fog) can
    /// find it; far (estimated) terrain comes out with alpha `FAR_ALPHA`, since the player has not seen it either.
    /// The caller must make the image opaque afterwards.
    pub mark_unknown: bool,
    /// Block light strength (0..=256, 0 off): baked block light holds the night tint off and adds a warm glow.
    pub glow: u32,
    /// Sharp-bilinear texel filter and nearest mip (`render`); off: nearest texel, finer mip.
    pub smooth: bool,
    /// Block textures over live land when a block is a few pixels wide (`Lookup::detail`).
    pub textures: bool,
    /// The cloud fog covers far terrain too (`Settings::far_fog`). Off, new ground over far terrain comes out of
    /// the far terrain (a cross-fade from its colour), not out of the fog.
    pub far_fog: bool,
}

impl Default for Style {
    fn default() -> Self {
        Style {
            relief: 2,
            contours: 0,
            grid: 0,
            light: 256,
            night: NIGHT,
            mark_unknown: false,
            glow: 0,
            smooth: false,
            textures: false,
            far_fog: false,
        }
    }
}

impl View {
    pub fn new(center: [f64; 2], bpp: f64) -> Self {
        View { center, bpp, rotation: 0.0, round: false, fog: true, style: Style::default() }
    }
}

/// `Sync`: `render` splits a big image into row bands drawn on several threads, all reading the same lookup.
pub trait Lookup: Sync {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region>;
    /// Full-resolution far terrain (chunks baked from the LOD source or its disk cache), consulted where `region`
    /// knows nothing. Lit like live ground, drawn a little darker.
    fn far_region(&self, _rx: i32, _rz: i32) -> Option<&Region> {
        None
    }
    /// Coarse far terrain (ARGB, 0 when none, already shaded) for pixels no region knows.
    fn far(&self, _x: i32, _z: i32) -> u32 {
        0
    }
    /// How far block (`x`, `z`) has come out of the fog behind the reveal front, 0..=256 (256: fully, the usual
    /// answer). A field over blocks (`reveal::Front`), not per chunk, so the edge has no chunk steps.
    fn reveal(&self, _x: i32, _z: i32) -> u32 {
        256
    }
    /// Whether `far` can answer anything but 0. A lookup without coarse far terrain says no, and the render then
    /// fills ground no region knows without asking per pixel.
    fn has_coarse_far(&self) -> bool {
        true
    }
    /// The region rectangle `[x0, z0, x1, z1]` (inclusive) outside which neither `region` nor `far_region` has
    /// anything, when the lookup knows one: the render then asks nothing out there.
    fn bounds(&self) -> Option<[i32; 4]> {
        None
    }
    /// The texture pattern of the block shown at (`x`, `y`, `z`) (y the map height there), when known.
    fn detail(&self, _x: i32, _y: i32, _z: i32) -> Option<&super::detail::Detail> {
        None
    }
}

pub const FOG: u32 = 0xFF1B_1E24;
/// Alpha of far (estimated) terrain in a `mark_unknown` image: not seen by the player, so the cloud fog covers it
/// like unknown ground. Far tiles come in chunk and region squares; counted as explored they cut straight-edged
/// holes into the fog (v0.3.3 fix, DESIGN.md).
pub const FAR_ALPHA: u8 = 128;
pub const FOG_HATCH: u32 = 0xFF24_2830;
pub const VOID: u32 = 0xFF12_1418;
/// Moonlit blue the map fades towards at night.
pub const NIGHT: u32 = 0xFF4A_5A8C;
const SHORE: u32 = 0xFFE8_F4FF;

/// Mip level for a zoom: the coarsest level whose texels are still no larger than a pixel.
#[inline]
pub fn mip_for(bpp: f64) -> usize {
    if bpp < 2.0 { 0 } else { (bpp.log2().floor() as usize).min(MIP_LEVELS - 1) }
}

impl View {
    /// Image pixel (fractional) -> world block (x, z).
    #[inline]
    pub fn to_world(&self, w: usize, h: usize, px: f64, py: f64) -> [f64; 2] {
        let (dx, dy) = ((px - w as f64 / 2.0) * self.bpp, (py - h as f64 / 2.0) * self.bpp);
        let (s, c) = self.rotation.sin_cos();
        [self.center[0] + dx * c - dy * s, self.center[1] + dx * s + dy * c]
    }
    /// World block (x, z) -> image pixel (fractional).
    #[inline]
    pub fn to_image(&self, w: usize, h: usize, x: f64, z: f64) -> [f64; 2] {
        let (dx, dz) = (x - self.center[0], z - self.center[1]);
        let (s, c) = self.rotation.sin_cos();
        let (ix, iy) = (dx * c + dz * s, -dx * s + dz * c);
        [ix / self.bpp + w as f64 / 2.0, iy / self.bpp + h as f64 / 2.0]
    }
}

/// `x.floor() as i32` for coordinates within the i32 range, without the libm call `floor` is on a baseline
/// x86-64 build (four of them a pixel were a tenth of a far-zoomed frame).
#[inline(always)]
fn ffloor(x: f64) -> i32 {
    let i = x as i32;
    i - ((x < i as f64) as i32)
}

/// The explored and the far region of a region cell.
type Pair<'a> = (Option<&'a Region>, Option<&'a Region>);

/// The regions under a render, looked up once: the image's region rectangle (cut to the lookup's `bounds`) as a
/// table, so no texel pays a hash lookup. A far-zoomed row crossed a region every few pixels and asked the
/// lookup's hash maps a dozen times for each.
struct Grid<'a> {
    lk: &'a dyn Lookup,
    x0: i32,
    z0: i32,
    w: i32,
    h: i32,
    cells: Vec<Pair<'a>>,
    bounds: Option<[i32; 4]>,
}

/// Region cells a render tabulates at most (16 bytes each); a wider view without `bounds` asks the lookup.
const GRID_MAX: i64 = 1 << 16;

impl<'a> Grid<'a> {
    fn new(lk: &'a dyn Lookup, v: &View, w: usize, h: usize) -> Self {
        let bounds = lk.bounds();
        let mut lo = [f64::MAX; 2];
        let mut hi = [f64::MIN; 2];
        for (px, py) in [(0.0, 0.0), (w as f64, 0.0), (0.0, h as f64), (w as f64, h as f64)] {
            let p = v.to_world(w, h, px, py);
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        // A region of margin: the filter and the relief read a few texels past the image.
        let reg = |b: f64| (b / SIZE as f64).floor().clamp(-1e9, 1e9) as i64;
        let (mut x0, mut z0, mut x1, mut z1) = (reg(lo[0]) - 1, reg(lo[1]) - 1, reg(hi[0]) + 1, reg(hi[1]) + 1);
        if let Some(b) = bounds {
            (x0, z0, x1, z1) = (x0.max(b[0] as i64), z0.max(b[1] as i64), x1.min(b[2] as i64), z1.min(b[3] as i64));
        }
        let (gw, gh) = ((x1 - x0 + 1).max(0), (z1 - z0 + 1).max(0));
        let mut g = Grid { lk, x0: x0.clamp(i32::MIN as i64, i32::MAX as i64) as i32, z0: z0.clamp(i32::MIN as i64, i32::MAX as i64) as i32, w: 0, h: 0, cells: Vec::new(), bounds };
        if gw > 0 && gh > 0 && gw * gh <= GRID_MAX {
            (g.w, g.h) = (gw as i32, gh as i32);
            g.cells.reserve_exact((gw * gh) as usize);
            for rz in z0..=z1 {
                for rx in x0..=x1 {
                    g.cells.push((lk.region(rx as i32, rz as i32), lk.far_region(rx as i32, rz as i32)));
                }
            }
        }
        g
    }

    #[inline]
    fn get(&self, rx: i32, rz: i32) -> Pair<'a> {
        let (i, j) = (rx.wrapping_sub(self.x0), rz.wrapping_sub(self.z0));
        if (i as u32) < self.w as u32 && (j as u32) < self.h as u32 {
            return self.cells[(j * self.w + i) as usize];
        }
        if let Some(b) = self.bounds {
            if rx < b[0] || rz < b[1] || rx > b[2] || rz > b[3] {
                return (None, None);
            }
        }
        (self.lk.region(rx, rz), self.lk.far_region(rx, rz))
    }

    /// Whether no region row from `rz0` to `rz1` can hold anything (all of them outside the bounds).
    #[inline]
    fn rows_empty(&self, rz0: i32, rz1: i32) -> bool {
        self.bounds.is_some_and(|b| rz1 < b[1] || rz0 > b[3])
    }
}

/// Texel (colour, height) at global texel coordinates of mip `level`, live first, then far.
#[inline]
fn texel(grid: &Grid, level: usize, tx: i32, tz: i32) -> (u32, i16) {
    let shift = 9 - level as i32;
    let side = SIZE >> level;
    let i = (tz & (side as i32 - 1)) as usize * side + (tx & (side as i32 - 1)) as usize;
    let (live, far) = grid.get(tx >> shift, tz >> shift);
    if let Some(r) = live {
        let c = r.level(level)[i];
        if c >> 24 != 0 {
            return (c, r.height_level(level)[i]);
        }
    }
    match far {
        Some(r) => (r.level(level)[i], r.height_level(level)[i]),
        None => (color::UNKNOWN, i16::MIN),
    }
}

/// Lights one known texel: hillshade from the north-west, a soft cast shadow, ambient occlusion in hollows,
/// water depth tone and shoreline, contour lines. `n` holds the (colour, height) of the west, north, east,
/// south and two-texels-north-west neighbours; `s` is blocks per texel.
#[inline]
fn light(c: u32, h: i16, n: &[(u32, i16); 5], s: f32, st: &Style) -> u32 {
    let hf = h as f32;
    let hn = |k: usize| if n[k].1 == i16::MIN { hf } else { n[k].1 as f32 };
    let depth = color::water_depth(c);
    if depth > 0 {
        // Deeper water darker; a light rim where it meets land.
        let mut out = color::shade(color::OPAQUE | c, 268 - depth * 9);
        if n[..4].iter().any(|&(nc, _)| nc >> 24 != 0 && color::water_depth(nc) == 0) {
            out = color::mix(out, SHORE, 70);
        }
        return out;
    }
    let mut c = color::OPAQUE | c;
    if st.relief > 0 {
        let k = st.relief as f32 * 0.5;
        let (w, nn, e, so) = (hn(0), hn(1), hn(2), hn(3));
        let dx = (e - w) / (2.0 * s);
        let dz = (so - nn) / (2.0 * s);
        // Facing the light (rising to the south-east) brighter, away from it darker.
        let lambert = ((dx + dz) * 0.32).clamp(-0.55, 0.45);
        // Ambient occlusion: hollows (neighbours higher on average) darker.
        let lap = ((w + nn + e + so) - 4.0 * hf) / s;
        let ao = (lap * 0.035).clamp(0.0, 0.22);
        // Cast shadow: terrain two texels towards the light rising above a 40 degree ray.
        let block = hn(4) - hf - 2.0 * s * 0.85;
        let shadow = (block * 0.06).clamp(0.0, 0.28);
        let f = (1.0 + k * (lambert - ao - shadow)).clamp(0.35, 1.6);
        c = color::shade(c, (f * 256.0) as u32);
    }
    if st.contours > 0 && s <= 4.0 {
        let i = st.contours as i32;
        let band = (h as i32).div_euclid(i);
        let crosses = |k: usize| n[k].1 != i16::MIN && (n[k].1 as i32).div_euclid(i) < band;
        if crosses(0) || crosses(1) || crosses(2) || crosses(3) {
            c = color::shade(c, if band % 5 == 0 { 170 } else { 205 });
        }
    }
    c
}

/// Output alpha of ground coming out of the fog in a `mark_unknown` image: `REVEAL_ALPHA + reveal * 125 / 256`
/// (129..=254), the colour the fully revealed terrain. Before the cloud pass the caller settles it to explored or
/// unexplored at the ramp's middle (`settle_revealing`).
pub const REVEAL_ALPHA: u8 = 129;

/// The reveal fraction (0..=256) a `mark_unknown` output alpha carries, `None` for any other alpha.
#[inline]
pub fn revealing(a: u8) -> Option<u32> {
    (REVEAL_ALPHA..=254).contains(&a).then(|| (a - REVEAL_ALPHA) as u32 * 256 / 125)
}

/// Hands the reveal ramp of a `mark_unknown` image to the cloud pass (0.3.7): ground past the middle of the ramp
/// counts as explored (alpha 255), the rest as unexplored (alpha 0). The cloud edge then sits at the front's
/// middle and recedes with it, feathered by the clouds themselves. Before, the deck stayed over the whole ramp
/// and the ramp was blended back to the terrain afterwards, while the deck's own feather started again past the
/// ramp's end: fog, a clear strip, a second fog rim, then the ground.
pub fn settle_revealing(img: &mut [u8]) {
    for a in img.iter_mut().skip(3).step_by(4) {
        if let Some(k) = revealing(*a) {
            *a = if k >= 128 { 255 } else { 0 };
        }
    }
}

/// Warm colour block light adds.
pub const GLOW: u32 = 0xFFFF_B45A;

/// Night tint and block light on a lit texel: light `bl` (0..=15) holds the night darkening off and adds a warm
/// glow, stronger the darker the map.
#[inline]
fn night_glow(px: u32, bl: u32, night: u32, st: &Style) -> u32 {
    let lf = if st.glow > 0 && bl > 0 { (bl * bl * 256 / 225) * st.glow.min(256) / 256 } else { 0 };
    let n = night * (256 - lf) / 256;
    let mut px = if n > 0 { color::mix(px, color::multiply(px, st.night), n) } else { px };
    if lf > 0 {
        let amt = lf * (36 + night * 140 / 256) / 256;
        px = color::mix(px, color::multiply(GLOW, px | 0xFF40_3020), amt / 2);
        px = color::add(px, GLOW, amt / 3);
    }
    px
}

// A texel as `lit` gives it, packed in a u64: colour (32 bits), output alpha (8), kind (2: 0 unknown, 1 far,
// 2 live), water (1), height (16).
const K_UNKNOWN: u64 = 0;
const K_FAR: u64 = 1;
const K_LIVE: u64 = 2;

#[inline]
fn pack(c: u32, a: u8, kind: u64, water: bool, h: i16) -> u64 {
    (c as u64) | (a as u64) << 32 | kind << 40 | (water as u64) << 42 | ((h as u16) as u64) << 48
}

/// The texel cache of one band of a render, kept between renders (no allocation once warm).
#[derive(Default)]
struct TexCache {
    val: Vec<u64>,
    stamp: Vec<u32>,
    now: u32,
}

/// Texel caches of the render workers, taken for a render and given back after it.
static TEX_CACHES: std::sync::Mutex<Vec<TexCache>> = std::sync::Mutex::new(Vec::new());

/// Render threads (0: automatic, see `render_threads`).
static THREADS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Sets the number of threads `render` uses (0: automatic). The image is the same with any number.
pub fn set_render_threads(n: usize) {
    THREADS.store(n, std::sync::atomic::Ordering::Relaxed);
}

/// Smaller images render on the calling thread: below this, starting threads costs more than they save.
const PAR_MIN_PIXELS: usize = 32 * 1024;

fn render_threads(pixels: usize) -> usize {
    if pixels < PAR_MIN_PIXELS {
        return 1;
    }
    match THREADS.load(std::sync::atomic::Ordering::Relaxed) {
        // Half the cores: the game draws on the others (6 of 12 cores drew a far-zoomed 1024 x 576 map 3x as fast as
        // one; more gained little).
        0 => (std::thread::available_parallelism().map_or(1, |n| n.get()) / 2).clamp(1, 8),
        n => n,
    }
}

/// Texels a render may cache (bigger views compute every texel they touch).
const CACHE_MAX: usize = 1 << 23;

struct Sampler<'a, 'g> {
    lk: &'a dyn Lookup,
    grid: &'g Grid<'a>,
    st: &'a Style,
    level: usize,
    side: usize,
    shift: i32,
    tscale: f32,
    night: u32,
    fog: bool,
}

impl<'a> Sampler<'a, '_> {
    /// Lights texel (`tx`, `tz`) of the render's mip level: relief, far shade, night and block light, the fog fade
    /// of newly explored ground. Unknown texels come out as kind 0; the pixel fills them in (hatch, coarse far).
    fn lit(&mut self, tx: i32, tz: i32) -> u64 {
        let (level, side, shift) = (self.level, self.side, self.shift);
        let (rx, rz) = (tx >> shift, tz >> shift);
        let (live, fars) = self.grid.get(rx, rz);
        let (ix, iz) = ((tx & (side as i32 - 1)) as usize, (tz & (side as i32 - 1)) as usize);
        let at = iz * side + ix;
        let (mut p, mut ph, mut src, mut far) = match live {
            Some(r) => (r.level(level)[at], r.height_level(level)[at], live, false),
            None => (color::UNKNOWN, i16::MIN, None, false),
        };
        if p >> 24 == 0 {
            if let Some(r) = fars {
                (p, ph, src, far) = (r.level(level)[at], r.height_level(level)[at], fars, true);
            }
        }
        if p >> 24 == 0 {
            return pack(0, 0, K_UNKNOWN, false, i16::MIN);
        }
        let r = src.unwrap();
        let n = if ix >= 2 && iz >= 2 && ix + 1 < side && iz + 1 < side {
            let (cs, hs) = (r.level(level), r.height_level(level));
            let g = |i: usize| (cs[i], hs[i]);
            [g(at - 1), g(at - side), g(at + 1), g(at + side), g(at - 2 * side - 2)]
        } else {
            [
                texel(self.grid, level, tx - 1, tz),
                texel(self.grid, level, tx, tz - 1),
                texel(self.grid, level, tx + 1, tz),
                texel(self.grid, level, tx, tz + 1),
                texel(self.grid, level, tx - 2, tz - 2),
            ]
        };
        let st = self.st;
        let mut px = light(p, ph, &n, self.tscale, st);
        if far {
            px = color::shade(px, 228);
        }
        px = night_glow(px, color::block_light(p), self.night, st);
        let mut a = if far && st.mark_unknown { FAR_ALPHA } else { 255 };
        if !far {
            // The block under the texel's centre.
            let half = (1i32 << level) >> 1;
            let reveal = self.lk.reveal((tx << level) + half, (tz << level) + half);
            // New ground over far terrain the player already sees (0.3.7): the chunk replaces the far texel with a
            // cross-fade, no fog in between.
            let under = if reveal < 256 && !(st.mark_unknown && st.far_fog) { self.far_under(fars, at, &n) } else { None };
            if let Some(fp) = under {
                px = color::mix(fp, px, reveal);
            } else if reveal < 256 {
                if st.mark_unknown {
                    a = REVEAL_ALPHA + (reveal * 125 / 256) as u8;
                } else {
                    let under = if self.fog { FOG } else { VOID };
                    px = color::mix(under, px, reveal);
                }
            }
        }
        pack(px, a, if far { K_FAR } else { K_LIVE }, color::water_depth(p) > 0, ph)
    }

    /// The far terrain texel under live texel `at` of region (`rx`, `rz`), lit as `lit` would show it (with the
    /// live neighbours `n` for its relief), when there is one.
    fn far_under(&mut self, far: Option<&Region>, at: usize, n: &[(u32, i16); 5]) -> Option<u32> {
        let r = far?;
        let (fp, fh) = (r.level(self.level)[at], r.height_level(self.level)[at]);
        if fp >> 24 == 0 {
            return None;
        }
        let px = color::shade(light(fp, fh, n, self.tscale, self.st), 228);
        Some(night_glow(px, color::block_light(fp), self.night, self.st))
    }
}

/// Renders `w` x `h` RGBA pixels into `out` (row stride `w * 4`). No allocation once warm: the caller owns the
/// buffer and reuses it every update, the texel caches are kept between renders.
///
/// A big image is drawn in bands of rows on up to `render_threads` threads (the caller's one among them). Every
/// row starts from the view alone and every texel's light depends only on the lookup, so the image is the same
/// bit for bit however it is split.
///
/// With `Style::smooth` the texels are filtered sharp-bilinear: flat inside a texel, blended over about one
/// screen pixel at its edges (plain bilinear when a texel is smaller than a pixel), so the map glides under a
/// moving or turning player instead of snapping a whole texel at a time; the mip level is the nearest one
/// (not the next finer), so a texel is never much smaller than a pixel either.
pub fn render(out: &mut [u8], w: usize, h: usize, v: &View, lk: &dyn Lookup) {
    debug_assert!(out.len() >= w * h * 4);
    if w == 0 || h == 0 {
        return;
    }
    let out = &mut out[..w * h * 4];
    let take = || TEX_CACHES.lock().map(|mut p| p.pop()).ok().flatten().unwrap_or_default();
    let give = |tc: TexCache| {
        if let Ok(mut p) = TEX_CACHES.lock() {
            p.push(tc);
        }
    };
    let grid = Grid::new(lk, v, w, h);
    let grid = &grid;
    let n = render_threads(w * h).min(h);
    if n <= 1 {
        let mut tc = take();
        render_rows(out, w, h, 0, h, v, grid, &mut tc);
        give(tc);
        return;
    }
    // A few bands per thread, so a thread that drew cheap rows (unknown ground) takes another band.
    let band = (h / (n * 4)).max(8);
    let jobs = std::sync::Mutex::new(out.chunks_mut(band * w * 4).enumerate());
    let work = || {
        let mut tc = take();
        loop {
            let Some((i, rows)) = jobs.lock().ok().and_then(|mut j| j.next()) else { break };
            let y0 = i * band;
            render_rows(rows, w, h, y0, (y0 + band).min(h), v, grid, &mut tc);
        }
        give(tc);
    };
    std::thread::scope(|s| {
        for _ in 1..n {
            s.spawn(work);
        }
        work();
    });
}

/// Rows `y0..y1` of the image into `out` (those rows only).
#[allow(clippy::too_many_arguments)]
fn render_rows(out: &mut [u8], w: usize, h: usize, y0: usize, y1: usize, v: &View, grid: &Grid, tc: &mut TexCache) {
    let lk = grid.lk;
    let st = &v.style;
    // The finer mip of the two around the scale (a texel at most a pixel wide), filtered down by the pixel's own
    // footprint: the nearest mip (0.3.4) magnified a coarser one between 1.41 and 2 texels a pixel, and those
    // zoom steps came out soft next to their neighbours.
    let level = mip_for(v.bpp * (1.0 + 1e-6));
    let side = SIZE >> level;
    let shift = 9 - level as i32;
    let tscale = (1usize << level) as f32;
    let (s, c) = v.rotation.sin_cos();
    // Step vectors of one pixel right / down in world space.
    let (rx_x, rx_z) = (c * v.bpp, s * v.bpp);
    let (dy_x, dy_z) = (-s * v.bpp, c * v.bpp);
    let r2 = (w.min(h) as f64 / 2.0).powi(2);
    let (hw, hh) = (w as f64 / 2.0, h as f64 / 2.0);
    let chunk_grid = st.grid & 1 != 0 && v.bpp <= 2.0;
    let region_grid = st.grid & 2 != 0;
    let night = 256u32.saturating_sub(st.light.min(256));
    let mut sm = Sampler {
        lk,
        grid,
        st,
        level,
        side,
        shift,
        tscale,
        night,
        fog: v.fog,
    };
    // The texel box the rows cover (their four corners), one texel of margin for the filter.
    let mut lo = [f64::MAX; 2];
    let mut hi = [f64::MIN; 2];
    let (ya, yb) = (y0 as f64, y1 as f64);
    for (px, py) in [(0.0, ya), (w as f64, ya), (0.0, yb), (w as f64, yb)] {
        let p = v.to_world(w, h, px, py);
        for k in 0..2 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    }
    let t0 = [(lo[0] / tscale as f64).floor() as i64 - 2, (lo[1] / tscale as f64).floor() as i64 - 2];
    let t1 = [(hi[0] / tscale as f64).floor() as i64 + 2, (hi[1] / tscale as f64).floor() as i64 + 2];
    let (cw, ch) = ((t1[0] - t0[0] + 1).max(0) as usize, (t1[1] - t0[1] + 1).max(0) as usize);
    // Below half a pixel per texel (the coarsest mip zoomed out further) neighbouring pixels are two texels or
    // more apart in any direction (2.83 when turned) and share none: the cache would only cost a miss per texel.
    let lonely = tscale as f64 / v.bpp < if v.rotation == 0.0 { 0.5 } else { 0.35 };
    let cached = !lonely && cw.saturating_mul(ch) <= CACHE_MAX && t0[0] > i32::MIN as i64 / 2 && t1[0] < i32::MAX as i64 / 2;
    if cached {
        let n = cw * ch;
        if tc.val.len() < n {
            tc.val.resize(n, 0);
            tc.stamp.resize(n, 0);
        }
        tc.now = tc.now.wrapping_add(1);
        if tc.now == 0 {
            tc.stamp.iter_mut().for_each(|s| *s = 0);
            tc.now = 1;
        }
    }
    let (ox, oz) = (t0[0] as i32, t0[1] as i32);
    let stamp_now = tc.now;
    let mut get = |sm: &mut Sampler, tx: i32, tz: i32| -> u64 {
        if cached {
            let (i, j) = ((tx - ox) as usize, (tz - oz) as usize);
            if i < cw && j < ch {
                let k = j * cw + i;
                if tc.stamp[k] != stamp_now {
                    tc.stamp[k] = stamp_now;
                    tc.val[k] = sm.lit(tx, tz);
                }
                return tc.val[k];
            }
        }
        sm.lit(tx, tz)
    };
    // Sharpness of the filter: screen pixels per texel. Above 1, flat texels with a one pixel blend at their
    // edges; below, the pixel's footprint (1 / ppt texels) as a box over the two nearest texels, its exact area
    // average whenever it covers no more than two (a wider tent, plain bilinear, blurred minified steps).
    let ppt = tscale as f64 / v.bpp;
    let inv = 1.0 / tscale as f64;
    // Block textures when a block is a few pixels wide.
    let ppb = (1.0 / v.bpp) as f32;
    let tex_k = if st.textures && level == 0 { super::detail::strength(ppb) } else { 0 };
    let mut tex_last: (i32, i32, i16, Option<&super::detail::Detail>) = (i32::MIN, i32::MIN, i16::MIN, None);
    // Empty spans (an unturned view only, where a row runs along world x): where neither the explored map nor
    // the far terrain has a region under a pixel's texels, the pixel is the unknown fill whatever the filter
    // does, so it is written straight away. A far-zoomed map is mostly that; it paid the full sampling for it.
    let span_ok = v.rotation == 0.0 && rx_x > 0.0 && rx_z == 0.0;
    let coarse = lk.has_coarse_far();
    let fog_px = {
        let mut o = color::to_rgba(if st.mark_unknown || v.fog { FOG } else { VOID });
        o[3] = if st.mark_unknown { 0 } else { 255 };
        o
    };
    let hatch_px = color::to_rgba(FOG_HATCH);
    let hatched = !st.mark_unknown && v.fog;
    let side_i = side as i32;
    for y in y0..y1 {
        let fy = y as f64 + 0.5 - hh;
        let fx0 = 0.5 - hw;
        let mut wx = v.center[0] + fx0 * rx_x + fy * dy_x;
        let mut wz = v.center[1] + fx0 * rx_z + fy * dy_z;
        let row = &mut out[(y - y0) * w * 4..(y - y0 + 1) * w * 4];
        // World x up to which the pixels of this row lie over no region at all, and the region column asked last.
        let mut empty_until = f64::NEG_INFINITY;
        let mut probed = i32::MIN;
        let bz_row = ffloor(wz);
        if span_ok && !coarse && !hatched && !v.round {
            // A row outside the lookup's bounds altogether: one fill.
            let tz = if st.smooth { ffloor(wz * inv - 0.5) } else { bz_row >> level };
            if grid.rows_empty(tz >> shift, (tz + 1) >> shift) {
                for px in row.chunks_exact_mut(4) {
                    px.copy_from_slice(&fog_px);
                }
                continue;
            }
        }
        for x in 0..w {
            if v.round && ((x as f64 + 0.5 - hw).powi(2) + fy * fy) > r2 {
                row[x * 4..x * 4 + 4].copy_from_slice(&[0, 0, 0, 0]);
                wx += rx_x;
                wz += rx_z;
                continue;
            }
            if wx < empty_until {
                if !coarse || lk.far(ffloor(wx), bz_row) >> 24 == 0 {
                    let o = if hatched && ((ffloor(wx) >> 2) + (bz_row >> 2)) & 3 == 0 { &hatch_px } else { &fog_px };
                    row[x * 4..x * 4 + 4].copy_from_slice(o);
                    wx += rx_x;
                    wz += rx_z;
                    continue;
                }
            }
            let (bx, bz) = (ffloor(wx), ffloor(wz));
            // The texels around the pixel and their weights; the dominant one decides alpha, grid and texture.
            let (tx, tz, fu, fv) = if st.smooth {
                let (u, w2) = (wx * inv - 0.5, wz * inv - 0.5);
                let (iu, iv) = (ffloor(u), ffloor(w2));
                let sharpen = |f: f64| ((f - 0.5) * ppt + 0.5).clamp(0.0, 1.0);
                (iu, iv, sharpen(u - iu as f64), sharpen(w2 - iv as f64))
            } else {
                (bx >> level, bz >> level, 0.0, 0.0)
            };
            if span_ok && (tx >> shift) != probed {
                let rx = tx >> shift;
                probed = rx;
                // The last texel column of the region, and the texel rows the filter may read.
                let last = rx * side_i + side_i - 1;
                let (rz0, rz1) = (tz >> shift, (tz + st.smooth as i32) >> shift);
                let none = |rz: i32| matches!(grid.get(rx, rz), (None, None));
                if none(rz0) && (rz1 == rz0 || none(rz1)) {
                    // Smooth reads texel columns floor(wx / t - 0.5) and the next: both inside the region while
                    // wx < (last - 0.5) * t (a texel early, clear of rounding). Plain reads floor(wx) >> level.
                    empty_until = if st.smooth { (last as f64 - 0.5) * tscale as f64 } else { (rx as f64 + 1.0) * SIZE as f64 };
                    if wx < empty_until && (!coarse || lk.far(bx, bz) >> 24 == 0) {
                        let o = if hatched && ((bx >> 2) + (bz >> 2)) & 3 == 0 { &hatch_px } else { &fog_px };
                        row[x * 4..x * 4 + 4].copy_from_slice(o);
                        wx += rx_x;
                        wz += rx_z;
                        continue;
                    }
                }
            }
            let q = [
                get(&mut sm, tx, tz),
                if fu > 0.0 { get(&mut sm, tx + 1, tz) } else { 0 },
                if fv > 0.0 { get(&mut sm, tx, tz + 1) } else { 0 },
                if fu > 0.0 && fv > 0.0 { get(&mut sm, tx + 1, tz + 1) } else { 0 },
            ];
            let (fu, fv) = (fu as f32, fv as f32);
            let wts = [(1.0 - fu) * (1.0 - fv), fu * (1.0 - fv), (1.0 - fu) * fv, fu * fv];
            let dom = (0..4).fold(0, |b, k| if wts[k] > wts[b] { k } else { b });
            // Unknown ground for this pixel: the coarse far terrain or the fog.
            let mut unknown: Option<(u32, u8, bool)> = None;
            let fill = |unknown: &mut Option<(u32, u8, bool)>| -> (u32, u8, bool) {
                *unknown.get_or_insert_with(|| {
                    let far = lk.far(bx, bz);
                    if far >> 24 != 0 {
                        // Far terrain drawn a little darker, so what is live and what is estimated stays readable.
                        let px = color::shade(color::OPAQUE | far, 214);
                        let px = if night > 0 { color::mix(px, color::multiply(px, st.night), night) } else { px };
                        (px, if st.mark_unknown { FAR_ALPHA } else { 255 }, true)
                    } else if st.mark_unknown {
                        (FOG, 0, false)
                    } else if v.fog && ((bx >> 2) + (bz >> 2)) & 3 == 0 {
                        (FOG_HATCH, 255, false)
                    } else if v.fog {
                        (FOG, 255, false)
                    } else {
                        (VOID, 255, false)
                    }
                })
            };
            let (mut r, mut g, mut b) = (0f32, 0f32, 0f32);
            let (mut alpha, mut known) = (255u8, true);
            for k in 0..4 {
                if wts[k] <= 0.0 {
                    continue;
                }
                let t = q[k];
                let (c, a, kn) = if (t >> 40) & 3 == K_UNKNOWN { fill(&mut unknown) } else { (t as u32, (t >> 32) as u8, true) };
                if k == dom {
                    alpha = a;
                    known = kn;
                }
                r += ((c >> 16) & 0xFF) as f32 * wts[k];
                g += ((c >> 8) & 0xFF) as f32 * wts[k];
                b += (c & 0xFF) as f32 * wts[k];
            }
            let mut px = color::OPAQUE
                | ((r + 0.5) as u32).min(255) << 16
                | ((g + 0.5) as u32).min(255) << 8
                | ((b + 0.5) as u32).min(255);
            if tex_k > 0 {
                let t = q[dom];
                // Explored and far ground alike (the far columns' blocks are saved with them); not water.
                if matches!((t >> 40) & 3, K_LIVE | K_FAR) && (t >> 42) & 1 == 0 {
                    let hgt = (t >> 48) as u16 as i16;
                    if (bx, bz, hgt) != (tex_last.0, tex_last.1, tex_last.2) {
                        tex_last = (bx, bz, hgt, lk.detail(bx, hgt as i32, bz));
                    }
                    if let Some(d) = tex_last.3 {
                        if let Some(ratio) = d.sample((wx - bx as f64) as f32, (wz - bz as f64) as f32, ppb) {
                            px = super::detail::apply(px, ratio, tex_k);
                        }
                    }
                }
            }
            if known && (chunk_grid || region_grid) {
                let (lx, lz) = (ffloor(wx - v.bpp), ffloor(wz - v.bpp));
                if region_grid && ((lx >> 9) != (bx >> 9) || (lz >> 9) != (bz >> 9)) {
                    px = color::mix(px, 0xFF10_1010, 120);
                } else if chunk_grid && ((lx >> 4) != (bx >> 4) || (lz >> 4) != (bz >> 4)) {
                    px = color::mix(px, 0xFF10_1010, 60);
                }
            }
            let mut o = color::to_rgba(px);
            o[3] = alpha;
            row[x * 4..x * 4 + 4].copy_from_slice(&o);
            wx += rx_x;
            wz += rx_z;
        }
    }
}

/// The region rectangle `[x0, z0, x1, z1]` (inclusive) a view of `w` x `h` touches (the rotated view's bounding
/// circle).
pub fn visible_rect(v: &View, w: usize, h: usize) -> [i32; 4] {
    let half = if v.rotation == 0.0 {
        [w as f64 / 2.0 * v.bpp, h as f64 / 2.0 * v.bpp]
    } else {
        let r = ((w * w + h * h) as f64).sqrt() / 2.0 * v.bpp;
        [r, r]
    };
    [
        ((v.center[0] - half[0]).floor() as i32) >> 9,
        ((v.center[1] - half[1]).floor() as i32) >> 9,
        ((v.center[0] + half[0]).floor() as i32) >> 9,
        ((v.center[1] + half[1]).floor() as i32) >> 9,
    ]
}

/// Regions a view of `w` x `h` touches (`visible_rect`), into `out` (cleared first).
pub fn visible_regions(v: &View, w: usize, h: usize, out: &mut Vec<(i32, i32)>) {
    out.clear();
    let [x0, z0, x1, z1] = visible_rect(v, w, h);
    for rz in z0..=z1 {
        for rx in x0..=x1 {
            out.push((rx, rz));
        }
    }
}

/// Explored share of the chunks a view covers, in percent (the exploration readout).
pub fn explored_percent(v: &View, w: usize, h: usize, lk: &dyn Lookup) -> f32 {
    let a = v.to_world(w, h, 0.0, 0.0);
    let b = v.to_world(w, h, w as f64 - 0.5, h as f64 - 0.5);
    let (cx0, cx1) = ((a[0].min(b[0]).floor() as i32) >> 4, (a[0].max(b[0]).floor() as i32) >> 4);
    let (cz0, cz1) = ((a[1].min(b[1]).floor() as i32) >> 4, (a[1].max(b[1]).floor() as i32) >> 4);
    // Sample at most 64x64 chunks so a far-zoomed map stays cheap.
    let step = (((cx1 - cx0).max(cz1 - cz0) / 64) + 1).max(1);
    let (mut seen, mut total) = (0u32, 0u32);
    let mut cz = cz0;
    while cz <= cz1 {
        let mut cx = cx0;
        while cx <= cx1 {
            total += 1;
            if let Some(r) = lk.region(cx >> 5, cz >> 5) {
                if r.chunk_explored((cx & 31) as usize, (cz & 31) as usize) {
                    seen += 1;
                }
            }
            cx += step;
        }
        cz += step;
    }
    if total == 0 { 0.0 } else { seen as f32 * 100.0 / total as f32 }
}

/// Smooths a position the game moves in steps (the player, 20 times a second) for the map: each new position is
/// reached over one step period from where the map shows it now, so the map glides instead of jumping a fraction
/// of a block every tick. Teleports (a jump over `SNAP` blocks) are taken at once.
#[derive(Clone, Copy, Debug, Default)]
pub struct Follow {
    from: [f64; 2],
    to: [f64; 2],
    /// When the last step came, and the step period in seconds (measured, 30..=120 ms).
    t0: Option<std::time::Instant>,
    period: f64,
}

impl Follow {
    pub const SNAP: f64 = 8.0;

    /// Feeds the game's position at `now`; returns the position to show.
    pub fn update(&mut self, raw: [f64; 2], now: std::time::Instant) -> [f64; 2] {
        let Some(t0) = self.t0 else {
            *self = Follow { from: raw, to: raw, t0: Some(now), period: 0.05 };
            return raw;
        };
        if raw != self.to {
            let shown = self.at(now);
            let dt = now.saturating_duration_since(t0).as_secs_f64();
            if (raw[0] - shown[0]).hypot(raw[1] - shown[1]) > Self::SNAP {
                self.from = raw;
            } else {
                self.from = shown;
                // A step after a pause is not the period.
                if dt < 0.25 {
                    self.period = (self.period * 0.7 + dt.clamp(0.03, 0.12) * 0.3).clamp(0.03, 0.12);
                }
            }
            self.to = raw;
            self.t0 = Some(now);
        }
        self.at(now)
    }

    /// The shown position at `now`.
    pub fn at(&self, now: std::time::Instant) -> [f64; 2] {
        let Some(t0) = self.t0 else { return self.to };
        let k = (now.saturating_duration_since(t0).as_secs_f64() / self.period.max(0.001)).clamp(0.0, 1.0);
        [self.from[0] + (self.to[0] - self.from[0]) * k, self.from[1] + (self.to[1] - self.from[1]) * k]
    }

    /// Forgets the position (a new world or dimension): the next one is taken as is.
    pub fn reset(&mut self) {
        self.t0 = None;
    }
}
