//! Create's deployer (DeployerBlock, DeployerBlockEntity, BeltDeployerCallbacks,
//! DeployerRenderer; MIT, THIRD_PARTY_NOTICES.md) on create:deployer, in its use mode.
//! It holds what the player gives it (a use swaps it with the player's hand). Turning, it
//! waits, reaches out two blocks ahead and draws back on Create's timer
//! (`getTimerSpeed`: `clamp(|RPM| * 2, 8, 512)` of 1000 a stroke). At full reach it works:
//! - an item on a depot there (`BeltDeployerCallbacks`): one item of the stack by Create's
//!   deploying recipes, then its item application ones (not the `_manual_only`), the held
//!   item used up unless the recipe keeps it;
//! - else, a block item it holds is placed there, when the place is free.
//!
//! Not as Create: no punch mode, no use of items on blocks or entities (`DeployerHandler`),
//! no filter, a held tool is used up like any item, and no belt (only a depot).
use super::kinetic::{Axis, Dir};
use super::millstone::Held;
use horizon_core::random::RandomSource;
use serde::{Deserialize, Serialize};

pub(super) const DEPLOYER: &str = "create:deployer";

/// `DeployerBlockEntity.State` (no DUMPING: it never holds more than its hand).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum State {
    #[default]
    Waiting,
    Expanding,
    Retracting,
}

/// A deployer's hand and stroke.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Deployer {
    #[serde(default)]
    pub held: Option<Held>,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub timer: i32,
}

/// `DeployerBlockEntity.getTimerSpeed`.
pub(super) fn timer_speed(speed: f32) -> i32 {
    if speed == 0.0 {
        0
    } else {
        (speed * 2.0).abs().clamp(8.0, 512.0) as i32
    }
}

impl Deployer {
    /// One tick of `DeployerBlockEntity.tick` at `speed`; `ready` says whether it has work
    /// ahead (asked only when it would start). Whether it works now, at full reach.
    pub fn tick(&mut self, speed: f32, ready: impl FnOnce() -> bool) -> bool {
        let step = timer_speed(speed);
        if step == 0 {
            return false;
        }
        if self.timer > 0 {
            self.timer -= step;
            return false;
        }
        match self.state {
            State::Waiting => {
                if ready() {
                    self.state = State::Expanding;
                    self.timer = 1000;
                } else {
                    self.timer = step * 10;
                }
                false
            }
            State::Expanding => {
                self.state = State::Retracting;
                self.timer = 1000;
                true
            }
            State::Retracting => {
                self.state = State::Waiting;
                self.timer = 500;
                false
            }
        }
    }
}

/// `DeployerBlockEntity.getHandOffset`: how far the hand is out, in blocks, for a stroke
/// in `state` with `timer` left, `partial` of a tick on at `speed`, holding or not.
pub(super) fn hand_offset(state: State, timer: i32, partial: f32, speed: f32, holding: bool) -> f32 {
    let step = timer_speed(speed) as f32;
    let progress = match state {
        State::Expanding => 1.0 - (timer as f32 - partial * step) / 1000.0,
        State::Retracting => (timer as f32 - partial * step) / 1000.0,
        State::Waiting => 0.0,
    };
    // Its reach to a full block ahead, as `start`'s ray finds one: .5 + .75.
    let reach = 1.25;
    let hand = if holding { 4.0 / 16.0 } else { 0.0 };
    (progress.clamp(0.0, 1.0) * (reach + hand)).min(21.0 / 16.0)
}

/// `DirectionalAxisKineticBlock.getRotationAxis`: its shaft's axis, across its facing.
pub(super) fn shaft_axis(facing: Dir, along_first: bool) -> Axis {
    match facing.axis() {
        Axis::X => {
            if along_first {
                Axis::Y
            } else {
                Axis::Z
            }
        }
        Axis::Y => {
            if along_first {
                Axis::X
            } else {
                Axis::Z
            }
        }
        Axis::Z => {
            if along_first {
                Axis::X
            } else {
                Axis::Y
            }
        }
    }
}

/// DeployerRenderer's pole and hand (`DEPLOYER_POLE`, `DEPLOYER_HAND_POINTING` or
/// `_HOLDING`): modelled facing south.
pub(super) fn arm_models(holding: bool) -> [&'static str; 2] {
    [
        "create:block/deployer/pole",
        if holding {
            "create:block/deployer/hand_holding"
        } else {
            "create:block/deployer/hand_pointing"
        },
    ]
}

impl super::super::GameLevel<'_> {
    /// DeployerBlock's placement, a DirectionalAxisKineticBlock's: facing the player, its
    /// shaft across their way (`saw::placement`, without the saw's flip).
    pub(in crate::game) fn tachyon_deployer_place(
        &self,
        state: horizon_world::registry::BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
    ) {
        if answer.is_some() || self.native_state_block(state) != Some(DEPLOYER) {
            return;
        }
        let (facing, along_first, _) = super::saw::placement(context.yaw, context.pitch);
        let placed = self
            .native_state_with_property(state, "facing", facing.name())
            .and_then(|s| {
                let along = if along_first { "true" } else { "false" };
                self.native_state_with_property(s, "axis_along_first", along)
            });
        if let Some(placed) = placed {
            *answer = Some(Some(placed));
        }
    }
}

impl super::super::Game<'_> {
    /// The owner's deployers turning: each one's stroke, and its work at full reach.
    pub(super) fn tachyon_deployers(&mut self) {
        use super::kinetic::Part;
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let turning: Vec<([i32; 3], Dir, f32)> = super::OWNERS.with_borrow(|owners| {
            let Some(o) = owners.get(dimension) else {
                return Vec::new();
            };
            let mut list: Vec<_> = (o.index.parts.iter())
                .filter_map(|(p, part)| match *part {
                    Part::Deployer { facing, .. } => {
                        let speed = o.shown.speeds.get(p).map_or(0.0, |s| s[0]);
                        (speed != 0.0).then_some((*p, facing, speed))
                    }
                    _ => None,
                })
                .collect();
            list.sort_by_key(|d| d.0);
            list
        });
        for (pos, facing, speed) in turning {
            let o = facing.offset();
            let target = [pos[0] + o[0] * 2, pos[1] + o[1] * 2, pos[2] + o[2] * 2];
            let mut d = super::OWNERS.with_borrow_mut(|owners| {
                owners.get_mut(dimension).and_then(|o| o.deployers.remove(&pos)).unwrap_or_default()
            });
            let before = d.clone();
            // Asked only when its wait is over, as `tick` would.
            let ready = d.state == State::Waiting && d.timer <= 0 && self.tachyon_deployer_ready(&d, target);
            let works = d.tick(speed, || ready);
            if works {
                self.tachyon_deployer_work(&mut d, target);
            }
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) {
                    if d.held.is_some() || d.state != State::Waiting || d.timer > 0 {
                        o.deployers.insert(pos, d.clone());
                    }
                    if d.held != before.held {
                        o.mills_changed = true;
                    }
                }
            });
        }
    }

    /// The recipe for the depot's item at `target` with the held item, if it has one.
    fn tachyon_deployer_recipe(&mut self, held: &Held, target: [i32; 3]) -> Option<(String, Vec<(String, u32, f32)>, bool)> {
        let dimension = self.native_dimension();
        let item = super::OWNERS.with_borrow(|owners| {
            let o = owners.get(dimension)?;
            if o.index.parts.get(&target) != Some(&super::kinetic::Part::Depot) {
                return None;
            }
            o.mills.get(&target)?.input.as_ref().map(|h| h.item.clone())
        })?;
        let recipes = self.tachyon_create_recipes();
        let r = recipes.deploying(&item, &held.item)?;
        let mut outs = vec![(r.result.clone(), 1, 1.0)];
        outs.extend(r.extra.iter().map(|o| (o.item.clone(), o.count, o.chance)));
        Some((item, outs, r.keep))
    }

    /// `shouldActivate` and BeltDeployerCallbacks' wait: work on the depot ahead, or a block
    /// to place where nothing stands.
    fn tachyon_deployer_ready(&mut self, d: &Deployer, target: [i32; 3]) -> bool {
        let Some(held) = d.held.as_ref() else {
            return false;
        };
        if self.tachyon_deployer_recipe(held, target).is_some() {
            return true;
        }
        self.tachyon_deployer_block(&held.item, target).is_some()
    }

    /// The state a held block item is placed in at `target`, when the place is free.
    fn tachyon_deployer_block(&self, item: &str, target: [i32; 3]) -> Option<horizon_world::registry::BlockStateId> {
        if !self.world.chunks.contains_key(&(target[0] >> 4, target[2] >> 4)) {
            return None;
        }
        let here = self.world.block(target[0], target[1], target[2]);
        let facts = self.res.facts().get(here);
        if !facts.replaceable || &*facts.name == item {
            return None;
        }
        self.pack.assets.registry.default_state(item)
    }

    /// The work at full reach: the depot's item by its recipe, else the held block placed.
    fn tachyon_deployer_work(&mut self, d: &mut Deployer, target: [i32; 3]) {
        let Some(held) = d.held.clone() else { return };
        let dimension = self.native_dimension();
        if let Some((input, outs, keep)) = self.tachyon_deployer_recipe(&held, target) {
            let mut made: Vec<Held> = Vec::new();
            for (item, count, chance) in outs {
                let n = (0..count).filter(|_| chance >= 1.0 || self.world.random.next_float() <= chance).count() as u32;
                if n > 0 {
                    made.push(Held { item, count: n });
                }
            }
            super::OWNERS.with_borrow_mut(|owners| {
                let Some(o) = owners.get_mut(dimension) else { return };
                let Some(mill) = o.mills.get_mut(&target) else { return };
                let Some(h) = mill.input.as_mut().filter(|h| h.item == input) else { return };
                h.count -= 1;
                if h.count == 0 {
                    mill.input = None;
                }
                mill.progress = 0.0;
                for m in made {
                    match mill.outputs.iter_mut().find(|o| o.item == m.item) {
                        Some(o) => o.count += m.count,
                        None => mill.outputs.push(m),
                    }
                }
                o.mills_changed = true;
            });
            if !keep {
                d.held = Some(held).filter(|h| h.count > 1).map(|h| Held { count: h.count - 1, ..h });
            }
            let at = target.map(|c| f64::from(c) + 0.5);
            self.native_level_play_sound("minecraft:entity.item.pickup", at, 4, 0.25, 0.75);
            return;
        }
        if let Some(state) = self.tachyon_deployer_block(&held.item, target)
            && self.native_set_block(dimension, target, state, 3)
        {
            d.held = Some(held).filter(|h| h.count > 1).map(|h| Held { count: h.count - 1, ..h });
        }
    }

    /// DeployerBlock's use: the player's hand and the deployer's swap.
    pub(super) fn tachyon_use_deployer(&mut self, pos: [i32; 3]) {
        let dimension = self.native_dimension();
        let mut d = super::OWNERS.with_borrow_mut(|owners| {
            owners.get_mut(dimension).and_then(|o| o.deployers.remove(&pos)).unwrap_or_default()
        });
        let given = self.held().filter(|s| s.count > 0).map(|s| Held { item: s.item.to_string(), count: s.count });
        if let Some(g) = &given {
            self.tachyon_spend_held(g.count);
        }
        if let Some(h) = d.held.take() {
            self.native_give_items(&h.item, h.count);
        }
        d.held = given;
        super::OWNERS.with_borrow_mut(|owners| {
            if let Some(o) = owners.get_mut(dimension) {
                o.deployers.insert(pos, d);
                o.mills_changed = true;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stroke_waits_reaches_out_works_and_draws_back_on_creates_timer() {
        let mut d = Deployer::default();
        // Nothing to do: it waits ten steps' time.
        assert!(!d.tick(32.0, || false));
        assert_eq!((d.state, d.timer), (State::Waiting, 640));
        d.timer = 0;
        assert!(!d.tick(32.0, || true));
        assert_eq!((d.state, d.timer), (State::Expanding, 1000));
        // 64 a tick at 32 RPM: 16 ticks out, then it works.
        let mut ticks = 0;
        while !d.tick(32.0, || true) {
            ticks += 1;
        }
        assert_eq!(ticks, 16);
        assert_eq!(d.state, State::Retracting);
        // Stopped, it holds still.
        assert!(!d.tick(0.0, || true));
        assert_eq!(d.timer, 1000);
    }

    #[test]
    fn its_hand_comes_out_with_the_stroke_and_its_shaft_runs_across() {
        assert_eq!(hand_offset(State::Waiting, 0, 0.0, 32.0, false), 0.0);
        assert!((hand_offset(State::Expanding, 500, 0.0, 32.0, false) - 0.625).abs() < 1e-6);
        assert_eq!(hand_offset(State::Expanding, 0, 0.0, 32.0, true), 21.0 / 16.0);
        assert_eq!(shaft_axis(Dir::Down, true), Axis::X);
        assert_eq!(shaft_axis(Dir::North, false), Axis::Y);
        assert_eq!(timer_speed(-300.0), 512);
        assert_eq!(timer_speed(1.0), 8);
    }
}
