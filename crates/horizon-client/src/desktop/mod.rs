//! Tachyon's desktop program: the launcher, or, with `GAME` in its
//! environment, the game the launcher started in an installation's folder.
mod launcher;
mod store;

use crate::tachyon::brand;
use horizon_client::{crash, logging};
use std::path::{Path, PathBuf};
use winit::event_loop::EventLoop;

/// This build, as the release names it (`TACHYON_BUILD`).
pub const BUILD: &str = match option_env!("TACHYON_BUILD") {
    Some(build) => build,
    None => env!("CARGO_PKG_VERSION"),
};

/// Set for the game the launcher starts: the program runs the game, not the launcher.
const GAME: &str = "TACHYON_GAME";
/// Why the game stopped, in its folder, for the launcher to show.
const ERROR_FILE: &str = "tachyon-last-error.txt";
/// Tachyon's icon for the windows (the .exe carries it too, `build.rs`).
const ICON: &[u8] = include_bytes!("../tachyon/icon-64.png");

/// Where the launcher keeps its things, and the game's resources.
pub struct Paths {
    /// `%APPDATA%\.tachyon`: the launcher's settings and log, and the
    /// installations' folders.
    pub data: PathBuf,
    /// Minecraft 26.3's prepared resources: `resources` beside the program
    /// (as the release unpacks), or `TACHYON_RESOURCES`. None if missing.
    pub resources: Option<PathBuf>,
}

impl Paths {
    fn new() -> Self {
        let prepared = |dir: &Path| dir.join("horizon-resources.json").is_file();
        let beside = std::env::current_exe()
            .ok()
            .and_then(|exe| Some(exe.parent()?.join("resources")));
        let resources = std::env::var_os("TACHYON_RESOURCES")
            .map(PathBuf::from)
            .into_iter()
            .chain(beside)
            // A development checkout's, as Horizon's own build finds them.
            .chain(std::env::current_dir().ok().map(|d| {
                d.join(".local/horizon-client")
                    .join(horizon_core::VANILLA_VERSION)
            }))
            .find(|dir| prepared(dir))
            .map(|dir| std::path::absolute(&dir).unwrap_or(dir));
        Self {
            data: logging::data_dir().join(".tachyon"),
            resources,
        }
    }
}

pub fn main() {
    if std::env::var_os(GAME).is_some() {
        std::process::exit(game());
    }
    let paths = Paths::new();
    let _ = std::fs::create_dir_all(&paths.data);
    logging::init(&paths.data, "Launcher");
    crash::install(paths.data.clone(), "launcher");
    horizon_client::brand::set(brand::NAME);
    horizon_client::window::set_icon(ICON);
    log::info!(
        "{} launcher {BUILD}, resources {:?}",
        brand::NAME,
        paths.resources
    );
    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            log::error!("Tachyon: no event loop: {e}");
            return;
        }
    };
    let mut launcher = launcher::Launcher::new(paths);
    if let Err(e) = event_loop.run_app(&mut launcher) {
        log::error!("Tachyon: the event loop stopped: {e}");
    }
    logging::flush();
}

/// The game, in the folder the launcher gave it (`HORIZON_HOME`, also the
/// working directory), as `main.rs` runs Horizon. Returns the exit code.
fn game() -> i32 {
    horizon_client::telemetry::mark_start();
    let home = logging::game_home();
    logging::init(&home, "Render thread");
    crash::install(home.clone(), "client");
    horizon_client::brand::set(brand::NAME);
    horizon_client::window::set_icon(ICON);
    log::info!("{} {BUILD} (Horizon {})", brand::NAME, crash::version());
    let _ = std::fs::remove_file(home.join(ERROR_FILE));
    let code = match crate::game::run() {
        Ok(()) => 0,
        Err(error) => {
            log::error!("Tachyon: {error}");
            crash::report_error(&home, "client", &*error);
            let _ = std::fs::write(home.join(ERROR_FILE), error.to_string());
            1
        }
    };
    logging::flush();
    code
}

/// Opens `dir` in the file manager.
fn open_folder(dir: &Path) {
    let _ = std::fs::create_dir_all(dir);
    #[cfg(windows)]
    let opened = open::that_detached(dir);
    #[cfg(not(windows))]
    let opened = std::process::Command::new(if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    })
    .arg(dir)
    .spawn()
    .map(drop);
    if let Err(e) = opened {
        log::warn!("Tachyon launcher: {} not opened: {e}", dir.display());
    }
}
