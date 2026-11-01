//! Create, ported on the device from its own file (`fetch.rs`), made for
//! Kinetica's engine to turn: its kinetic blocks get templates shaped as
//! they are (the porter guesses from blockstate properties alone: a log for
//! a shaft), and the parts that turn whole (a shaft, a cogwheel) leave the
//! chunk's mesh, for the engine draws them turning. The engine knows
//! Create's blocks by name (`game/tachyon_engine.rs`, `create_part`).
//!
//! Only files Tachyon writes change here: the porter's declarations, and
//! blockstates pointing at Create's own models in other ways.
//! Encased cogwheels keep the casings from BuilderTransformers and the
//! closed shaft defaults of EncasedCogwheelBlock (Create, MIT).
use serde_json::{Value, json};
use std::fs;
use std::path::Path;

/// Each engine block's template and its properties.
const TEMPLATES: &[(&str, &str, f64, &str)] = &[
    // A thin rod along its axis, as Kinetica's.
    ("shaft", "minecraft:iron_chain", 1.5, "minecraft:iron_block"),
    ("cogwheel", "minecraft:iron_chain", 1.5, "minecraft:oak_planks"),
    ("large_cogwheel", "minecraft:iron_chain", 1.5, "minecraft:oak_planks"),
    // Whole blocks round an axis, with no use of their own.
    ("andesite_encased_shaft", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("brass_encased_shaft", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("andesite_encased_cogwheel", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("brass_encased_cogwheel", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("andesite_encased_large_cogwheel", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("brass_encased_large_cogwheel", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
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
    // ChuteBlock's five facings and four shapes, stored in stair properties.
    ("chute", "minecraft:polished_andesite_stairs", 3.0, "minecraft:netherite_block"),
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
        if *block == "chute" {
            declaration["default_state"] = json!({
                "facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false",
            });
            declaration["shapes"] = chute_shape();
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
    // EncasedCogwheelBlock's default ends are closed. Keep
    // BuilderTransformers' material-specific casings in the chunk mesh.
    for block in [
        "andesite_encased_cogwheel",
        "brass_encased_cogwheel",
        "andesite_encased_large_cogwheel",
        "brass_encased_large_cogwheel",
    ] {
        rekey(&blockstates.join(format!("{block}.json")), |k| {
            let axis = k.split(',').find_map(|p| p.strip_prefix("axis="))?;
            let closed = k.split(',').any(|p| p == "top_shaft=false")
                && k.split(',').any(|p| p == "bottom_shaft=false");
            let key = format!("axis={axis}");
            // Axis-only keys already came through fix.
            (matches!(axis, "x" | "y" | "z") && (closed || k == key.as_str())).then_some(key)
        })?;
    }
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
    chute_models(&blockstates.join("chute.json"))?;
    Ok(())
}

/// `ChuteShapes`' vertical bounds, with an open centre instead of a solid
/// body, so item entities can enter. One shape property cannot carry the
/// diagonal facing too: all states use these hollow walls for now.
fn chute_shape() -> Value {
    let walls: [[f64; 6]; 8] = [
        [0.0625, 0.5, 0.0625, 0.125, 1.0, 0.9375],
        [0.875, 0.5, 0.0625, 0.9375, 1.0, 0.9375],
        [0.125, 0.5, 0.0625, 0.875, 1.0, 0.125],
        [0.125, 0.5, 0.875, 0.875, 1.0, 0.9375],
        [0.125, 0.0, 0.125, 0.1875, 0.5, 0.875],
        [0.8125, 0.0, 0.125, 0.875, 0.5, 0.875],
        [0.1875, 0.0, 0.125, 0.8125, 0.5, 0.1875],
        [0.1875, 0.0, 0.8125, 0.8125, 0.5, 0.875],
    ];
    json!({"property": "half", "values": {"bottom": walls, "top": walls}})
}

/// `ChuteGenerator`: keep Create's static models and their rotations.
/// Bottom means down; top keeps the horizontal facing. Stair shape names
/// alias normal, window, intersection and encased (both outer corners).
fn chute_models(file: &Path) -> Result<(), String> {
    let Some(state) = read(file) else { return Ok(()) };
    let Some(variants) = state["variants"].as_object() else { return Ok(()) };
    let mut kept = serde_json::Map::new();
    for (k, v) in variants {
        let property = |name: &str| {
            k.split(',').find_map(|p| p.split_once('=').filter(|(key, _)| *key == name).map(|(_, value)| value))
        };
        if property("waterlogged") == Some("true") {
            continue;
        }
        let Some(facing) = property("facing") else { continue };
        let shapes: &[&str] = match property("shape") {
            Some("normal") => &["straight"],
            Some("window") => &["inner_left"],
            Some("intersection") => &["inner_right"],
            Some("encased") => &["outer_left", "outer_right"],
            _ => continue,
        };
        let facing = match facing {
            "down" => "half=bottom".to_owned(),
            "north" | "south" | "west" | "east" => format!("facing={facing},half=top"),
            _ => continue,
        };
        for shape in shapes {
            kept.insert(format!("{facing},shape={shape}"), v.clone());
        }
    }
    if !kept.is_empty() {
        write(file, &json!({"variants": kept}))?;
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

    #[test]
    fn chutes_keep_static_models_for_every_stair_alias_and_a_hollow_template() {
        let dir = scratch("chute");
        let file = dir.join("assets/create/blockstates/chute.json");
        let mut variants = serde_json::Map::new();
        for facing in ["down", "north", "south", "west", "east"] {
            for shape in ["normal", "window", "intersection", "encased"] {
                variants.insert(
                    format!("facing={facing},shape={shape}"),
                    json!({"model": format!("create:block/chute/{facing}/{shape}"), "y": 90}),
                );
            }
        }
        write(&file, &json!({"variants": variants})).unwrap();
        fix(&dir).unwrap();
        let declaration = read(&dir.join("data/create/horizon/block/chute.json")).unwrap();
        assert_eq!(declaration["template"], "minecraft:polished_andesite_stairs");
        assert_eq!(declaration["default_state"]["half"], "bottom");
        assert_eq!(declaration["default_state"]["shape"], "straight");
        let walls: Vec<[f64; 6]> = serde_json::from_value(declaration["shapes"]["values"]["bottom"].clone()).unwrap();
        assert_eq!(walls.len(), 8);
        assert!(walls.iter().all(|b| b[3] <= 0.5 || b[0] >= 0.5 || b[5] <= 0.5 || b[2] >= 0.5));
        let state = read(&file).unwrap();
        assert_eq!(state["variants"].as_object().unwrap().len(), 25);
        assert_eq!(state["variants"]["half=bottom,shape=inner_left"]["model"], "create:block/chute/down/window");
        for facing in ["north", "south", "west", "east"] {
            for (shape, alias) in [("normal", "straight"), ("window", "inner_left"), ("intersection", "inner_right"), ("encased", "outer_left"), ("encased", "outer_right")] {
                let key = format!("facing={facing},half=top,shape={alias}");
                assert_eq!(state["variants"][&key]["model"], format!("create:block/chute/{facing}/{shape}"));
                assert_eq!(state["variants"][&key]["y"], 90);
            }
        }
        assert!(!TURNING.contains(&"chute"));
        fix(&dir).unwrap();
        assert_eq!(read(&file).unwrap(), state);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn encased_cogwheels_keep_only_their_closed_casing_models() {
        let dir = scratch("encased-cogs");
        let states = dir.join("assets/create/blockstates");
        let declarations = dir.join("data/create/horizon/block");
        let blocks = [
            "andesite_encased_cogwheel",
            "brass_encased_cogwheel",
            "andesite_encased_large_cogwheel",
            "brass_encased_large_cogwheel",
        ];
        for block in blocks {
            write(
                &declarations.join(format!("{block}.json")),
                &json!({"template": "minecraft:oak_log", "properties": {
                    "hardness": 0.5, "sounds": "minecraft:stone"}}),
            )
            .unwrap();
            let mut variants = serde_json::Map::new();
            for (axis, x, y) in [("x", 90, 90), ("y", 0, 0), ("z", 90, 0)] {
                for (top, bottom, suffix) in [
                    (false, false, ""),
                    (true, false, "_top"),
                    (false, true, "_bottom"),
                    (true, true, "_top_bottom"),
                ] {
                    let key = match axis {
                        "x" => format!("top_shaft={top},axis={axis},bottom_shaft={bottom}"),
                        "y" => format!("bottom_shaft={bottom},top_shaft={top},axis={axis}"),
                        _ => format!("axis={axis},bottom_shaft={bottom},top_shaft={top}"),
                    };
                    variants.insert(key, json!({
                        "model": format!("create:block/{block}{suffix}"),
                        "x": x, "y": y, "uvlock": true, "weight": 2,
                    }));
                }
            }
            write(&states.join(format!("{block}.json")), &json!({"variants": variants})).unwrap();
        }
        fix(&dir).unwrap();
        let mut fixed = Vec::new();
        for block in blocks {
            let declaration = read(&declarations.join(format!("{block}.json"))).unwrap();
            assert_eq!(declaration["template"], "minecraft:quartz_pillar");
            assert_eq!(declaration["properties"]["hardness"], 2.0);
            assert_eq!(declaration["properties"]["sounds"], "minecraft:oak_planks");
            assert!(declaration.get("shapes").is_none());
            let state = read(&states.join(format!("{block}.json"))).unwrap();
            assert_eq!(state["variants"].as_object().unwrap().len(), 3);
            for (axis, x, y) in [("x", 90, 90), ("y", 0, 0), ("z", 90, 0)] {
                let key = format!("axis={axis}");
                assert_eq!(state["variants"][key.as_str()], json!({
                    "model": format!("create:block/{block}"),
                    "x": x, "y": y, "uvlock": true, "weight": 2,
                }));
            }
            assert!(!dir.join(format!("assets/create/models/block/tachyon_still/{block}.json")).exists());
            fixed.push((block, declaration, state));
        }
        fix(&dir).unwrap();
        for (block, declaration, state) in fixed {
            assert_eq!(read(&declarations.join(format!("{block}.json"))).unwrap(), declaration);
            assert_eq!(read(&states.join(format!("{block}.json"))).unwrap(), state);
            assert!(!dir.join(format!("assets/create/models/block/tachyon_still/{block}.json")).exists());
        }
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_encased_cogwheel_states_are_left_alone() {
        let dir = scratch("encased-missing");
        fix(&dir).unwrap();
        for block in [
            "andesite_encased_cogwheel",
            "brass_encased_cogwheel",
            "andesite_encased_large_cogwheel",
            "brass_encased_large_cogwheel",
        ] {
            assert!(read(&dir.join(format!("assets/create/blockstates/{block}.json"))).is_none());
            assert!(!dir.join(format!("assets/create/models/block/tachyon_still/{block}.json")).exists());
        }
        let _ = fs::remove_dir_all(&dir);
    }
}
