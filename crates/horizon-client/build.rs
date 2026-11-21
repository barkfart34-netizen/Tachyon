//! Generates the native mod API, and gives the Windows executables Tachyon's icon
//! (`packaging/icons/tachyon.ico`) and 8 MB stack (`packaging/windows/exe.rs`).
#[path = "../../packaging/windows/exe.rs"]
#[allow(dead_code)]
mod windows_exe;
fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let root = std::path::Path::new(&dir).join("../..");
    let out_dir = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/horizon-client/src/game").display()
    );
    std::fs::write(
        out_dir.join("game_modules.rs"),
        horizon_game_discovery::declarations(
            &root.join("crates/horizon-client/src/game"),
            &[
                "native_hooks",
                "host",
                "creatures",
                "android",
                "orbital_glue",
                "tachyon_engine",
                "tachyon_mods",
                "touch_input",
            ],
        )
        .expect("discovering game modules"),
    )
    .expect("writing game modules");
    let tests =
        root.join("crates/horizon-client/src/crafting/ingredient/builtin_kinds_tests/families");
    println!("cargo:rerun-if-changed={}", tests.display());
    std::fs::write(
        out_dir.join("builtin_ingredient_test_modules.rs"),
        horizon_game_discovery::folder_modules(&tests).expect("discovering ingredient tests"),
    )
    .expect("writing ingredient test modules");
    horizon_mod_api_gen::generate(&root, &out_dir, horizon_mod_api_gen::Target::Client)
        .expect("generating native mod API");
    // Whether a native mod can run in a mob's turn (`game/creatures/mob_islands.rs`). The
    // launcher adds trait packages to the workspace's members.
    println!(
        "cargo:rerun-if-changed={}",
        root.join("Cargo.toml").display()
    );
    for dir in ["native-api", "native-crates"] {
        if root.join(dir).is_dir() {
            println!("cargo:rerun-if-changed={}", root.join(dir).display());
        }
    }
    if let Some(found) = horizon_mod_api_gen::mob_turn(&root) {
        println!("cargo:rustc-env=HORIZON_MOB_TURN={found}");
    }
    // The SDK builds this library, not the platform executable. Carry the same executable
    // policy without asking Cargo to link a binary this package no longer owns.
    let target_os = std::env::var("CARGO_CFG_TARGET_OS").expect("Cargo must provide the target OS");
    let target_env =
        std::env::var("CARGO_CFG_TARGET_ENV").expect("Cargo must provide the target environment");
    println!(
        "cargo:rerun-if-changed={}",
        root.join("packaging/executable_link.rs").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        root.join("crates/horizon-guest-stack/src/guest_stack_constants.rs")
            .display()
    );
    for arg in windows_exe::executable_link::arguments(&target_os, &target_env) {
        println!("cargo:horizon-executable-link-arg={arg}");
    }
    if std::env::var("CARGO_PKG_NAME").as_deref() != Ok("horizon-client") {
        windows_exe::resources("horizon-client");
    } else if target_os == "windows" {
        let manifest_dir =
            std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
        let ico = manifest_dir
            .join("../..")
            .join("packaging/icons/tachyon.ico");
        println!("cargo:rerun-if-changed={}", ico.display());
        println!("cargo:rerun-if-env-changed=WINDRES");
        println!("cargo:rerun-if-env-changed=AR");
        println!("cargo:rerun-if-env-changed=RC_PATH");
        for arg in windows_exe::executable_link::arguments(&target_os, &target_env) {
            println!("cargo:rustc-link-arg-bin=tachyon-desktop={arg}");
        }
        let (resource_manifest_dir, resource_icon) = match resource_path(&manifest_dir, &target_env)
            .and_then(|dir| Ok((dir, resource_path(&ico, &target_env)?)))
        {
            Ok(paths) => paths,
            Err(e) if std::env::var_os("CI").is_none() => {
                println!("cargo:warning=building without Tachyon's icon: {e}");
                return;
            }
            Err(e) => panic!("{e}"),
        };
        let mut resource = winresource::WindowsResource::new();
        resource.set_icon(&resource_icon);
        if std::env::var_os("CARGO_FEATURE_WINDOWS_SEGMENT_HEAP").is_some() {
            let manifest = manifest_dir.join("windows-segment-heap.manifest");
            println!("cargo:rerun-if-changed={}", manifest.display());
            let resource_manifest =
                resource_path(&manifest, &target_env).unwrap_or_else(|e| panic!("{e}"));
            resource.set_manifest_file(&resource_manifest);
        }
        let original_manifest_dir = std::env::var_os("CARGO_MANIFEST_DIR").unwrap();
        if cfg!(windows) && target_env == "gnu" {
            unsafe { std::env::set_var("CARGO_MANIFEST_DIR", &resource_manifest_dir) };
        }
        let result = resource.compile();
        if cfg!(windows) && target_env == "gnu" {
            unsafe { std::env::set_var("CARGO_MANIFEST_DIR", original_manifest_dir) };
        }
        result.expect("embedding Tachyon's icon in Windows resources");
    }
}

/// `path` as the resource compiler takes it: an error only when GNU windres would need an 8.3 alias
/// that the volume doesn't have.
fn resource_path(path: &std::path::Path, target_env: &str) -> Result<String, String> {
    let path = path.canonicalize().unwrap_or_else(|error| {
        panic!(
            "resolving Windows resource path {}: {error}",
            path.display()
        )
    });
    windows_path(path, target_env)
}

#[cfg(not(windows))]
fn windows_path(path: std::path::PathBuf, _target_env: &str) -> Result<String, String> {
    Ok(path.to_string_lossy().into_owned())
}

#[cfg(windows)]
fn windows_path(path: std::path::PathBuf, target_env: &str) -> Result<String, String> {
    let s = path.to_string_lossy();
    let trimmed = s.strip_prefix(r"\\?\").unwrap_or(&s);
    if target_env != "gnu" || trimmed.is_ascii() && !trimmed.chars().any(char::is_whitespace) {
        return Ok(trimmed.to_owned());
    }
    short_path(trimmed)
}

#[cfg(windows)]
fn short_path(path: &str) -> Result<String, String> {
    use std::os::windows::ffi::{OsStrExt, OsStringExt};

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetShortPathNameW(input: *const u16, output: *mut u16, capacity: u32) -> u32;
    }

    let input: Vec<u16> = std::ffi::OsStr::new(path)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut output = vec![0u16; 32_768];
    let length =
        unsafe { GetShortPathNameW(input.as_ptr(), output.as_mut_ptr(), output.len() as u32) };
    if length == 0 || length as usize >= output.len() {
        return Err(format!(
            "GNU windres needs an ASCII Windows resource path for {path}: {}",
            std::io::Error::last_os_error()
        ));
    }
    let short_path = std::ffi::OsString::from_wide(&output[..length as usize])
        .to_string_lossy()
        .into_owned();
    if !short_path.is_ascii() || short_path.chars().any(char::is_whitespace) {
        return Err(format!(
            "GNU windres needs an ASCII Windows resource path for {path}: the volume has no ASCII 8.3 path for this file"
        ));
    }
    Ok(short_path)
}
