//! Hashes of rendered frames (map and cloud fog) over a patchy world: explored ground, far terrain, holes, a
//! reveal ramp, coarse far terrain; every mip level, turned, round, smooth or not. For checking that an
//! optimisation leaves every pixel as it was (`tests::golden_frames_unchanged`, the `golden` example).

use crate::engine::baker::{bake_chunk, Layer};
use crate::engine::clouds::{Clouds, Look as CLook, Params};
use crate::engine::region::{fnv1a, Region};
use crate::engine::store::Store;
use crate::engine::view::{self, Lookup, View};
use crate::fake::{table, Plains, World};

struct Look<'a> {
    store: &'a Store,
    far: &'a Store,
    coarse: bool,
    reveal: bool,
}
impl Lookup for Look<'_> {
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.store.get(rx, rz)
    }
    fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.far.get(rx, rz)
    }
    fn far(&self, x: i32, z: i32) -> u32 {
        if self.coarse && (x >> 8) & 3 == 1 && (z >> 8) & 1 == 0 { 0xFF40_7030 | ((x >> 6) as u32 & 0xF) } else { 0 }
    }
    fn reveal(&self, x: i32, z: i32) -> u32 {
        if self.reveal { ((x + z * 3).rem_euclid(700) as u32).min(256) } else { 256 }
    }
}

/// Renders the scenes; the combined hash, and one line per frame through `line`.
pub fn run(mut line: impl FnMut(String)) -> u32 {
    let w = World { r: 32, cave: false };
    let t = table();
    let mut store = Store::new(512);
    let mut far = Store::new(512);
    for rz in -3..=3i32 {
        for rx in -3..=3i32 {
            let mut f = Region::new(rx, rz);
            let mut e = Region::new(rx, rz);
            for cz in 0..32i32 {
                for cx in 0..32i32 {
                    let c = bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap();
                    let k = (cx * 7 + cz * 13 + rx * 5 + rz * 3).rem_euclid(11);
                    if k != 0 && (rx + rz) % 3 != 0 {
                        f.put_chunk(&c);
                    }
                    if rx.abs() <= 1 && rz.abs() <= 1 && k > 3 && (cx - 16).pow(2) + (cz - 16).pow(2) < 300 {
                        e.put_chunk(&c);
                    }
                }
            }
            f.ensure_mips();
            e.ensure_mips();
            if (rx + rz) % 3 != 0 {
                far.regions.insert((rx, rz), f);
            }
            if rx.abs() <= 1 && rz.abs() <= 1 {
                store.regions.insert((rx, rz), e);
            }
        }
    }
    let (iw, ih) = (640usize, 360usize);
    let mut buf = vec![0u8; iw * ih * 4];
    let mut all = 0u32;
    for (ci, center) in [[100.3, 60.7], [-900.0, 1300.5], [5000.0, -4000.0]].into_iter().enumerate() {
        for bpp in [0.25f64, 0.7, 1.0, 2.0, 3.3, 8.0, 16.0, 31.0, 64.0, 100.0] {
            for mode in 0..12u32 {
                let mut v = View::new(center, bpp);
                v.style.smooth = mode & 1 != 0;
                v.style.mark_unknown = mode & 2 != 0;
                v.fog = mode < 8;
                v.rotation = if mode % 4 == 3 { 0.6 } else { 0.0 };
                v.round = mode == 5;
                v.style.grid = if mode == 6 { 3 } else { 0 };
                v.style.contours = if mode == 9 { 8 } else { 0 };
                v.style.light = if mode == 10 { 100 } else { 256 };
                v.style.far_fog = mode == 11;
                let lk = Look { store: &store, far: &far, coarse: mode == 4 || mode == 7, reveal: mode >= 8 };
                buf.fill(7);
                view::render(&mut buf, iw, ih, &v, &lk);
                let hr = fnv1a(&buf);
                let mut hc = 0;
                if v.style.mark_unknown && v.rotation == 0.0 && ci < 2 {
                    view::settle_revealing(&mut buf);
                    let base = buf.clone();
                    for (q, look) in [(1u8, CLook::Overworld), (3, CLook::Overworld), (2, CLook::Nether)] {
                        let p = Params { quality: q, look, reach: 10.0, ..Params::default() };
                        let mut c = Clouds::new();
                        let mut img = base.clone();
                        if c.update(&base, iw, ih, &v, 3.5, &p) {
                            c.composite(&mut img, iw, ih, &p, 3.5);
                        }
                        hc ^= fnv1a(&img).rotate_left(q as u32);
                    }
                }
                all = all.rotate_left(5) ^ hr ^ hc.rotate_left(13);
                line(format!("c{ci} bpp {bpp} mode {mode}: {hr:08x} {hc:08x}"));
            }
        }
    }
    all
}
