//! Tachyon's desktop launcher: an obsidian three-pane workspace with a left
//! icon rail, top navigation bar, rounded hero showcase card, quiet social/pack
//! side panel, and clean sub-pages for installations, accounts, patch notes,
//! and settings. Accounts are offline player names; the game runs as a child
//! process in the selected installation's folder.
use super::store::{self, Account, Installation, Store};
use super::{ERROR_FILE, GAME, ICON, Paths};
use crate::tachyon::brand::{self, P};
use crate::tachyon::draw::{self, Face, Image, Rect, Renderer, hex};
use crate::tachyon::ui::{Pointer, Ui};
use crate::tachyon::{fetch, import};
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

const TOP_BAR: f32 = 48.0;
const RAIL_W: f32 = 54.0;
const PANEL_WIDE: f32 = 244.0;
const PANEL_NARROW: f32 = 208.0;

/// Hairline border & tonal obsidian surface tokens for desktop chrome.
const CHROME_BG: u32 = 0x0c0c10;
const HAIRLINE: u32 = 0x1e1e26;
const CARD_BORDER: u32 = 0x262630;
const RAIL_ACTIVE_BG: u32 = 0x251811;
const HERO_PILL_BG: u32 = 0x0f0f14;
const HERO_PILL_HOVER: u32 = 0x18181f;
const HERO_PILL_RING: u32 = 0x3b3b47;
const BADGE_AMBER_BG: u32 = 0x18130e;
const BADGE_AMBER_RING: u32 = 0x3d2c1c;
const BADGE_GREEN_BG: u32 = 0x101c15;
const BADGE_GREEN_RING: u32 = 0x21412d;
const COPPER_LIGHT: u32 = 0xf2a979;

/// A panorama face in the atlas, in pixels: four fit in two rows of it.
const FACE: u32 = 512;
const BACKDROP_W: u32 = 800;
const BACKDROP_H: u32 = 450;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Page {
    Play,
    Installations,
    Accounts,
    Notes,
    Settings,
}

/// Primary icon rail destinations (Accounts & Notes live both in rail and top chrome).
const RAIL_PAGES: [(Page, &str); 4] = [
    (Page::Play, "Home"),
    (Page::Installations, "Installations"),
    (Page::Accounts, "Accounts"),
    (Page::Notes, "Patch notes"),
];

/// What changed in Tachyon, newest first: title, date, and release highlights.
const NOTES: &[(&str, &str, &[&str])] = &[
    (
        "Horizon 26.3 & Obsidian UI",
        "November 15, 2026",
        &[
            "Synced Tachyon with upstream Horizon 26.3 rendering, shader 0.2 pipeline, and native engine hooks.",
            "Redesigned the desktop launcher in an obsidian + burnt-copper 3-pane workspace with rounded hero showcase and icon rail.",
            "Added Fluid Valve, Smart Fluid Pipe, Hose Pulley, Item Vault, Redstone Link, and brass diodes to the Create 6.0.10 port.",
        ],
    ),
    (
        "Create, Kinetica & Chronicles",
        "November 8, 2026",
        &[
            "Kinetic rotation network solver, mechanical arms, deployers, saws, crushing wheels, and belts running natively in Rust.",
            "Chronicles quest progression, Orbital Laser integration, and Modrinth drop-in asset importing for Create mods.",
            "Each installation isolates its worlds, options, resource packs, and ported mods inside its own folder.",
        ],
    ),
    (
        "Mobile & Touch Profiles",
        "October 29, 2026",
        &[
            "Minecraft: Java Edition 26.3 running natively on Windows and Android with customizable Zalith touch controls.",
            "Tachyon loading overlay and reduced tile-GPU defaults on phones up to 120 FPS.",
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

/// The game, started by Play.
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

const SHOTS: [&str; 8] = [
    "1-play",
    "2-play-menu",
    "3-installations",
    "4-edit",
    "5-accounts",
    "6-notes",
    "7-settings",
    "8-drop",
];

/// What can be imported.
const KINDS: &str = "Worlds or resource packs (.zip), Horizon mods (.hmod), or Java mods (.jar) — blocks, items & recipes import automatically.";

/// Frames a page is shown before its picture, for the window to settle.
const SHOT_FRAMES: u32 = 30;

/// A file dropped on the window, being installed into an installation.
struct Importing {
    line: Arc<std::sync::Mutex<String>>,
    work: std::thread::JoinHandle<Result<import::Imported, String>>,
}

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
    wheel: f32,
    modifiers: ModifiersState,
    page: Page,
    history: Vec<Page>,
    history_pos: usize,
    scroll: f32,
    focus: Option<Field>,
    account_input: String,
    edit: Option<Edit>,
    menu: bool,
    note: Option<Note>,
    started: Instant,
    panorama: Vec<Image>,
    backdrop: Option<Image>,
    icon: Option<Image>,
    faces: HashMap<&'static str, Option<Image>>,
    game: Option<Running>,
    occluded: bool,
    cursor: CursorIcon,
    wanted_cursor: CursorIcon,
    snapshots: Option<Snapshots>,
    done: bool,
    dropping: bool,
    importing: Option<Importing>,
    fetch_ask: Option<usize>,
}

fn inset_uv(w: u32, h: u32) -> [f32; 4] {
    let ex = 0.5 / w.max(1) as f32;
    let ey = 0.5 / h.max(1) as f32;
    [ex, ey, 1.0 - ex, 1.0 - ey]
}

fn inset(size: u32) -> [f32; 4] {
    inset_uv(size, size)
}

fn panel_width(window_w: f32) -> f32 {
    if window_w >= 1120.0 {
        PANEL_WIDE
    } else if window_w >= 980.0 {
        PANEL_NARROW
    } else {
        0.0
    }
}

/// Computes hero card rectangle inside `content` so center controls and dropdown
/// anchor deterministically.
fn hero_rect(content: Rect) -> Rect {
    let pad_x = 24.0;
    let top_y = content.y + 50.0;
    let footer_reserve = 136.0;
    let h = (content.bottom() - top_y - footer_reserve).clamp(290.0, 480.0);
    Rect::new(content.x + pad_x, top_y, (content.w - pad_x * 2.0).max(200.0), h)
}

/// Centered frosted play capsule inside `hero`.
fn hero_play_pill(hero: Rect) -> Rect {
    let w = 276.0f32.min(hero.w - 48.0);
    let h = 62.0;
    let cx = hero.x + (hero.w - w) / 2.0;
    let cy = hero.y + hero.h * 0.43 - h / 2.0;
    Rect::new(cx, cy.max(hero.y + 24.0), w, h)
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

/// Synthesizes a misty, sunlit overgrown voxel ruin & lantern sanctuary
/// backdrop (`BACKDROP_W` x `BACKDROP_H` RGBA) used when Minecraft panorama
/// resources are not yet present.
fn generate_hero_backdrop() -> Vec<u8> {
    let (w, h) = (BACKDROP_W, BACKDROP_H);
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);

    let hash2 = |ix: i32, iy: i32| -> f32 {
        let mut n = (ix.wrapping_mul(374_761_393)) ^ (iy.wrapping_mul(668_265_263));
        n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
        ((n ^ (n >> 16)) as u32 & 0xffff) as f32 / 65535.0
    };
    let smooth = |x: f32, y: f32| -> f32 {
        let ix = x.floor() as i32;
        let iy = y.floor() as i32;
        let fx = x - ix as f32;
        let fy = y - iy as f32;
        let ux = fx * fx * (3.0 - 2.0 * fx);
        let uy = fy * fy * (3.0 - 2.0 * fy);
        let a = hash2(ix, iy);
        let b = hash2(ix + 1, iy);
        let c = hash2(ix, iy + 1);
        let d = hash2(ix + 1, iy + 1);
        let top = a + (b - a) * ux;
        let bot = c + (d - c) * ux;
        top + (bot - top) * uy
    };
    let fbm = |x: f32, y: f32| -> f32 {
        smooth(x, y) * 0.54
            + smooth(x * 2.15 + 7.3, y * 2.15 - 3.1) * 0.30
            + smooth(x * 4.6, y * 4.6) * 0.16
    };

    const BAYER4: [f32; 16] = [
        0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0,
        12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0,
        3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0,
        15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0,
    ];

    // Hanging lanterns: (cx_norm, top_anchor_y, bottom_y)
    const LANTERNS: [(f32, f32, f32); 3] = [
        (0.285, 0.255, 0.335),
        (0.715, 0.125, 0.225),
        (0.742, 0.265, 0.355),
    ];

    for py in 0..h {
        let v = py as f32 / h as f32;
        for px in 0..w {
            let u = px as f32 / w as f32;

            // Layered misty woodland cavern depth gradient.
            let sky_open = (-((u - 0.32) / 0.38).powi(2) - ((v - 0.22) / 0.42).powi(2)).exp();
            let mut r = 0.048 + sky_open * 0.14;
            let mut g = 0.068 + sky_open * 0.185;
            let mut b = 0.062 + sky_open * 0.155;

            // Far forested silhouettes & tree trunks at mid fog distance.
            let tree_col = ((u * 26.0).floor()) as i32;
            let tree_hash = hash2(tree_col, 91);
            if tree_hash > 0.52 && (0.14..0.72).contains(&v) {
                let in_trunk = ((u * 26.0).fract() - 0.5).abs() < 0.22;
                let canopy_r = 0.22 + (tree_hash - 0.52) * 0.35;
                if in_trunk && v > canopy_r {
                    r = r * 0.72 + 0.025;
                    g = g * 0.72 + 0.036;
                    b = b * 0.72 + 0.030;
                }
            }

            // Voxel stone temple archway / mechanical viaduct ruins.
            let in_left = (0.295..0.355).contains(&u) && (0.21..0.72).contains(&v);
            let in_right = (0.625..0.685).contains(&u) && (0.23..0.72).contains(&v);
            let in_beam = (0.265..0.715).contains(&u) && (0.185..0.260).contains(&v);
            if in_left || in_right || in_beam {
                let bx = (u * 80.0).floor() as i32;
                let by = (v * 45.0).floor() as i32;
                let mortar = ((u * 80.0).fract() < 0.08 || (v * 45.0).fract() < 0.08) as i32 as f32;
                let shade = hash2(bx, by) * 0.045 - mortar * 0.022;
                let left_rim = ((u - 0.295).abs() < 0.006 || (u - 0.625).abs() < 0.006) as i32 as f32;
                let top_rim = ((v - 0.185).abs() < 0.007) as i32 as f32;
                let sr = 0.092 + shade + (left_rim + top_rim) * 0.055;
                let sg = 0.086 + shade + (left_rim + top_rim) * 0.040;
                let sb = 0.090 + shade + (left_rim + top_rim) * 0.020;
                // Subtle moss clinging to top arch edge.
                let moss = (in_beam && v < 0.215 && hash2(bx, 7) > 0.35) as i32 as f32;
                let sr = sr * (1.0 - moss * 0.4) + 0.06 * moss;
                let sg = sg * (1.0 - moss * 0.4) + 0.14 * moss;
                let sb = sb * (1.0 - moss * 0.4) + 0.05 * moss;
                let fog = 0.26;
                r = r * fog + sr * (1.0 - fog);
                g = g * fog + sg * (1.0 - fog);
                b = b * fog + sb * (1.0 - fog);
            }

            // Sunlit emerald foliage break through distant canopy on right side.
            let em_dx = (u - 0.61) / 0.095;
            let em_dy = (v - 0.42) / 0.13;
            let em_d2 = em_dx * em_dx + em_dy * em_dy;
            if em_d2 < 1.15 {
                let leaves = fbm((px / 4) as f32 * 0.45, (py / 4) as f32 * 0.45);
                let burst = (1.0 - em_d2 / 1.15).powf(1.8) * (0.35 + 0.65 * leaves);
                r += burst * 0.20;
                g += burst * 0.38;
                b += burst * 0.08;
            }

            // Diagonal golden-amber sunbeams filtering through overhead opening.
            let proj = (u - 0.04) * 0.76 - v * 0.65;
            let beam1 = (-((proj - 0.03) / 0.070).powi(2)).exp();
            let beam2 = (-((proj - 0.15) / 0.036).powi(2)).exp() * 0.70;
            let beam3 = (-((proj + 0.08) / 0.042).powi(2)).exp() * 0.50;
            let beam_fade = (1.0 - v * 0.68).max(0.06) * (1.0 - (u * 0.92).min(0.78));
            let fog_wisp = 0.75 + 0.25 * fbm(u * 4.5 + v * 1.5, v * 4.5);
            let shaft = (beam1 + beam2 + beam3) * beam_fade * fog_wisp * 0.34;
            r += shaft * 1.02;
            g += shaft * 0.94;
            b += shaft * 0.76;

            // Layered meadow ground & sunlit center moss trail.
            let horizon = 0.66 - 0.045 * ((u - 0.48).abs() * 2.0) + 0.025 * smooth(u * 11.0, 2.0);
            if v > horizon {
                let depth = ((v - horizon) / 0.34).clamp(0.0, 1.0);
                let ground_tex = fbm(u * 22.0, v * 16.0);
                let trail = (-((u - (0.48 + (v - 0.68) * 0.16)) / (0.075 + depth * 0.085)).powi(2)).exp();
                let mut gr = 0.085 + ground_tex * 0.055 + trail * 0.11;
                let mut gg = 0.125 + ground_tex * 0.085 + trail * 0.095;
                let mut gb = 0.075 + ground_tex * 0.045 + trail * 0.075;

                // Crisp clustered vertical grass blades & sunlit meadow tips.
                let col_id = (px / 3) as i32;
                let blade_seed = hash2(col_id, 17);
                if blade_seed > 0.34 {
                    let blade_h = 0.022 + blade_seed * 0.038;
                    let base_y = horizon + 0.02 + hash2(col_id, 53) * 0.24;
                    if v > base_y - blade_h && v < base_y {
                        let tip = ((base_y - v) / blade_h).clamp(0.0, 1.0);
                        let light = (1.0 - depth * 0.55) * (0.45 + 0.55 * tip);
                        gr = gr * 0.5 + 0.13 * light + shaft * 0.35;
                        gg = gg * 0.5 + 0.23 * light + shaft * 0.38;
                        gb = gb * 0.5 + 0.09 * light + shaft * 0.20;
                        // Tiny pink/gold wildflower tips in sunlit clearing.
                        if tip > 0.78 && hash2(col_id, 99) > 0.88 && (0.32..0.66).contains(&u) {
                            if col_id % 2 == 0 {
                                gr += 0.34;
                                gg += 0.18;
                                gb += 0.26;
                            } else {
                                gr += 0.36;
                                gg += 0.24;
                                gb += 0.08;
                            }
                        }
                    }
                }

                let fog_dist = 1.0 - depth * 0.52;
                r = gr * fog_dist + shaft * 0.45;
                g = gg * fog_dist + shaft * 0.42;
                b = gb * fog_dist + shaft * 0.30;
            }

            // Overhanging foliage canopy + dangling vine tendrils.
            let canopy = 0.09 + 0.06 * fbm(u * 10.0, 1.5) + if u < 0.26 || u > 0.72 { 0.09 } else { 0.0 };
            let vine_col = (px / 5) as i32;
            let vine_len = if hash2(vine_col, 31) > 0.76 {
                canopy + 0.07 + hash2(vine_col, 41) * 0.18
            } else {
                0.0
            };
            let in_vine = px % 5 <= 1 && v < vine_len;
            if v < canopy || in_vine {
                let lx = (px / 5) as f32;
                let ly = (py / 5) as f32;
                let shade = 0.55 + 0.45 * fbm(lx * 0.5, ly * 0.5);
                let leaf_w = if in_vine { 0.85 } else { ((canopy - v) / 0.035).clamp(0.0, 1.0) };
                r = r * (1.0 - leaf_w) + (0.042 * shade) * leaf_w;
                g = g * (1.0 - leaf_w) + (0.078 * shade) * leaf_w;
                b = b * (1.0 - leaf_w) + (0.046 * shade) * leaf_w;
            }

            // Crisp voxel stone foreground framing pillars at far left & right edges.
            let left_fg = u < 0.125 && v > (0.44 + ((px / 32) as f32 * 0.09) % 0.16);
            let right_fg = u > 0.885 && v > (0.36 + ((px / 32) as f32 * 0.11) % 0.16);
            if left_fg || right_fg {
                let bx = (px / 4) as i32;
                let by = (py / 4) as i32;
                let tex = hash2(bx, by) * 0.03;
                let moss_top = v < 0.56;
                r = if moss_top { 0.055 } else { 0.040 } + tex;
                g = if moss_top { 0.088 } else { 0.048 } + tex;
                b = if moss_top { 0.048 } else { 0.042 } + tex;
            }

            // Crisp iron-chained copper lanterns with warm localized amber bloom.
            for (lx, y0, y1) in LANTERNS {
                let dx = (u - lx) * 1.77;
                // Thin chain & crisp lantern pixel housing.
                if dx.abs() < 0.0025 && (y0..y1 - 0.025).contains(&v) && py % 4 != 0 {
                    r = 0.12;
                    g = 0.11;
                    b = 0.13;
                }
                let cy = y1;
                if dx.abs() < 0.0085 && (cy - 0.022..cy + 0.012).contains(&v) {
                    if dx.abs() < 0.0045 && (cy - 0.014..cy + 0.006).contains(&v) {
                        r = 1.00;
                        g = 0.72;
                        b = 0.30;
                    } else {
                        r = 0.18;
                        g = 0.12;
                        b = 0.09;
                    }
                } else {
                    let d2 = dx * dx + (v - cy).powi(2);
                    let halo = (-d2 / (0.026 * 0.026)).exp() * 0.52 + (-d2 / (0.065 * 0.065)).exp() * 0.14;
                    r += halo * 0.96;
                    g += halo * 0.52;
                    b += halo * 0.14;
                }
            }

            // Edge vignette framing smoothly into obsidian #0b0b0e.
            let vx = ((u - 0.5).abs() * 2.0).powf(2.6);
            let vy = ((v - 0.48).abs() * 2.0).powf(2.6);
            let vig = (1.0 - (vx * 0.42 + vy * 0.44)).clamp(0.14, 1.0);
            r *= vig;
            g *= vig;
            b *= vig;

            let dither = (BAYER4[((py & 3) * 4 + (px & 3)) as usize] - 0.5) / 255.0;
            let to_u8 = |c: f32| ((c + dither).clamp(0.0, 1.0) * 255.0).round() as u8;
            rgba.extend_from_slice(&[to_u8(r), to_u8(g), to_u8(b), 255]);
        }
    }
    rgba
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
            let over =
                |i: usize| ((u32::from(hat[i]) * a + u32::from(base[i]) * (255 - a)) / 255) as u8;
            face.extend([over(0), over(1), over(2), 255]);
        }
    }
    Some(draw::resize_nearest(&face, 8, 8, 64, 64))
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
        let start_page = if first { Page::Accounts } else { Page::Play };
        Self {
            paths,
            store,
            window: None,
            renderer: None,
            pointer: Pointer::default(),
            wheel: 0.0,
            modifiers: ModifiersState::default(),
            page: start_page,
            history: vec![start_page],
            history_pos: 0,
            scroll: 0.0,
            focus: first.then_some(Field::Account),
            account_input: String::new(),
            edit: None,
            menu: false,
            note: None,
            started: Instant::now(),
            panorama: Vec::new(),
            backdrop: None,
            icon: None,
            faces: HashMap::new(),
            game: None,
            occluded: false,
            cursor: CursorIcon::Default,
            wanted_cursor: CursorIcon::Default,
            snapshots,
            done: false,
            dropping: false,
            importing: None,
            fetch_ask: None,
        }
    }

    /// Installs a file dropped on the window into the selected installation.
    fn import(&mut self, file: PathBuf) {
        if self.importing.is_some() {
            self.note(
                "Wait for the current import to finish, then drop the file again.",
                true,
            );
            return;
        }
        let installation = self.store.installations[self.store.selected_index()].clone();
        let home = installation.folder(&self.paths.data);
        let name = file
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        log::info!(
            "Tachyon launcher: importing {} into {}",
            file.display(),
            home.display()
        );
        let line = Arc::new(std::sync::Mutex::new(format!(
            "Importing {name} into {}",
            installation.name
        )));
        let said = line.clone();
        let work = std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&home);
            import::import(&file, &name, &home, &|l| *said.lock().unwrap() = l)
        });
        self.importing = Some(Importing { line, work });
    }

    /// Gets Create from Modrinth into the selected installation (the player
    /// said yes), off the window's thread, as an import.
    fn get_fetch(&mut self, f: fetch::Fetch) {
        if self.importing.is_some() {
            self.note("Wait for the current import to finish.", true);
            return;
        }
        let installation = self.store.installations[self.store.selected_index()].clone();
        let home = installation.folder(&self.paths.data);
        let cache = self.paths.data.join("cache");
        log::info!("Tachyon launcher: getting {} into {}", f.name, home.display());
        let line = Arc::new(std::sync::Mutex::new(format!(
            "Getting {} for {}",
            f.name, installation.name
        )));
        let said = line.clone();
        let work = std::thread::spawn(move || {
            let _ = std::fs::create_dir_all(&home);
            fetch::get(&f, &home, &cache, &|l| *said.lock().unwrap() = l)
        });
        self.importing = Some(Importing { line, work });
    }

    /// Notices an import ending, and says how it went.
    fn watch_import(&mut self) {
        let Some(importing) = &self.importing else {
            return;
        };
        if !importing.work.is_finished() {
            let line = importing.line.lock().unwrap().clone();
            self.note = Some(Note {
                text: format!("{line}..."),
                error: false,
                at: Instant::now(),
            });
            return;
        }
        let Some(importing) = self.importing.take() else {
            return;
        };
        match importing.work.join() {
            Ok(Ok(imported)) => {
                log::info!("Tachyon launcher: imported {}", imported.name);
                self.note(imported.note, false);
            }
            Ok(Err(e)) => self.note(e, true),
            Err(_) => self.note("The import stopped with an error.", true),
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
        self.dropping = false;
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
            6 => self.set_page(Page::Settings),
            _ => {
                self.set_page(Page::Installations);
                self.dropping = true;
            }
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
            self.note(
                format!("The launcher's settings could not be saved: {e}"),
                true,
            );
        }
    }

    fn set_page(&mut self, page: Page) {
        if self.page != page {
            self.page = page;
            self.scroll = 0.0;
            self.edit = None;
            self.focus = None;
            self.menu = false;
            self.history.truncate(self.history_pos + 1);
            self.history.push(page);
            self.history_pos = self.history.len() - 1;
        }
    }

    fn go_history(&mut self, delta: isize) {
        let next = self.history_pos as isize + delta;
        if next >= 0 && (next as usize) < self.history.len() {
            self.history_pos = next as usize;
            self.page = self.history[self.history_pos];
            self.scroll = 0.0;
            self.edit = None;
            self.focus = None;
            self.menu = false;
        }
    }

    /// The panorama, fallback hero artwork, and app icon into the atlas.
    fn load_images(&mut self, renderer: &mut Renderer) {
        self.panorama.clear();
        self.backdrop = None;
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
        if self.panorama.is_empty() {
            let rgba = generate_hero_backdrop();
            self.backdrop = renderer.image(&rgba, BACKDROP_W, BACKDROP_H);
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
            self.note(
                "Add an offline account first: a name is all it takes.",
                false,
            );
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

    fn stop_game(&mut self) {
        if let Some(running) = &mut self.game {
            let _ = running.child.kill();
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
        let size = |s: &str| {
            s.trim()
                .parse::<u32>()
                .ok()
                .filter(|n| (200..=16384).contains(n))
                .unwrap_or(0)
        };
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
            Some(i) if i < self.store.installations.len() => {
                self.store.installations[i] = installation
            }
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
        let shortcut = (self.modifiers.control_key() && !self.modifiers.alt_key())
            || self.modifiers.super_key();
        if shortcut {
            let letter =
                horizon_client::clipboard::shortcut_letter(&event.logical_key, event.physical_key);
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

    // Widgets & crisp vector iconography.

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
            ui.c.rect(r, 9.0f32.min(r.h / 2.0), hex(0xffffff, 0.06));
        }
        clicked
    }

    fn danger_button(&mut self, ui: &mut Ui, r: Rect, label: &str) -> bool {
        let (hover, clicked) = self.clickable(ui, r);
        ui.c.rect(r, 9.0, hex(P.danger, if hover { 0.22 } else { 0.10 }));
        ui.c.ring(r, 9.0, 1.0, hex(P.danger, if hover { 0.7 } else { 0.4 }));
        ui.centred(r, label, Face::Medium, 13.5, hex(P.danger, 1.0));
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
        ui.c.rect(r, 9.0, hex(CHROME_BG, 1.0));
        let border = if focused {
            P.accent
        } else if hover {
            P.muted
        } else {
            CARD_BORDER
        };
        ui.c.ring(r, 9.0, if focused { 1.5 } else { 1.0 }, hex(border, 1.0));
        let size = 14.0;
        let y = r.y + (r.h - ui.c.line(Face::Regular, size)) / 2.0;
        let room = r.w - 28.0;
        let text = self.text(field).to_owned();
        if text.is_empty() && !focused {
            let shown = ui.c.fit(placeholder, Face::Regular, size, room);
            ui.text(r.x + 14.0, y, &shown, Face::Regular, size, hex(P.muted, 1.0));
            return;
        }
        let mut shown = text.as_str();
        while !shown.is_empty() && ui.c.measure(shown, Face::Regular, size) > room {
            let mut chars = shown.chars();
            chars.next();
            shown = chars.as_str();
        }
        let w = ui.text(r.x + 14.0, y, shown, Face::Regular, size, hex(P.ink, 1.0));
        if focused && (self.started.elapsed().as_millis() / 530) % 2 == 0 {
            ui.c.rect(
                Rect::new(r.x + 15.0 + w, r.y + 11.0, 1.5, r.h - 22.0),
                0.0,
                hex(P.accent, 1.0),
            );
        }
    }

    fn label(ui: &mut Ui, x: f32, y: f32, text: &str) {
        ui.text(x, y, text, Face::Mono, 11.0, hex(P.muted, 1.0));
    }

    fn avatar(&self, ui: &mut Ui, account: Option<&Account>, r: Rect) {
        let radius = (r.w * 0.25).min(6.0);
        match account.and_then(|a| self.face(a)) {
            Some(face) => {
                ui.c.image_rounded(face, r, inset(64), radius, [1.0; 4]);
                ui.c.ring(r, radius, 1.0, hex(CARD_BORDER, 0.9));
            }
            None => {
                ui.c.rect(r, radius, hex(P.plate_soft, 1.0));
                ui.c.ring(r, radius, 1.0, hex(CARD_BORDER, 1.0));
                let initial = account
                    .and_then(|a| a.name.chars().next())
                    .map_or_else(|| "?".to_owned(), |c| c.to_ascii_uppercase().to_string());
                ui.centred(
                    r,
                    &initial,
                    Face::Medium,
                    (r.h * 0.48).max(10.0),
                    hex(P.ink_soft, 1.0),
                );
            }
        }
    }

    /// Geometric copper Tachyon `T` emblem for crisp consistency across rail,
    /// header, and installation rows.
    fn draw_emblem(ui: &mut Ui, r: Rect) {
        ui.c.rect(r, r.w * 0.26, hex(RAIL_ACTIVE_BG, 1.0));
        ui.c.ring(r, r.w * 0.26, 1.0, hex(P.accent, 0.55));
        let pad = r.w * 0.24;
        let top_h = (r.h * 0.14).max(2.0);
        let stem_w = (r.w * 0.16).max(2.5);
        ui.c.rect(
            Rect::new(r.x + pad, r.y + r.h * 0.26, r.w - pad * 2.0, top_h),
            1.0,
            hex(P.accent, 1.0),
        );
        ui.c.rect(
            Rect::new(
                r.x + (r.w - stem_w) / 2.0,
                r.y + r.h * 0.26 + top_h + 1.0,
                stem_w,
                r.h * 0.44 - top_h,
            ),
            1.0,
            hex(P.ink, 0.92),
        );
    }

    fn chevron(ui: &mut Ui, cx: f32, cy: f32, up: bool, color: [f32; 4]) {
        for i in 0..4 {
            let w = 8.0 - i as f32 * 2.1;
            let dy = i as f32 * 1.25;
            let y = if up { cy + 1.8 - dy } else { cy - 1.8 + dy };
            ui.c.rect(Rect::new(cx - w / 2.0, y, w, 1.4), 0.0, color);
        }
    }

    fn draw_arrow_left(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        for i in 0..5 {
            let x = cx - 3.0 + i as f32 * 1.1;
            let dy = i as f32 * 1.1;
            ui.c.rect(Rect::new(x, cy - dy - 0.7, 1.6, 1.6), 0.4, color);
            ui.c.rect(Rect::new(x, cy + dy - 0.7, 1.6, 1.6), 0.4, color);
        }
    }

    fn draw_arrow_right(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        for i in 0..5 {
            let x = cx + 2.0 - i as f32 * 1.1;
            let dy = i as f32 * 1.1;
            ui.c.rect(Rect::new(x, cy - dy - 0.7, 1.6, 1.6), 0.4, color);
            ui.c.rect(Rect::new(x, cy + dy - 0.7, 1.6, 1.6), 0.4, color);
        }
    }

    fn draw_rail_icon(ui: &mut Ui, page: Page, cx: f32, cy: f32, color: [f32; 4]) {
        match page {
            Page::Play => {
                // Clean house outline + gable roof.
                for i in 0..6 {
                    let s = i as f32 * 1.3;
                    ui.c.rect(Rect::new(cx - 0.8 - s, cy - 6.5 + i as f32 * 1.0, 2.0, 1.6), 0.0, color);
                    ui.c.rect(Rect::new(cx - 0.8 + s, cy - 6.5 + i as f32 * 1.0, 2.0, 1.6), 0.0, color);
                }
                ui.c.ring(Rect::new(cx - 5.5, cy - 1.5, 11.0, 8.0), 1.5, 1.4, color);
                ui.c.rect(Rect::new(cx - 1.6, cy + 2.2, 3.2, 4.2), 0.5, color);
            }
            Page::Installations => {
                // Stacked module / cube grid icon.
                ui.c.ring(Rect::new(cx - 6.5, cy - 6.5, 5.5, 5.5), 1.2, 1.3, color);
                ui.c.ring(Rect::new(cx + 1.0, cy - 6.5, 5.5, 5.5), 1.2, 1.3, color);
                ui.c.ring(Rect::new(cx - 6.5, cy + 1.0, 5.5, 5.5), 1.2, 1.3, color);
                ui.c.ring(Rect::new(cx + 1.0, cy + 1.0, 5.5, 5.5), 1.2, 1.3, color);
            }
            Page::Accounts => {
                // User head + shoulders.
                ui.c.ring(Rect::new(cx - 3.2, cy - 6.5, 6.4, 6.4), 3.2, 1.4, color);
                ui.c.ring(Rect::new(cx - 6.0, cy + 1.2, 12.0, 5.5), 2.8, 1.4, color);
            }
            Page::Notes => {
                // Changelog document card with crisp lines.
                ui.c.ring(Rect::new(cx - 6.0, cy - 7.0, 12.0, 14.0), 2.0, 1.4, color);
                ui.c.rect(Rect::new(cx - 3.5, cy - 3.5, 7.0, 1.4), 0.5, color);
                ui.c.rect(Rect::new(cx - 3.5, cy - 0.5, 7.0, 1.4), 0.5, color);
                ui.c.rect(Rect::new(cx - 3.5, cy + 2.5, 4.5, 1.4), 0.5, color);
            }
            Page::Settings => {
                // Mechanical cog gear ring + cardinal spokes.
                ui.c.ring(Rect::new(cx - 4.8, cy - 4.8, 9.6, 9.6), 4.8, 1.8, color);
                ui.c.rect(Rect::new(cx - 1.2, cy - 7.0, 2.4, 2.2), 0.5, color);
                ui.c.rect(Rect::new(cx - 1.2, cy + 4.8, 2.4, 2.2), 0.5, color);
                ui.c.rect(Rect::new(cx - 7.0, cy - 1.2, 2.2, 2.4), 0.5, color);
                ui.c.rect(Rect::new(cx + 4.8, cy - 1.2, 2.2, 2.4), 0.5, color);
            }
        }
    }

    fn draw_plus_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 6.5, cy - 6.5, 13.0, 13.0), 2.5, 1.2, color);
        ui.c.rect(Rect::new(cx - 3.5, cy - 0.8, 7.0, 1.6), 0.4, color);
        ui.c.rect(Rect::new(cx - 0.8, cy - 3.5, 1.6, 7.0), 0.4, color);
    }

    fn draw_sliders_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.rect(Rect::new(cx - 6.5, cy - 3.6, 13.0, 1.4), 0.5, color);
        ui.c.rect(Rect::new(cx - 6.5, cy + 2.2, 13.0, 1.4), 0.5, color);
        ui.c.rect(Rect::new(cx - 3.2, cy - 5.2, 3.2, 4.6), 1.4, color);
        ui.c.rect(Rect::new(cx + 0.6, cy + 0.6, 3.2, 4.6), 1.4, color);
    }

    fn draw_folder_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.rect(Rect::new(cx - 6.0, cy - 5.5, 5.2, 2.2), 0.6, color);
        ui.c.ring(Rect::new(cx - 6.0, cy - 3.5, 12.0, 9.0), 1.5, 1.3, color);
    }

    fn draw_bell_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 4.5, cy - 5.5, 9.0, 8.5), 3.0, 1.3, color);
        ui.c.rect(Rect::new(cx - 6.0, cy + 2.5, 12.0, 1.4), 0.5, color);
        ui.c.rect(Rect::new(cx - 1.5, cy + 4.5, 3.0, 1.5), 0.7, color);
    }

    fn draw_search_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 5.5, cy - 5.5, 8.5, 8.5), 4.2, 1.4, color);
        for i in 0..4 {
            let d = i as f32 * 0.9;
            ui.c.rect(Rect::new(cx + 1.5 + d, cy + 1.5 + d, 1.7, 1.7), 0.4, color);
        }
    }

    fn draw_friends_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 7.5, cy - 8.0, 7.0, 7.0), 3.5, 1.3, color);
        ui.c.ring(Rect::new(cx - 10.5, cy + 1.0, 13.0, 6.5), 3.0, 1.3, color);
        ui.c.ring(Rect::new(cx + 1.5, cy - 6.5, 5.5, 5.5), 2.7, 1.2, color);
        ui.c.ring(Rect::new(cx + 0.5, cy + 1.5, 8.5, 5.5), 2.5, 1.2, color);
    }

    fn layout(&mut self, ui: &mut Ui) {
        let (w, h) = (ui.c.width, ui.c.height);
        let right_w = panel_width(w);
        let content = Rect::new(
            RAIL_W,
            TOP_BAR,
            (w - RAIL_W - right_w).max(320.0),
            (h - TOP_BAR).max(200.0),
        );
        let right_panel = Rect::new(w - right_w, TOP_BAR, right_w, (h - TOP_BAR).max(200.0));

        // Floating overlays take taps before anything beneath.
        let note = self.note_rect(ui, content);
        if let Some(r) = note
            && ui.tapped(r)
        {
            self.note = None;
        }
        let menu = (self.menu && self.page == Page::Play).then(|| self.menu_rects(content));
        if let Some((frame, items)) = &menu {
            self.menu_taps(ui, *frame, items, Rect::new(0.0, 0.0, w, h));
        }

        // Page content, clipped to the central workspace.
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

        // Chrome rails and top bar.
        if right_w > 0.0 {
            self.right_panel(ui, right_panel);
        }
        self.rail(ui, h);
        self.top_bar(ui, w);

        if let Some((frame, items)) = &menu
            && self.menu
        {
            self.draw_menu(ui, *frame, items);
        }
        if let Some(r) = note {
            self.draw_note(ui, r);
        }
        if self.dropping {
            self.drop_overlay(ui, w, h);
        }
    }

    fn top_bar(&mut self, ui: &mut Ui, w: f32) {
        let bar = Rect::new(0.0, 0.0, w, TOP_BAR);
        ui.c.rect(bar, 0.0, hex(CHROME_BG, 1.0));
        ui.c.rect(Rect::new(0.0, TOP_BAR - 1.0, w, 1.0), 0.0, hex(HAIRLINE, 1.0));

        // Left: Brand monogram + wordmark + dropdown arrow + nav arrows + title.
        let brand_hit = Rect::new(10.0, 8.0, 108.0, 32.0);
        let (brand_hover, brand_click) = self.clickable(ui, brand_hit);
        if brand_hover {
            ui.c.rect(brand_hit, 8.0, hex(P.plate_soft, 1.0));
        }
        if brand_click {
            if self.page == Page::Play {
                self.menu = !self.menu;
            } else {
                self.set_page(Page::Play);
            }
        }
        Self::draw_emblem(ui, Rect::new(15.0, 13.0, 22.0, 22.0));
        let nw = ui.text(44.0, 15.0, brand::NAME, Face::Medium, 13.5, hex(P.ink, 1.0));
        Self::chevron(
            ui,
            44.0 + nw + 12.0,
            24.0,
            self.menu && self.page == Page::Play,
            hex(P.muted, 1.0),
        );

        let can_back = self.history_pos > 0;
        let can_fwd = self.history_pos + 1 < self.history.len();
        let back_r = Rect::new(126.0, 10.0, 26.0, 28.0);
        let (b_hover, b_click) = if can_back {
            self.clickable(ui, back_r)
        } else {
            (false, false)
        };
        if b_hover {
            ui.c.rect(back_r, 6.0, hex(P.plate_soft, 1.0));
        }
        Self::draw_arrow_left(
            ui,
            back_r.x + 13.0,
            back_r.y + 14.0,
            hex(if can_back { P.ink_soft } else { 0x3b3b45 }, 1.0),
        );
        if b_click {
            self.go_history(-1);
        }

        let fwd_r = Rect::new(154.0, 10.0, 26.0, 28.0);
        let (f_hover, f_click) = if can_fwd {
            self.clickable(ui, fwd_r)
        } else {
            (false, false)
        };
        if f_hover {
            ui.c.rect(fwd_r, 6.0, hex(P.plate_soft, 1.0));
        }
        Self::draw_arrow_right(
            ui,
            fwd_r.x + 13.0,
            fwd_r.y + 14.0,
            hex(if can_fwd { P.ink_soft } else { 0x3b3b45 }, 1.0),
        );
        if f_click {
            self.go_history(1);
        }

        let title = match self.page {
            Page::Play => "Home",
            Page::Installations if self.edit.is_some() => "Installation setup",
            Page::Installations => "Installations",
            Page::Accounts => "Accounts",
            Page::Notes => "Patch notes",
            Page::Settings => "Settings",
        };
        ui.text(192.0, 16.0, title, Face::Regular, 13.0, hex(P.ink_soft, 1.0));

        // Right cluster: status pill, account pill, utility icon buttons.
        let mut rx = w - 16.0;

        let running = self.game.is_some();
        let ready = self.paths.resources.is_some();
        let status_label = if self.importing.is_some() {
            "Importing..."
        } else if running {
            "1 instance running"
        } else if ready {
            "Ready to play"
        } else {
            "Resources missing"
        };
        let status_w = ui.c.measure(status_label, Face::Medium, 12.0) + 34.0;
        rx -= status_w;
        let status_r = Rect::new(rx, 10.0, status_w, 28.0);
        let (s_hover, s_click) = self.clickable(ui, status_r);
        let (s_bg, s_ring, dot_c, text_c) = if running || ready {
            (
                if s_hover { 0x16261d } else { BADGE_GREEN_BG },
                BADGE_GREEN_RING,
                P.success,
                if running { 0xbcefd0 } else { P.ink_soft },
            )
        } else {
            (
                if s_hover { 0x221a12 } else { BADGE_AMBER_BG },
                BADGE_AMBER_RING,
                P.accent,
                AMBER_TEXT_HEX,
            )
        };
        ui.c.rect(status_r, 14.0, hex(s_bg, 1.0));
        ui.c.ring(status_r, 14.0, 1.0, hex(s_ring, 1.0));
        ui.c.rect(
            Rect::new(status_r.x + 11.0, status_r.y + 10.5, 7.0, 7.0),
            3.5,
            hex(dot_c, 1.0),
        );
        ui.text(
            status_r.x + 24.0,
            status_r.y + 6.5,
            status_label,
            Face::Medium,
            12.0,
            hex(text_c, 1.0),
        );
        if s_click {
            self.set_page(Page::Play);
        }

        rx -= 8.0;
        let account = self.store.account().cloned();
        let acc_name = account
            .as_ref()
            .map_or("Add account", |a| a.name.as_str());
        let acc_fit = ui.c.fit(acc_name, Face::Medium, 12.0, 110.0);
        let acc_w = ui.c.measure(&acc_fit, Face::Medium, 12.0) + 52.0;
        rx -= acc_w;
        let acc_r = Rect::new(rx, 10.0, acc_w, 28.0);
        let (a_hover, a_click) = self.clickable(ui, acc_r);
        ui.c.rect(
            acc_r,
            14.0,
            hex(if a_hover { P.plate } else { P.surface }, 1.0),
        );
        ui.c.ring(acc_r, 14.0, 1.0, hex(CARD_BORDER, 1.0));
        self.avatar(
            ui,
            account.as_ref(),
            Rect::new(acc_r.x + 6.0, acc_r.y + 5.0, 18.0, 18.0),
        );
        ui.text(
            acc_r.x + 30.0,
            acc_r.y + 6.5,
            &acc_fit,
            Face::Medium,
            12.0,
            hex(if account.is_some() { P.ink } else { P.accent }, 1.0),
        );
        Self::chevron(ui, acc_r.right() - 12.0, acc_r.y + 14.0, false, hex(P.muted, 1.0));
        if a_click {
            self.set_page(Page::Accounts);
        }

        // Small crisp action icons (Folder, Notes, Mods/Installations).
        for kind in 0..3 {
            rx -= 34.0;
            let btn = Rect::new(rx, 10.0, 28.0, 28.0);
            let (hover, click) = self.clickable(ui, btn);
            if hover {
                ui.c.rect(btn, 7.0, hex(P.plate_soft, 1.0));
            }
            let ic = hex(if hover { P.ink } else { P.muted }, 1.0);
            match kind {
                0 => {
                    Self::draw_folder_icon(ui, btn.x + 14.0, btn.y + 14.0, ic);
                    if click {
                        super::open_folder(&self.paths.data);
                    }
                }
                1 => {
                    Self::draw_bell_icon(ui, btn.x + 14.0, btn.y + 14.0, ic);
                    if self.page != Page::Notes {
                        ui.c.rect(Rect::new(btn.right() - 7.0, btn.y + 5.0, 4.5, 4.5), 2.25, hex(P.accent, 1.0));
                    }
                    if click {
                        self.set_page(Page::Notes);
                    }
                }
                _ => {
                    Self::draw_search_icon(ui, btn.x + 14.0, btn.y + 14.0, ic);
                    if click {
                        self.set_page(Page::Installations);
                    }
                }
            }
        }
    }

    fn rail(&mut self, ui: &mut Ui, h: f32) {
        let rail = Rect::new(0.0, TOP_BAR, RAIL_W, (h - TOP_BAR).max(0.0));
        ui.c.rect(rail, 0.0, hex(CHROME_BG, 1.0));
        ui.c.rect(
            Rect::new(RAIL_W - 1.0, TOP_BAR, 1.0, rail.h),
            0.0,
            hex(HAIRLINE, 1.0),
        );

        let mut y = TOP_BAR + 14.0;
        for (page, _) in RAIL_PAGES {
            let r = Rect::new(9.0, y, 36.0, 36.0);
            let (hover, click) = self.clickable(ui, r);
            let active = self.page == page && (page != Page::Installations || self.edit.is_none());
            if active {
                ui.c.rect(r, 9.0, hex(RAIL_ACTIVE_BG, 1.0));
                ui.c.ring(r, 9.0, 1.3, hex(P.accent, 0.95));
            } else if hover {
                ui.c.rect(r, 9.0, hex(P.plate_soft, 1.0));
                ui.c.ring(r, 9.0, 1.0, hex(CARD_BORDER, 1.0));
            }
            let ic = hex(
                if active {
                    COPPER_LIGHT
                } else if hover {
                    P.ink
                } else {
                    P.muted
                },
                1.0,
            );
            Self::draw_rail_icon(ui, page, r.x + 18.0, r.y + 18.0, ic);
            if click {
                if page == Page::Installations {
                    self.edit = None;
                }
                self.set_page(page);
            }
            y += 44.0;
        }

        // Bottom: Quick New installation (+) & Settings gear.
        let settings_r = Rect::new(9.0, h - 50.0, 36.0, 36.0);
        let add_r = Rect::new(9.0, h - 94.0, 36.0, 36.0);

        let (add_hover, add_click) = self.clickable(ui, add_r);
        let add_active = self.page == Page::Installations && self.edit.as_ref().is_some_and(|e| e.index.is_none());
        if add_active {
            ui.c.rect(add_r, 9.0, hex(RAIL_ACTIVE_BG, 1.0));
            ui.c.ring(add_r, 9.0, 1.3, hex(P.accent, 0.95));
        } else if add_hover {
            ui.c.rect(add_r, 9.0, hex(P.plate_soft, 1.0));
        }
        Self::draw_plus_icon(
            ui,
            add_r.x + 18.0,
            add_r.y + 18.0,
            hex(if add_active { COPPER_LIGHT } else if add_hover { P.ink } else { P.muted }, 1.0),
        );
        if add_click {
            self.set_page(Page::Installations);
            self.edit = Some(Edit::default());
            self.focus = Some(Field::Name);
        }

        let (set_hover, set_click) = self.clickable(ui, settings_r);
        let set_active = self.page == Page::Settings;
        if set_active {
            ui.c.rect(settings_r, 9.0, hex(RAIL_ACTIVE_BG, 1.0));
            ui.c.ring(settings_r, 9.0, 1.3, hex(P.accent, 0.95));
        } else if set_hover {
            ui.c.rect(settings_r, 9.0, hex(P.plate_soft, 1.0));
        }
        Self::draw_rail_icon(
            ui,
            Page::Settings,
            settings_r.x + 18.0,
            settings_r.y + 18.0,
            hex(if set_active { COPPER_LIGHT } else if set_hover { P.ink } else { P.muted }, 1.0),
        );
        if set_click {
            self.set_page(Page::Settings);
        }
    }

    fn right_panel(&mut self, ui: &mut Ui, panel: Rect) {
        ui.c.rect(panel, 0.0, hex(CHROME_BG, 1.0));
        ui.c.rect(Rect::new(panel.x, panel.y, 1.0, panel.h), 0.0, hex(HAIRLINE, 1.0));

        let x = panel.x + 16.0;
        let w = panel.w - 32.0;
        let y = panel.y + 16.0;

        let fw = ui.text(x, y, "Friends", Face::Medium, 13.0, hex(P.ink, 1.0));
        ui.text(x + fw + 6.0, y + 1.0, "(Offline)", Face::Regular, 12.0, hex(P.muted, 1.0));

        let folder_label = "Open data";
        let fl_w = ui.c.measure(folder_label, Face::Regular, 11.5);
        let folder_r = Rect::new(panel.right() - 16.0 - fl_w - 6.0, y - 3.0, fl_w + 6.0, 22.0);
        let (f_hover, f_click) = self.clickable(ui, folder_r);
        ui.text(
            folder_r.x + 3.0,
            y + 1.0,
            folder_label,
            Face::Regular,
            11.5,
            hex(if f_hover { P.accent } else { P.muted }, 1.0),
        );
        if f_click {
            super::open_folder(&self.paths.data);
        }

        // Calm empty social state in the open middle region (matching Rustic balance).
        let mid_y = panel.y + (panel.h - 130.0) * 0.43;
        Self::draw_friends_icon(ui, panel.x + panel.w / 2.0, mid_y, hex(0x4a4a55, 1.0));
        ui.centred(
            Rect::new(x, mid_y + 18.0, w, 20.0),
            "No friends online yet.",
            Face::Regular,
            12.0,
            hex(P.muted, 1.0),
        );
        let mut ly = mid_y + 40.0;
        for line in ui.c.wrap(
            "Worlds and LAN sessions run offline using your active profile.",
            Face::Regular,
            11.5,
            w - 8.0,
        ) {
            ui.centred(Rect::new(x, ly, w, 16.0), &line, Face::Regular, 11.5, hex(0x565661, 1.0));
            ly += 16.0;
        }

        // Subtle docked Tachyon Engine & Profile status footer.
        let card = Rect::new(x, panel.bottom() - 102.0, w, 86.0);
        ui.c.rect(card, 10.0, hex(P.surface, 1.0));
        ui.c.ring(card, 10.0, 1.0, hex(HAIRLINE, 1.0));
        ui.c.rect(Rect::new(card.x + 12.0, card.y + 16.0, 6.0, 6.0), 3.0, hex(P.accent, 1.0));
        ui.text(card.x + 24.0, card.y + 12.0, "Tachyon Engine", Face::Medium, 12.0, hex(P.ink, 1.0));
        let mod_line = ui.c.fit("Create 6.0.10 · Kinetica · Chronicles", Face::Regular, 11.0, card.w - 24.0);
        ui.text(card.x + 12.0, card.y + 34.0, &mod_line, Face::Regular, 11.0, hex(P.ink_soft, 1.0));
        let inst = &self.store.installations[self.store.selected_index()];
        let res = if inst.width > 0 && inst.height > 0 {
            format!("{}x{}", inst.width, inst.height)
        } else {
            "1280x720".to_owned()
        };
        let meta = ui.c.fit(&format!("{} · {res} · {}", super::BUILD, brand::CONTENT), Face::Mono, 10.0, card.w - 24.0);
        ui.text(card.x + 12.0, card.y + 56.0, &meta, Face::Mono, 10.0, hex(P.muted, 1.0));
    }

    /// The panorama or synthesized sunlit forest-ruin artwork, clipped to `hero`'s rounded frame.
    fn hero_artwork(&self, ui: &mut Ui, hero: Rect) {
        if self.panorama.is_empty() {
            if let Some(bg) = self.backdrop {
                ui.c.image_rounded(bg, hero, inset_uv(BACKDROP_W, BACKDROP_H), 16.0, [1.0; 4]);
            } else {
                ui.c.gradient(hero, 16.0, hex(0x16221b, 1.0), hex(P.page, 1.0));
            }
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
                let src = [
                    e + u0 * (1.0 - 2.0 * e),
                    e,
                    e + u1 * (1.0 - 2.0 * e),
                    1.0 - e,
                ];
                ui.c.image_in_frame(
                    self.panorama[i % n],
                    Rect::new(x0, hero.y, x1 - x0, hero.h),
                    src,
                    hero,
                    16.0,
                    [1.0; 4],
                );
            }
            fx += f;
            i += 1;
        }
    }

    fn play_page(&mut self, ui: &mut Ui, content: Rect) {
        let hero = hero_rect(content);
        let gx = hero.x;
        let gy = content.y + 16.0;

        // Greeting row above Hero Showcase: "Welcome back, [avatar] <Player> v".
        let w1 = ui.text(gx, gy, "Welcome ", Face::Regular, 14.0, hex(P.ink, 1.0));
        let w2 = ui.text(gx + w1, gy, "back,", Face::Medium, 14.0, hex(P.accent, 1.0));
        let account = self.store.account().cloned();
        let pill_x = gx + w1 + w2 + 8.0;
        let name_text = account.as_ref().map_or("Select player", |a| a.name.as_str());
        let name_fit = ui.c.fit(name_text, Face::Medium, 13.5, 180.0);
        let name_w = ui.c.measure(&name_fit, Face::Medium, 13.5);
        let profile_r = Rect::new(pill_x, gy - 4.0, name_w + 46.0, 26.0);
        let (p_hover, p_click) = self.clickable(ui, profile_r);
        if p_hover {
            ui.c.rect(profile_r, 8.0, hex(P.plate_soft, 1.0));
        }
        self.avatar(ui, account.as_ref(), Rect::new(pill_x + 4.0, gy - 1.0, 20.0, 20.0));
        ui.text(pill_x + 30.0, gy, &name_fit, Face::Medium, 13.5, hex(P.ink, 1.0));
        Self::chevron(ui, profile_r.right() - 8.0, gy + 9.0, false, hex(P.muted, 1.0));
        if p_click {
            self.set_page(Page::Accounts);
        }

        let spec = "Horizon 26.3  ·  Create 6.0.10";
        let sw = ui.c.measure(spec, Face::Mono, 10.5);
        if hero.w - (profile_r.right() - hero.x) > sw + 24.0 {
            ui.text(hero.right() - sw, gy + 3.0, spec, Face::Mono, 10.5, hex(P.muted, 1.0));
        }

        // Rounded Hero Showcase Card with artwork + subtle cinematic vignette + 1px hairline border.
        self.hero_artwork(ui, hero);
        ui.c.gradient(
            Rect::new(hero.x, hero.y, hero.w, 64.0),
            16.0,
            hex(CHROME_BG, 0.52),
            hex(CHROME_BG, 0.0),
        );
        let bottom_fade = (hero.h * 0.45).min(200.0);
        ui.c.gradient(
            Rect::new(hero.x, hero.bottom() - bottom_fade, hero.w, bottom_fade),
            16.0,
            hex(CHROME_BG, 0.0),
            hex(CHROME_BG, 0.72),
        );
        ui.c.ring(hero, 16.0, 1.0, hex(CARD_BORDER, 0.95));

        // Centered Frosted Launch / Stop Pill + installation dropdown chevron.
        let installation = self.store.installations[self.store.selected_index()].clone();
        let running = self.game.is_some();
        let ready = self.paths.resources.is_some();
        let pill = hero_play_pill(hero);
        let action_area = Rect::new(pill.x, pill.y, pill.w - 40.0, pill.h);
        let chevron_area = Rect::new(pill.right() - 40.0, pill.y, 40.0, pill.h);

        let action_hover = (ready || running) && ui.hovered(action_area);
        let chev_hover = ui.hovered(chevron_area);
        if action_hover || chev_hover {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        let bg_color = if action_hover || chev_hover || self.menu {
            HERO_PILL_HOVER
        } else {
            HERO_PILL_BG
        };
        ui.c.rect(pill, 13.0, hex(bg_color, 0.90));
        ui.c.ring(
            pill,
            13.0,
            1.0,
            hex(
                if action_hover || chev_hover || self.menu {
                    P.accent
                } else {
                    HERO_PILL_RING
                },
                if action_hover || self.menu { 0.85 } else { 0.9 },
            ),
        );
        // Subtle vertical divider inside the frosted pill before chevron.
        ui.c.rect(
            Rect::new(chevron_area.x, pill.y + 14.0, 1.0, pill.h - 28.0),
            0.0,
            hex(0xffffff, 0.08),
        );

        let title = if running {
            "STOP GAME"
        } else if !ready {
            "RESOURCES MISSING"
        } else {
            "PLAY GAME"
        };
        let text_box = Rect::new(pill.x + 12.0, pill.y + 8.0, pill.w - 52.0, 28.0);
        ui.centred(
            text_box,
            title,
            Face::Medium,
            16.5,
            hex(if running { P.ink } else if ready { P.ink } else { P.muted }, 1.0),
        );
        let sub_text = format!("{}  (Horizon 26.3)", installation.name);
        let sub_fit = ui.c.fit(&sub_text, Face::Regular, 11.0, pill.w - 56.0);
        ui.centred(
            Rect::new(pill.x + 12.0, pill.y + 34.0, pill.w - 52.0, 18.0),
            &sub_fit,
            Face::Regular,
            11.0,
            hex(P.ink_soft, 0.85),
        );

        Self::chevron(
            ui,
            chevron_area.x + chevron_area.w / 2.0,
            pill.y + pill.h / 2.0,
            self.menu,
            hex(if chev_hover || self.menu { P.accent } else { P.ink_soft }, 1.0),
        );

        if ui.tapped(chevron_area) {
            self.menu = !self.menu;
        } else if ui.tapped(action_area) {
            if running {
                self.stop_game();
            } else {
                self.play(self.store.selected_index());
            }
        }

        // Continue line + amber/emerald status badge centered beneath frosted pill.
        let last = if installation.last_played > 0 {
            format!("Played {}", store::ago(installation.last_played, store::now()))
        } else {
            "Ready to build".to_owned()
        };
        let cont = format!("Continue : {}  ·  {last}", installation.name);
        let cont_fit = ui.c.fit(&cont, Face::Regular, 12.0, hero.w - 48.0);
        ui.centred(
            Rect::new(hero.x + 16.0, pill.bottom() + 12.0, hero.w - 32.0, 20.0),
            &cont_fit,
            Face::Regular,
            12.0,
            hex(P.ink, 0.92),
        );

        let badge_text = if running {
            "Game running"
        } else if ready {
            "Ready to play"
        } else {
            "Resources needed"
        };
        let bw = ui.c.measure(badge_text, Face::Medium, 11.5) + 32.0;
        let badge = Rect::new(hero.x + (hero.w - bw) / 2.0, pill.bottom() + 38.0, bw, 24.0);
        let (bbg, bring, bdot, btxt) = if running {
            (BADGE_GREEN_BG, BADGE_GREEN_RING, P.success, 0xc3f3d6)
        } else if ready {
            (BADGE_AMBER_BG, BADGE_AMBER_RING, P.accent, COPPER_LIGHT)
        } else {
            (0x221212, 0x4b2121, P.danger, 0xf2a0a0)
        };
        ui.c.rect(badge, 12.0, hex(bbg, 0.92));
        ui.c.ring(badge, 12.0, 1.0, hex(bring, 0.95));
        ui.c.rect(
            Rect::new(badge.x + 10.0, badge.y + 9.0, 6.0, 6.0),
            3.0,
            hex(bdot, 1.0),
        );
        ui.text(badge.x + 21.0, badge.y + 4.5, badge_text, Face::Medium, 11.5, hex(btxt, 1.0));

        // Hero bottom corner controls (Accounts pill & Edit installation sliders pill).
        let acc_btn = Rect::new(hero.x + 16.0, hero.bottom() - 44.0, 88.0, 28.0);
        let (ab_hover, ab_click) = self.clickable(ui, acc_btn);
        ui.c.rect(
            acc_btn,
            8.0,
            hex(if ab_hover { HERO_PILL_HOVER } else { HERO_PILL_BG }, 0.84),
        );
        ui.c.ring(
            acc_btn,
            8.0,
            1.0,
            hex(if ab_hover { P.accent } else { CARD_BORDER }, 0.9),
        );
        ui.centred(acc_btn, "Accounts", Face::Medium, 11.5, hex(P.ink_soft, 1.0));
        if ab_click {
            self.set_page(Page::Accounts);
        }

        let edit_btn = Rect::new(hero.right() - 46.0, hero.bottom() - 44.0, 30.0, 28.0);
        let (eb_hover, eb_click) = self.clickable(ui, edit_btn);
        ui.c.rect(
            edit_btn,
            8.0,
            hex(if eb_hover { HERO_PILL_HOVER } else { HERO_PILL_BG }, 0.84),
        );
        ui.c.ring(
            edit_btn,
            8.0,
            1.0,
            hex(if eb_hover { P.accent } else { CARD_BORDER }, 0.9),
        );
        Self::draw_sliders_icon(
            ui,
            edit_btn.x + 15.0,
            edit_btn.y + 14.0,
            hex(if eb_hover { COPPER_LIGHT } else { P.ink_soft }, 1.0),
        );
        if eb_click {
            let idx = self.store.selected_index();
            self.set_page(Page::Installations);
            self.edit = Some(Edit::of(idx, &installation));
            self.focus = Some(Field::Name);
        }

        // Bottom Patch Notes strip below Hero.
        let strip_y = hero.bottom() + 18.0;
        let nw = ui.text(hero.x, strip_y, "Patch notes", Face::Medium, 13.0, hex(P.ink, 1.0));
        ui.text(hero.x + nw + 6.0, strip_y + 1.0, "(3 highlights)", Face::Regular, 12.0, hex(P.muted, 1.0));

        let more_label = "All updates >";
        let mw = ui.c.measure(more_label, Face::Regular, 12.0);
        let more_r = Rect::new(hero.right() - mw - 6.0, strip_y - 3.0, mw + 6.0, 22.0);
        let (m_hover, m_click) = self.clickable(ui, more_r);
        ui.text(
            more_r.x + 3.0,
            strip_y + 0.5,
            more_label,
            Face::Regular,
            12.0,
            hex(if m_hover { P.accent } else { P.muted }, 1.0),
        );
        if m_click {
            self.set_page(Page::Notes);
        }

        let card_y = strip_y + 26.0;
        let card_h = (content.bottom() - card_y - 14.0).clamp(64.0, 92.0);
        let gap = 12.0;
        let cols = 3usize.min(NOTES.len());
        let card_w = (hero.w - gap * (cols as f32 - 1.0)) / cols as f32;
        for (i, (title, date, points)) in NOTES.iter().take(cols).enumerate() {
            let card = Rect::new(hero.x + i as f32 * (card_w + gap), card_y, card_w, card_h);
            let (hover, click) = self.clickable(ui, card);
            ui.c.rect(
                card,
                10.0,
                hex(if hover { P.plate_soft } else { P.surface }, 1.0),
            );
            ui.c.ring(
                card,
                10.0,
                1.0,
                hex(if hover { P.accent } else { HAIRLINE }, if hover { 0.7 } else { 1.0 }),
            );
            let meta = date.to_uppercase();
            let meta_fit = ui.c.fit(&meta, Face::Mono, 10.0, card.w - 24.0);
            ui.text(
                card.x + 12.0,
                card.y + 11.0,
                &meta_fit,
                Face::Mono,
                10.0,
                hex(if i == 0 { P.accent } else { P.muted }, 1.0),
            );
            let title_fit = ui.c.fit(title, Face::Medium, 12.5, card.w - 24.0);
            ui.text(card.x + 12.0, card.y + 28.0, &title_fit, Face::Medium, 12.5, hex(P.ink, 1.0));
            if let Some(first) = points.first() {
                let desc = ui.c.fit(first, Face::Regular, 11.0, card.w - 24.0);
                ui.text(card.x + 12.0, card.y + 50.0, &desc, Face::Regular, 11.0, hex(P.muted, 1.0));
            }
            if click {
                self.set_page(Page::Notes);
            }
        }
    }

    /// The installation popover anchored beneath the Hero's frosted Play pill.
    fn menu_rects(&self, content: Rect) -> (Rect, Vec<Rect>) {
        let hero = hero_rect(content);
        let pill = hero_play_pill(hero);
        let item_h = 50.0;
        let max_items = ((hero.bottom() - pill.bottom() - 24.0) / item_h)
            .floor()
            .max(2.0) as usize;
        let n = (self.store.installations.len() + 1).min(max_items.max(3));
        let frame_h = n as f32 * item_h + 14.0;
        let frame_w = 340.0f32.min(hero.w - 32.0);
        let fx = hero.x + (hero.w - frame_w) / 2.0;
        let fy = (pill.bottom() + 8.0).min(content.bottom() - frame_h - 16.0);
        let frame = Rect::new(fx, fy, frame_w, frame_h);
        let items = (0..n)
            .map(|i| {
                Rect::new(
                    frame.x + 7.0,
                    frame.y + 7.0 + i as f32 * item_h,
                    frame.w - 14.0,
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
        let shadow = Rect::new(frame.x - 4.0, frame.y + 4.0, frame.w + 8.0, frame.h + 12.0);
        ui.c.rect(shadow, 16.0, hex(0x000000, 0.55));
        ui.c.rect(frame, 12.0, hex(P.surface, 1.0));
        ui.c.ring(frame, 12.0, 1.0, hex(CARD_BORDER, 1.0));
        let last = items.len() - 1;
        let selected = self.store.selected_index();
        for (i, r) in items.iter().enumerate() {
            let hover = ui.hovered(*r);
            if hover {
                self.wanted_cursor = CursorIcon::Pointer;
                ui.c.rect(*r, 8.0, hex(P.plate_soft, 1.0));
            }
            if i == last {
                ui.c.rect(Rect::new(r.x + 6.0, r.y, r.w - 12.0, 1.0), 0.0, hex(HAIRLINE, 1.0));
                ui.text(
                    r.x + 14.0,
                    r.y + 16.0,
                    "Manage installations...",
                    Face::Medium,
                    13.0,
                    hex(P.accent, 1.0),
                );
                continue;
            }
            let Some(installation) = self.store.installations.get(i).cloned() else {
                continue;
            };
            if i == selected && !hover {
                ui.c.rect(*r, 8.0, hex(RAIL_ACTIVE_BG, 0.65));
            }
            Self::draw_emblem(ui, Rect::new(r.x + 10.0, r.y + 11.0, 28.0, 28.0));
            let name = ui.c.fit(&installation.name, Face::Medium, 13.5, r.w - 86.0);
            ui.text(r.x + 48.0, r.y + 7.0, &name, Face::Medium, 13.5, hex(P.ink, 1.0));
            let folder = installation.folder(&self.paths.data).display().to_string();
            let folder = ui.c.fit(&folder, Face::Regular, 11.0, r.w - 86.0);
            ui.text(r.x + 48.0, r.y + 26.0, &folder, Face::Regular, 11.0, hex(P.muted, 1.0));
            if i == selected {
                ui.c.rect(
                    Rect::new(r.right() - 20.0, r.y + r.h / 2.0 - 4.0, 8.0, 8.0),
                    4.0,
                    hex(P.accent, 1.0),
                );
            }
        }
    }

    /// Over everything while a file is dragged over the window.
    fn drop_overlay(&self, ui: &mut Ui, w: f32, h: f32) {
        let installation = &self.store.installations[self.store.selected_index()].name;
        ui.c.rect(Rect::new(0.0, 0.0, w, h), 0.0, hex(P.page, 0.86));
        let card_w = 620.0f32.min(w - 48.0);
        let lines = ui.c.wrap(KINDS, Face::Regular, 12.5, card_w - 64.0);
        let card_h = 186.0 + lines.len().max(1) as f32 * 18.0;
        let card = Rect::new((w - card_w) / 2.0, (h - card_h) / 2.0, card_w, card_h);
        ui.c.rect(card, 16.0, hex(P.surface, 0.98));
        ui.c.ring(card, 16.0, 1.5, hex(P.accent, 0.92));
        ui.centred(
            Rect::new(card.x + 24.0, card.y + 34.0, card.w - 48.0, 32.0),
            &format!("Drop to import into {installation}"),
            Face::Medium,
            21.0,
            hex(P.ink, 1.0),
        );
        let mut ly = card.y + 78.0;
        for line in lines {
            ui.centred(
                Rect::new(card.x + 32.0, ly, card.w - 64.0, 18.0),
                &line,
                Face::Regular,
                12.5,
                hex(P.ink_soft, 1.0),
            );
            ly += 18.0;
        }
        let mut bx = card.x + (card.w - (64.0 * 3.0 + 16.0)) / 2.0;
        for ext in [".ZIP", ".HMOD", ".JAR"] {
            let tag = Rect::new(bx, ly + 18.0, 64.0, 26.0);
            ui.c.rect(tag, 7.0, hex(RAIL_ACTIVE_BG, 1.0));
            ui.c.ring(tag, 7.0, 1.0, hex(P.accent, 0.55));
            ui.centred(tag, ext, Face::Mono, 11.0, hex(COPPER_LIGHT, 1.0));
            bx += 72.0;
        }
    }

    fn installations_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        if self.edit.is_some() {
            return self.edit_page(ui, content);
        }
        let x = content.x + 32.0;
        let w = (content.w - 64.0).min(920.0);
        let top = content.y + self.scroll;
        let mut y = top + 28.0;

        ui.text(x, y, "Installations", Face::Medium, 21.0, hex(P.ink, 1.0));
        let new = Rect::new(x + w - 168.0, y - 4.0, 168.0, 36.0);
        if self.button(ui, new, "New installation", true, true) {
            self.edit = Some(Edit::default());
            self.focus = Some(Field::Name);
        }
        y += 32.0;
        ui.text(
            x,
            y,
            "Each installation isolates its worlds, options, resource packs, and ported mods.",
            Face::Regular,
            13.0,
            hex(P.muted, 1.0),
        );
        y += 30.0;

        // Drop zone card.
        let zone = Rect::new(x, y, w, 64.0);
        ui.c.rect(
            zone,
            12.0,
            hex(if self.dropping { RAIL_ACTIVE_BG } else { P.surface }, 1.0),
        );
        ui.c.ring(
            zone,
            12.0,
            1.0,
            hex(if self.dropping { P.accent } else { CARD_BORDER }, 1.0),
        );
        let selected_name = self.store.installations[self.store.selected_index()]
            .name
            .clone();
        ui.text(
            zone.x + 18.0,
            zone.y + 12.0,
            &format!("Drop a file anywhere here to import into {selected_name}"),
            Face::Medium,
            13.5,
            hex(P.ink, 1.0),
        );
        let kinds = ui.c.fit(KINDS, Face::Regular, 12.0, zone.w - 36.0);
        ui.text(zone.x + 18.0, zone.y + 35.0, &kinds, Face::Regular, 12.0, hex(P.muted, 1.0));
        y += 80.0;

        // Modrinth fetch cards (Create & Create Aeronautics).
        Self::label(ui, x, y, "MODRINTH CONTENT");
        y += 20.0;
        let home = self.store.installations[self.store.selected_index()].folder(&self.paths.data);
        for (n, f) in fetch::ALL.into_iter().enumerate() {
            let have = fetch::installed(&f, &home);
            let asking = self.fetch_ask == Some(n);
            let card = Rect::new(x, y, w, 64.0);
            ui.c.rect(card, 12.0, hex(P.surface, 1.0));
            ui.c.ring(card, 12.0, 1.0, hex(HAIRLINE, 1.0));
            let what = if f.inner.is_empty() {
                "Its blocks, items, recipes, textures and sounds"
            } else {
                "Its blocks, items and recipes (contraptions expand separately)"
            };
            let (title, detail) = if asking {
                (f.ask(), "Kept locally so it works offline; Java mod code never executes.".to_owned())
            } else if have {
                (format!("{} installed in {selected_name}", f.name), format!("{what} imported from its Modrinth jar."))
            } else {
                (
                    format!("Get {} into {selected_name}", f.name),
                    format!("{what} downloaded directly from Modrinth."),
                )
            };
            let room = card.w - 36.0 - 220.0;
            let title = ui.c.fit(&title, Face::Medium, 13.5, room);
            ui.text(card.x + 18.0, card.y + 13.0, &title, Face::Medium, 13.5, hex(P.ink, 1.0));
            let detail = ui.c.fit(&detail, Face::Regular, 12.0, room);
            ui.text(card.x + 18.0, card.y + 35.0, &detail, Face::Regular, 12.0, hex(P.muted, 1.0));
            let free = self.importing.is_none();
            let by = card.y + (card.h - 34.0) / 2.0;
            if asking {
                let cancel = Rect::new(card.right() - 16.0 - 86.0, by, 86.0, 34.0);
                if self.button(ui, cancel, "Cancel", false, true) {
                    self.fetch_ask = None;
                }
                let download = Rect::new(cancel.x - 8.0 - 106.0, by, 106.0, 34.0);
                if self.button(ui, download, "Download", true, free) {
                    self.fetch_ask = None;
                    self.get_fetch(f);
                }
            } else {
                let label = if have { "Re-sync" } else { "Get" };
                let get = Rect::new(card.right() - 16.0 - 104.0, by, 104.0, 34.0);
                if self.button(ui, get, label, !have, free) {
                    self.fetch_ask = Some(n);
                }
            }
            y += 74.0;
        }

        y += 10.0;
        Self::label(ui, x, y, "PROFILES");
        y += 20.0;
        let now = store::now();
        let selected = self.store.selected_index();
        let installations = self.store.installations.clone();
        for (i, installation) in installations.iter().enumerate() {
            let row = Rect::new(x, y, w, 74.0);
            let hover = ui.hovered(row);
            let is_sel = i == selected;
            ui.c.rect(
                row,
                12.0,
                hex(if hover { P.plate_soft } else { P.surface }, 1.0),
            );
            ui.c.ring(
                row,
                12.0,
                1.0,
                hex(if is_sel { P.accent } else { HAIRLINE }, if is_sel { 0.65 } else { 1.0 }),
            );
            if is_sel {
                ui.c.rect(Rect::new(row.x + 1.0, row.y + 14.0, 3.0, row.h - 28.0), 1.5, hex(P.accent, 1.0));
            }
            Self::draw_emblem(ui, Rect::new(row.x + 18.0, row.y + 16.0, 40.0, 40.0));
            let tx = row.x + 72.0;
            let buttons = 78.0 + 8.0 + 72.0 + 8.0 + 82.0 + 16.0 + 16.0;
            let room = (row.right() - buttons - tx).max(80.0);
            let name = ui.c.fit(&installation.name, Face::Medium, 15.0, room);
            let nw = ui.text(tx, row.y + 15.0, &name, Face::Medium, 15.0, hex(P.ink, 1.0));
            if is_sel && tx + nw + 80.0 < row.right() - buttons {
                let pill = Rect::new(tx + nw + 10.0, row.y + 16.0, 72.0, 20.0);
                ui.c.rect(pill, 6.0, hex(RAIL_ACTIVE_BG, 1.0));
                ui.c.ring(pill, 6.0, 1.0, hex(P.accent, 0.55));
                ui.centred(pill, "ACTIVE", Face::Mono, 10.0, hex(COPPER_LIGHT, 1.0));
            }
            let played = if installation.last_played > 0 {
                store::ago(installation.last_played, now)
            } else {
                "Never played".to_owned()
            };
            let folder = installation.folder(&self.paths.data).display().to_string();
            let detail = format!("Horizon 26.3  ·  {played}  ·  {folder}");
            let detail = ui.c.fit(&detail, Face::Regular, 12.0, room);
            ui.text(tx, row.y + 42.0, &detail, Face::Regular, 12.0, hex(P.muted, 1.0));
            let by = row.y + (row.h - 34.0) / 2.0;
            let mut bx = row.right() - 16.0;
            bx -= 82.0;
            if self.button(ui, Rect::new(bx, by, 82.0, 34.0), "Folder", false, true) {
                super::open_folder(&installation.folder(&self.paths.data));
            }
            bx -= 72.0 + 8.0;
            if self.button(ui, Rect::new(bx, by, 72.0, 34.0), "Edit", false, true) {
                self.edit = Some(Edit::of(i, installation));
            }
            bx -= 78.0 + 8.0;
            let can_play = self.game.is_none() && self.paths.resources.is_some();
            if self.button(ui, Rect::new(bx, by, 78.0, 34.0), "Play", true, can_play) {
                self.play(i);
            }
            y += 84.0;
        }
        y - top + 24.0
    }

    fn edit_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 32.0;
        let w = (content.w - 64.0).min(680.0);
        let top = content.y + self.scroll;
        let mut y = top + 28.0;
        let index = self.edit.as_ref().and_then(|e| e.index);
        let existing = index.and_then(|i| self.store.installations.get(i)).cloned();
        let title = if existing.is_some() {
            "Edit installation"
        } else {
            "New installation"
        };
        ui.text(x, y, title, Face::Medium, 21.0, hex(P.ink, 1.0));
        y += 42.0;

        let panel = Rect::new(x, y, w, 392.0);
        ui.c.rect(panel, 14.0, hex(P.surface, 1.0));
        ui.c.ring(panel, 14.0, 1.0, hex(HAIRLINE, 1.0));

        let fx = panel.x + 24.0;
        let fw = panel.w - 48.0;
        let mut fy = panel.y + 24.0;
        Self::label(ui, fx, fy, "NAME");
        self.field(
            ui,
            Rect::new(fx, fy + 18.0, fw.min(440.0), 42.0),
            Field::Name,
            "Unnamed installation",
        );
        fy += 82.0;
        Self::label(ui, fx, fy, "GAME DIRECTORY");
        let own = existing.clone().unwrap_or_else(|| Installation {
            id: self.store.new_id(),
            ..Installation::default()
        });
        let default = own.own_folder(&self.paths.data).display().to_string();
        self.field(ui, Rect::new(fx, fy + 18.0, fw, 42.0), Field::Dir, &default);
        ui.text(
            fx,
            fy + 68.0,
            "Leave empty for an isolated folder inside Tachyon's directory.",
            Face::Regular,
            12.0,
            hex(P.muted, 1.0),
        );
        fy += 106.0;
        Self::label(ui, fx, fy, "WINDOW RESOLUTION");
        self.field(ui, Rect::new(fx, fy + 18.0, 120.0, 42.0), Field::Width, "Auto");
        ui.text(fx + 132.0, fy + 29.0, "x", Face::Mono, 14.0, hex(P.muted, 1.0));
        self.field(ui, Rect::new(fx + 152.0, fy + 18.0, 120.0, 42.0), Field::Height, "Auto");
        ui.text(fx + 286.0, fy + 31.0, "Default: 1280 x 720", Face::Regular, 12.0, hex(P.muted, 1.0));

        fy += 96.0;
        if self.button(ui, Rect::new(fx, fy, 112.0, 38.0), "Save", true, true) {
            self.save_edit();
        }
        if self.button(ui, Rect::new(fx + 122.0, fy, 100.0, 38.0), "Cancel", false, true) {
            self.edit = None;
            self.focus = None;
        }
        if let (Some(i), Some(installation)) = (index, &existing) {
            if self.button(ui, Rect::new(fx + 232.0, fy, 122.0, 38.0), "Open folder", false, true) {
                super::open_folder(&installation.folder(&self.paths.data));
            }
            if installation.id != store::DEFAULT_ID
                && self.danger_button(ui, Rect::new(fx + fw - 108.0, fy, 108.0, 38.0), "Delete")
            {
                let name = installation.name.clone();
                if i < self.store.installations.len() {
                    self.store.remove_installation(&installation.id);
                }
                self.edit = None;
                self.focus = None;
                self.save();
                self.note(
                    format!("{name} removed from list. Its folder and worlds stay on disk."),
                    false,
                );
            }
        }
        y + panel.h + 24.0 - top
    }

    fn accounts_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 32.0;
        let w = (content.w - 64.0).min(860.0);
        let top = content.y + self.scroll;
        let mut y = top + 28.0;
        ui.text(x, y, "Accounts", Face::Medium, 21.0, hex(P.ink, 1.0));
        y += 34.0;
        let about = "Offline accounts define your player name, UUID, and default skin for singleplayer worlds and offline/LAN servers.";
        for line in ui.c.wrap(about, Face::Regular, 13.0, w) {
            ui.text(x, y, &line, Face::Regular, 13.0, hex(P.muted, 1.0));
            y += 20.0;
        }
        y += 16.0;
        let accounts = self.store.accounts.clone();
        if accounts.is_empty() {
            let r = Rect::new(x, y, w, 64.0);
            ui.c.rect(r, 12.0, hex(P.surface, 1.0));
            ui.c.ring(r, 12.0, 1.0, hex(HAIRLINE, 1.0));
            ui.text(
                r.x + 20.0,
                r.y + 22.0,
                "No account yet: add a player name below to play.",
                Face::Regular,
                13.5,
                hex(P.muted, 1.0),
            );
            y += 74.0;
        }
        let mut remove = None;
        for account in &accounts {
            let row = Rect::new(x, y, w, 72.0);
            let hover = ui.hovered(row);
            let is_active = account.name == self.store.active;
            ui.c.rect(
                row,
                12.0,
                hex(if hover { P.plate_soft } else { P.surface }, 1.0),
            );
            ui.c.ring(
                row,
                12.0,
                1.0,
                hex(if is_active { P.accent } else { HAIRLINE }, if is_active { 0.65 } else { 1.0 }),
            );
            self.avatar(ui, Some(account), Rect::new(row.x + 18.0, row.y + 16.0, 40.0, 40.0));
            let tx = row.x + 74.0;
            ui.text(tx, row.y + 15.0, &account.name, Face::Medium, 15.0, hex(P.ink, 1.0));
            let detail = format!("Offline  ·  {}", account.uuid_text());
            ui.text(tx, row.y + 40.0, &detail, Face::Mono, 11.0, hex(P.muted, 1.0));
            let by = row.y + (row.h - 34.0) / 2.0;
            let remove_r = Rect::new(row.right() - 16.0 - 92.0, by, 92.0, 34.0);
            if self.button(ui, remove_r, "Remove", false, true) {
                remove = Some(account.name.clone());
            }
            let use_r = Rect::new(remove_r.x - 8.0 - 82.0, by, 82.0, 34.0);
            if is_active {
                ui.c.rect(use_r, 8.0, hex(RAIL_ACTIVE_BG, 1.0));
                ui.c.ring(use_r, 8.0, 1.0, hex(P.accent, 0.55));
                ui.centred(use_r, "ACTIVE", Face::Mono, 10.5, hex(COPPER_LIGHT, 1.0));
            } else if self.button(ui, use_r, "Use", true, true) {
                self.store.active = account.name.clone();
                self.save();
            }
            y += 82.0;
        }
        if let Some(name) = remove {
            self.store.remove_account(&name);
            self.save();
        }
        y += 14.0;
        Self::label(ui, x, y, "ADD AN OFFLINE ACCOUNT");
        self.field(ui, Rect::new(x, y + 18.0, 320.0, 42.0), Field::Account, "Player name");
        let valid = store::valid_name(self.account_input.trim());
        if self.button(ui, Rect::new(x + 332.0, y + 18.0, 96.0, 42.0), "Add", true, valid) {
            self.add_account();
        }
        ui.text(x, y + 68.0, "3 to 16 letters, digits or _.", Face::Regular, 12.0, hex(P.muted, 1.0));
        y += 108.0;
        y - top
    }

    fn notes_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 32.0;
        let w = (content.w - 64.0).min(820.0);
        let top = content.y + self.scroll;
        let mut y = top + 28.0;
        ui.text(x, y, "Patch notes", Face::Medium, 21.0, hex(P.ink, 1.0));
        y += 42.0;
        for (i, (title, date, points)) in NOTES.iter().enumerate() {
            let lines: Vec<Vec<String>> = points
                .iter()
                .map(|p| ui.c.wrap(p, Face::Regular, 13.5, w - 60.0))
                .collect();
            let body: f32 = lines.iter().map(|l| l.len() as f32 * 20.0 + 8.0).sum();
            let card = Rect::new(x, y, w, 72.0 + body + 12.0);
            ui.c.rect(card, 14.0, hex(P.surface, 1.0));
            ui.c.ring(
                card,
                14.0,
                1.0,
                hex(if i == 0 { P.accent } else { HAIRLINE }, if i == 0 { 0.45 } else { 1.0 }),
            );
            let date = date.to_uppercase();
            Self::label(ui, card.x + 22.0, card.y + 20.0, &date);
            if i == 0 {
                let dw = ui.c.measure(&date, Face::Mono, 11.0);
                let pill = Rect::new(card.x + 22.0 + dw + 10.0, card.y + 17.0, 58.0, 20.0);
                ui.c.rect(pill, 6.0, hex(RAIL_ACTIVE_BG, 1.0));
                ui.c.ring(pill, 6.0, 1.0, hex(P.accent, 0.55));
                ui.centred(pill, "LATEST", Face::Mono, 10.0, hex(COPPER_LIGHT, 1.0));
            }
            ui.text(card.x + 22.0, card.y + 40.0, title, Face::Medium, 17.0, hex(P.ink, 1.0));
            let mut ly = card.y + 72.0;
            for point in lines {
                ui.c.rect(Rect::new(card.x + 24.0, ly + 7.0, 5.0, 5.0), 2.5, hex(P.accent, 1.0));
                for line in point {
                    ui.text(card.x + 40.0, ly, &line, Face::Regular, 13.5, hex(P.ink_soft, 1.0));
                    ly += 20.0;
                }
                ly += 8.0;
            }
            y += card.h + 14.0;
        }
        y - top + 16.0
    }

    fn settings_page(&mut self, ui: &mut Ui, content: Rect) -> f32 {
        let x = content.x + 32.0;
        let w = (content.w - 64.0).min(860.0);
        let top = content.y + self.scroll;
        let mut y = top + 28.0;
        ui.text(x, y, "Settings", Face::Medium, 21.0, hex(P.ink, 1.0));
        y += 42.0;

        let card = Rect::new(x, y, w, 5.0 * 74.0 + 104.0 + 24.0);
        ui.c.rect(card, 14.0, hex(P.surface, 1.0));
        ui.c.ring(card, 14.0, 1.0, hex(HAIRLINE, 1.0));

        let rx = card.x + 22.0;
        let rw = card.w - 44.0;
        let mut ry = card.y + 8.0;

        let r = Rect::new(rx, ry, rw, 74.0);
        ui.row(
            r,
            1,
            "Keep the launcher open while playing",
            "Otherwise it hides while the game runs and returns when the game exits.",
            60.0,
        );
        if ui.hovered(r) {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        if let Some(on) = ui.switch(r, self.store.keep_open) {
            self.store.keep_open = on;
            self.save();
        }
        ry += 74.0;

        let r = Rect::new(rx, ry, rw, 74.0);
        ui.row(
            r,
            2,
            "Moving hero background",
            "Slowly pan the title panorama behind the Home showcase card.",
            60.0,
        );
        if ui.hovered(r) {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        if let Some(on) = ui.switch(r, !self.store.still) {
            self.store.still = !on;
            self.save();
        }
        ry += 74.0;

        let r = Rect::new(rx, ry, rw, 74.0);
        let data = self.paths.data.display().to_string();
        ui.row(r, 3, "Tachyon's folder", &data, 100.0);
        if self.row_button(ui, r, "Open") {
            super::open_folder(&self.paths.data);
        }
        ry += 74.0;

        let r = Rect::new(rx, ry, rw, 74.0);
        let resources = match &self.paths.resources {
            Some(dir) => format!("Minecraft 26.3 prepared: {}", dir.display()),
            None => "Not found. Place resources beside Tachyon.exe or set TACHYON_RESOURCES.".to_owned(),
        };
        ui.row(r, 4, "Resources", &resources, 0.0);
        ry += 74.0;

        let r = Rect::new(rx, ry, rw, 74.0);
        ui.row(
            r,
            5,
            "Launcher log",
            "Diagnostic log written to logs/latest.log inside Tachyon's folder.",
            100.0,
        );
        if self.row_button(ui, r, "Open") {
            super::open_folder(&self.paths.data.join("logs"));
        }
        ry += 74.0;

        let r = Rect::new(rx, ry, rw, 92.0);
        let about = format!(
            "{} {}. Built on Horizon (horizon-rust/horizon, MIT). Not an official Minecraft product; not affiliated with Mojang or Microsoft.",
            brand::NAME,
            super::BUILD
        );
        ui.row(r, 6, "About", &about, 0.0);
        card.bottom() + 24.0 - top
    }

    fn row_button(&mut self, ui: &mut Ui, row: Rect, label: &str) -> bool {
        let w = ui.button_width(label);
        let r = Rect::new(row.right() - w, row.y + (row.h - 34.0) / 2.0, w, 34.0);
        self.button(ui, r, label, false, true)
    }

    fn note_rect(&mut self, ui: &mut Ui, content: Rect) -> Option<Rect> {
        let note = self.note.as_ref()?;
        if !note.error && note.at.elapsed() > Duration::from_secs(6) {
            self.note = None;
            return None;
        }
        let w = 420.0f32.min(content.w - 48.0);
        let lines =
            ui.c.wrap(&note.text, Face::Regular, 13.5, w - 60.0)
                .len()
                .max(1);
        let h = 26.0 + lines as f32 * 20.0;
        Some(Rect::new(
            content.right() - w - 24.0,
            content.bottom() - h - 18.0,
            w,
            h,
        ))
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
        let shadow = Rect::new(r.x - 4.0, r.y + 3.0, r.w + 8.0, r.h + 10.0);
        ui.c.rect(shadow, 14.0, hex(0x000000, 0.5));
        ui.c.rect(r, 11.0, hex(P.surface, 1.0));
        ui.c.ring(r, 11.0, 1.0, hex(CARD_BORDER, 1.0));
        ui.c.rect(Rect::new(r.x + 12.0, r.y + 13.0, 3.0, r.h - 26.0), 1.5, hex(color, 1.0));
        let mut y = r.y + 13.0;
        for line in ui.c.wrap(&text, Face::Regular, 13.5, r.w - 60.0) {
            ui.text(r.x + 26.0, y, &line, Face::Regular, 13.5, hex(P.ink, 1.0));
            y += 20.0;
        }
        ui.text(r.right() - 24.0, r.y + 11.0, "×", Face::Regular, 15.0, hex(P.muted, 1.0));
    }
}

const AMBER_TEXT_HEX: u32 = 0xe89b5a;

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
            && let Some(w) = &self.window
        {
            match Renderer::new(w.clone()) {
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
        let scale = self
            .window
            .as_ref()
            .map_or(1.0, |w| w.scale_factor() as f32);
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
            WindowEvent::HoveredFile(_) => self.dropping = true,
            WindowEvent::HoveredFileCancelled => self.dropping = false,
            WindowEvent::DroppedFile(file) => {
                self.dropping = false;
                self.import(file);
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
        self.watch_import();
        let shown = self.game.is_none() || self.store.keep_open;
        if shown && (!self.occluded || self.snapshots.is_some()) {
            event_loop.set_control_flow(ControlFlow::Wait);
            if let Some(w) = &self.window {
                w.request_redraw();
            }
        } else {
            event_loop.set_control_flow(ControlFlow::WaitUntil(
                Instant::now() + Duration::from_millis(250),
            ));
        }
    }
}
