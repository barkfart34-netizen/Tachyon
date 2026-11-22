//! Create's own processing recipes, for its machines that Kinetica's engine
//! turns: read from the Create ported into the game folder
//! (`horizon-mods/create/data/create/recipe/`; Create's data is MIT, as its
//! code), once per registry. A Create machine works by them first and by
//! Kinetica's after; a Kinetica one the other way round.
//!
//! Read as Create 6.0.10 writes them: `ingredients` (an item, a tag, or a
//! list of either), `results` (`id`, `count`, `chance`), `processing_time`,
//! `heat_requirement` (`ProcessingRecipe`, `HeatCondition`), and NeoForge's
//! `neoforge:conditions`. A tag stands for each of its items. Item-only
//! mixing and compacting keep their required heat; recipes with a fluid
//! wait for Tachyon's fluids. Those for another mod, or for an item this
//! game doesn't have, are left out.
//! Haunting follows Create's `HauntingRecipe`, a single item ingredient
//! and rollable results, as its `SplashingRecipe` does.
use super::basin::{HeatRequirement, Ingredient, Mixing};
use super::millstone::{Milling, Output};
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

/// A milling's work when the recipe gives none (Kinetica's).
const MILLING_TIME: f32 = 100.0;
/// A mixing's work when the recipe gives none (Kinetica's).
const MIXING_TIME: f32 = 100.0;
/// The most item sets a mixing with tags stands for.
const MOST_SETS: usize = 64;
/// The most item sets a mechanical crafting recipe with tags stands for.
const MOST_CRAFTING_SETS: usize = 256;

/// The recipes of Create's machines that Tachyon works.
#[derive(Default)]
pub(super) struct Recipes {
    pub milling: Vec<Milling>,
    /// The crushing wheels', before milling.
    pub crushing: Vec<Milling>,
    /// A saw's (`create:cutting`).
    pub cutting: Vec<Milling>,
    pub pressing: Vec<Milling>,
    pub splashing: Vec<Milling>,
    pub haunting: Vec<Milling>,
    /// A mixer's, over a basin.
    pub mixing: Vec<Mixing>,
    /// A press's over a basin.
    pub compacting: Vec<Mixing>,
    /// An item used on a block in the world (`ItemApplicationRecipe`).
    pub application: Vec<Application>,
    /// A deployer's (`DeployerApplicationRecipe`): its held item used on an item.
    pub deploying: Vec<Application>,
    /// A spout's (`FillingRecipe`): an item and a fluid.
    pub filling: Vec<Filling>,
    /// An item drain's (`EmptyingRecipe`): an item into an item and a fluid.
    pub emptying: Vec<Emptying>,
    /// A sequence of steps over a depot/belt (`SequencedAssemblyRecipe`).
    pub sequenced: Vec<SequencedAssembly>,
    /// Mechanical crafting (`MechanicalCraftingRecipe`), aggregated by item count.
    pub mechanical_crafting: Vec<Mixing>,
}

/// `create:sequenced_assembly`: transforms `ingredient` through `loops` repetitions
/// of `steps` via `transitional_item` into one weighted pick from `results`.
pub(super) struct SequencedAssembly {
    pub ingredient: Vec<String>,
    pub transitional_item: String,
    pub loops: u32,
    pub steps: Vec<SequencedStep>,
    pub results: Vec<Output>,
}

/// One step in a [`SequencedAssembly`] sequence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SequencedStep {
    /// `create:deploying`: uses one of `held`; `keep` leaves the held item unconsumed.
    Deploying { held: Vec<String>, keep: bool },
    /// `create:pressing`: one mechanical press stroke.
    Pressing,
    /// `create:cutting`: one mechanical saw cut.
    Cutting,
    /// `create:filling`: spout fills `amount` mB of one of `fluids`.
    Filling { fluids: Vec<String>, amount: u32 },
}

impl SequencedStep {
    /// Returns `(consume_held, consume_fluid)` when `action` satisfies this step.
    fn matches(&self, action: SequencedAction<'_>) -> Option<(bool, u32)> {
        match (self, action) {
            (SequencedStep::Deploying { held, keep }, SequencedAction::Deploying { held: h })
                if held.iter().any(|i| i == h) =>
            {
                Some((!*keep, 0))
            }
            (SequencedStep::Pressing, SequencedAction::Pressing)
            | (SequencedStep::Cutting, SequencedAction::Cutting) => Some((false, 0)),
            (
                SequencedStep::Filling {
                    fluids,
                    amount: need,
                },
                SequencedAction::Filling { fluid, amount: have },
            ) if have >= *need => {
                let fluid = fluid.strip_prefix(super::fluids::FLUID).unwrap_or(fluid);
                fluids.iter().any(|f| f == fluid).then_some((false, *need))
            }
            _ => None,
        }
    }
}

/// An action offered to [`Recipes::advance_sequenced`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SequencedAction<'a> {
    Deploying { held: &'a str },
    Pressing,
    Cutting,
    Filling { fluid: &'a str, amount: u32 },
}

/// Outcome of one matching [`SequencedAction`] in [`Recipes::advance_sequenced`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum SequencedStepResult {
    /// Intermediate step succeeded; item becomes `next_item` at `next_step`.
    NextStep {
        next_item: String,
        next_step: u32,
        consume_held: bool,
        consume_fluid: u32,
    },
    /// Final step succeeded; assembly finishes into `item` x `count`.
    Complete {
        item: String,
        count: u32,
        consume_held: bool,
        consume_fluid: u32,
    },
}

/// `create:filling`: one of `input` filled with `amount` mB of `fluid`.
pub(super) struct Filling {
    pub input: Vec<String>,
    pub fluid: String,
    pub amount: u32,
    pub outputs: Vec<Output>,
}

/// `create:emptying`: one of `input` emptied into its outputs, one of them a fluid
/// (`fluid:` stack name, its count in mB).
pub(super) struct Emptying {
    pub input: Vec<String>,
    pub outputs: Vec<Output>,
}

/// A fluid ingredient as an alternative: `fluid:<id>#<mB>`.
const AMOUNT: char = '#';

/// An ingredient's fluid and amount, from an alternative `alternatives` gave.
fn fluid_part(item: &str) -> Option<(&str, u32)> {
    let (fluid, amount) = item.strip_prefix(super::fluids::FLUID)?.split_once(AMOUNT)?;
    Some((fluid, amount.parse().ok()?))
}

/// `create:item_application`: `held` used on a block of `on` makes it
/// `result` (the first result; the rest go to the player).
pub(super) struct Application {
    pub on: Vec<String>,
    pub held: Vec<String>,
    pub result: String,
    pub extra: Vec<Output>,
    /// `keep_held_item`: the held item isn't used up.
    pub keep: bool,
    /// Not a `_manual_only` one (AllRecipeTypes.CAN_BE_AUTOMATED): a deployer may work it.
    pub automated: bool,
}

/// What a recipe reader asks of the game.
pub(super) struct Lookup<'a> {
    /// The items of a tag (`c:ingots/iron`), none for a tag it doesn't know.
    pub tagged: &'a dyn Fn(&str) -> Vec<String>,
    /// Whether the game has the item.
    pub known: &'a dyn Fn(&str) -> bool,
}

impl Recipes {
    /// Reads the recipes under `dir` (`data/create/recipe`).
    pub fn read(dir: &Path, game: &Lookup) -> Self {
        let mut recipes = Self::default();
        let mut files = Vec::new();
        for kind in [
            "milling",
            "crushing",
            "cutting",
            "pressing",
            "splashing",
            "haunting",
            "mixing",
            "compacting",
            "item_application",
            "deploying",
            "filling",
            "emptying",
            "sequenced_assembly",
            "mechanical_crafting",
        ] {
            walk(&dir.join(kind), &mut files);
        }
        files.sort();
        for file in files {
            let Some(recipe) = std::fs::read(&file)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
            else {
                continue;
            };
            let applications = recipes.application.len();
            recipes.add(&recipe, game);
            // A `_manual_only` application is by hand alone (AllRecipeTypes.CAN_BE_AUTOMATED).
            let stem = file.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if stem.ends_with("_manual_only") {
                for r in &mut recipes.application[applications..] {
                    r.automated = false;
                }
            }
        }
        recipes
    }

    /// Adds one recipe, if Tachyon can work it.
    pub fn add(&mut self, recipe: &Value, game: &Lookup) {
        if !conditions_hold(recipe, game) {
            return;
        }
        let kind = recipe["type"].as_str().unwrap_or("");
        let heat_requirement = match recipe.get("heat_requirement") {
            None => HeatRequirement::None,
            Some(heat) => match serde_json::from_value::<HeatRequirement>(heat.clone()) {
                Ok(heat) => heat,
                Err(_) => return,
            },
        };
        // Only basin recipes can require heat (BasinRecipe.canRequireHeat).
        if heat_requirement != HeatRequirement::None
            && !matches!(kind, "create:mixing" | "create:compacting")
        {
            return;
        }
        let time = recipe["processing_time"].as_f64().map(|t| t as f32);
        match kind {
            "create:sequenced_assembly" => {
                if let Some(seq) = parse_sequenced_assembly(recipe, game) {
                    self.sequenced.push(seq);
                }
                return;
            }
            "create:mechanical_crafting" => {
                if let Some(crafted) = parse_mechanical_crafting(recipe, game, time) {
                    self.mechanical_crafting.extend(crafted);
                }
                return;
            }
            _ => {}
        }
        let Some(outputs) = outputs(recipe, game) else {
            return;
        };
        let Some(ingredients) = ingredients(recipe, game) else {
            return;
        };
        // Fluids only where Tachyon's fluids go: a basin's, a spout's and a drain's.
        let fluid = ingredients.iter().flatten().any(|i| super::fluids::is_fluid(i))
            || outputs.iter().any(|o| super::fluids::is_fluid(&o.item));
        if fluid && !matches!(kind, "create:mixing" | "create:compacting" | "create:filling" | "create:emptying") {
            return;
        }
        let single = |list: &mut Vec<Milling>, default: f32| {
            let [items] = ingredients.as_slice() else {
                return;
            };
            for item in items {
                list.push(Milling {
                    input: item.clone(),
                    time: time.unwrap_or(default).max(1.0),
                    outputs: copy(&outputs),
                });
            }
        };
        match kind {
            "create:milling" => single(&mut self.milling, MILLING_TIME),
            // Create's own default: none, crushed at once.
            "create:crushing" => single(&mut self.crushing, 0.0),
            // ProcessingRecipeParams' duration starts at none.
            "create:cutting" => single(&mut self.cutting, 0.0),
            "create:pressing" => single(&mut self.pressing, super::millstone::TIME),
            "create:splashing" => single(&mut self.splashing, super::fan::TIME as f32),
            "create:haunting" => single(&mut self.haunting, super::fan::TIME as f32),
            "create:mixing" | "create:compacting" => {
                let list = if kind == "create:mixing" {
                    &mut self.mixing
                } else {
                    &mut self.compacting
                };
                for set in sets(&ingredients) {
                    // A fluid's alternative names its amount: the set counts it in mB.
                    let set = set
                        .into_iter()
                        .map(|i| match fluid_part(&i.item) {
                            Some((fluid, amount)) => Ingredient {
                                item: super::fluids::stack_name(fluid),
                                count: amount * i.count,
                            },
                            None => i,
                        })
                        .collect();
                    list.push(Mixing {
                        inputs: set,
                        heat_requirement,
                        time: time.unwrap_or(MIXING_TIME).max(1.0),
                        outputs: copy(&outputs),
                    });
                }
            }
            "create:filling" => {
                let [items, fluids] = ingredients.as_slice() else {
                    return;
                };
                for f in fluids {
                    let Some((fluid, amount)) = fluid_part(f) else { continue };
                    self.filling.push(Filling {
                        input: items.clone(),
                        fluid: fluid.to_owned(),
                        amount,
                        outputs: copy(&outputs),
                    });
                }
            }
            "create:emptying" => {
                let [items] = ingredients.as_slice() else {
                    return;
                };
                self.emptying.push(Emptying {
                    input: items.clone(),
                    outputs: copy(&outputs),
                });
            }
            "create:item_application" | "create:deploying" => {
                let [on, held] = ingredients.as_slice() else {
                    return;
                };
                let mut outputs = outputs.into_iter();
                let Some(first) = outputs.next() else {
                    return;
                };
                let list = if kind == "create:deploying" {
                    &mut self.deploying
                } else {
                    &mut self.application
                };
                list.push(Application {
                    on: on.clone(),
                    held: held.clone(),
                    result: first.item,
                    extra: outputs.collect(),
                    keep: recipe["keep_held_item"].as_bool().unwrap_or(false),
                    automated: true,
                });
            }
            _ => {}
        }
    }

    /// The application of `held` on a block whose item is `block`.
    pub fn application(&self, block: &str, held: &str) -> Option<&Application> {
        (self.application.iter())
            .find(|r| r.on.iter().any(|i| i == block) && r.held.iter().any(|i| i == held))
    }

    /// `DeployerBlockEntity.getRecipe`: deploying first, then item application, neither
    /// `_manual_only`, for the held item used on `input`.
    pub fn deploying(&self, input: &str, held: &str) -> Option<&Application> {
        (self.deploying.iter().chain(&self.application)).find(|r| {
            r.automated && r.on.iter().any(|i| i == input) && r.held.iter().any(|i| i == held)
        })
    }

    pub fn milling(&self, item: &str) -> Option<&Milling> {
        self.milling.iter().find(|r| r.input == item)
    }
    pub fn crushing(&self, item: &str) -> Option<&Milling> {
        self.crushing.iter().find(|r| r.input == item)
    }
    pub fn pressing(&self, item: &str) -> Option<&Milling> {
        self.pressing.iter().find(|r| r.input == item)
    }
    pub fn splashing(&self, item: &str) -> Option<&Milling> {
        self.splashing.iter().find(|r| r.input == item)
    }
    pub fn haunting(&self, item: &str) -> Option<&Milling> {
        self.haunting.iter().find(|r| r.input == item)
    }

    /// Checks whether `item` at `step_idx` matches the next expected step under `action`,
    /// returning the transitional [`SequencedStepResult::NextStep`] or final weighted
    /// [`SequencedStepResult::Complete`] (using `roll` in `0.0..1.0`).
    pub fn advance_sequenced(
        &self,
        item: &str,
        step_idx: u32,
        action: SequencedAction<'_>,
        roll: f32,
    ) -> Option<SequencedStepResult> {
        let step = step_idx as usize;
        for recipe in &self.sequenced {
            let Some(total) = recipe.steps.len().checked_mul(recipe.loops as usize) else {
                continue;
            };
            if total == 0 || step >= total {
                continue;
            }
            let item_matches = if step_idx == 0 {
                recipe.ingredient.iter().any(|i| i == item)
            } else {
                recipe.transitional_item == item
            };
            if !item_matches {
                continue;
            }
            let expected = &recipe.steps[step % recipe.steps.len()];
            let Some((consume_held, consume_fluid)) = expected.matches(action) else {
                continue;
            };
            if step + 1 >= total {
                let output = pick_weighted(&recipe.results, roll)?;
                return Some(SequencedStepResult::Complete {
                    item: output.item.clone(),
                    count: output.count,
                    consume_held,
                    consume_fluid,
                });
            }
            return Some(SequencedStepResult::NextStep {
                next_item: recipe.transitional_item.clone(),
                next_step: step_idx + 1,
                consume_held,
                consume_fluid,
            });
        }
        None
    }

    /// Finds the first step index (`1..steps.len()`) where `item` is already a recipe's
    /// `transitional_item` and `action` matches, used when a transitional item moves onto
    /// a new station without saved step progress.
    pub fn find_sequenced_step(&self, item: &str, action: SequencedAction<'_>) -> Option<u32> {
        for recipe in &self.sequenced {
            if recipe.transitional_item != item {
                continue;
            }
            for (idx, step) in recipe.steps.iter().enumerate().skip(1) {
                if step.matches(action).is_some() {
                    return Some(idx as u32);
                }
            }
        }
        None
    }
}

/// Parses a `create:sequenced_assembly` recipe.
fn parse_sequenced_assembly(recipe: &Value, game: &Lookup) -> Option<SequencedAssembly> {
    let ingredient = alternatives(recipe.get("ingredient")?, game)?;
    if ingredient.is_empty() || ingredient.iter().any(|i| super::fluids::is_fluid(i)) {
        return None;
    }
    let trans_val = recipe.get("transitional_item")?;
    let transitional_item = trans_val["id"]
        .as_str()
        .or_else(|| trans_val["item"].as_str())
        .or_else(|| trans_val.as_str())?;
    if !(game.known)(transitional_item) {
        return None;
    }
    let loops = match recipe.get("loops") {
        None => 1,
        Some(v) => u32::try_from(v.as_u64()?).ok().filter(|&n| n > 0)?,
    };
    let results = outputs(recipe, game)?;
    if results
        .iter()
        .any(|o| super::fluids::is_fluid(&o.item) || !o.chance.is_finite() || o.chance < 0.0)
        || results.iter().map(|o| o.chance).sum::<f32>() <= 0.0
    {
        return None;
    }
    let seq = recipe.get("sequence")?.as_array()?;
    if seq.is_empty() {
        return None;
    }
    let mut steps = Vec::with_capacity(seq.len());
    for step in seq {
        steps.push(parse_sequenced_step(step, game)?);
    }
    Some(SequencedAssembly {
        ingredient,
        transitional_item: transitional_item.to_owned(),
        loops,
        steps,
        results,
    })
}

fn parse_sequenced_step(step: &Value, game: &Lookup) -> Option<SequencedStep> {
    if !conditions_hold(step, game) {
        return None;
    }
    if let Some(heat) = step.get("heat_requirement") {
        let heat = serde_json::from_value::<HeatRequirement>(heat.clone()).ok()?;
        if heat != HeatRequirement::None {
            return None;
        }
    }
    match step["type"].as_str()? {
        "create:deploying" => {
            let held_val = step_target(step, game)?;
            let held = alternatives(held_val, game)?;
            if held.is_empty() || held.iter().any(|i| super::fluids::is_fluid(i)) {
                return None;
            }
            let keep = match step.get("keep_held_item") {
                None => false,
                Some(v) => v.as_bool()?,
            };
            Some(SequencedStep::Deploying { held, keep })
        }
        "create:pressing" => valid_item_ingredients(step, game).then_some(SequencedStep::Pressing),
        "create:cutting" => valid_item_ingredients(step, game).then_some(SequencedStep::Cutting),
        "create:filling" => {
            let fluid_val = step_target(step, game)?;
            let alts = alternatives(fluid_val, game)?;
            let mut fluids = Vec::new();
            let mut amount = None;
            for alt in &alts {
                let (fluid, mb) = fluid_part(alt)?;
                if mb == 0 || amount.is_some_and(|prev| prev != mb) {
                    return None;
                }
                amount = Some(mb);
                if !fluids.iter().any(|f| f == fluid) {
                    fluids.push(fluid.to_owned());
                }
            }
            let amount = amount?;
            (!fluids.is_empty()).then_some(SequencedStep::Filling { fluids, amount })
        }
        _ => None,
    }
}

/// Validates an optional leading transitional ingredient and returns the step's secondary
/// argument (held item for deploying, fluid for filling).
fn step_target<'a>(step: &'a Value, game: &Lookup) -> Option<&'a Value> {
    let list = step.get("ingredients")?.as_array()?;
    match list.as_slice() {
        [first, second] => {
            let base = alternatives(first, game)?;
            if base.is_empty() || base.iter().any(|i| super::fluids::is_fluid(i)) {
                return None;
            }
            Some(second)
        }
        [only] => Some(only),
        _ => None,
    }
}

fn valid_item_ingredients(step: &Value, game: &Lookup) -> bool {
    let Some(list) = step.get("ingredients") else {
        return true;
    };
    let Some(arr) = list.as_array() else {
        return false;
    };
    arr.iter().all(|ing| {
        alternatives(ing, game)
            .is_some_and(|items| !items.is_empty() && !items.iter().any(|i| super::fluids::is_fluid(i)))
    })
}

/// Picks one entry from `results` weighted by `Output::chance` (`SequencedAssemblyRecipe.rollResult`).
fn pick_weighted(results: &[Output], roll: f32) -> Option<&Output> {
    let total: f32 = results.iter().map(|o| o.chance.max(0.0)).sum();
    if total <= 0.0 {
        return results.first();
    }
    let r = if roll.is_nan() { 0.0 } else { roll.clamp(0.0, 1.0) };
    let mut target = r * total;
    for entry in results {
        let weight = entry.chance.max(0.0);
        if weight <= 0.0 {
            continue;
        }
        target -= weight;
        if target < 0.0 {
            return Some(entry);
        }
    }
    results.iter().rfind(|o| o.chance > 0.0).or_else(|| results.last())
}

/// Parses a `create:mechanical_crafting` recipe into [`Mixing`] entries with aggregated
/// pattern symbol counts.
fn parse_mechanical_crafting(recipe: &Value, game: &Lookup, time: Option<f32>) -> Option<Vec<Mixing>> {
    if let Some(mirrored) = recipe.get("accept_mirrored") {
        mirrored.as_bool()?;
    }
    let result = recipe.get("result")?;
    if result.get("fluid").is_some() || result.get("amount").is_some() {
        return None;
    }
    let result_item = result["id"].as_str().or_else(|| result["item"].as_str())?;
    if !(game.known)(result_item) {
        return None;
    }
    let result_count = result["count"].as_u64().unwrap_or(1).max(1) as u32;
    let outputs = [Output {
        item: result_item.to_owned(),
        count: result_count,
        chance: 1.0,
    }];
    let pattern = recipe.get("pattern")?.as_array()?;
    if pattern.is_empty() {
        return None;
    }
    let key = recipe.get("key")?.as_object()?;
    if key.contains_key(" ") {
        return None;
    }
    let mut symbol_counts: Vec<(char, u32)> = Vec::new();
    for row in pattern {
        let line = row.as_str()?;
        if line.is_empty() {
            return None;
        }
        for ch in line.chars() {
            if ch == ' ' {
                continue;
            }
            match symbol_counts.iter_mut().find(|(c, _)| *c == ch) {
                Some((_, n)) => *n += 1,
                None => symbol_counts.push((ch, 1)),
            }
        }
    }
    if symbol_counts.is_empty() {
        return None;
    }
    let mut grouped: Vec<(Vec<String>, u32)> = Vec::with_capacity(symbol_counts.len());
    let mut buf = [0u8; 4];
    for (ch, count) in symbol_counts {
        let sym = ch.encode_utf8(&mut buf);
        let ing = key.get(sym)?;
        let items = alternatives(ing, game)?;
        if items.is_empty() || items.iter().any(|i| super::fluids::is_fluid(i)) {
            return None;
        }
        grouped.push((items, count));
    }
    let time = time.unwrap_or(MIXING_TIME).max(1.0);
    Some(
        sets_counted(&grouped, MOST_CRAFTING_SETS)
            .into_iter()
            .map(|inputs| Mixing {
                inputs,
                heat_requirement: HeatRequirement::None,
                time,
                outputs: copy(&outputs),
            })
            .collect(),
    )
}

/// The JSON files under `dir`, into `out`.
fn walk(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
}

/// Whether a recipe's `neoforge:conditions` all hold here: only Create (and
/// Minecraft) are loaded, and a tag is empty when the game has no item in
/// it. A condition Tachyon doesn't know doesn't hold.
fn conditions_hold(recipe: &Value, game: &Lookup) -> bool {
    let Some(list) = recipe.get("neoforge:conditions") else {
        return true;
    };
    list.as_array()
        .is_some_and(|list| list.iter().all(|c| holds(c, game)))
}

fn holds(condition: &Value, game: &Lookup) -> bool {
    match condition["type"].as_str().unwrap_or("") {
        "neoforge:mod_loaded" => {
            matches!(condition["modid"].as_str(), Some("create" | "minecraft" | "neoforge"))
        }
        "neoforge:not" => !holds(&condition["value"], game),
        "neoforge:and" => (condition["values"].as_array())
            .is_some_and(|v| v.iter().all(|c| holds(c, game))),
        "neoforge:or" => (condition["values"].as_array())
            .is_some_and(|v| v.iter().any(|c| holds(c, game))),
        "neoforge:tag_empty" => condition["tag"]
            .as_str()
            .is_none_or(|t| tagged(t, game).is_empty()),
        "neoforge:true" => true,
        _ => false,
    }
}

/// A recipe's results; a fluid as its `fluid:` stack name counted in mB. None when one is
/// a fluid Tachyon doesn't know or an item the game doesn't have.
fn outputs(recipe: &Value, game: &Lookup) -> Option<Vec<Output>> {
    let mut outputs = Vec::new();
    for result in recipe["results"].as_array()? {
        if result.get("amount").is_some() || result.get("fluid").is_some() {
            let fluid = result["id"].as_str().or_else(|| result["fluid"].as_str())?;
            if !super::fluids::FLUIDS.contains(&fluid) {
                return None;
            }
            outputs.push(Output {
                item: super::fluids::stack_name(fluid),
                count: result["amount"].as_u64().unwrap_or(1000).max(1) as u32,
                chance: 1.0,
            });
            continue;
        }
        let item = result["id"].as_str().or_else(|| result["item"].as_str())?;
        if !(game.known)(item) {
            return None;
        }
        outputs.push(Output {
            item: item.to_owned(),
            count: result["count"].as_u64().unwrap_or(1).max(1) as u32,
            chance: result["chance"].as_f64().unwrap_or(1.0) as f32,
        });
    }
    (!outputs.is_empty()).then_some(outputs)
}

/// A recipe's ingredients, each the items that may stand in it; None when
/// one is a fluid, or no item the game has.
fn ingredients(recipe: &Value, game: &Lookup) -> Option<Vec<Vec<String>>> {
    let mut list = Vec::new();
    for ingredient in recipe["ingredients"].as_array()? {
        let items = alternatives(ingredient, game)?;
        if items.is_empty() {
            return None;
        }
        list.push(items);
    }
    (!list.is_empty()).then_some(list)
}

/// The items one ingredient takes: an item, a tag's, or a list's; None for
/// a fluid or an ingredient type Tachyon doesn't know.
fn alternatives(ingredient: &Value, game: &Lookup) -> Option<Vec<String>> {
    if let Some(list) = ingredient.as_array() {
        let mut items = Vec::new();
        for i in list {
            for item in alternatives(i, game)? {
                if !items.contains(&item) {
                    items.push(item);
                }
            }
        }
        return Some(items);
    }
    // A fluid: each fluid it takes, with its amount (`fluid_part`).
    if ingredient.get("fluid").is_some() || ingredient.get("amount").is_some() {
        let amount = ingredient["amount"].as_u64().unwrap_or(1000);
        let fluids: Vec<&str> = if let Some(f) = ingredient["fluid"].as_str() {
            super::fluids::FLUIDS.into_iter().filter(|k| *k == f).collect()
        } else if let Some(tag) = ingredient["tag"].as_str() {
            super::fluids::tagged(tag)
        } else {
            Vec::new()
        };
        return Some(fluids.into_iter().map(|f| format!("{}{f}{AMOUNT}{amount}", super::fluids::FLUID)).collect());
    }
    if ingredient.get("type").is_some() {
        return None;
    }
    if let Some(item) = ingredient["item"].as_str() {
        return Some(if (game.known)(item) { vec![item.to_owned()] } else { Vec::new() });
    }
    if let Some(tag) = ingredient["tag"].as_str() {
        let mut items = tagged(tag, game);
        items.sort();
        return Some(items);
    }
    if let Some(s) = ingredient.as_str() {
        if let Some(tag) = s.strip_prefix('#') {
            let mut items = tagged(tag, game);
            items.sort();
            return Some(items);
        }
        return Some(if (game.known)(s) { vec![s.to_owned()] } else { Vec::new() });
    }
    None
}

/// The items of a tag: the game's, else for a NeoForge convention tag
/// (`c:`) its likely members by the tag's name.
fn tagged(tag: &str, game: &Lookup) -> Vec<String> {
    let items = (game.tagged)(tag);
    if items.is_empty() {
        convention(tag, game.known)
    } else {
        items
    }
}

/// The overworld's woods, as their logs are named.
const WOODS: [&str; 9] = ["oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "pale_oak"];

/// NeoForge's convention tags (`c:ingots/iron`) are the loader's: a port
/// gets only those an oracle saw. Their likely members by the tag's name,
/// vanilla's and Create's, those the game has.
fn convention(tag: &str, known: &dyn Fn(&str) -> bool) -> Vec<String> {
    let Some(path) = tag.strip_prefix("c:") else {
        return Vec::new();
    };
    let (group, m) = path.split_once('/').unwrap_or((path, ""));
    let names: Vec<String> = match (group, m) {
        ("ingots", m) => vec![format!("{m}_ingot")],
        ("nuggets", m) => vec![format!("{m}_nugget")],
        ("plates", "gold") => vec!["golden_sheet".to_owned(), "gold_sheet".to_owned()],
        ("plates", "obsidian") => vec!["sturdy_sheet".to_owned()],
        ("plates", m) => vec![format!("{m}_sheet"), format!("{m}_plate")],
        ("raw_materials", m) => vec![format!("raw_{m}")],
        ("storage_blocks", m) => vec![format!("{m}_block")],
        ("ores", m) => vec![format!("{m}_ore"), format!("deepslate_{m}_ore"), format!("nether_{m}_ore")],
        ("gems", "lapis") => vec!["lapis_lazuli".to_owned()],
        ("gems", "amethyst") => vec!["amethyst_shard".to_owned()],
        ("gems", m) => vec![m.to_owned()],
        ("dusts", "glowstone") => vec!["glowstone_dust".to_owned()],
        ("dusts", "obsidian") => vec!["powdered_obsidian".to_owned()],
        ("dusts", m) => vec![m.to_owned()],
        ("rods", "wooden") => vec!["stick".to_owned()],
        ("stones", "") => ["stone", "andesite", "diorite", "granite", "deepslate", "tuff"]
            .map(str::to_owned)
            .to_vec(),
        ("ender_pearls", "") => vec!["ender_pearl".to_owned()],
        ("glass_blocks", "") => [
            "glass",
            "tinted_glass",
            "tiled_glass",
            "framed_glass",
            "horizontal_framed_glass",
            "vertical_framed_glass",
        ]
        .map(str::to_owned)
        .to_vec(),
        ("obsidians", "") => ["obsidian", "crying_obsidian"].map(str::to_owned).to_vec(),
        ("cobblestones", "") => ["cobblestone", "cobbled_deepslate", "mossy_cobblestone"].map(str::to_owned).to_vec(),
        ("eggs", "") => ["egg", "blue_egg", "brown_egg"].map(str::to_owned).to_vec(),
        ("strings", "") => vec!["string".to_owned()],
        ("stripped_logs", "") => WOODS
            .iter()
            .map(|w| format!("stripped_{w}_log"))
            .chain(["stripped_crimson_stem", "stripped_warped_stem", "stripped_bamboo_block"].map(str::to_owned))
            .collect(),
        ("stripped_woods", "") => WOODS
            .iter()
            .map(|w| format!("stripped_{w}_wood"))
            .chain(["stripped_crimson_hyphae", "stripped_warped_hyphae"].map(str::to_owned))
            .collect(),
        _ => Vec::new(),
    };
    (names.into_iter())
        .flat_map(|n| [format!("minecraft:{n}"), format!("create:{n}")])
        .filter(|item| known(item))
        .collect()
}

/// The sets of items a mixing's ingredients stand for (an item asked twice
/// counts twice), at most [`MOST_SETS`].
fn sets(ingredients: &[Vec<String>]) -> Vec<Vec<Ingredient>> {
    let counted: Vec<(Vec<String>, u32)> =
        ingredients.iter().map(|items| (items.clone(), 1)).collect();
    sets_counted(&counted, MOST_SETS)
}

/// Expands `(alternatives, count)` ingredient groups into item-counted sets, aggregating
/// repeated items and bounding the total combinations to `limit`.
fn sets_counted(ingredients: &[(Vec<String>, u32)], limit: usize) -> Vec<Vec<Ingredient>> {
    if ingredients.is_empty() {
        return Vec::new();
    }
    let mut sets: Vec<Vec<(String, u32)>> = vec![Vec::new()];
    for (items, count) in ingredients {
        let mut next = Vec::new();
        'all: for set in &sets {
            for item in items {
                let mut s = set.clone();
                s.push((item.clone(), *count));
                next.push(s);
                if next.len() >= limit {
                    break 'all;
                }
            }
        }
        sets = next;
    }
    sets.into_iter()
        .map(|set| {
            let mut counts: Vec<(String, u32)> = Vec::new();
            for (item, n) in set {
                match counts.iter_mut().find(|(i, _)| *i == item) {
                    Some((_, total)) => *total += n,
                    None => counts.push((item, n)),
                }
            }
            counts
                .into_iter()
                .map(|(item, count)| Ingredient { item, count })
                .collect()
        })
        .collect()
}

fn copy(outputs: &[Output]) -> Vec<Output> {
    (outputs.iter())
        .map(|o| Output {
            item: o.item.clone(),
            count: o.count,
            chance: o.chance,
        })
        .collect()
}

/// The tags of a game's items, for [`Lookup::tagged`].
pub(super) fn tagged_in<'a>(
    tags: &'a HashMap<String, std::collections::HashSet<String>>,
) -> impl Fn(&str) -> Vec<String> + 'a {
    move |tag| {
        let tag = tag.strip_prefix('#').unwrap_or(tag);
        tags.get(tag)
            .map(|items| items.iter().cloned().collect())
            .unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn game() -> (impl Fn(&str) -> Vec<String>, impl Fn(&str) -> bool) {
        let tagged = |tag: &str| match tag {
            "c:nuggets/iron" => vec!["minecraft:iron_nugget".to_owned()],
            "c:raw_materials/iron" => vec!["minecraft:raw_iron".to_owned()],
            _ => Vec::new(),
        };
        // No nickel here: its convention tag stays empty.
        let known = |item: &str| !item.starts_with("ae2:") && !item.contains("nickel");
        (tagged, known)
    }

    #[test]
    fn create_recipes_are_read_as_tachyon_works_them() {
        let (tagged, known) = game();
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        let mut r = Recipes::default();
        r.add(
            &json!({"type": "create:milling", "ingredients": [{"item": "minecraft:wheat"}],
                "processing_time": 150, "results": [{"id": "create:wheat_flour"},
                {"chance": 0.25, "count": 2, "id": "create:wheat_flour"}]}),
            &game,
        );
        let wheat = r.milling("minecraft:wheat").unwrap();
        assert_eq!(wheat.time, 150.0);
        assert_eq!((wheat.outputs[1].count, wheat.outputs[1].chance), (2, 0.25));
        r.add(
            &json!({"type": "create:haunting", "ingredients": [{"item": "minecraft:stone"}],
                "results": [{"id": "minecraft:infested_stone"}]}),
            &game,
        );
        let stone = r.haunting("minecraft:stone").unwrap();
        assert_eq!(stone.time, super::super::fan::TIME as f32);
        assert_eq!(stone.outputs[0].item, "minecraft:infested_stone");
        assert_eq!((stone.outputs[0].count, stone.outputs[0].chance), (1, 1.0));
        assert_eq!(r.haunting.len(), 1);
        assert!(r.haunting("minecraft:wheat").is_none());
        // A tag stands for its items; a condition on an empty tag drops it.
        r.add(
            &json!({"neoforge:conditions": [{"type": "neoforge:not", "value":
                {"type": "neoforge:tag_empty", "tag": "c:raw_materials/iron"}}],
                "type": "create:milling", "ingredients": [{"tag": "c:raw_materials/iron"}],
                "results": [{"id": "create:crushed_raw_iron"}]}),
            &game,
        );
        assert!(r.milling("minecraft:raw_iron").is_some());
        r.add(
            &json!({"neoforge:conditions": [{"type": "neoforge:not", "value":
                {"type": "neoforge:tag_empty", "tag": "c:raw_materials/nickel"}}],
                "type": "create:milling", "ingredients": [{"tag": "c:raw_materials/nickel"}],
                "results": [{"id": "create:crushed_raw_nickel"}]}),
            &game,
        );
        assert_eq!(r.milling.len(), 2);
        // Another mod's, a fluid Tachyon doesn't know, an unknown heat requirement: left out.
        r.add(
            &json!({"neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "ae2"}],
                "type": "create:milling", "ingredients": [{"item": "minecraft:stone"}],
                "results": [{"id": "minecraft:gravel"}]}),
            &game,
        );
        r.add(
            &json!({"type": "create:mixing", "ingredients": [{"item": "minecraft:dirt"},
                {"type": "neoforge:single", "amount": 250, "fluid": "x:goo"}],
                "results": [{"id": "minecraft:mud"}]}),
            &game,
        );
        r.add(
            &json!({"type": "create:mixing", "heat_requirement": "unknown",
                "ingredients": [{"item": "minecraft:copper_ingot"}],
                "results": [{"id": "create:brass_ingot"}]}),
            &game,
        );
        assert_eq!((r.milling.len(), r.mixing.len()), (2, 0));
        // A mixing: its items counted, a tag's item standing in.
        r.add(
            &json!({"type": "create:mixing", "ingredients": [{"item": "minecraft:andesite"},
                {"tag": "c:nuggets/iron"}], "results": [{"id": "create:andesite_alloy"}]}),
            &game,
        );
        r.add(
            &json!({"type": "create:compacting", "ingredients": [{"item": "minecraft:snow_block"},
                {"item": "minecraft:snow_block"}], "results": [{"id": "minecraft:ice"}]}),
            &game,
        );
        let alloy = &r.mixing[0];
        assert_eq!(alloy.inputs[1].item, "minecraft:iron_nugget");
        let ice = &r.compacting[0];
        assert_eq!((r.mixing.len(), ice.inputs.len(), ice.inputs[0].count), (1, 1, 2));
        assert_eq!(alloy.heat_requirement, HeatRequirement::None);
        assert_eq!(ice.heat_requirement, HeatRequirement::None);
    }

    #[test]
    fn item_only_heated_recipes_are_read_and_work_only_with_their_heat() {
        use super::super::basin::Basin;
        use super::super::burner::Heat;

        let (tagged, known) = game();
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        let mut r = Recipes::default();
        r.add(
            &json!({"type": "create:mixing", "heat_requirement": "heated",
                "ingredients": [{"item": "minecraft:copper_ingot"}, {"item": "create:zinc_ingot"}],
                "results": [{"id": "create:brass_ingot", "count": 2}]}),
            &game,
        );
        r.add(
            &json!({"type": "create:compacting", "heat_requirement": "superheated",
                "ingredients": [{"item": "x:ore"}], "results": [{"id": "x:ingot"}]}),
            &game,
        );
        assert_eq!((r.mixing.len(), r.compacting.len()), (1, 1));
        assert_eq!(r.mixing[0].heat_requirement, HeatRequirement::Heated);
        assert_eq!(r.compacting[0].heat_requirement, HeatRequirement::Superheated);
        let mut basin = Basin::default();
        basin.put("minecraft:copper_ingot", 1);
        basin.put("create:zinc_ingot", 1);
        for heat in [Heat::None, Heat::Smouldering, Heat::Fading] {
            for _ in 0..81 {
                assert_eq!(basin.mix(32.0, &r.mixing, heat, &mut || 0.0), None);
            }
        }
        assert_eq!(basin.inputs.len(), 2);
        assert!(basin.outputs.is_empty());
        for _ in 0..81 {
            basin.mix(32.0, &r.mixing, Heat::Kindled, &mut || 0.0);
        }
        assert!(basin.inputs.is_empty());
        assert_eq!(
            (basin.outputs[0].item.as_str(), basin.outputs[0].count),
            ("create:brass_ingot", 2)
        );
        let mut basin = Basin::default();
        basin.put("x:ore", 1);
        for _ in 0..62 {
            assert_eq!(basin.compact(32.0, &r.compacting, Heat::Kindled, &mut || 0.0), None);
        }
        assert_eq!(basin.inputs[0].count, 1);
        for _ in 0..31 {
            basin.compact(32.0, &r.compacting, Heat::Seething, &mut || 0.0);
        }
        assert!(basin.inputs.is_empty());
        assert_eq!(basin.outputs[0].item, "x:ingot");
    }

    #[test]
    fn heat_does_not_admit_fluid_recipes_or_invalid_requirements() {
        let (tagged, known) = game();
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        let mut r = Recipes::default();
        for heat in ["none", "heated", "superheated"] {
            r.add(
                &json!({"type": "create:mixing", "heat_requirement": heat,
                    "ingredients": [{"item": "minecraft:dirt"}, {"fluid": "x:goo", "amount": 250}],
                    "results": [{"id": "minecraft:mud"}]}),
                &game,
            );
            r.add(
                &json!({"type": "create:compacting", "heat_requirement": heat,
                    "ingredients": [{"item": "minecraft:snow_block"}],
                    "results": [{"id": "x:goo", "amount": 250}]}),
                &game,
            );
        }
        for heat in [json!("unknown"), json!(1), Value::Null] {
            r.add(
                &json!({"type": "create:mixing", "heat_requirement": heat,
                    "ingredients": [{"item": "x:ore"}], "results": [{"id": "x:ingot"}]}),
                &game,
            );
        }
        r.add(
            &json!({"type": "create:milling", "heat_requirement": "heated",
                "ingredients": [{"item": "x:ore"}], "results": [{"id": "x:ingot"}]}),
            &game,
        );
        assert!(r.mixing.is_empty());
        assert!(r.compacting.is_empty());
        assert!(r.milling.is_empty());
    }

    #[test]
    fn fluids_go_in_basin_spout_and_drain_recipes_counted_in_mb() {
        let tagged = |_: &str| Vec::new();
        let known = |item: &str| !item.starts_with("x:");
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        let mut r = Recipes::default();
        r.add(
            &json!({"type": "create:mixing", "heat_requirement": "heated", "ingredients": [
                {"item": "minecraft:sugar"}, {"type": "neoforge:tag", "amount": 250, "tag": "c:milk"}],
                "results": [{"amount": 250, "id": "create:chocolate"}]}),
            &game,
        );
        let m = &r.mixing[0];
        assert_eq!(m.inputs[1].item, "fluid:minecraft:milk");
        assert_eq!(m.inputs[1].count, 250);
        assert_eq!((m.outputs[0].item.as_str(), m.outputs[0].count), ("fluid:create:chocolate", 250));
        r.add(
            &json!({"type": "create:filling", "ingredients": [{"item": "minecraft:glass_bottle"},
                {"type": "neoforge:tag", "amount": 250, "tag": "c:honey"}],
                "results": [{"id": "minecraft:honey_bottle"}]}),
            &game,
        );
        assert_eq!((r.filling[0].fluid.as_str(), r.filling[0].amount), ("create:honey", 250));
        r.add(
            &json!({"type": "create:emptying", "ingredients": [{"item": "minecraft:honey_bottle"}],
                "results": [{"id": "minecraft:glass_bottle"}, {"amount": 250, "id": "create:honey"}]}),
            &game,
        );
        assert_eq!(r.emptying[0].outputs[1].item, "fluid:create:honey");
        // A fluid in a milling isn't Tachyon's to work.
        r.add(
            &json!({"type": "create:milling", "ingredients": [{"item": "minecraft:stone"}],
                "results": [{"amount": 100, "id": "minecraft:water"}]}),
            &game,
        );
        assert!(r.milling.is_empty());
    }

    #[test]
    fn a_convention_tag_the_game_lacks_stands_for_its_likely_items() {
        let tagged = |_: &str| Vec::new();
        let known = |item: &str| matches!(item, "minecraft:iron_ingot" | "minecraft:raw_iron" | "create:zinc_nugget");
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        assert_eq!(tagged_in_game("c:ingots/iron", &game), ["minecraft:iron_ingot"]);
        assert_eq!(tagged_in_game("c:raw_materials/iron", &game), ["minecraft:raw_iron"]);
        assert_eq!(tagged_in_game("c:nuggets/zinc", &game), ["create:zinc_nugget"]);
        assert!(tagged_in_game("c:ingots/tin", &game).is_empty());
        assert!(tagged_in_game("minecraft:logs", &game).is_empty());
    }

    fn tagged_in_game(tag: &str, game: &Lookup) -> Vec<String> {
        tagged(tag, game)
    }

    #[test]
    fn a_mixing_with_tags_stands_for_each_set_of_their_items() {
        let ingredients = vec![
            vec!["a".to_owned(), "b".to_owned()],
            vec!["c".to_owned(), "d".to_owned()],
        ];
        assert_eq!(sets(&ingredients).len(), 4);
        let many = vec![(0..20).map(|i| i.to_string()).collect::<Vec<_>>(); 3];
        assert_eq!(sets(&many).len(), MOST_SETS);
    }

    #[test]
    fn sequenced_assembly_parses_advances_loops_and_rolls_weighted_results() {
        let (tagged, known) = game();
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        let mut r = Recipes::default();
        r.add(
            &json!({
                "type": "create:sequenced_assembly",
                "ingredient": {"tag": "c:plates/gold"},
                "transitional_item": {"id": "create:incomplete_precision_mechanism"},
                "loops": 5,
                "sequence": [
                    {
                        "type": "create:deploying",
                        "ingredients": [
                            {"item": "create:incomplete_precision_mechanism"},
                            {"item": "create:cogwheel"}
                        ],
                        "results": [{"id": "create:incomplete_precision_mechanism"}]
                    },
                    {
                        "type": "create:deploying",
                        "ingredients": [
                            {"item": "create:incomplete_precision_mechanism"},
                            {"item": "create:large_cogwheel"}
                        ],
                        "results": [{"id": "create:incomplete_precision_mechanism"}]
                    },
                    {
                        "type": "create:deploying",
                        "ingredients": [
                            {"item": "create:incomplete_precision_mechanism"},
                            {"tag": "c:nuggets/iron"}
                        ],
                        "results": [{"id": "create:incomplete_precision_mechanism"}]
                    }
                ],
                "results": [
                    {"chance": 120.0, "id": "create:precision_mechanism"},
                    {"chance": 8.0, "id": "create:golden_sheet"},
                    {"id": "minecraft:clock"}
                ]
            }),
            &game,
        );
        r.add(
            &json!({
                "type": "create:sequenced_assembly",
                "ingredient": {"item": "create:powdered_obsidian"},
                "transitional_item": {"id": "create:unprocessed_obsidian_sheet"},
                "sequence": [
                    {
                        "type": "create:filling",
                        "ingredients": [
                            {"item": "create:unprocessed_obsidian_sheet"},
                            {"type": "neoforge:single", "fluid": "minecraft:lava", "amount": 500}
                        ],
                        "results": [{"id": "create:unprocessed_obsidian_sheet"}]
                    },
                    {
                        "type": "create:pressing",
                        "ingredients": [{"item": "create:unprocessed_obsidian_sheet"}],
                        "results": [{"id": "create:unprocessed_obsidian_sheet"}]
                    }
                ],
                "results": [{"id": "create:sturdy_sheet"}]
            }),
            &game,
        );
        assert_eq!(r.sequenced.len(), 2);
        assert_eq!(r.sequenced[0].loops, 5);
        assert_eq!(r.sequenced[1].loops, 1);

        // Wrong action on step 0 is rejected.
        assert_eq!(
            r.advance_sequenced("create:golden_sheet", 0, SequencedAction::Pressing, 0.0),
            None
        );

        // Step through all 5 loops (15 steps) of Precision Mechanism.
        let mut item = "create:golden_sheet".to_owned();
        let mut step = 0;
        let step_items = [
            "create:cogwheel",
            "create:large_cogwheel",
            "minecraft:iron_nugget",
        ];
        for i in 0..14 {
            let held = step_items[i % 3];
            let res = r.advance_sequenced(
                &item,
                step,
                SequencedAction::Deploying { held },
                0.0,
            );
            assert_eq!(
                res,
                Some(SequencedStepResult::NextStep {
                    next_item: "create:incomplete_precision_mechanism".to_owned(),
                    next_step: i as u32 + 1,
                    consume_held: true,
                    consume_fluid: 0,
                })
            );
            item = "create:incomplete_precision_mechanism".to_owned();
            step += 1;
        }
        // Final step yields weighted output depending on roll.
        assert_eq!(
            r.advance_sequenced(
                &item,
                14,
                SequencedAction::Deploying { held: "minecraft:iron_nugget" },
                0.5,
            ),
            Some(SequencedStepResult::Complete {
                item: "create:precision_mechanism".to_owned(),
                count: 1,
                consume_held: true,
                consume_fluid: 0,
            })
        );
        assert_eq!(
            r.advance_sequenced(
                &item,
                14,
                SequencedAction::Deploying { held: "minecraft:iron_nugget" },
                0.995,
            ),
            Some(SequencedStepResult::Complete {
                item: "minecraft:clock".to_owned(),
                count: 1,
                consume_held: true,
                consume_fluid: 0,
            })
        );

        // Sturdy sheet filling + pressing path.
        assert_eq!(
            r.advance_sequenced(
                "create:powdered_obsidian",
                0,
                SequencedAction::Filling { fluid: "fluid:minecraft:lava", amount: 500 },
                0.0
            ),
            Some(SequencedStepResult::NextStep {
                next_item: "create:unprocessed_obsidian_sheet".to_owned(),
                next_step: 1,
                consume_held: false,
                consume_fluid: 500,
            })
        );
        assert_eq!(
            r.find_sequenced_step("create:unprocessed_obsidian_sheet", SequencedAction::Pressing),
            Some(1)
        );
        assert_eq!(
            r.advance_sequenced(
                "create:unprocessed_obsidian_sheet",
                1,
                SequencedAction::Pressing,
                0.0
            ),
            Some(SequencedStepResult::Complete {
                item: "create:sturdy_sheet".to_owned(),
                count: 1,
                consume_held: false,
                consume_fluid: 0,
            })
        );
    }

    #[test]
    fn mechanical_crafting_aggregates_pattern_counts_into_mixing_recipes() {
        let tagged = |_: &str| Vec::new();
        let known = |item: &str| matches!(
            item,
            "create:andesite_alloy" | "minecraft:stone" | "minecraft:oak_planks" | "create:crushing_wheel"
        );
        let game = Lookup {
            tagged: &tagged,
            known: &known,
        };
        let mut r = Recipes::default();
        r.add(
            &json!({
                "type": "create:mechanical_crafting",
                "accept_mirrored": false,
                "key": {
                    "A": {"item": "create:andesite_alloy"},
                    "P": {"item": "minecraft:oak_planks"},
                    "S": {"tag": "c:stones"}
                },
                "pattern": [
                    " AAA ",
                    "AAPAA",
                    "APSPA",
                    "AAPAA",
                    " AAA "
                ],
                "result": {"count": 2, "id": "create:crushing_wheel"}
            }),
            &game,
        );
        assert_eq!(r.mechanical_crafting.len(), 1);
        let wheel = &r.mechanical_crafting[0];
        assert_eq!(wheel.heat_requirement, HeatRequirement::None);
        assert_eq!(wheel.outputs[0].item, "create:crushing_wheel");
        assert_eq!(wheel.outputs[0].count, 2);
        let count_of = |id: &str| wheel.inputs.iter().find(|i| i.item == id).map(|i| i.count);
        assert_eq!(count_of("create:andesite_alloy"), Some(16));
        assert_eq!(count_of("minecraft:oak_planks"), Some(4));
        assert_eq!(count_of("minecraft:stone"), Some(1));
    }
}
