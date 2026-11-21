//! `Clouds::composite` leaves every pixel opaque without its old final opacity pass (the row paths write alpha).
use megamap_engine::clouds::{Clouds, Params, Theme};
use megamap_engine::view::View;

#[test]
fn composite_output_is_opaque_everywhere() {
    for (w, h, bpp, rot) in [(192usize, 192usize, 0.5f64, 0.6f64), (256, 256, 2.0, 0.0), (300, 170, 7.0, 1.3), (31, 77, 1.0, 0.2)] {
        let mut v = View::new([40.0, -20.0], bpp);
        v.rotation = rot;
        v.style.mark_unknown = true;
        // Explored checker with unknown (alpha 0), far (128) and revealing (129..=254) patches.
        let mut img = vec![0u8; w * h * 4];
        for y in 0..h {
            for x in 0..w {
                let [wx, wz] = v.to_world(w, h, x as f64, y as f64);
                let a = match (((wx / 24.0).floor() as i64 + (wz / 40.0).floor() as i64).rem_euclid(5)) as u8 {
                    0 => 0,
                    1 => 128,
                    2 => 200,
                    _ => 255,
                };
                img[(y * w + x) * 4..][..4].copy_from_slice(&[90, 140, 60, a]);
            }
        }
        for p in [
            Params::default(),
            Params { quality: 3, ..Params::default() },
            Params { quality: 2, rain: 1.0, thunder: 1.0, theme: Theme::Storm, lightning: 2, ..Params::default() },
        ] {
            let mut c = Clouds::new();
            for f in 0..3 {
                let t = 5.0 + f as f32 * 0.3;
                let mut out = img.clone();
                if c.update(&img, w, h, &v, t, &p) {
                    c.composite(&mut out, w, h, &p, t);
                    assert!(out.chunks_exact(4).all(|px| px[3] == 255), "{w}x{h} bpp {bpp} frame {f}");
                }
            }
        }
    }
}
