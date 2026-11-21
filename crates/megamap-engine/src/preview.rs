//! The live preview in the settings window: a small minimap (minimap tab) or map fragment (map tab) rendered from
//! the real map data around the player with the current settings, with its own animated clouds under a weather
//! that is picked in the preview only (the game's weather is untouched).
//!
//! Cheap by construction: `SIZE` x `SIZE` image pixels (one per GUI pixel), at most `FPS` frames a second, and only
//! while the settings window is open. If the fragment is fully explored the clouds would have nothing to cover, so
//! a lumpy fog edge is cut into the outer ring (`fog_edge`) to show the cloud look anyway.

use super::clouds::{Look, Params};
use super::settings::Target;

/// Preview image side in pixels.
pub const SIZE: usize = 116;
/// Preview frame rate cap.
pub const FPS: f64 = 20.0;

/// Preview weathers, in `ui::PREVIEW_WEATHERS` order.
pub const CLEAR: usize = 0;
pub const RAIN: usize = 1;
pub const STORM: usize = 2;
pub const SUNSET: usize = 3;
pub const NIGHT: usize = 4;
pub const NETHER: usize = 5;
pub const END: usize = 6;

/// The cloud parameters under preview weather `k`: the player's settings (density, puff, quality, theme, ...)
/// with the world part (rain, thunder, time, dimension look) replaced. `nether` / `end` are the player's look
/// strengths (1 = normal).
pub fn weather_params(base: Params, k: usize, nether: f32, end: f32) -> Params {
    let mut p = Params { rain: 0.0, thunder: 0.0, day_time: 6000, look: Look::Overworld, intensity: 1.0, ..base };
    match k {
        RAIN => p.rain = 1.0,
        STORM => {
            p.rain = 1.0;
            p.thunder = 1.0;
        }
        SUNSET => p.day_time = 12_600,
        NIGHT => p.day_time = 18_000,
        NETHER => {
            p.look = Look::Nether;
            p.intensity = nether;
        }
        END => {
            p.look = Look::End;
            p.intensity = end;
        }
        _ => {}
    }
    // The weather must show even with "react to weather" off: that switch is about the live world.
    p.weather = true;
    p
}

/// Whether a preview frame is due (`last` and `now` in seconds).
pub fn due(last: f64, now: f64) -> bool {
    now - last >= 1.0 / FPS - 1e-6 || now < last
}

/// Cuts a lumpy fog edge into an `n` x `n` RGBA image (alpha 0 outside a wobbly circle of about 0.40 n), so the
/// clouds have unexplored ground to cover. Pixels already unknown stay unknown. Returns how many pixels it cleared.
pub fn fog_edge(img: &mut [u8], n: usize) -> usize {
    let c = (n as f32 - 1.0) / 2.0;
    let r0 = n as f32 * 0.40;
    let mut cleared = 0;
    for y in 0..n {
        for x in 0..n {
            let (dx, dy) = (x as f32 - c, y as f32 - c);
            let a = dy.atan2(dx);
            let r = r0 * (1.0 + 0.10 * (3.0 * a).sin() + 0.06 * (7.0 * a + 1.3).sin());
            if dx * dx + dy * dy > r * r {
                let i = (y * n + x) * 4 + 3;
                if img[i] != 0 {
                    img[i] = 0;
                    cleared += 1;
                }
            }
        }
    }
    cleared
}

/// The area of the preview a hovered setting changes, as a rect in GUI pixels relative to the preview image
/// (`img` = [x, y, w, h]); `info_h` is the height of the info lines drawn under the image.
pub fn target_rect(t: Target, img: [f32; 4], info_h: f32) -> Option<[f32; 4]> {
    let [x, y, w, h] = img;
    let inset = |k: f32| [x + w * k, y + h * k, w * (1.0 - 2.0 * k), h * (1.0 - 2.0 * k)];
    match t {
        Target::None => None,
        Target::MiniFrame => Some([x - 2.0, y - 2.0, w + 4.0, h + 4.0]),
        Target::MiniMap => Some(inset(0.04)),
        Target::MiniInfo => Some([x, y + h + 1.0, w, info_h.max(10.0)]),
        Target::Entities => Some(inset(0.30)),
        Target::Clouds => Some([x, y, w, h]),
        Target::Terrain => Some(inset(0.18)),
        Target::Overlay => Some(inset(0.25)),
    }
}

#[cfg(test)]
mod tests {}
