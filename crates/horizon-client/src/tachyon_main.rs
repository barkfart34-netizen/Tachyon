//! Tachyon on the desktop: the launcher, and the game it starts as a second
//! process of this same program (`desktop::GAME` set in its environment).
#![forbid(unsafe_code)]
// No console window behind the launcher or the game: both log to files.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod desktop;
// The launchers' shared parts; some only the Android one uses.
#[allow(dead_code)]
mod tachyon;

fn main() {
    desktop::main();
}
