//! Create's crushing wheels, turned by Kinetica's engine, working as
//! Create's (`CrushingWheelBlock`, `CrushingWheelControllerBlockEntity`):
//! two wheels on parallel shafts with one block between them, turning
//! against each other, crush the dropped items that come into that gap by
//! Create's crushing recipes (else its milling ones), a whole stack at a
//! time, and send what they made out of the gap the way their rims go there.
//!
//! An item with no recipe passes through. Unlike Create's, the item stays
//! an entity held in the gap while it is crushed (the owner keeps its
//! countdown by its id), and the wheels don't hurt mobs yet.
use super::super::Game;
use super::kinetic::{Axis, Dir, Part, Speeds};
use super::{OWNERS, positive};
use horizon_client::items::ItemStack;
use horizon_core::random::RandomSource;
use std::collections::{HashMap, HashSet};

/// Below this, what the gap holds is crushed (Create's).
const DONE: f32 = 20.0;

/// A wheel pair's gap: the way it sends out what it crushed, and how fast
/// it crushes (Create's: the wheel's RPM / 50, times 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Gap {
    pub out: Dir,
    pub speed: f32,
}

/// The gaps of the wheel pairs turning against each other, by position.
pub(super) fn gaps(parts: &HashMap<[i32; 3], Part>, speeds: &Speeds) -> HashMap<[i32; 3], Gap> {
    let mut gaps = HashMap::new();
    for (p, part) in parts {
        let Part::CrushingWheel(axis) = *part else {
            continue;
        };
        let Some(s1) = speeds.get(p).map(|s| s[0]).filter(|s| *s != 0.0) else {
            continue;
        };
        for d in Dir::ALL.into_iter().filter(|d| d.axis() != axis) {
            let o = d.offset();
            let other = [p[0] + 2 * o[0], p[1] + 2 * o[1], p[2] + 2 * o[2]];
            if parts.get(&other) != Some(&Part::CrushingWheel(axis)) {
                continue;
            }
            let Some(s2) = speeds.get(&other).map(|s| s[0]).filter(|s| *s != 0.0) else {
                continue;
            };
            if (s1 > 0.0) == (s2 > 0.0) {
                continue;
            }
            let gap = [p[0] + o[0], p[1] + o[1], p[2] + o[2]];
            gaps.entry(gap).or_insert(Gap {
                out: rim(axis, d, s1),
                speed: s1.abs() / 50.0 * 4.0,
            });
        }
    }
    gaps
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

impl Game<'_> {
    /// The owner's crushing wheels: the dropped items in their gaps held
    /// and crushed, one stack a gap at a time.
    pub(super) fn tachyon_crushers(&mut self) {
        let dimension = self.dim.id();
        let gaps = OWNERS.with_borrow(|owners| {
            (owners.get(dimension))
                .map(|o| gaps(&o.index.parts, &o.shown.speeds))
                .unwrap_or_default()
        });
        let present: HashSet<u32> = self.dropped.iter().map(|d| d.id).collect();
        if gaps.is_empty() {
            OWNERS.with_borrow_mut(|owners| {
                if let Some(owner) = owners.get_mut(dimension) {
                    owner.crushing.clear();
                    owner.passed.retain(|id, _| present.contains(id));
                }
            });
            return;
        }
        let create = self.tachyon_create_recipes();
        let dropped = &mut self.dropped;
        let random = &mut self.world.random;
        // Each crushed item: its index, what it made, where it goes.
        let mut done: Vec<(usize, Vec<ItemStack>, [f64; 3], [f64; 3])> = Vec::new();
        OWNERS.with_borrow_mut(|owners| {
            let Some(owner) = owners.get_mut(dimension) else {
                return;
            };
            // An item that left its gap is no longer crushed there.
            let cells: HashMap<u32, [i32; 3]> = (dropped.iter())
                .map(|d| {
                    let p = d.position;
                    (d.id, [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32])
                })
                .collect();
            owner.crushing.retain(|id, c| cells.get(id) == Some(&c.0));
            owner.passed.retain(|id, _| present.contains(id));
            let mut busy: HashSet<[i32; 3]> = owner.crushing.values().map(|c| c.0).collect();
            for (i, item) in dropped.iter_mut().enumerate() {
                let p = item.position;
                let cell = [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32];
                let Some(gap) = gaps.get(&cell) else {
                    continue;
                };
                if owner.passed.get(&item.id) == Some(&cell) {
                    continue;
                }
                let name = item.stack.item.to_string();
                let recipe = create.crushing(&name).or_else(|| create.milling(&name));
                let Some(recipe) = recipe.filter(|_| item.stack.extra.is_none()) else {
                    owner.passed.insert(item.id, cell);
                    continue;
                };
                // Held in the middle of the gap while it waits or is crushed.
                item.position.x = f64::from(cell[0]) + 0.5;
                item.position.z = f64::from(cell[2]) + 0.5;
                item.position.y = item.position.y.max(f64::from(cell[1]) + 0.25);
                item.previous = item.position;
                item.velocity.x = 0.0;
                item.velocity.y = 0.0;
                item.velocity.z = 0.0;
                let left = match owner.crushing.get(&item.id) {
                    Some(&(_, left)) => step(left, gap.speed, item.stack.count),
                    None if busy.contains(&cell) => continue,
                    None => {
                        busy.insert(cell);
                        recipe.time
                    }
                };
                if left >= DONE {
                    owner.crushing.insert(item.id, (cell, left));
                    continue;
                }
                // Crushed: each item of the stack by the recipe's chances.
                owner.crushing.remove(&item.id);
                let n = item.stack.count;
                let mut made: Vec<ItemStack> = Vec::new();
                for o in &recipe.outputs {
                    let times = if o.chance >= 1.0 {
                        n
                    } else {
                        (0..n).filter(|_| random.next_float() <= o.chance).count() as u32
                    };
                    made.extend(stacks(&o.item, o.count * times));
                }
                let (at, velocity) = exit(cell, gap.out);
                done.push((i, made, at, velocity));
                owner.passed.insert(item.id, cell);
            }
        });
        // Last first, so the others keep their places.
        for (i, mut made, at, velocity) in done.into_iter().rev() {
            if made.is_empty() {
                self.dropped.remove(i);
                continue;
            }
            let item = &mut self.dropped[i];
            item.stack = made.remove(0);
            item.position.x = at[0];
            item.position.y = at[1];
            item.position.z = at[2];
            item.previous = item.position;
            item.velocity.x = velocity[0];
            item.velocity.y = velocity[1];
            item.velocity.z = velocity[2];
            item.pickup_delay = 20;
            for stack in made {
                self.spawn_item(stack, at, velocity, 20);
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
        let gap = found[&[1, 0, 0]];
        // Their rims go down in the gap: what they crush drops.
        assert_eq!(gap.out, Dir::Down);
        assert!((gap.speed - 2.56).abs() < 1e-4);
        // Both turning the same way: no gap.
        let same = HashMap::from([([0, 0, 0], [32.0; 2]), ([2, 0, 0], [32.0; 2])]);
        assert!(gaps(&parts, &same).is_empty());
        // The other way round, up.
        let up = HashMap::from([([0, 0, 0], [32.0; 2]), ([2, 0, 0], [-32.0; 2])]);
        assert_eq!(gaps(&parts, &up)[&[1, 0, 0]].out, Dir::Up);
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
}
