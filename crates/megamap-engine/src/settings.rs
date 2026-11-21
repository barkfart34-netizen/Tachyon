//! Player settings, saved as `key=value` lines; unknown keys and bad values fall back to defaults so an old or
//! hand-edited file never fails to load.

use super::lang::{Lang, Tr};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    Square,
    Round,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// The minimap is drawn (which layer it shows is `Settings::layer`).
    Shown,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Corner {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
}

/// One line of the minimap's info block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Info {
    Coords,
    Biome,
    Time,
    Light,
    Facing,
    Dimension,
}

impl Info {
    pub const ALL: [Info; 6] = [Info::Coords, Info::Biome, Info::Time, Info::Light, Info::Facing, Info::Dimension];
    pub fn key(self) -> &'static str {
        match self {
            Info::Coords => "coords",
            Info::Biome => "biome",
            Info::Time => "time",
            Info::Light => "light",
            Info::Facing => "facing",
            Info::Dimension => "dimension",
        }
    }
    /// Its translation key.
    pub fn name_key(self) -> &'static str {
        match self {
            Info::Coords => "megamap.info.coords",
            Info::Biome => "megamap.info.biome",
            Info::Time => "megamap.info.time",
            Info::Light => "megamap.info.light",
            Info::Facing => "megamap.info.facing",
            Info::Dimension => "megamap.info.dimension",
        }
    }
    pub fn from_key(k: &str) -> Option<Info> {
        Info::ALL.into_iter().find(|i| i.key() == k)
    }
}

pub fn default_info() -> Vec<(Info, bool)> {
    vec![
        (Info::Coords, true),
        (Info::Biome, true),
        (Info::Time, true),
        (Info::Light, false),
        (Info::Facing, false),
        (Info::Dimension, false),
    ]
}

/// The settings file format version this build writes (`version=` line). Files without it are 0.1/0.2 ones.
pub const VERSION: u32 = 5;

#[derive(Clone, Debug, PartialEq)]
pub struct Settings {
    pub shape: Shape,
    pub north_lock: bool,
    /// Index into `ZOOMS`.
    pub zoom: usize,
    /// Index into `SIZES`.
    pub size: usize,
    pub mode: Mode,
    pub corner: Corner,
    /// Percent.
    pub opacity: u32,
    /// The minimap's info lines, in display order, each on or off.
    pub info: Vec<(Info, bool)>,
    pub beams: bool,
    pub radar: bool,
    pub trail: bool,
    pub fog: bool,
    pub max_deaths: usize,
    /// Relief strength 0..=3 (`view::Style::relief`).
    pub relief: u8,
    /// Contour interval in blocks, one of `CONTOURS` (0 = off).
    pub contours: u8,
    /// Grid bits: 1 chunks, 2 regions.
    pub grid: u8,
    /// The map darkens towards a moonlit blue at night.
    pub day_night: bool,
    /// Far terrain (Horizon's level of detail data and its disk cache) past the live chunks.
    pub far: bool,
    /// Exploration fog drawn as animated clouds (else the flat hatch).
    pub clouds: bool,
    /// Percent.
    pub cloud_density: u32,
    /// Percent of the normal drift speed (0 = frozen).
    pub cloud_speed: u32,
    /// Percent.
    pub cloud_puff: u32,
    /// `clouds::Theme` index: 0 auto (time of day + weather), then the fixed themes.
    pub cloud_theme: u8,
    /// Clouds thicken, darken and hurry in rain and storms.
    pub cloud_weather: bool,
    /// 0 fast, 1 normal, 2 fine, 3 volumetric.
    pub cloud_quality: u8,
    /// Cloud animation frames per second, one of `CLOUD_FPS` (0 = every map frame).
    pub cloud_fps: u32,
    /// Cloud look: 0 follows the viewed map's dimension, 1 overworld, 2 nether, 3 end.
    pub cloud_dim: u8,
    /// Percent, one of `INTENSITIES`.
    pub nether_intensity: u32,
    pub end_intensity: u32,
    /// Overworld storms: 0 off, 1 sheet flashes, 2 flashes with bolts.
    pub lightning: u8,
    /// Percent of the normal strike rate, one of `LIGHTNING_RATES`.
    pub lightning_rate: u32,
    /// Light shafts from the sun or moon through cloud gaps (Volumetric quality).
    pub god_rays: bool,
    /// World map: the hard limit (in blocks, 0..`REACH_MAX`) of how far the clouds reach in over explored ground;
    /// the feather fits inside it (0 = only the unexplored).
    pub cloud_reach: u32,
    /// The same for the minimap.
    pub mini_reach: u32,
    /// The reach is set in chunks: the slider steps by 16 blocks and the limit rounds down to whole chunks (never
    /// past the value in blocks).
    pub reach_chunks: bool,
    /// Background cave-layer bake of the far terrain: 0 off, else the CPU share in percent it may use.
    /// Far terrain readers (threads reading stored chunks at once); 0 = auto (a quarter of the cores, up to 3).
    pub far_threads: u32,
    /// Minimap redraws per second, one of `FPS_CAPS` (0 = with the game).
    pub mini_fps: u32,
    /// World map redraws per second, one of `FPS_CAPS` (0 = with the game).
    pub map_fps: u32,
    /// UI theme: 0 auto (follows the time of day), 1 dark, 2 light.
    pub ui_theme: u8,
    /// Compass letters around the minimap.
    pub compass: bool,
    /// The world map fades and zooms in when opened.
    pub animations: bool,
    /// Minimap auto-zoom (`autozoom`): the scale follows how far the known map data reaches.
    pub autozoom: bool,
    /// Auto-zoom limits as `ZOOMS` indices: the closest (fewest blocks per pixel) and the farthest it may go.
    pub az_near: usize,
    pub az_far: usize,
    /// Entity drawing per category (`entities::Category::ALL` order): 0 dot / shape, 1 face icon, 2 3D sprite.
    pub ent_mode: [u8; 4],
    /// Frame around face icons and sprites in quarter GUI px, one of `FRAME_STEPS` (0 = none).
    pub face_frame: u8,
    /// Shown layer, a `layers::LayerMode` index: auto, surface, cave, fixed Y.
    pub layer: u8,
    /// The slice top for the fixed-Y layer, one of `FIXED_YS`.
    pub fixed_y: i32,
    /// Depth of the cave slice in blocks below its top, one of `CAVE_DEPTHS`.
    pub cave_depth: u32,
    /// When auto calls it surface (`layers::RULES`): 0 sky or height, 1 sky light only, 2 height only.
    pub surface_rule: u8,
    /// The interface (panels, frame, theme, tint) ignores time of day and weather.
    pub ui_fixed: bool,
    /// Block colours: 0 vanilla, 1 from the resource packs, 2 auto (packs when any are on).
    pub color_style: u8,
    /// Language of the map's own words: 0 as the game, 1 English, 2 Russian.
    pub lang: u8,
    /// Auto layer roof check: side of the square of columns above the player that must be roofed (0 = sky light and
    /// height only), one of `ROOF_SIZES`.
    pub roof_size: u8,
    /// How far above the head the roof check looks, blocks.
    pub roof_reach: u32,
    /// How long a new place must hold before auto switches the layer, tenths of a second.
    pub layer_delay: u8,
    /// The auto cave slice follows the player in steps of this many blocks.
    pub cave_step: u32,
    /// Indoors (`layers::INDOOR_MODES`): 0 off, 1 cut the ceiling, 2 and 3 also zoom the minimap in that many times.
    pub indoor: u8,
    /// Smooth sampling: the map glides under a moving or turning player (sharp-bilinear texels).
    pub smooth: bool,
    /// Block textures on a closely zoomed map.
    pub textures: bool,
    /// Light sources (torches, lava, glowstone) glow on the map, most at night.
    pub block_light: bool,
    /// How fast newly explored ground comes out of the fog behind the front (`reveal::speed`): 0 at once, 1 slow,
    /// 2 normal, 3 fast.
    pub reveal: u8,
    /// The cloud fog also covers far (LOD, estimated) terrain, as up to 0.3.4; off (default): far terrain shows
    /// through, the fog stays on ground with no terrain at all (0.3.5).
    pub far_fog: bool,
    /// The layer badge on the minimap and the map (`BADGE_MODES`): 0 an icon, 1 a short text, 2 none.
    pub badge: u8,
    /// The hostile warning under the minimap (`HOSTILE_MODES`): 0 a skull and "2 · 21 m", 1 the same as words, 2 none.
    pub hostile: u8,
    /// The info block's alignment under the minimap (`INFO_ALIGNS`): 0 centred, 1 left, 2 right (0.3.5).
    pub info_align: u8,
    /// Last opened settings category (global index into `CATEGORIES`).
    pub ui_cat: u8,
    /// Per-kind entity looks that differ from `entities::default_style`.
    pub entities: std::collections::BTreeMap<String, super::entities::Style>,
}

pub const CONTOURS: [u8; 4] = [0, 8, 16, 32];
pub const CLOUD_DENSITIES: [u32; 4] = [85, 100, 70, 55];
pub const CLOUD_SPEEDS: [u32; 4] = [100, 200, 50, 0];
pub const CLOUD_PUFFS: [u32; 3] = [60, 90, 30];
/// Default cloud reach over explored ground, blocks.
pub const REACH_DEFAULT: u32 = super::clouds::REACH_DEFAULT as u32;
pub const REACH_MAX: u32 = super::clouds::REACH_MAX;
/// What a click on a reach item steps through (a slider click sets any value); the first one is the default.
pub const REACH_CLICKS: [u32; 8] = [REACH_DEFAULT, 16, 24, 32, 48, 64, 0, 4];
/// 0.3.2 and earlier: the reach in chunks (one of these) was a feather width, at least 5 % of the image.
pub const OLD_CLOUD_REACHES: [u32; 5] = [3, 4, 6, 1, 2];
pub const REACH_UNIT: usize = 60;
pub const MINI_REACH: usize = 61;
pub const MAP_REACH: usize = 42;

/// A 0.3.2 reach (chunks of feather width) in blocks of the 0.3.3 hard limit: the old default 3 becomes the new
/// default, others in proportion (1 -> 3, 2 -> 7, 4 -> 13, 6 -> 20 blocks). The old one was pixel-floored (it
/// reached 50+ px into explored ground at any zoom), which is what the new limit is for.
pub fn migrate_reach(old_chunks: u32) -> u32 {
    ((old_chunks as f32 * REACH_DEFAULT as f32 / 3.0).round() as u32).min(REACH_MAX)
}
pub const FAR_THREADS: [u32; 4] = [0, 1, 2, 3];
pub const CLOUD_FPS: [u32; 6] = [0, 60, 30, 20, 15, 10];
pub const FPS_CAPS: [u32; 6] = [0, 144, 120, 60, 30, 20];
pub const INTENSITIES: [u32; 4] = [100, 150, 70, 40];
pub const LIGHTNING_RATES: [u32; 3] = [100, 250, 40];
/// Number of settings buttons (`buttons().len()`).
pub const BUTTON_COUNT: usize = 75;
/// Auto layer (0.3.4): the roof check's square side in columns (0 off), how high it looks, the switch delay in
/// tenths of a second and the cave slice step in blocks.
pub const ROOF_SIZE: usize = 62;
pub const ROOF_REACH: usize = 63;
pub const LAYER_DELAY: usize = 64;
pub const CAVE_STEP: usize = 65;
/// Indoors: cut the ceiling, zoom the minimap in (0.3.4).
pub const INDOOR: usize = 66;
/// Rendering (0.3.4): smooth sampling, block textures, block light.
pub const SMOOTH: usize = 67;
pub const TEXTURES: usize = 68;
pub const BLOCK_LIGHT: usize = 69;
/// The fog front's speed (0.3.5).
pub const REVEAL: usize = 70;
/// What a click on the fog front steps through: normal, fast, at once, slow.
pub const REVEAL_MODES: [u8; 4] = [2, 3, 0, 1];
/// Fog over far (LOD) terrain (0.3.5).
pub const FAR_FOG: usize = 71;
/// The layer badge: icon / text / none (0.3.5).
pub const BADGE: usize = 72;
pub const BADGE_MODES: [u8; 3] = [0, 1, 2];
/// The hostile warning: icon + number / text / off (0.3.5).
pub const HOSTILE: usize = 73;
pub const HOSTILE_MODES: [u8; 3] = [0, 1, 2];
/// The info block's alignment: centre / left / right (0.3.5).
pub const INFO_ALIGN: usize = 74;
pub const INFO_ALIGNS: [u8; 3] = [0, 1, 2];
pub const ROOF_SIZES: [u8; 4] = [3, 5, 0, 1];
pub const ROOF_REACHES: [u32; 4] = [24, 48, 64, 12];
pub const LAYER_DELAYS: [u8; 4] = [10, 20, 30, 5];
pub const CAVE_STEPS: [u32; 3] = [8, 16, 4];
/// "Reset all to defaults" (kept at its 0.3.0 index; the 0.3.1 buttons come after it).
pub const RESET_ALL: usize = 44;
pub const AUTOZOOM_FIRST: usize = 45;
pub const ENT_MODE_FIRST: usize = 48;
pub const FACE_FRAME: usize = 52;
/// Items drawn as sliders (`Settings::slider`): auto-zoom near and far, the icon frame.
pub const SLIDERS: [usize; 5] = [46, 47, FACE_FRAME, MAP_REACH, MINI_REACH];

/// The slider step a click at fraction `f` (0 left .. 1 right) of the track picks.
pub fn slider_step(f: f32, steps: usize) -> usize {
    if steps <= 1 {
        return 0;
    }
    ((f.clamp(0.0, 1.0) * (steps - 1) as f32).round() as usize).min(steps - 1)
}
pub const LAYER: usize = 53;
pub const FIXED_Y: usize = 54;
pub const CAVE_DEPTH: usize = 55;
pub const SURFACE_RULE: usize = 56;
pub const UI_FIXED: usize = 57;
pub const COLOR_STYLE: usize = 58;
pub const LANG: usize = 59;
/// Frame widths in quarter px: quarter steps to 1 px, then half steps to 3 px.
pub const FRAME_STEPS: [u8; 9] = [0, 1, 2, 3, 4, 6, 8, 10, 12];
pub const FIXED_YS: [i32; 8] = [64, 32, 0, -32, -56, 96, 128, 192];
pub const CAVE_DEPTHS: [u32; 5] = [16, 24, 32, 48, 8];
/// First flat button index of the info-line toggles (one per `info` position) and of the map tab.
pub const INFO_FIRST: usize = 13;
pub const MAP_TAB_FIRST: usize = 19;
pub const TAB_NAMES: [&str; 2] = ["megamap.tab.minimap", "megamap.tab.map"];

/// Blocks per minimap pixel.
pub const ZOOMS: [f64; 5] = [0.5, 1.0, 2.0, 4.0, 8.0];
/// Minimap side in GUI pixels.
pub const SIZES: [u32; 4] = [64, 96, 128, 160];
pub const OPACITIES: [u32; 4] = [100, 85, 70, 50];

impl Default for Settings {
    fn default() -> Self {
        Settings {
            shape: Shape::Square,
            north_lock: true,
            zoom: 1,
            size: 1,
            mode: Mode::Shown,
            corner: Corner::TopRight,
            opacity: 100,
            info: default_info(),
            beams: true,
            radar: true,
            trail: true,
            fog: true,
            max_deaths: 5,
            relief: 2,
            contours: 0,
            grid: 0,
            day_night: true,
            far: true,
            clouds: true,
            cloud_density: 85,
            cloud_speed: 100,
            cloud_puff: 60,
            cloud_theme: 0,
            cloud_weather: true,
            cloud_quality: if cfg!(target_os = "android") { 0 } else { 1 },
            cloud_fps: if cfg!(target_os = "android") { 20 } else { 0 },
            cloud_dim: 0,
            nether_intensity: 100,
            end_intensity: 100,
            lightning: 2,
            lightning_rate: 100,
            god_rays: true,
            cloud_reach: REACH_DEFAULT,
            mini_reach: REACH_DEFAULT,
            reach_chunks: false,
            far_threads: if cfg!(target_os = "android") { 1 } else { 0 },
            mini_fps: if cfg!(target_os = "android") { 30 } else { 0 },
            map_fps: 0,
            ui_theme: 0,
            compass: true,
            animations: true,
            autozoom: true,
            az_near: 0,
            az_far: 3,
            // Hostile, passive: face icons; players: face; other: dot.
            ent_mode: [1, 1, 1, 0],
            face_frame: 0,
            layer: 0,
            fixed_y: 64,
            cave_depth: 16,
            surface_rule: 0,
            ui_fixed: true,
            color_style: 2,
            lang: 0,
            roof_size: 3,
            roof_reach: 24,
            layer_delay: 10,
            cave_step: 8,
            indoor: 2,
            smooth: true,
            textures: true,
            block_light: true,
            reveal: 2,
            far_fog: false,
            badge: 0,
            hostile: 0,
            info_align: 0,
            ui_cat: 0,
            entities: Default::default(),
        }
    }
}

fn b(v: bool) -> &'static str {
    if v { "on" } else { "off" }
}

fn fps(v: u32) -> String {
    if v == 0 { "as game".into() } else { format!("{v}") }
}

fn next<T: Copy + PartialEq>(list: &[T], v: T) -> T {
    let i = list.iter().position(|&x| x == v).map_or(0, |p| (p + 1) % list.len());
    list[i]
}

fn pick<T: Copy + PartialEq>(list: &[T], v: T, d: T) -> T {
    if list.contains(&v) { v } else { d }
}

impl Settings {
    /// Whether an info line is shown.
    pub fn shows(&self, i: Info) -> bool {
        self.info.iter().any(|&(k, on)| k == i && on)
    }

    /// Moves the info line at `pos` one place up (`up`) or down; returns whether it moved.
    pub fn info_move(&mut self, pos: usize, up: bool) -> bool {
        let to = if up { pos.checked_sub(1) } else { Some(pos + 1) };
        match to {
            Some(t) if pos < self.info.len() && t < self.info.len() => {
                self.info.swap(pos, t);
                true
            }
            _ => false,
        }
    }

    /// The entity look for `kind`: the override, else the default.
    pub fn entity_style(&self, kind: &str) -> super::entities::Style {
        self.entities.get(kind).copied().unwrap_or_else(|| super::entities::default_style(kind))
    }

    /// Sets a look; one equal to the default drops the override so the file stays short.
    pub fn set_entity_style(&mut self, kind: &str, st: super::entities::Style) {
        if st == super::entities::default_style(kind) {
            self.entities.remove(kind);
        } else {
            self.entities.insert(kind.to_string(), st);
        }
    }

    /// The flat button indices of a settings tab (0 minimap, 1 map); the reset button is in both.
    pub fn tab_items(tab: usize) -> Vec<usize> {
        let mut v: Vec<usize> = CATEGORIES.iter().filter(|c| c.tab == tab).flat_map(|c| c.items()).collect();
        let mut seen = [false; BUTTON_COUNT];
        v.retain(|&i| !std::mem::replace(&mut seen[i], true));
        v
    }

    /// Item `i` back to its default value (cycling buttons are clicked round until the label matches).
    pub fn reset_item(&mut self, i: usize) {
        let d = Settings::default();
        if (INFO_FIRST..INFO_FIRST + 6).contains(&i) {
            self.info = d.info.clone();
            return;
        }
        if i == RESET_ALL {
            return;
        }
        match i {
            MAP_REACH => return self.cloud_reach = d.cloud_reach,
            MINI_REACH => return self.mini_reach = d.mini_reach,
            REACH_UNIT => return self.reach_chunks = d.reach_chunks,
            _ => {}
        }
        let want = d.buttons()[i].clone();
        for _ in 0..16 {
            if self.buttons()[i] == want {
                break;
            }
            self.click(i);
        }
    }

    /// Resets every item of a category.
    pub fn reset_category(&mut self, cat: usize) {
        if let Some(c) = CATEGORIES.get(cat) {
            for i in c.items() {
                self.reset_item(i);
            }
        }
    }

    /// Items of a category whose value differs from the default.
    pub fn changed_in(&self, cat: usize, labels: &[String], defaults: &[String]) -> usize {
        CATEGORIES.get(cat).map_or(0, |c| c.items().filter(|&i| i != RESET_ALL && labels[i] != defaults[i]).count())
    }

    pub fn zoom_bpp(&self) -> f64 {
        ZOOMS[self.zoom.min(ZOOMS.len() - 1)]
    }
    pub fn size_px(&self) -> u32 {
        SIZES[self.size.min(SIZES.len() - 1)]
    }
    pub fn cycle_zoom(&mut self) {
        self.zoom = (self.zoom + 1) % ZOOMS.len();
    }
    pub fn cycle_size(&mut self) {
        self.size = (self.size + 1) % SIZES.len();
    }
    pub fn cycle_mode(&mut self) {
        self.mode = match self.mode {
            Mode::Shown => Mode::Hidden,
            Mode::Hidden => Mode::Shown,
        };
    }
    pub fn layer_mode(&self) -> super::layers::LayerMode {
        super::layers::LayerMode::from_index(self.layer)
    }
    pub fn cycle_layer(&mut self) {
        self.layer = self.layer_mode().next().index();
    }
    /// Frame width in GUI px.
    pub fn frame_px(&self) -> f32 {
        self.face_frame as f32 / 4.0
    }

    /// Items drawn as sliders: `(position, steps)`.
    pub fn slider(&self, i: usize) -> Option<(usize, usize)> {
        match i {
            46 => Some((self.az_near.min(ZOOMS.len() - 1), ZOOMS.len())),
            47 => Some((self.az_far.min(ZOOMS.len() - 1), ZOOMS.len())),
            FACE_FRAME => Some((FRAME_STEPS.iter().position(|&f| f == self.face_frame).unwrap_or(0), FRAME_STEPS.len())),
            MAP_REACH | MINI_REACH => {
                let v = if i == MAP_REACH { self.cloud_reach } else { self.mini_reach };
                Some(if self.reach_chunks { ((v / 16) as usize, (REACH_MAX / 16 + 1) as usize) } else { (v as usize, REACH_MAX as usize + 1) })
            }
            _ => None,
        }
    }

    /// Sets a slider to step `pos` (clamped); the auto-zoom pair keeps near <= far.
    pub fn set_slider(&mut self, i: usize, pos: usize) {
        match i {
            46 => {
                self.az_near = pos.min(ZOOMS.len() - 1);
                self.az_far = self.az_far.max(self.az_near);
            }
            47 => {
                self.az_far = pos.min(ZOOMS.len() - 1);
                self.az_near = self.az_near.min(self.az_far);
            }
            FACE_FRAME => self.face_frame = FRAME_STEPS[pos.min(FRAME_STEPS.len() - 1)],
            MAP_REACH | MINI_REACH => {
                let v = if self.reach_chunks { pos as u32 * 16 } else { pos as u32 }.min(REACH_MAX);
                *if i == MAP_REACH { &mut self.cloud_reach } else { &mut self.mini_reach } = v;
            }
            _ => {}
        }
    }

    /// The hard cloud reach in blocks the engine gets: the value, rounded down to whole chunks in chunk mode.
    pub fn reach_blocks(&self, minimap: bool) -> u32 {
        let v = if minimap { self.mini_reach } else { self.cloud_reach }.min(REACH_MAX);
        if self.reach_chunks { v / 16 * 16 } else { v }
    }

    pub fn cycle_corner(&mut self) {
        self.corner = match self.corner {
            Corner::TopRight => Corner::TopLeft,
            Corner::TopLeft => Corner::BottomLeft,
            Corner::BottomLeft => Corner::BottomRight,
            Corner::BottomRight => Corner::TopRight,
        };
    }
    pub fn cycle_opacity(&mut self) {
        let i = OPACITIES.iter().position(|&o| o == self.opacity).unwrap_or(0);
        self.opacity = OPACITIES[(i + 1) % OPACITIES.len()];
    }

    fn reach_value(&self, tr: &Tr, minimap: bool) -> String {
        let v = self.reach_blocks(minimap);
        if self.reach_chunks { tr.f("megamap.v.chunks_blocks", &[&(v / 16), &v]) } else { tr.f("megamap.v.blocks", &[&v]) }
    }

    /// Item `i` as `(name, value)` in `tr`'s language; the value may be empty (a plain action button).
    pub fn item(&self, i: usize, tr: &Tr) -> (String, String) {
        let on = |v: bool| tr.t(if v { "megamap.on" } else { "megamap.off" });
        let fps = |v: u32| if v == 0 { tr.t("megamap.v.as_game") } else { v.to_string() };
        let pct = |v: u32| format!("{v}%");
        let zx = |z: usize| format!("{}x", 1.0 / ZOOMS[z.min(ZOOMS.len() - 1)]);
        let k = |name: &str| tr.t(&format!("megamap.set.{name}"));
        let named = |name: &str, v: String| (k(name), v);
        match i {
            0 => named("shape", tr.t(if self.shape == Shape::Round { "megamap.v.round" } else { "megamap.v.square" })),
            1 => named("north_lock", on(self.north_lock)),
            2 => named("size", self.size_px().to_string()),
            3 => named("zoom", format!("{}x", 1.0 / self.zoom_bpp())),
            4 => named("mode", tr.t(if self.mode == Mode::Hidden { "megamap.v.hidden" } else { "megamap.v.shown" })),
            5 => named("corner", tr.t(match self.corner {
                Corner::TopLeft => "megamap.corner.top_left",
                Corner::TopRight => "megamap.corner.top_right",
                Corner::BottomLeft => "megamap.corner.bottom_left",
                Corner::BottomRight => "megamap.corner.bottom_right",
            })),
            6 => named("opacity", pct(self.opacity)),
            7 => named("compass", on(self.compass)),
            8 => named("mini_fps", fps(self.mini_fps)),
            9 => named("radar", on(self.radar)),
            10 => named("beams", on(self.beams)),
            11 => named("trail", on(self.trail)),
            12 => named("deaths", self.max_deaths.to_string()),
            13..=18 => match self.info.get(i - INFO_FIRST) {
                Some(&(inf, v)) => (tr.f("megamap.set.info", &[&(i - INFO_FIRST + 1), &tr.t(inf.name_key())]), on(v)),
                None => (tr.f("megamap.set.info", &[&(i - INFO_FIRST + 1), &"-"]), on(false)),
            },
            19 => named("map_fps", fps(self.map_fps)),
            20 => named("fog", on(self.fog)),
            21 => named("relief", tr.t(&format!("megamap.relief.{}", self.relief.min(3)))),
            22 => named("contours", if self.contours == 0 { on(false) } else { tr.f("megamap.v.every", &[&self.contours]) }),
            23 => named("grid", tr.t(&format!("megamap.grid.{}", self.grid.min(3)))),
            24 => named("day_night", on(self.day_night)),
            25 => named("far", on(self.far)),
            26 => named("clouds", on(self.clouds)),
            27 => named("cloud_density", pct(self.cloud_density)),
            28 => named("cloud_speed", pct(self.cloud_speed)),
            29 => named("cloud_puff", pct(self.cloud_puff)),
            30 => named("cloud_theme", tr.t(&format!("megamap.theme.{}", super::clouds::Theme::from_index(self.cloud_theme).name()))),
            31 => named("cloud_weather", on(self.cloud_weather)),
            32 => named("cloud_quality", tr.t(&format!("megamap.quality.{}", self.cloud_quality.min(3)))),
            33 => named("cloud_fps", fps(self.cloud_fps)),
            34 => named("cloud_dim", tr.t(&format!("megamap.cloud_dim.{}", self.cloud_dim.min(3)))),
            35 => named("nether", pct(self.nether_intensity)),
            36 => named("end", pct(self.end_intensity)),
            37 => named("lightning", tr.t(&format!("megamap.lightning.{}", self.lightning.min(2)))),
            38 => named("lightning_rate", pct(self.lightning_rate)),
            39 => named("god_rays", on(self.god_rays)),
            40 => named("ui_theme", tr.t(&format!("megamap.ui_theme.{}", self.ui_theme.min(2)))),
            41 => named("animations", on(self.animations)),
            MAP_REACH => named("map_reach", self.reach_value(tr, false)),
            43 => named("far_threads", if self.far_threads == 0 { tr.t("megamap.v.auto") } else { self.far_threads.to_string() }),
            RESET_ALL => named("reset_all", String::new()),
            45 => named("autozoom", on(self.autozoom)),
            46 => named("az_near", zx(self.az_near)),
            47 => named("az_far", zx(self.az_far)),
            48..=51 => {
                let c = super::entities::Category::ALL[i - ENT_MODE_FIRST];
                (tr.t(c.name_key()), tr.t(&format!("megamap.ent_mode.{}", self.ent_mode[i - ENT_MODE_FIRST].min(2))))
            }
            FACE_FRAME => named("face_frame", if self.face_frame == 0 { tr.t("megamap.v.none") } else { tr.f("megamap.v.px", &[&self.frame_px()]) }),
            LAYER => named("layer", tr.t(&format!("megamap.layer.{}", self.layer_mode().key()))),
            FIXED_Y => named("fixed_y", tr.f("megamap.v.y", &[&self.fixed_y])),
            CAVE_DEPTH => named("cave_depth", tr.f("megamap.v.blocks", &[&self.cave_depth])),
            SURFACE_RULE => named("surface_rule", tr.t(&format!("megamap.surface_rule.{}", self.surface_rule.min(2)))),
            UI_FIXED => named("ui_fixed", on(self.ui_fixed)),
            COLOR_STYLE => named("color_style", tr.t(["megamap.color.vanilla", "megamap.color.pack", "megamap.color.auto"][(self.color_style as usize).min(2)])),
            LANG => named("lang", tr.t(super::lang::LANGS[(self.lang as usize).min(2)])),
            REACH_UNIT => named("reach_unit", tr.t(if self.reach_chunks { "megamap.v.unit_chunks" } else { "megamap.v.unit_blocks" })),
            MINI_REACH => named("mini_reach", self.reach_value(tr, true)),
            ROOF_SIZE => named("roof_size", if self.roof_size == 0 { on(false) } else { tr.f("megamap.v.roof", &[&self.roof_size]) }),
            ROOF_REACH => named("roof_reach", tr.f("megamap.v.blocks", &[&self.roof_reach])),
            LAYER_DELAY => named("layer_delay", tr.f("megamap.v.seconds", &[&(self.layer_delay as f32 / 10.0)])),
            CAVE_STEP => named("cave_step", tr.f("megamap.v.blocks", &[&self.cave_step])),
            INDOOR => named("indoor", match self.indoor {
                0 => on(false),
                1 => tr.t("megamap.v.indoor_cut"),
                z => tr.f("megamap.v.indoor_zoom", &[&z]),
            }),
            SMOOTH => named("smooth", on(self.smooth)),
            TEXTURES => named("textures", on(self.textures)),
            BLOCK_LIGHT => named("block_light", on(self.block_light)),
            REVEAL => named("reveal", tr.t(&format!("megamap.v.reveal_{}", self.reveal.min(3)))),
            FAR_FOG => named("far_fog", on(self.far_fog)),
            BADGE => named("badge", tr.t(&format!("megamap.v.badge_{}", self.badge.min(2)))),
            HOSTILE => named("hostile", tr.t(&format!("megamap.v.hostile_{}", self.hostile.min(2)))),
            INFO_ALIGN => named("info_align", tr.t(&format!("megamap.v.info_align_{}", self.info_align.min(2)))),
            _ => (String::new(), String::new()),
        }
    }

    /// The settings items as "name: value" labels in `tr`'s language, in order; `click` acts on the one at the same
    /// index.
    pub fn buttons_in(&self, tr: &Tr) -> Vec<String> {
        (0..BUTTON_COUNT)
            .map(|i| {
                let (n, v) = self.item(i, tr);
                if v.is_empty() { n } else { format!("{n}: {v}") }
            })
            .collect()
    }

    /// The labels in English: the canonical form `reset_item` and `changed_in` compare.
    pub fn buttons(&self) -> Vec<String> {
        self.buttons_in(&Tr::new(Lang::En))
    }

    pub fn click(&mut self, i: usize) {
        match i {
            0 => self.shape = if self.shape == Shape::Round { Shape::Square } else { Shape::Round },
            1 => self.north_lock = !self.north_lock,
            2 => self.cycle_size(),
            3 => self.cycle_zoom(),
            4 => self.cycle_mode(),
            5 => self.cycle_corner(),
            6 => self.cycle_opacity(),
            7 => self.compass = !self.compass,
            8 => self.mini_fps = next(&FPS_CAPS, self.mini_fps),
            9 => self.radar = !self.radar,
            10 => self.beams = !self.beams,
            11 => self.trail = !self.trail,
            12 => self.max_deaths = [1, 3, 5, 10, 0][[1, 3, 5, 10, 0].iter().position(|&n| n == self.max_deaths).map_or(0, |p| (p + 1) % 5)],
            13..=18 => {
                if let Some(e) = self.info.get_mut(i - INFO_FIRST) {
                    e.1 = !e.1;
                }
            }
            19 => self.map_fps = next(&FPS_CAPS, self.map_fps),
            20 => self.fog = !self.fog,
            21 => self.relief = (self.relief + 1) % 4,
            22 => self.contours = next(&CONTOURS, self.contours),
            23 => self.grid = (self.grid + 1) % 4,
            24 => self.day_night = !self.day_night,
            25 => self.far = !self.far,
            26 => self.clouds = !self.clouds,
            27 => self.cloud_density = next(&CLOUD_DENSITIES, self.cloud_density),
            28 => self.cloud_speed = next(&CLOUD_SPEEDS, self.cloud_speed),
            29 => self.cloud_puff = next(&CLOUD_PUFFS, self.cloud_puff),
            30 => self.cloud_theme = (self.cloud_theme + 1) % super::clouds::Theme::ALL.len() as u8,
            31 => self.cloud_weather = !self.cloud_weather,
            32 => self.cloud_quality = (self.cloud_quality + 1) % 4,
            33 => self.cloud_fps = next(&CLOUD_FPS, self.cloud_fps),
            34 => self.cloud_dim = (self.cloud_dim + 1) % 4,
            35 => self.nether_intensity = next(&INTENSITIES, self.nether_intensity),
            36 => self.end_intensity = next(&INTENSITIES, self.end_intensity),
            37 => self.lightning = (self.lightning + 1) % 3,
            38 => self.lightning_rate = next(&LIGHTNING_RATES, self.lightning_rate),
            39 => self.god_rays = !self.god_rays,
            40 => self.ui_theme = (self.ui_theme + 1) % 3,
            41 => self.animations = !self.animations,
            42 => self.cloud_reach = self.click_reach(self.cloud_reach),
            43 => self.far_threads = next(&FAR_THREADS, self.far_threads),
            44 => *self = Settings::default(),
            45 => self.autozoom = !self.autozoom,
            46 => self.az_near = (self.az_near + 1) % ZOOMS.len().min(self.az_far + 1).max(1),
            47 => {
                self.az_far = (self.az_far + 1) % ZOOMS.len();
                if self.az_far < self.az_near {
                    self.az_far = self.az_near;
                }
            }
            48..=51 => self.ent_mode[i - ENT_MODE_FIRST] = (self.ent_mode[i - ENT_MODE_FIRST] + 1) % 3,
            52 => self.face_frame = next(&FRAME_STEPS, self.face_frame),
            53 => self.cycle_layer(),
            54 => self.fixed_y = next(&FIXED_YS, self.fixed_y),
            55 => self.cave_depth = next(&CAVE_DEPTHS, self.cave_depth),
            56 => self.surface_rule = (self.surface_rule + 1) % super::layers::RULES as u8,
            57 => self.ui_fixed = !self.ui_fixed,
            58 => self.color_style = (self.color_style + 1) % 3,
            59 => self.lang = (self.lang + 1) % 3,
            REACH_UNIT => self.reach_chunks = !self.reach_chunks,
            MINI_REACH => self.mini_reach = self.click_reach(self.mini_reach),
            ROOF_SIZE => self.roof_size = next(&ROOF_SIZES, self.roof_size),
            ROOF_REACH => self.roof_reach = next(&ROOF_REACHES, self.roof_reach),
            LAYER_DELAY => self.layer_delay = next(&LAYER_DELAYS, self.layer_delay),
            CAVE_STEP => self.cave_step = next(&CAVE_STEPS, self.cave_step),
            INDOOR => self.indoor = next(&super::layers::INDOOR_MODES, self.indoor),
            SMOOTH => self.smooth = !self.smooth,
            TEXTURES => self.textures = !self.textures,
            BLOCK_LIGHT => self.block_light = !self.block_light,
            REVEAL => self.reveal = next(&REVEAL_MODES, self.reveal),
            FAR_FOG => self.far_fog = !self.far_fog,
            BADGE => self.badge = next(&BADGE_MODES, self.badge),
            HOSTILE => self.hostile = next(&HOSTILE_MODES, self.hostile),
            INFO_ALIGN => self.info_align = next(&INFO_ALIGNS, self.info_align),
            _ => {}
        }
    }

    fn click_reach(&self, v: u32) -> u32 {
        if self.reach_chunks {
            (v / 16 + 1) % (REACH_MAX / 16 + 1) * 16
        } else {
            next(&REACH_CLICKS, v)
        }
    }

    pub fn to_text(&self) -> String {
        let mut t = format!(
            "version={VERSION}\nshape={}\nnorth_lock={}\nzoom={}\nsize={}\nmode={}\ncorner={}\nopacity={}\nbeams={}\nradar={}\ntrail={}\nfog={}\nmax_deaths={}\n",
            if self.shape == Shape::Round { "round" } else { "square" },
            self.north_lock,
            self.zoom,
            self.size,
            match self.mode { Mode::Shown => "shown", Mode::Hidden => "hidden" },
            match self.corner {
                Corner::TopLeft => "top_left",
                Corner::TopRight => "top_right",
                Corner::BottomLeft => "bottom_left",
                Corner::BottomRight => "bottom_right",
            },
            self.opacity,
            self.beams,
            self.radar,
            self.trail,
            self.fog,
            self.max_deaths,
        );
        let info: Vec<String> = self.info.iter().map(|(k, on)| format!("{}:{}", k.key(), b(*on))).collect();
        t += &format!("info={}\n", info.join(","));
        t += &format!(
            "relief={}\ncontours={}\ngrid={}\nday_night={}\nfar={}\nclouds={}\ncloud_density={}\ncloud_speed={}\ncloud_puff={}\ncloud_theme={}\ncloud_weather={}\ncloud_quality={}\ncloud_fps={}\n",
            self.relief,
            self.contours,
            self.grid,
            self.day_night,
            self.far,
            self.clouds,
            self.cloud_density,
            self.cloud_speed,
            self.cloud_puff,
            self.cloud_theme,
            self.cloud_weather,
            self.cloud_quality,
            self.cloud_fps,
        );
        t += &format!(
            "cloud_dim={}\nnether_intensity={}\nend_intensity={}\nlightning={}\nlightning_rate={}\ngod_rays={}\nmini_fps={}\nmap_fps={}\nui_theme={}\ncompass={}\nanimations={}\ncloud_reach_blocks={}\nfar_threads={}\nmini_reach_blocks={}\nreach_chunks={}\n",
            self.cloud_dim,
            self.nether_intensity,
            self.end_intensity,
            self.lightning,
            self.lightning_rate,
            self.god_rays,
            self.mini_fps,
            self.map_fps,
            self.ui_theme,
            self.compass,
            self.animations,
            self.cloud_reach,
            self.far_threads,
            self.mini_reach,
            self.reach_chunks,
        );
        t += &format!(
            "autozoom={}\naz_near={}\naz_far={}\nent_mode={}\nface_frame={}\nui_cat={}\n",
            self.autozoom,
            self.az_near,
            self.az_far,
            self.ent_mode.iter().map(|m| m.to_string()).collect::<Vec<_>>().join(","),
            self.face_frame,
            self.ui_cat,
        );
        t += &format!(
            "layer={}\nfixed_y={}\ncave_depth={}\nsurface_rule={}\nui_fixed={}\ncolor_style={}\nlang={}\nroof_size={}\nroof_reach={}\nlayer_delay={}\ncave_step={}\nindoor={}\nsmooth={}\ntextures={}\nblock_light={}\nreveal={}\nfar_fog={}\nbadge={}\nhostile={}\ninfo_align={}\n",
            self.layer_mode().key(),
            self.fixed_y,
            self.cave_depth,
            self.surface_rule,
            self.ui_fixed,
            self.color_style,
            self.lang,
            self.roof_size,
            self.roof_reach,
            self.layer_delay,
            self.cave_step,
            self.indoor,
            self.smooth,
            self.textures,
            self.block_light,
            self.reveal,
            self.far_fog,
            self.badge,
            self.hostile,
            self.info_align,
        );
        for (k, st) in &self.entities {
            t += &format!("ent.{k}={}\n", st.to_text());
        }
        t
    }

    /// The map style these settings ask for, at global light `light` (256 = day).
    pub fn style(&self, light: u32, mark_unknown: bool) -> super::view::Style {
        super::view::Style {
            relief: self.relief.min(3),
            contours: self.contours,
            grid: self.grid & 3,
            light: if self.day_night { light.min(256) } else { 256 },
            night: super::view::NIGHT,
            mark_unknown,
            glow: if self.block_light { 256 } else { 0 },
            smooth: self.smooth,
            textures: self.textures,
            far_fog: self.far_fog,
        }
    }

    /// Cloud parameters; the caller fills in the weather and the time of day.
    pub fn cloud_params(&self) -> super::clouds::Params {
        super::clouds::Params {
            density: self.cloud_density as f32 / 100.0,
            speed: self.cloud_speed as f32 / 100.0,
            puff: self.cloud_puff as f32 / 100.0,
            quality: self.cloud_quality.min(3),
            theme: super::clouds::Theme::from_index(self.cloud_theme),
            weather: self.cloud_weather,
            lightning: self.lightning.min(2),
            lightning_rate: self.lightning_rate as f32 / 100.0,
            god_rays: self.god_rays,
            reach: self.reach_blocks(false) as f32,
            far_fog: self.far_fog,
            ..Default::default()
        }
    }

    /// Cloud parameters for a map of dimension `dim` (`minecraft:the_nether`...): the look follows it unless
    /// fixed by `cloud_dim`, with that look's intensity.
    pub fn cloud_params_for(&self, dim: &str) -> super::clouds::Params {
        use super::clouds::Look;
        let look = match self.cloud_dim {
            1 => Look::Overworld,
            2 => Look::Nether,
            3 => Look::End,
            _ => Look::of_dimension(dim),
        };
        let intensity = match look {
            Look::Overworld => 1.0,
            Look::Nether => self.nether_intensity as f32 / 100.0,
            Look::End => self.end_intensity as f32 / 100.0,
        };
        super::clouds::Params { look, intensity, ..self.cloud_params() }
    }

    pub fn from_text(text: &str) -> Settings {
        let mut s = Settings::default();
        let mut version = 0u32;
        // Pre-0.3 keys: the four info flags and passive mobs on the radar.
        let mut old_info: Vec<(Info, bool)> = Vec::new();
        let mut old_passive = None;
        let mut info_line = None;
        let mut old_cave = false;
        let mut frame_raw: Option<u8> = None;
        let mut reach_new = text.lines().any(|l| l.trim_start().starts_with("cloud_reach_blocks="));
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let (k, v) = (k.trim(), v.trim());
            let flag = |d: bool| match v {
                "true" => true,
                "false" => false,
                _ => d,
            };
            if let Some(kind) = k.strip_prefix("ent.") {
                if !kind.is_empty() && kind.len() < 128 && s.entities.len() < 1024 {
                    let d = super::entities::default_style(kind);
                    let st = super::entities::Style::from_text(v, d);
                    s.set_entity_style(kind, st);
                }
                continue;
            }
            match k {
                "version" => version = v.parse().unwrap_or(0),
                "shape" => s.shape = if v == "round" { Shape::Round } else { Shape::Square },
                "north_lock" => s.north_lock = flag(s.north_lock),
                "zoom" => s.zoom = v.parse().unwrap_or(s.zoom).min(ZOOMS.len() - 1),
                "size" => s.size = v.parse().unwrap_or(s.size).min(SIZES.len() - 1),
                "mode" => {
                    s.mode = if v == "hidden" { Mode::Hidden } else { Mode::Shown };
                    old_cave = v == "cave";
                }
                "corner" => {
                    s.corner = match v {
                        "top_left" => Corner::TopLeft,
                        "bottom_left" => Corner::BottomLeft,
                        "bottom_right" => Corner::BottomRight,
                        _ => Corner::TopRight,
                    }
                }
                "opacity" => s.opacity = v.parse().unwrap_or(100u32).clamp(10, 100),
                "coords" | "biome" | "time" | "light" => {
                    if let Some(i) = Info::from_key(k) {
                        old_info.push((i, flag(true)));
                    }
                }
                "info" => info_line = Some(v.to_string()),
                "beams" => s.beams = flag(s.beams),
                "radar" => s.radar = flag(s.radar),
                "radar_passive" => old_passive = Some(flag(false)),
                "trail" => s.trail = flag(s.trail),
                "fog" => s.fog = flag(s.fog),
                "max_deaths" => s.max_deaths = v.parse().unwrap_or(5usize).min(50),
                "relief" => s.relief = v.parse().unwrap_or(2u8).min(3),
                "contours" => s.contours = pick(&CONTOURS, v.parse().unwrap_or(0), 0),
                "grid" => s.grid = v.parse().unwrap_or(0u8).min(3),
                "day_night" => s.day_night = flag(s.day_night),
                "far" => s.far = flag(s.far),
                "clouds" => s.clouds = flag(s.clouds),
                "cloud_density" => s.cloud_density = pick(&CLOUD_DENSITIES, v.parse().unwrap_or(85), 85),
                "cloud_speed" => s.cloud_speed = pick(&CLOUD_SPEEDS, v.parse().unwrap_or(100), 100),
                "cloud_puff" => s.cloud_puff = pick(&CLOUD_PUFFS, v.parse().unwrap_or(60), 60),
                "cloud_theme" => s.cloud_theme = v.parse().unwrap_or(0u8).min(super::clouds::Theme::ALL.len() as u8 - 1),
                "cloud_weather" => s.cloud_weather = flag(s.cloud_weather),
                "cloud_quality" => s.cloud_quality = v.parse().unwrap_or(1u8).min(3),
                "cloud_fps" => s.cloud_fps = pick(&CLOUD_FPS, v.parse().unwrap_or(0), 0),
                "cloud_dim" => s.cloud_dim = v.parse().unwrap_or(0u8).min(3),
                "nether_intensity" => s.nether_intensity = pick(&INTENSITIES, v.parse().unwrap_or(100), 100),
                "end_intensity" => s.end_intensity = pick(&INTENSITIES, v.parse().unwrap_or(100), 100),
                "lightning" => s.lightning = v.parse().unwrap_or(2u8).min(2),
                "lightning_rate" => s.lightning_rate = pick(&LIGHTNING_RATES, v.parse().unwrap_or(100), 100),
                "god_rays" => s.god_rays = flag(s.god_rays),
                // 0.3.2: chunks of feather width; both maps take the migrated value unless the file has new ones.
                "cloud_reach" if !reach_new => {
                    let r = migrate_reach(pick(&OLD_CLOUD_REACHES, v.parse().unwrap_or(3), 3));
                    (s.cloud_reach, s.mini_reach) = (r, r);
                }
                "cloud_reach_blocks" => {
                    s.cloud_reach = v.parse().unwrap_or(REACH_DEFAULT).min(REACH_MAX);
                    reach_new = true;
                }
                "mini_reach_blocks" => {
                    s.mini_reach = v.parse().unwrap_or(REACH_DEFAULT).min(REACH_MAX);
                    reach_new = true;
                }
                "reach_chunks" => s.reach_chunks = flag(s.reach_chunks),
                "far_threads" => s.far_threads = pick(&FAR_THREADS, v.parse().unwrap_or(0), 0),
                "mini_fps" => s.mini_fps = pick(&FPS_CAPS, v.parse().unwrap_or(0), 0),
                "map_fps" => s.map_fps = pick(&FPS_CAPS, v.parse().unwrap_or(0), 0),
                "ui_theme" => s.ui_theme = v.parse().unwrap_or(0u8).min(2),
                "compass" => s.compass = flag(s.compass),
                "animations" => s.animations = flag(s.animations),
                "autozoom" => s.autozoom = flag(s.autozoom),
                "az_near" => s.az_near = v.parse().unwrap_or(s.az_near).min(ZOOMS.len() - 1),
                "az_far" => s.az_far = v.parse().unwrap_or(s.az_far).min(ZOOMS.len() - 1),
                "ent_mode" => {
                    for (k, m) in v.split(',').take(4).enumerate() {
                        s.ent_mode[k] = m.trim().parse().unwrap_or(s.ent_mode[k]).min(2);
                    }
                }
                "face_frame" => frame_raw = v.parse().ok(),
                "layer" => s.layer = super::layers::LayerMode::from_key(v).map_or(0, |m| m.index()),
                "fixed_y" => s.fixed_y = pick(&FIXED_YS, v.parse().unwrap_or(64), 64),
                "cave_depth" => s.cave_depth = pick(&CAVE_DEPTHS, v.parse().unwrap_or(16), 16),
                "surface_rule" => s.surface_rule = v.parse().unwrap_or(0u8).min(super::layers::RULES as u8 - 1),
                "ui_fixed" => s.ui_fixed = flag(s.ui_fixed),
                "color_style" => s.color_style = v.parse().unwrap_or(2u8).min(2),
                "lang" => s.lang = v.parse().unwrap_or(0u8).min(2),
                "roof_size" => s.roof_size = pick(&ROOF_SIZES, v.parse().unwrap_or(3), 3),
                "roof_reach" => s.roof_reach = pick(&ROOF_REACHES, v.parse().unwrap_or(24), 24),
                "layer_delay" => s.layer_delay = pick(&LAYER_DELAYS, v.parse().unwrap_or(10), 10),
                "cave_step" => s.cave_step = pick(&CAVE_STEPS, v.parse().unwrap_or(8), 8),
                "indoor" => s.indoor = pick(&super::layers::INDOOR_MODES, v.parse().unwrap_or(2), 2),
                "smooth" => s.smooth = flag(s.smooth),
                "textures" => s.textures = flag(s.textures),
                "block_light" => s.block_light = flag(s.block_light),
                "reveal" => s.reveal = pick(&REVEAL_MODES, v.parse().unwrap_or(2), 2),
                "far_fog" => s.far_fog = flag(s.far_fog),
                "badge" => s.badge = pick(&BADGE_MODES, v.parse().unwrap_or(0), 0),
                "hostile" => s.hostile = pick(&HOSTILE_MODES, v.parse().unwrap_or(0), 0),
                "info_align" => s.info_align = pick(&INFO_ALIGNS, v.parse().unwrap_or(0), 0),
                "ui_cat" => s.ui_cat = v.parse().unwrap_or(0u8).min(CATEGORIES.len() as u8 - 1),
                // 0.2 `frame_style`: the frame is fixed since 0.3.
                _ => {}
            }
        }
        if let Some(line) = info_line {
            // Known kinds in the saved order, each once; kinds the file lacks are appended (off) after them.
            let mut info: Vec<(Info, bool)> = Vec::new();
            for part in line.split(',') {
                let (k, on) = part.split_once(':').unwrap_or((part, "on"));
                if let Some(i) = Info::from_key(k.trim()) {
                    if !info.iter().any(|e| e.0 == i) {
                        info.push((i, on.trim() != "off"));
                    }
                }
            }
            for i in Info::ALL {
                if !info.iter().any(|e| e.0 == i) {
                    info.push((i, false));
                }
            }
            s.info = info;
        } else {
            for (i, on) in old_info {
                if let Some(e) = s.info.iter_mut().find(|e| e.0 == i) {
                    e.1 = on;
                }
            }
        }
        if s.az_far < s.az_near {
            s.az_far = s.az_near;
        }
        if version < 5 {
            // 0.3.1 kept the cave view in `mode` and the frame in whole px (default 1): the cave choice becomes the
            // cave layer, the old default frame becomes "none" (the 0.3.2 default) and other widths carry over.
            if old_cave {
                s.layer = super::layers::LayerMode::Cave.index();
            }
            s.face_frame = match frame_raw {
                None | Some(0) | Some(1) => 0,
                Some(px) => pick(&FRAME_STEPS, px.min(3) * 4, 0),
            };
        } else {
            s.face_frame = pick(&FRAME_STEPS, frame_raw.unwrap_or(0), 0);
        }
        if version < 4 {
            // 0.3.0 had no auto-zoom and drew every entity as its shape: keep that look on upgrade (the auto-zoom
            // stays off and the modes stay "dot"), only new installs get the 0.3.1 defaults.
            if version > 0 {
                s.autozoom = false;
                s.ent_mode = [0; 4];
            }
        }
        if version < 3 {
            // 0.2 throttled the clouds to 15 frames per second by default; 0.3 follows the map unless capped.
            if s.cloud_fps == 15 {
                s.cloud_fps = 0;
            }
            if old_passive == Some(true) {
                for e in super::entities::CATALOG.iter().filter(|e| e.1 == super::entities::Category::Passive) {
                    let mut st = s.entity_style(e.0);
                    st.mini = true;
                    s.set_entity_style(e.0, st);
                }
            }
        }
        s
    }
}

/// A settings category: a tab (0 minimap, 1 map), a name, a nav icon and its item groups under headers.
/// Items in the group under header `h` of category `cat` (for the folded header's count).
pub fn group_len(cat: usize, h: &str) -> usize {
    CATEGORIES.get(cat).and_then(|c| c.groups.iter().find(|g| g.0 == h)).map_or(0, |g| g.1.len())
}

pub struct Category {
    pub tab: usize,
    pub name: &'static str,
    pub icon: &'static [u16; 16],
    pub groups: &'static [(&'static str, &'static [usize])],
}

impl Category {
    pub fn items(&self) -> impl Iterator<Item = usize> + '_ {
        self.groups.iter().flat_map(|g| g.1.iter().copied())
    }
}

use super::icons as ic;

/// The settings categories, minimap ones first. An item may sit in both tabs (entities, clouds); every flat button
/// index is in at least one category.
pub const CATEGORIES: [Category; 13] = [
    Category {
        tab: 0,
        name: "megamap.cat.view",
        icon: &ic::EYE,
        groups: &[
            ("megamap.group.shape", &[0, 2, 5, 6]),
            ("megamap.group.orient", &[1, 7, 3, 4]),
            ("megamap.group.layers", &[53, 54, 55, 56]),
            ("megamap.group.auto_layer", &[ROOF_SIZE, ROOF_REACH, LAYER_DELAY, CAVE_STEP, INDOOR, BADGE]),
        ],
    },
    Category { tab: 0, name: "megamap.cat.info", icon: &ic::INFO, groups: &[("megamap.group.info_lines", &[13, 14, 15, 16, 17, 18, HOSTILE, INFO_ALIGN]), ("megamap.group.marks", &[10, 11, 12])] },
    Category { tab: 0, name: "megamap.cat.entities", icon: &ic::ENTITIES, groups: &[("megamap.group.radar", &[9]), ("megamap.group.display", &[50, 48, 49, 51, 52])] },
    Category { tab: 0, name: "megamap.cat.autozoom", icon: &ic::AUTOZOOM, groups: &[("megamap.group.autozoom", &[45, 46, 47])] },
    Category { tab: 0, name: "megamap.cat.clouds", icon: &ic::CLOUDS, groups: &[("megamap.group.fog", &[20, 61, 60, REVEAL, FAR_FOG]), ("megamap.group.clouds", &[26, 27, 28, 30])] },
    Category { tab: 0, name: "megamap.cat.perf", icon: &ic::PERF, groups: &[("megamap.group.fps", &[8, 33]), ("megamap.group.quality", &[32, SMOOTH, TEXTURES, BLOCK_LIGHT])] },
    Category {
        tab: 1,
        name: "megamap.cat.view",
        icon: &ic::EYE,
        groups: &[
            ("megamap.group.relief", &[21, 22]),
            ("megamap.group.grid_light", &[23, 24, BLOCK_LIGHT]),
            ("megamap.group.layers", &[53, 54, 55, 56]),
            ("megamap.group.auto_layer", &[ROOF_SIZE, ROOF_REACH, LAYER_DELAY, CAVE_STEP, INDOOR, BADGE]),
            ("megamap.group.colors", &[58]),
        ],
    },
    Category { tab: 1, name: "megamap.cat.waypoints", icon: &ic::FLAG, groups: &[("megamap.group.waypoints", &[10, 12]), ("megamap.group.trail", &[11])] },
    Category { tab: 1, name: "megamap.cat.entities", icon: &ic::ENTITIES, groups: &[("megamap.group.display", &[50, 48, 49, 51, 52]), ("megamap.group.radar", &[9])] },
    Category {
        tab: 1,
        name: "megamap.cat.clouds",
        icon: &ic::CLOUDS,
        groups: &[
            ("megamap.group.explore_fog", &[20, 42, 60, REVEAL, FAR_FOG]),
            ("megamap.group.clouds", &[26, 27, 28, 29, 30, 31]),
            ("megamap.group.dims", &[34, 35, 36]),
            ("megamap.group.storm", &[37, 38, 39]),
        ],
    },
    Category { tab: 1, name: "megamap.cat.data", icon: &ic::DATA, groups: &[("megamap.group.far", &[25, 43])] },
    Category { tab: 1, name: "megamap.cat.perf", icon: &ic::PERF, groups: &[("megamap.group.fps", &[19, 33]), ("megamap.group.quality", &[32, SMOOTH, TEXTURES])] },
    Category { tab: 1, name: "megamap.cat.ui", icon: &ic::WINDOW, groups: &[("megamap.group.look", &[57, 40, 41, 59]), ("megamap.group.reset", &[44])] },
];

/// Global category indices of a tab, in order.
pub fn tab_categories(tab: usize) -> impl Iterator<Item = usize> {
    (0..CATEGORIES.len()).filter(move |&c| CATEGORIES[c].tab == tab)
}

/// A row of the settings content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Row {
    /// A group header (its translation key).
    Header(&'static str),
    Item(usize),
    /// "Reset category" at the end of a category.
    ResetCategory(usize),
}

/// The content rows: the category's groups under their headers, or, with a search, every matching item (label or
/// hint, case-insensitive) of the whole settings, each once.
/// A group whose header is in `collapsed` shows the header alone (a click on it folds and unfolds the group).
pub fn rows(cat: usize, search: &str, labels: &[String], tr: &Tr, collapsed: &[&'static str], out: &mut Vec<Row>) {
    out.clear();
    let q = search.trim().to_lowercase();
    if q.is_empty() {
        let Some(c) = CATEGORIES.get(cat) else { return };
        for (h, items) in c.groups {
            out.push(Row::Header(h));
            if !collapsed.contains(h) {
                out.extend(items.iter().map(|&i| Row::Item(i)));
            }
        }
        out.push(Row::ResetCategory(cat));
        return;
    }
    let mut seen = [false; BUTTON_COUNT];
    for c in CATEGORIES.iter() {
        for i in c.items() {
            let hit = labels.get(i).is_some_and(|l| l.to_lowercase().contains(&q)) || tr.t(hint(i)).to_lowercase().contains(&q);
            if hit && !std::mem::replace(&mut seen[i], true) {
                out.push(Row::Item(i));
            }
        }
    }
}

/// What a setting does (a translation key), shown under the content when it is hovered.
pub fn hint(i: usize) -> &'static str {
    const HINTS: [&str; BUTTON_COUNT] = [
        "megamap.hint.shape",
        "megamap.hint.north_lock",
        "megamap.hint.size",
        "megamap.hint.zoom",
        "megamap.hint.mode",
        "megamap.hint.corner",
        "megamap.hint.opacity",
        "megamap.hint.compass",
        "megamap.hint.mini_fps",
        "megamap.hint.radar",
        "megamap.hint.beams",
        "megamap.hint.trail",
        "megamap.hint.deaths",
        "megamap.hint.info",
        "megamap.hint.info",
        "megamap.hint.info",
        "megamap.hint.info",
        "megamap.hint.info",
        "megamap.hint.info",
        "megamap.hint.map_fps",
        "megamap.hint.fog",
        "megamap.hint.relief",
        "megamap.hint.contours",
        "megamap.hint.grid",
        "megamap.hint.day_night",
        "megamap.hint.far",
        "megamap.hint.clouds",
        "megamap.hint.cloud_density",
        "megamap.hint.cloud_speed",
        "megamap.hint.cloud_puff",
        "megamap.hint.cloud_theme",
        "megamap.hint.cloud_weather",
        "megamap.hint.cloud_quality",
        "megamap.hint.cloud_fps",
        "megamap.hint.cloud_dim",
        "megamap.hint.nether",
        "megamap.hint.end",
        "megamap.hint.lightning",
        "megamap.hint.lightning_rate",
        "megamap.hint.god_rays",
        "megamap.hint.ui_theme",
        "megamap.hint.animations",
        "megamap.hint.map_reach",
        "megamap.hint.far_threads",
        "megamap.hint.reset_all",
        "megamap.hint.autozoom",
        "megamap.hint.az_near",
        "megamap.hint.az_far",
        "megamap.hint.ent_mode",
        "megamap.hint.ent_mode",
        "megamap.hint.ent_mode",
        "megamap.hint.ent_mode",
        "megamap.hint.face_frame",
        "megamap.hint.layer",
        "megamap.hint.fixed_y",
        "megamap.hint.cave_depth",
        "megamap.hint.surface_rule",
        "megamap.hint.ui_fixed",
        "megamap.hint.color_style",
        "megamap.hint.lang",
        "megamap.hint.reach_unit",
        "megamap.hint.mini_reach",
        "megamap.hint.roof_size",
        "megamap.hint.roof_reach",
        "megamap.hint.layer_delay",
        "megamap.hint.cave_step",
        "megamap.hint.indoor",
        "megamap.hint.smooth",
        "megamap.hint.textures",
        "megamap.hint.block_light",
        "megamap.hint.reveal",
        "megamap.hint.far_fog",
        "megamap.hint.badge",
        "megamap.hint.hostile",
        "megamap.hint.info_align",
    ];
    HINTS.get(i).copied().unwrap_or("")
}

/// Which part of the preview a setting changes (the preview outlines it on hover).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    None,
    MiniFrame,
    MiniMap,
    MiniInfo,
    Entities,
    Clouds,
    Terrain,
    Overlay,
}

pub fn target(i: usize) -> Target {
    match i {
        0 | 2 | 5 | 6 | 7 | 57 => Target::MiniFrame,
        1 | 3 | 4 | 45..=47 | 8 => Target::MiniMap,
        13..=18 => Target::MiniInfo,
        9 | 48..=52 => Target::Entities,
        20 | 26..=39 | 42 | 60 | 61 | 70 | 71 => Target::Clouds,
        21 | 22 | 24 | 25 | 43 | 19 | 53..=56 | 58 | 62..=66 | 67..=69 => Target::Terrain,
        72..=74 => Target::MiniInfo,
        59 => Target::MiniInfo,
        10 | 11 | 12 | 23 => Target::Overlay,
        _ => Target::None,
    }
}
