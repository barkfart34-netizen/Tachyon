//! Kinetica's basin and mechanical mixer. A basin holds up to nine stacks
//! of what is put in it (a player's use, a hopper); a mixer turning over it
//! mixes them by its recipes (`kinetica/data/kinetica/tachyon/mixing.json`
//! in the built-in content, written by tachyon/art/kinetica.py) whenever
//! the basin holds all one asks for, faster the faster it turns. A hopper
//! under the basin takes out what was mixed; a player's use with an empty
//! hand takes it out (else the items to mix); breaking it drops all it
//! holds.
//!
//! The world's owner keeps what each basin holds with the millstones'
//! (`basins` in the dimension's `kinetica-*.json`).
//! Create's item-only basin recipes also test the blaze burner's heat
//! directly below them (`BasinBlockEntity`, `BasinRecipe.match`).

use super::super::Game;
use super::burner::Heat;
use super::millstone::{Held, Output, add_all, roll_outputs};
use super::press::{self, Beat};
use horizon_client::items::ItemStack;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// How many stacks of items to mix a basin holds, and of what was mixed:
/// full, it takes no more, and mixes no more.
const STACKS: usize = 9;

/// What a basin holds: the items to mix, how far the mixing at hand is
/// (ticks, [`mixing_ticks`]), and what was mixed.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Basin {
    #[serde(default)]
    pub inputs: Vec<Held>,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub outputs: Vec<Held>,
}

/// A mixing recipe: all its `inputs` mixed in `time` work into the outputs.
#[derive(Deserialize)]
pub(super) struct Mixing {
    pub inputs: Vec<Ingredient>,
    #[serde(default)]
    pub heat_requirement: HeatRequirement,
    #[serde(default = "time")]
    pub time: f32,
    pub outputs: Vec<Output>,
}
/// Create's `HeatCondition`: no heat, heated, or superheated. Tachyon asks
/// for kindled or better for heated; Create also accepts fading there.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum HeatRequirement {
    #[default]
    None,
    Heated,
    Superheated,
}

impl HeatRequirement {
    pub fn test(self, heat: Heat) -> bool {
        match self {
            HeatRequirement::None => true,
            HeatRequirement::Heated => matches!(heat, Heat::Kindled | Heat::Seething),
            HeatRequirement::Superheated => heat == Heat::Seething,
        }
    }
}
/// An item a mixing asks for, and how many.
#[derive(Deserialize)]
pub(super) struct Ingredient {
    pub item: String,
    #[serde(default = "one")]
    pub count: u32,
}
fn time() -> f32 {
    100.0
}
fn one() -> u32 {
    1
}

#[derive(Deserialize)]
struct Recipes {
    recipes: Vec<Mixing>,
}

/// The mixing recipes of the built-in content, read once.
pub(super) fn recipes() -> &'static [Mixing] {
    static RECIPES: OnceLock<Vec<Mixing>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        let Some(root) = horizon_client::brand::content() else {
            return Vec::new();
        };
        let path = root.join("kinetica/data/kinetica/tachyon/mixing.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Vec::new();
        };
        serde_json::from_str::<Recipes>(&text)
            .map(|r| r.recipes)
            .map_err(|e| log::warn!("Tachyon: {}: {e}", path.display()))
            .unwrap_or_default()
    })
}

impl Basin {
    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty() && self.outputs.is_empty()
    }
    /// How many of `item` it holds to mix.
    fn has(&self, item: &str) -> u32 {
        (self.inputs.iter())
            .filter(|h| h.item == item)
            .map(|h| h.count)
            .sum()
    }
    /// The first of `recipes` it holds all the items and has the heat for.
    pub fn ready<'a>(&self, recipes: &'a [Mixing], heat: Heat) -> Option<&'a Mixing> {
        recipes.iter().find(|r| {
            r.heat_requirement.test(heat)
                && r.inputs.iter().all(|i| self.has(&i.item) >= i.count)
        })
    }
    /// Puts in up to `count` of `item`: onto its stacks up to 64, the rest
    /// as new stacks while there is room. How many it took.
    pub fn put(&mut self, item: &str, count: u32) -> u32 {
        let mut left = count;
        for s in self.inputs.iter_mut().filter(|s| s.item == item) {
            let n = left.min(64u32.saturating_sub(s.count));
            s.count += n;
            left -= n;
        }
        while left > 0 && self.inputs.len() < STACKS {
            let n = left.min(64);
            self.inputs.push(Held {
                item: item.to_owned(),
                count: n,
            });
            left -= n;
        }
        count - left
    }
    /// What was mixed, or else the items to mix.
    pub fn take(&mut self) -> Vec<Held> {
        if self.outputs.is_empty() {
            self.progress = 0.0;
            return std::mem::take(&mut self.inputs);
        }
        std::mem::take(&mut self.outputs)
    }
    /// Everything it holds (dropped when it is broken).
    pub fn take_all(&mut self) -> Vec<Held> {
        let mut out = std::mem::take(&mut self.outputs);
        out.append(&mut self.inputs);
        self.progress = 0.0;
        out
    }
    /// One item of what was mixed: a hopper's take.
    pub fn take_one(&mut self) -> Option<Held> {
        let h = self.outputs.first_mut()?;
        h.count -= 1;
        let one = Held {
            item: h.item.clone(),
            count: 1,
        };
        if h.count == 0 {
            self.outputs.remove(0);
        }
        Some(one)
    }
    /// Puts back what a hopper took (`take_one`) and couldn't keep.
    pub fn give_back(&mut self, h: Held) {
        add_all(&mut self.outputs, &[h]);
    }
    /// A tick of mixing at `speed` (RPM) by the first of `recipes` it holds
    /// all the items and has the heat for, the outputs each by its chance
    /// (`roll` gives 0 to 1). What it mixes into (the first output), while
    /// it does.
    pub fn mix(
        &mut self,
        speed: f32,
        recipes: &[Mixing],
        heat: Heat,
        roll: &mut impl FnMut() -> f32,
    ) -> Option<String> {
        if speed == 0.0 {
            return None;
        }
        let recipe = self.ready(recipes, heat)?;
        let into = recipe.outputs.first()?.item.clone();
        let ticks = mixing_ticks(speed, recipe.time);
        if self.progress < ticks {
            self.progress = (self.progress + 1.0).min(ticks);
            if self.progress < ticks {
                return Some(into);
            }
        }
        // Mixed, if what it makes fits (else it waits).
        if !self.apply(recipe, roll) {
            return None;
        }
        self.progress = 0.0;
        Some(into)
    }

    /// A tick of a press at `speed` (RPM) over the basin compacting by the
    /// first of `recipes` it holds all the items and has the heat for (Create's
    /// `PressingBehaviour` in basin mode): its cycle runs, and its head
    /// coming down in the middle compacts them once, if what it makes fits.
    /// What it compacts into, while it does.
    pub fn compact(
        &mut self,
        speed: f32,
        recipes: &[Mixing],
        heat: Heat,
        roll: &mut impl FnMut() -> f32,
    ) -> Option<String> {
        // Nothing to compact: the next begins a cycle of its own.
        let Some(recipe) = self.ready(recipes, heat) else {
            self.progress = 0.0;
            return None;
        };
        if speed == 0.0 {
            return None;
        }
        let into = recipe.outputs.first()?.item.clone();
        let mut ticks = self.progress as i32;
        match press::beat(&mut ticks, speed) {
            Beat::Done => ticks = 0,
            Beat::Press => {
                self.apply(recipe, roll);
            }
            Beat::Running => {}
        }
        self.progress = ticks as f32;
        Some(into)
    }

    /// `recipe` made once of what the basin holds: its outputs, each by its
    /// chance, if they fit with what it made before. Whether it was.
    fn apply(&mut self, recipe: &Mixing, roll: &mut impl FnMut() -> f32) -> bool {
        let made = roll_outputs(&recipe.outputs, roll);
        let mut outputs = self.outputs.clone();
        add_all(&mut outputs, &made);
        if outputs.len() > STACKS {
            return false;
        }
        self.outputs = outputs;
        for i in &recipe.inputs {
            let mut n = i.count;
            for s in self.inputs.iter_mut().filter(|s| s.item == i.item) {
                let k = n.min(s.count);
                s.count -= k;
                n -= k;
            }
        }
        self.inputs.retain(|s| s.count > 0);
        true
    }
}

/// The ticks a mixer at `speed` (RPM) takes over a recipe of `time` work, as
/// Create's: 20 for its head to come down, then
/// `log2(512 / RPM) * ceil(15 * time / 100) + 1` (log2 rounded down), at
/// least 1.
pub(super) fn mixing_ticks(speed: f32, time: f32) -> f32 {
    let quotient = ((512.0 / speed.abs().max(1.0)) as u32).max(1);
    let log2 = quotient.ilog2();
    let per = (15.0 * time / 100.0).ceil() as u32;
    20.0 + (log2 * per + 1).max(1) as f32
}

impl Game<'_> {
    /// The player used a basin: the held stack goes in (as much as it has
    /// room for); with an empty hand, or what doesn't go in, what was mixed
    /// comes out, else the items to mix. Whether anything moved.
    pub(super) fn tachyon_use_basin(&mut self, basin: &mut Basin) -> bool {
        // A stack with components (a name, enchantments) stays in the hand:
        // a basin keeps items by name and count.
        let held = (self.held())
            .filter(|s| s.extra.is_none())
            .map(|s| (s.item.to_string(), s.count));
        if let Some((item, count)) = held {
            let took = basin.put(&item, count);
            if took > 0 {
                self.tachyon_spend_held(took);
                return true;
            }
        }
        let out = basin.take();
        for h in &out {
            self.give_or_drop(ItemStack::new(&h.item, h.count));
        }
        !out.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(inputs: &[(&str, u32)], output: (&str, u32), time: f32) -> Mixing {
        Mixing {
            inputs: (inputs.iter())
                .map(|(item, count)| Ingredient {
                    item: (*item).into(),
                    count: *count,
                })
                .collect(),
            heat_requirement: HeatRequirement::None,
            time,
            outputs: vec![Output {
                item: output.0.into(),
                count: output.1,
                chance: 1.0,
            }],
        }
    }

    #[test]
    fn a_press_over_a_basin_compacts_once_a_cycle() {
        let block = [recipe(&[("x:nugget", 9)], ("x:ingot", 1), 100.0)];
        let mut b = Basin::default();
        b.put("x:nugget", 18);
        // At 32 RPM the head comes down on the 31st tick of each 62.
        for _ in 0..30 {
            assert_eq!(
                b.compact(32.0, &block, Heat::None, &mut || 0.0),
                Some("x:ingot".into())
            );
        }
        assert!(b.outputs.is_empty());
        b.compact(32.0, &block, Heat::None, &mut || 0.0);
        assert_eq!(b.outputs[0].count, 1);
        for _ in 31..62 * 2 {
            b.compact(32.0, &block, Heat::None, &mut || 0.0);
        }
        assert_eq!(b.outputs[0].count, 2);
        assert!(b.inputs.is_empty());
        assert_eq!(b.compact(32.0, &block, Heat::None, &mut || 0.0), None);
    }

    #[test]
    fn a_basin_mixes_once_it_holds_all_a_recipe_asks_for() {
        let bronze = [recipe(&[("x:copper", 3), ("x:tin", 1)], ("x:bronze", 4), 100.0)];
        let mut b = Basin::default();
        assert_eq!(b.put("x:copper", 5), 5);
        // No tin yet: nothing to mix.
        assert_eq!(b.mix(32.0, &bronze, Heat::None, &mut || 0.0), None);
        b.put("x:tin", 1);
        // At 32 RPM: 20 ticks down, then log2(16) * 15 + 1.
        assert_eq!(mixing_ticks(32.0, 100.0), 81.0);
        assert_eq!(mixing_ticks(-256.0, 100.0), 36.0);
        assert_eq!(mixing_ticks(1.0, 100.0), 156.0);
        for _ in 0..80 {
            assert_eq!(
                b.mix(32.0, &bronze, Heat::None, &mut || 0.0).as_deref(),
                Some("x:bronze")
            );
            assert!(b.outputs.is_empty());
        }
        b.mix(32.0, &bronze, Heat::None, &mut || 0.0);
        assert_eq!(b.outputs, [Held { item: "x:bronze".into(), count: 4 }]);
        assert_eq!(b.inputs, [Held { item: "x:copper".into(), count: 2 }]);
        // What was mixed comes out first, then the rest.
        assert_eq!(b.take_one().unwrap().item, "x:bronze");
        assert_eq!(b.take()[0].count, 3);
        assert_eq!(b.take()[0].item, "x:copper");
        assert!(b.is_empty());
    }

    #[test]
    fn a_basin_holds_nine_stacks() {
        let mut b = Basin::default();
        assert_eq!(b.put("x:a", 64 * 8), 64 * 8);
        assert_eq!(b.put("x:b", 100), 64);
        assert_eq!(b.put("x:c", 1), 0);
        assert_eq!(b.inputs.len(), 9);
    }

    #[test]
    fn heat_requirements_test_each_burner_level() {
        for heat in [
            Heat::None,
            Heat::Smouldering,
            Heat::Fading,
            Heat::Kindled,
            Heat::Seething,
        ] {
            assert!(HeatRequirement::None.test(heat));
            assert_eq!(
                HeatRequirement::Heated.test(heat),
                matches!(heat, Heat::Kindled | Heat::Seething)
            );
            assert_eq!(HeatRequirement::Superheated.test(heat), heat == Heat::Seething);
        }
        let r: Mixing = serde_json::from_str(
            r#"{"inputs": [{"item": "x:copper"}], "outputs": [{"item": "x:alloy"}]}"#,
        )
        .unwrap();
        assert_eq!(r.heat_requirement, HeatRequirement::None);
    }

    #[test]
    fn a_heated_mixing_waits_for_kindled_heat_and_rechecks_before_finishing() {
        let mut brass = [recipe(&[("x:copper", 1), ("x:zinc", 1)], ("x:brass", 2), 100.0)];
        brass[0].heat_requirement = HeatRequirement::Heated;
        let mut b = Basin::default();
        b.put("x:copper", 1);
        b.put("x:zinc", 1);
        let inputs = b.inputs.clone();
        let mut no_roll = || -> f32 { panic!("a basin without heat must not roll outputs") };
        for heat in [Heat::None, Heat::Smouldering, Heat::Fading] {
            assert!(b.ready(&brass, heat).is_none());
            for _ in 0..81 {
                assert_eq!(b.mix(32.0, &brass, heat, &mut no_roll), None);
            }
            assert_eq!(b.progress, 0.0);
            assert_eq!(b.inputs, inputs);
            assert!(b.outputs.is_empty());
        }
        for _ in 0..80 {
            assert_eq!(
                b.mix(32.0, &brass, Heat::Kindled, &mut || 0.0).as_deref(),
                Some("x:brass")
            );
        }
        assert_eq!(b.mix(32.0, &brass, Heat::Fading, &mut no_roll), None);
        assert_eq!(b.inputs, inputs);
        assert!(b.outputs.is_empty());
        b.mix(32.0, &brass, Heat::Kindled, &mut || 0.0);
        assert!(b.inputs.is_empty());
        assert_eq!(b.outputs, [Held { item: "x:brass".into(), count: 2 }]);
    }

    #[test]
    fn superheated_compacting_waits_for_seething_heat() {
        let mut hot = [recipe(&[("x:ore", 1)], ("x:ingot", 1), 100.0)];
        hot[0].heat_requirement = HeatRequirement::Superheated;
        let mut b = Basin::default();
        b.put("x:ore", 1);
        for heat in [Heat::None, Heat::Smouldering, Heat::Fading, Heat::Kindled] {
            for _ in 0..62 {
                assert_eq!(b.compact(32.0, &hot, heat, &mut || 0.0), None);
            }
            assert_eq!(b.progress, 0.0);
            assert!(b.outputs.is_empty());
        }
        for _ in 0..30 {
            b.compact(32.0, &hot, Heat::Seething, &mut || 0.0);
        }
        assert!(b.outputs.is_empty());
        // Losing heat on the press tick consumes nothing and resets its cycle.
        assert_eq!(b.compact(32.0, &hot, Heat::Kindled, &mut || 0.0), None);
        assert_eq!(b.inputs[0].count, 1);
        assert_eq!(b.progress, 0.0);
        for _ in 0..31 {
            b.compact(32.0, &hot, Heat::Seething, &mut || 0.0);
        }
        assert!(b.inputs.is_empty());
        assert_eq!(b.outputs, [Held { item: "x:ingot".into(), count: 1 }]);
    }

    #[test]
    fn a_recipe_without_enough_heat_does_not_hide_the_next_one() {
        let mut first = recipe(&[("x:ore", 1)], ("x:hot", 1), 100.0);
        first.heat_requirement = HeatRequirement::Heated;
        let recipes = [first, recipe(&[("x:ore", 1)], ("x:cold", 1), 100.0)];
        let mut b = Basin::default();
        b.put("x:ore", 1);
        assert_eq!(b.ready(&recipes, Heat::None).unwrap().outputs[0].item, "x:cold");
        assert_eq!(b.ready(&recipes, Heat::Kindled).unwrap().outputs[0].item, "x:hot");
        assert_eq!(b.ready(&recipes, Heat::Seething).unwrap().outputs[0].item, "x:hot");
    }
}
