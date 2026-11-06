//! Bedrock-style touch formats, movement/button input and HUD, following horizon-android's
//! `TouchControls`, `touch/layout.rs` and `touch/custom.rs` (MIT, THIRD_PARTY_NOTICES.md).
//! No game, platform or file I/O. World gestures belong to later stages.
use crate::gamepad::{self, Button, Edges, PadState};
use std::{
    collections::{BTreeMap, VecDeque},
    time::Duration,
};

mod custom;
pub mod hud;
pub mod layout;
pub use custom::CustomButton;
pub use layout::{Control, Mode, Placement, Settings};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TouchScreen {
    #[default]
    Inactive,
    Playing,
    Inventory,
    Menu,
}

/// Owned gameplay values supplied before a finger goes down. Item-use intent
/// is computed read-only by the game and copied into that gesture, not polled
/// again after the held item changes.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TouchContext {
    pub screen: TouchScreen,
    pub held_item: Option<String>,
    pub hold_uses_item: bool,
    pub projectile_crosshair: bool,
    pub force_crosshair: bool,
    pub shield_available: bool,
    pub context_use: bool,
    pub vertical_movement: bool,
    pub swimming: bool,
    pub mounted: bool,
    pub sneaking: bool,
}

/// A single world tap owns its aim and the item-use intent from finger down.
/// Aim is a normalized screen point, to be projected by the gameplay adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TouchTap {
    pub aim: [f32; 2],
    pub held_use: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TouchRequest {
    Chat,
    Settings,
    Debug,
    Perspective,
    /// One canonical swap action, not a held custom key.
    Offhand,
}

/// Taken out of the overlay before borrowing the game. Pad buttons describe
/// fixed touch actions, never controller bindings. Look is in yaw/pitch degrees.
/// The adapter must retain aim through its consuming tick and consume queued
/// taps once, in order. Cancellation or an engine change must discard old frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TouchFrame {
    pub pad: PadState,
    /// Left/forward input; eight keyboard directions unless analog is enabled.
    pub movement: Option<[f32; 2]>,
    pub edges: Vec<(Button, bool)>,
    /// Ground D-pad crouch policy, captured at down rather than at release.
    pub sneak_toggle: bool,
    pub look_degrees: [f32; 2],
    pub aim: Option<[f32; 2]>,
    pub taps: VecDeque<TouchTap>,
    pub key_events: Vec<(String, bool)>,
    pub requests: Vec<TouchRequest>,
    pub holding: bool,
    pub holding_use: bool,
    pub shield_held: bool,
}

/// Returned after the game borrow ends. Latch applies to the current committed
/// hold; mining progress is 0..1. Neither value performs gameplay itself.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TouchFeedback {
    pub latch_use: bool,
    pub break_progress: f32,
}

#[derive(Clone, Copy)]
enum Role {
    Move([f32; 2]),
    Button(Button),
    Shield,
    SneakToggle,
    Ignore,
}

/// Only control fingers are claimed at this stage. World gestures come later.
/// Clear and dispatch the returned releases before changing settings or screens.
#[derive(Default)]
pub struct TouchControls {
    pub settings: Settings,
    pub context: TouchContext,
    fingers: BTreeMap<u64, (Role, [f32; 2])>,
    pending: PadState,
    pending_sneak_toggle: bool,
    edges: Edges,
    requests: Vec<TouchRequest>,
    aspect: f32,
    surface: [u32; 2],
    last_sneak_tap: Option<Duration>,
    sprint_latched: bool,
}

impl TouchControls {
    pub fn viewport(&mut self, width: u32, height: u32) {
        self.surface = [width.max(1), height.max(1)];
        self.aspect = (width.max(1) as f32 / height.max(1) as f32).max(0.1);
    }

    fn aspect(&self) -> f32 {
        if self.aspect > 0.0 {
            self.aspect
        } else {
            16.0 / 9.0
        }
    }

    pub fn visible(&self, control: Control) -> bool {
        self.settings.visible(control)
            && (control != Control::Shield || self.context.shield_available)
    }

    /// Shared hit and HUD geometry, without changing the saved layout.
    pub fn placement(&self, control: Control) -> Placement {
        self.settings
            .placement(control)
            .fitted(self.aspect(), self.settings.button_scale)
    }

    pub fn control_at(&self, point: [f32; 2]) -> Option<Control> {
        Control::ALL.into_iter().rev().find(|c| {
            self.visible(*c) && layout::contains(self.placement(*c).rect(self.aspect()), point)
        })
    }

    pub fn owns_pointer(&self, id: u64) -> bool {
        self.fingers.contains_key(&id)
    }

    fn button(control: Control) -> Option<Button> {
        Some(match control {
            Control::Sprint => Button::LeftStick,
            Control::Jump => Button::South,
            Control::Sneak => Button::RightStick,
            Control::Attack => Button::RightTrigger,
            Control::Use | Control::Shield => Button::LeftTrigger,
            Control::Pause => Button::Start,
            Control::Inventory => Button::North,
            _ => return None,
        })
    }

    fn press(state: &mut PadState, button: Button) {
        match button {
            Button::LeftTrigger => state.triggers[0] = 1.0,
            Button::RightTrigger => state.triggers[1] = 1.0,
            _ => state.set(button, true),
        }
    }

    /// Fractions of the drawable surface; `now` is time since a caller-owned epoch.
    /// A role never changes while that finger is down, even outside its button.
    pub fn down(&mut self, id: u64, point: [f32; 2], now: Duration) -> bool {
        if self.owns_pointer(id) {
            return true;
        }
        let Some(control) = self.control_at(point) else {
            return false;
        };
        let role = match control {
            Control::Move => {
                if self.fingers.values().any(|(r, _)| matches!(r, Role::Move(_))) {
                    Role::Ignore
                } else {
                    Role::Move(self.placement(Control::Move).center)
                }
            }
            Control::Shield => {
                Self::press(&mut self.pending, Button::LeftTrigger);
                Role::Shield
            }
            Control::Sneak
                if self.settings.mode == Mode::Dpad
                    && !self.context.vertical_movement
                    && !self.context.swimming
                    && !self.context.mounted =>
            {
                // The classic D-pad crouches on the second tap, strictly before 300 ms.
                if self
                    .last_sneak_tap
                    .take()
                    .and_then(|t| now.checked_sub(t))
                    .is_some_and(|elapsed| elapsed < Duration::from_millis(300))
                {
                    Role::SneakToggle
                } else {
                    self.last_sneak_tap = Some(now);
                    Role::Ignore
                }
            }
            Control::Sprint if self.settings.sprint_toggle => {
                self.sprint_latched = !self.sprint_latched;
                Role::Ignore
            }
            other => match Self::button(other) {
                Some(button) => Role::Button(button),
                None => {
                    match other {
                        Control::Chat => self.requests.push(TouchRequest::Chat),
                        Control::Settings => self.requests.push(TouchRequest::Settings),
                        Control::Debug => self.requests.push(TouchRequest::Debug),
                        Control::Perspective => self.requests.push(TouchRequest::Perspective),
                        Control::Offhand => self.requests.push(TouchRequest::Offhand),
                        _ => {}
                    }
                    Role::Ignore
                }
            },
        };
        if let Role::Button(button) = role {
            Self::press(&mut self.pending, button);
        } else if matches!(role, Role::SneakToggle) {
            self.pending_sneak_toggle = true;
            Self::press(&mut self.pending, Button::RightStick);
        }
        self.fingers.insert(id, (role, point));
        true
    }

    pub fn moved(&mut self, id: u64, point: [f32; 2]) {
        if point.iter().all(|v| v.is_finite())
            && let Some((_, previous)) = self.fingers.get_mut(&id)
        {
            *previous = point;
        }
    }

    pub fn up(&mut self, id: u64) {
        self.fingers.remove(&id);
    }

    pub fn shield_held(&self) -> bool {
        self.fingers.values().any(|(r, _)| matches!(r, Role::Shield))
    }

    pub fn state(&self) -> PadState {
        let mut state = PadState::default();
        if self.settings.sprint_toggle && self.settings.sprint_button && self.sprint_latched {
            state.set(Button::LeftStick, true);
        }
        for (role, point) in self.fingers.values() {
            match role {
                Role::Move(anchor) => {
                    let radius = self.placement(Control::Move).size / 2.0;
                    let mut stick = [
                        (point[0] - anchor[0]) * self.aspect() / radius,
                        (anchor[1] - point[1]) / radius,
                    ];
                    if self.settings.mode == Mode::Dpad {
                        stick = stick.map(|v| if v.abs() < 1.0 / 3.0 { 0.0 } else { v.signum() });
                    }
                    let sprint = stick[1] > 1.0;
                    let length = stick[0].hypot(stick[1]).max(1.0);
                    state.left = stick.map(|v| v / length);
                    if self.settings.mode != Mode::Dpad
                        && self.settings.sprint_with_joystick
                        && sprint
                    {
                        state.set(Button::LeftStick, true);
                    }
                }
                Role::Button(button) => Self::press(&mut state, *button),
                Role::Shield => Self::press(&mut state, Button::LeftTrigger),
                Role::SneakToggle => Self::press(&mut state, Button::RightStick),
                Role::Ignore => {}
            }
        }
        state
    }

    /// Short button presses survive until one poll. The next poll releases them.
    /// Touch uses no controller deadzone, and defaults to Java's eight directions.
    pub fn poll_frame(&mut self) -> TouchFrame {
        let pad = self.state().merge(&std::mem::take(&mut self.pending));
        let movement = if self.settings.analog_movement {
            gamepad::move_vector(pad.left, 0.0)
        } else {
            gamepad::java_move_vector(pad.left, 0.0)
        };
        TouchFrame {
            pad,
            movement,
            edges: self.edges.update(&pad, 0.5),
            sneak_toggle: std::mem::take(&mut self.pending_sneak_toggle)
                || self.fingers.values().any(|(r, _)| matches!(r, Role::SneakToggle)),
            requests: std::mem::take(&mut self.requests),
            shield_held: self.shield_held(),
            ..TouchFrame::default()
        }
    }

    /// Cancel pending presses and requests, releasing only already emitted holds.
    pub fn clear(&mut self) -> TouchFrame {
        self.fingers.clear();
        self.pending = PadState::default();
        self.pending_sneak_toggle = false;
        self.requests.clear();
        self.last_sneak_tap = None;
        self.sprint_latched = false;
        TouchFrame {
            edges: self.edges.release_all(),
            ..TouchFrame::default()
        }
    }
}
