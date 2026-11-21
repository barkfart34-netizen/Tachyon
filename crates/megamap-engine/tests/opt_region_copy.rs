//! The lean `Region::copy_pixels` (no mips allocated) saves and draws exactly like the region it copies.
use megamap_engine::baker::{bake_chunk, Layer};
use megamap_engine::region::{Region, MIP_LEVELS};
pub use megamap_engine as engine;
#[path = "suite/fake.rs"]
mod fake;
use fake::{table, Plains, World};

fn region() -> Region {
    let (w, t) = (World { r: 12, cave: false }, table());
    let mut r = Region::new(0, 0);
    for cz in 0..11 {
        for cx in 0..12 {
            r.put_chunk(&bake_chunk(&w, &Plains, &t, cx, cz, Layer::Surface).unwrap());
        }
    }
    r
}

#[test]
fn copy_pixels_encodes_and_mips_like_the_original() {
    let mut r = region();
    r.saved_generation = 3;
    let mut c = r.copy_pixels();
    assert_eq!((c.rx, c.rz, c.explored, c.generation, c.saved_generation), (r.rx, r.rz, r.explored, r.generation, 3));
    assert_eq!(c.colors, r.colors);
    assert_eq!(c.heights, r.heights);
    // The save path: byte-identical files.
    assert_eq!(c.encode(), r.encode());
    // Drawn after all: the mips come out as the original's.
    r.ensure_mips();
    c.ensure_mips();
    for l in 0..MIP_LEVELS {
        assert_eq!(c.level(l), r.level(l), "colour mip {l}");
        assert_eq!(c.height_level(l), r.height_level(l), "height mip {l}");
    }
    // A later change rebuilds them as usual.
    let (w, t) = (World { r: 12, cave: false }, table());
    for k in [&mut r, &mut c] {
        k.put_chunk(&bake_chunk(&w, &Plains, &t, 5, 11, Layer::Surface).unwrap());
        k.ensure_mips();
    }
    for l in 1..MIP_LEVELS {
        assert_eq!(c.level(l), r.level(l));
        assert_eq!(c.height_level(l), r.height_level(l));
    }
}

#[test]
fn copy_pixels_with_generation_zero_still_builds_mips() {
    let mut r = region();
    r.generation = 0;
    let mut c = r.copy_pixels();
    c.ensure_mips();
    assert_eq!(c.level(MIP_LEVELS - 1).len(), (512 >> (MIP_LEVELS - 1)) * (512 >> (MIP_LEVELS - 1)));
}
