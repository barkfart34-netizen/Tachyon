//! Create's brackets (BracketBlockItem, BracketedBlockEntityBehaviour,
//! IWrenchableWithBracket; MIT, THIRD_PARTY_NOTICES.md) after Kinetics for
//! Horizon's bracket (sakurartro, MIT). A bracket used on a bare shaft or
//! cogwheel goes on the clicked face; here it is a `bracket` state of the
//! block (`create_port` declares it, with the models and the drop), not a
//! block entity's. A wrench takes it off before it turns the block.
use super::kinetic::Axis;
use horizon_client::items::ItemStack;

const ITEMS: [(&str, &str); 2] = [
    ("create:wooden_bracket", "wooden"),
    ("create:metal_bracket", "metal"),
];
const BRACKETED: [&str; 3] = ["create:shaft", "create:cogwheel", "create:large_cogwheel"];

/// The face's direction name, as the `bracket` state spells it.
fn facing_name(face: [i32; 3]) -> Option<&'static str> {
    Some(match face {
        [0, -1, 0] => "down",
        [0, 1, 0] => "up",
        [0, 0, -1] => "north",
        [0, 0, 1] => "south",
        [-1, 0, 0] => "west",
        [1, 0, 0] => "east",
        _ => return None,
    })
}

/// The state value for a `material` bracket put on `face` of a part on
/// `axis`; none on a face the axis runs through.
fn value(material: &str, axis: Axis, face: [i32; 3]) -> Option<String> {
    let along = match axis {
        Axis::X => face[0] != 0,
        Axis::Y => face[1] != 0,
        Axis::Z => face[2] != 0,
    };
    (!along)
        .then(|| facing_name(face).map(|f| format!("{material}_{f}")))
        .flatten()
}

/// The item of a `bracket` state value.
fn item_of(value: &str) -> Option<&'static str> {
    let material = value.split('_').next()?;
    ITEMS
        .iter()
        .find(|(_, m)| *m == material)
        .map(|(item, _)| *item)
}

impl super::super::Game<'_> {
    /// A bracket used on a shaft or cogwheel goes on (the old one back to
    /// the player).
    pub(in crate::game) fn tachyon_bracket_use(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        face: [i32; 3],
        claimed: &mut bool,
    ) {
        let Some((item, material)) = ITEMS.iter().find(|(item, _)| *item == &*stack.item) else {
            return;
        };
        if *claimed || !self.may_build() || self.block_action_restricted(pos) {
            return;
        }
        if !BRACKETED.contains(&self.native_block(pos).as_str()) {
            return;
        }
        let axis = self
            .native_block_property(pos, "axis")
            .as_deref()
            .and_then(Axis::parse);
        let Some(next) = axis.and_then(|axis| value(material, axis, face)) else {
            return;
        };
        let now = self.native_block_property(pos, "bracket");
        *claimed = true;
        if now.as_deref() == Some(next.as_str()) {
            return;
        }
        self.native_set_block_property(pos, "bracket", &next);
        if !self.creative {
            self.native_take_items(item, 1);
            if let Some(old) = now.as_deref().and_then(item_of) {
                self.native_give_items(old, 1);
            }
        }
    }

    /// `IWrenchableWithBracket.tryRemoveBracket`: a bracket at `pos` comes
    /// off into the inventory. Whether there was one.
    pub(super) fn tachyon_take_bracket(&mut self, pos: [i32; 3]) -> bool {
        let Some(old) = self
            .native_block_property(pos, "bracket")
            .filter(|v| v != "none")
        else {
            return false;
        };
        self.native_set_block_property(pos, "bracket", "none");
        if !self.creative
            && let Some(item) = item_of(&old)
        {
            self.native_give_items(item, 1);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bracket_goes_on_a_side_face_only() {
        assert_eq!(
            value("metal", Axis::Y, [1, 0, 0]).as_deref(),
            Some("metal_east")
        );
        assert_eq!(
            value("wooden", Axis::X, [0, -1, 0]).as_deref(),
            Some("wooden_down")
        );
        assert_eq!(value("metal", Axis::Y, [0, 1, 0]), None);
        assert_eq!(value("metal", Axis::Z, [0, 0, -1]), None);
        assert_eq!(value("metal", Axis::Z, [1, 1, 0]), None);
        assert_eq!(item_of("wooden_up"), Some("create:wooden_bracket"));
        assert_eq!(item_of("metal_east"), Some("create:metal_bracket"));
        assert_eq!(item_of("none"), None);
    }
}
