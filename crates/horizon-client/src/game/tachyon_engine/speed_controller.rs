//! Rotation speed controllers by Create's SpeedControllerBlockEntity and
//! RotationPropagator (MIT, THIRD_PARTY_NOTICES.md). The lower shaft and the
//! large cogwheel above it can turn at different speeds; the target's sign
//! chooses the driven side's direction, not the driving side's sign.

use super::kinetic::{Axis, Part};

/// SpeedControllerBlockEntity.DEFAULT_SPEED, in RPM.
pub(super) const DEFAULT_SPEED: i32 = 16;
/// The stationary partial used by Create's SpeedControllerRenderer.
pub(super) const BRACKET_MODEL: &str = "create:block/rotation_speed_controller/bracket";

/// KineticScrollValueBehaviour's limits, from CKinetics.maxRotationSpeed.
pub(super) fn limit(speed: i32) -> i32 {
    let max = super::kinetic::MAX_SPEED as i32;
    speed.clamp(-max, max)
}

/// RotationPropagator.isLargeCogToSpeedController: a large cogwheel right
/// above the controller, on the other horizontal axis. Whether the link
/// points into the controller rather than the cogwheel.
pub(super) fn connection(from: Part, to: Part, diff: [i32; 3]) -> Option<bool> {
    let (shaft, wheel, targeting) = match (from, to, diff) {
        (Part::SpeedController(a), Part::LargeCog(b), [0, 1, 0]) => (a, b, false),
        (Part::LargeCog(b), Part::SpeedController(a), [0, -1, 0]) => (a, b, true),
        _ => return None,
    };
    (shaft != Axis::Y && wheel != Axis::Y && shaft != wheel).then_some(targeting)
}

/// Which side first gave the controller its speed in this network.
#[derive(Clone, Copy)]
pub(super) enum Drive {
    None,
    Shaft,
    Wheel,
}

/// SpeedControllerBlockEntity.getDesiredOutputSpeed and getConveyedSpeed.
/// A shaft-fed controller sets the wheel's speed; a wheel-fed one sets its
/// shaft's. The return link keeps the driving side's speed, and a faster
/// speed already turning the same way wins, as in Create.
pub(super) fn conveyed(target: i32, shaft: f32, wheel: f32, drive: Drive, targeting: bool) -> f32 {
    let desired = if target == 0 || (targeting && wheel == 0.0) {
        0.0
    } else {
        match (drive, targeting) {
            (Drive::None, false) => 0.0,
            (Drive::None | Drive::Wheel, true) => limit(target) as f32,
            (Drive::Wheel, false) => wheel,
            (Drive::Shaft, true) => shaft,
            (Drive::Shaft, false) => limit(target) as f32,
        }
    };
    let compare = if targeting { shaft } else { wheel };
    if desired >= 0.0 && compare >= 0.0 {
        desired.max(compare)
    } else if desired < 0.0 && compare < 0.0 {
        desired.min(compare)
    } else {
        desired
    }
}

#[cfg(test)]
mod tests {
    use super::super::{add, kinetic};
    use super::*;
    use kinetic::Dir;
    use std::collections::{HashMap, HashSet};

    fn line(axis: Axis, source: Part, machine: Part) -> HashMap<[i32; 3], Part> {
        let below = axis.dirs()[0].offset();
        let wheel_axis = if axis == Axis::X { Axis::Z } else { Axis::X };
        HashMap::from([
            (below.map(|v| v * 2), source),
            (below, Part::Shaft(axis)),
            ([0, 0, 0], Part::SpeedController(axis)),
            ([0, 1, 0], Part::LargeCog(wheel_axis)),
            (add([0, 1, 0], wheel_axis.dirs()[1].offset()), machine),
        ])
    }

    fn solve(
        parts: &HashMap<[i32; 3], Part>,
        source: [i32; 3],
        speed: f32,
        target: i32,
    ) -> kinetic::Solved {
        kinetic::solve(
            parts,
            &HashMap::from([(source, speed)]),
            &HashSet::new(),
            &HashMap::from([([0, 0, 0], target)]),
        )
    }

    #[test]
    fn a_controller_drives_the_large_cogwheel_at_its_signed_target() {
        for (axis, wheel_axis) in [(Axis::X, Axis::Z), (Axis::Z, Axis::X)] {
            let source = axis.dirs()[0].offset().map(|v| v * 2);
            let machine = add([0, 1, 0], wheel_axis.dirs()[1].offset());
            let parts = line(axis, Part::Motor(axis.dirs()[1]), Part::Fan(wheel_axis));
            for input in [-32.0, 32.0] {
                for target in [-500, -64, DEFAULT_SPEED, 64, 500] {
                    let s = solve(&parts, source, input, target);
                    let want = limit(target) as f32;
                    assert_eq!(s.speeds[&[0, 0, 0]], [input; 2]);
                    assert_eq!(s.speeds[&[0, 1, 0]], [want; 2]);
                    assert_eq!(s.speeds[&machine], [want; 2]);
                    assert!(s.jammed.is_empty());
                }
                let s = kinetic::solve(
                    &parts, &HashMap::from([(source, input)]), &HashSet::new(), &HashMap::new(),
                );
                assert_eq!(s.speeds[&[0, 1, 0]], [DEFAULT_SPEED as f32; 2]);
            }
        }
    }

    #[test]
    fn the_load_is_at_the_target_speed_but_capacity_is_at_the_source_speed() {
        let parts = line(Axis::X, Part::Crank(Dir::East), Part::Press(Axis::Z));
        for input in [-32.0, 32.0] {
            for target in [-64, 64] {
                let s = solve(&parts, [-2, 0, 0], input, target);
                let stress = s.stress[&[0, 1, 1]];
                assert_eq!((stress.load, stress.capacity), (8.0 * 64.0, 8.0 * 32.0));
                assert_eq!(s.stress[&[-2, 0, 0]], stress);
                assert_eq!(s.stress[&[0, 0, 0]], stress);
                assert!(stress.over());
                assert!(s.speeds.is_empty());
                assert!(s.jammed.is_empty());
            }
            let s = solve(&parts, [-2, 0, 0], input, DEFAULT_SPEED);
            assert_eq!(s.stress[&[0, 0, 0]].load, 8.0 * DEFAULT_SPEED as f32);
            assert!(!s.stress[&[0, 0, 0]].over());
            assert_eq!(s.speeds[&[0, 1, 1]], [DEFAULT_SPEED as f32; 2]);
        }
        assert_eq!(Part::SpeedController(Axis::X).impact(), 0.0);
        assert_eq!(kinetic::capacity(Part::SpeedController(Axis::X), 64.0), 0.0);
    }

    #[test]
    fn no_input_or_a_zero_target_leaves_the_output_still() {
        let parts = line(Axis::X, Part::Motor(Dir::East), Part::Fan(Axis::Z));
        let s = kinetic::solve(&parts, &HashMap::new(), &HashSet::new(), &HashMap::new());
        assert!(s.speeds.is_empty());
        assert!(s.stress.is_empty());
        assert!(s.jammed.is_empty());
        let stopped = solve(&parts, [-2, 0, 0], 0.0, 64);
        assert!(stopped.speeds.is_empty());
        for input in [-32.0, 32.0] {
            let s = solve(&parts, [-2, 0, 0], input, 0);
            assert_eq!(s.speeds[&[0, 0, 0]], [input; 2]);
            assert!(!s.speeds.contains_key(&[0, 1, 0]));
            assert!(!s.speeds.contains_key(&[0, 1, 1]));
        }
    }

    #[test]
    fn a_wheel_can_drive_the_controller_without_the_return_link_changing_it() {
        let parts = HashMap::from([
            ([0, 1, -1], Part::Motor(Dir::South)),
            ([0, 1, 0], Part::LargeCog(Axis::Z)),
            ([0, 0, 0], Part::SpeedController(Axis::X)),
            ([1, 0, 0], Part::Fan(Axis::X)),
        ]);
        for input in [-32.0, 32.0] {
            for target in [-64, 64] {
                let s = solve(&parts, [0, 1, -1], input, target);
                assert_eq!(s.speeds[&[0, 1, 0]], [input; 2]);
                assert_eq!(s.speeds[&[0, 0, 0]], [target as f32; 2]);
                assert_eq!(s.speeds[&[1, 0, 0]], [target as f32; 2]);
                assert!(s.jammed.is_empty());
            }
        }
    }

    #[test]
    fn only_a_perpendicular_horizontal_large_cogwheel_above_connects() {
        let controller = Part::SpeedController(Axis::X);
        let wheel = Part::LargeCog(Axis::Z);
        assert_eq!(connection(controller, wheel, [0, 1, 0]), Some(false));
        assert_eq!(connection(wheel, controller, [0, -1, 0]), Some(true));
        for other in [
            Part::Cog(Axis::Z), Part::LargeCog(Axis::X), Part::LargeCog(Axis::Y), Part::Shaft(Axis::Z),
        ] {
            assert_eq!(connection(controller, other, [0, 1, 0]), None);
        }
        for d in Dir::ALL {
            assert_eq!(controller.shaft_toward(d), d.axis() == Axis::X);
            if d != Dir::Up {
                assert_eq!(connection(controller, wheel, d.offset()), None);
            }
        }
        assert_eq!(connection(controller, wheel, [0, 2, 0]), None);
    }

    #[test]
    fn conveyed_speed_keeps_a_faster_speed_only_in_the_same_direction() {
        assert_eq!(conveyed(16, -32.0, 0.0, Drive::Shaft, false), 16.0);
        assert_eq!(conveyed(16, 32.0, 64.0, Drive::Shaft, false), 64.0);
        assert_eq!(conveyed(-16, 32.0, -64.0, Drive::Shaft, false), -64.0);
        assert_eq!(conveyed(-16, 32.0, 64.0, Drive::Shaft, false), -16.0);
        assert_eq!(conveyed(16, 0.0, 0.0, Drive::None, false), 0.0);
        assert_eq!(conveyed(16, 0.0, 0.0, Drive::None, true), 0.0);
    }
}
