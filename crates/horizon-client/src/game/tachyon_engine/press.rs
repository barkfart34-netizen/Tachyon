//! The mechanical press's cycle, Kinetica's and Create's alike, as Create's
//! (`PressingBehaviour`, `MechanicalPressBlockEntity`; Create's code is
//! MIT-licensed, THIRD_PARTY_NOTICES.md). A press at work runs a cycle of
//! 240, faster the faster it turns; its head comes down to the middle of
//! it and presses once there: one item of the stack on the depot under it
//! (`millstone::Mill::press`), the items of one recipe in the basin under it
//! (`basin::Basin::compact`), or, with neither under it, one item of a
//! stack lying on the ground in the block right under it.
//!
//! A game shows each press at work with its head going down and up by the
//! same cycle, run on its own from when the press began (`PressHeads`).
use super::super::Game;
use super::kinetic::Part;
use super::OWNERS;
use super::millstone::{self, roll_outputs};
use horizon_client::items::ItemStack;
use horizon_core::random::RandomSource;
use std::collections::HashMap;

/// A whole cycle's running ticks (Create's).
pub(super) const CYCLE: i32 = 240;
/// Ticks between an idle press's looks for items lying under it (Create's).
const SCAN: u64 = 10;
/// The pickup delay of what a press makes of one item of a stack lying on
/// the ground (Create's: an item entity's default).
const PICKUP_DELAY: u32 = 10;

/// How far a press at `speed` (RPM) runs its cycle a tick: from 1 to 60
/// at 512 RPM, in whole ticks; none still (Create's).
pub(super) fn step(speed: f32) -> i32 {
    if speed == 0.0 {
        return 0;
    }
    (1.0 + 59.0 * (speed.abs() / 512.0).min(1.0)) as i32
}

/// What a tick of a press's cycle comes to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Beat {
    /// On its way.
    Running,
    /// Its head is down: it presses.
    Press,
    /// Past the end: done (a new one starts from 0).
    Done,
}

/// A tick of a cycle `ticks` along, at `speed` (Create's
/// `PressingBehaviour.tick`): at the middle the head is down and presses;
/// past the end the cycle is done; else it runs on, stopping once at the
/// middle.
pub(super) fn beat(ticks: &mut i32, speed: f32) -> Beat {
    let press = *ticks == CYCLE / 2 && speed != 0.0;
    if *ticks > CYCLE {
        return Beat::Done;
    }
    let before = *ticks;
    *ticks += step(speed);
    if before < CYCLE / 2 && *ticks >= CYCLE / 2 {
        *ticks = CYCLE / 2;
    }
    if press { Beat::Press } else { Beat::Running }
}

/// How far down a press's head is, from 0 (up) to 1 (down on its work),
/// `ticks` into its cycle (Create's `getRenderedHeadOffset`): down fast to
/// the middle, then up from two thirds.
pub(super) fn head(ticks: f32) -> f32 {
    let cycle = CYCLE as f32;
    if ticks < cycle * 2.0 / 3.0 {
        (ticks / cycle * 2.0).powi(3).clamp(0.0, 1.0)
    } else {
        ((cycle - ticks) / cycle * 3.0).clamp(0.0, 1.0)
    }
}

/// The cycles a game shows its presses at work by: each one's ticks before
/// and after the last tick.
#[derive(Default)]
pub(super) struct PressHeads(HashMap<[i32; 3], (i32, i32)>);

impl PressHeads {
    /// A tick: the presses at work (with their speed) run their cycles,
    /// one done starting again; the others are forgotten.
    pub fn tick(&mut self, working: impl Iterator<Item = ([i32; 3], f32)>) {
        let mut next = HashMap::new();
        for (p, speed) in working {
            let (_, mut now) = self.0.get(&p).copied().unwrap_or((0, 0));
            let before = now;
            if beat(&mut now, speed) == Beat::Done {
                next.insert(p, (0, 0));
            } else {
                next.insert(p, (before, now));
            }
        }
        self.0 = next;
    }
    /// How far down the head of the press at `p` is, `alpha` into the tick.
    pub fn down(&self, p: [i32; 3], alpha: f32) -> f32 {
        self.0.get(&p).map_or(0.0, |&(before, now)| {
            head(before as f32 + (now - before) as f32 * alpha)
        })
    }
}

impl Game<'_> {
    /// The owner's presses with no depot or basin under them: an idle one
    /// looks every few ticks for a dropped item lying on the ground in the
    /// block right under it that it presses (Kinetica's or Create's
    /// recipes, its own first), and runs a cycle; at its middle it presses
    /// one item of the first stack lying there (Create's `applyInWorld`,
    /// `tryProcessInWorld`).
    pub(super) fn tachyon_presses_on_ground(&mut self) {
        let dimension = self.dim.id();
        let create = self.tachyon_create_recipes();
        // Each press with nothing under it to work: where, how fast, and
        // whether it is Create's.
        let presses: Vec<([i32; 3], f32, bool, u64)> = OWNERS.with_borrow(|owners| {
            let Some(o) = owners.get(dimension) else {
                return Vec::new();
            };
            (o.index.parts.iter())
                .filter(|(_, part)| matches!(part, Part::Press(_)))
                .filter(|(p, _)| {
                    let under = |d: i32| o.index.parts.get(&[p[0], p[1] - d, p[2]]);
                    let works = |q: Option<&Part>| matches!(q, Some(Part::Depot | Part::Basin));
                    !(works(under(1)) || (under(1).is_none() && works(under(2))))
                })
                .map(|(p, _)| {
                    let speed = o.shown.speeds.get(p).map_or(0.0, |s| s[0]);
                    (*p, speed, o.index.create.contains(p), o.ticks)
                })
                .collect()
        });
        // The dropped items lying on the ground in the block under a press,
        // `inset` in from its sides: their indices (Create's: an eighth in
        // to look, the whole block to press).
        let lying = |game: &Self, p: [i32; 3], inset: f64| -> Vec<usize> {
            let lo = [f64::from(p[0]) + inset, f64::from(p[1] - 1) + inset, f64::from(p[2]) + inset];
            let hi = [f64::from(p[0] + 1) - inset, f64::from(p[1]) - inset, f64::from(p[2] + 1) - inset];
            (game.dropped.iter().enumerate())
                .filter(|(_, d)| {
                    let b = d.bounds();
                    let (a, z) = (b.min_position(), b.max_position());
                    d.on_ground
                        && a.x < hi[0]
                        && z.x > lo[0]
                        && a.y < hi[1]
                        && z.y > lo[1]
                        && a.z < hi[2]
                        && z.z > lo[2]
                })
                .map(|(i, _)| i)
                .collect()
        };
        let recipe_for = |item: &str, theirs: bool| {
            let (ours, create) = (millstone::press_recipe(item), create.pressing(item));
            if theirs { create.or(ours) } else { ours.or(create) }
        };
        let mut work: HashMap<[i32; 3], String> = HashMap::new();
        let mut cycles: HashMap<[i32; 3], i32> =
            OWNERS.with_borrow(|owners| owners.get(dimension).map(|o| o.press_cycles.clone()).unwrap_or_default());
        cycles.retain(|p, _| presses.iter().any(|q| q.0 == *p));
        // What pressing one item of a stack made: the stacks, where, how fast.
        let mut made: Vec<(ItemStack, [f64; 3], [f64; 3])> = Vec::new();
        for &(p, speed, theirs, ticks) in &presses {
            if speed == 0.0 {
                continue;
            }
            let seen = lying(self, p, 0.125);
            let pressable = (seen.iter()).any(|&i| recipe_for(&self.dropped[i].stack.item, theirs).is_some());
            let Some(mut cycle) = cycles.get(&p).copied() else {
                // Idle: a look now and then for an item it presses.
                if ticks % SCAN == 0 && pressable {
                    cycles.insert(p, 0);
                }
                continue;
            };
            if let Some(&i) = seen.first() {
                work.insert(p, self.dropped[i].stack.item.to_string());
            }
            match beat(&mut cycle, speed) {
                Beat::Done => {
                    // Create looks again at once after pressing.
                    if pressable {
                        cycles.insert(p, 0);
                    } else {
                        cycles.remove(&p);
                    }
                    continue;
                }
                Beat::Press => {
                    // The first item entity lying in the block, whether or
                    // not it has a recipe (Create's, without bulk pressing).
                    let here = lying(self, p, 0.0);
                    if let Some(&i) = here.first()
                        && let Some(recipe) = recipe_for(&self.dropped[i].stack.item, theirs)
                    {
                        let random = &mut self.world.random;
                        let outputs: Vec<ItemStack> = roll_outputs(&recipe.outputs, &mut || random.next_float())
                            .into_iter()
                            .map(|h| ItemStack::new(&h.item, h.count))
                            .collect();
                        let item = &mut self.dropped[i];
                        let at = [item.position.x, item.position.y, item.position.z];
                        if item.stack.count == 1 {
                            // The item becomes what it made (the rest beside it).
                            let mut outputs = outputs.into_iter();
                            match outputs.next() {
                                Some(first) => item.stack = first,
                                None => item.stack.count = 0,
                            }
                            let v = [item.velocity.x, item.velocity.y, item.velocity.z];
                            made.extend(outputs.map(|s| (s, at, v)));
                        } else {
                            item.stack.count -= 1;
                            for s in outputs {
                                let r = &mut self.world.random;
                                let v = [0, 1, 2].map(|_| f64::from(r.next_float() - 0.5) * 0.1);
                                made.push((s, at, v));
                            }
                        }
                    }
                }
                Beat::Running => {}
            }
            cycles.insert(p, cycle);
        }
        self.dropped.retain(|d| d.stack.count > 0);
        for (stack, at, v) in made {
            self.spawn_item(stack, at, v, PICKUP_DELAY);
        }
        OWNERS.with_borrow_mut(|owners| {
            if let Some(owner) = owners.get_mut(dimension) {
                owner.press_cycles = cycles;
                owner.press_work = work;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_press_presses_once_in_the_middle_of_its_cycle() {
        // 32 RPM: 4 a tick; still, none; 256 RPM: 30; never over 60.
        assert_eq!((step(32.0), step(0.0), step(-256.0), step(9999.0)), (4, 0, 30, 60));
        let mut ticks = 0;
        let mut beats = Vec::new();
        loop {
            let b = beat(&mut ticks, 32.0);
            beats.push(b);
            if b == Beat::Done {
                break;
            }
        }
        // 30 ticks down to the middle, pressing on the 31st, then on past
        // 240, done on the 62nd.
        assert_eq!(beats.len(), 62);
        assert_eq!(beats.iter().position(|b| *b == Beat::Press), Some(30));
        assert_eq!(beats.iter().filter(|b| **b == Beat::Press).count(), 1);
        // Stopped, it waits where it is.
        let mut ticks = 120;
        assert_eq!(beat(&mut ticks, 0.0), Beat::Running);
        assert_eq!(ticks, 120);
    }

    #[test]
    fn its_head_comes_down_in_the_middle_and_goes_back_up() {
        assert_eq!(head(0.0), 0.0);
        assert_eq!(head(120.0), 1.0);
        assert!(head(60.0) > 0.0 && head(60.0) < 1.0);
        assert_eq!(head(240.0), 0.0);
        let mut heads = PressHeads::default();
        for _ in 0..30 {
            heads.tick([([0, 0, 0], 32.0)].into_iter());
        }
        assert_eq!(heads.down([0, 0, 0], 1.0), 1.0);
        // No longer at work: up.
        heads.tick(std::iter::empty());
        assert_eq!(heads.down([0, 0, 0], 1.0), 0.0);
    }
}
