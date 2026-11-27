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

pub const VERSION: &str = "3";
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

struct ObjRestoreSpec {
    rel_json: &'static str,
    model_loc: &'static str,
    parent: &'static str,
    textures: &'static [(&'static str, &'static str)],
    display_json: &'static str,
}

fn obj_specs_for(mod_id: &str) -> &'static [ObjRestoreSpec] {
    match mod_id {
        "offroad" => &[
            ObjRestoreSpec {
                rel_json: "block/rockcutting_wheel/wheel",
                model_loc: "offroad:models/block/rockcutting_wheel/rockcutting_wheel.obj",
                parent: "block/block",
                textures: &[("wheel", "offroad:block/rockcutting_wheel")],
                display_json: "{}",
            },
            ObjRestoreSpec {
                rel_json: "block/rockcutting_wheel/item",
                model_loc: "offroad:models/block/rockcutting_wheel/item.obj",
                parent: "block/block",
                textures: &[("wheel", "offroad:block/rockcutting_wheel"), ("base", "offroad:block/rockcutting_base")],
                display_json: r#"{"thirdperson_righthand":{"rotation":[75,45,0],"translation":[3.5,2.5,0],"scale":[0.35,0.35,0.35]},"thirdperson_lefthand":{"rotation":[75,45,0],"translation":[0,-1.5,0],"scale":[0.35,0.35,0.35]},"firstperson_righthand":{"rotation":[0,45,0],"translation":[4,0,0],"scale":[0.3,0.3,0.3]},"firstperson_lefthand":{"rotation":[0,45,0],"translation":[0,0,4],"scale":[0.3,0.3,0.3]},"ground":{"translation":[2,3,2],"scale":[0.25,0.25,0.25]},"gui":{"rotation":[30,-135,0],"translation":[-5.25,-2,0],"scale":[0.45,0.45,0.45]},"head":{"translation":[6,-3.75,4.25],"scale":[0.75,0.75,0.75]},"fixed":{"translation":[4,-2,4],"scale":[0.5,0.5,0.5]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/small_tire/small_tire",
                model_loc: "offroad:models/item/small_tire/block.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"gui":{"rotation":[30,135,90],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/small_tire/item",
                model_loc: "offroad:models/item/small_tire/item.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"thirdperson_righthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.375,0.375,0.375]},"thirdperson_lefthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.375,0.375,0.375]},"firstperson_righthand":{"rotation":[0,45,0],"scale":[0.4,0.4,0.4]},"firstperson_lefthand":{"rotation":[0,225,0],"scale":[0.4,0.4,0.4]},"ground":{"translation":[0,3,0],"scale":[0.25,0.25,0.25]},"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.5,0.5,0.5]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.6,0.6,0.6]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/tire/tire",
                model_loc: "offroad:models/item/tire/block.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"gui":{"rotation":[30,135,90],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/tire/item",
                model_loc: "offroad:models/item/tire/item.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"thirdperson_righthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.375,0.375,0.375]},"thirdperson_lefthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.375,0.375,0.375]},"firstperson_righthand":{"rotation":[0,45,0],"scale":[0.4,0.4,0.4]},"firstperson_lefthand":{"rotation":[0,225,0],"scale":[0.4,0.4,0.4]},"ground":{"translation":[0,3,0],"scale":[0.25,0.25,0.25]},"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.6,0.6,0.6]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/large_tire/large_tire",
                model_loc: "offroad:models/item/large_tire/block.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"gui":{"rotation":[30,135,90],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/large_tire/item",
                model_loc: "offroad:models/item/large_tire/item.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"thirdperson_righthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.35,0.35,0.35]},"thirdperson_lefthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.325,0.325,0.325]},"firstperson_righthand":{"rotation":[0,45,0],"scale":[0.4,0.4,0.4]},"firstperson_lefthand":{"rotation":[0,225,0],"scale":[0.4,0.4,0.4]},"ground":{"translation":[0,3,0],"scale":[0.25,0.25,0.25]},"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.6,0.6,0.6]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/monstrous_tire/monstrous_tire",
                model_loc: "offroad:models/item/monstrous_tire/block.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("tire_1", "offroad:block/tire/tire_1"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"gui":{"rotation":[30,135,90],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "item/monstrous_tire/item",
                model_loc: "offroad:models/item/monstrous_tire/item.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("tire_0", "offroad:block/tire/tire_0"), ("tire_1", "offroad:block/tire/tire_1"), ("particle", "offroad:block/custom_break_particles/wheel")],
                display_json: r#"{"thirdperson_righthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.2,0.2,0.2]},"thirdperson_lefthand":{"rotation":[75,45,0],"translation":[0,2.5,0],"scale":[0.2,0.2,0.2]},"firstperson_righthand":{"rotation":[0,45,0],"scale":[0.4,0.4,0.4]},"firstperson_lefthand":{"rotation":[0,225,0],"scale":[0.4,0.4,0.4]},"ground":{"translation":[0,3,0],"scale":[0.25,0.25,0.25]},"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.3,0.3,0.3]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.6,0.6,0.6]}}"#,
            },
        ],
        "simulated" => &[
            ObjRestoreSpec {
                rel_json: "block/mecanum_wheel/block",
                model_loc: "simulated:models/block/mecanum_wheel/block.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("mecanum_wheel", "simulated:block/mecanum_wheel"), ("particle", "create:block/dark_metal_block")],
                display_json: "{}",
            },
            ObjRestoreSpec {
                rel_json: "block/mecanum_wheel/block_flipped",
                model_loc: "simulated:models/block/mecanum_wheel/block_flipped.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("mecanum_wheel", "simulated:block/mecanum_wheel"), ("particle", "create:block/dark_metal_block")],
                display_json: "{}",
            },
            ObjRestoreSpec {
                rel_json: "block/mecanum_wheel/item",
                model_loc: "simulated:models/block/mecanum_wheel/item.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("mecanum_wheel", "simulated:block/mecanum_wheel"), ("particle", "create:block/dark_metal_block")],
                display_json: r#"{"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "block/plunger_wheel/block",
                model_loc: "simulated:models/block/plunger_wheel/block.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("plunger_wheel", "simulated:block/plunger_wheel"), ("particle", "simulated:block/plunger_wheel")],
                display_json: r#"{"gui":{"rotation":[30,135,90],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "block/plunger_wheel/item",
                model_loc: "simulated:models/block/plunger_wheel/item.obj",
                parent: "block/block",
                textures: &[("axis", "create:block/axis"), ("axis_top", "create:block/axis_top"), ("plunger_wheel", "simulated:block/plunger_wheel"), ("particle", "simulated:block/plunger_wheel")],
                display_json: r#"{"gui":{"rotation":[30,225,0],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "block/steering_wheel/wheel",
                model_loc: "simulated:models/block/steering_wheel/wheel.obj",
                parent: "block/block",
                textures: &[("steering_wheel", "simulated:block/steering_wheel"), ("planks", "block/oak_planks"), ("particle", "create:block/andesite_casing")],
                display_json: r#"{"gui":{"rotation":[30,135,90],"translation":[0,0,0],"scale":[0.4,0.4,0.4]},"fixed":{"rotation":[90,0,0],"translation":[0,0,0],"scale":[0.45,0.45,0.45]}}"#,
            },
            ObjRestoreSpec {
                rel_json: "block/steering_wheel/item",
                model_loc: "simulated:models/block/steering_wheel/item.obj",
                parent: "block/block",
                textures: &[("steering_wheel", "simulated:block/steering_wheel"), ("spruce_planks", "block/spruce_planks"), ("particle", "create:block/andesite_casing")],
                display_json: r#"{"thirdperson_righthand":{"rotation":[75,45,0],"translation":[3.5,2.5,0],"scale":[0.375,0.375,0.375]},"thirdperson_lefthand":{"rotation":[75,45,0],"translation":[0,-1.5,0],"scale":[0.375,0.375,0.375]},"firstperson_righthand":{"rotation":[0,45,0],"translation":[4,0,0],"scale":[0.4,0.4,0.4]},"firstperson_lefthand":{"rotation":[0,45,0],"translation":[0,0,4],"scale":[0.4,0.4,0.4]},"ground":{"translation":[2,3,2],"scale":[0.25,0.25,0.25]},"gui":{"rotation":[30,-135,0],"translation":[-8,-1,0],"scale":[0.625,0.625,0.625]},"head":{"translation":[6,-3.75,4.25],"scale":[0.75,0.75,0.75]},"fixed":{"translation":[4,-2,4],"scale":[0.5,0.5,0.5]}}"#,
            },
        ],
        _ => &[],
    }
}

/// Restores native `neoforge:obj` model files (which `horizon-assets` bakes directly with `.obj`/`.mtl`)
/// if a previous port version replaced them or if an offroad tire points at `block/air`.
fn restore_obj_models(mod_id: &str, dir: &Path) -> Result<(), String> {
    for spec in obj_specs_for(mod_id) {
        let json_path = model_path(dir, mod_id, spec.rel_json);
        let obj_rel = spec.model_loc.split(':').nth(1).unwrap_or("");
        let obj_disk = dir.join(format!("assets/{mod_id}/{obj_rel}"));
        if !json_path.is_file() || !obj_disk.is_file() {
            continue;
        }
        let Some(existing) = read_json(&json_path) else {
            continue;
        };
        let needs_restore = existing.get("loader").and_then(Value::as_str) != Some("neoforge:obj")
            || existing.get("parent").and_then(Value::as_str) == Some("block/air")
            || existing.get("elements").is_some();
        if !needs_restore {
            continue;
        }
        let mut tex = Map::new();
        for (k, v) in spec.textures {
            tex.insert((*k).to_owned(), json!(v));
        }
        let mut doc = Map::new();
        doc.insert("parent".into(), json!(spec.parent));
        doc.insert("ambientocclusion".into(), json!(false));
        doc.insert("textures".into(), Value::Object(tex));
        doc.insert("loader".into(), json!("neoforge:obj"));
        doc.insert("flip_v".into(), json!(true));
        doc.insert("model".into(), json!(spec.model_loc));
        if spec.display_json != "{}"
            && let Ok(disp) = serde_json::from_str::<Value>(spec.display_json)
        {
            doc.insert("display".into(), disp);
        }
        write_json(&json_path, &Value::Object(doc))?;
    }
    Ok(())
}

fn is_tachyon_tex_key(k: &str) -> bool {
    let Some(rest) = k.strip_prefix('p') else {
        return false;
    };
    let Some((digits, _)) = rest.split_once('_') else {
        return false;
    };
    !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
}

fn strip_tachyon_merged(t_obj: &mut Map<String, Value>) {
    if let Some(els) = t_obj.get_mut("elements").and_then(Value::as_array_mut) {
        els.retain(|el| !el.get("name").and_then(Value::as_str).is_some_and(|n| n.starts_with("tachyon:")));
    }
    if let Some(tm) = t_obj.get_mut("textures").and_then(Value::as_object_mut) {
        let stale: Vec<String> = tm.keys().filter(|k| is_tachyon_tex_key(k)).cloned().collect();
        for k in stale {
            tm.remove(&k);
        }
    }
}

fn shift_vec3(v: &mut Value, offset: [f64; 3]) {
    if let Some(arr) = v.as_array_mut()
        && arr.len() == 3
    {
        for (idx, off) in offset.iter().enumerate() {
            if let Some(n) = arr[idx].as_f64() {
                arr[idx] = json!((n + off).clamp(-16.0, 32.0));
            }
        }
    }
}

fn merge_partials(dir: &Path, mod_id: &str, target_rel: &str, partials: &[(&str, [f64; 3])]) -> Result<(), String> {
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
    strip_tachyon_merged(t_obj);

    let mut extra_elements: Vec<Value> = Vec::new();
    let mut extra_textures: Vec<(String, Value)> = Vec::new();

    for (idx, (partial_rel, offset)) in partials.iter().enumerate() {
        let partial_path = model_path(dir, mod_id, partial_rel);
        if !partial_path.is_file() {
            continue;
        }
        let Some(partial) = read_json(&partial_path) else {
            continue;
        };
        if partial.get("loader").is_some() {
            continue;
        }
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
                    if *offset != [0.0; 3] {
                        if let Some(from) = el_obj.get_mut("from") {
                            shift_vec3(from, *offset);
                        }
                        if let Some(to) = el_obj.get_mut("to") {
                            shift_vec3(to, *offset);
                        }
                        if let Some(origin) = el_obj.get_mut("rotation").and_then(|r| r.get_mut("origin")) {
                            shift_vec3(origin, *offset);
                        }
                    }
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

    if !extra_elements.is_empty() {
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
    }
    write_json(&target_path, &target)
}

/// Ensures every cuboid element has `from[i] <= to[i]` and stays within `[-16, 32]`.
fn normalize_cuboids_in_doc(doc: &mut Value) -> bool {
    let Some(els) = doc.get_mut("elements").and_then(Value::as_array_mut) else {
        return false;
    };
    let mut changed = false;
    for el in els {
        let Some(el_obj) = el.as_object_mut() else {
            continue;
        };
        let (Some(fr), Some(to)) = (
            el_obj.get("from").and_then(Value::as_array).cloned(),
            el_obj.get("to").and_then(Value::as_array).cloned(),
        ) else {
            continue;
        };
        if fr.len() != 3 || to.len() != 3 {
            continue;
        }
        let mut new_fr = [0.0_f64; 3];
        let mut new_to = [0.0_f64; 3];
        let mut elem_changed = false;
        for i in 0..3 {
            let (Some(a), Some(b)) = (fr[i].as_f64(), to[i].as_f64()) else {
                break;
            };
            let ca = a.clamp(-16.0, 32.0);
            let cb = b.clamp(-16.0, 32.0);
            let (lo, hi) = if ca > cb { (cb, ca) } else { (ca, cb) };
            if lo != a || hi != b {
                elem_changed = true;
            }
            new_fr[i] = lo;
            new_to[i] = hi;
        }
        if elem_changed {
            el_obj.insert("from".into(), json!(new_fr));
            el_obj.insert("to".into(), json!(new_to));
            changed = true;
        }
    }
    changed
}

fn normalize_all_models_in_dir(dir: &Path) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            normalize_all_models_in_dir(&path)?;
        } else if path.extension().and_then(|e| e.to_str()) == Some("json")
            && let Some(mut doc) = read_json(&path)
            && normalize_cuboids_in_doc(&mut doc)
        {
            write_json(&path, &doc)?;
        }
    }
    Ok(())
}

fn fix_models(mod_id: &str, dir: &Path) -> Result<(), String> {
    restore_obj_models(mod_id, dir)?;

    match mod_id {
        "aeronautics" => {
            let blend_path = model_path(dir, "aeronautics", "block/levitite_blend");
            if blend_path.is_file()
                && let Some(mut blend) = read_json(&blend_path)
            {
                if blend.get("elements").is_none()
                    && blend.get("parent").is_none()
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
            let z = [0.0; 3];
            let merges: &[(&str, &[(&str, [f64; 3])])] = &[
                ("block/adjustable_burner/block_fire", &[("block/adjustable_burner/redstone_indicator", z)]),
                ("block/andesite_propeller/block", &[("block/andesite_propeller/propeller", z)]),
                ("block/wooden_propeller/block", &[("block/wooden_propeller/propeller", z)]),
                ("block/smart_propeller/block", &[("block/smart_propeller/hinge", z), ("block/smart_propeller/propeller", z)]),
                ("block/propeller_bearing/block", &[("block/propeller_bearing/bearing_plate", z)]),
                ("block/gyroscopic_propeller_bearing/block", &[("block/gyroscopic_propeller_bearing/metal_bearing_plate", z)]),
                ("block/mounted_potato_cannon/block_horizontal", &[("block/mounted_potato_cannon/partials/barrel", z), ("block/mounted_potato_cannon/partials/bellow", z), ("block/mounted_potato_cannon/partials/cog", z)]),
                ("block/mounted_potato_cannon/block_horizontal_powered", &[("block/mounted_potato_cannon/partials/barrel", z), ("block/mounted_potato_cannon/partials/bellow", z), ("block/mounted_potato_cannon/partials/cog", z)]),
                ("block/mounted_potato_cannon/block_vertical", &[("block/mounted_potato_cannon/partials/barrel", z), ("block/mounted_potato_cannon/partials/bellow", z), ("block/mounted_potato_cannon/partials/cog", z)]),
                ("block/mounted_potato_cannon/block_vertical_powered", &[("block/mounted_potato_cannon/partials/barrel", z), ("block/mounted_potato_cannon/partials/bellow", z), ("block/mounted_potato_cannon/partials/cog", z)]),
                ("block/steam_vent/block_iron", &[("block/steam_vent/redstone", z), ("block/steam_vent/steam_base", z)]),
                ("block/steam_vent/block_gold", &[("block/steam_vent/redstone", z), ("block/steam_vent/steam_base", z)]),
            ];
            for (target, parts) in merges {
                merge_partials(dir, "aeronautics", target, parts)?;
            }
        }
        "offroad" => {
            // Strip any stale v2 elements from rockcutting_wheel (its wheel is a native neoforge:obj).
            for rel in ["block/rockcutting_wheel/block_horizontal", "block/rockcutting_wheel/block_vertical"] {
                merge_partials(dir, "offroad", rel, &[])?;
            }
            let z = [0.0; 3];
            merge_partials(
                dir,
                "offroad",
                "block/wheel_mount/block",
                &[
                    ("block/wheel_mount/mount", z),
                    ("block/wheel_mount/tele_outer", z),
                    ("block/wheel_mount/tele_inner", z),
                    ("block/wheel_mount/diode_left", z),
                    ("block/wheel_mount/diode_right", z),
                ],
            )?;
        }
        "simulated" => {
            // Strip any stale v2 wheel elements from steering_wheel base post blocks.
            for rel in ["block/steering_wheel/block", "block/steering_wheel/block_up"] {
                merge_partials(dir, "simulated", rel, &[])?;
            }
            let z = [0.0; 3];
            let c8 = [8.0, 8.0, 8.0];
            let merges: &[(&str, &[(&str, [f64; 3])])] = &[
                ("block/physics_assembler/block", &[("block/physics_assembler/lever", z)]),
                ("block/throttle_lever/block", &[("block/throttle_lever/handle", z), ("block/throttle_lever/button", z), ("block/throttle_lever/diode", z)]),
                ("block/portable_engine/block", &[("block/portable_engine/mouth", z), ("block/portable_engine/hatch_bottom", z), ("block/portable_engine/hatch_top", z), ("block/portable_engine/exhaust_pipe_left", z), ("block/portable_engine/exhaust_pipe_right", z), ("block/portable_engine/exhaust_outlet_left", z), ("block/portable_engine/exhaust_outlet_right", z)]),
                ("block/directional_gearshift/horizontal/middle", &[("block/directional_gearshift/barrel", z), ("block/directional_gearshift/barrel_shaft", z)]),
                ("block/directional_gearshift/vertical/middle", &[("block/directional_gearshift/barrel", z), ("block/directional_gearshift/barrel_shaft", z)]),
                ("block/redstone_accumulator/block_front", &[("block/redstone_accumulator/diode", z)]),
                ("block/docking_connector/block", &[("block/docking_connector/main_piston_1", z), ("block/docking_connector/main_piston_2", z), ("block/docking_connector/side_piston_1", z), ("block/docking_connector/side_piston_2", z)]),
                ("block/swivel_bearing/block", &[("block/swivel_bearing/ironcog", z)]),
                ("block/altitude_sensor/block", &[("block/altitude_sensor/indicator", z), ("block/altitude_sensor/linear_case", z), ("block/altitude_sensor/linear_hand", z)]),
                ("block/gimbal_sensor/block", &[("block/gimbal_sensor/gimbal", c8), ("block/gimbal_sensor/compass", c8), ("block/gimbal_sensor/needle", c8), ("block/gimbal_sensor/redstone_indicator", z)]),
                ("block/tilt_sensor/block", &[("block/tilt_sensor/pendulum", z)]),
                ("block/velocity_sensor/block_horizontal", &[("block/velocity_sensor/fan", z), ("block/velocity_sensor/diode", z)]),
                ("block/velocity_sensor/block_vertical", &[("block/velocity_sensor/fan", z), ("block/velocity_sensor/diode", z)]),
                ("block/rope_winch/block_horizontal", &[("block/rope_winch/rope_coil", z), ("block/rope_winch/shaft", z)]),
                ("block/rope_winch/block_vertical", &[("block/rope_winch/rope_coil", z), ("block/rope_winch/shaft", z)]),
                ("block/analog_transmission/block", &[("block/analog_transmission/gear", z)]),
                ("block/laser_pointer/block", &[("block/laser_pointer/lens_off", z)]),
                ("block/navigation_table/block", &[("block/navigation_table/nav_table_pointer", z), ("block/navigation_table/redstone_indicator", z)]),
                ("block/redstone_inductor/block", &[("block/redstone_inductor/redstone_indicator", z)]),
                ("block/torsion_spring/block", &[("block/torsion_spring/spring", z)]),
            ];
            for (target, parts) in merges {
                merge_partials(dir, "simulated", target, parts)?;
            }
        }
        _ => {}
    }

    normalize_all_models_in_dir(&dir.join(format!("assets/{mod_id}/models")))?;
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
    fn fix_models_merges_partials_restores_obj_and_normalizes_cuboids() {
        let dir = scratch("models");
        write_json(
            &model_path(&dir, "offroad", "block/wheel_mount/block"),
            &json!({"textures": {"0": "offroad:base"}, "elements": [{"from": [0,0,0], "to": [16,4,16], "faces": {"up": {"texture": "#0"}}}]}),
        )
        .unwrap();
        write_json(
            &model_path(&dir, "offroad", "block/wheel_mount/mount"),
            &json!({"textures": {"0": "offroad:mount"}, "elements": [{"from": [12,4,4], "to": [4,12,12], "faces": {"north": {"texture": "#0"}}}]}),
        )
        .unwrap();
        let tire_obj = dir.join("assets/offroad/models/item/tire/block.obj");
        fs::create_dir_all(tire_obj.parent().unwrap()).unwrap();
        fs::write(&tire_obj, "v 0 0 0\n").unwrap();
        // Simulate a broken v2 overwritten tire JSON
        write_json(
            &model_path(&dir, "offroad", "item/tire/tire"),
            &json!({"parent": "minecraft:block/block", "elements": [{"name": "tachyon:obj_part", "from": [0,0,0], "to": [16,16,16]}]}),
        )
        .unwrap();

        fix("offroad", &dir).unwrap();

        let mount = read_json(&model_path(&dir, "offroad", "block/wheel_mount/block")).unwrap();
        let els = mount["elements"].as_array().unwrap();
        assert_eq!(els.len(), 2);
        assert_eq!(mount["textures"]["p0_0"], "offroad:mount");
        // Inverted [12,4,4]..[4,12,12] normalized to [4,4,4]..[12,12,12]
        assert_eq!(els[1]["from"], json!([4.0, 4.0, 4.0]));
        assert_eq!(els[1]["to"], json!([12.0, 12.0, 12.0]));

        // Second fix() call must remain idempotent
        fix("offroad", &dir).unwrap();
        let mount_again = read_json(&model_path(&dir, "offroad", "block/wheel_mount/block")).unwrap();
        assert_eq!(mount_again["elements"].as_array().unwrap().len(), 2);

        let tire = read_json(&model_path(&dir, "offroad", "item/tire/tire")).unwrap();
        assert_eq!(tire["loader"], "neoforge:obj");
        assert_eq!(tire["flip_v"], true);
        assert!(tire.get("elements").is_none());
        let _ = fs::remove_dir_all(&dir);
    }
}
