//! Create's heat under a basin, following `BlazeBurnerBlock`,
//! `BlazeBurnerBlockEntity` and `BlazeBurnerBlockItem` (MIT). A filled cage
//! starts smouldering; furnace fuel kindles it, and a blaze cake makes it
//! seething. Fuel burns only while its chunk is loaded, and is kept with
//! the other machines in the dimension's `kinetica-*.json`.
//!
//! The respawn anchor template's `charges` stand for the five heat levels.
//! Block updates carry the heat to clients; the chunk draws Create's cage
//! model, without `BlazeBurnerRenderer`'s head and rods. An empty cage lit
//! with flint and steel follows `LitBlazeBurnerBlock`: passive heat only,
//! never a fueled blaze. Create exposes no item handler for either cage.
use super::super::Game;
use super::kinetic::Part;
use super::{CLIENT, OWNERS};
use horizon_client::items::ItemStack;
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_core::random::RandomSource;
use serde::{Deserialize, Serialize};

const MAX_HEAT_CAPACITY: u32 = 10000;
const INSERTION_THRESHOLD: u32 = 500;
pub(super) const EMPTY_BURNER: &str = "create:empty_blaze_burner";

/// Whether `entity_kind` can be captured into `EMPTY_BURNER` to fill it.
pub(super) fn can_capture_blaze(entity_kind: &str) -> bool {
    matches!(entity_kind, "minecraft:blaze" | "blaze")
}

/// Whether a spawner spawning `entity_kind` fills `EMPTY_BURNER` on right-click.
pub(super) fn can_capture_spawner(entity_kind: Option<&str>) -> bool {
    entity_kind.is_some_and(can_capture_blaze)
}

/// `BlazeBurnerBlock.HeatLevel`, in its order, represented by `charges`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Heat {
    #[default]
    None,
    Smouldering,
    Fading,
    Kindled,
    Seething,
}

impl Heat {
    pub fn name(self) -> &'static str {
        match self {
            Heat::None => "none",
            Heat::Smouldering => "smouldering",
            Heat::Fading => "fading",
            Heat::Kindled => "kindled",
            Heat::Seething => "seething",
        }
    }
    fn charge(self) -> &'static str {
        match self {
            Heat::None => "0",
            Heat::Smouldering => "1",
            Heat::Fading => "2",
            Heat::Kindled => "3",
            Heat::Seething => "4",
        }
    }
}

/// `BasinBlockEntity.getHeatLevelOf`: a burner's state, or passive heat
/// from a lit empty cage, not kindled or seething heat.
fn heat(name: &str, charges: Option<&str>) -> Heat {
    match name {
        "create:blaze_burner" => match charges {
            Some("1") => Heat::Smouldering,
            Some("2") => Heat::Fading,
            Some("3") => Heat::Kindled,
            Some("4") => Heat::Seething,
            _ => Heat::None,
        },
        "create:lit_blaze_burner" => Heat::Smouldering,
        _ => Heat::None,
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Fuel {
    #[default]
    None,
    Normal,
    Special,
}

/// A filled burner's `activeFuel` and `remainingBurnTime`, plus the heat
/// held forever by `applyCreativeFuel`. Empty cages have no such state.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) struct Burner {
    #[serde(default)]
    fuel: Fuel,
    #[serde(default)]
    remaining: u32,
    #[serde(default)]
    creative: Option<Heat>,
}

impl Burner {
    fn heat(&self) -> Heat {
        if let Some(heat) = self.creative {
            return heat;
        }
        match self.fuel {
            Fuel::None => Heat::Smouldering,
            Fuel::Special => Heat::Seething,
            Fuel::Normal if self.remaining < MAX_HEAT_CAPACITY / 80 => Heat::Fading,
            Fuel::Normal => Heat::Kindled,
        }
    }
    /// Whether normal and special fuels would both be rejected by `feed`:
    /// creative-cake burners reject all fuels, and special fuel above
    /// `INSERTION_THRESHOLD` rejects both downgrades and re-insertion.
    pub(super) fn is_feed_locked(&self) -> bool {
        self.creative.is_some()
            || (self.fuel == Fuel::Special && self.remaining > INSERTION_THRESHOLD)
    }
    /// `BlazeBurnerBlockEntity.tick`: special fuel leaves half the normal
    /// capacity behind when it runs out. Whether the saved state changed.
    fn tick(&mut self) -> bool {
        if self.creative.is_some() || (self.remaining == 0 && self.fuel == Fuel::None) {
            return false;
        }
        self.remaining = self.remaining.saturating_sub(1);
        if self.remaining == 0 {
            if self.fuel == Fuel::Special {
                self.fuel = Fuel::Normal;
                self.remaining = MAX_HEAT_CAPACITY / 2;
            } else {
                self.fuel = Fuel::None;
            }
        }
        true
    }
    /// `tryUpdateFuel`: same fuel adds time at or below 500. A real player
    /// may overflow normal fuel up to 10000; neither special fuel nor a
    /// downgrade accepts that overflow. A fresh fuel's own time is not capped.
    fn feed(&mut self, fuel: Fuel, mut time: u32, overflow: bool) -> bool {
        if self.creative.is_some() || fuel == Fuel::None || time == 0 || fuel < self.fuel {
            return false;
        }
        if fuel == self.fuel {
            if self.remaining <= INSERTION_THRESHOLD {
                time = time.saturating_add(self.remaining);
            } else if overflow && fuel == Fuel::Normal {
                time = if self.remaining < MAX_HEAT_CAPACITY {
                    self.remaining.saturating_add(time).min(MAX_HEAT_CAPACITY)
                } else {
                    self.remaining
                };
            } else {
                return false;
            }
        }
        self.fuel = fuel;
        self.remaining = time;
        true
    }
    /// `applyCreativeFuel` cycles from the block's heat, skipping fading.
    fn creative(&mut self, heat: Heat) {
        self.creative = Some(match heat {
            Heat::None | Heat::Seething => Heat::Smouldering,
            Heat::Smouldering | Heat::Fading => Heat::Kindled,
            Heat::Kindled => Heat::Seething,
        });
        self.fuel = Fuel::None;
        self.remaining = 0;
    }

    /// Mechanical arm fuel insertion (`BlazeBurnerBlockEntity.tryInsert` with `forceOverflow=false`):
    /// feeds furnace fuel or Blaze Cakes without overflowing past the insertion threshold.
    pub(super) fn arm_feed(&mut self, item: &str, furnace: i32, special: bool, regular: bool) -> bool {
        let special = special || item == "create:blaze_cake";
        let (f, time) = fuel(furnace, special, regular);
        self.feed(f, time, false)
    }
}

/// `tryUpdateFuel`'s fallback order. AllItems gives blaze cake 3200 special
/// ticks; special tags use that too, and regular tags only replace a
/// missing furnace value (1600, coal's time).
fn fuel(furnace: i32, special: bool, regular: bool) -> (Fuel, u32) {
    if special {
        (Fuel::Special, 3200)
    } else if furnace > 0 {
        (Fuel::Normal, furnace as u32)
    } else if regular {
        (Fuel::Normal, 1600)
    } else {
        (Fuel::None, 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BurnerAction {
    Extinguish,
    SoulFire,
    Ignite,
    CreativeCake,
    Feed(Fuel, u32),
}

/// Non-fuel click plan for `create:lit_blaze_burner`: a shovel extinguishes it and
/// a `minecraft:soul_fire_base_blocks` item converts it only when `charges != Some("1")`.
fn lit_burner_action(
    item: &str,
    soul_fire_base: bool,
    charges: Option<&str>,
) -> Option<BurnerAction> {
    if item.ends_with("_shovel") {
        Some(BurnerAction::Extinguish)
    } else if soul_fire_base && charges != Some("1") {
        Some(BurnerAction::SoulFire)
    } else {
        None
    }
}

/// Non-fuel click plan for `create:blaze_burner`: flint & steel on an empty (`Heat::None`)
/// cage, or a creative blaze cake on a fueled cage.
fn burner_special_action(was: Heat, item: &str) -> Option<BurnerAction> {
    if was == Heat::None {
        (item == "minecraft:flint_and_steel").then_some(BurnerAction::Ignite)
    } else if item == "create:creative_blaze_cake" {
        Some(BurnerAction::CreativeCake)
    } else {
        None
    }
}

fn remote_burner_takes_feed(heat: Heat, locked: bool, next_fuel: Fuel) -> bool {
    !locked && !(heat == Heat::Seething && next_fuel < Fuel::Special)
}

impl Game<'_> {
    /// Heat read from a block state, shared by basins and the goggles.
    pub(super) fn tachyon_heat(&self, pos: [i32; 3]) -> Heat {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.pack.assets.registry.state(state).map_or(Heat::None, |s| {
            heat(&s.name, s.properties.get("charges").map(String::as_str))
        })
    }
    fn tachyon_burner_state(&mut self, pos: [i32; 3], name: &str, charges: &str) {
        let registry = self.pack.assets.registry;
        let Some(mut state) = registry
            .default_state(name)
            .and_then(|s| registry.state(s))
            .cloned()
        else {
            return;
        };
        state.properties.insert("charges".into(), charges.into());
        if let Ok(next) = registry.resolve(&state)
            && next != self.world.block(pos[0], pos[1], pos[2])
            && let Err(e) = self.level_set(pos, next)
        {
            log::warn!("Tachyon: blaze burner at {pos:?}: {e}");
        }
    }
    /// Loaded burners burn once in the owner's block entity phase, before
    /// basins test their recipes. Unloaded burners keep their remaining time.
    pub(super) fn tachyon_burners(&mut self) {
        let changes = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(self.dim.id()).or_default();
            let before = owner.burners.len();
            owner.burners.retain(|p, _| {
                !owner.index.known.contains(&(p[0] >> 4, p[2] >> 4))
                    || (owner.index.parts.get(p) == Some(&Part::Burner)
                        && self.tachyon_heat(*p) != Heat::None)
            });
            owner.mills_changed |= before != owner.burners.len();
            let mut changes = Vec::new();
            for (p, part) in &owner.index.parts {
                if *part != Part::Burner {
                    continue;
                }
                let was = self.tachyon_heat(*p);
                if was == Heat::None {
                    continue;
                }
                let burner = owner.burners.entry(*p).or_default();
                owner.mills_changed |= burner.tick();
                let heat = burner.heat();
                if was != heat {
                    changes.push((*p, heat));
                }
            }
            changes
        });
        for (p, heat) in changes {
            self.tachyon_burner_state(p, "create:blaze_burner", heat.charge());
        }
    }
    fn tachyon_burner_charges(&self, pos: [i32; 3]) -> Option<&str> {
        let state = self.world.block(pos[0], pos[1], pos[2]);
        self.pack
            .assets
            .registry
            .state(state)
            .and_then(|s| s.properties.get("charges").map(String::as_str))
    }
    fn tachyon_burner_action(
        &self,
        pos: [i32; 3],
        lit: bool,
        stack: &ItemStack,
    ) -> Option<BurnerAction> {
        let items = self.res.items();
        let tagged = |tag: &str| {
            items.item_tags.get(tag).is_some_and(|t| t.contains(&*stack.item))
        };
        if lit {
            return lit_burner_action(
                &stack.item,
                tagged("minecraft:soul_fire_base_blocks"),
                self.tachyon_burner_charges(pos),
            );
        }
        let was = self.tachyon_heat(pos);
        if let Some(special) = burner_special_action(was, &stack.item) {
            return Some(special);
        }
        if was == Heat::None {
            return None;
        }
        let special =
            &*stack.item == "create:blaze_cake" || tagged("create:blaze_burner_fuel/special");
        let (f, time) = fuel(
            self.res.providers().burn_time(items, stack, false),
            special,
            tagged("create:blaze_burner_fuel/regular"),
        );
        (f != Fuel::None && time > 0).then_some(BurnerAction::Feed(f, time))
    }
    /// Whether `tachyon_use_burner` would act for `held` at `pos`.
    pub(super) fn tachyon_burner_takes_use(
        &self,
        pos: [i32; 3],
        lit: bool,
        held: Option<&ItemStack>,
    ) -> bool {
        let Some(stack) = held else {
            return false;
        };
        let Some(action) = self.tachyon_burner_action(pos, lit, stack) else {
            return false;
        };
        match action {
            BurnerAction::Extinguish
            | BurnerAction::SoulFire
            | BurnerAction::Ignite
            | BurnerAction::CreativeCake => true,
            BurnerAction::Feed(f, time) => {
                if self.remote.is_some() {
                    let locked = CLIENT.with_borrow(|c| c.shown.held.contains_key(&pos));
                    remote_burner_takes_feed(self.tachyon_heat(pos), locked, f)
                } else {
                    let dimension = self.dim.id();
                    OWNERS.with_borrow(|owners| {
                        let mut burner = owners
                            .get(dimension)
                            .and_then(|o| o.burners.get(&pos))
                            .cloned()
                            .unwrap_or_default();
                        burner.feed(f, time, true)
                    })
                }
            }
        }
    }
    /// `BlazeBurnerBlock.useItemOn` / `tryInsert`: one fuel item, its
    /// crafting remainder returned to the hand or inventory, none spent in
    /// creative mode. `LitBlazeBurnerBlock` also accepts a shovel or soul sand.
    pub(super) fn tachyon_use_burner(&mut self, pos: [i32; 3], lit: bool) -> bool {
        let Some(stack) = self.held().cloned() else {
            return false;
        };
        let Some(action) = self.tachyon_burner_action(pos, lit, &stack) else {
            return false;
        };
        let creative = match action {
            BurnerAction::Extinguish => {
                self.tachyon_burner_state(pos, "create:blaze_burner", "0");
                self.damage_held(1);
                return true;
            }
            BurnerAction::SoulFire => {
                self.tachyon_burner_state(pos, "create:lit_blaze_burner", "1");
                return true;
            }
            BurnerAction::Ignite => {
                self.tachyon_burner_state(pos, "create:lit_blaze_burner", "0");
                self.damage_held(1);
                return true;
            }
            BurnerAction::CreativeCake => true,
            BurnerAction::Feed(_, _) => false,
        };
        let was = self.tachyon_heat(pos);
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let heat = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            let burner = owner.burners.entry(pos).or_default();
            match action {
                BurnerAction::CreativeCake => burner.creative(was),
                BurnerAction::Feed(f, time) if burner.feed(f, time, true) => {}
                _ => return None,
            }
            let heat = burner.heat();
            owner.save(&root, dimension);
            Some(heat)
        });
        let Some(heat) = heat else {
            return false;
        };
        self.tachyon_burner_state(pos, "create:blaze_burner", heat.charge());
        if !self.creative && !creative {
            let remainder = self
                .res
                .items()
                .get(&stack.item)
                .and_then(|i| i.remainder.clone());
            self.tachyon_spend_held(1);
            if let Some(item) = remainder {
                let rest = ItemStack::new(&item, 1);
                if self.held().is_none() {
                    *self.held_mut() = Some(rest);
                } else {
                    self.give_or_drop(rest);
                }
            }
        }
        true
    }

    /// Evaluates or performs automated Mechanical Arm fuel insertion into a Blaze Burner at `pos`.
    pub(super) fn tachyon_burner_arm_feed(
        &mut self,
        pos: [i32; 3],
        stack: &ItemStack,
        simulate: bool,
    ) -> bool {
        if self.tachyon_heat(pos) == Heat::None {
            return false;
        }
        let items = self.res.items();
        let tagged = |tag: &str| {
            items.item_tags.get(tag).is_some_and(|t| t.contains(&*stack.item))
        };
        let special =
            &*stack.item == "create:blaze_cake" || tagged("create:blaze_burner_fuel/special");
        let furnace = self.res.providers().burn_time(items, stack, false);
        let regular = tagged("create:blaze_burner_fuel/regular");
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let next_heat = OWNERS.with_borrow_mut(|owners| {
            if simulate {
                let mut burner = owners
                    .get(dimension)
                    .and_then(|o| o.burners.get(&pos))
                    .cloned()
                    .unwrap_or_default();
                return burner
                    .arm_feed(&stack.item, furnace, special, regular)
                    .then(|| burner.heat());
            }
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            let burner = owner.burners.entry(pos).or_default();
            if !burner.arm_feed(&stack.item, furnace, special, regular) {
                return None;
            }
            let heat = burner.heat();
            owner.mills_changed = true;
            owner.save(&root, dimension);
            Some(heat)
        });
        let Some(heat) = next_heat else {
            return false;
        };
        if !simulate {
            self.tachyon_burner_state(pos, "create:blaze_burner", heat.charge());
        }
        true
    }

    /// Captures a right-clicked Blaze entity into a held `create:empty_blaze_burner`.
    pub(in crate::game) fn tachyon_capture_blaze(&mut self, id: u32, kind: &str) -> bool {
        if crate::tachyon_native_mods::create_active()
            || self.remote.is_some()
            || !can_capture_blaze(kind)
            || !self
                .held()
                .is_some_and(|h| &*h.item == EMPTY_BURNER && h.count > 0)
            || self.res.items().get("create:blaze_burner").is_none()
        {
            return false;
        }
        let Some(index) = self.mobs.iter().position(|m| m.id == id && !m.removed) else {
            return false;
        };
        self.break_leash(id, true, false);
        self.mobs[index].removed = true;
        self.native_entity_left(id);
        self.filled_result(ItemStack::new("create:blaze_burner", 1));
        true
    }

    /// Uses `create:empty_blaze_burner` on a Blaze Spawner (to fill it) or places an empty cage.
    pub(in crate::game) fn tachyon_empty_burner_use(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
        claimed: &mut bool,
    ) {
        if *claimed
            || self.remote.is_some()
            || &*stack.item != EMPTY_BURNER
            || stack.count == 0
        {
            return;
        }
        if let Some(horizon_client::chunk::block_entity::BlockEntity::Spawner(s)) =
            self.world.block_entities.get(&(pos[0], pos[1], pos[2]))
        {
            use horizon_client::chunk::block_entity::Next;
            let data = match &s.next {
                Some(Next::Own(d)) => Some(d),
                Some(Next::Potential(i)) => s.potentials.get(*i).map(|(d, _)| d),
                None => s.potentials.first().map(|(d, _)| d),
            };
            let entity_id = data.and_then(|d| {
                match d.entity.get(&horizon_core::nbt::JavaString::from("id")) {
                    Some(horizon_core::nbt::Tag::String(s)) => Some(s.to_string_lossy()),
                    _ => None,
                }
            });
            if can_capture_spawner(entity_id.as_deref())
                && self.res.items().get("create:blaze_burner").is_some()
            {
                *claimed = true;
                self.filled_result(ItemStack::new("create:blaze_burner", 1));
            }
            return;
        }
        let replaceable = |p: [i32; 3]| {
            self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4))
                && self
                    .res
                    .facts()
                    .get(self.world.block(p[0], p[1], p[2]))
                    .replaceable
        };
        let target = if replaceable(pos) {
            pos
        } else {
            let next = [pos[0] + face[0], pos[1] + face[1], pos[2] + face[2]];
            if !replaceable(next) {
                return;
            }
            next
        };
        if !self.may_build() || self.block_action_restricted(target) {
            return;
        }
        let registry = self.pack.assets.registry;
        let Some(mut state) = registry
            .default_state("create:blaze_burner")
            .and_then(|s| registry.state(s))
            .cloned()
        else {
            return;
        };
        state.properties.insert("charges".into(), "0".into());
        let Ok(next) = registry.resolve(&state) else {
            return;
        };
        let dimension = self.native_dimension();
        if !self.native_set_block(dimension, target, next, 3) {
            return;
        }
        *claimed = true;
        if !self.creative {
            self.tachyon_spend_held(1);
        }
    }
    /// A window's smoke and flames, by
    /// `BlazeBurnerBlockEntity.spawnParticles`'s heat levels: no flame when
    /// smouldering, soul flame when seething.
    pub(super) fn tachyon_burner_particles(&mut self) {
        if self.window.is_none() {
            return;
        }
        let burners: Vec<([i32; 3], Heat)> = self.tachyon_view(|index, _| {
            (index.parts.iter())
                .filter(|(_, part)| matches!(part, Part::Burner | Part::LitBurner))
                .map(|(p, _)| (*p, self.tachyon_heat(*p)))
                .collect()
        });
        for (p, heat) in burners {
            if heat == Heat::None {
                continue;
            }
            let random = self.particles.random();
            if random.next_int_bound(4).unwrap_or(1) != 0 {
                continue;
            }
            let at = [
                f64::from(p[0]) + 0.25 + random.next_double() * 0.5,
                f64::from(p[1]) + 0.9,
                f64::from(p[2]) + 0.25 + random.next_double() * 0.5,
            ];
            let kind = match heat {
                Heat::Seething => ParticleType::SoulFireFlame,
                Heat::Fading | Heat::Kindled => ParticleType::Flame,
                _ => ParticleType::LargeSmoke,
            };
            self.particles
                .add(&ParticleOptions::simple(kind), at, [0.0, 0.0625, 0.0]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normal_fuel_kindles_then_fades_below_125_ticks() {
        let mut b = Burner::default();
        assert_eq!(b.heat(), Heat::Smouldering);
        assert!(b.feed(Fuel::Normal, 1600, true));
        for _ in 0..1600 - 125 {
            assert_eq!(b.heat(), Heat::Kindled);
            assert!(b.tick());
        }
        assert_eq!((b.remaining, b.heat()), (125, Heat::Kindled));
        b.tick();
        assert_eq!((b.remaining, b.heat()), (124, Heat::Fading));
        for _ in 0..123 {
            b.tick();
            assert_eq!(b.heat(), Heat::Fading);
        }
        b.tick();
        assert_eq!((b.remaining, b.heat()), (0, Heat::Smouldering));
        assert!(!b.tick());
    }

    #[test]
    fn blaze_cake_seethes_for_3200_ticks_then_leaves_5000_normal_ticks() {
        let mut b = Burner::default();
        let (kind, time) = fuel(0, true, false);
        assert!(b.feed(kind, time, true));
        for _ in 0..3199 {
            b.tick();
            assert_eq!(b.heat(), Heat::Seething);
        }
        b.tick();
        assert_eq!((b.remaining, b.heat()), (5000, Heat::Kindled));
        for _ in 0..5000 {
            b.tick();
        }
        assert_eq!(b.heat(), Heat::Smouldering);
    }

    #[test]
    fn refueling_follows_the_threshold_overflow_and_fuel_order() {
        let mut b = Burner::default();
        assert!(!b.feed(Fuel::None, 0, true));
        assert!(b.feed(Fuel::Normal, 500, false));
        assert!(b.feed(Fuel::Normal, 1600, false));
        assert_eq!(b.remaining, 2100);
        assert!(!b.feed(Fuel::Normal, 1600, false));
        assert!(b.feed(Fuel::Normal, 16000, true));
        assert_eq!(b.remaining, 10000);
        // A real player can waste fuel even when already at capacity.
        assert!(b.feed(Fuel::Normal, 1600, true));
        assert_eq!(b.remaining, 10000);
        assert!(b.feed(Fuel::Special, 3200, true));
        assert!(!b.feed(Fuel::Normal, 20000, true));
        assert!(!b.feed(Fuel::Special, 3200, true));
        for _ in 0..2700 {
            b.tick();
        }
        assert!(b.feed(Fuel::Special, 3200, false));
        assert_eq!(b.remaining, 3700);
        let mut fresh = Burner::default();
        assert!(fresh.feed(Fuel::Normal, 20000, true));
        assert_eq!(fresh.remaining, 20000);
        assert!(fresh.feed(Fuel::Normal, 1600, true));
        assert_eq!(fresh.remaining, 20000);
        let mut low = Burner::default();
        assert!(low.feed(Fuel::Normal, 500, false));
        assert!(low.feed(Fuel::Normal, 20000, false));
        assert_eq!(low.remaining, 20500);
        assert_eq!(fuel(300, false, true), (Fuel::Normal, 300));
        assert_eq!(fuel(0, false, true), (Fuel::Normal, 1600));
        assert_eq!(fuel(20000, true, true), (Fuel::Special, 3200));
    }

    #[test]
    fn empty_and_lit_cages_do_not_give_recipe_heat() {
        for h in [
            Heat::None,
            Heat::Smouldering,
            Heat::Fading,
            Heat::Kindled,
            Heat::Seething,
        ] {
            assert_eq!(heat("create:blaze_burner", Some(h.charge())), h);
        }
        assert_eq!(heat("minecraft:respawn_anchor", Some("4")), Heat::None);
        assert_eq!(heat("create:lit_blaze_burner", Some("1")), Heat::Smouldering);
        assert_eq!(heat("create:blaze_burner", None), Heat::None);
    }

    #[test]
    fn creative_cake_cycles_active_heat_without_counting_down() {
        let mut b = Burner::default();
        for h in [Heat::Kindled, Heat::Seething, Heat::Smouldering] {
            let was = b.heat();
            b.creative(was);
            assert_eq!(b.heat(), h);
            assert!(!b.tick());
            assert!(!b.feed(Fuel::Normal, 1600, true));
        }
        // A command's block heat can differ from the saved fuel's heat.
        let mut b = Burner::default();
        b.creative(Heat::Kindled);
        assert_eq!(b.heat(), Heat::Seething);
        b.creative(Heat::Fading);
        assert_eq!(b.heat(), Heat::Kindled);
    }

    #[test]
    fn a_tachyon_burners_fuel_is_saved_by_dimension() {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("tachyon-burner-{}-{unique}", std::process::id()));
        let mut owner = super::super::Owner::default();
        let mut b = Burner::default();
        b.feed(Fuel::Special, 3200, true);
        b.tick();
        owner.burners.insert([1, 64, -3], b.clone());
        owner.save(&root, "minecraft:overworld");
        let mut loaded = super::super::Owner::default();
        loaded.load(&root, "minecraft:overworld");
        assert_eq!(loaded.burners.get(&[1, 64, -3]), Some(&b));
        let mut nether = super::super::Owner::default();
        nether.load(&root, "minecraft:the_nether");
        assert!(nether.burners.is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn burner_use_does_not_swallow_blocks_or_redundant_clicks() {
        assert_eq!(lit_burner_action("minecraft:iron_shovel", false, Some("0")), Some(BurnerAction::Extinguish));
        assert_eq!(lit_burner_action("minecraft:soul_sand", true, Some("0")), Some(BurnerAction::SoulFire));
        assert_eq!(lit_burner_action("minecraft:soul_sand", true, Some("1")), None);
        assert_eq!(lit_burner_action("create:basin", false, Some("0")), None);

        assert_eq!(burner_special_action(Heat::None, "minecraft:flint_and_steel"), Some(BurnerAction::Ignite));
        assert_eq!(burner_special_action(Heat::None, "create:basin"), None);
        assert_eq!(burner_special_action(Heat::None, "minecraft:coal"), None);
        assert_eq!(burner_special_action(Heat::Smouldering, "create:creative_blaze_cake"), Some(BurnerAction::CreativeCake));

        let mut b = Burner::default();
        b.feed(Fuel::Special, 3200, true);
        assert_eq!(b.heat(), Heat::Seething);
        assert!(!b.feed(Fuel::Normal, 1600, true));
        assert!(b.is_feed_locked());
        assert!(!remote_burner_takes_feed(b.heat(), b.is_feed_locked(), Fuel::Special));
        assert!(!remote_burner_takes_feed(b.heat(), b.is_feed_locked(), Fuel::Normal));

        b.remaining = INSERTION_THRESHOLD;
        assert!(!b.is_feed_locked());
        assert!(remote_burner_takes_feed(b.heat(), b.is_feed_locked(), Fuel::Special));
        assert!(!remote_burner_takes_feed(b.heat(), b.is_feed_locked(), Fuel::Normal));

        let mut creative = Burner::default();
        creative.creative(Heat::None);
        assert!(creative.is_feed_locked());
        assert!(!creative.feed(Fuel::Normal, 1600, true));
        assert!(!creative.feed(Fuel::Special, 3200, true));
        assert!(!remote_burner_takes_feed(creative.heat(), creative.is_feed_locked(), Fuel::Normal));
        assert!(!remote_burner_takes_feed(creative.heat(), creative.is_feed_locked(), Fuel::Special));
    }

    #[test]
    fn empty_burner_capture_and_arm_feed_obey_create_rules() {
        assert_eq!(EMPTY_BURNER, "create:empty_blaze_burner");
        assert!(can_capture_blaze("minecraft:blaze"));
        assert!(!can_capture_blaze("minecraft:zombie"));
        assert!(can_capture_spawner(Some("minecraft:blaze")));
        assert!(!can_capture_spawner(Some("minecraft:skeleton")));
        assert!(!can_capture_spawner(None));

        let mut b = Burner::default();
        assert!(b.arm_feed("minecraft:coal", 1600, false, false));
        assert_eq!((b.heat(), b.remaining), (Heat::Kindled, 1600));
        // Mechanical arm respects INSERTION_THRESHOLD and will not overflow past 500.
        assert!(!b.arm_feed("minecraft:coal", 1600, false, false));
        // Upgrading normal -> special via Blaze Cake works anytime.
        assert!(b.arm_feed("create:blaze_cake", 0, false, false));
        assert_eq!((b.heat(), b.remaining), (Heat::Seething, 3200));
    }
}
