//! The app's folders, and the game's resources: Tachyon's APK carries them
//! ready made (`resources.zip`, prepared from Minecraft 26.3 by the build), so
//! the first start only unpacks them. No Java runs on the phone.
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use winit::platform::android::activity::AndroidApp;

/// The app's folders.
#[derive(Clone, Debug)]
pub struct Paths {
    /// The launcher's own data: the player's name.
    pub data: PathBuf,
    /// Where the resources are unpacked (`horizon-client/26.3` inside).
    pub root: PathBuf,
    /// The game's folder: worlds, options, logs. In the app's internal
    /// storage: the external one may not hold the worlds' session locks.
    pub home: PathBuf,
    /// A folder a computer reaches over USB: control layouts in and out.
    pub shared: PathBuf,
    pub cache: PathBuf,
}

impl Paths {
    pub fn new(app: &AndroidApp) -> Self {
        let internal = app
            .internal_data_path()
            .unwrap_or_else(|| PathBuf::from("/data/local/tmp/tachyon"));
        let shared = app
            .external_data_path()
            .filter(|p| fs::create_dir_all(p).is_ok())
            .unwrap_or_else(|| internal.join("shared"));
        Self {
            data: internal.join("launcher"),
            root: internal.join("resources"),
            home: internal.join("game"),
            shared,
            cache: internal.join("cache"),
        }
    }
    pub fn resources(&self) -> PathBuf {
        self.root
            .join("horizon-client")
            .join(horizon_core::VANILLA_VERSION)
    }
    /// The unpacked resources' id, written once they are all there.
    fn stamp(&self) -> PathBuf {
        self.root.join(".unpacked")
    }
    pub fn prepared(&self) -> bool {
        self.resources().join("horizon-resources.json").is_file() && self.stamp().is_file()
    }
}

/// A step on its own thread: the last line it logged, how far it is, and how it ended.
#[derive(Clone, Default)]
pub struct Progress {
    pub line: String,
    pub fraction: Option<f32>,
    pub done: Option<Result<(), String>>,
}

pub struct Task {
    pub what: &'static str,
    state: Arc<Mutex<Progress>>,
}

/// What a task's work reports through.
pub struct Report<'a> {
    state: &'a Mutex<Progress>,
}

impl Report<'_> {
    pub fn line(&self, line: String) {
        log::info!("Tachyon launcher: {line}");
        self.state.lock().unwrap().line = line;
    }
    pub fn fraction(&self, fraction: f32) {
        self.state.lock().unwrap().fraction = Some(fraction);
    }
}

impl Task {
    pub fn spawn(
        what: &'static str,
        work: impl FnOnce(&Report) -> Result<(), String> + Send + 'static,
    ) -> Self {
        let state = Arc::new(Mutex::new(Progress::default()));
        let thread_state = state.clone();
        std::thread::spawn(move || {
            let report = Report {
                state: &thread_state,
            };
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&report)))
                .unwrap_or_else(|_| Err("The launcher's worker stopped unexpectedly".into()));
            if let Err(e) = &result {
                log::warn!("Tachyon launcher: {what} failed: {e}");
            }
            thread_state.lock().unwrap().done = Some(result);
        });
        Self { what, state }
    }
    pub fn progress(&self) -> Progress {
        self.state.lock().unwrap().clone()
    }
}

/// A small text asset of the APK.
fn asset_text(app: &AndroidApp, name: &str) -> Option<String> {
    let file = std::ffi::CString::new(name).ok()?;
    let mut asset = app.asset_manager().open(&file)?;
    let mut text = String::new();
    std::io::Read::read_to_string(&mut asset, &mut text).ok()?;
    Some(text.trim().to_owned())
}

/// Whether the resources the APK carries are unpacked already (the same id).
pub fn up_to_date(app: &AndroidApp, paths: &Paths) -> bool {
    paths.prepared()
        && asset_text(app, "resources.id")
            .is_some_and(|id| fs::read_to_string(paths.stamp()).is_ok_and(|s| s.trim() == id))
}

/// Unpacks `resources.zip` from the APK into the resources folder, unless the
/// same resources are there already. A new APK with other resources replaces
/// them; the worlds and settings live elsewhere and stay.
pub fn install(app: &AndroidApp, paths: &Paths, report: &Report) -> Result<(), String> {
    if up_to_date(app, paths) {
        return Ok(());
    }
    let id = asset_text(app, "resources.id")
        .ok_or("This build of Tachyon carries no resources (resources.id is missing)")?;
    report.line("Unpacking Minecraft's resources (the first start only)".into());
    let partial = paths.root.with_extension("partial");
    let _ = fs::remove_dir_all(&partial);
    fs::create_dir_all(&partial).map_err(|e| e.to_string())?;
    let file = std::ffi::CString::new("resources.zip").unwrap();
    let asset = app
        .asset_manager()
        .open(&file)
        .ok_or("This build of Tachyon carries no resources.zip")?;
    let mut zip = zip::ZipArchive::new(asset).map_err(|e| format!("resources.zip: {e}"))?;
    let count = zip.len();
    for i in 0..count {
        let mut entry = zip.by_index(i).map_err(|e| format!("resources.zip: {e}"))?;
        let Some(relative) = entry.enclosed_name() else {
            continue;
        };
        let out = partial.join(relative);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| e.to_string())?;
            continue;
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let mut file = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        std::io::copy(&mut entry, &mut file).map_err(|e| format!("{}: {e}", out.display()))?;
        if i % 64 == 0 {
            report.fraction(i as f32 / count as f32);
        }
    }
    report.fraction(1.0);
    let _ = fs::remove_dir_all(&paths.root);
    fs::rename(&partial, &paths.root).map_err(|e| e.to_string())?;
    fs::write(paths.stamp(), id).map_err(|e| e.to_string())?;
    report.line("Ready".into());
    Ok(())
}

/// The player's name, kept by the launcher.
pub fn player_name(paths: &Paths) -> Option<String> {
    let name = fs::read_to_string(paths.data.join("name.txt")).ok()?;
    let name = name.trim();
    (!name.is_empty()).then(|| name.to_owned())
}

pub fn set_player_name(paths: &Paths, name: &str) -> Result<(), String> {
    fs::create_dir_all(&paths.data).map_err(|e| e.to_string())?;
    fs::write(paths.data.join("name.txt"), name.trim()).map_err(|e| e.to_string())
}

/// A name vanilla accepts for an offline player: 3 to 16 of `A-Z a-z 0-9 _`.
pub fn valid_name(name: &str) -> bool {
    (3..=16).contains(&name.len()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// The game's arguments for Play: the resources and the player's name.
pub fn game_args(paths: &Paths) -> Vec<String> {
    let mut args = vec![
        "--resources".to_owned(),
        paths.resources().display().to_string(),
    ];
    if let Some(name) = player_name(paths) {
        args.push("--name".into());
        args.push(name);
    }
    args
}

/// Bytes free where the resources go, if the system says.
pub fn free_space(dir: &Path) -> Option<u64> {
    let dir = std::ffi::CString::new(dir.as_os_str().as_encoded_bytes()).ok()?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    // SAFETY: a valid C string and a statvfs to fill.
    #[allow(unsafe_code)]
    let ok = unsafe { libc::statvfs(dir.as_ptr(), stat.as_mut_ptr()) } == 0;
    // SAFETY: statvfs filled it.
    #[allow(unsafe_code)]
    let stat = unsafe { stat.assume_init() };
    ok.then(|| stat.f_bavail as u64 * stat.f_frsize as u64)
}
