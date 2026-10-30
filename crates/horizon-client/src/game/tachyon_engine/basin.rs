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

use super::super::Game;
use super::millstone::{Held, Output, add_all};
use horizon_client::items::ItemStack;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// How many stacks of items to mix a basin holds, and of what was mixed:
/// full, it takes no more, and mixes no more.
const STACKS: usize = 9;

/// What a basin holds: the items to mix, how far the mixing at hand is
/// (work: 1/16 for each RPM a tick), and what was mixed.
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
    #[serde(default = "time")]
    pub time: f32,
    pub outputs: Vec<Output>,
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
    /// The first of `recipes` it holds all the items for.
    pub fn ready<'a>(&self, recipes: &'a [Mixing]) -> Option<&'a Mixing> {
        recipes
            .iter()
            .find(|r| r.inputs.iter().all(|i| self.has(&i.item) >= i.count))
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
    /// all the items for, the outputs each by its chance (`roll` gives 0 to
    /// 1). What it mixes into (the first output), while it does.
    pub fn mix(
        &mut self,
        speed: f32,
        recipes: &[Mixing],
        roll: &mut impl FnMut() -> f32,
    ) -> Option<String> {
        if speed == 0.0 {
            return None;
        }
        let recipe = self.ready(recipes)?;
        let into = recipe.outputs.first()?.item.clone();
        if self.progress < recipe.time {
            self.progress = (self.progress + speed.abs() / 16.0).min(recipe.time);
            if self.progress < recipe.time {
                return Some(into);
            }
        }
        // Mixed: its outputs, if they fit (else it waits).
        let made: Vec<Held> = (recipe.outputs.iter())
            .filter(|o| o.chance >= 1.0 || roll() < o.chance)
            .map(|o| Held {
                item: o.item.clone(),
                count: o.count,
            })
            .collect();
        let mut outputs = self.outputs.clone();
        add_all(&mut outputs, &made);
        if outputs.len() > STACKS {
            return None;
        }
        self.outputs = outputs;
        self.progress = 0.0;
        for i in &recipe.inputs {
            let mut n = i.count;
            for s in self.inputs.iter_mut().filter(|s| s.item == i.item) {
                let k = n.min(s.count);
                s.count -= k;
                n -= k;
            }
        }
        self.inputs.retain(|s| s.count > 0);
        Some(into)
    }
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
            time,
            outputs: vec![Output {
                item: output.0.into(),
                count: output.1,
                chance: 1.0,
            }],
        }
    }

    #[test]
    fn a_basin_mixes_once_it_holds_all_a_recipe_asks_for() {
        let bronze = [recipe(&[("x:copper", 3), ("x:tin", 1)], ("x:bronze", 4), 100.0)];
        let mut b = Basin::default();
        assert_eq!(b.put("x:copper", 5), 5);
        // No tin yet: nothing to mix.
        assert_eq!(b.mix(32.0, &bronze, &mut || 0.0), None);
        b.put("x:tin", 1);
        // 2 work a tick at 32 RPM: 50 ticks.
        for _ in 0..49 {
            assert_eq!(b.mix(32.0, &bronze, &mut || 0.0).as_deref(), Some("x:bronze"));
            assert!(b.outputs.is_empty());
        }
        b.mix(32.0, &bronze, &mut || 0.0);
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
}
