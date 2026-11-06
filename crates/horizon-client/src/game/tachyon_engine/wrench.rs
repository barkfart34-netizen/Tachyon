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

impl super::super::Game<'_> {
    /// A use of `stack` on the block at `pos`, `face` the clicked face.
    pub(in crate::game) fn tachyon_wrench(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
        claimed: &mut bool,
    ) {
        if *claimed || &*stack.item != WRENCH || self.block_action_restricted(pos) {
            return;
        }
        let block = self.native_block(pos);
        let Some(name) = block.strip_prefix("create:") else {
            return;
        };
        let axis_now = self.native_block_property(pos, "axis");
        let axis = axis_now.as_deref().and_then(Axis::parse);
        let kinetic = super::create_part(name, |key| match key {
            "axis" => axis_now.as_deref(),
            _ => None,
        })
        .is_some()
            || self.native_block_property(pos, "facing").is_some_and(|facing| {
                super::create_part(name, |key| (key == "facing").then_some(facing.as_str())).is_some()
            });
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        if block == water_wheel::LARGE && !sneaking {
            return;
        }
        match action(kinetic, sneaking, axis, face) {
            Action::Nothing => return,
            Action::Rotate(turned) => {
                let value = match turned {
                    Axis::X => "x",
                    Axis::Y => "y",
                    Axis::Z => "z",
                };
                self.native_set_block_property(pos, "axis", value);
            }
            Action::Remove => {
                // Into the inventory (nothing in creative); a full one drops it.
                let drop = !self.creative && self.native_give_items(&block, 1) == 0;
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
    }
}
