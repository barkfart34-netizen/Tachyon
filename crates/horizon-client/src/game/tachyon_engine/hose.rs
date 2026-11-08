//! Create's hose pulley (HosePulleyBlock, HosePulleyBlockEntity, HosePulleyRenderer,
//! FluidDrainingBehaviour, FluidFillingBehaviour; MIT, THIRD_PARTY_NOTICES.md) on
//! create:hose_pulley. Turned (4 stress per RPM) its hose goes down `RPM / 512` blocks a tick, or up
//! turned the other way, stopping above the first block a fluid can't wash away; stopped, it is a
//! fluid end for the pumps (`fluids.rs`) through its pipe side: pulled from, it takes the highest
//! source of the fluid body its hose is in, the farthest of them first, within 128 blocks
//! (`hosePulleyRange`), water without end once the body is 10000 blocks (`hosePulleyBlockThreshold`);
//! pushed into, it pours a bucket's source where the hose reaches, the lowest free place first.
//!
//! Not as Create: no fluids but water and lava placed, a body's blocks are counted again for each
//! bucket instead of kept, no infinite-lava rule, the hose's coil doesn't scroll.
use super::fluids::{BUCKET, Tank};
use super::kinetic::{Dir, Part};
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

/// `hosePulleyRange`: how far from its hose a pulley takes or pours.
pub(super) const RANGE: i32 = 128;
/// `hosePulleyBlockThreshold`: a body of water this large has no end.
pub(super) const THRESHOLD: usize = 10_000;

/// `Direction.getClockWise` (seen from above).
pub(super) fn clockwise(d: Dir) -> Dir {
    match d {
        Dir::North => Dir::East,
        Dir::East => Dir::South,
        Dir::South => Dir::West,
        Dir::West => Dir::North,
        _ => d,
    }
}

pub(super) fn counter_clockwise(d: Dir) -> Dir {
    clockwise(clockwise(clockwise(d)))
}

/// `Direction.fromYRot`: the way a player at `yaw` looks, level.
pub(super) fn horizontal(yaw: f32) -> Dir {
    match ((f64::from(yaw) / 90.0 + 0.5).floor() as i32).rem_euclid(4) {
        0 => Dir::South,
        1 => Dir::West,
        2 => Dir::North,
        _ => Dir::East,
    }
}

/// A hose: how far down it reaches (blocks), and whether it is on its way.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Hose {
    pub offset: f32,
    pub moving: bool,
}

/// `HosePulleyBlockEntity.tick`: the hose a tick on at `speed`, `open(n)` telling whether the block
/// `n` under the pulley may take it; stopped, it settles on a whole block.
pub(super) fn step(hose: Hose, speed: f32, open: impl Fn(i32) -> bool) -> Hose {
    if speed == 0.0 {
        return Hose { offset: hose.offset.round(), moving: false };
    }
    let mut offset = hose.offset + speed / 512.0;
    let mut moving = true;
    if offset < 0.0 {
        offset = 0.0;
        moving = false;
    }
    if !open(offset.ceil() as i32) {
        offset = offset.trunc();
        moving = false;
    }
    Hose { offset, moving }
}

/// What a fluid end finds in a place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Cell {
    Source(&'static str),
    Flowing(&'static str),
    /// Air or something a fluid washes away.
    Free,
    Blocked,
}

impl Cell {
    fn fluid(self) -> Option<&'static str> {
        match self {
            Cell::Source(f) | Cell::Flowing(f) => Some(f),
            _ => None,
        }
    }
}

fn add(a: [i32; 3], d: Dir) -> [i32; 3] {
    let o = d.offset();
    [a[0] + o[0], a[1] + o[1], a[2] + o[2]]
}

fn distance(a: [i32; 3], b: [i32; 3]) -> i64 {
    (0..3).map(|i| i64::from(a[i] - b[i]).pow(2)).sum()
}

/// FluidDrainingBehaviour: the fluid of the body at `root` and the source to take from it, its
/// highest and of those the farthest from the hose; no source to take when it is water without
/// end. None with no fluid there.
pub(super) fn drain(root: [i32; 3], cell: impl Fn([i32; 3]) -> Cell) -> Option<(&'static str, Option<[i32; 3]>)> {
    let fluid = cell(root).fluid()?;
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut best: Option<[i32; 3]> = None;
    while let Some(p) = queue.pop_front() {
        let here = cell(p);
        if here == Cell::Source(fluid)
            && best.is_none_or(|b| (p[1], distance(p, root)) > (b[1], distance(b, root)))
        {
            best = Some(p);
        }
        if seen.len() >= THRESHOLD {
            if fluid == "minecraft:water" {
                return Some((fluid, None));
            }
            continue;
        }
        for d in Dir::ALL {
            let q = add(p, d);
            if distance(q, root) > i64::from(RANGE * RANGE) || seen.contains(&q) {
                continue;
            }
            if cell(q).fluid() == Some(fluid) {
                seen.insert(q);
                queue.push_back(q);
            }
        }
    }
    best.map(|b| (fluid, Some(b)))
}

/// FluidFillingBehaviour: where a bucket of `fluid` poured at `root` goes: the lowest free place
/// reached through free places and the same fluid, no higher than `root`, the nearest of them.
pub(super) fn fill(root: [i32; 3], fluid: &str, cell: impl Fn([i32; 3]) -> Cell) -> Option<[i32; 3]> {
    let passes = |c: Cell| c == Cell::Free || c.fluid() == Some(fluid);
    let takes = |c: Cell| c == Cell::Free || c == Cell::Flowing(fluid_static(fluid));
    if !passes(cell(root)) {
        return None;
    }
    let mut seen = HashSet::from([root]);
    let mut queue = VecDeque::from([root]);
    let mut best: Option<[i32; 3]> = None;
    while let Some(p) = queue.pop_front() {
        if takes(cell(p)) && best.is_none_or(|b| (p[1], distance(p, root)) < (b[1], distance(b, root))) {
            best = Some(p);
        }
        if seen.len() >= THRESHOLD {
            continue;
        }
        for d in Dir::ALL {
            let q = add(p, d);
            if q[1] > root[1] || distance(q, root) > i64::from(RANGE * RANGE) || seen.contains(&q) {
                continue;
            }
            if passes(cell(q)) {
                seen.insert(q);
                queue.push_back(q);
            }
        }
    }
    best
}

fn fluid_static(fluid: &str) -> &'static str {
    match fluid {
        "minecraft:lava" => "minecraft:lava",
        _ => "minecraft:water",
    }
}

impl super::super::Game<'_> {
    /// What a hose finds at `p`.
    pub(super) fn tachyon_cell(&self, p: [i32; 3]) -> Cell {
        if !self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4)) {
            return Cell::Blocked;
        }
        let state = self.world.block(p[0], p[1], p[2]);
        let Some(s) = self.pack.assets.registry.state(state) else {
            return Cell::Blocked;
        };
        let source = s.properties.get("level").is_none_or(|l| l == "0");
        let fluid = match s.name.as_str() {
            "minecraft:water" => "minecraft:water",
            "minecraft:lava" => "minecraft:lava",
            _ if self.res.facts().get(state).replaceable => return Cell::Free,
            _ => return Cell::Blocked,
        };
        if source { Cell::Source(fluid) } else { Cell::Flowing(fluid) }
    }

    /// The owner's hoses a tick, each pulley's by its speed.
    pub(super) fn tachyon_hoses(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let pulleys: Vec<([i32; 3], f32, Hose)> = super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return Vec::new() };
            let parts = &o.index.parts;
            o.hoses.retain(|p, _| matches!(parts.get(p), Some(Part::HosePulley(_))));
            (parts.iter())
                .filter(|(_, part)| matches!(part, Part::HosePulley(_)))
                .map(|(p, _)| {
                    let speed = o.shown.speeds.get(p).map_or(0.0, |s| s[0]);
                    (*p, speed, o.hoses.get(p).copied().unwrap_or_default())
                })
                .collect()
        });
        for (p, speed, hose) in pulleys {
            let next = step(hose, speed, |n| n > 0 && self.tachyon_cell([p[0], p[1] - n, p[2]]) != Cell::Blocked);
            if next != hose {
                super::OWNERS.with_borrow_mut(|owners| {
                    if let Some(o) = owners.get_mut(dimension) {
                        o.hoses.insert(p, next);
                        o.mills_changed |= next.offset != hose.offset && !next.moving;
                    }
                });
            }
        }
    }

    /// Where a stopped hose of the pulley at `p` reaches, if it is down.
    fn tachyon_hose_root(&self, p: [i32; 3]) -> Option<[i32; 3]> {
        let dimension = self.native_dimension();
        let hose = super::OWNERS.with_borrow(|owners| owners.get(dimension).and_then(|o| o.hoses.get(&p)).copied())?;
        let down = hose.offset.ceil() as i32;
        (!hose.moving && down >= 1).then_some([p[0], p[1] - down, p[2]])
    }

    /// A pulley's fluid for a pump: what it holds, else a bucket from the body its hose is in.
    pub(super) fn tachyon_hose_peek(&mut self, p: [i32; 3]) -> Option<(String, u32)> {
        let dimension = self.native_dimension();
        let held = super::OWNERS.with_borrow(|owners| {
            owners.get(dimension).and_then(|o| o.open_ends.get(&p)).filter(|t| t.amount > 0).cloned()
        });
        if let Some(t) = held {
            return Some((t.fluid, t.amount));
        }
        let root = self.tachyon_hose_root(p)?;
        let (fluid, take) = drain(root, |q| self.tachyon_cell(q))?;
        if let Some(q) = take {
            let air = self.pack.assets.registry.default_state("minecraft:air")?;
            if !self.native_set_block(dimension, q, air, 3) {
                return None;
            }
        }
        super::OWNERS.with_borrow_mut(|owners| {
            let o = owners.get_mut(dimension)?;
            o.open_ends.insert(p, Tank { fluid: fluid.to_owned(), amount: BUCKET });
            Some((fluid.to_owned(), BUCKET))
        })
    }

    /// A pump's fluid into a pulley: held until a bucket's worth, then poured where the hose reaches.
    pub(super) fn tachyon_hose_offer(&mut self, p: [i32; 3], fluid: &str, amount: u32) -> u32 {
        let dimension = self.native_dimension();
        if !matches!(fluid, "minecraft:water" | "minecraft:lava") {
            return 0;
        }
        let Some(root) = self.tachyon_hose_root(p) else { return 0 };
        let n = super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return 0 };
            o.open_ends.entry(p).or_default().fill(fluid, amount, BUCKET)
        });
        let full = super::OWNERS.with_borrow(|owners| {
            owners.get(dimension).and_then(|o| o.open_ends.get(&p)).is_some_and(|t| t.amount >= BUCKET && t.fluid == fluid)
        });
        if full
            && let Some(at) = fill(root, fluid, |q| self.tachyon_cell(q))
            && let Some(source) = self.pack.assets.registry.default_state(fluid)
            && self.native_set_block(dimension, at, source, 3)
        {
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) {
                    o.open_ends.remove(&p);
                }
            });
        }
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn a_hose_goes_down_by_its_speed_and_stops_over_the_ground() {
        let open = |n: i32| n <= 3;
        let mut h = Hose::default();
        for _ in 0..20 {
            h = step(h, 256.0, open);
        }
        assert_eq!(h, Hose { offset: 3.0, moving: false });
        let up = step(h, -256.0, open);
        assert_eq!(up, Hose { offset: 2.5, moving: true });
        assert_eq!(step(Hose { offset: 0.2, moving: true }, -256.0, open), Hose::default());
        assert_eq!(step(Hose { offset: 1.6, moving: true }, 0.0, open), Hose { offset: 2.0, moving: false });
        assert_eq!(clockwise(Dir::North), Dir::East);
        assert_eq!(counter_clockwise(Dir::North), Dir::West);
        assert_eq!(horizontal(0.0), Dir::South);
        assert_eq!(horizontal(90.0), Dir::West);
    }

    #[test]
    fn a_hose_drains_the_top_farthest_first_and_fills_the_bottom_nearest_first() {
        // A pool 3 wide, 2 deep, under the root at its middle's top.
        let mut world: HashMap<[i32; 3], Cell> = HashMap::new();
        for x in -1..=1 {
            for y in 0..2 {
                world.insert([x, y, 0], Cell::Source("minecraft:water"));
            }
        }
        let cell = |p: [i32; 3]| world.get(&p).copied().unwrap_or(Cell::Blocked);
        let (fluid, take) = drain([0, 1, 0], cell).unwrap();
        assert_eq!(fluid, "minecraft:water");
        assert!(matches!(take, Some([-1 | 1, 1, 0])));
        assert!(drain([5, 5, 5], cell).is_none());
        // An empty pit: filled from its bottom, near the hose first.
        let mut pit: HashMap<[i32; 3], Cell> = HashMap::new();
        for x in -1..=1 {
            for y in 0..3 {
                pit.insert([x, y, 0], Cell::Free);
            }
        }
        let cell = |p: [i32; 3]| pit.get(&p).copied().unwrap_or(Cell::Blocked);
        assert_eq!(fill([0, 2, 0], "minecraft:water", cell), Some([0, 0, 0]));
        pit.insert([0, 0, 0], Cell::Source("minecraft:water"));
        let cell = |p: [i32; 3]| pit.get(&p).copied().unwrap_or(Cell::Blocked);
        assert!(matches!(fill([0, 2, 0], "minecraft:water", cell), Some([-1 | 1, 0, 0])));
        assert_eq!(fill([0, 2, 0], "minecraft:water", |_| Cell::Blocked), None);
    }
}
