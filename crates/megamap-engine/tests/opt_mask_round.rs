//! The glue's `mask_round` (round minimap corners cleared, every minimap frame) with row runs clears exactly the
//! pixels of the old per-pixel loop. Both are copied verbatim from `src/glue.rs` (the glue only builds in the game).

fn mask_round_per_pixel(buf: &mut [u8], w: usize, h: usize) {
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let r2 = hw.min(hh).powi(2);
    for y in 0..h {
        let dy = y as f32 + 0.5 - hh;
        for x in 0..w {
            let dx = x as f32 + 0.5 - hw;
            if dx * dx + dy * dy > r2 {
                buf[(y * w + x) * 4..(y * w + x) * 4 + 4].copy_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
}

fn mask_round(buf: &mut [u8], w: usize, h: usize) {
    if w == 0 {
        return;
    }
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let r2 = hw.min(hh).powi(2);
    let outside = |x: usize, dy: f32| {
        let dx = x as f32 + 0.5 - hw;
        dx * dx + dy * dy > r2
    };
    // The pixels kept in a row are one run (the test is monotone in |dx|): clear the two ends in place of a
    // test and a 4-byte store per pixel, same pixels as the per-pixel loop.
    for (y, row) in buf[..w * h * 4].chunks_exact_mut(w * 4).enumerate() {
        let dy = y as f32 + 0.5 - hh;
        let a = (0..w).find(|&x| !outside(x, dy)).unwrap_or(w);
        let b = (a..w).rev().find(|&x| !outside(x, dy)).map_or(w, |x| x + 1);
        row[..a * 4].fill(0);
        row[b * 4..].fill(0);
    }
}

#[test]
fn mask_round_row_runs_match_the_per_pixel_loop() {
    let mut sizes: Vec<(usize, usize)> = vec![(0, 0), (0, 5), (5, 0), (1, 1), (2, 2), (3, 7), (7, 3), (1, 64), (64, 1)];
    // Every square the game makes (GUI minimap sizes at GUI scales 1..4, the settings previews) and odd ones.
    sizes.extend((1..=520).map(|n| (n, n)));
    sizes.extend([(255, 256), (256, 255), (300, 170), (170, 300), (1023, 577)]);
    for (w, h) in sizes {
        // A longer buffer than the image (the glue passes `&mut buf[..n]` but must not touch past w * h).
        let src: Vec<u8> = (0..w * h * 4 + 8).map(|i| (i * 31 % 251) as u8 | 1).collect();
        let (mut a, mut b) = (src.clone(), src.clone());
        mask_round_per_pixel(&mut a, w, h);
        mask_round(&mut b, w, h);
        assert_eq!(a, b, "{w}x{h}");
    }
}
