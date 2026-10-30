//! Kinetica's encased fan. A fan turning blows along its axis, out of the
//! side no part drives it from (with a part on both sides or neither, the
//! way it turns); turned the other way, it pulls. The air goes 3 blocks, and
//! further the faster the fan turns (20 at 256 RPM), until a block with a
//! full collision stops it, and pushes the dropped items in it.
//!
//! What the air goes through works the items further on, dropped ones and
//! those on a depot alike, after `TIME` ticks in it (as Create's bulk
//! processing): water (or a waterlogged block) washes them by
//! `kinetica/data/kinetica/tachyon/washing.json`, fire or a lit campfire
//! smokes them and lava blasts them by the furnaces' recipes.

use super::super::Game;
use super::kinetic::{Axis, Dir, Part};
use super::millstone::{self, Held, Milling};
use super::{OWNERS, Owner};
use horizon_client::items::ItemStack;
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_core::geometry::Vec3;
use horizon_core::random::RandomSource;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::OnceLock;

/// Ticks an item spends where the air works it before it is worked.
pub(super) const TIME: u32 = 150;

/// What the air works the items with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Process {
    Washing,
    Smoking,
    Blasting,
}

impl Process {
    /// Its name in the goggles' keys (`kinetica.hud.washing`).
    pub fn name(self) -> &'static str {
        match self {
            Process::Washing => "washing",
            Process::Smoking => "smoking",
            Process::Blasting => "blasting",
        }
    }
    /// The process a block the air goes through starts.
    fn of<'a>(name: &str, property: impl Fn(&str) -> Option<&'a str>) -> Option<Self> {
        match name {
            "minecraft:water" | "minecraft:bubble_column" => Some(Process::Washing),
            "minecraft:lava" => Some(Process::Blasting),
            "minecraft:fire" => Some(Process::Smoking),
            "minecraft:campfire" => (property("lit") == Some("true")).then_some(Process::Smoking),
            _ => (property("waterlogged") == Some("true")).then_some(Process::Washing),
        }
    }
}

/// The washing recipes of the built-in content, read once.
pub(super) fn washing() -> &'static [Milling] {
    static RECIPES: OnceLock<Vec<Milling>> = OnceLock::new();
    RECIPES.get_or_init(|| millstone::read("washing.json"))
}

/// How far a fan at `speed` (RPM) blows: 3 blocks, 20 at 256 RPM.
pub(super) fn range(speed: f32) -> i32 {
    ((speed.abs() * 20.0 / 256.0) as i32).clamp(3, 20)
}

/// Where the fan at `pos` blows out of, and whether it pushes (else pulls):
/// the side of its axis no part drives it from; with a part on both sides
/// or neither, the way it turns.
pub(super) fn front(
    parts: &HashMap<[i32; 3], Part>,
    pos: [i32; 3],
    axis: Axis,
    speed: f32,
) -> (Dir, bool) {
    let [neg, plus] = axis.dirs();
    let joined = |d: Dir| {
        let o = d.offset();
        (parts.get(&[pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]]))
            .is_some_and(|q| q.shaft_toward(d.opposite()))
    };
    match (joined(neg), joined(plus)) {
        (true, false) => (plus, speed > 0.0),
        (false, true) => (neg, speed < 0.0),
        _ => (if speed >= 0.0 { plus } else { neg }, true),
    }
}

/// Each one's ticks in its process go up (from 1 when it came or its
/// process changed); the others are forgotten. Those that reached `TIME`
/// (their count back to 0).
fn tick<K: Copy + Eq + Hash>(
    timers: &mut HashMap<K, (Process, u32)>,
    seen: Vec<(K, Process)>,
) -> Vec<(K, Process)> {
    let mut ready = Vec::new();
    let mut next = HashMap::with_capacity(seen.len());
    for (k, p) in seen {
        let ticks = match timers.get(&k) {
            Some((q, t)) if *q == p => t + 1,
            _ => 1,
        };
        if ticks >= TIME {
            ready.push((k, p));
            next.insert(k, (p, 0));
        } else {
            next.insert(k, (p, ticks));
        }
    }
    *timers = next;
    ready
}

/// The air's push on an item going `way` (a unit step): up to 0.4 blocks a
/// tick along it; a lift upward beats gravity.
fn push(v: &mut Vec3, way: [f64; 3]) {
    const ACCELERATION: f64 = 0.06;
    let along = v.x * way[0] + v.y * way[1] + v.z * way[2];
    if along < 0.4 {
        let a = if way[1] > 0.0 { ACCELERATION + 0.04 } else { ACCELERATION };
        v.x += way[0] * a;
        v.y += way[1] * a;
        v.z += way[2] * a;
    }
}

/// The fans turning in a game's view (the owner's, or a client's): each
/// one's place, where it blows out of, whether it pushes, its speed.
fn turning(parts: &HashMap<[i32; 3], Part>, speeds: &super::kinetic::Speeds) -> Vec<Fan> {
    (parts.iter())
        .filter_map(|(p, part)| {
            let Part::Fan(axis) = part else {
                return None;
            };
            let speed = speeds.get(p)?[0];
            if speed == 0.0 {
                return None;
            }
            let (front, push) = front(parts, *p, *axis, speed);
            Some(Fan {
                pos: *p,
                front,
                push,
                speed,
            })
        })
        .collect()
}

struct Fan {
    pos: [i32; 3],
    front: Dir,
    push: bool,
    speed: f32,
}

impl Fan {
    /// The way the air goes, a unit step.
    fn way(&self) -> [f64; 3] {
        let o = self.front.offset().map(f64::from);
        if self.push { o } else { o.map(|v| -v) }
    }
}

impl Game<'_> {
    /// The cells a fan at `pos` blows through out of `front` at `speed`,
    /// each with the process the air carries there.
    fn tachyon_stream(&self, pos: [i32; 3], front: Dir, speed: f32) -> Vec<([i32; 3], Option<Process>)> {
        let registry = self.pack.assets.registry;
        let o = front.offset();
        let mut process = None;
        let mut out = Vec::new();
        for i in 1..=range(speed) {
            let at = [pos[0] + o[0] * i, pos[1] + o[1] * i, pos[2] + o[2] * i];
            let state = self.world.block(at[0], at[1], at[2]);
            if self.res.facts().get(state).full_collision {
                break;
            }
            if let Some(s) = registry.state(state)
                && let Some(p) = Process::of(&s.name, |k| s.properties.get(k).map(String::as_str))
            {
                process = Some(p);
            }
            out.push((at, process));
        }
        out
    }

    /// The owner's fans: their air pushes the dropped items in it, and works
    /// what has been where it works long enough, dropped or on a depot. What
    /// each fan does shows in the goggles (`Owner::fan_work`).
    pub(super) fn tachyon_fans(&mut self) {
        let dimension = self.dim.id();
        let fans = OWNERS.with_borrow(|owners| {
            owners
                .get(dimension)
                .map(|o| turning(&o.index.parts, &o.shown.speeds))
                .unwrap_or_default()
        });
        // The cells blown through: the air's way and its process there.
        let mut cells: HashMap<[i32; 3], ([f64; 3], Option<Process>)> = HashMap::new();
        let mut work = HashMap::new();
        for fan in &fans {
            let stream = self.tachyon_stream(fan.pos, fan.front, fan.speed);
            let process = stream.iter().rev().find_map(|c| c.1).map_or("air", Process::name);
            let shown = if fan.push {
                process.to_owned()
            } else {
                format!("pull:{process}")
            };
            work.insert(fan.pos, shown);
            for (at, process) in stream {
                cells.insert(at, (fan.way(), process));
            }
        }
        // The dropped items in the air: pushed, and timed where it works.
        let mut items = Vec::new();
        for item in &mut self.dropped {
            let p = item.position;
            let cell = [
                p.x.floor() as i32,
                (p.y + 0.125).floor() as i32,
                p.z.floor() as i32,
            ];
            let Some((way, process)) = cells.get(&cell) else {
                continue;
            };
            push(&mut item.velocity, *way);
            if let Some(process) = process {
                items.push((item.id, *process));
            }
        }
        let (items, depots) = OWNERS.with_borrow_mut(|owners| {
            let owner: &mut Owner = owners.entry(dimension).or_default();
            owner.fan_work = work;
            let items = tick(&mut owner.fan_items, items);
            // The depots where the air works, holding something to work.
            let depots: Vec<([i32; 3], Process)> = (cells.iter())
                .filter_map(|(at, (_, p))| Some((*at, (*p)?)))
                .filter(|(at, _)| {
                    owner.index.parts.get(at) == Some(&Part::Depot)
                        && owner.mills.get(at).is_some_and(|m| m.input.is_some())
                })
                .collect();
            let depots: Vec<([i32; 3], Process, Held)> = tick(&mut owner.fan_depots, depots)
                .into_iter()
                .filter_map(|(at, p)| Some((at, p, owner.mills.get_mut(&at)?.input.take()?)))
                .collect();
            (items, depots)
        });
        for (id, process) in items {
            self.tachyon_work_item(id, process);
        }
        // A depot's stack worked whole into what it made, if that fits; else
        // it stays.
        for (at, process, held) in depots {
            let stack = ItemStack::new(&held.item, held.count);
            let made: Option<Vec<Held>> = self.tachyon_process(process, &stack).map(|made| {
                (made.iter())
                    .map(|s| Held {
                        item: s.item.to_string(),
                        count: s.count,
                    })
                    .collect()
            });
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                let mill = owner.mills.entry(at).or_default();
                match made {
                    Some(made) if mill.add_made(&made) => owner.mills_changed = true,
                    _ => mill.input = Some(held),
                }
            });
        }
    }

    /// The dropped item `id` worked by `process`: its stack becomes what it
    /// made (the rest beside it), or it goes when it made nothing.
    fn tachyon_work_item(&mut self, id: u32, process: Process) {
        let Some(i) = self.dropped.iter().position(|d| d.id == id) else {
            return;
        };
        let stack = self.dropped[i].stack.clone();
        let Some(made) = self.tachyon_process(process, &stack) else {
            return;
        };
        let at = self.dropped[i].position;
        let mut stacks = self.tachyon_split(made);
        if stacks.is_empty() {
            self.dropped.remove(i);
            return;
        }
        self.dropped[i].stack = stacks.remove(0);
        for s in stacks {
            self.spawn_item(s, [at.x, at.y, at.z], [0.0; 3], 10);
        }
    }

    /// What `process` makes of a whole stack: each item washed (the outputs
    /// by their chances), or cooked by a smoker's recipe (smoking) or a blast
    /// furnace's, else a furnace's (blasting). None when nothing works it.
    pub(super) fn tachyon_process(&mut self, process: Process, stack: &ItemStack) -> Option<Vec<ItemStack>> {
        let n = stack.count;
        if process == Process::Washing {
            let recipe = washing().iter().find(|r| r.input == *stack.item)?;
            let mut made = Vec::new();
            for o in &recipe.outputs {
                let times = if o.chance >= 1.0 {
                    n
                } else {
                    (0..n)
                        .filter(|_| self.world.random.next_float() < o.chance)
                        .count() as u32
                };
                if times > 0 {
                    made.push(ItemStack::new(&o.item, o.count * times));
                }
            }
            return Some(made);
        }
        let recipes = self.res.recipes();
        let cooked = if process == Process::Smoking {
            recipes.cook("smoking", stack)
        } else {
            (recipes.cook("blasting", stack)).or_else(|| recipes.cook("smelting", stack))
        }?;
        let mut result = cooked.result.clone();
        result.count *= n;
        Some(vec![result])
    }

    /// Stacks no bigger than their items' most.
    fn tachyon_split(&self, made: Vec<ItemStack>) -> Vec<ItemStack> {
        let catalog = self.res.items();
        let mut out = Vec::new();
        for s in made {
            let most = catalog.get(&s.item).map_or(64, |i| i.max_stack).max(1);
            let mut left = s.count;
            while left > 0 {
                let n = left.min(most);
                out.push(ItemStack {
                    count: n,
                    ..s.clone()
                });
                left -= n;
            }
        }
        out
    }

    /// A window's fans blowing: now and then a puff along each one's air,
    /// and where it works, its process's own (splashes, smoke, flames).
    pub(super) fn tachyon_fan_particles(&mut self) {
        if self.window.is_none() {
            return;
        }
        let fans = self.tachyon_view(|index, shown| turning(&index.parts, &shown.speeds));
        for fan in fans {
            let stream = self.tachyon_stream(fan.pos, fan.front, fan.speed);
            let Some(&(far, _)) = stream.last() else {
                continue;
            };
            let way = fan.way();
            let random = self.particles.random();
            if random.next_float() < 0.3 {
                // From the fan's face (pushing) or the air's far end (pulling).
                let from = if fan.push { fan.pos } else { far };
                let mut at = from.map(|v| f64::from(v) + 0.5);
                for (i, w) in way.iter().enumerate() {
                    if *w == 0.0 {
                        at[i] += random.next_double() - 0.5;
                    } else {
                        at[i] += w * 0.5;
                    }
                }
                let speed = 0.15 + f64::from(fan.speed.abs()) / 1024.0;
                let v = way.map(|w| w * speed);
                self.particles.add(&ParticleOptions::simple(ParticleType::Cloud), at, v);
            }
            for (cell, process) in stream {
                let Some(process) = process else {
                    continue;
                };
                let random = self.particles.random();
                if random.next_float() >= 0.12 {
                    continue;
                }
                let at = [
                    f64::from(cell[0]) + random.next_double(),
                    f64::from(cell[1]) + random.next_double() * 0.6,
                    f64::from(cell[2]) + random.next_double(),
                ];
                let kind = match process {
                    Process::Washing => ParticleType::Splash,
                    Process::Smoking => ParticleType::Smoke,
                    Process::Blasting => ParticleType::Flame,
                };
                let v = way.map(|w| w * 0.05);
                self.particles.add(&ParticleOptions::simple(kind), at, v);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fan_blows_out_of_the_side_it_is_not_driven_from() {
        let parts = HashMap::from([
            ([0, 0, 0], Part::Motor(Dir::East)),
            ([1, 0, 0], Part::Fan(Axis::X)),
        ]);
        assert_eq!(front(&parts, [1, 0, 0], Axis::X, 32.0), (Dir::East, true));
        // Turned the other way, it pulls.
        assert_eq!(front(&parts, [1, 0, 0], Axis::X, -32.0), (Dir::East, false));
        // Alone, it blows the way it turns.
        let parts = HashMap::from([([1, 0, 0], Part::Fan(Axis::Z))]);
        assert_eq!(front(&parts, [1, 0, 0], Axis::Z, -8.0), (Dir::North, true));
    }

    #[test]
    fn what_stays_in_the_air_long_enough_is_worked() {
        let mut timers = HashMap::new();
        for _ in 0..TIME - 1 {
            assert!(tick(&mut timers, vec![(7u32, Process::Washing)]).is_empty());
        }
        assert_eq!(tick(&mut timers, vec![(7, Process::Washing)]), [(7, Process::Washing)]);
        // Out of the air, it starts again.
        tick(&mut timers, vec![(7, Process::Washing)]);
        assert!(tick(&mut timers, Vec::new()).is_empty());
        assert!(timers.is_empty());
    }

    #[test]
    fn the_air_reaches_further_the_faster_the_fan() {
        assert_eq!(range(8.0), 3);
        assert_eq!(range(128.0), 10);
        assert_eq!(range(-256.0), 20);
    }
}
