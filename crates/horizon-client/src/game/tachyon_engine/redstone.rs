//! Create's analog lever and latches (AnalogLeverBlock, AnalogLeverBlockEntity, AnalogLeverRenderer,
//! PoweredLatchBlock, ToggleLatchBlock, AbstractDiodeBlock; MIT, THIRD_PARTY_NOTICES.md).
//!
//! - The analog lever gives 0 to 15, one more for each use, one less for a sneaking use; its
//!   strength strongly into the block it is on (`getDirectSignal`). Its handle is drawn turned by
//!   the strength, a quarter turn at 15.
//! - The powered latch, a diode, gives 15 out of its front once its back is powered, and stops
//!   once a side is (the last of them to change wins, as `PoweredLatchBlock.tick`).
//! - The toggle latch gives 15 or nothing, swapped each time its back is powered and by a use.
//! - The brass diodes (BrassDiodeBlock, Pulse*BlockEntity) count ticks to their interval (2, the
//!   timer's 20): the pulse repeater gives a short pulse once its back is powered (counting to the
//!   interval and one more),
//!   the pulse extender keeps giving for the interval after its back goes off, the pulse timer
//!   gives a pulse each interval while its back is off. A use inverts what one gives; a sneaking
//!   use steps its interval through 2, 5, 10, 20, 40, 100, 200, 600 and 1200 ticks (Create's
//!   scroll box, not here).
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
pub(super) const PULSE_REPEATER: &str = "create:pulse_repeater";
pub(super) const PULSE_EXTENDER: &str = "create:pulse_extender";
pub(super) const PULSE_TIMER: &str = "create:pulse_timer";
/// The intervals a sneaking use steps through.
pub(super) const INTERVALS: [u32; 9] = [2, 5, 10, 20, 40, 100, 200, 600, 1200];

/// A brass diode's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Pulse {
    Repeater,
    Extender,
    Timer,
}

/// A brass diode's interval (`maxState`) and count (`state`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(super) struct Diode {
    pub max: u32,
    pub state: u32,
}

impl Pulse {
    /// `BrassDiodeBlockEntity.defaultValue`.
    pub fn default_interval(self) -> u32 {
        if self == Pulse::Timer { 20 } else { 2 }
    }

    /// `updateState` of each: a tick of the count, and whether it powers after it.
    pub fn tick(self, d: &mut Diode, powered: bool, powering: bool) -> bool {
        let max = d.max.max(2);
        let at_min = d.state == 0;
        match self {
            Pulse::Repeater => {
                if at_min && !powered {
                    return powering;
                }
                if d.state > max + 1 {
                    if !powered && !powering {
                        d.state = 0;
                    }
                    return powering;
                }
                d.state += 1;
                let mut out = powering;
                if d.state == max - 1 && !powering {
                    out = true;
                }
                if d.state == max + 1 && powering {
                    out = false;
                }
                out
            }
            Pulse::Extender => {
                if at_min && !powered {
                    return powering;
                }
                if at_min || powered {
                    d.state = max;
                    return true;
                }
                if d.state == 1 {
                    if !powered {
                        d.state = 0;
                    }
                    return false;
                }
                d.state -= 1;
                powering
            }
            Pulse::Timer => {
                if powered || d.state >= max - 1 {
                    d.state = 0;
                } else {
                    d.state += 1;
                }
                !powered && if max == 2 { d.state == 0 } else { d.state <= 1 }
            }
        }
    }
}

/// The interval after `max` a sneaking use steps to.
pub(super) fn next_interval(max: u32) -> u32 {
    INTERVALS.into_iter().find(|i| *i > max).unwrap_or(INTERVALS[0])
}

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
        LATCH | TOGGLE_LATCH | PULSE_REPEATER | PULSE_EXTENDER | PULSE_TIMER => {
            // BrassDiodeBlock.getOutputSignal: powering, unless inverted.
            let on = (property("powering").as_deref() == Some("true")) ^ (property("inverted").as_deref() == Some("true"));
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
        if matches!(
            self.native_state_block(state),
            Some(ANALOG_LEVER | LATCH | TOGGLE_LATCH | PULSE_REPEATER | PULSE_EXTENDER | PULSE_TIMER | super::link::LINK)
        ) {
            *source = true;
        }
    }

    /// `getSignal` and `getDirectSignal` for these blocks, `direction` vanilla's.
    pub(in crate::game) fn tachyon_block_signal(&self, state: BlockStateId, direction: Direction, direct: bool, signal: &mut i32) {
        let Some(s) = self.registry.state(state) else { return };
        if s.name == super::link::LINK {
            *signal = super::link::signal(|k| s.properties.get(k).cloned(), of(direction), direct);
            return;
        }
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
            // RedstoneLinkBlock.getStateForPlacement: facing out of the face clicked.
            Some(super::link::LINK) => self.native_state_with_property(state, "facing", of(context.face).name()),
            Some(LATCH | TOGGLE_LATCH | PULSE_REPEATER | PULSE_EXTENDER | PULSE_TIMER) => {
                self.native_state_with_property(state, "facing", horizontal.opposite().name())
            }
            _ => return,
        };
        if let Some(placed) = placed {
            *answer = Some(Some(placed));
        }
    }
}

impl super::super::Game<'_> {
    /// Sets the properties `values` of the block at `p` and tells its neighbours its signal changed.
    pub(super) fn tachyon_redstone_set(&mut self, p: [i32; 3], values: &[(&str, String)]) {
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
            PULSE_REPEATER | PULSE_EXTENDER | PULSE_TIMER if !sneaking => {
                let on = self.native_block_property(pos, "inverted").as_deref() == Some("true");
                self.tachyon_redstone_set(pos, &[("inverted", (!on).to_string())]);
                true
            }
            name @ (PULSE_REPEATER | PULSE_EXTENDER | PULSE_TIMER) => {
                let kind = match name {
                    PULSE_REPEATER => Pulse::Repeater,
                    PULSE_EXTENDER => Pulse::Extender,
                    _ => Pulse::Timer,
                };
                let dimension = self.native_dimension();
                let max = super::OWNERS.with_borrow_mut(|owners| {
                    let o = owners.entry(dimension).or_default();
                    let d = o.diodes.entry(pos).or_insert(Diode { max: kind.default_interval(), state: 0 });
                    d.max = next_interval(d.max);
                    d.state = d.state.min(d.max);
                    o.mills_changed = true;
                    d.max
                });
                self.native_chat(&format!("Interval: {max} ticks"));
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
        let diodes: Vec<([i32; 3], Dir, Pulse)> = super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return Vec::new() };
            let parts = &o.index.parts;
            o.diodes.retain(|p, _| matches!(parts.get(p), Some(Part::Diode(_, _))));
            (parts.iter())
                .filter_map(|(p, part)| match *part {
                    Part::Diode(facing, kind) => Some((*p, facing, kind)),
                    _ => None,
                })
                .collect()
        });
        for (p, facing, kind) in diodes {
            let at = BlockPos::new(p[0], p[1], p[2]);
            let o = facing.offset();
            let powered = self.with_level(|b, level, ticks| b.signal(level, ticks, at.offset(o[0], o[1], o[2]), core(facing)) > 0);
            let powering = self.native_block_property(p, "powering").as_deref() == Some("true");
            let was = self.native_block_property(p, "powered").as_deref() == Some("true");
            let next = super::OWNERS.with_borrow_mut(|owners| {
                let o = owners.get_mut(dimension)?;
                let d = o.diodes.entry(p).or_insert(Diode { max: kind.default_interval(), state: 0 });
                let before = *d;
                let next = kind.tick(d, powered, powering);
                o.mills_changed |= *d != before && d.state == 0;
                Some(next)
            });
            let Some(next) = next else { continue };
            if next != powering || powered != was {
                self.tachyon_redstone_set(p, &[("powered", powered.to_string()), ("powering", next.to_string())]);
            }
        }
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
    fn brass_diodes_pulse_extend_and_time_as_creates() {
        // A repeater at 2: powered, it gives a pulse two ticks long, then nothing while powered.
        let mut d = Diode { max: 2, state: 0 };
        let mut on = false;
        let mut seen = Vec::new();
        for _ in 0..6 {
            on = Pulse::Repeater.tick(&mut d, true, on);
            seen.push(on);
        }
        assert_eq!(seen, [true, true, false, false, false, false]);
        // An extender at 5: on while powered, then five ticks more.
        let mut d = Diode { max: 5, state: 0 };
        let mut on = Pulse::Extender.tick(&mut d, true, false);
        assert!(on);
        let mut count = 0;
        while on {
            on = Pulse::Extender.tick(&mut d, false, on);
            count += 1;
        }
        assert_eq!(count, 5);
        // A timer at 20: a pulse two ticks long every 20, none while powered.
        let mut d = Diode { max: 20, state: 0 };
        let pulses = (0..40).filter(|_| Pulse::Timer.tick(&mut d, false, false)).count();
        assert_eq!(pulses, 4);
        assert!(!Pulse::Timer.tick(&mut d, true, true));
        assert_eq!(next_interval(2), 5);
        assert_eq!(next_interval(1200), 2);
        let inverted = |k: &str| match k {
            "facing" => Some("east".to_owned()),
            "inverted" => Some("true".to_owned()),
            _ => None,
        };
        assert_eq!(signal(PULSE_TIMER, inverted, Dir::East, false), Some(15));
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
