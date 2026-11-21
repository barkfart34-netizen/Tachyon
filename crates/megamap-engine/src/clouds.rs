//! The exploration fog as living clouds over unexplored ground. Pure CPU (the mod has no shader hook): a coarse
//! node grid (one node per 3 to 8 image pixels) carries cloud opacity, colour terms and an additive glow; the
//! composite pass interpolates the nodes bilinearly over the map pixels the view left unknown (alpha below 255:
//! 0 unknown, `view::FAR_ALPHA` far estimated terrain; see `view::Style::mark_unknown`). Both passes split the rows over a few threads on big maps.
//!
//! Density is world-anchored value-noise fbm, domain-warped for curling wisps and blended with worley heaps for
//! cumulus; octaves finer than the node spacing fade to their mean instead of aliasing (so a zoomed-out map
//! does not shimmer). A thin, faster high layer (cirrus streaks) rides on top with parallax and casts a shadow
//! on the main deck. Light is a short Beer-Lambert march towards the sun through the density field, with a
//! powder term, a silver lining on thin sun-facing edges and moonlit rims at night. The Volumetric quality treats
//! density as a heightfield for soft self-shadowing between billows and adds light shafts (god rays) through
//! the gaps. The cloud edge is feathered by a signed distance to the explored ground, roughened by the noise,
//! and reaches a little over explored terrain as a veil.
//!
//! Looks: the overworld follows the time of day (sunset, night) and the weather (storm darkening, lightning
//! flashes and crawling bolts); the Nether is churning ash in red-orange and crimson lit from below by lava
//! with dark veins and drifting embers, no day or weather; the End is a violet haze with a slow shimmer and
//! faint sparkles.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Theme {
    Auto,
    Day,
    Sunset,
    Night,
    Storm,
}

impl Theme {
    pub const ALL: [Theme; 5] = [Theme::Auto, Theme::Day, Theme::Sunset, Theme::Night, Theme::Storm];
    pub fn name(self) -> &'static str {
        match self {
            Theme::Auto => "auto",
            Theme::Day => "day",
            Theme::Sunset => "sunset",
            Theme::Night => "night",
            Theme::Storm => "storm",
        }
    }
    pub fn index(self) -> u8 {
        Theme::ALL.iter().position(|&t| t == self).unwrap_or(0) as u8
    }
    pub fn from_index(i: u8) -> Theme {
        Theme::ALL[i as usize % Theme::ALL.len()]
    }
}

/// Which world's sky the clouds belong to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Look {
    Overworld,
    Nether,
    End,
}

impl Look {
    /// From a dimension id (`minecraft:the_nether`, `minecraft:the_end`, anything else is overworld-like).
    pub fn of_dimension(dim: &str) -> Look {
        let d = dim.to_ascii_lowercase();
        if d.contains("nether") {
            Look::Nether
        } else if d.contains("the_end") || d.ends_with(":end") || d == "end" {
            Look::End
        } else {
            Look::Overworld
        }
    }
}

/// What the player chose plus what the world says this frame.
#[derive(Clone, Copy, Debug)]
pub struct Params {
    /// 0..=1: how much of the unexplored area is under cloud (1 = all of it, lower leaves gaps).
    pub density: f32,
    /// Drift speed multiplier (1 = about 6 blocks a second at map scale).
    pub speed: f32,
    /// 0..=1: flat sheets to round, billowy heaps.
    pub puff: f32,
    /// 0 fast (8 px nodes), 1 normal (6 px), 2 fine (4 px), 3 volumetric (4 px + heightfield shadows, god rays).
    pub quality: u8,
    pub theme: Theme,
    /// Rain and thunder strength (0..=1 each), from the level; ignored when `weather` is off.
    pub rain: f32,
    pub thunder: f32,
    pub weather: bool,
    /// Minecraft day time in ticks (0..24000, 6000 noon).
    pub day_time: u32,
    pub look: Look,
    /// Strength of the Nether / End look (1 = normal).
    pub intensity: f32,
    /// 0 off, 1 flashes, 2 flashes and bolts.
    pub lightning: u8,
    /// Strike rate multiplier.
    pub lightning_rate: f32,
    pub god_rays: bool,
    /// The hard limit of how far the clouds reach in over explored ground, in world blocks (constant in the world
    /// at any zoom): no cloud or fill pixel lies further than this from the unexplored; the feather fits inside.
    pub reach: f32,
    /// The clouds also cover far (estimated, `view::FAR_ALPHA`) terrain, as up to 0.3.4. Off (0.3.5 default): far
    /// terrain shows through; the fog keeps to the ground with no terrain at all and comes back over the edge of
    /// the far coverage with a soft front (`LOD_FRONT_BLOCKS`).
    pub far_fog: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            density: 0.85,
            speed: 1.0,
            puff: 0.6,
            quality: 1,
            theme: Theme::Auto,
            rain: 0.0,
            thunder: 0.0,
            weather: true,
            day_time: 6000,
            look: Look::Overworld,
            intensity: 1.0,
            lightning: 2,
            lightning_rate: 1.0,
            god_rays: true,
            reach: REACH_DEFAULT,
            far_fog: false,
        }
    }
}

/// The colours one frame uses (0..255 floats).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub lit: [f32; 3],
    pub shade: [f32; 3],
    pub rim: [f32; 3],
}

fn rgbf(c: u32) -> [f32; 3] {
    [((c >> 16) & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, (c & 0xFF) as f32]
}

fn lerp3(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

fn scale3(a: [f32; 3], k: f32) -> [f32; 3] {
    [a[0] * k, a[1] * k, a[2] * k]
}

const DAY: [u32; 3] = [0xF6F9FD, 0x9AA8BF, 0xFFFFFF];
const SUNSET: [u32; 3] = [0xFFC896, 0x7A5E86, 0xFFE2B8];
const NIGHT: [u32; 3] = [0x4A5A86, 0x141B30, 0x9AAAD8];
const STORM: [u32; 3] = [0x8E95A2, 0x3A3F4B, 0xB6BCC6];
const NETHER: [u32; 3] = [0xC8482A, 0x3A0A10, 0xFF9A50];
const NETHER_GLOW: u32 = 0xFF6A18;
const END: [u32; 3] = [0xB490E6, 0x241634, 0xEED6FF];
const END_SHIMMER: u32 = 0x78D8E8;

fn fixed(p: [u32; 3]) -> Palette {
    Palette { lit: rgbf(p[0]), shade: rgbf(p[1]), rim: rgbf(p[2]) }
}

fn blend(a: Palette, b: Palette, t: f32) -> Palette {
    Palette { lit: lerp3(a.lit, b.lit, t), shade: lerp3(a.shade, b.shade, t), rim: lerp3(a.rim, b.rim, t) }
}

#[inline]
fn smooth(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Sun height for a day time: 1 at noon, -1 at midnight.
pub fn sun_height(day_time: u32) -> f32 {
    (((day_time % 24000) as f32 - 6000.0) / 24000.0 * std::f32::consts::TAU).cos()
}

/// Daylight (0 night..1 day) and twilight (1 at sunrise/sunset) for a day time.
pub fn daylight(day_time: u32) -> (f32, f32) {
    let s = sun_height(day_time);
    (smooth(-0.25, 0.3, s), (1.0 - s.abs() / 0.35).clamp(0.0, 1.0))
}

/// Storm strength 0..=1 this frame (rain and thunder, or the fixed storm theme).
fn storminess(p: &Params) -> f32 {
    if p.look != Look::Overworld {
        return 0.0;
    }
    let w = if p.weather { (p.rain * 0.55 + p.thunder * 0.45).clamp(0.0, 1.0) } else { 0.0 };
    if p.theme == Theme::Storm { w.max(0.6) } else { w }
}

fn thunder(p: &Params) -> f32 {
    if p.look != Look::Overworld {
        return 0.0;
    }
    let t = if p.weather { p.thunder } else { 0.0 };
    if p.theme == Theme::Storm { t.max(0.6) } else { t }
}

/// The tone the gaps between clouds show over unexplored ground: sky far below in the overworld, a glowing
/// dark crimson in the Nether, the void in the End.
pub fn sky(p: &Params) -> [f32; 3] {
    match p.look {
        Look::Nether => return rgbf(0x3A0C08),
        Look::End => return rgbf(0x140C22),
        Look::Overworld => {}
    }
    const DAY_SKY: u32 = 0x7896BE;
    const SUNSET_SKY: u32 = 0x8C6C8A;
    const NIGHT_SKY: u32 = 0x0E1528;
    const STORM_SKY: u32 = 0x343A45;
    let base = match p.theme {
        Theme::Day => rgbf(DAY_SKY),
        Theme::Sunset => rgbf(SUNSET_SKY),
        Theme::Night => rgbf(NIGHT_SKY),
        Theme::Storm => rgbf(STORM_SKY),
        Theme::Auto => {
            let (day, twilight) = daylight(p.day_time);
            lerp3(lerp3(rgbf(NIGHT_SKY), rgbf(DAY_SKY), day), rgbf(SUNSET_SKY), twilight * 0.7)
        }
    };
    let st = storminess(p);
    let (day, _) = daylight(p.day_time);
    lerp3(base, scale3(rgbf(STORM_SKY), 0.4 + 0.6 * day), (st * 0.9).min(0.85))
}

pub fn palette(p: &Params) -> Palette {
    match p.look {
        Look::Nether => return fixed(NETHER),
        Look::End => return fixed(END),
        Look::Overworld => {}
    }
    let base = match p.theme {
        Theme::Day => return fixed(DAY),
        Theme::Sunset => return fixed(SUNSET),
        Theme::Night => return fixed(NIGHT),
        Theme::Storm => return fixed(STORM),
        Theme::Auto => {
            let (day, twilight) = daylight(p.day_time);
            let pal = blend(fixed(NIGHT), fixed(DAY), day);
            blend(pal, fixed(SUNSET), twilight * 0.75)
        }
    };
    if !p.weather {
        return base;
    }
    let storm = (p.rain * 0.55 + p.thunder * 0.35).clamp(0.0, 0.85);
    let (day, _) = daylight(p.day_time);
    // A storm at night stays dark: blend towards a darkened storm tone.
    let mut st = fixed(STORM);
    let k = 0.35 + 0.65 * day;
    st.lit = scale3(st.lit, k);
    st.shade = scale3(st.shade, k);
    st.rim = scale3(st.rim, k);
    blend(base, st, storm)
}

// --- noise -----------------------------------------------------------------------------------------------------

#[inline]
fn hash3(x: i32, y: i32, z: i32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x8DA6_B343) ^ (y as u32).wrapping_mul(0xD816_3841) ^ (z as u32).wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x85EB_CA6B);
    h ^= h >> 16;
    h
}

#[inline]
fn unit(h: u32) -> f32 {
    (h & 0xFFFF) as f32 / 65535.0
}

/// `x.floor()` as an integer and as a float, without the libm call `floor` is on a baseline x86-64 build (the
/// noise and the grain take several per sample). The same value for every |x| below 2^31.
#[inline(always)]
fn ffloor(x: f32) -> (i32, f32) {
    let i = x as i32;
    let i = i - ((x < i as f32) as i32);
    (i, i as f32)
}

/// Smooth 3D value noise in 0..=1.
#[inline]
pub fn value3(x: f32, y: f32, z: f32) -> f32 {
    let ((ix, fx), (iy, fy), (iz, fz)) = (ffloor(x), ffloor(y), ffloor(z));
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (tx, ty, tz) = (s(x - fx), s(y - fy), s(z - fz));
    let v = |dx: i32, dy: i32, dz: i32| unit(hash3(ix + dx, iy + dy, iz + dz));
    let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
    let x00 = l(v(0, 0, 0), v(1, 0, 0), tx);
    let x10 = l(v(0, 1, 0), v(1, 1, 0), tx);
    let x01 = l(v(0, 0, 1), v(1, 0, 1), tx);
    let x11 = l(v(0, 1, 1), v(1, 1, 1), tx);
    l(l(x00, x10, ty), l(x01, x11, ty), tz)
}

/// Fractal sum of `octaves` value-noise layers, normalised to 0..=1.
#[inline]
pub fn fbm(x: f32, y: f32, z: f32, octaves: u32) -> f32 {
    fbm_aa(x, y, z, octaves, 0.0)
}

const LACUNARITY: f32 = 2.03;

/// `fbm` that fades every octave whose frequency (`nyq` = cycles per node of the first octave) passes about a
/// quarter of the node rate to its mean, so the field never holds detail the grid cannot show: no shimmer when
/// zoomed out, and the skipped octaves cost nothing.
#[inline]
pub fn fbm_aa(x: f32, y: f32, z: f32, octaves: u32, nyq: f32) -> f32 {
    let (mut sum, mut amp, mut norm, mut f) = (0.0, 0.5, 0.0, 1.0);
    for o in 0..octaves {
        let w = 1.0 - smooth(0.22, 0.45, nyq * f);
        if w > 0.0 {
            // Each octave shifted so their lattices do not line up.
            let k = o as f32 * 17.31;
            let v = value3(x * f + k, y * f - k, z * f + k * 0.5);
            sum += amp * (0.5 + (v - 0.5) * w);
        } else {
            sum += amp * 0.5;
        }
        norm += amp;
        amp *= 0.5;
        f *= LACUNARITY;
    }
    sum / norm
}

/// 2D worley (cellular) noise: 1 at feature points falling to 0 a cell away; round heaps.
#[inline]
pub fn worley(x: f32, y: f32, seed: i32) -> f32 {
    let (fx, fy) = (ffloor(x).0, ffloor(y).0);
    let mut best = 4.0f32;
    for dy in -1..=1 {
        for dx in -1..=1 {
            let h = hash3(fx + dx, fy + dy, seed);
            let px = (fx + dx) as f32 + unit(h);
            let py = (fy + dy) as f32 + unit(h >> 16 | h << 16);
            let d = (px - x) * (px - x) + (py - y) * (py - y);
            best = best.min(d);
        }
    }
    (1.0 - best.sqrt()).clamp(0.0, 1.0)
}

// --- threads ---------------------------------------------------------------------------------------------------

fn threads() -> usize {
    static N: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
    // Half the cores like the map render (the game draws on the others), but four where there are four.
    *N.get_or_init(|| {
        let n = std::thread::available_parallelism().map_or(1, |n| n.get());
        (n / 2).max(n.min(4)).clamp(1, 8)
    })
}

/// Runs `f(row index, row)` over `out` cut into rows of `row` items, the rows split over a few threads when
/// there are at least `min` items.
fn par_rows<T: Send>(out: &mut [T], row: usize, min: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = out.len().div_ceil(row.max(1));
    let n = threads().min(rows);
    if n <= 1 || out.len() < min {
        for (r, chunk) in out.chunks_mut(row.max(1)).enumerate() {
            f(r, chunk);
        }
        return;
    }
    let per = rows.div_ceil(n);
    std::thread::scope(|s| {
        for (k, band) in out.chunks_mut(per * row).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (j, chunk) in band.chunks_mut(row).enumerate() {
                    f(k * per + j, chunk);
                }
            });
        }
    });
}

// --- the cloud layer -------------------------------------------------------------------------------------------

/// Noise samples of one node.
#[derive(Clone, Copy, Default)]
struct Field {
    /// Main deck density 0..1.
    d: f32,
    /// High wisp layer density 0..1 (at its parallax position) and its value at the shadow offset.
    hi: f32,
    hi_shadow: f32,
    /// Look-specific: Nether vein ridge, End shimmer phase.
    aux: f32,
    /// Cloud deck reach 0..1: how much of the deck covers this node (1 over and near the unexplored, feathering
    /// out over explored ground with a noisy, billowing edge).
    edge: f32,
    /// Fill coverage 0..1: how much the opaque sky / fog tone between the clouds hides the map. Exactly 1 over and
    /// around every unexplored pixel (so the chunk staircase of the explored border never shows), smooth outside.
    cg: f32,
}

/// The noise of one node: depends only on the node's world position, the time and the look, so it is cached by
/// global lattice index and reused while the map pans (see `Clouds::update`).
#[derive(Clone, Copy, Default)]
struct Noise {
    d: f32,
    hi: f32,
    hi_shadow: f32,
    aux: f32,
    /// Low-frequency billow noise that roughens the edge.
    lo: f32,
    ok: bool,
}

impl Noise {
    fn mix(a: Noise, b: Noise, k: f32) -> Noise {
        let l = |x: f32, y: f32| x + (y - x) * k;
        Noise { d: l(a.d, b.d), hi: l(a.hi, b.hi), hi_shadow: l(a.hi_shadow, b.hi_shadow), aux: l(a.aux, b.aux), lo: l(a.lo, b.lo), ok: true }
    }
}

/// Everything the node noise depends on besides the world position.
#[derive(Clone, Copy)]
struct NoiseCfg {
    drift: f32,
    morph: f32,
    warp: f32,
    puff: f32,
    octaves: u32,
    node_blocks: f32,
    hi_on: bool,
    sun: (f32, f32),
    shadow_off: f32,
    look: Look,
    t: f32,
}

/// Node noise at world block (`wx`, `wz`) for one feature scale.
fn sample_noise(wx: f64, wz: f64, c: &NoiseCfg, scale: f32) -> Noise {
    let nyq = scale * c.node_blocks;
    let (drift, morph, warp, puff) = (c.drift, c.morph, c.warp, c.puff);
    let (x, z) = ((wx as f32 + drift) * scale, (wz as f32 + drift * 0.35) * scale);
    // Domain warp: two cheap low-octave fields bend the coordinates into curls and wisps.
    let qx = fbm_aa(x * 0.5 + 5.2, z * 0.5 + 1.3, morph * 0.7, 2, nyq * 0.5) - 0.5;
    let qz = fbm_aa(x * 0.5 - 3.1, z * 0.5 + 7.7, morph * 0.7, 2, nyq * 0.5) - 0.5;
    let (ux, uz) = (x + qx * warp * 2.0, z + qz * warp * 2.0);
    let base = fbm_aa(ux, uz, morph, c.octaves, nyq);
    let heap = if puff > 0.05 { worley(ux * 0.9 + morph * 0.6, uz * 0.9, 7) * 0.6 + worley(ux * 1.9, uz * 1.9 - morph, 11) * 0.4 } else { 0.5 };
    // fbm piles up around 0.5: stretch it so the density threshold has a real range to cut.
    let raw = base * (1.0 - puff * 0.5) + heap * puff * 0.5;
    let d = smooth(0.0, 1.0, ((raw - 0.5) * 2.2 + 0.5).clamp(0.0, 1.0));
    let (mut hi, mut hi_shadow) = (0.0, 0.0);
    if c.hi_on {
        // High layer: elongated streaks, faster and in another direction, world anchored like the deck (a
        // camera parallax made it slide over the map while dragging); its shadow falls away from the sun.
        let streak = |ox: f32, oz: f32| {
            let (hx, hz) = ((wx as f32 + ox + drift * 1.8) * scale * 0.7, (wz as f32 + oz - drift * 0.6) * scale * 0.7);
            // Stretched along x: long thin cirrus.
            let n = fbm_aa(hx * 0.45 + qz, hz * 2.4, morph * 1.3 + 9.0, 4, nyq * 1.2);
            smooth(0.56, 0.78, n)
        };
        hi = streak(0.0, 0.0);
        hi_shadow = streak(-c.sun.0 * c.shadow_off * 4.0, -c.sun.1 * c.shadow_off * 4.0);
    }
    let aux = match c.look {
        // Ridged noise: thin dark veins where it peaks.
        Look::Nether => {
            let r = fbm_aa(ux * 1.4 + 3.3, uz * 1.4, morph * 1.6, 3, nyq * 1.4);
            1.0 - (2.0 * r - 1.0).abs()
        }
        Look::End => fbm_aa(x * 0.8 + 11.0, z * 0.8, c.t * 0.12, 2, nyq * 0.8),
        Look::Overworld => 0.0,
    };
    // Billows at a few times the cloud scale: the fine detail stays in the deck, so the fill's own edge never
    // steps from node to node.
    let lo = fbm_aa(ux * 1.3 + 17.0, uz * 1.3 - 4.0, morph, 3, nyq * 1.3);
    Noise { d, hi, hi_shadow, aux, lo, ok: true }
}

/// Nodes of the cloud grid beyond the visible lattice on every side: the light march and the edge shape near
/// the image border see real neighbours, so a node looks the same wherever the pan puts it on screen.
const EXTRA: usize = 6;
/// Rings of the grid outside the image that are never shaded (see the shading pass).
const SKIP: usize = EXTRA - 2;

/// Shaded output of one node.
#[derive(Clone, Copy, Default)]
struct Node {
    a: f32,
    /// 0 shade..1 lit.
    lum: f32,
    rim: f32,
    /// Additive light (lightning, lava glow, god rays, shimmer), 0..255 per channel.
    add: [f32; 3],
    /// High-layer wisp opacity, drawn over the deck in the lit colour.
    wisp: f32,
    /// Fill coverage (see `Field::cg`).
    cg: f32,
}

/// A bright dot (ember, sparkle or a bolt pixel) drawn after the composite: image x, y, colour, strength.
#[derive(Clone, Copy, Debug)]
struct Dot {
    x: f32,
    y: f32,
    c: [f32; 3],
    k: f32,
    big: bool,
}

/// Node grid state, reused frame to frame.
#[derive(Default)]
pub struct Clouds {
    cell: usize,
    nw: usize,
    nh: usize,
    /// Distance in nodes to the nearest node whose cell holds an unexplored pixel (0 on those).
    dout: Vec<f32>,
    /// The same, blurred: the smooth field the feathered edge follows (no chunk staircase in its contours).
    dsm: Vec<f32>,
    /// Per node: 1 = some unexplored pixel in its cell, 2 = some explored pixel, 3 = both; 4 = seen in the image;
    /// 8 = some pixel with no terrain at all (unexplored and not far terrain, or any unexplored with `far_fog`).
    flags: Vec<u8>,
    /// Distance in nodes to the nearest node with a no-terrain pixel (flag 8), and its blurred copy: the far
    /// terrain's own mask, kept apart from the explored one (`dout`). Only filled when far terrain is in view.
    dvoid: Vec<f32>,
    dvsm: Vec<f32>,
    scratch: Vec<f32>,
    field: Vec<Field>,
    node: Vec<Node>,
    /// Per-node final colour (a, premultiplied r, g, b, add r, g, b, fill), scratch for `composite`: `coarse` on
    /// the node grid, `col` refined to half the cell (`rnw` wide, `rcell` pixels apart).
    coarse: Vec<[f32; 8]>,
    col: Vec<[f32; 8]>,
    tmp8: Vec<[f32; 8]>,
    /// Composite scratch: first pixel column past each refined cell column, and which refined cells are empty.
    cxe: Vec<usize>,
    cemp: Vec<bool>,
    rnw: usize,
    rcell: usize,
    dots: Vec<Dot>,
    /// Current lightning strike: global lattice node position, start time, seed.
    strike: Option<([f64; 2], f32, u32)>,
    /// World-anchored lattice: node (i, j) of the grid is global node `g0 + (i, j)` and sits at image pixel
    /// `(i * cell - off[0], j * cell - off[1])` (sub-pixel). Panning moves `g0` and `off`, never the nodes.
    g0: [i64; 2],
    off: [f64; 2],
    /// The distance domain (flags, dout, dsm): the grid plus `dm` nodes on every side, `dw` x `dh`.
    dw: usize,
    dh: usize,
    dm: usize,
    /// Flags of the last frame's domain (bit 4 = really seen), its global origin, size and lattice key: nodes
    /// that pan off screen keep their explored state for the distance field instead of a guess.
    seen: Vec<u8>,
    seen_g0: [i64; 2],
    seen_dw: usize,
    seen_dh: usize,
    seen_key: Option<(usize, u64, u64)>,
    /// Node noise of this and the last frame, the last one's origin, width and key (lattice + time + look).
    noise: Vec<Noise>,
    noise_prev: Vec<Noise>,
    noise_g0: [i64; 2],
    noise_w: usize,
    noise_key: Option<[u64; 13]>,
    colx: Vec<u32>,
    /// Per image pixel: the hard reach factor (255 on the unexplored, 0 past the reach), and its scratch.
    kmask: Vec<u8>,
    /// The same against the void for far terrain pixels, and the fading "void was here" of every domain node
    /// (1 under void, falling to 0 over `VOID_FADE_S` once terrain arrives), with last frame's for the pan.
    kfar: Vec<u8>,
    voidv: Vec<f32>,
    void_prev: Vec<f32>,
    void_g0: [i64; 2],
    void_dw: usize,
    void_dh: usize,
    void_key: Option<(usize, u64, u64)>,
    void_at: Option<std::time::Instant>,
    easing: bool,
    mw: usize,
    khd: Vec<Vec<u16>>,
    rowy: Vec<u32>,
    /// Nodes whose noise came from the cache in the last update (for tests and the bench).
    pub reused: usize,
    /// Nodes whose noise was computed in the last update.
    pub computed: usize,
    /// Milliseconds of the last update by stage: explored-flag scan, flags outside the image, distance + blur,
    /// noise, field, shading, lightning and motes.
    pub prof: [f32; 7],
    pub last_nodes: usize,
    /// Wind phase in blocks, integrated frame by frame (`phase += speed * dt`) from a smoothed, bounded speed:
    /// a weather change ramps the speed, it never rescales the whole elapsed time (which made the clouds race).
    pub phase: f32,
    /// The smoothed wind speed (blocks per second) and the time of the last update.
    pub wind: f32,
    last_t: Option<f32>,
    /// The detail tile for the per-pixel cloud grain (see `detail_tile`).
    tile: Vec<f32>,
    /// Image pixel -> tile texel affine map `[u0, du/dx, du/dy, v0, dv/dx, dv/dy]` (world anchored, drifting
    /// with the wind) and the grain strength of this frame (0 = off).
    daff: [f32; 6],
    grain: f32,
}

/// Side of the square, tileable detail texture (texels).
pub const TILE: usize = 128;
/// World blocks per detail texel, per map block per pixel: the grain sits at about 1.7 image pixels.
const TEXEL_PX: f32 = 1.7;

/// Amplitude ratio between neighbouring octaves of the detail tile.
const DETAIL_FALLOFF: f32 = 0.7;

/// A tileable 4-octave value-noise fbm in 0..1 (wraps at `TILE`), the cloud grain: crisp billow edges and fine
/// light/shade inside the deck that the node grid (4 to 8 px) cannot carry.
pub fn detail_tile() -> Vec<f32> {
    let mut t = vec![0.0f32; TILE * TILE];
    let mut amp = 0.5;
    // 0.3.4: a fourth octave at 2 texels (about 3.4 px) and a slower falloff, so the finest band carries the crisp
    // rims the references have (tools/cloud_detail.py: fine-band energy 0.14x the references at q3 in 0.3.3).
    for (o, period) in [8usize, 16, 32, 64].into_iter().enumerate() {
        let g = |x: usize, y: usize| unit(hash3((x % period) as i32, (y % period) as i32, 977 + o as i32));
        let step = TILE as f32 / period as f32;
        for y in 0..TILE {
            for x in 0..TILE {
                let (fx, fy) = (x as f32 / step, y as f32 / step);
                let (x0, y0) = (fx as usize, fy as usize);
                let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
                let (sx, sy) = (tx * tx * (3.0 - 2.0 * tx), ty * ty * (3.0 - 2.0 * ty));
                let a = g(x0, y0) + (g(x0 + 1, y0) - g(x0, y0)) * sx;
                let b = g(x0, y0 + 1) + (g(x0 + 1, y0 + 1) - g(x0, y0 + 1)) * sx;
                t[y * TILE + x] += (a + (b - a) * sy) * amp;
            }
        }
        amp *= DETAIL_FALLOFF;
    }
    // Stretch to use the whole 0..1 range around 0.5.
    let mean = t.iter().sum::<f32>() / t.len() as f32;
    let dev = (t.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / t.len() as f32).sqrt().max(1e-4);
    t.iter_mut().for_each(|v| *v = (0.5 + (*v - mean) / dev * 0.18).clamp(0.0, 1.0));
    t
}

/// Bilinear, wrapping sample of the detail tile (u, v in texels).
#[inline]
fn tile_at(t: &[f32], u: f32, v: f32) -> f32 {
    let ((iu, fu), (iv, fv)) = (ffloor(u), ffloor(v));
    // TILE is a power of two: the mask is the Euclidean remainder.
    let (x0, y0) = ((iu & (TILE as i32 - 1)) as usize, (iv & (TILE as i32 - 1)) as usize);
    let (x1, y1) = ((x0 + 1) & (TILE - 1), (y0 + 1) & (TILE - 1));
    let (tx, ty) = (u - fu, v - fv);
    let a = t[y0 * TILE + x0] + (t[y0 * TILE + x1] - t[y0 * TILE + x0]) * tx;
    let b = t[y1 * TILE + x0] + (t[y1 * TILE + x1] - t[y1 * TILE + x0]) * tx;
    a + (b - a) * ty
}

/// Longest time step the wind integrates at once: a hitch or a paused game does not jump the clouds.
pub const MAX_DT: f32 = 0.1;

/// How far the cloud deck reaches over explored ground, in feather widths (the fill stops near 0.85).
pub const EDGE_OUT: f32 = 1.4;

/// The largest feather width in image pixels: 22 % of the longer side, less on a small image (`m * m / 1500`: 9 px
/// on the 116 px settings preview, 44 px on a 256 px minimap, full share from 512 px up).
pub fn reach_cap_px(w: usize, h: usize) -> f32 {
    let m = w.max(h) as f32;
    (0.22 * m).min(m * m / 1500.0)
}

/// Width of the soft front where the fog comes back at the edge of the far (LOD) terrain, world blocks, and its
/// floor in image pixels (so a zoomed-out map still fades over several pixels instead of a chunk-shaped step).
pub const LOD_FRONT_BLOCKS: f32 = 16.0;
pub const LOD_FRONT_MIN_PX: f32 = 4.0;

/// The far-terrain front width in image pixels at `bpp`: `LOD_FRONT_BLOCKS`, at least `LOD_FRONT_MIN_PX`, within
/// `reach_cap_px`.
pub fn lod_front_px(bpp: f64, w: usize, h: usize) -> f32 {
    (LOD_FRONT_BLOCKS / bpp.max(1e-6) as f32).max(LOD_FRONT_MIN_PX).min(reach_cap_px(w, h).max(LOD_FRONT_MIN_PX))
}

/// Seconds the fog takes to thin out over far terrain that has just streamed in.
pub const VOID_FADE_S: f32 = 0.8;

/// Default reach of the clouds over explored ground, blocks.
pub const REACH_DEFAULT: f32 = 10.0;
/// Largest reach the settings offer, blocks.
pub const REACH_MAX: u32 = 64;

/// The hard reach limit in image pixels: `reach` blocks at `bpp` blocks a pixel, never more than `reach_cap_px`
/// (which only ever lowers it, on a deep zoom).
pub fn reach_px(reach: f32, bpp: f64, w: usize, h: usize) -> f32 {
    (reach.max(0.0) / bpp.max(1e-6) as f32).min(reach_cap_px(w, h))
}

/// The narrowest soft fade at the hard limit, image pixels: below it the fill ends in a one- or two-pixel step
/// that traces the chunk staircase of the explored border (10 blocks at 4 blocks a pixel gave 1.25 px).
pub const FEATHER_MIN_PX: f32 = 3.5;

/// Width (px) of the soft fade at the hard limit: the outer half of the reach (a narrower one makes a visible
/// step in the fill, more than 0.12 per pixel at a 20 px reach), at least `FEATHER_MIN_PX`, but never wider than
/// the reach itself: the limit in blocks stays hard (a reach narrower than the minimum fades over all of it).
pub fn reach_fade_px(rpx: f32) -> f32 {
    (0.5 * rpx).max(FEATHER_MIN_PX.min(rpx)).max(1.0)
}

/// Per-pixel reach factor (0..255) of the hard limit: 255 on the unexplored, falling to exactly 0 at `rpx` pixels
/// (Euclidean) from the nearest unexplored pixel, with a smooth fade over the last `reach_fade_px`. `src_margin`
/// tells whether a pixel outside the image (`x`, `y` may be negative) is unexplored; margin `m` pixels.
/// Two passes: horizontal distances per (extended) row, then a vertical min over +-reach rows, both contiguous.
/// One thread scope: each band of output rows computes its own horizontal rows plus an `m`-row halo, so no
/// second spawn and no barrier between the passes (the spawns were most of the cost at a small reach).
#[allow(clippy::too_many_arguments)]
fn reach_mask(out: &mut Vec<u8>, pool: &mut Vec<Vec<u16>>, img: &[u8], w: usize, h: usize, m: usize, rpx: f32, val: u8, eq: bool, src_margin: impl Fn(isize, isize) -> bool + Sync) {
    // A source pixel: alpha `val` (`eq`) or any other alpha (`!eq`).
    let is = move |a: u8| (a == val) == eq;
    let ew = w + 2 * m;
    let cap = (m + 1).min(255) as u16;
    let far = cap * cap;
    let fade = reach_fade_px(rpx);
    let r2 = rpx * rpx;
    let rr = (rpx.floor() as usize).min(m);
    out.resize(w * h, 0);
    // Horizontal squared distance of extended row `yy` (image row yy - m) into `row`; false when nothing is
    // within the cap.
    let hrow = |yy: usize, row: &mut [u16]| -> bool {
        let y = yy as isize - m as isize;
        let inside = y >= 0 && (y as usize) < h;
        let src = |x: usize| -> bool {
            let xi = x as isize - m as isize;
            if inside && xi >= 0 && (xi as usize) < w {
                is(img[(y as usize * w + xi as usize) * 4 + 3])
            } else {
                src_margin(xi, y)
            }
        };
        let mut run = cap;
        for x in 0..m {
            run = if src(x) { 0 } else { (run + 1).min(cap) };
        }
        let mut any = run < cap;
        if inside {
            // Run-skipping sweeps: a long explored stretch is filled at the cap and a long unexplored one at 0
            // in one go (the search loops vectorize), only the cells within the cap of a border walk one by one.
            // Values are kept squared (at most 255^2), so the vertical pass is a saturating add and a min; min of
            // squares is the square of the min, so the right sweep compares squares.
            let px = &img[y as usize * w * 4..(y as usize + 1) * w * 4];
            let alpha0 = |x: usize| is(px[x * 4 + 3]);
            let mut x = 0;
            while x < w {
                if alpha0(x) {
                    let k = px[x * 4..].chunks_exact(4).position(|p| !is(p[3])).unwrap_or(w - x);
                    row[x..x + k].fill(0);
                    x += k;
                    run = 0;
                    any = true;
                } else if run >= cap {
                    let k = px[x * 4..].chunks_exact(4).position(|p| is(p[3])).unwrap_or(w - x);
                    row[x..x + k].fill(far);
                    x += k;
                } else {
                    run += 1;
                    row[x] = run * run;
                    x += 1;
                }
            }
            run = cap;
            for x in (m + w..ew).rev() {
                run = if src(x) { 0 } else { (run + 1).min(cap) };
            }
            any |= run < cap;
            let mut x = w;
            while x > 0 {
                if run >= cap {
                    match row[..x].iter().rposition(|&v| v == 0) {
                        Some(k) => {
                            x = k;
                            run = 0;
                        }
                        None => break,
                    }
                } else {
                    x -= 1;
                    run = if row[x] == 0 { 0 } else { run + 1 };
                    row[x] = row[x].min(run * run);
                }
            }
            return any;
        }
        for (x, o) in row.iter_mut().enumerate() {
            run = if src(x + m) { 0 } else { (run + 1).min(cap) };
            *o = run;
        }
        run = cap;
        for x in (m + w..ew).rev() {
            run = if src(x) { 0 } else { (run + 1).min(cap) };
        }
        for o in row.iter_mut().rev() {
            run = if *o == 0 { 0 } else { (run + 1).min(cap) };
            let d = (*o).min(run);
            any |= d < cap;
            *o = d * d;
        }
        any
    };
    let band = |y0: usize, rows: &mut [u8], hd: &mut Vec<u16>| {
        let n = rows.len() / w;
        // Extended rows y0 + m - rr .. y0 + n + m + rr.
        let (e0, en) = (y0 + m - rr, n + 2 * rr);
        hd.resize(en * w, far);
        let any: Vec<bool> = hd.chunks_exact_mut(w).enumerate().map(|(k, row)| hrow(e0 + k, row)).collect();
        let mut best = vec![u16::MAX; w];
        for (j, orow) in rows.chunks_exact_mut(w).enumerate() {
            if !any[j..=j + 2 * rr].iter().any(|&a| a) {
                orow.fill(0);
                continue;
            }
            best.fill(u16::MAX);
            for (k, &a) in any[j..=j + 2 * rr].iter().enumerate() {
                if !a {
                    continue;
                }
                let dy = k as isize - rr as isize;
                // Squared pixel distances in u16, saturating: past 255 px is beyond any reach the cap allows
                // (`reach_cap_px`), so a saturated sum reads as "far".
                let d2 = (dy * dy) as u16;
                for (b, &v) in best.iter_mut().zip(&hd[(j + k) * w..(j + k + 1) * w]) {
                    *b = (*b).min(d2.saturating_add(v));
                }
            }
            for (o, &b) in orow.iter_mut().zip(&best) {
                *o = if b == 0 {
                    255
                } else if b as f32 >= r2 {
                    0
                } else {
                    let k = ((rpx - (b as f32).sqrt()) / fade).clamp(0.0, 1.0);
                    (k * k * (3.0 - 2.0 * k) * 255.0).round() as u8
                };
            }
        }
    };
    let nt = if w * h < 16 * 1024 { 1 } else { threads().min(h) };
    // Kept per-band buffers: a fresh few hundred KB a frame would page-fault every time.
    if pool.len() < nt {
        pool.resize_with(nt, Vec::new);
    }
    if nt <= 1 {
        band(0, &mut out[..], &mut pool[0]);
        return;
    }
    let per = h.div_ceil(nt);
    let band = &band;
    std::thread::scope(|s| {
        let mut it = out.chunks_mut(per * w).enumerate().zip(pool.iter_mut());
        // The calling thread takes the first band itself: one spawn fewer.
        let first = it.next();
        for ((k, rows), hd) in it {
            s.spawn(move || band(k * per, rows, hd));
        }
        if let Some(((_, rows), hd)) = first {
            band(0, rows, hd);
        }
    });
}
/// Extra wind share in a full storm.
pub const STORM_WIND: f32 = 0.4;
/// Time constant (seconds) of the wind speed smoothing.
pub const WIND_TAU: f32 = 5.0;

/// The wind speed the clouds head for (blocks per second): the setting, faster in a storm.
pub fn wind_target(p: &Params) -> f32 {
    let storm = storminess(p);
    6.0 * match p.look {
        // A storm adds 40 %, eased over WIND_TAU: the old doubling read as the clouds racing off after
        // `/weather thunder`.
        Look::Overworld => p.speed * (1.0 + STORM_WIND * storm),
        Look::Nether => p.speed * 0.45,
        Look::End => p.speed * 0.3,
    }
}

pub fn cell_for(quality: u8) -> usize {
    match quality {
        0 => 8,
        1 => 6,
        _ => 4,
    }
}

/// Chamfer distance (in nodes) from every node to the nearest node where `seed` is true; capped at `cap`.
fn chamfer(out: &mut [f32], nw: usize, nh: usize, seed: impl Fn(usize) -> bool, cap: f32) {
    const D: f32 = std::f32::consts::SQRT_2;
    for (i, o) in out.iter_mut().enumerate() {
        *o = if seed(i) { 0.0 } else { cap };
    }
    // Each pass: first the terms from the previous row for the whole row (branch-free, vectorised), then the
    // serial run along the row. `min` is exact, so the result is the same as the textbook per-node order.
    let from_row = |out: &mut [f32], y: usize, p: usize| {
        let (a, b) = if p < y { out.split_at_mut(y * nw) } else { let (b, a) = out.split_at_mut(p * nw); (a, b) };
        let (prev, cur) = if p < y { (&a[p * nw..(p + 1) * nw], &mut b[..nw]) } else { (&a[..nw], &mut b[y * nw..(y + 1) * nw]) };
        cur[0] = cur[0].min(prev[0] + 1.0);
        if nw > 1 {
            cur[0] = cur[0].min(prev[1] + D);
            cur[nw - 1] = cur[nw - 1].min(prev[nw - 1] + 1.0).min(prev[nw - 2] + D);
        }
        for x in 1..nw.saturating_sub(1) {
            cur[x] = cur[x].min(prev[x] + 1.0).min(prev[x - 1] + D).min(prev[x + 1] + D);
        }
    };
    for y in 0..nh {
        if y > 0 {
            from_row(out, y, y - 1);
        }
        let row = &mut out[y * nw..(y + 1) * nw];
        for x in 1..nw {
            row[x] = row[x].min(row[x - 1] + 1.0);
        }
    }
    for y in (0..nh).rev() {
        if y + 1 < nh {
            from_row(out, y, y + 1);
        }
        let row = &mut out[y * nw..(y + 1) * nw];
        for x in (0..nw.saturating_sub(1)).rev() {
            row[x] = row[x].min(row[x + 1] + 1.0);
        }
    }
}

/// Doubles the resolution of an `nw` x `nh` grid of 8-vectors spaced `cell` pixels: new nodes halfway use the
/// 4-tap (-1, 9, 9, -1) / 16 interpolator, old nodes stay. Alpha and fill are clamped to 0..1, colours to >= 0.
/// Returns the new size and spacing. `cell` must be even.
fn refine(src: &[[f32; 8]], nw: usize, nh: usize, cell: usize, out: &mut Vec<[f32; 8]>, tmp: &mut Vec<[f32; 8]>) -> (usize, usize, usize) {
    let (rw, rh) = (2 * nw - 1, 2 * nh - 1);
    // Every element of both is written below: no zero fill when the size holds (3.6 MB a frame at full size).
    if tmp.len() != rw * nh {
        tmp.clear();
        tmp.resize(rw * nh, [0.0; 8]);
    }
    if out.len() != rw * rh {
        out.clear();
        out.resize(rw * rh, [0.0; 8]);
    }
    let tap = |a: &[f32; 8], b: &[f32; 8], c: &[f32; 8], d: &[f32; 8]| {
        let mut o = [0.0f32; 8];
        for k in 0..8 {
            o[k] = (9.0 * (b[k] + c[k]) - a[k] - d[k]) * (1.0 / 16.0);
        }
        o[0] = o[0].clamp(0.0, 1.0);
        o[7] = o[7].clamp(0.0, 1.0);
        for k in 1..7 {
            o[k] = o[k].max(0.0);
        }
        o
    };
    // Both passes by output row, the rows over a few threads on a big grid (each row reads only its sources).
    par_rows(&mut tmp[..], rw, 16 * 1024, |y, trow| {
        let row = &src[y * nw..(y + 1) * nw];
        let at = |x: isize| &row[x.clamp(0, nw as isize - 1) as usize];
        for x in 0..nw {
            trow[2 * x] = row[x];
            if x + 1 < nw {
                let xi = x as isize;
                trow[2 * x + 1] = tap(at(xi - 1), at(xi), at(xi + 1), at(xi + 2));
            }
        }
    });
    let tmp: &[[f32; 8]] = tmp;
    par_rows(&mut out[..], rw, 16 * 1024, |oy, orow| {
        let y = oy / 2;
        let trow = |y: isize| &tmp[y.clamp(0, nh as isize - 1) as usize * rw..][..rw];
        if oy % 2 == 0 {
            orow.copy_from_slice(trow(y as isize));
        } else {
            let yi = y as isize;
            let (a, b, c, d) = (trow(yi - 1), trow(yi), trow(yi + 1), trow(yi + 2));
            for x in 0..rw {
                orow[x] = tap(&a[x], &b[x], &c[x], &d[x]);
            }
        }
    });
    (rw, rh, cell / 2)
}

/// Separable box blur of radius `r` (in nodes) over an `nw` x `nh` grid, edges clamped. Running sums: O(n).
fn box_blur(v: &mut [f32], tmp: &mut [f32], nw: usize, nh: usize, r: usize) {
    let k = 1.0 / (2 * r + 1) as f32;
    // Rows on this thread: the pass is memory-bound and short, threads measured slower at every map size.
    {
        let v: &[f32] = v;
        par_rows(&mut tmp[..nw * nh], nw, usize::MAX, |y, out| {
            let row = &v[y * nw..(y + 1) * nw];
            let at = |x: isize| row[x.clamp(0, nw as isize - 1) as usize];
            let mut acc: f32 = (-(r as isize)..=r as isize).map(at).sum();
            // Clamped reads only near the ends.
            let (lo, hi) = (r.min(nw), nw.saturating_sub(r + 1).max(r.min(nw)));
            for x in 0..lo {
                out[x] = acc * k;
                acc += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
            }
            for x in lo..hi {
                out[x] = acc * k;
                acc += row[x + r + 1] - row[x - r];
            }
            for x in hi..nw {
                out[x] = acc * k;
                acc += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
            }
        });
    }
    // Columns: one running sum per column, advanced a whole row at a time (contiguous, vectorised) instead of
    // walking each column with a stride of a row.
    let row = |y: isize| &tmp[y.clamp(0, nh as isize - 1) as usize * nw..][..nw];
    let mut acc = vec![0f32; nw];
    for d in -(r as isize)..=r as isize {
        for (a, b) in acc.iter_mut().zip(row(d)) {
            *a += *b;
        }
    }
    for y in 0..nh {
        let (add, sub) = (row(y as isize + r as isize + 1), row(y as isize - r as isize));
        let out = &mut v[y * nw..(y + 1) * nw];
        for x in 0..nw {
            out[x] = acc[x] * k;
            acc[x] += add[x] - sub[x];
        }
    }
}

impl Clouds {
    pub fn new() -> Self {
        Self::default()
    }

    /// Node grid and distance domain sizes of the last update (for benches).
    pub fn dims(&self) -> [usize; 4] {
        [self.nw, self.nh, self.dw, self.dh]
    }

    fn resize(&mut self, w: usize, h: usize, cell: usize, dm: usize) {
        let (nw, nh) = (w / cell + 2 * EXTRA + 3, h / cell + 2 * EXTRA + 3);
        if (nw, nh, cell) != (self.nw, self.nh, self.cell) {
            self.cell = cell;
            self.nw = nw;
            self.nh = nh;
            let n = nw * nh;
            self.field.clear();
            self.field.resize(n, Field::default());
            self.node.clear();
            self.node.resize(n, Node::default());
        }
        let (dw, dh) = (nw + 2 * dm, nh + 2 * dm);
        if (dw, dh) != (self.dw, self.dh) || self.dout.len() != dw * dh {
            self.dw = dw;
            self.dh = dh;
            let n = dw * dh;
            for v in [&mut self.dout, &mut self.dsm, &mut self.scratch] {
                v.clear();
                v.resize(n, 0.0);
            }
            self.flags.clear();
            self.flags.resize(n, 0);
        }
        self.dm = dm;
    }

    /// Whether fog is still thinning out over far terrain that has just arrived: the caller keeps the cloud frames
    /// coming until it is gone.
    pub fn easing(&self) -> bool {
        self.easing
    }

    /// Forgets the off-screen explored state kept for the distance field (a layer or world switch).
    pub fn forget(&mut self) {
        self.void_key = None;
        self.seen_key = None;
        self.noise_key = None;
    }

    /// Global lattice origin and sub-pixel offset of the last update (for tests).
    pub fn lattice(&self) -> ([i64; 2], [f64; 2], usize) {
        (self.g0, self.off, self.cell)
    }

    /// Recomputes the nodes for an image of `w` x `h` RGBA pixels in `img` (alpha < 255 = unexplored), viewed through
    /// `view` at time `t` seconds. Returns false (nothing to draw) when the image has no unexplored pixel.
    ///
    /// The node lattice is anchored to the world: a node is a fixed world point for a given zoom and rotation, and
    /// a pan only moves the lattice origin and a sub-pixel offset. The explored flags, the distance field and the
    /// noise are therefore the same for the same ground wherever it is on screen; the noise is reused from the last
    /// frame for every node still in view at the same time step, so a drag computes only the newly revealed edge.
    pub fn update(&mut self, img: &[u8], w: usize, h: usize, view: &super::view::View, t: f32, p: &Params) -> bool {
        let cell = cell_for(p.quality);
        // Reach: `p.reach` blocks in the world, so the same at every zoom (in pixels it shrinks as the map zooms
        // out), capped on a deep zoom so the clouds do not drown the screen.
        let rpx = reach_px(p.reach, view.bpp, w, h);
        // The feather fits inside the hard limit: the deck's soft overlap (EDGE_OUT feather widths past the first
        // node ring) ends near `rpx`, and the per-pixel reach mask cuts whatever the node grid lets through.
        let feather = (rpx / (EDGE_OUT * cell as f32)).max(0.5);
        let cap = feather * EDGE_OUT + 6.0;
        let r = ((feather / 2.5).round() as usize).clamp(1, 24);
        // The far-terrain front, in nodes: wider than the explored feather, fixed in world blocks.
        let lfeather = (lod_front_px(view.bpp, w, h) / (EDGE_OUT * cell as f32)).max(feather);
        let lcap = lfeather * EDGE_OUT + 6.0;
        let lr = ((lfeather / 2.5).round() as usize).clamp(1, 24);
        // The distance domain reaches past the grid as far as an unexplored node still shapes the edge (capped).
        let dm = ((cap + 3.0 * r as f32).ceil() as usize).max(if p.far_fog { 0 } else { (lcap + 3.0 * lr as f32).ceil() as usize }).min(40);
        self.resize(w, h, cell, dm);
        let (nw, nh, dw, dh) = (self.nw, self.nh, self.dw, self.dh);
        self.dots.clear();
        self.last_nodes = 0;
        self.reused = 0;
        self.computed = 0;
        let mut clock = std::time::Instant::now();
        let mut lap = |k: usize, prof: &mut [f32; 7]| {
            let n = std::time::Instant::now();
            prof[k] = (n - clock).as_secs_f32() * 1e3;
            clock = n;
        };

        // The lattice: image pixel (px, py) is lattice position (ua + px, va + py) / cell, where (ua, va) is the
        // top-left corner rotated into the view and divided by the zoom.
        let bpp = view.bpp.max(1e-9);
        let (rs, rc) = view.rotation.sin_cos();
        let ua = (view.center[0] * rc + view.center[1] * rs) / bpp - w as f64 / 2.0;
        let va = (-view.center[0] * rs + view.center[1] * rc) / bpp - h as f64 / 2.0;
        let cf = cell as f64;
        let (fx0, fy0) = ((ua / cf).floor(), (va / cf).floor());
        self.g0 = [fx0 as i64 - EXTRA as i64, fy0 as i64 - EXTRA as i64];
        self.off = [ua - fx0 * cf + (EXTRA * cell) as f64, va - fy0 * cf + (EXTRA * cell) as f64];
        let (g0, off) = (self.g0, self.off);
        let lkey = (cell | (p.far_fog as usize) << 16, bpp.to_bits(), view.rotation.to_bits());
        // World block of grid node (i, j), straight from its global index: bit-identical for every pan.
        let node_world = move |i: usize, j: usize| -> [f64; 2] {
            let (u, v) = ((g0[0] + i as i64) as f64 * cf * bpp, (g0[1] + j as i64) as f64 * cf * bpp);
            [u * rc - v * rs, u * rs + v * rc]
        };

        // Which node cells hold unexplored / explored pixels: every pixel counts (a sparse sample would miss
        // thin unexplored strips and leave them uncovered). Pixel x belongs to the node nearest to it.
        self.colx.clear();
        self.colx.extend((0..w).map(|x| (((x as f64 + off[0]) / cf + 0.5).floor() as usize + dm) as u32));
        self.rowy.clear();
        self.rowy.extend((0..h).map(|y| (((y as f64 + off[1]) / cf + 0.5).floor() as usize + dm) as u32));
        self.flags.iter_mut().for_each(|f| *f = 0);
        {
            // By node row in parallel: the image rows of node row j are a run (rowy is monotonic).
            let (colx, rowy) = (&self.colx, &self.rowy);
            // Unknown pixel: 5, plus 8 when it has no terrain (far terrain only counts there with `far_fog`).
            let lut: [u8; 256] = std::array::from_fn(|a| match a {
                255 => 6,
                a if a == super::view::FAR_ALPHA as usize && !p.far_fog => 21,
                _ => 13,
            });
            let (j0, j1) = (rowy[0] as usize, rowy[h - 1] as usize);
            par_rows(&mut self.flags[j0 * dw..(j1 + 1) * dw], dw, 8192, |jj, out| {
                let j = (j0 + jj) as u32;
                let y0 = rowy.partition_point(|&r| r < j);
                let y1 = rowy.partition_point(|&r| r <= j);
                for y in y0..y1 {
                    let row = &img[y * w * 4..(y + 1) * w * 4];
                    for x in 0..w {
                        out[colx[x] as usize] |= lut[row[x * 4 + 3] as usize];
                    }
                }
            });
        }
        let any = self.flags.iter().any(|f| f & 1 != 0);
        lap(0, &mut self.prof);
        if !any {
            return false;
        }
        // Nodes outside the image: what an earlier frame saw there (same lattice), else a copy of the nearest
        // node in the image, so the cloud runs off the border instead of feathering out along it.
        let (rx0, rx1) = (self.colx[0] as usize, self.colx[w - 1] as usize);
        let (ry0, ry1) = (self.rowy[0] as usize, self.rowy[h - 1] as usize);
        let seen_ok = self.seen_key == Some(lkey) && self.seen.len() == self.seen_dw * self.seen_dh;
        let dg = [g0[0] - dm as i64, g0[1] - dm as i64];
        for j in 0..dh {
            for i in 0..dw {
                if (rx0..=rx1).contains(&i) && (ry0..=ry1).contains(&j) {
                    continue;
                }
                let mut v = 0u8;
                if seen_ok {
                    let (pi, pj) = (dg[0] + i as i64 - self.seen_g0[0], dg[1] + j as i64 - self.seen_g0[1]);
                    if pi >= 0 && pj >= 0 && (pi as usize) < self.seen_dw && (pj as usize) < self.seen_dh {
                        let s = self.seen[pj as usize * self.seen_dw + pi as usize];
                        if s & 4 != 0 {
                            v = s;
                        }
                    }
                }
                if v == 0 {
                    v = self.flags[j.clamp(ry0, ry1) * dw + i.clamp(rx0, rx1)] & 11;
                }
                self.flags[j * dw + i] = v;
            }
        }
        self.seen.clear();
        self.seen.extend_from_slice(&self.flags);
        self.seen_g0 = dg;
        self.seen_dw = dw;
        self.seen_dh = dh;
        self.seen_key = Some(lkey);
        // The hard reach limit per pixel. Outside the image a pixel counts as unexplored only when its whole node
        // is (flags 1): a guess never lets the clouds further in than the limit.
        {
            let m = (rpx.ceil() as usize + 1).min(256);
            let (flags, cf) = (&self.flags, cf);
            let node = move |p: isize, o: f64, n: usize| -> Option<usize> {
                let k = ((p as f64 + o) / cf + 0.5).floor() as i64 + dm as i64;
                (k >= 0 && (k as usize) < n).then_some(k as usize)
            };
            let margin = |x: isize, y: isize| match (node(x, off[0], dw), node(y, off[1], dh)) {
                (Some(i), Some(j)) => flags[j * dw + i] & 3 == 1,
                _ => false,
            };
            reach_mask(&mut self.kmask, &mut self.khd, img, w, h, m, rpx, 255, false, margin);
            self.mw = w;
        }
        // Far terrain in the image: the fog keeps off it but for a narrow front at the void (`lod_front_px`), cut
        // per pixel like the reach over explored ground (0.3.10: the node grid alone let the fill 9 to 25 px in,
        // hundreds of blocks of loaded far terrain on a zoomed-out map). Where the void has just gone (far chunks
        // streamed in) the fog thins out over `VOID_FADE_S` instead of going a chunk square at a time.
        self.easing = false;
        if !p.far_fog && self.flags.iter().any(|&f| f & 16 != 0) {
            let now = std::time::Instant::now();
            let dt = self.void_at.map_or(1.0, |t| now.saturating_duration_since(t).as_secs_f32());
            self.void_at = Some(now);
            let step = dt / VOID_FADE_S;
            let prev_ok = self.void_key == Some(lkey) && self.voidv.len() == self.void_dw * self.void_dh;
            std::mem::swap(&mut self.voidv, &mut self.void_prev);
            self.voidv.clear();
            self.voidv.resize(dw * dh, 0.0);
            let (pg, pw, ph) = (self.void_g0, self.void_dw, self.void_dh);
            let mut any = false;
            for j in 0..dh {
                for i in 0..dw {
                    let f = self.flags[j * dw + i];
                    // Void and nothing else under the node.
                    let v = if f & (8 | 16 | 2) == 8 {
                        1.0
                    } else if prev_ok {
                        let (pi, pj) = (dg[0] + i as i64 - pg[0], dg[1] + j as i64 - pg[1]);
                        if pi >= 0 && pj >= 0 && (pi as usize) < pw && (pj as usize) < ph { (self.void_prev[pj as usize * pw + pi as usize] - step).max(0.0) } else { 0.0 }
                    } else {
                        0.0
                    };
                    any |= v > 0.0 && v < 1.0;
                    self.voidv[j * dw + i] = v;
                }
            }
            (self.void_g0, self.void_dw, self.void_dh, self.void_key) = (dg, dw, dh, Some(lkey));
            self.easing = any;
            let lrpx = lod_front_px(view.bpp, w, h);
            let m = (lrpx.ceil() as usize + 1).min(256);
            let flags = &self.flags;
            let node = move |p: isize, o: f64, n: usize| -> Option<usize> {
                let k = ((p as f64 + o) / cf + 0.5).floor() as i64 + dm as i64;
                (k >= 0 && (k as usize) < n).then_some(k as usize)
            };
            let margin = |x: isize, y: isize| match (node(x, off[0], dw), node(y, off[1], dh)) {
                (Some(i), Some(j)) => flags[j * dw + i] & 8 != 0,
                _ => false,
            };
            reach_mask(&mut self.kfar, &mut self.khd, img, w, h, m, lrpx, 0, true, margin);
            let (kfar, voidv) = (&self.kfar, &self.voidv);
            let far_a = super::view::FAR_ALPHA;
            par_rows(&mut self.kmask[..w * h], w, 64 * 1024, |y, row| {
                let gy = ((y as f64 + off[1]) / cf) as f32 + dm as f32;
                let j0 = (gy as usize).min(dh - 2);
                let fy = (gy - j0 as f32).clamp(0.0, 1.0);
                for (x, k) in row.iter_mut().enumerate() {
                    if img[(y * w + x) * 4 + 3] != far_a {
                        continue;
                    }
                    let mut v = kfar[y * w + x];
                    if any && v < 255 {
                        let gx = ((x as f64 + off[0]) / cf) as f32 + dm as f32;
                        let i0 = (gx as usize).min(dw - 2);
                        let fx = (gx - i0 as f32).clamp(0.0, 1.0);
                        let at = |i: usize, j: usize| voidv[j * dw + i];
                        let a = at(i0, j0) + (at(i0 + 1, j0) - at(i0, j0)) * fx;
                        let b = at(i0, j0 + 1) + (at(i0 + 1, j0 + 1) - at(i0, j0 + 1)) * fx;
                        let e = a + (b - a) * fy;
                        // Smoothstep, so the fade neither starts nor ends with a kink.
                        v = v.max((e * e * (3.0 - 2.0 * e) * 255.0) as u8);
                    }
                    *k = v;
                }
            });
        } else {
            self.void_key = None;
        }
        lap(1, &mut self.prof);
        // Distance (in nodes) to the unexplored, and a blurred copy for the edge shape.
        {
            let flags = &self.flags;
            chamfer(&mut self.dout, dw, dh, |i| flags[i] & 1 != 0, cap);
        }
        self.dsm.copy_from_slice(&self.dout);
        for _ in 0..3 {
            box_blur(&mut self.dsm, &mut self.scratch, dw, dh, r);
        }
        // Far terrain in view (an unknown node with no void pixel): its own distance field to the void. The fog
        // over the explored side is min(explored mask, far mask), so neither leaks the other's shape.
        let lod = !p.far_fog && self.flags.iter().any(|&f| f & 9 == 1);
        if lod {
            self.dvoid.resize(dw * dh, 0.0);
            self.dvsm.resize(dw * dh, 0.0);
            let flags = &self.flags;
            chamfer(&mut self.dvoid, dw, dh, |i| flags[i] & 8 != 0, lcap);
            self.dvsm.copy_from_slice(&self.dvoid);
            for _ in 0..3 {
                box_blur(&mut self.dvsm, &mut self.scratch, dw, dh, lr);
            }
        }
        lap(2, &mut self.prof);
        let quality = p.quality.min(3);
        let drift = self.advance(t, p);
        // Grain: world anchored (it pans with the map and drifts with the deck), about TEXEL_PX pixels a texel.
        if self.tile.len() != TILE * TILE {
            self.tile = detail_tile();
        }
        self.grain = [0.0, 0.6, 1.0, 1.0][p.quality.min(3) as usize];
        {
            let k = 1.0 / (view.bpp as f32 * TEXEL_PX).max(1e-4);
            let o = view.to_world(w, h, 0.0, 0.0);
            let ox = view.to_world(w, h, 1.0, 0.0);
            let oy = view.to_world(w, h, 0.0, 1.0);
            let (u0, v0) = ((o[0] as f32 + drift) * k, (o[1] as f32 + drift * 0.35) * k);
            // Kept small so f32 keeps sub-texel precision far from the origin.
            let wrap = |x: f32| x.rem_euclid(TILE as f32 * 64.0);
            self.daff = [
                wrap(u0),
                (ox[0] - o[0]) as f32 * k,
                (oy[0] - o[0]) as f32 * k,
                wrap(v0),
                (ox[1] - o[1]) as f32 * k,
                (oy[1] - o[1]) as f32 * k,
            ];
        }
        let churn = match p.look {
            Look::Overworld => 0.035,
            Look::Nether => 0.09,
            Look::End => 0.05,
        };
        let puff = match p.look {
            Look::Nether => (p.puff + 0.3).min(1.0),
            Look::End => p.puff * 0.3,
            Look::Overworld => p.puff.clamp(0.0, 1.0),
        };
        let (sdx, sdz) = sun_dir(p.day_time);
        let cfg = NoiseCfg {
            drift,
            morph: t * churn * (0.5 + p.speed * 0.5),
            warp: match p.look {
                Look::Overworld => 0.22 + (1.0 - puff) * 0.35,
                Look::Nether => 0.45,
                Look::End => 0.8,
            },
            puff,
            octaves: [4u32, 5, 6, 6][quality as usize],
            node_blocks: cell as f32 * view.bpp as f32,
            hi_on: p.look == Look::Overworld && quality >= 1,
            sun: (sdx, sdz),
            shadow_off: 3.0 * view.bpp as f32,
            look: p.look,
            t,
        };
        // Feature scale: fixed in the world within a zoom octave (110 blocks up to 2.5 blocks a pixel, then twice
        // that per doubling), cross-faded to the next octave over the last quarter of it. The old continuous
        // `110 * bpp / 2.5` stretched every cloud while zooming, which read as the fog regenerating.
        let lz = (view.bpp as f32 / 2.5).max(1.0).log2();
        let lvl = lz.floor();
        let mixk = smooth(0.75, 1.0, lz - lvl);
        let scale0 = 1.0 / (110.0 * lvl.exp2());
        let sun_img = {
            let (s, c) = (view.rotation.sin() as f32, view.rotation.cos() as f32);
            (sdx * c + sdz * s, -sdx * s + sdz * c)
        };
        // Reuse last frame's noise for every node still on the grid when nothing it depends on changed.
        let nkey = [
            lkey.0 as u64,
            lkey.1,
            lkey.2,
            t.to_bits() as u64,
            drift.to_bits() as u64,
            cfg.morph.to_bits() as u64,
            cfg.warp.to_bits() as u64,
            puff.to_bits() as u64,
            quality as u64 | (cfg.hi_on as u64) << 8,
            sdx.to_bits() as u64 | (sdz.to_bits() as u64) << 32,
            p.look as u64,
            mixk.to_bits() as u64,
            scale0.to_bits() as u64,
        ];
        std::mem::swap(&mut self.noise, &mut self.noise_prev);
        self.noise.clear();
        self.noise.resize(nw * nh, Noise::default());
        let lim = feather * EDGE_OUT + 2.5;
        let llim = lfeather * EDGE_OUT + 2.5;
        if self.noise_key == Some(nkey) && self.noise_w > 0 {
            let (pw, ph) = (self.noise_w, self.noise_prev.len() / self.noise_w);
            let (sx, sy) = (g0[0] - self.noise_g0[0], g0[1] - self.noise_g0[1]);
            for j in 0..nh {
                let pj = j as i64 + sy;
                if pj < 0 || pj >= ph as i64 {
                    continue;
                }
                for i in 0..nw {
                    let pi = i as i64 + sx;
                    if pi >= 0 && pi < pw as i64 {
                        let n = self.noise_prev[pj as usize * pw + pi as usize];
                        if n.ok {
                            self.noise[j * nw + i] = n;
                            self.reused += 1;
                        }
                    }
                }
            }
        }
        {
            let (dout, dvoid) = (&self.dout, &self.dvoid);
            par_rows(&mut self.noise, nw, 1024, |ny, row| {
                for (nx, n) in row.iter_mut().enumerate() {
                    let di = (ny + dm) * dw + nx + dm;
                    if n.ok || dout[di] > lim || (lod && dvoid[di] > llim) {
                        continue;
                    }
                    let [wx, wz] = node_world(nx, ny);
                    let a = sample_noise(wx, wz, &cfg, scale0);
                    *n = if mixk <= 0.0 {
                        a
                    } else {
                        let b = sample_noise(wx, wz, &cfg, scale0 * 0.5);
                        if mixk >= 1.0 { b } else { Noise::mix(a, b, mixk) }
                    };
                }
            });
        }
        self.computed = self.noise.iter().filter(|n| n.ok).count() - self.reused;
        lap(3, &mut self.prof);
        self.noise_key = Some(nkey);
        self.noise_g0 = g0;
        self.noise_w = nw;
        {
            let (dout, dsm, noise, dvoid, dvsm) = (&self.dout, &self.dsm, &self.noise, &self.dvoid, &self.dvsm);
            // On this thread: a few thousand cheap nodes, where the thread spawns cost more than they save.
            par_rows(&mut self.field, nw, usize::MAX, |ny, row| {
                for (nx, f) in row.iter_mut().enumerate() {
                    let di = (ny + dm) * dw + nx + dm;
                    let n = &noise[ny * nw + nx];
                    if dout[di] > lim || !n.ok || (lod && dvoid[di] > llim) {
                        *f = Field::default();
                        continue;
                    }
                    // Edge position 0 (at the unexplored) .. 1 (feather width out), pushed in and out by the cloud
                    // density so the edge billows instead of following a line; built on the blurred distance so
                    // no chunk corner survives in its contours.
                    let rough = ((n.lo - 0.5) * 3.0 + (n.d - 0.5) * 0.4).clamp(-1.0, 1.0);
                    let te = (dsm[di] - 1.0).max(0.0) / feather - rough * 0.45;
                    // Soft overlap: the deck thins out over the explored ground up to EDGE_OUT feather widths.
                    let edge = 1.0 - smooth(0.15, EDGE_OUT - 0.1, te);
                    // The opaque fill stops short of the deck, so the deck's own puffs make the outer edge. The
                    // floor on the raw distance keeps every unexplored pixel fully hidden: the four nodes around
                    // any such pixel lie within 1.5 nodes of a node that holds it.
                    let mut cg = (1.0 - smooth(-0.35, 0.85, te)).max(1.0 - smooth(1.5, 1.5 + 16.0 / cell as f32, dout[di]));
                    let mut edge = edge.max(cg);
                    if lod {
                        // The same edge against the void, over the wider far front and rougher (the far coverage
                        // comes in chunk and region squares: the billows break its straight sides), with the same
                        // floor that keeps every no-terrain pixel hidden.
                        let tv = (dvsm[di] - 1.0).max(0.0) / lfeather - rough * 0.6;
                        let cgv = (1.0 - smooth(-0.35, 0.85, tv)).max(1.0 - smooth(1.5, 1.5 + 16.0 / cell as f32, dvoid[di]));
                        let ev = (1.0 - smooth(0.15, EDGE_OUT - 0.1, tv)).max(cgv);
                        edge = edge.min(ev);
                        cg = cg.min(cgv);
                    }
                    *f = Field { d: n.d, hi: n.hi, hi_shadow: n.hi_shadow, aux: n.aux, edge, cg };
                }
            });
        }
        self.last_nodes = self.field.iter().filter(|f| f.edge > 0.0).count();
        lap(4, &mut self.prof);

        // Shading.
        let storm = storminess(p);
        let density = (p.density + storm * 0.25).clamp(0.0, 1.0);
        let thr = 1.0 - density;
        let sun_h = sun_height(p.day_time);
        let (day, twilight) = daylight(p.day_time);
        // Sun elevation for the march: low sun = long shadows. At night the moon (opposite) lights the clouds.
        // Capped below the zenith: a noon sun straight above flattens the deck, a slight tilt models it.
        let elev = sun_h.abs().clamp(0.08, 0.72);
        let steps = [1usize, 3, 4, 7][quality as usize];
        let px_node = 6.0 / cell as f32;
        let step = (1.4 + (1.0 - elev) * 2.2) * px_node;
        // Light vector: horizontal part along the sun direction on the image, vertical part = elevation.
        let (lh, lv) = ((1.0 - elev * elev).max(0.0).sqrt(), elev);
        // Height rise per node of a ray towards the light (in density units): flat at low sun.
        let rise = lv / lh.max(0.05) * 0.035 / px_node;
        // Gradient scale: density per node to a slope, independent of the node size.
        let kn = 6.0 * px_node;
        let (sx, sy) = (sun_img.0 * step, sun_img.1 * step);
        let volumetric = quality >= 3;
        let rays = volumetric && p.god_rays && p.look == Look::Overworld;
        let _pal = palette(p);
        let sun_col = if p.look == Look::Overworld {
            let warm = lerp3(rgbf(0xFFF4E0), rgbf(0xFFB070), twilight);
            if sun_h > 0.0 { scale3(warm, 0.55 + 0.45 * day) } else { scale3(rgbf(0x9DB4FF), 0.45) }
        } else {
            [0.0; 3]
        };
        let shaft_k = (0.25 + twilight * 0.6) * (1.0 - storm * 0.7) * if sun_h > 0.0 { 1.0 } else { 0.35 };
        let intensity = p.intensity.clamp(0.0, 2.0);
        let glow_col = rgbf(NETHER_GLOW);
        let shimmer_col = rgbf(END_SHIMMER);
        {
            let field = &self.field;
            let alpha_of = |f: &Field| -> f32 {
                let body = smooth(thr - 0.14, thr + 0.2, f.d + 0.1);
                (body * f.edge).clamp(0.0, 1.0)
            };
            let sample = |x: f32, y: f32| -> &Field {
                let xi = (x.round() as isize).clamp(0, nw as isize - 1) as usize;
                let yi = (y.round() as isize).clamp(0, nh as isize - 1) as usize;
                &field[yi * nw + xi]
            };
            // Bilinear density (and edge) between nodes, so slopes and shadows do not step node to node.
            let dens = |x: f32, y: f32| -> (f32, f32) {
                let x = x.clamp(0.0, (nw - 1) as f32);
                let y = y.clamp(0.0, (nh - 1) as f32);
                let (x0, y0) = (x as usize, y as usize);
                let (x1, y1) = ((x0 + 1).min(nw - 1), (y0 + 1).min(nh - 1));
                let (tx, ty) = (x - x0 as f32, y - y0 as f32);
                let at = |xx: usize, yy: usize| &field[yy * nw + xx];
                let l = |a: f32, b: f32, t: f32| a + (b - a) * t;
                let (a, b, c, d) = (at(x0, y0), at(x1, y0), at(x0, y1), at(x1, y1));
                (l(l(a.d, b.d, tx), l(c.d, d.d, tx), ty), l(l(a.edge, b.edge, tx), l(c.edge, d.edge, tx), ty))
            };
            par_rows(&mut self.node, nw, 1024, |ny, row| {
                for (nx, out) in row.iter_mut().enumerate() {
                    let f = &field[ny * nw + nx];
                    // The outer rings of the grid only feed the light march and the edge distance: the composite
                    // (and its 4-tap refine) reads 2 rings past the image at most, so they are not shaded.
                    let outside = ny < SKIP || nx < SKIP || ny + SKIP >= nh || nx + SKIP >= nw;
                    if outside || (f.edge <= 0.0 && f.cg <= 0.0) {
                        *out = Node::default();
                        continue;
                    }
                    let a = alpha_of(f);
                    let (fx, fy) = (nx as f32, ny as f32);
                    // The deck seen from above is a heightfield (density = cloud top height). Its normal against
                    // the light gives the sculpted billows; a march towards the light finds taller tops that
                    // shade this one (Beer-Lambert through what sticks out above the ray).
                    let g = |dx: f32, dy: f32| dens(fx + dx, fy + dy).0;
                    let (gx, gy) = ((g(2.0, 0.0) - g(-2.0, 0.0)) * 0.25 * kn, (g(0.0, 2.0) - g(0.0, -2.0)) * 0.25 * kn);
                    let lambert = (((-gx * sun_img.0 - gy * sun_img.1) * lh + lv) / (gx * gx + gy * gy + 1.0).sqrt()).clamp(0.0, 1.0);
                    let mut tau = 0.0f32;
                    for k in 1..=steps {
                        let kk = k as f32 * if volumetric { 0.7 } else { 1.0 };
                        let (sd_, se) = dens(fx + sx * kk, fy + sy * kk);
                        let over = sd_ - (f.d + kk * step * rise);
                        if over > 0.0 {
                            tau += over * se.max(0.3) * if volumetric { 0.7 } else { 1.0 };
                        }
                    }
                    let trans = (-tau * 5.0).exp();
                    // Thin cloud lets light through (powder-like brightening of the wisps).
                    let thick = smooth(thr, thr + 0.45, f.d);
                    let mut lum = 0.3 + 0.7 * lambert * (0.25 + 0.75 * trans);
                    lum += (1.0 - thick) * 0.18 * trans;
                    // The high layer's shadow on the deck.
                    lum *= 1.0 - f.hi_shadow * 0.3;
                    let lum = lum.clamp(0.0, 1.0);
                    // Silver lining (moonlit rim at night): thin edges facing the light.
                    let thin = (1.0 - (a - 0.4).abs() * 2.4).clamp(0.0, 1.0);
                    let rim = (thin * (0.3 + 0.7 * lambert) * trans).clamp(0.0, 1.0);
                    let mut add = [0.0f32; 3];
                    match p.look {
                        Look::Overworld => {
                            if rays {
                                // Light shafts: from a gap, look towards the light; the more open sky up-sun
                                // with cloud beside it, the brighter the shaft.
                                let mut open = 0.0;
                                let mut occl = 0.0f32;
                                for k in 1..=10 {
                                    let s = sample(fx + sun_img.0 * 2.2 * k as f32, fy + sun_img.1 * 2.2 * k as f32);
                                    let sa = alpha_of(s);
                                    open += 1.0 - sa;
                                    occl = occl.max(sa);
                                }
                                let shaft = (open / 10.0) * occl * (1.0 - a) * f.edge * shaft_k;
                                for c in 0..3 {
                                    add[c] += sun_col[c] * shaft * 0.55;
                                }
                            }
                        }
                        Look::Nether => {
                            // Lava light from below shows through thin ash; thick cores and veins stay dark.
                            let pulse = 0.8 + 0.2 * (t * 1.3 + fx * 0.07 + fy * 0.05).sin();
                            let under = (1.0 - smooth(thr + 0.05, thr + 0.55, f.d)) * f.edge * pulse;
                            let vein = smooth(0.8, 0.96, f.aux) * smooth(thr, thr + 0.3, f.d) * 0.8;
                            let k = (under * 0.75 * intensity) * (1.0 - vein);
                            for c in 0..3 {
                                add[c] += glow_col[c] * k;
                            }
                            *out = Node { a: a.max(f.edge * 0.55 * intensity.min(1.0)), lum: (lum * (1.0 - vein * 0.85)).clamp(0.0, 1.0), rim: rim * 0.5, add, wisp: 0.0, cg: f.cg };
                            continue;
                        }
                        Look::End => {
                            let sh = ((f.aux * 9.0 + t * 0.9).sin() * 0.5 + 0.5) * intensity;
                            for c in 0..3 {
                                add[c] += shimmer_col[c] * sh * 0.22 * a;
                            }
                            *out = Node { a: (a * 0.85).max(f.edge * 0.5), lum, rim: rim * 0.8, add, wisp: 0.0, cg: f.cg };
                            continue;
                        }
                    }
                    *out = Node { a, lum, rim, add, wisp: f.hi * f.edge * 0.75, cg: f.cg };
                }
            });
        }

        lap(5, &mut self.prof);
        // Lightning: a strike now and then in a thick part of the deck, a flash around it and (mode 2) a bolt
        // crawling over the cloud tops.
        let th = thunder(p);
        if p.lightning > 0 && th > 0.15 {
            let slot = (t * 4.0).floor() as i32;
            let every = (40.0 / (p.lightning_rate.max(0.05) * th)).max(2.0) as u32;
            let busy = self.strike.is_some_and(|(_, s, _)| t - s < 0.6 && t >= s);
            if !busy && hash3(slot, 77, 5) % every == 0 {
                let seed = hash3(slot, 3, 1);
                let n = nw * nh;
                let mut at = None;
                for k in 0..96 {
                    let i = (seed as usize + k * 7919) % n;
                    if self.node[i].a > 0.6 && self.field[i].edge > 0.8 {
                        at = Some(i);
                        break;
                    }
                }
                if let Some(i) = at {
                    let gpos = [(self.g0[0] + (i % nw) as i64) as f64, (self.g0[1] + (i / nw) as i64) as f64];
                    self.strike = Some((gpos, slot as f32 / 4.0, seed));
                }
            }
            if let Some((gpos, start, seed)) = self.strike {
                // The strike stays on its ground while the map pans.
                let (cx, cy) = ((gpos[0] - self.g0[0] as f64) as f32, (gpos[1] - self.g0[1] as f64) as f32);
                let pos = [cx * cell as f32 - self.off[0] as f32, cy * cell as f32 - self.off[1] as f32];
                let age = t - start;
                if !(0.0..0.6).contains(&age) {
                    self.strike = None;
                } else {
                    // Two flickers then a fade.
                    let flick = if age < 0.08 { 1.0 } else if age < 0.14 { 0.3 } else if age < 0.22 { 0.85 } else { (1.0 - (age - 0.22) / 0.38).max(0.0) * 0.6 };
                    let radius = 90.0 / cell as f32;
                    let flash = rgbf(0xE8EEFF);
                    for ny in 0..nh {
                        for nx in 0..nw {
                            let i = ny * nw + nx;
                            let nd = &mut self.node[i];
                            if nd.a <= 0.0 {
                                continue;
                            }
                            let d2 = (nx as f32 - cx).powi(2) + (ny as f32 - cy).powi(2);
                            let k = (1.0 - d2 / (radius * radius)).max(0.0);
                            let k = (k * k * 0.9 + 0.12) * flick * nd.a;
                            for c in 0..3 {
                                nd.add[c] += flash[c] * k * 0.7;
                            }
                        }
                    }
                    if p.lightning >= 2 && age < 0.3 {
                        self.bolt(pos, seed, flick, w, h);
                    }
                }
            }
        } else {
            self.strike = None;
        }

        // Embers (Nether) and sparkles (End): world-anchored points that live a few seconds each.
        if p.look != Look::Overworld && intensity > 0.0 {
            self.motes(w, h, view, t, p.look, intensity);
        }
        lap(6, &mut self.prof);
        true
    }

    /// Integrates the wind phase up to time `t`: the speed eases towards `wind_target` (time constant
    /// `WIND_TAU`, never above the stormiest target), the step is at most `MAX_DT`, and a clock that runs backwards
    /// (a new world) only resets the reference. Returns the phase in blocks.
    pub fn advance(&mut self, t: f32, p: &Params) -> f32 {
        let target = wind_target(p);
        let dt = match self.last_t {
            Some(l) if t >= l => (t - l).min(MAX_DT),
            Some(_) => 0.0,
            None => {
                self.wind = target;
                0.0
            }
        };
        self.last_t = Some(t);
        let k = 1.0 - (-dt / WIND_TAU).exp();
        let cap = 12.0 * p.speed.max(0.0) + 1e-3;
        self.wind = (self.wind + (target - self.wind) * k).clamp(0.0, cap);
        self.phase += self.wind * dt;
        // Kept in a range where f32 still resolves a fraction of a block (the noise is world-anchored and does
        // not repeat, so a wrap shows as one jump every few hours at most).
        if self.phase.abs() > 1.0e6 {
            self.phase = 0.0;
        }
        self.phase
    }

    /// Fill coverage and deck alpha at image pixel (`x`, `y`) as the last `composite` drew them (for tests).
    /// The hard reach factor at an image pixel (1 on the unexplored, 0 past the reach), for tests.
    pub fn kmask_at(&self, x: usize, y: usize) -> f32 {
        self.kmask.get(y * self.mw + x).map_or(1.0, |&k| k as f32 / 255.0)
    }

    pub fn mask_at(&self, x: usize, y: usize) -> (f32, f32) {
        let (cell, rw) = (self.rcell.max(1) as f64, self.rnw);
        let rh = self.col.len() / rw.max(1);
        let sk = (SKIP * self.cell) as f64;
        let (u, v) = ((x as f64 + self.off[0] - sk) / cell, (y as f64 + self.off[1] - sk) / cell);
        let (nx, ny) = ((u.floor() as usize).min(rw - 2), (v.floor() as usize).min(rh - 2));
        let (tx, ty) = ((u - nx as f64) as f32, (v - ny as f64) as f32);
        let i = ny * rw + nx;
        let f = |n: &[f32; 8]| (n[7], n[0]);
        let (a, b, c, d) = (f(&self.col[i]), f(&self.col[i + 1]), f(&self.col[i + rw]), f(&self.col[i + rw + 1]));
        let l = |p: f32, q: f32, t: f32| p + (q - p) * t;
        let k = self.kmask.get(y * self.mw + x).map_or(1.0, |&k| k as f32 / 255.0);
        (l(l(a.0, b.0, tx), l(c.0, d.0, tx), ty) * k, l(l(a.1, b.1, tx), l(c.1, d.1, tx), ty) * k)
    }

    /// A jagged bolt from `from`, with one branch, as dots.
    fn bolt(&mut self, from: [f32; 2], seed: u32, k: f32, w: usize, h: usize) {
        let col = rgbf(0xF4F6FF);
        let glow = rgbf(0xA8B8FF);
        let mut rng = seed;
        let mut next = || {
            rng = hash3(rng as i32, 17, 23);
            unit(rng)
        };
        let mut paths: Vec<([f32; 2], f32, usize)> = vec![(from, next() * std::f32::consts::TAU, 18)];
        let mut b = 0;
        while b < paths.len() && b < 4 {
            let (mut p, mut ang, len) = paths[b];
            for s in 0..len {
                ang += (next() - 0.5) * 1.3;
                let seg = 4.0 + next() * 5.0;
                let q = [p[0] + ang.cos() * seg, p[1] + ang.sin() * seg];
                let n = seg.ceil() as usize;
                for j in 0..n {
                    let f = j as f32 / n as f32;
                    let (x, y) = (p[0] + (q[0] - p[0]) * f, p[1] + (q[1] - p[1]) * f);
                    if x >= 0.0 && y >= 0.0 && (x as usize) < w && (y as usize) < h {
                        let fade = 1.0 - s as f32 / len as f32 * 0.6;
                        self.dots.push(Dot { x, y, c: glow, k: 0.45 * k * fade, big: true });
                        self.dots.push(Dot { x, y, c: col, k: 0.95 * k * fade, big: false });
                    }
                }
                p = q;
                if b == 0 && s == len / 2 && paths.len() < 3 {
                    paths.push((p, ang + 0.9 * if next() > 0.5 { 1.0 } else { -1.0 }, len / 2));
                }
            }
            b += 1;
        }
    }

    fn motes(&mut self, w: usize, h: usize, view: &super::view::View, t: f32, look: Look, intensity: f32) {
        // Spacing grows with zoom so the count on screen stays about the same.
        let spacing = (14.0 * view.bpp).max(4.0) as f32;
        let corners = [[0.0, 0.0], [w as f64, 0.0], [0.0, h as f64], [w as f64, h as f64]].map(|c| view.to_world(w, h, c[0], c[1]));
        let (mut x0, mut x1, mut z0, mut z1) = (f64::MAX, f64::MIN, f64::MAX, f64::MIN);
        for c in corners {
            x0 = x0.min(c[0]);
            x1 = x1.max(c[0]);
            z0 = z0.min(c[1]);
            z1 = z1.max(c[1]);
        }
        let (gx0, gx1) = ((x0 / spacing as f64).floor() as i32, (x1 / spacing as f64).ceil() as i32);
        let (gz0, gz1) = ((z0 / spacing as f64).floor() as i32, (z1 / spacing as f64).ceil() as i32);
        if (gx1 - gx0) as i64 * (gz1 - gz0) as i64 > 40_000 {
            return;
        }
        let chance = match look {
            Look::Nether => 0.10,
            _ => 0.06,
        } * intensity.min(2.0);
        let ember = rgbf(0xFFB040);
        let hot = rgbf(0xFFF0B0);
        let star = rgbf(0xF0E0FF);
        let (nw, cell) = (self.nw, self.cell as f32);
        for gz in gz0..gz1 {
            for gx in gx0..gx1 {
                let hsh = hash3(gx, gz, 4242);
                if unit(hsh) > chance {
                    continue;
                }
                let life = 3.0 + unit(hsh >> 7) * 3.0;
                let phase = unit(hsh >> 3) * life;
                let age = ((t + phase) % life) / life;
                let epoch = ((t + phase) / life).floor() as i32;
                let h2 = hash3(gx, gz, epoch);
                let (mut wx, mut wz) = ((gx as f32 + unit(h2)) * spacing, (gz as f32 + unit(h2 >> 16)) * spacing);
                let (c, k, big) = match look {
                    Look::Nether => {
                        // Embers drift with the hot air and flicker out.
                        wx += age * spacing * 0.8;
                        wz -= age * spacing * 1.4;
                        let flick = 0.7 + 0.3 * (t * 13.0 + gx as f32).sin();
                        (lerp3(hot, ember, age), (1.0 - age) * flick, unit(h2 >> 9) > 0.7)
                    }
                    _ => {
                        let tw = (age * std::f32::consts::PI).sin();
                        (star, tw * tw * 0.8, false)
                    }
                };
                let [ix, iy] = view.to_image(w, h, wx as f64, wz as f64);
                let (ix, iy) = (ix as f32, iy as f32);
                if ix < 0.0 || iy < 0.0 || ix >= w as f32 || iy >= h as f32 {
                    continue;
                }
                // Only over the clouds.
                let (gx, gy) = ((ix + self.off[0] as f32) / cell, (iy + self.off[1] as f32) / cell);
                let ni = (gy.round() as usize).min(self.nh - 1) * nw + (gx.round() as usize).min(nw - 1);
                let a = self.node[ni].a;
                if a < 0.2 {
                    continue;
                }
                self.dots.push(Dot { x: ix, y: iy, c, k: k * a * intensity.min(1.3), big });
            }
        }
    }

    /// Blends the clouds over `img` and makes every pixel opaque: the map, then the opaque fill (sky tone between
    /// the clouds), then the cloud deck, both with smooth coverages that reach over the explored border. Works a node cell at a
    /// time, stepping the interpolated values linearly across each row of the cell; rows of cells run in
    /// parallel on big maps.
    pub fn composite(&mut self, img: &mut [u8], w: usize, h: usize, p: &Params, _t: f32) {
        let pal = palette(p);
        // Only the nodes the image can reach (the outer SKIP rings are never shaded): crop them before the refine.
        let (gw, cell) = (self.nw, self.cell.max(1));
        let (nw, nh) = (self.nw - 2 * SKIP, self.nh - 2 * SKIP);
        if self.coarse.len() != nw * nh {
            self.coarse.clear();
            self.coarse.resize(nw * nh, [0.0; 8]);
        }
        let wisp_col = lerp3(pal.lit, pal.rim, 0.5);
        let sky = sky(p);
        // The tone between the clouds: mostly sky (or the Nether / End glow), a touch of the fog grey.
        let fog = rgbf(super::view::FOG);
        let fill = lerp3(fog, sky, 0.9);
        for i in 0..nw * nh {
            let gi = (i / nw + SKIP) * gw + i % nw + SKIP;
            let nd = if gi < self.node.len() { self.node[gi] } else { Node::default() };
            let a = nd.a.max(nd.wisp);
            if a <= 0.0 && nd.add == [0.0; 3] && nd.cg <= 0.0 {
                self.coarse[i] = [0.0; 8];
                continue;
            }
            let mut c = lerp3(pal.shade, pal.lit, nd.lum);
            c = lerp3(c, pal.rim, (nd.rim * 0.6).min(1.0));
            // Wisps over the deck: a lighter, thin layer.
            if nd.wisp > 0.0 {
                let k = nd.wisp / a.max(1e-3);
                c = lerp3(c, wisp_col, (k * 0.6).min(1.0));
            }
            // Premultiplied by alpha, so a cloud edge fading to nothing does not fade through black (the dark
            // halo the plain colour interpolation drew against empty nodes).
            self.coarse[i] = [a, c[0] * a, c[1] * a, c[2] * a, nd.add[0], nd.add[1], nd.add[2], nd.cg];
        }
        // Refine the node grid to half the cell size with a 4-tap interpolating filter: plain bilinear between
        // the coarse nodes kinks the shading at every node row and column, which showed as faint grid seams.
        let (nw, nh, cell) = refine(&self.coarse, nw, nh, cell, &mut self.col, &mut self.tmp8);
        self.rnw = nw;
        self.rcell = cell;
        let inv = 1.0 / cell as f32;
        let col = &self.col;
        let (tile, daff, grain) = (&self.tile, self.daff, if self.tile.len() == TILE * TILE { self.grain } else { 0.0 });
        // Refined node (i, j) sits at image pixel (i * cell - offx, j * cell - offy): the sub-pixel offset of the
        // world-anchored lattice, so the clouds slide with the map instead of snapping node to node.
        let (offx, offy, rc) = ((self.off[0] - (SKIP * self.cell) as f64) as f32, (self.off[1] - (SKIP * self.cell) as f64) as f32, cell as f32);
        // Per frame: pixel bounds of every cell column and the empty cells (all four corners clear), so the row
        // loop neither recomputes them per row nor per segment.
        let empty = |c: &[f32; 8]| c[0] == 0.0 && c[4] + c[5] + c[6] == 0.0 && c[7] == 0.0;
        self.cxe.clear();
        self.cxe.extend((0..nw).map(|nx| ((((nx + 1) as f32) * rc - offx).ceil().max(0.0) as usize).min(w)));
        self.cemp.clear();
        self.cemp.extend((0..nw * nh).map(|i| empty(&col[i])));
        let (cxe, cemp, kmask) = (&self.cxe, &self.cemp, &self.kmask);
        par_rows(&mut img[..w * h * 4], w * 4, 64 * 1024, |y, row| {
            let gyf = (y as f32 + offy) * inv;
            let ny = gyf.floor().max(0.0) as usize;
            if ny + 1 >= nh {
                for px in row.chunks_exact_mut(4) {
                    px[3] = 255;
                }
                return;
            }
            let fy = gyf - ny as f32;
            let mut x = 0usize;
            let mut nx = (offx * inv).floor().max(0.0) as usize;
            while x < w {
                if nx + 1 >= nw {
                    for px in row[x * 4..].chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                    break;
                }
                // Pixels of this column of cells: (x + offx) / rc < nx + 1.
                let xe = cxe[nx];
                if xe <= x {
                    nx += 1;
                    continue;
                }
                let i = ny * nw + nx;
                if cemp[i] && cemp[i + 1] && cemp[i + nw] && cemp[i + nw + 1] {
                    for px in row[x * 4..xe * 4].chunks_exact_mut(4) {
                        px[3] = 255;
                    }
                    x = xe;
                    nx += 1;
                    continue;
                }
                let fx = (x as f32 + offx) * inv - nx as f32;
                let (gx, gy) = (x as f32, y as f32);
                let mut gu = daff[0] + daff[1] * gx + daff[2] * gy;
                let mut gv = daff[3] + daff[4] * gx + daff[5] * gy;
                let (c00, c10, c01, c11) = (&col[i], &col[i + 1], &col[i + nw], &col[i + nw + 1]);
                let mut l = [0.0f32; 8];
                let mut d = [0.0f32; 8];
                for k in 0..8 {
                    let a = c00[k] + (c01[k] - c00[k]) * fy;
                    let r = c10[k] + (c11[k] - c10[k]) * fy;
                    d[k] = (r - a) * inv;
                    l[k] = a + (r - a) * fx;
                }
                for (px, &km) in row[x * 4..xe * 4].chunks_exact_mut(4).zip(&kmask[y * w + x..y * w + xe]) {
                    // One rule for every pixel, explored or not: the map, then the opaque fill, then the
                    // deck. No per-pixel switch, so nothing of the explored border's shape shows; the fill
                    // coverage is 1 over all unexplored pixels by construction (see `Field::cg`). The hard reach
                    // factor `km` scales all three (exactly 0 past the reach, 1 on the unexplored).
                    let kf = km as f32 * (1.0 / 255.0);
                    let (a, cg, ad) = if km == 255 { (l[0], l[7], 1.0) } else { (l[0] * kf, l[7] * kf, kf) };
                    if km != 0 && (a > 0.002 || cg > 0.002 || (l[4] + l[5] + l[6]) * ad > 0.5) {
                        // Grain: erodes the deck's soft edges into crisp billows (strongest at half
                        // coverage) and shades its inside a little. The fill under it is untouched, so
                        // unexplored ground stays hidden.
                        let (mut m, mut lum) = (1.0f32, 1.0f32);
                        if grain > 0.0 && a > 0.002 {
                            let dn = tile_at(tile, gu, gv) - 0.5;
                            m = (1.0 + dn * grain * 4.5 * (1.0 - a)).clamp(0.0, 1.0 / a.max(1e-3));
                            lum = 1.0 + dn * grain * 0.35;
                        }
                        let a2 = (a * m).min(1.0);
                        let ak = cg.max(a2);
                        for k in 0..3 {
                            let under = px[k] as f32 + (fill[k] - px[k] as f32) * cg;
                            let v = under * (1.0 - a2) + l[k + 1] * ad * m * lum + l[k + 4] * ak;
                            px[k] = v.clamp(0.0, 255.0) as u8;
                        }
                    }
                    gu += daff[1];
                    gv += daff[4];
                    px[3] = 255;
                    for k in 0..8 {
                        l[k] += d[k];
                    }
                }
                x = xe;
                nx += 1;
            }
        });
        // Embers, sparkles, bolts.
        for dot in &self.dots {
            let (x, y) = (dot.x as isize, dot.y as isize);
            let mut put = |x: isize, y: isize, k: f32| {
                if x < 0 || y < 0 || x as usize >= w || y as usize >= h {
                    return;
                }
                let i = (y as usize * w + x as usize) * 4;
                for c in 0..3 {
                    let under = img[i + c] as f32;
                    img[i + c] = (under + (dot.c[c] - under) * k.clamp(0.0, 1.0)) as u8;
                }
            };
            put(x, y, dot.k);
            if dot.big {
                for (dx, dy) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    put(x + dx, y + dy, dot.k * 0.5);
                }
            }
        }
        // No final opacity pass: every row path above already wrote alpha 255 to each of its pixels, and the dots
        // only touch colour.
    }
}

/// Horizontal direction towards the sun (or the moon at night) on the map, unit length: it rises in the east
/// (+x) and sets in the west, leaning a little south.
pub fn sun_dir(day_time: u32) -> (f32, f32) {
    let a = (day_time % 12000) as f32 / 12000.0 * std::f32::consts::PI;
    let (x, z) = (a.cos(), 0.45);
    let l = (x * x + z * z).sqrt();
    (x / l, z / l)
}
