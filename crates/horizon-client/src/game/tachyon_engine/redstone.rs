//! Create's analog lever and latches (AnalogLeverBlock, AnalogLeverBlockEntity, AnalogLeverRenderer,
//! PoweredLatchBlock, ToggleLatchBlock, AbstractDiodeBlock; MIT, THIRD_PARTY_NOTICES.md).
//!
//! - The analog lever gives 0 to 15, one more for each use, one less for a sneaking use; its
//!   strength strongly into the block it is on (`getDirectSignal`). Its handle is drawn turned by
//!   the strength, a quarter turn at 15.
//! - The powered latch, a diode, gives 15 out of its front once its back is powered, and stops
//!   once a side is (the last of them to change wins, as `PoweredLatchBlock.tick`).
//! - The toggle latch gives 15 or nothing, swapped each time its back is powered and by a use.
//!
//! Both latches read their back as vanilla's diodes do (`DiodeBlock.getInputSignal`): the signal
//! from the block behind them, toward them. Their strength is kept in their states (`power`,
//! `powering`), so the signal is answered from the state alone.
//!
//! Not as Create: the latches change in the tick their input does (no scheduled one-tick delay),
//! the indicator isn't tinted by the strength, the lever drops no particles.
use super::kinetic::Dir;
use horizon_core::geometry::Direction;
use horizon_core::position::BlockPos;
use horizon_world::registry::BlockStateId;

pub(super) const ANALOG_LEVER: &str = "create:analog_lever";
pub(super) const LATCH: &str = "create:powered_latch";
pub(super) const TOGGLE_LATCH: &str = "create:powered_toggle_latch";

/// `FaceAttachedHorizontalDirectionalBlock.getConnectedDirection`: the way from the block a
/// lever is on to the lever.
pub(super) fn connected(face: &str, facing: Dir) -> Dir {
    match face {
        "floor" => Dir::Up,
        "ceiling" => Dir::Down,
        _ => facing,
    }
}

/// `AnalogLeverBlockEntity.changeState`: the next strength.
pub(super) fn next_power(power: u8, sneaking: bool) -> u8 {
    if sneaking { power.saturating_sub(1) } else { (power + 1).min(15) }
}

/// `PoweredLatchBlock.tick`: `powering` after the back (`back`, was `was_back`) and sides (`side`,
/// was `was_side`) changed.
pub(super) fn latch(powering: bool, was_back: bool, back: bool, was_side: bool, side: bool) -> bool {
    let mut out = powering;
    if back != was_back {
        if back {
            out = true;
        } else if was_side {
            out = false;
        }
    }
    if side != was_side {
        if side {
            out = false;
        } else if was_back {
            out = true;
        }
    }
    out
}

/// The signal one of these blocks gives toward `direction` (vanilla's: from the block asking to
/// it), `direct` for what passes through the block it touches; None for another block.
pub(super) fn signal(name: &str, property: impl Fn(&str) -> Option<String>, direction: Dir, direct: bool) -> Option<i32> {
    let facing = property("facing").and_then(|f| Dir::parse(&f));
    match name {
        ANALOG_LEVER => {
            let power = property("power").and_then(|p| p.parse::<i32>().ok()).unwrap_or(0);
            let toward = connected(&property("face").unwrap_or_default(), facing?);
            Some(if !direct || direction == toward { power } else { 0 })
        }
        LATCH | TOGGLE_LATCH => {
            let on = property("powering").as_deref() == Some("true");
            Some(if on && Some(direction) == facing { 15 } else { 0 })
        }
        _ => None,
    }
}

fn core(d: Dir) -> Direction {
    match d {
        Dir::Down => Direction::Down,
        Dir::Up => Direction::Up,
        Dir::North => Direction::North,
        Dir::South => Direction::South,
        Dir::West => Direction::West,
        Dir::East => Direction::East,
    }
}

fn of(d: Direction) -> Dir {
    match d {
        Direction::Down => Dir::Down,
        Direction::Up => Dir::Up,
        Direction::North => Dir::North,
        Direction::South => Dir::South,
        Direction::West => Dir::West,
        Direction::East => Dir::East,
    }
}

impl super::super::GameLevel<'_> {
    /// `isSignalSource` for these blocks.
    pub(in crate::game) fn tachyon_signal_source(&self, state: BlockStateId, source: &mut bool) {
        if matches!(self.native_state_block(state), Some(ANALOG_LEVER | LATCH | TOGGLE_LATCH)) {
            *source = true;
        }
    }

    /// `getSignal` and `getDirectSignal` for these blocks, `direction` vanilla's.
    pub(in crate::game) fn tachyon_block_signal(&self, state: BlockStateId, direction: Direction, direct: bool, signal: &mut i32) {
        let Some(s) = self.registry.state(state) else { return };
        if let Some(n) = self::signal(&s.name, |k| s.properties.get(k).cloned(), of(direction), direct) {
            *signal = n;
        }
    }

    /// Placing a lever (on the face clicked, facing the player's way on a floor or ceiling) or a
    /// latch (its back toward the player, as a repeater).
    pub(in crate::game) fn tachyon_redstone_place(
        &self,
        state: BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<BlockStateId>>,
    ) {
        if answer.is_some() {
            return;
        }
        let horizontal = super::hose::horizontal(context.yaw);
        let placed = match self.native_state_block(state) {
            Some(ANALOG_LEVER) => {
                let (face, facing) = match of(context.face) {
                    Dir::Up => ("floor", horizontal),
                    Dir::Down => ("ceiling", horizontal),
                    d => ("wall", d),
                };
                self.native_state_with_property(state, "face", face)
                    .and_then(|s| self.native_state_with_property(s, "facing", facing.name()))
                    .and_then(|s| self.native_state_with_property(s, "power", "0"))
            }
            Some(LATCH | TOGGLE_LATCH) => self.native_state_with_property(state, "facing", horizontal.opposite().name()),
            _ => return,
        };
        if let Some(placed) = placed {
            *answer = Some(Some(placed));
        }
    }
}

impl super::super::Game<'_> {
    /// Sets the properties `values` of the block at `p` and tells its neighbours its signal changed.
    fn tachyon_redstone_set(&mut self, p: [i32; 3], values: &[(&str, String)]) {
        let registry = self.pack.assets.registry;
        let state = self.world.block(p[0], p[1], p[2]);
        let Some(mut s) = registry.state(state).cloned() else { return };
        for (k, v) in values {
            s.properties.insert((*k).into(), v.clone());
        }
        let Ok(id) = registry.resolve(&s) else { return };
        if id == state {
            return;
        }
        let dimension = self.native_dimension();
        if !self.native_set_block(dimension, p, id, 3) {
            return;
        }
        let at = BlockPos::new(p[0], p[1], p[2]);
        self.with_level(|b, level, ticks| {
            let block = b.block(id);
            b.update_neighbors_at(level, ticks, at, block);
            for d in Dir::ALL {
                let o = d.offset();
                b.update_neighbors_at(level, ticks, at.offset(o[0], o[1], o[2]), block);
            }
        });
    }

    /// A use of a lever or a latch.
    pub(super) fn tachyon_use_redstone(&mut self, pos: [i32; 3], sneaking: bool) -> bool {
        match self.native_block(pos).as_str() {
            ANALOG_LEVER => {
                let power = self.native_block_property(pos, "power").and_then(|p| p.parse().ok()).unwrap_or(0);
                self.tachyon_redstone_set(pos, &[("power", next_power(power, sneaking).to_string())]);
                true
            }
            LATCH | TOGGLE_LATCH if !sneaking => {
                let on = self.native_block_property(pos, "powering").as_deref() == Some("true");
                self.tachyon_redstone_set(pos, &[("powering", (!on).to_string())]);
                true
            }
            _ => false,
        }
    }

    /// The owner's latches a tick: each reads its back and sides.
    pub(super) fn tachyon_latches(&mut self) {
        use super::kinetic::Part;
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let latches: Vec<([i32; 3], Dir, bool)> = super::OWNERS.with_borrow(|owners| {
            let Some(o) = owners.get(dimension) else { return Vec::new() };
            (o.index.parts.iter())
                .filter_map(|(p, part)| match *part {
                    Part::Latch(facing) => Some((*p, facing, false)),
                    Part::ToggleLatch(facing) => Some((*p, facing, true)),
                    _ => None,
                })
                .collect()
        });
        for (p, facing, toggle) in latches {
            let at = BlockPos::new(p[0], p[1], p[2]);
            let input = |d: Dir| {
                let o = d.offset();
                (at.offset(o[0], o[1], o[2]), core(d))
            };
            let (behind, back_way) = input(facing);
            let sides = [super::hose::clockwise(facing), super::hose::counter_clockwise(facing)].map(input);
            let (back, side) = self.with_level(|b, level, ticks| {
                let back = b.signal(level, ticks, behind, back_way) > 0;
                let side = sides.iter().any(|(q, d)| b.signal(level, ticks, *q, *d) > 0);
                (back, side)
            });
            let property = |k: &str| self.native_block_property(p, k).as_deref() == Some("true");
            let (was_back, was_side, powering) = (property("powered"), property("powered_side"), property("powering"));
            if toggle {
                if back != was_back {
                    let next = if back { !powering } else { powering };
                    self.tachyon_redstone_set(p, &[("powered", back.to_string()), ("powering", next.to_string())]);
                }
                continue;
            }
            if back != was_back || side != was_side {
                let next = latch(powering, was_back, back, was_side, side);
                self.tachyon_redstone_set(
                    p,
                    &[("powered", back.to_string()), ("powered_side", side.to_string()), ("powering", next.to_string())],
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_lever_counts_to_fifteen_and_powers_the_block_it_is_on() {
        assert_eq!(next_power(0, false), 1);
        assert_eq!(next_power(15, false), 15);
        assert_eq!(next_power(0, true), 0);
        assert_eq!(next_power(7, true), 6);
        let lever = |k: &str| match k {
            "face" => Some("floor".to_owned()),
            "facing" => Some("north".to_owned()),
            "power" => Some("9".to_owned()),
            _ => None,
        };
        assert_eq!(signal(ANALOG_LEVER, lever, Dir::East, false), Some(9));
        assert_eq!(signal(ANALOG_LEVER, lever, Dir::Up, true), Some(9));
        assert_eq!(signal(ANALOG_LEVER, lever, Dir::East, true), Some(0));
        assert_eq!(connected("wall", Dir::West), Dir::West);
        assert_eq!(signal("minecraft:stone", lever, Dir::Up, false), None);
    }

    #[test]
    fn a_latch_sets_by_its_back_and_resets_by_its_sides() {
        let on = |k: &str| match k {
            "facing" => Some("south".to_owned()),
            "powering" => Some("true".to_owned()),
            _ => None,
        };
        assert_eq!(signal(LATCH, on, Dir::South, false), Some(15));
        assert_eq!(signal(LATCH, on, Dir::North, false), Some(0));
        // Back on: set; side on: reset; side off with the back still on: set again.
        assert!(latch(false, false, true, false, false));
        assert!(!latch(true, true, true, false, true));
        assert!(latch(false, true, true, true, false));
        // Back off keeps it set, unless a side is on.
        assert!(latch(true, true, false, false, false));
        assert!(!latch(true, true, false, true, true));
    }
}
