//! The app around the game: logcat, the activity's Java side, the folders,
//! and Tachyon's launcher that runs before the game in the same window.
mod controls;
pub mod install;
pub mod jni;
mod launcher;
mod logcat;
mod shell;

use crate::tachyon::brand;
use winit::event_loop::EventLoop;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp;

/// This build of the app, as the build names it (`TACHYON_BUILD`).
pub const BUILD: &str = match option_env!("TACHYON_BUILD") {
    Some(build) => build,
    None => env!("CARGO_PKG_VERSION"),
};

/// Tachyon's lighter settings for a phone, applied once per version of them
/// (`tachyon-defaults` in the game folder): Horizon's Performance preset (8
/// chunks, fast clouds and leaves, fewer particles), 75% resolution, VSync
/// up to 120 FPS (a 90 or 120 Hz screen gets its rate), and 4 worker threads
/// for a phone's few big cores.
fn phone_defaults(home: &std::path::Path) {
    const VERSION: &str = "2";
    let marker = home.join("tachyon-defaults");
    if std::fs::read_to_string(&marker).is_ok_and(|v| v.trim() == VERSION) {
        return;
    }
    let options = home.join("options.txt");
    let mut settings = horizon_client::settings::Settings::load(&options);
    horizon_client::graphics::apply_preset(
        horizon_client::graphics::Preset::Performance,
        &mut settings,
    );
    settings.max_fps = 120;
    settings.vsync = true;
    settings.horizon.resolution_scale = 75;
    settings.horizon.workers = 4;
    settings.save(&options);
    let _ = std::fs::write(marker, VERSION);
    log::info!("Tachyon: phone defaults {VERSION} applied");
}

/// The last game's crash, written when it stopped with an error.
fn take_crash(paths: &install::Paths) -> Option<String> {
    let file = paths.data.join("last-crash.txt");
    let text = std::fs::read_to_string(&file).ok()?;
    let _ = std::fs::remove_file(file);
    Some(text)
}

#[allow(unsafe_code)]
pub fn main(app: AndroidApp) {
    logcat::redirect();
    jni::set_activity(&app);
    let paths = install::Paths::new(&app);
    for dir in [&paths.data, &paths.home, &paths.cache] {
        let _ = std::fs::create_dir_all(dir);
    }
    // SAFETY: no other thread reads the environment yet (the logcat thread
    // only copies a pipe).
    unsafe { std::env::set_var("HORIZON_HOME", &paths.home) };
    // SAFETY: as above.
    unsafe { std::env::set_var("TMPDIR", &paths.cache) };
    // The GPU timer costs a phone frame bandwidth: only for the graph overlay.
    let graphics = horizon_client::graphics::Graphics::load(&paths.home);
    if graphics.overlay == horizon_client::graphics::Overlay::Graph {
        // SAFETY: as above.
        unsafe { std::env::set_var("TACHYON_GPU_TIMER", "1") };
    }
    // Tachyon is not Horizon: no "playing now" ping to Horizon's servers.
    // SAFETY: as above.
    unsafe { std::env::set_var("HORIZON_NO_HEARTBEAT", "1") };
    let _ = std::env::set_current_dir(&paths.home);
    horizon_client::telemetry::mark_start();
    horizon_client::logging::init(&paths.home, "Render thread");
    horizon_client::crash::install(paths.home.clone(), "client");
    jni::install_clipboard();
    horizon_client::brand::set(brand::NAME);
    phone_defaults(&paths.home);
    crate::tachyon::mods::settle(&paths.home);
    log::info!(
        "{} {BUILD} on {}",
        brand::NAME,
        jni::platform("model", "").unwrap_or_default()
    );
    let crash = take_crash(&paths);
    let event_loop = match EventLoop::builder().with_android_app(app.clone()).build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            log::error!("Tachyon: no event loop: {e}");
            return;
        }
    };
    jni::set_wake(event_loop.create_proxy());
    let data = paths.data.clone();
    let home = paths.home.clone();
    let mut shell = shell::Shell::new(launcher::Launcher::new(app, paths, crash));
    if let Err(e) = event_loop.run_app(&mut shell) {
        log::error!("Tachyon: the event loop stopped: {e}");
    }
    horizon_client::discord_presence::stop();
    if let Some(error) = shell.error.take() {
        log::error!("Horizon: {error}");
        horizon_client::crash::report_error(&home, "client", &*error);
        let _ = std::fs::write(data.join("last-crash.txt"), error.to_string());
    }
    horizon_client::logging::flush();
    // A finished activity's process may host the next one, which needs a new
    // event loop: winit makes only one per process. `_exit` skips the C++
    // exit handlers, which the activity's UI threads still running crash in.
    // SAFETY: the game saved and the log is flushed; nothing is left to run.
    unsafe { libc::_exit(0) }
}
