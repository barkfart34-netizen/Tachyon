//! Kinetica's millstone: items put in it are ground while it turns, faster
//! the faster it turns, into what its recipes give
//! (`kinetica/data/kinetica/tachyon/milling.json` in the built-in content,
//! written by tachyon/art/kinetica.py). A player puts the held stack in
//! with a use, and takes out what was ground with a use of anything that
//! doesn't grind; breaking it drops all it holds.
//!
//! The world's owner keeps what each millstone holds with the motors' speeds
//! (the dimension's `kinetica-*.json`), and tells its players which are at
//! work, on what (the goggles show it).

use super::super::Game;
use horizon_client::items::ItemStack;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

/// Work for one item when its recipe gives no time: 100, five seconds at
/// 16 RPM.
const TIME: f32 = 100.0;
/// The most of the input a millstone takes.
const INPUT_MAX: u32 = 64;
/// How many stacks of what was ground it keeps: full, it stops.
const OUTPUT_STACKS: usize = 9;

/// A stack a millstone holds.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Held {
    pub item: String,
    pub count: u32,
}

/// What a millstone holds: the items to grind, how far the one at hand is
/// (work: 1/16 for each RPM a tick), and what came out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Mill {
    #[serde(default)]
    pub input: Option<Held>,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub outputs: Vec<Held>,
}

/// A milling recipe: one `input` ground in `time` work into the outputs.
#[derive(Deserialize)]
pub(super) struct Milling {
    pub input: String,
    #[serde(default = "time")]
    pub time: f32,
    pub outputs: Vec<Output>,
}
/// An output and its chance (0 to 1).
#[derive(Deserialize)]
pub(super) struct Output {
    pub item: String,
    #[serde(default = "one")]
    pub count: u32,
    #[serde(default = "always")]
    pub chance: f32,
}
fn time() -> f32 {
    TIME
}
fn one() -> u32 {
    1
}
fn always() -> f32 {
    1.0
}

#[derive(Deserialize)]
struct Recipes {
    recipes: Vec<Milling>,
}

/// The milling recipes of the built-in content, read once.
pub(super) fn recipes() -> &'static [Milling] {
    static RECIPES: OnceLock<Vec<Milling>> = OnceLock::new();
    RECIPES.get_or_init(|| {
        let Some(root) = horizon_client::brand::content() else {
            return Vec::new();
        };
        let path = root.join("kinetica/data/kinetica/tachyon/milling.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            return Vec::new();
        };
        serde_json::from_str::<Recipes>(&text)
            .map(|r| r.recipes)
            .map_err(|e| log::warn!("Tachyon: {}: {e}", path.display()))
            .unwrap_or_default()
    })
}

/// The recipe that grinds `item`.
pub(super) fn recipe(item: &str) -> Option<&'static Milling> {
    recipes().iter().find(|r| r.input == item)
}

impl Mill {
    /// Whether it holds nothing.
    pub fn is_empty(&self) -> bool {
        self.input.is_none() && self.outputs.is_empty()
    }
    /// Everything it holds, as stacks (for a player, or dropped).
    pub fn take_all(&mut self) -> Vec<Held> {
        let mut out = std::mem::take(&mut self.outputs);
        out.extend(self.input.take());
        self.progress = 0.0;
        out
    }
    /// What was ground, or else the items to grind.
    pub fn take(&mut self) -> Vec<Held> {
        if self.outputs.is_empty() {
            self.progress = 0.0;
            return self.input.take().into_iter().collect();
        }
        std::mem::take(&mut self.outputs)
    }
    /// Puts in up to `count` of `item` (which `recipe` grinds): how many it
    /// took.
    pub fn put(&mut self, item: &str, count: u32) -> u32 {
        match &mut self.input {
            Some(h) if h.item == item => {
                let n = count.min(INPUT_MAX - h.count.min(INPUT_MAX));
                h.count += n;
                n
            }
            Some(_) => 0,
            None => {
                let n = count.min(INPUT_MAX);
                if n > 0 {
                    self.input = Some(Held {
                        item: item.to_owned(),
                        count: n,
                    });
                    self.progress = 0.0;
                }
                n
            }
        }
    }
    /// Whether `made` fits among the outputs.
    fn fits(&self, made: &[Held]) -> bool {
        let mut stacks = self.outputs.clone();
        add_all(&mut stacks, made);
        stacks.len() <= OUTPUT_STACKS
    }
    /// A tick of grinding at `speed` (RPM) with `recipe`, the outputs each
    /// by its chance (`roll` gives 0 to 1). Whether it ground.
    pub fn grind(
        &mut self,
        speed: f32,
        recipe: Option<&Milling>,
        roll: &mut impl FnMut() -> f32,
    ) -> bool {
        let (Some(input), Some(recipe)) = (self.input.as_ref(), recipe) else {
            return false;
        };
        if speed == 0.0 || input.item != recipe.input {
            return false;
        }
        if self.progress < recipe.time {
            self.progress = (self.progress + speed.abs() / 16.0).min(recipe.time);
            if self.progress < recipe.time {
                return true;
            }
        }
        // One ground: its outputs, if they fit (else it waits).
        let made: Vec<Held> = recipe
            .outputs
            .iter()
            .filter(|o| o.chance >= 1.0 || roll() < o.chance)
            .map(|o| Held {
                item: o.item.clone(),
                count: o.count,
            })
            .collect();
        if !self.fits(&made) {
            return false;
        }
        add_all(&mut self.outputs, &made);
        self.progress = 0.0;
        if let Some(input) = self.input.as_mut() {
            input.count -= 1;
            if input.count == 0 {
                self.input = None;
            }
        }
        true
    }
}

/// Adds `made` to `stacks`: onto stacks of the same item up to 64, the rest
/// as new stacks.
fn add_all(stacks: &mut Vec<Held>, made: &[Held]) {
    for m in made {
        let mut left = m.count;
        for s in stacks.iter_mut().filter(|s| s.item == m.item) {
            let n = left.min(64u32.saturating_sub(s.count));
            s.count += n;
            left -= n;
        }
        while left > 0 {
            let n = left.min(64);
            stacks.push(Held {
                item: m.item.clone(),
                count: n,
            });
            left -= n;
        }
    }
}

impl Game<'_> {
    /// The player used a millstone at `pos` (the world's owner): the held
    /// stack goes in when it grinds, else what it holds comes out. Whether
    /// anything moved.
    pub(super) fn tachyon_use_millstone(&mut self, mill: &mut Mill) -> bool {
        let held = self.held().map(|s| (s.item.to_string(), s.count));
        if let Some((item, count)) = held
            && recipe(&item).is_some()
        {
            let took = mill.put(&item, count);
            if took == 0 {
                return false;
            }
            if !self.creative {
                let cell = self.held_mut();
                if let Some(s) = cell.as_mut() {
                    s.count -= took;
                    if s.count == 0 {
                        *cell = None;
                    }
                }
            }
            return true;
        }
        let out = mill.take();
        for h in &out {
            self.give_or_drop(ItemStack::new(&h.item, h.count));
        }
        !out.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cobble() -> Milling {
        serde_json::from_str(
            r#"{"input": "minecraft:cobblestone", "time": 50,
                "outputs": [{"item": "minecraft:gravel"}, {"item": "minecraft:flint", "chance": 0.5}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn a_millstone_grinds_one_by_one_faster_when_faster() {
        let r = cobble();
        let mut m = Mill::default();
        assert_eq!(m.put("minecraft:cobblestone", 70), 64);
        assert_eq!(m.put("minecraft:stone", 1), 0);
        let mut low = || 0.0f32;
        // At 16 RPM, 1 work a tick: the first is ground on the 50th.
        for _ in 0..49 {
            assert!(m.grind(16.0, Some(&r), &mut low));
            assert!(m.outputs.is_empty());
        }
        assert!(m.grind(16.0, Some(&r), &mut low));
        assert_eq!(m.input.as_ref().unwrap().count, 63);
        let gravel = Held {
            item: "minecraft:gravel".into(),
            count: 1,
        };
        let flint = Held {
            item: "minecraft:flint".into(),
            count: 1,
        };
        assert_eq!(m.outputs, [gravel.clone(), flint]);
        // At 256 RPM, 16 a tick: four ticks an item; the flint missed.
        let mut high = || 0.9f32;
        for _ in 0..4 {
            m.grind(-256.0, Some(&r), &mut high);
        }
        assert_eq!(m.outputs[0].count, 2);
        assert_eq!(m.outputs[1].count, 1);
        // Still, it doesn't.
        assert!(!m.grind(0.0, Some(&r), &mut high));
        assert_eq!(m.take().len(), 2);
        assert_eq!(m.take()[0].count, 62);
        assert!(m.is_empty());
    }

    #[test]
    fn a_full_millstone_waits() {
        let r = cobble();
        let mut m = Mill::default();
        m.put("minecraft:cobblestone", 2);
        m.outputs = (0..OUTPUT_STACKS)
            .map(|i| Held {
                item: format!("x:{i}"),
                count: 64,
            })
            .collect();
        let mut roll = || 0.9f32;
        for _ in 0..100 {
            m.grind(16.0, Some(&r), &mut roll);
        }
        assert_eq!(m.input.as_ref().unwrap().count, 2);
        assert_eq!(m.take_all().len(), OUTPUT_STACKS + 1);
    }
}
