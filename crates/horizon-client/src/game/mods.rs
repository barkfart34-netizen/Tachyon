//! Mods in the game (`docs/mods.md`): content mods' blocks, items, data and
//! assets (`content_root`), and the Mods page's rows. Code mods are native
//! (`native_hooks.rs`); WebAssembly code mods don't run any more.
use super::Game;
use horizon_client::{
    mod_list::{self, Installed},
    text::Text,
};
use std::path::PathBuf;

#[derive(Default)]
pub(super) struct ModsState {
    /// `--dev-mod PATH`: always on.
    pub(super) dev: Vec<PathBuf>,
    /// The WebAssembly mods found that no longer run, for the one-time notice.
    stopped: Vec<String>,
}

/// `root` with the content mods that are on layered over it
/// (docs/modpacks.md): their blocks, items, data and assets. Read once at
/// start, before the resources load; a mod whose content is refused is left
/// out here and its row on the Mods page says why.
pub(super) fn content_root(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
) -> std::path::PathBuf {
    content_root_reported(root, enabled, dev, |_| {})
}

/// Report the accepted packages for the startup display, using the same
/// discovery and validation as content_root. An unsuccessful overlay has
/// no active content mods, even if individual packages could be read.
pub(super) fn content_root_reported(
    root: &std::path::Path,
    enabled: &std::collections::BTreeSet<String>,
    dev: &[std::path::PathBuf],
    report: impl FnOnce(Vec<(String, String)>),
) -> std::path::PathBuf {
    use horizon_client::mod_content::{self, ContentMod};
    use horizon_mods::{Capability, Content, Package};
    let here = std::env::current_dir().unwrap_or_default();
    // A build's own content mods (Tachyon's), always on and first.
    let built_in = horizon_client::brand::content().map_or_else(Vec::new, horizon_mods::find);
    let mut paths = built_in.clone();
    paths.extend(horizon_mods::find(&here.join(mod_list::DIR)));
    paths.retain(|p| !dev.contains(p));
    paths.extend(dev.iter().cloned());
    let mut mods: Vec<ContentMod> = Vec::new();
    let mut names = Vec::new();
    for path in &paths {
        let Ok(package) = Package::read(path) else {
            continue;
        };
        let m = &package.manifest;
        let on = enabled.contains(&m.id) || dev.contains(path) || built_in.contains(path);
        if !on
            || !m.capabilities.contains(&Capability::Registries)
            || mods.iter().any(|c| c.id == m.id)
        {
            continue;
        }
        let content = Content::read(path, m).and_then(|content| {
            // The first of two mods owning one namespace stays on.
            let own = (m.id.as_str(), content.namespaces.as_slice());
            match mods
                .iter()
                .find_map(|c| horizon_mods::content::clash((&c.id, &c.content.namespaces), own))
            {
                Some(e) => Err(e),
                None => Ok(content),
            }
        });
        match content {
            Ok(content) => {
                names.push((m.id.clone(), m.name.clone()));
                mods.push(ContentMod {
                    id: m.id.clone(),
                    content,
                });
            }
            Err(e) => log::warn!("Horizon: mod {} content not loaded: {e}", m.id),
        }
    }
    // The overlay beside the resources when that folder takes it, so its
    // links to them stay on one disk (Windows copies across disks: the
    // resources are hundreds of megabytes); else in the game folder.
    let beside = root.parent().map(|p| p.join(".horizon-cache")).filter(|c| {
        let probe = c.join(".probe");
        let ok = std::fs::create_dir_all(c).is_ok() && std::fs::write(&probe, b"").is_ok();
        let _ = std::fs::remove_file(probe);
        ok
    });
    let cache = beside.unwrap_or_else(|| here.join(".horizon-cache"));
    match mod_content::resource_root(root, &mods, &cache) {
        Ok(out) => {
            if out != root {
                log::info!(
                    "Horizon: content from {} mod(s) in {}",
                    mods.len(),
                    out.display()
                );
            }
            report(names);
            out
        }
        Err(e) => {
            log::warn!("Horizon: content mods not loaded: {e}");
            report(Vec::new());
            root.to_owned()
        }
    }
}

/// A WebAssembly code mod's row on the Mods page.
const STOPPED: &str = "Doesn't run any more: needs a native port";
/// The player was told once about the WebAssembly mods that stopped (`horizon-mods/<this>`).
const NOTICE_SEEN: &str = ".native-notice";

impl Game<'_> {
    /// Lists the installed mods on the Mods page. Content mods load with the resources
    /// (`content_root`); native ones are built into the game (`native_hooks.rs`). WebAssembly code
    /// mods don't run any more: their rows say so, and the player hears it once in the chat
    /// (`tell_native_now`).
    pub(super) fn start_mods(&mut self) {
        let dir = Self::options_path().with_file_name(mod_list::DIR);
        let mut paths = horizon_mods::find(&dir);
        paths.retain(|p| !self.mods.dev.contains(p));
        paths.extend(self.mods.dev.iter().cloned());
        self.settings.mods.installed = paths
            .iter()
            .map(|path| {
                let file = path.file_name().unwrap_or_default().to_string_lossy();
                let package = match horizon_mods::Package::read(path) {
                    Ok(package) => package,
                    Err(e) => {
                        return Installed {
                            id: String::new(),
                            title: file.into_owned(),
                            description: String::new(),
                            problem: Some(format!("Can't be read: {e}")),
                            dev: false,
                            native: None,
                        };
                    }
                };
                let m = &package.manifest;
                let problem = package.wasm.is_some().then(|| STOPPED.to_owned());
                Installed {
                    id: m.id.clone(),
                    title: format!("{} {}", m.name, m.version),
                    description: m.description.clone(),
                    problem,
                    dev: self.mods.dev.contains(path),
                    native: None,
                }
            })
            .collect();
        // A mod with a native version built in says so instead, and isn't news.
        self.list_native_mods();
        let installed = &self.settings.mods.installed;
        self.mods.stopped = installed
            .iter()
            .filter(|row| row.problem.as_deref() == Some(STOPPED))
            .map(|row| row.title.clone())
            .collect();
    }
    /// Once, in the first world after the update: which WebAssembly mods stopped.
    pub(super) fn tell_native_now(&mut self) {
        if self.mods.stopped.is_empty() || self.player.is_none() {
            return;
        }
        let names = std::mem::take(&mut self.mods.stopped).join(", ");
        let seen = Self::options_path()
            .with_file_name(mod_list::DIR)
            .join(NOTICE_SEEN);
        if seen.exists() {
            return;
        }
        self.system_message(Text::literal(format!(
            "Horizon mods are native now: {names} stopped running until they have a native version. The launcher's Mods page has the native ones."
        )));
        let _ = std::fs::write(&seen, "");
    }
}
