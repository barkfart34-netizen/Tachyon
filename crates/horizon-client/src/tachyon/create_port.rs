//! Create, ported on the device from its own file (`fetch.rs`), made for
//! Kinetica's engine to turn: its kinetic blocks get templates shaped as
//! they are (the porter guesses from blockstate properties alone: a log for
//! a shaft), and the parts that turn whole (a shaft, a cogwheel) leave the
//! chunk's mesh, for the engine draws them turning. The engine knows
//! Create's blocks by name (`game/tachyon_engine.rs`, `create_part`).
//!
//! The porter's declarations, blockstates pointing at Create's own models
//! in other ways, and loot predicates using the templates' properties
//! change here, all in the player's fetched port.
//! BlazeBurnerBlock's heat uses an anchor's five `charges`; its cage model
//! stays in the chunk, without BlazeBurnerRenderer's head and rods.
//!
//! GaugeBlock's facing and axis_along_first become one orientation per
//! shaft axis, keeping GaugeGenerator and DirectionalAxisBlockStateGen's
//! casing models and rotations. GogglesItem supplies the head slot.
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
    ("speedometer", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("stressometer", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("clutch", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("gearshift", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("encased_chain_drive", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
    ("adjustable_chain_gearshift", "minecraft:quartz_pillar", 2.0, "minecraft:oak_planks"),
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
    // HorizontalAxisKineticBlock's shaft is across the player's facing.
    ("rotation_speed_controller", "minecraft:white_glazed_terracotta", 3.0, "minecraft:gold_block"),
    ("millstone", "minecraft:polished_andesite", 2.0, "minecraft:stone"),
    ("mechanical_mixer", "minecraft:polished_andesite", 2.5, "minecraft:copper_block"),
    // ArmBlock's floor-mounted arm; the engine draws its moving bodies.
    ("mechanical_arm", "minecraft:polished_andesite", 2.5, "minecraft:copper_block"),
    // A hollow block with a floor.
    ("basin", "minecraft:composter", 2.0, "minecraft:copper_block"),
    // Five heat levels, not an anchor's charging or respawn use.
    ("blaze_burner", "minecraft:respawn_anchor", 3.0, "minecraft:gold_block"),
    ("lit_blaze_burner", "minecraft:respawn_anchor", 3.0, "minecraft:gold_block"),
    // Turning wheels, a whole block to stand on: a crushing wheel along its
    // axis, a small water wheel the way it faces (its shaft's).
    ("crushing_wheel", "minecraft:iron_chain", 2.0, "minecraft:stone"),
    ("water_wheel", "minecraft:waxed_lightning_rod", 1.5, "minecraft:oak_planks"),
    ("large_water_wheel", "minecraft:iron_chain", 1.5, "minecraft:oak_planks"),
    ("flywheel", "minecraft:iron_chain", 1.5, "minecraft:copper_block"),
    // The large wheel's body round it (WaterWheelStructuralBlock), facing
    // the way to the wheel; no item, drawn by the wheel.
    ("water_wheel_structure", "minecraft:waxed_lightning_rod", 1.5, "minecraft:oak_planks"),
];

/// The blocks a whole block in every state, by their template's property.
const WHOLE: &[(&str, &str, &[&str])] = &[
    ("creative_motor", "facing", &["down", "up", "north", "south", "west", "east"]),
    ("water_wheel", "facing", &["down", "up", "north", "south", "west", "east"]),
    ("large_water_wheel", "axis", &["x", "y", "z"]),
    ("crushing_wheel", "axis", &["x", "y", "z"]),
    ("flywheel", "axis", &["x", "y", "z"]),
    ("water_wheel_structure", "facing", &["down", "up", "north", "south", "west", "east"]),
];

/// The blocks drawn by the engine alone, turning whole.
const TURNING: &[&str] = &[
    "shaft",
    "cogwheel",
    "large_cogwheel",
    "crushing_wheel",
    "large_water_wheel",
    "flywheel",
];

/// The version of [`fix`]: a port fixed by an older one is fixed again.
const VERSION: &str = "2";
const STAMP: &str = ".tachyon-fix";

/// Fixes a Create ported into `dir` (`horizon-mods/create`) by an older
/// Tachyon again, once: the engine has learned more of its blocks since.
/// `jar`, Create's own file where it is kept, gives back their recipes.
pub fn refresh(dir: &Path, jar: Option<&Path>) {
    if !dir.join("data/create/horizon/block").is_dir()
        || fs::read_to_string(dir.join(STAMP)).is_ok_and(|v| v.trim() == VERSION)
    {
        return;
    }
    match fix(dir) {
        Ok(()) => log::info!("Tachyon: Create's port fixed again for this version"),
        Err(e) => log::warn!("Tachyon: Create's port: {e}"),
    }
    if let Some(jar) = jar.filter(|j| j.is_file())
        && let Err(e) = restore_recipes(dir, jar)
    {
        log::warn!("Tachyon: Create's recipes: {e}");
    }
}

/// The recipes of the blocks the porter left out (no vanilla block to
/// copy) that the engine makes: Create's own, from its file.
const RECIPES: &[&str] = &[
    "crafting/kinetics/encased_chain_drive",
    "crafting/kinetics/encased_chain_drive_from_zinc",
    "crafting/kinetics/adjustable_chain_gearshift",
    "crafting/kinetics/large_water_wheel",
];

/// Puts [`RECIPES`] from Create's `jar` into the port at `dir`, their
/// ingredients written as the porter writes them (`"id"`, `"#tag"`), and
/// the iron nuggets' convention tag they use where the port has none.
pub fn restore_recipes(dir: &Path, jar: &Path) -> Result<(), String> {
    let file = fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    for recipe in RECIPES {
        let out = dir.join(format!("data/create/recipe/{recipe}.json"));
        if out.is_file() {
            continue;
        }
        let Ok(mut entry) = zip.by_name(&format!("data/create/recipe/{recipe}.json")) else {
            continue;
        };
        let mut bytes = Vec::new();
        std::io::Read::read_to_end(&mut entry, &mut bytes).map_err(|e| e.to_string())?;
        let Ok(mut json) = serde_json::from_slice::<Value>(&bytes) else {
            continue;
        };
        porter_ingredients(&mut json);
        write(&out, &json)?;
    }
    let iron = dir.join("data/c/tags/item/nuggets/iron.json");
    if !iron.is_file() {
        write(&iron, &json!({"values": ["minecraft:iron_nugget"]}))?;
    }
    Ok(())
}

/// A 1.21.1 recipe's ingredients as the game reads them: `{"item": id}`
/// as `id`, `{"tag": t}` as `#t`, a list of them each so.
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
        if *block == "water_wheel_structure" {
            declaration["item"] = json!(false);
        }
        if *block == "chute" {
            declaration["default_state"] = json!({
                "facing": "north", "half": "bottom", "shape": "straight", "waterlogged": "false",
            });
            declaration["shapes"] = chute_shape();
        }
        // AllShapes.SPEED_CONTROLLER: the base, casing and top plate.
        if *block == "rotation_speed_controller" {
            let body = [
                [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],
                [0.0625, 0.0625, 0.0625, 0.9375, 0.8125, 0.9375],
                [0.0, 0.5, 0.0, 1.0, 0.875, 1.0],
            ];
            let values: serde_json::Map<String, Value> = ["north", "south", "west", "east"]
                .into_iter()
                .map(|f| (f.to_owned(), json!(body)))
                .collect();
            declaration["shapes"] = json!({"property": "facing", "values": values});
        }
        if matches!(*block, "blaze_burner" | "lit_blaze_burner") {
            // AllShapes.HEATER_BLOCK_SHAPE: 1,0,1 to 15,14,15. A filled
            // BlazeBurnerBlockItem starts smouldering, an empty lit cage
            // with regular fire. The lit block has no item of its own.
            let lit = *block == "lit_blaze_burner";
            let initial = if lit { "0" } else { "1" };
            let values: serde_json::Map<String, Value> = ["0", "1", "2", "3", "4"]
                .into_iter()
                .map(|v| (v.to_owned(), json!([[0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375]])))
                .collect();
            declaration["shapes"] = json!({"property": "charges", "values": values});
            declaration["default_state"] = json!({"charges": initial});
            declaration["item"] = json!(!lit);
            declaration["properties"]["explosion_resistance"] = json!(6.0);
            declaration["properties"]["requires_tool"] = json!(true);
            // Per-state light isn't declared yet: the filled cage keeps
            // the anchor's emission; both lit flames emit regular fire's 12.
            declaration["properties"]["light"] = if lit { json!(12) } else { Value::Null };
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
    controller_state(&blockstates.join("rotation_speed_controller.json"))?;
    // ChainDriveBlock's single drive on each axis: its chain to the drives
    // beside it is the engine's, not a state.
    for chain in ["encased_chain_drive", "adjustable_chain_gearshift"] {
        rekey(&blockstates.join(format!("{chain}.json")), |k| {
            let axis = k.split(',').find_map(|p| p.strip_prefix("axis="))?;
            let single = k.split(',').all(|p| {
                p.starts_with("axis=") || matches!(p, "part=none" | "axis_along_first=false" | "powered=false")
            });
            (matches!(axis, "x" | "y" | "z") && single).then(|| format!("axis={axis}"))
        })?;
    }
    // ArmBlock on the floor only, for now.
    rekey(&blockstates.join("mechanical_arm.json"), |k| {
        (k == "ceiling=false" || k.is_empty()).then(String::new)
    })?;
    // DirectionalAxisKineticBlock's shaft is perpendicular to its facing.
    for gauge in ["speedometer", "stressometer"] {
        rekey(&blockstates.join(format!("{gauge}.json")), |k| {
            if matches!(k, "axis=x" | "axis=y" | "axis=z") {
                return Some(k.to_owned());
            }
            let facing = k.split(',').find_map(|p| p.strip_prefix("facing="))?;
            let first = k.split(',').find_map(|p| p.strip_prefix("axis_along_first="))?;
            let axis = match (facing, first) {
                ("up", "true") => "x",
                ("north", "false") => "y",
                ("up", "false") => "z",
                _ => return None,
            };
            Some(format!("axis={axis}"))
        })?;
    }
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
    // AllItems.GOGGLES and GogglesItem: worn in the head slot, its model kept.
    let file = dir.join("data/create/horizon/item/goggles.json");
    if let Some(mut goggles) = read(&file) {
        goggles["template"] = json!("minecraft:leather_helmet");
        write(&file, &goggles)?;
    }
    chute_models(&blockstates.join("chute.json"))?;
    burner_flames(&blockstates.join("lit_blaze_burner.json"))?;
    burner_loot(&dir.join("data/create/loot_table/blocks/blaze_burner.json"))?;
    fs::write(dir.join(STAMP), VERSION).map_err(|e| format!("{}: {e}", dir.display()))?;
    Ok(())
}

/// LitBlazeBurnerBlock's regular/soul model variants by the template's
/// charges: 1 for soul, the other states regular. The models are the
/// player's fetched ones; a second fix keeps the remapping.
fn burner_flames(file: &Path) -> Result<(), String> {
    let Some(state) = read(file) else {
        return Ok(());
    };
    let Some(variants) = state["variants"].as_object() else {
        return Ok(());
    };
    let regular = variants
        .get("flame_type=regular")
        .or_else(|| variants.get("charges=0"));
    let soul = variants
        .get("flame_type=soul")
        .or_else(|| variants.get("charges=1"));
    let (Some(regular), Some(soul)) = (regular, soul) else {
        return Ok(());
    };
    let variants: serde_json::Map<String, Value> = ["0", "1", "2", "3", "4"]
        .into_iter()
        .map(|c| {
            (
                format!("charges={c}"),
                if c == "1" { soul.clone() } else { regular.clone() },
            )
        })
        .collect();
    write(file, &json!({"variants": variants}))
}

/// BlazeBurnerBlock.buildLootTable tests `blaze`. Remap its fetched
/// predicates to `charges` so breaking a filled burner still drops it.
fn burner_loot(file: &Path) -> Result<(), String> {
    fn remap(value: &mut Value) {
        if value["condition"] == "minecraft:block_state_property"
            && value["block"] == "create:blaze_burner"
            && let Some(properties) = value.get_mut("properties").and_then(Value::as_object_mut)
        {
            let charges = match properties.get("blaze").and_then(Value::as_str) {
                Some("none") => Some("0"),
                Some("smouldering") => Some("1"),
                Some("fading") => Some("2"),
                Some("kindled") => Some("3"),
                Some("seething") => Some("4"),
                _ => None,
            };
            if let Some(charges) = charges {
                properties.remove("blaze");
                properties.insert("charges".into(), json!(charges));
            }
        }
        match value {
            Value::Array(list) => list.iter_mut().for_each(remap),
            Value::Object(map) => map.values_mut().for_each(remap),
            _ => {}
        }
    }
    let Some(mut loot) = read(file) else {
        return Ok(());
    };
    remap(&mut loot);
    write(file, &loot)
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

/// SpeedControllerBlock's two horizontal axes as the four-way template's
/// facings. Leave the casing in the chunk model; SpeedControllerRenderer's
/// shaft and bracket are drawn by the engine from the device's models.
fn controller_state(file: &Path) -> Result<(), String> {
    let Some(state) = read(file) else {
        return Ok(());
    };
    let Some(variants) = state["variants"].as_object() else {
        return Ok(());
    };
    let mut kept = serde_json::Map::new();
    for (facing, axis) in [("north", "x"), ("south", "x"), ("west", "z"), ("east", "z")] {
        if let Some(model) = variants.get(&format!("axis={axis}"))
            .or_else(|| variants.get(&format!("facing={facing}")))
        {
            kept.insert(format!("facing={facing}"), model.clone());
        }
    }
    if kept.is_empty() {
        return Ok(());
    }
    write(file, &json!({"variants": kept}))
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
        write(
            &states.join("mechanical_arm.json"),
            &json!({"variants": {
                "ceiling=false": {"model": "create:block/mechanical_arm/block"},
                "ceiling=true": {"model": "create:block/mechanical_arm/block", "x": 180}}}),
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
        let body = read(&dir.join("data/create/horizon/block/water_wheel_structure.json")).unwrap();
        assert_eq!(body["template"], "minecraft:waxed_lightning_rod");
        assert_eq!(body["item"], false);
        assert_eq!(body["shapes"]["property"], "facing");
        assert_eq!(fs::read_to_string(dir.join(STAMP)).unwrap(), VERSION);
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
        let arm = read(&dir.join("data/create/horizon/block/mechanical_arm.json")).unwrap();
        assert_eq!(arm["template"], "minecraft:polished_andesite");
        let arm = read(&states.join("mechanical_arm.json")).unwrap();
        assert_eq!(arm["variants"].as_object().unwrap().len(), 1);
        assert_eq!(arm["variants"][""]["model"], "create:block/mechanical_arm/block");
        assert!(arm["variants"][""]["x"].is_null());
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
    fn left_out_recipes_come_back_from_creates_file_as_the_porter_writes_them() {
        let dir = scratch("recipes");
        fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("create.jar");
        {
            let mut zip = zip::ZipWriter::new(fs::File::create(&jar).unwrap());
            let options = zip::write::SimpleFileOptions::default();
            zip.start_file("data/create/recipe/crafting/kinetics/encased_chain_drive.json", options)
                .unwrap();
            let recipe = json!({"type": "minecraft:crafting_shapeless",
                "ingredients": [{"item": "create:andesite_casing"}, {"tag": "c:nuggets/iron"}],
                "result": {"count": 1, "id": "create:encased_chain_drive"}});
            std::io::Write::write_all(&mut zip, recipe.to_string().as_bytes()).unwrap();
            zip.finish().unwrap();
        }
        restore_recipes(&dir, &jar).unwrap();
        let out = read(&dir.join("data/create/recipe/crafting/kinetics/encased_chain_drive.json")).unwrap();
        assert_eq!(out["ingredients"], json!(["create:andesite_casing", "#c:nuggets/iron"]));
        let iron = read(&dir.join("data/c/tags/item/nuggets/iron.json")).unwrap();
        assert_eq!(iron["values"], json!(["minecraft:iron_nugget"]));
        // Missing ones are skipped; a second time keeps what is there.
        restore_recipes(&dir, &jar).unwrap();
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

    #[test]
    fn create_gauges_keep_three_native_orientations_and_wearable_goggles() {
        let dir = scratch("gauges");
        let states = dir.join("assets/create/blockstates");
        let declarations = dir.join("data/create/horizon/block");
        let x = json!({"model": "create:block/gauge/base", "x": 0, "y": 90});
        let y = json!({"model": "create:block/gauge/base_wall", "x": 90, "y": 270});
        let z = json!({"model": "create:block/gauge/base", "x": 0, "y": 0});
        for (block, reversed) in [("speedometer", false), ("stressometer", true)] {
            write(
                &declarations.join(format!("{block}.json")),
                &json!({"template": "minecraft:oak_log"}),
            )
            .unwrap();
            let mut variants = serde_json::Map::new();
            for facing in ["down", "up", "north", "south", "west", "east"] {
                for first in ["false", "true"] {
                    let key = if reversed {
                        format!("axis_along_first={first},facing={facing}")
                    } else {
                        format!("facing={facing},axis_along_first={first}")
                    };
                    let variant = match (facing, first) {
                        ("up", "true") => x.clone(),
                        ("north", "false") => y.clone(),
                        ("up", "false") => z.clone(),
                        _ => json!({"model": "create:block/gauge/base_wall", "x": 180, "y": 180}),
                    };
                    variants.insert(key, variant);
                }
            }
            write(&states.join(format!("{block}.json")), &json!({"variants": variants})).unwrap();
        }
        let goggles_file = dir.join("data/create/horizon/item/goggles.json");
        let mut goggles = json!({
            "template": "minecraft:stick",
            "components": {"minecraft:max_stack_size": 1},
        });
        write(&goggles_file, &goggles).unwrap();
        let model_file = dir.join("assets/create/models/item/goggles.json");
        let model = json!({"parent": "minecraft:item/generated"});
        write(&model_file, &model).unwrap();
        fix(&dir).unwrap();
        let expected = json!({"variants": {"axis=x": x, "axis=y": y, "axis=z": z}});
        let mut normalized = Vec::new();
        for block in ["speedometer", "stressometer"] {
            let declaration = read(&declarations.join(format!("{block}.json"))).unwrap();
            assert_eq!(declaration["template"], "minecraft:quartz_pillar");
            assert_eq!(declaration["properties"]["hardness"], 2.0);
            assert_eq!(declaration["properties"]["sounds"], "minecraft:oak_planks");
            assert!(declaration.get("shapes").is_none());
            let file = states.join(format!("{block}.json"));
            let state = read(&file).unwrap();
            assert_eq!(
                state["variants"].as_object().unwrap().keys().collect::<Vec<_>>(),
                ["axis=x", "axis=y", "axis=z"]
            );
            assert_eq!(state, expected);
            assert!(
                !dir.join(format!("assets/create/models/block/tachyon_still/{block}.json")).exists()
            );
            normalized.push((file.clone(), fs::read(&file).unwrap()));
        }
        goggles["template"] = json!("minecraft:leather_helmet");
        assert_eq!(read(&goggles_file), Some(goggles.clone()));
        assert_eq!(read(&model_file), Some(model.clone()));
        fix(&dir).unwrap();
        for (file, bytes) in normalized {
            assert_eq!(fs::read(file).unwrap(), bytes);
        }
        assert_eq!(read(&goggles_file), Some(goggles));
        assert_eq!(read(&model_file), Some(model));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_controller_keeps_its_casing_and_gets_four_horizontal_states() {
        let dir = scratch("controller");
        let state_file = dir.join("assets/create/blockstates/rotation_speed_controller.json");
        write(&state_file, &json!({"variants": {
            "axis=x": {"model": "create:block/rotation_speed_controller/block", "y": 90},
            "axis=z": {"model": "create:block/rotation_speed_controller/block", "y": 0},
        }})).unwrap();
        fix(&dir).unwrap();
        let declaration = read(&dir.join("data/create/horizon/block/rotation_speed_controller.json")).unwrap();
        assert_eq!(declaration["template"], "minecraft:white_glazed_terracotta");
        assert_eq!(declaration["properties"]["hardness"], 3.0);
        assert_eq!(declaration["shapes"]["property"], "facing");
        let state = read(&state_file).unwrap();
        assert_eq!(state["variants"].as_object().unwrap().len(), 4);
        for (facing, turn) in [("north", 90), ("south", 90), ("west", 0), ("east", 0)] {
            let model = &state["variants"][format!("facing={facing}")];
            assert_eq!(model["model"], "create:block/rotation_speed_controller/block");
            assert_eq!(model["y"], turn);
            assert_eq!(declaration["shapes"]["values"][facing], json!([
                [0.0, 0.0, 0.0, 1.0, 0.25, 1.0],
                [0.0625, 0.0625, 0.0625, 0.9375, 0.8125, 0.9375],
                [0.0, 0.5, 0.0, 1.0, 0.875, 1.0],
            ]));
        }
        assert!(!TURNING.contains(&"rotation_speed_controller"));
        fix(&dir).unwrap();
        assert_eq!(read(&state_file), Some(state));
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn blaze_burners_keep_the_fetched_models_with_heat_states_and_cage_shapes() {
        let dir = scratch("burner");
        let states = dir.join("assets/create/blockstates");
        let cage = json!({"variants": {"": {"model": "create:block/blaze_burner/block"}}});
        write(&states.join("blaze_burner.json"), &cage).unwrap();
        write(
            &states.join("lit_blaze_burner.json"),
            &json!({"variants": {
                "flame_type=regular": {"model": "create:block/blaze_burner/block_with_fire"},
                "flame_type=soul": {"model": "create:block/blaze_burner/block_with_soul_fire"}}}),
        )
        .unwrap();
        let loot = dir.join("data/create/loot_table/blocks/blaze_burner.json");
        let conditions: Vec<Value> = ["none", "smouldering", "fading", "kindled", "seething"]
            .into_iter()
            .map(|heat| {
                json!({"condition": "minecraft:block_state_property", "block": "create:blaze_burner",
                    "properties": {"blaze": heat}})
            })
            .collect();
        write(&loot, &json!({"pools": [{"entries": [{"conditions": conditions}]}]})).unwrap();
        fix(&dir).unwrap();
        let declarations = dir.join("data/create/horizon/block");
        for (block, initial) in [("blaze_burner", "1"), ("lit_blaze_burner", "0")] {
            let d = read(&declarations.join(format!("{block}.json"))).unwrap();
            assert_eq!(d["template"], "minecraft:respawn_anchor");
            assert_eq!(d["default_state"]["charges"], initial);
            assert_eq!(d["item"], block == "blaze_burner");
            assert_eq!(d["properties"]["hardness"], 3.0);
            assert_eq!(d["properties"]["explosion_resistance"], 6.0);
            assert_eq!(d["shapes"]["property"], "charges");
            for charge in ["0", "1", "2", "3", "4"] {
                assert_eq!(
                    d["shapes"]["values"][charge],
                    json!([[0.0625, 0.0, 0.0625, 0.9375, 0.875, 0.9375]])
                );
            }
        }
        assert_eq!(read(&states.join("blaze_burner.json")), Some(cage));
        let lit = read(&states.join("lit_blaze_burner.json")).unwrap();
        assert_eq!(
            lit["variants"]["charges=0"]["model"],
            "create:block/blaze_burner/block_with_fire"
        );
        assert_eq!(
            lit["variants"]["charges=1"]["model"],
            "create:block/blaze_burner/block_with_soul_fire"
        );
        assert_eq!(lit["variants"].as_object().unwrap().len(), 5);
        let remapped = read(&loot).unwrap();
        let conditions = remapped["pools"][0]["entries"][0]["conditions"].as_array().unwrap();
        for (condition, charge) in conditions.iter().zip(["0", "1", "2", "3", "4"]) {
            assert_eq!(condition["properties"]["charges"], charge);
            assert!(condition["properties"].get("blaze").is_none());
        }
        fix(&dir).unwrap();
        assert_eq!(read(&states.join("lit_blaze_burner.json")), Some(lit));
        assert_eq!(read(&loot), Some(remapped));
        let _ = fs::remove_dir_all(&dir);
    }
}
