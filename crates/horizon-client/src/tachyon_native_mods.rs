//! Shared native `horizon-mod-api` bootstrap for Tachyon on both Desktop and Android.
//!
//! Statically links Tachyon's bundled `horizon-mod-api` mods (`horizon_quests`) into
//! the engine without on-device Rust compilation or `[native]` source directories on disk.

use horizon_mod_package::{packages, runtime::Runtime};
use std::sync::Once;

/// Statically linked `horizon-mod-api` mods bundled into Tachyon.
pub const BUILTIN_MODS: &[mod_api::__private::native::BuiltMod] = &[horizon_quests::BUILT];

/// Initializes the `horizon-mod-api` platform slots, registers Tachyon's statically linked
/// native mods, and reads their bundled asset/data manifests from `brand::content()`.
///
/// Must be called after `brand::set_content(...)` (and on Android, after `content.zip` has
/// been unpacked into the content directory). Safe to call at most once per process.
pub fn install() -> Runtime {
    static INIT: Once = Once::new();
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
        let mods = vec![mod_api::ModEntry::new(
            horizon_quests::hq_meta::ID,
            horizon_quests::hooks(),
        )];
        mod_api::__private::install(mods.into());
        horizon_client::save::level_mods::set_loaded(Some(
            BUILTIN_MODS
                .iter()
                .map(|m| horizon_client::save::level_mods::ModRecord {
                    id: m.id.to_owned(),
                    version: m.version.to_owned(),
                })
                .collect(),
        ));
    });

    let mut bundled = Vec::new();
    if let Some(root) = horizon_client::brand::content() {
        for expected in BUILTIN_MODS {
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
        }
    } else {
        log::warn!("Tachyon: brand::content() not set when loading bundled native mods");
    }

    if bundled.len() == BUILTIN_MODS.len() {
        Runtime::from_bundled(BUILTIN_MODS, bundled)
    } else {
        Runtime::default()
    }
}
