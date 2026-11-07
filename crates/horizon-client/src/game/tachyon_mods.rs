//! Horizon's official native mods (horizon-rust/horizon-mods, `native/`),
//! built into Tachyon: Coordinates HUD, FPS Counter, Keystrokes, Minimap,
//! Recipes and Zoom. Each bundle's module is here as it is (MIT, Copyright
//! (c) 2026 Ludovic Marie and the Horizon contributors: `tachyon_mods/LICENSE`);
//! its hooks, which horizon-mixins would weave into the hook points, are the
//! functions below, which the hook points call. Several mods on one hook run
//! as horizon-mixins orders them: by mod id.
//!
//! As for any native mod, Options > Mods turns each off (`native_on`).
mod coords_hud;
mod fps_overlay;
mod keystrokes;
mod minimap;
mod recipes;
mod recipes_glue;
mod zoom;

use super::{
    Game,
    native_hooks::{NativeMinimap, NativeMod, NativePanelEvent, NativeZoom},
};
use horizon_client::{extras::Corner, extras_hud::Corners, hud::HudBatch};
use std::time::Duration;

/// The four corners, in the order the mods' `corner` settings count them.
const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomLeft,
    Corner::BottomRight,
];

/// The compiled ports for the early loading display, before a Game exists.
/// Native ports are on until disabled, exactly as native_on checks them.
pub(super) fn loading_mods(disabled: &std::collections::BTreeSet<String>) -> Vec<(String, String)> {
    [
        (coords_hud::ID, coords_hud::NAME),
        (fps_overlay::ID, fps_overlay::NAME),
        (keystrokes::ID, keystrokes::NAME),
        (minimap::ID, minimap::NAME),
        (recipes::ID, recipes::NAME),
        (zoom::ID, "Zoom"),
        (super::tachyon_engine::chronicles::ID, "Chronicles"),
    ]
    .into_iter()
    .filter(|(id, _)| !disabled.contains(*id))
    .map(|(id, name)| (id.to_owned(), name.to_owned()))
    .collect()
}

/// The hooks, for `native_hooks.rs`.
impl Game<'_> {
    pub(super) fn tachyon_frame(&mut self, _elapsed: Duration) {
        recipes_glue::frame(self);
    }
    pub(super) fn tachyon_hud(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        coords_hud_box(self, batch, corners);
        fps_hud(self, batch, corners);
        keystrokes_hud(self, batch, corners);
        self.tachyon_engine_hud(batch, corners);
        self.tachyon_assist_hud(batch);
    }
    pub(super) fn tachyon_key(&mut self, key: &str, pressed: bool) {
        zoom_key(self, key, pressed);
        self.tachyon_quests_key(key, pressed);
    }
    pub(super) fn tachyon_wheel(&self, steps: f32, claimed: &mut bool) {
        if self.native_on(zoom::ID) && zoom::wheel(steps) {
            *claimed = true;
        }
    }
    pub(super) fn tachyon_command(&mut self, command: &str, ran: &mut bool) {
        coords_hud_command(self, command, ran);
    }
    pub(super) fn tachyon_zoom(&self, out: &mut Option<NativeZoom>) {
        if out.is_none()
            && self.native_on(zoom::ID)
            && let Some((factor, smooth, turn)) = zoom::view()
        {
            *out = Some(NativeZoom {
                factor,
                smooth,
                turn,
            });
        }
    }
    pub(super) fn tachyon_minimap(&self, out: &mut Option<NativeMinimap>) {
        if out.is_some() || !self.native_on(minimap::ID) {
            return;
        }
        let settings = minimap::settings(|key| self.native_setting(minimap::ID, key));
        let (size, blocks_per_pixel) = settings.view();
        *out = Some(NativeMinimap {
            corner: CORNERS[settings.corner],
            size,
            blocks_per_pixel,
            rotate: settings.rotate,
            round: settings.round,
        });
    }
    pub(super) fn tachyon_mods(&mut self, mods: &mut Vec<NativeMod>) {
        let row = |id: &str, name: &str, version: &str, description: &str| NativeMod {
            id: id.into(),
            title: format!("{name} {version}"),
            description: description.into(),
            keys: Vec::new(),
        };
        mods.push(row(
            coords_hud::ID,
            coords_hud::NAME,
            coords_hud::VERSION,
            coords_hud::DESCRIPTION,
        ));
        mods.push(row(
            fps_overlay::ID,
            fps_overlay::NAME,
            fps_overlay::VERSION,
            fps_overlay::DESCRIPTION,
        ));
        mods.push(row(
            keystrokes::ID,
            keystrokes::NAME,
            keystrokes::VERSION,
            keystrokes::DESCRIPTION,
        ));
        mods.push(row(
            minimap::ID,
            minimap::NAME,
            minimap::VERSION,
            minimap::DESCRIPTION,
        ));
        mods.push(NativeMod {
            keys: recipes::KEYS.iter().map(|k| k.map(String::from)).collect(),
            ..row(
                recipes::ID,
                recipes::NAME,
                recipes::VERSION,
                recipes::DESCRIPTION,
            )
        });
        mods.push(NativeMod {
            keys: vec![[
                zoom::KEY.into(),
                "Zoom key".into(),
                zoom::DEFAULT_KEY.into(),
            ]],
            ..row(zoom::ID, "Zoom", zoom::VERSION, zoom::DESCRIPTION)
        });
        use super::tachyon_engine::chronicles;
        mods.push(NativeMod {
            keys: vec![[
                chronicles::KEY.into(),
                "Quest book".into(),
                chronicles::DEFAULT_KEY.into(),
            ]],
            ..row(
                chronicles::ID,
                "Chronicles",
                "0.1.0",
                "The modpack's quests: a book of what to do next, and rewards.",
            )
        });
    }
    pub(super) fn tachyon_settings(&mut self, mod_id: &str, buttons: &mut Vec<String>) {
        let get = |id: &str, key: &str| self.native_setting(id, key);
        match mod_id {
            coords_hud::ID => buttons.push(coords_hud::shown_label(coords_hud::shown(|| {
                get(coords_hud::ID, "shown")
            }))),
            fps_overlay::ID => {
                buttons.extend(fps_overlay::settings(|k| get(fps_overlay::ID, k)).labels());
            }
            keystrokes::ID => {
                buttons.extend(keystrokes::settings(|k| get(keystrokes::ID, k)).labels());
            }
            minimap::ID => buttons.extend(minimap::settings(|k| get(minimap::ID, k)).labels()),
            recipes::ID => {
                buttons.push(recipes::give_label(recipes::gives(get(
                    recipes::ID,
                    "give",
                ))));
            }
            zoom::ID => buttons.extend(zoom::settings(|k| get(zoom::ID, k)).labels()),
            _ => {}
        }
    }
    pub(super) fn tachyon_settings_click(&mut self, mod_id: &str, button: usize) {
        match mod_id {
            coords_hud::ID if button == 0 => {
                let shown = !coords_hud::shown(|| self.native_setting(coords_hud::ID, "shown"));
                coords_hud::set_shown(shown);
                self.native_set_setting(coords_hud::ID, "shown", flag(shown));
            }
            fps_overlay::ID => {
                let mut s = fps_overlay::settings(|k| self.native_setting(fps_overlay::ID, k));
                s.click(button);
                fps_overlay::set_settings(s);
                self.keep(fps_overlay::ID, s.stored());
            }
            keystrokes::ID => {
                let mut s = keystrokes::settings(|k| self.native_setting(keystrokes::ID, k));
                s.click(button);
                keystrokes::set_settings(s);
                self.keep(keystrokes::ID, s.stored());
            }
            minimap::ID => {
                let mut s = minimap::settings(|k| self.native_setting(minimap::ID, k));
                s.click(button);
                minimap::set_settings(s);
                self.keep(minimap::ID, s.stored());
            }
            recipes::ID if button == 0 => {
                let give = !recipes::gives(self.native_setting(recipes::ID, "give"));
                self.native_set_setting(recipes::ID, "give", flag(give));
            }
            zoom::ID => {
                let mut s = zoom::settings(|k| self.native_setting(zoom::ID, k));
                s.click(button);
                zoom::set_settings(s);
                self.keep(zoom::ID, s.stored());
            }
            _ => {}
        }
    }
    pub(super) fn tachyon_panel_event(&mut self, mod_id: &str, event: &NativePanelEvent) {
        if mod_id == recipes::ID && self.native_on(recipes::ID) {
            recipes_glue::event(self, event);
        }
    }
    /// Keeps a mod's stored settings.
    fn keep<const N: usize>(&self, mod_id: &str, stored: [(&'static str, String); N]) {
        for (key, value) in stored {
            self.native_set_setting(mod_id, key, &value);
        }
    }
}

fn flag(on: bool) -> &'static str {
    if on { "true" } else { "false" }
}

/// Coordinates HUD's box in the top-left corner, drawn as the WebAssembly
/// mods' panels were; the debug screen takes that corner.
fn coords_hud_box(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    use coords_hud as m;
    if !game.native_on(m::ID) || !game.native_corner_shown(Corner::TopLeft) {
        return;
    }
    if !m::shown(|| game.native_setting(m::ID, "shown")) {
        return;
    }
    let Some(p) = game.native_player() else {
        return;
    };
    let lines = m::lines(p.x, p.y, p.z, p.yaw);
    let w = lines
        .iter()
        .map(|l| game.native_text_width(l))
        .fold(0.0, f32::max)
        + 4.0;
    let h = 10.0 * lines.len() as f32 + 2.0;
    let (x, y) = corners.place(Corner::TopLeft, w, h);
    game.native_fill(batch, [x, y, w, h], m::BACKGROUND);
    for (i, line) in lines.iter().enumerate() {
        let at = [x + 2.0, y + 2.0 + 10.0 * i as f32];
        game.native_text(batch, line, at, m::INK, true);
    }
}

/// `/where` puts the position in chat; `/coords` hides or shows the box.
fn coords_hud_command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    use coords_hud as m;
    let name = command.split(' ').next().unwrap_or("");
    if !game.native_on(m::ID) || !matches!(name, "where" | "coords") {
        return;
    }
    *ran = true;
    if name == "coords" {
        let shown = !m::shown(|| game.native_setting(m::ID, "shown"));
        m::set_shown(shown);
        game.native_set_setting(m::ID, "shown", flag(shown));
        game.native_chat(if shown {
            "Coordinates panel shown"
        } else {
            "Coordinates panel hidden"
        });
        return;
    }
    match game.native_player() {
        Some(p) => game.native_chat(&m::where_line(p.x, p.y, p.z, p.dimension)),
        None => game.native_chat("Not in a world"),
    }
}

/// The FPS counter's box in its corner, as the extra drew it.
fn fps_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    use fps_overlay as m;
    if !game.native_on(m::ID) {
        return;
    }
    let settings = m::settings(|key| game.native_setting(m::ID, key));
    let corner = CORNERS[settings.corner];
    if !game.native_corner_shown(corner) {
        return;
    }
    let text = settings.text(game.native_frame_rate());
    let w = game.native_text_width(&text) + 4.0;
    let (x, y) = corners.place(corner, w, 12.0);
    game.native_fill(batch, [x, y, w, 12.0], m::BACKGROUND);
    game.native_text(batch, &text, [x + 2.0, y + 2.0], m::INK, true);
}

/// The keys in their corner, as the extra drew them.
fn keystrokes_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    use keystrokes as m;
    if !game.native_on(m::ID) {
        return;
    }
    let settings = m::settings(|key| game.native_setting(m::ID, key));
    let corner = CORNERS[settings.corner];
    if !game.native_corner_shown(corner) {
        return;
    }
    let mut down = |name: &str| game.native_mapping(name).unwrap_or_default();
    let view = m::View {
        moves: m::MOVES.map(&mut down),
        jump: down("key.jump").0,
        attack: down("key.attack").0,
        use_: down("key.use").0,
        cps: None,
    };
    let view = m::View {
        cps: settings.cps.then(|| game.native_clicks()),
        ..view
    };
    let (w, h) = m::SIZE;
    let (x, y) = corners.place(corner, w, h);
    let game = &*game;
    for shape in m::shapes(&view, &|text| game.native_text_width(text)) {
        match shape {
            m::Shape::Fill([dx, dy, w, h], argb) => {
                game.native_fill(batch, [x + dx, y + dy, w, h], argb);
            }
            m::Shape::Text(text, [dx, dy], rgb, shadow) => {
                game.native_text(batch, &text, [x + dx, y + dy], rgb, shadow);
            }
        }
    }
}

/// The zoom key went down or up: the player's choice of key, kept as the
/// WebAssembly Zoom kept it.
fn zoom_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !game.native_on(zoom::ID) {
        return;
    }
    let bound = game.native_mod_key(zoom::ID, zoom::KEY, zoom::DEFAULT_KEY);
    if key != bound {
        return;
    }
    let playing = game.native_playing();
    let game = &*game;
    zoom::key(playing, pressed, |name| game.native_setting(zoom::ID, name));
}
