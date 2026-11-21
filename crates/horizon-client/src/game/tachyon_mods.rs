//! Horizon's official native mods (horizon-rust/horizon-mods, `native/`),
//! built into Tachyon: Coordinates HUD, FPS Counter, Keystrokes, Minimap,
//! Recipes and Zoom. Each bundle's module is here as it is (MIT, Copyright
//! (c) 2026 Ludovic Marie and the Horizon contributors: `tachyon_mods/LICENSE`);
//! its hooks, which horizon-mixins would weave into the hook points, are the
//! functions below, which the hook points call. Several mods on one hook run
//! as horizon-mixins orders them: by mod id.
//!
//! As for any native mod, Options > Mods turns each off (`on`).
mod coords_hud;
mod fps_overlay;
mod keystrokes;
mod minimap;
mod recipes;
mod recipes_glue;
mod zoom;

use super::Game;
use horizon_client::{
    extras::Corner,
    extras_hud::Corners,
    hud::HudBatch,
    mod_list::{
        Installed, KeyConflictContext, KeyMapping, NativePage, NativeSetting, NativeSettingValue,
    },
};
use std::time::Duration;

/// The four corners, in the order the mods' `corner` settings count them.
const CORNERS: [Corner; 4] = [
    Corner::TopLeft,
    Corner::TopRight,
    Corner::BottomLeft,
    Corner::BottomRight,
];

/// The four corners' labels for Options > Mods choice settings.
fn corner_options() -> Vec<String> {
    fps_overlay::CORNERS.iter().map(|c| c.1.to_owned()).collect()
}

fn key_mapping([id, label, key]: [&str; 3]) -> KeyMapping {
    KeyMapping {
        id: id.to_owned(),
        label: label.to_owned(),
        key: key.to_owned(),
        modifiers: 0,
        context: KeyConflictContext::Universal,
        category: String::new(),
    }
}

/// The compiled ports for the early loading display, before a Game exists.
/// Native ports are on until disabled, exactly as `on()` checks them.
pub(super) fn loading_mods(disabled: &std::collections::BTreeSet<String>) -> Vec<(String, String)> {
    [
        (coords_hud::ID, coords_hud::NAME),
        (fps_overlay::ID, fps_overlay::NAME),
        (keystrokes::ID, keystrokes::NAME),
        (minimap::ID, minimap::NAME),
        (recipes::ID, recipes::NAME),
        (zoom::ID, "Zoom"),
        (super::orbital_glue::ID, "Orbital Laser"),
    ]
    .into_iter()
    .filter(|(id, _)| !disabled.contains(*id))
    .map(|(id, name)| (id.to_owned(), name.to_owned()))
    .collect()
}

/// The hooks, for `native_hooks`.
impl Game<'_> {
    /// Reads a Tachyon built-in mod's saved key binding, or its default.
    pub(in crate::game) fn tachyon_mod_key(&self, mod_id: &str, name: &str, default: &str) -> String {
        self.settings.mods.binding(mod_id, name, (default, 0)).0
    }
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
    }
    pub(super) fn tachyon_wheel(&self, steps: f32, claimed: &mut bool) {
        if self.on(zoom::ID) && zoom::wheel(steps) {
            *claimed = true;
        }
    }
    pub(super) fn tachyon_command(&mut self, command: &str, ran: &mut bool) {
        coords_hud_command(self, command, ran);
    }
    pub(super) fn tachyon_zoom(&self, out: &mut Option<mod_api::Zoom>) {
        if out.is_none()
            && self.on(zoom::ID)
            && let Some((factor, smooth, turn)) = zoom::view()
        {
            *out = Some(mod_api::Zoom {
                factor,
                smooth,
                turn,
            });
        }
    }
    pub(super) fn tachyon_minimap(&self, out: &mut Option<mod_api::Minimap>) {
        if out.is_some() || !self.on(minimap::ID) {
            return;
        }
        let settings = minimap::settings(|key| self.stored_setting(minimap::ID, key));
        let (size, blocks_per_pixel) = settings.view();
        *out = Some(mod_api::Minimap {
            corner: mod_api::Corner::ALL[settings.corner],
            size,
            blocks_per_pixel,
            rotate: settings.rotate,
            round: settings.round,
        });
    }
    /// Options > Mods rows for Tachyon's built-in native mods and Orbital Laser.
    pub(super) fn tachyon_list_mods(&mut self) {
        let mut built_in: Vec<(Installed, Vec<KeyMapping>)> = Vec::new();
        built_in.push(super::orbital_glue::installed_row(self));
        let row = |id: &str, name: &str, version: &str, description: &str| Installed {
            id: id.into(),
            title: format!("{name} {version}"),
            description: description.into(),
            problem: None,
            dev: false,
            native: None,
        };
        built_in.push((
            row(
                coords_hud::ID,
                coords_hud::NAME,
                coords_hud::VERSION,
                coords_hud::DESCRIPTION,
            ),
            Vec::new(),
        ));
        built_in.push((
            row(
                fps_overlay::ID,
                fps_overlay::NAME,
                fps_overlay::VERSION,
                fps_overlay::DESCRIPTION,
            ),
            Vec::new(),
        ));
        built_in.push((
            row(
                keystrokes::ID,
                keystrokes::NAME,
                keystrokes::VERSION,
                keystrokes::DESCRIPTION,
            ),
            Vec::new(),
        ));
        built_in.push((
            row(
                minimap::ID,
                minimap::NAME,
                minimap::VERSION,
                minimap::DESCRIPTION,
            ),
            Vec::new(),
        ));
        built_in.push((
            row(
                recipes::ID,
                recipes::NAME,
                recipes::VERSION,
                recipes::DESCRIPTION,
            ),
            recipes::KEYS.into_iter().map(key_mapping).collect(),
        ));
        built_in.push((
            row(zoom::ID, "Zoom", zoom::VERSION, zoom::DESCRIPTION),
            vec![key_mapping([zoom::KEY, "Zoom key", zoom::DEFAULT_KEY])],
        ));
        self.settings
            .mods
            .installed
            .retain(|existing| !built_in.iter().any(|(m, _)| m.id == existing.id));
        for (mut row, mappings) in built_in {
            let settings = self.tachyon_page_settings(&row.id).unwrap_or_default();
            row.native = Some(NativePage { mappings, settings });
            self.settings.mods.installed.push(row);
        }
    }
    /// Settings shown on a Tachyon built-in mod's Options > Mods page.
    pub(super) fn tachyon_page_settings(&self, mod_id: &str) -> Option<Vec<NativeSetting>> {
        let get = |id: &str, key: &str| self.stored_setting(id, key);
        let toggle = |key: &str, label: &str, on: bool| NativeSetting {
            key: key.to_owned(),
            label: label.to_owned(),
            tooltip: None,
            value: NativeSettingValue::Toggle(on),
        };
        let choice = |key: &str, label: &str, options: Vec<String>, index: usize| NativeSetting {
            key: key.to_owned(),
            label: label.to_owned(),
            tooltip: None,
            value: NativeSettingValue::Choice { options, index },
        };
        Some(match mod_id {
            coords_hud::ID => vec![toggle(
                "shown",
                "Shown",
                coords_hud::shown(|| get(coords_hud::ID, "shown")),
            )],
            fps_overlay::ID => {
                let s = fps_overlay::settings(|k| get(fps_overlay::ID, k));
                vec![
                    choice("corner", "Corner", corner_options(), s.corner),
                    toggle("lows", "Minimum and 1% low", s.lows),
                ]
            }
            keystrokes::ID => {
                let s = keystrokes::settings(|k| get(keystrokes::ID, k));
                vec![
                    choice("corner", "Corner", corner_options(), s.corner),
                    toggle("cps", "Clicks per second", s.cps),
                ]
            }
            minimap::ID => {
                let s = minimap::settings(|k| get(minimap::ID, k));
                let sizes = minimap::SIZES.iter().map(|sz| sz.1.to_owned()).collect();
                let shapes = vec!["Square".to_owned(), "Round".to_owned()];
                let zooms = minimap::ZOOMS
                    .iter()
                    .map(|&n| {
                        if n < 1.0 {
                            "½ block per pixel".to_owned()
                        } else if n == 1.0 {
                            "1 block per pixel".to_owned()
                        } else {
                            format!("{n} blocks per pixel")
                        }
                    })
                    .collect();
                vec![
                    choice("corner", "Corner", corner_options(), s.corner),
                    choice("size", "Size", sizes, s.size),
                    choice("shape", "Shape", shapes, usize::from(s.round)),
                    choice("zoom", "Zoom", zooms, s.zoom),
                    toggle("rotate", "Turn with the player", s.rotate),
                ]
            }
            recipes::ID => vec![toggle(
                "give",
                "Click gives items (creative)",
                recipes::gives(get(recipes::ID, "give")),
            )],
            zoom::ID => {
                let s = zoom::settings(|k| get(zoom::ID, k));
                let levels = zoom::LEVELS.iter().map(|l| format!("{l}x")).collect();
                let index = zoom::LEVELS.iter().position(|&l| l == s.level).unwrap_or(0);
                vec![
                    choice("level", "Starting zoom", levels, index),
                    toggle("smooth", "Smooth zoom", s.smooth),
                    toggle("sensitivity", "Slower turning", s.sensitivity),
                ]
            }
            super::orbital_glue::ID => Vec::new(),
            _ => return None,
        })
    }
    /// Applies a setting change from Options > Mods for a Tachyon built-in mod.
    pub(super) fn tachyon_setting_event(
        &mut self,
        mod_id: &str,
        key: &str,
        value: &NativeSettingValue,
    ) -> bool {
        match mod_id {
            coords_hud::ID if key == "shown" => {
                if let NativeSettingValue::Toggle(shown) = *value {
                    coords_hud::set_shown(shown);
                    self.store_setting(coords_hud::ID, "shown", flag(shown));
                }
                true
            }
            fps_overlay::ID => {
                let mut s = fps_overlay::settings(|k| self.stored_setting(fps_overlay::ID, k));
                match (key, value) {
                    ("corner", NativeSettingValue::Choice { index, .. }) => {
                        s.corner = (*index).min(fps_overlay::CORNERS.len() - 1);
                    }
                    ("lows", NativeSettingValue::Toggle(on)) => s.lows = *on,
                    _ => return true,
                }
                fps_overlay::set_settings(s);
                self.keep(fps_overlay::ID, s.stored());
                true
            }
            keystrokes::ID => {
                let mut s = keystrokes::settings(|k| self.stored_setting(keystrokes::ID, k));
                match (key, value) {
                    ("corner", NativeSettingValue::Choice { index, .. }) => {
                        s.corner = (*index).min(keystrokes::CORNERS.len() - 1);
                    }
                    ("cps", NativeSettingValue::Toggle(on)) => s.cps = *on,
                    _ => return true,
                }
                keystrokes::set_settings(s);
                self.keep(keystrokes::ID, s.stored());
                true
            }
            minimap::ID => {
                let mut s = minimap::settings(|k| self.stored_setting(minimap::ID, k));
                match (key, value) {
                    ("corner", NativeSettingValue::Choice { index, .. }) => {
                        s.corner = (*index).min(minimap::CORNERS.len() - 1);
                    }
                    ("size", NativeSettingValue::Choice { index, .. }) => {
                        s.size = (*index).min(minimap::SIZES.len() - 1);
                    }
                    ("shape", NativeSettingValue::Choice { index, .. }) => s.round = *index == 1,
                    ("zoom", NativeSettingValue::Choice { index, .. }) => {
                        s.zoom = (*index).min(minimap::ZOOMS.len() - 1);
                    }
                    ("rotate", NativeSettingValue::Toggle(on)) => s.rotate = *on,
                    _ => return true,
                }
                minimap::set_settings(s);
                self.keep(minimap::ID, s.stored());
                true
            }
            recipes::ID if key == "give" => {
                if let NativeSettingValue::Toggle(give) = *value {
                    self.store_setting(recipes::ID, "give", flag(give));
                }
                true
            }
            zoom::ID => {
                let mut s = zoom::settings(|k| self.stored_setting(zoom::ID, k));
                match (key, value) {
                    ("level", NativeSettingValue::Choice { index, .. }) => {
                        s.level = zoom::LEVELS[(*index).min(zoom::LEVELS.len() - 1)];
                    }
                    ("smooth", NativeSettingValue::Toggle(on)) => s.smooth = *on,
                    ("sensitivity", NativeSettingValue::Toggle(on)) => s.sensitivity = *on,
                    _ => return true,
                }
                zoom::set_settings(s);
                self.keep(zoom::ID, s.stored());
                true
            }
            _ => false,
        }
    }
    pub(super) fn tachyon_panel_event(&mut self, mod_id: &str, event: &mod_api::PanelEvent) {
        if mod_id == recipes::ID && self.on(recipes::ID) {
            recipes_glue::event(self, event);
        }
    }
    /// Keeps a mod's stored settings.
    fn keep<const N: usize>(&self, mod_id: &str, stored: [(&'static str, String); N]) {
        for (key, value) in stored {
            self.store_setting(mod_id, key, &value);
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
    if !game.on(m::ID) || !game.native_corner_shown(Corner::TopLeft) {
        return;
    }
    if !m::shown(|| game.stored_setting(m::ID, "shown")) {
        return;
    }
    let Some(p) = game.native_player() else {
        return;
    };
    let lines = m::lines(p.x, p.y, p.z, p.yaw);
    let w = lines
        .iter()
        .map(|l| game.native_text_width(batch, l))
        .fold(0.0, f32::max)
        + 4.0;
    let h = 10.0 * lines.len() as f32 + 2.0;
    let (x, y) = corners.place(Corner::TopLeft, w, h);
    game.native_fill(batch, [x, y, w, h], m::BACKGROUND);
    for (i, line) in lines.iter().enumerate() {
        let at = [x + 2.0, y + 2.0 + 10.0 * i as f32];
        game.native_text(batch, line, at, 0xFF00_0000 | m::INK, true);
    }
}

/// `/where` puts the position in chat; `/coords` hides or shows the box.
fn coords_hud_command(game: &mut Game<'_>, command: &str, ran: &mut bool) {
    use coords_hud as m;
    let name = command.split(' ').next().unwrap_or("");
    if !game.on(m::ID) || !matches!(name, "where" | "coords") {
        return;
    }
    *ran = true;
    if name == "coords" {
        let shown = !m::shown(|| game.stored_setting(m::ID, "shown"));
        m::set_shown(shown);
        game.store_setting(m::ID, "shown", flag(shown));
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
    if !game.on(m::ID) {
        return;
    }
    let settings = m::settings(|key| game.stored_setting(m::ID, key));
    let corner = CORNERS[settings.corner];
    if !game.native_corner_shown(corner) {
        return;
    }
    let text = settings.text(game.native_frame_rate());
    let w = game.native_text_width(batch, &text) + 4.0;
    let (x, y) = corners.place(corner, w, 12.0);
    game.native_fill(batch, [x, y, w, 12.0], m::BACKGROUND);
    game.native_text(batch, &text, [x + 2.0, y + 2.0], 0xFF00_0000 | m::INK, true);
}

/// The keys in their corner, as the extra drew it.
fn keystrokes_hud(game: &mut Game<'_>, batch: &mut HudBatch, corners: &mut Corners) {
    use keystrokes as m;
    if !game.on(m::ID) {
        return;
    }
    let settings = m::settings(|key| game.stored_setting(m::ID, key));
    let corner = CORNERS[settings.corner];
    if !game.native_corner_shown(corner) {
        return;
    }
    let mut down = |name: &str| {
        let pressed = game.mapping(name).ok().flatten().is_some_and(|m| m.down);
        let label = horizon_client::keybinds::display_name(game.settings.keys.get(name)).plain();
        (pressed, label)
    };
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
    for shape in m::shapes(&view, &|text| game.native_text_width(batch, text)) {
        match shape {
            m::Shape::Fill([dx, dy, w, h], argb) => {
                game.native_fill(batch, [x + dx, y + dy, w, h], argb);
            }
            m::Shape::Text(text, [dx, dy], rgb, shadow) => {
                game.native_text(batch, &text, [x + dx, y + dy], 0xFF00_0000 | rgb, shadow);
            }
        }
    }
}

/// The zoom key went down or up: the player's choice of key, kept as the
/// WebAssembly Zoom kept it.
fn zoom_key(game: &mut Game<'_>, key: &str, pressed: bool) {
    if !game.on(zoom::ID) {
        return;
    }
    let bound = game.tachyon_mod_key(zoom::ID, zoom::KEY, zoom::DEFAULT_KEY);
    if key != bound {
        return;
    }
    let playing = game.playing();
    let game = &*game;
    zoom::key(playing, pressed, |name| game.stored_setting(zoom::ID, name));
}
