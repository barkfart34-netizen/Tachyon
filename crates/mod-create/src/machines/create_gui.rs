//! Shared UI theme, screen coordination, and flat-panel / touch-friendly widgets for Create's custom mod screens
//! (`Host::mod_screen_open("create")`).
//!
//! Helper module only (not a `Hooks` implementation).

// Shared toolkit constants/helpers are consumed progressively across Wave 1-3 screens.
#![allow(dead_code)]

use std::sync::{Mutex, PoisonError};

use create_kinetics::rules::trains::station_screen::TextField;
use crate::compat::{Host, ScreenRenderPostEvent};

/// Shared mod screen owner id for every Create screen.
pub const MOD: &str = "create";

// --- palette (fill uses ARGB 0xAA_RRGGBB; text uses RGB 0xRRGGBB) -----------------------------------------------

pub const DIM: u32 = 0x78_0C0B0A;
pub const PANEL_DARK: u32 = 0xFA_24211D;
pub const PANEL_BRASS: u32 = 0xFA_2D251C;
pub const PANEL_PARCHMENT: u32 = 0xFF_E7DCC3;
pub const WOOD_BOARD: u32 = 0xFF_6D4C2D;
pub const WOOD_TRIM: u32 = 0xFF_4B321C;
pub const FRAME_BRASS: u32 = 0xFF_C49A45;
pub const FRAME_DARK: u32 = 0xFF_5C5142;
pub const FRAME_PARCHMENT: u32 = 0xFF_9B8765;
pub const HEADER_BG: u32 = 0xFF_3B2F22;
pub const ROW_BG: u32 = 0xFF_322C25;
pub const ROW_ALT: u32 = 0xFF_2A2520;
pub const ROW_ACTIVE: u32 = 0xFF_3E5330;
pub const ROW_DIM: u32 = 0xFF_25221F;
pub const ROW_PARCHMENT: u32 = 0xFF_DDD0B3;
pub const SLOT_BG: u32 = 0xFF_1B1815;
pub const SLOT_HOVER: u32 = 0xFF_4B4236;
pub const SLOT_SELECTED: u32 = 0xFF_D7A844;
pub const INPUT_BG: u32 = 0xFF_181512;
pub const INPUT_FOCUS: u32 = 0xFF_C49A45;
pub const INPUT_PARCHMENT: u32 = 0xFF_F3EBD8;
pub const INPUT_PARCHMENT_FOCUS: u32 = 0xFF_FFF9EB;
pub const BTN: u32 = 0xFF_4E4539;
pub const BTN_HOVER: u32 = 0xFF_695D4D;
pub const BTN_ON: u32 = 0xFF_B07D2B;
pub const BTN_ON_HOVER: u32 = 0xFF_CE983C;
pub const BTN_PRIMARY: u32 = 0xFF_2F6F3E;
pub const BTN_PRIMARY_HOVER: u32 = 0xFF_3E8C50;
pub const BTN_WARN: u32 = 0xFF_843333;
pub const BTN_WARN_HOVER: u32 = 0xFF_A54444;
pub const BTN_PARCHMENT: u32 = 0xFF_C9B996;
pub const BTN_PARCHMENT_HOVER: u32 = 0xFF_D9CBA9;
pub const BTN_DISABLED: u32 = 0xFF_35322E;

pub const TEXT_LIGHT: u32 = 0xF2EDE2;
pub const TEXT_MUTED: u32 = 0xA89F91;
pub const TEXT_GOLD: u32 = 0xF5D27A;
pub const TEXT_DARK: u32 = 0x35291E;
pub const TEXT_GREEN: u32 = 0x8DE08D;
pub const TEXT_RED: u32 = 0xEB7A7A;

// --- active screen tag ------------------------------------------------------------------------------------------

static ACTIVE: Mutex<Option<&'static str>> = Mutex::new(None);

fn lock_active() -> std::sync::MutexGuard<'static, Option<&'static str>> {
    ACTIVE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Opens the `create` mod screen on behalf of `tag`.
pub fn open(host: &mut dyn Host, tag: &'static str) -> bool {
    *lock_active() = Some(tag);
    if host.mod_screen_open(MOD) {
        true
    } else {
        let mut active = lock_active();
        if *active == Some(tag) {
            *active = None;
        }
        false
    }
}

/// Whether `tag` owns the currently displayed `create` mod screen.
pub fn is_active(host: &dyn Host, tag: &'static str) -> bool {
    host.mod_screen() == Some(MOD) && *lock_active() == Some(tag)
}

/// Closes the mod screen if `tag` still owns it.
pub fn close(host: &mut dyn Host, tag: &'static str) {
    let should_close = {
        let mut active = lock_active();
        if *active == Some(tag) {
            *active = None;
            true
        } else {
            false
        }
    };
    if should_close && host.mod_screen() == Some(MOD) {
        host.mod_screen_close(MOD);
    }
}

/// Clears `tag` ownership when vanilla reports the mod screen closing.
pub fn on_screen_closing(tag: &'static str) {
    let mut active = lock_active();
    if *active == Some(tag) {
        *active = None;
    }
}

/// Returns `true` if `tag` no longer owns the open screen (and clears stale ownership if the host switched away).
pub fn sync_closed(host: &dyn Host, tag: &'static str) -> bool {
    if host.mod_screen() != Some(MOD) {
        on_screen_closing(tag);
        return true;
    }
    *lock_active() != Some(tag)
}

// --- geometry ---------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl UiRect {
    pub const fn new(x: i32, y: i32, w: i32, h: i32) -> Self {
        Self { x, y, w, h }
    }

    pub fn contains(self, pt: [f32; 2]) -> bool {
        pt[0] >= self.x as f32
            && pt[0] < (self.x + self.w) as f32
            && pt[1] >= self.y as f32
            && pt[1] < (self.y + self.h) as f32
    }

    pub const fn inset(self, d: i32) -> Self {
        let w = self.w - d * 2;
        let h = self.h - d * 2;
        Self {
            x: self.x + d,
            y: self.y + d,
            w: if w > 0 { w } else { 0 },
            h: if h > 0 { h } else { 0 },
        }
    }

    pub const fn offset(self, dx: i32, dy: i32) -> Self {
        Self { x: self.x + dx, y: self.y + dy, w: self.w, h: self.h }
    }
}

/// Centers a panel in GUI space while clamping top/left to `>= 4` on narrow mobile screens.
pub fn screen_origin(size: [i32; 2], panel_w: i32, panel_h: i32) -> (i32, i32) {
    (((size[0] - panel_w) / 2).max(4), ((size[1] - panel_h) / 2).max(4))
}

// --- drawing ----------------------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BtnStyle {
    Normal,
    Primary,
    Danger,
    ToggleOn,
    Parchment,
}

pub fn fill(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, r: UiRect, argb: u32) {
    host.fill(event.batch, [r.x as f32, r.y as f32, r.w as f32, r.h as f32], argb);
}

pub fn panel(host: &dyn Host, event: &mut ScreenRenderPostEvent<'_>, r: UiRect, frame: u32, bg: u32) {
    fill(host, event, r, frame);
    fill(host, event, r.inset(2), bg);
}

pub fn header(
    host: &dyn Host,
    event: &mut ScreenRenderPostEvent<'_>,
    r: UiRect,
    icon: Option<&str>,
    title: &str,
    right_badge: Option<&str>,
) {
    fill(host, event, r, HEADER_BG);
    fill(host, event, UiRect::new(r.x, r.y + r.h - 1, r.w, 1), FRAME_BRASS);
    let mut tx = r.x + 6;
    if let Some(item_id) = icon {
        host.item(event.batch, item_id, [tx as f32, (r.y + (r.h - 16) / 2) as f32], 16.0);
        tx += 20;
    }
    let badge_w = right_badge.map_or(0.0, |b| host.text_width(b) + 12.0);
    let max_title_w = (r.x + r.w - tx) as f32 - badge_w - 6.0;
    let shown_title = truncate(host, title, max_title_w.max(24.0));
    host.text(event.batch, &shown_title, [tx as f32, (r.y + (r.h - 8) / 2) as f32], TEXT_GOLD, true);
    if let Some(badge) = right_badge {
        let bw = host.text_width(badge);
        host.text(
            event.batch,
            badge,
            [(r.x + r.w - 6) as f32 - bw, (r.y + (r.h - 8) / 2) as f32],
            TEXT_MUTED,
            false,
        );
    }
}

pub fn button(
    host: &dyn Host,
    event: &mut ScreenRenderPostEvent<'_>,
    r: UiRect,
    label: &str,
    style: BtnStyle,
    enabled: bool,
) -> bool {
    let hovered = enabled && r.contains(event.screen.mouse);
    let (border, bg, ink) = if !enabled {
        (FRAME_DARK, BTN_DISABLED, TEXT_MUTED)
    } else {
        match style {
            BtnStyle::Normal => (FRAME_DARK, if hovered { BTN_HOVER } else { BTN }, TEXT_LIGHT),
            BtnStyle::Primary => (0xFF_54A867, if hovered { BTN_PRIMARY_HOVER } else { BTN_PRIMARY }, TEXT_LIGHT),
            BtnStyle::Danger => (0xFF_B35454, if hovered { BTN_WARN_HOVER } else { BTN_WARN }, TEXT_LIGHT),
            BtnStyle::ToggleOn => (FRAME_BRASS, if hovered { BTN_ON_HOVER } else { BTN_ON }, 0x1E160B),
            BtnStyle::Parchment => (FRAME_PARCHMENT, if hovered { BTN_PARCHMENT_HOVER } else { BTN_PARCHMENT }, TEXT_DARK),
        }
    };
    fill(host, event, r, border);
    fill(host, event, r.inset(1), bg);
    let shown = truncate(host, label, (r.w - 6).max(6) as f32);
    let tw = host.text_width(&shown);
    let shadow = enabled && !matches!(style, BtnStyle::ToggleOn | BtnStyle::Parchment);
    host.text(
        event.batch,
        &shown,
        [r.x as f32 + ((r.w as f32 - tw) / 2.0).max(2.0), r.y as f32 + (r.h as f32 - 8.0) / 2.0],
        ink,
        shadow,
    );
    hovered
}

pub fn item_slot(
    host: &dyn Host,
    event: &mut ScreenRenderPostEvent<'_>,
    r: UiRect,
    item_id: Option<&str>,
    selected: bool,
) -> bool {
    let hovered = r.contains(event.screen.mouse);
    let border = if selected {
        SLOT_SELECTED
    } else if hovered {
        FRAME_BRASS
    } else {
        FRAME_DARK
    };
    fill(host, event, r, border);
    fill(host, event, r.inset(1), if hovered { SLOT_HOVER } else { SLOT_BG });
    if let Some(id) = item_id {
        let icon_x = r.x as f32 + (r.w as f32 - 16.0) / 2.0;
        let icon_y = r.y as f32 + (r.h as f32 - 16.0) / 2.0;
        host.item(event.batch, id, [icon_x, icon_y], 16.0);
    }
    hovered
}

pub fn text_field(
    host: &dyn Host,
    event: &mut ScreenRenderPostEvent<'_>,
    r: UiRect,
    field: &TextField,
    placeholder: &str,
    dark_bg: bool,
) {
    let border = if field.focused {
        INPUT_FOCUS
    } else if dark_bg {
        FRAME_DARK
    } else {
        FRAME_PARCHMENT
    };
    let bg = if dark_bg {
        INPUT_BG
    } else if field.focused {
        INPUT_PARCHMENT_FOCUS
    } else {
        INPUT_PARCHMENT
    };
    fill(host, event, r, border);
    fill(host, event, r.inset(1), bg);

    let val = field.value();
    let tx = r.x + 4;
    let ty = r.y + (r.h - 8) / 2;
    let inner_w = (r.w - 8).max(8) as f32;
    if val.is_empty() {
        if !placeholder.is_empty() {
            let ph = truncate(host, placeholder, inner_w);
            let ink = if dark_bg { TEXT_MUTED } else { 0x8F826A };
            host.text(event.batch, &ph, [tx as f32, ty as f32], ink, false);
        }
        if field.focused {
            let cur_col = if dark_bg { 0xFF_F5D27A } else { 0xFF_35291E };
            fill(host, event, UiRect::new(tx, ty - 1, 1, 10), cur_col);
        }
        return;
    }

    let chars: Vec<char> = val.chars().collect();
    let cursor = field.cursor().min(chars.len());
    let mut start = 0usize;
    if field.focused {
        let before: String = chars[..cursor].iter().collect();
        if host.text_width(&before) > inner_w {
            start = cursor;
            while start > 0 {
                let candidate: String = chars[start - 1..cursor].iter().collect();
                if host.text_width(&candidate) > inner_w {
                    break;
                }
                start -= 1;
            }
        }
    }
    let tail: String = chars[start..].iter().collect();
    let shown = truncate(host, &tail, inner_w);
    if field.is_selected() {
        let sel_w = host.text_width(&shown).min(inner_w) as i32 + 1;
        fill(host, event, UiRect::new(tx, ty - 1, sel_w, 10), 0xA0_4C7FFF);
    }
    let ink = if dark_bg { TEXT_LIGHT } else { TEXT_DARK };
    host.text(event.batch, &shown, [tx as f32, ty as f32], ink, false);
    if field.focused {
        let before_visible: String = chars[start..cursor].iter().collect();
        let cx = tx + host.text_width(&before_visible).min(inner_w) as i32;
        let cur_col = if dark_bg { 0xFF_F5D27A } else { 0xFF_35291E };
        fill(host, event, UiRect::new(cx, ty - 1, 1, 10), cur_col);
    }
}

pub fn truncate(host: &dyn Host, text: &str, max_w: f32) -> String {
    if host.text_width(text) <= max_w {
        return text.to_owned();
    }
    let dots = "..";
    let dots_w = host.text_width(dots);
    if dots_w >= max_w {
        return String::new();
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if host.text_width(&out) + dots_w > max_w {
            out.pop();
            break;
        }
    }
    out.push_str(dots);
    out
}

pub fn clean_wire(s: &str) -> String {
    s.chars().filter(|c| !matches!(c, '\u{1e}' | '\u{1f}' | '\n' | '\r' | '\0')).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_rect_hit_testing_and_inset() {
        let r = UiRect::new(10, 20, 40, 20);
        assert!(r.contains([10.0, 20.0]));
        assert!(r.contains([49.9, 39.9]));
        assert!(!r.contains([50.0, 20.0]));
        assert!(!r.contains([10.0, 40.0]));
        assert_eq!(r.inset(2), UiRect::new(12, 22, 36, 16));
        assert_eq!(UiRect::new(0, 0, 2, 2).inset(5), UiRect::new(5, 5, 0, 0));
    }

    #[test]
    fn screen_origin_centers_and_clamps_on_small_screens() {
        assert_eq!(screen_origin([480, 270], 300, 200), (90, 35));
        assert_eq!(screen_origin([200, 150], 340, 220), (4, 4));
    }

    #[test]
    fn clean_wire_strips_control_delimiters_and_keeps_text() {
        assert_eq!(clean_wire("ab\u{1f}cd\u{1e}ef\ngh\r\0"), "abcdefgh");
        assert_eq!(clean_wire("Create Filter #1"), "Create Filter #1");
    }
}
