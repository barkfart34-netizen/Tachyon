//! Aeronautics flight/rover telemetry HUD (`render_gui_post`).
//! Real textured 6-DOF block models are submitted by `horizon-client`.

use mod_api::{ClientHost, RenderGuiPostEvent, SubmitCustomGeometryEvent};

use crate::solid;

pub(crate) fn submit_custom_geometry(
    _host: &dyn ClientHost,
    _event: &mut SubmitCustomGeometryEvent<'_>,
) {
    // Intentionally empty: real textured block models and offroad tires are
    // rendered via `Game::aeronautics_vessel_geometry` in `horizon-client`.
}

pub(crate) fn render_gui_post(host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
    let Some(pilot_id) = solid::local_pilot_vessel_id() else {
        return;
    };
    let vessels = solid::snapshot_for_render(host.dimension());
    let Some(v) = vessels.iter().find(|v| v.snapshot.id == pilot_id) else {
        return;
    };

    let s = &v.snapshot;
    let t = &s.telemetry;
    let mode = if t.grounded { "ROVER" } else { "AIRSHIP" };
    let line = format!(
        "{mode} | ALT {:.1}m | SPD {:.1} m/s | THR {:+.0}% | BRN {}/15 | Shift/V: Keluar",
        t.altitude,
        t.speed_mps,
        t.throttle * 100.0,
        s.burner_heat,
    );

    let [screen_w, screen_h] = event.draw.size();
    let box_w = 340.0_f32.min((screen_w - 32.0).max(200.0));
    let box_h = 22.0;
    let x0 = (screen_w - box_w) * 0.5;
    let y0 = (screen_h - 68.0).max(48.0);

    // `DrawContext::fill` takes `[x, y, width, height]` in GUI pixels.
    event.draw.fill([x0, y0, box_w, box_h], 0xD8_1E1813);
    event.draw.fill([x0, y0, box_w, 1.0], 0xFF_B8924A);
    event.draw.fill([x0, y0 + box_h - 1.0, box_w, 1.0], 0xFF_B8924A);
    event.draw.fill([x0, y0, 1.0, box_h], 0xFF_B8924A);
    event.draw.fill([x0 + box_w - 1.0, y0, 1.0, box_h], 0xFF_B8924A);

    let tw = event.draw.text_width(&line);
    let tx = (x0 + (box_w - tw) * 0.5).max(x0 + 6.0);
    event.draw.text(&line, [tx, y0 + 5.0], 0xFF_F3E7C9, true);

    let bar_pad = 8.0;
    let half_w = ((box_w - bar_pad * 3.0) * 0.5).max(20.0);
    let bar_y = y0 + box_h - 4.0;
    let lift_fill = ((t.lift_ratio / 1.5).clamp(0.0, 1.0) as f32) * half_w;
    let heat_fill = (f32::from(s.burner_heat) / 15.0).clamp(0.0, 1.0) * half_w;

    event.draw.fill([x0 + bar_pad, bar_y, half_w, 2.0], 0x60_3A3024);
    event.draw.fill(
        [x0 + bar_pad, bar_y, lift_fill, 2.0],
        if t.lift_ratio >= 1.0 { 0xFF_6BD17D } else { 0xFF_D6A64A },
    );
    let hx = x0 + bar_pad * 2.0 + half_w;
    event.draw.fill([hx, bar_y, half_w, 2.0], 0x60_3A3024);
    event.draw.fill(
        [hx, bar_y, heat_fill, 2.0],
        if s.burner_heat > 10 { 0xFF_64D4FF } else { 0xFF_E67E22 },
    );
}
