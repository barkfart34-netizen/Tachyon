//! Follows horizon-android's `TouchControls::draw`, sprite and circle helpers
//! (`touch.rs`) and `layout::contains`. No input, platform calls or asset loading.
pub use super::Control;
use super::{Mode, TouchContext, TouchControls, TouchFeedback, layout};
use crate::{
    gamepad::{Button as PadButton, PadState},
    hud::{HudAtlas, HudBatch, rgba},
};

/// A visible control, in painter's order. The engine resolves layout, scale and
/// mirroring once; drawing and hit-testing share physical bounds.
#[derive(Clone, Copy, Debug)]
pub struct Button {
    pub control: Control,
    /// [x, y, width, height], in physical pixels.
    pub bounds: [f32; 4],
    pub opacity: f32,
    pub pressed: bool,
}
impl Button {
    pub fn contains(&self, point: [f32; 2]) -> bool {
        valid_bounds(self.bounds) && layout::contains(self.bounds, point)
    }
}
/// The topmost control at a physical point. Hidden controls are omitted from the snapshot.
pub fn control_at(buttons: &[Button], point: [f32; 2]) -> Option<Control> {
    buttons
        .iter()
        .rev()
        .find(|b| b.contains(point))
        .map(|b| b.control)
}

/// Supplied only when a tap-mode pointer should be visible. Its lifetime and
/// progress belong to the input/gameplay adapter, not to the drawing clock.
#[derive(Clone, Copy, Debug)]
pub struct Marker {
    pub point: [f32; 2],
    /// Diameter in GUI pixels, as in the fork; the point is in physical pixels.
    pub size: f32,
    pub progress: f32,
}

/// Owned drawing values from the existing layout, pad and gameplay contracts.
#[derive(Default)]
pub struct State {
    pub mode: Mode,
    pub pad: PadState,
    pub context: TouchContext,
    pub marker: Option<Marker>,
}

impl TouchControls {
    /// Read-only physical bounds from the same placement used by control_at.
    /// Supply the drawable viewport first, without rounded GUI dimensions.
    pub fn hud_buttons(&self) -> Vec<Button> {
        if self.surface.contains(&0) {
            return Vec::new();
        }
        let [w, h] = self.surface.map(|v| v as f32);
        let pad = self.state();
        Control::ALL
            .into_iter()
            .filter(|c| self.visible(*c))
            .map(|control| {
                let p = self.placement(control);
                let rect = p.rect(self.aspect());
                let [x, y, width, height] = rect;
                let held = Self::button(control).is_some_and(|button| match button {
                    PadButton::LeftTrigger => pad.triggers[0] > 0.0,
                    PadButton::RightTrigger => pad.triggers[1] > 0.0,
                    _ => pad.held(button),
                });
                Button {
                    control,
                    bounds: [x * w, y * h, width * w, height * h],
                    opacity: p.opacity,
                    pressed: held
                        || self.fingers.values().any(|(_, point)| layout::contains(rect, *point)),
                }
            })
            .collect()
    }

    /// The adapter supplies an optional normalized pointer and mining feedback.
    /// This does not create gestures, consume frames or change their lifetime.
    pub fn hud_state(&self, pointer: Option<[f32; 2]>, feedback: TouchFeedback) -> State {
        let [w, h] = self.surface.map(|v| v as f32);
        State {
            mode: self.settings.mode,
            pad: self.state(),
            context: self.context.clone(),
            marker: pointer
                .filter(|_| {
                    !self.surface.contains(&0)
                        && !self.settings.crosshair()
                        && !self.context.force_crosshair
                })
                .map(|point| Marker {
                    point: [point[0] * w, point[1] * h],
                    size: self.settings.circle_size,
                    progress: feedback.break_progress,
                }),
        }
    }

    pub fn draw(
        &self,
        b: &mut HudBatch,
        a: &HudAtlas,
        pointer: Option<[f32; 2]>,
        feedback: TouchFeedback,
        opacity: f32,
    ) {
        draw(
            b,
            a,
            &self.hud_buttons(),
            &self.hud_state(pointer, feedback),
            opacity,
        );
    }
}

pub fn draw(b: &mut HudBatch, a: &HudAtlas, buttons: &[Button], state: &State, opacity: f32) {
    if !b.scale.is_finite() || b.scale <= 0.0 {
        return;
    }
    let opacity = unit(opacity, 1.0);
    if opacity == 0.0 {
        return;
    }
    for button in buttons {
        let alpha = unit(button.opacity, 0.65) * opacity;
        if !valid_bounds(button.bounds) || alpha == 0.0 {
            continue;
        }
        let bounds = button.bounds.map(|v| v / b.scale);
        let color = rgba(255, 255, 255, (alpha * 255.0).round() as u8);
        if button.control == Control::Move {
            movement(b, a, bounds, state, color);
        } else {
            let pressed =
                button.pressed || (button.control == Control::Sneak && state.context.sneaking);
            button_art(b, a, bounds, button.control, state, pressed, color);
        }
    }
    if let Some(m) = state.marker.filter(|m| m.point.iter().all(|v| v.is_finite())) {
        let center = m.point.map(|v| v / b.scale);
        let radius = if m.size.is_finite() {
            m.size.clamp(16.0, 64.0)
        } else {
            32.0
        } / 2.0;
        let color = |r, alpha: u8| rgba(r, r, r, (f32::from(alpha) * opacity).round() as u8);
        circle(b, a, center, radius + 0.7, color(0, 110), false);
        circle(b, a, center, radius, color(255, 210), false);
        let inner = (radius - 1.5).max(0.0) * unit(m.progress, 0.0);
        if inner > 0.0 {
            // A complete circle grows in radius, never in angular coverage.
            circle(b, a, center, inner, color(255, 90), true);
            circle(b, a, center, inner, color(255, 220), false);
        }
    }
}

fn movement(b: &mut HudBatch, a: &HudAtlas, [x, y, w, h]: [f32; 4], s: &State, color: u32) {
    let dpad = s.mode == Mode::Dpad;
    let mut left = s.pad.left.map(|v| {
        if v.is_finite() {
            v.clamp(-1.0, 1.0)
        } else {
            0.0
        }
    });
    let length = left[0].hypot(left[1]);
    if !dpad && length > 1.0 {
        left = left.map(|v| v / length);
    }
    if !dpad {
        if !sprite(b, a, "ui/joystick_frame", [x, y, w, h], false, color) {
            circle(
                b,
                a,
                [x + w / 2.0, y + h / 2.0],
                w.min(h) / 2.0 - 0.7,
                color,
                false,
            );
        }
        let knob = [
            x + w * (0.33 + left[0] * 0.25),
            y + h * (0.33 - left[1] * 0.25),
            w * 0.34,
            h * 0.34,
        ];
        if !sprite(b, a, "ui/joystick_knob", knob, false, color) {
            circle(
                b,
                a,
                [knob[0] + knob[2] / 2.0, knob[1] + knob[3] / 2.0],
                knob[2].min(knob[3]) / 2.0,
                color,
                true,
            );
        }
        return;
    }
    let cell = |dx, dy| [x + w / 3.0 * dx, y + h / 3.0 * dy, w / 3.0, h / 3.0];
    for (dx, dy, name, label, pressed) in [
        (1.0, 0.0, "up", "^", left[1] > 0.2),
        (0.0, 1.0, "left", "<", left[0] < -0.2),
        (2.0, 1.0, "right", ">", left[0] > 0.2),
        (1.0, 2.0, "down", "v", left[1] < -0.2),
    ] {
        if !sprite(b, a, &format!("controls/{name}"), cell(dx, dy), pressed, color) {
            fallback(b, a, cell(dx, dy), label, pressed, color);
        }
    }
    if left[1].abs() > 0.2 {
        let (dy, prefix, labels) = if left[1] > 0.0 {
            (0.0, "up", ["^<", "^>"])
        } else {
            (2.0, "down", ["v<", "v>"])
        };
        for (dx, suffix, label, pressed) in [
            (0.0, "left", labels[0], left[0] < -0.2),
            (2.0, "right", labels[1], left[0] > 0.2),
        ] {
            if !sprite(
                b,
                a,
                &format!("controls/{prefix}_{suffix}"),
                cell(dx, dy),
                pressed,
                color,
            ) {
                fallback(b, a, cell(dx, dy), label, pressed, color);
            }
        }
    }
}

fn button_art(
    b: &mut HudBatch,
    a: &HudAtlas,
    bounds: [f32; 4],
    c: Control,
    s: &State,
    pressed: bool,
    color: u32,
) {
    let theme = match c {
        Control::Chat => Some("chat"),
        Control::Pause => Some("pause"),
        Control::Settings => Some("settings"),
        Control::Debug => Some("debug"),
        Control::Perspective => Some("perspective"),
        _ => None,
    };
    if let Some(theme) = theme {
        let key = format!("gui/horizon/touch/{theme}");
        if pressed && fitted(b, a, &format!("{key}_pressed"), bounds, color, None)
            || fitted(
                b,
                a,
                &format!("{key}_normal"),
                bounds,
                if pressed { dim(color) } else { color },
                None,
            )
        {
            return;
        }
    }
    let (name, label) = match c {
        Control::Sprint => ("ui/sprint", "Sprint"),
        Control::Attack => ("ui/attack", "Attack"),
        Control::Use => ("ui/interact", "Use"),
        Control::Jump if s.context.swimming => ("controls/waterascend", "Swim up"),
        Control::Sneak if s.context.swimming => ("controls/waterdescend", "Swim down"),
        Control::Jump if s.context.vertical_movement => ("controls/flyingascend", "Fly up"),
        Control::Sneak if s.context.vertical_movement => ("controls/flyingdescend", "Fly down"),
        Control::Sneak if s.context.mounted => ("controls/dismount", "Dismount"),
        Control::Jump if s.mode == Mode::Dpad => ("controls/jump_dpad", "Jump"),
        Control::Sneak if s.mode == Mode::Dpad => ("controls/sneak_dpad", "Sneak"),
        Control::Jump => ("ui/jump", "Jump"),
        Control::Sneak => ("ui/sneak", "Sneak"),
        Control::Pause => ("ui/pause_icon", "Pause"),
        Control::Settings => ("ui/gear", "Settings"),
        Control::Inventory => ("ui/more-dots", "Inventory"),
        Control::Chat => ("", "Chat"),
        Control::Debug => ("", "F3"),
        Control::Perspective => ("", "View"),
        Control::Offhand => ("", "Swap"),
        Control::Shield => ("", "Shield"),
        Control::Move => return,
    };
    let crop = match c {
        Control::Chat => Some([200, 82, 18, 18]),
        Control::Debug => Some([236, 82, 18, 18]),
        Control::Perspective => Some([236, 46, 18, 18]),
        _ => None,
    };
    if crop.is_some()
        && fitted(
            b,
            a,
            "gui/bedrock/gui",
            bounds,
            if pressed { dim(color) } else { color },
            crop,
        )
    {
        return;
    }
    if matches!(c, Control::Inventory | Control::Pause | Control::Settings) {
        if c != Control::Inventory || !sprite(b, a, "ui/joystick_knob", bounds, pressed, color) {
            fallback(b, a, bounds, "", pressed, color);
        }
        let [x, y, w, h] = bounds;
        let inset = if c == Control::Inventory { 0.2 } else { 0.23 };
        let icon = [
            x + w * inset,
            y + h * inset,
            w * (1.0 - 2.0 * inset),
            h * (1.0 - 2.0 * inset),
        ];
        if !sprite(b, a, name, icon, false, color) {
            text(b, a, bounds, label, pressed, color);
        }
    } else if name.is_empty() || !sprite(b, a, name, bounds, pressed, color) {
        fallback(b, a, bounds, label, pressed, color);
    }
}

fn sprite(
    b: &mut HudBatch,
    a: &HudAtlas,
    name: &str,
    bounds: [f32; 4],
    pressed: bool,
    color: u32,
) -> bool {
    let key = format!("gui/bedrock/{name}");
    (pressed && fitted(b, a, &format!("{key}_pressed"), bounds, color, None))
        || fitted(
            b,
            a,
            &key,
            bounds,
            if pressed { dim(color) } else { color },
            None,
        )
}
// Fit wide/short glyphs without stretching. Keep UVs inside texel centres.
fn fitted(
    b: &mut HudBatch,
    a: &HudAtlas,
    key: &str,
    [x, y, w, h]: [f32; 4],
    color: u32,
    crop: Option<[u32; 4]>,
) -> bool {
    let Some(mut r) = a.sprites.get(key).copied().filter(|r| r.w > 0 && r.h > 0) else {
        return false;
    };
    if let Some([sx, sy, sw, sh]) = crop {
        if sw == 0 || sh == 0 || sx.saturating_add(sw) > r.w || sy.saturating_add(sh) > r.h {
            return false;
        }
        let (u, v, du, dv) = (
            r.u0,
            r.v0,
            (r.u1 - r.u0) / r.w as f32,
            (r.v1 - r.v0) / r.h as f32,
        );
        r.u0 = u + sx as f32 * du;
        r.u1 = u + (sx + sw) as f32 * du;
        r.v0 = v + sy as f32 * dv;
        r.v1 = v + (sy + sh) as f32 * dv;
        r.w = sw;
        r.h = sh;
    }
    let scale = (w / r.w as f32).min(h / r.h as f32);
    let (width, height) = (r.w as f32 * scale, r.h as f32 * scale);
    let (u, v) = ((r.u0 + r.u1) / 2.0, (r.v0 + r.v1) / 2.0);
    r.u0 = (r.u0 + 0.5 / a.width.max(1) as f32).min(u);
    r.u1 = (r.u1 - 0.5 / a.width.max(1) as f32).max(u);
    r.v0 = (r.v0 + 0.5 / a.height.max(1) as f32).min(v);
    r.v1 = (r.v1 - 0.5 / a.height.max(1) as f32).max(v);
    b.rect_uv(
        x + (w - width) / 2.0,
        y + (h - height) / 2.0,
        width,
        height,
        r,
        color,
    );
    true
}

fn fallback(
    b: &mut HudBatch,
    a: &HudAtlas,
    [x, y, w, h]: [f32; 4],
    label: &str,
    pressed: bool,
    color: u32,
) {
    let alpha = color.to_le_bytes()[3];
    let background = if pressed {
        rgba(70, 70, 70, alpha)
    } else {
        rgba(0, 0, 0, alpha / 2)
    };
    b.fill(a, x, y, w, h, background);
    let edge = (w.min(h) / 18.0).max(1.0 / b.scale).min(w.min(h) / 2.0);
    for [x, y, w, h] in [
        [x, y, w, edge],
        [x, y + h - edge, w, edge],
        [x, y + edge, edge, h - 2.0 * edge],
        [x + w - edge, y + edge, edge, h - 2.0 * edge],
    ] {
        b.fill(a, x, y, w, h, color);
    }
    if !label.is_empty() {
        text(b, a, [x, y, w, h], label, pressed, color);
    }
}
fn text(
    b: &mut HudBatch,
    a: &HudAtlas,
    [x, y, w, h]: [f32; 4],
    label: &str,
    pressed: bool,
    color: u32,
) {
    let offset = if pressed { (h / 18.0).min(2.0) } else { 0.0 };
    let width = HudBatch::text_width(a, label);
    let start = b.vertices.len();
    b.text(
        a,
        label,
        x + (w - width) / 2.0,
        y + (h - 8.0) / 2.0 + offset,
        color,
        false,
    );
    let scale = (w * 0.8 / width.max(1.0)).min(h * 0.5 / 8.0).min(1.0);
    b.scale_since(start, x + w / 2.0, y + h / 2.0 + offset, scale, scale);
}
fn circle(
    b: &mut HudBatch,
    a: &HudAtlas,
    center: [f32; 2],
    radius: f32,
    color: u32,
    filled: bool,
) {
    static POINTS: std::sync::OnceLock<[[f32; 2]; 65]> = std::sync::OnceLock::new();
    let points = POINTS.get_or_init(|| {
        std::array::from_fn(|i| {
            let angle = i as f32 / 64.0 * std::f32::consts::TAU;
            [angle.cos(), angle.sin()]
        })
    });
    let outer = if filled { radius } else { radius + 0.7 };
    let inner = if filled { 0.0 } else { (radius - 0.7).max(0.0) };
    for segment in points.windows(2) {
        let point = |p: [f32; 2], r| [center[0] + p[0] * r, center[1] + p[1] * r];
        b.fill_quad(
            a,
            [
                point(segment[0], outer),
                point(segment[1], outer),
                point(segment[1], inner),
                point(segment[0], inner),
            ],
            color,
        );
    }
}
fn unit(value: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(0.0, 1.0)
    } else {
        fallback
    }
}
fn dim(color: u32) -> u32 {
    rgba(160, 160, 160, color.to_le_bytes()[3])
}
fn valid_bounds([x, y, w, h]: [f32; 4]) -> bool {
    [x, y, w, h, x + w, y + h].iter().all(|v| v.is_finite()) && w > 0.0 && h > 0.0
}
