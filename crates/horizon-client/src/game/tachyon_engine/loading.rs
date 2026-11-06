//! Tachyon's own startup display, in GUI pixels: two of Create's cogwheels
//! turning faster as the load goes on, over drifting stars, the wordmark, the
//! step with its percent, a bar, a tip that changes every few seconds, and
//! the time (with an estimate from the last load), memory and mods. Only the idea of a mod-loader window is
//! shared with Forge and NeoForge; none of their code or art is used.
use crate::game::{Progress, UiResources};
use horizon_client::hud::{HudAtlas, HudBatch, HudVertex};
use std::{
    collections::{BTreeMap, VecDeque},
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
const PAGE_LOW: u32 = color(0x0d1630);
const GLOW: u32 = color(0x7fe6ff);
const DIM: u32 = color(0x7d8fb0);
const STAR: u32 = color(0xcfe3ff);
const COG: u32 = color(0x2a6f8f);
const COG_FACE: u32 = color(0x1c3a52);

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

/// Two meshing cogwheels, Create's large and small: the small turns twice as
/// fast the other way. Their speed follows the loading's progress.
const LARGE_TEETH: u32 = 16;
const SMALL_TEETH: u32 = 8;
/// How long a tip stays, and how long it fades in.
const TIP_SECONDS: f32 = 7.0;
const TIP_FADE: f32 = 0.6;
const STARS: u32 = 56;
/// The last complete load's length, to say about how long is left.
const LOAD_SECONDS_FILE: &str = ".tachyon-load-seconds";

/// The tips the screen turns through, Indonesian then English.
const TIPS: [(&str, &str); 14] = [
    (
        "Roda gigi kecil yang bersentuhan berputar berlawanan arah.",
        "Small cogwheels that mesh turn opposite ways.",
    ),
    (
        "Roda gigi besar ke kecil: putarannya jadi dua kali lebih cepat.",
        "A large cogwheel driving a small one doubles the speed.",
    ),
    (
        "Pakai goggles untuk melihat RPM dan stress sebuah jaringan.",
        "Wear goggles to read a network's speed and stress.",
    ),
    (
        "Jaringan yang kelebihan stress berhenti: tambah sumber putaran.",
        "An overstressed network stops: add another source.",
    ),
    (
        "Kincir air berputar mengikuti arah aliran air di sekitarnya.",
        "A water wheel turns the way the water around it flows.",
    ),
    (
        "Kipas ke air mencuci barang, ke api mengasap, ke lava membakar.",
        "A fan through water washes, through fire smokes, through lava blasts.",
    ),
    (
        "Press di atas basin memadatkan isinya (compacting).",
        "A press over a basin compacts what is in it.",
    ),
    (
        "Blaze burner di bawah basin memberi panas untuk resep heated.",
        "A blaze burner under a basin heats it for heated recipes.",
    ),
    (
        "Dua crushing wheel yang berlawanan menghancurkan barang di celahnya.",
        "Two crushing wheels turning inwards crush what falls between them.",
    ),
    (
        "Mechanical arm memindahkan barang antar depot, corong, dan mesin.",
        "A mechanical arm moves items between depots, funnels and machines.",
    ),
    (
        "Buku quest Chronicles menunjukkan apa yang bisa dibuat berikutnya.",
        "The Chronicles quest book shows what to build next.",
    ),
    (
        "Reruntuhan Lost Realms menyimpan peti di menara pengawasnya.",
        "Lost Realms ruins keep chests in their watchtowers.",
    ),
    (
        "Di HP, tombol bola T membuka menu kontrol: Zalith atau Bedrock.",
        "On a phone, the T ball opens the controls menu: Zalith or Bedrock.",
    ),
    (
        "Loading pertama setelah update lebih lama: konten mod disusun sekali.",
        "The first load after an update is longer: mod content is built once.",
    ),
];

/// A step's words in Indonesian, for a game set to it.
fn indonesian(step: &'static str) -> &'static str {
    match step {
        "Preparing resource packs and mods" => "Menyiapkan resource pack dan mod",
        "Loading menu" => "Memuat menu",
        "Loading assets" => "Memuat aset",
        "Loading facts, biomes and environment" => "Memuat data blok dan bioma",
        "Loading biomes and environment" => "Memuat bioma dan lingkungan",
        "Loading pack tables" => "Memuat tabel pack",
        "Loading collision and motion" => "Memuat tabrakan dan gerak",
        "Preparing game resources" => "Menyiapkan sumber daya game",
        "Loading mobs and items" => "Memuat mob dan item",
        "Loading recipes and transformers" => "Memuat resep",
        "Loading world resources" => "Memuat sumber daya dunia",
        "World resources ready" => "Dunia siap",
        other => other,
    }
}

/// Whether the game's language is Indonesian (`options.txt`, read only).
fn game_is_indonesian() -> bool {
    std::fs::read_to_string("options.txt").is_ok_and(|text| {
        text.lines()
            .any(|line| line.trim().strip_prefix("lang:") == Some("id_id"))
    })
}

/// `m:ss`.
fn clock(seconds: f32) -> String {
    let s = seconds.max(0.0) as u32;
    format!("{}:{:02}", s / 60, s % 60)
}

/// A colour's alpha scaled by `a` (0..1).
fn faded(color: u32, a: f32) -> u32 {
    let alpha = ((color >> 24) as f32 * a.clamp(0.0, 1.0)) as u32;
    (color & 0x00ff_ffff) | alpha << 24
}

/// Where everything goes, in GUI pixels inside the safe edges: the cogs
/// above the panel, or beside it on a short (landscape phone) screen.
#[derive(Debug)]
struct Layout {
    /// The large cog's centre and radius (to its teeth's tips); the small
    /// one meshes at its upper right.
    cog: (f32, f32, f32),
    x: f32,
    width: f32,
    title: f32,
    title_scale: f32,
    step: f32,
    bar: f32,
    bar_height: f32,
    tip: f32,
    footer: f32,
}

/// Where the small cog sits, from the large one's centre and radius.
fn small_cog(cx: f32, cy: f32, r: f32) -> (f32, f32, f32) {
    let rs = r * 0.55;
    let d = (r + rs) * 0.84;
    let a = -0.55f32;
    (cx + d * a.cos(), cy + d * a.sin(), rs)
}

impl Layout {
    fn new(width: f32, height: f32, margins: [f32; 4]) -> Self {
        let [left, top, right, bottom] = margins;
        let (left, right) = (left.max(12.0), right.max(12.0));
        let (top, bottom) = (top.max(8.0), bottom.max(8.0));
        let inner_w = (width - left - right).max(0.0);
        let inner_h = (height - top - bottom).max(0.0);
        let footer = top + inner_h - 9.0;
        // A short screen (a phone held sideways) puts the cogs beside the panel.
        let side = inner_h < 180.0 && inner_w > 1.6 * inner_h;
        if side {
            let title_scale = if inner_h >= 120.0 { 2.0 } else { 1.0 };
            let panel_h = 9.0 * title_scale + 8.0 + 9.0 + 4.0 + 4.0 + 8.0 + 9.0;
            let box_size = (inner_h - 12.0).min(inner_w * 0.34).max(0.0);
            let r = box_size * 0.36;
            let panel_x = left + box_size + 12.0;
            let width = (inner_w - box_size - 12.0).clamp(0.0, 420.0);
            let title = top + ((inner_h - 12.0 - panel_h) * 0.5).max(0.0);
            Self {
                cog: (left + box_size * 0.42, top + (inner_h - 12.0) * 0.55, r),
                x: panel_x,
                width,
                title,
                title_scale,
                step: title + 9.0 * title_scale + 8.0,
                bar: title + 9.0 * title_scale + 8.0 + 13.0,
                bar_height: 4.0,
                tip: title + 9.0 * title_scale + 8.0 + 13.0 + 4.0 + 8.0,
                footer,
            }
        } else {
            let title_scale = if inner_h >= 260.0 { 3.0 } else { 2.0 };
            let panel_h = 9.0 * title_scale + 10.0 + 9.0 + 4.0 + 6.0 + 12.0 + 9.0;
            let cog_box = ((inner_h - 14.0 - panel_h) * 0.8).clamp(0.0, 130.0);
            let r = cog_box * 0.42;
            let width = inner_w.min(420.0);
            let x = left + (inner_w - width) * 0.5;
            let block = cog_box + 10.0 + panel_h;
            let start = top + ((inner_h - 14.0 - block) * 0.45).max(0.0);
            let title = start + cog_box + 10.0;
            let step = title + 9.0 * title_scale + 10.0;
            Self {
                cog: (left + inner_w * 0.5 - r * 0.35, start + cog_box * 0.58, r),
                x,
                width,
                title,
                title_scale,
                step,
                bar: step + 13.0,
                bar_height: 6.0,
                tip: step + 13.0 + 6.0 + 12.0,
                footer,
            }
        }
    }
}

/// A cogwheel: a disc with `teeth` teeth at `angle`, a darker hub and its
/// shaft's square hole, all of quads.
fn cog(batch: &mut HudBatch, atlas: &HudAtlas, (cx, cy, r): (f32, f32, f32), teeth: u32, angle: f32) {
    if r < 2.0 {
        return;
    }
    let body = r * 0.8;
    let segments = 36;
    let point = |a: f32, d: f32| [cx + d * a.cos(), cy + d * a.sin()];
    let disc = |batch: &mut HudBatch, d: f32, color: u32| {
        for i in 0..segments {
            let a0 = angle + i as f32 / segments as f32 * std::f32::consts::TAU;
            let a1 = angle + (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
            let (p0, p1) = (point(a0, d), point(a1, d));
            batch.fill_quad(atlas, [[cx, cy], p0, p1, p1], color);
        }
    };
    let step = std::f32::consts::TAU / teeth as f32;
    for k in 0..teeth {
        let a = angle + k as f32 * step;
        let half = step * 0.22;
        let tip = step * 0.15;
        batch.fill_quad(
            atlas,
            [
                point(a - half, body * 0.96),
                point(a - tip, r),
                point(a + tip, r),
                point(a + half, body * 0.96),
            ],
            COG,
        );
    }
    disc(batch, body, COG);
    disc(batch, body * 0.78, COG_FACE);
    disc(batch, body * 0.34, COG);
    let s = body * 0.13;
    let corner = |dx: f32, dy: f32| {
        let (c, sn) = (angle.cos(), angle.sin());
        [cx + dx * c - dy * sn, cy + dx * sn + dy * c]
    };
    batch.fill_quad(
        atlas,
        [corner(-s, -s), corner(s, -s), corner(s, s), corner(-s, s)],
        PAGE,
    );
}

/// A small integer hash, for the stars' fixed places.
fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ x >> 16
}

/// Slow stars drifting left, twinkling: the launcher's deep space.
fn stars(batch: &mut HudBatch, atlas: &HudAtlas, w: f32, h: f32, seconds: f32) {
    for i in 0..STARS {
        let a = hash(i * 3 + 1);
        let b = hash(i * 3 + 2);
        let c = hash(i * 3 + 3);
        let speed = 2.0 + (a % 7) as f32;
        let x = ((b % 10_000) as f32 / 10_000.0 * (w + 8.0) - seconds * speed).rem_euclid(w + 8.0) - 4.0;
        let y = (c % 10_000) as f32 / 10_000.0 * h;
        let twinkle = 0.45 + 0.4 * (seconds * (0.6 + (a % 5) as f32 * 0.25) + i as f32).sin();
        let size = if a % 9 == 0 { 2.0 } else { 1.0 };
        batch.fill(atlas, x, y, size, size, faded(STAR, twinkle));
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
    percent: u32,
    shown: f32,
    started: Instant,
    frame: Instant,
    /// The cogs' turn, integrated so a change of speed doesn't jump.
    turn: f32,
    sampled: Option<Instant>,
    memory: Option<(u64, u64)>,
    /// The game's language is Indonesian.
    indonesian: bool,
    /// The last complete load's seconds, and whether this one was saved.
    expected: Option<f32>,
    saved: bool,
    font_epoch: u64,
    /// What the cached labels were laid out for: size, margins, percent,
    /// tip, the second shown and the memory sample.
    label_key: Option<LabelKey>,
    labels: Vec<HudVertex>,
    /// Where the tip's text is, to fade it in.
    tip_vertices: std::ops::Range<usize>,
    pending_patches: Vec<horizon_client::font::Patch>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct LabelKey {
    size: (f32, f32),
    margins: [f32; 4],
    percent: u32,
    tip: usize,
    second: u32,
    revision: u64,
}

impl Screen {
    pub(in crate::game) fn for_brand(started: Instant) -> Option<Self> {
        (horizon_client::brand::name() == "Tachyon").then(|| {
            let mut screen = Self::new(started);
            screen.indonesian = game_is_indonesian();
            screen.expected = std::fs::read_to_string(LOAD_SECONDS_FILE)
                .ok()
                .and_then(|t| t.trim().parse::<f32>().ok())
                .filter(|s| s.is_finite() && *s > 0.0);
            screen
        })
    }

    fn new(started: Instant) -> Self {
        Self {
            status: Status {
                revision: 0,
                ..Default::default()
            },
            percent: 0,
            shown: 0.0,
            started,
            frame: Instant::now(),
            turn: 0.0,
            sampled: None,
            memory: None,
            indonesian: false,
            expected: None,
            saved: false,
            font_epoch: 0,
            label_key: None,
            labels: Vec::new(),
            tip_vertices: 0..0,
            pending_patches: Vec::new(),
        }
    }

    fn say<'a>(&self, english: &'a str, indonesian: &'a str) -> &'a str {
        if self.indonesian { indonesian } else { english }
    }

    pub(in crate::game) fn draw(&mut self, batch: &mut HudBatch, atlas: &HudAtlas, progress: &Progress) {
        if self.font_epoch != atlas.font.epoch() {
            // A new renderer starts from the atlas's source pixels. Cached
            // glyph positions and patches from the previous texture no longer apply.
            self.pending_patches.clear();
            self.font_epoch = atlas.font.epoch();
            self.label_key = None;
        }
        batch.patches.append(&mut self.pending_patches);
        if let Some(trace) = &progress.trace {
            let status = trace.lock().unwrap();
            if status.revision != self.status.revision {
                self.status = status.clone();
            }
        }
        let now = Instant::now();
        let seconds = now.duration_since(self.started).as_secs_f32();
        let dt = now.duration_since(self.frame).as_secs_f32().clamp(0.0, 0.25);
        self.shown = smooth(self.shown, self.status.done as f32 / STEPS as f32, dt);
        self.percent = (self.shown * 100.0).round() as u32;
        self.turn += dt * (0.5 + 2.5 * self.shown);
        self.frame = now;
        let ready = self.status.done == STEPS;
        if ready && !self.saved {
            // Remembered for the next start's estimate.
            self.saved = true;
            let _ = std::fs::write(LOAD_SECONDS_FILE, format!("{seconds:.0}"));
        }
        // Two small /proc reads, or two Windows API calls, once a second.
        if self
            .sampled
            .is_none_or(|at| now.duration_since(at) >= Duration::from_secs(1))
        {
            self.sampled = Some(now);
            if let Some(memory) = horizon_threads::process_memory().filter(|m| m.1 != 0) {
                self.memory = Some(memory);
            }
        }
        #[cfg(target_os = "android")]
        let margins = crate::game::android::hud_margins().map(|m| m / batch.scale);
        #[cfg(not(target_os = "android"))]
        let margins = [0.0; 4];
        let (w, h) = (batch.width, batch.height);
        let l = Layout::new(w, h, margins);
        batch.vertices.reserve(self.labels.len() + 1200);
        batch.fill_gradient(atlas, (0.0, 0.0, w, h), PAGE, PAGE_LOW);
        stars(batch, atlas, w, h, seconds);
        let (cx, cy, r) = l.cog;
        cog(batch, atlas, small_cog(cx, cy, r), SMALL_TEETH, -2.0 * self.turn + 0.2);
        cog(batch, atlas, l.cog, LARGE_TEETH, self.turn);
        if l.width <= 0.0 {
            return;
        }
        let tip = (seconds / TIP_SECONDS) as usize % TIPS.len();
        let key = LabelKey {
            size: (w, h),
            margins,
            percent: self.percent,
            tip,
            second: seconds as u32,
            revision: self.status.revision,
        };
        if self.label_key != Some(key) {
            // HudBatch.text resolves and lays out text with temporary buffers.
            // Do that only when a label changes (at most a few times a second).
            let first = batch.vertices.len();
            let tips = self.labels(batch, atlas, &l, seconds, tip);
            self.labels.clear();
            self.labels.extend_from_slice(&batch.vertices[first..]);
            self.tip_vertices = tips.start - first..tips.end - first;
            self.label_key = Some(key);
        } else {
            let first = batch.vertices.len();
            batch.vertices.extend_from_slice(&self.labels);
            let fade = ((seconds % TIP_SECONDS) / TIP_FADE).min(1.0);
            for v in &mut batch.vertices[first + self.tip_vertices.start..first + self.tip_vertices.end] {
                v.color = faded(v.color, fade);
            }
        }
        // The bar: its track, the eased fill and a light running along it
        // while the work has no total.
        batch.fill(atlas, l.x, l.bar, l.width, l.bar_height, TRACK);
        batch.fill(atlas, l.x, l.bar, l.width * self.shown, l.bar_height, ACCENT);
        if !ready {
            let (start, width) = segment(seconds, false);
            let x = l.x + l.width * self.shown;
            let room = l.width * (1.0 - self.shown);
            batch.fill(atlas, x + room * start, l.bar, room * width, l.bar_height, GLOW);
        }
    }

    /// The text: title, step and percent, the tip, and the footer (mods,
    /// time, memory). Returns where the tip's vertices are, to fade it.
    fn labels(
        &self,
        batch: &mut HudBatch,
        atlas: &HudAtlas,
        l: &Layout,
        seconds: f32,
        tip: usize,
    ) -> std::ops::Range<usize> {
        let name = horizon_client::brand::name().to_uppercase();
        let name_width = HudBatch::text_width(atlas, &name).max(1.0);
        let title_scale = l.title_scale.min(l.width / name_width);
        let x = l.x + (l.width - name_width * title_scale) * 0.5;
        let first = batch.vertices.len();
        batch.text(atlas, &name, x, l.title, INK, false);
        batch.scale_since(first, x, l.title, title_scale, title_scale);
        let percent = format!("{}%", self.percent);
        let percent_width = HudBatch::text_width(atlas, &percent);
        let step = if self.indonesian {
            indonesian(self.status.step)
        } else {
            self.status.step
        };
        line(batch, atlas, step, l.x, l.step, (l.width - percent_width - 8.0).max(0.0), INK);
        batch.text(atlas, &percent, l.x + l.width - percent_width, l.step, ACCENT, false);
        let (id, en) = TIPS[tip];
        let text = format!("{} {}", self.say("Tip:", "Tips:"), self.say(en, id));
        let tips_start = batch.vertices.len();
        line(batch, atlas, &text, l.x, l.tip, l.width, SOFT);
        let tips = tips_start..batch.vertices.len();
        // The footer: mods on the left, time and memory on the right.
        let mut time = clock(seconds);
        if let Some(expected) = self.expected
            && self.status.done < STEPS
            && expected > seconds
        {
            time = format!(
                "{time}  ·  {} {}",
                self.say("about", "sekitar"),
                clock(expected - seconds)
            ) + self.say(" left", " lagi");
        }
        if let Some((used, _)) = self.memory {
            time = format!("{time}  ·  {} MB", used / (1024 * 1024));
        }
        let time_width = HudBatch::text_width(atlas, &time).min(l.width);
        let mods = self.status.mods.as_ref().map_or_else(
            || self.say("Tachyon: discovering mods", "Tachyon: mencari mod").to_owned(),
            |mods| mods.label(l.width - time_width - 12.0).to_owned(),
        );
        line(batch, atlas, &mods, l.x, l.footer, (l.width - time_width - 12.0).max(0.0), DIM);
        line(batch, atlas, &time, l.x + l.width - time_width, l.footer, time_width, DIM);
        tips
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
        let (cx, cy, r) = l.cog;
        let (sx, sy, sr) = small_cog(cx, cy, r);
        assert!(cx - r >= 0.0 && cy - r >= 0.0, "{l:?}");
        assert!(sx + sr <= width && sy - sr >= 0.0, "{l:?}");
        assert!(l.x >= 0.0 && l.x + l.width <= width, "{l:?}");
        assert!(l.title >= 0.0);
        assert!(l.title + 9.0 * l.title_scale < l.step, "{l:?}");
        assert!(l.step + 9.0 <= l.bar, "{l:?}");
        assert!(l.bar + l.bar_height < l.tip, "{l:?}");
        assert!(l.tip + 9.0 <= l.footer, "{l:?}");
        assert!(l.footer + 9.0 <= height, "{l:?}");
        // Above the panel, the cogs end before the title; beside it, before its left.
        assert!(cy + r <= l.title || cx + r <= l.x, "{l:?}");
    }

    #[test]
    fn landscape_phone_puts_the_cogs_beside_the_panel_inside_safe_edges() {
        let l = Layout::new(320.0, 160.0, [20.0, 8.0, 12.0, 6.0]);
        fits(&l, 308.0, 154.0);
        assert!(l.x > l.cog.0 && l.cog.0 >= 20.0);
        let notched = Layout::new(320.0, 160.0, [24.0, 30.0, 24.0, 30.0]);
        fits(&notched, 296.0, 130.0);
    }

    #[test]
    fn desktop_centers_a_bounded_panel_under_the_cogs() {
        for (w, h) in [(426.0, 240.0), (640.0, 360.0), (1280.0, 720.0), (3840.0, 2160.0)] {
            let l = Layout::new(w, h, [0.0; 4]);
            fits(&l, w, h);
            assert!(l.width <= 420.0);
            assert!((l.x + l.width * 0.5 - w * 0.5).abs() < 0.01);
        }
    }

    #[test]
    fn tips_and_steps_read_in_both_languages_and_times_are_clocks() {
        assert!(TIPS.iter().all(|(id, en)| !id.is_empty() && !en.is_empty() && id != en));
        for stage in 0..=9 {
            let step = checkpoint(stage).1;
            assert_ne!(indonesian(step), step, "{step}");
        }
        assert_ne!(indonesian(Status::default().step), Status::default().step);
        assert_eq!(clock(0.0), "0:00");
        assert_eq!(clock(83.9), "1:23");
        assert_eq!(faded(0xff12_3456, 0.5) >> 24, 0x7f);
        assert_eq!(faded(0xff12_3456, 0.5) & 0x00ff_ffff, 0x12_3456);
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
        let key = screen.label_key.unwrap();
        assert!(!screen.labels.is_empty());
        assert!(!screen.tip_vertices.is_empty());

        progress.set(6);
        batch.vertices.clear();
        screen.draw(&mut batch, &atlas, &progress);
        assert_eq!(screen.status.done, 5);
        assert_ne!(screen.label_key.unwrap().revision, key.revision);

        batch.width = 640.0;
        batch.vertices.clear();
        screen.draw(&mut batch, &atlas, &progress);
        assert_eq!(screen.label_key.unwrap().size.0, 640.0);
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
