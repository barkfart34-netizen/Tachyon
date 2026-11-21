//! A small deterministic fake world for the tests and the benchmark.

use crate::engine::baker::{ChunkSource, ColorTable, Tints, MC_GRASS, MC_WATER};

pub const AIR: u32 = 0;
pub const STONE: u32 = 1;
pub const GRASS: u32 = 2;
pub const WATER: u32 = 3;
pub const SAND: u32 = 4;
pub const GLASS: u32 = 5;

pub fn table() -> ColorTable {
    let mut t = ColorTable::with_len(8);
    t.set(AIR, 0, 0);
    t.set(STONE, 11, 0x707070);
    t.set(GRASS, MC_GRASS, 0x7FB238);
    t.set(WATER, MC_WATER, 0x4040FF);
    t.set(SAND, 2, 0xF7E9A3);
    t.set(GLASS, 0, 0);
    t
}

/// Rolling hills with a sea below y=62, chunks `[-r, r)` loaded; `cave` adds a hollow at y 20..30.
pub struct World {
    pub r: i32,
    pub cave: bool,
}

impl World {
    pub fn height(&self, x: i32, z: i32) -> i32 {
        let h = 64.0 + 6.0 * ((x as f32) * 0.07).sin() + 5.0 * ((z as f32) * 0.05).cos();
        h as i32
    }
}

impl ChunkSource for World {
    fn block(&self, [x, y, z]: [i32; 3]) -> u32 {
        if !self.loaded([x, z]) || y < -64 {
            return AIR;
        }
        let h = self.height(x, z);
        if self.cave && (20..30).contains(&y) {
            return AIR;
        }
        if y < h - 3 {
            STONE
        } else if y < h {
            if h <= 62 { SAND } else { GRASS }
        } else if y < 62 {
            WATER
        } else {
            AIR
        }
    }
    fn biome(&self, _pos: [i32; 3]) -> Option<u16> {
        Some(1)
    }
    fn section_top(&self, [x, z]: [i32; 2]) -> i32 {
        ((self.height(x, z).max(62) >> 4) + 1) << 4
    }
    fn loaded(&self, [x, z]: [i32; 2]) -> bool {
        let (cx, cz) = (x >> 4, z >> 4);
        cx >= -self.r && cx < self.r && cz >= -self.r && cz < self.r
    }
    fn min_y(&self) -> i32 {
        -64
    }
}

pub struct Plains;
impl Tints for Plains {
    fn tint(&self, _b: u16, _x: i32, _z: i32) -> (u32, u32, u32) {
        (0x91BD59, 0x77AB2F, 0x3F76E4)
    }
}
