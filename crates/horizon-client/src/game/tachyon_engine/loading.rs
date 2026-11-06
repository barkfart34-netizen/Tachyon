//! Tachyon's own startup display: a light wordmark over navy, cyan tracks and
//! a short message history, in GUI pixels. Only the idea of a mod-loader window is
//! shared with Forge and NeoForge; none of their code or art is used.
use crate::game::{Progress, UiResources};
use horizon_client::hud::{HudAtlas, HudBatch, HudVertex};
use std::{
    collections::{BTreeMap, VecDeque},
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const LOG_ROWS: usize = 5;
const STEPS: u32 = 8;

const fn color(hex: u32) -> u32 {
    0xff00_0000 | (hex & 0xff) << 16 | (hex & 0xff00) | (hex >> 16 & 0xff)
}
// The launcher's deep-space palette (tachyon/brand.rs).
const PAGE: u32 = color(0x070a14);
const TRACK: u32 = color(0x1b2440);
const INK: u32 = color(0xeaf2ff);
const SOFT: u32 = color(0xbdd0ec);
const ACCENT: u32 = color(0x38d6ff);

/// Fed at the checkpoints, not polled from the atomic: even steps that
/// finish between frames stay in the history. No global logger is replaced.
#[derive(Clone)]
struct Messages(VecDeque<&'static str>);

impl Messages {
    fn new() -> Self {
        Self(VecDeque::with_capacity(LOG_ROWS))
    }
    fn push(&mut self, line: &'static str) {
        if self.0.back() == Some(&line) {
            return;
        }
        if self.0.len() == LOG_ROWS {
            self.0.pop_front();
        }
        self.0.push_back(line);
    }
}

/// Mod ids, not namespaces: one port can own several namespaces. Merge
/// accepted content packages and compiled native ports by their shared ids,
/// not by settings that may name mods which aren't installed.
#[derive(Clone)]
struct ModNames {
    count: usize,
    short: String,
    wide: String,
}

impl ModNames {
    fn new(packages: Vec<(String, String)>) -> Self {
        let mut names = BTreeMap::new();
        for (id, name) in packages {
            if !id.is_empty() {
                names.entry(id).or_insert(name);
            }
        }
        let count = names.len();
        let short = format!(
            "{}: {count} mod{}",
            horizon_client::brand::name(),
            if count == 1 { "" } else { "s" }
        );
        let names = names
            .values()
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join(", ");
        let wide = if count == 0 {
            short.clone()
        } else {
            format!("{short} - {names}")
        };
        Self { count, short, wide }
    }

    fn label(&self, width: f32) -> &str {
        if width >= 340.0 && self.count != 0 {
            &self.wide
        } else {
            &self.short
        }
    }
}

/// The original checkpoints mix starts and completions. In particular 6
/// starts the items/mobs work; it does not complete another step. The eight
/// measured completions are assets, facts, biomes/environment, pack tables,
/// collision/motion, items, recipes/transformers, and the remaining tables.
fn checkpoint(stage: u32) -> (u32, &'static str) {
    match stage {
        0 => (0, "Loading assets"),
        1 => (1, "Loading facts, biomes and environment"),
        2 => (2, "Loading biomes and environment"),
        3 => (3, "Loading pack tables"),
        4 => (4, "Loading collision and motion"),
        5 => (5, "Preparing game resources"),
        6 => (5, "Loading mobs and items"),
        7 => (6, "Loading recipes and transformers"),
        8 => (7, "Loading world resources"),
        _ => (STEPS, "World resources ready"),
    }
}

pub(in crate::game) type Trace = Arc<Mutex<Status>>;

#[derive(Clone)]
pub(in crate::game) struct Status {
    revision: u64,
    done: u32,
    step: &'static str,
    messages: Messages,
    natives: Vec<(String, String)>,
    mods: Option<ModNames>,
}

impl Default for Status {
    fn default() -> Self {
        let step = "Preparing resource packs and mods";
        let mut messages = Messages::new();
        messages.push(step);
        Self {
            revision: 1,
            done: 0,
            step,
            messages,
            natives: Vec::new(),
            mods: None,
        }
    }
}

impl Status {
    pub(in crate::game) fn menu(&mut self) {
        self.step = "Loading menu";
        self.messages.push(self.step);
        self.revision += 1;
    }
    pub(in crate::game) fn checkpoint(&mut self, stage: u32) {
        (self.done, self.step) = checkpoint(stage);
        self.messages.push(self.step);
        self.revision += 1;
    }
    /// Set once before the content worker reports, without another disk scan.
    pub(in crate::game) fn natives(&mut self, packages: Vec<(String, String)>) {
        self.natives = packages;
        self.revision += 1;
    }
    pub(in crate::game) fn mods(&mut self, mut packages: Vec<(String, String)>) {
        // A content fallback does not unload compiled native code.
        packages.extend(self.natives.iter().cloned());
        self.mods = Some(ModNames::new(packages));
        self.revision += 1;
    }
}

/// Quiet, frame-rate independent easing; a suspended window never makes
/// the fill overshoot when it returns.
fn smooth(shown: f32, target: f32, seconds: f32) -> f32 {
    let target = target.clamp(0.0, 1.0);
    shown + (target - shown) * (1.0 - (-12.0 * seconds.clamp(0.0, 0.25)).exp())
}

/// The current work has no item total. Shuttle a 30% segment instead of
/// inventing one; only the final checkpoint is a full current-step bar.
fn segment(seconds: f32, ready: bool) -> (f32, f32) {
    if ready {
        (0.0, 1.0)
    } else {
        let phase = seconds.rem_euclid(2.0) * std::f32::consts::PI;
        (0.35 * (1.0 - phase.cos()), 0.3)
    }
}

#[derive(Debug)]
struct Layout {
    x: f32,
    width: f32,
    title: f32,
    title_scale: f32,
    overall: f32,
    step: f32,
    memory: Option<f32>,
    logs: f32,
    log_rows: usize,
    log_pitch: f32,
    mods: f32,
    bar_height: f32,
}

impl Layout {
    fn new(width: f32, height: f32, memory: bool, margins: [f32; 4]) -> Self {
        // The five-line desktop panel needs 206 pixels inside safe margins.
        let compact = height - margins[1].max(16.0) - margins[3].max(16.0) < 206.0;
        let gutter = if compact { 12.0 } else { 24.0 };
        let edge = if compact { 6.0 } else { 16.0 };
        let [left, top, right, bottom] = margins;
        let (left, right) = (left.max(gutter), right.max(gutter));
        let (top, bottom) = (top.max(edge), bottom.max(edge));
        let available = (height - top - bottom).max(0.0);
        let title_scale = if compact {
            if available < 136.0 {
                1.0
            } else {
                2.0
            }
        } else {
            3.0
        };
        let memory = memory && available >= 127.0;
        let tight = compact && available < 107.0;
        let gap = if tight {
            2.0
        } else if compact {
            4.0
        } else {
            8.0
        };
        let pitch = if tight {
            18.0
        } else if compact {
            20.0
        } else {
            26.0
        };
        let log_rows = if compact { 3 } else { LOG_ROWS };
        let log_pitch = if tight {
            10.0
        } else if compact {
            11.0
        } else {
            12.0
        };
        let overall = 9.0 * title_scale + 2.0 * gap;
        let logs = overall + pitch * if memory { 3.0 } else { 2.0 } + gap;
        let mods = logs + log_rows as f32 * log_pitch + gap;
        let title = top
            + ((available - mods - 9.0) * 0.5)
                .max(0.0)
                .min(available * 0.18);
        let panel = (width - left - right).clamp(0.0, 560.0);
        Self {
            x: left + ((width - left - right - panel) * 0.5).max(0.0),
            width: panel,
            title,
            title_scale,
            overall: title + overall,
            step: title + overall + pitch,
            memory: memory.then_some(title + overall + 2.0 * pitch),
            logs: title + logs,
            log_rows,
            log_pitch,
            mods: title + mods,
            bar_height: if tight {
                3.0
            } else if compact {
                4.0
            } else {
                6.0
            },
        }
    }
}

pub(in crate::game) fn early_gui() -> UiResources {
    UiResources {
        atlas: HudAtlas::loading(),
        panorama: Arc::new(horizon_client::gfx::panorama::Faces::colour([0; 4])),
    }
}

/// Kept across the core-to-game handoff: animation, messages, mod names and
/// the last memory sample don't restart when the second worker is running.
pub(in crate::game) struct Screen {
    status: Status,
    overall_text: String,
    shown: f32,
    started: Instant,
    frame: Instant,
    sampled: Option<Instant>,
    memory: Option<(u64, u64)>,
    memory_text: String,
    font_epoch: u64,
    label_layout: Option<(f32, f32, [f32; 4])>,
    labels: Vec<HudVertex>,
    pending_patches: Vec<horizon_client::font::Patch>,
}

impl Screen {
    pub(in crate::game) fn for_brand(started: Instant) -> Option<Self> {
        (horizon_client::brand::name() == "Tachyon").then(|| Self::new(started))
    }

    fn new(started: Instant) -> Self {
        Self {
            status: Status {
                revision: 0,
                ..Default::default()
            },
            overall_text: format!("0 / {STEPS} steps"),
            shown: 0.0,
            started,
            frame: Instant::now(),
            sampled: None,
            memory: None,
            memory_text: String::with_capacity(64),
            font_epoch: 0,
            label_layout: None,
            labels: Vec::new(),
            pending_patches: Vec::new(),
        }
    }

    pub(in crate::game) fn draw(&mut self, batch: &mut HudBatch, atlas: &HudAtlas, progress: &Progress) {
        if self.font_epoch != atlas.font.epoch() {
            // A new renderer starts from the atlas's source pixels. Cached
            // glyph positions and patches from the previous texture no longer apply.
            self.pending_patches.clear();
            self.font_epoch = atlas.font.epoch();
            self.label_layout = None;
        }
        batch.patches.append(&mut self.pending_patches);
        if let Some(trace) = &progress.trace {
            let status = trace.lock().unwrap();
            if status.revision != self.status.revision {
                self.status = status.clone();
                self.overall_text.clear();
                let _ = write!(self.overall_text, "{} / {STEPS} steps", status.done);
                self.label_layout = None;
            }
        }
        let now = Instant::now();
        self.shown = smooth(
            self.shown,
            self.status.done as f32 / STEPS as f32,
            now.duration_since(self.frame).as_secs_f32(),
        );
        self.frame = now;
        // Two small /proc reads, or two Windows API calls, once a second.
        // A failed sample keeps the last good one instead of flickering.
        if self
            .sampled
            .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(1))
        {
            self.sampled = Some(now);
            if let Some(memory) = horizon_threads::process_memory().filter(|m| m.1 != 0)
                && self.memory != Some(memory)
            {
                self.memory = Some(memory);
                self.memory_text.clear();
                let _ = write!(
                    self.memory_text,
                    "Memory: {} / {} MB (RAM)",
                    memory.0 / (1024 * 1024),
                    memory.1 / (1024 * 1024)
                );
                self.label_layout = None;
            }
        }
        #[cfg(target_os = "android")]
        let margins = crate::game::android::hud_margins().map(|m| m / batch.scale);
        #[cfg(not(target_os = "android"))]
        let margins = [0.0; 4];
        let l = Layout::new(batch.width, batch.height, self.memory.is_some(), margins);
        let (w, h) = (batch.width, batch.height);
        batch.vertices.reserve(self.labels.len() + 48);
        batch.fill(atlas, 0.0, 0.0, w, h, PAGE);
        if l.width <= 0.0 {
            return;
        }
        let layout = (w, h, margins);
        if self.label_layout != Some(layout) {
            // HudBatch.text resolves and lays out text with temporary buffers.
            // Do that only when a label, layout or font changes, not each frame.
            let first = batch.vertices.len();
            labels(
                batch,
                atlas,
                &l,
                &self.status,
                &self.overall_text,
                &self.memory_text,
            );
            self.labels.clear();
            self.labels.extend_from_slice(&batch.vertices[first..]);
            self.label_layout = Some(layout);
        } else {
            batch.vertices.extend_from_slice(&self.labels);
        }
        bar(batch, atlas, &l, l.overall, (0.0, self.shown), ACCENT);
        bar(
            batch,
            atlas,
            &l,
            l.step,
            segment(
                now.duration_since(self.started).as_secs_f32(),
                self.status.done == STEPS,
            ),
            ACCENT,
        );
        if let (Some(y), Some((used, max))) = (l.memory, self.memory) {
            bar(
                batch,
                atlas,
                &l,
                y,
                (0.0, (used as f64 / max as f64).clamp(0.0, 1.0) as f32),
                SOFT,
            );
        }
    }

    /// Surface acquisition can skip a frame before glyph patches are uploaded.
    /// Retain them for the next attempt rather than losing cached letters or
    /// rasterizing them again on every frame of an occluded window.
    pub(in crate::game) fn after_frame(&mut self, batch: &mut HudBatch, submitted: bool) {
        if !submitted {
            self.pending_patches.append(&mut batch.patches);
        }
    }
}

fn labels(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    l: &Layout,
    status: &Status,
    overall: &str,
    memory: &str,
) {
    let name = horizon_client::brand::name();
    let name_width = HudBatch::text_width(atlas, name).max(1.0);
    let title_scale = l.title_scale.min(l.width / name_width);
    let x = l.x + (l.width - name_width * title_scale) * 0.5;
    let first = batch.vertices.len();
    batch.text(atlas, name, x, l.title, INK, false);
    batch.scale_since(first, x, l.title, title_scale, title_scale);
    let rule = l.width.min(32.0);
    batch.fill(
        atlas,
        l.x + (l.width - rule) * 0.5,
        l.overall - 4.0,
        rule,
        1.0,
        ACCENT,
    );
    let detail = HudBatch::text_width(atlas, overall).min(l.width);
    line(
        batch,
        atlas,
        "Overall loading",
        l.x,
        l.overall,
        (l.width - detail - 8.0).max(0.0),
        SOFT,
    );
    line(
        batch,
        atlas,
        overall,
        l.x + l.width - detail,
        l.overall,
        detail,
        SOFT,
    );
    line(batch, atlas, status.step, l.x, l.step, l.width, INK);
    if let Some(y) = l.memory {
        line(batch, atlas, memory, l.x, y, l.width, SOFT);
    }
    let messages = &status.messages.0;
    for (i, message) in messages
        .iter()
        .skip(messages.len().saturating_sub(l.log_rows))
        .enumerate()
    {
        line(
            batch,
            atlas,
            message,
            l.x,
            l.logs + i as f32 * l.log_pitch,
            l.width,
            SOFT,
        );
    }
    let mods = status
        .mods
        .as_ref()
        .map_or("Tachyon: discovering mods", |mods| mods.label(l.width));
    line(batch, atlas, mods, l.x, l.mods, l.width, INK);
}

fn bar(
    batch: &mut HudBatch,
    atlas: &HudAtlas,
    l: &Layout,
    y: f32,
    (start, width): (f32, f32),
    color: u32,
) {
    batch.fill(atlas, l.x, y + 11.0, l.width, l.bar_height, TRACK);
    if width > 0.0 {
        batch.fill(
            atlas,
            l.x + start * l.width,
            y + 11.0,
            width * l.width,
            l.bar_height,
            color,
        );
    }
}

/// Font.fit returns a character count, not a byte offset. Trim without
/// allocating, keeping non-ASCII package names at a UTF-8 boundary.
fn end(atlas: &HudAtlas, text: &str, width: f32) -> usize {
    text.char_indices()
        .nth(atlas.font.fit(text, width))
        .map_or(text.len(), |(i, _)| i)
}

fn line(batch: &mut HudBatch, atlas: &HudAtlas, text: &str, x: f32, y: f32, width: f32, color: u32) {
    if width <= 0.0 {
        return;
    }
    let at = end(atlas, text, width);
    if at == text.len() {
        batch.text(atlas, text, x, y, color, false);
    } else {
        let dots = HudBatch::text_width(atlas, "...");
        if dots <= width {
            let at = end(atlas, text, width - dots);
            let advance = batch.text(atlas, &text[..at], x, y, color, false);
            batch.text(atlas, "...", x + advance, y, color, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_ring_keeps_the_last_five_steps_in_order() {
        let mut messages = Messages::new();
        assert!(messages.0.is_empty());
        let capacity = messages.0.capacity();
        for line in [
            "menu", "assets", "facts", "biomes", "packs", "items", "items", "recipes",
        ] {
            messages.push(line);
        }
        assert_eq!(
            messages.0.iter().copied().collect::<Vec<_>>(),
            ["facts", "biomes", "packs", "items", "recipes"]
        );
        assert_eq!(messages.0.capacity(), capacity);
    }

    #[test]
    fn progress_counts_completions_not_the_start_of_items() {
        let expected = [0, 1, 2, 3, 4, 5, 5, 6, 7, 8];
        for (stage, done) in expected.into_iter().enumerate() {
            assert_eq!(checkpoint(stage as u32).0, done);
        }
        assert_eq!(checkpoint(6).1, "Loading mobs and items");
        assert_eq!(checkpoint(u32::MAX).0, STEPS);
        let mut status = Status::default();
        status.menu();
        for stage in 0..=9 {
            status.checkpoint(stage);
        }
        assert_eq!(status.done, STEPS);
        assert_eq!(status.messages.0.len(), LOG_ROWS);
        assert_eq!(status.messages.0.back(), Some(&"World resources ready"));
    }

    #[test]
    fn worker_checkpoints_share_history_but_pack_reload_keeps_its_own_scale() {
        let trace: Trace = Arc::new(Mutex::new(Status::default()));
        let progress = Progress {
            trace: Some(trace.clone()),
            ..Progress::with(&crate::game::STAGE_LINES)
        };
        let worker = progress.clone();
        for stage in 0..=9 {
            worker.set(stage);
        }
        worker.mods(vec![
            ("kinetica".into(), "Kinetica".into()),
            ("lost_realms".into(), "Lost Realms".into()),
            ("chronicles".into(), "Chronicles".into()),
            ("create".into(), "Create".into()),
        ]);
        let status = trace.lock().unwrap();
        assert_eq!(status.done, STEPS);
        assert_eq!(status.messages.0.len(), LOG_ROWS);
        assert_eq!(status.mods.as_ref().unwrap().count, 4);
        assert_eq!(progress.fraction(), 1.0);

        let reload = Progress::with(&["Prepare packs", "Load packs", "Packs ready"]);
        reload.set(1);
        assert!(reload.trace.is_none());
        assert_eq!(reload.line(), "Load packs");
        assert_eq!(reload.fraction(), 0.5);
    }

    #[test]
    fn bars_ease_without_overshoot_and_unknown_work_stays_indeterminate() {
        let first = smooth(0.0, 0.75, 1.0 / 60.0);
        assert!(first > 0.0 && first < 0.75);
        assert_eq!(smooth(first, 0.75, 0.0), first);
        assert!(smooth(first, 1.5, 30.0) <= 1.0);
        let twice = smooth(smooth(0.0, 0.75, 0.05), 0.75, 0.05);
        assert!((twice - smooth(0.0, 0.75, 0.1)).abs() < 0.00001);
        for i in 0..100 {
            let (start, width) = segment(i as f32 / 10.0, false);
            assert_eq!(width, 0.3);
            assert!(start >= 0.0 && start + width <= 1.00001);
        }
        assert_eq!(segment(1.0, true), (0.0, 1.0));
    }

    #[test]
    fn mods_are_deduplicated_by_package_id_not_display_name_or_namespace() {
        let names = ModNames::new(vec![
            ("kinetica".into(), "Kinetica".into()),
            ("create".into(), "Create".into()),
            ("create".into(), "Duplicate port".into()),
            ("other".into(), "Create".into()),
            (String::new(), "Unreadable package".into()),
        ]);
        assert_eq!(names.count, 3);
        assert!(names.wide.ends_with("Create, Kinetica, Create"));
        assert!(!names.wide.contains("Duplicate port"));
        assert_eq!(names.label(294.0), names.short);
        assert_eq!(names.label(378.0), names.wide);
        let empty = ModNames::new(Vec::new());
        assert_eq!(empty.count, 0);
        assert_eq!(empty.short, empty.wide);
        let one = ModNames::new(vec![("create".into(), "Create".into())]);
        assert!(one.short.ends_with(": 1 mod"));
    }

    #[test]
    fn enabled_native_ports_join_content_once_and_survive_a_content_fallback() {
        let disabled = std::collections::BTreeSet::from([
            "fps_overlay".to_owned(),
            "keystrokes".to_owned(),
        ]);
        let native = crate::game::tachyon_mods::loading_mods(&disabled);
        assert_eq!(native.len(), 5);
        let mut status = Status::default();
        status.natives(native);
        status.mods(vec![
            ("kinetica".into(), "Kinetica".into()),
            ("lost_realms".into(), "Lost Realms".into()),
            ("chronicles".into(), "Chronicles".into()),
            ("create".into(), "Create".into()),
        ]);
        let names = status.mods.as_ref().unwrap();
        assert_eq!(names.count, 8);
        assert_eq!(names.wide.matches("Chronicles").count(), 1);
        assert!(names.wide.contains("Minimap"));
        assert!(!names.wide.contains("FPS Counter"));
        assert!(!names.wide.contains("Keystrokes"));

        status.mods(Vec::new());
        let names = status.mods.as_ref().unwrap();
        assert_eq!(names.count, 5);
        assert!(!names.wide.contains("Create"));
        let all: std::collections::BTreeSet<String> =
            crate::game::tachyon_mods::loading_mods(&Default::default())
                .into_iter()
                .map(|(id, _)| id)
                .collect();
        assert!(crate::game::tachyon_mods::loading_mods(&all).is_empty());
    }

    fn fits(l: &Layout, width: f32, height: f32) {
        assert!(l.x >= 0.0 && l.x + l.width <= width);
        assert!(l.title >= 0.0);
        assert!(l.title + 9.0 * l.title_scale < l.overall);
        assert!(l.overall + 11.0 + l.bar_height < l.step);
        let last = l.memory.unwrap_or(l.step);
        assert!(last + 11.0 + l.bar_height < l.logs);
        assert!(l.logs + l.log_rows as f32 * l.log_pitch <= l.mods);
        assert!(l.mods + 9.0 <= height);
    }

    #[test]
    fn landscape_phone_fits_three_bars_and_three_messages_inside_safe_edges() {
        let l = Layout::new(320.0, 160.0, true, [20.0, 8.0, 12.0, 6.0]);
        fits(&l, 308.0, 154.0);
        assert!(l.x >= 20.0 && l.memory.is_some());
        assert_eq!(l.log_rows, 3);
        let without_memory = Layout::new(320.0, 160.0, false, [0.0; 4]);
        fits(&without_memory, 320.0, 160.0);
        assert!(without_memory.memory.is_none());
        let notched = Layout::new(320.0, 160.0, true, [24.0, 30.0, 24.0, 30.0]);
        fits(&notched, 296.0, 130.0);
        assert!(notched.memory.is_none());
    }

    #[test]
    fn desktop_centers_a_bounded_panel_with_five_messages() {
        for (w, h) in [(426.0, 240.0), (1280.0, 720.0), (3840.0, 2160.0)] {
            let l = Layout::new(w, h, true, [0.0; 4]);
            fits(&l, w, h);
            assert_eq!(l.log_rows, LOG_ROWS);
            assert!(l.width <= 560.0);
            assert!((l.x + l.width * 0.5 - w * 0.5).abs() < 0.01);
        }
        let short = Layout::new(426.0, 230.0, true, [0.0; 4]);
        fits(&short, 426.0, 230.0);
        assert_eq!(short.log_rows, 3);
    }

    #[test]
    fn early_font_needs_no_game_assets_and_trims_unicode_at_a_byte_boundary() {
        let atlas = HudAtlas::loading();
        assert_eq!(atlas.pixels.len(), (atlas.width * atlas.height * 4) as usize);
        assert!(atlas.glyph('T').is_some());
        let text = "Créate";
        let at = end(&atlas, text, HudBatch::text_width(&atlas, "Cré"));
        assert!(text.is_char_boundary(at));
        assert_eq!(&text[..at], "Cré");
    }

    #[test]
    fn cached_labels_refresh_on_progress_and_resize() {
        let progress = Progress {
            trace: Some(Arc::new(Mutex::new(Status::default()))),
            ..Progress::with(&crate::game::STAGE_LINES)
        };
        let atlas = HudAtlas::loading();
        let mut screen = Screen::new(Instant::now());
        screen.sampled = Some(Instant::now() + Duration::from_secs(3600));
        let mut batch = HudBatch::new([640, 480]);
        batch.width = 320.0;
        batch.height = 160.0;
        screen.draw(&mut batch, &atlas, &progress);
        let original = screen.labels.iter().map(|v| v.pos).collect::<Vec<_>>();
        let capacity = screen.labels.capacity();

        batch.vertices.clear();
        batch.patches.clear();
        screen.draw(&mut batch, &atlas, &progress);
        assert_eq!(screen.labels.capacity(), capacity);
        assert_eq!(
            screen.labels.iter().map(|v| v.pos).collect::<Vec<_>>(),
            original
        );

        progress.set(6);
        batch.vertices.clear();
        screen.draw(&mut batch, &atlas, &progress);
        assert_eq!(screen.overall_text, "5 / 8 steps");
        assert_ne!(
            screen.labels.iter().map(|v| v.pos).collect::<Vec<_>>(),
            original
        );

        batch.width = 640.0;
        batch.vertices.clear();
        screen.draw(&mut batch, &atlas, &progress);
        assert_eq!(screen.label_layout.unwrap().0, 640.0);
    }

    #[test]
    fn skipped_frames_retain_glyphs_until_submission_and_forget_an_old_atlas() {
        let atlas = HudAtlas::loading();
        let progress = Progress::with(&crate::game::STAGE_LINES);
        let mut screen = Screen::new(Instant::now());
        // Keep this glyph test independent of process-memory readings.
        screen.sampled = Some(Instant::now() + Duration::from_secs(3600));
        let mut first = HudBatch::new([640, 480]);
        screen.draw(&mut first, &atlas, &progress);
        let glyphs = first.patches.len();
        assert!(glyphs > 0);
        screen.after_frame(&mut first, false);
        assert!(first.patches.is_empty());

        let mut retry = HudBatch::new([640, 480]);
        screen.draw(&mut retry, &atlas, &progress);
        assert_eq!(retry.patches.len(), glyphs);
        screen.after_frame(&mut retry, true);
        let mut next = HudBatch::new([640, 480]);
        screen.draw(&mut next, &atlas, &progress);
        assert!(next.patches.is_empty());

        screen.pending_patches.push(horizon_client::font::Patch {
            x: u32::MAX,
            y: 0,
            width: 1,
            height: 1,
            rgba: vec![255; 4],
        });
        atlas.font.reset_cache();
        let mut replacement = HudBatch::new([640, 480]);
        screen.draw(&mut replacement, &atlas, &progress);
        assert!(!replacement.patches.is_empty());
        assert!(replacement.patches.iter().all(|p| p.x != u32::MAX));
        assert_eq!(screen.font_epoch, atlas.font.epoch());
    }
}
