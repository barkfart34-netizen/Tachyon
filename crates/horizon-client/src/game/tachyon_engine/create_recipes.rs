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

/// The recipes of Create's machines that Tachyon works.
#[derive(Default)]
pub(super) struct Recipes {
    pub milling: Vec<Milling>,
    /// The crushing wheels', before milling.
    pub crushing: Vec<Milling>,
    pub pressing: Vec<Milling>,
    pub splashing: Vec<Milling>,
    pub haunting: Vec<Milling>,
    /// A mixer's, over a basin.
    pub mixing: Vec<Mixing>,
    /// A press's over a basin.
    pub compacting: Vec<Mixing>,
    /// An item used on a block in the world (`ItemApplicationRecipe`).
    pub application: Vec<Application>,
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
            "pressing",
            "splashing",
            "haunting",
            "mixing",
            "compacting",
            "item_application",
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
            recipes.add(&recipe, game);
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
        let Some(outputs) = outputs(recipe, game) else {
            return;
        };
        let Some(ingredients) = ingredients(recipe, game) else {
            return;
        };
        let time = recipe["processing_time"].as_f64().map(|t| t as f32);
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
                    list.push(Mixing {
                        inputs: set,
                        heat_requirement,
                        time: time.unwrap_or(MIXING_TIME).max(1.0),
                        outputs: copy(&outputs),
                    });
                }
            }
            "create:item_application" => {
                let [on, held] = ingredients.as_slice() else {
                    return;
                };
                let mut outputs = outputs.into_iter();
                let Some(first) = outputs.next() else {
                    return;
                };
                self.application.push(Application {
                    on: on.clone(),
                    held: held.clone(),
                    result: first.item,
                    extra: outputs.collect(),
                    keep: recipe["keep_held_item"].as_bool().unwrap_or(false),
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

/// A recipe's item results; None when one is a fluid or an item the game
/// doesn't have.
fn outputs(recipe: &Value, game: &Lookup) -> Option<Vec<Output>> {
    let mut outputs = Vec::new();
    for result in recipe["results"].as_array()? {
        if result.get("amount").is_some() || result.get("fluid").is_some() {
            return None;
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
    if ingredient.get("fluid").is_some() || ingredient.get("amount").is_some() {
        return None;
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
        ("raw_materials", m) => vec![format!("raw_{m}")],
        ("storage_blocks", m) => vec![format!("{m}_block")],
        ("ores", m) => vec![format!("{m}_ore"), format!("deepslate_{m}_ore"), format!("nether_{m}_ore")],
        ("gems", "lapis") => vec!["lapis_lazuli".to_owned()],
        ("gems", "amethyst") => vec!["amethyst_shard".to_owned()],
        ("gems", m) => vec![m.to_owned()],
        ("dusts", "glowstone") => vec!["glowstone_dust".to_owned()],
        ("dusts", m) => vec![m.to_owned()],
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
    let mut sets: Vec<Vec<String>> = vec![Vec::new()];
    for items in ingredients {
        let mut next = Vec::new();
        'all: for set in &sets {
            for item in items {
                let mut s = set.clone();
                s.push(item.clone());
                next.push(s);
                if next.len() >= MOST_SETS {
                    break 'all;
                }
            }
        }
        sets = next;
    }
    sets.into_iter()
        .map(|set| {
            let mut counts: Vec<(String, u32)> = Vec::new();
            for item in set {
                match counts.iter_mut().find(|(i, _)| *i == item) {
                    Some((_, n)) => *n += 1,
                    None => counts.push((item, 1)),
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
        // Another mod's, a fluid, an unknown heat requirement: left out.
        r.add(
            &json!({"neoforge:conditions": [{"type": "neoforge:mod_loaded", "modid": "ae2"}],
                "type": "create:milling", "ingredients": [{"item": "minecraft:stone"}],
                "results": [{"id": "minecraft:gravel"}]}),
            &game,
        );
        r.add(
            &json!({"type": "create:mixing", "ingredients": [{"item": "minecraft:dirt"},
                {"type": "neoforge:single", "amount": 250, "fluid": "minecraft:water"}],
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
                    "ingredients": [{"item": "minecraft:dirt"}, {"fluid": "minecraft:water", "amount": 250}],
                    "results": [{"id": "minecraft:mud"}]}),
                &game,
            );
            r.add(
                &json!({"type": "create:compacting", "heat_requirement": heat,
                    "ingredients": [{"item": "minecraft:snow_block"}],
                    "results": [{"id": "minecraft:water", "amount": 250}]}),
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
}
