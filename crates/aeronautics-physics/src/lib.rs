//! Pure-Rust 6-DOF rigid-body physics, buoyancy, aerodynamics, propulsion and offroad wheels
//! for Create Aeronautics (`aeronautics`, `simulated`, `offroad`) in Tachyon.

pub mod aero;
pub mod assembly;
pub mod body;
pub mod buoyancy;
pub mod catalog;
pub mod mass;
pub mod math;
pub mod sensors;
pub mod wheels;

pub use aero::{AeroResult, PilotInput, compute_propeller_rpm, step_aero};
pub use assembly::{AssemblyError, WorldBlockSample, assemble_from_world};
pub use body::{RigidContraption, StepTelemetry};
pub use buoyancy::{BalloonPocket, BuoyancySummary, detect_balloons, solve_buoyancy};
pub use catalog::{BlockMass, BlockRole, TireKind, classify, is_engine, is_envelope, is_sail, is_unmovable};
pub use mass::{BlockCell, MassProperties, compute_mass_properties};
pub use math::{
    Facing, GRAVITY, IDENTITY_QUAT, Pos, Quat, Vec3, air_density, quat_from_euler,
    quat_integrate_omega, quat_mul, quat_normalize, quat_rotate, quat_rotate_inv, quat_slerp,
    quat_to_yaw_pitch_roll,
};
pub use sensors::{
    altitude_sensor_signal, burner_state_for_heat, burner_variant_for_heat,
    velocity_sensor_signal,
};
pub use wheels::{WheelSummary, step_wheels};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::math::v_len;
    use std::collections::HashMap;

    fn make_balloon_airship() -> RigidContraption {
        let mut blocks = Vec::new();
        // Hull platform at y = 0 (spruce planks)
        for x in -1..=1 {
            for z in -1..=1 {
                blocks.push(BlockCell::new([x, 0, z], "minecraft:spruce_planks", Facing::North));
            }
        }
        // Burner at center y = 1 + propeller facing north + steering wheel
        let mut burner = BlockCell::new([0, 1, 0], "aeronautics:adjustable_burner", Facing::North);
        burner.heat = 10;
        blocks.push(burner);
        blocks.push(BlockCell::new([0, 0, -2], "aeronautics:propeller_bearing", Facing::North));
        blocks.push(BlockCell::new([0, 1, -1], "simulated:steering_wheel", Facing::North));

        // 5x5x5 hollow envelope dome above burner (y = 2..=6, envelope walls on sides & ceiling)
        for y in 2..=6 {
            for x in -2_i32..=2 {
                for z in -2_i32..=2 {
                    let is_wall = x.abs() == 2 || z.abs() == 2 || y == 6;
                    if is_wall {
                        blocks.push(BlockCell::new([x, y, z], "aeronautics:white_envelope", Facing::North));
                    }
                }
            }
        }
        RigidContraption::from_blocks(1, [0.0, 70.0, 0.0], blocks)
    }

    fn make_offroad_rover() -> RigidContraption {
        let mut blocks = Vec::new();
        for x in -1..=1 {
            for z in -2..=2 {
                blocks.push(BlockCell::new([x, 0, z], "minecraft:spruce_planks", Facing::North));
            }
        }
        blocks.push(BlockCell::new([0, 1, 0], "simulated:steering_wheel", Facing::North));
        blocks.push(BlockCell::new([0, 1, 1], "simulated:black_portable_engine", Facing::North));
        for (wx, wz) in [(-2, -2), (2, -2), (-2, 2), (2, 2)] {
            let mut wheel = BlockCell::new([wx, 0, wz], "offroad:wheel_mount", Facing::East);
            wheel.tire = Some(TireKind::Standard);
            blocks.push(wheel);
        }
        RigidContraption::from_blocks(2, [0.0, 65.1, 0.0], blocks)
    }

    #[test]
    fn mass_and_inertia_compute_from_blocks() {
        let blocks = vec![
            BlockCell::new([-1, 0, 0], "minecraft:iron_block", Facing::North),
            BlockCell::new([1, 0, 0], "minecraft:iron_block", Facing::North),
        ];
        let props = compute_mass_properties(&blocks);
        assert!((props.total_mass - 900.0).abs() < 1e-6);
        assert!((props.center_of_mass[0] - 0.5).abs() < 1e-6);
        assert!(props.inertia_diag[1] > props.inertia_diag[0]);
    }

    #[test]
    fn balloon_cavity_detects_pocket_and_lifts_vessel() {
        let mut ship = make_balloon_airship();
        assert_eq!(ship.balloons.len(), 1);
        assert_eq!(ship.balloons[0].volume, 3 * 3 * 4); // 36 air cells inside 5x5x5 dome (y=2..=5)

        let start_y = ship.position[1];
        for _ in 0..40 {
            ship.step(0.05, PilotInput { vertical: 0.6, ..Default::default() }, |_| false);
        }
        assert!(ship.position[1] > start_y + 1.0, "airship should climb, y={}", ship.position[1]);
    }

    #[test]
    fn balloon_reaches_altitude_ceiling_and_levitite_lifts() {
        let high_density = air_density(64.0);
        let ultra_high_density = air_density(1000.0);
        assert!(high_density > ultra_high_density * 4.0);

        let blocks = vec![
            BlockCell::new([0, 0, 0], "minecraft:oak_planks", Facing::North),
            BlockCell::new([0, 2, 0], "aeronautics:levitite", Facing::North),
            BlockCell::new([0, 2, 1], "aeronautics:pearlescent_levitite", Facing::North),
        ];
        let mut ship = RigidContraption::from_blocks(3, [0.0, 64.0, 0.0], blocks);
        for _ in 0..30 {
            ship.step(0.05, PilotInput::default(), |_| false);
        }
        assert!(ship.position[1] > 64.5);
    }

    #[test]
    fn propeller_accelerates_forward_and_reaches_terminal_speed() {
        let blocks = vec![
            BlockCell::new([0, 0, 0], "aeronautics:levitite", Facing::North),
            BlockCell::new([0, 0, -1], "aeronautics:propeller_bearing", Facing::North),
            BlockCell::new([0, 0, 1], "simulated:white_portable_engine", Facing::North),
            BlockCell::new([-1, 0, 0], "simulated:white_symmetric_sail", Facing::North),
            BlockCell::new([1, 0, 0], "simulated:white_symmetric_sail", Facing::North),
        ];
        let mut craft = RigidContraption::from_blocks(4, [0.0, 100.0, 0.0], blocks);
        let mut last_z = craft.position[2];
        for _ in 0..100 {
            craft.step(0.05, PilotInput { throttle: 1.0, ..Default::default() }, |_| false);
        }
        assert!(craft.position[2] < last_z - 5.0, "north is -Z");
        assert!(v_len(craft.velocity) > 2.0 && v_len(craft.velocity) < 46.0);
        last_z = craft.position[2];
        for _ in 0..40 {
            craft.step(0.05, PilotInput { brake: true, ..Default::default() }, |_| false);
        }
        assert!(craft.position[2] <= last_z);
        assert!(v_len(craft.velocity) < 12.0);
    }

    #[test]
    fn gyroscopic_yaw_and_pendulum_righting_work() {
        let mut ship = make_balloon_airship();
        ship.orientation = quat_from_euler(0.0, 0.25, -0.2);
        // Idle step should bring pitch & roll back toward upright.
        for _ in 0..40 {
            ship.step(0.05, PilotInput::default(), |_| false);
        }
        let (_, pitch, roll) = quat_to_yaw_pitch_roll(ship.orientation);
        assert!(pitch.abs() < 6.0 && roll.abs() < 6.0, "pitch={pitch}, roll={roll}");

        // Active yaw input rotates ship heading cleanly.
        let (yaw0, _, _) = quat_to_yaw_pitch_roll(ship.orientation);
        for _ in 0..25 {
            ship.step(0.05, PilotInput { yaw: 1.0, ..Default::default() }, |_| false);
        }
        let (yaw1, _, _) = quat_to_yaw_pitch_roll(ship.orientation);
        assert!((yaw1 - yaw0).abs() > 5.0, "yaw0={yaw0} yaw1={yaw1}");
    }

    #[test]
    fn offroad_rover_settles_drives_turns_and_brakes_on_ground() {
        let mut rover = make_offroad_rover();
        let flat_ground = |p: Pos| p[1] <= 63;

        // Settle onto ground at y = 64.
        for _ in 0..40 {
            rover.step(0.05, PilotInput::default(), flat_ground);
        }
        assert!(rover.grounded);
        assert!(rover.position[1] >= 63.8 && rover.position[1] <= 66.0, "y={}", rover.position[1]);

        let start_z = rover.position[2];
        for _ in 0..50 {
            rover.step(0.05, PilotInput { throttle: 1.0, ..Default::default() }, flat_ground);
        }
        assert!(rover.position[2] < start_z - 1.5, "rover should roll north (-Z)");

        for _ in 0..40 {
            rover.step(0.05, PilotInput { brake: true, ..Default::default() }, flat_ground);
        }
        assert!(v_len(rover.velocity) < 3.0);
    }

    #[test]
    fn hull_world_collision_prevents_floor_penetration() {
        let blocks = vec![BlockCell::new([0, 0, 0], "minecraft:stone", Facing::North)];
        let mut drop = RigidContraption::from_blocks(5, [0.0, 66.0, 0.0], blocks);
        let floor = |p: Pos| p[1] <= 63;
        for _ in 0..80 {
            drop.step(0.05, PilotInput::default(), floor);
        }
        assert!(drop.grounded);
        assert!(drop.position[1] >= 63.99, "must not sink below floor y=64, got {}", drop.position[1]);
    }

    #[test]
    fn assemble_bfs_and_snap_disassembly_roundtrip() {
        let mut world: HashMap<Pos, WorldBlockSample> = HashMap::new();
        world.insert([10, 64, 10], WorldBlockSample {
            id: "simulated:physics_assembler".into(),
            facing: Facing::North,
            powered: false,
            heat: 0,
            tire: None,
        });
        world.insert([10, 64, 9], WorldBlockSample {
            id: "aeronautics:propeller_bearing".into(),
            facing: Facing::North,
            powered: true,
            heat: 0,
            tire: None,
        });
        world.insert([10, 63, 10], WorldBlockSample {
            id: "minecraft:bedrock".into(),
            facing: Facing::North,
            powered: false,
            heat: 0,
            tire: None,
        });

        let mut ship = assemble_from_world(99, [10, 64, 10], 128, |p| world.get(&p).cloned())
            .expect("should assemble 2 movable blocks and skip bedrock");
        assert_eq!(ship.blocks.len(), 2);

        // Rotate ~90° CW (yaw = -90 deg) and snap back to world grid.
        ship.orientation = quat_from_euler(-std::f64::consts::FRAC_PI_2, 0.0, 0.0);
        let snapped = ship.snap_disassembly();
        assert_eq!(snapped.len(), 2);
        let (_, prop_cell, steps) = snapped.iter().find(|(_, c, _)| c.id == "aeronautics:propeller_bearing").unwrap();
        assert_eq!(*steps, 1);
        assert_eq!(prop_cell.facing, Facing::East);
    }

    #[test]
    fn sensors_and_burner_variants_cover_full_range() {
        assert_eq!(altitude_sensor_signal(-64.0, -64.0, 320.0), 0);
        assert_eq!(altitude_sensor_signal(320.0, -64.0, 320.0), 15);
        assert_eq!(velocity_sensor_signal(0.0, 30.0), 0);
        assert_eq!(velocity_sensor_signal(30.0, 30.0), 15);
        assert_eq!(burner_state_for_heat(0), (false, "fire"));
        assert_eq!(burner_state_for_heat(8), (true, "fire"));
        assert_eq!(burner_state_for_heat(14), (true, "soulful"));
        assert_eq!(burner_variant_for_heat(0), "fire");
        assert_eq!(burner_variant_for_heat(4), "fire");
        assert_eq!(burner_variant_for_heat(9), "fire");
        assert_eq!(burner_variant_for_heat(15), "soulful");
    }
}
