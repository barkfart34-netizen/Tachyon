//! The name the game shows for itself in its menus: Horizon, unless a build
//! of it under another name (Tachyon's Android app) sets its own at start.
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static NAME: OnceLock<&'static str> = OnceLock::new();

/// Sets the name, once, before the first frame.
pub fn set(name: &'static str) {
    let _ = NAME.set(name);
}

pub fn name() -> &'static str {
    NAME.get().copied().unwrap_or("Horizon")
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
        assert_eq!(super::titled("Settings"), format!("{} Settings", super::name()));
    }
}
