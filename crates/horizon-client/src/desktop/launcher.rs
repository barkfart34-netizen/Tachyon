//! Tachyon's desktop launcher: a Deep Space & Cherenkov Cyan 3-pane workspace
//! with a left icon rail, top navigation bar, rounded hero showcase card, quiet
//! social/engine side panel, and clean sub-pages for installations, accounts,
//! patch notes, and settings. Accounts are offline player names; the game runs
//! as a child process in the selected installation's folder.
use super::store::{self, Account, Installation, Store};
use super::{ERROR_FILE, GAME, ICON, Paths};
use crate::tachyon::brand::{self, CYAN_GLOW, P, SPECULAR, VIOLET};
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

const TOP_BAR: f32 = 46.0;
const RAIL_W: f32 = 56.0;
const PANEL_WIDE: f32 = 248.0;
const PANEL_NARROW: f32 = 212.0;

/// Hairline border & tonal Deep Space surface tokens for desktop chrome.
const CHROME_BG: u32 = P.page;
const HAIRLINE: u32 = P.rule;
const CARD_BORDER: u32 = SPECULAR;
const CARD_HOVER: u32 = 0x131b30;
const RAIL_ACTIVE_BG: u32 = 0x0e2238;
const HERO_PILL_BG: u32 = 0x091020;
const HERO_PILL_HOVER: u32 = 0x101e38;
const HERO_PILL_RING: u32 = 0x2b4673;
const BADGE_CYAN_BG: u32 = 0x0b1d33;
const BADGE_CYAN_RING: u32 = 0x1e4a73;
const BADGE_GREEN_BG: u32 = 0x091d1b;
const BADGE_GREEN_RING: u32 = 0x1b4c42;
const BADGE_WARN_BG: u32 = 0x1e1326;
const BADGE_WARN_RING: u32 = 0x4e2b4b;
const WARN_TEXT: u32 = 0xf4a9bf;
const NAV_DISABLED: u32 = 0x2f3b59;
const EMPTY_ICON: u32 = 0x3f4f73;
const EMPTY_HINT: u32 = 0x5c6e94;

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
        "Horizon 26.3 & Cherenkov UI",
        "November 15, 2026",
        &[
            "Synced Tachyon with Horizon 26.3 rendering, shader 0.2, and native engine hooks.",
            "Redesigned the desktop launcher in a Deep Space & Cherenkov Cyan 3-pane workspace with atmospheric showcase.",
            "Added Fluid Valve, Smart Fluid Pipe, Hose Pulley, Item Vault, Redstone Link, and brass diodes to the Create 6.0.10 port.",
        ],
    ),
    (
        "Create, Kinetica & Chronicles",
        "November 8, 2026",
        &[
            "Kinetic rotation network, mechanical arms, deployers, saws, and belts in Rust.",
            "Chronicles quest progression, Orbital Laser integration, and Modrinth drop-in asset importing for Create mods.",
            "Each installation isolates its worlds, options, resource packs, and ported mods inside its own folder.",
        ],
    ),
    (
        "Mobile & Touch Profiles",
        "October 29, 2026",
        &[
            "Java Edition 26.3 running natively on Windows & Android with Zalith touch profiles.",
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
    let top_y = content.y + 52.0;
    let footer_reserve = 160.0;
    let h = (content.bottom() - top_y - footer_reserve).clamp(280.0, 480.0);
    Rect::new(content.x + pad_x, top_y, (content.w - pad_x * 2.0).max(200.0), h)
}

/// Centered frosted play capsule inside `hero`.
fn hero_play_pill(hero: Rect, wide: bool) -> Rect {
    let w = (if wide { 304.0f32 } else { 276.0f32 }).min(hero.w - 48.0);
    let h = 64.0;
    let cx = hero.x + (hero.w - w) / 2.0;
    let cy = hero.y + hero.h * 0.53 - h / 2.0;
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

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn sd_box(px: f32, py: f32, cx: f32, cy: f32, hx: f32, hy: f32) -> f32 {
    let dx = (px - cx).abs() - hx;
    let dy = (py - cy).abs() - hy;
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)
}

/// Anti-aliased kinetic cogwheel mask `(fill, rim)` in aspect-corrected space.
fn cog_mask(ax: f32, ay: f32, cx: f32, cy: f32, radius: f32, teeth: f32, phase: f32) -> (f32, f32) {
    let dx = ax - cx;
    let dy = ay - cy;
    let r = dx.hypot(dy);
    let ang = dy.atan2(dx) + phase;
    let tooth = (ang * teeth).cos().clamp(-0.35, 0.35) * (radius * 0.085);
    let outer = radius + tooth;
    let inner = radius * 0.70;
    let hub_outer = radius * 0.28;
    let hub_inner = radius * 0.13;
    let rim_ring = (1.0 - smoothstep(outer - 0.004, outer + 0.004, r))
        * smoothstep(inner - 0.004, inner + 0.004, r);
    let hub_ring = (1.0 - smoothstep(hub_outer - 0.004, hub_outer + 0.004, r))
        * smoothstep(hub_inner - 0.004, hub_inner + 0.004, r);
    let spoke_ang = ((ang * 3.0).sin().abs() * r).min(1.0);
    let spokes = (1.0 - smoothstep(radius * 0.032, radius * 0.045, spoke_ang))
        * smoothstep(hub_outer - 0.01, hub_outer, r)
        * (1.0 - smoothstep(inner, inner + 0.01, r));
    let fill = rim_ring.max(hub_ring).max(spokes);
    let edge = (1.0 - smoothstep(0.0, 0.006, (r - outer).abs())).max(
        1.0 - smoothstep(0.0, 0.005, (r - inner).abs()),
    ) * fill;
    (fill, edge)
}

/// Synthesizes an atmospheric Deep Space & Kinetic-Arcane sanctuary showcase
/// (`BACKDROP_W` x `BACKDROP_H` RGBA) used when Minecraft panorama resources
/// are not yet present.
fn generate_hero_backdrop() -> Vec<u8> {
    let (w, h) = (BACKDROP_W, BACKDROP_H);
    let aspect = w as f32 / h as f32;
    let mut scene = vec![[0.0f32; 3]; (w * h) as usize];

    let hash2 = |ix: i32, iy: i32| -> f32 {
        let mut n = ix.wrapping_mul(374_761_393) ^ iy.wrapping_mul(668_265_263);
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
        smooth(x, y) * 0.55
            + smooth(x * 2.13 + 7.3, y * 2.13 - 3.1) * 0.29
            + smooth(x * 4.45 - 2.7, y * 4.45 + 5.9) * 0.16
    };

    const BAYER4: [f32; 16] = [
        0.0 / 16.0, 8.0 / 16.0, 2.0 / 16.0, 10.0 / 16.0,
        12.0 / 16.0, 4.0 / 16.0, 14.0 / 16.0, 6.0 / 16.0,
        3.0 / 16.0, 11.0 / 16.0, 1.0 / 16.0, 9.0 / 16.0,
        15.0 / 16.0, 7.0 / 16.0, 13.0 / 16.0, 5.0 / 16.0,
    ];

    // Warm amber hanging lanterns + cool Cherenkov cyan wisps: (u, top_y, orb_y, warm)
    const LANTERNS: [(f32, f32, f32, bool); 4] = [
        (0.285, 0.230, 0.325, true),
        (0.715, 0.165, 0.265, true),
        (0.365, 0.0, 0.545, false),
        (0.640, 0.0, 0.515, false),
    ];

    // Pass 1: Twilight sky, distant voxel ridges, kinetic cogs, sanctuary portal & light shafts.
    for py in 0..h {
        let v = py as f32 / h as f32;
        for px in 0..w {
            let u = px as f32 / w as f32;
            let ax = (u - 0.5) * aspect;
            let ay = v - 0.5;

            // Twilight deep-space sky fading into arcane violet & horizon teal mist.
            let sky_t = smoothstep(0.0, 0.68, v);
            let mut r = 0.024 + sky_t * 0.040;
            let mut g = 0.038 + sky_t * 0.115;
            let mut b = 0.088 + sky_t * 0.175;

            // Arcane violet nebula wisps in upper sky & right flank.
            let neb = fbm(u * 3.6 + 1.4, v * 2.8 + 0.7);
            let neb_mask = smoothstep(0.42, 0.85, neb) * (1.0 - smoothstep(0.35, 0.72, v));
            r += neb_mask * 0.14;
            g += neb_mask * 0.05;
            b += neb_mask * 0.26;

            // Crisp O(1) pinpoint starfield in upper sky.
            if v < 0.48 {
                let gx = (u * 56.0).floor() as i32;
                let gy = (v * 32.0).floor() as i32;
                let star_p = hash2(gx, gy);
                if star_p > 0.74 {
                    let sx = (gx as f32 + 0.2 + hash2(gx + 19, gy) * 0.6) / 56.0;
                    let sy = (gy as f32 + 0.2 + hash2(gx, gy + 37) * 0.6) / 32.0;
                    let d_px = ((u - sx) * w as f32).hypot((v - sy) * h as f32);
                    let fade_v = 1.0 - smoothstep(0.18, 0.46, v);
                    let twinkle = (-d_px * d_px * 1.55).exp() * fade_v * (0.45 + 0.55 * star_p);
                    let tint = hash2(gx + 5, gy + 11);
                    r += twinkle * (0.72 + tint * 0.28);
                    g += twinkle * (0.86 + tint * 0.10);
                    b += twinkle * 1.0;
                }
            }

            // Luminous Cherenkov-cyan + violet horizon mist glow behind central archway.
            let portal_d2 = ((u - 0.50) / 0.24).powi(2) + ((v - 0.39) / 0.21).powi(2);
            let portal_mist = (-portal_d2 * 1.55).exp();
            r += portal_mist * 0.105;
            g += portal_mist * 0.285;
            b += portal_mist * 0.405;

            // Distant stepped-silhouette mountain ridges in atmospheric fog.
            let u_vox = (u * 38.0).floor() / 38.0;
            let u_far = u * 0.55 + u_vox * 0.45;
            let far_h = 0.47 - 0.11 * smooth(u_far * 4.5 + 1.2, 2.0) + 0.04 * (u - 0.5).abs();
            let far_cov = smoothstep(far_h - 0.004, far_h + 0.004, v);
            if far_cov > 0.0 {
                let mist = smoothstep(0.40, 0.68, v) * 0.55;
                let mr = 0.036 + mist * 0.035;
                let mg = 0.056 + mist * 0.100;
                let mb = 0.125 + mist * 0.135;
                r = r * (1.0 - far_cov) + mr * far_cov;
                g = g * (1.0 - far_cov) + mg * far_cov;
                b = b * (1.0 - far_cov) + mb * far_cov;
            }

            let mid_h = 0.55 - 0.07 * smooth(u_far * 7.5 - 3.0, 5.0) + 0.03 * ((u - 0.5) * 2.0).powi(2);
            let mid_cov = smoothstep(mid_h - 0.004, mid_h + 0.004, v);
            if mid_cov > 0.0 {
                let rim = (1.0 - smoothstep(0.0, 0.012, (v - mid_h).abs())) * 0.12;
                let mr = 0.024 + rim * 0.2;
                let mg = 0.038 + rim * 0.65;
                let mb = 0.085 + rim * 0.85;
                r = r * (1.0 - mid_cov) + mr * mid_cov;
                g = g * (1.0 - mid_cov) + mg * mid_cov;
                b = b * (1.0 - mid_cov) + mb * mid_cov;
            }

            // Monumental kinetic cogwheels perched inside and flanking the sanctuary arch.
            let (c1, e1) = cog_mask(ax, ay, 0.0, -0.140, 0.215, 16.0, 0.18);
            let (c2, e2) = cog_mask(ax, ay, -0.32, -0.220, 0.115, 12.0, -0.25);
            let cog_fill = (c1 * 0.52 + c2 * 0.36).min(0.62);
            if cog_fill > 0.0 {
                let cr = 0.052 + e1 * 0.06 + e2 * 0.15;
                let cg = 0.086 + e1 * 0.28 + e2 * 0.12;
                let cb = 0.155 + e1 * 0.38 + e2 * 0.30;
                r = r * (1.0 - cog_fill) + cr * cog_fill;
                g = g * (1.0 - cog_fill) + cg * cog_fill;
                b = b * (1.0 - cog_fill) + cb * cog_fill;
            }

            // Grand stepped basalt sanctuary archway with staggered ashlar stones & inner cyan edge glow.
            let d_left = sd_box(u, v, 0.322, 0.425, 0.028, 0.255);
            let d_right = sd_box(u, v, 0.678, 0.425, 0.028, 0.255);
            let d_lintel = sd_box(u, v, 0.500, 0.162, 0.228, 0.032);
            let d_cap = sd_box(u, v, 0.500, 0.128, 0.190, 0.014);
            let d_arch = d_left.min(d_right).min(d_lintel).min(d_cap);
            let arch_cov = 1.0 - smoothstep(-0.0025, 0.0025, d_arch);
            if arch_cov > 0.0 {
                let row = (v * 30.0).floor() as i32;
                let stagger = if row % 2 == 0 { 0.0 } else { 0.5 };
                let u_brick = u * 34.0 + stagger;
                let v_brick = v * 30.0;
                let bx = u_brick.floor() as i32;
                let by = row;
                let seam = (u_brick.fract() < 0.055 || v_brick.fract() < 0.065) as i32 as f32;
                let stone_var = hash2(bx, by) * 0.016 - seam * 0.012;
                let rim = 1.0 - smoothstep(0.0, 0.008, d_arch.abs());
                let portal_face = ((u - 0.5).abs() < 0.160 && v > 0.175) as i32 as f32;
                let inner_jamb = ((u - 0.349).abs() < 0.0038 || (u - 0.651).abs() < 0.0038)
                    && (0.20..0.41).contains(&v);
                let rune = if inner_jamb && (py / 5) % 2 == 0 { 0.72 } else { 0.0 };
                let ar = 0.034 + stone_var + rim * (0.040 + portal_face * 0.05) + rune * 0.14;
                let ag = 0.048 + stone_var + rim * (0.120 + portal_face * 0.16) + rune * 0.48;
                let ab = 0.092 + stone_var + rim * (0.200 + portal_face * 0.24) + rune * 0.62;
                r = r * (1.0 - arch_cov) + ar * arch_cov;
                g = g * (1.0 - arch_cov) + ag * arch_cov;
                b = b * (1.0 - arch_cov) + ab * arch_cov;
            }

            // Volumetric diagonal Cherenkov-cyan light shafts streaming from upper-left.
            let proj = (u - 0.06) * 0.77 - v * 0.63;
            let b1 = (-((proj - 0.02) / 0.072).powi(2)).exp();
            let b2 = (-((proj - 0.14) / 0.038).powi(2)).exp() * 0.68;
            let b3 = (-((proj + 0.09) / 0.044).powi(2)).exp() * 0.45;
            let beam_fade = (1.0 - v * 0.55).max(0.08) * (1.0 - smoothstep(0.55, 0.96, u));
            let wisp = 0.68 + 0.32 * fbm(u * 4.5 + v * 1.4, v * 4.5);
            let shaft = (b1 + b2 + b3) * beam_fade * wisp * 0.23;
            r += shaft * 0.34;
            g += shaft * 0.82;
            b += shaft * 1.04;

            // Subtle glowing portal core pulse illuminating the upper-center sanctuary.
            let core = (-(((u - 0.50) / 0.16).powi(2) + ((v - 0.37) / 0.15).powi(2)) * 2.1).exp();
            r += core * 0.095;
            g += core * 0.260;
            b += core * 0.385;

            // Hanging amber lanterns & floating cyan arcane wisps.
            for (lx, y0, cy, warm) in LANTERNS {
                let dx = (u - lx) * aspect;
                if warm && dx.abs() < 0.0022 && (y0..cy - 0.020).contains(&v) && py % 3 != 0 {
                    r = 0.12;
                    g = 0.14;
                    b = 0.22;
                }
                let d_orb = dx.hypot(v - cy);
                if warm {
                    let housing = 1.0 - smoothstep(0.011, 0.015, sd_box(dx, v - cy, 0.0, 0.0, 0.007, 0.012));
                    let flame = 1.0 - smoothstep(0.004, 0.008, d_orb);
                    let halo = (-d_orb * d_orb / (0.032 * 0.032)).exp() * 0.46
                        + (-d_orb * d_orb / (0.095 * 0.095)).exp() * 0.13;
                    r = r * (1.0 - housing * 0.7) + halo * 1.00 + flame * 1.05;
                    g = g * (1.0 - housing * 0.7) + halo * 0.56 + flame * 0.78;
                    b = b * (1.0 - housing * 0.7) + halo * 0.16 + flame * 0.34;
                } else {
                    let wisp_core = (-d_orb * d_orb / (0.007 * 0.007)).exp() * 0.85;
                    let wisp_halo = (-d_orb * d_orb / (0.045 * 0.045)).exp() * 0.26;
                    r += wisp_core * 0.48 + wisp_halo * 0.18;
                    g += wisp_core * 0.95 + wisp_halo * 0.56;
                    b += wisp_core * 1.00 + wisp_halo * 0.80;
                }
            }

            scene[(py * w + px) as usize] = [r, g, b];
        }
    }

    // Pass 2: Reflective twilight obsidian pool, framing ruins, foreground motes & vignette.
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for py in 0..h {
        let v = py as f32 / h as f32;
        for px in 0..w {
            let u = px as f32 / w as f32;
            let [mut r, mut g, mut b] = scene[(py * w + px) as usize];

            let horizon = 0.680 + 0.014 * smooth(u * 6.0, 3.0);
            if v >= horizon {
                let dv = ((v - horizon) / (1.0 - horizon)).clamp(0.0, 1.0);
                let ripple = (v * 200.0 + fbm(u * 16.0, v * 34.0) * 6.5).sin() * (0.002 + dv * 0.014);
                let su = (u + ripple).clamp(0.0, 0.999);
                let sv = (horizon - (v - horizon) * 0.90 - ripple.abs() * 0.3).clamp(0.0, 0.999);
                let rx = ((su * w as f32) as u32).min(w - 1);
                let ry = ((sv * h as f32) as u32).min(h - 1);
                let [rr, rg, rb] = scene[(ry * w + rx) as usize];

                let fresnel = 0.34 + 0.36 * (1.0 - dv).powf(1.5);
                let depth_fade = 1.0 - dv * 0.62;
                let glint = smoothstep(0.55, 0.95, (u * 28.0 + fbm(u * 12.0, v * 48.0) * 4.0).sin())
                    * (-((u - 0.50) / 0.25).powi(2)).exp()
                    * (1.0 - dv * 0.7)
                    * 0.14;
                r = (0.016 + rr * fresnel) * depth_fade + glint * 0.35;
                g = (0.028 + rg * fresnel) * depth_fade + glint * 0.85;
                b = (0.062 + rb * fresnel) * depth_fade + glint * 1.00;

                // Subtle shoreline rim along the water's edge.
                let shore = 1.0 - smoothstep(0.0, 0.008, v - horizon);
                r += shore * 0.03;
                g += shore * 0.09;
                b += shore * 0.14;
            }

            // Crisp stepped obsidian framing ledges on far left & right edges.
            let left_step = horizon - 0.22 + ((u * 14.0).floor() / 14.0) * 0.45;
            let right_step = horizon - 0.26 + (((1.0 - u) * 14.0).floor() / 14.0) * 0.48;
            let in_left_fg = u < 0.135 && v > left_step;
            let in_right_fg = u > 0.865 && v > right_step;
            if in_left_fg || in_right_fg {
                let bx = (px / 5) as i32;
                let by = (py / 5) as i32;
                let tex = hash2(bx, by) * 0.018;
                let top_dist = if in_left_fg { (v - left_step).abs() } else { (v - right_step).abs() };
                let cyan_rim = (1.0 - smoothstep(0.0, 0.009, top_dist)) * 0.16;
                r = 0.022 + tex + cyan_rim * 0.20;
                g = 0.032 + tex + cyan_rim * 0.75;
                b = 0.066 + tex + cyan_rim * 1.00;
            }

            // Soft upper-corner canopy haze framing without hard barcode bars.
            let corner_haze = ((0.18 - u).max(0.0) / 0.18).max((u - 0.82).max(0.0) / 0.18)
                * (1.0 - smoothstep(0.0, 0.22, v));
            if corner_haze > 0.0 {
                let leaf = 0.5 + 0.5 * fbm(u * 14.0, v * 14.0);
                let mask = (corner_haze * (0.45 + 0.55 * leaf)).clamp(0.0, 0.88);
                r = r * (1.0 - mask) + 0.020 * mask;
                g = g * (1.0 - mask) + 0.032 * mask;
                b = b * (1.0 - mask) + 0.068 * mask;
            }

            // Subtle radial & edge vignette settling seamlessly into #070a14.
            let vx = ((u - 0.5).abs() * 2.0).powf(2.4);
            let vy = ((v - 0.48).abs() * 2.0).powf(2.4);
            let vig = (1.0 - (vx * 0.40 + vy * 0.42)).clamp(0.18, 1.0);
            r = r * vig + 0.027 * (1.0 - vig);
            g = g * vig + 0.039 * (1.0 - vig);
            b = b * vig + 0.078 * (1.0 - vig);

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

    /// Geometric Cherenkov + Violet Tachyon `T` emblem matching `icon-64.png`.
    fn draw_emblem(ui: &mut Ui, r: Rect) {
        let rad = r.w * 0.25;
        ui.c.gradient(r, rad, hex(0x101830, 1.0), hex(0x070a14, 1.0));
        ui.c.ring(r, rad, 1.0, hex(P.accent, 0.45));

        // Top violet-to-cyan crossbar.
        let pad_x = r.w * 0.16;
        let bar_y = r.y + r.h * 0.24;
        let bar_h = (r.h * 0.145).max(2.2);
        ui.c.gradient_across(
            Rect::new(r.x + pad_x, bar_y, r.w - pad_x * 2.0, bar_h),
            (bar_h * 0.25).max(0.5),
            hex(VIOLET, 1.0),
            hex(P.accent, 1.0),
        );

        // Ice-white vertical stem.
        let stem_w = (r.w * 0.165).max(2.4);
        let stem_y = bar_y + bar_h + (r.h * 0.025);
        ui.c.rect(
            Rect::new(r.x + (r.w - stem_w) / 2.0, stem_y, stem_w, r.h * 0.41),
            0.8,
            hex(P.ink, 0.96),
        );

        // Three Cherenkov speed streaks to the left of the stem.
        let stem_left = r.x + (r.w - stem_w) / 2.0;
        let tick_h = (r.h * 0.055).max(1.2);
        for (idx, (len_frac, dy_frac)) in [(0.21, 0.10), (0.15, 0.23), (0.09, 0.34)].iter().enumerate() {
            let tw = r.w * len_frac;
            let ty = stem_y + r.h * dy_frac;
            let alpha = 0.95 - idx as f32 * 0.15;
            ui.c.gradient_across(
                Rect::new(stem_left - r.w * 0.06 - tw, ty, tw, tick_h),
                tick_h * 0.5,
                hex(P.accent, 0.15),
                hex(CYAN_GLOW, alpha),
            );
        }
    }

    fn chevron(ui: &mut Ui, cx: f32, cy: f32, up: bool, color: [f32; 4]) {
        for i in 0..5 {
            let t = i as f32 * 0.9;
            let dy = if up { 1.8 - t * 0.85 } else { -1.8 + t * 0.85 };
            ui.c.rect(Rect::new(cx - 3.6 + t - 0.7, cy + dy - 0.7, 1.5, 1.5), 0.6, color);
            ui.c.rect(Rect::new(cx + 3.6 - t - 0.8, cy + dy - 0.7, 1.5, 1.5), 0.6, color);
        }
    }

    fn draw_arrow_left(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        for i in 0..5 {
            let x = cx - 3.0 + i as f32 * 1.1;
            let dy = i as f32 * 1.1;
            ui.c.rect(Rect::new(x, cy - dy - 0.7, 1.5, 1.5), 0.5, color);
            ui.c.rect(Rect::new(x, cy + dy - 0.7, 1.5, 1.5), 0.5, color);
        }
    }

    fn draw_arrow_right(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        for i in 0..5 {
            let x = cx + 2.0 - i as f32 * 1.1;
            let dy = i as f32 * 1.1;
            ui.c.rect(Rect::new(x, cy - dy - 0.7, 1.5, 1.5), 0.5, color);
            ui.c.rect(Rect::new(x, cy + dy - 0.7, 1.5, 1.5), 0.5, color);
        }
    }

    fn draw_rail_icon(ui: &mut Ui, page: Page, cx: f32, cy: f32, color: [f32; 4]) {
        match page {
            Page::Play => {
                // Clean house outline + gable roof.
                for i in 0..6 {
                    let s = i as f32 * 1.25;
                    ui.c.rect(Rect::new(cx - 0.8 - s, cy - 6.6 + i as f32 * 1.0, 1.8, 1.5), 0.3, color);
                    ui.c.rect(Rect::new(cx - 0.8 + s, cy - 6.6 + i as f32 * 1.0, 1.8, 1.5), 0.3, color);
                }
                ui.c.ring(Rect::new(cx - 5.5, cy - 1.5, 11.0, 8.0), 1.6, 1.4, color);
                ui.c.rect(Rect::new(cx - 1.5, cy + 2.3, 3.0, 4.1), 0.5, color);
            }
            Page::Installations => {
                // Stacked module / cube grid icon.
                ui.c.ring(Rect::new(cx - 6.5, cy - 6.5, 5.5, 5.5), 1.4, 1.3, color);
                ui.c.ring(Rect::new(cx + 1.0, cy - 6.5, 5.5, 5.5), 1.4, 1.3, color);
                ui.c.ring(Rect::new(cx - 6.5, cy + 1.0, 5.5, 5.5), 1.4, 1.3, color);
                ui.c.ring(Rect::new(cx + 1.0, cy + 1.0, 5.5, 5.5), 1.4, 1.3, color);
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
                // Clean 8-tooth precision cog gear + axle hub.
                ui.c.ring(Rect::new(cx - 4.6, cy - 4.6, 9.2, 9.2), 4.6, 1.6, color);
                ui.c.ring(Rect::new(cx - 1.6, cy - 1.6, 3.2, 3.2), 1.6, 1.2, color);
                for k in 0..8 {
                    let ang = k as f32 * std::f32::consts::FRAC_PI_4;
                    let tx = cx + ang.cos() * 5.8 - 1.1;
                    let ty = cy + ang.sin() * 5.8 - 1.1;
                    ui.c.rect(Rect::new(tx, ty, 2.2, 2.2), 0.7, color);
                }
            }
        }
    }

    fn draw_plus_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 6.5, cy - 6.5, 13.0, 13.0), 3.0, 1.25, color);
        ui.c.rect(Rect::new(cx - 3.5, cy - 0.75, 7.0, 1.5), 0.5, color);
        ui.c.rect(Rect::new(cx - 0.75, cy - 3.5, 1.5, 7.0), 0.5, color);
    }

    fn draw_folder_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.rect(Rect::new(cx - 6.0, cy - 5.5, 5.2, 2.2), 0.7, color);
        ui.c.ring(Rect::new(cx - 6.0, cy - 3.5, 12.0, 9.0), 1.6, 1.3, color);
    }

    fn draw_bell_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 4.5, cy - 5.5, 9.0, 8.5), 3.2, 1.3, color);
        ui.c.rect(Rect::new(cx - 6.0, cy + 2.5, 12.0, 1.4), 0.5, color);
        ui.c.rect(Rect::new(cx - 1.5, cy + 4.5, 3.0, 1.5), 0.7, color);
    }

    fn draw_search_icon(ui: &mut Ui, cx: f32, cy: f32, color: [f32; 4]) {
        ui.c.ring(Rect::new(cx - 5.5, cy - 5.5, 8.6, 8.6), 4.3, 1.4, color);
        for i in 0..7 {
            let d = i as f32 * 0.55;
            ui.c.rect(Rect::new(cx + 1.5 + d, cy + 1.5 + d, 1.5, 1.5), 0.5, color);
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
        ui.c.rule(0.0, TOP_BAR - 1.0, w, hex(HAIRLINE, 1.0));

        // Left: Brand monogram + wordmark + dropdown arrow + nav arrows + breadcrumb.
        let nw = ui.c.measure(brand::NAME, Face::Medium, 13.5);
        let brand_hit = Rect::new(10.0, 8.0, 38.0 + nw + 24.0, 30.0);
        let (brand_hover, brand_click) = self.clickable(ui, brand_hit);
        if brand_hover || (self.menu && self.page == Page::Play) {
            ui.c.rect(brand_hit, 8.0, hex(P.plate_soft, 1.0));
            ui.c.ring(brand_hit, 8.0, 1.0, hex(HAIRLINE, 1.0));
        }
        if brand_click {
            if self.page == Page::Play {
                self.menu = !self.menu;
            } else {
                self.set_page(Page::Play);
            }
        }
        Self::draw_emblem(ui, Rect::new(15.0, 12.0, 22.0, 22.0));
        ui.text(44.0, 14.0, brand::NAME, Face::Medium, 13.5, hex(P.ink, 1.0));
        Self::chevron(
            ui,
            44.0 + nw + 12.0,
            23.0,
            self.menu && self.page == Page::Play,
            hex(if brand_hover { P.accent } else { P.muted }, 1.0),
        );

        let can_back = self.history_pos > 0;
        let can_fwd = self.history_pos + 1 < self.history.len();
        let back_r = Rect::new(brand_hit.right() + 10.0, 10.0, 26.0, 26.0);
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
            back_r.y + 13.0,
            hex(if can_back { P.ink_soft } else { NAV_DISABLED }, 1.0),
        );
        if b_click {
            self.go_history(-1);
        }

        let fwd_r = Rect::new(back_r.right() + 2.0, 10.0, 26.0, 26.0);
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
            fwd_r.y + 13.0,
            hex(if can_fwd { P.ink_soft } else { NAV_DISABLED }, 1.0),
        );
        if f_click {
            self.go_history(1);
        }

        ui.c.rect(Rect::new(fwd_r.right() + 8.0, 16.0, 1.0, 14.0), 0.0, hex(HAIRLINE, 1.0));
        let title = match self.page {
            Page::Play => "Home",
            Page::Installations if self.edit.is_some() => "Installation setup",
            Page::Installations => "Installations",
            Page::Accounts => "Accounts",
            Page::Notes => "Patch notes",
            Page::Settings => "Settings",
        };
        ui.text(fwd_r.right() + 18.0, 15.0, title, Face::Regular, 13.0, hex(P.ink_soft, 1.0));

        // Right cluster: ONE status pill, ONE active player chip, subtle icon actions.
        let mut rx = w - 16.0;

        let running = self.game.is_some();
        let ready = self.paths.resources.is_some();
        let status_label = if self.importing.is_some() {
            "Importing..."
        } else if running {
            "1 instance running"
        } else if ready {
            "Ready"
        } else {
            "Resources missing"
        };
        let status_w = ui.c.measure(status_label, Face::Medium, 12.0) + 34.0;
        rx -= status_w;
        let status_r = Rect::new(rx, 9.0, status_w, 28.0);
        let (s_hover, s_click) = self.clickable(ui, status_r);
        let (s_bg, s_ring, dot_c, text_c) = if self.importing.is_some() {
            (
                if s_hover { 0x102742 } else { BADGE_CYAN_BG },
                BADGE_CYAN_RING,
                P.accent,
                CYAN_GLOW,
            )
        } else if running {
            (
                if s_hover { 0x0f2a26 } else { BADGE_GREEN_BG },
                BADGE_GREEN_RING,
                P.success,
                0x9ff5d0,
            )
        } else if ready {
            (
                if s_hover { 0x102742 } else { BADGE_CYAN_BG },
                BADGE_CYAN_RING,
                P.accent,
                P.ink_soft,
            )
        } else {
            (
                if s_hover { 0x2a1a35 } else { BADGE_WARN_BG },
                BADGE_WARN_RING,
                P.danger,
                WARN_TEXT,
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
            if !ready && !running {
                self.set_page(Page::Settings);
            } else {
                self.set_page(Page::Play);
            }
        }

        rx -= 8.0;
        let account = self.store.account().cloned();
        let acc_name = account
            .as_ref()
            .map_or("Add account", |a| a.name.as_str());
        let acc_fit = ui.c.fit(acc_name, Face::Medium, 12.0, 110.0);
        let acc_w = ui.c.measure(&acc_fit, Face::Medium, 12.0) + 52.0;
        rx -= acc_w;
        let acc_r = Rect::new(rx, 9.0, acc_w, 28.0);
        let (a_hover, a_click) = self.clickable(ui, acc_r);
        ui.c.rect(
            acc_r,
            14.0,
            hex(if a_hover { CARD_HOVER } else { P.surface }, 1.0),
        );
        ui.c.ring(
            acc_r,
            14.0,
            1.0,
            hex(if a_hover { P.accent } else { CARD_BORDER }, if a_hover { 0.65 } else { 1.0 }),
        );
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

        // Small crisp action icons (Folder, Notes, Installations).
        for kind in 0..3 {
            rx -= 34.0;
            let btn = Rect::new(rx, 9.0, 28.0, 28.0);
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
                        ui.c.rect(
                            Rect::new(btn.right() - 7.0, btn.y + 5.0, 4.5, 4.5),
                            2.25,
                            hex(P.accent, 1.0),
                        );
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
            Rect::new(RAIL_W - 1.0 / ui.c.scale, TOP_BAR, 1.0 / ui.c.scale, rail.h),
            0.0,
            hex(HAIRLINE, 1.0),
        );

        let mut y = TOP_BAR + 14.0;
        for (page, _) in RAIL_PAGES {
            let r = Rect::new(9.0, y, 38.0, 38.0);
            let (hover, click) = self.clickable(ui, r);
            let active = self.page == page && (page != Page::Installations || self.edit.is_none());
            if active {
                ui.c.rect(Rect::new(0.0, r.y + 9.0, 3.0, r.h - 18.0), 1.5, hex(P.accent, 1.0));
                ui.c.rect(r, 10.0, hex(RAIL_ACTIVE_BG, 0.95));
                ui.c.ring(r, 10.0, 1.0, hex(P.accent, 0.65));
            } else if hover {
                ui.c.rect(r, 10.0, hex(P.plate_soft, 1.0));
                ui.c.ring(r, 10.0, 1.0, hex(CARD_BORDER, 1.0));
            }
            let ic = hex(
                if active {
                    CYAN_GLOW
                } else if hover {
                    P.ink
                } else {
                    P.muted
                },
                1.0,
            );
            Self::draw_rail_icon(ui, page, r.x + 19.0, r.y + 19.0, ic);
            if click {
                if page == Page::Installations {
                    self.edit = None;
                }
                self.set_page(page);
            }
            y += 46.0;
        }

        // Gentle horizontal divider above pinned bottom actions (+ New & Settings).
        ui.c.rule(16.0, h - 108.0, RAIL_W - 32.0, hex(HAIRLINE, 1.0));
        let settings_r = Rect::new(9.0, h - 52.0, 38.0, 38.0);
        let add_r = Rect::new(9.0, h - 96.0, 38.0, 38.0);

        let (add_hover, add_click) = self.clickable(ui, add_r);
        let add_active = self.page == Page::Installations && self.edit.as_ref().is_some_and(|e| e.index.is_none());
        if add_active {
            ui.c.rect(Rect::new(0.0, add_r.y + 9.0, 3.0, add_r.h - 18.0), 1.5, hex(P.accent, 1.0));
            ui.c.rect(add_r, 10.0, hex(RAIL_ACTIVE_BG, 0.95));
            ui.c.ring(add_r, 10.0, 1.0, hex(P.accent, 0.65));
        } else if add_hover {
            ui.c.rect(add_r, 10.0, hex(P.plate_soft, 1.0));
            ui.c.ring(add_r, 10.0, 1.0, hex(CARD_BORDER, 1.0));
        }
        Self::draw_plus_icon(
            ui,
            add_r.x + 19.0,
            add_r.y + 19.0,
            hex(if add_active { CYAN_GLOW } else if add_hover { P.ink } else { P.muted }, 1.0),
        );
        if add_click {
            self.set_page(Page::Installations);
            self.edit = Some(Edit::default());
            self.focus = Some(Field::Name);
        }

        let (set_hover, set_click) = self.clickable(ui, settings_r);
        let set_active = self.page == Page::Settings;
        if set_active {
            ui.c.rect(Rect::new(0.0, settings_r.y + 9.0, 3.0, settings_r.h - 18.0), 1.5, hex(P.accent, 1.0));
            ui.c.rect(settings_r, 10.0, hex(RAIL_ACTIVE_BG, 0.95));
            ui.c.ring(settings_r, 10.0, 1.0, hex(P.accent, 0.65));
        } else if set_hover {
            ui.c.rect(settings_r, 10.0, hex(P.plate_soft, 1.0));
            ui.c.ring(settings_r, 10.0, 1.0, hex(CARD_BORDER, 1.0));
        }
        Self::draw_rail_icon(
            ui,
            Page::Settings,
            settings_r.x + 19.0,
            settings_r.y + 19.0,
            hex(if set_active { CYAN_GLOW } else if set_hover { P.ink } else { P.muted }, 1.0),
        );
        if set_click {
            self.set_page(Page::Settings);
        }
    }

    fn right_panel(&mut self, ui: &mut Ui, panel: Rect) {
        ui.c.rect(panel, 0.0, hex(CHROME_BG, 1.0));
        ui.c.rect(
            Rect::new(panel.x, panel.y, 1.0 / ui.c.scale, panel.h),
            0.0,
            hex(HAIRLINE, 1.0),
        );

        let x = panel.x + 16.0;
        let w = panel.w - 32.0;
        let y = panel.y + 16.0;

        let fw = ui.text(x, y, "Friends", Face::Medium, 13.5, hex(P.ink, 1.0));
        ui.text(x + fw + 6.0, y + 1.0, "(Offline)", Face::Regular, 12.0, hex(P.muted, 1.0));

        let folder_label = "Open data";
        let fl_w = ui.c.measure(folder_label, Face::Regular, 11.5);
        let folder_r = Rect::new(panel.right() - 16.0 - fl_w - 6.0, y - 2.0, fl_w + 6.0, 22.0);
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

        // Calm empty social state in the open middle region.
        let mid_y = panel.y + (panel.h - 140.0) * 0.43;
        Self::draw_friends_icon(ui, panel.x + panel.w / 2.0, mid_y, hex(EMPTY_ICON, 1.0));
        ui.centred(
            Rect::new(x, mid_y + 18.0, w, 20.0),
            "No friends online yet.",
            Face::Medium,
            12.0,
            hex(P.ink_soft, 0.72),
        );
        let mut ly = mid_y + 42.0;
        for line in ui.c.wrap(
            "Worlds and LAN sessions run offline using your active profile.",
            Face::Regular,
            11.5,
            w - 8.0,
        ) {
            ui.centred(Rect::new(x, ly, w, 16.0), &line, Face::Regular, 11.5, hex(EMPTY_HINT, 1.0));
            ly += 16.0;
        }

        // Refined docked Tachyon Engine summary panel at bottom.
        let card = Rect::new(x, panel.bottom() - 124.0, w, 108.0);
        ui.c.rect(card, 12.0, hex(P.surface, 1.0));
        ui.c.ring(card, 12.0, 1.0, hex(HAIRLINE, 1.0));
        ui.c.gradient_across(
            Rect::new(card.x + 14.0, card.y, 44.0, 2.0),
            1.0,
            hex(P.accent, 0.9),
            hex(VIOLET, 0.9),
        );

        ui.c.rect(Rect::new(card.x + 14.0, card.y + 16.0, 6.0, 6.0), 3.0, hex(P.accent, 1.0));
        ui.text(card.x + 26.0, card.y + 12.0, "Tachyon Engine", Face::Medium, 12.5, hex(P.ink, 1.0));
        let ver = format!("v{}", super::BUILD);
        let vw = ui.c.measure(&ver, Face::Mono, 10.5);
        ui.text(card.right() - 14.0 - vw, card.y + 14.0, &ver, Face::Mono, 10.5, hex(P.muted, 1.0));

        let mod_line = ui.c.fit("Create 6.0.10 · Kinetica · Chronicles", Face::Regular, 11.5, card.w - 28.0);
        ui.text(card.x + 14.0, card.y + 34.0, &mod_line, Face::Regular, 11.5, hex(P.ink_soft, 1.0));
        ui.c.rule(card.x + 14.0, card.y + 58.0, card.w - 28.0, hex(HAIRLINE, 1.0));

        let inst = &self.store.installations[self.store.selected_index()];
        let res = if inst.width > 0 && inst.height > 0 {
            format!("{}x{}", inst.width, inst.height)
        } else {
            "1280x720".to_owned()
        };
        let col_w = (card.w - 36.0) / 2.0;
        ui.text(card.x + 14.0, card.y + 67.0, "WINDOW", Face::Mono, 9.5, hex(P.muted, 1.0));
        let res_fit = ui.c.fit(&res, Face::Mono, 11.0, col_w);
        ui.text(card.x + 14.0, card.y + 82.0, &res_fit, Face::Mono, 11.0, hex(P.ink, 1.0));

        let rx_col = card.x + 14.0 + col_w + 8.0;
        ui.text(rx_col, card.y + 67.0, "CONTENT", Face::Mono, 9.5, hex(P.muted, 1.0));
        let pack_fit = ui.c.fit(brand::CONTENT, Face::Mono, 11.0, col_w);
        ui.text(rx_col, card.y + 82.0, &pack_fit, Face::Mono, 11.0, hex(CYAN_GLOW, 1.0));
    }

    /// The panorama or synthesized Deep Space & Kinetic-Arcane artwork, clipped to `hero`'s rounded frame.
    fn hero_artwork(&self, ui: &mut Ui, hero: Rect) {
        if self.panorama.is_empty() {
            if let Some(bg) = self.backdrop {
                ui.c.image_rounded(bg, hero, inset_uv(BACKDROP_W, BACKDROP_H), 16.0, [1.0; 4]);
            } else {
                ui.c.gradient(hero, 16.0, hex(0x0c1732, 1.0), hex(P.page, 1.0));
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
                    [0.80, 0.87, 0.98, 1.0],
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

        // Clean greeting row: "Welcome back, <avatar> Steve" on left; modpack summary on right.
        let w1 = ui.text(gx, gy, "Welcome ", Face::Regular, 14.5, hex(P.ink, 1.0));
        let w2 = ui.text(gx + w1, gy, "back,", Face::Medium, 14.5, hex(P.accent, 1.0));
        let account = self.store.account().cloned();
        let px = gx + w1 + w2 + 9.0;
        let name_text = account.as_ref().map_or("Select player", |a| a.name.as_str());
        let name_fit = ui.c.fit(name_text, Face::Medium, 14.5, 180.0);
        let name_w = ui.c.measure(&name_fit, Face::Medium, 14.5);
        let profile_r = Rect::new(px, gy - 3.0, 26.0 + name_w + 4.0, 25.0);
        let (p_hover, p_click) = self.clickable(ui, profile_r);
        self.avatar(ui, account.as_ref(), Rect::new(px, gy - 1.0, 20.0, 20.0));
        ui.text(
            px + 27.0,
            gy,
            &name_fit,
            Face::Medium,
            14.5,
            hex(if p_hover { CYAN_GLOW } else { P.ink }, 1.0),
        );
        if p_click {
            self.set_page(Page::Accounts);
        }

        let spec = "Horizon 26.3 · Create + Arcane";
        let sw = ui.c.measure(spec, Face::Mono, 11.0);
        if hero.w - (profile_r.right() - hero.x) > sw + 24.0 {
            ui.text(hero.right() - sw, gy + 2.5, spec, Face::Mono, 11.0, hex(P.ink_soft, 0.72));
        }

        // Commanding Hero Showcase Card with hairline specular rim #253358.
        self.hero_artwork(ui, hero);
        ui.c.gradient(
            Rect::new(hero.x, hero.y, hero.w, 64.0),
            16.0,
            hex(CHROME_BG, 0.45),
            hex(CHROME_BG, 0.0),
        );
        let bottom_fade = (hero.h * 0.42).min(190.0);
        ui.c.gradient(
            Rect::new(hero.x, hero.bottom() - bottom_fade, hero.w, bottom_fade),
            16.0,
            hex(CHROME_BG, 0.0),
            hex(CHROME_BG, 0.68),
        );
        ui.c.ring(hero, 16.0, 1.0, hex(CARD_BORDER, 0.98));
        ui.c.rect(
            Rect::new(hero.x + 20.0, hero.y + 1.0, hero.w - 40.0, 1.0),
            0.0,
            hex(CYAN_GLOW, 0.14),
        );

        // Centered Frosted-Glass CTA Block + integrated instance selector chevron.
        let installation = self.store.installations[self.store.selected_index()].clone();
        let running = self.game.is_some();
        let ready = self.paths.resources.is_some();
        let pill = hero_play_pill(hero, !ready && !running);
        let action_area = Rect::new(pill.x, pill.y, pill.w - 44.0, pill.h);
        let chevron_area = Rect::new(pill.right() - 44.0, pill.y, 44.0, pill.h);

        let action_hover = ui.hovered(action_area);
        let chev_hover = ui.hovered(chevron_area);
        if action_hover || chev_hover {
            self.wanted_cursor = CursorIcon::Pointer;
        }
        let lit = action_hover || chev_hover || self.menu;
        let glow_color = if running { P.success } else { P.accent };
        ui.c.glow(
            pill,
            14.0,
            hex(glow_color, if lit { 0.24 } else { 0.13 }),
            16.0,
        );
        let bg_color = if lit { HERO_PILL_HOVER } else { HERO_PILL_BG };
        ui.c.rect(pill, 14.0, hex(bg_color, 0.88));
        ui.c.rect(
            Rect::new(pill.x + 14.0, pill.y + 1.0, pill.w - 28.0, 1.0),
            0.0,
            hex(CYAN_GLOW, if lit { 0.36 } else { 0.20 }),
        );
        ui.c.ring(
            pill,
            14.0,
            1.2,
            hex(
                if running {
                    P.success
                } else if lit {
                    P.accent
                } else {
                    HERO_PILL_RING
                },
                if lit { 0.95 } else { 0.85 },
            ),
        );
        ui.c.rect(
            Rect::new(chevron_area.x, pill.y + 14.0, 1.0, pill.h - 28.0),
            0.0,
            hex(P.ink, 0.12),
        );

        let title = if running {
            "STOP GAME"
        } else if !ready {
            "PREPARE RESOURCES"
        } else {
            "PLAY"
        };
        let text_box = Rect::new(pill.x + 12.0, pill.y + 8.0, pill.w - 56.0, 28.0);
        ui.centred(text_box, title, Face::Medium, 16.5, hex(P.ink, 1.0));
        let sub_text = format!("{}  ·  Horizon 26.3", installation.name);
        let sub_fit = ui.c.fit(&sub_text, Face::Regular, 11.5, pill.w - 60.0);
        ui.centred(
            Rect::new(pill.x + 12.0, pill.y + 35.0, pill.w - 56.0, 18.0),
            &sub_fit,
            Face::Regular,
            11.5,
            hex(P.ink_soft, 0.88),
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
            } else if !ready {
                self.set_page(Page::Settings);
                self.note(
                    "Place Horizon 26.3 resources beside Tachyon.exe or set TACHYON_RESOURCES.",
                    true,
                );
            } else {
                self.play(self.store.selected_index());
            }
        }

        // Concise subtitle & actionable status pill underneath CTA.
        let last = if installation.last_played > 0 {
            format!("Played {}", store::ago(installation.last_played, store::now()))
        } else {
            "Ready to build".to_owned()
        };
        let cont = format!("Continue: {} · {last}", installation.name);
        let cont_fit = ui.c.fit(&cont, Face::Regular, 12.0, hero.w - 48.0);
        let cont_w = ui.c.measure(&cont_fit, Face::Regular, 12.0) + 22.0;
        ui.c.rect(
            Rect::new(hero.x + (hero.w - cont_w) / 2.0, pill.bottom() + 12.0, cont_w, 22.0),
            11.0,
            hex(CHROME_BG, 0.56),
        );
        ui.centred(
            Rect::new(hero.x + 16.0, pill.bottom() + 13.0, hero.w - 32.0, 20.0),
            &cont_fit,
            Face::Regular,
            12.0,
            hex(P.ink, 0.94),
        );

        let actionable = if self.importing.is_some() {
            Some(("Importing content...", BADGE_CYAN_BG, BADGE_CYAN_RING, P.accent, CYAN_GLOW, false))
        } else if running {
            Some(("Game running", BADGE_GREEN_BG, BADGE_GREEN_RING, P.success, 0x9ff5d0, false))
        } else if !ready {
            Some(("Locate resources in Settings  >", BADGE_CYAN_BG, BADGE_CYAN_RING, P.accent, CYAN_GLOW, true))
        } else {
            None
        };
        if let Some((badge_text, bbg, bring, bdot, btxt, open_settings)) = actionable {
            let bw = ui.c.measure(badge_text, Face::Medium, 11.5) + 32.0;
            let badge = Rect::new(hero.x + (hero.w - bw) / 2.0, pill.bottom() + 40.0, bw, 25.0);
            let (b_hover, b_click) = if open_settings {
                self.clickable(ui, badge)
            } else {
                (false, false)
            };
            ui.c.rect(badge, 12.5, hex(if b_hover { 0x112a46 } else { bbg }, 0.92));
            ui.c.ring(
                badge,
                12.5,
                1.0,
                hex(if b_hover { P.accent } else { bring }, 0.95),
            );
            ui.c.rect(
                Rect::new(badge.x + 10.0, badge.y + 9.5, 6.0, 6.0),
                3.0,
                hex(bdot, 1.0),
            );
            ui.text(badge.x + 21.0, badge.y + 5.0, badge_text, Face::Medium, 11.5, hex(btxt, 1.0));
            if b_click {
                self.set_page(Page::Settings);
            }
        }

        // Latest updates editorial strip with comfortable spacing and 2-line previews.
        let strip_y = hero.bottom() + 20.0;
        let uw = ui.text(hero.x, strip_y, "Latest updates", Face::Medium, 13.5, hex(P.ink, 1.0));
        ui.text(hero.x + uw + 8.0, strip_y + 1.0, "·  3 releases", Face::Regular, 12.0, hex(P.muted, 1.0));

        let more_label = "All updates >";
        let mw = ui.c.measure(more_label, Face::Medium, 12.0);
        let more_r = Rect::new(hero.right() - mw - 6.0, strip_y - 3.0, mw + 6.0, 22.0);
        let (m_hover, m_click) = self.clickable(ui, more_r);
        ui.text(
            more_r.x + 3.0,
            strip_y + 0.5,
            more_label,
            Face::Medium,
            12.0,
            hex(if m_hover { P.accent } else { P.muted }, 1.0),
        );
        if m_click {
            self.set_page(Page::Notes);
        }

        let card_y = strip_y + 24.0;
        let card_h = (content.bottom() - card_y - 18.0).clamp(84.0, 108.0);
        let gap = 14.0;
        let cols = 3usize.min(NOTES.len());
        let card_w = (hero.w - gap * (cols as f32 - 1.0)) / cols as f32;
        let tag_colors = [P.accent, VIOLET, P.success];
        for (i, (title, date, points)) in NOTES.iter().take(cols).enumerate() {
            let card = Rect::new(hero.x + i as f32 * (card_w + gap), card_y, card_w, card_h);
            let (hover, click) = self.clickable(ui, card);
            ui.c.rect(
                card,
                11.0,
                hex(if hover { CARD_HOVER } else { P.surface }, 1.0),
            );
            ui.c.ring(
                card,
                11.0,
                1.0,
                hex(if hover { P.accent } else { HAIRLINE }, if hover { 0.72 } else { 1.0 }),
            );
            let tag = tag_colors[i % tag_colors.len()];
            ui.c.rect(Rect::new(card.x + 14.0, card.y + 15.0, 6.0, 6.0), 3.0, hex(tag, 1.0));
            let meta = date.to_uppercase();
            let meta_fit = ui.c.fit(&meta, Face::Mono, 10.0, card.w - 40.0);
            ui.text(
                card.x + 26.0,
                card.y + 12.0,
                &meta_fit,
                Face::Mono,
                10.0,
                hex(if i == 0 { CYAN_GLOW } else { P.muted }, 1.0),
            );
            let title_fit = ui.c.fit(title, Face::Medium, 13.0, card.w - 28.0);
            ui.text(card.x + 14.0, card.y + 30.0, &title_fit, Face::Medium, 13.0, hex(P.ink, 1.0));
            if let Some(first) = points.first() {
                let mut wrapped = ui.c.wrap(first, Face::Regular, 11.5, card.w - 28.0);
                if wrapped.len() > 2 {
                    let second = format!("{} {}", wrapped[1], wrapped[2..].join(" "));
                    wrapped[1] = ui.c.fit(&second, Face::Regular, 11.5, card.w - 28.0);
                    wrapped.truncate(2);
                }
                for (li, line) in wrapped.into_iter().enumerate() {
                    let fitted = ui.c.fit(&line, Face::Regular, 11.5, card.w - 28.0);
                    ui.text(
                        card.x + 14.0,
                        card.y + 51.0 + li as f32 * 16.0,
                        &fitted,
                        Face::Regular,
                        11.5,
                        hex(P.ink_soft, 0.74),
                    );
                }
            }
            if click {
                self.set_page(Page::Notes);
            }
        }
    }

    /// Shared obsidian-cyan pill badge for `ACTIVE`, `LATEST`, and file type tags.
    fn draw_tag_pill(ui: &mut Ui, r: Rect, label: &str, size: f32) {
        ui.c.rect(r, 6.0, hex(RAIL_ACTIVE_BG, 1.0));
        ui.c.ring(r, 6.0, 1.0, hex(P.accent, 0.55));
        ui.centred(r, label, Face::Mono, size, hex(CYAN_GLOW, 1.0));
    }

    /// The installation popover anchored beneath the Hero's frosted Play pill.
    fn menu_rects(&self, content: Rect) -> (Rect, Vec<Rect>) {
        let hero = hero_rect(content);
        let wide = self.paths.resources.is_none() && self.game.is_none();
        let pill = hero_play_pill(hero, wide);
        let item_h = 52.0;
        let max_items = ((content.bottom() - pill.bottom() - 28.0) / item_h)
            .floor()
            .max(2.0) as usize;
        let n = (self.store.installations.len() + 1).min(max_items.clamp(3, 6));
        let frame_h = n as f32 * item_h + 12.0;
        let frame_w = 404.0f32.min(hero.w - 32.0);
        let fx = hero.x + (hero.w - frame_w) / 2.0;
        let fy = (pill.bottom() + 8.0).min(content.bottom() - frame_h - 16.0);
        let frame = Rect::new(fx, fy, frame_w, frame_h);
        let items = (0..n)
            .map(|i| {
                Rect::new(
                    frame.x + 8.0,
                    frame.y + 8.0 + i as f32 * item_h,
                    frame.w - 16.0,
                    item_h - 4.0,
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
        ui.c.rect(shadow, 16.0, hex(0x02040a, 0.72));
        ui.c.rect(frame, 12.0, hex(P.surface, 1.0));
        ui.c.ring(frame, 12.0, 1.0, hex(CARD_BORDER, 1.0));
        let last = items.len() - 1;
        let selected = self.store.selected_index();
        for (i, r) in items.iter().enumerate() {
            let hover = ui.hovered(*r);
            if hover {
                self.wanted_cursor = CursorIcon::Pointer;
                ui.c.rect(*r, 8.0, hex(CARD_HOVER, 1.0));
            }
            if i == last {
                ui.c.rect(Rect::new(r.x + 6.0, r.y, r.w - 12.0, 1.0), 0.0, hex(HAIRLINE, 1.0));
                ui.text(
                    r.x + 14.0,
                    r.y + 17.0,
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
                ui.c.rect(*r, 8.0, hex(RAIL_ACTIVE_BG, 0.75));
                ui.c.ring(*r, 8.0, 1.0, hex(P.accent, 0.35));
            }
            Self::draw_emblem(ui, Rect::new(r.x + 10.0, r.y + 10.0, 28.0, 28.0));
            let name = ui.c.fit(&installation.name, Face::Medium, 13.5, r.w - 94.0);
            ui.text(r.x + 48.0, r.y + 6.0, &name, Face::Medium, 13.5, hex(P.ink, 1.0));
            let folder = installation.folder(&self.paths.data).display().to_string();
            let folder = ui.c.fit(&folder, Face::Regular, 11.0, r.w - 94.0);
            ui.text(r.x + 48.0, r.y + 25.0, &folder, Face::Regular, 11.0, hex(P.muted, 1.0));
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
        let card_h = 148.0 + lines.len().max(1) as f32 * 18.0;
        let card = Rect::new((w - card_w) / 2.0, (h - card_h) / 2.0, card_w, card_h);
        ui.c.glow(card, 16.0, hex(P.accent, 0.16), 20.0);
        ui.c.rect(card, 16.0, hex(P.surface, 0.98));
        ui.c.ring(card, 16.0, 1.5, hex(P.accent, 0.92));
        ui.centred(
            Rect::new(card.x + 24.0, card.y + 30.0, card.w - 48.0, 32.0),
            &format!("Drop to import into {installation}"),
            Face::Medium,
            21.0,
            hex(P.ink, 1.0),
        );
        let mut ly = card.y + 72.0;
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
            Self::draw_tag_pill(ui, Rect::new(bx, ly + 14.0, 64.0, 26.0), ext, 11.0);
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
        let zone = Rect::new(x, y, w, 66.0);
        ui.c.rect(
            zone,
            12.0,
            hex(if self.dropping { RAIL_ACTIVE_BG } else { 0x0b1020 }, 1.0),
        );
        ui.c.ring(
            zone,
            12.0,
            1.0,
            hex(if self.dropping { P.accent } else { CARD_BORDER }, 1.0),
        );
        ui.c.rect(
            Rect::new(zone.x + 1.0, zone.y + 15.0, 3.0, zone.h - 30.0),
            1.5,
            hex(VIOLET, 0.85),
        );
        let selected_name = self.store.installations[self.store.selected_index()]
            .name
            .clone();
        ui.text(
            zone.x + 20.0,
            zone.y + 13.0,
            &format!("Drop a file anywhere here to import into {selected_name}"),
            Face::Medium,
            13.5,
            hex(P.ink, 1.0),
        );
        let kinds = ui.c.fit(KINDS, Face::Regular, 12.0, zone.w - 40.0);
        ui.text(zone.x + 20.0, zone.y + 36.0, &kinds, Face::Regular, 12.0, hex(P.muted, 1.0));
        y += 84.0;

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
                hex(if hover { CARD_HOVER } else { P.surface }, 1.0),
            );
            ui.c.ring(
                row,
                12.0,
                1.0,
                hex(if is_sel { P.accent } else { HAIRLINE }, if is_sel { 0.65 } else { 1.0 }),
            );
            if is_sel {
                ui.c.rect(
                    Rect::new(row.x + 1.0, row.y + 14.0, 3.0, row.h - 28.0),
                    1.5,
                    hex(P.accent, 1.0),
                );
            }
            Self::draw_emblem(ui, Rect::new(row.x + 18.0, row.y + 16.0, 40.0, 40.0));
            let tx = row.x + 72.0;
            let buttons = 78.0 + 8.0 + 72.0 + 8.0 + 82.0 + 16.0 + 16.0;
            let room = (row.right() - buttons - tx).max(80.0);
            let name = ui.c.fit(&installation.name, Face::Medium, 15.0, room);
            let nw = ui.text(tx, row.y + 15.0, &name, Face::Medium, 15.0, hex(P.ink, 1.0));
            if is_sel && tx + nw + 80.0 < row.right() - buttons {
                Self::draw_tag_pill(ui, Rect::new(tx + nw + 10.0, row.y + 16.0, 72.0, 20.0), "ACTIVE", 10.0);
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
        y += 30.0;
        ui.text(
            x,
            y,
            "Configure profile name, isolated game directory, and default launch resolution.",
            Face::Regular,
            13.0,
            hex(P.muted, 1.0),
        );
        y += 30.0;

        let panel = Rect::new(x, y, w, 392.0);
        ui.c.rect(panel, 14.0, hex(P.surface, 1.0));
        ui.c.ring(panel, 14.0, 1.0, hex(HAIRLINE, 1.0));

        let fx = panel.x + 24.0;
        let fw = panel.w - 48.0;
        let mut fy = panel.y + 24.0;
        Self::label(ui, fx, fy, "NAME");
        self.field(
            ui,
            Rect::new(fx, fy + 18.0, fw, 42.0),
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
                hex(if hover { CARD_HOVER } else { P.surface }, 1.0),
            );
            ui.c.ring(
                row,
                12.0,
                1.0,
                hex(if is_active { P.accent } else { HAIRLINE }, if is_active { 0.65 } else { 1.0 }),
            );
            if is_active {
                ui.c.rect(
                    Rect::new(row.x + 1.0, row.y + 14.0, 3.0, row.h - 28.0),
                    1.5,
                    hex(P.accent, 1.0),
                );
            }
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
                Self::draw_tag_pill(ui, use_r, "ACTIVE", 10.5);
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
        let bullet_colors = [P.accent, VIOLET, P.success];
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
                hex(if i == 0 { P.accent } else { HAIRLINE }, if i == 0 { 0.50 } else { 1.0 }),
            );
            let date = date.to_uppercase();
            Self::label(ui, card.x + 22.0, card.y + 20.0, &date);
            if i == 0 {
                let dw = ui.c.measure(&date, Face::Mono, 11.0);
                Self::draw_tag_pill(ui, Rect::new(card.x + 22.0 + dw + 10.0, card.y + 17.0, 58.0, 20.0), "LATEST", 10.0);
            }
            ui.text(card.x + 22.0, card.y + 40.0, title, Face::Medium, 17.0, hex(P.ink, 1.0));
            let bullet = bullet_colors[i % bullet_colors.len()];
            let mut ly = card.y + 72.0;
            for point in lines {
                ui.c.rect(Rect::new(card.x + 24.0, ly + 7.0, 5.0, 5.0), 2.5, hex(bullet, 1.0));
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

        let card = Rect::new(x, y, w, 5.0 * 74.0 + 96.0 + 16.0);
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
        ui.row_ruleless(r, 6, "About", &about, 0.0);
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
        ui.c.rect(shadow, 14.0, hex(0x02040a, 0.65));
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
