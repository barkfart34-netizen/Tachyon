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

// ---- The large wheel's body -------------------------------------------------
// LargeWaterWheelBlock and WaterWheelStructuralBlock (Create, MIT): the wheel
// fills the eight blocks round it in its plane with structure blocks, each
// facing the next block on the way to the wheel. A wheel goes where its body
// has no room; a structure block that goes takes its wheel; a wheel that goes
// takes its structure blocks. After Kinetics for Horizon's port (sakurartro,
// MIT, THIRD_PARTY_NOTICES.md), on Create's own blocks.

/// The large wheel and its structure blocks.
pub(super) const LARGE: &str = "create:large_water_wheel";
pub(super) const STRUCTURE: &str = "create:water_wheel_structure";

/// `Direction.getClockWise(axis)`.
fn clockwise(d: Dir, axis: Axis) -> Dir {
    if d.axis() == axis {
        return d;
    }
    use Dir::*;
    match (axis, d) {
        // getClockWiseX: up, south, down, north.
        (Axis::X, Up) => South,
        (Axis::X, North) => Up,
        (Axis::X, Down) => North,
        (Axis::X, _) => Down,
        // getClockWise: north, east, south, west.
        (Axis::Y, North) => East,
        (Axis::Y, East) => South,
        (Axis::Y, South) => West,
        (Axis::Y, _) => North,
        // getClockWiseZ: up, east, down, west.
        (Axis::Z, Up) => East,
        (Axis::Z, East) => Down,
        (Axis::Z, Down) => West,
        (Axis::Z, _) => Up,
    }
}

/// The eight blocks a large wheel on `axis` fills round itself
/// (`LargeWaterWheelBlock.tick`), each with its structure block's facing.
pub(super) fn body(axis: Axis) -> Vec<([i32; 3], Dir)> {
    let mut out = Vec::new();
    for side in Dir::ALL.into_iter().filter(|d| d.axis() != axis) {
        out.push((side.offset(), side.opposite()));
        let target = clockwise(side, axis);
        out.push((add(side.offset(), target.offset()), target.opposite()));
    }
    out
}

/// Whether a block can be built over (`canBeReplaced`): vanilla's air,
/// fluids and plants; `layers` is a snow layer's height.
pub(super) fn replaceable(name: &str, layers: Option<&str>) -> bool {
    let Some(name) = name.strip_prefix("minecraft:") else { return false };
    match name {
        "air" | "cave_air" | "void_air" | "water" | "lava" | "short_grass" | "tall_grass" | "fern"
        | "large_fern" | "seagrass" | "tall_seagrass" | "vine" | "dead_bush" | "fire" | "soul_fire"
        | "bubble_column" | "glow_lichen" | "hanging_roots" | "light" | "structure_void" | "sculk_vein"
        | "nether_sprouts" | "crimson_roots" | "warped_roots" | "short_dry_grass" | "tall_dry_grass" => true,
        "snow" => layers == Some("1"),
        _ => false,
    }
}

/// What `stillValid` sees in a block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Body {
    Other,
    Wheel(Axis),
    Structure(Dir),
}

impl Body {
    fn of(name: &str, property: impl Fn(&str) -> Option<String>) -> Body {
        let parsed = match name {
            LARGE => property("axis").as_deref().and_then(Axis::parse).map(Body::Wheel),
            STRUCTURE => property("facing").as_deref().and_then(Dir::parse).map(Body::Structure),
            _ => None,
        };
        parsed.unwrap_or(Body::Other)
    }
}

/// `WaterWheelStructuralBlock.stillValid` for the structure block at `pos`.
pub(super) fn still_valid(
    get: &impl Fn([i32; 3]) -> Body,
    pos: [i32; 3],
    facing: Dir,
    adjacent: bool,
) -> bool {
    let target = add(pos, facing.offset());
    let part = get(target);
    if !adjacent
        && let Body::Structure(next) = part
        && still_valid(get, target, next, true)
    {
        return true;
    }
    matches!(part, Body::Wheel(axis) if axis != facing.axis())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Task {
    /// A large wheel was placed: fill its body.
    Build,
    /// A structure block went: its wheel goes too.
    Broken,
    /// A large wheel went: its structure blocks go.
    Removed,
}

/// Changes seen while a level ticks, settled after its tick.
static QUEUE: std::sync::Mutex<Vec<(&'static str, [i32; 3], Task)>> =
    std::sync::Mutex::new(Vec::new());

fn take(dimension: &str) -> Vec<([i32; 3], Task)> {
    let mut all = QUEUE.lock().unwrap_or_else(|e| e.into_inner());
    let mut out = Vec::new();
    all.retain(|&(d, p, t)| {
        let here = d == dimension;
        if here {
            out.push((p, t));
        }
        !here
    });
    out
}

fn near() -> impl Iterator<Item = [i32; 3]> {
    (-1..=1)
        .flat_map(|x| (-1..=1).flat_map(move |y| (-1..=1).map(move |z| [x, y, z])))
        .filter(|d| *d != [0; 3])
}

/// The axis a face is on.
fn face_axis(face: horizon_core::geometry::Direction) -> Axis {
    use horizon_core::geometry::Direction as D;
    match face {
        D::West | D::East => Axis::X,
        D::Down | D::Up => Axis::Y,
        D::North | D::South => Axis::Z,
    }
}

impl super::super::GameLevel<'_> {
    /// A large wheel came or went, or a structure block went.
    pub(in crate::game) fn tachyon_water_wheel_changed(
        &self,
        dimension: &'static str,
        pos: [i32; 3],
        old: horizon_world::registry::BlockStateId,
        new: horizon_world::registry::BlockStateId,
    ) {
        let (old, new) = (self.native_state_block(old), self.native_state_block(new));
        let queue = |task| {
            QUEUE.lock().unwrap_or_else(|e| e.into_inner()).push((dimension, pos, task));
        };
        if new == Some(LARGE) && old != Some(LARGE) {
            queue(Task::Build);
        }
        if old == Some(LARGE) && new != Some(LARGE) {
            queue(Task::Removed);
        }
        if old == Some(STRUCTURE) && new != Some(STRUCTURE) {
            queue(Task::Broken);
        }
    }

    /// `LargeWaterWheelBlock.getStateForPlacement`: on the clicked face's
    /// axis, and only where the eight blocks round it can be built over.
    pub(in crate::game) fn tachyon_water_wheel_place(
        &self,
        state: horizon_world::registry::BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
    ) {
        if answer.is_some() || self.native_state_block(state) != Some(LARGE) {
            return;
        }
        let axis = face_axis(context.face);
        let p = context.pos;
        let fits = body(axis).into_iter().all(|(off, _)| {
            let there = self.world.block(p.x + off[0], p.y + off[1], p.z + off[2]);
            let thin = self.native_state_with_property(there, "layers", "1") == Some(there);
            (self.native_state_block(there)).is_some_and(|name| replaceable(name, thin.then_some("1")))
        });
        if !fits {
            *answer = Some(None);
        } else if let Some(placed) = self.native_state_with_property(state, "axis", match axis {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        }) {
            *answer = Some(Some(placed));
        }
    }
}

impl super::super::Game<'_> {
    /// After a level's tick: build the bodies of new wheels, and take away
    /// the wheels and the structure blocks left without each other.
    pub(in crate::game) fn tachyon_water_wheel_settle(&mut self, dimension: &str) {
        use horizon_core::position::BlockPos;
        let tasks = take(dimension);
        if tasks.is_empty() {
            return;
        }
        fn destroy(game: &mut super::super::Game<'_>, p: [i32; 3], drop: bool) {
            let at = BlockPos::new(p[0], p[1], p[2]);
            game.with_level(|b, level, ticks| b.destroy_block(level, ticks, at, drop));
            let _ = game.process_level_events();
        }
        for (pos, task) in tasks {
            match task {
                Task::Build => {
                    if self.native_block(pos) != LARGE {
                        continue;
                    }
                    let axis = self.native_block_property(pos, "axis");
                    let Some(axis) = axis.as_deref().and_then(Axis::parse) else {
                        continue;
                    };
                    let mut missing = Vec::new();
                    let mut blocked = false;
                    for (off, facing) in body(axis) {
                        let p = add(pos, off);
                        let name = self.native_block(p);
                        let facing_now = self.native_block_property(p, "facing");
                        if name == STRUCTURE && facing_now.as_deref() == Some(facing.name()) {
                            continue;
                        }
                        let layers = self.native_block_property(p, "layers");
                        if !replaceable(&name, layers.as_deref()) {
                            blocked = true;
                            break;
                        }
                        missing.push((p, facing));
                    }
                    if blocked {
                        // Create: `destroyBlock(pos, false)`.
                        destroy(self, pos, false);
                        continue;
                    }
                    let Some(default) = self.pack.assets.registry.default_state(STRUCTURE) else {
                        log::warn!("Tachyon: no {STRUCTURE} for a large water wheel's body");
                        continue;
                    };
                    for (p, facing) in missing {
                        let placed = self.native_state_with_property(default, "facing", facing.name());
                        if let Some(id) = placed {
                            self.native_set_block(dimension, p, id, 3);
                        }
                    }
                }
                Task::Broken => {
                    if self.native_block(pos) == STRUCTURE {
                        continue;
                    }
                    let wheels: Vec<[i32; 3]> = near()
                        .map(|d| add(pos, d))
                        .filter(|&m| {
                            let axis = self.native_block_property(m, "axis");
                            self.native_block(m) == LARGE
                                && axis.as_deref().and_then(Axis::parse).is_some_and(|axis| {
                                    body(axis).iter().any(|(off, _)| add(m, *off) == pos)
                                })
                        })
                        .collect();
                    let drop = !self.creative;
                    for m in wheels {
                        destroy(self, m, drop);
                    }
                }
                Task::Removed => {
                    let get = |q: [i32; 3]| {
                        Body::of(&self.native_block(q), |key| self.native_block_property(q, key))
                    };
                    let stale: Vec<[i32; 3]> = near()
                        .map(|d| add(pos, d))
                        .filter(|&p| {
                            matches!(get(p), Body::Structure(f) if !still_valid(&get, p, f, false))
                        })
                        .collect();
                    let Some(air) = self.pack.assets.registry.default_state("minecraft:air") else {
                        continue;
                    };
                    for p in stale {
                        self.native_set_block(dimension, p, air, 3);
                    }
                }
            }
        }
    }
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
        let flow = |p: [i32; 3]| match p {
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

    #[test]
    fn the_body_is_the_ring_of_eight_each_leading_to_the_wheel() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let ring = body(axis);
            assert_eq!(ring.len(), 8);
            assert_eq!(ring.iter().map(|(p, _)| *p).collect::<HashSet<_>>().len(), 8);
            let wheel = |p: [i32; 3]| {
                if p == [0; 3] {
                    Body::Wheel(axis)
                } else if let Some((_, f)) = ring.iter().find(|(q, _)| *q == p) {
                    Body::Structure(*f)
                } else {
                    Body::Other
                }
            };
            for (p, facing) in &ring {
                assert_eq!(axis.pick(*p), 0);
                assert!(p.iter().all(|c| c.abs() <= 1));
                assert!(still_valid(&wheel, *p, *facing, false), "{axis:?} {p:?}");
            }
            // Without the wheel no structure block is valid.
            let gone = |p: [i32; 3]| if p == [0; 3] { Body::Other } else { wheel(p) };
            assert!(ring.iter().all(|(p, f)| !still_valid(&gone, *p, *f, false)));
        }
    }

    #[test]
    fn only_air_fluids_and_plants_make_room_for_the_body() {
        for free in ["minecraft:air", "minecraft:water", "minecraft:short_grass", "minecraft:vine"] {
            assert!(replaceable(free, None), "{free}");
        }
        assert!(replaceable("minecraft:snow", Some("1")));
        assert!(!replaceable("minecraft:snow", Some("2")));
        for taken in ["minecraft:stone", "create:shaft", STRUCTURE, "air"] {
            assert!(!replaceable(taken, None), "{taken}");
        }
    }
}
