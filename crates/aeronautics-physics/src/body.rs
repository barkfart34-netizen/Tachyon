//! 6-DOF rigid-body vessel state, step integration, terrain collision, and snapped disassembly.

use std::collections::HashSet;

use crate::aero::{self, PilotInput};
use crate::buoyancy::{self, BalloonPocket};
use crate::mass::{self, BlockCell, MassProperties};
use crate::math::{
    GRAVITY, IDENTITY_QUAT, Pos, Quat, Vec3, quat_integrate_omega, quat_rotate, quat_rotate_inv,
    quat_to_yaw_pitch_roll, v_add, v_len, v_scale, v_sub,
};
use crate::wheels;

const MAX_LINEAR_SPEED: f64 = 45.0;
const MAX_ANGULAR_SPEED: f64 = 4.0;

/// Live HUD/instrument readout returned after each physics step.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct StepTelemetry {
    pub altitude: f64,
    pub speed_mps: f64,
    pub vertical_speed: f64,
    pub lift_ratio: f64,
    pub throttle: f32,
    pub yaw_deg: f64,
    pub pitch_deg: f64,
    pub roll_deg: f64,
    pub grounded: bool,
}

/// A live assembled Create Aeronautics / Simulated / Offroad contraption.
#[derive(Clone, Debug, PartialEq)]
pub struct RigidContraption {
    pub id: u64,
    /// World position of local `[0, 0, 0]` corner origin.
    pub position: Vec3,
    pub velocity: Vec3,
    pub orientation: Quat,
    pub angular_velocity: Vec3,
    pub throttle: f32,
    pub burner_heat: u8,
    pub engine_rpm: f32,
    pub blocks: Vec<BlockCell>,
    pub mass: MassProperties,
    pub balloons: Vec<BalloonPocket>,
    pub grounded: bool,
}

impl RigidContraption {
    /// Constructs a rigid body at `position` from a non-empty list of local blocks.
    pub fn from_blocks(id: u64, position: Vec3, blocks: Vec<BlockCell>) -> Self {
        let mass = mass::compute_mass_properties(&blocks);
        let balloons = buoyancy::detect_balloons(&blocks);
        let max_heat = blocks.iter().map(|b| b.heat).max().unwrap_or(0);
        Self {
            id,
            position,
            velocity: [0.0; 3],
            orientation: IDENTITY_QUAT,
            angular_velocity: [0.0; 3],
            throttle: 0.0,
            burner_heat: max_heat,
            engine_rpm: 0.0,
            blocks,
            mass,
            balloons,
            grounded: false,
        }
    }

    /// Recomputes mass properties and hot-air cavities after blocks change.
    pub fn refresh_structure(&mut self) {
        self.mass = mass::compute_mass_properties(&self.blocks);
        self.balloons = buoyancy::detect_balloons(&self.blocks);
    }

    /// Transforms a point in contraption-local coordinates into world coordinates.
    pub fn local_to_world(&self, local: Vec3) -> Vec3 {
        let rel = v_sub(local, self.mass.center_of_mass);
        let cm_world = v_add(self.position, self.mass.center_of_mass);
        v_add(cm_world, quat_rotate(self.orientation, rel))
    }

    /// Transforms a world coordinate into contraption-local coordinates.
    pub fn world_to_local(&self, world: Vec3) -> Vec3 {
        let cm_world = v_add(self.position, self.mass.center_of_mass);
        let rel = quat_rotate_inv(self.orientation, v_sub(world, cm_world));
        v_add(rel, self.mass.center_of_mass)
    }

    /// Advances the 6-DOF rigid body by `dt` seconds given pilot controls and a world solid query.
    pub fn step(
        &mut self,
        dt: f64,
        input: PilotInput,
        is_solid: impl Fn(Pos) -> bool,
    ) -> StepTelemetry {
        let dt = dt.clamp(0.001, 0.1);
        let input = input.clamped();
        self.throttle = input.throttle;

        let cm_world = v_add(self.position, self.mass.center_of_mass);
        let weight_n = self.mass.total_mass * GRAVITY;
        let gravity_force = [0.0, -weight_n, 0.0];

        let buoy = buoyancy::solve_buoyancy(
            cm_world[1],
            self.orientation,
            self.mass.center_of_mass,
            &self.blocks,
            &self.balloons,
            (self.burner_heat > 0).then_some(self.burner_heat),
            input.vertical,
        );

        let aero = aero::step_aero(
            cm_world[1],
            self.velocity,
            self.orientation,
            self.angular_velocity,
            &self.mass,
            &self.blocks,
            input,
        );

        let solid_vec = |w: Vec3| -> bool {
            is_solid([w[0].floor() as i32, w[1].floor() as i32, w[2].floor() as i32])
        };

        let wheel_res = wheels::step_wheels(
            self.position,
            self.velocity,
            self.orientation,
            self.angular_velocity,
            &self.mass,
            &self.blocks,
            input,
            &solid_vec,
        );

        let total_force = v_add(
            v_add(gravity_force, buoy.force_world),
            v_add(aero.force_world, wheel_res.force_world),
        );
        let total_torque = v_add(buoy.torque_local, v_add(aero.torque_local, wheel_res.torque_local));

        let inv_m = 1.0 / self.mass.total_mass;
        self.velocity = v_add(self.velocity, v_scale(total_force, inv_m * dt));
        let speed = v_len(self.velocity);
        if speed > MAX_LINEAR_SPEED {
            self.velocity = v_scale(self.velocity, MAX_LINEAR_SPEED / speed);
        }

        for i in 0..3 {
            self.angular_velocity[i] += (total_torque[i] / self.mass.inertia_diag[i]) * dt;
        }
        let ang_speed = v_len(self.angular_velocity);
        if ang_speed > MAX_ANGULAR_SPEED {
            self.angular_velocity = v_scale(self.angular_velocity, MAX_ANGULAR_SPEED / ang_speed);
        }

        self.position = v_add(self.position, v_scale(self.velocity, dt));
        self.orientation = quat_integrate_omega(self.orientation, self.angular_velocity, dt);

        let hull_grounded = self.resolve_world_collision(&is_solid);
        self.grounded = hull_grounded || wheel_res.grounded_wheels > 0;

        let engine_count = self
            .blocks
            .iter()
            .filter(|b| crate::catalog::is_engine(&b.id))
            .count();
        self.engine_rpm = aero::compute_propeller_rpm(engine_count, 48.0, input.throttle);

        let (yaw_deg, pitch_deg, roll_deg) = quat_to_yaw_pitch_roll(self.orientation);
        let lift_n = buoy.hot_air_lift_n + buoy.levitite_lift_n + aero.wing_lift_n;
        StepTelemetry {
            altitude: self.position[1] + self.mass.center_of_mass[1],
            speed_mps: v_len(self.velocity),
            vertical_speed: self.velocity[1],
            lift_ratio: lift_n / weight_n.max(1.0),
            throttle: self.throttle,
            yaw_deg,
            pitch_deg,
            roll_deg,
            grounded: self.grounded,
        }
    }

    fn resolve_world_collision(&mut self, is_solid: &impl Fn(Pos) -> bool) -> bool {
        let mut hit_ground = false;
        // Sub-step separation up to 3 passes so hull corners rest cleanly atop terrain blocks.
        for _ in 0..3 {
            let mut push_y = 0.0_f64;
            let mut push_xz = [0.0_f64; 2];
            for cell in &self.blocks {
                // Skip wheel mount hubs themselves when tires are fitted so suspension handles contact.
                if cell.tire.is_some() {
                    continue;
                }
                let local_c = [
                    f64::from(cell.local[0]) + 0.5,
                    f64::from(cell.local[1]) + 0.5,
                    f64::from(cell.local[2]) + 0.5,
                ];
                let w = self.local_to_world(local_c);
                let bottom = w[1] - 0.5;
                let gx = w[0].floor() as i32;
                let gy = bottom.floor() as i32;
                let gz = w[2].floor() as i32;
                if is_solid([gx, gy, gz]) {
                    let top_of_block = f64::from(gy + 1);
                    let penetration_y = top_of_block - bottom;
                    if penetration_y > 0.0 && penetration_y <= 1.05 {
                        push_y = push_y.max(penetration_y);
                        hit_ground = true;
                    } else {
                        let cx = f64::from(gx) + 0.5;
                        let cz = f64::from(gz) + 0.5;
                        let dx = w[0] - cx;
                        let dz = w[2] - cz;
                        if dx.abs() > dz.abs() {
                            push_xz[0] += dx.signum() * 0.12;
                        } else {
                            push_xz[1] += dz.signum() * 0.12;
                        }
                    }
                }
            }
            if push_y > 0.0 {
                self.position[1] += push_y;
                if self.velocity[1] < 0.0 {
                    self.velocity[1] = 0.0;
                }
                // Ground hull friction slows un-wheeled belly landings.
                self.velocity[0] *= 0.85;
                self.velocity[2] *= 0.85;
                self.angular_velocity[0] *= 0.7;
                self.angular_velocity[2] *= 0.7;
            }
            if push_xz[0] != 0.0 || push_xz[1] != 0.0 {
                self.position[0] += push_xz[0].clamp(-0.5, 0.5);
                self.position[2] += push_xz[1].clamp(-0.5, 0.5);
                self.velocity[0] *= 0.5;
                self.velocity[2] *= 0.5;
            }
            if push_y == 0.0 && push_xz == [0.0, 0.0] {
                break;
            }
        }
        hit_ground
    }

    /// Snaps every contraption block back to unique world grid positions with nearest 90° yaw steps.
    /// Returns `(world_pos, cell_with_rotated_facing, cw_90_steps)`.
    pub fn snap_disassembly(&self) -> Vec<(Pos, BlockCell, i32)> {
        let (yaw_deg, _, _) = quat_to_yaw_pitch_roll(self.orientation);
        // Positive yaw turns counter-clockwise in Y-up; clockwise 90° steps = round(-yaw / 90).
        let cw_steps = ((-yaw_deg / 90.0).round() as i32).rem_euclid(4);
        let yaw_rad = -f64::from(cw_steps) * std::f64::consts::FRAC_PI_2;
        let (sin_y, cos_y) = yaw_rad.sin_cos();

        let cm_world = v_add(self.position, self.mass.center_of_mass);
        let anchor_world = [
            (cm_world[0] - self.mass.center_of_mass[0]).round() as i32,
            (cm_world[1] - self.mass.center_of_mass[1]).round() as i32,
            (cm_world[2] - self.mass.center_of_mass[2]).round() as i32,
        ];

        let mut used: HashSet<Pos> = HashSet::with_capacity(self.blocks.len());
        let mut out = Vec::with_capacity(self.blocks.len());

        for cell in &self.blocks {
            let rx = f64::from(cell.local[0]);
            let rz = f64::from(cell.local[2]);
            let wx = (rx * cos_y - rz * sin_y).round() as i32 + anchor_world[0];
            let wy = cell.local[1] + anchor_world[1];
            let wz = (rx * sin_y + rz * cos_y).round() as i32 + anchor_world[2];
            let mut target = [wx, wy, wz];
            while used.contains(&target) {
                target[1] += 1;
            }
            used.insert(target);
            let mut placed = cell.clone();
            placed.facing = cell.facing.rotate_cw(cw_steps);
            out.push((target, placed, cw_steps));
        }
        out
    }
}
