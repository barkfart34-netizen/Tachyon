//! The app around the game: logcat, the activity's Java side, the folders,
//! and Tachyon's launcher that runs before the game in the same window.
pub mod brand;
mod controls;
mod draw;
pub mod install;
pub mod jni;
mod launcher;
mod logcat;
mod shell;
mod ui;

use winit::event_loop::EventLoop;
use winit::platform::android::EventLoopBuilderExtAndroid;
use winit::platform::android::activity::AndroidApp;

/// This build of the app, as the build names it (`TACHYON_BUILD`).
pub const BUILD: &str = match option_env!("TACHYON_BUILD") {
    Some(build) => build,
    None => env!("CARGO_PKG_VERSION"),
};

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
    // Tachyon is not Horizon: no "playing now" ping to Horizon's servers.
    // SAFETY: as above.
    unsafe { std::env::set_var("HORIZON_NO_HEARTBEAT", "1") };
    let _ = std::env::set_current_dir(&paths.home);
    // A phone's first start: Tachyon's lighter defaults, Horizon's Performance
    // preset (8 chunks, fast clouds and leaves, fewer particles) at 60 FPS and
    // 75% resolution, instead of a desktop's.
    let options = paths.home.join("options.txt");
    if !options.exists() {
        let mut settings = horizon_client::settings::Settings::load(&options);
        horizon_client::graphics::apply_preset(
            horizon_client::graphics::Preset::Performance,
            &mut settings,
        );
        settings.max_fps = 60;
        settings.horizon.resolution_scale = 75;
        settings.save(&options);
    }
    horizon_client::telemetry::mark_start();
    horizon_client::logging::init(&paths.home, "Render thread");
    horizon_client::crash::install(paths.home.clone(), "client");
    jni::install_clipboard();
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
