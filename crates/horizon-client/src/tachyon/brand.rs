//! Tachyon: the name and the look of its launchers, in one place so a new
//! name or palette is one change. The game underneath is Horizon
//! (horizon-rust/horizon, MIT); its crates keep their names so Horizon's
//! changes keep merging in.
use horizon_theme::Palette;

pub const NAME: &str = "Tachyon";
/// Under the wordmark.
pub const TAGLINE: &str = "Minecraft: Java Edition, native on Android. Built on Horizon, in Rust.";
/// Where the source lives (private), for the About row.
pub const SOURCE: &str = "github.com/Rxuser2/Tachyon";
/// The folder in the resources holding the content mods Tachyon carries
/// built in (the modpack's, from `tachyon/content`).
pub const CONTENT: &str = "tachyon-mods";

/// Obsidian matte and burnt copper: the launcher's palette, in the design
/// system's roles (`horizon_theme::Palette`).
pub const P: Palette = Palette {
    page: 0x0b0b0e,
    plate_soft: 0x17171d,
    plate: 0x202028,
    surface: 0x111115,
    rule: 0x24242d,
    ink: 0xf2f2f5,
    ink_soft: 0xa6a6b2,
    muted: 0x6f6f7b,
    accent: 0xe07a3d,
    on_accent: 0x120a05,
    danger: 0xe05555,
    success: 0x3ddc84,
    dark: true,
};

/// The copper highlight bar: warm burnt orange fading into deep ember.
pub const STREAK: [u32; 2] = [0xe07a3d, 0xb85224];
