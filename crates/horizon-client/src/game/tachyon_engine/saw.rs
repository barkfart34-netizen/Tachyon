//! Create's mechanical saw (SawBlock, SawBlockEntity,
//! BlockBreakingKineticBlockEntity, SawRenderer; MIT,
//! THIRD_PARTY_NOTICES.md) on create:mechanical_saw. Facing up, it cuts the
//! items lying on it by Create's cutting recipes (else a stonecutter's) and
//! pushes what it made off the side it turns toward. Facing sideways, it
//! breaks the block in front, and a log it cuts brings down the logs above.
//!
//! Not as Create: the items stay item entities on the blade while it cuts
//! (so nothing is lost when the world closes), one recipe per input (the
//! first), no crack on the block it breaks, no harm to who stands on it.
use super::kinetic::{Axis, Dir, Part};
use horizon_client::items::ItemStack;
use horizon_core::random::RandomSource;
use std::collections::{HashSet, VecDeque};

pub(super) const SAW: &str = "create:mechanical_saw";
/// TreeCutter's limit on what one cut fells.
const MOST_LOGS: usize = 256;

/// `DirectionalAxisKineticBlock.getStateForPlacement` with SawBlock's
/// flipped casing: the blade faces away from where the player looks, its
/// shaft across their horizontal direction (`along_first`: along x).
pub(super) fn placement(yaw: f32, pitch: f32) -> (Dir, bool, bool) {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let look = [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()];
    let axis = (0..3)
        .max_by(|a, b| look[*a].abs().total_cmp(&look[*b].abs()))
        .unwrap_or(1);
    let looking = Dir::ALL
        .into_iter()
        .find(|d| d.offset()[axis] == look[axis].signum() as i32 && d.offset()[axis] != 0)
        .unwrap_or(Dir::Down);
    // Direction.fromYRot: 0 south, 1 west, 2 north, 3 east.
    let horizontal = match ((f64::from(yaw) / 90.0 + 0.5).floor() as i32).rem_euclid(4) {
        0 => Dir::South,
        1 => Dir::West,
        2 => Dir::North,
        _ => Dir::East,
    };
    let facing = looking.opposite();
    let along_first = if facing.axis() == Axis::Y {
        horizontal.axis() == Axis::X
    } else {
        facing.axis() == Axis::Z
    };
    (
        facing,
        along_first,
        facing.axis() == Axis::Y && horizontal.positive(),
    )
}

/// `SawBlockEntity.getItemMovementVec`: the side an upward saw pushes to.
pub(super) fn movement(along_first: bool, speed: f32) -> Dir {
    let d = if along_first { Dir::North } else { Dir::East };
    if speed < 0.0 { d.opposite() } else { d }
}

/// Where a made stack leaves the blade, and how fast: just past the edge.
fn ejection(pos: [i32; 3], out: Dir) -> ([f64; 3], [f64; 3]) {
    let d = out.offset().map(f64::from);
    let mut at = [0, 1, 2].map(|i| f64::from(pos[i]) + 0.5 + d[i] * 0.55);
    at[1] = f64::from(pos[1]) + 1.0;
    (at, [d[0] * 0.0625, 0.125, d[2] * 0.0625])
}

/// `SawBlock.updateEntityAfterFallOn`: an item that has come to rest on the
/// upward casing, inside this block.
fn landed(pos: [i32; 3], at: [f64; 3], on_ground: bool) -> bool {
    on_ground
        && [
            at[0].floor() as i32,
            at[1].floor() as i32,
            at[2].floor() as i32,
        ] == pos
        && at[1] >= f64::from(pos[1]) + 0.75 - 0.01
}

/// `SawBlockEntity` per tick: `remainingTime -= clamp(|speed| / 24, 1, 128)`.
pub(super) fn processing_speed(speed: f32) -> f32 {
    (speed.abs() / 24.0).clamp(1.0, 128.0)
}

/// `BlockBreakingKineticBlockEntity`: the next progress (out of 10) and the
/// ticks to wait before the one after, for a block of `hardness` at `speed`.
pub(super) fn break_step(progress: i32, speed: f32, hardness: f32) -> (i32, i32) {
    let breaking = speed.abs() / 100.0;
    let step = ((breaking / hardness.max(0.0001)) as i32).clamp(1, (10 - progress).max(1));
    (progress + step, (hardness / breaking.max(0.0001)) as i32)
}

/// SawRenderer's blade: its partial by the turning, and its pose in the
/// block (`partialFacing`, built facing south; a vertical blade along x is
/// turned a quarter).
pub(super) fn blade(facing: Dir, along_first: bool, speed: f32) -> (&'static str, glam::Mat4) {
    use glam::{Mat4, Vec3};
    let horizontal = facing.axis() != Axis::Y;
    let model = match (horizontal, speed.total_cmp(&0.0)) {
        (true, std::cmp::Ordering::Greater) => {
            "create:block/mechanical_saw/blade_horizontal_active"
        }
        (true, std::cmp::Ordering::Less) => "create:block/mechanical_saw/blade_horizontal_reversed",
        (true, _) => "create:block/mechanical_saw/blade_horizontal_inactive",
        (false, std::cmp::Ordering::Greater) => "create:block/mechanical_saw/blade_vertical_active",
        (false, std::cmp::Ordering::Less) => "create:block/mechanical_saw/blade_vertical_reversed",
        (false, _) => "create:block/mechanical_saw/blade_vertical_inactive",
    };
    // AngleHelper.horizontalAngle and verticalAngle.
    let y = match facing {
        Dir::South | Dir::Up | Dir::Down => 0.0,
        Dir::West => -90.0,
        Dir::North => 180.0,
        Dir::East => -270.0,
    };
    let x = match facing {
        Dir::Up => -90.0,
        Dir::Down => 90.0,
        _ => 0.0,
    };
    let c = Vec3::splat(0.5);
    let about = |m: Mat4| Mat4::from_translation(c) * m * Mat4::from_translation(-c);
    let mut pose = about(
        Mat4::from_rotation_y(f32::to_radians(y)) * Mat4::from_rotation_x(f32::to_radians(x)),
    );
    if !horizontal && along_first {
        pose *= about(Mat4::from_rotation_y(std::f32::consts::FRAC_PI_2));
    }
    (model, pose)
}

/// Create's runtime cutting recipes for every wood (`stripAndMakePlanks`):
/// a log or wood is stripped, a stripped one makes six planks.
fn wood_cut(item: &str) -> Option<(String, u32)> {
    let (ns, path) = item.split_once(':')?;
    if let Some(rest) = path.strip_prefix("stripped_") {
        let wood = ["_log", "_wood", "_stem", "_hyphae", "_block"]
            .iter()
            .find_map(|end| rest.strip_suffix(end))?;
        let count = if wood == "bamboo" { 3 } else { 6 };
        return Some((format!("{ns}:{wood}_planks"), count));
    }
    ["_log", "_wood", "_stem", "_hyphae"]
        .iter()
        .any(|end| path.ends_with(end))
        .then(|| (format!("{ns}:stripped_{path}"), 1))
        .or_else(|| (path == "bamboo_block").then(|| (format!("{ns}:stripped_bamboo_block"), 1)))
}

/// What TreeCutter fells: logs, stems and woods.
fn is_log(name: &str) -> bool {
    let path = name.split_once(':').map_or(name, |(_, p)| p);
    path.ends_with("_log")
        || path.ends_with("_stem")
        || path.ends_with("_wood")
        || path.ends_with("_hyphae")
}

/// The logs joined to the one cut at `cut` (by faces and edges, never below
/// it), as TreeCutter's search, nearest first.
fn tree(cut: [i32; 3], log: impl Fn([i32; 3]) -> bool) -> Vec<[i32; 3]> {
    let mut seen = HashSet::from([cut]);
    let mut queue = VecDeque::from([cut]);
    let mut out = Vec::new();
    while let Some(p) = queue.pop_front() {
        for dx in -1..=1 {
            for dy in 0..=1 {
                for dz in -1..=1 {
                    let q = [p[0] + dx, p[1] + dy, p[2] + dz];
                    if q[1] < cut[1] || !seen.insert(q) || !log(q) {
                        continue;
                    }
                    out.push(q);
                    if out.len() >= MOST_LOGS {
                        return out;
                    }
                    queue.push_back(q);
                }
            }
        }
    }
    out
}

/// A sideways saw's work on the block in front.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) struct Breaker {
    target: [i32; 3],
    progress: i32,
    wait: i32,
}

impl super::super::Game<'_> {
    /// The cut a stack gets: Create's cutting, else a stonecutter's first
    /// recipe, and how long it takes. Every item of the stack is cut.
    fn tachyon_cut(&mut self, stack: &ItemStack) -> Option<(f32, Vec<ItemStack>)> {
        let create = self.tachyon_create_recipes();
        if let Some(recipe) = create.cutting.iter().find(|r| r.input == *stack.item) {
            let mut made = Vec::new();
            for o in &recipe.outputs {
                let mut count = 0;
                for _ in 0..stack.count {
                    count += if o.chance >= 1.0 {
                        o.count
                    } else {
                        (0..o.count)
                            .filter(|_| self.world.random.next_float() <= o.chance)
                            .count() as u32
                    };
                }
                if count > 0 {
                    made.push(ItemStack::new(&o.item, count));
                }
            }
            return Some((recipe.time, made));
        }
        if let Some((item, count)) =
            wood_cut(&stack.item).filter(|(item, _)| self.res.items().get(item).is_some())
        {
            return Some((50.0, vec![ItemStack::new(&item, count * stack.count)]));
        }
        let stone = self.stations().recipes.stonecutter(stack);
        let first = stone.first()?;
        let mut result = first.result.clone();
        result.count *= stack.count;
        Some((50.0, vec![result]))
    }

    /// The saws turning: an upward one cuts what lies on it, a sideways one
    /// breaks what is in front.
    pub(super) fn tachyon_saws(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let saws: Vec<([i32; 3], Dir, bool, f32)> = super::OWNERS.with_borrow(|owners| {
            let Some(o) = owners.get(dimension) else {
                return Vec::new();
            };
            let mut saws: Vec<_> = (o.index.parts.iter())
                .filter_map(|(p, part)| {
                    let Part::Saw {
                        facing,
                        along_first,
                    } = *part
                    else {
                        return None;
                    };
                    let speed = o.shown.speeds.get(p).map_or(0.0, |s| s[0]);
                    (speed != 0.0).then_some((*p, facing, along_first, speed))
                })
                .collect();
            saws.sort_by_key(|s| s.0);
            saws
        });
        let mut seen: HashSet<u32> = HashSet::new();
        let mut kept: HashSet<[i32; 3]> = HashSet::new();
        for (pos, facing, along_first, speed) in saws {
            if facing == Dir::Up {
                self.tachyon_saw_cut(pos, along_first, speed, &mut seen);
            } else if facing != Dir::Down {
                kept.insert(pos);
                self.tachyon_saw_break(pos, facing, speed);
            }
        }
        super::OWNERS.with_borrow_mut(|owners| {
            if let Some(o) = owners.get_mut(dimension) {
                o.saw_items.retain(|id, _| seen.contains(id));
                o.saw_breakers.retain(|p, _| kept.contains(p));
            }
        });
    }

    fn tachyon_saw_cut(
        &mut self,
        pos: [i32; 3],
        along_first: bool,
        speed: f32,
        seen: &mut HashSet<u32>,
    ) {
        let dimension = self.native_dimension();
        let resting: Vec<(u32, ItemStack)> = (self.dropped.iter())
            .filter(|d| d.health > 0 && d.stack.count > 0)
            .filter(|d| landed(pos, [d.position.x, d.position.y, d.position.z], d.on_ground))
            .map(|d| (d.id, d.stack.clone()))
            .collect();
        for (id, stack) in resting {
            seen.insert(id);
            let left = super::OWNERS.with_borrow(|owners| {
                owners
                    .get(dimension)
                    .and_then(|o| o.saw_items.get(&id).copied())
            });
            let left = match left {
                Some(left) => left - processing_speed(speed),
                None => match self.tachyon_cut(&stack) {
                    // The first tick only starts it, as Create's `start`.
                    Some((time, _)) => time,
                    None => continue,
                },
            };
            if left > 0.0 {
                super::OWNERS.with_borrow_mut(|owners| {
                    if let Some(o) = owners.get_mut(dimension) {
                        o.saw_items.insert(id, left);
                    }
                });
                continue;
            }
            let Some((_, made)) = self.tachyon_cut(&stack) else {
                continue;
            };
            let Some(i) = self.dropped.iter().position(|d| d.id == id) else {
                continue;
            };
            self.dropped.remove(i);
            let (at, motion) = ejection(pos, movement(along_first, speed));
            for s in self.tachyon_split(made) {
                self.spawn_item(s, at, motion, 10);
            }
        }
    }

    fn tachyon_saw_break(&mut self, pos: [i32; 3], facing: Dir, speed: f32) {
        let dimension = self.native_dimension();
        let o = facing.offset();
        let target = [pos[0] + o[0], pos[1] + o[1], pos[2] + o[2]];
        if !self
            .world
            .chunks
            .contains_key(&(target[0] >> 4, target[2] >> 4))
        {
            return;
        }
        let state = self.world.block(target[0], target[1], target[2]);
        let facts = self.res.facts().get(state);
        let (name, hardness) = (facts.name.to_string(), facts.hardness);
        let breakable = !facts.replaceable && hardness >= 0.0 && !name.ends_with("_sapling");
        let mut breaker = super::OWNERS.with_borrow(|owners| {
            owners
                .get(dimension)
                .and_then(|o| o.saw_breakers.get(&pos).copied())
        });
        if !breakable {
            breaker = None;
        } else {
            let mut b = breaker.filter(|b| b.target == target).unwrap_or(Breaker {
                target,
                progress: 0,
                wait: 0,
            });
            if b.wait > 0 {
                b.wait -= 1;
            } else {
                let (progress, wait) = break_step(b.progress, speed, hardness);
                b.progress = progress;
                b.wait = wait;
                let at = target.map(|c| f64::from(c) + 0.5);
                self.native_level_play_sound(&facts_hit_sound(&name), at, 4, 0.25, 1.0);
            }
            if b.progress >= 10 {
                self.tachyon_saw_fell(target, is_log(&name));
                breaker = None;
            } else {
                breaker = Some(b);
            }
        }
        super::OWNERS.with_borrow_mut(|owners| {
            if let Some(o) = owners.get_mut(dimension) {
                match breaker {
                    Some(b) => o.saw_breakers.insert(pos, b),
                    None => o.saw_breakers.remove(&pos),
                };
            }
        });
    }

    /// Breaks the block cut, with its drops, and the logs above it.
    fn tachyon_saw_fell(&mut self, cut: [i32; 3], log: bool) {
        let mut all = vec![cut];
        if log {
            let facts = self.res.facts();
            all.extend(tree(cut, |p| {
                self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4))
                    && is_log(&facts.get(self.world.block(p[0], p[1], p[2])).name)
            }));
        }
        for p in all {
            let at = horizon_core::position::BlockPos::new(p[0], p[1], p[2]);
            self.with_level(|b, level, ticks| b.destroy_block(level, ticks, at, true));
        }
        let _ = self.process_level_events();
    }
}

/// The sound of hitting a block: its sound type's, guessed from its name.
fn facts_hit_sound(name: &str) -> String {
    if is_log(name) || name.contains("planks") {
        "minecraft:block.wood.hit".to_owned()
    } else {
        "minecraft:block.stone.hit".to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_blade_faces_away_from_the_look_and_its_shaft_across() {
        // Looking down: the blade up, the shaft along the player's way.
        assert_eq!(placement(0.0, 90.0), (Dir::Up, false, true));
        assert_eq!(placement(90.0, 90.0), (Dir::Up, true, false));
        // Looking north (yaw 180), level: the blade faces south.
        assert_eq!(placement(180.0, 0.0).0, Dir::South);
        assert!(placement(180.0, 0.0).1);
        assert_eq!(placement(-90.0, 0.0).0, Dir::West);
        assert!(!placement(-90.0, 0.0).1);
    }

    #[test]
    fn an_upward_saw_pushes_with_its_turning_and_cuts_faster_with_speed() {
        assert_eq!(movement(false, 32.0), Dir::East);
        assert_eq!(movement(true, -32.0), Dir::South);
        assert_eq!(processing_speed(8.0), 1.0);
        assert_eq!(processing_speed(96.0), 4.0);
        assert_eq!(processing_speed(-10000.0), 128.0);
        assert!(landed([0, 64, 0], [0.5, 64.75, 0.5], true));
        assert!(!landed([0, 64, 0], [0.5, 64.75, 0.5], false));
        assert!(!landed([0, 64, 0], [1.5, 64.75, 0.5], true));
        let (at, motion) = ejection([0, 64, 0], Dir::West);
        assert!(at[0] < 0.0 && motion[0] < 0.0 && at[1] == 65.0);
    }

    #[test]
    fn the_blade_is_drawn_by_its_turning_and_faces_out() {
        use glam::Vec3;
        let (model, pose) = blade(Dir::Up, false, 16.0);
        assert_eq!(model, "create:block/mechanical_saw/blade_vertical_active");
        // The model's south side ends up on top.
        let p = pose.transform_point3(Vec3::new(0.5, 0.5, 1.0));
        assert!((p - Vec3::new(0.5, 1.0, 0.5)).length() < 1e-5, "{p}");
        let (model, pose) = blade(Dir::East, false, -1.0);
        assert_eq!(
            model,
            "create:block/mechanical_saw/blade_horizontal_reversed"
        );
        let p = pose.transform_point3(Vec3::new(0.5, 0.5, 1.0));
        assert!((p - Vec3::new(1.0, 0.5, 0.5)).length() < 1e-5, "{p}");
        assert_eq!(
            blade(Dir::North, true, 0.0).0,
            "create:block/mechanical_saw/blade_horizontal_inactive"
        );
    }

    #[test]
    fn wood_is_stripped_then_cut_into_planks() {
        assert_eq!(
            wood_cut("minecraft:oak_log"),
            Some(("minecraft:stripped_oak_log".into(), 1))
        );
        assert_eq!(
            wood_cut("minecraft:stripped_oak_log"),
            Some(("minecraft:oak_planks".into(), 6))
        );
        assert_eq!(
            wood_cut("minecraft:stripped_crimson_hyphae"),
            Some(("minecraft:crimson_planks".into(), 6))
        );
        assert_eq!(
            wood_cut("minecraft:bamboo_block"),
            Some(("minecraft:stripped_bamboo_block".into(), 1))
        );
        assert_eq!(
            wood_cut("minecraft:stripped_bamboo_block"),
            Some(("minecraft:bamboo_planks".into(), 3))
        );
        assert_eq!(wood_cut("minecraft:stone"), None);
    }

    #[test]
    fn a_harder_block_takes_longer_to_break() {
        // 256 RPM on wood (2): steps of 1, a tick apart.
        assert_eq!(break_step(0, 256.0, 2.0), (1, 0));
        // On a soft block at speed, the rest at once.
        assert_eq!(break_step(7, 256.0, 0.1), (10, 0));
        // Stone (1.5) at 32 RPM: 4 ticks between steps.
        assert_eq!(break_step(0, 32.0, 1.5), (1, 4));
    }

    #[test]
    fn a_cut_log_fells_the_logs_joined_above_it() {
        let trunk = |p: [i32; 3]| p[0] == 0 && p[2] == 0 && (0..5).contains(&p[1]);
        let branch = |p: [i32; 3]| trunk(p) || p == [1, 4, 0] || p == [5, 5, 5] || p == [0, -1, 0];
        let felled = tree([0, 0, 0], branch);
        assert_eq!(felled.len(), 5);
        assert!(
            felled.contains(&[1, 4, 0])
                && !felled.contains(&[5, 5, 5])
                && !felled.contains(&[0, -1, 0])
        );
        assert!(
            is_log("minecraft:oak_log")
                && is_log("minecraft:crimson_stem")
                && !is_log("minecraft:oak_planks")
        );
    }
}
