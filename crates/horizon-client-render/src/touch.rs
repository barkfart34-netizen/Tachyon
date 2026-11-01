//! Bedrock-style touch formats and value contracts, following horizon-android's
//! `touch.rs`, `touch/layout.rs` and `touch/custom.rs`. No input is taken here.
use crate::gamepad::PadState;
use std::collections::VecDeque;

mod custom;
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
}

/// Taken out of the overlay before borrowing the game. Pad buttons describe
/// fixed touch actions, never controller bindings. Look is in yaw/pitch degrees.
/// The adapter must retain aim through its consuming tick and consume queued
/// taps once, in order. Cancellation or an engine change must discard old frames.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TouchFrame {
    pub pad: PadState,
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
