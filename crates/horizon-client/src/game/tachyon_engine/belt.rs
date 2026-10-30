//! Kinetica's belt and funnel. Belt segments in a line turn together, on
//! pulleys a shaft beside any of them turns; turning, they carry the dropped
//! items on them along the line (Create's speed, a block in 480 / RPM ticks)
//! and keep them to its middle. Past its end an item falls off, or goes into
//! a funnel there.
//!
//! A funnel sits on a container's face (a chest, a furnace, a depot, a
//! basin...), facing out of it. It puts the dropped items that come into it
//! into the container, through that face; used, it turns to taking out
//! instead: a stack of up to 16 every few ticks while nothing lies at its
//! mouth, dropped out of it (onto a belt, say).

use super::super::Game;
use super::super::machines::dir_from_name;
use super::kinetic::{Axis, Dir, Part};
use super::{OWNERS, Owner};
use std::collections::HashMap;

/// Ticks between a funnel's takings out.
const FUNNEL_TICKS: u64 = 8;
/// The most a funnel takes out at once.
const FUNNEL_STACK: u32 = 16;
/// How far round its block a funnel reaches for the items to put in.
const FUNNEL_REACH: f64 = 0.25;

/// What a belt along `axis` turning at `speed` (RPM) carries an item by in a
/// tick: Create's speed, the way its pulleys' tops go (by the right hand
/// about their axis, the other level one).
pub(super) fn carry(axis: Axis, speed: f32) -> [f64; 3] {
    let v = f64::from(speed) / 480.0;
    match axis {
        Axis::X => [-v, 0.0, 0.0],
        _ => [0.0, 0.0, v],
    }
}

/// The axis of a belt's pulleys: the other level one.
pub(super) fn pulleys(axis: Axis) -> Axis {
    if axis == Axis::X { Axis::Z } else { Axis::X }
}

/// The belts turning, and what each carries by.
fn moving(parts: &HashMap<[i32; 3], Part>, speeds: &super::kinetic::Speeds) -> HashMap<[i32; 3], [f64; 3]> {
    (parts.iter())
        .filter_map(|(p, part)| {
            let Part::Belt(axis) = part else {
                return None;
            };
            let speed = speeds.get(p)?[0];
            (speed != 0.0).then(|| (*p, carry(*axis, speed)))
        })
        .collect()
}

/// Whether an item's box `lo`..`hi` meets the block at `pos` grown by
/// `grow` all round.
fn meets(pos: [i32; 3], lo: [f64; 3], hi: [f64; 3], grow: f64) -> bool {
    (0..3).all(|i| {
        let min = f64::from(pos[i]) - grow;
        lo[i] < min + 1.0 + 2.0 * grow && hi[i] > min
    })
}

impl Game<'_> {
    /// The owner's belts carry the dropped items on them: each by the belt
    /// under its middle, else under its edge (so it goes off the end).
    pub(super) fn tachyon_belts(&mut self) {
        let dimension = self.dim.id();
        let belts = OWNERS.with_borrow(|owners| {
            (owners.get(dimension))
                .map(|o| moving(&o.index.parts, &o.shown.speeds))
                .unwrap_or_default()
        });
        if belts.is_empty() {
            return;
        }
        for item in &mut self.dropped {
            let p = item.position;
            let y = p.y.floor();
            // On the belt's top (9/16), not over it.
            if p.y - y > 0.75 {
                continue;
            }
            let cell = |dx: f64, dz: f64| [(p.x + dx).floor() as i32, y as i32, (p.z + dz).floor() as i32];
            let under = [(0.0, 0.0), (-0.125, 0.0), (0.125, 0.0), (0.0, -0.125), (0.0, 0.125)]
                .into_iter()
                .map(|(dx, dz)| cell(dx, dz))
                .find_map(|c| Some((c, *belts.get(&c)?)));
            let Some((at, way)) = under else {
                continue;
            };
            // Along the belt at its speed; across, to its middle.
            let v = &mut item.velocity;
            if way[0] != 0.0 {
                v.x = way[0];
                v.z = (f64::from(at[2]) + 0.5 - p.z) * 0.2;
            } else {
                v.z = way[2];
                v.x = (f64::from(at[0]) + 0.5 - p.x) * 0.2;
            }
        }
    }

    /// The owner's funnels: putting the items that come into them into the
    /// container behind, or taking stacks out of it.
    pub(super) fn tachyon_funnels(&mut self) {
        let dimension = self.dim.id();
        let (funnels, ticks) = OWNERS.with_borrow_mut(|owners| {
            let Some(owner): Option<&mut Owner> = owners.get_mut(dimension) else {
                return (Vec::new(), 0);
            };
            let funnels: Vec<([i32; 3], Dir, bool)> = (owner.index.parts.iter())
                .filter_map(|(p, part)| match part {
                    Part::Funnel(d) => Some((*p, *d, owner.funnels_out.contains(p))),
                    _ => None,
                })
                .collect();
            owner.funnel_work = (funnels.iter())
                .map(|(p, _, out)| (*p, (if *out { "extracting" } else { "inserting" }).to_owned()))
                .collect();
            (funnels, owner.ticks)
        });
        for (pos, facing, out) in funnels {
            let o = facing.offset();
            let behind = (pos[0] - o[0], pos[1] - o[1], pos[2] - o[2]);
            let Some(face) = dir_from_name(facing.name()) else {
                continue;
            };
            let near: Vec<usize> = (self.dropped.iter().enumerate())
                .filter(|(_, d)| {
                    let b = d.bounds();
                    let (lo, hi) = (b.min_position(), b.max_position());
                    meets(pos, [lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z], FUNNEL_REACH)
                })
                .map(|(i, _)| i)
                .collect();
            if !out {
                // Last first, so the others keep their places.
                for i in near.into_iter().rev() {
                    let stack = self.dropped[i].stack.clone();
                    match self.tachyon_container_put(behind, stack, face) {
                        None => {
                            self.dropped.remove(i);
                        }
                        Some(rest) => self.dropped[i].stack = rest,
                    }
                }
                continue;
            }
            if !near.is_empty() || ticks % FUNNEL_TICKS != 0 {
                continue;
            }
            let Some(stack) = self.tachyon_container_take(behind, face, FUNNEL_STACK) else {
                continue;
            };
            // Out of its mouth: over its rod when it faces the side.
            let reach = if facing.axis() == Axis::Y { 0.8 } else { 0.3 };
            let mut at = [0usize, 1, 2].map(|i| f64::from(pos[i]) + 0.5 + f64::from(o[i]) * reach);
            if facing.axis() != Axis::Y {
                at[1] = f64::from(pos[1]) + 0.7;
            }
            let velocity = o.map(|v| f64::from(v) * 0.12);
            self.spawn_item(stack, at, velocity, 10);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_belt_carries_the_way_its_pulleys_tops_go() {
        // Pulleys along X turning forward: their tops go south.
        assert_eq!(carry(Axis::Z, 48.0), [0.0, 0.0, 0.1]);
        // Pulleys along Z: west.
        assert_eq!(carry(Axis::X, 48.0), [-0.1, 0.0, 0.0]);
        assert_eq!(carry(Axis::X, -48.0), [0.1, 0.0, 0.0]);
        assert_eq!(pulleys(Axis::X), Axis::Z);
    }

    #[test]
    fn a_funnel_reaches_a_little_round_its_block() {
        let lo = [1.6, 0.5, 0.4];
        let hi = [1.85, 0.75, 0.65];
        assert!(meets([1, 0, 0], lo, hi, 0.25));
        // Just past its west side.
        assert!(meets([2, 0, 0], lo, hi, 0.25));
        assert!(!meets([2, 0, 0], lo, hi, 0.0));
        assert!(!meets([3, 0, 0], lo, hi, 0.25));
    }
}
