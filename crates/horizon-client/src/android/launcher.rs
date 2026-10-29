//! Tachyon's launcher: the wordmark, Play, the player's name and the touch
//! controls, drawn in the game's window before the game takes it over. The
//! first start unpacks the resources the APK carries.
use crate::tachyon::brand::{self, P};
use super::controls;
use crate::tachyon::draw::{Face, Rect, Renderer, hex};
use super::install::{self, Paths, Task};
use super::jni;
use crate::tachyon::ui::{CONTROL, GUTTER, Pointer, ROW, Ui};
use std::sync::Arc;
use std::time::Instant;
use winit::event::{TouchPhase, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::platform::android::activity::AndroidApp;
use winit::window::Window;

/// The task that installs an imported file.
const IMPORTING: &str = "Importing";

#[derive(Clone, Copy, PartialEq)]
enum Page {
    Play,
    Settings,
}

/// The player's name, as the soft keyboard types it.
struct Editing {
    text: String,
}

pub struct Launcher {
    app: AndroidApp,
    pub paths: Paths,
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pointer: Pointer,
    page: Page,
    scroll: f32,
    task: Option<Task>,
    /// The last task's failure or a message, until another starts.
    error: Option<String>,
    note: Option<String>,
    editing: Option<Editing>,
    /// The touch controls, kept in the game's horizon.txt.
    touch: bool,
    started: Instant,
    /// The game's last crash.
    crash: Option<String>,
    /// The last session's performance line (`tachyon-performance.txt`).
    performance: Option<String>,
    /// The display cutout's margins in pixels, asked again when the window changes.
    insets: Option<[f32; 4]>,
    /// Set by Play: the arguments the game starts with.
    pub play: Option<Vec<String>>,
    /// The emulator test's arguments (the intent extra `tachyon.smoke`,
    /// comma separated): the game starts with them once the resources are ready.
    smoke: Option<Vec<String>>,
}

/// The display cutout's margins in pixels: left, top, right, bottom.
fn insets() -> [f32; 4] {
    let mut insets = [0.0; 4];
    if let Some(text) = jni::platform("insets", "") {
        for (inset, value) in insets.iter_mut().zip(text.split(',')) {
            *inset = value.parse().unwrap_or(0.0);
        }
    }
    insets
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

impl Launcher {
    pub fn new(app: AndroidApp, paths: Paths, crash: Option<String>) -> Self {
        let touch = horizon_client::ui_theme::preference(&paths.home, "touchControls")
            .is_none_or(|v| v != "false");
        let mut launcher = Self {
            app,
            paths,
            window: None,
            renderer: None,
            pointer: Pointer::default(),
            page: Page::Play,
            scroll: 0.0,
            task: None,
            error: None,
            note: None,
            editing: None,
            touch,
            started: Instant::now(),
            crash,
            performance: None,
            insets: None,
            play: None,
            smoke: None,
        };
        launcher.smoke = jni::platform("intent_extra", "tachyon.smoke")
            .map(|text| text.split(',').map(str::to_owned).collect());
        launcher.performance =
            std::fs::read_to_string(launcher.paths.home.join("tachyon-performance.txt")).ok();
        if !install::up_to_date(&launcher.app, &launcher.paths) {
            launcher.unpack();
        }
        launcher
    }

    fn unpack(&mut self) {
        let (app, paths) = (self.app.clone(), self.paths.clone());
        self.error = None;
        self.task = Some(Task::spawn("Getting ready", move |report| {
            install::install(&app, &paths, report).map_err(|e| {
                match install::free_space(paths.root.parent().unwrap_or(&paths.root)) {
                    Some(free) if free < 1 << 30 => format!(
                        "{e}. Only {} MB are free on this phone: the resources need about 1 GB.",
                        free >> 20
                    ),
                    _ => e,
                }
            })
        }));
    }

    /// Installs the picked file into the game folder, off the UI's thread.
    fn import(&mut self, path: String, name: String) {
        if self.busy() {
            self.error = Some("Wait for the current task to finish, then import again.".into());
            return;
        }
        let home = self.paths.home.clone();
        self.error = None;
        self.note = None;
        self.task = Some(Task::spawn(IMPORTING, move |report| {
            let file = std::path::PathBuf::from(path);
            let result = crate::tachyon::import::import(&file, &name, &home, &|line| report.line(line));
            let _ = std::fs::remove_file(&file);
            report.line(result?.note);
            Ok(())
        }));
    }

    fn busy(&self) -> bool {
        self.task.is_some()
    }

    pub fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_none() {
            match event_loop.create_window(Window::default_attributes().with_title(brand::NAME)) {
                Ok(w) => self.window = Some(Arc::new(w)),
                Err(e) => {
                    log::error!("Tachyon launcher: no window: {e}");
                    return;
                }
            }
        }
        if self.renderer.is_none()
            && let Some(window) = &self.window
        {
            match Renderer::new(window.clone()) {
                Ok(r) => self.renderer = Some(r),
                Err(e) => log::error!("Tachyon launcher: no renderer: {e}"),
            }
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    /// Whether the launcher has its window and draws in it: the game can
    /// only start then, as it needs the window's surface.
    pub fn on_screen(&self) -> bool {
        self.window.is_some() && self.renderer.is_some()
    }

    /// The window is going away (the app left the screen): the surface goes with it.
    pub fn suspended(&mut self) {
        self.renderer = None;
    }

    /// The window and renderer go to the game: the surface must be released first.
    pub fn hand_over(&mut self) -> Option<Arc<Window>> {
        self.renderer = None;
        jni::show_keyboard(false);
        self.window.take()
    }

    pub fn window_event(&mut self, event: WindowEvent) {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor() as f32);
        match event {
            WindowEvent::Resized(size) => {
                self.insets = None;
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
            WindowEvent::Touch(touch) => {
                let at = (
                    touch.location.x as f32 / scale,
                    touch.location.y as f32 / scale,
                );
                match touch.phase {
                    TouchPhase::Started => self.pointer.press(at),
                    TouchPhase::Moved => self.pointer.moved(at),
                    TouchPhase::Ended => self.pointer.release(),
                    TouchPhase::Cancelled => self.pointer.cancel(),
                }
            }
            WindowEvent::RedrawRequested => self.draw(),
            _ => {}
        }
    }

    /// A game that could not start.
    pub fn failed(&mut self, error: String) {
        self.error = Some(error);
    }

    /// Keyboard text for the name being edited, and the back gesture.
    fn typed(&mut self) {
        let typed = jni::take_typed();
        if typed.contains(&jni::Typed::Back) {
            if self.editing.take().is_some() {
                jni::show_keyboard(false);
            } else {
                self.page = Page::Play;
                self.scroll = 0.0;
            }
            return;
        }
        if self.editing.is_none() {
            return;
        }
        for t in typed {
            let Some(editing) = &mut self.editing else {
                return;
            };
            match t {
                jni::Typed::Back => {}
                jni::Typed::Text(text) => {
                    editing.text.extend(text.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '_'));
                    editing.text.truncate(16);
                }
                jni::Typed::Backspace => {
                    editing.text.pop();
                }
                jni::Typed::Enter => self.save_name(),
            }
        }
    }

    fn save_name(&mut self) {
        let Some(editing) = self.editing.take() else {
            return;
        };
        jni::show_keyboard(false);
        let name = editing.text.trim().to_owned();
        if !install::valid_name(&name) {
            self.error = Some("A name is 3 to 16 letters, digits or _.".into());
            return;
        }
        match install::set_player_name(&self.paths, &name) {
            Ok(()) => self.error = None,
            Err(e) => self.error = Some(format!("The name could not be saved: {e}")),
        }
    }

    pub fn about_to_wait(&mut self) {
        self.typed();
        if let Some(task) = &self.task {
            let progress = task.progress();
            if let Some(done) = progress.done {
                match done {
                    Err(e) => self.error = Some(e),
                    // An import's last line says what came in.
                    Ok(()) if task.what == IMPORTING => self.note = Some(progress.line),
                    Ok(()) => {}
                }
                self.task = None;
            }
        }
        match jni::take_picked() {
            Some(jni::Picked::File { path, name }) => self.import(path, name),
            Some(jni::Picked::Failed(why)) => self.error = Some(why),
            Some(jni::Picked::Cancelled) => self.note = None,
            None => {}
        }
        if !self.busy()
            && self.paths.prepared()
            && self.on_screen()
            && let Some(extra) = self.smoke.take()
        {
            log::info!("Tachyon launcher: smoke test, playing with {extra:?}");
            let mut args = install::game_args(&self.paths);
            args.extend(extra);
            self.play = Some(args);
        }
        if let Some(w) = &self.window {
            w.request_redraw();
        }
    }

    fn draw(&mut self) {
        let Some(mut renderer) = self.renderer.take() else {
            return;
        };
        if self.insets.is_none() {
            self.insets = Some(insets());
        }
        let insets = self.insets.unwrap_or_default();
        let inset = |i: usize| insets[i] / renderer.scale;
        let margins = (inset(0).max(GUTTER), inset(2).max(GUTTER));
        let tap = std::mem::take(&mut self.pointer.tap);
        let scroll = std::mem::take(&mut self.pointer.scroll);
        self.scroll = (self.scroll + scroll).min(0.0);
        let tap_pointer = Pointer {
            down: self.pointer.down,
            tap,
            ..Pointer::default()
        };
        let result = renderer.frame(hex(P.page, 1.0), |c| {
            let mut ui = Ui::new(c, &tap_pointer);
            self.layout(&mut ui, margins);
        });
        if let Err(e) = result {
            log::warn!("Tachyon launcher: frame failed: {e}");
        }
        self.renderer = Some(renderer);
    }

    /// The streak behind the wordmark: thin bands from the accent to violet,
    /// fading out to the right.
    fn streak(&self, ui: &mut Ui, x: f32, y: f32, w: f32) {
        let t = self.started.elapsed().as_secs_f32();
        let bands = 48;
        for i in 0..bands {
            let f = i as f32 / bands as f32;
            let alpha = (1.0 - f) * 0.55;
            let shimmer = ((t * 1.6 - f * 6.0).sin() * 0.5 + 0.5) * 0.25;
            let band = Rect::new(x + f * w, y, w / bands as f32 + 0.5, 3.0);
            ui.c.rect(band, 0.0, mix(brand::STREAK[0], brand::STREAK[1], f, alpha + shimmer * (1.0 - f)));
        }
    }

    fn layout(&mut self, ui: &mut Ui, (left, right): (f32, f32)) {
        let (w, h) = (ui.c.width, ui.c.height);
        let content = Rect::new(left, 0.0, w - left - right, h);
        let top = 64.0;
        let page_h = match self.page {
            Page::Play => self.play_page(ui, content, top + self.scroll),
            Page::Settings => self.settings_page(ui, content, top + self.scroll),
        };
        // Scrolling stops at the end of the page.
        let overflow = (page_h - h + 24.0).max(0.0);
        self.scroll = self.scroll.max(-overflow);
        // The bar over the page.
        ui.c.rect(Rect::new(0.0, 0.0, w, 56.0), 0.0, hex(P.page, 1.0));
        ui.text(left, 18.0, brand::NAME, Face::Medium, 17.0, hex(P.ink, 1.0));
        let mut x = content.right();
        for (page, label) in [(Page::Settings, "Settings"), (Page::Play, "Play")] {
            let tw = ui.c.measure(label, Face::Medium, horizon_theme::UI.px) + 28.0;
            x -= tw;
            let r = Rect::new(x, 12.0, tw, CONTROL);
            if self.page == page {
                ui.c.rect(r, r.h / 2.0, hex(P.plate, 1.0));
            }
            ui.centred(
                r,
                label,
                Face::Medium,
                horizon_theme::UI.px,
                hex(if self.page == page { P.ink } else { P.muted }, 1.0),
            );
            if ui.tapped(r) && self.page != page {
                self.page = page;
                self.scroll = 0.0;
                if self.editing.take().is_some() {
                    jni::show_keyboard(false);
                }
            }
            x -= 4.0;
        }
    }

    /// Returns the page's height.
    fn play_page(&mut self, ui: &mut Ui, r: Rect, mut y: f32) -> f32 {
        let status = if self.busy() {
            "Getting ready"
        } else if self.paths.prepared() {
            "Ready"
        } else {
            "Not ready"
        };
        let caption = format!("Minecraft {} · {status}", horizon_core::VANILLA_VERSION);
        ui.text(r.x, y, &caption, Face::Regular, 12.0, hex(P.muted, 1.0));
        y += 20.0;
        let display = 52.0;
        let wordmark = brand::NAME.to_uppercase();
        let mark_w = ui.text(r.x, y, &wordmark, Face::Medium, display, hex(P.ink, 1.0));
        let line = ui.c.line(Face::Medium, display);
        self.streak(ui, r.x, y + line + 2.0, mark_w.max(120.0) * 1.6);
        // Play, the page's one main action.
        let play = Rect::new(r.right() - 160.0, y + 8.0, 160.0, 48.0);
        let ready = self.paths.prepared() && !self.busy();
        if ui.button(play, "Play", true, ready) {
            self.play = Some(install::game_args(&self.paths));
        }
        y += line + 18.0;
        for line in ui.c.wrap(brand::TAGLINE, Face::Regular, 14.0, r.w.min(620.0)) {
            ui.text(r.x, y, &line, Face::Regular, 14.0, hex(P.ink_soft, 1.0));
            y += 21.0;
        }
        y += 14.0;
        ui.c.rule(r.x, y, r.w, hex(P.rule, 1.0));
        if let Some(crash) = self.crash.clone() {
            let row = Rect::new(r.x, y, r.w, ROW);
            ui.c.rect(
                Rect::new(row.x - 10.0, row.y + row.h / 2.0 - 3.0, 6.0, 6.0),
                3.0,
                hex(P.danger, 1.0),
            );
            ui.row(row, 0, "The game stopped with an error last time", &crash, 90.0);
            if ui.row_button(row, "Dismiss", false, true) {
                self.crash = None;
            }
            y += ROW;
        }
        // 01 the player's name.
        let row = Rect::new(r.x, y, r.w, ROW);
        let (name, detail) = match (&self.editing, install::player_name(&self.paths)) {
            (Some(e), _) => (format!("{}|", e.text), "Type a name, then Save.".to_owned()),
            (None, Some(name)) => (format!("Playing as {name}"), "Offline: the name your worlds and LAN games know you by.".to_owned()),
            (None, None) => ("No name yet".to_owned(), "Pick the name you play as (3 to 16 letters, digits or _).".to_owned()),
        };
        ui.row(row, 1, &name, &detail, 90.0);
        if self.editing.is_some() {
            if ui.row_button(row, "Save", true, true) {
                self.save_name();
            }
        } else if ui.row_button(row, "Edit", false, true) {
            self.editing = Some(Editing {
                text: install::player_name(&self.paths).unwrap_or_default(),
            });
            jni::take_typed();
            jni::show_keyboard(true);
        }
        y += ROW;
        // 02 the touch controls.
        let row = Rect::new(r.x, y, r.w, ROW);
        ui.row(
            row,
            2,
            "Touch controls",
            "A joystick and buttons over the game. Off for a controller or a keyboard.",
            48.0,
        );
        if let Some(on) = ui.switch(row, self.touch) {
            self.touch = on;
            set_preference(&self.paths.home, "touchControls", if on { "true" } else { "false" });
        }
        y += ROW;
        // 03 the resources.
        let row = Rect::new(r.x, y, r.w, ROW);
        let detail = if self.paths.prepared() {
            "Unpacked on this phone, from the resources this build of Tachyon carries."
        } else {
            "They unpack from the app on the first start: about a minute."
        };
        ui.row(row, 3, "Resources", detail, 0.0);
        y += ROW;
        // 04 bringing in worlds, packs and mods from the phone's files.
        let row = Rect::new(r.x, y, r.w, ROW * 1.5);
        ui.row(
            row,
            4,
            "Import a world, pack or mod",
            "A world or resource pack (.zip), a Horizon mod (.hmod), or a Java mod (.jar): its blocks, items and recipes come in; what it does in code can't.",
            90.0,
        );
        if ui.row_button(row, "Import", false, !self.busy()) {
            self.note = Some("Pick the file in the phone's files.".into());
            self.error = None;
            jni::pick_file();
        }
        y += ROW * 1.5;
        // 05 how the last session ran, for reporting.
        if let Some(perf) = self.performance.clone() {
            let row = Rect::new(r.x, y, r.w, ROW * 1.5);
            ui.row(row, 5, "Last session's performance", &perf, 70.0);
            if ui.row_button(row, "Copy", false, true) {
                jni::platform("clipboard_set", &perf);
                self.note = Some("Copied: paste it in the chat.".into());
            }
            y += ROW * 1.5;
        }
        y = self.task_and_error(ui, r, y + 12.0);
        y - self.scroll
    }

    fn task_and_error(&mut self, ui: &mut Ui, r: Rect, mut y: f32) -> f32 {
        if let Some(task) = &self.task {
            let progress = task.progress();
            ui.text(r.x, y, task.what, Face::Medium, 14.0, hex(P.ink, 1.0));
            y += 22.0;
            let line = ui.c.fit(&progress.line, Face::Mono, 12.0, r.w);
            ui.text(r.x, y, &line, Face::Mono, 12.0, hex(P.muted, 1.0));
            y += 20.0;
            let t = self.started.elapsed().as_secs_f32();
            ui.progress(Rect::new(r.x, y, r.w, 3.0), progress.fraction, t);
            y += 16.0;
        }
        for (text, color) in [(self.note.clone(), P.ink_soft), (self.error.clone(), P.danger)] {
            let Some(text) = text else {
                continue;
            };
            for line in ui.c.wrap(&text, Face::Regular, 13.0, r.w) {
                ui.text(r.x, y, &line, Face::Regular, 13.0, hex(color, 1.0));
                y += 19.0;
            }
            y += 8.0;
        }
        y
    }

    fn settings_page(&mut self, ui: &mut Ui, r: Rect, mut y: f32) -> f32 {
        ui.text(r.x, y, "Settings", Face::Medium, 44.0, hex(P.ink, 1.0));
        y += ui.c.line(Face::Medium, 44.0) + 4.0;
        ui.text(r.x, y, "The controls and the app's files. The game's own options are in game.", Face::Regular, 14.0, hex(P.muted, 1.0));
        y += 34.0;
        ui.c.rule(r.x, y, r.w, hex(P.rule, 1.0));
        let mut index = 0;
        let mut next = |y: &mut f32| {
            index += 1;
            let row = Rect::new(r.x, *y, r.w, ROW);
            *y += ROW;
            (row, index)
        };
        // The control layout, and Zalith Launcher 2's layouts in and out.
        let (row, i) = next(&mut y);
        let layouts = controls::layouts(&self.paths.home);
        let current = controls::selected(&self.paths.home);
        let index = layouts.iter().position(|(file, _)| *file == current).unwrap_or(0);
        let name = layouts.get(index).map_or("Default", |(_, name)| name.as_str());
        ui.row(
            row,
            i,
            "Control layout",
            &format!("{name}, {} of {}.", index + 1, layouts.len()),
            90.0,
        );
        if ui.row_button(row, "Next", false, layouts.len() > 1) {
            let (file, _) = &layouts[(index + 1) % layouts.len()];
            set_preference(&self.paths.home, "controlLayout", file);
        }
        let (row, i) = next(&mut y);
        let folder = self.paths.shared.join("controls");
        ui.row(
            row,
            i,
            "Import and export layouts",
            &format!(
                "Zalith Launcher 2's layouts work as they are: copy their .json files to {}, then Import. Export puts the current layout there.",
                folder.display()
            ),
            190.0,
        );
        let import_w = ui.button_width("Import");
        let export_w = ui.button_width("Export");
        let import_rect = Rect::new(
            row.right() - export_w - 8.0 - import_w,
            row.y + (row.h - CONTROL) / 2.0,
            import_w,
            CONTROL,
        );
        if ui.button(import_rect, "Import", false, true) {
            match controls::import(&folder, &self.paths.home) {
                Ok(0) => self.note = Some(format!("No layouts to import in {}.", folder.display())),
                Ok(n) => self.note = Some(format!("Imported {n} control layouts.")),
                Err(e) => self.error = Some(e),
            }
        }
        if ui.row_button(row, "Export", false, true) {
            match controls::export(&self.paths.home, &folder) {
                Ok(file) => self.note = Some(format!("Exported to {}.", file.display())),
                Err(e) => self.error = Some(e),
            }
        }
        // Unpacking again, for resources that went missing.
        let (row, i) = next(&mut y);
        ui.row(row, i, "Resources", "Unpack them again from the app, if the game says some are missing.", 140.0);
        if ui.row_button(row, "Unpack again", false, !self.busy()) {
            let _ = std::fs::remove_file(self.paths.root.join(".unpacked"));
            self.unpack();
            self.page = Page::Play;
            self.scroll = 0.0;
        }
        // About.
        let (row, i) = next(&mut y);
        ui.row(
            row,
            i,
            &format!("{} {}", brand::NAME, super::BUILD),
            &format!(
                "Built on Horizon by Ludovic Marie (MIT) and its Android port by MocLG. Source: {}.",
                brand::SOURCE
            ),
            0.0,
        );
        let (row, i) = next(&mut y);
        ui.row(
            row,
            i,
            "Not an official Minecraft product",
            "Not approved by or associated with Mojang or Microsoft. For your own copy of Minecraft: Java Edition.",
            0.0,
        );
        y = self.task_and_error(ui, r, y + 12.0);
        y - self.scroll
    }
}

/// One `key:value` line of the game's horizon.txt (`ui_theme::preference`).
fn set_preference(dir: &std::path::Path, key: &str, value: &str) {
    let path = dir.join("horizon.txt");
    let text = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| l.split_once(':').is_none_or(|(k, _)| k.trim() != key))
        .map(str::to_owned)
        .collect();
    lines.push(format!("{key}:{value}"));
    let _ = std::fs::write(path, lines.join("\n") + "\n");
}
