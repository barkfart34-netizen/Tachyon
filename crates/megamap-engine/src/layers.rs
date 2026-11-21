//! Automatic map layer: whether the player is under the open sky or in a cave, with hysteresis so walking under a
//! tree or a bridge does not flip the map, and the Xaero-style cave slice (a window of `depth` blocks under a cut
//! ceiling) the cave layer bakes. Pure and cheap: the glue feeds it one probe every half second.

use super::baker::{ChunkSource, ColorTable, MC_NONE, MC_PLANT, MC_WATER};

/// What layer the map shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerMode {
    /// Surface or cave, by where the player is.
    Auto,
    Surface,
    Cave,
    /// The cave slice at a fixed height (`Settings::fixed_y`).
    FixedY,
}

impl LayerMode {
    pub const ALL: [LayerMode; 4] = [LayerMode::Auto, LayerMode::Surface, LayerMode::Cave, LayerMode::FixedY];
    pub fn index(self) -> u8 {
        LayerMode::ALL.iter().position(|&m| m == self).unwrap_or(0) as u8
    }
    pub fn from_index(i: u8) -> LayerMode {
        LayerMode::ALL[(i as usize).min(3)]
    }
    pub fn key(self) -> &'static str {
        match self {
            LayerMode::Auto => "auto",
            LayerMode::Surface => "surface",
            LayerMode::Cave => "cave",
            LayerMode::FixedY => "fixed_y",
        }
    }
    pub fn from_key(k: &str) -> Option<LayerMode> {
        LayerMode::ALL.into_iter().find(|m| m.key() == k)
    }
    pub fn next(self) -> LayerMode {
        LayerMode::from_index((self.index() + 1) % 4)
    }
}

/// "Surface if": 0 sky light or above the ground, 1 sky light only, 2 above the ground only.
pub const RULES: usize = 3;

/// Sky light at the player at or above which the sky counts as seen; at or below `SKY_CAVE` it counts as hidden.
pub const SKY_SURFACE: u8 = 10;
pub const SKY_CAVE: u8 = 4;
/// Blocks under the top of the column that still count as on the surface (standing in a ditch), and the depth
/// under it past which the player is surely under ground.
pub const GROUND_SURFACE: i32 = 3;
pub const GROUND_CAVE: i32 = 6;
/// Seconds the evidence must hold before the layer flips: into a cave slowly (a short tunnel is not a cave), out
/// faster.
pub const ENTER_S: f32 = 1.0;
pub const EXIT_S: f32 = 0.5;
/// Cross-fade length of a layer switch.
pub const FADE_S: f32 = 0.35;

/// One look at the player's spot.
#[derive(Clone, Copy, Debug, Default)]
pub struct Probe {
    /// Feet block y.
    pub y: i32,
    /// Sky light 0..=15 at the feet; `None` when unknown.
    pub sky: Option<u8>,
    /// The top block of the player's column in the surface map; `None` when not baked yet.
    pub ground: Option<i32>,
}

/// What the probe says: surely open sky, surely cave, or in between (keep the current layer).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Evidence {
    Surface,
    Cave,
    Unsure,
}

pub fn evidence(p: Probe, rule: u8) -> Evidence {
    let sky = match p.sky {
        Some(s) if s >= SKY_SURFACE => Evidence::Surface,
        Some(s) if s <= SKY_CAVE => Evidence::Cave,
        _ => Evidence::Unsure,
    };
    let height = match p.ground {
        Some(g) if p.y >= g - GROUND_SURFACE => Evidence::Surface,
        Some(g) if p.y < g - GROUND_CAVE => Evidence::Cave,
        _ => Evidence::Unsure,
    };
    match rule {
        1 => sky,
        2 => height,
        // Either sign of open sky wins; a cave needs both (or the one that is known).
        _ => match (sky, height) {
            (Evidence::Surface, _) | (_, Evidence::Surface) => Evidence::Surface,
            (Evidence::Cave, Evidence::Cave) | (Evidence::Cave, Evidence::Unsure) | (Evidence::Unsure, Evidence::Cave) => Evidence::Cave,
            _ => Evidence::Unsure,
        },
    }
}

// ---- the roof check (0.3.4) ----
//
// Sky light alone flips the map in a house at night or under a thick canopy, and the surface map's height says
// "cave" under a tree or a bridge and in deep water. The roof check looks straight up from a square of columns
// around the player: a cave has a natural roof over every column, several solid blocks thick; a building, a
// bridge or ice has a thin one with air above; a tree or the water has none (leaves, glass and water are not
// roofs). The verdict feeds the same dwell-time machine as before.

/// Solid blocks in a row that make a roof natural (cave rock) rather than built (a floor, a roof, a bridge).
pub const ROOF_THICK: i32 = 3;
/// Blocks above the feet the roof scan starts at: the head is at +1, so +2 is the first block over it.
pub const ROOF_FROM: i32 = 2;

/// What the columns above the player hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Roof {
    /// Columns looked at (loaded ones).
    pub columns: u8,
    /// Columns with a run of at least `ROOF_THICK` solid blocks within reach.
    pub thick: u8,
    /// Columns with solid blocks over the head but no thick run (a building, a bridge, ice).
    pub thin: u8,
    /// Columns with nothing solid up to the top of the world data: open sky.
    pub open: u8,
    /// Columns with nothing solid within reach, the world data going higher (a tall cavern, or a mountain in the
    /// same chunk).
    pub clear: u8,
    /// Columns with water over the head and nothing solid: the player is in open water.
    pub wet: u8,
    /// The lowest solid block over the head, any column.
    pub lowest: Option<i32>,
    /// Of the four directions, how many meet a solid block at the feet or the head within `WALL_REACH` (a room's
    /// walls; a bridge or an overhang has none).
    pub walls: u8,
}

/// How far the wall check looks along each axis, blocks.
pub const WALL_REACH: i32 = 12;

/// Whether a block state is a roof: a solid map colour, not a plant (leaves), water or a clear block (glass, air).
pub fn solid(table: &ColorTable, id: u32) -> bool {
    matches!(table.get(id), Some((mc, _)) if mc != MC_NONE && mc != MC_PLANT && mc != MC_WATER)
}

/// Looks up from the `size` x `size` columns centred on the player's feet `pos`, at most `reach` blocks over the
/// head. `size` 0 looks at nothing. About `size² * reach` block lookups, a few microseconds.
pub fn roof_probe(src: &dyn ChunkSource, table: &ColorTable, pos: [i32; 3], size: u8, reach: i32) -> Roof {
    let mut roof = Roof::default();
    if size == 0 {
        return roof;
    }
    let half = size as i32 / 2;
    for dz in -half..=half {
        for dx in -half..=half {
            let (x, z) = (pos[0] + dx, pos[2] + dz);
            if !src.loaded([x, z]) {
                continue;
            }
            roof.columns += 1;
            let top = src.section_top([x, z]);
            let to = (pos[1] + ROOF_FROM + reach.max(1)).min(top);
            let (mut run, mut any, mut thick) = (0, false, false);
            // The head block too: in water to the eyes counts as wet.
            let mut wet = matches!(table.get(src.block([x, pos[1] + 1, z])), Some((MC_WATER, _)));
            for y in pos[1] + ROOF_FROM..to {
                let id = src.block([x, y, z]);
                if solid(table, id) {
                    if !any {
                        roof.lowest = Some(roof.lowest.map_or(y, |l: i32| l.min(y)));
                    }
                    any = true;
                    run += 1;
                    if run >= ROOF_THICK {
                        thick = true;
                        break;
                    }
                } else {
                    run = 0;
                    wet |= matches!(table.get(id), Some((MC_WATER, _)));
                }
            }
            if thick {
                roof.thick += 1;
            } else if any {
                roof.thin += 1;
            } else {
                if to >= top {
                    roof.open += 1;
                } else {
                    roof.clear += 1;
                }
                roof.wet += wet as u8;
            }
        }
    }
    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
        let hit = (1..=WALL_REACH).any(|d| {
            let (x, z) = (pos[0] + dx * d, pos[2] + dz * d);
            src.loaded([x, z]) && (0..2).any(|h| solid(table, src.block([x, pos[1] + h, z])))
        });
        roof.walls += hit as u8;
    }
    roof
}

/// What the roof and the sky light say together:
/// - bright sky light, open sky over any column, or open water over the head: the surface;
/// - a thick roof over every column: a cave (bright sky light wins first: a cave mouth stays the surface);
/// - a thin roof somewhere and no thick one everywhere: a building, a bridge, ice: the surface;
/// - no roof within reach and dark: a tall cavern; otherwise unsure (keep the layer).
pub fn roof_evidence(roof: Roof, sky: Option<u8>) -> Evidence {
    if roof.columns == 0 {
        return Evidence::Unsure;
    }
    if sky.is_some_and(|s| s >= SKY_SURFACE) || roof.open > 0 || roof.wet > 0 {
        return Evidence::Surface;
    }
    if roof.thick == roof.columns {
        return Evidence::Cave;
    }
    if roof.thin > 0 {
        return Evidence::Surface;
    }
    match sky {
        Some(s) if s <= SKY_CAVE => Evidence::Cave,
        _ => Evidence::Unsure,
    }
}

// ---- indoors (0.3.4) ----
//
// Under a built roof (every column roofed, at least one thinly, nothing open, the ceiling low) the map cuts
// everything from the ceiling up, so the room's floor, walls and furniture show, and the minimap zooms in.

/// Highest ceiling over the feet, blocks, that still counts as a room (higher: a hall, a bridge, a cliff).
pub const INDOOR_MAX_H: i32 = 8;
/// Dwell times in and out of the indoor view, seconds (probes come every 150 ms: in after two, out after
/// eight). Leaving takes that many probes in a row that say "not a room": one odd probe (a window, a door, the edge of the roof under the probe
/// square, a chunk not loaded for a moment) must not zoom the minimap back out.
pub const INDOOR_ENTER_S: f32 = 0.25;
pub const INDOOR_EXIT_S: f32 = 1.2;
/// Indoor setting values (`Settings::indoor`): 0 off, 1 cut only, 2 and 3 cut and zoom in that many times.
pub const INDOOR_MODES: [u8; 4] = [2, 3, 1, 0];

/// Whether the roof check says "a room" with feet at `feet_y`: every column roofed, at least one thinly (a thick
/// roof everywhere is the cave's business), nothing open or wet, the ceiling within `INDOOR_MAX_H`, walls on at
/// least three sides (a door in the fourth; a bridge has none).
pub fn is_room(roof: Roof, feet_y: i32) -> bool {
    let roofed = roof.thin as u16 + roof.thick as u16;
    roof.columns > 0
        && roof.open == 0
        && roof.wet == 0
        && roof.thin > 0
        && roofed == roof.columns as u16
        && roof.walls >= 3
        && roof.lowest.is_some_and(|l| l - feet_y <= INDOOR_MAX_H)
}

/// `is_room` for a player already indoors: looser, so walking past a window, a door or the edge of the roof does
/// not count as leaving (three quarters of the columns roofed, a quarter may be open, two walls, a slightly higher
/// ceiling allowed).
pub fn is_room_keep(roof: Roof, feet_y: i32) -> bool {
    let roofed = roof.thin as u16 + roof.thick as u16;
    roof.columns > 0
        && roof.wet == 0
        && roofed * 4 >= roof.columns as u16 * 3
        && roof.walls >= 2
        && roof.lowest.is_some_and(|l| l - feet_y <= INDOOR_MAX_H + 4)
}

/// The indoor cut: the highest block y the indoor slice shows. Head height (`feet_y + 1`: the floor, the furniture
/// and the walls show, the ceiling and the storeys above do not), kept while the feet stay within a block of it
/// so a step or a slab does not rebake the slice; never at or above the lowest ceiling block over the player.
pub fn indoor_cut(prev: Option<i32>, feet_y: i32, lowest: Option<i32>) -> i32 {
    let head = feet_y + 1;
    let under = |c: i32| lowest.is_none_or(|l| c < l);
    match prev {
        Some(c) if (c - head).abs() <= 1 && under(c) => c,
        _ => head,
    }
}

/// The indoor minimap zoom (bpp multiplier, 1 none) for an indoor setting.
pub fn indoor_zoom(mode: u8) -> f64 {
    if mode >= 2 { 1.0 / mode as f64 } else { 1.0 }
}

/// One frame of easing the zoom multiplier `k` toward `target` over `dt` seconds (about 0.3 s to settle).
pub fn ease_zoom(k: f64, target: f64, dt: f64) -> f64 {
    let a = 1.0 - (-dt.max(0.0) * 10.0).exp();
    let k = k + (target - k) * a;
    if (k - target).abs() < 1e-3 { target } else { k }
}

/// Dwell times for a delay setting in tenths of a second: entering the cave layer takes the delay, leaving it half
/// (at least 0.3 s), so the map comes back to the surface quickly at a cave mouth.
pub fn dwell(delay_tenths: u8) -> (f32, f32) {
    let enter = (delay_tenths as f32 / 10.0).max(0.1);
    (enter, (enter / 2.0).max(0.3))
}

/// Blocks the player may stray out of the cave slice's step before it moves (no rebake at a step's edge).
pub const SLICE_HYST: i32 = 2;

/// The auto cave slice's height: steps of `step` blocks that follow the player, with a margin so walking along a
/// step's edge does not move it back and forth.
#[derive(Clone, Copy, Debug, Default)]
pub struct SliceTracker {
    base: Option<(i32, i32)>,
}

impl SliceTracker {
    /// The slice ceiling for the player's feet at `y`: two blocks over the head at the step's highest spot.
    pub fn top(&mut self, y: i32, step: i32) -> i32 {
        let step = step.max(1);
        let base = match self.base {
            Some((b, s)) if s == step && y >= b - SLICE_HYST && y < b + step + SLICE_HYST => b,
            _ => y.div_euclid(step) * step,
        };
        self.base = Some((base, step));
        base + step + 1
    }
    pub fn reset(&mut self) {
        self.base = None;
    }
}

/// The surface / cave state machine with dwell times.
#[derive(Clone, Debug, Default)]
pub struct Detector {
    pub cave: bool,
    /// Since when (seconds) the evidence has pointed to the other layer.
    since: Option<f32>,
}

impl Detector {
    /// Feeds a probe at time `t` (seconds); returns whether the layer flipped.
    pub fn update(&mut self, t: f32, p: Probe, rule: u8) -> bool {
        self.step(t, evidence(p, rule), ENTER_S, EXIT_S)
    }
    /// Feeds one verdict at time `t`; the layer flips once the other side has held `enter` (into the cave) or
    /// `exit` (out) seconds without a break. Returns whether it flipped.
    pub fn step(&mut self, t: f32, ev: Evidence, enter: f32, exit: f32) -> bool {
        let want = match ev {
            Evidence::Surface => false,
            Evidence::Cave => true,
            Evidence::Unsure => self.cave,
        };
        if want == self.cave {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(t);
        let need = if want { enter } else { exit };
        if t - since >= need {
            self.cave = want;
            self.since = None;
            return true;
        }
        false
    }
    pub fn reset(&mut self, cave: bool) {
        self.cave = cave;
        self.since = None;
    }
}

/// Which layer is shown for a mode: `Some(top)` for the cave slice (its key, the ceiling cut), `None` for the
/// surface. The slice moves in steps of 8 blocks so the cave layer is not rebaked at every step up or down.
pub fn slice(mode: LayerMode, auto_cave: bool, player_y: i32, fixed_y: i32) -> Option<i32> {
    match mode {
        LayerMode::Surface => None,
        LayerMode::FixedY => Some(fixed_y),
        LayerMode::Cave => Some(bucket_top(player_y)),
        LayerMode::Auto => auto_cave.then(|| bucket_top(player_y)),
    }
}

/// `slice` with the auto / cave ceiling already chosen (`SliceTracker`).
pub fn slice_at(mode: LayerMode, auto_cave: bool, top: i32, fixed_y: i32) -> Option<i32> {
    match mode {
        LayerMode::Surface => None,
        LayerMode::FixedY => Some(fixed_y),
        LayerMode::Cave => Some(top),
        LayerMode::Auto => auto_cave.then_some(top),
    }
}

/// The ceiling of the slice for a player at `y`: two blocks over the head of the highest spot of its 8-block bucket.
pub fn bucket_top(y: i32) -> i32 {
    y.div_euclid(8) * 8 + 9
}

/// The window the cave slice looks through: blocks above `top` are cut off (the ceiling), the floor search stops
/// `depth` blocks under it.
pub fn cave_window(top: i32, depth: i32) -> (i32, i32) {
    (top, top - depth.max(1))
}

/// Cross-fade progress (0 old image, 1 new) `dt` seconds after a switch.
pub fn fade(dt: f32) -> f32 {
    let x = (dt / FADE_S).clamp(0.0, 1.0);
    x * x * (3.0 - 2.0 * x)
}

/// Longest the last image of the old layer is held over a new layer that is still being baked (seconds), before
/// the cross-fade starts anyway.
pub const HOLD_MAX_S: f32 = 1.5;

/// Blends `old`, an image (`w` x `h` RGBA) drawn under view `ov`, into `img` drawn under view `v` at fade
/// progress `k` (0 old, 1 `img`), with the old image moved to where its ground is now: it turns and slides with
/// the camera instead of standing still over the live map. Pixels the old image did not cover keep `img`.
pub fn crossfade_warped(img: &mut [u8], w: usize, h: usize, v: &super::view::View, old: &[u8], ov: &super::view::View, k: f32) {
    if k >= 1.0 || old.len() < w * h * 4 || img.len() < w * h * 4 || w == 0 || h == 0 {
        return;
    }
    let wk = (k.clamp(0.0, 1.0) * 256.0) as u32;
    // New pixel centre -> old image position (pixel units, 0 = the first pixel's centre): affine in (x, y).
    let map = |x: f64, y: f64| {
        let p = v.to_world(w, h, x + 0.5, y + 0.5);
        let o = ov.to_image(w, h, p[0], p[1]);
        [o[0] - 0.5, o[1] - 0.5]
    };
    let o0 = map(0.0, 0.0);
    let (ox, oy) = (map(1.0, 0.0), map(0.0, 1.0));
    let (dx, dy) = ([ox[0] - o0[0], ox[1] - o0[1]], [oy[0] - o0[0], oy[1] - o0[1]]);
    let (wf, hf) = ((w - 1) as f64, (h - 1) as f64);
    for y in 0..h {
        let row = &mut img[y * w * 4..(y + 1) * w * 4];
        let (mut u, mut t) = (o0[0] + dy[0] * y as f64, o0[1] + dy[1] * y as f64);
        for px in row.chunks_exact_mut(4) {
            let (fu, ft) = (u, t);
            u += dx[0];
            t += dx[1];
            if !(-0.5..=wf + 0.5).contains(&fu) || !(-0.5..=hf + 0.5).contains(&ft) {
                continue;
            }
            let (cu, ct) = (fu.clamp(0.0, wf), ft.clamp(0.0, hf));
            let (x0, y0) = (cu.floor() as usize, ct.floor() as usize);
            let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
            let (ax, ay) = (((cu - x0 as f64) * 256.0) as u32, ((ct - y0 as f64) * 256.0) as u32);
            let at = |x: usize, y: usize, c: usize| old[(y * w + x) * 4 + c] as u32;
            for c in 0..4 {
                let top = at(x0, y0, c) * (256 - ax) + at(x1, y0, c) * ax;
                let bot = at(x0, y1, c) * (256 - ax) + at(x1, y1, c) * ax;
                let o = (top * (256 - ay) + bot * ay) >> 16;
                px[c] = ((px[c] as u32 * wk + o * (256 - wk)) >> 8) as u8;
            }
        }
    }
}

/// Blends `old` into `img` (both RGBA, same size) at fade progress `k` (1 keeps `img`).
pub fn crossfade(img: &mut [u8], old: &[u8], k: f32) {
    if k >= 1.0 {
        return;
    }
    let w = (k.clamp(0.0, 1.0) * 256.0) as u32;
    for (d, &o) in img.iter_mut().zip(old) {
        *d = ((*d as u32 * w + o as u32 * (256 - w)) >> 8) as u8;
    }
}
