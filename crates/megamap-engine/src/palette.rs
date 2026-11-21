//! Block colours from the active resource packs: the alpha-weighted average of the texture a block shows on top,
//! with its tint source (grass, foliage, water) kept so the baker still multiplies it by the biome colour. The
//! glue reads the textures on a worker thread (`BlockStates` + `ClientResources`) and calls `override_table`; the
//! result is cached by `pack_hash` (resources generation, pack list, shader pack).

use super::baker::{ColorTable, MC_GRASS, MC_PLANT, MC_WATER};

/// A map colour id no vanilla `MapColor` uses: "plain colour, no biome tint" (a flower whose vanilla map colour is
/// `PLANT` but whose texture is not tinted).
pub const MC_PLAIN: u8 = 63;

/// Colour style: 0 vanilla map colours, 1 resource pack textures, 2 auto (textures when a resource pack is on).
pub const STYLES: usize = 3;

/// How a texture is tinted in the world.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint {
    None,
    Grass,
    Foliage,
    Water,
    /// A fixed multiplier (`0xRRGGBB`).
    Fixed(u32),
}

/// One block state's look from its texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sample {
    pub id: u32,
    /// Average colour, `0xRRGGBB`, before the tint.
    pub rgb: u32,
    pub tint: Tint,
}

/// Alpha-weighted average colour of an RGBA texture (`0xRRGGBB`); `None` when it is fully clear. Only the first
/// square frame of an animated strip counts.
pub fn average(width: u32, height: u32, rgba: &[u8]) -> Option<u32> {
    let w = width as usize;
    let h = (height.min(width.max(1)) as usize).min(rgba.len() / 4 / w.max(1));
    let (mut r, mut g, mut b, mut a) = (0u64, 0u64, 0u64, 0u64);
    for px in rgba[..w * h * 4].chunks_exact(4) {
        let al = px[3] as u64;
        r += px[0] as u64 * al;
        g += px[1] as u64 * al;
        b += px[2] as u64 * al;
        a += al;
    }
    if a == 0 {
        return None;
    }
    Some(((r / a) as u32) << 16 | ((g / a) as u32) << 8 | (b / a) as u32)
}

/// The map colour id a sample takes, from the vanilla one: water stays water (the baker's depth walk needs it), a
/// tinted texture takes the biome tint's id, an untinted one drops the vanilla tint.
pub fn map_id(vanilla: u8, tint: Tint) -> u8 {
    if vanilla == MC_WATER {
        return MC_WATER;
    }
    match tint {
        Tint::Grass => MC_GRASS,
        Tint::Foliage => MC_PLANT,
        Tint::Water => MC_WATER,
        Tint::None | Tint::Fixed(_) if vanilla == MC_GRASS || vanilla == MC_PLANT => MC_PLAIN,
        _ => vanilla,
    }
}

/// The colour a sample stores: the average, times a fixed tint.
pub fn sample_rgb(s: &Sample) -> u32 {
    match s.tint {
        Tint::Fixed(t) => super::color::multiply(super::color::OPAQUE | s.rgb, t) & 0xFF_FFFF,
        _ => s.rgb,
    }
}

/// A slight lift of contrast and warmth so the textures read like a shader pack's lit world (there is no shader
/// API for mods: the map only knows a pack is on).
pub fn shader_tint(rgb: u32) -> u32 {
    let ch = |s: u32, k: f32, add: f32| {
        let v = ((rgb >> s) & 0xFF) as f32;
        (((v - 128.0) * k + 128.0 + add).clamp(0.0, 255.0) as u32) << s
    };
    ch(16, 1.08, 4.0) | ch(8, 1.08, 1.0) | ch(0, 1.08, -4.0)
}

/// Whether the textures replace the vanilla colours for `style` (`STYLES`): auto takes them with any resource pack
/// on (vanilla textures average close to the vanilla map colours, so there is no gain without one).
pub fn use_pack(style: u8, packs_on: bool) -> bool {
    match style {
        0 => false,
        1 => true,
        _ => packs_on,
    }
}

/// Writes the samples over a copy of the vanilla table. States without a sample, air and `NONE` stay as they were.
pub fn override_table(vanilla: &ColorTable, samples: &[Sample], shader: bool) -> ColorTable {
    let mut t = vanilla.clone();
    for s in samples {
        let Some((mc, _)) = vanilla.get(s.id) else { continue };
        let mut rgb = sample_rgb(s);
        if shader {
            rgb = shader_tint(rgb);
        }
        t.set(s.id, map_id(mc, s.tint), rgb);
    }
    t
}

/// Cache key of a palette: the resources generation, the pack list (in order) and the shader pack.
pub fn pack_hash(generation: u64, packs: &[String], shader: &str, style: u8) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let mut eat = |b: &[u8]| {
        for &x in b {
            h ^= x as u64;
            h = h.wrapping_mul(0x0100_0000_01b3);
        }
        h ^= 0xff;
        h = h.wrapping_mul(0x0100_0000_01b3);
    };
    eat(&generation.to_le_bytes());
    for p in packs {
        eat(p.as_bytes());
    }
    eat(shader.as_bytes());
    eat(&[style]);
    h
}

/// Caches decoded texture averages by texture name across the states that share one (every stair, slab and wall
/// of a material uses the same few textures).
#[derive(Default)]
pub struct AverageCache {
    map: std::collections::HashMap<String, Option<u32>>,
    pub decoded: usize,
}

impl AverageCache {
    pub fn get(&mut self, name: &str, load: impl FnOnce(&str) -> Option<(u32, u32, Vec<u8>)>) -> Option<u32> {
        if let Some(v) = self.map.get(name) {
            return *v;
        }
        self.decoded += 1;
        let v = load(name).and_then(|(w, h, px)| average(w, h, &px));
        self.map.insert(name.to_string(), v);
        v
    }
}
