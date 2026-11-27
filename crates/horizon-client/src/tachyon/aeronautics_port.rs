//! Post-import declarations, state properties, and cross-mod recipe fixups
//! for Create Aeronautics (`simulated`, `aeronautics`, `offroad`) in Tachyon.
//!
//! `horizon-port-pack` skips blocks whose Forge/NeoForge state properties do not
//! match a vanilla template 1:1 (such as `adjustable_burner`, `physics_assembler`,
//! `steering_wheel`, `propeller_bearing`, `wheel_mount`, and portable engines) as
//! well as recipes with NeoForge conditions or 1.20/1.21 format quirks. This
//! module writes clean `horizon-mod-package` block/item declarations and upgrades
//! crafting recipes automatically after JAR import and during startup refresh.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;
use std::fs;
use std::io::Read;
use std::path::Path;
use zip::ZipArchive;

pub const VERSION: &str = "1";
const STAMP: &str = ".tachyon-aero-fix";

const DYES: &[&str] = &[
    "white",
    "orange",
    "magenta",
    "light_blue",
    "yellow",
    "lime",
    "pink",
    "gray",
    "light_gray",
    "cyan",
    "purple",
    "blue",
    "brown",
    "green",
    "red",
    "black",
];

const FACING_4: &[&str] = &["north", "south", "west", "east"];
const FACING_6: &[&str] = &["down", "up", "north", "south", "west", "east"];
const AXIS_3: &[&str] = &["x", "y", "z"];
const AXIS_XZ: &[&str] = &["x", "z"];
const FACES_3: &[&str] = &["floor", "wall", "ceiling"];
const NAMEPLATE_POS: &[&str] = &["single", "left", "middle", "right"];

enum PropDef {
    Bool(&'static str, &'static str),
    Enum(&'static str, &'static [&'static str], &'static str),
}

struct BlockSpec<'a> {
    name: &'a str,
    hardness: f64,
    sounds: &'a str,
    occludes: bool,
    props: Vec<PropDef>,
}

impl<'a> BlockSpec<'a> {
    fn new(name: &'a str, hardness: f64, sounds: &'a str, occludes: bool, props: Vec<PropDef>) -> Self {
        Self {
            name,
            hardness,
            sounds,
            occludes,
            props,
        }
    }
}

/// Returns `true` if `mod_id` is one of the Create Aeronautics bundle mods.
pub fn is_aeronautics_mod(mod_id: &str) -> bool {
    matches!(mod_id, "simulated" | "aeronautics" | "offroad")
}

fn read_json(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    serde_json::from_slice(&bytes).ok()
}

fn write_json(path: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    fs::write(path, format!("{text}\n")).map_err(|e| format!("{}: {e}", path.display()))
}

fn build_block_decl(spec: &BlockSpec<'_>) -> Value {
    let mut state_properties = Map::new();
    let mut default_state = Map::new();
    for p in &spec.props {
        match p {
            PropDef::Bool(k, def) => {
                state_properties.insert((*k).to_owned(), json!({"type": "boolean"}));
                default_state.insert((*k).to_owned(), json!(*def));
            }
            PropDef::Enum(k, vals, def) => {
                state_properties.insert((*k).to_owned(), json!({"type": "enum", "values": vals}));
                default_state.insert((*k).to_owned(), json!(*def));
            }
        }
    }

    let mut obj = Map::new();
    // Base template has zero built-in state properties so explicit state_properties never collide.
    obj.insert("template".to_owned(), json!("minecraft:polished_andesite"));
    obj.insert(
        "properties".to_owned(),
        json!({
            "hardness": spec.hardness,
            "sounds": spec.sounds,
            "occludes": spec.occludes,
        }),
    );
    if !state_properties.is_empty() {
        obj.insert("state_properties".to_owned(), Value::Object(state_properties));
        obj.insert("default_state".to_owned(), Value::Object(default_state));
    }
    Value::Object(obj)
}

fn aeronautics_specs() -> Vec<BlockSpec<'static>> {
    let mut out = vec![
        BlockSpec::new(
            "adjustable_burner",
            3.0,
            "minecraft:netherite_block",
            false,
            vec![
                PropDef::Bool("powered", "false"),
                PropDef::Enum("variant", &["fire", "soulful"], "fire"),
            ],
        ),
        BlockSpec::new(
            "andesite_propeller",
            2.0,
            "minecraft:stone",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("reversed", "false"),
            ],
        ),
        BlockSpec::new(
            "wooden_propeller",
            1.5,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("reversed", "false"),
            ],
        ),
        BlockSpec::new(
            "smart_propeller",
            2.5,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("axis", AXIS_XZ, "z"),
                PropDef::Bool("ceiling", "false"),
                PropDef::Bool("reversed", "false"),
            ],
        ),
        BlockSpec::new(
            "propeller_bearing",
            2.5,
            "minecraft:copper_block",
            false,
            vec![PropDef::Enum("facing", FACING_6, "north")],
        ),
        BlockSpec::new(
            "gyroscopic_propeller_bearing",
            3.0,
            "minecraft:gold_block",
            false,
            vec![PropDef::Enum("facing", FACING_6, "north")],
        ),
        BlockSpec::new(
            "steam_vent",
            2.5,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("powered", "false"),
                PropDef::Enum("variant", &["gold", "iron"], "iron"),
                PropDef::Bool("waterlogged", "false"),
            ],
        ),
        BlockSpec::new(
            "mounted_potato_cannon",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Bool("blocked", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new("levitite", 2.0, "minecraft:amethyst_block", false, vec![]),
        BlockSpec::new("pearlescent_levitite", 2.0, "minecraft:amethyst_block", false, vec![]),
        BlockSpec::new("levitite_blend", 1.0, "minecraft:sand", true, vec![]),
    ];

    for dye in DYES {
        let env: &'static str = Box::leak(format!("{dye}_envelope").into_boxed_str());
        let shaft: &'static str = Box::leak(format!("{dye}_envelope_encased_shaft").into_boxed_str());
        out.push(BlockSpec::new(env, 0.8, "minecraft:white_wool", false, vec![]));
        out.push(BlockSpec::new(
            shaft,
            1.5,
            "minecraft:oak_planks",
            true,
            vec![PropDef::Enum("axis", AXIS_3, "y")],
        ));
    }
    out
}

fn simulated_specs() -> Vec<BlockSpec<'static>> {
    let mut out = vec![
        BlockSpec::new(
            "physics_assembler",
            3.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("face", FACES_3, "floor"),
                PropDef::Enum("facing", FACING_4, "north"),
            ],
        ),
        BlockSpec::new(
            "steering_wheel",
            2.0,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("on_floor", "true"),
                PropDef::Bool("waterlogged", "false"),
            ],
        ),
        BlockSpec::new(
            "throttle_lever",
            1.5,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Enum("face", FACES_3, "floor"),
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("inverted", "false"),
            ],
        ),
        BlockSpec::new(
            "altitude_sensor",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("dial", &["linear", "radial"], "linear"),
                PropDef::Enum("face", FACES_3, "floor"),
                PropDef::Enum("facing", FACING_4, "north"),
            ],
        ),
        BlockSpec::new(
            "velocity_sensor",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Enum("powered", &["0", "1", "2"], "0"),
            ],
        ),
        BlockSpec::new(
            "swivel_bearing",
            2.5,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Bool("assembled", "false"),
                PropDef::Enum("facing", FACING_6, "up"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "swivel_bearing_link_block",
            2.0,
            "minecraft:stone",
            false,
            vec![PropDef::Enum("facing", FACING_6, "up")],
        ),
        BlockSpec::new(
            "docking_connector",
            2.5,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Bool("extended", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new("paired_docking_connector", 2.5, "minecraft:iron_block", false, vec![]),
        BlockSpec::new(
            "analog_transmission",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("axis", AXIS_3, "z"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "gimbal_sensor",
            2.0,
            "minecraft:copper_block",
            false,
            vec![PropDef::Enum("axis", AXIS_XZ, "z")],
        ),
        BlockSpec::new(
            "directional_linked_receiver",
            1.5,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "modulating_linked_receiver",
            1.5,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "redstone_magnet",
            2.5,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "torsion_spring",
            2.0,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "spring",
            2.0,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Enum("size", &["small", "medium", "large"], "medium"),
            ],
        ),
        BlockSpec::new(
            "laser_pointer",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_6, "north"),
                PropDef::Bool("inverted", "false"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "laser_sensor",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("powered", "false"),
                PropDef::Enum("target", FACES_3, "wall"),
            ],
        ),
        BlockSpec::new(
            "optical_sensor",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("powered", "false"),
                PropDef::Enum("target", FACES_3, "wall"),
            ],
        ),
        BlockSpec::new(
            "redstone_inductor",
            1.5,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("inverted", "false"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "linked_typewriter",
            2.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("powered", "false"),
            ],
        ),
        BlockSpec::new(
            "merging_glue",
            1.0,
            "minecraft:slime_block",
            false,
            vec![PropDef::Enum("facing", FACING_6, "north")],
        ),
        BlockSpec::new(
            "navigation_table",
            2.0,
            "minecraft:oak_planks",
            false,
            vec![PropDef::Enum("facing", FACING_6, "north")],
        ),
        BlockSpec::new(
            "rope_connector",
            1.5,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ),
        BlockSpec::new(
            "rope_winch",
            2.0,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ),
        BlockSpec::new(
            "copper_handle",
            1.5,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ),
        BlockSpec::new(
            "iron_handle",
            2.0,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ),
    ];

    for dye in DYES {
        let engine: &'static str = Box::leak(format!("{dye}_portable_engine").into_boxed_str());
        let sail: &'static str = Box::leak(format!("{dye}_symmetric_sail").into_boxed_str());
        let handle: &'static str = Box::leak(format!("{dye}_handle").into_boxed_str());
        let plate: &'static str = Box::leak(format!("{dye}_nameplate").into_boxed_str());
        out.push(BlockSpec::new(
            engine,
            3.0,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Bool("lit", "false"),
            ],
        ));
        out.push(BlockSpec::new(
            sail,
            0.8,
            "minecraft:white_wool",
            false,
            vec![PropDef::Enum("axis", AXIS_3, "y")],
        ));
        out.push(BlockSpec::new(
            handle,
            1.5,
            "minecraft:oak_planks",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ));
        out.push(BlockSpec::new(
            plate,
            1.5,
            "minecraft:copper_block",
            false,
            vec![
                PropDef::Enum("facing", FACING_4, "north"),
                PropDef::Enum("position", NAMEPLATE_POS, "single"),
            ],
        ));
    }
    out
}

fn offroad_specs() -> Vec<BlockSpec<'static>> {
    vec![
        BlockSpec::new(
            "wheel_mount",
            3.0,
            "minecraft:iron_block",
            false,
            vec![PropDef::Enum("facing", FACING_4, "north")],
        ),
        BlockSpec::new(
            "borehead_bearing",
            3.0,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ),
        BlockSpec::new(
            "rockcutting_wheel",
            3.5,
            "minecraft:iron_block",
            false,
            vec![
                PropDef::Bool("axis_along_first", "false"),
                PropDef::Enum("facing", FACING_6, "north"),
            ],
        ),
    ]
}

fn extra_items(mod_id: &str) -> &'static [&'static str] {
    match mod_id {
        "offroad" => &["small_tire", "tire", "large_tire", "monstrous_tire"],
        "simulated" => &[
            "contraption_diagram",
            "creative_physics_staff",
            "engine_assembly",
            "gyroscopic_mechanism",
            "honey_glue",
            "incomplete_engine_assembly",
            "incomplete_gyroscopic_mechanism",
            "plunger_launcher",
            "rope_coupling",
        ],
        "aeronautics" => &[
            "aviators_goggles",
            "end_stone_powder",
            "levitite_blend_bucket",
            "music_disc_cloud_skipper",
        ],
        _ => &[],
    }
}

/// Ensures every key block, state property, and pure item declaration in
/// `dir` (`horizon-mods/<mod_id>`) is valid for `horizon-mod-package`.
pub fn fix(mod_id: &str, dir: &Path) -> Result<(), String> {
    let specs = match mod_id {
        "aeronautics" => aeronautics_specs(),
        "simulated" => simulated_specs(),
        "offroad" => offroad_specs(),
        _ => return Ok(()),
    };

    let block_dir = dir.join(format!("data/{mod_id}/horizon/block"));
    let item_dir = dir.join(format!("data/{mod_id}/horizon/item"));
    for spec in &specs {
        let block_file = block_dir.join(format!("{}.json", spec.name));
        write_json(&block_file, &build_block_decl(spec))?;
        // Remove any stale item file whose block declaration now supplies `item: true`.
        let colliding_item = item_dir.join(format!("{}.json", spec.name));
        if colliding_item.is_file() {
            let _ = fs::remove_file(&colliding_item);
        }
    }

    for item in extra_items(mod_id) {
        let item_file = item_dir.join(format!("{item}.json"));
        if !item_file.is_file() {
            write_json(&item_file, &json!({"template": "minecraft:stick"}))?;
        }
    }

    let _ = fs::create_dir_all(dir);
    fs::write(dir.join(STAMP), VERSION).map_err(|e| format!("{}: {e}", dir.join(STAMP).display()))
}

fn porter_ingredients(recipe: &mut Value) {
    fn one(v: &Value) -> Value {
        if let Some(list) = v.as_array() {
            return Value::Array(list.iter().map(one).collect());
        }
        if let Some(item) = v["item"].as_str() {
            return json!(item);
        }
        if let Some(tag) = v["tag"].as_str() {
            return json!(format!("#{tag}"));
        }
        v.clone()
    }
    for field in ["ingredient", "base", "addition", "template"] {
        if !recipe[field].is_null() {
            recipe[field] = one(&recipe[field]);
        }
    }
    if let Some(list) = recipe["ingredients"].as_array_mut() {
        for v in list {
            *v = one(v);
        }
    }
    if let Some(key) = recipe["key"].as_object_mut() {
        for v in key.values_mut() {
            *v = one(v);
        }
    }
}

fn has_false_neoforge_condition(recipe: &Value) -> bool {
    recipe
        .get("neoforge:conditions")
        .and_then(Value::as_array)
        .is_some_and(|arr| {
            arr.iter()
                .any(|c| c.get("type").and_then(Value::as_str) == Some("neoforge:false"))
        })
}

fn normalize_recipe(mut json: Value) -> Option<Value> {
    if has_false_neoforge_condition(&json) {
        return None;
    }
    let mc_recipe = json["type"]
        .as_str()
        .is_some_and(|t| !t.contains(':') || t.starts_with("minecraft:"));
    if !mc_recipe {
        return None;
    }
    if horizon_port_pack::carry::result_before_1_20_5(&json) {
        return None;
    }
    horizon_port_pack::carry::renamed_ids(&mut json);
    if let Some(obj) = json.as_object_mut() {
        obj.remove("neoforge:conditions");
    }
    horizon_port_pack::carry::upgrade_recipe(&mut json);
    porter_ingredients(&mut json);
    Some(json)
}

/// Restores & upgrades crafting recipes for `mod_id` from its inner `jar` into `dir`.
pub fn restore_recipes(mod_id: &str, dir: &Path, jar: &Path) -> Result<(), String> {
    if !is_aeronautics_mod(mod_id) {
        return Ok(());
    }
    let file = fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = ZipArchive::new(file).map_err(|e| e.to_string())?;
    restore_recipes_from_zip(mod_id, dir, &mut zip)
}

fn restore_recipes_from_zip<R: Read + std::io::Seek>(
    mod_id: &str,
    dir: &Path,
    zip: &mut ZipArchive<R>,
) -> Result<(), String> {
    let prefix = format!("data/{mod_id}/recipe/");
    let mut names = Vec::new();
    for res in zip.file_names() {
        let name = res.map_err(|e| e.to_string())?;
        if name.starts_with(&prefix) && name.ends_with(".json") {
            names.push(name.into_owned());
        }
    }

    for name in names {
        let out = dir.join(&name);
        let mut raw = Vec::new();
        let Ok(mut entry) = zip.by_name(&name) else { continue };
        if entry.read_to_end(&mut raw).is_err() {
            continue;
        }
        let Ok(parsed) = serde_json::from_slice::<Value>(&raw) else { continue };
        match normalize_recipe(parsed) {
            Some(upgraded) => {
                if read_json(&out).as_ref() != Some(&upgraded) {
                    write_json(&out, &upgraded)?;
                }
            }
            None if out.is_file() => {
                let _ = fs::remove_file(&out);
            }
            None => {}
        }
    }

    // Also clean any already-exported vanilla-type recipes under dir/data/<mod_id>/recipe.
    let recipe_dir = dir.join(&prefix);
    if recipe_dir.is_dir() {
        clean_existing_recipes(&recipe_dir)?;
    }

    // Minimal NeoForge convention tag fallbacks used by Aeronautics/Simulated recipes.
    let tags: BTreeMap<&str, &str> = [
        ("nuggets/iron", "minecraft:iron_nugget"),
        ("ingots/iron", "minecraft:iron_ingot"),
        ("ingots/copper", "minecraft:copper_ingot"),
        ("rods/wooden", "minecraft:stick"),
        ("dusts/redstone", "minecraft:redstone"),
    ]
    .into_iter()
    .collect();
    for (tag, item) in tags {
        let file = dir.join(format!("data/c/tags/item/{tag}.json"));
        if !file.is_file() {
            write_json(&file, &json!({"values": [item]}))?;
        }
    }
    Ok(())
}

fn clean_existing_recipes(dir: &Path) -> Result<(), String> {
    let Ok(entries) = fs::read_dir(dir) else { return Ok(()) };
    for entry in entries.flatten() {
        let p = entry.path();
        if p.is_dir() {
            clean_existing_recipes(&p)?;
        } else if p.extension().is_some_and(|e| e == "json")
            && let Some(val) = read_json(&p)
        {
            let mc_recipe = val["type"]
                .as_str()
                .is_some_and(|t| !t.contains(':') || t.starts_with("minecraft:"));
            if mc_recipe && let Some(upgraded) = normalize_recipe(val.clone()) && upgraded != val {
                write_json(&p, &upgraded)?;
            }
        }
    }
    Ok(())
}

/// Refreshes any ported `simulated`, `aeronautics`, or `offroad` folders under
/// `<home>/horizon-mods/` that have not been stamped with [`VERSION`].
pub fn refresh_all(home: &Path, bundle_jar: Option<&Path>) {
    let mods_root = home.join(horizon_client::mod_list::DIR);
    let mut bundle_zip = bundle_jar
        .filter(|p| p.is_file())
        .and_then(|p| fs::File::open(p).ok())
        .and_then(|f| ZipArchive::new(f).ok());

    for inner in super::fetch::AERONAUTICS.inner {
        let Some(mod_id) = inner
            .take
            .first()
            .and_then(|s| s.strip_prefix("assets/"))
            .map(|s| s.trim_end_matches('/'))
        else {
            continue;
        };
        let dir = mods_root.join(mod_id);
        if !dir.join("horizon-mod.toml").is_file() {
            continue;
        }
        if fs::read_to_string(dir.join(STAMP)).is_ok_and(|v| v.trim() == VERSION) {
            continue;
        }
        match fix(mod_id, &dir) {
            Ok(()) => log::info!("Tachyon: refreshed {mod_id} block/item declarations"),
            Err(e) => log::warn!("Tachyon: {mod_id} fixup failed: {e}"),
        }
        if let Some(outer) = bundle_zip.as_mut()
            && let Ok(mut entry) = outer.by_name(inner.jar)
        {
            let mut inner_bytes = Vec::new();
            if entry.read_to_end(&mut inner_bytes).is_ok()
                && let Ok(mut inner_zip) = ZipArchive::new(std::io::Cursor::new(inner_bytes))
                && let Err(e) = restore_recipes_from_zip(mod_id, &dir, &mut inner_zip)
            {
                log::warn!("Tachyon: {mod_id} recipe restore failed: {e}");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tachyon-aero-port-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn fixes_all_three_mods_and_removes_colliding_items() {
        for (mod_id, sample_block, expect_prop, pure_item) in [
            ("aeronautics", "adjustable_burner", "variant", "aviators_goggles"),
            ("simulated", "physics_assembler", "face", "creative_physics_staff"),
            ("offroad", "wheel_mount", "facing", "large_tire"),
        ] {
            let dir = scratch(mod_id);
            write_json(
                &dir.join(format!("data/{mod_id}/horizon/item/{sample_block}.json")),
                &json!({"template": "minecraft:stick"}),
            )
            .unwrap();

            fix(mod_id, &dir).unwrap();

            let decl = read_json(&dir.join(format!("data/{mod_id}/horizon/block/{sample_block}.json")))
                .expect("block decl exists");
            assert_eq!(decl["template"], "minecraft:polished_andesite");
            assert!(!decl["state_properties"][expect_prop].is_null());
            assert!(
                !dir.join(format!("data/{mod_id}/horizon/item/{sample_block}.json"))
                    .exists(),
                "colliding block item must be removed"
            );
            assert!(
                dir.join(format!("data/{mod_id}/horizon/item/{pure_item}.json"))
                    .is_file(),
                "pure item {pure_item} must exist"
            );
            assert_eq!(fs::read_to_string(dir.join(STAMP)).unwrap(), VERSION);
            let _ = fs::remove_dir_all(&dir);
        }
    }

    #[test]
    fn upgrades_recipes_and_strips_neoforge_conditions() {
        let dir = scratch("recipes");
        let jar = dir.join("inner.jar");
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&jar).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            zip.start_file("data/aeronautics/recipe/adjustable_burner.json", opts)
                .unwrap();
            zip.write_all(
                br#"{
                    "type": "minecraft:crafting_shaped",
                    "neoforge:conditions": [{"type": "neoforge:true"}],
                    "key": {"C": {"item": "minecraft:chain"}, "P": {"tag": "c:plates/iron"}},
                    "pattern": ["C", "P"],
                    "result": {"id": "aeronautics:adjustable_burner", "count": 1}
                }"#,
            )
            .unwrap();
            zip.start_file("data/aeronautics/recipe/disabled.json", opts)
                .unwrap();
            zip.write_all(
                br#"{
                    "type": "minecraft:crafting_shapeless",
                    "neoforge:conditions": [{"type": "neoforge:false"}],
                    "ingredients": [{"item": "minecraft:stick"}],
                    "result": {"id": "aeronautics:wooden_propeller", "count": 1}
                }"#,
            )
            .unwrap();
            zip.finish().unwrap();
        }

        fix("aeronautics", &dir).unwrap();
        restore_recipes("aeronautics", &dir, &jar).unwrap();

        let burner = read_json(&dir.join("data/aeronautics/recipe/adjustable_burner.json")).unwrap();
        assert!(burner.get("neoforge:conditions").is_none());
        assert_eq!(burner["key"]["C"], "minecraft:iron_chain");
        assert_eq!(burner["key"]["P"], "#c:plates/iron");
        assert!(!dir.join("data/aeronautics/recipe/disabled.json").exists());
        let _ = fs::remove_dir_all(&dir);
    }
}
