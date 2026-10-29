//! What a player brings into a game folder from elsewhere: a world, a
//! resource pack, a Horizon mod, or a Java mod's jar, whose content (blocks,
//! items, recipes, loot, worldgen, textures) Horizon's own porting tool
//! carries over (`horizon_port_pack::check`, as Horizon's launcher installs a
//! jar). The jar's code does not come along: Horizon has no Java.
//!
//! Everything is made from the player's own file on their own device: a
//! ported mod's assets are the mod authors', for the player's use.
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// What an import added, for the launcher to say.
#[derive(Debug, Clone, PartialEq)]
pub struct Imported {
    pub what: Kind,
    /// The world's, pack's or mod's name.
    pub name: String,
    /// One line for the player: what came in, and what did not.
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    World,
    ResourcePack,
    Mod,
}

/// Installs `file`, which the player knows as `name` (its extension tells a
/// jar from a zip), into the game folder `home`. `report` hears what it is
/// doing.
pub fn import(
    file: &Path,
    name: &str,
    home: &Path,
    report: &dyn Fn(String),
) -> Result<Imported, String> {
    let lower = name.to_ascii_lowercase();
    let extension = Path::new(&lower)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_owned();
    let stem = Path::new(name)
        .file_stem()
        .and_then(|s| s.to_str())
        .map(clean_name)
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "Imported".to_owned());
    match extension.as_str() {
        "mcworld" | "mcpack" | "mcaddon" | "mctemplate" => Err(
            "That is a Bedrock Edition file: Tachyon plays Java Edition worlds, packs and mods."
                .into(),
        ),
        "jar" => java_mod(file, home, report),
        "hmod" => horizon_mod(file, &stem, home),
        "mrpack" => Err("That is a Modrinth modpack: import its mods' jars one by one.".into()),
        _ => archive(file, &stem, home, report),
    }
}

/// A name for a folder: no path separators or characters Windows refuses.
fn clean_name(name: &str) -> String {
    name.chars()
        .map(|c| match c {
            '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
            c if c.is_control() => '_',
            c => c,
        })
        .collect::<String>()
        .trim()
        .trim_matches('.')
        .to_owned()
}

/// `dir/name`, or `dir/name (2)`... when that is taken.
fn free(dir: &Path, name: &str, extension: &str) -> PathBuf {
    let file = |n: &str| dir.join(format!("{n}{extension}"));
    let mut path = file(name);
    let mut n = 2;
    while path.exists() {
        path = file(&format!("{name} ({n})"));
        n += 1;
    }
    path
}

/// Turns a mod on in the game folder's `horizon-mods.txt`, as Options > Mods would.
fn enable(home: &Path, id: &str) {
    let mut list = horizon_client::mod_list::ModList::load(home);
    list.enabled.insert(id.to_owned());
    list.save(home);
}

fn horizon_mod(file: &Path, stem: &str, home: &Path) -> Result<Imported, String> {
    let manifest = horizon_mods::Package::read(file)
        .map_err(|e| format!("Not a Horizon mod: {e}"))?
        .manifest;
    let dir = home.join(horizon_client::mod_list::DIR);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(format!("{}.hmod", clean_name(&manifest.id)));
    fs::copy(file, &target).map_err(|e| format!("{}: {e}", target.display()))?;
    enable(home, &manifest.id);
    let name = if manifest.name.is_empty() {
        stem.to_owned()
    } else {
        manifest.name.clone()
    };
    Ok(Imported {
        what: Kind::Mod,
        note: format!("{name} is installed and on."),
        name,
    })
}

/// A Java mod: its content carried over into a folder mod, turned on.
fn java_mod(file: &Path, home: &Path, report: &dyn Fn(String)) -> Result<Imported, String> {
    report("Porting the mod's content (its code can't come along)".into());
    let work = home.join(".tachyon-import");
    let _ = fs::remove_dir_all(&work);
    let check = horizon_port_pack::check::check(file, &work)?;
    if check.verdict == horizon_port_pack::check::Verdict::No {
        let _ = fs::remove_dir_all(&work);
        return Err(format!("{} can't be ported: {}", check.name, check.summary));
    }
    let mods = home.join(horizon_client::mod_list::DIR);
    fs::create_dir_all(&mods).map_err(|e| e.to_string())?;
    let target = mods.join(clean_name(&check.mod_id));
    // A newer jar of the same mod replaces the older port.
    if target.exists() {
        fs::remove_dir_all(&target).map_err(|e| format!("{}: {e}", target.display()))?;
    }
    fs::rename(work.join("carry"), &target).map_err(|e| format!("{}: {e}", target.display()))?;
    let _ = fs::copy(work.join("check.json"), target.join("tachyon-check.json"));
    let _ = fs::remove_dir_all(&work);
    enable(home, &check.mod_id);
    let ports = check
        .ports
        .iter()
        .take(4)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    let mut note = format!(
        "{} {} is installed and on: {ports}.",
        check.name, check.version
    );
    if !check.skipped.is_empty() {
        note.push_str(" What it does in code (machines, screens, entities...) doesn't come along.");
    }
    Ok(Imported {
        what: Kind::Mod,
        name: check.name,
        note,
    })
}

/// A zip: a world (with `level.dat`), a resource pack (`pack.mcmeta` and
/// `assets/`), or a Horizon mod (`horizon-mod.toml`), at its top or in one
/// folder.
fn archive(
    file: &Path,
    stem: &str,
    home: &Path,
    report: &dyn Fn(String),
) -> Result<Imported, String> {
    let open = || -> Result<zip::ZipArchive<fs::File>, String> {
        let f = fs::File::open(file).map_err(|e| e.to_string())?;
        zip::ZipArchive::new(f)
            .map_err(|_| "Not a zip: a world, a pack or a mod comes as one.".to_owned())
    };
    let zip = open()?;
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    drop(zip);
    // The folder holding `marker`: "" at the top, or "<folder>/" one level down.
    let prefix = |marker: &str| -> Option<String> {
        names
            .iter()
            .filter_map(|n| n.strip_suffix(marker))
            .filter(|p| p.is_empty() || (p.ends_with('/') && p.matches('/').count() == 1))
            .min_by_key(|p| p.len())
            .map(str::to_owned)
    };
    if prefix("horizon-mod.toml").as_deref() == Some("") {
        return horizon_mod(file, stem, home);
    }
    if let Some(prefix) = prefix("level.dat") {
        let name = prefix.trim_end_matches('/');
        let name = if name.is_empty() {
            stem.to_owned()
        } else {
            clean_name(name)
        };
        let saves = home.join("saves");
        let target = free(&saves, &name, "");
        report(format!("Unpacking the world {name}"));
        extract(open()?, &prefix, &target)?;
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(name);
        return Ok(Imported {
            what: Kind::World,
            note: format!("The world {name} is in Singleplayer."),
            name,
        });
    }
    if let Some(prefix) = prefix("pack.mcmeta") {
        let has = |dir: &str| {
            names
                .iter()
                .any(|n| n.starts_with(&format!("{prefix}{dir}")))
        };
        if !has("assets/") {
            return Err(
                "That is a data pack: it goes in a world's datapacks folder, not in the game's."
                    .into(),
            );
        }
        let packs = home.join("resourcepacks");
        fs::create_dir_all(&packs).map_err(|e| e.to_string())?;
        let name = if prefix.is_empty() {
            stem.to_owned()
        } else {
            clean_name(prefix.trim_end_matches('/'))
        };
        let target = if prefix.is_empty() {
            let target = free(&packs, &name, ".zip");
            fs::copy(file, &target).map_err(|e| format!("{}: {e}", target.display()))?;
            target
        } else {
            // A pack zipped with its folder: unpacked as a folder pack.
            let target = free(&packs, &name, "");
            extract(open()?, &prefix, &target)?;
            target
        };
        let name = target
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or(name);
        return Ok(Imported {
            what: Kind::ResourcePack,
            note: format!("The resource pack {name} is in Options > Resource Packs."),
            name,
        });
    }
    Err("Not a world, a resource pack or a mod: a world has level.dat, a pack pack.mcmeta.".into())
}

/// The entries under `prefix` into `target`, without leaving it.
fn extract(mut zip: zip::ZipArchive<fs::File>, prefix: &str, target: &Path) -> Result<(), String> {
    let partial = PathBuf::from(format!("{}.partial", target.display()));
    let _ = fs::remove_dir_all(&partial);
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let Some(path) = entry.enclosed_name() else {
            continue;
        };
        let Ok(rest) = path.strip_prefix(prefix.trim_end_matches('/')) else {
            continue;
        };
        if rest.as_os_str().is_empty() {
            continue;
        }
        let out = partial.join(rest);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        io::copy(&mut entry, &mut file).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    fs::rename(&partial, target).map_err(|e| format!("{}: {e}", target.display()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tachyon-import-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn zip_of(path: &Path, files: &[(&str, &[u8])]) {
        let mut zip = zip::ZipWriter::new(fs::File::create(path).unwrap());
        for (name, bytes) in files {
            zip.start_file(*name, zip::write::SimpleFileOptions::default())
                .unwrap();
            zip.write_all(bytes).unwrap();
        }
        zip.finish().unwrap();
    }

    #[test]
    fn a_zipped_world_folder_lands_in_saves() {
        let dir = scratch("world");
        let file = dir.join("upload.zip");
        zip_of(
            &file,
            &[
                ("My World/level.dat", b"x"),
                ("My World/region/r.0.0.mca", b"y"),
            ],
        );
        let home = dir.join("home");
        let got = import(&file, "My World.zip", &home, &|_| {}).unwrap();
        assert_eq!(got.what, Kind::World);
        assert!(home.join("saves/My World/level.dat").is_file());
        assert!(home.join("saves/My World/region/r.0.0.mca").is_file());
        // The same world again comes in beside it.
        let again = import(&file, "My World.zip", &home, &|_| {}).unwrap();
        assert_eq!(again.name, "My World (2)");
    }

    #[test]
    fn a_resource_pack_zip_is_copied_whole() {
        let dir = scratch("pack");
        let file = dir.join("p.zip");
        zip_of(
            &file,
            &[("pack.mcmeta", b"{}"), ("assets/minecraft/x.png", b"z")],
        );
        let home = dir.join("home");
        let got = import(&file, "Faithful.zip", &home, &|_| {}).unwrap();
        assert_eq!(got.what, Kind::ResourcePack);
        assert!(home.join("resourcepacks/Faithful.zip").is_file());
    }

    #[test]
    fn data_packs_bedrock_files_and_strangers_are_refused() {
        let dir = scratch("refused");
        let home = dir.join("home");
        let data = dir.join("d.zip");
        zip_of(
            &data,
            &[("pack.mcmeta", b"{}"), ("data/x/recipe/a.json", b"{}")],
        );
        assert!(
            import(&data, "d.zip", &home, &|_| {})
                .unwrap_err()
                .contains("data pack")
        );
        assert!(
            import(&data, "w.mcworld", &home, &|_| {})
                .unwrap_err()
                .contains("Bedrock")
        );
        let other = dir.join("o.zip");
        zip_of(&other, &[("readme.txt", b"hi")]);
        assert!(import(&other, "o.zip", &home, &|_| {}).is_err());
    }

    #[test]
    fn names_are_safe_folder_names() {
        assert_eq!(clean_name("a/b:c*?"), "a_b_c__");
        assert_eq!(clean_name("..world.."), "world");
    }
}
