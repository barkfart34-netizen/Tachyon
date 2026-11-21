//! megamap's engine: everything that does not touch Horizon. The glue in `lib.rs` feeds it the game's data
//! through the small traits below; the `engine-tests` crate in the work folder includes this module by path
//! and tests it without building the game.
#![allow(dead_code)]

pub mod autozoom;
pub mod baker;
pub mod clouds;
pub mod preview;
pub mod portrait;
pub mod color;
pub mod detail;
pub mod entities;
pub mod far;
pub mod hlod;
pub mod lang;
pub mod layers;
pub mod palette;
pub mod icons;
pub mod infoblock;
pub mod radar;
pub mod region;
pub mod reveal;
pub mod settings;
pub mod store;
pub mod style;
pub mod trail;
pub mod ui;
pub mod view;
pub mod waypoints;
