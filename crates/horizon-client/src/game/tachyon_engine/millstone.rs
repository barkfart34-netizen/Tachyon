//! Kinetica's millstone and depot. Items put in a millstone are ground
//! while it turns, faster the faster it turns, into what its recipes give
//! (`kinetica/data/kinetica/tachyon/milling.json` in the built-in content,
//! written by tachyon/art/kinetica.py). A player puts the held stack in
//! with a use, and takes out what was ground with a use of anything that
//! doesn't grind; breaking it drops all it holds.
//!
//! A depot holds one stack: a mechanical press turning over it works the
//! stack by the press's recipes (`pressing.json`). A hopper puts items in
//! either and takes what they made out from under them.
//!
//! The world's owner keeps what each holds with the motors' speeds (the
//! dimension's `kinetica-*.json`), and tells its players which are at work,
//! on what (the goggles show it), and what each depot holds (drawn on it).

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

/// The recipes of `file` in Kinetica's `data/kinetica/tachyon/`.
pub(super) fn read(file: &str) -> Vec<Milling> {
    let Some(root) = horizon_client::brand::content() else {
        return Vec::new();
    };
    let path = root.join("kinetica/data/kinetica/tachyon").join(file);
    let Ok(text) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Recipes>(&text)
        .map(|r| r.recipes)
        .map_err(|e| log::warn!("Tachyon: {}: {e}", path.display()))
        .unwrap_or_default()
}

/// The milling recipes of the built-in content, read once.
pub(super) fn recipes() -> &'static [Milling] {
    static RECIPES: OnceLock<Vec<Milling>> = OnceLock::new();
    RECIPES.get_or_init(|| read("milling.json"))
}

/// The press's recipes, read once.
pub(super) fn pressing() -> &'static [Milling] {
    static RECIPES: OnceLock<Vec<Milling>> = OnceLock::new();
    RECIPES.get_or_init(|| read("pressing.json"))
}

/// The recipe that grinds `item`.
pub(super) fn recipe(item: &str) -> Option<&'static Milling> {
    recipes().iter().find(|r| r.input == item)
}

/// The recipe that presses `item`.
pub(super) fn press_recipe(item: &str) -> Option<&'static Milling> {
    pressing().iter().find(|r| r.input == item)
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
    /// One item of what was made, else (when `input_too`) of the items to
    /// work: a hopper's take, and whether it was of the items to work.
    pub fn take_one(&mut self, input_too: bool) -> Option<(Held, bool)> {
        if self.outputs.is_empty() {
            if !input_too {
                return None;
            }
            let one = take_from(&mut self.input)?;
            if self.input.is_none() {
                self.progress = 0.0;
            }
            return Some((one, true));
        }
        let h = &mut self.outputs[0];
        h.count -= 1;
        let one = Held {
            item: h.item.clone(),
            count: 1,
        };
        if h.count == 0 {
            self.outputs.remove(0);
        }
        Some((one, false))
    }
    /// Puts back what a hopper took (`take_one`) and couldn't keep.
    pub fn give_back(&mut self, h: Held, input: bool) {
        match &mut self.input {
            Some(i) if input && i.item == h.item => i.count += h.count,
            None if input => self.input = Some(h),
            _ => add_all(&mut self.outputs, &[h]),
        }
    }
    /// Adds what was made elsewhere (a fan's air working the stack) to
    /// what it made, if it fits. Whether it did.
    pub fn add_made(&mut self, made: &[Held]) -> bool {
        if !self.fits(made) {
            return false;
        }
        add_all(&mut self.outputs, made);
        true
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

/// One item of the stack in `cell`.
fn take_from(cell: &mut Option<Held>) -> Option<Held> {
    let h = cell.as_mut()?;
    h.count -= 1;
    let one = Held {
        item: h.item.clone(),
        count: 1,
    };
    if h.count == 0 {
        *cell = None;
    }
    Some(one)
}

/// Adds `made` to `stacks`: onto stacks of the same item up to 64, the rest
/// as new stacks.
pub(super) fn add_all(stacks: &mut Vec<Held>, made: &[Held]) {
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
            self.tachyon_spend_held(took);
            return true;
        }
        let out = mill.take();
        for h in &out {
            self.give_or_drop(ItemStack::new(&h.item, h.count));
        }
        !out.is_empty()
    }

    /// The player used a depot: an empty one takes the held stack, else all
    /// it holds comes out. Whether anything moved.
    pub(super) fn tachyon_use_depot(&mut self, depot: &mut Mill) -> bool {
        if depot.is_empty() {
            let held = (self.held())
                .filter(|s| s.extra.is_none())
                .map(|s| (s.item.to_string(), s.count));
            let Some((item, count)) = held else {
                return false;
            };
            let took = depot.put(&item, count);
            self.tachyon_spend_held(took);
            return took > 0;
        }
        for h in depot.take_all() {
            self.give_or_drop(ItemStack::new(&h.item, h.count));
        }
        true
    }

    /// `n` of the held stack gone into a machine (not in creative mode).
    pub(super) fn tachyon_spend_held(&mut self, n: u32) {
        if self.creative || n == 0 {
            return;
        }
        let cell = self.held_mut();
        if let Some(s) = cell.as_mut() {
            s.count = s.count.saturating_sub(n);
            if s.count == 0 {
                *cell = None;
            }
        }
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
    fn a_hopper_takes_what_was_made_one_by_one() {
        let mut m = Mill::default();
        m.put("minecraft:iron_ingot", 2);
        assert_eq!(m.take_one(false), None);
        let sheet = Held {
            item: "x:sheet".into(),
            count: 2,
        };
        m.give_back(sheet.clone(), false);
        assert_eq!(m.take_one(false).unwrap().0.item, "x:sheet");
        assert_eq!(m.take_one(true).unwrap(), (Held { count: 1, ..sheet }, false));
        let (ingot, input) = m.take_one(true).unwrap();
        assert_eq!((ingot.item.as_str(), input), ("minecraft:iron_ingot", true));
        assert_eq!(m.input.as_ref().unwrap().count, 1);
        // Back where it came from.
        m.give_back(ingot, true);
        assert_eq!(m.input.as_ref().unwrap().count, 2);
        assert!(m.outputs.is_empty());
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
