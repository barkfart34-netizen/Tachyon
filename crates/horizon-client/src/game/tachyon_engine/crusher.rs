//! Create's crushing wheels, turned by Kinetica's engine, working as
//! Create's (`CrushingWheelBlock`, `CrushingWheelControllerBlock`,
//! `CrushingWheelControllerBlockEntity`; Create's code is MIT-licensed,
//! THIRD_PARTY_NOTICES.md): two wheels on parallel shafts with a block of
//! room between them make a gap there. While they turn against each other
//! the gap takes in a dropped item that comes into it, a whole stack, and
//! crushes it by Create's crushing recipes (else its milling ones), slower
//! the bigger the stack; what it made comes out of the gap the way their
//! rims go there, and passes the gap. A stack no recipe crushes is lost, as
//! in Create. Stopped, the gap keeps what it holds; with its wheels gone it
//! drops it.
//!
//! Unlike Create's, no block fills the gap: the owner keeps the stack it
//! took in (saved with the machines), the items waiting for it are held in
//! the gap, a stack with components passes (the owner keeps items by name
//! and count), and the wheels don't hurt mobs yet.
use super::super::Game;
use super::create_recipes::Recipes;
use super::kinetic::{Axis, Dir, Part, Speeds};
use super::{OWNERS, Owner, add, positive};
use horizon_client::entities::ItemEntity;
use horizon_client::items::ItemStack;
use horizon_core::random::RandomSource;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The countdown of a stack no recipe crushes (Create's).
const NO_RECIPE: f32 = 100.0;
/// Under this the stack is crushed and what it made comes out (Create's
/// applies the recipe then, emptying its timer, and puts it out the next
/// tick).
const DONE: f32 = 20.0;

/// A turning gap: the way it sends out what it crushed, and how fast it
/// crushes (Create's: its wheel's RPM / 50, times 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Gap {
    pub out: Dir,
    pub speed: f32,
}

/// What a gap holds: the stack it took in, and what is left of its
/// countdown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Inside {
    pub item: String,
    pub count: u32,
    pub left: f32,
}

/// What came out of a gap: the gap, the stacks, where, and how fast.
type Out = ([i32; 3], Vec<ItemStack>, [f64; 3], [f64; 3]);

/// The wheels round a gap at `gap`: their axis, and the side of the gap the
/// first of them is on in Create's order of directions (the other is
/// across). None when no two wheels on one axis face each other over it.
pub(super) fn pair_round(parts: &HashMap<[i32; 3], Part>, gap: [i32; 3]) -> Option<(Axis, Dir)> {
    Dir::ALL.into_iter().find_map(|d| {
        let Some(Part::CrushingWheel(axis)) = parts.get(&add(gap, d.offset())).copied() else {
            return None;
        };
        let across = parts.get(&add(gap, d.opposite().offset()));
        (d.axis() != axis && across == Some(&Part::CrushingWheel(axis))).then_some((axis, d))
    })
}

/// The gaps between wheels, by position: each turning (Some) or not.
pub(super) fn gaps(parts: &HashMap<[i32; 3], Part>, speeds: &Speeds) -> HashMap<[i32; 3], Option<Gap>> {
    let mut gaps = HashMap::new();
    for (p, part) in parts {
        let Part::CrushingWheel(axis) = *part else {
            continue;
        };
        for d in Dir::ALL.into_iter().filter(|d| d.axis() != axis) {
            let gap = add(*p, d.offset());
            if gaps.contains_key(&gap) {
                continue;
            }
            if let Some(pair) = pair_round(parts, gap) {
                gaps.insert(gap, turning(gap, pair, speeds));
            }
        }
    }
    gaps
}

/// How a gap with wheels along `axis` on its side `d` and across works, as
/// Create's controller: only while they turn against each other, at the
/// speed of the wheel on side `d`, the way its rim goes in the gap.
fn turning(gap: [i32; 3], (axis, d): (Axis, Dir), speeds: &Speeds) -> Option<Gap> {
    let speed = |p: [i32; 3]| speeds.get(&p).map_or(0.0, |s| s[0]);
    let (s1, s2) = (speed(add(gap, d.offset())), speed(add(gap, d.opposite().offset())));
    if s1 == 0.0 || s2 == 0.0 || (s1 > 0.0) == (s2 > 0.0) {
        return None;
    }
    Some(Gap {
        out: rim(axis, d.opposite(), s1),
        speed: s1.abs() / 50.0 * 4.0,
    })
}

/// The way the rim of a wheel along `axis` turning at `speed` goes on its
/// side `d` (Create's controller facing: positive axis × side, by the
/// speed's sign).
fn rim(axis: Axis, d: Dir, speed: f32) -> Dir {
    let a = positive(axis).offset();
    let o = d.offset();
    let s = if speed > 0.0 { 1 } else { -1 };
    let way = [
        (a[1] * o[2] - a[2] * o[1]) * s,
        (a[2] * o[0] - a[0] * o[2]) * s,
        (a[0] * o[1] - a[1] * o[0]) * s,
    ];
    Dir::ALL
        .into_iter()
        .find(|d| d.offset() == way)
        .unwrap_or(Dir::Down)
}

/// A tick of crushing a stack of `count` at `speed`: what is left of the
/// countdown, down by `speed / log2(count)` (a lone item's by 20), 0.25 to
/// 20 a tick (Create's).
pub(super) fn step(left: f32, speed: f32, count: u32) -> f32 {
    let per = if count <= 1 {
        f32::INFINITY
    } else {
        speed / (count as f32).log2()
    };
    left - per.clamp(0.25, 20.0)
}

/// Where what was crushed comes out of a gap at `gap` sent `out`, and how
/// fast (Create's: just past the gap's middle that way; 0.25 a tick
/// sideways, 0.5 up, dropped down).
fn exit(gap: [i32; 3], out: Dir) -> ([f64; 3], [f64; 3]) {
    let o = out.offset().map(f64::from);
    let at = [0, 1, 2].map(|i| f64::from(gap[i]) + 0.5 + 0.55 * o[i]);
    let velocity = match out {
        Dir::Up => [0.0, 0.5, 0.0],
        Dir::Down => [0.0; 3],
        _ => [o[0] * 0.25, 0.0, o[2] * 0.25],
    };
    (at, velocity)
}

/// The stacks of `count` of `item`, at most 64 each.
fn stacks(item: &str, mut count: u32) -> Vec<ItemStack> {
    let mut out = Vec::new();
    while count > 0 {
        let n = count.min(64);
        out.push(ItemStack::new(item, n));
        count -= n;
    }
    out
}

/// A tick of the turning gaps: the stacks in them crushed (what they made
/// into `out`), the dropped items in them taken in by an empty one, else
/// held there till it is. Whether a stack went in or out.
fn crush(
    owner: &mut Owner,
    create: &Recipes,
    turning: &HashMap<[i32; 3], Gap>,
    dropped: &mut [ItemEntity],
    random: &mut impl RandomSource,
    out: &mut Vec<Out>,
) -> bool {
    let mut moved = false;
    for (p, gap) in turning {
        let Some(inside) = owner.crushers.get_mut(p) else {
            continue;
        };
        inside.left = step(inside.left, gap.speed, inside.count);
        owner.mills_changed = true;
        if inside.left >= DONE {
            continue;
        }
        let Some(inside) = owner.crushers.remove(p) else {
            continue;
        };
        moved = true;
        // Each item of the stack by the recipe's chances; nothing of a
        // stack no recipe crushes.
        let recipe = create.crushing(&inside.item).or_else(|| create.milling(&inside.item));
        let mut made: Vec<ItemStack> = Vec::new();
        for o in recipe.map_or(&[][..], |r| r.outputs.as_slice()) {
            let times = if o.chance >= 1.0 {
                inside.count
            } else {
                (0..inside.count).filter(|_| random.next_float() <= o.chance).count() as u32
            };
            made.extend(stacks(&o.item, o.count * times));
        }
        let (at, velocity) = exit(*p, gap.out);
        out.push((*p, made, at, velocity));
    }
    for item in dropped.iter_mut() {
        let at = item.position;
        let cell = [at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32];
        if !turning.contains_key(&cell) || owner.passed.get(&item.id) == Some(&cell) {
            continue;
        }
        if item.stack.extra.is_some() {
            owner.passed.insert(item.id, cell);
            continue;
        }
        if owner.crushers.contains_key(&cell) {
            item.position.x = f64::from(cell[0]) + 0.5;
            item.position.z = f64::from(cell[2]) + 0.5;
            item.position.y = item.position.y.max(f64::from(cell[1]) + 0.25);
            item.previous = item.position;
            item.velocity.x = 0.0;
            item.velocity.y = 0.0;
            item.velocity.z = 0.0;
            continue;
        }
        let name = item.stack.item.to_string();
        let recipe = create.crushing(&name).or_else(|| create.milling(&name));
        owner.crushers.insert(cell, Inside {
            item: name,
            count: item.stack.count,
            left: recipe.map_or(NO_RECIPE, |r| r.time),
        });
        moved = true;
        // Gone into the gap.
        item.stack.count = 0;
    }
    moved
}

impl Game<'_> {
    /// The owner's crushing wheels: each turning gap takes in a dropped item
    /// and crushes it, the others coming into it wait there. What a gap
    /// held whose wheels are gone (their chunks loaded) drops there.
    pub(super) fn tachyon_crushers(&mut self) {
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let found = OWNERS.with_borrow(|owners| {
            (owners.get(dimension))
                .map(|o| gaps(&o.index.parts, &o.shown.speeds))
                .unwrap_or_default()
        });
        let pairs: HashSet<[i32; 3]> = found.keys().copied().collect();
        // A gap with a block in it is none (Create's controller takes the
        // place of what was there).
        let facts = self.res.facts();
        let turning: HashMap<[i32; 3], Gap> = (found.into_iter())
            .filter_map(|(p, gap)| Some((p, gap?)))
            .filter(|(p, _)| facts.get(self.world.block(p[0], p[1], p[2])).replaceable)
            .collect();
        let present: HashSet<u32> = self.dropped.iter().map(|d| d.id).collect();
        let create = (!turning.is_empty()).then(|| self.tachyon_create_recipes());
        let chunks = &self.world.chunks;
        let loaded = |p: [i32; 3]| {
            [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]]
                .iter()
                .all(|[x, z]| chunks.contains_key(&((p[0] + x) >> 4, (p[2] + z) >> 4)))
        };
        let dropped = &mut self.dropped;
        let random = &mut self.world.random;
        let mut out: Vec<Out> = Vec::new();
        // What the gaps whose wheels are gone held.
        let mut lost: Vec<([i32; 3], ItemStack)> = Vec::new();
        OWNERS.with_borrow_mut(|owners| {
            let Some(owner) = owners.get_mut(dimension) else {
                return;
            };
            owner.passed.retain(|id, _| present.contains(id));
            owner.gaps = turning.keys().copied().collect();
            let gone: Vec<[i32; 3]> = (owner.crushers.keys())
                .filter(|p| !pairs.contains(*p) && loaded(**p))
                .copied()
                .collect();
            for p in &gone {
                if let Some(inside) = owner.crushers.remove(p) {
                    lost.extend(stacks(&inside.item, inside.count).into_iter().map(|s| (*p, s)));
                }
            }
            let moved = create.is_some_and(|create| crush(owner, &create, &turning, dropped, random, &mut out));
            // Saved at once when items left the world or came back to it.
            if moved || !gone.is_empty() {
                owner.save(&root, dimension);
            }
        });
        for (p, stack) in lost {
            self.pop_resource(p, stack);
        }
        self.dropped.retain(|d| d.stack.count > 0);
        // What came out passes the gap it came from.
        for (gap, made, at, velocity) in out {
            for stack in made {
                self.spawn_item(stack, at, velocity, 0);
                if let Some(id) = self.dropped.last().map(|d| d.id) {
                    OWNERS.with_borrow_mut(|owners| {
                        if let Some(owner) = owners.get_mut(dimension) {
                            owner.passed.insert(id, gap);
                        }
                    });
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheels_turning_against_each_other_make_a_gap_between_them() {
        let parts = HashMap::from([
            ([0, 0, 0], Part::CrushingWheel(Axis::Z)),
            ([2, 0, 0], Part::CrushingWheel(Axis::Z)),
        ]);
        let speeds = HashMap::from([([0, 0, 0], [-32.0; 2]), ([2, 0, 0], [32.0; 2])]);
        let found = gaps(&parts, &speeds);
        assert_eq!(found.len(), 1);
        let gap = found[&[1, 0, 0]].unwrap();
        // Their rims go down in the gap: what they crush drops.
        assert_eq!(gap.out, Dir::Down);
        assert!((gap.speed - 2.56).abs() < 1e-4);
        // Both turning the same way: a gap that doesn't work.
        let same = HashMap::from([([0, 0, 0], [32.0; 2]), ([2, 0, 0], [32.0; 2])]);
        assert_eq!(gaps(&parts, &same)[&[1, 0, 0]], None);
        // The other way round, up.
        let up = HashMap::from([([0, 0, 0], [32.0; 2]), ([2, 0, 0], [-32.0; 2])]);
        assert_eq!(gaps(&parts, &up)[&[1, 0, 0]].unwrap().out, Dir::Up);
        // At two speeds, the west wheel's (Create's order: down, up, north,
        // south, west, east).
        let two = HashMap::from([([0, 0, 0], [-8.0; 2]), ([2, 0, 0], [256.0; 2])]);
        assert!((gaps(&parts, &two)[&[1, 0, 0]].unwrap().speed - 0.64).abs() < 1e-4);
        // Wheels on different axes make none.
        let crossed = HashMap::from([
            ([0, 0, 0], Part::CrushingWheel(Axis::Z)),
            ([2, 0, 0], Part::CrushingWheel(Axis::Y)),
        ]);
        assert!(gaps(&crossed, &speeds).is_empty());
    }

    #[test]
    fn a_stack_is_crushed_slower_the_bigger_it_is() {
        // A lone item: 20 a tick, whatever the speed.
        assert_eq!(step(400.0, 2.56, 1), 380.0);
        // Four: 2.56 / 2 a tick.
        assert!((step(400.0, 2.56, 4) - 398.72).abs() < 1e-3);
        // Never under 0.25.
        assert_eq!(step(400.0, 0.1, 64), 399.75);
        let (at, velocity) = exit([1, 5, 0], Dir::Down);
        assert!((at[1] - 4.95).abs() < 1e-9 && at[0] == 1.5 && at[2] == 0.5);
        assert_eq!(velocity, [0.0; 3]);
        assert_eq!(exit([0, 0, 0], Dir::East).1, [0.25, 0.0, 0.0]);
        assert_eq!(stacks("x:a", 130).len(), 3);
    }

    #[test]
    fn a_gap_takes_in_a_stack_crushes_it_and_lets_out_what_it_made() {
        let mut owner = Owner::default();
        let create = Recipes::default();
        let turning = HashMap::from([([1, 0, 0], Gap { out: Dir::Down, speed: 2.56 })]);
        let mut random = horizon_core::random::LegacyRandom::new(1);
        let mut out = Vec::new();
        let at = horizon_core::geometry::Vec3::new(1.5, 0.3, 0.5);
        let mut dropped = vec![ItemEntity {
            health: 5,
            fire_ticks: 0,
            id: 7,
            stack: ItemStack::new("x:stone", 2),
            thrower: None,
            target: None,
            position: at,
            previous: at,
            velocity: horizon_core::geometry::Vec3::ZERO,
            on_ground: true,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
        }];
        // Taken in: gone from the world, kept by the gap.
        assert!(crush(&mut owner, &create, &turning, &mut dropped, &mut random, &mut out));
        assert_eq!(dropped[0].stack.count, 0);
        assert_eq!(owner.crushers[&[1, 0, 0]].left, NO_RECIPE);
        // No recipe crushes it: lost when its countdown runs out (100 - 20
        // at 2.56 a tick).
        let mut ticks = 0;
        while owner.crushers.contains_key(&[1, 0, 0]) {
            crush(&mut owner, &create, &turning, &mut [], &mut random, &mut out);
            ticks += 1;
        }
        assert_eq!(ticks, 32);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.is_empty());
    }
}
