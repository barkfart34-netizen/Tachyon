//! Tachyon's desktop launcher, laid out after the Minecraft Launcher: the
//! account and the game in a sidebar; the game's tabs (Play, Installations,
//! Accounts, Patch notes) under its title; the panorama; and a bar with the
//! installation, PLAY and the player. Accounts are offline ones: a name.
//! The game runs as a second process of this program in the installation's
//! folder; the launcher hides while it runs and comes back when it stops.
use super::store::{self, Account, Installation, Store};
use super::{ERROR_FILE, GAME, ICON, Paths};
use crate::tachyon::brand::{self, P};
use crate::tachyon::draw::{self, Face, Image, Rect, Renderer, hex};
use crate::tachyon::ui::{Pointer, Ui};
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::window::{CursorIcon, Window, WindowId};

const SIDEBAR: f32 = 248.0;
const HEADER: f32 = 116.0;
const BAR: f32 = 100.0;
/// A panorama face in the atlas, in pixels: four fit in two rows of it.
const FACE: u32 = 512;

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Play,
    Installations,
    Accounts,
    Notes,
    Settings,
}

const TABS: [(Page, &str); 4] = [
    (Page::Play, "Play"),
    (Page::Installations, "Installations"),
    (Page::Accounts, "Accounts"),
    (Page::Notes, "Patch notes"),
];

/// What changed in Tachyon, newest first: a title, a date and its points.
const NOTES: &[(&str, &str, &[&str])] = &[
    (
        "Tachyon for Windows",
        "October 29, 2026",
        &[
            "The Tachyon launcher on the desktop: installations, offline accounts and the game a click away.",
            "Each installation keeps its own worlds, options and resource packs, in its own folder.",
            "The game runs beside the launcher, which hides while you play and comes back when the game closes.",
        ],
    ),
    (
        "Lighter on phones",
        "October 29, 2026",
        &[
            "Occlusion culling only where it pays on a phone's tiled GPU, and no GPU timer unless the graph overlay asks for it.",
            "Lighter defaults on a phone: the Performance preset, 75% resolution and up to 120 FPS.",
            "The launcher shows how the last session ran, to copy into a report.",
        ],
    ),
    (
        "Tachyon for Android",
        "October 28, 2026",
        &[
            "Minecraft: Java Edition 26.3 runs natively on Android, built on Horizon in Rust.",
            "Touch controls, and Minecraft's resources unpacked once from the app.",
        ],
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Field {
    Account,
    Name,
    Dir,
    Width,
    Height,
}

/// The fields of the installation form, in Tab's order.
const FORM: [Field; 4] = [Field::Name, Field::Dir, Field::Width, Field::Height];

/// The installation being edited (`index`), or a new one.
#[derive(Default)]
struct Edit {
    index: Option<usize>,
    name: String,
    dir: String,
    width: String,
    height: String,
}

impl Edit {
    fn of(index: usize, i: &Installation) -> Self {
        let size = |n: u32| if n > 0 { n.to_string() } else { String::new() };
        Self {
            index: Some(index),
            name: i.name.clone(),
            dir: i.dir.clone(),
            width: size(i.width),
            height: size(i.height),
        }
    }
}

/// The game, started by PLAY.
struct Running {
    child: Child,
    started: Instant,
    home: PathBuf,
}

/// Pictures of every page, for CI (`TACHYON_SNAPSHOTS`, a folder): the
/// launcher shows each in turn with made-up accounts, saves it and quits.
struct Snapshots {
    dir: PathBuf,
    step: usize,
    frames: u32,
    started: Instant,
}

const SHOTS: [&str; 7] = [
    "1-play",
    "2-play-menu",
    "3-installations",
    "4-edit",
    "5-accounts",
    "6-notes",
    "7-settings",
];

/// Frames a page is shown before its picture, for the window to settle.
const SHOT_FRAMES: u32 = 30;

/// A message over the page: an error stays until dismissed.
struct Note {
    text: String,
    error: bool,
    at: Instant,
}

pub struct Launcher {
    paths: Paths,
    store: Store,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pointer: Pointer,
    /// Wheel distance since the last frame, in dp.
    wheel: f32,
    modifiers: ModifiersState,
    page: Page,
    scroll: f32,
    focus: Option<Field>,
    /// The name being typed for a new account.
    account_input: String,
    edit: Option<Edit>,
    /// The installation menu above the selector is open.
    menu: bool,
    note: Option<Note>,
    started: Instant,
    /// The title screen's panorama, faces 0 to 3 (a turn around), in the atlas.
    panorama: Vec<Image>,
    icon: Option<Image>,
    /// Default skins' faces by skin, in the atlas.
    faces: HashMap<&'static str, Option<Image>>,
    game: Option<Running>,
    /// The window is covered or minimized: nothing to draw.
    occluded: bool,
    cursor: CursorIcon,
    /// The cursor the frame's widgets want under the mouse.
    wanted_cursor: CursorIcon,
    snapshots: Option<Snapshots>,
    /// The launcher is done (the snapshots taken): the event loop ends.
    done: bool,
}

/// A colour between two, `t` from 0 to 1.
fn mix(a: u32, b: u32, t: f32, alpha: f32) -> [f32; 4] {
    let (a, b) = (hex(a, alpha), hex(b, alpha));
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        alpha,
    ]
}

/// The whole of an image `size` pixels wide, half a texel in from its
/// edges so the atlas's neighbours do not bleed in.
fn inset(size: u32) -> [f32; 4] {
    let e = 0.5 / size as f32;
    [e, e, 1.0 - e, 1.0 - e]
}

/// `rgba` (`w` by `h`) resampled to `nw` by `nh`, each pixel the mean of
/// those it covers.
fn resize_area(rgba: &[u8], w: u32, h: u32, nw: u32, nh: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity((nw * nh * 4) as usize);
    let span = |i: u32, from: u32, to: u32| {
        let start = i * from / to;
        (start, ((i + 1) * from / to).max(start + 1))
    };
    for y in 0..nh {
        let (y0, y1) = span(y, h, nh);
        for x in 0..nw {
            let (x0, x1) = span(x, w, nw);
            let mut sum = [0u32; 4];
            for sy in y0..y1 {
                for sx in x0..x1 {
                    let i = ((sy * w + sx) * 4) as usize;
                    for (c, s) in sum.iter_mut().enumerate() {
                        *s += u32::from(rgba[i + c]);
                    }
                }
            }
            let n = (y1 - y0) * (x1 - x0);
            out.extend(sum.map(|s| (s / n) as u8));
        }
    }
    out
}

/// A skin's face (the head's front with the hat over it), its 8 pixels
/// grown to 64 so they stay sharp.
fn skin_face(root: &Path, skin: &str) -> Option<Vec<u8>> {
    let file = root
        .join("assets/minecraft/textures")
        .join(format!("{skin}.png"));
    let (rgba, w, h) = draw::decode_png(&std::fs::read(file).ok()?)?;
    if w < 64 || h < 16 {
        return None;
    }
    let px = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
    };
    let mut face = Vec::with_capacity(8 * 8 * 4);
    for y in 0..8 {
        for x in 0..8 {
            let (base, hat) = (px(8 + x, 8 + y), px(40 + x, 8 + y));
            let a = u32::from(hat[3]);
            let over = |i: usize| ((u32::from(hat[i]) * a + u32::from(base[i]) * (255 - a)) / 255) as u8;
            face.extend([over(0), over(1), over(2), 255]);
        }
    }
    Some(draw::resize_nearest(&face, 8, 8, 64, 64))
}

/// The installation selector in the bottom bar of the main area.
fn selector_rect(main: Rect, h: f32) -> Rect {
    Rect::new(main.x + 28.0, h - BAR + (BAR - 60.0) / 2.0, 280.0, 60.0)
}

impl Launcher {
    pub fn new(paths: Paths) -> Self {
        let snapshots = std::env::var_os("TACHYON_SNAPSHOTS").map(|dir| Snapshots {
            dir: PathBuf::from(dir),
            step: 0,
            frames: 0,
            started: Instant::now(),
        });
        let store = if snapshots.is_some() {
            Self::made_up_store()
        } else {
            Store::load(&paths.data)
        };
        let first = store.accounts.is_empty();
        Self {
            paths,
            store,
            window: None,
            renderer: None,
            pointer: Pointer::default(),
            wheel: 0.0,
            modifiers: ModifiersState::default(),
            // The first start asks for a name before anything else.
            page: if first { Page::Accounts } else { Page::Play },
            scroll: 0.0,
            focus: first.then_some(Field::Account),
            account_input: String::new(),
            edit: None,
            menu: false,
            note: None,
            started: Instant::now(),
            panorama: Vec::new(),
            icon: None,
            faces: HashMap::new(),
            game: None,
            occluded: false,
            cursor: CursorIcon::Default,
            wanted_cursor: CursorIcon::Default,
            snapshots,
            done: false,
        }
    }

    /// Accounts and installations for the snapshots (never saved).
    fn made_up_store() -> Store {
        let mut store = Store::default();
        store.add_account("Alex_77");
        store.add_account("Steve");
        store.installations[0].last_played = store::now() - 2 * 86400;
        store.installations.push(Installation {
            id: "installation-1".into(),
            name: "Creative builds".into(),
            width: 1600,
            height: 900,
            ..Installation::default()
        });
        store
    }

    /// The launcher as the snapshot `step` shows it.
    fn snapshot_step(&mut self, step: usize) {
        self.menu = false;
        self.note = None;
        self.edit = None;
        self.focus = None;
        match step {
            0 => self.set_page(Page::Play),
            1 => {
                self.set_page(Page::Play);
                self.menu = true;
            }
            2 => self.set_page(Page::Installations),
            3 => {
                self.set_page(Page::Installations);
                self.edit = self.store.installations.get(1).map(|i| Edit::of(1, i));
                self.focus = Some(Field::Name);
            }
            4 => {
                self.set_page(Page::Accounts);
                self.account_input = "Notch".into();
                self.focus = Some(Field::Account);
                self.note("Playing as Steve.", false);
            }
            5 => self.set_page(Page::Notes),
            _ => self.set_page(Page::Settings),
        }
    }

    /// Steps the snapshots on by a frame; asks `renderer` for the picture
    /// when a page has settled.
    fn snapshot_frame(&mut self, renderer: &mut Renderer) {
        let Some(shots) = &mut self.snapshots else {
            return;
        };
        if shots.started.elapsed() > Duration::from_secs(120) {
            log::warn!("Tachyon launcher: snapshots took too long");
            self.done = true;
            return;
        }
        shots.frames += 1;
        let (step, frames) = (shots.step, shots.frames);
        if frames > SHOT_FRAMES {
            shots.step += 1;
            shots.frames = 0;
            if shots.step == SHOTS.len() {
                log::info!("Tachyon launcher: snapshots taken");
                self.done = true;
            }
            return;
        }
        if frames == SHOT_FRAMES {
            let _ = std::fs::create_dir_all(&shots.dir);
            let path = shots.dir.join(format!("{}.png", SHOTS[step]));
            if !renderer.capture(path) {
                log::warn!("Tachyon launcher: this window's frames cannot be copied");
                self.done = true;
            }
        }
        if frames == 1 {
            self.snapshot_step(step);
        }
    }

    fn note(&mut self, text: impl Into<String>, error: bool) {
        let text = text.into();
        if error {
            log::warn!("Tachyon launcher: {text}");
        }
        self.note = Some(Note {
            text,
            error,
            at: Instant::now(),
        });
    }

    fn save(&mut self) {
        if self.snapshots.is_some() {
            return;
        }
        if let Err(e) = self.store.save(&self.paths.data) {
            self.note(format!("The launcher's settings could not be saved: {e}"), true);
        }
    }

    fn set_page(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.scroll = 0.0;
            self.edit = None;
            self.focus = None;
        }
    }

    /// The panorama and the icon, into a new renderer's atlas.
    fn load_images(&mut self, renderer: &mut Renderer) {
        self.panorama.clear();
        self.faces.clear();
        if let Some(root) = &self.paths.resources {
            let dir = root.join("assets/minecraft/textures/gui/title/background");
            for face in 0..4 {
                let image = std::fs::read(dir.join(format!("panorama_{face}.png")))
                    .ok()
                    .and_then(|bytes| draw::decode_png(&bytes))
                    .and_then(|(rgba, w, h)| {
                        renderer.image(&resize_area(&rgba, w, h, FACE, FACE), FACE, FACE)
                    });
                match image {
                    Some(image) => self.panorama.push(image),
                    None => {
                        log::warn!("Tachyon launcher: no panorama_{face}");
                        self.panorama.clear();
                        break;
                    }
                }
            }
        }
        self.icon = draw::decode_png(ICON).and_then(|(rgba, w, h)| renderer.image(&rgba, w, h));
    }

    /// The faces of the accounts' skins not in the atlas yet.
    fn load_faces(&mut self, renderer: &mut Renderer) {
        let Some(root) = self.paths.resources.clone() else {
            return;
        };
        for account in &self.store.accounts {
            let skin = horizon_client::tab_list::default_skin(account.uuid());
            if !self.faces.contains_key(skin) {
                let face = skin_face(&root, skin).and_then(|rgba| renderer.image(&rgba, 64, 64));
                self.faces.insert(skin, face);
            }
        }
    }

    fn face(&self, account: &Account) -> Option<Image> {
        let skin = horizon_client::tab_list::default_skin(account.uuid());
        self.faces.get(skin).copied().flatten()
    }

    fn play(&mut self, index: usize) {
        self.menu = false;
        if self.game.is_some() {
            return;
        }
        let Some(account) = self.store.account().cloned() else {
            self.set_page(Page::Accounts);
            self.focus = Some(Field::Account);
            self.note("Add an offline account first: a name is all it takes.", false);
            return;
        };
        let Some(resources) = self.paths.resources.clone() else {
            self.note(
                "Minecraft's resources are missing: keep the resources folder next to Tachyon.exe.",
                true,
            );
            return;
        };
        let Some(installation) = self.store.installations.get(index).cloned() else {
            return;
        };
        let home = installation.folder(&self.paths.data);
        if let Err(e) = std::fs::create_dir_all(&home) {
            self.note(format!("{} could not be made: {e}", home.display()), true);
            return;
        }
        let mut args: Vec<OsString> = vec![
            "--resources".into(),
            resources.into(),
            "--name".into(),
            account.name.clone().into(),
        ];
        if installation.width > 0 {
            args.extend(["--width".into(), installation.width.to_string().into()]);
        }
        if installation.height > 0 {
            args.extend(["--height".into(), installation.height.to_string().into()]);
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                self.note(format!("The game could not start: {e}"), true);
                return;
            }
        };
        log::info!(
            "Tachyon launcher: playing {} as {} in {}",
            installation.name,
            account.name,
            home.display()
        );
        let spawned = Command::new(exe)
            .args(&args)
            .current_dir(&home)
            .env(GAME, "1")
            .env("HORIZON_HOME", &home)
            // Tachyon is not Horizon: no "playing now" ping to Horizon's servers,
            // and no "Playing Horizon" (Horizon's Discord application) on Discord.
            .env("HORIZON_NO_HEARTBEAT", "1")
            .env("HORIZON_NO_DISCORD", "1")
            .stdin(Stdio::null())
            .spawn();
        match spawned {
            Ok(child) => {
                self.game = Some(Running {
                    child,
                    started: Instant::now(),
                    home,
                });
                self.store.selected = installation.id.clone();
                if let Some(i) = self.store.installations.get_mut(index) {
                    i.last_played = store::now();
                }
                self.save();
                self.note = None;
                if !self.store.keep_open
                    && let Some(w) = &self.window
                {
                    w.set_visible(false);
                }
            }
            Err(e) => self.note(format!("The game could not start: {e}"), true),
        }
    }

    /// Notices the game stopping: the launcher comes back, with why if it failed.
    fn watch_game(&mut self) {
        let Some(running) = &mut self.game else {
            return;
        };
        let status = match running.child.try_wait() {
            Ok(Some(status)) => status,
            Ok(None) => return,
            Err(e) => {
                log::warn!("Tachyon launcher: the game's state is unknown: {e}");
                return;
            }
        };
        let Some(running) = self.game.take() else {
            return;
        };
        log::info!(
            "Tachyon launcher: the game stopped ({status}) after {} s",
            running.started.elapsed().as_secs()
        );
        if !status.success() {
            let why = std::fs::read_to_string(running.home.join(ERROR_FILE))
                .ok()
                .map(|t| t.trim().to_owned())
                .filter(|t| !t.is_empty());
            let text = match why {
                Some(why) => format!("The game stopped: {why}"),
                None => format!(
                    "The game stopped unexpectedly ({status}). Its log is in {}.",
                    running.home.join("logs").display()
                ),
            };
            self.note(text, true);
        }
        if let Some(w) = &self.window {
            w.set_visible(true);
            w.focus_window();
            w.request_redraw();
        }
    }

    fn add_account(&mut self) {
        let name = self.account_input.trim().to_owned();
        if !store::valid_name(&name) {
            self.note("A name is 3 to 16 letters, digits or _.", true);
            return;
        }
        self.store.add_account(&name);
        self.account_input.clear();
        self.focus = None;
        self.save();
        let active = self.store.active.clone();
        self.note(format!("Playing as {active}."), false);
    }

    fn save_edit(&mut self) {
        let Some(edit) = self.edit.take() else {
            return;
        };
        let size = |s: &str| s.trim().parse::<u32>().ok().filter(|n| (200..=16384).contains(n)).unwrap_or(0);
        let mut installation = match edit.index.and_then(|i| self.store.installations.get(i)) {
            Some(i) => i.clone(),
            None => Installation {
                id: self.store.new_id(),
                ..Installation::default()
            },
        };
        let name = edit.name.trim();
        installation.name = if name.is_empty() {
            "Unnamed installation".into()
        } else {
            name.into()
        };
        installation.dir = edit.dir.trim().to_owned();
        installation.width = size(&edit.width);
        installation.height = size(&edit.height);
        match edit.index {
            Some(i) if i < self.store.installations.len() => self.store.installations[i] = installation,
            _ => self.store.installations.push(installation),
        }
        self.focus = None;
        self.save();
    }

    fn text(&self, field: Field) -> &str {
        match field {
            Field::Account => &self.account_input,
            _ => {
                let Some(edit) = &self.edit else {
                    return "";
                };
                match field {
                    Field::Name => &edit.name,
                    Field::Dir => &edit.dir,
                    Field::Width => &edit.width,
                    _ => &edit.height,
                }
            }
        }
    }

    fn text_mut(&mut self, field: Field) -> Option<&mut String> {
        match field {
            Field::Account => Some(&mut self.account_input),
            Field::Name => self.edit.as_mut().map(|e| &mut e.name),
            Field::Dir => self.edit.as_mut().map(|e| &mut e.dir),
            Field::Width => self.edit.as_mut().map(|e| &mut e.width),
            Field::Height => self.edit.as_mut().map(|e| &mut e.height),
        }
    }

    /// Typed or pasted text, as much of it as the field takes.
    fn insert(&mut self, field: Field, text: &str) {
        let (allowed, max): (fn(char) -> bool, usize) = match field {
            Field::Account => (|c| c.is_ascii_alphanumeric() || c == '_', 16),
            Field::Name => (|c| !c.is_control(), 32),
            Field::Dir => (|c| !c.is_control(), 260),
            Field::Width | Field::Height => (|c| c.is_ascii_digit(), 5),
        };
        let Some(target) = self.text_mut(field) else {
            return;
        };
        for c in text.chars().filter(|c| allowed(*c)) {
            if target.chars().count() >= max {
                break;
            }
            target.push(c);
        }
    }

    fn key(&mut self, event: &KeyEvent) {
        match &event.logical_key {
            Key::Named(NamedKey::Escape) => {
                if self.menu {
                    self.menu = false;
                } else if self.focus.is_some() {
                    self.focus = None;
                } else if self.edit.is_some() {
                    self.edit = None;
                } else if self.note.is_some() {
                    self.note = None;
                }
                return;
            }
            Key::Named(NamedKey::Enter) => {
                match self.focus {
                    Some(Field::Account) => self.add_account(),
                    Some(_) => self.save_edit(),
                    None if self.page == Page::Play && !self.menu => {
                        self.play(self.store.selected_index())
                    }
                    None => {}
                }
                return;
            }
            Key::Named(NamedKey::Tab) => {
                if self.edit.is_some() {
                    let n = FORM.len();
                    let at = self.focus.and_then(|f| FORM.iter().position(|o| *o == f));
                    let next = match at {
                        None => 0,
                        Some(i) if self.modifiers.shift_key() => (i + n - 1) % n,
                        Some(i) => (i + 1) % n,
                    };
                    self.focus = Some(FORM[next]);
                }
                return;
            }
            Key::Named(NamedKey::Backspace) => {
                if let Some(field) = self.focus
                    && let Some(text) = self.text_mut(field)
                {
                    text.pop();
                }
                return;
            }
            _ => {}
        }
        let Some(field) = self.focus else {
            return;
        };
        // Ctrl (not AltGr, which is Ctrl and Alt) or Cmd: only paste is a shortcut here.
        let shortcut = (self.modifiers.control_key() && !self.modifiers.alt_key())
            || self.modifiers.super_key();
        if shortcut {
            let letter = horizon_client::clipboard::shortcut_letter(&event.logical_key, event.physical_key);
            if letter == Some('v') {
                use horizon_client::clipboard::Clipboard as _;
                let pasted = horizon_client::clipboard::SystemClipboard::default().get();
                self.insert(field, &pasted);
            }
            return;
        }
        if let Some(text) = &event.text {
            let text = text.to_string();
            self.insert(field, &text);
        }
    }

    fn draw(&mut self) {
        let Some(mut renderer) = self.renderer.take() else {
            return;
        };
        self.load_faces(&mut renderer);
        self.snapshot_frame(&mut renderer);
        let tap = self.pointer.tap.take();
        // A tap anywhere leaves the field being typed in, unless it is the field's.
        if tap.is_some() {
            self.focus = None;
        }
        let wheel = std::mem::take(&mut self.wheel);
        self.scroll = (self.scroll + wheel).min(0.0);
        let pointer = Pointer {
            down: self.pointer.down,
            tap,
            hover: self.pointer.hover,
            ..Pointer::default()
        };
        self.wanted_cursor = CursorIcon::Default;
        let result = renderer.frame(hex(P.page, 1.0), |c| {
            let mut ui = Ui::new(c, &pointer);
            self.layout(&mut ui);
        });
        if let Err(e) = result {
            log::warn!("Tachyon launcher: frame failed: {e}");
        }
        self.renderer = Some(renderer);
        if self.wanted_cursor != self.cursor {
            self.cursor = self.wanted_cursor;
            if let Some(w) = &self.window {
                w.set_cursor(self.cursor);
            }
        }
    }

    // Widgets with the mouse in mind: a hand over what can be clicked.

    fn clickable(&mut self, ui: &mut Ui, r: Rect) -> (bool, bool) {
        let hover = ui.hovered(r);
        if hover {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        (hover, ui.tapped(r))
    }

    fn button(&mut self, ui: &mut Ui, r: Rect, label: &str, primary: bool, enabled: bool) -> bool {
        let hover = enabled && ui.hovered(r);
        if hover {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        let pressed = ui.pressed(r);
        let clicked = ui.button(r, label, primary, enabled);
        if hover && !pressed {
            ui.c.rect(r, r.h / 2.0, hex(0xffffff, 0.07));
        }
        clicked
    }

    fn danger_button(&mut self, ui: &mut Ui, r: Rect, label: &str) -> bool {
        let (hover, clicked) = self.clickable(ui, r);
        ui.c.rect(r, r.h / 2.0, hex(P.danger, if hover { 0.24 } else { 0.12 }));
        ui.centred(r, label, Face::Medium, 14.0, hex(P.danger, 1.0));
        clicked
    }

    fn field(&mut self, ui: &mut Ui, r: Rect, field: Field, placeholder: &str) {
        let (hover, clicked) = self.clickable(ui, r);
        if hover {
            self.wanted_cursor = CursorIcon::Text;
        }
        if clicked {
            self.focus = Some(field);
        }
        let focused = self.focus == Some(field);
        ui.c.rect(r, 10.0, hex(P.page, 1.0));
        let border = if focused {
            P.accent
        } else if hover {
            P.muted
        } else {
            P.rule
        };
        ui.c.ring(r, 10.0, if focused { 1.5 } else { 1.0 }, hex(border, 1.0));
        let size = 14.0;
        let y = r.y + (r.h - ui.c.line(Face::Regular, size)) / 2.0;
        let room = r.w - 28.0;
        let text = self.text(field).to_owned();
        if text.is_empty() && !focused {
            let shown = ui.c.fit(placeholder, Face::Regular, size, room);
            ui.text(r.x + 14.0, y, &shown, Face::Regular, size, hex(P.muted, 1.0));
            return;
        }
        // A long value shows its end, where the caret is.
        let mut shown = text.as_str();
        while !shown.is_empty() && ui.c.measure(shown, Face::Regular, size) > room {
            let mut chars = shown.chars();
            chars.next();
            shown = chars.as_str();
        }
        let w = ui.text(r.x + 14.0, y, shown, Face::Regular, size, hex(P.ink, 1.0));
        if focused && (self.started.elapsed().as_millis() / 530) % 2 == 0 {
            ui.c.rect(
                Rect::new(r.x + 15.0 + w, r.y + 12.0, 1.5, r.h - 24.0),
                0.0,
                hex(P.accent, 1.0),
            );
        }
    }

    fn label(ui: &mut Ui, x: f32, y: f32, text: &str) {
        ui.text(x, y, text, Face::Mono, 11.0, hex(P.muted, 1.0));
    }

    fn avatar(&self, ui: &mut Ui, account: Option<&Account>, r: Rect) {
        match account.and_then(|a| self.face(a)) {
            Some(face) => {
                let frame = Rect::new(r.x - 2.0, r.y - 2.0, r.w + 4.0, r.h + 4.0);
                ui.c.rect(frame, 6.0, hex(P.plate, 1.0));
                ui.c.image(face, r, inset(64), [1.0; 4]);
            }
            None => {
                ui.c.rect(r, 8.0, hex(P.plate, 1.0));
                let initial = account
                    .and_then(|a| a.name.chars().next())
                    .map_or_else(|| "?".to_owned(), |c| c.to_ascii_uppercase().to_string());
                ui.centred(r, &initial, Face::Medium, r.h * 0.45, hex(P.ink_soft, 1.0));
            }
        }
    }

    fn icon(&self, ui: &mut Ui, r: Rect) {
        match self.icon {
            Some(icon) => ui.c.image(icon, r, inset(icon.width()), [1.0; 4]),
            None => ui.c.rect(r, r.w * 0.22, hex(P.accent, 1.0)),
        }
    }

    /// A small triangle, pointing down or up.
    fn chevron(ui: &mut Ui, cx: f32, cy: f32, up: bool, color: [f32; 4]) {
        for i in 0..4 {
            let w = 9.0 - i as f32 * 2.4;
            let dy = i as f32 * 1.4;
            let y = if up { cy + 2.0 - dy } else { cy - 2.0 + dy };
            ui.c.rect(Rect::new(cx - w / 2.0, y, w, 1.6), 0.0, color);
        }
    }

    /// The streak of the brand: thin bands from the accent to violet, fading
    /// out to the right.
    fn streak(&self, ui: &mut Ui, x: f32, y: f32, w: f32) {
        let t = self.started.elapsed().as_secs_f32();
        let bands = 48;
        for i in 0..bands {
            let f = i as f32 / bands as f32;
            let alpha = (1.0 - f) * 0.55;
            let shimmer = ((t * 1.6 - f * 6.0).sin() * 0.5 + 0.5) * 0.25;
            let band = Rect::new(x + f * w, y, w / bands as f32 + 0.5, 3.0);
            ui.c.rect(
                band,
                0.0,
                mix(brand::STREAK[0], brand::STREAK[1], f, alpha + shimmer * (1.0 - f)),
            );
        }
    }

    fn layout(&mut self, ui: &mut Ui) {
        let (w, h) = (ui.c.width, ui.c.height);
        let main = Rect::new(SIDEBAR, 0.0, w - SIDEBAR, h);
        let content = Rect::new(main.x, HEADER, main.w, h - HEADER);
        // What is drawn over the rest takes its taps first.
        let note = self.note_rect(ui, content);
        if let Some(r) = note
            && ui.tapped(r)
        {
            self.note = None;
        }
        let menu = (self.menu && self.page == Page::Play).then(|| self.menu_rects(main, h));
        if let Some((frame, items)) = &menu {
            self.menu_taps(ui, *frame, items, Rect::new(0.0, 0.0, w, h));
        }
        // The page, then the header and the sidebar over what scrolled under them.
        ui.clip = Some(content);
        let page_h = match self.page {
            Page::Play => {
                self.play_page(ui, content);
                0.0
            }
            Page::Installations => self.installations_page(ui, content),
            Page::Accounts => self.accounts_page(ui, content),
            Page::Notes => self.notes_page(ui, content),
            Page::Settings => self.settings_page(ui, content),
        };
        ui.clip = None;
        let overflow = (page_h - content.h).max(0.0);
        self.scroll = self.scroll.max(-overflow);
        self.header(ui, main);
        self.sidebar(ui, h);
        if let Some((frame, items)) = &menu
            && self.menu
        {
            self.draw_menu(ui, *frame, items);
        }
        if let Some(r) = note {
            self.draw_note(ui, r);
        }
    }

    fn sidebar(&mut self, ui: &mut Ui, h: f32) {
        ui.c.rect(Rect::new(0.0, 0.0, SIDEBAR, h), 0.0, hex(P.surface, 1.0));
        ui.c.rect(Rect::new(SIDEBAR - 1.0, 0.0, 1.0, h), 0.0, hex(P.rule, 1.0));
        let x = 14.0;
        let w = SIDEBAR - 28.0;
        // The account.
        let card = Rect::new(x, 14.0, w, 64.0);
        let (hover, clicked) = self.clickable(ui, card);
        if clicked {
            self.set_page(Page::Accounts);
        }
        if hover || self.page == Page::Accounts {
            ui.c.rect(card, 12.0, hex(P.plate_soft, 1.0));
        }
        let account = self.store.account().cloned();
        self.avatar(ui, account.as_ref(), Rect::new(card.x + 12.0, card.y + 14.0, 36.0, 36.0));
        let tx = card.x + 60.0;
        let (name, detail) = match &account {
            Some(a) => (a.name.as_str(), "Offline account"),
            None => ("No account", "Add one to play"),
        };
        let name = ui.c.fit(name, Face::Medium, 15.0, card.right() - tx - 8.0);
        ui.text(tx, card.y + 13.0, &name, Face::Medium, 15.0, hex(P.ink, 1.0));
        ui.text(tx, card.y + 35.0, detail, Face::Regular, 12.0, hex(P.muted, 1.0));
        ui.c.rect(Rect::new(x, 92.0, w, 1.0), 0.0, hex(P.rule, 1.0));
        // The game.
        Self::label(ui, x + 10.0, 110.0, "GAMES");
        let item = Rect::new(x, 132.0, w, 60.0);
        let (hover, clicked) = self.clickable(ui, item);
        if clicked {
            self.set_page(Page::Play);
        }
        let selected = self.page != Page::Settings;
        if selected || hover {
            ui.c.rect(item, 12.0, hex(if selected { P.plate } else { P.plate_soft }, 1.0));
        }
        if selected {
            ui.c.gradient(
                Rect::new(item.x, item.y + 14.0, 3.0, item.h - 28.0),
                1.5,
                hex(brand::STREAK[0], 1.0),
                hex(brand::STREAK[1], 1.0),
            );
        }
        self.icon(ui, Rect::new(item.x + 14.0, item.y + 12.0, 36.0, 36.0));
        ui.text(item.x + 62.0, item.y + 11.0, brand::NAME, Face::Medium, 15.0, hex(P.ink, 1.0));
        ui.text(
            item.x + 62.0,
            item.y + 33.0,
            "Java Edition 26.3",
            Face::Regular,
            12.0,
            hex(P.muted, 1.0),
        );
        // Settings, at the bottom.
        let settings = Rect::new(x, h - 104.0, w, 48.0);
        let (hover, clicked) = self.clickable(ui, settings);
        if clicked {
            self.set_page(Page::Settings);
        }
        let on = self.page == Page::Settings;
        if on || hover {
            ui.c.rect(settings, 12.0, hex(if on { P.plate } else { P.plate_soft }, 1.0));
        }
        ui.text(
            settings.x + 16.0,
            settings.y + 14.0,
            "Settings",
            Face::Medium,
            14.0,
            hex(if on { P.ink } else { P.ink_soft }, 1.0),
        );
        let version = format!("{} {}", brand::NAME, super::BUILD);
        Self::label(ui, x + 16.0, h - 36.0, &version);
    }

    fn header(&mut self, ui: &mut Ui, main: Rect) {
        ui.c.rect(Rect::new(main.x, 0.0, main.w, HEADER), 0.0, hex(P.page, 1.0));
        let x = main.x + 40.0;
        if self.page == Page::Settings {
            ui.text(x, 34.0, "Settings", Face::Medium, 28.0, hex(P.ink, 1.0));
        } else {
            let w = ui.text(x, 24.0, "TACHYON", Face::Medium, 26.0, hex(P.ink, 1.0));
            Self::label(ui, x + w + 14.0, 36.0, "JAVA EDITION 26.3");
            let mut tx = x;
            let y = HEADER - 46.0;
            for (page, label) in TABS {
                let lw = ui.c.measure(label, Face::Medium, 14.0);
                let (hover, clicked) = self.clickable(ui, Rect::new(tx - 12.0, y, lw + 24.0, 46.0));
                if clicked {
                    self.set_page(page);
                }
                let active = self.page == page;
                let color = if active {
                    P.ink
                } else if hover {
                    P.ink_soft
                } else {
                    P.muted
                };
                ui.text(tx, y + 13.0, label, Face::Medium, 14.0, hex(color, 1.0));
                if active {
                    ui.c.gradient_across(
                        Rect::new(tx, HEADER - 3.0, lw, 3.0),
                        1.5,
                        hex(brand::STREAK[0], 1.0),
                        hex(brand::STREAK[1], 1.0),
                    );
                }
                tx += lw + 34.0;
            }
        }
        ui.c.rect(Rect::new(main.x, HEADER - 1.0, main.w, 1.0), 0.0, hex(P.rule, 1.0));
    }

    /// The panorama, its four faces side by side, turning slowly.
    fn panorama(&self, ui: &mut Ui, hero: Rect) {
        if self.panorama.is_empty() {
            ui.c.gradient(hero, 0.0, hex(0x0d1a38, 1.0), hex(P.page, 1.0));
            return;
        }
        let f = hero.h;
        let n = self.panorama.len();
        let strip = f * n as f32;
        let t = if self.store.still {
            0.0
        } else {
            self.started.elapsed().as_secs_f32()
        };
        let offset = (t * 7.0) % strip;
        let e = 0.5 / FACE as f32;
        let mut fx = hero.x - offset;
        let mut i = 0;
        while fx < hero.right() {
            let (x0, x1) = (fx.max(hero.x), (fx + f).min(hero.right()));
            if x1 > x0 {
                let (u0, u1) = ((x0 - fx) / f, (x1 - fx) / f);
                let src = [e + u0 * (1.0 - 2.0 * e), e, e + u1 * (1.0 - 2.0 * e), 1.0 - e];
                ui.c.image(
                    self.panorama[i % n],
                    Rect::new(x0, hero.y, x1 - x0, hero.h),
                    src,
                    [1.0; 4],
                );
            }
            fx += f;
            i += 1;
        }
    }

    fn play_page(&mut self, ui: &mut Ui, content: Rect) {
        let hero = Rect::new(content.x, content.y, content.w, content.h - BAR);
        self.panorama(ui, hero);
        // Shade for the words: under the header, at the bottom and on the left.
        ui.c.gradient(
            Rect::new(hero.x, hero.y, hero.w, 90.0),
            0.0,
            hex(P.page, 0.7),
            hex(P.page, 0.0),
        );
        let fade = (hero.h * 0.65).min(320.0);
        ui.c.gradient(
            Rect::new(hero.x, hero.bottom() - fade, hero.w, fade),
            0.0,
            hex(P.page, 0.0),
            hex(P.page, 0.95),
        );
        ui.c.gradient_across(
            Rect::new(hero.x, hero.y, hero.w * 0.7, hero.h),
            0.0,
            hex(P.page, 0.6),
            hex(P.page, 0.0),
        );
        let x = hero.x + 48.0;
        let mut y = hero.bottom() - 150.0;
        ui.text(x, y, "NATIVE  ·  OFFLINE  ·  RUST", Face::Mono, 12.0, hex(P.accent, 1.0));
        y += 24.0;
        ui.text(
            x,
            y,
            "Minecraft: Java Edition 26.3",
            Face::Medium,
            30.0,
            hex(P.ink, 1.0),
        );
        y += 46.0;
        self.streak(ui, x, y, 260.0);
        y += 14.0;
        ui.text(
            x,
            y,
            "Built on Horizon, without Java. Pick a name, press Play.",
            Face::Regular,
            15.0,
            hex(P.ink_soft, 1.0),
        );
        self.bottom_bar(ui, Rect::new(content.x, content.bottom() - BAR, content.w, BAR));
    }

    fn bottom_bar(&mut self, ui: &mut Ui, bar: Rect) {
        ui.c.rect(bar, 0.0, hex(P.surface, 1.0));
        ui.c.rect(Rect::new(bar.x, bar.y, bar.w, 1.0), 0.0, hex(P.rule, 1.0));
        // The installation, on the left.
        let selector = selector_rect(bar, bar.bottom());
        let (hover, clicked) = self.clickable(ui, selector);
        if clicked {
            self.menu = !self.menu;
        }
        let open = self.menu;
        ui.c.rect(
            selector,
            12.0,
            hex(if hover || open { P.plate } else { P.plate_soft }, 1.0),
        );
        let installation = self.store.installations[self.store.selected_index()].clone();
        self.icon(ui, Rect::new(selector.x + 14.0, selector.y + 14.0, 32.0, 32.0));
        let tx = selector.x + 58.0;
        let room = selector.right() - tx - 34.0;
        let name = ui.c.fit(&installation.name, Face::Medium, 15.0, room);
        ui.text(tx, selector.y + 11.0, &name, Face::Medium, 15.0, hex(P.ink, 1.0));
        let detail = if installation.last_played > 0 {
            format!("26.3  ·  {}", store::ago(installation.last_played, store::now()))
        } else {
            "26.3  ·  Never played".to_owned()
        };
        let detail = ui.c.fit(&detail, Face::Regular, 12.0, room);
        ui.text(tx, selector.y + 33.0, &detail, Face::Regular, 12.0, hex(P.muted, 1.0));
        Self::chevron(
            ui,
            selector.right() - 20.0,
            selector.y + selector.h / 2.0,
            !open,
            hex(P.ink_soft, 1.0),
        );
        // PLAY, in the middle.
        let running = self.game.is_some();
        let ready = self.paths.resources.is_some();
        let enabled = ready && !running;
        let pw = 240.0;
        let px = (bar.x + (bar.w - pw) / 2.0).max(selector.right() + 24.0);
        let play = Rect::new(px, bar.y + (BAR - 64.0) / 2.0, pw, 64.0);
        let hover = enabled && ui.hovered(play);
        if hover {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        let pressed = enabled && ui.pressed(play);
        let alpha = if !enabled {
            horizon_theme::DISABLED
        } else if pressed {
            horizon_theme::PRESSED
        } else {
            1.0
        };
        if enabled {
            let glow = Rect::new(play.x - 5.0, play.y - 5.0, play.w + 10.0, play.h + 10.0);
            ui.c.rect(glow, 18.0, hex(P.accent, if hover { 0.22 } else { 0.1 }));
        }
        ui.c.gradient_across(
            play,
            14.0,
            hex(brand::STREAK[0], alpha),
            mix(brand::STREAK[0], brand::STREAK[1], 0.55, alpha),
        );
        if hover && !pressed {
            ui.c.rect(play, 14.0, hex(0xffffff, 0.1));
        }
        let label = if running {
            "PLAYING"
        } else if !ready {
            "NO RESOURCES"
        } else {
            "PLAY"
        };
        let ink = hex(P.on_accent, 1.0);
        ui.centred(
            Rect::new(play.x, play.y + 6.0, play.w, 32.0),
            label,
            Face::Medium,
            21.0,
            ink,
        );
        let sub = ui.c.fit(&installation.name, Face::Mono, 11.0, play.w - 32.0);
        ui.centred(
            Rect::new(play.x, play.y + 36.0, play.w, 18.0),
            &sub,
            Face::Mono,
            11.0,
            hex(P.on_accent, 0.7),
        );
        if enabled && ui.tapped(play) {
            self.play(self.store.selected_index());
        }
        // The player, on the right.
        let account = self.store.account().cloned();
        let who = Rect::new(bar.right() - 28.0 - 210.0, bar.y + (BAR - 56.0) / 2.0, 210.0, 56.0);
        let (hover, clicked) = self.clickable(ui, who);
        if clicked {
            self.set_page(Page::Accounts);
        }
        if hover {
            ui.c.rect(who, 12.0, hex(P.plate_soft, 1.0));
        }
        let avatar = Rect::new(who.right() - 50.0, who.y + 10.0, 36.0, 36.0);
        self.avatar(ui, account.as_ref(), avatar);
        let right = avatar.x - 12.0;
        let caption = "PLAYING AS";
        let cw = ui.c.measure(caption, Face::Mono, 10.0);
        ui.text(right - cw, who.y + 11.0, caption, Face::Mono, 10.0, hex(P.muted, 1.0));
        let (name, color) = match &account {
            Some(a) => (a.name.clone(), P.ink),
            None => ("Add an account".to_owned(), P.accent),
        };
        let nw = ui.c.measure(&name, Face::Medium, 15.0);
        ui.text(right - nw, who.y + 27.0, &name, Face::Medium, 15.0, hex(color, 1.0));
    }

    /// The installation menu above the selector: its frame and items (the
    /// installations, then "Manage installations").
    fn menu_rects(&self, main: Rect, h: f32) -> (Rect, Vec<Rect>) {
        let selector = selector_rect(main, h);
        let item_h = 50.0;
        let room = ((selector.y - 40.0 - HEADER) / item_h).floor().max(2.0) as usize;
        let n = (self.store.installations.len() + 1).min(room);
        let frame_h = n as f32 * item_h + 16.0;
        let frame = Rect::new(selector.x, selector.y - 10.0 - frame_h, 340.0, frame_h);
        let items = (0..n)
            .map(|i| {
                Rect::new(
                    frame.x + 8.0,
                    frame.y + 8.0 + i as f32 * item_h,
                    frame.w - 16.0,
                    item_h,
                )
            })
            .collect();
        (frame, items)
    }

    fn menu_taps(&mut self, ui: &mut Ui, frame: Rect, items: &[Rect], window: Rect) {
        let last = items.len() - 1;
        for (i, r) in items.iter().enumerate() {
            if ui.tapped(*r) {
                self.menu = false;
                if i == last {
                    self.set_page(Page::Installations);
                } else if let Some(installation) = self.store.installations.get(i) {
                    self.store.selected = installation.id.clone();
                    self.save();
                }
                return;
            }
        }
        if !ui.tapped(frame) && ui.tapped(window) {
            self.menu = false;
        }
    }

    fn draw_menu(&mut self, ui: &mut Ui, frame: Rect, items: &[Rect]) {
        let shadow = Rect::new(frame.x - 4.0, frame.y + 2.0, frame.w + 8.0, frame.h + 10.0);
        ui.c.rect(shadow, 18.0, hex(0x000000, 0.4));
        ui.c.rect(frame, 14.0, hex(P.plate_soft, 1.0));
        ui.c.ring(frame, 14.0, 1.0, hex(P.rule, 1.0));
        let last = items.len() - 1;
        let selected = self.store.selected_index();
        for (i, r) in items.iter().enumerate() {
            let hover = ui.hovered(*r);
            if hover {
                self.wanted_cursor = CursorIcon::Pointer;
                ui.c.rect(*r, 10.0, hex(P.plate, 1.0));
            }
            if i == last {
                ui.text(
                    r.x + 14.0,
                    r.y + 15.0,
                    "Manage installations",
                    Face::Medium,
                    14.0,
                    hex(P.accent, 1.0),
                );
                continue;
            }
            let Some(installation) = self.store.installations.get(i).cloned() else {
                continue;
            };
            self.icon(ui, Rect::new(r.x + 10.0, r.y + 11.0, 28.0, 28.0));
            let name = ui.c.fit(&installation.name, Face::Medium, 14.0, r.w - 90.0);
            ui.text(r.x + 50.0, r.y + 7.0, &name, Face::Medium, 14.0, hex(P.ink, 1.0));
            let folder = installation.folder(&self.paths.data).display().to_string();
            let folder = ui.c.fit(&folder, Face::Regular, 11.0, r.w - 90.0);
            ui.text(r.x + 50.0, r.y + 27.0, &folder, Face::Regular, 11.0, hex(P.muted, 1.0));
            if i == selected {
                let dot = Rect::new(r.right() - 22.0, r.y + r.h / 2.0 - 4.0, 8.0, 8.0);
                ui.c.rect(dot, 4.0, hex(P.accent, 1.0));
            }
        }
    }

    fn installations_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        if self.edit.is_some() {
            return self.edit_page(ui, content);
        }
        let x = content.x + 40.0;
        let w = (content.w - 80.0).min(980.0);
        let top = content.y + self.scroll;
        let mut y = top + 32.0;
        ui.text(x, y, "Installations", Face::Medium, 22.0, hex(P.ink, 1.0));
        let new = Rect::new(x + w - 190.0, y - 4.0, 190.0, 40.0);
        if self.button(ui, new, "New installation", true, true) {
            self.edit = Some(Edit::default());
            self.focus = Some(Field::Name);
        }
        y += 38.0;
        ui.text(
            x,
            y,
            "Each installation keeps its own worlds, options and resource packs in its own folder.",
            Face::Regular,
            13.0,
            hex(P.muted, 1.0),
        );
        y += 36.0;
        let now = store::now();
        let selected = self.store.selected_index();
        let installations = self.store.installations.clone();
        for (i, installation) in installations.iter().enumerate() {
            let row = Rect::new(x, y, w, 78.0);
            let hover = ui.hovered(row);
            ui.c.rect(
                row,
                14.0,
                hex(if hover { P.plate_soft } else { P.surface }, 1.0),
            );
            self.icon(ui, Rect::new(row.x + 18.0, row.y + 17.0, 44.0, 44.0));
            let tx = row.x + 78.0;
            // Play, Edit and Folder with their gaps, and room before them.
            let buttons = 84.0 + 8.0 + 76.0 + 8.0 + 90.0 + 16.0 + 24.0;
            let room = (row.right() - buttons - tx).max(80.0);
            let name = ui.c.fit(&installation.name, Face::Medium, 16.0, room);
            let nw = ui.text(tx, row.y + 17.0, &name, Face::Medium, 16.0, hex(P.ink, 1.0));
            if i == selected {
                let pill = Rect::new(tx + nw + 10.0, row.y + 18.0, 74.0, 20.0);
                ui.c.rect(pill, 10.0, hex(P.accent, 0.14));
                ui.centred(pill, "SELECTED", Face::Mono, 10.0, hex(P.accent, 1.0));
            }
            let played = if installation.last_played > 0 {
                store::ago(installation.last_played, now)
            } else {
                "Never played".to_owned()
            };
            let folder = installation.folder(&self.paths.data).display().to_string();
            let detail = format!("Java Edition 26.3  ·  {played}  ·  {folder}");
            let detail = ui.c.fit(&detail, Face::Regular, 12.0, room);
            ui.text(tx, row.y + 44.0, &detail, Face::Regular, 12.0, hex(P.muted, 1.0));
            let by = row.y + (row.h - 36.0) / 2.0;
            let mut bx = row.right() - 16.0;
            bx -= 90.0;
            if self.button(ui, Rect::new(bx, by, 90.0, 36.0), "Folder", false, true) {
                super::open_folder(&installation.folder(&self.paths.data));
            }
            bx -= 76.0 + 8.0;
            if self.button(ui, Rect::new(bx, by, 76.0, 36.0), "Edit", false, true) {
                self.edit = Some(Edit::of(i, installation));
            }
            bx -= 84.0 + 8.0;
            let can_play = self.game.is_none() && self.paths.resources.is_some();
            if self.button(ui, Rect::new(bx, by, 84.0, 36.0), "Play", true, can_play) {
                self.play(i);
            }
            y += 88.0;
        }
        y - top + 24.0
    }

    fn edit_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 40.0;
        let w = (content.w - 80.0).min(680.0);
        let top = content.y + self.scroll;
        let mut y = top + 32.0;
        let index = self.edit.as_ref().and_then(|e| e.index);
        let existing = index.and_then(|i| self.store.installations.get(i)).cloned();
        let title = if existing.is_some() {
            "Edit installation"
        } else {
            "New installation"
        };
        ui.text(x, y, title, Face::Medium, 22.0, hex(P.ink, 1.0));
        y += 58.0;
        Self::label(ui, x, y, "NAME");
        self.field(ui, Rect::new(x, y + 18.0, w.min(440.0), 44.0), Field::Name, "Unnamed installation");
        y += 88.0;
        Self::label(ui, x, y, "GAME DIRECTORY");
        let own = existing.clone().unwrap_or_else(|| Installation {
            id: self.store.new_id(),
            ..Installation::default()
        });
        let default = own.own_folder(&self.paths.data).display().to_string();
        self.field(ui, Rect::new(x, y + 18.0, w, 44.0), Field::Dir, &default);
        ui.text(
            x,
            y + 68.0,
            "Empty: a folder of its own in Tachyon's. Worlds, options and packs live here.",
            Face::Regular,
            12.0,
            hex(P.muted, 1.0),
        );
        y += 108.0;
        Self::label(ui, x, y, "RESOLUTION");
        self.field(ui, Rect::new(x, y + 18.0, 120.0, 44.0), Field::Width, "Auto");
        ui.text(x + 132.0, y + 30.0, "×", Face::Regular, 16.0, hex(P.muted, 1.0));
        self.field(ui, Rect::new(x + 156.0, y + 18.0, 120.0, 44.0), Field::Height, "Auto");
        ui.text(
            x + 292.0,
            y + 32.0,
            "Auto: 1280 × 720",
            Face::Regular,
            12.0,
            hex(P.muted, 1.0),
        );
        y += 96.0;
        if self.button(ui, Rect::new(x, y, 120.0, 40.0), "Save", true, true) {
            self.save_edit();
        }
        if self.button(ui, Rect::new(x + 132.0, y, 110.0, 40.0), "Cancel", false, true) {
            self.edit = None;
            self.focus = None;
        }
        if let (Some(i), Some(installation)) = (index, &existing) {
            if self.button(ui, Rect::new(x + 254.0, y, 130.0, 40.0), "Open folder", false, true) {
                super::open_folder(&installation.folder(&self.paths.data));
            }
            if installation.id != store::DEFAULT_ID
                && self.danger_button(ui, Rect::new(x + w - 120.0, y, 120.0, 40.0), "Delete")
            {
                let name = installation.name.clone();
                if i < self.store.installations.len() {
                    self.store.remove_installation(&installation.id);
                }
                self.edit = None;
                self.focus = None;
                self.save();
                self.note(
                    format!("{name} is gone from the list. Its folder and worlds are still on disk."),
                    false,
                );
            }
        }
        y += 60.0;
        y - top + 24.0
    }

    fn accounts_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 40.0;
        let w = (content.w - 80.0).min(860.0);
        let top = content.y + self.scroll;
        let mut y = top + 32.0;
        ui.text(x, y, "Accounts", Face::Medium, 22.0, hex(P.ink, 1.0));
        y += 40.0;
        let about = "Offline accounts: a name is all an account is. It gives the player's UUID and default skin, as offline mode does. Worlds and offline-mode servers take them; servers that check Microsoft accounts do not.";
        for line in ui.c.wrap(about, Face::Regular, 14.0, w) {
            ui.text(x, y, &line, Face::Regular, 14.0, hex(P.ink_soft, 1.0));
            y += 21.0;
        }
        y += 18.0;
        let accounts = self.store.accounts.clone();
        if accounts.is_empty() {
            let r = Rect::new(x, y, w, 64.0);
            ui.c.rect(r, 14.0, hex(P.surface, 1.0));
            ui.c.ring(r, 14.0, 1.0, hex(P.rule, 1.0));
            ui.text(
                r.x + 20.0,
                r.y + 22.0,
                "No account yet: add one below to play.",
                Face::Regular,
                14.0,
                hex(P.muted, 1.0),
            );
            y += 74.0;
        }
        let mut remove = None;
        for account in &accounts {
            let row = Rect::new(x, y, w, 74.0);
            let hover = ui.hovered(row);
            ui.c.rect(
                row,
                14.0,
                hex(if hover { P.plate_soft } else { P.surface }, 1.0),
            );
            self.avatar(ui, Some(account), Rect::new(row.x + 18.0, row.y + 17.0, 40.0, 40.0));
            let tx = row.x + 76.0;
            ui.text(tx, row.y + 16.0, &account.name, Face::Medium, 16.0, hex(P.ink, 1.0));
            let detail = format!("Offline  ·  {}", account.uuid_text());
            ui.text(tx, row.y + 42.0, &detail, Face::Mono, 11.0, hex(P.muted, 1.0));
            let by = row.y + (row.h - 36.0) / 2.0;
            let remove_r = Rect::new(row.right() - 16.0 - 96.0, by, 96.0, 36.0);
            if self.button(ui, remove_r, "Remove", false, true) {
                remove = Some(account.name.clone());
            }
            let use_r = Rect::new(remove_r.x - 8.0 - 84.0, by, 84.0, 36.0);
            if account.name == self.store.active {
                ui.c.rect(use_r, 18.0, hex(P.accent, 0.14));
                ui.centred(use_r, "ACTIVE", Face::Mono, 11.0, hex(P.accent, 1.0));
            } else if self.button(ui, use_r, "Use", true, true) {
                self.store.active = account.name.clone();
                self.save();
            }
            y += 84.0;
        }
        if let Some(name) = remove {
            self.store.remove_account(&name);
            self.save();
        }
        y += 18.0;
        Self::label(ui, x, y, "ADD AN OFFLINE ACCOUNT");
        self.field(ui, Rect::new(x, y + 18.0, 320.0, 44.0), Field::Account, "Player name");
        let valid = store::valid_name(self.account_input.trim());
        if self.button(ui, Rect::new(x + 332.0, y + 18.0, 100.0, 44.0), "Add", true, valid) {
            self.add_account();
        }
        ui.text(
            x,
            y + 72.0,
            "3 to 16 letters, digits or _.",
            Face::Regular,
            12.0,
            hex(P.muted, 1.0),
        );
        y += 110.0;
        y - top
    }

    fn notes_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 40.0;
        let w = (content.w - 80.0).min(820.0);
        let top = content.y + self.scroll;
        let mut y = top + 32.0;
        ui.text(x, y, "Patch notes", Face::Medium, 22.0, hex(P.ink, 1.0));
        y += 52.0;
        for (i, (title, date, points)) in NOTES.iter().enumerate() {
            let lines: Vec<Vec<String>> = points
                .iter()
                .map(|p| ui.c.wrap(p, Face::Regular, 14.0, w - 64.0))
                .collect();
            let body: f32 = lines.iter().map(|l| l.len() as f32 * 21.0 + 8.0).sum();
            let card = Rect::new(x, y, w, 76.0 + body + 12.0);
            ui.c.rect(card, 16.0, hex(P.surface, 1.0));
            let date = date.to_uppercase();
            Self::label(ui, card.x + 24.0, card.y + 22.0, &date);
            if i == 0 {
                let dw = ui.c.measure(&date, Face::Mono, 11.0);
                let pill = Rect::new(card.x + 24.0 + dw + 10.0, card.y + 19.0, 58.0, 20.0);
                ui.c.rect(pill, 10.0, hex(P.accent, 0.14));
                ui.centred(pill, "LATEST", Face::Mono, 10.0, hex(P.accent, 1.0));
            }
            ui.text(card.x + 24.0, card.y + 40.0, title, Face::Medium, 18.0, hex(P.ink, 1.0));
            let mut ly = card.y + 76.0;
            for point in lines {
                ui.c.rect(
                    Rect::new(card.x + 28.0, ly + 8.0, 5.0, 5.0),
                    2.5,
                    hex(P.accent, 1.0),
                );
                for line in point {
                    ui.text(card.x + 44.0, ly, &line, Face::Regular, 14.0, hex(P.ink_soft, 1.0));
                    ly += 21.0;
                }
                ly += 8.0;
            }
            y += card.h + 16.0;
        }
        y - top + 16.0
    }

    fn settings_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 40.0;
        let w = (content.w - 80.0).min(860.0);
        let top = content.y + self.scroll;
        let mut y = top + 16.0;
        let r = Rect::new(x, y, w, 76.0);
        ui.row(
            r,
            1,
            "Keep the launcher open while playing",
            "Otherwise it hides while the game runs and comes back when the game closes.",
            60.0,
        );
        if ui.hovered(r) {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        if let Some(on) = ui.switch(r, self.store.keep_open) {
            self.store.keep_open = on;
            self.save();
        }
        y += 76.0;
        let r = Rect::new(x, y, w, 76.0);
        ui.row(
            r,
            2,
            "Moving background",
            "The panorama behind Play turns slowly.",
            60.0,
        );
        if ui.hovered(r) {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        if let Some(on) = ui.switch(r, !self.store.still) {
            self.store.still = !on;
            self.save();
        }
        y += 76.0;
        let r = Rect::new(x, y, w, 76.0);
        let data = self.paths.data.display().to_string();
        ui.row(r, 3, "Tachyon's folder", &data, 100.0);
        if self.row_button(ui, r, "Open") {
            super::open_folder(&self.paths.data);
        }
        y += 76.0;
        let r = Rect::new(x, y, w, 76.0);
        let resources = match &self.paths.resources {
            Some(dir) => format!("Minecraft 26.3, prepared: {}", dir.display()),
            None => "Not found. Keep the resources folder next to Tachyon.exe, or name it in TACHYON_RESOURCES.".to_owned(),
        };
        ui.row(r, 4, "Resources", &resources, 0.0);
        y += 76.0;
        let r = Rect::new(x, y, w, 76.0);
        ui.row(
            r,
            5,
            "Launcher log",
            "What the launcher did, for a bug report: logs/latest.log in Tachyon's folder.",
            100.0,
        );
        if self.row_button(ui, r, "Open") {
            super::open_folder(&self.paths.data.join("logs"));
        }
        y += 76.0;
        let r = Rect::new(x, y, w, 104.0);
        let about = format!(
            "{} {}. Built on Horizon (horizon-rust/horizon, MIT), Minecraft: Java Edition rewritten in Rust. Not an official Minecraft product; not approved by or associated with Mojang or Microsoft.",
            brand::NAME,
            super::BUILD
        );
        ui.row(r, 6, "About", &about, 0.0);
        y += 104.0;
        y - top + 24.0
    }

    fn row_button(&mut self, ui: &mut Ui, row: Rect, label: &str) -> bool {
        let w = ui.button_width(label);
        let r = Rect::new(row.right() - w, row.y + (row.h - 36.0) / 2.0, w, 36.0);
        self.button(ui, r, label, false, true)
    }

    /// Where the note shows: the top right of the page. None without one, or
    /// once a message (not an error) has been read for a while.
    fn note_rect(&mut self, ui: &mut Ui, content: Rect) -> Option<Rect> {
        let note = self.note.as_ref()?;
        if !note.error && note.at.elapsed() > Duration::from_secs(6) {
            self.note = None;
            return None;
        }
        let w = 460.0f32.min(content.w - 48.0);
        let lines = ui.c.wrap(&note.text, Face::Regular, 14.0, w - 64.0).len().max(1);
        let h = 28.0 + lines as f32 * 21.0;
        Some(Rect::new(content.right() - w - 24.0, content.y + 16.0, w, h))
    }

    fn draw_note(&mut self, ui: &mut Ui, r: Rect) {
        let Some(note) = &self.note else {
            return;
        };
        let text = note.text.clone();
        let color = if note.error { P.danger } else { P.success };
        if ui.hovered(r) {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        let shadow = Rect::new(r.x - 4.0, r.y + 2.0, r.w + 8.0, r.h + 10.0);
        ui.c.rect(shadow, 16.0, hex(0x000000, 0.4));
        ui.c.rect(r, 12.0, hex(P.plate, 1.0));
        ui.c.rect(Rect::new(r.x + 12.0, r.y + 14.0, 3.0, r.h - 28.0), 1.5, hex(color, 1.0));
        let mut y = r.y + 14.0;
        for line in ui.c.wrap(&text, Face::Regular, 14.0, r.w - 64.0) {
            ui.text(r.x + 28.0, y, &line, Face::Regular, 14.0, hex(P.ink, 1.0));
            y += 21.0;
        }
        ui.text(r.right() - 26.0, r.y + 12.0, "×", Face::Regular, 16.0, hex(P.muted, 1.0));
    }
}

impl ApplicationHandler for Launcher {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            let attributes = Window::default_attributes()
                .with_title(format!("{} Launcher", brand::NAME))
                .with_inner_size(LogicalSize::new(1200.0, 740.0))
                .with_min_inner_size(LogicalSize::new(1024.0, 640.0))
                .with_window_icon(horizon_client::window::icon());
            match event_loop.create_window(attributes) {
                Ok(w) => self.window = Some(Arc::new(w)),
                Err(e) => {
                    log::error!("Tachyon launcher: no window: {e}");
                    event_loop.exit();
                    return;
                }
            }
        }
        if self.renderer.is_none()
            && let Some(window) = &self.window
        {
            match Renderer::new(window.clone()) {
                Ok(mut renderer) => {
                    self.load_images(&mut renderer);
                    self.renderer = Some(renderer);
                }
                Err(e) => {
                    log::error!("Tachyon launcher: no renderer: {e}");
                    event_loop.exit();
                    return;
                }
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32);
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = &mut self.renderer {
                    r.resize(size.width, size.height, scale);
                }
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                if let (Some(r), Some(w)) = (&mut self.renderer, &self.window) {
                    let size = w.inner_size();
                    r.resize(size.width, size.height, scale_factor as f32);
                }
            }
            WindowEvent::Occluded(occluded) => self.occluded = occluded,
            WindowEvent::Focused(false) => self.pointer.cancel(),
            WindowEvent::CursorMoved { position, .. } => {
                let at = (position.x as f32 / scale, position.y as f32 / scale);
                self.pointer.hover = Some(at);
                self.pointer.moved(at);
                // The wheel scrolls; a drag with the button down does not.
                self.pointer.scroll = 0.0;
            }
            WindowEvent::CursorLeft { .. } => self.pointer.hover = None,
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => {
                    if let Some(at) = self.pointer.hover {
                        self.pointer.press(at);
                    }
                }
                ElementState::Released => self.pointer.release(),
            },
            WindowEvent::MouseWheel { delta, .. } => {
                self.wheel += match delta {
                    MouseScrollDelta::LineDelta(_, lines) => lines * 48.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / scale,
                };
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::KeyboardInput { event, .. } if event.state == ElementState::Pressed => {
                self.key(&event);
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.done {
            event_loop.exit();
            return;
        }
        self.watch_game();
        let shown = self.game.is_none() || self.store.keep_open;
        if shown && (!self.occluded || self.snapshots.is_some()) {
            // Drawn at the display's rate (the renderer waits for it): the
            // panorama turns and the caret blinks.
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        } else {
            // Hidden or covered: only the game's end to watch for.
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(250),
            ));
        }
    }
}
