//! The official native mods built into Tachyon (`game/tachyon_mods.rs`)
//! that a new game folder has off: the FPS counter and Keystrokes, for the
//! player to turn on in Options > Mods. The others (Coordinates HUD,
//! Minimap, Recipes, Zoom) are on, as any native mod is.
use horizon_client::mod_list::ModList;
use std::path::Path;

const OFF: [&str; 2] = ["fps_overlay", "keystrokes"];

/// Their `mod.<id>:false` lines in `dir`'s `horizon-mods.txt`, for the
/// mods its player never turned on or off.
pub fn settle(dir: &Path) {
    let mut list = ModList::load(dir);
    let mut changed = false;
    for id in OFF {
        if !list.enabled.contains(id) && !list.disabled.contains(id) {
            changed |= list.disabled.insert(id.to_owned());
        }
    }
    if changed {
        list.save(dir);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_once_and_the_players_choice_kept() {
        let dir = std::env::temp_dir().join(format!("tachyon-mods-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        settle(&dir);
        let list = ModList::load(&dir);
        assert!(list.disabled.contains("fps_overlay") && list.disabled.contains("keystrokes"));
        // The player turned the counter on: it stays on.
        let mut list = ModList::load(&dir);
        list.disabled.remove("fps_overlay");
        list.enabled.insert("fps_overlay".into());
        list.save(&dir);
        settle(&dir);
        let list = ModList::load(&dir);
        assert!(list.enabled.contains("fps_overlay") && !list.disabled.contains("fps_overlay"));
        assert!(list.disabled.contains("keystrokes"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
