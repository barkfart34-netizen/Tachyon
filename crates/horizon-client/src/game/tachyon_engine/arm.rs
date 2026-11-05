//! Create's mechanical arm, from ArmBlockEntity, ArmInteractionPoint,
//! AllArmInteractionPointTypes, ArmAngleTarget and ArmRenderer (MIT). One
//! stack moves at a time; insertion is simulated before an input is chosen
//! and again when it is reached. Only ordinary ROUND_ROBIN is used.
//!
//! There is no point-selection tool. Within Create's strict five-block
//! range, containers, depots and belts west of the arm, or north on the same
//! X, are inputs; the others are outputs. Basins and inserting funnels are
//! always outputs; extracting funnels are excluded. Points are sorted by
//! (X, Y, Z), saved, and rediscovered every 20 ticks while searching, only
//! when the surrounding chunks are loaded. A moving arm keeps its selection.

use super::super::{Game, machines::dir_from_name};
use super::{Kinds, NATURE_TICKS, OWNERS, kinetic::Part, millstone::Mill};
use glam::{DMat4, DVec3, Mat4, Vec3};
use horizon_client::{entities::ItemEntity, items::ItemStack, mesher::Vertex};
use horizon_core::geometry::Direction;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const RANGE: i32 = 5;
const STACK: u32 = 64;
const LAZY_TICKS: u8 = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(super) enum Phase {
    #[default]
    SearchInputs,
    MoveToInput,
    SearchOutputs,
    MoveToOutput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
enum Kind {
    Container,
    Depot,
    Belt,
    Basin,
    Funnel([i32; 3]),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct Point {
    pos: [i32; 3],
    kind: Kind,
    input: bool,
}

impl Point {
    fn new(arm: [i32; 3], pos: [i32; 3], kind: Kind) -> Option<Self> {
        let d = [0, 1, 2].map(|i| i64::from(pos[i]) - i64::from(arm[i]));
        if pos == arm || d.iter().any(|v| v.abs() >= i64::from(RANGE))
            || d.iter().map(|v| v * v).sum::<i64>() >= i64::from(RANGE * RANGE)
        {
            return None;
        }
        let input = matches!(kind, Kind::Container | Kind::Depot | Kind::Belt)
            && (pos[0] < arm[0] || (pos[0] == arm[0] && pos[2] < arm[2]));
        Some(Self { pos, kind, input })
    }

    /// AllArmInteractionPointTypes.DepotPoint and FunnelPoint; ordinary
    /// containers and basins use ArmInteractionPoint's center and DOWN.
    fn target(self) -> (DVec3, DVec3) {
        let at = DVec3::from_array(self.pos.map(f64::from));
        match self.kind {
            Kind::Depot | Kind::Belt => (at + DVec3::new(0.5, 14.0 / 16.0, 0.5), DVec3::NEG_Y),
            Kind::Funnel(facing) => {
                let facing = DVec3::from_array(facing.map(f64::from));
                (at + DVec3::splat(0.5) - facing * 0.15, -facing)
            }
            _ => (at + DVec3::splat(0.5), DVec3::NEG_Y),
        }
    }
}

/// ArmAngleTarget's four angles, in degrees. Only floor mounting is ported.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Angles {
    base: f32,
    lower: f32,
    upper: f32,
    head: f32,
}

impl Default for Angles {
    fn default() -> Self {
        Self { base: 0.0, lower: 135.0, upper: 45.0, head: 0.0 }
    }
}

impl Angles {
    fn at(arm: [i32; 3], point: Point) -> Self {
        let (claw, facing) = point.target();
        let origin = DVec3::from_array(arm.map(f64::from)) + DVec3::new(0.5, 0.5 + 6.0 / 16.0, 0.5);
        let diff = claw - facing * 0.5 - origin;
        let horizontal = (diff.x * diff.x + diff.z * diff.z).sqrt() as f32;
        let base = diff.x.atan2(diff.z).to_degrees() as f32 + 180.0;
        let offset = diff.y.atan2(f64::from(horizontal)).to_degrees() as f32;
        let a = 14.0f32 / 16.0;
        let b = 15.0f32 / 16.0;
        let distance = ((diff.y * diff.y + f64::from(horizontal * horizontal)) as f32)
            .sqrt().clamp(1.0 / 8.0, a + b);
        let alpha = (-b * b + a * a + distance * distance) / (2.0 * a * distance);
        let beta = (-distance * distance + a * a + b * b) / (2.0 * b * a);
        let lower = f64::from(alpha).acos().to_degrees() as f32 + offset;
        let upper = f64::from(beta).acos().to_degrees() as f32;
        let lower = if lower.is_nan() { 0.0 } else { lower };
        let upper = if upper.is_nan() { 0.0 } else { upper };
        let head = DMat4::from_rotation_x(f64::from(upper + 180.0).to_radians())
            .transform_vector3(DVec3::new(0.0, f64::from(b), 0.0));
        let head = DMat4::from_rotation_x(f64::from(lower - 90.0).to_radians())
            .transform_vector3(head + DVec3::new(0.0, f64::from(a), 0.0));
        let head = DMat4::from_rotation_y(f64::from(base).to_radians()).transform_vector3(head) + origin;
        let diff = claw - head;
        let horizontal = (diff.x * diff.x + diff.z * diff.z).sqrt() as f32;
        let head = -(lower + upper + 135.0 - diff.y.atan2(f64::from(horizontal)).to_degrees() as f32);
        Self { base, lower, upper, head }
    }
}

fn step(speed: f32) -> f32 {
    if speed.is_finite() { speed.abs().min(256.0) / 1024.0 } else { 0.0 }
}

fn angle_lerp(progress: f32, from: f32, to: f32) -> f32 {
    from + ((to - from + 180.0).rem_euclid(360.0) - 180.0) * progress
}

/// The stack's complete ItemStack.CODEC, stored as SNBT rather than losing
/// damage or components in a name/count pair.
mod stack {
    use horizon_client::{command::snbt::parse_fully, items::ItemStack, save, snbt::to_snbt};
    use serde::{Deserialize, Deserializer, Serializer, de::Error};

    pub(super) fn serialize<S: Serializer>(stack: &Option<ItemStack>, serializer: S) -> Result<S::Ok, S::Error> {
        match stack {
            Some(s) => serializer.serialize_some(&to_snbt(&save::item_to_nbt(s))),
            None => serializer.serialize_none(),
        }
    }

    pub(super) fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<ItemStack>, D::Error> {
        let Some(text) = Option::<String>::deserialize(deserializer)? else {
            return Ok(None);
        };
        let tag = parse_fully(&text).map_err(|e| D::Error::custom(e.message.plain()))?;
        save::item_from_nbt(&tag).map(Some).ok_or_else(|| D::Error::custom("an arm's held stack is empty"))
    }
}

/// What the owner saves, including both independent round-robin cursors.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Arm {
    inputs: Vec<Point>,
    outputs: Vec<Point>,
    phase: Phase,
    progress: f32,
    chased: i32,
    #[serde(with = "stack")]
    pub(super) held: Option<ItemStack>,
    last_input: i32,
    last_output: i32,
    previous: Angles,
    previous_base: f32,
    #[serde(skip, default = "lazy_ticks")]
    lazy: u8,
    #[serde(skip)]
    started: bool,
}

fn lazy_ticks() -> u8 { LAZY_TICKS }

impl Default for Arm {
    fn default() -> Self {
        Self {
            inputs: Vec::new(), outputs: Vec::new(), phase: Phase::SearchInputs,
            progress: 0.0, chased: -1, held: None, last_input: -1, last_output: -1,
            previous: Angles::default(), previous_base: 0.0, lazy: LAZY_TICKS, started: false,
        }
    }
}

/// A client's movement and held stack. Points themselves stay with the owner.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Snapshot {
    phase: Phase,
    progress: f32,
    previous: Angles,
    previous_base: f32,
    target: Option<Angles>,
    #[serde(with = "stack")]
    held: Option<ItemStack>,
}

impl Snapshot {
    /// ArmBlockEntity.tickMovementProgress: first retract, then extend;
    /// the base takes the shortest turn throughout and keeps its last angle
    /// while searching. Interpolate the tick's progress for drawing.
    fn angles(&self, speed: f32, alpha: f32) -> Angles {
        let progress = (self.progress - step(speed) * (1.0 - alpha.clamp(0.0, 1.0))).clamp(0.0, 1.0);
        let rest = Angles::default();
        let (from, to) = if progress < 0.5 {
            (self.previous, rest)
        } else {
            (rest, self.target.unwrap_or(rest))
        };
        let p = if progress == 1.0 { 1.0 } else { (progress % 0.5) * 2.0 };
        Angles {
            base: angle_lerp(progress, self.previous_base, self.target.map_or(self.previous_base, |a| a.base)),
            lower: from.lower + (to.lower - from.lower) * p,
            upper: from.upper + (to.upper - from.upper) * p,
            head: angle_lerp(p, from.head % 360.0, to.head % 360.0),
        }
    }
}

/// ArmInteractionPoint's item handler operations. A simulation never changes
/// the handler, and extraction is always from one slot.
trait Transfer {
    fn valid(&self, point: Point) -> bool;
    fn slots(&self, point: Point) -> usize;
    fn extract(&mut self, point: Point, slot: usize, most: u32, simulate: bool) -> Option<ItemStack>;
    fn insert(&mut self, point: Point, stack: ItemStack, simulate: bool) -> Option<ItemStack>;
    fn keep_alive(&mut self, point: Point);
}

impl Arm {
    fn searching(&self) -> bool {
        matches!(self.phase, Phase::SearchInputs | Phase::SearchOutputs)
    }

    fn points(&mut self, mut points: Vec<Point>) -> bool {
        if !self.searching() { return false; }
        points.sort_by_key(|p| p.pos);
        points.dedup_by_key(|p| p.pos);
        let (inputs, outputs): (Vec<_>, Vec<_>) = points.into_iter().partition(|p| p.input);
        if self.inputs == inputs && self.outputs == outputs { return false; }
        self.inputs = inputs;
        self.outputs = outputs;
        self.last_input = -1;
        self.last_output = -1;
        true
    }

    fn target(&self) -> Option<Point> {
        let index = usize::try_from(self.chased).ok()?;
        match self.phase {
            Phase::MoveToInput => self.inputs.get(index).copied(),
            Phase::MoveToOutput => self.outputs.get(index).copied(),
            _ => None,
        }
    }

    pub(super) fn snapshot(&self, pos: [i32; 3]) -> Snapshot {
        Snapshot {
            phase: self.phase, progress: self.progress, previous: self.previous,
            previous_base: self.previous_base, target: self.target().map(|p| Angles::at(pos, p)),
            held: self.held.clone(),
        }
    }

    fn select(&mut self, input: bool, index: usize) {
        self.phase = if input { Phase::MoveToInput } else { Phase::MoveToOutput };
        self.chased = index as i32;
        self.progress = 0.0;
        self.previous = Angles::default();
        if input { self.last_input = index as i32; } else { self.last_output = index as i32; }
    }

    fn distributable(&self, transfer: &mut impl Transfer, point: Point, slot: usize) -> u32 {
        let Some(stack) = transfer.extract(point, slot, STACK, true) else { return 0; };
        let count = stack.count;
        let mut rest = Some(stack);
        for output in &self.outputs {
            if !transfer.valid(*output) { continue; }
            let Some(stack) = rest.take() else { break; };
            rest = transfer.insert(*output, stack, true);
        }
        count.saturating_sub(rest.map_or(0, |s| s.count))
    }

    /// ROUND_ROBIN scans only after the last choice. A failed scan resets
    /// the cursor, but does not wrap and try again in that same search.
    fn search_inputs(&mut self, transfer: &mut impl Transfer) -> bool {
        let before = self.last_input;
        let start = self.last_input.saturating_add(1).max(0) as usize;
        let mut chosen = None;
        for i in start..self.inputs.len() {
            let point = self.inputs[i];
            if transfer.valid(point) && (0..transfer.slots(point)).any(|j| self.distributable(transfer, point, j) > 0) {
                chosen = Some(i);
                break;
            }
        }
        if let Some(i) = chosen { self.select(true, i); } else { self.last_input = -1; }
        if self.last_input == self.inputs.len() as i32 - 1 { self.last_input = -1; }
        chosen.is_some() || before != self.last_input
    }

    fn search_outputs(&mut self, transfer: &mut impl Transfer) -> bool {
        let Some(held) = self.held.as_ref() else { return false; };
        let before = self.last_output;
        let start = self.last_output.saturating_add(1).max(0) as usize;
        let mut chosen = None;
        for i in start..self.outputs.len() {
            let point = self.outputs[i];
            if transfer.valid(point) && transfer.insert(point, held.clone(), true).is_none_or(|r| r.count < held.count) {
                chosen = Some(i);
                break;
            }
        }
        if let Some(i) = chosen { self.select(false, i); } else { self.last_output = -1; }
        if self.last_output == self.outputs.len() as i32 - 1 { self.last_output = -1; }
        chosen.is_some() || before != self.last_output
    }

    fn finish(&mut self, pos: [i32; 3]) {
        self.previous = self.target().map_or_else(Angles::default, |p| Angles::at(pos, p));
        if self.target().is_some() { self.previous_base = self.previous.base; }
        self.phase = if self.held.is_some() { Phase::SearchOutputs } else { Phase::SearchInputs };
        self.progress = 0.0;
        self.chased = -1;
    }

    fn lazy_tick(&mut self, transfer: &mut impl Transfer) -> bool {
        self.progress >= 0.5 && self.phase == Phase::SearchOutputs && self.search_outputs(transfer)
    }

    /// ArmBlockEntity.tick and SmartBlockEntity's lazy counter. A phase or
    /// cursor change asks for an immediate save; progress is saved periodically.
    fn tick(&mut self, pos: [i32; 3], speed: f32, locked: bool, transfer: &mut impl Transfer) -> bool {
        self.progress = if self.progress.is_finite() { self.progress.clamp(0.0, 1.0) } else { 0.0 };
        let mut changed = false;
        if !self.started {
            self.started = true;
            changed |= self.lazy_tick(transfer);
        }
        if self.lazy == 0 {
            self.lazy = LAZY_TICKS;
            changed |= self.lazy_tick(transfer);
        } else {
            self.lazy -= 1;
        }
        let was_reached = self.progress >= 1.0;
        self.progress = (self.progress + step(speed)).min(1.0);
        let reached = !was_reached && self.progress >= 1.0;
        if self.progress < 1.0 {
            if self.phase == Phase::MoveToInput && let Some(point) = self.target() {
                transfer.keep_alive(point);
            }
            return changed;
        }
        match self.phase {
            Phase::SearchInputs if !locked => changed |= self.search_inputs(transfer),
            Phase::MoveToInput => {
                if let Some(point) = self.target().filter(|p| transfer.valid(*p)) {
                    for slot in 0..transfer.slots(point) {
                        let n = self.distributable(transfer, point, slot);
                        if n == 0 { continue; }
                        self.held = transfer.extract(point, slot, n, false);
                        break;
                    }
                }
                self.finish(pos);
                changed = true;
            }
            Phase::MoveToOutput => {
                if let Some(point) = self.target().filter(|p| transfer.valid(*p))
                    && let Some(stack) = self.held.take()
                {
                    self.held = transfer.insert(point, stack, false);
                }
                self.finish(pos);
                changed = true;
            }
            _ => {}
        }
        if reached { changed |= self.lazy_tick(transfer); }
        changed
    }
}

fn kind(part: Option<Part>, container: bool, extracting: bool) -> Option<Kind> {
    match part {
        Some(Part::Depot) => Some(Kind::Depot),
        Some(Part::Belt(_)) => Some(Kind::Belt),
        Some(Part::Basin) => Some(Kind::Basin),
        Some(Part::Funnel(d)) => (!extracting).then(|| Kind::Funnel(d.offset())),
        _ => container.then_some(Kind::Container),
    }
}

fn kind_at(game: &Game<'_>, kinds: &Kinds, pos: [i32; 3]) -> Option<Kind> {
    if !game.world.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4)) { return None; }
    let part = kinds.part(game.world.block(pos[0], pos[1], pos[2]).0);
    let extracting = OWNERS.with_borrow(|owners| owners.get(game.dim.id()).is_some_and(|o| o.funnels_out.contains(&pos)));
    let container = game.tachyon_arm_container_slots((pos[0], pos[1], pos[2])).is_some();
    kind(part, container, extracting)
}

/// DepotItemHandler's main slot precedes the processing outputs, unlike a
/// hopper's take. A press does not protect its input from an arm.
fn depot_extract(mill: &mut Mill, slot: usize, most: u32, simulate: bool) -> Option<ItemStack> {
    let h = if slot == 0 { mill.input.as_ref()? } else { mill.outputs.get(slot - 1)? };
    let n = h.count.min(most);
    if n == 0 { return None; }
    let stack = ItemStack::new(&h.item, n);
    if !simulate {
        if slot == 0 {
            let h = mill.input.as_mut()?;
            h.count -= n;
            if h.count == 0 { mill.input = None; mill.progress = 0.0; }
        } else {
            let h = mill.outputs.get_mut(slot - 1)?;
            h.count -= n;
            if h.count == 0 { mill.outputs.remove(slot - 1); }
        }
    }
    Some(stack)
}

fn on_belt(item: &ItemEntity, pos: [i32; 3]) -> bool {
    let p = item.position;
    item.stack.count > 0 && p.x.floor() as i32 == pos[0] && p.y.floor() as i32 == pos[1]
        && p.z.floor() as i32 == pos[2] && p.y - f64::from(pos[1]) <= 0.75
}

struct Access<'a, 'g> {
    game: &'a mut Game<'g>,
    kinds: Arc<Kinds>,
}

impl Access<'_, '_> {
    fn belt_item(&self, pos: [i32; 3]) -> Option<usize> {
        self.game.dropped.iter().position(|item| on_belt(item, pos))
    }

    fn put_at(&mut self, pos: [i32; 3], mut stack: ItemStack, face: Direction, simulate: bool) -> Option<ItemStack> {
        let part = self.kinds.part(self.game.world.block(pos[0], pos[1], pos[2]).0);
        if !matches!(part, Some(Part::Depot | Part::Basin | Part::Millstone)) {
            return self.game.tachyon_arm_container_put((pos[0], pos[1], pos[2]), stack, face, simulate);
        }
        // These existing machines store names and counts, not components.
        if stack.extra.is_some() || stack.damage != 0 { return Some(stack); }
        let limit = self.game.items().get(&stack.item).map_or(STACK, |i| i.max_stack).clamp(1, STACK);
        let grinds = part != Some(Part::Millstone) || super::millstone::recipe(&stack.item).is_some()
            || self.game.tachyon_create_recipes().milling(&stack.item).is_some();
        let n = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(self.game.dim.id()).or_default();
            if part == Some(Part::Basin) {
                let mut basin = owner.basins.get(&pos).cloned().unwrap_or_default();
                let mut left = stack.count;
                for h in basin.inputs.iter_mut().filter(|h| h.item == &*stack.item) {
                    let n = left.min(limit.saturating_sub(h.count));
                    h.count += n;
                    left -= n;
                }
                while left > 0 && basin.inputs.len() < 9 {
                    let n = left.min(limit);
                    basin.inputs.push(super::millstone::Held { item: stack.item.to_string(), count: n });
                    left -= n;
                }
                let n = stack.count - left;
                if !simulate && n > 0 { owner.basins.insert(pos, basin); owner.mills_changed = true; }
                return n;
            }
            let mut mill = owner.mills.get(&pos).cloned().unwrap_or_default();
            let room = mill.input.as_ref().map_or(limit, |h| {
                if h.item == &*stack.item { limit.saturating_sub(h.count) } else { 0 }
            });
            let fits = grinds && (part != Some(Part::Depot) || mill.outputs.is_empty());
            let n = if fits { mill.put(&stack.item, stack.count.min(room)) } else { 0 };
            if !simulate && n > 0 { owner.mills.insert(pos, mill); owner.mills_changed = true; }
            n
        });
        stack.count -= n;
        (stack.count > 0).then_some(stack)
    }
}

impl Transfer for Access<'_, '_> {
    fn valid(&self, point: Point) -> bool {
        kind_at(self.game, &self.kinds, point.pos) == Some(point.kind)
    }

    fn slots(&self, point: Point) -> usize {
        match point.kind {
            Kind::Depot => OWNERS.with_borrow(|owners| {
                owners.get(self.game.dim.id()).and_then(|o| o.mills.get(&point.pos)).map_or(1, |m| 1 + m.outputs.len())
            }),
            Kind::Belt => usize::from(self.belt_item(point.pos).is_some()),
            Kind::Container => self.game.tachyon_arm_container_slots((point.pos[0], point.pos[1], point.pos[2])).unwrap_or(0),
            _ => 0,
        }
    }

    fn extract(&mut self, point: Point, slot: usize, most: u32, simulate: bool) -> Option<ItemStack> {
        match point.kind {
            Kind::Depot => OWNERS.with_borrow_mut(|owners| {
                let owner = owners.get_mut(self.game.dim.id())?;
                let mill = owner.mills.get_mut(&point.pos)?;
                let stack = depot_extract(mill, slot, most, simulate)?;
                if !simulate { owner.mills_changed = true; }
                Some(stack)
            }),
            Kind::Belt if slot == 0 => {
                let i = self.belt_item(point.pos)?;
                let item = &mut self.game.dropped[i];
                let stack = ItemStack { count: item.stack.count.min(most), ..item.stack.clone() };
                if stack.count == 0 { return None; }
                if !simulate {
                    item.stack.count -= stack.count;
                    if item.stack.count == 0 { self.game.dropped.remove(i); }
                }
                Some(stack)
            }
            Kind::Container => self.game.tachyon_arm_container_take((point.pos[0], point.pos[1], point.pos[2]), slot, most, simulate),
            _ => None,
        }
    }

    fn insert(&mut self, point: Point, mut stack: ItemStack, simulate: bool) -> Option<ItemStack> {
        match point.kind {
            Kind::Belt => {
                let limit = self.game.items().get(&stack.item).map_or(STACK, |i| i.max_stack).clamp(1, STACK);
                let at = self.belt_item(point.pos);
                let room = at.map_or(limit, |i| {
                    let held = &self.game.dropped[i].stack;
                    if held.same_item_same_components(&stack) { limit.saturating_sub(held.count) } else { 0 }
                });
                let n = stack.count.min(room);
                if !simulate && n > 0 {
                    if let Some(i) = at { self.game.dropped[i].stack.count += n; } else {
                        let p = point.pos.map(f64::from);
                        self.game.spawn_item(ItemStack { count: n, ..stack.clone() }, [p[0] + 0.5, p[1] + 0.7, p[2] + 0.5], [0.0; 3], 10);
                    }
                }
                stack.count -= n;
                (stack.count > 0).then_some(stack)
            }
            Kind::Funnel(facing) => {
                let powered = OWNERS.with_borrow(|owners| owners.get(self.game.dim.id()).is_some_and(|o| o.powered.contains(&point.pos)));
                if powered { return Some(stack); }
                let face = super::kinetic::Dir::ALL.into_iter().find(|d| d.offset() == facing).and_then(|d| dir_from_name(d.name()));
                let Some(face) = face else { return Some(stack); };
                let behind = [0, 1, 2].map(|i| point.pos[i] - facing[i]);
                self.put_at(behind, stack, face, simulate)
            }
            _ => self.put_at(point.pos, stack, Direction::Up, simulate),
        }
    }

    fn keep_alive(&mut self, point: Point) {
        if point.kind == Kind::Belt && self.valid(point) && let Some(i) = self.belt_item(point.pos) {
            let velocity = &mut self.game.dropped[i].velocity;
            velocity.x = 0.0;
            velocity.y = 0.0;
            velocity.z = 0.0;
        }
    }
}

/// ArmRenderer's six partial models and the pose at the head. Partial models
/// are authored at the joints; unlike the cog, they are not uncentered here.
fn poses(origin: Vec3, angles: Angles, held: bool, block: bool) -> ([(&'static str, Mat4); 6], Mat4) {
    let base = Mat4::from_translation(origin + Vec3::splat(0.5))
        * Mat4::from_translation(Vec3::new(0.0, 4.0 / 16.0, 0.0))
        * Mat4::from_rotation_y(angles.base.to_radians());
    let lower = base * Mat4::from_translation(Vec3::new(0.0, 2.0 / 16.0, 0.0))
        * Mat4::from_rotation_x(angles.lower.to_radians());
    let upper = lower * Mat4::from_translation(Vec3::new(0.0, 0.0, -14.0 / 16.0))
        * Mat4::from_rotation_x((angles.upper - 180.0).to_radians());
    let head = upper * Mat4::from_translation(Vec3::new(0.0, 0.0, -15.0 / 16.0))
        * Mat4::from_rotation_x((angles.head - 45.0).to_radians());
    let spread = if !held { 1.0 / 16.0 } else if block { 3.0 / 16.0 } else { 5.0 / 64.0 };
    ([
        ("create:block/mechanical_arm/base", base),
        ("create:block/mechanical_arm/lower_body", lower),
        ("create:block/mechanical_arm/upper_body", upper),
        ("create:block/mechanical_arm/claw_base", head),
        ("create:block/mechanical_arm/upper_claw_grip", head * Mat4::from_translation(Vec3::new(0.0, spread, -6.0 / 16.0))),
        ("create:block/mechanical_arm/lower_claw_grip", head * Mat4::from_translation(Vec3::new(0.0, -spread, -6.0 / 16.0))),
    ], head)
}

impl Game<'_> {
    fn tachyon_arm_points(&self, pos: [i32; 3], kinds: &Kinds) -> Option<Vec<Point>> {
        // ArmBlockEntity.initInteractionPoints waits for range + 1, so an
        // unloaded chunk cannot erase saved points.
        let range = RANGE + 1;
        for x in ((pos[0] - range) >> 4)..=((pos[0] + range) >> 4) {
            for z in ((pos[2] - range) >> 4)..=((pos[2] + range) >> 4) {
                if !self.world.chunks.contains_key(&(x, z)) { return None; }
            }
        }
        let mut points = Vec::new();
        for x in -RANGE..=RANGE {
            for y in -RANGE..=RANGE {
                for z in -RANGE..=RANGE {
                    if x * x + y * y + z * z >= RANGE * RANGE || [x, y, z] == [0; 3] { continue; }
                    let at = super::add(pos, [x, y, z]);
                    if let Some(kind) = kind_at(self, kinds, at) && let Some(point) = Point::new(pos, at, kind) {
                        points.push(point);
                    }
                }
            }
        }
        Some(points)
    }

    /// The server's arms, after speeds are solved. Remove each state while
    /// calling handlers, so a depot or basin can borrow the owner's map.
    /// Reinsert it before saving the transfer and the held stack together.
    pub(super) fn tachyon_arms(&mut self) {
        if self.remote.is_some() { return; }
        let Some(kinds) = self.tachyon_kinds() else { return; };
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let (mut arms, drops, ticks) = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            let gone: Vec<_> = owner.arms.keys().filter(|p| {
                owner.index.parts.get(*p) != Some(&Part::Arm)
                    && self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4))
            }).copied().collect();
            let mut drops = Vec::new();
            for p in &gone {
                if let Some(mut arm) = owner.arms.remove(p) && let Some(stack) = arm.held.take() { drops.push((*p, stack)); }
            }
            if !gone.is_empty() { owner.save(&root, dimension); }
            let arms: Vec<_> = owner.index.parts.iter().filter(|(_, p)| **p == Part::Arm).map(|(p, _)| {
                (*p, owner.shown.speeds.get(p).map_or(0.0, |s| s[0]), owner.powered.contains(p))
            }).collect();
            (arms, drops, owner.ticks)
        });
        for (pos, stack) in drops { self.pop_resource(pos, stack); }
        arms.sort_by_key(|(p, _, _)| *p);
        for (pos, speed, locked) in arms {
            let mut arm = OWNERS.with_borrow_mut(|owners| owners.entry(dimension).or_default().arms.remove(&pos).unwrap_or_default());
            let mut save = false;
            if arm.searching() && (!arm.started || ticks % NATURE_TICKS == 1)
                && let Some(points) = self.tachyon_arm_points(pos, &kinds)
            {
                save |= arm.points(points);
            }
            let before = arm.snapshot(pos);
            save |= arm.tick(pos, speed, locked, &mut Access { game: self, kinds: kinds.clone() });
            let changed = before != arm.snapshot(pos);
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                owner.arms.insert(pos, arm);
                owner.mills_changed |= changed;
                if save { owner.save(&root, dimension); }
            });
        }
    }

    pub(super) fn tachyon_render_arm(&self, origin: Vec3, arm: &Snapshot, speed: f32, alpha: f32, light: u32, out: &mut Vec<Vertex>) {
        let item = arm.held.as_ref().map(|stack| self.resolve_item(stack, horizon_assets::item_model::DisplayContext::Fixed, None, 0));
        let block = arm.held.as_ref().is_some_and(|s| self.items().get(&s.item).is_some_and(|i| i.block.is_some()))
            && item.as_ref().is_some_and(|i| i.uses_block_light());
        let (pieces, head) = poses(origin, arm.angles(speed, alpha), arm.held.is_some(), block);
        for (model, pose) in pieces { self.submit_item_world(&self.tachyon_model(model), pose, light, out); }
        if let Some(item) = item {
            let scale = if block { 0.5 } else { 0.625 };
            let pose = head * Mat4::from_rotation_x(std::f32::consts::FRAC_PI_2)
                * Mat4::from_translation(Vec3::new(0.0, -if block { 9.0 / 16.0 } else { 10.0 / 16.0 }, 0.0))
                * Mat4::from_scale(Vec3::splat(scale));
            self.submit_item_world(&item, pose, light, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::{add, kinetic::Dir, millstone::Held};
    use horizon_world::nbt::Tag;
    use std::collections::HashMap;

    const AT: [i32; 3] = [0; 3];

    /// A handler with independent input slots and output space. Simulating
    /// records a call but never consumes an item or fills the output.
    #[derive(Default)]
    struct Handlers {
        inputs: HashMap<[i32; 3], Vec<Option<ItemStack>>>,
        room: HashMap<[i32; 3], u32>,
        taken: Vec<(Point, usize, ItemStack)>,
        put: Vec<(Point, ItemStack)>,
        alive: Vec<Point>,
        simulations: usize,
    }

    impl Transfer for Handlers {
        fn valid(&self, point: Point) -> bool {
            self.inputs.contains_key(&point.pos) || self.room.contains_key(&point.pos)
        }
        fn slots(&self, point: Point) -> usize {
            self.inputs.get(&point.pos).map_or(0, Vec::len)
        }
        fn extract(&mut self, point: Point, slot: usize, most: u32, simulate: bool) -> Option<ItemStack> {
            let cell = self.inputs.get_mut(&point.pos)?.get_mut(slot)?;
            let stack = cell.as_mut()?;
            let out = ItemStack { count: stack.count.min(most), ..stack.clone() };
            if out.count == 0 { return None; }
            if !simulate {
                stack.count -= out.count;
                if stack.count == 0 { *cell = None; }
                self.taken.push((point, slot, out.clone()));
            }
            Some(out)
        }
        fn insert(&mut self, point: Point, mut stack: ItemStack, simulate: bool) -> Option<ItemStack> {
            if simulate { self.simulations += 1; }
            let Some(room) = self.room.get_mut(&point.pos) else { return Some(stack); };
            let n = stack.count.min(*room);
            if !simulate && n > 0 {
                *room -= n;
                self.put.push((point, ItemStack { count: n, ..stack.clone() }));
            }
            stack.count -= n;
            (stack.count > 0).then_some(stack)
        }
        fn keep_alive(&mut self, point: Point) {
            self.alive.push(point);
        }
    }

    fn point(pos: [i32; 3]) -> Point {
        Point::new(AT, pos, Kind::Container).unwrap()
    }

    fn setup(inputs: &[[i32; 3]], outputs: &[[i32; 3]]) -> (Arm, Handlers) {
        let mut arm = Arm::default();
        arm.points(inputs.iter().chain(outputs).copied().map(point).collect());
        let handlers = Handlers {
            inputs: inputs.iter().map(|p| (*p, vec![Some(ItemStack::new("minecraft:stone", 128))])).collect(),
            room: outputs.iter().map(|p| (*p, 256)).collect(),
            ..Handlers::default()
        };
        (arm, handlers)
    }

    fn pair() -> (Arm, Handlers) {
        setup(&[[-1, 0, 0]], &[[1, 0, 0]])
    }

    fn arrive(arm: &mut Arm, handlers: &mut Handlers) {
        arm.progress = 1.0;
        assert!(arm.tick(AT, 0.0, false, handlers));
    }

    #[test]
    fn each_move_takes_1024_over_rpm_ticks_up_to_256() {
        for (speed, ticks) in [(32.0, 32), (64.0, 16), (-128.0, 8), (256.0, 4), (512.0, 4), (3.0, 342)] {
            let (mut arm, mut handlers) = pair();
            for _ in 1..ticks {
                arm.tick(AT, speed, false, &mut handlers);
                assert_eq!(arm.phase, Phase::SearchInputs);
            }
            arm.tick(AT, speed, false, &mut handlers);
            assert_eq!((arm.phase, arm.progress), (Phase::MoveToInput, 0.0));
            assert!(arm.held.is_none());
            assert!(handlers.taken.is_empty());
            for _ in 1..ticks {
                arm.tick(AT, speed, false, &mut handlers);
                assert_eq!(arm.phase, Phase::MoveToInput);
            }
            arm.tick(AT, speed, false, &mut handlers);
            assert_eq!((arm.phase, arm.progress), (Phase::SearchOutputs, 0.0));
            assert_eq!(arm.held.as_ref().unwrap().count, 64);
            assert_eq!(handlers.taken.len(), 1);
            assert_eq!(handlers.alive.len(), ticks as usize - 1);
            assert!(arm.search_outputs(&mut handlers));
            for _ in 1..ticks {
                arm.tick(AT, speed, false, &mut handlers);
                assert_eq!(arm.phase, Phase::MoveToOutput);
                assert!(handlers.put.is_empty());
            }
            arm.tick(AT, speed, false, &mut handlers);
            assert_eq!((arm.phase, arm.progress), (Phase::SearchInputs, 0.0));
            assert!(arm.held.is_none());
            assert_eq!(handlers.put.len(), 1);
            assert_eq!(handlers.put[0].1.count, 64);
        }
    }

    #[test]
    fn a_stopped_arm_keeps_its_approach_and_the_input_alive() {
        let (mut arm, mut handlers) = pair();
        arm.select(true, 0);
        arm.progress = 0.25;
        for speed in [0.0, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(!arm.tick(AT, speed, false, &mut handlers));
            assert_eq!((arm.phase, arm.progress), (Phase::MoveToInput, 0.25));
        }
        assert_eq!(handlers.alive, [point([-1, 0, 0]); 4]);
        assert!(handlers.taken.is_empty());
    }

    #[test]
    fn redstone_stops_new_pickups_but_not_the_stack_in_flight() {
        let (mut arm, mut handlers) = pair();
        arm.progress = 1.0;
        arm.tick(AT, 256.0, true, &mut handlers);
        assert_eq!(arm.phase, Phase::SearchInputs);
        arm.tick(AT, 256.0, false, &mut handlers);
        assert_eq!(arm.phase, Phase::MoveToInput);
        for _ in 0..4 { arm.tick(AT, 256.0, true, &mut handlers); }
        assert_eq!(arm.phase, Phase::SearchOutputs);
        assert!(arm.search_outputs(&mut handlers));
        for _ in 0..4 { arm.tick(AT, 256.0, true, &mut handlers); }
        assert_eq!(arm.phase, Phase::SearchInputs);
        assert!(arm.held.is_none());
        for _ in 0..8 { arm.tick(AT, 256.0, true, &mut handlers); }
        assert_eq!(arm.phase, Phase::SearchInputs);
        assert_eq!(handlers.taken.len(), 1);
    }

    #[test]
    fn roles_are_fixed_by_x_then_z_inside_creates_strict_range() {
        let origin = [10, 64, -10];
        for kind in [Kind::Container, Kind::Depot, Kind::Belt] {
            for (offset, input) in [([-1, 0, 0], true), ([0, 0, -1], true), ([1, 0, 0], false), ([0, 0, 1], false), ([0, 4, 0], false)] {
                let p = Point::new(origin, add(origin, offset), kind).unwrap();
                assert_eq!(p.input, input);
            }
        }
        for kind in [Kind::Basin, Kind::Funnel([0, 0, 1])] {
            assert!(!Point::new(origin, [9, 64, -10], kind).unwrap().input);
        }
        for offset in [[0; 3], [5, 0, 0], [-5, 0, 0], [3, 0, 4], [3, 3, 3]] {
            assert!(Point::new(origin, add(origin, offset), Kind::Container).is_none());
        }
        assert!(Point::new(AT, [-3, 2, -3], Kind::Depot).is_some());
        assert!(Point::new([i32::MIN; 3], [i32::MAX; 3], Kind::Container).is_none());
        assert_eq!(kind(Some(Part::Funnel(Dir::North)), true, true), None);
        assert_eq!(kind(Some(Part::Funnel(Dir::North)), false, false), Some(Kind::Funnel([0, 0, -1])));
        assert_eq!(kind(Some(Part::Depot), true, false), Some(Kind::Depot));
        assert_eq!(kind(None, true, false), Some(Kind::Container));
        assert_eq!(kind(None, false, false), None);
    }

    #[test]
    fn points_are_sorted_saved_and_not_replaced_mid_move() {
        let mut arm = Arm::default();
        let points = vec![point([2, 0, 0]), point([-1, 1, 0]), point([-2, 0, 0]), point([1, 0, 0]), point([-1, 0, 1]), point([-2, 0, 0])];
        assert!(arm.points(points.clone()));
        assert_eq!(arm.inputs.iter().map(|p| p.pos).collect::<Vec<_>>(), [[-2, 0, 0], [-1, 0, 1], [-1, 1, 0]]);
        assert_eq!(arm.outputs.iter().map(|p| p.pos).collect::<Vec<_>>(), [[1, 0, 0], [2, 0, 0]]);
        arm.last_input = 1;
        arm.last_output = 0;
        let mut points = points;
        points.reverse();
        assert!(!arm.points(points));
        assert_eq!((arm.last_input, arm.last_output), (1, 0));
        arm.select(true, 1);
        let target = arm.target();
        assert!(!arm.points(Vec::new()));
        assert_eq!(arm.target(), target);
        arm.finish(AT);
        assert!(arm.points(Vec::new()));
        assert_eq!((arm.last_input, arm.last_output), (-1, -1));
    }

    #[test]
    fn input_and_output_round_robin_have_independent_cursors() {
        let (mut arm, mut handlers) = setup(&[[-3, 0, 0], [-2, 0, 0], [-1, 0, 0]], &[[1, 0, 0], [2, 0, 0], [3, 0, 0]]);
        arm.last_output = 1;
        for (input, output) in [(0, 2), (1, 0), (2, 1), (0, 2)] {
            let before = arm.last_output;
            assert!(arm.search_inputs(&mut handlers));
            assert_eq!(arm.chased, input);
            assert_eq!(arm.last_output, before);
            arrive(&mut arm, &mut handlers);
            let before = arm.last_input;
            assert!(arm.search_outputs(&mut handlers));
            assert_eq!(arm.chased, output);
            assert_eq!(arm.last_input, before);
            arrive(&mut arm, &mut handlers);
            assert_eq!(arm.phase, Phase::SearchInputs);
            assert!(arm.held.is_none());
        }
        assert_eq!(handlers.taken.iter().map(|(p, _, _)| p.pos[0]).collect::<Vec<_>>(), [-3, -2, -1, -3]);
        assert_eq!(handlers.put.iter().map(|(p, _)| p.pos[0]).collect::<Vec<_>>(), [3, 1, 2, 3]);
    }

    #[test]
    fn a_failed_round_robin_scan_wraps_on_the_next_search_only() {
        let (mut arm, mut handlers) = setup(&[[-2, 0, 0], [-1, 0, 0]], &[[1, 0, 0], [2, 0, 0]]);
        handlers.inputs.remove(&[-1, 0, 0]);
        handlers.room.insert([2, 0, 0], 0);
        arm.last_input = 0;
        assert!(arm.search_inputs(&mut handlers));
        assert_eq!((arm.phase, arm.chased, arm.last_input), (Phase::SearchInputs, -1, -1));
        assert!(arm.search_inputs(&mut handlers));
        assert_eq!(arm.chased, 0);
        arrive(&mut arm, &mut handlers);
        arm.last_output = 0;
        assert!(arm.search_outputs(&mut handlers));
        assert_eq!((arm.phase, arm.chased, arm.last_output), (Phase::SearchOutputs, -1, -1));
        assert!(arm.search_outputs(&mut handlers));
        assert_eq!(arm.chased, 0);
    }

    #[test]
    fn pickup_rechecks_all_output_space_and_takes_from_one_slot() {
        let (mut arm, mut handlers) = setup(&[[-1, 0, 0]], &[[1, 0, 0], [2, 0, 0]]);
        handlers.inputs.get_mut(&[-1, 0, 0]).unwrap().push(Some(ItemStack::new("minecraft:stone", 64)));
        handlers.room = HashMap::from([([1, 0, 0], 5), ([2, 0, 0], 7)]);
        let before = handlers.inputs.clone();
        assert!(arm.search_inputs(&mut handlers));
        assert_eq!(handlers.inputs, before);
        assert_eq!(handlers.room, HashMap::from([([1, 0, 0], 5), ([2, 0, 0], 7)]));
        handlers.room = HashMap::from([([1, 0, 0], 2), ([2, 0, 0], 3)]);
        arrive(&mut arm, &mut handlers);
        assert_eq!(arm.held.as_ref().unwrap().count, 5);
        assert_eq!(handlers.inputs[&[-1, 0, 0]][0].as_ref().unwrap().count, 123);
        assert_eq!(handlers.inputs[&[-1, 0, 0]][1].as_ref().unwrap().count, 64);
        assert_eq!(handlers.taken.len(), 1);
        assert_eq!(handlers.taken[0].1, 0);
        assert!(arm.search_outputs(&mut handlers));
        arrive(&mut arm, &mut handlers);
        assert_eq!(arm.phase, Phase::SearchOutputs);
        assert_eq!(arm.held.as_ref().unwrap().count, 3);
        assert!(arm.search_outputs(&mut handlers));
        assert_eq!(arm.chased, 1);
        arrive(&mut arm, &mut handlers);
        assert!(arm.held.is_none());
        assert_eq!(arm.phase, Phase::SearchInputs);
        assert_eq!(handlers.put.iter().map(|(_, s)| s.count).collect::<Vec<_>>(), [2, 3]);
    }

    #[test]
    fn full_outputs_and_missing_points_do_not_consume_or_lose_items() {
        let (mut arm, mut handlers) = pair();
        handlers.room.insert([1, 0, 0], 0);
        assert!(!arm.search_inputs(&mut handlers));
        assert_eq!(arm.phase, Phase::SearchInputs);
        handlers.room.insert([1, 0, 0], 64);
        assert!(arm.search_inputs(&mut handlers));
        handlers.room.insert([1, 0, 0], 0);
        arrive(&mut arm, &mut handlers);
        assert!(arm.held.is_none());
        assert!(handlers.taken.is_empty());
        handlers.room.insert([1, 0, 0], 64);
        assert!(arm.search_inputs(&mut handlers));
        let source = handlers.inputs.remove(&[-1, 0, 0]).unwrap();
        arrive(&mut arm, &mut handlers);
        assert_eq!(arm.phase, Phase::SearchInputs);
        assert!(handlers.taken.is_empty());
        handlers.inputs.insert([-1, 0, 0], source);
        assert!(arm.search_inputs(&mut handlers));
        arrive(&mut arm, &mut handlers);
        let held = arm.held.clone();
        assert!(arm.search_outputs(&mut handlers));
        handlers.room.remove(&[1, 0, 0]);
        arrive(&mut arm, &mut handlers);
        assert_eq!(arm.phase, Phase::SearchOutputs);
        assert_eq!(arm.held, held);
        assert!(handlers.put.is_empty());
    }

    #[test]
    fn lazy_searches_run_before_movement_and_require_halfway_progress() {
        let (mut arm, mut handlers) = pair();
        arm.phase = Phase::SearchOutputs;
        arm.held = Some(ItemStack::new("minecraft:stone", 8));
        arm.progress = 0.375;
        arm.started = true;
        arm.lazy = 0;
        arm.tick(AT, 128.0, false, &mut handlers);
        assert_eq!((arm.phase, arm.progress), (Phase::SearchOutputs, 0.5));
        assert_eq!(handlers.simulations, 0);
        arm.lazy = 0;
        arm.tick(AT, 128.0, false, &mut handlers);
        assert_eq!((arm.phase, arm.progress), (Phase::MoveToOutput, 0.125));
        assert_eq!(handlers.simulations, 1);
        arm.phase = Phase::SearchOutputs;
        arm.chased = -1;
        arm.progress = 0.5;
        arm.started = false;
        arm.lazy = LAZY_TICKS;
        arm.tick(AT, 128.0, false, &mut handlers);
        assert_eq!((arm.phase, arm.progress), (Phase::MoveToOutput, 0.125));
        assert_eq!(handlers.simulations, 2);
    }

    #[test]
    fn the_lazy_counter_is_reset_to_ten_and_post_decremented() {
        let (mut arm, mut handlers) = pair();
        arm.phase = Phase::SearchOutputs;
        arm.progress = 1.0;
        arm.held = Some(ItemStack::new("minecraft:stone", 8));
        arm.started = true;
        handlers.room.insert([1, 0, 0], 0);
        for scans in 0..2 {
            for _ in 0..10 { arm.tick(AT, 0.0, false, &mut handlers); }
            assert_eq!(arm.lazy, 0);
            assert_eq!(handlers.simulations, scans);
            arm.tick(AT, 0.0, false, &mut handlers);
            assert_eq!(arm.lazy, LAZY_TICKS);
            assert_eq!(handlers.simulations, scans + 1);
        }
    }

    #[test]
    fn reaching_rest_runs_a_destination_search_without_waiting_for_lazy() {
        let (mut arm, mut handlers) = pair();
        arm.phase = Phase::SearchOutputs;
        arm.progress = 0.75;
        arm.held = Some(ItemStack::new("minecraft:stone", 8));
        arm.started = true;
        arm.tick(AT, 256.0, false, &mut handlers);
        assert_eq!((arm.phase, arm.progress), (Phase::MoveToOutput, 0.0));
        assert_eq!(handlers.simulations, 1);
    }

    #[test]
    fn depot_extraction_keeps_main_and_processing_slots_separate() {
        let mut mill = Mill {
            input: Some(Held { item: "minecraft:iron_ingot".into(), count: 5 }),
            progress: 17.0,
            outputs: vec![Held { item: "create:iron_sheet".into(), count: 2 }],
        };
        let before = mill.clone();
        assert_eq!(depot_extract(&mut mill, 0, 4, true), Some(ItemStack::new("minecraft:iron_ingot", 4)));
        assert_eq!(mill, before);
        assert_eq!(depot_extract(&mut mill, 1, 64, true), Some(ItemStack::new("create:iron_sheet", 2)));
        assert_eq!(mill, before);
        depot_extract(&mut mill, 0, 4, false).unwrap();
        assert_eq!(mill.input.as_ref().unwrap().count, 1);
        assert_eq!(mill.progress, 17.0);
        depot_extract(&mut mill, 0, 64, false).unwrap();
        assert!(mill.input.is_none());
        assert_eq!(mill.progress, 0.0);
        depot_extract(&mut mill, 1, 64, false).unwrap();
        assert!(mill.outputs.is_empty());
    }

    #[test]
    fn points_phase_cursors_and_complete_held_stack_survive_saving() {
        let (mut arm, _) = pair();
        let mut stack = ItemStack::new("minecraft:diamond_pickaxe", 1);
        stack.damage = 7;
        stack.enchant("minecraft:unbreaking", 3);
        stack.data_mut().custom_name = Some(Tag::String("Saved claw".into()));
        stack.data_mut().other.push(("minecraft:custom_data".into(), Tag::compound([("bytes", Tag::ByteArray(vec![u8::MAX, 0, 127]))])));
        arm.held = Some(stack);
        arm.select(false, 0);
        arm.progress = 0.375;
        arm.last_input = 2;
        arm.last_output = 3;
        arm.previous_base = 270.0;
        arm.lazy = 0;
        arm.started = true;
        let json = serde_json::to_value(&arm).unwrap();
        assert!(json["held"].is_string());
        assert!(json.get("lazy").is_none());
        assert!(json.get("started").is_none());
        let loaded: Arm = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(loaded.snapshot(AT), arm.snapshot(AT));
        assert_eq!(loaded.inputs, arm.inputs);
        assert_eq!(loaded.outputs, arm.outputs);
        assert_eq!((loaded.last_input, loaded.last_output), (2, 3));
        assert_eq!(loaded.lazy, LAZY_TICKS);
        assert!(!loaded.started);
        assert_eq!(serde_json::to_value(loaded).unwrap(), json);
        let empty: Arm = serde_json::from_str("{}").unwrap();
        assert_eq!(empty.snapshot(AT), Arm::default().snapshot(AT));
    }

    #[test]
    fn malformed_held_stack_reports_the_parser_message() {
        let text = "{} trailing";
        let message = horizon_client::command::snbt::parse_fully(text).unwrap_err().message.plain();
        let error = serde_json::from_value::<Arm>(serde_json::json!({"held": text})).unwrap_err();
        assert_eq!(error.to_string(), message);
    }

    #[test]
    fn the_pose_retracts_then_extends_and_the_base_takes_the_short_turn() {
        let mut snapshot = Snapshot {
            phase: Phase::MoveToOutput,
            progress: 0.25,
            previous: Angles { base: 350.0, lower: 90.0, upper: 30.0, head: -90.0 },
            previous_base: 350.0,
            target: Some(Angles { base: 10.0, lower: 60.0, upper: 120.0, head: 90.0 }),
            held: None,
        };
        assert_eq!(snapshot.angles(0.0, 1.0), Angles { base: 355.0, lower: 112.5, upper: 37.5, head: -45.0 });
        snapshot.progress = 0.5;
        assert_eq!(snapshot.angles(0.0, 1.0), Angles { base: 360.0, ..Angles::default() });
        assert_eq!(snapshot.angles(256.0, 0.0), Angles { base: 355.0, lower: 112.5, upper: 37.5, head: -45.0 });
        snapshot.progress = 0.75;
        assert_eq!(snapshot.angles(0.0, 1.0), Angles { base: 365.0, lower: 97.5, upper: 82.5, head: 45.0 });
        snapshot.progress = 1.0;
        assert_eq!(snapshot.angles(0.0, 1.0), Angles { base: 370.0, lower: 60.0, upper: 120.0, head: 90.0 });
        snapshot.target = None;
        assert_eq!(snapshot.angles(0.0, 1.0), Angles { base: 350.0, ..Angles::default() });
        for (pos, base) in [([-1, 0, 0], 90.0), ([1, 0, 0], 270.0), ([0, 0, 1], 180.0)] {
            let angles = Angles::at(AT, point(pos));
            assert_eq!(angles.base, base);
            assert!([angles.lower, angles.upper, angles.head].iter().all(|a| a.is_finite()));
        }
    }

    #[test]
    fn partial_models_follow_creates_joint_origins_and_claw_spacing() {
        let origin = Vec3::new(4.0, 2.0, -3.0);
        let angles = Angles { base: 0.0, lower: 0.0, upper: 180.0, head: 45.0 };
        let (pieces, head) = poses(origin, angles, false, false);
        assert_eq!(pieces.map(|p| p.0), [
            "create:block/mechanical_arm/base", "create:block/mechanical_arm/lower_body",
            "create:block/mechanical_arm/upper_body", "create:block/mechanical_arm/claw_base",
            "create:block/mechanical_arm/upper_claw_grip", "create:block/mechanical_arm/lower_claw_grip",
        ]);
        assert_eq!(pieces[0].1.transform_point3(Vec3::ZERO), origin + Vec3::new(0.5, 0.75, 0.5));
        assert_eq!(pieces[1].1.transform_point3(Vec3::ZERO), origin + Vec3::new(0.5, 0.875, 0.5));
        assert_eq!(pieces[2].1.transform_point3(Vec3::ZERO), origin + Vec3::new(0.5, 0.875, -0.375));
        assert_eq!(head.transform_point3(Vec3::ZERO), origin + Vec3::new(0.5, 0.875, -1.3125));
        for (held, block, spread) in [(false, false, 1.0 / 16.0), (true, false, 5.0 / 64.0), (true, true, 3.0 / 16.0)] {
            let (pieces, head) = poses(origin, angles, held, block);
            let at = head.transform_point3(Vec3::ZERO);
            assert_eq!(pieces[4].1.transform_point3(Vec3::ZERO) - at, Vec3::new(0.0, spread, -6.0 / 16.0));
            assert_eq!(pieces[5].1.transform_point3(Vec3::ZERO) - at, Vec3::new(0.0, -spread, -6.0 / 16.0));
        }
    }
}
