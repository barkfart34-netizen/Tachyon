//! Create's wrench (WrenchItem, IWrenchable; MIT, THIRD_PARTY_NOTICES.md)
//! on the engine's blocks: a use turns a part's axis a quarter about the
//! clicked face; sneaking picks the part up into the inventory. After
//! Kinetics for Horizon's wrench (sakurartro, MIT). Parts set by a facing
//! (a motor, a crank, a wheel) aren't turned, and a large water wheel only
//! comes away: its body is built round its axis.
use super::kinetic::Axis;
use super::water_wheel;
use horizon_client::items::ItemStack;

pub(super) const WRENCH: &str = "create:wrench";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Action {
    /// Set the part's `axis` to this one.
    Rotate(Axis),
    /// Pick the part up.
    Remove,
    Nothing,
}

/// The axis of a face's normal.
fn face_axis(face: [i32; 3]) -> Option<Axis> {
    match face {
        [x, 0, 0] if x.abs() == 1 => Some(Axis::X),
        [0, y, 0] if y.abs() == 1 => Some(Axis::Y),
        [0, 0, z] if z.abs() == 1 => Some(Axis::Z),
        _ => None,
    }
}

/// A quarter turn of `axis` about `about`: the other two axes swap.
fn rotated(axis: Axis, about: Axis) -> Axis {
    if axis == about {
        return axis;
    }
    [Axis::X, Axis::Y, Axis::Z]
        .into_iter()
        .find(|a| *a != axis && *a != about)
        .unwrap_or(axis)
}

/// What a wrench does to a part (`kinetic`) of `axis`, clicked on `face`.
pub(super) fn action(kinetic: bool, sneaking: bool, axis: Option<Axis>, face: [i32; 3]) -> Action {
    if !kinetic {
        return Action::Nothing;
    }
    if sneaking {
        return Action::Remove;
    }
    let (Some(axis), Some(about)) = (axis, face_axis(face)) else {
        return Action::Nothing;
    };
    match rotated(axis, about) {
        turned if turned == axis => Action::Nothing,
        turned => Action::Rotate(turned),
    }
}

pub(super) enum WrenchPlan {
    RemoveBracket(horizon_world::registry::BlockStateId),
    Rotate {
        value: &'static str,
        state: horizon_world::registry::BlockStateId,
    },
    Remove {
        block: String,
        replacement: horizon_world::registry::BlockStateId,
    },
}

impl super::super::Game<'_> {
    pub(super) fn tachyon_wrench_plan(
        &self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
    ) -> Option<WrenchPlan> {
        if &*stack.item != WRENCH || self.block_action_restricted(pos) {
            return None;
        }
        let block = self.native_block(pos);
        let name = block.strip_prefix("create:")?;
        let cur = self.world.block(pos[0], pos[1], pos[2]);
        let state_desc = self.pack.assets.registry.state(cur)?;
        let axis_now = state_desc.properties.get("axis").map(String::as_str);
        let axis = axis_now.and_then(Axis::parse);
        let kinetic = super::create_part(name, |key| {
            state_desc.properties.get(key).map(String::as_str)
        })
        .is_some();
        // IWrenchableWithBracket: a bracket comes off first.
        if kinetic
            && state_desc
                .properties
                .get("bracket")
                .is_some_and(|b| b != "none")
        {
            let mut parsed = state_desc.clone();
            parsed.properties.insert("bracket".into(), "none".into());
            let state = self.pack.assets.registry.id(&parsed)?;
            return Some(WrenchPlan::RemoveBracket(state));
        }
        let sneaking = self.tachyon_sneaking();
        if block == water_wheel::LARGE && !sneaking {
            return None;
        }
        match action(kinetic, sneaking, axis, face) {
            Action::Nothing => None,
            Action::Rotate(turned) => {
                let value = match turned {
                    Axis::X => "x",
                    Axis::Y => "y",
                    Axis::Z => "z",
                };
                let cur = self.world.block(pos[0], pos[1], pos[2]);
                let mut parsed = self.pack.assets.registry.state(cur)?.clone();
                parsed.properties.insert("axis".into(), value.into());
                let state = self.pack.assets.registry.id(&parsed)?;
                Some(WrenchPlan::Rotate { value, state })
            }
            Action::Remove => {
                let replacement = if self.native_block_property(pos, "waterlogged").as_deref()
                    == Some("true")
                {
                    self.pack
                        .assets
                        .registry
                        .default_state("minecraft:water")
                        .unwrap_or(self.res.facts().air)
                } else {
                    self.res.facts().air
                };
                Some(WrenchPlan::Remove { block, replacement })
            }
        }
    }

    /// A use of `stack` on the block at `pos`, `face` the clicked face.
    pub(in crate::game) fn tachyon_wrench(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
        claimed: &mut bool,
    ) {
        if *claimed {
            return;
        }
        let Some(plan) = self.tachyon_wrench_plan(stack, pos, face) else {
            return;
        };
        match plan {
            WrenchPlan::RemoveBracket(_) => {
                if !self.tachyon_take_bracket(pos) {
                    return;
                }
            }
            WrenchPlan::Rotate { value, .. } => {
                self.native_set_block_property(pos, "axis", value);
            }
            WrenchPlan::Remove { block, .. } => {
                // Into the inventory (nothing in creative); a full one drops it.
                let item = super::tachyon_pick_block_item(&block).unwrap_or(&block);
                let drop = !self.creative && self.native_give_items(item, 1) == 0;
                let at = horizon_core::position::BlockPos::new(pos[0], pos[1], pos[2]);
                self.with_level(|b, level, ticks| b.destroy_block(level, ticks, at, drop));
                let _ = self.process_level_events();
            }
        }
        *claimed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_quarter_turn_swaps_the_other_two_axes_and_its_own_end_stays() {
        assert_eq!(action(true, false, Some(Axis::Y), [1, 0, 0]), Action::Rotate(Axis::Z));
        assert_eq!(action(true, false, Some(Axis::Z), [0, 1, 0]), Action::Rotate(Axis::X));
        assert_eq!(action(true, false, Some(Axis::X), [0, 0, -1]), Action::Rotate(Axis::Y));
        assert_eq!(action(true, false, Some(Axis::Y), [0, -1, 0]), Action::Nothing);
        for a in [Axis::X, Axis::Y, Axis::Z] {
            for f in [Axis::X, Axis::Y, Axis::Z] {
                assert_eq!(rotated(rotated(a, f), f), a);
            }
        }
    }

    #[test]
    fn sneaking_picks_up_and_other_blocks_are_left_alone() {
        assert_eq!(action(true, true, Some(Axis::Y), [0, 1, 0]), Action::Remove);
        assert_eq!(action(true, true, None, [0, 1, 0]), Action::Remove);
        assert_eq!(action(false, true, None, [1, 0, 0]), Action::Nothing);
        assert_eq!(action(true, false, None, [1, 0, 0]), Action::Nothing);

        // Non-axis Create blocks (fluid valve, smart pipe, belt, deployer, chute) pass all
        // blockstate properties into `create_part` so sneaking removes them cleanly.
        assert!(
            super::super::create_part("fluid_valve", |k| match k {
                "facing" => Some("north"),
                "axis_along_first" => Some("true"),
                _ => None,
            })
            .is_some()
        );
        assert!(
            super::super::create_part("smart_fluid_pipe", |k| match k {
                "face" => Some("wall"),
                "facing" => Some("east"),
                _ => None,
            })
            .is_some()
        );
        assert!(
            super::super::create_part("belt", |k| match k {
                "facing" => Some("south"),
                "slope" => Some("horizontal"),
                _ => None,
            })
            .is_some()
        );
        assert_eq!(
            super::super::tachyon_pick_block_item("create:metal_girder_encased_shaft"),
            Some("create:metal_girder")
        );
    }
}
