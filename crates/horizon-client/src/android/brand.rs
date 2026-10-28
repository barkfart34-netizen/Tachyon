//! Tachyon: the name and the look of the Android app, in one place so a new
//! name or palette is one change. The game underneath is Horizon
//! (horizon-rust/horizon, MIT); its crates keep their names so Horizon's
//! changes keep merging in.
use horizon_theme::Palette;

pub const NAME: &str = "Tachyon";
/// Under the wordmark.
pub const TAGLINE: &str = "Minecraft: Java Edition, native on Android. Built on Horizon, in Rust.";
/// Where the source lives (private), for the About row.
pub const SOURCE: &str = "github.com/Rxuser2/Tachyon";

/// Deep space and Cherenkov blue: the launcher's palette, in the design
/// system's roles (`horizon_theme::Palette`).
pub const P: Palette = Palette {
    page: 0x070a14,
    plate_soft: 0x0f1526,
    plate: 0x172038,
    surface: 0x0e1424,
    rule: 0x1b2440,
    ink: 0xeaf2ff,
    ink_soft: 0xbdd0ec,
    muted: 0x7b88a6,
    accent: 0x38d6ff,
    on_accent: 0x03111b,
    danger: 0xff6b6b,
    success: 0x3ddc97,
    dark: true,
};

/// The streak behind the wordmark: from the accent to violet.
pub const STREAK: [u32; 2] = [0x38d6ff, 0x7c5cff];
