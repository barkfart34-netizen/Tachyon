//! Create's fluids (FluidPipeBlock, PumpBlock/PumpBlockEntity, FluidNetwork, OpenEndedPipe,
//! FluidTankBlock, SpoutBlockEntity, ItemDrainBlockEntity, GenericItemFilling/Emptying; MIT,
//! THIRD_PARTY_NOTICES.md), as Tachyon runs them:
//! - A fluid is a stack named `fluid:<id>` counted in mB (`FLUID`), so a basin's stacks and
//!   Create's recipes carry them as items.
//! - A pipe connects where `connects` says (FluidPipeBlock.canConnectTo: pipes, a pump along
//!   its axis, tanks, basins, spouts, drains), and a pipe with one connection opens its other
//!   end too (`FluidPipeBlock.updateBlockState`).
//! - A turning pump pulls through the pipes behind it and pushes out of its front (its
//!   facing, whichever way it turns), `max(1, |RPM| / 2)` mB a tick (FluidNetwork's
//!   `transferSpeed`), the pipes reached within `RANGE` blocks (`mechanicalPumpRange`).
//! - An open pipe end draws a source block in front of it (water without end, as Create's
//!   bottomless water; lava and others are taken) and pours a source block out per bucket.
//! - A tank holds 8 buckets a block (`fluidTankCapacity`); a tank's fluid sinks into the tank
//!   under it. A spout (1 bucket) fills the item on a depot two blocks below in 20 ticks
//!   (`FILLING_TIME`), an item drain (1.5 buckets) empties the items on it in 20 ticks.
//!
//! Not as Create: tanks don't merge into wider multiblocks (a column shares by sinking),
//! fluids aren't drawn in pipes, tanks or basins (the goggles read them), no valves, smart
//! pipes, hose pulleys or potions, and branching pipes fill their ends in turn, not by
//! Create's pressure split.
use super::kinetic::{Axis, Dir, Part};
use super::millstone::Held;
use horizon_client::items::ItemStack;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

/// The prefix of a fluid's stack name.
pub(super) const FLUID: &str = "fluid:";
/// `mechanicalPumpRange`: the pipes a pump reaches.
pub(super) const RANGE: usize = 16;
/// A bucket, in mB.
pub(super) const BUCKET: u32 = 1000;
/// `FILLING_TIME` of the spout and the item drain.
pub(super) const FILLING_TIME: u32 = 20;

/// The fluids Tachyon knows: Minecraft's, NeoForge's milk and Create's own.
pub(super) const FLUIDS: [&str; 6] = [
    "minecraft:water",
    "minecraft:lava",
    "minecraft:milk",
    "create:honey",
    "create:chocolate",
    "create:tea",
];

/// Whether a stack name is a fluid's.
pub(super) fn is_fluid(item: &str) -> bool {
    item.starts_with(FLUID)
}

/// A fluid's stack name.
pub(super) fn stack_name(fluid: &str) -> String {
    format!("{FLUID}{fluid}")
}

/// A fluid's name for people.
pub(super) fn fluid_name(fluid: &str) -> &'static str {
    match fluid.strip_prefix(FLUID).unwrap_or(fluid) {
        "minecraft:water" => "Water",
        "minecraft:lava" => "Lava",
        "minecraft:milk" => "Milk",
        "create:honey" => "Honey",
        "create:chocolate" => "Chocolate",
        "create:tea" => "Builder's Tea",
        _ => "Fluid",
    }
}

/// The bucket a fluid fills (GenericItemFilling: a bucket and 1000 mB).
pub(super) fn bucket_of(fluid: &str) -> Option<&'static str> {
    Some(match fluid.strip_prefix(FLUID).unwrap_or(fluid) {
        "minecraft:water" => "minecraft:water_bucket",
        "minecraft:lava" => "minecraft:lava_bucket",
        "minecraft:milk" => "minecraft:milk_bucket",
        "create:honey" => "create:honey_bucket",
        "create:chocolate" => "create:chocolate_bucket",
        _ => return None,
    })
}

/// The fluid a filled bucket empties (GenericItemEmptying).
pub(super) fn fluid_of_bucket(item: &str) -> Option<&'static str> {
    FLUIDS.into_iter().find(|f| bucket_of(f) == Some(item))
}

/// A fluid's candidates for a fluid tag (`c:honey`, `minecraft:water`): Create's and
/// Minecraft's fluid of that name.
pub(super) fn tagged(tag: &str) -> Vec<&'static str> {
    let name = tag.rsplit(':').next().unwrap_or(tag);
    FLUIDS
        .into_iter()
        .filter(|f| f.rsplit(':').next() == Some(name))
        .collect()
}

/// One fluid and how much of it (a tank, a spout's or drain's, an open end's buffer).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Tank {
    #[serde(default)]
    pub fluid: String,
    #[serde(default)]
    pub amount: u32,
}

impl Tank {
    /// Fills in up to `amount` of `fluid` while it fits in `capacity`: how much it took.
    pub fn fill(&mut self, fluid: &str, amount: u32, capacity: u32) -> u32 {
        if self.amount > 0 && self.fluid != fluid {
            return 0;
        }
        let n = amount.min(capacity.saturating_sub(self.amount));
        if n > 0 {
            self.fluid = fluid.to_owned();
            self.amount += n;
        }
        n
    }
    /// Takes out up to `amount`: how much it gave.
    pub fn drain(&mut self, amount: u32) -> u32 {
        let n = amount.min(self.amount);
        self.amount -= n;
        if self.amount == 0 {
            self.fluid.clear();
        }
        n
    }
}

/// What a fluid part holds, in mB.
pub(super) fn capacity(part: Part) -> u32 {
    match part {
        Part::Tank => 8 * BUCKET,
        Part::Spout => BUCKET,
        Part::Drain => 1500,
        _ => 0,
    }
}

/// A part's bit for side `d` in a pipe's mask.
pub(super) fn bit(d: Dir) -> u8 {
    1 << Dir::ALL.iter().position(|x| *x == d).unwrap_or(0)
}

/// Whether a pipe beside `neighbour` (on its side `d`, from the pipe) connects to it
/// (FluidPipeBlock.canConnectTo).
pub(super) fn connects(neighbour: Option<Part>, d: Dir) -> bool {
    match neighbour {
        Some(Part::Pipe(_) | Part::Tank | Part::CreativeTank | Part::Spout | Part::Drain | Part::Basin) => true,
        Some(Part::Pump(facing)) => facing.axis() == d.axis(),
        Some(Part::Valve { facing, along_first }) => valve_axis(facing, along_first) == d.axis(),
        Some(Part::SmartPipe(a)) => a == d.axis(),
        // HosePulleyBlock.hasPipeTowards: its counter-clockwise side.
        Some(Part::HosePulley(facing)) => super::hose::counter_clockwise(facing) == d.opposite(),
        _ => false,
    }
}

/// FluidValveBlock.getPipeAxis: the axis that is neither its facing's nor its shaft's.
pub(super) fn valve_axis(facing: Dir, along_first: bool) -> Axis {
    let shaft = super::deployer::shaft_axis(facing, along_first);
    [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .find(|a| *a != facing.axis() && *a != shaft)
        .unwrap_or(Axis::Y)
}

/// A straight pipe's sides along `axis`.
pub(super) fn straight(axis: Axis) -> u8 {
    let [a, b] = axis.dirs();
    bit(a) | bit(b)
}

/// `FluidPipeBlock.updateBlockState`: the sides a pipe opens, by which of its neighbours it
/// connects to: one alone opens the other end too, none keeps a straight pipe (`before`, if
/// it was one) or one along `preferred`.
pub(super) fn pipe_sides(connected: u8, before: u8, preferred: Dir) -> u8 {
    let count = connected.count_ones();
    if count >= 2 {
        return connected;
    }
    if count == 1 {
        let d = Dir::ALL.into_iter().find(|d| connected & bit(*d) != 0).unwrap_or(preferred);
        return connected | bit(d.opposite());
    }
    if before.count_ones() == 2 {
        return before;
    }
    bit(preferred) | bit(preferred.opposite())
}

/// An end a pump reaches through its pipes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum End {
    /// A tank, basin, spout or drain.
    Holder([i32; 3]),
    /// The open end of a pipe, into the space at this position.
    Open([i32; 3]),
    /// A hose pulley: what its hose reaches (`hose.rs`).
    Hose([i32; 3]),
}

fn add(a: [i32; 3], d: Dir) -> [i32; 3] {
    let o = d.offset();
    [a[0] + o[0], a[1] + o[1], a[2] + o[2]]
}

/// The ends a pump at `pump` reaches out of its side `side`: what is right there, or through
/// the pipes from there (within `RANGE`), nearest first.
pub(super) fn ends(parts: &HashMap<[i32; 3], Part>, pump: [i32; 3], side: Dir) -> Vec<End> {
    let first = add(pump, side);
    match parts.get(&first) {
        Some(Part::Tank | Part::CreativeTank | Part::Spout | Part::Drain | Part::Basin) => {
            return vec![End::Holder(first)];
        }
        Some(Part::HosePulley(f)) if super::hose::counter_clockwise(*f) == side.opposite() => {
            return vec![End::Hose(first)];
        }
        Some(Part::Pipe(mask)) if mask & bit(side.opposite()) != 0 => {}
        _ => return Vec::new(),
    }
    let mut out = Vec::new();
    let mut seen: HashSet<[i32; 3]> = HashSet::from([pump, first]);
    let mut queue = VecDeque::from([(first, 1usize)]);
    while let Some((p, distance)) = queue.pop_front() {
        let Some(Part::Pipe(mask)) = parts.get(&p) else { continue };
        for d in Dir::ALL {
            if mask & bit(d) == 0 {
                continue;
            }
            let q = add(p, d);
            if q == pump || !seen.insert(q) {
                continue;
            }
            match parts.get(&q) {
                Some(Part::Pipe(m)) if m & bit(d.opposite()) != 0 => {
                    if distance < RANGE {
                        queue.push_back((q, distance + 1));
                    }
                }
                Some(Part::Tank | Part::CreativeTank | Part::Spout | Part::Drain | Part::Basin) => {
                    out.push(End::Holder(q));
                }
                Some(Part::HosePulley(f)) if super::hose::counter_clockwise(*f) == d.opposite() => {
                    out.push(End::Hose(q));
                }
                // Another pump, a pipe turned away, a closed valve or a pulley's side: no way on.
                Some(Part::Pump(_) | Part::Pipe(_) | Part::Valve { .. } | Part::HosePulley(_)) => {}
                _ => out.push(End::Open(q)),
            }
        }
    }
    out
}

/// `FluidNetwork.transferSpeed` for a pump at `speed`: mB a tick.
pub(super) fn rate(speed: f32) -> u32 {
    ((speed.abs() / 2.0) as u32).max(1)
}

/// The world beside an open pipe end, as the fluids need it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Space {
    /// A source block of this fluid.
    Source(&'static str),
    /// Air or something a fluid washes away.
    Free,
    /// Anything else.
    Blocked,
}

impl super::super::Game<'_> {
    /// What is at `p` for an open pipe end.
    fn tachyon_space(&self, p: [i32; 3]) -> Space {
        if !self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4)) {
            return Space::Blocked;
        }
        let state = self.world.block(p[0], p[1], p[2]);
        let Some(s) = self.pack.assets.registry.state(state) else {
            return Space::Blocked;
        };
        let level = s.properties.get("level").map(String::as_str);
        match s.name.as_str() {
            "minecraft:water" if level == Some("0") => Space::Source("minecraft:water"),
            "minecraft:lava" if level == Some("0") => Space::Source("minecraft:lava"),
            _ if self.res.facts().get(state).replaceable => Space::Free,
            _ => Space::Blocked,
        }
    }

    /// The owner's fluids a tick: each turning pump moves its fluid, open ends pour, tanks
    /// settle, spouts fill and drains empty.
    pub(super) fn tachyon_fluids(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let (parts, pumps) = super::OWNERS.with_borrow(|owners| {
            let Some(o) = owners.get(dimension) else {
                return (HashMap::new(), Vec::new());
            };
            let parts: HashMap<[i32; 3], Part> = (o.index.parts.iter())
                .filter_map(|(p, part)| {
                    let turning = o.shown.speeds.get(p).is_some_and(|s| s[0] != 0.0);
                    let part = match *part {
                        Part::Pipe(_)
                        | Part::Pump(_)
                        | Part::Tank
                        | Part::CreativeTank
                        | Part::Spout
                        | Part::Drain
                        | Part::Basin
                        | Part::HosePulley(_) => *part,
                        Part::SmartPipe(a) => Part::Pipe(straight(a)),
                        // FluidValveBlockEntity: open while its shaft turns.
                        Part::Valve { facing, along_first } if turning => {
                            Part::Pipe(straight(valve_axis(facing, along_first)))
                        }
                        Part::Valve { .. } => *part,
                        _ => return None,
                    };
                    Some((*p, part))
                })
                .collect();
            let mut pumps: Vec<([i32; 3], Dir, f32)> = (parts.iter())
                .filter_map(|(p, part)| match part {
                    Part::Pump(facing) => {
                        let speed = o.shown.speeds.get(p).map_or(0.0, |s| s[0]);
                        (speed != 0.0).then_some((*p, *facing, speed))
                    }
                    _ => None,
                })
                .collect();
            pumps.sort_by_key(|p| p.0);
            (parts, pumps)
        });
        self.tachyon_valves();
        if parts.is_empty() {
            return;
        }
        for (pos, facing, speed) in pumps {
            let from = ends(&parts, pos, facing.opposite());
            let to = ends(&parts, pos, facing);
            self.tachyon_pump(&parts, &from, &to, rate(speed));
        }
        self.tachyon_tanks_settle(&parts);
        self.tachyon_spouts(&parts);
        self.tachyon_drains(&parts);
    }

    /// Moves up to `rate` mB from the first of `from` that has fluid into `to`, in turn.
    fn tachyon_pump(&mut self, parts: &HashMap<[i32; 3], Part>, from: &[End], to: &[End], rate: u32) {
        for source in from {
            let Some((fluid, have)) = self.tachyon_fluid_peek(parts, *source) else {
                continue;
            };
            let mut left = have.min(rate);
            let mut moved = 0;
            for target in to {
                if left == 0 {
                    break;
                }
                let n = self.tachyon_fluid_offer(parts, *target, &fluid, left);
                left -= n;
                moved += n;
            }
            if moved > 0 {
                self.tachyon_fluid_take(parts, *source, moved);
                return;
            }
        }
    }

    /// What fluid an end could give, and how much.
    fn tachyon_fluid_peek(&mut self, parts: &HashMap<[i32; 3], Part>, end: End) -> Option<(String, u32)> {
        let dimension = self.native_dimension();
        match end {
            End::Holder(p) => super::OWNERS.with_borrow(|owners| {
                let o = owners.get(dimension)?;
                if parts.get(&p) == Some(&Part::Basin) {
                    let b = o.basins.get(&p)?;
                    let h = b.outputs.iter().find(|h| is_fluid(&h.item))?;
                    return Some((h.item[FLUID.len()..].to_owned(), h.count));
                }
                let t = o.tanks.get(&p)?;
                // CreativeFluidTankBlockEntity: never runs out.
                if parts.get(&p) == Some(&Part::CreativeTank) {
                    return (t.amount > 0).then(|| (t.fluid.clone(), 64 * BUCKET));
                }
                (t.amount > 0).then(|| (t.fluid.clone(), t.amount))
            }),
            End::Open(p) => {
                let buffered = super::OWNERS.with_borrow(|owners| {
                    owners.get(dimension).and_then(|o| o.open_ends.get(&p)).filter(|t| t.amount > 0).cloned()
                });
                if let Some(t) = buffered {
                    return Some((t.fluid, t.amount));
                }
                let Space::Source(fluid) = self.tachyon_space(p) else {
                    return None;
                };
                // Water without end (Create's bottomless water); other sources are taken.
                if fluid != "minecraft:water" {
                    let air = self.pack.assets.registry.default_state("minecraft:air")?;
                    if !self.native_set_block(dimension, p, air, 3) {
                        return None;
                    }
                }
                super::OWNERS.with_borrow_mut(|owners| {
                    let o = owners.get_mut(dimension)?;
                    o.open_ends.insert(p, Tank { fluid: fluid.to_owned(), amount: BUCKET });
                    Some((fluid.to_owned(), BUCKET))
                })
            }
            End::Hose(p) => self.tachyon_hose_peek(p),
        }
    }

    /// Takes `amount` from an end that `tachyon_fluid_peek` showed has it.
    fn tachyon_fluid_take(&mut self, parts: &HashMap<[i32; 3], Part>, end: End, amount: u32) {
        let dimension = self.native_dimension();
        super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return };
            match end {
                End::Holder(p) if parts.get(&p) == Some(&Part::Basin) => {
                    if let Some(b) = o.basins.get_mut(&p) {
                        b.take_fluid(amount);
                    }
                }
                End::Holder(p) if parts.get(&p) == Some(&Part::CreativeTank) => {}
                End::Holder(p) => {
                    if let Some(t) = o.tanks.get_mut(&p) {
                        t.drain(amount);
                    }
                }
                End::Open(p) | End::Hose(p) => {
                    if let Some(t) = o.open_ends.get_mut(&p) {
                        t.drain(amount);
                    }
                    o.open_ends.retain(|_, t| t.amount > 0);
                }
            }
            o.mills_changed = true;
        });
    }

    /// Puts up to `amount` of `fluid` into an end: how much it took. An open end pours a
    /// source block for each bucket, where it is free.
    fn tachyon_fluid_offer(&mut self, parts: &HashMap<[i32; 3], Part>, end: End, fluid: &str, amount: u32) -> u32 {
        let dimension = self.native_dimension();
        match end {
            End::Holder(p) => super::OWNERS.with_borrow_mut(|owners| {
                let Some(o) = owners.get_mut(dimension) else { return 0 };
                let part = parts.get(&p).copied();
                if part == Some(Part::CreativeTank) {
                    return 0;
                }
                // A running boiler uses up the water it takes (`steam.rs`).
                if fluid == "minecraft:water"
                    && let Some(bottom) = o.boiling.get(&p).copied()
                {
                    *o.boiled.entry(bottom).or_insert(0) += amount;
                    return amount;
                }
                let n = if part == Some(Part::Basin) {
                    o.basins.entry(p).or_default().put(&stack_name(fluid), amount)
                } else {
                    let cap = part.map_or(0, capacity);
                    o.tanks.entry(p).or_default().fill(fluid, amount, cap)
                };
                o.mills_changed |= n > 0;
                n
            }),
            End::Open(p) => {
                let placeable = matches!(fluid, "minecraft:water" | "minecraft:lava");
                if !placeable || self.tachyon_space(p) != Space::Free {
                    return 0;
                }
                let n = super::OWNERS.with_borrow_mut(|owners| {
                    let Some(o) = owners.get_mut(dimension) else { return 0 };
                    o.open_ends.entry(p).or_default().fill(fluid, amount, BUCKET)
                });
                let full = super::OWNERS.with_borrow(|owners| {
                    owners.get(dimension).and_then(|o| o.open_ends.get(&p)).is_some_and(|t| t.amount >= BUCKET)
                });
                if full
                    && let Some(source) = self.pack.assets.registry.default_state(fluid)
                    && self.native_set_block(dimension, p, source, 3)
                {
                    super::OWNERS.with_borrow_mut(|owners| {
                        if let Some(o) = owners.get_mut(dimension) {
                            o.open_ends.remove(&p);
                        }
                    });
                }
                n
            }
            End::Hose(p) => self.tachyon_hose_offer(p, fluid, amount),
        }
    }

    /// FluidValveBlockEntity: a valve's `enabled` follows its shaft, open while it turns.
    fn tachyon_valves(&mut self) {
        let dimension = self.native_dimension();
        let valves: Vec<([i32; 3], bool)> = super::OWNERS.with_borrow(|owners| {
            let Some(o) = owners.get(dimension) else { return Vec::new() };
            (o.index.parts.iter())
                .filter(|(_, part)| matches!(part, Part::Valve { .. }))
                .map(|(p, _)| (*p, o.shown.speeds.get(p).is_some_and(|s| s[0] != 0.0)))
                .collect()
        });
        let registry = self.pack.assets.registry;
        for (p, open) in valves {
            let value = if open { "true" } else { "false" };
            if self.native_block_property(p, "enabled").as_deref() == Some(value) {
                continue;
            }
            let state = self.world.block(p[0], p[1], p[2]);
            let Some(mut s) = registry.state(state).cloned() else { continue };
            s.properties.insert("enabled".into(), value.into());
            if let Ok(id) = registry.resolve(&s) {
                self.native_set_block(dimension, p, id, 2);
            }
        }
    }

    /// A tank's fluid sinks into the tank under it.
    fn tachyon_tanks_settle(&mut self, parts: &HashMap<[i32; 3], Part>) {
        let dimension = self.native_dimension();
        super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return };
            let mut tanks: Vec<[i32; 3]> = (o.tanks.keys())
                .filter(|p| parts.get(*p) == Some(&Part::Tank))
                .copied()
                .collect();
            tanks.sort_by_key(|p| p[1]);
            for p in tanks {
                let below = [p[0], p[1] - 1, p[2]];
                if parts.get(&below) != Some(&Part::Tank) {
                    continue;
                }
                let Some(top) = o.tanks.get(&p).cloned().filter(|t| t.amount > 0) else { continue };
                let n = o.tanks.entry(below).or_default().fill(&top.fluid, top.amount, 8 * BUCKET);
                if n > 0 {
                    if let Some(t) = o.tanks.get_mut(&p) {
                        t.drain(n);
                    }
                    o.mills_changed = true;
                }
            }
            o.tanks.retain(|p, t| t.amount > 0 && parts.contains_key(p));
        });
    }

    /// The fill a spout's fluid gives the item `item`: its fluid amount and what it makes
    /// (Create's filling recipes, then a bucket).
    fn tachyon_filling(&mut self, item: &str, fluid: &str) -> Option<(u32, Vec<Held>)> {
        let recipes = self.tachyon_create_recipes();
        if let Some(r) = recipes.filling.iter().find(|r| r.fluid == fluid && r.input.iter().any(|i| i == item)) {
            let made = r.outputs.iter().map(|o| Held { item: o.item.clone(), count: o.count }).collect();
            return Some((r.amount, made));
        }
        if item == "minecraft:bucket" {
            let full = bucket_of(fluid)?;
            if self.res.items().get(full).is_some() {
                return Some((BUCKET, vec![Held { item: full.to_owned(), count: 1 }]));
            }
        }
        None
    }

    /// What an item empties into: the item left and the fluid (Create's emptying recipes,
    /// then a bucket).
    fn tachyon_emptying(&mut self, item: &str) -> Option<(Held, String, u32)> {
        let recipes = self.tachyon_create_recipes();
        if let Some(r) = recipes.emptying.iter().find(|r| r.input.iter().any(|i| i == item)) {
            let left = r.outputs.iter().find(|o| !is_fluid(&o.item))?;
            let fluid = r.outputs.iter().find(|o| is_fluid(&o.item))?;
            return Some((
                Held { item: left.item.clone(), count: left.count },
                fluid.item[FLUID.len()..].to_owned(),
                fluid.count,
            ));
        }
        let fluid = fluid_of_bucket(item)?;
        Some((Held { item: "minecraft:bucket".into(), count: 1 }, fluid.to_owned(), BUCKET))
    }

    /// Each spout fills one item of the stack on the depot two blocks under it, in
    /// `FILLING_TIME` ticks, when it holds the fluid for it.
    fn tachyon_spouts(&mut self, parts: &HashMap<[i32; 3], Part>) {
        let dimension = self.native_dimension();
        let mut spouts: Vec<[i32; 3]> = (parts.iter()).filter(|(_, p)| **p == Part::Spout).map(|(p, _)| *p).collect();
        spouts.sort();
        for pos in spouts {
            let depot = [pos[0], pos[1] - 2, pos[2]];
            if !self.tachyon_is_depot(depot) {
                continue;
            }
            let (item, tank) = super::OWNERS.with_borrow(|owners| {
                let o = owners.get(dimension);
                let item = o.and_then(|o| o.mills.get(&depot)).and_then(|m| m.input.as_ref()).map(|h| h.item.clone());
                (item, o.and_then(|o| o.tanks.get(&pos)).cloned().unwrap_or_default())
            });
            let fill = item.as_deref().filter(|_| tank.amount > 0).and_then(|i| self.tachyon_filling(i, &tank.fluid));
            let Some((amount, made)) = fill.filter(|(amount, _)| tank.amount >= *amount) else {
                super::OWNERS.with_borrow_mut(|owners| {
                    if let Some(o) = owners.get_mut(dimension) {
                        o.spouting.remove(&pos);
                    }
                });
                continue;
            };
            let input = item.unwrap_or_default();
            super::OWNERS.with_borrow_mut(|owners| {
                let Some(o) = owners.get_mut(dimension) else { return };
                let ticks = o.spouting.entry(pos).or_insert(0);
                *ticks += 1;
                if *ticks < FILLING_TIME {
                    return;
                }
                o.spouting.remove(&pos);
                let Some(mill) = o.mills.get_mut(&depot) else { return };
                let Some(h) = mill.input.as_mut().filter(|h| h.item == input) else { return };
                h.count -= 1;
                if h.count == 0 {
                    mill.input = None;
                }
                super::millstone::add_all(&mut mill.outputs, &made);
                if let Some(t) = o.tanks.get_mut(&pos) {
                    t.drain(amount);
                }
                o.mills_changed = true;
            });
        }
    }

    /// Whether the block at `p` is a depot.
    fn tachyon_is_depot(&self, p: [i32; 3]) -> bool {
        let dimension = self.native_dimension();
        super::OWNERS.with_borrow(|owners| {
            owners.get(dimension).is_some_and(|o| o.index.parts.get(&p) == Some(&Part::Depot))
        })
    }

    /// Each item drain empties one item of the stack on it in `FILLING_TIME` ticks, when its
    /// fluid fits.
    fn tachyon_drains(&mut self, parts: &HashMap<[i32; 3], Part>) {
        let dimension = self.native_dimension();
        let mut drains: Vec<[i32; 3]> = (parts.iter()).filter(|(_, p)| **p == Part::Drain).map(|(p, _)| *p).collect();
        drains.sort();
        for pos in drains {
            let item = super::OWNERS.with_borrow(|owners| {
                owners.get(dimension).and_then(|o| o.mills.get(&pos)).and_then(|m| m.input.as_ref()).map(|h| h.item.clone())
            });
            let Some(item) = item else { continue };
            let Some((left, fluid, amount)) = self.tachyon_emptying(&item) else { continue };
            super::OWNERS.with_borrow_mut(|owners| {
                let Some(o) = owners.get_mut(dimension) else { return };
                let room = {
                    let t = o.tanks.get(&pos).cloned().unwrap_or_default();
                    (t.amount == 0 || t.fluid == fluid) && t.amount + amount <= capacity(Part::Drain)
                };
                if !room {
                    return;
                }
                let ticks = o.spouting.entry(pos).or_insert(0);
                *ticks += 1;
                if *ticks < FILLING_TIME {
                    return;
                }
                o.spouting.remove(&pos);
                let Some(mill) = o.mills.get_mut(&pos) else { return };
                let Some(h) = mill.input.as_mut().filter(|h| h.item == item) else { return };
                h.count -= 1;
                if h.count == 0 {
                    mill.input = None;
                }
                super::millstone::add_all(&mut mill.outputs, std::slice::from_ref(&left));
                o.tanks.entry(pos).or_default().fill(&fluid, amount, capacity(Part::Drain));
                o.mills_changed = true;
            });
        }
    }

    /// A bucket used on a tank or a spout: a full one pours in, an empty one takes a bucket
    /// out. Whether it did.
    pub(super) fn tachyon_use_tank(&mut self, pos: [i32; 3], part: Part) -> bool {
        let dimension = self.native_dimension();
        let Some(held) = self.held().map(|s| s.item.to_string()) else {
            return false;
        };
        if part == Part::CreativeTank {
            if let Some(fluid) = fluid_of_bucket(&held) {
                super::OWNERS.with_borrow_mut(|owners| {
                    if let Some(o) = owners.get_mut(dimension) {
                        o.tanks.insert(pos, Tank { fluid: fluid.to_owned(), amount: BUCKET });
                        o.mills_changed = true;
                    }
                });
                return true;
            }
            if held == "minecraft:bucket" {
                let full = super::OWNERS.with_borrow(|owners| {
                    owners.get(dimension).and_then(|o| o.tanks.get(&pos)).and_then(|t| bucket_of(&t.fluid))
                });
                if let Some(full) = full {
                    self.tachyon_spend_held(1);
                    self.give_or_drop(ItemStack::new(full, 1));
                    return true;
                }
            }
            return false;
        }
        if let Some(fluid) = fluid_of_bucket(&held) {
            let took = super::OWNERS.with_borrow_mut(|owners| {
                let o = owners.get_mut(dimension)?;
                let t = o.tanks.entry(pos).or_default();
                let cap = capacity(part);
                if t.amount + BUCKET > cap || (t.amount > 0 && t.fluid != fluid) {
                    return None;
                }
                t.fill(fluid, BUCKET, cap);
                o.mills_changed = true;
                Some(())
            });
            if took.is_some() && !self.creative {
                self.tachyon_spend_held(1);
                self.give_or_drop(ItemStack::new("minecraft:bucket", 1));
            }
            return took.is_some();
        }
        if held == "minecraft:bucket" {
            let got = super::OWNERS.with_borrow_mut(|owners| {
                let o = owners.get_mut(dimension)?;
                let t = o.tanks.get_mut(&pos)?;
                let full = bucket_of(&t.fluid).filter(|_| t.amount >= BUCKET)?;
                t.drain(BUCKET);
                o.mills_changed = true;
                Some(full)
            });
            if let Some(full) = got {
                self.tachyon_spend_held(1);
                self.give_or_drop(ItemStack::new(full, 1));
                return true;
            }
        }
        false
    }

    /// A bucket used on a basin: a full one pours into its fluids, an empty one takes a
    /// bucket of what it made (else of what it holds to mix). Whether it did.
    pub(super) fn tachyon_use_basin_fluid(&mut self, basin: &mut super::basin::Basin) -> bool {
        let Some(held) = self.held().map(|s| s.item.to_string()) else {
            return false;
        };
        if let Some(fluid) = fluid_of_bucket(&held) {
            // All of it, or none.
            if basin.fluid_room(&stack_name(fluid)) < BUCKET {
                return true;
            }
            basin.put(&stack_name(fluid), BUCKET);
            if !self.creative {
                self.tachyon_spend_held(1);
                self.give_or_drop(ItemStack::new("minecraft:bucket", 1));
            }
            return true;
        }
        if held == "minecraft:bucket"
            && let Some(fluid) = basin.bucket_out()
            && let Some(full) = bucket_of(&fluid)
        {
            self.tachyon_spend_held(1);
            self.give_or_drop(ItemStack::new(full, 1));
            return true;
        }
        false
    }
}

fn dir_of(d: horizon_core::geometry::Direction) -> Dir {
    use horizon_core::geometry::Direction as D;
    match d {
        D::Down => Dir::Down,
        D::Up => Dir::Up,
        D::North => Dir::North,
        D::South => Dir::South,
        D::West => Dir::West,
        D::East => Dir::East,
    }
}

impl super::super::GameLevel<'_> {
    /// The engine's part a block state is.
    fn tachyon_part_of(&self, state: horizon_world::registry::BlockStateId) -> Option<Part> {
        let s = self.registry.state(state)?;
        super::part_of(&s.name, |k| s.properties.get(k).map(String::as_str))
    }

    /// The part beside `pos` on side `d`, `known` standing for that side's block when given.
    fn tachyon_part_beside(
        &self,
        pos: [i32; 3],
        d: Dir,
        known: Option<(Dir, horizon_world::registry::BlockStateId)>,
    ) -> Option<Part> {
        if let Some((side, state)) = known
            && side == d
        {
            return self.tachyon_part_of(state);
        }
        let q = add(pos, d);
        self.tachyon_part_of(self.world.block(q[0], q[1], q[2]))
    }

    /// A pipe's state at `pos` with its sides opened by `pipe_sides`.
    fn tachyon_pipe_state(
        &self,
        state: horizon_world::registry::BlockStateId,
        pos: [i32; 3],
        preferred: Dir,
        known: Option<(Dir, horizon_world::registry::BlockStateId)>,
    ) -> Option<horizon_world::registry::BlockStateId> {
        let before = match self.tachyon_part_of(state) {
            Some(Part::Pipe(mask)) => mask,
            _ => 0,
        };
        let connected = Dir::ALL
            .into_iter()
            .filter(|d| connects(self.tachyon_part_beside(pos, *d, known), *d))
            .fold(0, |m, d| m | bit(d));
        let sides = pipe_sides(connected, before, preferred);
        let mut out = state;
        for d in Dir::ALL {
            let open = if sides & bit(d) != 0 { "true" } else { "false" };
            out = self.native_state_with_property(out, d.name(), open)?;
        }
        Some(out)
    }

    /// The axis a straight pipe placed at `pos` lies along: toward a pipe beside it, else the look.
    fn tachyon_pipe_axis(&self, pos: [i32; 3], looking: Dir) -> Axis {
        Dir::ALL
            .into_iter()
            .find(|d| connects(self.tachyon_part_beside(pos, *d, None), *d))
            .map_or(looking.axis(), Dir::axis)
    }

    /// A tank's `top` and `bottom` (FluidTankBlock): whether it ends its column there.
    fn tachyon_tank_state(
        &self,
        state: horizon_world::registry::BlockStateId,
        pos: [i32; 3],
        known: Option<(Dir, horizon_world::registry::BlockStateId)>,
    ) -> Option<horizon_world::registry::BlockStateId> {
        let kind = self.tachyon_part_of(state);
        let tank = |d| self.tachyon_part_beside(pos, d, known) == kind;
        let top = if tank(Dir::Up) { "false" } else { "true" };
        let bottom = if tank(Dir::Down) { "false" } else { "true" };
        let s = self.native_state_with_property(state, "top", top)?;
        self.native_state_with_property(s, "bottom", bottom)
    }

    /// Placing a pipe (`FluidPipeBlock.getStateForPlacement`), a pump (PumpBlock's: facing
    /// the player, or toward a pipe beside it off that axis) or a tank.
    pub(in crate::game) fn tachyon_fluid_place(
        &self,
        state: horizon_world::registry::BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
    ) {
        if answer.is_some() {
            return;
        }
        let pos = [context.pos.x, context.pos.y, context.pos.z];
        let looking = super::drill::looking(context.yaw, context.pitch);
        let placed = match self.native_state_block(state) {
            Some("create:fluid_pipe") => self.tachyon_pipe_state(state, pos, looking, None),
            Some("create:fluid_tank" | "create:creative_fluid_tank") => self.tachyon_tank_state(state, pos, None),
            Some("create:mechanical_pump") => {
                let target = if context.sneaking { looking } else { looking.opposite() };
                let t = target.offset();
                let best = Dir::ALL
                    .into_iter()
                    .filter(|d| connects(self.tachyon_part_beside(pos, *d, None), *d))
                    .min_by_key(|d| {
                        let o = d.offset();
                        (0..3).map(|i| (o[i] - t[i]).pow(2)).sum::<i32>()
                    });
                let facing = match best {
                    Some(d) if d.axis() != target.axis() && !context.sneaking => d,
                    _ => target,
                };
                self.native_state_with_property(state, "facing", facing.name())
            }
            // DirectionalAxisKineticBlock with FluidValveBlock.prefersConnectionTo: its pipe along
            // a pipe beside it, else along the look; its facing up, or the player's way.
            Some("create:fluid_valve") => {
                let pipe = self.tachyon_pipe_axis(pos, looking);
                let facing = if pipe == Axis::Y { super::hose::horizontal(context.yaw).opposite() } else { Dir::Up };
                let along = [false, true].into_iter().find(|a| valve_axis(facing, *a) == pipe).unwrap_or(false);
                self.native_state_with_property(state, "facing", facing.name())
                    .and_then(|s| self.native_state_with_property(s, "axis_along_first", if along { "true" } else { "false" }))
                    .and_then(|s| self.native_state_with_property(s, "enabled", "false"))
            }
            // SmartFluidPipeBlock.getStateForPlacement: on a wall for an upright pipe.
            Some("create:smart_fluid_pipe") => {
                let pipe = self.tachyon_pipe_axis(pos, looking);
                let (face, facing) = if pipe == Axis::Y {
                    ("wall", super::hose::horizontal(context.yaw).opposite())
                } else if looking.axis() == pipe {
                    ("floor", looking)
                } else {
                    ("floor", pipe.dirs()[1])
                };
                self.native_state_with_property(state, "face", face)
                    .and_then(|s| self.native_state_with_property(s, "facing", facing.name()))
            }
            Some("create:hose_pulley") => {
                let facing = super::hose::horizontal(context.yaw).opposite();
                self.native_state_with_property(state, "facing", facing.name())
            }
            // ItemVaultBlock.getStateForPlacement: along a vault beside it, else the player's way.
            Some("create:item_vault") => {
                let beside = [Dir::North, Dir::South, Dir::West, Dir::East].into_iter().find_map(|d| {
                    match self.tachyon_part_beside(pos, d, None) {
                        Some(Part::Vault(a)) => Some(a),
                        _ => None,
                    }
                });
                let axis = beside.unwrap_or_else(|| super::hose::horizontal(context.yaw).axis());
                self.native_state_with_property(state, "axis", if axis == Axis::X { "x" } else { "z" })
                    .and_then(|s| self.native_state_with_property(s, "large", "false"))
            }
            _ => return,
        };
        if let Some(placed) = placed {
            *answer = Some(Some(placed));
        }
    }

    /// A pipe's sides and a tank's ends follow their neighbours (`updateShape`).
    pub(in crate::game) fn tachyon_fluid_update_shape(
        &self,
        pos: horizon_core::position::BlockPos,
        state: horizon_world::registry::BlockStateId,
        direction: horizon_core::geometry::Direction,
        neighbour: horizon_world::registry::BlockStateId,
        new: &mut Option<horizon_world::registry::BlockStateId>,
    ) {
        if new.is_some() {
            return;
        }
        let d = dir_of(direction);
        let p = [pos.x, pos.y, pos.z];
        let known = Some((d, neighbour));
        let next = match self.native_state_block(state) {
            Some("create:fluid_pipe") => self.tachyon_pipe_state(state, p, d, known),
            Some("create:fluid_tank" | "create:creative_fluid_tank") => self.tachyon_tank_state(state, p, known),
            _ => return,
        };
        if let Some(next) = next.filter(|n| *n != state) {
            *new = Some(next);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::kinetic::Dir;

    fn pipe(sides: &[Dir]) -> Part {
        Part::Pipe(sides.iter().fold(0, |m, d| m | bit(*d)))
    }

    #[test]
    fn a_pipe_alone_runs_straight_and_one_connection_opens_the_far_end() {
        assert_eq!(pipe_sides(0, 0, Dir::North), bit(Dir::North) | bit(Dir::South));
        assert_eq!(pipe_sides(bit(Dir::Up), 0, Dir::North), bit(Dir::Up) | bit(Dir::Down));
        let corner = bit(Dir::Up) | bit(Dir::East);
        assert_eq!(pipe_sides(corner, 0, Dir::North), corner);
        // Its neighbours gone, a straight pipe stays as it was.
        let along = bit(Dir::West) | bit(Dir::East);
        assert_eq!(pipe_sides(0, along, Dir::North), along);
        assert!(connects(Some(Part::Pump(Dir::East)), Dir::West));
        assert!(!connects(Some(Part::Pump(Dir::Up)), Dir::West));
        assert!(!connects(Some(Part::Depot), Dir::West));
    }

    #[test]
    fn a_pump_reaches_the_tank_down_its_pipes_and_an_open_end() {
        // Tank <- pipe <- pipe <- pump(east) -> pipe -> open end.
        let parts = HashMap::from([
            ([0, 0, 0], Part::Tank),
            ([1, 0, 0], pipe(&[Dir::West, Dir::East])),
            ([2, 0, 0], pipe(&[Dir::West, Dir::East])),
            ([3, 0, 0], Part::Pump(Dir::East)),
            ([4, 0, 0], pipe(&[Dir::West, Dir::East])),
        ]);
        assert_eq!(ends(&parts, [3, 0, 0], Dir::West), vec![End::Holder([0, 0, 0])]);
        assert_eq!(ends(&parts, [3, 0, 0], Dir::East), vec![End::Open([5, 0, 0])]);
        // A pipe turned away from the pump isn't reached.
        let parts = HashMap::from([([3, 0, 0], Part::Pump(Dir::East)), ([4, 0, 0], pipe(&[Dir::Up, Dir::Down]))]);
        assert!(ends(&parts, [3, 0, 0], Dir::East).is_empty());
    }

    #[test]
    fn a_long_line_stops_at_the_pumps_range() {
        let mut parts = HashMap::from([([0, 0, 0], Part::Pump(Dir::East))]);
        for x in 1..=20 {
            parts.insert([x, 0, 0], pipe(&[Dir::West, Dir::East]));
        }
        parts.insert([21, 0, 0], Part::Tank);
        assert!(ends(&parts, [0, 0, 0], Dir::East).is_empty());
        parts.insert([12, 0, 0], Part::Tank);
        assert_eq!(ends(&parts, [0, 0, 0], Dir::East), vec![End::Holder([12, 0, 0])]);
    }

    #[test]
    fn tanks_take_one_fluid_up_to_their_capacity_and_pumps_move_half_their_speed() {
        let mut t = Tank::default();
        assert_eq!(t.fill("minecraft:water", 9000, capacity(Part::Tank)), 8000);
        assert_eq!(t.fill("minecraft:lava", 1, 16000), 0);
        assert_eq!(t.drain(500), 500);
        assert_eq!(t.amount, 7500);
        assert_eq!(rate(256.0), 128);
        assert_eq!(rate(-32.0), 16);
        assert_eq!(rate(1.0), 1);
        assert_eq!(bucket_of("minecraft:lava"), Some("minecraft:lava_bucket"));
        assert_eq!(fluid_of_bucket("create:honey_bucket"), Some("create:honey"));
        assert_eq!(tagged("c:honey"), vec!["create:honey"]);
        assert_eq!(tagged("c:milk"), vec!["minecraft:milk"]);
    }
}
