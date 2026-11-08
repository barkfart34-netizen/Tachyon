//! Create's belts (BeltConnectorItem, BeltBlock, BeltRenderer, BeltPart; MIT,
//! THIRD_PARTY_NOTICES.md) on create:belt. A belt connector used on a shaft, then on a second
//! shaft along the same horizontal axis and level (`canConnect`: the line runs across the shafts,
//! under `maxBeltLength` 20 apart, nothing solid between), lays a belt from the first to the
//! second: `start`, `middle`s, `end`, facing from the first to the second. A sneaking use, or a
//! second shaft that doesn't fit, lets the first go. The engine runs it as Kinetica's belt
//! (`belt.rs`: it turns with the shafts beside its pulleys and carries what lies on it), and draws
//! Create's belt partials with a turning pulley at each end. Breaking a segment breaks the whole
//! belt (`BeltBlock.onRemove`): its ends go back to shafts, the rest to air.
//!
//! Not as Create: only level belts (no slopes, vertical or sideways ones), the belt's texture
//! doesn't scroll, no casings, dyes or tunnels on it.
use super::kinetic::{Axis, Dir};
use horizon_client::items::ItemStack;

/// `maxBeltLength`: a second shaft must be closer than this.
pub(super) const MAX_LENGTH: i32 = 20;
pub(super) const BELT: &str = "create:belt";
pub(super) const CONNECTOR: &str = "create:belt_connector";

/// A level belt's blocks from shaft `first` to shaft `second` (both along `axis`): each position
/// with its `part`, and the belt's facing. Err says why it can't be laid; `free` says whether a
/// place between may take it.
pub(super) fn plan(
    first: [i32; 3],
    second: [i32; 3],
    axis: Axis,
    second_axis: Axis,
    free: impl Fn([i32; 3]) -> bool,
) -> Result<(Dir, Vec<([i32; 3], &'static str)>), &'static str> {
    if axis == Axis::Y || second_axis != axis {
        return Err("both shafts must lie the same way, level");
    }
    let d = [second[0] - first[0], second[1] - first[1], second[2] - first[2]];
    if d[1] != 0 {
        return Err("only level belts are laid for now");
    }
    let along = if axis == Axis::X { 2 } else { 0 };
    if d[axis_index(axis)] != 0 || d[along] == 0 {
        return Err("the second shaft must be in line, across the shafts");
    }
    if d[along].abs() >= MAX_LENGTH {
        return Err("too long: at most 20 blocks");
    }
    let facing = match (along, d[along] > 0) {
        (0, true) => Dir::East,
        (0, false) => Dir::West,
        (_, true) => Dir::South,
        (_, false) => Dir::North,
    };
    let step = facing.offset();
    let n = d[along].abs();
    let mut out = Vec::new();
    for i in 0..=n {
        let p = [first[0] + step[0] * i, first[1], first[2] + step[2] * i];
        let part = if i == 0 {
            "start"
        } else if i == n {
            "end"
        } else {
            if !free(p) {
                return Err("something is in the way");
            }
            "middle"
        };
        out.push((p, part));
    }
    Ok((facing, out))
}

fn axis_index(axis: Axis) -> usize {
    match axis {
        Axis::X => 0,
        Axis::Y => 1,
        Axis::Z => 2,
    }
}

/// BeltRenderer's partials for a part, top and bottom.
pub(super) fn partials(part: &str) -> [&'static str; 2] {
    match part {
        "start" => ["create:block/belt/start", "create:block/belt/start_bottom"],
        "end" => ["create:block/belt/end", "create:block/belt/end_bottom"],
        _ => ["create:block/belt/middle", "create:block/belt/middle_bottom"],
    }
}

/// Whether a belt part has a pulley (a shaft) in it.
pub(super) fn pulley(part: &str) -> bool {
    matches!(part, "start" | "end" | "pulley")
}

thread_local! {
    /// The first shaft a belt connector was used on.
    static FIRST: std::cell::Cell<Option<[i32; 3]>> = const { std::cell::Cell::new(None) };
}

impl super::super::Game<'_> {
    /// A belt connector used on a shaft: the first chosen, or the belt laid to the second.
    pub(in crate::game) fn tachyon_belt_connector_use(&mut self, stack: &ItemStack, pos: [i32; 3], claimed: &mut bool) {
        if *claimed || &*stack.item != CONNECTOR || !self.may_build() {
            return;
        }
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        let shaft_axis = |g: &Self, p: [i32; 3]| -> Option<Axis> {
            (g.native_block(p) == "create:shaft")
                .then(|| g.native_block_property(p, "axis"))
                .flatten()
                .and_then(|a| Axis::parse(&a))
        };
        let Some(axis) = shaft_axis(self, pos) else {
            if sneaking {
                FIRST.set(None);
            }
            return;
        };
        *claimed = true;
        let first = FIRST.get();
        if sneaking || first.is_none() || first == Some(pos) {
            FIRST.set((!sneaking).then_some(pos));
            if !sneaking {
                self.native_chat("Belt: now use it on the second shaft");
            }
            return;
        }
        FIRST.set(None);
        let first = first.unwrap_or(pos);
        let Some(first_axis) = shaft_axis(self, first) else {
            self.native_chat("Belt: the first shaft is gone");
            return;
        };
        let free = |p: [i32; 3]| {
            let state = self.world.block(p[0], p[1], p[2]);
            self.res.facts().get(state).replaceable
                || (self.native_block(p) == "create:shaft" && self.native_block_property(p, "axis").as_deref() == Some(axis_name(first_axis)))
        };
        let (facing, blocks) = match plan(first, pos, first_axis, axis, free) {
            Ok(plan) => plan,
            Err(why) => {
                self.native_chat(&format!("Belt: {why}"));
                return;
            }
        };
        let registry = self.pack.assets.registry;
        let dimension = self.native_dimension();
        for (p, part) in blocks {
            let Some(mut state) = registry.default_state(BELT).and_then(|s| registry.state(s)).cloned() else {
                return;
            };
            state.properties.insert("facing".into(), facing.name().into());
            state.properties.insert("part".into(), part.into());
            state.properties.insert("slope".into(), "horizontal".into());
            let Ok(id) = registry.resolve(&state) else { return };
            self.native_set_block(dimension, p, id, 3);
        }
        if !self.creative {
            self.tachyon_spend_held(1);
        }
    }

    /// Belts broken in part break whole: from beside each `removed` place, along the belt.
    pub(super) fn tachyon_belts_broken(&mut self, removed: &[[i32; 3]]) {
        let dimension = self.native_dimension();
        for p in removed {
            if self.native_block(*p) == BELT {
                continue;
            }
            for d in [Dir::North, Dir::South, Dir::West, Dir::East] {
                let o = d.offset();
                let mut q = [p[0] + o[0], p[1], p[2] + o[2]];
                for _ in 0..MAX_LENGTH {
                    if self.native_block(q) != BELT {
                        break;
                    }
                    let facing = self.native_block_property(q, "facing").and_then(|f| Dir::parse(&f));
                    if facing.map(Dir::axis) != Some(d.axis()) {
                        break;
                    }
                    let part = self.native_block_property(q, "part").unwrap_or_default();
                    let registry = self.pack.assets.registry;
                    let next = if pulley(&part) {
                        let across = if d.axis() == Axis::X { "z" } else { "x" };
                        registry.default_state("create:shaft").and_then(|s| registry.state(s)).cloned().and_then(|mut s| {
                            s.properties.insert("axis".into(), across.into());
                            registry.resolve(&s).ok()
                        })
                    } else {
                        registry.default_state("minecraft:air")
                    };
                    if let Some(next) = next {
                        self.native_set_block(dimension, q, next, 3);
                    }
                    if part == "start" || part == "end" {
                        break;
                    }
                    q = [q[0] + o[0], q[1], q[2] + o[2]];
                }
            }
        }
    }
}

fn axis_name(a: Axis) -> &'static str {
    match a {
        Axis::X => "x",
        Axis::Y => "y",
        Axis::Z => "z",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_belt_runs_across_its_shafts_level_and_under_twenty_blocks() {
        let open = |_: [i32; 3]| true;
        let (facing, blocks) = plan([0, 0, 0], [0, 0, 4], Axis::X, Axis::X, open).unwrap();
        assert_eq!(facing, Dir::South);
        assert_eq!(blocks.len(), 5);
        assert_eq!(blocks[0], ([0, 0, 0], "start"));
        assert_eq!(blocks[2], ([0, 0, 2], "middle"));
        assert_eq!(blocks[4], ([0, 0, 4], "end"));
        let (facing, _) = plan([3, 0, 0], [0, 0, 0], Axis::Z, Axis::Z, open).unwrap();
        assert_eq!(facing, Dir::West);
        assert!(plan([0, 0, 0], [0, 0, 19], Axis::X, Axis::X, open).is_ok());
        assert!(plan([0, 0, 0], [0, 0, 20], Axis::X, Axis::X, open).is_err());
        // Along the shafts, sloped, or shafts lying different ways: no.
        assert!(plan([0, 0, 0], [4, 0, 0], Axis::X, Axis::X, open).is_err());
        assert!(plan([0, 0, 0], [0, 2, 2], Axis::X, Axis::X, open).is_err());
        assert!(plan([0, 0, 0], [0, 0, 4], Axis::X, Axis::Z, open).is_err());
        assert!(plan([0, 0, 0], [0, 0, 4], Axis::Y, Axis::Y, open).is_err());
        // Something in the way.
        assert!(plan([0, 0, 0], [0, 0, 4], Axis::X, Axis::X, |p| p != [0, 0, 2]).is_err());
        assert_eq!(partials("end")[1], "create:block/belt/end_bottom");
        assert!(pulley("start") && !pulley("middle"));
    }
}
