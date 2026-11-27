//! Aerodynamic lift/drag, propeller & steam-vent propulsion, and pilot control torques.

use crate::catalog::{self, BlockRole};
use crate::mass::{BlockCell, MassProperties};
use crate::math::{Quat, Vec3, air_density, quat_rotate, quat_rotate_inv, v_add, v_cross, v_dot, v_len, v_scale};

/// Normalized pilot controls for airships, planes, and offroad rovers.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PilotInput {
    /// Forward/reverse throttle (`-1.0..=1.0`).
    pub throttle: f32,
    /// Nose pitch up/down (`-1.0..=1.0`).
    pub pitch: f32,
    /// Steering / rudder yaw (`-1.0..=1.0`, positive = left / CCW).
    pub yaw: f32,
    /// Aileron roll (`-1.0..=1.0`).
    pub roll: f32,
    /// Direct vertical lift trim (burner boost / vertical thrusters, `-1.0..=1.0`).
    pub vertical: f32,
    /// Handbrake / air-anchor brake.
    pub brake: bool,
}

impl PilotInput {
    pub fn clamped(self) -> Self {
        Self {
            throttle: self.throttle.clamp(-1.0, 1.0),
            pitch: self.pitch.clamp(-1.0, 1.0),
            yaw: self.yaw.clamp(-1.0, 1.0),
            roll: self.roll.clamp(-1.0, 1.0),
            vertical: self.vertical.clamp(-1.0, 1.0),
            brake: self.brake,
        }
    }
}

/// Result of one aerodynamic & propulsion evaluation step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct AeroResult {
    pub force_world: Vec3,
    pub torque_local: Vec3,
    pub total_thrust_n: f64,
    pub wing_lift_n: f64,
    pub active_propellers: usize,
}

/// Computes target propeller RPM from attached portable engines / motors and throttle.
pub fn compute_propeller_rpm(engines: usize, base_rpm: f32, throttle: f32) -> f32 {
    let drive = if engines > 0 {
        64.0 + (engines.min(8) as f32 - 1.0) * 24.0
    } else {
        base_rpm.max(32.0)
    };
    drive * throttle.clamp(-1.0, 1.0)
}

/// Evaluates propeller thrust, steam vents, sail wing lift, quadratic drag, and control torques.
pub fn step_aero(
    world_y: f64,
    velocity_world: Vec3,
    orientation: Quat,
    omega_local: Vec3,
    mass: &MassProperties,
    blocks: &[BlockCell],
    input: PilotInput,
) -> AeroResult {
    let input = input.clamped();
    let rho = air_density(world_y);
    let vel_local = quat_rotate_inv(orientation, velocity_world);
    let speed = v_len(vel_local);

    let mut force_local = [0.0_f64; 3];
    let mut torque_local = [0.0_f64; 3];

    let mut engines = 0_usize;
    let mut propellers = 0_usize;
    let mut gyros = 0_usize;
    let mut helms = 0_usize;
    let mut sail_count = 0_usize;
    let mut drag_area_total = 0.0_f64;

    for cell in blocks {
        let info = catalog::classify(&cell.id);
        drag_area_total += info.drag_area;
        match info.role {
            BlockRole::Engine => engines += 1,
            BlockRole::Propeller => propellers += 1,
            BlockRole::GyroPropeller => {
                propellers += 1;
                gyros += 1;
            }
            BlockRole::Helm => helms += 1,
            BlockRole::Sail => sail_count += 1,
            _ => {}
        }
    }

    let engine_mult = if engines > 0 {
        1.0 + 0.35 * (engines.saturating_sub(1).min(6) as f64)
    } else {
        // Without a dedicated engine block, propellers still run at auxiliary speed when powered/piloted.
        0.7
    };

    let mut total_thrust_n = 0.0_f64;
    for cell in blocks {
        let role = catalog::classify(&cell.id).role;
        let r = [
            f64::from(cell.local[0]) + 0.5 - mass.center_of_mass[0],
            f64::from(cell.local[1]) + 0.5 - mass.center_of_mass[1],
            f64::from(cell.local[2]) + 0.5 - mass.center_of_mass[2],
        ];
        match role {
            BlockRole::Propeller | BlockRole::GyroPropeller => {
                let mag = 5_200.0 * engine_mult * f64::from(input.throttle) * (0.35 + 0.65 * rho);
                total_thrust_n += mag.abs();
                let dir = cell.facing.unit();
                let f = v_scale(dir, mag);
                force_local = v_add(force_local, f);
                // Dampen off-center propeller torque by 75% so slightly asymmetric airships remain flyable.
                torque_local = v_add(torque_local, v_scale(v_cross(r, f), 0.25));
            }
            BlockRole::SteamVent => {
                let active = if cell.powered { 1.0 } else { f64::from(input.throttle.abs()) };
                let mag = 3_600.0 * active * (0.4 + 0.6 * rho);
                total_thrust_n += mag;
                let dir = cell.facing.unit();
                let f = v_scale(dir, mag);
                force_local = v_add(force_local, f);
                torque_local = v_add(torque_local, v_scale(v_cross(r, f), 0.2));
            }
            _ => {}
        }
    }

    // Sail wing lift: proportional to sail area * horizontal airspeed * (base camber + small pitch angle).
    let horiz_speed = (vel_local[0] * vel_local[0] + vel_local[2] * vel_local[2]).sqrt();
    let wing_lift_n = if sail_count > 0 && horiz_speed > 0.25 {
        let aoa_factor = (0.35 - vel_local[1] / horiz_speed.max(1.0) * 1.6).clamp(-0.6, 1.4);
        (sail_count as f64) * 55.0 * rho * horiz_speed * horiz_speed.min(18.0) * aoa_factor
    } else {
        0.0
    };
    force_local[1] += wing_lift_n;

    // Aerodynamic quadratic + linear drag scaled cleanly by ship frontal area.
    let eff_area = drag_area_total.sqrt() * 2.2;
    if speed > 1e-4 {
        let drag_mag = rho * eff_area * (6.5 * speed + 1.8 * speed * speed);
        let drag_local = v_scale(vel_local, -drag_mag / speed);
        force_local = v_add(force_local, drag_local);
    }

    if input.brake {
        force_local = v_add(force_local, v_scale(vel_local, -mass.total_mass * 3.5));
    }

    // Pilot control torques from gyroscopic propellers + helm + aerodynamic control surfaces.
    let authority = 1.0 + (gyros as f64) * 0.7 + (helms as f64) * 0.35 + (sail_count as f64) * 0.08;
    torque_local[0] += f64::from(input.pitch) * mass.inertia_diag[0] * 1.4 * authority;
    torque_local[1] += f64::from(input.yaw) * mass.inertia_diag[1] * 1.6 * authority;
    torque_local[2] += f64::from(input.roll) * mass.inertia_diag[2] * 1.3 * authority;

    // Upright righting spring + strong angular damping keeps vessels controllable without uncontrollable spin.
    let up_local = quat_rotate_inv(orientation, [0.0, 1.0, 0.0]);
    let righting = v_cross([0.0, 1.0, 0.0], up_local);
    // Align body +Y back toward world +Y when pitch/roll aren't actively held.
    let righting_strength = if input.pitch.abs() < 0.05 && input.roll.abs() < 0.05 {
        2.6 + (gyros as f64) * 1.2
    } else {
        0.7
    };
    torque_local[0] += righting[0] * mass.inertia_diag[0] * righting_strength;
    torque_local[2] += righting[2] * mass.inertia_diag[2] * righting_strength;

    // Rotational damping proportional to each inertia axis.
    let damp = 3.2 + if input.brake { 4.0 } else { 0.0 };
    torque_local[0] -= omega_local[0] * mass.inertia_diag[0] * damp;
    torque_local[1] -= omega_local[1] * mass.inertia_diag[1] * (damp * 0.85);
    torque_local[2] -= omega_local[2] * mass.inertia_diag[2] * damp;

    // Prevent NaN/Runaway in extreme angular velocity ticks.
    if v_dot(force_local, force_local).is_nan() {
        force_local = [0.0; 3];
    }

    AeroResult {
        force_world: quat_rotate(orientation, force_local),
        torque_local,
        total_thrust_n,
        wing_lift_n,
        active_propellers: propellers,
    }
}
