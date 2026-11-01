//! Water wheels by Create's WaterWheelBlockEntity and
//! LargeWaterWheelBlockEntity (MIT, THIRD_PARTY_NOTICES.md): flows in the
//! wheel's plane vote for a turn, at four neighbours for a small wheel and
//! twelve outer rim blocks for a large one. The winning direction turns at
//! 8 RPM divided by the size (1 or 2).

use super::add;
use super::kinetic::{Axis, Dir};
use glam::Vec3;

/// The LARGE partial in Create's AllPartialModels, about Y.
pub(super) const LARGE_MODEL: &str = "create:block/large_water_wheel/block";

/// The LARGE or LARGE_EXTENSION partial from Create's WaterWheelRenderer.
/// The template has no extension property, so follow the negative-side
/// neighbour as LargeWaterWheelBlock.updateShape does.
pub(super) fn large_model(axis: Axis, wheel_at: impl Fn([i32; 3]) -> bool) -> &'static str {
    if wheel_at(axis.dirs()[0].offset()) {
        "create:block/large_water_wheel/block_extension"
    } else {
        LARGE_MODEL
    }
}

/// WaterWheelBlockEntity.getOffsetsToCheck: the four sides of a small
/// wheel, or each side two blocks out with one block to either side of it.
fn offsets(axis: Axis, size: i32) -> Vec<[i32; 3]> {
    let mut out = Vec::new();
    for d in Dir::ALL {
        if d.axis() == axis {
            continue;
        }
        let middle = d.offset().map(|c| c * size);
        out.push(middle);
        if size == 2 {
            for side in Dir::ALL {
                if side.axis() != axis && side.axis() != d.axis() {
                    out.push(add(middle, side.offset()));
                }
            }
        }
    }
    out
}

/// WaterWheelBlockEntity.determineAndApplyFlowScore and getGeneratedSpeed:
/// project each flow into the plane, normalize it and the rim position,
/// and count its direction only when its dot along the tangent exceeds
/// 0.5 in magnitude. Clamp the score, then divide the speed by the size.
pub(super) fn speed(axis: Axis, large: bool, flow_at: impl Fn([i32; 3]) -> Vec3) -> f32 {
    let size = if large { 2 } else { 1 };
    let along = Vec3::from(axis.dirs()[1].offset().map(|c| c as f32));
    let score: i32 = offsets(axis, size)
        .into_iter()
        .map(|offset| {
            let normal = Vec3::from(offset.map(|c| c as f32)).normalize();
            let mut flow = flow_at(offset);
            flow -= along * flow.dot(along);
            if flow.length_squared() == 0.0 {
                return 0;
            }
            let round = normal.cross(flow.normalize()).dot(along);
            if round.abs() > 0.5 {
                round.signum() as i32
            } else {
                0
            }
        })
        .sum();
    score.clamp(-1, 1) as f32 * 8.0 / size as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn a_large_wheel_checks_the_twelve_outer_rim_blocks() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let small = offsets(axis, 1);
            assert_eq!(small.len(), 4);
            assert_eq!(small.iter().collect::<HashSet<_>>().len(), 4);
            for p in small {
                assert_eq!(axis.pick(p), 0);
                assert_eq!(p.iter().map(|v| v.abs()).sum::<i32>(), 1);
            }
            let large = offsets(axis, 2);
            assert_eq!(large.len(), 12);
            assert_eq!(large.iter().collect::<HashSet<_>>().len(), 12);
            for p in large {
                assert_eq!(axis.pick(p), 0);
                assert!(matches!(p.iter().map(|v| v * v).sum::<i32>(), 4 | 5));
            }
        }
    }

    #[test]
    fn water_wheels_turn_by_the_vote_and_their_size() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let along = Vec3::from(axis.dirs()[1].offset().map(|c| c as f32));
            let tangent = |p: [i32; 3]| along.cross(Vec3::from(p.map(|c| c as f32)));
            for (large, want) in [(false, 8.0), (true, 4.0)] {
                assert_eq!(speed(axis, large, tangent), want);
                assert_eq!(speed(axis, large, |p| -tangent(p)), -want);
                assert_eq!(
                    speed(axis, large, |p| tangent(p) * 0.001 + along * 100.0),
                    want
                );
                assert_eq!(speed(axis, large, |p| Vec3::from(p.map(|c| c as f32))), 0.0);
                assert_eq!(speed(axis, large, |_| along), 0.0);
                assert_eq!(speed(axis, large, |_| Vec3::ZERO), 0.0);
            }
        }
    }

    #[test]
    fn a_large_wheel_takes_flow_from_its_own_rim_not_the_small_wheels() {
        let outer = |p| if p == [2, 0, 0] { Vec3::NEG_Y } else { Vec3::ZERO };
        assert_eq!(speed(Axis::Z, true, outer), -4.0);
        assert_eq!(speed(Axis::Z, false, outer), 0.0);
        let inner = |p| if p == [1, 0, 0] { Vec3::NEG_Y } else { Vec3::ZERO };
        assert_eq!(speed(Axis::Z, true, inner), 0.0);
        assert_eq!(speed(Axis::Z, false, inner), -8.0);
    }

    #[test]
    fn opposing_flows_cancel_whatever_their_strength() {
        let flow = |p| match p {
            [2, 0, 0] => Vec3::NEG_Y * 0.001,
            [-2, 0, 0] => Vec3::NEG_Y * 100.0,
            _ => Vec3::ZERO,
        };
        assert_eq!(speed(Axis::Z, true, flow), 0.0);
        assert_eq!(
            speed(Axis::Z, true, |p| if p == [0, -2, 0] { Vec3::X } else { flow(p) }),
            4.0
        );
    }

    #[test]
    fn the_diagonal_rim_position_is_normalized_before_voting() {
        let radial = |p| if p == [2, 1, 0] { Vec3::X } else { Vec3::ZERO };
        assert_eq!(speed(Axis::Z, true, radial), 0.0);
        let round = |p| if p == [2, 1, 0] { Vec3::Y } else { Vec3::ZERO };
        assert_eq!(speed(Axis::Z, true, round), 4.0);
        let shallow = |p| if p == [2, 0, 0] { Vec3::new(2.0, 1.0, 0.0) } else { Vec3::ZERO };
        assert_eq!(speed(Axis::Z, true, shallow), 0.0);
    }

    #[test]
    fn a_large_wheel_extends_only_a_wheel_on_the_negative_side() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            assert_eq!(large_model(axis, |_| false), LARGE_MODEL);
            for d in Dir::ALL {
                let want = if d == axis.dirs()[0] {
                    "create:block/large_water_wheel/block_extension"
                } else {
                    LARGE_MODEL
                };
                assert_eq!(large_model(axis, |p| p == d.offset()), want);
            }
        }
    }
}
