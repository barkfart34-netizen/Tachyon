//! Shared native `horizon-mod-api` bootstrap for Tachyon on both Desktop and Android.
//!
//! Statically links Tachyon's bundled `horizon-mod-api` mods (`horizon_quests`) into
//! the engine without on-device Rust compilation or `[native]` source directories on disk.

use horizon_mod_package::{packages, runtime::Runtime};
use std::sync::{
    Once,
    atomic::{AtomicBool, Ordering},
};

/// Base statically linked `horizon-mod-api` mods bundled into Tachyon.
const BUILTIN_BASE: &[mod_api::__private::native::BuiltMod] = &[horizon_quests::BUILT];
/// Statically linked `horizon-mod-api` mods when Create (`mod-create`) is staged.
const BUILTIN_WITH_CREATE: &[mod_api::__private::native::BuiltMod] =
    &[horizon_quests::BUILT, mod_create::BUILT];

/// Statically linked `horizon-mod-api` mods known to Tachyon.
pub const BUILTIN_MODS: &[mod_api::__private::native::BuiltMod] = BUILTIN_WITH_CREATE;

static CREATE_ACTIVE: AtomicBool = AtomicBool::new(false);

/// Whether the vendored `mod-create` native mod is active for this session.
pub fn create_active() -> bool {
    CREATE_ACTIVE.load(Ordering::Relaxed)
}

/// Marker written into `horizon-mods/create/` once `create_port::stage_create_horizon`
/// has staged the `Create-Horizon` declarations and Create 6.0.10 JAR assets.
pub const CREATE_HORIZON_STAMP: &str = ".tachyon-create-horizon-v2";

fn create_package_dir() -> std::path::PathBuf {
    horizon_client::logging::game_home()
        .join(horizon_client::mod_list::DIR)
        .join("create")
}

fn create_ready() -> Option<packages::AdjacentManifest> {
    let home = horizon_client::logging::game_home();
    if horizon_client::mod_list::ModList::load(&home)
        .disabled
        .contains("create")
    {
        return None;
    }
    let dir = create_package_dir();
    if !dir.join(CREATE_HORIZON_STAMP).is_file() || !dir.join("horizon-mod.toml").is_file() {
        return None;
    }
    match packages::read_bundled_native(&dir) {
        Ok(pkg)
            if pkg.manifest.id == mod_create::BUILT.id
                && pkg.manifest.version == mod_create::BUILT.version
                && pkg.manifest.side == mod_create::BUILT.side =>
        {
            Some(pkg)
        }
        Ok(pkg) => {
            log::warn!(
                "Tachyon: create package identity mismatch (got {} {} {:?}) at {}",
                pkg.manifest.id,
                pkg.manifest.version,
                pkg.manifest.side,
                dir.display()
            );
            None
        }
        Err(error) => {
            log::warn!(
                "Tachyon: staged create package unreadable at {}: {error}",
                dir.display()
            );
            None
        }
    }
}

/// Initializes the `horizon-mod-api` platform slots, registers Tachyon's statically linked
/// native mods, and reads their bundled asset/data manifests from `brand::content()` and
/// (when staged) `<home>/horizon-mods/create`.
///
/// Must be called after `brand::set_content(...)` (and on Android, after `content.zip` has
/// been unpacked into the content directory). Safe to call at most once per process.
pub fn install() -> Runtime {
    static INIT: Once = Once::new();
    let staged_create = create_ready();
    let has_create = staged_create.is_some();
    INIT.call_once(|| {
        if let Err(error) = horizon_engine_slots::install_seal(mod_points::__private::seal) {
            log::warn!("Tachyon: engine seal already installed: {error}");
        }
        if let Err(error) = mod_platform::__private::initialize() {
            log::error!("Tachyon: mod platform instrumentation unavailable: {error}");
        }
        if let Err(error) = horizon_engine_slots::install_platform(horizon_engine_slots::Platform {
            install_packages: mod_platform::__private::install_packages,
            install_registration_data: mod_platform::__private::install_registration_data,
            install_parts: mod_platform::__private::install_parts,
            profiler_is_enabled: mod_platform::__private::profiler::is_enabled,
            profiler_start: mod_platform::__private::profiler::start,
            profiler_stop: mod_platform::__private::profiler::stop,
            profiler_summary: mod_platform::__private::profiler::summary,
            profiler_firefox_profile: mod_platform::__private::profiler::firefox_profile,
        }) {
            log::warn!("Tachyon: engine mod platform already installed: {error}");
        }
        crate::game::startup_progress::install(false);
        crate::entry::install_noop_code_runtime();
        let mut mods = vec![mod_api::ModEntry::new(
            horizon_quests::hq_meta::ID,
            horizon_quests::hooks(),
        )];
        let builtin = if has_create {
            mods.push(mod_api::ModEntry::new("create", mod_create::hooks()));
            CREATE_ACTIVE.store(true, Ordering::Relaxed);
            BUILTIN_WITH_CREATE
        } else {
            BUILTIN_BASE
        };
        mod_api::__private::install(mods.into());
        horizon_client::save::level_mods::set_loaded(Some(
            builtin
                .iter()
                .map(|m| horizon_client::save::level_mods::ModRecord {
                    id: m.id.to_owned(),
                    version: m.version.to_owned(),
                })
                .collect(),
        ));
    });

    let active_create = create_active();
    let expected_slice = if active_create {
        BUILTIN_WITH_CREATE
    } else {
        BUILTIN_BASE
    };

    let mut bundled = Vec::new();
    if let Some(root) = horizon_client::brand::content() {
        let expected = &horizon_quests::BUILT;
        let path = root.join(expected.id);
        match packages::read_bundled_native(&path) {
            Ok(pkg) => {
                if pkg.manifest.id == expected.id
                    && pkg.manifest.version == expected.version
                    && pkg.manifest.side == expected.side
                {
                    bundled.push(pkg);
                } else {
                    log::error!(
                        "Tachyon: bundled native mod {} identity mismatch at {}",
                        expected.id,
                        path.display()
                    );
                }
            }
            Err(error) => {
                log::error!(
                    "Tachyon: bundled native mod {} missing or unreadable at {}: {error}",
                    expected.id,
                    path.display()
                );
            }
        }
    } else {
        log::warn!("Tachyon: brand::content() not set when loading bundled native mods");
    }

    if active_create && let Some(pkg) = staged_create {
        bundled.push(pkg);
    }

    if bundled.len() == expected_slice.len() {
        Runtime::from_bundled(expected_slice, bundled)
    } else {
        Runtime::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::fs;
    use std::path::{Path, PathBuf};

    struct TempDir(PathBuf);

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
        fs::create_dir_all(dst)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            let dst_path = dst.join(entry.file_name());
            if entry.file_type()?.is_dir() {
                copy_dir_recursive(&entry.path(), &dst_path)?;
            } else {
                fs::copy(entry.path(), &dst_path)?;
            }
        }
        Ok(())
    }

    #[test]
    fn create_decl_loads_via_horizon_mod_package_and_matches_mod_create() {
        let decl_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tachyon/content/_create_decl");
        assert!(
            decl_dir.is_dir(),
            "missing _create_decl dir at {}",
            decl_dir.display()
        );

        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let tmp = TempDir(
            std::env::temp_dir().join(format!("tachyon-create-decl-test-{}-{nonce}", std::process::id())),
        );
        fs::create_dir_all(&tmp.0).unwrap();

        fs::copy(
            decl_dir.join("manifest.toml.template"),
            tmp.0.join("horizon-mod.toml"),
        )
        .unwrap();
        for dir_name in ["assets", "data"] {
            copy_dir_recursive(&decl_dir.join(dir_name), &tmp.0.join(dir_name)).unwrap();
        }

        let pkg = packages::read_bundled_native(&tmp.0)
            .expect("staged _create_decl manifest must validate as bundled native");
        assert_eq!(pkg.manifest.id, mod_create::BUILT.id);
        assert_eq!(pkg.manifest.version, mod_create::BUILT.version);
        assert_eq!(pkg.manifest.side, mod_create::BUILT.side);

        let content = horizon_mod_package::Content::read_declaring(&tmp.0, &pkg.manifest, &[])
            .expect("_create_decl content must pass horizon-mod-package validation");

        let blocks: HashSet<&str> = content.blocks.iter().map(|b| b.id.as_str()).collect();
        let items: HashSet<&str> = content
            .items
            .iter()
            .map(|i| i.id.as_str())
            .chain(
                content
                    .blocks
                    .iter()
                    .filter(|b| b.item)
                    .map(|b| b.id.as_str()),
            )
            .collect();

        for id in mod_create::declared_blocks() {
            let full = format!("create:{id}");
            assert!(
                blocks.contains(full.as_str()),
                "declared_blocks entry {full} missing from _create_decl blocks"
            );
        }
        for id in mod_create::declared_block_entities() {
            let full = format!("create:{id}");
            assert!(
                blocks.contains(full.as_str()),
                "declared_block_entities entry {full} missing from _create_decl blocks"
            );
        }
        for id in mod_create::declared_items() {
            let full = format!("create:{id}");
            assert!(
                items.contains(full.as_str()),
                "declared_items entry {full} missing from _create_decl items"
            );
        }

        let fluid_ids: HashSet<&str> = content.fluids.iter().map(|f| f.id.as_str()).collect();
        assert!(fluid_ids.contains("create:chocolate"));
        assert!(fluid_ids.contains("create:honey"));
    }
}
