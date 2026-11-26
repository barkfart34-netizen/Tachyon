//! Pure-Rust runtime asset staging for the bundled `Create-Horizon` (`mod-create`)
//! native mod from the player's own `create-1.21.1-6.0.10.jar`.
//!
//! Mirrors `Create-Horizon/tools/{build_content,goggles_assets,ponder_assets}.py`
//! so neither the repository nor a Tachyon release ships Create's copyrighted
//! textures, models, blockstates, sounds or Ponder schematics.
use horizon_core::nbt::{self, Compound, Limits, Tag};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

const PREFIX: &str = "assets/create/";

#[derive(Deserialize)]
struct AssetsSpec {
    models: Vec<String>,
    base_blocks: HashSet<String>,
    door_blocks: HashSet<String>,
    static_blockstates: HashMap<String, Value>,
    blocks: BTreeMap<String, BlockDeclSpec>,
    custom_models: HashSet<String>,
    goggles: GogglesSpec,
}

#[derive(Deserialize)]
struct BlockDeclSpec {
    template: Option<String>,
    #[serde(default)]
    state_properties: HashSet<String>,
}

#[derive(Deserialize)]
struct GogglesSpec {
    prefixes: Vec<String>,
    keys: Vec<String>,
    required: Vec<String>,
}

/// Stages `Create-Horizon` declarations from `decl_dir` together with
/// Create's artwork, sounds, goggles lang and Ponder assets extracted from
/// `jar_path` into `dest_dir` (`horizon-mods/create`).
pub fn stage_create_horizon(
    decl_dir: &Path,
    jar_path: &Path,
    dest_dir: &Path,
) -> Result<(), String> {
    if !decl_dir.is_dir() {
        return Err(format!(
            "Create declarations directory missing: {}",
            decl_dir.display()
        ));
    }
    let spec: AssetsSpec = read_json(&decl_dir.join("assets_spec.json"))?;
    let manifest_src = decl_dir.join("manifest.toml.template");
    let manifest = fs::read_to_string(&manifest_src)
        .map_err(|e| format!("{}: {e}", manifest_src.display()))?;

    let jar_file = fs::File::open(jar_path)
        .map_err(|e| format!("{}: {e}", jar_path.display()))?;
    let mut jar = ZipArchive::new(jar_file).map_err(|e| format!("{}: {e}", jar_path.display()))?;
    let entries = index_entries(&jar)?;

    let lang_bytes = read_entry(&mut jar, &format!("{PREFIX}lang/en_us.json"))?;
    let en_us: HashMap<String, String> = serde_json::from_slice(&lang_bytes)
        .map_err(|e| format!("assets/create/lang/en_us.json: {e}"))?;

    fs::create_dir_all(dest_dir).map_err(|e| format!("{}: {e}", dest_dir.display()))?;
    for dir_name in ["assets", "data"] {
        let target = dest_dir.join(dir_name);
        if target.exists() {
            fs::remove_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
        }
        let src = decl_dir.join(dir_name);
        if src.is_dir() {
            copy_dir_all(&src, &target)?;
        }
    }
    // Remove legacy v1 stamp if migrating from the old tachyon_engine port.
    let _ = fs::remove_file(dest_dir.join(".tachyon-create"));
    fs::write(dest_dir.join("horizon-mod.toml"), manifest)
        .map_err(|e| format!("{}: {e}", dest_dir.join("horizon-mod.toml").display()))?;

    import_goggles(&en_us, &spec.goggles, dest_dir)?;
    let ponder_models = import_ponder(decl_dir, &mut jar, &entries, &en_us, dest_dir)?;
    import_assets(&spec, &mut jar, &entries, dest_dir, &ponder_models)?;

    fs::write(
        dest_dir.join(horizon_client::tachyon_native_mods::CREATE_HORIZON_STAMP),
        "create-horizon d58c0538a6 / create 6.0.10\n",
    )
    .map_err(|e| format!("write stamp: {e}"))?;
    Ok(())
}

fn index_entries<R: Read + std::io::Seek>(jar: &ZipArchive<R>) -> Result<HashSet<String>, String> {
    let mut out = HashSet::with_capacity(jar.len());
    for res in jar.file_names() {
        let name = res.map_err(|e| e.to_string())?;
        out.insert(name.into_owned());
    }
    Ok(out)
}

fn read_entry<R: Read + std::io::Seek>(
    jar: &mut ZipArchive<R>,
    name: &str,
) -> Result<Vec<u8>, String> {
    let mut entry = jar
        .by_name(name)
        .map_err(|e| format!("Create jar lacks {name}: {e}"))?;
    let mut out = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut out)
        .map_err(|e| format!("{name}: {e}"))?;
    Ok(out)
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn write_json_compact(path: &Path, value: &Value) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|e| format!("{}: {e}", path.display()))?;
    write_bytes(path, &bytes)
}

fn copy_jar_asset<R: Read + std::io::Seek>(
    jar: &mut ZipArchive<R>,
    dest_dir: &Path,
    rel: &str,
) -> Result<Vec<u8>, String> {
    let bytes = read_entry(jar, &format!("{PREFIX}{rel}"))?;
    write_bytes(&dest_dir.join(PREFIX).join(rel), &bytes)?;
    Ok(bytes)
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for entry in fs::read_dir(src).map_err(|e| format!("{}: {e}", src.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        let to = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&entry.path(), &to)?;
        } else if ty.is_file() {
            fs::copy(entry.path(), &to).map_err(|e| format!("{}: {e}", to.display()))?;
        }
    }
    Ok(())
}

// --- goggles language subset ---------------------------------------------------------------------

fn import_goggles(
    en_us: &HashMap<String, String>,
    spec: &GogglesSpec,
    dest_dir: &Path,
) -> Result<(), String> {
    let mut chosen = BTreeMap::new();
    for (k, v) in en_us {
        if spec.prefixes.iter().any(|p| k.starts_with(p)) || spec.keys.iter().any(|exact| exact == k)
        {
            chosen.insert(k.clone(), v.clone());
        }
    }
    for req in &spec.required {
        if !chosen.contains_key(req) {
            return Err(format!("Create's language file lacks {req}"));
        }
    }
    let bytes = serde_json::to_vec_pretty(&chosen).map_err(|e| e.to_string())?;
    write_bytes(&dest_dir.join("assets/create/goggles/lang.json"), &bytes)
}

// --- blockstates, models, sounds, textures -------------------------------------------------------

fn find_models(value: &Value, found: &mut HashSet<String>) {
    match value {
        Value::Object(map) => {
            for (k, v) in map {
                if k == "model"
                    && let Some(s) = v.as_str()
                {
                    if let Some(stripped) = s.strip_prefix("create:") {
                        found.insert(stripped.to_owned());
                    }
                } else {
                    find_models(v, found);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                find_models(item, found);
            }
        }
        _ => {}
    }
}

fn door_blockstate(state: &Value) -> Value {
    let mut out = Map::new();
    if let Some(variants) = state.get("variants").and_then(Value::as_object) {
        for (k, v) in variants {
            if !k.ends_with("visible=false") {
                let new_key = k.strip_suffix(",visible=true").unwrap_or(k);
                out.insert(new_key.to_owned(), v.clone());
            }
        }
    }
    json!({"variants": out})
}

fn fit_blockstate(state: &Value, declared: &HashSet<String>) -> Value {
    let Some(variants) = state.get("variants").and_then(Value::as_object) else {
        return state.clone();
    };
    let mut seen: HashMap<&str, Vec<&str>> = HashMap::new();
    for key in variants.keys() {
        for pair in key.split(',').filter(|s| !s.is_empty()) {
            if let Some((name, val)) = pair.split_once('=') {
                seen.entry(name).or_default().push(val);
            }
        }
    }
    let mut resting: HashMap<&str, &str> = HashMap::new();
    for (name, values) in seen {
        if !declared.contains(name) {
            let chosen = if values.contains(&"false") {
                "false"
            } else {
                values[0]
            };
            resting.insert(name, chosen);
        }
    }
    let mut out = Map::new();
    for (key, value) in variants {
        let pairs: Vec<(&str, &str)> = key
            .split(',')
            .filter(|s| !s.is_empty())
            .filter_map(|p| p.split_once('='))
            .collect();
        if pairs
            .iter()
            .any(|(name, val)| resting.get(name).is_some_and(|r| r != val))
        {
            continue;
        }
        let filtered: Vec<String> = pairs
            .into_iter()
            .filter(|(name, _)| declared.contains(*name))
            .map(|(name, val)| format!("{name}={val}"))
            .collect();
        out.insert(filtered.join(","), value.clone());
    }
    json!({"variants": out})
}

fn collect_json_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
        let entry = entry.map_err(|e| e.to_string())?;
        let p = entry.path();
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            collect_json_files(&p, out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some("json") {
            out.push(p);
        }
    }
    Ok(())
}

fn collect_textures_from_map(obj: &Map<String, Value>, textures: &mut HashSet<String>) {
    if let Some(tex) = obj.get("textures").and_then(Value::as_object) {
        for sprite in tex.values().filter_map(Value::as_str) {
            if let Some(s) = sprite.strip_prefix("create:") {
                textures.insert(s.to_owned());
            }
        }
    }
}

fn import_assets<R: Read + std::io::Seek>(
    spec: &AssetsSpec,
    jar: &mut ZipArchive<R>,
    entries: &HashSet<String>,
    dest_dir: &Path,
    ponder_models: &HashSet<String>,
) -> Result<(), String> {
    let mut models: HashSet<String> = spec.models.iter().cloned().collect();
    models.extend(ponder_models.iter().cloned());

    let mut item_files = Vec::new();
    collect_json_files(&dest_dir.join(PREFIX).join("items"), &mut item_files)?;
    for path in item_files {
        let val: Value = read_json(&path)?;
        find_models(&val, &mut models);
    }

    for (name, block_decl) in &spec.blocks {
        let rel = format!("blockstates/{name}.json");
        if let Some(custom) = spec.static_blockstates.get(name) {
            write_json_compact(&dest_dir.join(PREFIX).join(&rel), custom)?;
            find_models(custom, &mut models);
            continue;
        }
        let raw = copy_jar_asset(jar, dest_dir, &rel)?;
        let mut state: Value =
            serde_json::from_slice(&raw).map_err(|e| format!("{rel}: {e}"))?;
        let mut rewrite = false;

        if spec.door_blocks.contains(name) {
            state = door_blockstate(&state);
            rewrite = true;
        } else if name == "belt" {
            let mut out = Map::new();
            if let Some(variants) = state.get("variants").and_then(Value::as_object) {
                for (k, v) in variants {
                    if k.ends_with("waterlogged=false") {
                        out.insert(k.replace(",waterlogged=false", ""), v.clone());
                    }
                }
            }
            state = json!({"variants": out});
            rewrite = true;
        } else if name == "hand_crank" {
            let mut out = Map::new();
            if let Some(variants) = state.get("variants").and_then(Value::as_object) {
                for (k, v) in variants {
                    if k.ends_with("waterlogged=false") {
                        let first = k.split(',').next().unwrap_or(k);
                        out.insert(first.to_owned(), v.clone());
                    }
                }
            }
            state = json!({"variants": out});
            rewrite = true;
        } else if name == "water_wheel" {
            let mut out = Map::new();
            if let Some(variants) = state.get("variants").and_then(Value::as_object) {
                for (axis, face) in [("x", "east"), ("y", "up"), ("z", "south")] {
                    if let Some(v) = variants.get(&format!("facing={face}")) {
                        out.insert(format!("axis={axis}"), v.clone());
                    }
                }
            }
            state = json!({"variants": out});
            rewrite = true;
        } else if !spec.base_blocks.contains(name)
            && block_decl.template.as_deref() == Some("minecraft:glass")
        {
            state = fit_blockstate(&state, &block_decl.state_properties);
            rewrite = true;
        }

        if rewrite {
            write_json_compact(&dest_dir.join(PREFIX).join(&rel), &state)?;
        }
        find_models(&state, &mut models);
    }

    let mut textures = HashSet::new();
    for custom in &spec.custom_models {
        let p = dest_dir.join(PREFIX).join(format!("models/{custom}.json"));
        if p.is_file() {
            let val: Value = read_json(&p)?;
            if let Some(obj) = val.as_object() {
                collect_textures_from_map(obj, &mut textures);
            }
        }
    }

    let mut queue: VecDeque<String> = models.into_iter().collect();
    let mut copied = HashSet::new();
    while let Some(name) = queue.pop_front() {
        if !copied.insert(name.clone()) || spec.custom_models.contains(&name) {
            continue;
        }
        let raw = copy_jar_asset(jar, dest_dir, &format!("models/{name}.json"))?;
        let model: Value =
            serde_json::from_slice(&raw).map_err(|e| format!("models/{name}.json: {e}"))?;
        let Some(obj) = model.as_object() else {
            continue;
        };
        if obj.get("loader").and_then(Value::as_str) == Some("neoforge:obj")
            && let Some(mesh_path) = obj.get("model").and_then(Value::as_str)
        {
            let mesh = mesh_path.strip_prefix("create:").unwrap_or(mesh_path);
            let mesh_bytes = copy_jar_asset(jar, dest_dir, mesh)?;
            let mesh_text = String::from_utf8_lossy(&mesh_bytes);
            let parent_dir = Path::new(mesh).parent();
            for line in mesh_text.lines() {
                if let Some(mtl) = line.strip_prefix("mtllib ").and_then(|rest| rest.split_whitespace().next()) {
                    let rel_mtl = match parent_dir {
                        Some(p) if !p.as_os_str().is_empty() => {
                            format!("{}/{mtl}", p.to_string_lossy().replace('\\', "/"))
                        }
                        _ => mtl.to_owned(),
                    };
                    if entries.contains(&format!("{PREFIX}{rel_mtl}")) {
                        copy_jar_asset(jar, dest_dir, &rel_mtl)?;
                    }
                }
            }
        }
        if let Some(parent) = obj.get("parent").and_then(Value::as_str)
            && let Some(stripped) = parent.strip_prefix("create:")
        {
            queue.push_back(stripped.to_owned());
        }
        if let Some(overrides) = obj.get("overrides") {
            let mut extra = HashSet::new();
            find_models(overrides, &mut extra);
            queue.extend(extra);
        }
        collect_textures_from_map(obj, &mut textures);
        if let Some(children) = obj.get("children").and_then(Value::as_object) {
            for child in children.values() {
                let Some(cobj) = child.as_object() else {
                    continue;
                };
                if let Some(parent) = cobj.get("parent").and_then(Value::as_str)
                    && let Some(stripped) = parent.strip_prefix("create:")
                {
                    queue.push_back(stripped.to_owned());
                }
                collect_textures_from_map(cobj, &mut textures);
            }
        }
    }

    // Spruce texture patches matching WaterWheelRenderer.generateModel.
    let wheel = dest_dir.join(PREFIX).join("models/block/water_wheel/wheel.json");
    if wheel.is_file() {
        let mut val: Value = read_json(&wheel)?;
        val["textures"] = json!({"log": "block/spruce_log", "planks": "block/spruce_planks"});
        write_json_compact(&wheel, &val)?;
    }
    for part in ["block", "block_extension"] {
        let large = dest_dir
            .join(PREFIX)
            .join(format!("models/block/large_water_wheel/{part}.json"));
        if large.is_file() {
            let mut val: Value = read_json(&large)?;
            val["textures"] = json!({
                "log": "block/spruce_log",
                "planks": "block/spruce_planks",
                "log_top": "block/spruce_log_top",
            });
            write_json_compact(&large, &val)?;
        }
    }

    // Sounds.json + referenced .ogg files.
    let sounds_raw = copy_jar_asset(jar, dest_dir, "sounds.json")?;
    let sounds: Value =
        serde_json::from_slice(&sounds_raw).map_err(|e| format!("sounds.json: {e}"))?;
    let mut sound_files = HashSet::new();
    if let Some(map) = sounds.as_object() {
        for event in map.values() {
            if let Some(list) = event.get("sounds").and_then(Value::as_array) {
                for sound in list {
                    let (name, kind) = match sound {
                        Value::String(s) => (s.as_str(), "file"),
                        Value::Object(o) => (
                            o.get("name").and_then(Value::as_str).unwrap_or(""),
                            o.get("type").and_then(Value::as_str).unwrap_or("file"),
                        ),
                        _ => ("", ""),
                    };
                    if kind == "file"
                        && let Some(stripped) = name.strip_prefix("create:")
                    {
                        sound_files.insert(format!("sounds/{stripped}.ogg"));
                    }
                }
            }
        }
    }
    for rel in sound_files {
        copy_jar_asset(jar, dest_dir, &rel)?;
    }

    // Textures + optional .mcmeta.
    let mut sorted_textures: Vec<String> = textures.into_iter().collect();
    sorted_textures.sort();
    for sprite in sorted_textures {
        let rel = format!("textures/{sprite}.png");
        copy_jar_asset(jar, dest_dir, &rel)?;
        let mcmeta = format!("{rel}.mcmeta");
        if entries.contains(&format!("{PREFIX}{mcmeta}")) {
            copy_jar_asset(jar, dest_dir, &mcmeta)?;
        }
    }
    Ok(())
}

// --- Ponder scene pack ---------------------------------------------------------------------------

struct ModelResolver<'a, R: Read + std::io::Seek> {
    jar: &'a mut ZipArchive<R>,
    entries: &'a HashSet<String>,
    supported_cache: HashMap<String, bool>,
    models_cache: HashMap<(String, Vec<(String, String)>), Vec<(String, i64, i64)>>,
}

impl<'a, R: Read + std::io::Seek> ModelResolver<'a, R> {
    fn json_asset(&mut self, kind: &str, name: &str) -> Value {
        let path = format!("{PREFIX}{kind}/{name}.json");
        if !self.entries.contains(&path) {
            return Value::Object(Map::new());
        }
        read_entry(self.jar, &path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_else(|| Value::Object(Map::new()))
    }

    fn supported(&mut self, name: &str) -> bool {
        if let Some(&res) = self.supported_cache.get(name) {
            return res;
        }
        let model = self.json_asset("models", name);
        let res = if !model.as_object().is_some_and(|o| !o.is_empty()) || model.get("loader").is_some()
        {
            false
        } else if let Some(parent) = model.get("parent").and_then(Value::as_str)
            && let Some(stripped) = parent.strip_prefix("create:")
        {
            self.supported(stripped)
        } else {
            true
        };
        self.supported_cache.insert(name.to_owned(), res);
        res
    }

    fn check(conditions: &Map<String, Value>, props: &BTreeMap<String, String>) -> bool {
        for (k, v) in conditions {
            if k == "OR" {
                let any = v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_object)
                    .any(|sub| Self::check(sub, props));
                if !any {
                    return false;
                }
            } else if k == "AND" {
                let all = v
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_object)
                    .all(|sub| Self::check(sub, props));
                if !all {
                    return false;
                }
            } else {
                let cur = props.get(k).map(String::as_str).unwrap_or("").to_lowercase();
                let wanted = match v {
                    Value::String(s) => s.to_lowercase(),
                    Value::Bool(b) => b.to_string(),
                    Value::Number(n) => n.to_string(),
                    _ => String::new(),
                };
                if !wanted.split('|').any(|opt| opt == cur) {
                    return false;
                }
            }
        }
        true
    }

    fn models_for(
        &mut self,
        id: &str,
        props: &BTreeMap<String, String>,
    ) -> Vec<(String, i64, i64)> {
        let Some(short) = id.strip_prefix("create:") else {
            return Vec::new();
        };
        let key_props: Vec<(String, String)> =
            props.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        let cache_key = (id.to_owned(), key_props);
        if let Some(cached) = self.models_cache.get(&cache_key) {
            return cached.clone();
        }
        let asset = self.json_asset("blockstates", short);
        let mut candidates: Vec<Value> = Vec::new();
        if let Some(variants) = asset.get("variants").and_then(Value::as_object) {
            let mut sorted: Vec<(&String, &Value)> = variants.iter().collect();
            sorted.sort_by(|a, b| {
                let ca = a.0.bytes().filter(|&c| c == b'=').count();
                let cb = b.0.bytes().filter(|&c| c == b'=').count();
                cb.cmp(&ca)
            });
            for (spec_str, model) in sorted {
                let mut cond = Map::new();
                for pair in spec_str.split(',').filter(|s| !s.is_empty()) {
                    if let Some((k, v)) = pair.split_once('=') {
                        cond.insert(k.to_owned(), Value::String(v.to_owned()));
                    }
                }
                if Self::check(&cond, props) {
                    candidates.push(model.clone());
                    break;
                }
            }
        }
        if let Some(multi) = asset.get("multipart").and_then(Value::as_array) {
            for part in multi {
                let when_map = part.get("when").and_then(Value::as_object);
                let ok = when_map.is_none_or(|m| Self::check(m, props));
                if ok && let Some(apply) = part.get("apply") {
                    candidates.push(apply.clone());
                }
            }
        }
        let mut out = Vec::new();
        for cand in candidates {
            let entry = if let Some(arr) = cand.as_array() {
                arr.first().cloned().unwrap_or(Value::Null)
            } else {
                cand
            };
            let Some(mname) = entry.get("model").and_then(Value::as_str) else {
                continue;
            };
            if let Some(stripped) = mname.strip_prefix("create:")
                && self.supported(stripped)
            {
                let x = entry.get("x").and_then(Value::as_i64).unwrap_or(0);
                let y = entry.get("y").and_then(Value::as_i64).unwrap_or(0);
                out.push((stripped.to_owned(), x, y));
            }
        }
        self.models_cache.insert(cache_key, out.clone());
        out
    }
}

fn read_nbt_gz(raw: &[u8]) -> Result<Compound, String> {
    let mut decoder = flate2::read::GzDecoder::new(raw);
    let mut decompressed = Vec::new();
    decoder
        .read_to_end(&mut decompressed)
        .map_err(|e| format!("gunzip nbt: {e}"))?;
    let (named, _) =
        nbt::read_named(&decompressed, Limits::templates()).map_err(|e| format!("nbt: {e}"))?;
    match named.tag {
        Tag::Compound(c) => Ok(c),
        other => Err(format!("expected root compound, got {:?}", other.id())),
    }
}

fn tag_i32(tag: Option<&Tag>) -> Option<i32> {
    match tag? {
        Tag::Byte(v) => Some(i32::from(*v)),
        Tag::Short(v) => Some(i32::from(*v)),
        Tag::Int(v) => Some(*v),
        Tag::Long(v) => i32::try_from(*v).ok(),
        _ => None,
    }
}

fn tag_number_value(tag: &Tag) -> Value {
    match tag {
        Tag::Byte(v) => json!(*v),
        Tag::Short(v) => json!(*v),
        Tag::Int(v) => json!(*v),
        Tag::Long(v) => json!(*v),
        Tag::Float(bits) => json!(f32::from_bits(*bits)),
        Tag::Double(bits) => json!(f64::from_bits(*bits)),
        _ => json!(0),
    }
}

fn nbt_get<'a>(c: &'a Compound, key: &str) -> Option<&'a Tag> {
    c.get(&nbt::JavaString::from(key))
}

fn nbt_str(c: &Compound, key: &str) -> Option<String> {
    match nbt_get(c, key)? {
        Tag::String(s) => Some(s.to_string_lossy()),
        _ => None,
    }
}

fn nbt_i32_triple(tag: Option<&Tag>) -> Option<[i32; 3]> {
    let items = match tag? {
        Tag::List(items) if items.len() == 3 => items,
        _ => return None,
    };
    Some([
        tag_i32(Some(&items[0]))?,
        tag_i32(Some(&items[1]))?,
        tag_i32(Some(&items[2]))?,
    ])
}

fn nbt_block_pos(tag: Option<&Tag>) -> Option<[i32; 3]> {
    match tag? {
        Tag::Compound(c) => Some([
            tag_i32(nbt_get(c, "X"))?,
            tag_i32(nbt_get(c, "Y"))?,
            tag_i32(nbt_get(c, "Z"))?,
        ]),
        Tag::IntArray(v) if v.len() == 3 => Some([v[0], v[1], v[2]]),
        Tag::List(items) if items.len() == 3 => Some([
            tag_i32(Some(&items[0]))?,
            tag_i32(Some(&items[1]))?,
            tag_i32(Some(&items[2]))?,
        ]),
        _ => None,
    }
}

type PaletteEntry = (String, BTreeMap<String, String>);
type SchematicBlock = ([i32; 3], usize);

fn parse_ponder_structure(
    root: &Compound,
) -> Result<([i32; 3], Vec<PaletteEntry>, Vec<SchematicBlock>, Vec<([i32; 3], Value)>), String> {
    let size = nbt_i32_triple(nbt_get(root, "size"))
        .ok_or_else(|| "ponder nbt missing size".to_owned())?;
    let mut palette = Vec::new();
    if let Some(Tag::List(items)) = nbt_get(root, "palette") {
        for item in items {
            let Tag::Compound(entry) = item else {
                continue;
            };
            let name = nbt_str(entry, "Name").unwrap_or_default();
            let mut props = BTreeMap::new();
            if let Some(Tag::Compound(pmap)) = nbt_get(entry, "Properties") {
                for (k, v) in pmap {
                    if let Tag::String(sv) = v {
                        props.insert(k.to_string_lossy(), sv.to_string_lossy());
                    }
                }
            }
            palette.push((name, props));
        }
    }
    let mut blocks = Vec::new();
    let mut speeds = Vec::new();
    if let Some(Tag::List(items)) = nbt_get(root, "blocks") {
        for item in items {
            let Tag::Compound(entry) = item else {
                continue;
            };
            let Some(pos) = nbt_i32_triple(nbt_get(entry, "pos")) else {
                continue;
            };
            let Some(state_idx) = tag_i32(nbt_get(entry, "state")).and_then(|i| usize::try_from(i).ok())
            else {
                continue;
            };
            if palette.get(state_idx).is_some_and(|(n, _)| n != "minecraft:air") {
                blocks.push((pos, state_idx));
            }
            if let Some(Tag::Compound(be)) = nbt_get(entry, "nbt")
                && let Some(speed) = nbt_get(be, "Speed")
            {
                let val = tag_number_value(speed);
                if val.as_f64().is_some_and(|f| f != 0.0) {
                    speeds.push((pos, val));
                }
            }
        }
    }
    Ok((size, palette, blocks, speeds))
}

fn parse_arm_points(root: &Compound) -> Vec<Value> {
    let mut arms = Vec::new();
    let Some(Tag::List(items)) = nbt_get(root, "blocks") else {
        return arms;
    };
    for item in items {
        let Tag::Compound(entry) = item else { continue };
        let Some(at) = nbt_i32_triple(nbt_get(entry, "pos")) else {
            continue;
        };
        let Some(Tag::Compound(be)) = nbt_get(entry, "nbt") else {
            continue;
        };
        let Some(Tag::List(points)) = nbt_get(be, "InteractionPoints") else {
            continue;
        };
        if points.is_empty() {
            continue;
        }
        let mut pts = Vec::new();
        for p in points {
            let Tag::Compound(pc) = p else { continue };
            let Some(ty) = nbt_str(pc, "Type") else {
                continue;
            };
            let mode = nbt_str(pc, "Mode").unwrap_or_else(|| "DEPOSIT".to_owned());
            let Some(off) = nbt_block_pos(nbt_get(pc, "Pos")) else {
                continue;
            };
            let abs = [at[0] + off[0], at[1] + off[1], at[2] + off[2]];
            pts.push(json!([ty, mode, abs]));
        }
        arms.push(json!([at, pts]));
    }
    arms
}

fn parse_tank_data(root: &Compound, palette: &[PaletteEntry]) -> Vec<Value> {
    let Some(Tag::List(items)) = nbt_get(root, "blocks") else {
        return Vec::new();
    };
    let mut rows: HashMap<[i32; 3], ([i32; 3], [i32; 3], i32, i32, i32, String, i64)> =
        HashMap::new();
    for item in items {
        let Tag::Compound(entry) = item else { continue };
        let Some(pos) = nbt_i32_triple(nbt_get(entry, "pos")) else {
            continue;
        };
        let Some(state_idx) = tag_i32(nbt_get(entry, "state")).and_then(|i| usize::try_from(i).ok())
        else {
            continue;
        };
        let name = palette.get(state_idx).map(|(n, _)| n.as_str()).unwrap_or("");
        if !matches!(name, "create:fluid_tank" | "create:creative_fluid_tank") {
            continue;
        }
        let Some(Tag::Compound(data)) = nbt_get(entry, "nbt") else {
            continue;
        };
        let mut controller = pos;
        if let (Some(ctrl), Some(known)) = (
            nbt_block_pos(nbt_get(data, "Controller")),
            nbt_block_pos(nbt_get(data, "LastKnownPos")),
        ) {
            controller = [
                pos[0] + ctrl[0] - known[0],
                pos[1] + ctrl[1] - known[1],
                pos[2] + ctrl[2] - known[2],
            ];
        }
        let (fluid, amount) = match nbt_get(data, "TankContent") {
            Some(Tag::Compound(content)) => {
                let f = nbt_str(content, "FluidName")
                    .or_else(|| nbt_str(content, "id"))
                    .unwrap_or_else(|| "minecraft:empty".to_owned());
                let a = match nbt_get(content, "Amount").or_else(|| nbt_get(content, "amount")) {
                    Some(Tag::Long(v)) => *v,
                    other => i64::from(tag_i32(other).unwrap_or(0)),
                };
                (if f == "minecraft:empty" { String::new() } else { f }, a)
            }
            _ => (String::new(), 0),
        };
        let size = tag_i32(nbt_get(data, "Size")).unwrap_or(0);
        let height = tag_i32(nbt_get(data, "Height")).unwrap_or(0);
        let window = tag_i32(nbt_get(data, "Window")).unwrap_or(1);
        rows.insert(pos, (pos, controller, size, height, window, fluid, amount));
    }
    let snapshot = rows.clone();
    let mut out = Vec::with_capacity(rows.len());
    for mut row in rows.into_values() {
        if row.2 == 0
            && let Some(head) = snapshot.get(&row.1)
        {
            row.2 = head.2;
            row.3 = head.3;
            row.4 = head.4;
            row.5 = head.5.clone();
            row.6 = head.6;
        }
        out.push(json!([row.0, row.1, row.2, row.3, row.4, row.5, row.6]));
    }
    out
}

fn parse_board_data(root: &Compound, palette: &[PaletteEntry]) -> Vec<Value> {
    let Some(Tag::List(items)) = nbt_get(root, "blocks") else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for item in items {
        let Tag::Compound(entry) = item else { continue };
        let Some(pos) = nbt_i32_triple(nbt_get(entry, "pos")) else {
            continue;
        };
        let Some(state_idx) = tag_i32(nbt_get(entry, "state")).and_then(|i| usize::try_from(i).ok())
        else {
            continue;
        };
        if palette.get(state_idx).map(|(n, _)| n.as_str()) != Some("create:display_board") {
            continue;
        }
        let Some(Tag::Compound(be)) = nbt_get(entry, "nbt") else {
            continue;
        };
        if tag_i32(nbt_get(be, "Controller")).is_some_and(|c| c != 0) {
            let x = tag_i32(nbt_get(be, "XSize")).unwrap_or(1);
            let y = tag_i32(nbt_get(be, "YSize")).unwrap_or(1);
            out.push(json!([pos, x, y]));
        }
    }
    out
}

fn opposite_dir(v: &str) -> &str {
    match v {
        "north" => "south",
        "south" => "north",
        "east" => "west",
        "west" => "east",
        "up" => "down",
        "down" => "up",
        other => other,
    }
}

fn json_pos(v: &Value) -> Option<[i32; 3]> {
    let a = v.as_array()?;
    if a.len() != 3 {
        return None;
    }
    Some([a[0].as_i64()? as i32, a[1].as_i64()? as i32, a[2].as_i64()? as i32])
}

fn expand_properties<R: Read + std::io::Seek>(
    jar: &mut ZipArchive<R>,
    entries: &HashSet<String>,
    palette: &[PaletteEntry],
    blocks: &[SchematicBlock],
    ops: &[Value],
) -> Vec<Vec<Value>> {
    let mut original: HashMap<[i32; 3], PaletteEntry> = HashMap::new();
    for (pos, idx) in blocks {
        if let Some(p) = palette.get(*idx) {
            original.insert(*pos, p.clone());
        }
    }
    let mut changed: HashMap<[i32; 3], PaletteEntry> = HashMap::new();
    let mut values_cache: HashMap<(String, String), Vec<String>> = HashMap::new();

    let mut prop_values = |jar: &mut ZipArchive<R>, id: &str, key: &str| -> Vec<String> {
        if key == "axis" {
            return vec!["x".into(), "y".into(), "z".into()];
        }
        if key == "power" {
            return (0..16).map(|n| n.to_string()).collect();
        }
        let ck = (id.to_owned(), key.to_owned());
        if let Some(v) = values_cache.get(&ck) {
            return v.clone();
        }
        let mut seen = Vec::new();
        if let Some(short) = id.strip_prefix("create:") {
            let file = format!("{PREFIX}blockstates/{short}.json");
            if entries.contains(&file)
                && let Ok(bytes) = read_entry(jar, &file)
                && let Ok(val) = serde_json::from_slice::<Value>(&bytes)
                && let Some(variants) = val.get("variants").and_then(Value::as_object)
            {
                for spec in variants.keys() {
                    for item in spec.split(',') {
                        if let Some((k, v)) = item.split_once('=')
                            && k == key
                            && !seen.iter().any(|s| s == v)
                        {
                            seen.push(v.to_owned());
                        }
                    }
                }
            }
        }
        if seen.is_empty() {
            seen = if key.ends_with("facing") {
                vec!["north".into(), "east".into(), "south".into(), "west".into()]
            } else {
                vec!["false".into(), "true".into()]
            };
        }
        values_cache.insert(ck, seen.clone());
        seen
    };

    let mut out = Vec::new();
    for raw_op in ops {
        let Some(arr) = raw_op.as_array() else {
            continue;
        };
        if arr.len() < 2 {
            continue;
        }
        let tick = arr[0].clone();
        let Some(op) = arr[1].as_str() else {
            continue;
        };
        if op == "update" && arr.len() >= 5 {
            let Some(positions) = arr[2].as_array() else {
                continue;
            };
            let new_id_opt = arr[3].as_str();
            let Some(specs) = arr[4].as_object() else {
                continue;
            };
            let mut groups: BTreeMap<(String, BTreeMap<String, String>), Vec<[i32; 3]>> =
                BTreeMap::new();
            for pval in positions {
                let Some(spot) = json_pos(pval) else { continue };
                let Some((old_id, old_props)) =
                    changed.get(&spot).or_else(|| original.get(&spot)).cloned()
                else {
                    continue;
                };
                let new_id = new_id_opt.unwrap_or(&old_id).to_owned();
                let mut props = if new_id != old_id {
                    BTreeMap::new()
                } else {
                    old_props.clone()
                };
                for (k, spec) in specs {
                    let resolved = match spec {
                        Value::Object(m) => m
                            .get("from")
                            .and_then(Value::as_str)
                            .and_then(|from| old_props.get(from))
                            .and_then(|val| {
                                if m.get("opposite").and_then(Value::as_bool) == Some(true) {
                                    Some(opposite_dir(val).to_owned())
                                } else if let Some(add) = m.get("add").and_then(Value::as_i64) {
                                    val.parse::<i64>().ok().map(|n| (n + add).to_string())
                                } else {
                                    Some(val.clone())
                                }
                            }),
                        Value::String(s) => Some(s.clone()),
                        Value::Bool(b) => Some(b.to_string()),
                        Value::Number(n) => Some(n.to_string()),
                        _ => None,
                    };
                    if let Some(v) = resolved {
                        props.insert(k.clone(), v);
                    }
                }
                if new_id == old_id && props == old_props {
                    continue;
                }
                changed.insert(spot, (new_id.clone(), props.clone()));
                groups.entry((new_id, props)).or_default().push(spot);
            }
            for ((id, props), pts) in groups {
                out.push(vec![
                    tick.clone(),
                    json!("block"),
                    json!(pts),
                    json!(id),
                    json!(props),
                ]);
            }
        } else if op == "properties" && arr.len() >= 4 {
            let Some(positions) = arr[2].as_array() else {
                continue;
            };
            let Some(mods) = arr[3].as_object() else {
                continue;
            };
            let mut groups: BTreeMap<(String, BTreeMap<String, String>), Vec<[i32; 3]>> =
                BTreeMap::new();
            for pval in positions {
                let Some(spot) = json_pos(pval) else { continue };
                let Some((id, old_props)) =
                    changed.get(&spot).or_else(|| original.get(&spot)).cloned()
                else {
                    continue;
                };
                let mut props = old_props.clone();
                for (k, v) in mods {
                    let chosen = match v {
                        Value::Null => {
                            let variants = prop_values(jar, &id, k);
                            let cur = props.get(k);
                            if let Some(idx) =
                                cur.and_then(|c| variants.iter().position(|x| x == c))
                            {
                                variants[(idx + 1) % variants.len()].clone()
                            } else if cur.is_none() && variants.len() >= 2 {
                                variants[1].clone()
                            } else {
                                continue;
                            }
                        }
                        Value::String(s) => s.clone(),
                        Value::Bool(b) => b.to_string(),
                        Value::Number(n) => n.to_string(),
                        _ => continue,
                    };
                    props.insert(k.clone(), chosen);
                }
                if props == old_props {
                    continue;
                }
                changed.insert(spot, (id.clone(), props.clone()));
                groups.entry((id, props)).or_default().push(spot);
            }
            for ((id, props), pts) in groups {
                out.push(vec![
                    tick.clone(),
                    json!("block"),
                    json!(pts),
                    json!(id),
                    json!(props),
                ]);
            }
        } else {
            if op == "block" && arr.len() >= 5 {
                if let (Some(pts), Some(id), Some(pmap)) = (
                    arr[2].as_array(),
                    arr[3].as_str(),
                    arr[4].as_object(),
                ) {
                    let props: BTreeMap<String, String> = pmap
                        .iter()
                        .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                        .collect();
                    for p in pts {
                        if let Some(spot) = json_pos(p) {
                            changed.insert(spot, (id.to_owned(), props.clone()));
                        }
                    }
                }
            } else if op == "restore"
                && arr.len() >= 3
                && let Some(pts) = arr[2].as_array()
            {
                for p in pts {
                    if let Some(spot) = json_pos(p) {
                        changed.remove(&spot);
                    }
                }
            }
            out.push(arr.clone());
        }
    }
    out
}

fn import_ponder<R: Read + std::io::Seek>(
    decl_dir: &Path,
    jar: &mut ZipArchive<R>,
    entries: &HashSet<String>,
    en_us: &HashMap<String, String>,
    dest_dir: &Path,
) -> Result<HashSet<String>, String> {
    let mut catalogue: Value = read_json(&decl_dir.join("ponder_storyboards.json"))?;
    let ponder_jar_name = entries
        .iter()
        .find(|n| n.starts_with("META-INF/jarjar/ponder-") && n.ends_with(".jar"))
        .cloned()
        .ok_or_else(|| "Create jar lacks META-INF/jarjar/ponder-*.jar".to_owned())?;
    let ponder_bytes = read_entry(jar, &ponder_jar_name)?;
    let mut ponder_zip = ZipArchive::new(Cursor::new(ponder_bytes))
        .map_err(|e| format!("{ponder_jar_name}: {e}"))?;

    for name in ["logo", "widgets"] {
        let bytes = read_entry(&mut ponder_zip, &format!("assets/ponder/textures/gui/{name}.png"))?;
        write_bytes(
            &dest_dir.join(format!("assets/create/textures/gui/ponder/{name}.png")),
            &bytes,
        )?;
    }
    let ponder_lang_bytes = read_entry(&mut ponder_zip, "assets/ponder/lang/en_us.json")?;
    let ponder_lang: HashMap<String, String> =
        serde_json::from_slice(&ponder_lang_bytes).map_err(|e| e.to_string())?;
    let mut ui_lang = BTreeMap::new();
    for (k, v) in ponder_lang {
        if k.starts_with("ponder.ui.") {
            ui_lang.insert(k, v);
        }
    }
    catalogue["ui"] = json!(ui_lang);

    if let Some(tags) = catalogue.get_mut("tags").and_then(Value::as_array_mut) {
        for tag in tags {
            let id = tag
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned();
            let prefix = format!("create.ponder.tag.{id}");
            tag["title"] = json!(en_us.get(&prefix).cloned().unwrap_or_default());
            tag["description"] =
                json!(en_us.get(&format!("{prefix}.description")).cloned().unwrap_or_default());
        }
    }

    let mut resolver = ModelResolver {
        jar,
        entries,
        supported_cache: HashMap::new(),
        models_cache: HashMap::new(),
    };
    let mut models = HashSet::new();
    let ponder_out = dest_dir.join("assets/create/ponder");
    fs::create_dir_all(&ponder_out).map_err(|e| e.to_string())?;

    let mut scenes = catalogue
        .get_mut("scenes")
        .and_then(Value::as_array_mut)
        .map(std::mem::take)
        .unwrap_or_default();

    for (index, scene_val) in scenes.iter_mut().enumerate() {
        let Some(scene) = scene_val.as_object_mut() else {
            continue;
        };
        let key = scene
            .remove("key")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let path = scene
            .remove("path")
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_default();
        let ops = scene
            .remove("ops")
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        let mut end = scene
            .remove("duration")
            .and_then(|v| v.as_i64())
            .unwrap_or(80);
        let bounds = scene.remove("bounds").unwrap_or(Value::Null);
        let scale = scene.remove("scale").unwrap_or(Value::Null);
        let offset = scene.remove("offset").unwrap_or(Value::Null);

        let prefix = format!("create.ponder.{key}.");
        let header = en_us.get(&format!("{prefix}header")).cloned().unwrap_or_default();
        let file_name = format!("scene_{index}.json");
        scene.insert("title".to_owned(), Value::String(header));
        scene.insert("file".to_owned(), Value::String(file_name.clone()));

        let nbt_bytes = read_entry(resolver.jar, &format!("{PREFIX}ponder/{path}.nbt"))?;
        let root = read_nbt_gz(&nbt_bytes)?;
        let (size, palette, blocks, speeds) = parse_ponder_structure(&root)?;

        let scheduled: HashSet<String> = ops
            .iter()
            .filter_map(|v| v.as_array())
            .filter(|a| a.len() >= 3 && a[1].as_str() == Some("caption"))
            .filter_map(|a| a[2].as_str().map(str::to_owned))
            .collect();

        let expanded = expand_properties(resolver.jar, resolver.entries, &palette, &blocks, &ops);
        let mut events = Vec::with_capacity(expanded.len());
        let mut palette_states: Vec<PaletteEntry> = palette.clone();

        for mut ev in expanded {
            let op_name = ev.get(1).and_then(Value::as_str).unwrap_or("");
            if op_name == "caption" && ev.len() >= 4 {
                let cap_key = ev[2].as_str().unwrap_or("").to_owned();
                let Some(text) = en_us.get(&cap_key) else {
                    continue;
                };
                let tick = ev[0].as_i64().unwrap_or(0);
                let dur = ev[3].as_i64().unwrap_or(0).min(140);
                end = end.max(tick + dur);
                ev[2] = Value::String(text.clone());
            } else if op_name == "block"
                && ev.len() >= 5
                && let (Some(id), Some(pmap)) = (ev[3].as_str(), ev[4].as_object())
            {
                let props: BTreeMap<String, String> = pmap
                    .iter()
                    .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_owned())))
                    .collect();
                palette_states.push((id.to_owned(), props));
            }
            events.push(Value::Array(ev));
        }

        let mut text_keys: Vec<(u32, &String)> = en_us
            .keys()
            .filter_map(|k| {
                let rest = k.strip_prefix(&prefix)?;
                let suffix = rest.strip_prefix("text_")?;
                let num = suffix.parse::<u32>().ok()?;
                Some((num, k))
            })
            .collect();
        text_keys.sort_by_key(|(n, _)| *n);
        for (_, tk) in text_keys {
            if !scheduled.contains(tk)
                && let Some(text) = en_us.get(tk)
            {
                events.push(json!([end, "caption", text, 100]));
                end += 100;
            }
        }

        let mut fallback = Vec::new();
        for (id, props) in &palette_states {
            let parts = resolver.models_for(id, props);
            if !parts.is_empty() {
                for (name, _, _) in &parts {
                    models.insert(name.clone());
                }
                fallback.push(json!([[id, props], parts]));
            }
        }

        let arms = parse_arm_points(&root);
        let tanks = parse_tank_data(&root, &palette);
        let boards = parse_board_data(&root, &palette);
        let timeline = json!({
            "size": size,
            "palette": palette,
            "blocks": blocks,
            "speeds": speeds,
            "events": events,
            "models": fallback,
            "arms": arms,
            "tanks": tanks,
            "boards": boards,
            "duration": end.max(80),
            "bounds": bounds,
            "scale": scale,
            "offset": offset,
        });
        write_json_compact(&ponder_out.join(&file_name), &timeline)?;
    }

    catalogue["scenes"] = Value::Array(scenes);
    write_json_compact(&ponder_out.join("catalog.json"), &catalogue)?;
    Ok(models)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("tachyon-create-stage-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn door_and_fit_blockstate_strip_undeclared_properties() {
        let door = json!({"variants": {
            "facing=north,half=lower,visible=true": {"model": "create:block/andesite_door/bottom"},
            "facing=north,half=lower,visible=false": {"model": "minecraft:block/air"}
        }});
        let mapped = door_blockstate(&door);
        let obj = mapped["variants"].as_object().unwrap();
        assert_eq!(obj.len(), 1);
        assert!(obj.contains_key("facing=north,half=lower"));

        let glass = json!({"variants": {
            "axis=x,waterlogged=false": {"model": "create:block/shaft"},
            "axis=x,waterlogged=true": {"model": "create:block/shaft_wet"}
        }});
        let decl: HashSet<String> = ["axis".to_owned()].into_iter().collect();
        let fitted = fit_blockstate(&glass, &decl);
        let fobj = fitted["variants"].as_object().unwrap();
        assert_eq!(fobj.len(), 1);
        assert_eq!(fobj["axis=x"]["model"], "create:block/shaft");
    }

    #[test]
    fn stage_create_horizon_unpacks_decl_goggles_models_sounds_and_textures() {
        let root = scratch("synthetic");
        let decl = root.join("decl");
        let dest = root.join("dest");
        let jar = root.join("create.jar");

        write_json_compact(
            &decl.join("data/create/horizon/block/shaft.json"),
            &json!({"template": "minecraft:glass", "state_properties": {"axis": {"values": ["x", "y", "z"]}}}),
        )
        .unwrap();
        write_json_compact(
            &decl.join("assets/create/items/shaft.json"),
            &json!({"model": {"type": "minecraft:model", "model": "create:item/shaft"}}),
        )
        .unwrap();
        write_json_compact(
            &decl.join("ponder_storyboards.json"),
            &json!({"tags": [], "scenes": []}),
        )
        .unwrap();
        fs::write(
            decl.join("manifest.toml.template"),
            "id = \"create\"\nname = \"Create\"\nversion = \"0.1.3\"\nside = \"both\"\n",
        )
        .unwrap();
        write_json_compact(
            &decl.join("assets_spec.json"),
            &json!({
                "models": ["block/shaft_half"],
                "base_blocks": ["shaft"],
                "door_blocks": [],
                "static_blockstates": {},
                "blocks": {
                    "shaft": {"template": "minecraft:glass", "state_properties": ["axis"]}
                },
                "custom_models": [],
                "goggles": {
                    "prefixes": ["create.gui.goggles.", "create.tooltip.", "create.boiler.", "create.hint.", "create.recipe.assembly."],
                    "keys": [],
                    "required": ["create.gui.goggles.kinetic_stats"]
                }
            }),
        )
        .unwrap();

        let opts = SimpleFileOptions::default();
        let mut pbuf = Vec::new();
        {
            let mut pz = zip::ZipWriter::new(Cursor::new(&mut pbuf));
            pz.start_file("assets/ponder/textures/gui/logo.png", opts).unwrap();
            pz.write_all(b"PNG").unwrap();
            pz.start_file("assets/ponder/textures/gui/widgets.png", opts).unwrap();
            pz.write_all(b"PNG").unwrap();
            pz.start_file("assets/ponder/lang/en_us.json", opts).unwrap();
            pz.write_all(br#"{"ponder.ui.next":"Next"}"#).unwrap();
            pz.finish().unwrap();
        }
        {
            let mut z = zip::ZipWriter::new(fs::File::create(&jar).unwrap());
            z.start_file("META-INF/jarjar/ponder-1.0.jar", opts).unwrap();
            z.write_all(&pbuf).unwrap();
            z.start_file("assets/create/lang/en_us.json", opts).unwrap();
            z.write_all(br#"{"create.gui.goggles.kinetic_stats":"Kinetic Stats","unrelated":"x"}"#)
                .unwrap();
            z.start_file("assets/create/blockstates/shaft.json", opts).unwrap();
            z.write_all(br#"{"variants":{"axis=x":{"model":"create:block/shaft"}}}"#)
                .unwrap();
            for m in ["block/shaft", "block/shaft_half", "item/shaft"] {
                z.start_file(format!("assets/create/models/{m}.json"), opts).unwrap();
                z.write_all(br#"{"textures":{"0":"create:block/axis"}}"#).unwrap();
            }
            z.start_file("assets/create/sounds.json", opts).unwrap();
            z.write_all(br#"{"cog":{"sounds":["create:cog"]}}"#).unwrap();
            z.start_file("assets/create/sounds/cog.ogg", opts).unwrap();
            z.write_all(b"OGG").unwrap();
            z.start_file("assets/create/textures/block/axis.png", opts).unwrap();
            z.write_all(b"PNG").unwrap();
            z.finish().unwrap();
        }

        stage_create_horizon(&decl, &jar, &dest).unwrap();
        assert!(dest.join(horizon_client::tachyon_native_mods::CREATE_HORIZON_STAMP).is_file());
        assert!(dest.join("assets/create/textures/block/axis.png").is_file());
        assert!(dest.join("assets/create/sounds/cog.ogg").is_file());
        let goggles: Value = read_json(&dest.join("assets/create/goggles/lang.json")).unwrap();
        assert_eq!(goggles["create.gui.goggles.kinetic_stats"], "Kinetic Stats");
        assert!(goggles.get("unrelated").is_none());
        let _ = fs::remove_dir_all(&root);
    }
}
