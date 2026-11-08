//! Create's mechanical drill (DrillBlock, DrillBlockEntity, DrillRenderer,
//! BlockBreakingKineticBlockEntity; MIT, THIRD_PARTY_NOTICES.md) on
//! create:mechanical_drill. Turning, it breaks the block its head faces,
//! as a sideways saw does (`saw.rs`, `tachyon_saw_break`): progress by
//! `speed / 100` against the block's hardness, its drops where it stood.
//! It takes 4 stress units per RPM (AllBlocks), its shaft out of the back.
//!
//! Not as Create: no harm to who stands in front (`DrillBlock.entityInside`),
//! no crack on the block it breaks, no cobblestone generator shortcut
//! (`DrillBlockEntity.optimiseCobbleGen`: the drops are dropped, not put
//! straight in the belt or hopper below), and no drilling on contraptions.
use super::kinetic::Dir;

pub(super) const DRILL: &str = "create:mechanical_drill";

/// `BlockBreakingKineticBlockEntity.isBreakable`: not air, not a fluid, not
/// unbreakable (hardness -1).
pub(super) fn breakable(name: &str, hardness: f32) -> bool {
    let air = matches!(name, "minecraft:air" | "minecraft:cave_air" | "minecraft:void_air");
    let fluid = matches!(name, "minecraft:water" | "minecraft:lava" | "minecraft:bubble_column");
    !air && !fluid && hardness >= 0.0
}

/// `DirectionalKineticBlock.getStateForPlacement`: its back to the one
/// shaft beside it (`preferred`; none when shafts come from two axes),
/// else facing the player (`look` is where they look), or away from them
/// when sneaking.
pub(super) fn facing(preferred: Option<Dir>, look: Dir, sneaking: bool) -> Dir {
    match preferred {
        Some(shaft) if !sneaking => shaft.opposite(),
        _ if sneaking => look,
        _ => look.opposite(),
    }
}

/// `BlockPlaceContext.getNearestLookingDirection`: the axis the look is
/// most along.
pub(super) fn looking(yaw: f32, pitch: f32) -> Dir {
    let (y, p) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let look = [-y.sin() * p.cos(), -p.sin(), y.cos() * p.cos()];
    let axis = (0..3)
        .max_by(|a, b| look[*a].abs().total_cmp(&look[*b].abs()))
        .unwrap_or(1);
    Dir::ALL
        .into_iter()
        .find(|d| d.offset()[axis] != 0 && d.offset()[axis] == look[axis].signum() as i32)
        .unwrap_or(Dir::Down)
}

impl super::super::GameLevel<'_> {
    /// DrillBlock's placement (`facing`): the shafts beside it are the
    /// engine's parts with a shaft toward it.
    pub(in crate::game) fn tachyon_drill_place(
        &self,
        state: horizon_world::registry::BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
    ) {
        if answer.is_some() || self.native_state_block(state) != Some(DRILL) {
            return;
        }
        let p = context.pos;
        let mut preferred: Option<Dir> = None;
        for d in Dir::ALL {
            let o = d.offset();
            let there = self.world.block(p.x + o[0], p.y + o[1], p.z + o[2]);
            let Some(s) = self.registry.state(there) else { continue };
            let part = super::part_of(&s.name, |k| s.properties.get(k).map(String::as_str));
            if !part.is_some_and(|part| part.shaft_toward(d.opposite())) {
                continue;
            }
            if preferred.is_some_and(|f| f.axis() != d.axis()) {
                preferred = None;
                break;
            }
            preferred = Some(d);
        }
        let facing = facing(preferred, looking(context.yaw, context.pitch), context.sneaking);
        if let Some(placed) = self.native_state_with_property(state, "facing", facing.name()) {
            *answer = Some(Some(placed));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_drill_breaks_what_is_solid_or_soft_but_not_air_fluids_or_bedrock() {
        assert!(breakable("minecraft:stone", 1.5));
        assert!(breakable("minecraft:short_grass", 0.0));
        assert!(!breakable("minecraft:air", 0.0));
        assert!(!breakable("minecraft:water", 100.0));
        assert!(!breakable("minecraft:bedrock", -1.0));
    }

    #[test]
    fn a_drill_faces_the_player_or_away_from_the_shaft_beside_it() {
        // Looking north at where it goes: it faces south, at the player.
        assert_eq!(looking(180.0, 0.0), Dir::North);
        assert_eq!(facing(None, Dir::North, false), Dir::South);
        assert_eq!(facing(None, Dir::North, true), Dir::North);
        // A shaft to the west: its back to it, its head east.
        assert_eq!(facing(Some(Dir::West), Dir::Down, false), Dir::East);
        assert_eq!(looking(0.0, 90.0), Dir::Down);
    }
}
