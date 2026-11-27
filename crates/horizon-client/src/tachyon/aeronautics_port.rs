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

pub const VERSION: &str = "2";
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
const AUGER_SECTION: &[&str] = &["single", "front", "end"];

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

    out.push(BlockSpec::new(
        "auger_cog",
        2.0,
        "minecraft:polished_andesite",
        false,
        vec![
            PropDef::Enum("axis", AXIS_3, "y"),
            PropDef::Bool("encased", "false"),
            PropDef::Enum("section", AUGER_SECTION, "single"),
        ],
    ));
    out.push(BlockSpec::new(
        "auger_shaft",
        2.0,
        "minecraft:polished_andesite",
        false,
        vec![
            PropDef::Enum("axis", AXIS_3, "y"),
            PropDef::Bool("encased", "false"),
            PropDef::Enum("section", AUGER_SECTION, "single"),
            PropDef::Bool("down", "false"),
            PropDef::Bool("east", "false"),
            PropDef::Bool("north", "false"),
            PropDef::Bool("south", "false"),
            PropDef::Bool("up", "false"),
            PropDef::Bool("west", "false"),
        ],
    ));
    out.push(BlockSpec::new(
        "directional_gearshift",
        2.5,
        "minecraft:polished_andesite",
        false,
        vec![
            PropDef::Bool("axis_along_first", "false"),
            PropDef::Enum("facing", FACING_6, "north"),
            PropDef::Bool("left_powered", "false"),
            PropDef::Bool("right_powered", "false"),
        ],
    ));
    out.push(BlockSpec::new(
        "redstone_accumulator",
        1.5,
        "minecraft:stone",
        false,
        vec![
            PropDef::Enum("facing", FACING_4, "north"),
            PropDef::Bool("powered", "false"),
            PropDef::Bool("powering", "false"),
            PropDef::Bool("side_powered", "false"),
        ],
    ));
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

    fix_models(mod_id, dir)?;

    let _ = fs::create_dir_all(dir);
    fs::write(dir.join(STAMP), VERSION).map_err(|e| format!("{}: {e}", dir.join(STAMP).display()))
}

fn model_path(dir: &Path, mod_id: &str, rel: &str) -> std::path::PathBuf {
    dir.join(format!("assets/{mod_id}/models/{rel}.json"))
}

fn rewrite_face_textures(obj: &mut Map<String, Value>, prefix: &str) {
    if let Some(key) = obj
        .get("texture")
        .and_then(Value::as_str)
        .and_then(|tex| tex.strip_prefix('#'))
        .map(str::to_owned)
    {
        obj.insert("texture".into(), json!(format!("#{prefix}{key}")));
    }
}

fn box_element(from: [f64; 3], to: [f64; 3], tex: &str) -> Value {
    let f = |u: [f64; 4]| json!({"uv": u, "texture": tex});
    json!({
        "name": "tachyon:obj_part",
        "from": from,
        "to": to,
        "faces": {
            "north": f([0.0, 0.0, 16.0, 16.0]),
            "south": f([0.0, 0.0, 16.0, 16.0]),
            "east": f([0.0, 0.0, 16.0, 16.0]),
            "west": f([0.0, 0.0, 16.0, 16.0]),
            "up": f([0.0, 0.0, 16.0, 16.0]),
            "down": f([0.0, 0.0, 16.0, 16.0]),
        }
    })
}

fn rotated_box_element(from: [f64; 3], to: [f64; 3], angle: f64, axis: &str, tex: &str) -> Value {
    let mut el = box_element(from, to, tex);
    if let Some(m) = el.as_object_mut() {
        m.insert(
            "rotation".into(),
            json!({"angle": angle, "axis": axis, "origin": [8.0, 8.0, 8.0]}),
        );
    }
    el
}

fn tire_elements(radius_px: f64, half_w_px: f64, tread_tex: &str, hub_tex: &str) -> Vec<Value> {
    let y0 = 8.0 - half_w_px;
    let y1 = 8.0 + half_w_px;
    let inner = (radius_px * 0.42).max(3.5);
    let hub_w = (half_w_px + 0.6).min(7.8);
    vec![
        // Octagonal tread segments for round silhouette without neoforge:obj
        box_element([8.0 - radius_px, y0, 8.0 - radius_px * 0.45], [8.0 + radius_px, y1, 8.0 + radius_px * 0.45], tread_tex),
        box_element([8.0 - radius_px * 0.45, y0, 8.0 - radius_px], [8.0 + radius_px * 0.45, y1, 8.0 + radius_px], tread_tex),
        rotated_box_element([8.0 - radius_px, y0 + 0.05, 8.0 - radius_px * 0.45], [8.0 + radius_px, y1 - 0.05, 8.0 + radius_px * 0.45], 45.0, "y", tread_tex),
        rotated_box_element([8.0 - radius_px * 0.45, y0 + 0.05, 8.0 - radius_px], [8.0 + radius_px * 0.45, y1 - 0.05, 8.0 + radius_px], 45.0, "y", tread_tex),
        // Central rim + axle hub
        box_element([8.0 - inner, 8.0 - hub_w, 8.0 - inner], [8.0 + inner, 8.0 + hub_w, 8.0 + inner], hub_tex),
    ]
}

fn write_obj_replacement(dir: &Path, mod_id: &str, rel: &str, elements: Vec<Value>, default_tex: &[(&str, &str)]) -> Result<(), String> {
    let path = model_path(dir, mod_id, rel);
    if !path.is_file() {
        return Ok(());
    }
    let Some(mut doc) = read_json(&path) else {
        return Ok(());
    };
    let Some(obj) = doc.as_object_mut() else {
        return Ok(());
    };
    let is_obj = obj.get("loader").and_then(Value::as_str) == Some("neoforge:obj")
        || obj.get("elements").and_then(Value::as_array).is_some_and(Vec::is_empty);
    if !is_obj && obj.contains_key("elements") {
        return Ok(());
    }
    obj.remove("loader");
    obj.remove("model");
    obj.remove("flip_v");
    obj.insert("parent".into(), json!("minecraft:block/block"));
    let tex_map = obj
        .entry("textures")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(tm) = tex_map.as_object_mut() {
        for (k, v) in default_tex {
            tm.entry((*k).to_owned()).or_insert_with(|| json!(v));
        }
    }
    obj.insert("elements".into(), Value::Array(elements));
    write_json(&path, &doc)
}

fn merge_partials(dir: &Path, mod_id: &str, target_rel: &str, partials: &[&str]) -> Result<(), String> {
    let target_path = model_path(dir, mod_id, target_rel);
    if !target_path.is_file() {
        return Ok(());
    }
    let Some(mut target) = read_json(&target_path) else {
        return Ok(());
    };
    let Some(t_obj) = target.as_object_mut() else {
        return Ok(());
    };
    let already_merged = t_obj
        .get("elements")
        .and_then(Value::as_array)
        .is_some_and(|els| {
            els.iter()
                .any(|el| el.get("name").and_then(Value::as_str).is_some_and(|n| n.starts_with("tachyon:")))
        });
    if already_merged {
        return Ok(());
    }

    let mut extra_elements: Vec<Value> = Vec::new();
    let mut extra_textures: Vec<(String, Value)> = Vec::new();

    for (idx, partial_rel) in partials.iter().enumerate() {
        let partial_path = model_path(dir, mod_id, partial_rel);
        if !partial_path.is_file() {
            continue;
        }
        let Some(partial) = read_json(&partial_path) else {
            continue;
        };
        let prefix = format!("p{idx}_");
        if let Some(texs) = partial.get("textures").and_then(Value::as_object) {
            for (k, v) in texs {
                if k == "particle" {
                    continue;
                }
                extra_textures.push((format!("{prefix}{k}"), v.clone()));
            }
        }
        if let Some(els) = partial.get("elements").and_then(Value::as_array) {
            for el in els {
                let mut cloned = el.clone();
                if let Some(el_obj) = cloned.as_object_mut() {
                    el_obj.insert("name".into(), json!(format!("tachyon:{partial_rel}")));
                    if let Some(faces) = el_obj.get_mut("faces").and_then(Value::as_object_mut) {
                        for face_val in faces.values_mut() {
                            if let Some(f_obj) = face_val.as_object_mut() {
                                rewrite_face_textures(f_obj, &prefix);
                            }
                        }
                    }
                }
                extra_elements.push(cloned);
            }
        }
    }

    if extra_elements.is_empty() {
        return Ok(());
    }

    let tex_obj = t_obj
        .entry("textures")
        .or_insert_with(|| Value::Object(Map::new()));
    if let Some(tm) = tex_obj.as_object_mut() {
        for (k, v) in extra_textures {
            tm.entry(k).or_insert(v);
        }
    }
    let els_val = t_obj
        .entry("elements")
        .or_insert_with(|| Value::Array(Vec::new()));
    if let Some(arr) = els_val.as_array_mut() {
        arr.extend(extra_elements);
    }
    write_json(&target_path, &target)
}

fn fix_models(mod_id: &str, dir: &Path) -> Result<(), String> {
    match mod_id {
        "aeronautics" => {
            let blend_path = model_path(dir, "aeronautics", "block/levitite_blend");
            if blend_path.is_file()
                && let Some(mut blend) = read_json(&blend_path)
            {
                if blend.get("elements").is_none() && blend.get("parent").is_none()
                    && let Some(obj) = blend.as_object_mut()
                {
                    obj.insert("parent".into(), json!("minecraft:block/cube_all"));
                    obj.entry("textures")
                        .or_insert_with(|| json!({}))
                        .as_object_mut()
                        .map(|t| {
                            t.entry("all".to_owned())
                                .or_insert_with(|| json!("aeronautics:fluid/levitite_blend_still"))
                        });
                    write_json(&blend_path, &blend)?;
                }
            }
            let merges: &[(&str, &[&str])] = &[
                ("block/adjustable_burner/block_fire", &["block/adjustable_burner/redstone_indicator"]),
                ("block/andesite_propeller/block", &["block/andesite_propeller/propeller"]),
                ("block/wooden_propeller/block", &["block/wooden_propeller/propeller"]),
                ("block/smart_propeller/block", &["block/smart_propeller/hinge", "block/smart_propeller/propeller"]),
                ("block/propeller_bearing/block", &["block/propeller_bearing/bearing_plate"]),
                (
                    "block/gyroscopic_propeller_bearing/block",
                    &[
                        "block/gyroscopic_propeller_bearing/metal_bearing_plate",
                        "block/gyroscopic_propeller_bearing/piston_head",
                        "block/gyroscopic_propeller_bearing/piston_pole",
                    ],
                ),
                (
                    "block/mounted_potato_cannon/block_horizontal",
                    &[
                        "block/mounted_potato_cannon/partials/barrel",
                        "block/mounted_potato_cannon/partials/bellow",
                        "block/mounted_potato_cannon/partials/cog",
                    ],
                ),
                (
                    "block/mounted_potato_cannon/block_horizontal_powered",
                    &[
                        "block/mounted_potato_cannon/partials/barrel",
                        "block/mounted_potato_cannon/partials/bellow",
                        "block/mounted_potato_cannon/partials/cog",
                    ],
                ),
                (
                    "block/mounted_potato_cannon/block_vertical",
                    &[
                        "block/mounted_potato_cannon/partials/barrel",
                        "block/mounted_potato_cannon/partials/bellow",
                        "block/mounted_potato_cannon/partials/cog",
                    ],
                ),
                (
                    "block/mounted_potato_cannon/block_vertical_powered",
                    &[
                        "block/mounted_potato_cannon/partials/barrel",
                        "block/mounted_potato_cannon/partials/bellow",
                        "block/mounted_potato_cannon/partials/cog",
                    ],
                ),
                ("block/steam_vent/block_iron", &["block/steam_vent/redstone", "block/steam_vent/steam_base"]),
                ("block/steam_vent/block_gold", &["block/steam_vent/redstone", "block/steam_vent/steam_base"]),
            ];
            for (target, parts) in merges {
                merge_partials(dir, "aeronautics", target, parts)?;
            }
        }
        "offroad" => {
            let saw_elems = || {
                vec![
                    box_element([0.0, 2.0, 5.0], [16.0, 14.0, 11.0], "#wheel"),
                    box_element([2.0, 0.0, 5.0], [14.0, 16.0, 11.0], "#wheel"),
                    rotated_box_element([0.0, 2.0, 5.0], [16.0, 14.0, 11.0], 45.0, "z", "#wheel"),
                    box_element([5.0, 5.0, 3.0], [11.0, 11.0, 13.0], "#base"),
                ]
            };
            write_obj_replacement(
                dir,
                "offroad",
                "block/rockcutting_wheel/wheel",
                saw_elems(),
                &[
                    ("wheel", "offroad:block/rockcutting_wheel"),
                    ("base", "offroad:block/rockcutting_base"),
                    ("particle", "offroad:block/rockcutting_wheel"),
                ],
            )?;
            write_obj_replacement(
                dir,
                "offroad",
                "block/rockcutting_wheel/item",
                saw_elems(),
                &[
                    ("wheel", "offroad:block/rockcutting_wheel"),
                    ("base", "offroad:block/rockcutting_base"),
                    ("particle", "offroad:block/rockcutting_wheel"),
                ],
            )?;
            for (name, radius, half_w) in [
                ("small_tire", 11.5_f64, 5.0_f64),
                ("tire", 15.5, 7.0),
                ("large_tire", 19.5, 7.0),
                ("monstrous_tire", 30.0, 7.0),
            ] {
                let els = tire_elements(radius, half_w, "#tire_0", "#axis");
                let defaults = [
                    ("tire_0", "offroad:block/tire/tire_0"),
                    ("axis", "create:block/axis"),
                    ("particle", "offroad:block/custom_break_particles/wheel"),
                ];
                write_obj_replacement(dir, "offroad", &format!("item/{name}/{name}"), els.clone(), &defaults)?;
                write_obj_replacement(dir, "offroad", &format!("item/{name}/item"), els, &defaults)?;
            }
            merge_partials(dir, "offroad", "block/rockcutting_wheel/block_horizontal", &["block/rockcutting_wheel/wheel"])?;
            merge_partials(dir, "offroad", "block/rockcutting_wheel/block_vertical", &["block/rockcutting_wheel/wheel"])?;
            merge_partials(
                dir,
                "offroad",
                "block/wheel_mount/block",
                &[
                    "block/wheel_mount/mount",
                    "block/wheel_mount/tele_outer",
                    "block/wheel_mount/tele_inner",
                    "block/wheel_mount/spring_lower",
                    "block/wheel_mount/spring_middle",
                    "block/wheel_mount/spring_upper",
                    "block/wheel_mount/diode_left",
                    "block/wheel_mount/diode_right",
                ],
            )?;
        }
        "simulated" => {
            let sw_elems = vec![
                // Circular steering rim + spokes + center hub
                box_element([2.0, 7.0, 1.0], [14.0, 9.0, 3.0], "#steering_wheel"),
                box_element([1.0, 7.0, 12.0], [15.0, 9.0, 14.0], "#steering_wheel"),
                box_element([1.0, 7.0, 2.0], [3.0, 9.0, 13.0], "#steering_wheel"),
                box_element([13.0, 7.0, 2.0], [15.0, 9.0, 13.0], "#steering_wheel"),
                box_element([5.5, 6.5, 5.5], [10.5, 9.5, 10.5], "#steering_wheel"),
                box_element([7.0, 7.2, 3.0], [9.0, 8.8, 12.0], "#steering_wheel"),
                box_element([3.0, 7.2, 7.0], [13.0, 8.8, 9.0], "#steering_wheel"),
            ];
            write_obj_replacement(
                dir,
                "simulated",
                "block/steering_wheel/wheel",
                sw_elems.clone(),
                &[
                    ("steering_wheel", "simulated:block/steering_wheel"),
                    ("particle", "create:block/andesite_casing"),
                ],
            )?;
            let mut sw_item = sw_elems;
            sw_item.push(box_element([4.0, 0.0, 4.0], [12.0, 7.0, 12.0], "#spruce_planks"));
            write_obj_replacement(
                dir,
                "simulated",
                "block/steering_wheel/item",
                sw_item,
                &[
                    ("steering_wheel", "simulated:block/steering_wheel"),
                    ("spruce_planks", "minecraft:block/spruce_planks"),
                    ("particle", "create:block/andesite_casing"),
                ],
            )?;

            let mecanum_elems = tire_elements(16.0, 7.5, "#0", "#axis");
            for rel in [
                "block/mecanum_wheel/block",
                "block/mecanum_wheel/block_flipped",
                "block/mecanum_wheel/item",
            ] {
                write_obj_replacement(
                    dir,
                    "simulated",
                    rel,
                    mecanum_elems.clone(),
                    &[
                        ("0", "simulated:block/mecanum_wheel"),
                        ("axis", "create:block/axis"),
                        ("particle", "create:block/dark_metal_block"),
                    ],
                )?;
            }

            let plunger_elems = tire_elements(18.0, 7.0, "#plunger_wheel", "#axis");
            for rel in ["block/plunger_wheel/block", "block/plunger_wheel/item"] {
                write_obj_replacement(
                    dir,
                    "simulated",
                    rel,
                    plunger_elems.clone(),
                    &[
                        ("plunger_wheel", "simulated:block/plunger_wheel"),
                        ("axis", "create:block/axis"),
                        ("particle", "simulated:block/plunger_wheel"),
                    ],
                )?;
            }

            let merges: &[(&str, &[&str])] = &[
                ("block/steering_wheel/block", &["block/steering_wheel/wheel"]),
                ("block/steering_wheel/block_up", &["block/steering_wheel/wheel"]),
                ("block/physics_assembler/block", &["block/physics_assembler/lever"]),
                (
                    "block/throttle_lever/block",
                    &[
                        "block/throttle_lever/handle",
                        "block/throttle_lever/button",
                        "block/throttle_lever/diode",
                    ],
                ),
                (
                    "block/portable_engine/block",
                    &[
                        "block/portable_engine/mouth",
                        "block/portable_engine/hatch_bottom",
                        "block/portable_engine/hatch_top",
                        "block/portable_engine/exhaust_pipe_left",
                        "block/portable_engine/exhaust_pipe_right",
                        "block/portable_engine/exhaust_outlet_left",
                        "block/portable_engine/exhaust_outlet_right",
                    ],
                ),
                (
                    "block/directional_gearshift/horizontal/middle",
                    &[
                        "block/directional_gearshift/barrel",
                        "block/directional_gearshift/barrel_shaft",
                    ],
                ),
                (
                    "block/directional_gearshift/vertical/middle",
                    &[
                        "block/directional_gearshift/barrel",
                        "block/directional_gearshift/barrel_shaft",
                    ],
                ),
                ("block/redstone_accumulator/block_front", &["block/redstone_accumulator/diode"]),
                (
                    "block/docking_connector/block",
                    &[
                        "block/docking_connector/foot",
                        "block/docking_connector/main_piston_1",
                        "block/docking_connector/main_piston_2",
                        "block/docking_connector/side_piston_1",
                        "block/docking_connector/side_piston_2",
                    ],
                ),
                ("block/swivel_bearing/block", &["block/swivel_bearing/ironcog"]),
                (
                    "block/altitude_sensor/block",
                    &[
                        "block/altitude_sensor/indicator",
                        "block/altitude_sensor/linear_case",
                        "block/altitude_sensor/linear_hand",
                    ],
                ),
                (
                    "block/gimbal_sensor/block",
                    &[
                        "block/gimbal_sensor/gimbal",
                        "block/gimbal_sensor/compass",
                        "block/gimbal_sensor/needle",
                        "block/gimbal_sensor/redstone_indicator",
                    ],
                ),
                ("block/tilt_sensor/block", &["block/tilt_sensor/pendulum"]),
                ("block/velocity_sensor/block_horizontal", &["block/velocity_sensor/fan", "block/velocity_sensor/diode"]),
                ("block/velocity_sensor/block_vertical", &["block/velocity_sensor/fan", "block/velocity_sensor/diode"]),
                ("block/rope_winch/block_horizontal", &["block/rope_winch/rope_coil", "block/rope_winch/shaft"]),
                ("block/rope_winch/block_vertical", &["block/rope_winch/rope_coil", "block/rope_winch/shaft"]),
                ("block/analog_transmission/block", &["block/analog_transmission/gear"]),
                ("block/laser_pointer/block", &["block/laser_pointer/lens_off"]),
                ("block/navigation_table/block", &["block/navigation_table/nav_table_pointer", "block/navigation_table/redstone_indicator"]),
                ("block/redstone_inductor/block", &["block/redstone_inductor/redstone_indicator"]),
                ("block/torsion_spring/block", &["block/torsion_spring/spring"]),
            ];
            for (target, parts) in merges {
                merge_partials(dir, "simulated", target, parts)?;
            }
        }
        _ => {}
    }
    Ok(())
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

    #[test]
    fn simulated_includes_auger_directional_and_accumulator() {
        let specs = simulated_specs();
        for name in ["auger_cog", "auger_shaft", "directional_gearshift", "redstone_accumulator"] {
            assert!(specs.iter().any(|s| s.name == name), "missing {name}");
        }
    }

    #[test]
    fn fix_models_merges_partials_and_converts_neoforge_obj() {
        let dir = scratch("models");
        write_json(
            &model_path(&dir, "offroad", "block/wheel_mount/block"),
            &json!({"textures": {"0": "offroad:base"}, "elements": [{"from": [0,0,0], "to": [16,4,16], "faces": {"up": {"texture": "#0"}}}]}),
        )
        .unwrap();
        write_json(
            &model_path(&dir, "offroad", "block/wheel_mount/mount"),
            &json!({"textures": {"0": "offroad:mount"}, "elements": [{"from": [4,4,4], "to": [12,12,12], "faces": {"north": {"texture": "#0"}}}]}),
        )
        .unwrap();
        write_json(
            &model_path(&dir, "offroad", "item/tire/tire"),
            &json!({"loader": "neoforge:obj", "model": "offroad:models/item/tire/block.obj", "textures": {}}),
        )
        .unwrap();

        fix("offroad", &dir).unwrap();

        let mount = read_json(&model_path(&dir, "offroad", "block/wheel_mount/block")).unwrap();
        assert_eq!(mount["elements"].as_array().unwrap().len(), 2);
        assert_eq!(mount["textures"]["p0_0"], "offroad:mount");

        // Second fix() call must remain idempotent
        fix("offroad", &dir).unwrap();
        let mount_again = read_json(&model_path(&dir, "offroad", "block/wheel_mount/block")).unwrap();
        assert_eq!(mount_again["elements"].as_array().unwrap().len(), 2);

        let tire = read_json(&model_path(&dir, "offroad", "item/tire/tire")).unwrap();
        assert!(tire.get("loader").is_none());
        assert!(!tire["elements"].as_array().unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
