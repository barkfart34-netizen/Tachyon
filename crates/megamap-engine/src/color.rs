//! Colour helpers. Map pixels are `0xAARRGGBB`; alpha 0 means "nothing known here".
//!
//! Baked map pixels carry a material in the alpha byte instead of opacity: `0xFF` is land, `0x80 | depth`
//! (depth 1..=15 blocks) is water. The view turns that into the depth tone and the shoreline at compose time,
//! and the image it outputs is always opaque.

pub const UNKNOWN: u32 = 0;
pub const OPAQUE: u32 = 0xFF00_0000;
pub const WATER_TAG: u32 = 0x80;

/// A water pixel: colour plus depth (clamped to 1..=15) in the alpha byte.
#[inline]
pub fn water(rgb: u32, depth: u32) -> u32 {
    (WATER_TAG | depth.clamp(1, 15)) << 24 | (rgb & 0xFF_FFFF)
}

/// Water depth of a baked pixel; 0 for land and unknown.
#[inline]
pub fn water_depth(c: u32) -> u32 {
    let a = c >> 24;
    if a & 0xF0 == WATER_TAG { a & 0x0F } else { 0 }
}

/// A land pixel lit by block light `light` (0..=15, torches, lava, glowstone): the alpha byte is
/// `0xF0 | (15 - light)`, so unlit land stays `0xFF` and maps saved before the light existed read as unlit.
#[inline]
pub fn land(rgb: u32, light: u32) -> u32 {
    (0xF0 | (15 - light.min(15))) << 24 | (rgb & 0xFF_FFFF)
}

/// Block light (0..=15) of a baked land pixel; 0 for water, unknown and unlit land.
#[inline]
pub fn block_light(c: u32) -> u32 {
    let a = c >> 24;
    if a & 0xF0 == 0xF0 { 15 - (a & 0x0F) } else { 0 }
}

/// Per-block brightness jitter (fixed point around 256), deterministic from the block position, so flat ground
/// reads as a surface instead of a flat fill. `amp` is the half range (10 = about +-4%).
#[inline]
pub fn jitter(x: i32, z: i32, amp: u32) -> u32 {
    let mut h = (x as u32).wrapping_mul(0x9E37_79B1) ^ (z as u32).wrapping_mul(0x85EB_CA77);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^= h >> 12;
    256 - amp + (h % (2 * amp + 1))
}

/// Linear blend of two colours in 0..=256, alpha taken from `a`.
#[inline]
pub fn lerp(a: u32, b: u32, t: u32) -> u32 {
    (a & 0xFF00_0000) | (mix(a, b, t) & 0xFF_FFFF)
}

/// Adds `add` (0xRRGGBB, scaled by `t`/256) to `c`, saturating; alpha kept.
#[inline]
pub fn add(c: u32, add: u32, t: u32) -> u32 {
    let [a, r, g, b] = parts(c);
    let [_, ar, ag, ab] = parts(add);
    let s = |x: u32, y: u32| (x + ((y * t) >> 8)).min(255);
    a << 24 | s(r, ar) << 16 | s(g, ag) << 8 | s(b, ab)
}

#[inline]
pub fn rgb(r: u8, g: u8, b: u8) -> u32 {
    OPAQUE | (r as u32) << 16 | (g as u32) << 8 | b as u32
}

#[inline]
pub fn parts(c: u32) -> [u32; 4] {
    [c >> 24, (c >> 16) & 0xFF, (c >> 8) & 0xFF, c & 0xFF]
}

/// `c` times `f` (fixed point, 256 = 1.0) per channel, alpha kept.
#[inline]
pub fn shade(c: u32, f: u32) -> u32 {
    let [a, r, g, b] = parts(c);
    let m = |v: u32| ((v * f) >> 8).min(255);
    a << 24 | m(r) << 16 | m(g) << 8 | m(b)
}

/// Channel-wise multiply of `c` by tint `t` (0xRRGGBB), as vanilla tints a grey grass texture.
#[inline]
pub fn multiply(c: u32, t: u32) -> u32 {
    let [a, r, g, b] = parts(c);
    let [_, tr, tg, tb] = parts(t);
    a << 24 | (r * tr / 255) << 16 | (g * tg / 255) << 8 | (b * tb / 255)
}

/// `a` blended towards `b` by `t` (0..=256), result opaque.
#[inline]
pub fn mix(a: u32, b: u32, t: u32) -> u32 {
    let [_, ar, ag, ab] = parts(a);
    let [_, br, bg, bb] = parts(b);
    let l = |x: u32, y: u32| (x * (256 - t) + y * t) >> 8;
    OPAQUE | l(ar, br) << 16 | l(ag, bg) << 8 | l(ab, bb)
}

/// Average of up to four pixels, skipping unknown ones; unknown when all four are. The material follows the
/// majority: two or more water texels give water with their mean depth, otherwise land with the brightest block
/// light of the four (a torch stays visible zoomed out).
#[inline]
pub fn average4(p: [u32; 4]) -> u32 {
    let (mut n, mut r, mut g, mut b, mut wn, mut wd, mut lit) = (0u32, 0u32, 0u32, 0u32, 0u32, 0u32, 0u32);
    for c in p {
        if c >> 24 != 0 {
            let [_, cr, cg, cb] = parts(c);
            n += 1;
            r += cr;
            g += cg;
            b += cb;
            let d = water_depth(c);
            if d > 0 {
                wn += 1;
                wd += d;
            }
            lit = lit.max(block_light(c));
        }
    }
    if n == 0 {
        return UNKNOWN;
    }
    let rgb = (r / n) << 16 | (g / n) << 8 | (b / n);
    if wn >= 2 { water(rgb, wd / wn) } else { land(rgb, lit) }
}

/// ARGB to the RGBA bytes `native_mod_image` takes.
#[inline]
pub fn to_rgba(c: u32) -> [u8; 4] {
    let [a, r, g, b] = parts(c);
    [r as u8, g as u8, b as u8, a as u8]
}

/// Eight waypoint colours, picked to stay apart on green, blue and grey terrain.
pub const PALETTE: [u32; 8] = [
    0xFFE5_3935, // red
    0xFFFF_B300, // amber
    0xFFFD_D835, // yellow
    0xFF43_A047, // green
    0xFF00_ACC1, // cyan
    0xFF1E_88E5, // blue
    0xFF8E_24AA, // purple
    0xFFF5_F5F5, // white
];

/// The palette colours' names, as lang keys.
pub const PALETTE_NAMES: [&str; 8] = [
    "megamap.color_name.red",
    "megamap.color_name.amber",
    "megamap.color_name.yellow",
    "megamap.color_name.green",
    "megamap.color_name.cyan",
    "megamap.color_name.blue",
    "megamap.color_name.purple",
    "megamap.color_name.white",
];
