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
use horizon_core::{
    geometry::{Direction, Vec3},
    position::BlockPos,
    random::RandomSource,
};
use horizon_world::{
    level::{Behaviours, LevelAccess, PlaceContext, PlaceOutcome, Ticks, data::ItemKind, flags},
    registry::{BlockRegistry, BlockStateId},
};
use serde::{Deserialize, Serialize};

pub(super) trait PlacementValidator {
    fn directional_can_place(&mut self, ctx: &PlaceContext) -> bool;
    fn has_binding(&self, item: &str) -> bool;
    fn is_scaffolding(&self, item: &str) -> bool;
    fn placement_state(&mut self, ctx: &PlaceContext) -> Option<(BlockPos, BlockStateId)>;
    fn state_for_placement(&mut self, default: BlockStateId, ctx: &PlaceContext) -> Option<BlockStateId>;
    fn can_survive(&mut self, pos: BlockPos, state: BlockStateId) -> bool;
    fn is_unobstructed(&self, pos: BlockPos, state: BlockStateId) -> bool;
}

struct BehavioursPlacement<'a, L: LevelAccess + ?Sized> {
    registry: &'a BlockRegistry,
    behaviours: &'a Behaviours,
    level: &'a mut L,
    ticks: &'a mut Ticks,
}

impl<L: LevelAccess + ?Sized> PlacementValidator for BehavioursPlacement<'_, L> {
    fn directional_can_place(&mut self, ctx: &PlaceContext) -> bool {
        self.behaviours.directional_can_place(self.level, self.ticks, ctx)
    }

    fn has_binding(&self, item: &str) -> bool {
        self.behaviours.item_binding(item).is_some()
    }

    fn is_scaffolding(&self, item: &str) -> bool {
        self.behaviours
            .item_binding(item)
            .is_some_and(|bind| bind.kind == ItemKind::Scaffolding)
    }

    fn placement_state(&mut self, ctx: &PlaceContext) -> Option<(BlockPos, BlockStateId)> {
        if ctx.item == "create:chute" {
            return super::chute::preview_place(
                self.registry,
                self.behaviours,
                self.level,
                self.ticks,
                ctx,
            );
        }
        let preview = self.behaviours.placement_preview(self.level, self.ticks, ctx)?;
        Some((preview.pos, preview.state))
    }

    fn state_for_placement(
        &mut self,
        default: BlockStateId,
        ctx: &PlaceContext,
    ) -> Option<BlockStateId> {
        let block = self.behaviours.block(default);
        self.behaviours
            .state_for_placement(self.level, self.ticks, block, ctx)
    }

    fn can_survive(&mut self, pos: BlockPos, state: BlockStateId) -> bool {
        self.behaviours.can_survive(self.level, self.ticks, pos, state)
    }

    fn is_unobstructed(&self, pos: BlockPos, state: BlockStateId) -> bool {
        self.level.is_unobstructed(pos, state)
    }
}

/// Validates a resolved `BlockItem` placement (`directional_can_place`, `placement_state` or
/// `state_for_placement`, optional rotated pillar `axis`, `waterlogged` in water, `can_survive`,
/// and `is_unobstructed`).
pub(super) fn validate_block_placement<V: PlacementValidator>(
    validator: &mut V,
    registry: &BlockRegistry,
    default_state: BlockStateId,
    ctx: &PlaceContext,
    forced_axis: Option<&str>,
    water_present: bool,
) -> Option<(BlockPos, BlockStateId, bool)> {
    if !validator.directional_can_place(ctx) {
        return None;
    }
    let has_binding = validator.has_binding(&ctx.item);
    let (pos, computed) = if has_binding {
        let (p, s) = validator.placement_state(ctx)?;
        if p != ctx.clicked_pos {
            return None;
        }
        (p, s)
    } else {
        let s = validator.state_for_placement(default_state, ctx)?;
        (ctx.clicked_pos, s)
    };
    let mut final_state = computed;
    if let Some(axis) = forced_axis
        && let Some(mut parsed) = registry.state(final_state).cloned()
        && parsed.properties.get("axis").is_some_and(|cur| cur != axis)
    {
        parsed.properties.insert("axis".into(), axis.into());
        final_state = registry.id(&parsed)?;
    }
    if water_present
        && let Some(mut parsed) = registry.state(final_state).cloned()
        && parsed.properties.get("waterlogged").is_some_and(|v| v == "false")
    {
        parsed.properties.insert("waterlogged".into(), "true".into());
        final_state = registry.id(&parsed)?;
    }
    if !has_binding || final_state != computed {
        if !validator.is_scaffolding(&ctx.item) && !validator.can_survive(pos, final_state) {
            return None;
        }
        if !validator.is_unobstructed(pos, final_state) {
            return None;
        }
    }
    Some((pos, final_state, has_binding))
}

pub(super) fn deployer_target_replaceable(
    here_name: &str,
    here_replaceable: bool,
    placed_block: &str,
) -> bool {
    here_replaceable && here_name != placed_block
}

fn direction_of(d: Dir) -> Direction {
    match d {
        Dir::Down => Direction::Down,
        Dir::Up => Direction::Up,
        Dir::North => Direction::North,
        Dir::South => Direction::South,
        Dir::West => Direction::West,
        Dir::East => Direction::East,
    }
}

fn deployer_look(d: Dir) -> (f32, f32) {
    match d {
        Dir::South => (0.0, 0.0),
        Dir::West => (90.0, 0.0),
        Dir::North => (180.0, 0.0),
        Dir::East => (-90.0, 0.0),
        Dir::Down => (0.0, 90.0),
        Dir::Up => (0.0, -90.0),
    }
}

pub(super) const DEPLOYER: &str = "create:deployer";

/// `DeployerBlockEntity.State` (no DUMPING: it never holds more than its hand).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum State {
    #[default]
    Waiting,
    Expanding,
    Retracting,
}

/// A deployer's hand, mode, filter and stroke.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub(super) struct Deployer {
    #[serde(default)]
    pub held: Option<Held>,
    #[serde(default)]
    pub state: State,
    #[serde(default)]
    pub timer: i32,
    #[serde(default)]
    pub filter: Option<String>,
    #[serde(default)]
    pub punch: bool,
    #[serde(default)]
    pub damage: u32,
}

impl Deployer {
    pub(super) fn has_state(&self) -> bool {
        self.held.is_some()
            || self.filter.is_some()
            || self.punch
            || self.damage > 0
            || self.state != State::Waiting
            || self.timer > 0
    }

    pub(super) fn matches_filter(&self, item: &str) -> bool {
        self.filter.as_deref().is_none_or(|f| f == item)
    }

    pub(super) fn set_filter(&mut self, filter: Option<String>) {
        self.filter = filter;
    }

    pub(super) fn toggle_punch(&mut self) -> bool {
        self.punch = !self.punch;
        self.punch
    }

    /// Consumes one item or adds 1 durability damage when `max_durability > 0`, breaking
    /// the tool when `damage >= max_durability`. Returns false if nothing was held.
    pub(super) fn consume_or_damage(&mut self, max_durability: u32) -> bool {
        let Some(held) = self.held.as_mut() else {
            return false;
        };
        if max_durability > 0 {
            self.damage += 1;
            if self.damage >= max_durability {
                self.damage = 0;
                if held.count > 1 {
                    held.count -= 1;
                } else {
                    self.held = None;
                }
            }
            return true;
        }
        self.damage = 0;
        if held.count > 1 {
            held.count -= 1;
        } else {
            self.held = None;
        }
        true
    }
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
            let ready = d.state == State::Waiting && d.timer <= 0 && self.tachyon_deployer_ready(&d, facing, target);
            let works = d.tick(speed, || ready);
            if works {
                self.tachyon_deployer_work(&mut d, facing, target);
            }
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) {
                    if d.has_state() {
                        o.deployers.insert(pos, d.clone());
                    }
                    if d.held != before.held || d.punch != before.punch || d.filter != before.filter {
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

    fn tachyon_deployer_punch_mob_present(&self, target: [i32; 3]) -> bool {
        let box_at = horizon_core::geometry::Aabb::unit_cube(Vec3::new(
            f64::from(target[0]),
            f64::from(target[1]),
            f64::from(target[2]),
        ));
        self.mobs
            .iter()
            .any(|m| m.alive() && !m.removed && m.bounds().intersects(box_at))
    }

    /// Validates `ctx` as a `BlockItem` placement (`BlockItem -> block` via `info.block`,
    /// `directional_can_place`, `state_for_placement`, `waterlogged`, `can_survive`,
    /// `is_unobstructed`), optionally forcing rotated pillar `axis`.
    pub(super) fn tachyon_validated_block_state(
        &mut self,
        ctx: &PlaceContext,
        forced_axis: Option<&str>,
    ) -> Option<(BlockPos, BlockStateId, bool)> {
        let default_state = self.res.items().get(&ctx.item).and_then(|i| i.block)?;
        let registry = self.pack.assets.registry;
        let before = self.world.block(ctx.clicked_pos.x, ctx.clicked_pos.y, ctx.clicked_pos.z);
        let water_present = registry.state(before).is_some_and(|s| {
            (s.name == "minecraft:water"
                && s.properties.get("level").is_none_or(|l| l == "0"))
                || s.properties.get("waterlogged").is_some_and(|v| v == "true")
        });
        self.with_level(|b, level, ticks| {
            let mut validator = BehavioursPlacement {
                registry,
                behaviours: b,
                level,
                ticks,
            };
            validate_block_placement(
                &mut validator,
                registry,
                default_state,
                ctx,
                forced_axis,
                water_present,
            )
        })
    }

    /// Places a block validated by `tachyon_validated_block_state` via `Behaviours::place_block`
    /// (`chute::place` for `create:chute`, or `set_block` fallback if no `ItemBinding` or adjusted
    /// axis/waterlogging).
    pub(super) fn tachyon_place_validated_block(
        &mut self,
        ctx: &PlaceContext,
        forced_axis: Option<&str>,
    ) -> Option<(BlockPos, BlockStateId)> {
        let (pos, final_state, has_binding) =
            self.tachyon_validated_block_state(ctx, forced_axis)?;
        let previous = self.world.block(pos.x, pos.y, pos.z);
        let registry = self.pack.assets.registry;
        self.ensure_stateful_entity_table();
        let placed = self.with_level(|b, level, ticks| {
            if has_binding {
                let outcome = if ctx.item == "create:chute" {
                    super::chute::place(registry, b, level, ticks, ctx)
                } else {
                    b.place_block(level, ticks, ctx)
                };
                let PlaceOutcome::Placed { pos, state } = outcome else {
                    return None;
                };
                if state != final_state {
                    b.set_block(level, ticks, pos, final_state, flags::UPDATE_ALL);
                }
                Some((pos, final_state))
            } else if b.set_block(level, ticks, pos, final_state, flags::UPDATE_ALL | 8) {
                Some((pos, final_state))
            } else {
                None
            }
        })?;
        let new_name = self.res.facts().get(placed.1).name.clone();
        if !self.world.block_entities.stateful_entity(placed.1.0)
            && self.res.facts().get(previous).name != new_name
            && let Some(entity) = self.new_block_entity_for_state(placed.1)
        {
            self.world
                .block_entities
                .insert((placed.0.x, placed.0.y, placed.0.z), entity);
        }
        let _ = self.process_level_events();
        Some(placed)
    }

    /// `shouldActivate` and BeltDeployerCallbacks' wait: work on the depot ahead, or a block
    /// to place where nothing stands (or a mob ahead in punch mode).
    fn tachyon_deployer_ready(&mut self, d: &Deployer, facing: Dir, target: [i32; 3]) -> bool {
        if d.punch && self.tachyon_deployer_punch_mob_present(target) {
            return true;
        }
        let Some(held) = d.held.as_ref() else {
            return false;
        };
        if self.tachyon_sequenced_depot_ready(
            target,
            super::create_recipes::SequencedAction::Deploying { held: &held.item },
        ) || self.tachyon_deployer_recipe(held, target).is_some()
        {
            return true;
        }
        if !d.punch && self.tachyon_deployer_block(&held.item, facing, target).is_some() {
            return true;
        }
        false
    }

    /// The validated placement context and state for a held block item at `target`.
    fn tachyon_deployer_block(
        &mut self,
        item: &str,
        facing: Dir,
        target: [i32; 3],
    ) -> Option<(PlaceContext, BlockStateId)> {
        if !self.world.chunks.contains_key(&(target[0] >> 4, target[2] >> 4)) {
            return None;
        }
        let block_state = self.res.items().get(item).and_then(|i| i.block)?;
        let here = self.world.block(target[0], target[1], target[2]);
        let facts = self.res.facts().get(here);
        let placed_block = &*self.res.facts().get(block_state).name;
        if !deployer_target_replaceable(&facts.name, facts.replaceable, placed_block) {
            return None;
        }
        let (yaw, pitch) = deployer_look(facing);
        let ctx = PlaceContext {
            clicked_pos: BlockPos::new(target[0], target[1], target[2]),
            face: direction_of(facing.opposite()),
            hit: Vec3::new(
                f64::from(target[0]) + 0.5,
                f64::from(target[1]) + 0.5,
                f64::from(target[2]) + 0.5,
            ),
            inside: false,
            yaw,
            pitch,
            sneaking: false,
            item: item.to_owned(),
            can_use_game_master_blocks: false,
        };
        let (_, state, _) = self.tachyon_validated_block_state(&ctx, None)?;
        Some((ctx, state))
    }

    /// The work at full reach: punch mode attacks mobs at `target`, else sequenced assembly
    /// or regular deploying on a depot, else places the held block.
    fn tachyon_deployer_work(&mut self, d: &mut Deployer, facing: Dir, target: [i32; 3]) {
        if d.punch {
            let box_at = horizon_core::geometry::Aabb::unit_cube(Vec3::new(
                f64::from(target[0]),
                f64::from(target[1]),
                f64::from(target[2]),
            ));
            let (damage, max_durability) = d.held.as_ref().map_or((1.0, 0), |h| {
                let info = self.items().get(&h.item);
                let atk = info.map_or(1.0, |i| i.attack_damage).max(1.0);
                let dur = info.map_or(0, |i| i.max_damage);
                (atk, dur)
            });
            let mut hit_id = None;
            for mob in &self.mobs {
                if mob.alive() && !mob.removed && mob.bounds().intersects(box_at) {
                    hit_id = Some(mob.id);
                    break;
                }
            }
            if let Some(id) = hit_id {
                for mob in &mut self.mobs {
                    if mob.id == id {
                        mob.damage(
                            crate::game::entity::damage_level_of!(self),
                            damage,
                            horizon_client::mobs::Damage::of(
                                horizon_client::mobs::DamageKind::PlayerAttack,
                            ),
                        );
                        break;
                    }
                }
                if max_durability > 0 {
                    d.consume_or_damage(max_durability);
                }
                return;
            }
        }
        let Some(held) = d.held.clone() else { return };
        let dimension = self.native_dimension();
        let max_durability = self.items().get(&held.item).map_or(0, |i| i.max_damage);
        if let Some((consume_held, _)) = self.tachyon_sequenced_depot_work(
            target,
            super::create_recipes::SequencedAction::Deploying { held: &held.item },
        ) {
            if consume_held {
                d.consume_or_damage(max_durability);
            }
            let at = target.map(|c| f64::from(c) + 0.5);
            if let Some(level) = self.native_level(dimension) {
                self.native_level_play_sound(level, "minecraft:entity.item.pickup", at, 4, 0.25, 0.75);
            }
            return;
        }
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
                d.consume_or_damage(max_durability);
            }
            let at = target.map(|c| f64::from(c) + 0.5);
            if let Some(level) = self.native_level(dimension) {
                self.native_level_play_sound(level, "minecraft:entity.item.pickup", at, 4, 0.25, 0.75);
            }
            return;
        }
        if !d.punch
            && let Some((ctx, _)) = self.tachyon_deployer_block(&held.item, facing, target)
            && self.tachyon_place_validated_block(&ctx, None).is_some()
        {
            d.consume_or_damage(0);
        }
    }

    /// DeployerBlock's use: wrench toggles punch mode, empty-hand sneak clears filter,
    /// otherwise swaps the player's hand and the deployer's held stack (respecting filter
    /// and preserving tool durability).
    pub(super) fn tachyon_use_deployer(&mut self, pos: [i32; 3]) -> bool {
        let dimension = self.native_dimension();
        let sneaking = self.tachyon_sneaking();
        let mut d = super::OWNERS.with_borrow_mut(|owners| {
            owners.get_mut(dimension).and_then(|o| o.deployers.remove(&pos)).unwrap_or_default()
        });
        if self.held().is_some_and(|s| &*s.item == super::wrench::WRENCH) && !sneaking {
            d.toggle_punch();
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) {
                    o.deployers.insert(pos, d);
                    o.mills_changed = true;
                }
            });
            return true;
        }
        let given = self
            .held()
            .filter(|s| s.count > 0)
            .map(|s| (Held { item: s.item.to_string(), count: s.count }, s.damage.max(0) as u32));
        if given.is_none() && sneaking && d.filter.is_some() {
            d.set_filter(None);
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) {
                    if d.has_state() {
                        o.deployers.insert(pos, d);
                    }
                    o.mills_changed = true;
                }
            });
            return true;
        }
        if let Some((ref g, _)) = given
            && !d.matches_filter(&g.item)
        {
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) && d.has_state() {
                    o.deployers.insert(pos, d);
                }
            });
            return false;
        }
        if given.is_none() && d.held.is_none() {
            super::OWNERS.with_borrow_mut(|owners| {
                if let Some(o) = owners.get_mut(dimension) && d.has_state() {
                    o.deployers.insert(pos, d);
                }
            });
            return false;
        }
        if let Some((ref g, _)) = given {
            self.tachyon_spend_held(g.count);
        }
        let prev_damage = d.damage;
        if let Some(h) = d.held.take() {
            let mut stack = horizon_client::items::ItemStack::new(&h.item, h.count);
            stack.damage = prev_damage;
            self.give_or_drop(stack);
        }
        match given {
            Some((g, dmg)) => {
                d.held = Some(g);
                d.damage = dmg;
            }
            None => {
                d.held = None;
                d.damage = 0;
            }
        }
        super::OWNERS.with_borrow_mut(|owners| {
            if let Some(o) = owners.get_mut(dimension) {
                if d.has_state() {
                    o.deployers.insert(pos, d);
                }
                o.mills_changed = true;
            }
        });
        true
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

    #[test]
    fn deployer_and_assist_validate_placement_axis_waterlogging_and_survival() {
        use horizon_core::nbt::{Compound, JavaString, Tag};

        let nbt_state = |name: &str, properties: &[(&str, &str)]| -> Tag {
            let mut map = Compound::new();
            map.insert(JavaString::from("id"), Tag::String(name.into()));
            if !properties.is_empty() {
                let mut props = Compound::new();
                for (k, v) in properties {
                    props.insert(JavaString::from(*k), Tag::String((*v).into()));
                }
                map.insert(JavaString::from("properties"), Tag::Compound(props));
            }
            Tag::Compound(map)
        };
        let mut defaults = Compound::new();
        defaults.insert(JavaString::from("minecraft:air"), Tag::Int(0));
        defaults.insert(JavaString::from("minecraft:stone"), Tag::Int(1));
        defaults.insert(JavaString::from("minecraft:wheat"), Tag::Int(2));
        defaults.insert(JavaString::from("create:shaft"), Tag::Int(3));
        let mut root = Compound::new();
        root.insert(JavaString::from("DataVersion"), Tag::Int(horizon_world::DATA_VERSION));
        root.insert(
            JavaString::from("states"),
            Tag::List(vec![
                nbt_state("minecraft:air", &[]),
                nbt_state("minecraft:stone", &[]),
                nbt_state("minecraft:wheat", &[("age", "0")]),
                nbt_state("create:shaft", &[("axis", "y"), ("waterlogged", "false")]),
                nbt_state("create:shaft", &[("axis", "x"), ("waterlogged", "false")]),
                nbt_state("create:shaft", &[("axis", "x"), ("waterlogged", "true")]),
            ]),
        );
        root.insert(JavaString::from("defaults"), Tag::Compound(defaults));
        let registry = BlockRegistry::from_nbt(&Tag::Compound(root)).expect("test registry");

        #[derive(Default)]
        struct FakeValidator {
            can_place: bool,
            has_binding: bool,
            scaffolding: bool,
            bound_pos: Option<BlockPos>,
            survives: bool,
            unobstructed: bool,
            checked_survival: bool,
        }
        impl PlacementValidator for FakeValidator {
            fn directional_can_place(&mut self, _ctx: &PlaceContext) -> bool {
                self.can_place
            }
            fn has_binding(&self, _item: &str) -> bool {
                self.has_binding
            }
            fn is_scaffolding(&self, _item: &str) -> bool {
                self.scaffolding
            }
            fn placement_state(&mut self, ctx: &PlaceContext) -> Option<(BlockPos, BlockStateId)> {
                Some((self.bound_pos.unwrap_or(ctx.clicked_pos), BlockStateId(2)))
            }
            fn state_for_placement(
                &mut self,
                default: BlockStateId,
                _ctx: &PlaceContext,
            ) -> Option<BlockStateId> {
                Some(default)
            }
            fn can_survive(&mut self, _pos: BlockPos, _state: BlockStateId) -> bool {
                self.checked_survival = true;
                self.survives
            }
            fn is_unobstructed(&self, _pos: BlockPos, _state: BlockStateId) -> bool {
                self.unobstructed
            }
        }

        let pos = BlockPos::new(4, 64, -2);
        let ctx = PlaceContext {
            clicked_pos: pos,
            face: Direction::Up,
            hit: Vec3::new(4.5, 64.5, -1.5),
            inside: false,
            yaw: 0.0,
            pitch: 0.0,
            sneaking: false,
            item: "create:shaft".into(),
            can_use_game_master_blocks: false,
        };

        // 1. Replaceability guard blocks solid or identical targets.
        assert!(deployer_target_replaceable("minecraft:air", true, "create:shaft"));
        assert!(!deployer_target_replaceable("minecraft:stone", false, "create:shaft"));
        assert!(!deployer_target_replaceable("minecraft:water", true, "minecraft:water"));

        // 2. Assist placement applies forced axis + waterlogged=true and checks survival/obstruction.
        let mut ok = FakeValidator {
            can_place: true,
            survives: true,
            unobstructed: true,
            ..FakeValidator::default()
        };
        assert_eq!(
            validate_block_placement(&mut ok, &registry, BlockStateId(3), &ctx, Some("x"), true),
            Some((pos, BlockStateId(5), false))
        );
        assert!(ok.checked_survival);

        // 3. Survival and obstruction rejections prevent placement.
        let mut cannot_survive = FakeValidator {
            can_place: true,
            survives: false,
            unobstructed: true,
            ..FakeValidator::default()
        };
        assert_eq!(
            validate_block_placement(&mut cannot_survive, &registry, BlockStateId(2), &ctx, None, false),
            None
        );
        let mut blocked = FakeValidator {
            can_place: true,
            survives: true,
            unobstructed: false,
            ..FakeValidator::default()
        };
        assert_eq!(
            validate_block_placement(&mut blocked, &registry, BlockStateId(3), &ctx, None, false),
            None
        );

        // 4. Directional/target shift checks reject shifted or non-placeable targets.
        let mut shifted = FakeValidator {
            can_place: true,
            has_binding: true,
            bound_pos: Some(BlockPos::new(4, 65, -2)),
            survives: true,
            unobstructed: true,
            ..FakeValidator::default()
        };
        assert_eq!(
            validate_block_placement(&mut shifted, &registry, BlockStateId(2), &ctx, None, false),
            None
        );
        let mut denied_dir = FakeValidator {
            can_place: false,
            survives: true,
            unobstructed: true,
            ..FakeValidator::default()
        };
        assert_eq!(
            validate_block_placement(&mut denied_dir, &registry, BlockStateId(3), &ctx, None, false),
            None
        );
    }

    #[test]
    fn deployer_filter_punch_mode_and_tool_durability_work_as_expected() {
        let old: Deployer = serde_json::from_str("{}").unwrap();
        assert_eq!(old.filter, None);
        assert!(!old.punch);
        assert_eq!(old.damage, 0);
        assert!(!old.has_state());

        let mut d = Deployer::default();
        assert!(d.toggle_punch());
        assert!(d.punch);
        assert!(d.has_state());
        assert!(!d.toggle_punch());

        d.set_filter(Some("minecraft:iron_ingot".into()));
        assert!(d.matches_filter("minecraft:iron_ingot"));
        assert!(!d.matches_filter("minecraft:gold_ingot"));

        d.held = Some(Held { item: "create:sand_paper".into(), count: 1 });
        assert!(d.consume_or_damage(3));
        assert_eq!(d.damage, 1);
        assert!(d.held.is_some());
        assert!(d.consume_or_damage(3));
        assert_eq!(d.damage, 2);
        assert!(d.consume_or_damage(3));
        assert_eq!(d.damage, 0);
        assert_eq!(d.held, None);

        d.held = Some(Held { item: "create:andesite_alloy".into(), count: 2 });
        assert!(d.consume_or_damage(0));
        assert_eq!(d.held.as_ref().unwrap().count, 1);
        assert!(d.consume_or_damage(0));
        assert_eq!(d.held, None);
    }
}
