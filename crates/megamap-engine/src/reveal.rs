//! The fog front (0.3.5): newly explored ground comes out of the fog behind a front that moves away from the
//! player and from the ground seen before, instead of every chunk fading out on its own timer.
//!
//! Every block of a fresh chunk holds a distance `d` (blocks): how far the front has to travel, over explored
//! ground, to get there from where it starts — the ground seen before (a chunk explored and not fresh) and the
//! player (straight-line, so ground with nothing seen around it, as after a teleport, opens up around the player).
//! The front is at `r`, which grows with an ease-out towards the farthest fresh block; a block is out of the fog
//! once `r` has passed it (`amount`: a ramp `SOFT` blocks wide behind the front). New chunks rebase the field
//! (`d -= r`, `r = 0`) and propagate from what is around them, so the front never jumps back and has no steps at
//! chunk edges. Unexplored ground is never in the field: the fog over it stays whatever the front does.

use std::collections::{HashMap, HashSet};

/// Width of the ramp behind the front, blocks (the cloud-less fog fades over it; the cloud pass takes its middle).
pub const SOFT: f32 = 6.0;
/// The widest ramp a far zoom gets, blocks.
pub const SOFT_MAX: f32 = 32.0;

/// The ramp width for a view at `bpp` blocks a pixel: `SOFT`, or three pixels when wider, up to `SOFT_MAX`.
pub fn soft_for(bpp: f64) -> f32 {
    SOFT.max(3.0 * bpp as f32).min(SOFT_MAX)
}
/// A fresh chunk further than this from the player when the field is rebuilt (the place left by a teleport) comes
/// out at once: the field stays a few hundred blocks wide.
pub const DROP_DIST: f32 = 768.0;
const DIAG: f32 = std::f32::consts::SQRT_2;

/// Front speed: a floor in blocks per second and the ease-out time constant (seconds) of the catch-up towards the
/// farthest fresh block.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Speed {
    pub base: f32,
    pub tau: f32,
}

/// The settings' speed levels (`Settings::reveal`): 0 off (new ground shows at once), 1 slow, 2 normal, 3 fast.
pub fn speed(level: u8) -> Option<Speed> {
    match level {
        0 => None,
        1 => Some(Speed { base: 1.5, tau: 3.5 }),
        2 => Some(Speed { base: 3.0, tau: 2.2 }),
        _ => Some(Speed { base: 6.0, tau: 1.2 }),
    }
}

struct Fresh {
    /// `d` per block, index `z * 16 + x` (chunk-local).
    d: Box<[f32; 256]>,
    /// The largest `d` (the chunk is done once the front is `SOFT` past it).
    max: f32,
}

#[derive(Default)]
pub struct Front {
    chunks: HashMap<(i32, i32), Fresh>,
    /// Explored since the last `step`: hidden until the field takes them in.
    pending: HashSet<(i32, i32)>,
    /// The front, in the units of `d`.
    pub r: f32,
    /// Seconds since the field last took pending chunks in (a big field is rebuilt at most `REBUILD_SECS` apart).
    wait: f32,
}

/// A field of more chunks than `REBUILD_BIG` takes pending chunks in at most this often (seconds): after a
/// teleport the whole loaded square arrives over a few seconds, a frame or so per chunk.
const REBUILD_SECS: f32 = 0.1;
const REBUILD_BIG: usize = 64;

impl Front {
    /// A newly explored chunk (hidden from now on until the front passes it).
    pub fn add(&mut self, cx: i32, cz: i32) {
        if !self.chunks.contains_key(&(cx, cz)) {
            self.pending.insert((cx, cz));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty() && self.pending.is_empty()
    }

    /// Lets everything out at once.
    pub fn clear(&mut self) {
        self.chunks.clear();
        self.pending.clear();
        self.r = 0.0;
    }

    pub fn len(&self) -> usize {
        self.chunks.len() + self.pending.len()
    }

    /// How far block (`x`, `z`) is out of the fog: 0 hidden .. 256 clear (every block not fresh). `soft`: the ramp
    /// width in blocks (at least `SOFT`; a zoomed-out map passes more so the ramp spans a few pixels).
    #[inline]
    pub fn amount(&self, x: i32, z: i32, soft: f32) -> u32 {
        if self.is_empty() {
            return 256;
        }
        let key = (x >> 4, z >> 4);
        match self.chunks.get(&key) {
            Some(f) => {
                let e = self.r - f.d[((z & 15) * 16 + (x & 15)) as usize];
                if e <= 0.0 {
                    0
                } else if e >= soft {
                    256
                } else {
                    let t = e / soft;
                    (t * t * (3.0 - 2.0 * t) * 256.0) as u32
                }
            }
            None if self.pending.contains(&key) => 0,
            None => 256,
        }
    }

    /// The farthest the front still has to go (0 when idle).
    pub fn backlog(&self) -> f32 {
        self.chunks.values().map(|f| f.max).fold(f32::NEG_INFINITY, f32::max).max(self.r) - self.r
    }

    /// Advances the front by `dt` seconds with the player at `player` (blocks); `explored(cx, cz)` tells the
    /// ground seen before; `soft` the widest ramp a view uses (a chunk is let go once the front is that far past
    /// it). Returns whether anything is still coming out of the fog (the map then redraws).
    pub fn step(&mut self, dt: f32, player: [f64; 2], sp: Speed, soft: f32, explored: impl Fn(i32, i32) -> bool) -> bool {
        self.wait += dt.max(0.0);
        if !self.pending.is_empty() && (self.wait >= REBUILD_SECS || self.chunks.len() < REBUILD_BIG) {
            self.wait = 0.0;
            self.rebuild(player, &explored);
        }
        if self.chunks.is_empty() {
            self.r = 0.0;
            return !self.pending.is_empty();
        }
        let back = self.backlog();
        self.r += dt.clamp(0.0, 0.25) * (sp.base + back / sp.tau.max(0.05));
        let r = self.r;
        let soft = soft.max(SOFT);
        self.chunks.retain(|_, f| f.max > r - soft);
        if self.chunks.is_empty() {
            self.r = 0.0;
        }
        true
    }

    /// Takes the pending chunks into the field: rebases it to `r = 0`, seeds the new blocks with their straight
    /// distance to the player, and relaxes everything over explored ground (two chamfer sweeps, twice) from the
    /// ground seen before and from the blocks already in the field. A source the front has passed (below 0) passes
    /// 0 on: new ground next to it starts at the front instead of popping out at once.
    fn rebuild(&mut self, player: [f64; 2], explored: &impl Fn(i32, i32) -> bool) {
        let r = self.r;
        for f in self.chunks.values_mut() {
            f.d.iter_mut().for_each(|d| *d -= r);
            f.max -= r;
        }
        self.r = 0.0;
        let (px, pz) = (player[0] as f32, player[1] as f32);
        for (cx, cz) in self.pending.drain() {
            let mut d = Box::new([0f32; 256]);
            for (i, v) in d.iter_mut().enumerate() {
                let (x, z) = ((cx * 16 + (i & 15) as i32) as f32 + 0.5, (cz * 16 + (i >> 4) as i32) as f32 + 0.5);
                *v = ((x - px).powi(2) + (z - pz).powi(2)).sqrt();
            }
            self.chunks.insert((cx, cz), Fresh { d, max: 0.0 });
        }
        // The place a teleport left behind: out at once.
        let (pcx, pcz) = ((px / 16.0).floor() as i32, (pz / 16.0).floor() as i32);
        let lim = (DROP_DIST / 16.0) as i32;
        self.chunks.retain(|&(cx, cz), _| (cx - pcx).abs() <= lim && (cz - pcz).abs() <= lim);
        if self.chunks.is_empty() {
            return;
        }
        let (mut x0, mut z0, mut x1, mut z1) = (i32::MAX, i32::MAX, i32::MIN, i32::MIN);
        for &(cx, cz) in self.chunks.keys() {
            (x0, z0, x1, z1) = (x0.min(cx), z0.min(cz), x1.max(cx), z1.max(cz));
        }
        // One chunk of margin: the ground seen around the fresh chunks seeds the front.
        let (x0, z0, x1, z1) = (x0 - 1, z0 - 1, x1 + 1, z1 + 1);
        let (cw, ch) = ((x1 - x0 + 1) as usize, (z1 - z0 + 1) as usize);
        let (w, h) = (cw * 16, ch * 16);
        // Cells: the distance; obstacles (unexplored) stay infinite, seeds (seen before) 0, fresh blocks relax.
        let mut g = vec![f32::INFINITY; w * h];
        let mut free = vec![false; w * h];
        for j in 0..ch {
            for i in 0..cw {
                let key = (x0 + i as i32, z0 + j as i32);
                let fresh = self.chunks.get(&key);
                let seed = fresh.is_none() && explored(key.0, key.1);
                for b in 0..256 {
                    let at = (j * 16 + (b >> 4)) * w + i * 16 + (b & 15);
                    match fresh {
                        Some(f) => {
                            g[at] = f.d[b];
                            free[at] = true;
                        }
                        None if seed => g[at] = 0.0,
                        None => {}
                    }
                }
            }
        }
        for _ in 0..2 {
            for y in 0..h {
                for x in 0..w {
                    let at = y * w + x;
                    if !free[at] {
                        continue;
                    }
                    let mut v = g[at];
                    if x > 0 {
                        v = v.min(g[at - 1].max(0.0) + 1.0);
                    }
                    if y > 0 {
                        v = v.min(g[at - w].max(0.0) + 1.0);
                        if x > 0 {
                            v = v.min(g[at - w - 1].max(0.0) + DIAG);
                        }
                        if x + 1 < w {
                            v = v.min(g[at - w + 1].max(0.0) + DIAG);
                        }
                    }
                    g[at] = v;
                }
            }
            for y in (0..h).rev() {
                for x in (0..w).rev() {
                    let at = y * w + x;
                    if !free[at] {
                        continue;
                    }
                    let mut v = g[at];
                    if x + 1 < w {
                        v = v.min(g[at + 1].max(0.0) + 1.0);
                    }
                    if y + 1 < h {
                        v = v.min(g[at + w].max(0.0) + 1.0);
                        if x + 1 < w {
                            v = v.min(g[at + w + 1].max(0.0) + DIAG);
                        }
                        if x > 0 {
                            v = v.min(g[at + w - 1].max(0.0) + DIAG);
                        }
                    }
                    g[at] = v;
                }
            }
        }
        for (&(cx, cz), f) in self.chunks.iter_mut() {
            let (i, j) = ((cx - x0) as usize, (cz - z0) as usize);
            let mut max = f32::NEG_INFINITY;
            for b in 0..256 {
                let v = g[(j * 16 + (b >> 4)) * w + i * 16 + (b & 15)];
                f.d[b] = v;
                max = max.max(v);
            }
            f.max = max;
        }
    }
}
