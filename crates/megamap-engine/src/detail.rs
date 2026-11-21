//! Block textures on a closely zoomed map. A baked map pixel is one block's colour (biome tint, jitter and block
//! light already in it); the texture only adds the pattern: each texel as a ratio to the texture's average
//! (128 = 1.0), so the tint and the lighting the map computed stay as they are. Ratios are kept at 16, 8, 4
//! and 2 texels a side; the view takes the finest that is not finer than a screen pixel, so a moving map does
//! not shimmer.

/// One texture's pattern.
#[derive(Clone, Debug, PartialEq)]
pub struct Detail {
    /// `(side, ratios)` from 16 down to 2 texels a side; ratios per channel, row major, `z` (texture `v`) down.
    mips: Vec<(usize, Vec<[u8; 3]>)>,
}

/// Map pixels per block where the textures start to show, and where they are at full strength.
pub const DETAIL_FROM: f32 = 3.0;
pub const DETAIL_FULL: f32 = 6.0;

/// How much of the pattern shows at `ppb` map pixels per block (0..=256).
#[inline]
pub fn strength(ppb: f32) -> u32 {
    let t = ((ppb - DETAIL_FROM) / (DETAIL_FULL - DETAIL_FROM)).clamp(0.0, 1.0);
    (t * t * (3.0 - 2.0 * t) * 256.0) as u32
}

impl Detail {
    /// From an RGBA texture (the first square frame of an animated strip), scaled to 16 x 16. `None` for a
    /// fully clear one.
    pub fn build(width: u32, height: u32, rgba: &[u8]) -> Option<Detail> {
        let w = width as usize;
        if w == 0 {
            return None;
        }
        let h = (height as usize).min(w).min(rgba.len() / 4 / w);
        if h == 0 {
            return None;
        }
        // Box filter (or repeat) onto 16 x 16, alpha weighted; clear texels left at the average.
        const N: usize = 16;
        let mut px = [[0f32; 4]; N * N];
        for (j, out) in px.iter_mut().enumerate() {
            let (ox, oz) = (j % N, j / N);
            let (x0, x1) = (ox * w / N, ((ox + 1) * w / N).max(ox * w / N + 1).min(w));
            let (z0, z1) = (oz * h / N, ((oz + 1) * h / N).max(oz * h / N + 1).min(h));
            let mut acc = [0f32; 4];
            for z in z0..z1 {
                for x in x0..x1 {
                    let p = &rgba[(z * w + x) * 4..(z * w + x) * 4 + 4];
                    let a = p[3] as f32 / 255.0;
                    acc[0] += p[0] as f32 * a;
                    acc[1] += p[1] as f32 * a;
                    acc[2] += p[2] as f32 * a;
                    acc[3] += a;
                }
            }
            *out = acc;
        }
        let total: f32 = px.iter().map(|p| p[3]).sum();
        if total <= 0.0 {
            return None;
        }
        let avg = [0, 1, 2].map(|c| px.iter().map(|p| p[c]).sum::<f32>() / total);
        let base: Vec<[u8; 3]> = px
            .iter()
            .map(|p| {
                if p[3] < 0.25 {
                    return [128; 3];
                }
                [0, 1, 2].map(|c| {
                    let v = p[c] / p[3];
                    // Offset by a little so a near-black channel does not blow up.
                    let r = (v + 12.0) / (avg[c] + 12.0);
                    (r * 128.0).round().clamp(24.0, 255.0) as u8
                })
            })
            .collect();
        let mut mips = vec![(N, base)];
        while mips.last().unwrap().0 > 2 {
            let (s, src) = mips.last().unwrap();
            let (s, h) = (*s, *s / 2);
            let mut dst = Vec::with_capacity(h * h);
            for z in 0..h {
                for x in 0..h {
                    let at = |dx: usize, dz: usize| src[(z * 2 + dz) * s + x * 2 + dx];
                    let q = [at(0, 0), at(1, 0), at(0, 1), at(1, 1)];
                    dst.push([0, 1, 2].map(|c| ((q.iter().map(|p| p[c] as u32).sum::<u32>() + 2) / 4) as u8));
                }
            }
            mips.push((h, dst));
        }
        Some(Detail { mips })
    }

    /// The ratio at block fraction (`fx`, `fz`) (0..1 each) for `ppb` map pixels per block; `None` below 2.
    #[inline]
    pub fn sample(&self, fx: f32, fz: f32, ppb: f32) -> Option<[u8; 3]> {
        let (side, r) = self.mips.iter().find(|(s, _)| *s as f32 <= ppb)?;
        let s = *side;
        let x = ((fx * s as f32) as usize).min(s - 1);
        let z = ((fz * s as f32) as usize).min(s - 1);
        Some(r[z * s + x])
    }
}

/// `c` (ARGB) times `ratio` (128 = 1.0) blended in by `k` (0..=256); alpha kept.
#[inline]
pub fn apply(c: u32, ratio: [u8; 3], k: u32) -> u32 {
    let ch = |v: u32, r: u8| {
        let r = 128 * 256 + (r as i32 - 128) * k as i32;
        ((v as i32 * r) >> 15).clamp(0, 255) as u32
    };
    (c & 0xFF00_0000) | ch((c >> 16) & 0xFF, ratio[0]) << 16 | ch((c >> 8) & 0xFF, ratio[1]) << 8 | ch(c & 0xFF, ratio[2])
}
