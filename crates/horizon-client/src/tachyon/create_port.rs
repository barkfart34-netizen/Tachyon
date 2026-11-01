//! Create, ported on the device from its own file (`fetch.rs`), made for
//! Kinetica's engine to turn: its kinetic blocks get templates shaped as
//! they are (the porter guesses from blockstate properties alone: a log for
//! a shaft), and the parts that turn whole (a shaft, a cogwheel) leave the
//! chunk's mesh, for the engine draws them turning. The engine knows
//! Create's blocks by name (`game/tachyon_engine.rs`, `create_part`).
//!
//! Only files Tachyon writes change here: the porter's declarations, and
//! blockstates pointing at Create's own models in other ways.
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

/// Each kinetic block's template and its properties.
const TEMPLATES: &[(&str, &str, f64, &str)] = &[
    // A thin rod along its axis, as Kinetica's.
    ("shaft", "minecraft:iron_chain", 1.5, "minecraft:iron_block"),
    ("cogwheel", "minecraft:iron_chain", 1.5, "minecraft:oak_planks"),
    ("large_cogwheel", "minecraft:iron_chain", 1.5, "minecraft:oak_planks"),
    // Whole blocks round an axis, with no use of their own.
    ("andesite_encased_shaft", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("brass_encased_shaft", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("gearbox", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("clutch", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("gearshift", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("encased_fan", "minecraft:quartz_pillar", 2.0, "minecraft:copper_block"),
    // Six ways, out of the block they were put on.
    ("creative_motor", "minecraft:waxed_lightning_rod", 1.5, "minecraft:copper_block"),
    ("hand_crank", "minecraft:waxed_lightning_rod", 0.5, "minecraft:oak_planks"),
    ("andesite_funnel", "minecraft:waxed_lightning_rod", 1.0, "minecraft:copper_block"),
    ("brass_funnel", "minecraft:waxed_lightning_rod", 1.0, "minecraft:copper_block"),
    // A tray 13 high, as Create's depot.
    ("depot", "minecraft:end_portal_frame", 2.0, "minecraft:copper_block"),
    // Four ways, a whole block.
    ("mechanical_press", "minecraft:white_glazed_terracotta", 2.5, "minecraft:copper_block"),
    ("millstone", "minecraft:polished_andesite", 2.0, "minecraft:stone"),
    ("mechanical_mixer", "minecraft:polished_andesite", 2.5, "minecraft:copper_block"),
    // A hollow block with a floor.
    ("basin", "minecraft:composter", 2.0, "minecraft:copper_block"),
    // Turning wheels, a whole block to stand on: a crushing wheel along its
    // axis, a small water wheel the way it faces (its shaft's).
    ("crushing_wheel", "minecraft:iron_chain", 2.0, "minecraft:stone"),
    ("water_wheel", "minecraft:waxed_lightning_rod", 1.5, "minecraft:oak_planks"),
    ("large_water_wheel", "minecraft:iron_chain", 1.5, "minecraft:oak_planks"),
];

/// The blocks a whole block in every state, by their template's property.
const WHOLE: &[(&str, &str, &[&str])] = &[
    ("creative_motor", "facing", &["down", "up", "north", "south", "west", "east"]),
    ("water_wheel", "facing", &["down", "up", "north", "south", "west", "east"]),
    ("large_water_wheel", "axis", &["x", "y", "z"]),
    ("crushing_wheel", "axis", &["x", "y", "z"]),
];

/// The blocks drawn by the engine alone, turning whole.
const TURNING: &[&str] = &["shaft", "cogwheel", "large_cogwheel", "crushing_wheel", "large_water_wheel"];

/// Makes the Create ported into `dir` (`horizon-mods/create`) the engine's.
pub fn fix(dir: &Path) -> Result<(), String> {
    let declarations = dir.join("data/create/horizon/block");
    let blockstates = dir.join("assets/create/blockstates");
    for (block, template, hardness, sounds) in TEMPLATES {
        let file = declarations.join(format!("{block}.json"));
        let mut declaration: Value = read(&file).unwrap_or_else(|| json!({}));
        declaration["template"] = json!(template);
        declaration["properties"]["hardness"] = json!(hardness);
        declaration["properties"]["sounds"] = json!(sounds);
        // A motor is a box, not its template's rod; a wheel a whole block.
        if let Some((_, property, values)) = WHOLE.iter().find(|(b, _, _)| b == block) {
            let body = if *block == "creative_motor" {
                [[0.0625, 0.0625, 0.0625, 0.9375, 0.9375, 0.9375]]
            } else {
                [[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]]
            };
            let values: serde_json::Map<String, Value> = (values.iter())
                .map(|v| ((*v).to_owned(), json!(body)))
                .collect();
            declaration["shapes"] = json!({"property": property, "values": values});
        }
        write(&file, &declaration)?;
    }
    // A turning block's chunk model: its particle alone.
    for block in TURNING {
        let still = format!("block/tachyon_still/{block}");
        let model = json!({
            "parent": "minecraft:block/block",
            "textures": {"particle": particle(dir, block).unwrap_or_else(|| "create:block/axis".into())},
            "elements": [],
        });
        write(&dir.join(format!("assets/create/models/{still}.json")), &model)?;
        let state = json!({"variants": {"": {"model": format!("create:{still}")}}});
        write(&blockstates.join(format!("{block}.json")), &state)?;
    }
    // The templates' properties: an axis where Create has a facing, none
    // where it has `powered` or a direction a basin points.
    rekey(&blockstates.join("clutch.json"), |k| k.strip_suffix(",powered=false").map(str::to_owned))?;
    rekey(&blockstates.join("gearshift.json"), |k| k.strip_suffix(",powered=false").map(str::to_owned))?;
    rekey(&blockstates.join("encased_fan.json"), |k| {
        let axis = match k {
            "facing=east" => "x",
            "facing=up" => "y",
            "facing=south" => "z",
            _ => return None,
        };
        Some(format!("axis={axis}"))
    })?;
    rekey(&blockstates.join("basin.json"), |k| (k == "facing=down").then(String::new))?;
    for funnel in ["andesite_funnel", "brass_funnel"] {
        rekey(&blockstates.join(format!("{funnel}.json")), |k| {
            let facing = k
                .split(',')
                .find_map(|p| p.strip_prefix("facing="))?;
            let inserting = k.contains("extracting=false")
                && !k.contains("powered=true")
                && !k.contains("waterlogged=true");
            inserting.then(|| format!("facing={facing}"))
        })?;
    }
    Ok(())
}

/// The particle texture of a block's model, as its blockstate names it.
fn particle(dir: &Path, block: &str) -> Option<String> {
    let state = read(&dir.join(format!("assets/create/blockstates/{block}.json")))?;
    let variant = state["variants"].as_object()?.values().next()?;
    let variant = if variant.is_array() { &variant[0] } else { variant };
    let model = variant["model"].as_str()?.strip_prefix("create:")?.to_owned();
    let model = read(&dir.join(format!("assets/create/models/{model}.json")))?;
    model["textures"]["particle"].as_str().map(str::to_owned)
}

/// Rewrites a blockstate's variant keys by `key`, dropping those it gives
/// None for; leaves a missing file alone.
fn rekey(file: &Path, key: impl Fn(&str) -> Option<String>) -> Result<(), String> {
    let Some(state) = read(file) else {
        return Ok(());
    };
    let Some(variants) = state["variants"].as_object() else {
        return Ok(());
    };
    let mut kept = serde_json::Map::new();
    for (k, v) in variants {
        if let Some(k) = key(k) {
            kept.entry(k).or_insert_with(|| v.clone());
        }
    }
    if kept.is_empty() {
        return Ok(());
    }
    write(file, &json!({"variants": kept}))
}

fn read(file: &Path) -> Option<Value> {
    serde_json::from_slice(&fs::read(file).ok()?).ok()
}

fn write(file: &Path, value: &Value) -> Result<(), String> {
    if let Some(parent) = file.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    let text = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
    fs::write(file, text).map_err(|e| format!("{}: {e}", file.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tachyon-create-port-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn create_blocks_get_the_engines_templates_and_states() {
        let dir = scratch("fix");
        let states = dir.join("assets/create/blockstates");
        write(
            &dir.join("data/create/horizon/block/shaft.json"),
            &json!({"template": "minecraft:oak_log"}),
        )
        .unwrap();
        write(
            &states.join("shaft.json"),
            &json!({"variants": {"axis=x": {"model": "create:block/shaft", "x": 90}}}),
        )
        .unwrap();
        write(
            &dir.join("assets/create/models/block/shaft.json"),
            &json!({"textures": {"particle": "create:block/axis"}}),
        )
        .unwrap();
        write(
            &states.join("clutch.json"),
            &json!({"variants": {
                "axis=x,powered=false": {"model": "create:block/clutch/block"},
                "axis=x,powered=true": {"model": "create:block/clutch/block_powered"}}}),
        )
        .unwrap();
        write(
            &states.join("encased_fan.json"),
            &json!({"variants": {
                "facing=east": {"model": "create:block/encased_fan/block", "x": 90, "y": 90},
                "facing=west": {"model": "create:block/encased_fan/block", "x": 90, "y": 270},
                "facing=up": {"model": "create:block/encased_fan/block"}}}),
        )
        .unwrap();
        fix(&dir).unwrap();
        let shaft = read(&dir.join("data/create/horizon/block/shaft.json")).unwrap();
        assert_eq!(shaft["template"], "minecraft:iron_chain");
        assert_eq!(shaft["properties"]["hardness"], 1.5);
        let state = read(&states.join("shaft.json")).unwrap();
        assert_eq!(state["variants"][""]["model"], "create:block/tachyon_still/shaft");
        let still = read(&dir.join("assets/create/models/block/tachyon_still/shaft.json")).unwrap();
        assert_eq!(still["textures"]["particle"], "create:block/axis");
        let large = read(&dir.join("data/create/horizon/block/large_water_wheel.json")).unwrap();
        assert_eq!(large["template"], "minecraft:iron_chain");
        assert_eq!(large["properties"]["hardness"], 1.5);
        assert_eq!(large["shapes"]["property"], "axis");
        for axis in ["x", "y", "z"] {
            assert_eq!(
                large["shapes"]["values"][axis],
                json!([[0.0, 0.0, 0.0, 1.0, 1.0, 1.0]])
            );
        }
        let state = read(&states.join("large_water_wheel.json")).unwrap();
        assert_eq!(
            state["variants"][""]["model"],
            "create:block/tachyon_still/large_water_wheel"
        );
        let still =
            read(&dir.join("assets/create/models/block/tachyon_still/large_water_wheel.json")).unwrap();
        assert_eq!(still["elements"], json!([]));
        let clutch = read(&states.join("clutch.json")).unwrap();
        assert_eq!(
            clutch["variants"].as_object().unwrap().keys().collect::<Vec<_>>(),
            ["axis=x"]
        );
        let fan = read(&states.join("encased_fan.json")).unwrap();
        assert_eq!(fan["variants"]["axis=x"]["y"], 90);
        assert_eq!(fan["variants"].as_object().unwrap().len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }
}
