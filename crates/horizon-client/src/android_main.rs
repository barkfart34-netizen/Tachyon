//! Tachyon on Android: the library the app's activity loads (`android/`). The
//! same game as `main.rs` (Horizon's), behind Tachyon's launcher drawn in the
//! game's window.
#![deny(unsafe_code)]
mod android;
mod tachyon;

#[allow(unsafe_code)]
#[unsafe(no_mangle)]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
    android::main(app);
}
