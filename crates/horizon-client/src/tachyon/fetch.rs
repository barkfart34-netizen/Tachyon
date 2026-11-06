//! Mods Tachyon fetches from Modrinth instead of carrying: their assets are
//! their authors' (Create's textures, models and sounds are All Rights
//! Reserved, its code and data MIT), so neither the repository nor a
//! release holds them. When the player asks, the game downloads the one file
//! a [`Fetch`] pins by its SHA-512 from Modrinth, refuses it if the hash
//! differs, keeps it so it works offline after, and ports it on the device
//! as an imported jar is (`import.rs`). Only the paths the fetch takes (and
//! the jar's metadata) are read out of it; its code never runs.
use super::import::{self, Imported};
use sha2::{Digest, Sha512};
use std::fs;
use std::io::{Read, Write};
use std::path::Path;
use std::time::Duration;

/// A mod fetched from Modrinth: one file of one version.
#[derive(Debug, Clone, Copy)]
pub struct Fetch {
    /// Its id once ported (`horizon-mods/<id>`).
    pub id: &'static str,
    pub name: &'static str,
    /// Its Modrinth project and version, for the player.
    pub modrinth: &'static str,
    pub version: &'static str,
    /// The file, its size in bytes and its SHA-512 (hex).
    pub file: &'static str,
    pub size: u64,
    pub sha512: &'static str,
    /// What comes out of the jar: these paths (a folder ends with `/`), beside
    /// its metadata (`pack.mcmeta`, `META-INF/*.toml`).
    pub take: &'static [&'static str],
}

/// Create 6.0.10 for Minecraft 1.21.1 (NeoForge): its blocks, items, recipes,
/// textures, models and sounds; Kinetica's engine turns its machines.
pub const CREATE: Fetch = Fetch {
    id: "create",
    name: "Create",
    modrinth: "create",
    version: "6.0.10+mc1.21.1",
    file: "create-1.21.1-6.0.10.jar",
    size: 19_123_767,
    sha512: "11cc8fc049d2f67f6548c7abfada6b82a3adb5c7ca410a742de04bbca76e03862c518721b88d806f6e6d768a4d68531fdb903a85859b25d1484d550cc7bafd4b",
    take: &["assets/create/", "data/create/", "data/c/", "data/minecraft/"],
};

const API: &str = "https://api.modrinth.com/v2";
/// Where Modrinth keeps its files: a URL elsewhere is refused.
const CDN: &str = "https://cdn.modrinth.com/";

impl Fetch {
    /// The question before downloading.
    pub fn ask(&self) -> String {
        format!(
            "{} needs its textures, models and sounds from Modrinth (about {} MB). Download?",
            self.name,
            self.size.div_ceil(1 << 20)
        )
    }
}

/// Whether `f` is ported into the game folder `home`.
pub fn installed(f: &Fetch, home: &Path) -> bool {
    home.join(horizon_client::mod_list::DIR).join(f.id).is_dir()
}

/// Gets `f` into the game folder `home`: its file from `cache` if it is
/// there and whole, else from Modrinth (kept in `cache`), then ported.
/// `report` hears what it is doing.
pub fn get(f: &Fetch, home: &Path, cache: &Path, report: &dyn Fn(String)) -> Result<Imported, String> {
    fs::create_dir_all(cache).map_err(|e| format!("{}: {e}", cache.display()))?;
    let jar = cache.join(f.file);
    if !hash_of(&jar).is_some_and(|h| h == f.sha512) {
        download(f, &jar, report)?;
    }
    report(format!("Taking {}'s content out of its file", f.name));
    let taken = cache.join(format!("{}-content.jar", f.id));
    let result = take(&jar, &taken, f.take).and_then(|()| import::import(&taken, f.file, home, report));
    let _ = fs::remove_file(&taken);
    let imported = result?;
    // Create's machines, for Kinetica's engine to turn.
    if f.id == CREATE.id {
        let port = home.join(horizon_client::mod_list::DIR).join(f.id);
        super::create_port::fix(&port)?;
        super::create_port::restore_recipes(&port, &jar)?;
    }
    Ok(imported)
}

fn agent() -> ureq::Agent {
    ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(20))
        .timeout_read(Duration::from_secs(60))
        .user_agent(&format!(
            "Tachyon/{} (Rxuser2/Tachyon; a Horizon build)",
            env!("CARGO_PKG_VERSION")
        ))
        .build()
}

/// The file's address on Modrinth's CDN, found by its hash.
fn url(f: &Fetch) -> Result<String, String> {
    let lookup = format!("{API}/version_file/{}?algorithm=sha512", f.sha512);
    let text = agent()
        .get(&lookup)
        .call()
        .map_err(|e| format!("Modrinth didn't answer: {e}"))?
        .into_string()
        .map_err(|e| e.to_string())?;
    let version: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    (version["files"].as_array().into_iter().flatten())
        .find(|file| file["hashes"]["sha512"].as_str() == Some(f.sha512))
        .and_then(|file| file["url"].as_str())
        .filter(|url| url.starts_with(CDN))
        .map(str::to_owned)
        .ok_or_else(|| format!("Modrinth no longer has {}.", f.file))
}

/// Downloads `f` to `to`, checking its hash on the way.
fn download(f: &Fetch, to: &Path, report: &dyn Fn(String)) -> Result<(), String> {
    report(format!("Asking Modrinth for {}", f.file));
    let url = url(f)?;
    let response = agent()
        .get(&url)
        .call()
        .map_err(|e| format!("The download failed: {e}"))?;
    let part = to.with_extension("part");
    let mut out = fs::File::create(&part).map_err(|e| format!("{}: {e}", part.display()))?;
    let mut reader = response.into_reader();
    let mut hasher = Sha512::new();
    let mut buffer = vec![0u8; 1 << 16];
    let (mut done, mut said) = (0u64, u64::MAX);
    let total = f.size.div_ceil(1 << 20);
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|e| format!("The download stopped: {e}"))?;
        if n == 0 {
            break;
        }
        done += n as u64;
        if done > f.size {
            drop(out);
            let _ = fs::remove_file(&part);
            return Err(format!("{} from Modrinth is bigger than it should be: not used.", f.file));
        }
        hasher.update(&buffer[..n]);
        out.write_all(&buffer[..n]).map_err(|e| e.to_string())?;
        if done >> 20 != said {
            said = done >> 20;
            report(format!("Downloading {} from Modrinth: {said} / {total} MB", f.name));
        }
    }
    drop(out);
    if hex(&hasher.finalize()) != f.sha512 {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "{} from Modrinth isn't the file expected (its SHA-512 differs): not used.",
            f.file
        ));
    }
    fs::rename(&part, to).map_err(|e| format!("{}: {e}", to.display()))
}

/// The SHA-512 of the file at `path` (hex), if it can be read.
fn hash_of(path: &Path) -> Option<String> {
    let mut file = fs::File::open(path).ok()?;
    let mut hasher = Sha512::new();
    std::io::copy(&mut file, &mut hasher).ok()?;
    Some(hex(&hasher.finalize()))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Whether the jar's entry `name` comes along: under a taken path, or the
/// jar's metadata; never code.
fn taken(name: &str, take: &[&str]) -> bool {
    if name.ends_with('/') || name.ends_with(".class") {
        return false;
    }
    let metadata = name == "pack.mcmeta"
        || (name.starts_with("META-INF/") && !name[9..].contains('/') && name.ends_with(".toml"));
    metadata || take.iter().any(|t| name.starts_with(t))
}

/// Writes to `out` a jar of `jar`'s entries that come along.
fn take(jar: &Path, out: &Path, take: &[&str]) -> Result<(), String> {
    let file = fs::File::open(jar).map_err(|e| format!("{}: {e}", jar.display()))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| e.to_string())?;
    let target = fs::File::create(out).map_err(|e| format!("{}: {e}", out.display()))?;
    let mut writer = zip::ZipWriter::new(target);
    let options = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut bytes = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
        let name = entry.name().to_owned();
        if !taken(&name, take) {
            continue;
        }
        bytes.clear();
        entry.read_to_end(&mut bytes).map_err(|e| format!("{name}: {e}"))?;
        writer.start_file(name.as_str(), options).map_err(|e| e.to_string())?;
        writer.write_all(&bytes).map_err(|e| e.to_string())?;
    }
    writer.finish().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_taken_paths_and_the_metadata_come_along() {
        let take = CREATE.take;
        assert!(taken("assets/create/textures/block/andesite_casing.png", take));
        assert!(taken("data/create/recipe/crushing/iron_ore.json", take));
        assert!(taken("META-INF/neoforge.mods.toml", take));
        assert!(taken("pack.mcmeta", take));
        assert!(!taken("com/simibubi/create/Create.class", take));
        assert!(!taken("assets/create/", take));
        assert!(!taken("assets/minecraft/textures/block/stone.png", take));
        assert!(!taken("META-INF/jarjar/flywheel.jar", take));
        assert!(!taken("META-INF/jarjar/metadata.toml", take));
        assert!(!taken("data/create/sneaky.class", take));
    }

    #[test]
    fn a_hash_is_lowercase_hex() {
        assert_eq!(hex(&[0x0a, 0xff]), "0aff");
        assert_eq!(CREATE.sha512.len(), 128);
        assert!(CREATE.ask().contains("about 19 MB"));
    }
}
