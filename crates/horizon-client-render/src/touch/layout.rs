//! Per-mode layouts from horizon-android's `touch/layout.rs`. Centers are
//! normalized; sizes are fractions of surface height, not Zalith's units.
use serde::{Deserialize, Serialize};

pub const FORMAT_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Mode {
    #[default]
    Crosshair,
    Tap,
    Dpad,
}
impl Mode {
    pub const ALL: [Self; 3] = [Self::Crosshair, Self::Tap, Self::Dpad];
    pub const SCHEMES: [(Self, bool, &str); 4] = [
        (Self::Crosshair, false, "Joystick & aim crosshair"),
        (Self::Tap, false, "Joystick & tap to interact"),
        (Self::Dpad, false, "D-Pad & tap to interact"),
        (Self::Dpad, true, "D-Pad & aim crosshair"),
    ];
    pub fn index(self) -> usize {
        match self {
            Self::Crosshair => 0,
            Self::Tap => 1,
            Self::Dpad => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Control {
    Move,
    Sprint,
    Jump,
    Sneak,
    Attack,
    Use,
    Chat,
    Pause,
    Settings,
    Inventory,
    Debug,
    Perspective,
    Offhand,
    Shield,
}
impl Control {
    pub const ALL: [Self; 14] = [
        Self::Move,
        Self::Sprint,
        Self::Jump,
        Self::Sneak,
        Self::Attack,
        Self::Use,
        Self::Chat,
        Self::Pause,
        Self::Settings,
        Self::Inventory,
        Self::Debug,
        Self::Perspective,
        Self::Offhand,
        Self::Shield,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Placement {
    pub center: [f32; 2],
    pub size: f32,
    pub opacity: f32,
}
impl Placement {
    pub(super) fn new(x: f32, y: f32, size: f32) -> Self {
        Self {
            center: [x, y],
            size,
            opacity: 0.65,
        }
    }
    pub fn rect(self, aspect: f32) -> [f32; 4] {
        let sx = self.size / aspect.max(0.1);
        [
            self.center[0] - sx / 2.0,
            self.center[1] - self.size / 2.0,
            sx,
            self.size,
        ]
    }
    pub fn sanitize(&mut self) {
        self.size = bounded(self.size, 0.045, 0.45, 0.15);
        self.opacity = bounded(self.opacity, 0.0, 1.0, 0.65);
        for c in &mut self.center {
            *c = bounded(*c, 0.025, 0.975, 0.5);
        }
    }
    /// Fit a sanitized copy to the viewport, following touch/custom.rs's fit_placement.
    pub fn fitted(mut self, aspect: f32, scale: f32) -> Self {
        self.sanitize();
        let aspect = aspect.max(0.1);
        self.size = (self.size * bounded(scale, 0.5, 2.0, 1.0))
            .min(aspect)
            .min(1.0);
        let half = [self.size / aspect / 2.0, self.size / 2.0];
        for (i, half) in half.into_iter().enumerate() {
            self.center[i] = self.center[i].clamp(half, 1.0 - half);
        }
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    #[serde(deserialize_with = "deserialize_version")]
    pub version: u32,
    pub mode: Mode,
    pub split_controls: bool,
    pub contextual_interact: bool,
    pub left_handed: bool,
    pub sprint_with_joystick: bool,
    pub swap_jump_sneak: bool,
    pub sensitivity: f32,
    pub sensitivity_horizontal: f32,
    pub sensitivity_vertical: f32,
    pub offhand_button: bool,
    pub shield_button: bool,
    pub circle_size: f32,
    pub button_scale: f32,
    pub custom_buttons: Vec<super::CustomButton>,
    pub analog_movement: bool,
    pub sprint_button: bool,
    pub sprint_toggle: bool,
    pub inventory_controls: bool,
    pub virtual_mouse: bool,
    pub virtual_mouse_sensitivity: f32,
    pub crosshair_gestures: bool,
    pub attack_indicator_left: bool,
    /// Shared across modes: 0 follows the scheme, 1 shows, 2 hides.
    #[serde(deserialize_with = "deserialize_visibility")]
    pub button_visibility: [u8; 14],
    pub horizontal_margin: f32,
    pub vertical_margin: f32,
    #[serde(deserialize_with = "deserialize_layouts")]
    pub layouts: [[Placement; 14]; 3],
}

impl Default for Settings {
    fn default() -> Self {
        let base = [
            Placement::new(0.13, 0.76, 0.32),
            Placement::new(0.72, 0.78, 0.105),
            Placement::new(0.92, 0.78, 0.16),
            Placement::new(0.81, 0.86, 0.115),
            Placement::new(0.80, 0.64, 0.16),
            Placement::new(0.92, 0.52, 0.16),
            Placement::new(0.46, 0.055, 0.075),
            Placement::new(0.50, 0.055, 0.075),
            Placement::new(0.54, 0.055, 0.075),
            Placement::new(0.73, 0.95, 0.075),
            Placement::new(0.42, 0.055, 0.075),
            Placement::new(0.58, 0.055, 0.075),
            Placement::new(0.80, 0.96, 0.075),
            Placement::new(0.92, 0.36, 0.12),
        ];
        let mut layouts = [base; 3];
        layouts[2][Control::Sneak as usize] = Placement::new(0.13, 0.76, 0.10);
        layouts[2][Control::Jump as usize] = Placement::new(0.90, 0.78, 0.14);
        Self {
            version: FORMAT_VERSION,
            mode: Mode::Crosshair,
            split_controls: false,
            contextual_interact: false,
            left_handed: false,
            sprint_with_joystick: false,
            swap_jump_sneak: false,
            sensitivity: 1.0,
            sensitivity_horizontal: 1.0,
            sensitivity_vertical: 1.0,
            offhand_button: false,
            shield_button: false,
            circle_size: 32.0,
            button_scale: 1.0,
            custom_buttons: Vec::new(),
            analog_movement: false,
            sprint_button: true,
            sprint_toggle: false,
            inventory_controls: false,
            virtual_mouse: false,
            virtual_mouse_sensitivity: 1.0,
            crosshair_gestures: false,
            attack_indicator_left: true,
            button_visibility: [0; 14],
            horizontal_margin: 0.0,
            vertical_margin: 0.0,
            layouts,
        }
    }
}

fn deserialize_version<'de, D: serde::Deserializer<'de>>(d: D) -> Result<u32, D::Error> {
    let version = u32::deserialize(d)?;
    if version != FORMAT_VERSION {
        return Err(serde::de::Error::custom(
            "unsupported Bedrock touch format version",
        ));
    }
    Ok(version)
}

// Append new controls without resetting a player's older layout.
fn deserialize_layouts<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<[[Placement; 14]; 3], D::Error> {
    let rows = Vec::<Vec<Placement>>::deserialize(d)?;
    if rows.len() != 3 || rows.iter().any(|r| !matches!(r.len(), 10 | 12 | 14)) {
        return Err(serde::de::Error::custom(
            "expected three touch layouts of 10, 12 or 14 controls",
        ));
    }
    let mut layouts = Settings::default().layouts;
    for (output, input) in layouts.iter_mut().zip(rows) {
        output[..input.len()].copy_from_slice(&input);
    }
    Ok(layouts)
}

fn deserialize_visibility<'de, D: serde::Deserializer<'de>>(d: D) -> Result<[u8; 14], D::Error> {
    let old = Vec::<u8>::deserialize(d)?;
    if !matches!(old.len(), 12 | 14) {
        return Err(serde::de::Error::custom("expected 12 or 14 visibility values"));
    }
    let mut values = [0; 14];
    values[..old.len()].copy_from_slice(&old);
    Ok(values)
}

impl Settings {
    /// Decode and sanitize data supplied by the caller, without filesystem I/O.
    pub fn parse(json: &str) -> Result<Self, serde_json::Error> {
        let mut settings: Self = serde_json::from_str(json)?;
        settings.sanitize();
        Ok(settings)
    }
    /// Encode a sanitized copy; an editor's preview is not changed or saved.
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        let mut settings = self.clone();
        settings.sanitize();
        serde_json::to_string_pretty(&settings)
    }
    pub fn sanitize(&mut self) {
        self.version = FORMAT_VERSION;
        if self.mode == Mode::Tap && self.split_controls {
            self.mode = Mode::Crosshair;
            self.split_controls = false;
            self.crosshair_gestures = true;
        }
        for value in [
            &mut self.sensitivity,
            &mut self.sensitivity_horizontal,
            &mut self.sensitivity_vertical,
        ] {
            *value = bounded(*value, 0.1, 3.0, 1.0);
        }
        self.virtual_mouse_sensitivity = bounded(self.virtual_mouse_sensitivity, 0.25, 3.0, 1.0);
        self.button_scale = bounded(self.button_scale, 0.5, 2.0, 1.0);
        self.circle_size = bounded(self.circle_size, 16.0, 64.0, 32.0);
        for value in [&mut self.horizontal_margin, &mut self.vertical_margin] {
            *value = bounded(*value, 0.0, 0.12, 0.0);
        }
        for value in &mut self.button_visibility {
            if *value > 2 {
                *value = 0;
            }
        }
        self.custom_buttons.truncate(32);
        self.custom_buttons
            .retain(|b| super::CustomButton::valid_token(&b.key));
        for button in &mut self.custom_buttons {
            for p in &mut button.layouts {
                p.sanitize();
            }
        }
        for p in self.layouts.iter_mut().flatten() {
            p.sanitize();
        }
    }
    pub fn crosshair(&self) -> bool {
        self.mode == Mode::Crosshair || self.split_controls
    }
    pub fn visible(&self, c: Control) -> bool {
        match self.button_visibility[c as usize] {
            1 => return true,
            2 => return false,
            _ => {}
        }
        match c {
            Control::Offhand => self.offhand_button,
            Control::Shield => self.shield_button,
            Control::Attack | Control::Use => self.crosshair(),
            Control::Sprint => {
                self.sprint_button && self.mode != Mode::Dpad && !self.sprint_with_joystick
            }
            _ => true,
        }
    }
    pub fn slot(&self, c: Control) -> usize {
        match c {
            Control::Jump if self.mode == Mode::Dpad && self.swap_jump_sneak => {
                Control::Sneak as usize
            }
            Control::Sneak if self.mode == Mode::Dpad && self.swap_jump_sneak => {
                Control::Jump as usize
            }
            _ => c as usize,
        }
    }
    pub fn placement(&self, c: Control) -> Placement {
        let mut p = self.layouts[self.mode.index()][self.slot(c)];
        if self.left_handed {
            p.center[0] = 1.0 - p.center[0];
        }
        p
    }
}

fn bounded(value: f32, min: f32, max: f32, fallback: f32) -> f32 {
    if value.is_finite() {
        value.clamp(min, max)
    } else {
        fallback
    }
}

pub fn contains([x, y, w, h]: [f32; 4], p: [f32; 2]) -> bool {
    (x..=x + w).contains(&p[0]) && (y..=y + h).contains(&p[1])
}
