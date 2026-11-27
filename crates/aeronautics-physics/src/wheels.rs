//! Offroad raycast wheel suspension, traction drive, steering, and lateral grip friction.

use crate::aero::PilotInput;
use crate::catalog::TireKind;
use crate::mass::{BlockCell, MassProperties};
use crate::math::{Quat, Vec3, quat_rotate, quat_rotate_inv, v_add, v_cross, v_dot, v_scale};

/// Result of one ground-wheel suspension & drive evaluation step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WheelSummary {
    pub grounded_wheels: usize,
    pub total_wheels: usize,
    pub force_world: Vec3,
    pub torque_local: Vec3,
}

/// Evaluates every mounted wheel on the contraption against world terrain via `is_solid`.
pub fn step_wheels(
    body_pos: Vec3,
    body_vel: Vec3,
    orientation: Quat,
    omega_local: Vec3,
    mass: &MassProperties,
    blocks: &[BlockCell],
    input: PilotInput,
    is_solid: &dyn Fn(Vec3) -> bool,
) -> WheelSummary {
    let input = input.clamped();
    let down_world = quat_rotate(orientation, [0.0, -1.0, 0.0]);
    let mut grounded_wheels = 0_usize;
    let mut force_world = [0.0_f64; 3];
    let mut torque_local = [0.0_f64; 3];

    let wheels: Vec<(&BlockCell, TireKind)> = blocks
        .iter()
        .filter_map(|c| c.tire.map(|t| (c, t)))
        .collect();
    let total_wheels = wheels.len();
    if total_wheels == 0 {
        return WheelSummary::default();
    }

    let drive_scale = 1.0 / (total_wheels.max(1) as f64).sqrt();

    for (cell, tire) in wheels {
        let local_center = [
            f64::from(cell.local[0]) + 0.5,
            f64::from(cell.local[1]) + 0.5,
            f64::from(cell.local[2]) + 0.5,
        ];
        let r_local = [
            local_center[0] - mass.center_of_mass[0],
            local_center[1] - mass.center_of_mass[1],
            local_center[2] - mass.center_of_mass[2],
        ];
        let hub_world = v_add(body_pos, quat_rotate(orientation, local_center));
        let rest_len = tire.rest_length();

        let Some(hit_dist) = raycast_ground(hub_world, down_world, rest_len, is_solid) else {
            continue;
        };

        grounded_wheels += 1;
        let compression = (rest_len - hit_dist).clamp(0.0, rest_len);

        // Velocity at wheel contact patch = v_cm + R * (omega x r)
        let r_world = quat_rotate(orientation, r_local);
        let omega_world = quat_rotate(orientation, omega_local);
        let patch_vel = v_add(body_vel, v_cross(omega_world, r_world));
        let vel_local = quat_rotate_inv(orientation, patch_vel);

        // Spring + vertical damping force along local +Y.
        let up_vel = vel_local[1];
        let spring_f = (compression * tire.stiffness() - up_vel * tire.damping()).max(0.0);

        // Longitudinal traction along contraption facing (`-Z` forward or cell's horizontal forward).
        let drive_force = f64::from(input.throttle) * tire.max_drive_force() * drive_scale;
        let rolling_drag = -vel_local[2] * if input.brake { 2_400.0 } else { 140.0 };
        let forward_f = -drive_force + rolling_drag;

        // Lateral tire grip eliminates unrealistic ice-skating sideways drift.
        let lateral_f = -vel_local[0] * 1_650.0;

        let wheel_force_local = [lateral_f, spring_f, forward_f];
        let wheel_force_world = quat_rotate(orientation, wheel_force_local);
        force_world = v_add(force_world, wheel_force_world);

        // Suspension pitch/roll torque around CoM; damp excessive drive pitch-up.
        let raw_torque = v_cross(r_local, wheel_force_local);
        torque_local = v_add(torque_local, [raw_torque[0] * 0.45, raw_torque[1] * 0.15, raw_torque[2] * 0.45]);
    }

    // Grounded Ackerman-style steering yaw torque proportional to pilot yaw input.
    if grounded_wheels > 0 && input.yaw.abs() > 0.01 {
        let vel_local = quat_rotate_inv(orientation, body_vel);
        let speed_factor = (0.45 + v_dot(vel_local, vel_local).sqrt() * 0.12).clamp(0.45, 1.5);
        torque_local[1] += f64::from(input.yaw) * mass.inertia_diag[1] * 2.2 * speed_factor;
    }

    WheelSummary {
        grounded_wheels,
        total_wheels,
        force_world,
        torque_local,
    }
}

fn raycast_ground(origin: Vec3, down: Vec3, max_dist: f64, is_solid: &dyn Fn(Vec3) -> bool) -> Option<f64> {
    let steps = 16;
    let step_len = max_dist / f64::from(steps);
    for i in 1..=steps {
        let d = f64::from(i) * step_len;
        let sample = v_add(origin, v_scale(down, d));
        if is_solid(sample) {
            return Some(d);
        }
    }
    None
}
