//! 6-DOF 3D vessel rendering (`submit_custom_geometry`) and flight/rover telemetry HUD (`render_gui_post`).

use aeronautics_physics::math::{Vec3, v_add, v_len, v_scale, v_sub};
use aeronautics_physics::{BlockRole, Facing};
use mod_api::{
    ClientHost, RenderGuiPostEvent, SubmitCustomGeometryEvent, WorldDepth, WorldLine, WorldQuad,
};

use crate::net::role_from_code;
use crate::solid::{self, ClientVessel};

fn shade_rgb(rgb: u32, factor: f32) -> u32 {
    let r = ((((rgb >> 16) & 0xFF) as f32) * factor).round().clamp(0.0, 255.0) as u32;
    let g = ((((rgb >> 8) & 0xFF) as f32) * factor).round().clamp(0.0, 255.0) as u32;
    let b = (((rgb & 0xFF) as f32) * factor).round().clamp(0.0, 255.0) as u32;
    0xFF00_0000 | (r << 16) | (g << 8) | b
}

const CORNERS: [[f64; 3]; 8] = [
    [0.0, 0.0, 0.0],
    [1.0, 0.0, 0.0],
    [0.0, 1.0, 0.0],
    [1.0, 1.0, 0.0],
    [0.0, 0.0, 1.0],
    [1.0, 0.0, 1.0],
    [0.0, 1.0, 1.0],
    [1.0, 1.0, 1.0],
];

// (corner indices, shading multiplier)
const FACES: [([usize; 4], f32); 6] = [
    ([2, 6, 7, 3], 1.00), // top (+Y)
    ([0, 1, 5, 4], 0.62), // bottom (-Y)
    ([4, 5, 7, 6], 0.86), // south (+Z)
    ([0, 2, 3, 1], 0.80), // north (-Z)
    ([1, 3, 7, 5], 0.88), // east (+X)
    ([0, 4, 6, 2], 0.75), // west (-X)
];

const EDGES: [[usize; 2]; 12] = [
    [0, 1], [1, 3], [3, 2], [2, 0],
    [4, 5], [5, 7], [7, 6], [6, 4],
    [0, 4], [1, 5], [2, 6], [3, 7],
];

fn push_box(
    quads: &mut Vec<WorldQuad>,
    lines: &mut Vec<WorldLine>,
    pos: Vec3,
    ori: [f64; 4],
    com: Vec3,
    bmin: [f64; 3],
    bmax: [f64; 3],
    rgb: u32,
    outline_argb: Option<u32>,
) {
    let verts: [Vec3; 8] = std::array::from_fn(|i| {
        let c = CORNERS[i];
        let lx = if c[0] > 0.5 { bmax[0] } else { bmin[0] };
        let ly = if c[1] > 0.5 { bmax[1] } else { bmin[1] };
        let lz = if c[2] > 0.5 { bmax[2] } else { bmin[2] };
        solid::to_world(pos, ori, com, [lx, ly, lz])
    });
    for (idx, shade) in FACES {
        quads.push(WorldQuad::new(
            idx.map(|vi| verts[vi]),
            shade_rgb(rgb, shade),
            WorldDepth::Tested,
        ));
    }
    if let Some(edge_col) = outline_argb {
        for [a, b] in EDGES {
            lines.push(WorldLine::new(verts[a], verts[b], edge_col, 1.5, WorldDepth::Tested));
        }
    }
}

fn push_propeller_blades(
    quads: &mut Vec<WorldQuad>,
    pos: Vec3,
    ori: [f64; 4],
    com: Vec3,
    local_block: [i32; 3],
    facing: Facing,
    spin_rad: f64,
    rgb: u32,
) {
    let center = [
        f64::from(local_block[0]) + 0.5,
        f64::from(local_block[1]) + 0.5,
        f64::from(local_block[2]) + 0.5,
    ];
    let dir = facing.unit();
    let hub = v_add(center, v_scale(dir, 0.58));

    // Orthogonal basis (u, v) perpendicular to facing axis.
    let (u, v) = match facing {
        Facing::Up | Facing::Down => ([1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        Facing::North | Facing::South => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        Facing::East | Facing::West => ([0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
    };

    let blade_col = shade_rgb(rgb, 1.08);
    let len = 1.15;
    let width = 0.14;
    let thickness = v_scale(dir, 0.06);

    for k in 0..2 {
        let angle = spin_rad + f64::from(k) * std::f64::consts::FRAC_PI_2;
        let (sa, ca) = angle.sin_cos();
        let axis_a = v_add(v_scale(u, ca), v_scale(v, sa));
        let axis_b = v_add(v_scale(u, -sa), v_scale(v, ca));

        let p0 = solid::to_world(
            pos,
            ori,
            com,
            v_add(v_sub(hub, v_scale(axis_a, len)), v_sub(thickness, v_scale(axis_b, width))),
        );
        let p1 = solid::to_world(
            pos,
            ori,
            com,
            v_add(v_add(hub, v_scale(axis_a, len)), v_sub(thickness, v_scale(axis_b, width))),
        );
        let p2 = solid::to_world(
            pos,
            ori,
            com,
            v_add(v_add(hub, v_scale(axis_a, len)), v_add(thickness, v_scale(axis_b, width))),
        );
        let p3 = solid::to_world(
            pos,
            ori,
            com,
            v_add(v_sub(hub, v_scale(axis_a, len)), v_add(thickness, v_scale(axis_b, width))),
        );
        quads.push(WorldQuad::new([p0, p1, p2, p3], blade_col, WorldDepth::Tested));
        quads.push(WorldQuad::new([p3, p2, p1, p0], shade_rgb(rgb, 0.85), WorldDepth::Tested));
    }
}

pub(crate) fn submit_custom_geometry(
    host: &dyn ClientHost,
    event: &mut SubmitCustomGeometryEvent<'_>,
) {
    let vessels: Vec<ClientVessel> = solid::snapshot_for_render(host.dimension());
    if vessels.is_empty() {
        return;
    }

    let partial = f64::from(event.view.partial_tick);
    let time_ticks = event.view.game_time as f64 + partial;

    let mut quads = Vec::new();
    let mut lines = Vec::new();

    for v in &vessels {
        let (pos, ori) = solid::interpolated_pose(v, partial);
        let com = v.snapshot.center_of_mass;
        let spin_rad = time_ticks * f64::from(v.snapshot.engine_rpm.abs().max(12.0)) * 0.045;

        for b in &v.snapshot.blocks {
            let x0 = f64::from(b.local[0]);
            let y0 = f64::from(b.local[1]);
            let z0 = f64::from(b.local[2]);
            let role = role_from_code(b.role_code);
            let outline = if matches!(role, BlockRole::Envelope | BlockRole::Sail) {
                None
            } else {
                Some(0x55_141820)
            };
            push_box(
                &mut quads,
                &mut lines,
                pos,
                ori,
                com,
                [x0, y0, z0],
                [x0 + 1.0, y0 + 1.0, z0 + 1.0],
                b.rgb,
                outline,
            );

            if matches!(role, BlockRole::Propeller | BlockRole::GyroPropeller) {
                push_propeller_blades(
                    &mut quads,
                    pos,
                    ori,
                    com,
                    b.local,
                    b.facing,
                    spin_rad,
                    0xDC_AF55,
                );
            }

            if let Some(tire) = b.tire {
                let r = tire.radius().clamp(0.45, 1.75);
                let tire_y0 = y0 - r * 0.55;
                let tire_y1 = y0 + r * 0.45;
                let pad = (r - 0.5).max(0.05) * 0.45;
                push_box(
                    &mut quads,
                    &mut lines,
                    pos,
                    ori,
                    com,
                    [x0 - pad, tire_y0, z0 - pad],
                    [x0 + 1.0 + pad, tire_y1, z0 + 1.0 + pad],
                    0x20_2226,
                    Some(0x90_0C0D10),
                );
            }
        }
    }

    if !quads.is_empty() {
        let _ = event.draw.quads(&quads);
    }
    if !lines.is_empty() {
        let _ = event.draw.lines(&lines);
    }
}

pub(crate) fn render_gui_post(host: &dyn ClientHost, event: &mut RenderGuiPostEvent<'_>) {
    let vessels = solid::snapshot_for_render(host.dimension());
    if vessels.is_empty() {
        return;
    }

    let chosen = solid::local_pilot_vessel_id()
        .and_then(|id| vessels.iter().find(|v| v.snapshot.id == id))
        .or_else(|| {
            let cam = host.camera()?;
            vessels.iter().find(|v| {
                let d = v_sub(cam.position, v.snapshot.position);
                v_len(d) <= 14.0
            })
        });
    let Some(v) = chosen else {
        return;
    };

    let s = &v.snapshot;
    let t = &s.telemetry;
    let piloting = solid::local_pilot_vessel_id() == Some(s.id);
    let mode = if piloting {
        if t.grounded { "ROVER [PILOT]" } else { "AIRSHIP [PILOT]" }
    } else if t.grounded {
        "ROVER NEARBY"
    } else {
        "AIRSHIP NEARBY"
    };

    let heading = ((t.yaw_deg.rem_euclid(360.0)) * 10.0).round() / 10.0;
    let line1 = format!(
        "{mode}  |  ALT {:.1}m  |  SPD {:.1} m/s  |  THR {:+.0}%  |  LIFT {:.2}x  |  HDG {:.0}°",
        t.altitude,
        t.speed_mps,
        t.throttle * 100.0,
        t.lift_ratio,
        heading
    );
    let line2 = if piloting {
        format!(
            "W/S Throttle  •  A/D Steer  •  Space/Shift Ascend/Descend  •  Burner {}/15  •  /aero land",
            s.burner_heat
        )
    } else {
        format!(
            "Burner {}/15  •  Klik kemudi atau /aero pilot untuk mengendalikan  •  /aero land untuk turun",
            s.burner_heat
        )
    };

    let [screen_w, _] = event.draw.size();
    let box_w = 500.0_f32.min((screen_w - 24.0).max(260.0));
    let x0 = (screen_w - box_w) * 0.5;
    let y0 = 10.0;
    event.draw.fill([x0, y0, x0 + box_w, y0 + 36.0], 0xB8_0F141C);
    event.draw.fill([x0, y0, x0 + box_w, y0 + 2.0], if piloting { 0xFF_E0A838 } else { 0xFF_4FA8D8 });
    event.draw.text(&line1, [x0 + 10.0, y0 + 7.0], 0xFF_F2F5F8, true);
    event.draw.text(&line2, [x0 + 10.0, y0 + 20.0], 0xFF_B4C0CE, false);
}
