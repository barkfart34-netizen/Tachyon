//! Kinetica's encased fan. A fan turning blows along its axis, out of the
//! side no part drives it from (with a part on both sides or neither, the
//! way it turns); turned the other way, it pulls. The air goes 3 blocks, and
//! further the faster the fan turns (20 at 256 RPM), until a block with a
//! full collision stops it, and pushes the dropped items in it.
//!
//! What the air goes through works the items further on, dropped ones and
//! those on a depot alike, after [`time`] ticks in it, longer for a bigger
//! stack (as Create's bulk processing): water (or a waterlogged block) washes them by
//! `kinetica/data/kinetica/tachyon/washing.json` (else Create's splashing),
//! soul fire or a lit soul campfire haunts them by Create's haunting,
//! fire or a lit campfire smokes them and lava blasts them by the furnaces'
//! recipes. Haunting follows Create's `AllFanProcessingTypes.HauntingType`
//! and `HauntingRecipe` (MIT, THIRD_PARTY_NOTICES.md).

use super::super::Game;
use super::create_recipes::Recipes;
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

/// Ticks an item spends where the air works it before it is worked, for
/// each 16 of a stack.
pub(super) const TIME: u32 = 150;

/// The ticks a stack of `count` spends where the air works it before it is
/// worked, as Create's: `TIME` for each 16 items begun, and one.
pub(super) fn time(count: u32) -> u32 {
    TIME * ((count.max(1) - 1) / 16 + 1) + 1
}

/// What the air works the items with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Process {
    Washing,
    Haunting,
    Smoking,
    Blasting,
}

impl Process {
    /// Its name in the goggles' keys (`kinetica.hud.washing`).
    pub fn name(self) -> &'static str {
        match self {
            Process::Washing => "washing",
            Process::Haunting => "haunting",
            Process::Smoking => "smoking",
            Process::Blasting => "blasting",
        }
    }
    /// The process a block the air goes through starts, in Create's
    /// `FanProcessingTypeRegistry` order: washing, haunting, smoking, blasting.
    fn of<'a>(name: &str, property: impl Fn(&str) -> Option<&'a str>) -> Option<Self> {
        if matches!(name, "minecraft:water" | "minecraft:bubble_column")
            || property("waterlogged") == Some("true")
        {
            return Some(Process::Washing);
        }
        match name {
            "minecraft:soul_fire" => Some(Process::Haunting),
            "minecraft:soul_campfire" => {
                (property("lit") == Some("true")).then_some(Process::Haunting)
            }
            "minecraft:fire" => Some(Process::Smoking),
            "minecraft:campfire" => (property("lit") == Some("true")).then_some(Process::Smoking),
            "minecraft:lava" => Some(Process::Blasting),
            _ => None,
        }
    }
}

/// The washing recipes of the built-in content, read once.
pub(super) fn washing() -> &'static [Milling] {
    static RECIPES: OnceLock<Vec<Milling>> = OnceLock::new();
    RECIPES.get_or_init(|| millstone::read("washing.json"))
}

/// What washing or haunting makes of a whole stack: each item's outputs
/// rolled by their chances, in Create's `RecipeApplier` and
/// `ProcessingOutput` order. None when no recipe works it; haunting has no
/// washing fallback.
fn work(
    process: Process,
    stack: &ItemStack,
    create: &Recipes,
    roll: &mut impl FnMut() -> f32,
) -> Option<Vec<ItemStack>> {
    let recipe = match process {
        // Kinetica's washing, else Create's splashing.
        Process::Washing => (washing().iter().find(|r| r.input == *stack.item))
            .or_else(|| create.splashing(&stack.item)),
        Process::Haunting => create.haunting(&stack.item),
        _ => None,
    }?;
    let mut counts = vec![0; recipe.outputs.len()];
    for _ in 0..stack.count {
        for (o, count) in recipe.outputs.iter().zip(&mut counts) {
            *count += if o.chance >= 1.0 {
                o.count
            } else {
                (0..o.count).filter(|_| roll() <= o.chance).count() as u32
            };
        }
    }
    Some(
        recipe
            .outputs
            .iter()
            .zip(counts)
            .filter(|(_, count)| *count > 0)
            .map(|(o, count)| ItemStack::new(&o.item, count))
            .collect(),
    )
}

/// How far a fan at `speed` (RPM) blows, as Create's: from 3 blocks still
/// to 20 at 256 RPM, in proportion between (whole blocks).
pub(super) fn range(speed: f32) -> i32 {
    (3.0 + 17.0 * (speed.abs() / 256.0).min(1.0)) as i32
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
/// process changed, due after the [`time`] of its stack's count then, as
/// Create's); the others are forgotten. Those that came due (started again
/// after).
fn tick<K: Copy + Eq + Hash>(
    timers: &mut HashMap<K, (Process, u32, u32)>,
    seen: Vec<(K, Process, u32)>,
) -> Vec<(K, Process)> {
    let mut ready = Vec::new();
    let mut next = HashMap::with_capacity(seen.len());
    for (k, p, count) in seen {
        let (ticks, due) = match timers.get(&k) {
            Some((q, t, due)) if *q == p => (t + 1, *due),
            _ => (1, time(count)),
        };
        if ticks >= due {
            ready.push((k, p));
        } else {
            next.insert(k, (p, ticks, due));
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
            if !self.world.chunks.contains_key(&(at[0] >> 4, at[2] >> 4)) {
                break;
            }
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
        let (fans, chutes) = OWNERS.with_borrow(|owners| {
            let Some(owner) = owners.get(dimension) else {
                return (Vec::new(), HashMap::new());
            };
            let chutes: HashMap<[i32; 3], Part> = owner.index.parts.iter()
                .filter(|(_, part)| matches!(part, Part::Chute(_, _)))
                .map(|(p, part)| (*p, *part)).collect();
            (turning(&owner.index.parts, &owner.shown.speeds), chutes)
        });
        // The cells blown through: the air's way and its process there.
        let mut cells: HashMap<[i32; 3], ([f64; 3], Option<Process>)> = HashMap::new();
        let mut work = HashMap::new();
        let mut chute_feeds = Vec::new();
        for fan in &fans {
            let stream = self.tachyon_stream(fan.pos, fan.front, fan.speed);
            // AirCurrent must reach the line first. ChuteBlockEntity then
            // propagates its signed push or pull through the connections,
            // rather than adding the same fan once per reached block.
            if fan.front.axis() == Axis::Y
                && let Some((at, _)) = stream.iter().find(|(at, _)| chutes.contains_key(at))
            {
                chute_feeds.push((*at, fan.front == Dir::Down, fan.speed.abs() * fan.way()[1] as f32));
            }
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
                items.push((item.id, *process, item.stack.count));
            }
        }
        let (items, depots) = OWNERS.with_borrow_mut(|owners| {
            let owner: &mut Owner = owners.entry(dimension).or_default();
            owner.fan_work = work;
            owner.chute_air = super::chute::airflow(&chutes, &chute_feeds);
            let items = tick(&mut owner.fan_items, items);
            // The depots where the air works, holding something to work.
            let depots: Vec<([i32; 3], Process, u32)> = (cells.iter())
                .filter_map(|(at, (_, p))| Some((*at, (*p)?)))
                .filter(|(at, _)| owner.index.parts.get(at) == Some(&Part::Depot))
                .filter_map(|(at, p)| Some((at, p, owner.mills.get(&at)?.input.as_ref()?.count)))
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

    /// What `process` makes of a whole stack: each item washed or haunted
    /// (the outputs by their chances), or cooked by a smoker's recipe
    /// (smoking) or a blast furnace's, else a furnace's (blasting). None when
    /// nothing works it.
    pub(super) fn tachyon_process(&mut self, process: Process, stack: &ItemStack) -> Option<Vec<ItemStack>> {
        let n = stack.count;
        if matches!(process, Process::Washing | Process::Haunting) {
            let create = self.tachyon_create_recipes();
            return work(process, stack, &create, &mut || self.world.random.next_float());
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
    /// Haunting's soul flames and smoke follow Create's
    /// `AllFanProcessingTypes.HauntingType.spawnProcessingParticles`.
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
                let puff = if process == Process::Haunting {
                    random.next_int_bound(8).unwrap_or(1) == 0
                } else {
                    random.next_float() < 0.12
                };
                if !puff {
                    continue;
                }
                let at = [
                    f64::from(cell[0]) + random.next_double(),
                    f64::from(cell[1])
                        + if process == Process::Haunting {
                            0.45
                        } else {
                            random.next_double() * 0.6
                        },
                    f64::from(cell[2]) + random.next_double(),
                ];
                let kind = match process {
                    Process::Washing => ParticleType::Splash,
                    Process::Haunting => ParticleType::SoulFireFlame,
                    Process::Smoking => ParticleType::Smoke,
                    Process::Blasting => ParticleType::Flame,
                };
                let smoke = process == Process::Haunting && random.next_int_bound(2).unwrap_or(1) == 0;
                let v = if process == Process::Haunting {
                    [0.0; 3]
                } else {
                    way.map(|w| w * 0.05)
                };
                self.particles.add(&ParticleOptions::simple(kind), at, v);
                if smoke {
                    self.particles.add(
                        &ParticleOptions::simple(ParticleType::Smoke),
                        [at[0], at[1] - 0.2, at[2]],
                        [0.0; 3],
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soul_fire_and_a_lit_soul_campfire_haunt_the_air() {
        let lit = |k: &str| (k == "lit").then_some("true");
        let unlit = |k: &str| (k == "lit").then_some("false");
        assert_eq!(Process::of("minecraft:soul_fire", |_| None), Some(Process::Haunting));
        assert_eq!(Process::of("minecraft:soul_campfire", lit), Some(Process::Haunting));
        assert_eq!(Process::of("minecraft:soul_campfire", unlit), None);
        assert_eq!(Process::of("minecraft:soul_campfire", |_| None), None);
        assert_eq!(Process::Haunting.name(), "haunting");
        // Ordinary fire still smokes; lava still blasts.
        assert_eq!(Process::of("minecraft:fire", |_| None), Some(Process::Smoking));
        assert_eq!(Process::of("minecraft:campfire", lit), Some(Process::Smoking));
        assert_eq!(Process::of("minecraft:campfire", unlit), None);
        assert_eq!(Process::of("minecraft:lava", |_| None), Some(Process::Blasting));
        assert_eq!(Process::of("minecraft:stone", |_| None), None);
    }

    #[test]
    fn water_wins_over_a_campfires_fire_in_creates_order() {
        let wet = |k: &str| match k {
            "waterlogged" | "lit" => Some("true"),
            _ => None,
        };
        for name in [
            "minecraft:campfire",
            "minecraft:soul_campfire",
            "minecraft:oak_slab",
        ] {
            assert_eq!(Process::of(name, wet), Some(Process::Washing));
        }
        for name in ["minecraft:water", "minecraft:bubble_column"] {
            assert_eq!(Process::of(name, |_| None), Some(Process::Washing));
        }
    }

    #[test]
    fn haunting_works_a_whole_stack_and_leaves_items_without_its_recipe() {
        let mut create = Recipes::default();
        create.haunting.push(
            serde_json::from_str(
                r#"{"input": "minecraft:stone", "outputs": [{"item": "minecraft:infested_stone", "count": 2}]}"#,
            )
            .unwrap(),
        );
        let mut roll = || -> f32 { panic!("an output always made doesn't roll") };
        let stone = ItemStack::new("minecraft:stone", 17);
        assert_eq!(
            work(Process::Haunting, &stone, &create, &mut roll).unwrap(),
            [ItemStack::new("minecraft:infested_stone", 34)]
        );
        // Even an item that washing works has no haunting fallback.
        create.splashing.push(
            serde_json::from_str(
                r#"{"input": "minecraft:gravel", "outputs": [{"item": "minecraft:flint"}]}"#,
            )
            .unwrap(),
        );
        let gravel = ItemStack::new("minecraft:gravel", 16);
        assert!(work(Process::Haunting, &gravel, &create, &mut roll).is_none());
        assert_eq!(gravel, ItemStack::new("minecraft:gravel", 16));
    }

    #[test]
    fn haunting_rolls_each_items_outputs_by_their_chances() {
        let create = Recipes {
            haunting: vec![serde_json::from_str(
                r#"{"input": "minecraft:lapis_lazuli", "outputs": [
                    {"item": "minecraft:prismarine_shard", "chance": 0.75},
                    {"item": "minecraft:prismarine_crystals", "chance": 0.125}]}"#,
            )
            .unwrap()],
            ..Recipes::default()
        };
        let lapis = ItemStack::new("minecraft:lapis_lazuli", 4);
        let mut roll = || 0.5;
        assert_eq!(
            work(Process::Haunting, &lapis, &create, &mut roll).unwrap(),
            [ItemStack::new("minecraft:prismarine_shard", 4)]
        );
        let mut roll = || 0.9;
        assert!(work(Process::Haunting, &lapis, &create, &mut roll).unwrap().is_empty());
    }

    #[test]
    fn haunting_rolls_each_output_item_before_the_next_input_item() {
        let create = Recipes {
            haunting: vec![serde_json::from_str(
                r#"{"input": "minecraft:lapis_lazuli", "outputs": [
                    {"item": "minecraft:prismarine_shard", "count": 2, "chance": 0.75},
                    {"item": "minecraft:prismarine_crystals", "chance": 0.125}]}"#,
            )
            .unwrap()],
            ..Recipes::default()
        };
        let mut rolls = [0.75, 0.9, 0.125, 0.8, 0.2, 0.5].into_iter();
        let mut roll = || rolls.next().unwrap();
        assert_eq!(
            work(
                Process::Haunting,
                &ItemStack::new("minecraft:lapis_lazuli", 2),
                &create,
                &mut roll,
            )
            .unwrap(),
            [
                ItemStack::new("minecraft:prismarine_shard", 2),
                ItemStack::new("minecraft:prismarine_crystals", 1),
            ]
        );
        assert!(rolls.next().is_none());
    }

    #[test]
    fn changing_to_haunting_starts_a_new_time_for_the_stack() {
        let mut timers = HashMap::new();
        tick(&mut timers, vec![(7u32, Process::Washing, 1)]);
        assert!(tick(&mut timers, vec![(7, Process::Haunting, 17)]).is_empty());
        assert_eq!(timers[&7], (Process::Haunting, 1, time(17)));
        for _ in 1..time(17) - 1 {
            assert!(tick(&mut timers, vec![(7, Process::Haunting, 17)]).is_empty());
        }
        assert_eq!(tick(&mut timers, vec![(7, Process::Haunting, 17)]), [(7, Process::Haunting)]);
    }

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
        // Up to 16 items: 151 ticks; 17 to 32: 301.
        assert_eq!((time(1), time(16), time(17), time(64)), (151, 151, 301, 601));
        for _ in 0..time(16) - 1 {
            assert!(tick(&mut timers, vec![(7u32, Process::Washing, 16)]).is_empty());
        }
        // Due when it began, though the stack grew since.
        assert_eq!(tick(&mut timers, vec![(7, Process::Washing, 17)]), [(7, Process::Washing)]);
        // Out of the air, it starts again.
        tick(&mut timers, vec![(7, Process::Washing, 16)]);
        assert!(tick(&mut timers, Vec::new()).is_empty());
        assert!(timers.is_empty());
    }

    #[test]
    fn the_air_reaches_further_the_faster_the_fan() {
        assert_eq!(range(8.0), 3);
        assert_eq!(range(128.0), 11);
        assert_eq!(range(-256.0), 20);
    }
}
