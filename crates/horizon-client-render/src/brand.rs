//! The name the game shows for itself in its menus: Horizon, unless a build
//! of it under another name (Tachyon's Android app) sets its own at start;
//! and the content mods such a build carries built in.
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

static NAME: OnceLock<&'static str> = OnceLock::new();
static CONTENT: OnceLock<PathBuf> = OnceLock::new();

/// Sets the name, once, before the first frame.
pub fn set(name: &'static str) {
    let _ = NAME.set(name);
}

pub fn name() -> &'static str {
    NAME.get().copied().unwrap_or("Horizon")
}

/// Sets the folder of the content mods the build carries (Tachyon's
/// `tachyon-mods` in its resources), once, before the resources load. They
/// are always on, and the Mods page doesn't list them.
pub fn set_content(dir: PathBuf) {
    let _ = CONTENT.set(dir);
}

/// The built-in content mods' folder, if the build has one.
pub fn content() -> Option<&'static Path> {
    CONTENT.get().map(PathBuf::as_path)
}

/// `"<name> <rest>"` (e.g. "Horizon Settings"), kept for the program's life:
/// the menus take their titles as `&'static str`.
pub fn titled(rest: &'static str) -> &'static str {
    static TITLES: Mutex<Option<HashMap<&'static str, &'static str>>> = Mutex::new(None);
    let mut titles = TITLES.lock().unwrap();
    titles
        .get_or_insert_with(HashMap::new)
        .entry(rest)
        .or_insert_with(|| Box::leak(format!("{} {rest}", name()).into_boxed_str()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn titles_carry_the_name() {
        assert_eq!(
            super::titled("Settings"),
            format!("{} Settings", super::name())
        );
    }
}
