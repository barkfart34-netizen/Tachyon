//! The stored custom button from horizon-android's `touch/custom.rs`.
//! Key dispatch and per-finger ownership belong to later stages.
use super::layout::Placement;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CustomButton {
    pub key: String,
    pub layouts: [Placement; 3],
}

impl CustomButton {
    pub fn new(key: String, index: usize) -> Self {
        Self {
            key,
            layouts: [Placement::new(
                0.68 + (index % 3) as f32 * 0.1,
                0.35 + (index / 3 % 3) as f32 * 0.12,
                0.105,
            ); 3],
        }
    }
    pub fn valid_token(token: &str) -> bool {
        token == "key.keyboard.unknown"
            || crate::keybinds::key_code(token).is_some()
            || token
                .strip_prefix("action:")
                .is_some_and(|name| crate::keybinds::MAPPINGS.iter().any(|m| m.0 == name))
    }
}
