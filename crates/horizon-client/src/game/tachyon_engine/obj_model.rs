//! Models in Wavefront OBJ, as NeoForge's `neoforge:obj` loader reads them
//! for mods (Create's water wheel, crushing wheel, blaze burner...): a JSON
//! model naming the `.obj` file (`"loader": "neoforge:obj"`, `"model"`,
//! `"flip_v"`), whose `.mtl` materials name the JSON's textures
//! (`map_Kd #plates`). Read from a ported mod's own folder
//! (`horizon-mods/<namespace>`) into the quads the item renderer draws, for
//! the parts Kinetica's engine turns. Positions are in block units, as the
//! files have them; a triangle is a quad with its last corner twice, a
//! larger face a fan of them.
use horizon_assets::bake::ModelQuad;
use horizon_assets::item_bake::BakedItemModel;
use horizon_assets::model::{Direction, Material};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// The quads of model `id` (`create:block/water_wheel/wheel`) when it is an
/// OBJ model of a mod under `mods`; None for any other.
pub(super) fn bake(mods: &Path, id: &str) -> Option<BakedItemModel> {
    let json = read_json(&model_file(mods, id)?)?;
    if json["loader"].as_str() != Some("neoforge:obj") {
        return None;
    }
    let obj_path = asset_file(mods, json["model"].as_str()?)?;
    let obj = std::fs::read_to_string(&obj_path).ok()?;
    let textures = textures(mods, &json);
    let flip_v = json["flip_v"].as_bool().unwrap_or(false);
    let mtl = (obj.lines())
        .find_map(|l| l.trim().strip_prefix("mtllib "))
        .and_then(|name| std::fs::read_to_string(obj_path.with_file_name(name.trim())).ok())
        .unwrap_or_default();
    let quads = parse(&obj, &mtl, &textures, flip_v);
    if quads.is_empty() {
        return None;
    }
    let particle = textures.get("particle").cloned().unwrap_or_default();
    Some(BakedItemModel {
        quads,
        uses_block_light: true,
        particle: Material {
            sprite: particle,
            force_translucent: false,
        },
        transforms: Default::default(),
    })
}

/// A model's JSON file: `ns:path` in `mods/ns/assets/ns/models/path.json`.
fn model_file(mods: &Path, id: &str) -> Option<PathBuf> {
    let (ns, path) = id.split_once(':')?;
    Some(mods.join(ns).join("assets").join(ns).join("models").join(format!("{path}.json")))
}

/// Any asset file: `ns:path` in `mods/ns/assets/ns/path`.
fn asset_file(mods: &Path, id: &str) -> Option<PathBuf> {
    let (ns, path) = id.split_once(':')?;
    if path.split('/').any(|p| p.is_empty() || p == "..") {
        return None;
    }
    Some(mods.join(ns).join("assets").join(ns).join(path))
}

fn read_json(file: &Path) -> Option<Value> {
    serde_json::from_slice(&std::fs::read(file).ok()?).ok()
}

/// A model's textures with its parents' under them (a parent outside the
/// mods, a vanilla one, adds none), `#key` references followed, ids given
/// their namespace.
fn textures(mods: &Path, json: &Value) -> HashMap<String, String> {
    let mut chain = vec![json.clone()];
    while chain.len() < 16 {
        let Some(parent) = chain.last().and_then(|j| j["parent"].as_str()) else {
            break;
        };
        let Some(parent) = (parent.contains(':') && !parent.starts_with("minecraft:"))
            .then(|| model_file(mods, parent))
            .flatten()
            .and_then(|f| read_json(&f))
        else {
            break;
        };
        chain.push(parent);
    }
    let mut raw: HashMap<String, String> = HashMap::new();
    for model in chain.iter().rev() {
        if let Some(map) = model["textures"].as_object() {
            for (k, v) in map {
                if let Some(v) = v.as_str() {
                    raw.insert(k.clone(), v.to_owned());
                }
            }
        }
    }
    let mut out = HashMap::new();
    for k in raw.keys() {
        let v = resolve(&raw, &raw[k]);
        if !v.is_empty() && !v.starts_with('#') {
            out.insert(k.clone(), namespaced(&v));
        }
    }
    out
}

/// A texture value with its `#key` references followed (a few deep).
fn resolve(raw: &HashMap<String, String>, start: &str) -> String {
    let mut v = start;
    for _ in 0..8 {
        match v.strip_prefix('#') {
            Some(key) => v = raw.get(key).map_or("", String::as_str),
            None => break,
        }
    }
    v.to_owned()
}

fn namespaced(id: &str) -> String {
    if id.contains(':') {
        id.to_owned()
    } else {
        format!("minecraft:{id}")
    }
}

/// The quads of an OBJ file, its materials' textures by `mtl` and the
/// model's `textures`; `flip_v` turns its texture coordinates upside down.
fn parse(obj: &str, mtl: &str, textures: &HashMap<String, String>, flip_v: bool) -> Vec<ModelQuad> {
    // Each material's sprite: `map_Kd #key` names a texture of the model.
    let mut sprites: HashMap<String, String> = HashMap::new();
    let mut current: Option<String> = None;
    for line in mtl.lines() {
        let line = line.trim();
        if let Some(name) = line.strip_prefix("newmtl ") {
            current = Some(name.trim().to_owned());
        } else if let (Some(map), Some(name)) = (line.strip_prefix("map_Kd "), current.as_ref()) {
            let map = map.trim();
            let sprite = match map.strip_prefix('#') {
                Some(key) => textures.get(key).cloned(),
                None => Some(namespaced(map)),
            };
            if let Some(sprite) = sprite {
                sprites.insert(name.clone(), sprite);
            }
        }
    }
    let missing = || "minecraft:missingno".to_owned();
    let mut positions: Vec<[f32; 3]> = Vec::new();
    let mut uvs: Vec<[f32; 2]> = Vec::new();
    let mut sprite = missing();
    let mut quads = Vec::new();
    for line in obj.lines() {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("v") => {
                let v: Vec<f32> = words.take(3).filter_map(|w| w.parse().ok()).collect();
                if let [x, y, z] = v[..] {
                    positions.push([x, y, z]);
                }
            }
            Some("vt") => {
                let v: Vec<f32> = words.take(2).filter_map(|w| w.parse().ok()).collect();
                if let [u, v] = v[..] {
                    uvs.push([u, if flip_v { 1.0 - v } else { v }]);
                }
            }
            Some("usemtl") => {
                let name = words.next().unwrap_or("");
                sprite = sprites.get(name).cloned().unwrap_or_else(missing);
            }
            Some("f") => {
                let corners: Vec<([f32; 3], [f32; 2])> = words
                    .filter_map(|w| {
                        let mut ids = w.split('/');
                        let p = index(ids.next()?, positions.len())?;
                        let t = ids.next().and_then(|t| index(t, uvs.len()));
                        Some((positions[p], t.map_or([0.0, 0.0], |t| uvs[t])))
                    })
                    .collect();
                let material = Material {
                    sprite: sprite.clone(),
                    force_translucent: false,
                };
                match corners.len() {
                    3 | 4 => quads.extend(quad(&corners, &material)),
                    n if n > 4 => {
                        for i in 1..n - 1 {
                            quads.extend(quad(&[corners[0], corners[i], corners[i + 1]], &material));
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
    quads
}

/// An OBJ index (from 1, or back from the end when negative) into a list of
/// `len`.
fn index(word: &str, len: usize) -> Option<usize> {
    let i: i64 = word.parse().ok()?;
    let i = if i < 0 { len as i64 + i } else { i - 1 };
    (0..len as i64).contains(&i).then_some(i as usize)
}

/// A quad of three or four corners (a triangle's last twice), facing the
/// way its normal points most.
fn quad(corners: &[([f32; 3], [f32; 2])], material: &Material) -> Option<ModelQuad> {
    let at = |i: usize| corners[i.min(corners.len() - 1)];
    let (p, t): (Vec<[f32; 3]>, Vec<[f32; 2]>) = (0..4).map(at).unzip();
    let sub = |a: [f32; 3], b: [f32; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let (a, b) = (sub(p[1], p[0]), sub(p[2], p[0]));
    let n = [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]];
    let facing = facing(n)?;
    Some(ModelQuad {
        positions: [p[0], p[1], p[2], p[3]],
        uv: [t[0], t[1], t[2], t[3]],
        facing,
        cull: None,
        material: material.clone(),
        tint_index: -1,
        shade_direction_override: None,
        light_emission: 0,
        light: [0, 0],
        color: None,
        ambient_occlusion: None,
        render_type: None,
        shade: true,
    })
}

/// The direction a normal points most along; None for no normal (a face of
/// no area).
fn facing(n: [f32; 3]) -> Option<Direction> {
    let [x, y, z] = n;
    let (ax, ay, az) = (x.abs(), y.abs(), z.abs());
    if ax.max(ay).max(az) <= f32::EPSILON {
        return None;
    }
    Some(if ay >= ax && ay >= az {
        if y > 0.0 { Direction::Up } else { Direction::Down }
    } else if ax >= az {
        if x > 0.0 { Direction::East } else { Direction::West }
    } else if z > 0.0 {
        Direction::South
    } else {
        Direction::North
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_obj_face_becomes_a_quad_with_its_materials_sprite() {
        let mtl = "newmtl plate\nmap_Kd #plates\nnewmtl raw\nmap_Kd block/stone\n";
        let obj = "mtllib x.mtl\nv 0 0 0\nv 1 0 0\nv 1 1 0\nv 0 1 0\nv 0.5 1.5 0\n\
                   vt 0 0\nvt 1 0\nvt 1 1\nvt 0 1\nusemtl plate\nf 1/1/1 2/2/1 3/3/1 4/4/1\n\
                   usemtl raw\nf 1/1 2/2 3/3\nf -5/1 -4/2 -3/3 -1/4 -2/4\n";
        let textures = HashMap::from([("plates".to_owned(), "create:block/plates".to_owned())]);
        let quads = parse(obj, mtl, &textures, true);
        // A quad, a triangle, a pentagon as three triangles.
        assert_eq!(quads.len(), 5);
        assert_eq!(quads[0].material.sprite, "create:block/plates");
        assert_eq!(quads[0].facing, Direction::South);
        // flip_v: 1 - v.
        assert_eq!(quads[0].uv[0], [0.0, 1.0]);
        assert_eq!(quads[1].material.sprite, "minecraft:block/stone");
        assert_eq!(quads[1].positions[3], quads[1].positions[2]);
    }

    #[test]
    fn a_models_textures_follow_its_parents_and_references() {
        let dir = std::env::temp_dir().join(format!("tachyon-obj-{}", std::process::id()));
        let models = dir.join("m/assets/m/models/block");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(
            models.join("textures.json"),
            r##"{"parent": "block/block", "textures": {"planks": "block/oak_planks", "particle": "#planks"}}"##,
        )
        .unwrap();
        std::fs::write(
            models.join("wheel.json"),
            r#"{"parent": "m:block/textures", "loader": "neoforge:obj", "flip_v": true, "model": "m:models/block/wheel.obj"}"#,
        )
        .unwrap();
        std::fs::write(models.join("wheel.mtl"), "newmtl p\nmap_Kd #planks\n").unwrap();
        std::fs::write(
            models.join("wheel.obj"),
            "mtllib wheel.mtl\nv 0 0 0\nv 1 0 0\nv 1 0 1\nvt 0 0\nusemtl p\nf 1/1 3/1 2/1\n",
        )
        .unwrap();
        let baked = bake(&dir, "m:block/wheel").unwrap();
        assert_eq!(baked.quads.len(), 1);
        assert_eq!(baked.quads[0].material.sprite, "minecraft:block/oak_planks");
        assert_eq!(baked.quads[0].facing, Direction::Up);
        assert_eq!(baked.particle.sprite, "minecraft:block/oak_planks");
        // A JSON model of elements is not one of these.
        std::fs::write(models.join("cube.json"), r#"{"elements": []}"#).unwrap();
        assert!(bake(&dir, "m:block/cube").is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
