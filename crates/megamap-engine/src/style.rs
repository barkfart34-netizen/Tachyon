//! Fixed UI style: the frame around the minimap and the world map, and the toolbar's button states.
//!
//! The frame is one designer grey bevel (edge, highlight top/left, shade bottom/right, drop shadow), always
//! opaque. It does not follow the minimap opacity (a translucent frame let the world show through and changed
//! with the time of day), the UI theme, the weather, the clouds or the light.

/// Outer 1 px edge.
pub const FRAME_EDGE: u32 = 0xFF1A_1E24;
/// Bevel highlight (top and left).
pub const FRAME_LIGHT: u32 = 0xFFB4_BCC8;
/// Bevel shade (bottom and right).
pub const FRAME_DARK: u32 = 0xFF4E_5562;
/// Middle grey: single-line borders of the world map's panels, toolbar and fields (both themes).
pub const FRAME_MID: u32 = 0xFF6E_7682;
/// The drop shadow under the frame (two offset layers of this).
pub const FRAME_SHADOW: u32 = 0x5000_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct FrameColors {
    pub edge: u32,
    pub light: u32,
    pub dark: u32,
    pub mid: u32,
    pub shadow: u32,
}

/// The frame colours. The arguments are what could tint it and deliberately do not: the minimap opacity
/// (percent), the day time, the rain and thunder strength and the UI theme (0 auto, 1 dark, 2 light).
pub fn frame_colors(_opacity: u32, _day_time: u32, _rain: f32, _thunder: f32, _ui_theme: u8) -> FrameColors {
    FrameColors { edge: FRAME_EDGE, light: FRAME_LIGHT, dark: FRAME_DARK, mid: FRAME_MID, shadow: FRAME_SHADOW }
}

/// Whether the interface uses its light theme. `ui_theme` 1 dark, 2 light; 0 auto follows the time of day, unless
/// `ui_fixed` ("the interface does not depend on the time of day or weather") pins it to dark. The weather never
/// enters: it is in the signature so the independence is visible and tested.
pub fn ui_light(ui_theme: u8, ui_fixed: bool, daylight: f32, _rain: f32, _thunder: f32) -> bool {
    match ui_theme {
        1 => false,
        2 => true,
        _ => !ui_fixed && daylight > 0.6,
    }
}

/// The bevel colour of a round frame's segment facing angle `a` (radians, screen space, y down): lit from the
/// top left. Opaque.
pub fn ring_color(a: f32) -> u32 {
    let lit = ((-a.cos() - a.sin()) * std::f32::consts::FRAC_1_SQRT_2 * 0.5 + 0.5).clamp(0.0, 1.0);
    0xFF00_0000 | mix_rgb(FRAME_DARK, FRAME_LIGHT, lit)
}

pub fn mix_rgb(a: u32, b: u32, t: f32) -> u32 {
    let ch = |s: u32| {
        let (x, y) = (((a >> s) & 0xFF) as f32, ((b >> s) & 0xFF) as f32);
        ((x + (y - x) * t).round() as u32) << s
    };
    ch(16) | ch(8) | ch(0)
}

/// A toolbar / nav button's look.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ButtonLook {
    pub fill: u32,
    pub border: u32,
    pub icon: u32,
}

/// Button colours for its state: idle, hovered, active (the toggle is on), hovered and active. `dark` picks the
/// dark UI theme's fill; the border stays the fixed frame grey unless active.
pub fn button_look(hover: bool, active: bool, dark: bool, accent: u32) -> ButtonLook {
    let base = if dark { 0xE02A_2E36 } else { 0xF0F4_F6F9 };
    let hi = if dark { 0xF03A_404A } else { 0xFFFF_FFFF };
    let ink = if dark { 0xFFE6_E9EE } else { 0xFF2A_2F37 };
    match (hover, active) {
        (false, false) => ButtonLook { fill: base, border: FRAME_MID, icon: ink },
        (true, false) => ButtonLook { fill: hi, border: FRAME_LIGHT, icon: ink },
        (false, true) => ButtonLook { fill: base, border: accent, icon: accent },
        (true, true) => ButtonLook { fill: hi, border: accent, icon: accent },
    }
}

/// The one-pixel rings that draw an icon frame `width` GUI pixels wide (0..=3, quarter steps): `(offset, alpha)`
/// with offset 1 the ring right outside the icon. Whole pixels are opaque; the fractional rest is an outer ring at
/// that coverage, so a 0.25 or 1.5 px frame reads at its width without a hard staircase.
pub fn frame_rings(width: f32) -> Vec<(f32, f32)> {
    let w = width.clamp(0.0, 3.0);
    let whole = w.floor() as usize;
    let mut v: Vec<(f32, f32)> = (1..=whole).map(|i| (i as f32, 1.0)).collect();
    let frac = w - whole as f32;
    if frac > 0.01 {
        v.push(((whole + 1) as f32, frac));
    }
    v
}
