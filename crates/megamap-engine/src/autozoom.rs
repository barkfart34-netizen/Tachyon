//! Minimap auto-zoom: picks the scale from how far the known map data reaches around the player.
//!
//! The target is the scale at which the minimap's half side covers the known-data radius (so the map is filled
//! with data and not with the unexplored backdrop), clamped to the player's "max zoom in" / "max zoom out".
//! It is quantised to half-octave levels with hysteresis (a level changes only when the target leaves it by
//! more than `MARGIN`, and not sooner than `DWELL` seconds after the last change) and the shown scale eases to
//! the level in log space, so walking along a data edge does not make the map pump.
//!
//! The manual zoom (Z / wheel) acts as a multiplier on the automatic scale while auto-zoom is on: zoom index 1
//! (1x) is neutral, each step doubles or halves it; the result is still clamped to the limits.

/// Levels per octave (2 = steps of sqrt 2).
pub const STEPS_PER_OCTAVE: f64 = 2.0;
/// How far (in levels) the target must pass a level boundary before the level changes.
pub const MARGIN: f64 = 0.5;
/// Minimum seconds between level changes.
pub const DWELL: f64 = 1.5;
/// Easing time constant of the shown scale, seconds.
pub const TAU: f64 = 0.35;

#[derive(Clone, Debug)]
pub struct AutoZoom {
    /// Current level: log2(bpp) * STEPS_PER_OCTAVE, an integer once settled.
    pub level: i32,
    /// Shown blocks per minimap pixel.
    pub bpp: f64,
    /// Seconds since the last level change.
    since: f64,
    started: bool,
}

impl Default for AutoZoom {
    fn default() -> Self {
        AutoZoom { level: 0, bpp: 1.0, since: f64::INFINITY, started: false }
    }
}

/// The unclamped target scale: the minimap half side (`px / 2` pixels) covers `known_radius` blocks.
pub fn target_bpp(known_radius: f64, px: u32) -> f64 {
    let half = (px.max(2) as f64) / 2.0;
    (known_radius.max(1.0) / half).max(1e-3)
}

fn level_of(bpp: f64) -> f64 {
    bpp.log2() * STEPS_PER_OCTAVE
}

fn bpp_of(level: f64) -> f64 {
    (level / STEPS_PER_OCTAVE).exp2()
}

impl AutoZoom {
    /// One frame: `known_radius` in blocks, `px` the minimap side, `near` / `far` the bpp limits (near < far),
    /// `mult` the manual multiplier, `dt` seconds. Returns the bpp to draw with.
    pub fn update(&mut self, known_radius: f64, px: u32, near: f64, far: f64, mult: f64, dt: f64) -> f64 {
        let (near, far) = if near <= far { (near, far) } else { (far, near) };
        let t = (target_bpp(known_radius, px) * mult).clamp(near, far);
        let tl = level_of(t);
        let (lo, hi) = (level_of(near).ceil() as i32, level_of(far).floor() as i32);
        if !self.started {
            self.level = (tl.round() as i32).clamp(lo.min(hi), hi.max(lo));
            self.bpp = bpp_of(self.level as f64).clamp(near, far);
            self.started = true;
            self.since = 0.0;
            return self.bpp;
        }
        self.since += dt.clamp(0.0, 1.0);
        let cur = self.level as f64;
        // At a limit the target cannot pass the boundary by the margin, so reaching the limit is enough.
        let at_limit = (tl <= lo as f64 + 1e-9 || tl >= hi as f64 - 1e-9) && tl.round() as i32 != self.level;
        if ((tl - cur).abs() > 0.5 + MARGIN || at_limit) && self.since >= DWELL {
            self.level = tl.round() as i32;
            self.since = 0.0;
        }
        self.level = self.level.clamp(lo.min(hi), hi.max(lo));
        let goal = bpp_of(self.level as f64).clamp(near, far);
        let k = 1.0 - (-dt.max(0.0) / TAU).exp();
        let d = level_of(goal) - level_of(self.bpp);
        // Settle exactly once close, so the image key stops changing.
        self.bpp = if d.abs() < 2e-3 { goal } else { bpp_of(level_of(self.bpp) + d * k) };
        self.bpp
    }

    /// Forget the state (auto-zoom switched on again, dimension change): the next update snaps.
    pub fn reset(&mut self) {
        self.started = false;
    }
}

/// The known-data radius around the player from a ring scan: `known(dx, dz)` says whether the chunk at that
/// offset (in chunks) has map data. Returns the radius (blocks) of the largest square ring that is still at least
/// `fill` known, scanning out to `max_chunks`.
pub fn known_radius(max_chunks: i32, fill: f64, mut known: impl FnMut(i32, i32) -> bool) -> f64 {
    let mut best = 0;
    for r in 1..=max_chunks.max(1) {
        let mut have = 0;
        let mut total = 0;
        // Sample the ring sparsely past 16 chunks (every other chunk) to keep the scan cheap.
        let step = if r > 16 { 2 } else { 1 };
        let mut i = -r;
        while i <= r {
            for (dx, dz) in [(i, -r), (i, r), (-r, i), (r, i)] {
                total += 1;
                have += known(dx, dz) as i32;
            }
            i += step;
        }
        if (have as f64) < fill * total as f64 {
            break;
        }
        best = r;
    }
    (best as f64 + 0.5) * 16.0
}
