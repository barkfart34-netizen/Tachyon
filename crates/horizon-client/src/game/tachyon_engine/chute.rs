//! Create's chute (`ChuteBlock`, `AbstractChuteBlock`, `ChuteBlockEntity`,
//! `ChuteItemHandler`; MIT, THIRD_PARTY_NOTICES.md): one stack moves under
//! gravity, or with a fan's push and pull through connected chutes. It
//! waits halfway when its output is blocked, and gives only past the end.
//! The owner saves the stack with its components and its position.
use super::super::Game;
use super::kinetic::{Axis, ChuteShape, Dir, Part};
use super::{OWNERS, add};
use horizon_client::items::ItemStack;
use horizon_core::{
    geometry::{Aabb, Direction, Vec3},
    position::BlockPos,
};
use horizon_world::{
    level::{Behaviours, LevelAccess, PlaceContext, PlaceOutcome, Ticks, flags},
    registry::{BlockRegistry, BlockStateId},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Create's `ChuteBlockEntity.getItemMotion`: chutes per tick, with
/// gravity only when the combined fan speed is zero or downward.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Air {
    pub(super) push: f32,
    pub(super) pull: f32,
}

impl Air {
    pub(super) fn motion(self) -> f32 {
        let motion = (self.push + self.pull) / 64.0;
        (motion.clamp(-20.0, 20.0) + if motion <= 0.0 { -4.0 } else { 0.0 }) / 20.0
    }
    /// `ChuteBlockEntity.updateAirFlow`: an upper fan can also take items
    /// from the space below the bottom chute.
    fn bottom_reach(self) -> f32 {
        let speed = self.pull - self.push;
        if speed >= 128.0 {
            3.0
        } else if speed >= 64.0 {
            2.0
        } else {
            (speed / 32.0).clamp(0.0, 1.0)
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Chute {
    #[serde(with = "stack_nbt")]
    pub(super) item: Option<ItemStack>,
    pub(super) position: f32,
    pub(super) air: Air,
    pub(super) bottom_pull_distance: f32,
    #[serde(skip)]
    pub(super) previous: f32,
    #[serde(skip)]
    entity_search_cooldown: i32,
}

/// The inventory ends of a chute. Giving is first simulated at the
/// midpoint, then performed only past 0 (down) or 1 (up).
trait Transfer {
    fn take(&mut self, up: bool, most: u32) -> Option<ItemStack>;
    fn give(&mut self, item: &ItemStack, up: bool, simulate: bool) -> Option<ItemStack>;
}

impl Chute {
    fn accept(&mut self, stack: ItemStack, position: f32) -> bool {
        if self.item.is_some() || stack.count == 0 {
            return false;
        }
        self.item = Some(stack);
        self.position = position;
        self.previous = position;
        true
    }
    /// `ChuteItemHandler.insertItem`: no merging, even with the same
    /// item; a single insertion is limited to the item's stack size.
    fn insert(&mut self, mut stack: ItemStack, most: u32) -> Option<ItemStack> {
        if self.item.is_some() || stack.count == 0 {
            return Some(stack);
        }
        let count = stack.count.min(most.max(1));
        let position = if self.air.motion() < 0.0 { 1.0 } else { 0.0 };
        self.accept(ItemStack { count, ..stack.clone() }, position);
        stack.count -= count;
        (stack.count > 0).then_some(stack)
    }
    /// `ChuteItemHandler.extractItem` resets the remaining stack's
    /// position, as `ChuteBlockEntity.setItem` does.
    fn extract(&mut self, most: u32) -> Option<ItemStack> {
        if most == 0 {
            return None;
        }
        let current = self.item.as_mut()?;
        let count = current.count.min(most);
        let out = ItemStack { count, ..current.clone() };
        current.count -= count;
        if current.count == 0 {
            self.item = None;
        }
        self.position = if self.air.motion() < 0.0 { 1.0 } else { 0.0 };
        self.previous = self.position;
        Some(out)
    }
    /// `ChuteBlockEntity.tickAirStreams`: an empty chute searches, then
    /// waits five ticks. Even zero pull reach has a small search area.
    fn search_due(&mut self) -> bool {
        let due = self.entity_search_cooldown <= 0;
        self.entity_search_cooldown = self.entity_search_cooldown.saturating_sub(1);
        if due && self.item.is_none() {
            self.entity_search_cooldown = 5;
            self.air.motion() > 0.0
        } else {
            false
        }
    }
    /// `ChuteBlockEntity.tick`: an empty chute extracts up to 16 and
    /// returns that tick. Partial output keeps the rest at its old offset.
    fn tick(&mut self, ends: &mut impl Transfer) {
        self.previous = self.position;
        let motion = self.air.motion();
        let up = motion > 0.0;
        let Some(item) = self.item.as_ref() else {
            if motion != 0.0
                && let Some(stack) = ends.take(up, 16)
            {
                self.accept(stack, if up { 0.0 } else { 1.0 });
            }
            return;
        };
        let mut next = self.position + motion;
        if (!up && next < 0.5) || (up && next > 0.5) {
            let remainder = ends.give(item, up, true);
            if remainder.as_ref().is_some_and(|r| r.count == item.count) {
                next = 0.5;
            } else if (!up && next < 0.0) || (up && next > 1.0) {
                self.item = ends.give(item, up, false);
                next = self.position;
            }
        }
        self.position = next;
    }
}

/// `ChuteBlockEntity.read` starts interpolation at the saved height. Read
/// the owner's rows separately: one bad stack must not empty every chute.
pub(super) fn read(value: &serde_json::Value) -> HashMap<[i32; 3], Chute> {
    let Some(rows) = value.as_array() else { return HashMap::new() };
    rows.iter()
        .filter_map(|row| {
            let (pos, mut chute) = serde_json::from_value::<([i32; 3], Chute)>(row.clone()).ok()?;
            chute.previous = chute.position;
            Some((pos, chute))
        })
        .collect()
}

/// Stacks have no serde form. The world's item codec preserves damage,
/// names and other components in the owner's JSON file as NBT bytes.
mod stack_nbt {
    use horizon_items::{item_nbt, items::ItemStack};
    use horizon_world::nbt::{self, Limits};
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub(super) fn serialize<S: Serializer>(
        item: &Option<ItemStack>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let bytes = item
            .as_ref()
            .map(|s| nbt::write_network(&item_nbt::item_to_nbt(s), Limits::default()))
            .transpose()
            .map_err(serde::ser::Error::custom)?;
        bytes.serialize(serializer)
    }
    pub(super) fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<ItemStack>, D::Error> {
        let Some(bytes) = Option::<Vec<u8>>::deserialize(deserializer)? else {
            return Ok(None);
        };
        let (tag, used) =
            nbt::read_network(&bytes, Limits::default()).map_err(serde::de::Error::custom)?;
        if used != bytes.len() {
            return Err(serde::de::Error::custom("trailing chute item bytes"));
        }
        item_nbt::item_from_nbt(&tag)
            .map(Some)
            .ok_or_else(|| serde::de::Error::custom("invalid chute item"))
    }
}

fn open(facing: Dir, shape: ChuteShape) -> bool {
    facing == Dir::Down || shape == ChuteShape::Intersection
}

/// `ChuteBlockEntity.getTargetChute`: a diagonal chute gives to the
/// block below and opposite its facing, not the block directly below.
fn target(pos: [i32; 3], facing: Dir) -> [i32; 3] {
    let below = add(pos, Dir::Down.offset());
    if facing.axis() == Axis::Y { below } else { add(below, facing.opposite().offset()) }
}

// getInputChute reverses Iterate.directions, skipping its first (down).
const INPUT_FACINGS: [Dir; 5] = [Dir::Down, Dir::South, Dir::North, Dir::East, Dir::West];

fn input_pos(pos: [i32; 3], facing: Dir) -> [i32; 3] {
    let above = add(pos, Dir::Up.offset());
    if facing == Dir::Down { above } else { add(above, facing.offset()) }
}

fn neighbours(pos: [i32; 3], facing: Dir) -> impl Iterator<Item = [i32; 3]> {
    std::iter::once(target(pos, facing))
        .chain(INPUT_FACINGS.into_iter().map(move |d| input_pos(pos, d)))
}

fn inputs(parts: &HashMap<[i32; 3], Part>, pos: [i32; 3]) -> Vec<[i32; 3]> {
    INPUT_FACINGS
        .into_iter()
        .filter_map(|facing| {
            let at = input_pos(pos, facing);
            matches!(parts.get(&at), Some(Part::Chute(d, _)) if *d == facing).then_some(at)
        })
        .collect()
}

/// `ChuteBlockEntity.calculatePull/calculatePush`: pull adds the upper
/// branches and travels downward; push travels upward, divided among the
/// inputs. Feeds are the first chute reached by each vertical fan's air.
pub(super) fn airflow(
    parts: &HashMap<[i32; 3], Part>,
    feeds: &[([i32; 3], bool, f32)],
) -> HashMap<[i32; 3], Air> {
    let mut air = HashMap::<[i32; 3], Air>::new();
    let mut pulls = HashMap::new();
    let mut pushes = HashMap::new();
    for &(pos, above, speed) in feeds {
        let direct = if above { &mut pulls } else { &mut pushes };
        *direct.entry(pos).or_insert(0.0) += speed;
    }
    let mut order: Vec<[i32; 3]> = parts.keys().copied().collect();
    order.sort_by_key(|p| (p[1], p[0], p[2]));
    for &pos in order.iter().rev() {
        let pull = pulls.get(&pos).copied().unwrap_or_else(|| {
            inputs(parts, pos).iter().map(|p| air.get(p).map_or(0.0, |a| a.pull)).sum()
        });
        air.entry(pos).or_default().pull = pull;
    }
    for pos in order {
        let Some(Part::Chute(facing, _)) = parts.get(&pos) else { continue };
        let below = target(pos, *facing);
        let push = pushes.get(&pos).copied().unwrap_or_else(|| {
            air.get(&below).map_or(0.0, |a| a.push / inputs(parts, below).len().max(1) as f32)
        });
        air.entry(pos).or_default().push = push;
    }
    air
}

fn kind(registry: &BlockRegistry, state: BlockStateId) -> Option<Part> {
    let state = registry.state(state)?;
    super::part_of(&state.name, |k| state.properties.get(k).map(String::as_str))
}

/// `ChuteBlock.updateChuteState`: exposed ends are intersections;
/// straight diagonal runs are normal, or keep their iron casing.
fn updated(pos: [i32; 3], part: Part, at: impl Fn([i32; 3]) -> Option<Part>) -> Part {
    let Part::Chute(facing, shape) = part else { return part };
    if facing != Dir::Down && !matches!(at(target(pos, facing)), Some(Part::Chute(_, _))) {
        return Part::Chute(Dir::Down, ChuteShape::Normal);
    }
    let connections: Vec<Dir> = [Dir::North, Dir::South, Dir::West, Dir::East]
        .into_iter()
        .filter(|d| matches!(at(add(add(pos, Dir::Up.offset()), d.offset())), Some(Part::Chute(f, _)) if f == *d))
        .collect();
    let next = if facing == Dir::Down {
        if !connections.is_empty() {
            ChuteShape::Intersection
        } else if shape == ChuteShape::Intersection {
            ChuteShape::Normal
        } else {
            shape
        }
    } else if connections == [facing]
        && !matches!(
            at(add(pos, Dir::Up.offset())),
            Some(Part::Chute(Dir::Down, _) | Part::Funnel(Dir::Down))
        )
    {
        if shape == ChuteShape::Encased { shape } else { ChuteShape::Normal }
    } else {
        ChuteShape::Intersection
    };
    Part::Chute(facing, next)
}

fn with_part(registry: &BlockRegistry, state: BlockStateId, part: Part) -> BlockStateId {
    let Some(current) = registry.state(state) else { return state };
    let Part::Chute(facing, shape) = part else { return state };
    let mut next = current.clone();
    next.properties
        .insert("half".into(), if facing == Dir::Down { "bottom" } else { "top" }.into());
    if facing != Dir::Down {
        next.properties.insert("facing".into(), facing.name().into());
    }
    next.properties.insert("shape".into(), shape.property().into());
    registry.id(&next).unwrap_or(state)
}

/// `ChuteBlock.getStateForPlacement`, using stair properties only as
/// storage. The template's stair behaviour is removed by mod_content.rs.
pub(in crate::game) fn place<L: LevelAccess + ?Sized>(
    registry: &BlockRegistry,
    b: &Behaviours,
    level: &mut L,
    ticks: &mut Ticks,
    ctx: &PlaceContext,
) -> PlaceOutcome {
    if ctx.item != "create:chute" {
        return b.place_block(level, ticks, ctx);
    }
    let Some((proposed, _)) = b.placement_state(level, ticks, ctx) else {
        return PlaceOutcome::Failed;
    };
    let o = ctx.face.normal();
    let face = Dir::ALL.into_iter().find(|d| d.offset() == o).unwrap_or(Dir::Down);
    let facing = if !ctx.sneaking && face.axis() != Axis::Y { face } else { Dir::Down };
    let p = [proposed.x, proposed.y, proposed.z];
    if neighbours(p, facing).any(|q| !level.is_loaded(BlockPos::new(q[0], q[1], q[2]))) {
        return PlaceOutcome::Failed;
    }
    // ChuteBlock.canSurvive: no chute directly below a diagonal chute.
    if matches!(kind(registry, level.block_state(proposed.offset(0, 1, 0))), Some(Part::Chute(d, _)) if d != Dir::Down)
    {
        return PlaceOutcome::Failed;
    }
    let before = level.block_state(proposed);
    let outcome = b.place_block(level, ticks, ctx);
    let PlaceOutcome::Placed { pos, state } = outcome else { return outcome };
    let p = [pos.x, pos.y, pos.z];
    let part = updated(p, Part::Chute(facing, ChuteShape::Normal), |q| {
        kind(registry, level.block_state(BlockPos::new(q[0], q[1], q[2])))
    });
    let mut placed = with_part(registry, state, part);
    if let Some(old) = registry.state(before)
        && (matches!(old.name.as_str(), "minecraft:water" | "minecraft:bubble_column")
            || old.properties.get("waterlogged").is_some_and(|v| v == "true"))
        && let Some(current) = registry.state(placed)
    {
        let mut wet = current.clone();
        wet.properties.insert("waterlogged".into(), "true".into());
        placed = registry.id(&wet).unwrap_or(placed);
    }
    if placed != state {
        b.set_block(level, ticks, pos, placed, flags::UPDATE_ALL);
    }
    PlaceOutcome::Placed { pos, state: placed }
}

/// `ChuteBlockEntity.handleDownwardOutput/handleUpwardOutput`.
fn ejection(pos: [i32; 3], up: bool, motion: f32) -> ([f64; 3], [f64; 3]) {
    let [x, y, z] = pos.map(f64::from);
    if up {
        ([x + 0.5, y + 1.0, z + 0.5], [0.0, f64::from(motion) * 2.0, 0.0])
    } else {
        ([x + 0.5, y - 0.25, z + 0.5], [0.0, -0.25, 0.0])
    }
}

/// `ChuteBlockEntity.findEntities`: the vertical pull segment, inflated
/// by .45, including the area around its lower mouth when reach is zero.
fn search_area(pos: [i32; 3], reach: f32) -> Aabb {
    let [x, y, z] = pos.map(f64::from);
    let radius = f64::from(0.45f32);
    Aabb::new(Vec3::new(x + 0.5, y - f64::from(reach), z + 0.5), Vec3::new(x + 0.5, y, z + 0.5))
        .inflate(Vec3::new(radius, radius, radius))
}

/// `AbstractChuteBlock.updateEntityAfterFallOn`: the open mouth's centre.
/// The hollow template doesn't stop a central item, so also catch a stack
/// whose last position was above the lip before this tick's fall.
fn falling_at_top(pos: [i32; 3], bounds: Aabb, previous_y: f64, velocity_y: f64) -> bool {
    let [x, y, z] = pos.map(f64::from);
    let (lo, hi) = (bounds.min_position(), bounds.max_position());
    let top = y + 1.0;
    velocity_y <= 0.0
        && hi.x > x + 0.25
        && lo.x < x + 0.75
        && hi.z > z + 0.25
        && lo.z < z + 0.75
        && lo.y <= top + 0.25
        && (lo.y >= top - 0.25 || previous_y >= top)
}

struct Ends<'g, 'r> {
    game: &'g mut Game<'r>,
    pos: [i32; 3],
    facing: Dir,
    shape: ChuteShape,
    air: Air,
    parts: &'g HashMap<[i32; 3], Part>,
    flows: &'g HashMap<[i32; 3], Air>,
}

impl Ends<'_, '_> {
    fn loaded(&self, pos: [i32; 3]) -> bool {
        self.game.world.chunks.contains_key(&(pos[0] >> 4, pos[2] >> 4))
    }
    fn empty(&self, pos: [i32; 3]) -> bool {
        if !self.loaded(pos) {
            return false;
        }
        OWNERS.with_borrow(|owners| {
            owners
                .get(self.game.dim.id())
                .and_then(|o| o.chutes.get(&pos))
                .is_none_or(|c| c.item.is_none())
        })
    }
    fn give_chute(
        &mut self,
        pos: [i32; 3],
        item: &ItemStack,
        position: f32,
        simulate: bool,
    ) -> Option<ItemStack> {
        if !self.empty(pos) {
            return Some(item.clone());
        }
        if !simulate {
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(self.game.dim.id()).or_default();
                owner.chutes.entry(pos).or_default().accept(item.clone(), position);
            });
        }
        None
    }
}

impl Transfer for Ends<'_, '_> {
    fn take(&mut self, up: bool, most: u32) -> Option<ItemStack> {
        let pos = add(self.pos, if up { Dir::Down } else { Dir::Up }.offset());
        // Regular chutes give through their topology, never by extracting
        // a neighbour's capability (`ChuteBlockEntity.grabCapability`).
        if !self.loaded(pos) || self.parts.contains_key(&pos) {
            return None;
        }
        self.game.tachyon_container_take(
            (pos[0], pos[1], pos[2]),
            if up { Direction::Up } else { Direction::Down },
            most,
        )
    }
    fn give(&mut self, item: &ItemStack, up: bool, simulate: bool) -> Option<ItemStack> {
        let pos = add(self.pos, if up { Dir::Up } else { Dir::Down }.offset());
        if !self.loaded(pos) {
            return Some(item.clone());
        }
        let at = (pos[0], pos[1], pos[2]);
        let face = if up { Direction::Down } else { Direction::Up };
        if (!up || open(self.facing, self.shape))
            && !self.parts.contains_key(&pos)
            && self.game.tachyon_container_present(at)
        {
            let rest = if simulate {
                self.game.tachyon_container_preview(at, item.clone(), face)
            } else {
                self.game.tachyon_container_put(at, item.clone(), face)
            };
            if rest.as_ref().is_none_or(|r| r.count < item.count) {
                return rest;
            }
            if up || self.facing == Dir::Down {
                return Some(item.clone());
            }
        }
        if !up {
            let below = target(self.pos, self.facing);
            if !self.loaded(below) {
                return Some(item.clone());
            }
            if self.parts.contains_key(&below) {
                return self.give_chute(
                    below,
                    item,
                    if self.facing == Dir::Down { 1.0 } else { 0.51 },
                    simulate,
                );
            }
            // Diagonal chutes never drop from an unconnected lower end.
            if self.facing != Dir::Down {
                return Some(item.clone());
            }
        } else {
            let inputs = inputs(self.parts, self.pos);
            let mut best = None;
            let mut fastest = -1.0;
            for &p in &inputs {
                let motion = self.flows.get(&p).copied().unwrap_or_default().motion();
                if motion >= 0.0 && motion > fastest && self.empty(p) {
                    best = Some(p);
                    fastest = motion;
                }
            }
            if let Some(p) = best {
                return self.give_chute(p, item, 0.0, simulate);
            }
            if !inputs.is_empty() {
                return Some(item.clone());
            }
            // Missing chunks may contain a branch; they aren't open air.
            if INPUT_FACINGS.into_iter().any(|d| !self.loaded(input_pos(self.pos, d))) {
                return Some(item.clone());
            }
        }
        let state = self.game.world.block(pos[0], pos[1], pos[2]);
        if kind(self.game.pack.assets.registry, state)
            == Some(Part::Funnel(if up { Dir::Up } else { Dir::Down }))
        {
            return Some(item.clone());
        }
        let b = self.game.res.behaviours();
        let blocked = if up {
            b.is_face_sturdy(state, Direction::Down)
        } else {
            b.is_face_sturdy_rigid(state, Direction::Up)
        };
        if blocked {
            return Some(item.clone());
        }
        if !simulate {
            let (at, velocity) = ejection(self.pos, up, self.air.motion());
            self.game.spawn_item(item.clone(), at, velocity, 10);
        }
        None
    }
}

impl Game<'_> {
    pub(in crate::game) fn tachyon_chute(&self, pos: [i32; 3]) -> Option<(Dir, ChuteShape)> {
        let part = self.tachyon_kinds()?.part(self.world.block(pos[0], pos[1], pos[2]).0)?;
        match part {
            Part::Chute(d, shape) => Some((d, shape)),
            _ => None,
        }
    }
    fn tachyon_chute_limit(&self, stack: &ItemStack) -> u32 {
        if let Some(horizon_world::nbt::Tag::Int(n)) =
            stack.data().and_then(|d| d.other("minecraft:max_stack_size"))
        {
            return (*n).max(1) as u32;
        }
        self.items().get(&stack.item).map_or(64, |i| i.max_stack).max(1)
    }
    pub(in crate::game) fn tachyon_chute_put(
        &mut self,
        pos: [i32; 3],
        stack: ItemStack,
    ) -> Option<ItemStack> {
        let most = self.tachyon_chute_limit(&stack);
        let count = stack.count;
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            let chute = owner.chutes.entry(pos).or_default();
            chute.air = owner.chute_air.get(&pos).copied().unwrap_or(chute.air);
            let rest = chute.insert(stack, most);
            if rest.as_ref().is_none_or(|r| r.count < count) {
                owner.save(&root, dimension);
            }
            rest
        })
    }
    pub(in crate::game) fn tachyon_chute_preview(
        &self,
        pos: [i32; 3],
        stack: ItemStack,
    ) -> Option<ItemStack> {
        let most = self.tachyon_chute_limit(&stack);
        OWNERS.with_borrow(|owners| {
            let mut chute = owners
                .get(self.dim.id())
                .and_then(|o| o.chutes.get(&pos))
                .cloned()
                .unwrap_or_default();
            chute.insert(stack, most)
        })
    }
    pub(in crate::game) fn tachyon_chute_take(
        &mut self,
        pos: [i32; 3],
        most: u32,
    ) -> Option<ItemStack> {
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            let item = owner.chutes.get_mut(&pos)?.extract(most)?;
            owner.save(&root, dimension);
            Some(item)
        })
    }
    pub(in crate::game) fn tachyon_chute_peek(
        &self,
        pos: [i32; 3],
        most: u32,
    ) -> Option<ItemStack> {
        OWNERS.with_borrow(|owners| {
            let item = owners.get(self.dim.id())?.chutes.get(&pos)?.item.as_ref()?;
            (most > 0).then(|| ItemStack { count: item.count.min(most), ..item.clone() })
        })
    }
    /// A hopper puts back only what it just took and couldn't insert.
    pub(in crate::game) fn tachyon_chute_return(&mut self, pos: [i32; 3], stack: ItemStack) {
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            let chute = owner.chutes.entry(pos).or_default();
            match chute.item.as_mut() {
                Some(current) => current.count += stack.count,
                None => chute.item = Some(stack),
            }
            owner.save(&root, dimension);
        });
    }
    /// Neighbour updates belong to Create, not to the template's stairs.
    pub(super) fn tachyon_chute_shapes(&mut self) {
        let parts = OWNERS.with_borrow(|owners| {
            owners.get(self.dim.id()).map(|o| o.index.parts.clone()).unwrap_or_default()
        });
        for (&pos, &part) in &parts {
            let Part::Chute(facing, _) = part else { continue };
            if neighbours(pos, facing)
                .any(|p| !self.world.chunks.contains_key(&(p[0] >> 4, p[2] >> 4)))
            {
                continue;
            }
            let next = updated(pos, part, |p| parts.get(&p).copied());
            if part == next {
                continue;
            }
            let state = self.world.block(pos[0], pos[1], pos[2]);
            let new = with_part(self.pack.assets.registry, state, next);
            if new != state && self.level_set(pos, new).is_ok() {
                OWNERS.with_borrow_mut(|owners| {
                    if let Some(owner) = owners.get_mut(self.dim.id()) {
                        owner.index.parts.insert(pos, next);
                    }
                });
            }
        }
    }
    fn tachyon_chute_reach(&self, pos: [i32; 3], air: Air, parts: &HashMap<[i32; 3], Part>) -> f32 {
        if air.motion() <= 0.0 || parts.contains_key(&add(pos, Dir::Down.offset())) {
            return 0.0;
        }
        let mut reach = air.bottom_reach();
        for i in 1..=reach.ceil() as i32 {
            let below = [pos[0], pos[1] - i, pos[2]];
            if !self.world.chunks.contains_key(&(below[0] >> 4, below[2] >> 4))
                || self
                    .res
                    .facts()
                    .get(self.world.block(below[0], below[1], below[2]))
                    .full_collision
            {
                reach = reach.min((i - 1) as f32);
                break;
            }
        }
        reach
    }
    /// The owner ticks copies, without holding OWNERS across inventory
    /// calls. Neighbour transfers and the current chute are saved together.
    pub(super) fn tachyon_chutes(&mut self) {
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let (parts, flows) = OWNERS.with_borrow(|owners| {
            let Some(owner) = owners.get(dimension) else {
                return (HashMap::new(), HashMap::new());
            };
            let parts: HashMap<[i32; 3], Part> = owner
                .index
                .parts
                .iter()
                .filter(|(_, part)| matches!(part, Part::Chute(_, _)))
                .map(|(p, part)| (*p, *part))
                .collect();
            (parts, owner.chute_air.clone())
        });
        let mut order: Vec<[i32; 3]> = parts.keys().copied().collect();
        order.sort_by_key(|p| (p[1], p[0], p[2]));
        for pos in order {
            let Part::Chute(facing, shape) = parts[&pos] else { continue };
            let air = flows.get(&pos).copied().unwrap_or_default();
            let mut chute = OWNERS.with_borrow(|owners| {
                owners.get(dimension).and_then(|o| o.chutes.get(&pos)).cloned().unwrap_or_default()
            });
            let before = chute.clone();
            let mut received = false;
            chute.air = air;
            chute.bottom_pull_distance = self.tachyon_chute_reach(pos, air, &parts);
            if chute.search_due() {
                let area = search_area(pos, chute.bottom_pull_distance);
                let i = self
                    .dropped
                    .iter()
                    .position(|d| d.health > 0 && d.stack.count > 0 && d.bounds().intersects(area));
                if let Some(i) = i {
                    let item = self.dropped.remove(i);
                    let height = (item.bounds().center().y - f64::from(pos[1])) as f32;
                    received = chute.accept(item.stack, height);
                }
            }
            // AbstractChuteBlock.updateEntityAfterFallOn: an exposed open
            // mouth accepts a falling stack, never one ejected upward.
            if chute.item.is_none()
                && open(facing, shape)
                && !matches!(
                    parts.get(&add(pos, Dir::Up.offset())),
                    Some(Part::Chute(Dir::Down, _))
                )
                && (air.motion() <= 0.0 || !inputs(&parts, pos).is_empty())
            {
                let i = self.dropped.iter().position(|d| {
                    d.health > 0
                        && d.stack.count > 0
                        && falling_at_top(pos, d.bounds(), d.previous.y, d.velocity.y)
                });
                if let Some(i) = i {
                    let item = self.dropped[i].stack.clone();
                    let count = item.count;
                    let rest = chute.insert(item.clone(), self.tachyon_chute_limit(&item));
                    if rest.as_ref().is_none_or(|r| r.count < count) {
                        received = true;
                        match rest {
                            None => {
                                self.dropped.remove(i);
                            }
                            Some(rest) => self.dropped[i].stack = rest,
                        }
                    }
                }
            }
            chute.tick(&mut Ends {
                game: self,
                pos,
                facing,
                shape,
                air,
                parts: &parts,
                flows: &flows,
            });
            // Fast upward air can receive and pass a stack this tick,
            // leaving both snapshots empty but changing a neighbour.
            let transferred = received || chute.item != before.item;
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                owner.mills_changed |= transferred
                    || chute.position != before.position
                    || chute.air != before.air
                    || chute.bottom_pull_distance != before.bottom_pull_distance;
                owner.chutes.insert(pos, chute);
                if transferred {
                    owner.save(&root, dimension);
                }
            });
        }
    }
    /// `ChuteRenderer`: reuse the depot's item renderer, at half scale,
    /// turning with its interpolated height, only in vertical windows or
    /// the exposed space an upper fan pulls through below the chute.
    pub(super) fn tachyon_chute_items(
        &self,
        camera: glam::DVec3,
        alpha: f64,
        out: &mut Vec<horizon_client::mesher::Vertex>,
    ) {
        let draws = self.tachyon_view(|index, shown| {
            shown
                .chutes
                .iter()
                .filter_map(|(pos, offsets)| {
                    let Part::Chute(Dir::Down, shape) = index.parts.get(pos)? else { return None };
                    let height = offsets[0] + (offsets[1] - offsets[0]) * alpha as f32;
                    if *shape != ChuteShape::Window && (offsets[2] == 0.0 || height > 0.5) {
                        return None;
                    }
                    let (item, count) = shown.held.get(pos)?.first()?;
                    Some((*pos, ItemStack::new(item, *count), height))
                })
                .collect::<Vec<_>>()
        });
        for (pos, stack, height) in draws {
            let at = glam::DVec3::from(pos.map(f64::from));
            if (at + glam::DVec3::splat(0.5)).distance_squared(camera) > super::VIEW * super::VIEW {
                continue;
            }
            let state = self.resolve_item(
                &stack,
                horizon_assets::item_model::DisplayContext::Fixed,
                None,
                0,
            );
            let light = horizon_client::block_entity_render::vertex_light(
                self.world.light(pos[0], pos[1], pos[2]),
            );
            let pose = glam::Mat4::from_translation(
                (at - camera).as_vec3() + glam::Vec3::new(0.5, height, 0.5),
            ) * glam::Mat4::from_scale(glam::Vec3::splat(0.5))
                * glam::Mat4::from_rotation_x(height * std::f32::consts::PI)
                * glam::Mat4::from_rotation_y(height * std::f32::consts::PI);
            self.submit_item_world(&state, pose, light, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestEnds {
        above: Option<ItemStack>,
        below: Option<ItemStack>,
        drop: bool,
        dropped: Vec<ItemStack>,
        given: u32,
    }
    impl Transfer for TestEnds {
        fn take(&mut self, up: bool, most: u32) -> Option<ItemStack> {
            let cell = if up { &mut self.below } else { &mut self.above };
            let stack = cell.as_mut()?;
            let out = ItemStack { count: stack.count.min(most), ..stack.clone() };
            stack.count -= out.count;
            if stack.count == 0 {
                *cell = None;
            }
            Some(out)
        }
        fn give(&mut self, item: &ItemStack, up: bool, simulate: bool) -> Option<ItemStack> {
            if self.drop {
                if !simulate {
                    self.dropped.push(item.clone());
                }
                return None;
            }
            let cell = if up { &mut self.above } else { &mut self.below };
            let room = match cell {
                None => 64,
                Some(stack) if stack.same_item_same_components(item) => {
                    64u32.saturating_sub(stack.count)
                }
                Some(_) => 0,
            };
            let count = room.min(item.count);
            if !simulate && count > 0 {
                self.given += count;
                match cell {
                    Some(stack) => stack.count += count,
                    None => *cell = Some(ItemStack { count, ..item.clone() }),
                }
            }
            (count < item.count).then(|| ItemStack { count: item.count - count, ..item.clone() })
        }
    }

    #[test]
    fn a_chute_falls_without_air_and_rises_only_with_positive_air() {
        assert_eq!(Air::default().motion(), -0.2);
        assert_eq!(Air { push: 128.0, pull: 0.0 }.motion(), 0.1);
        assert_eq!(Air { push: 0.0, pull: 256.0 }.motion(), 0.2);
        assert_eq!(Air { push: -128.0, pull: 0.0 }.motion(), -0.3);
        assert_eq!(Air { push: 128.0, pull: -128.0 }.motion(), -0.2);
        assert_eq!(Air { push: 4096.0, pull: 0.0 }.motion(), 1.0);
        assert_eq!(Air { push: -4096.0, pull: 0.0 }.motion(), -1.2);
    }

    #[test]
    fn an_empty_upward_chute_searches_even_without_bottom_reach_then_waits_five_ticks() {
        let mut chute = Chute { air: Air { push: 256.0, pull: 0.0 }, ..Chute::default() };
        assert_eq!(chute.air.bottom_reach(), 0.0);
        assert!(chute.search_due());
        for _ in 0..5 {
            assert!(!chute.search_due());
        }
        assert!(chute.search_due());
        let near = Aabb::of_size(Vec3::new(0.5, 9.9, 0.5), Vec3::new(0.25, 0.25, 0.25));
        let far = near.move_by(Vec3::new(0.0, -1.0, 0.0));
        assert!(search_area([0, 10, 0], 0.0).intersects(near));
        assert!(!search_area([0, 10, 0], 0.0).intersects(far));
        assert!(search_area([0, 10, 0], 2.0).intersects(far));
        assert!(!Chute::default().search_due());
        assert_eq!(Air { push: 0.0, pull: 16.0 }.bottom_reach(), 0.5);
        assert_eq!(Air { push: 0.0, pull: 32.0 }.bottom_reach(), 1.0);
        assert_eq!(Air { push: 0.0, pull: 64.0 }.bottom_reach(), 2.0);
        assert_eq!(Air { push: 0.0, pull: 128.0 }.bottom_reach(), 3.0);
    }

    #[test]
    fn a_falling_item_enters_the_mouth_but_an_upward_item_does_not() {
        let item = Aabb::of_size(Vec3::new(0.5, 11.0, 0.5), Vec3::new(0.25, 0.25, 0.25));
        assert!(falling_at_top([0, 10, 0], item, 11.1, -0.1));
        assert!(!falling_at_top([0, 10, 0], item, 10.9, 0.1));
        assert!(!falling_at_top([0, 10, 0], item.move_by(Vec3::new(1.0, 0.0, 0.0)), 11.1, -0.1));
        assert!(falling_at_top([0, 10, 0], item.move_by(Vec3::new(0.0, -1.0, 0.0)), 11.1, -1.0));
    }

    #[test]
    fn an_empty_chute_takes_16_from_above_and_waits_that_tick() {
        let mut chute = Chute::default();
        let mut ends =
            TestEnds { above: Some(ItemStack::new("minecraft:stone", 32)), ..TestEnds::default() };
        chute.tick(&mut ends);
        assert_eq!(chute.item, Some(ItemStack::new("minecraft:stone", 16)));
        assert_eq!(chute.position, 1.0);
        assert_eq!(ends.above.unwrap().count, 16);
    }

    #[test]
    fn a_chute_gives_below_only_past_its_end() {
        let mut chute = Chute::default();
        chute.accept(ItemStack::new("minecraft:stone", 16), 1.0);
        let mut ends = TestEnds::default();
        for _ in 0..4 {
            chute.tick(&mut ends);
        }
        assert_eq!(ends.given, 0);
        assert!(chute.item.is_some());
        for _ in 0..3 {
            chute.tick(&mut ends);
        }
        assert!(chute.item.is_none());
        assert_eq!(ends.below, Some(ItemStack::new("minecraft:stone", 16)));
    }

    #[test]
    fn a_full_inventory_holds_the_stack_halfway_and_partial_output_keeps_the_rest() {
        let mut chute = Chute::default();
        chute.accept(ItemStack::new("minecraft:stone", 16), 1.0);
        let mut ends =
            TestEnds { below: Some(ItemStack::new("minecraft:stone", 64)), ..TestEnds::default() };
        for _ in 0..10 {
            chute.tick(&mut ends);
        }
        assert_eq!(chute.position, 0.5);
        assert_eq!(chute.item.as_ref().unwrap().count, 16);
        ends.below.as_mut().unwrap().count = 60;
        for _ in 0..3 {
            chute.tick(&mut ends);
        }
        assert_eq!(ends.given, 4);
        assert_eq!(chute.item.as_ref().unwrap().count, 12);
        chute.tick(&mut ends);
        assert_eq!(chute.position, 0.5);
    }

    #[test]
    fn a_bottom_chute_drops_its_stack_and_an_upper_end_ejects_with_its_motion() {
        let mut chute = Chute::default();
        chute.accept(ItemStack::new("minecraft:stone", 8), 1.0);
        let mut ends = TestEnds { drop: true, ..TestEnds::default() };
        for _ in 0..7 {
            chute.tick(&mut ends);
        }
        assert_eq!(ends.dropped, [ItemStack::new("minecraft:stone", 8)]);
        assert!(chute.item.is_none());
        assert_eq!(ejection([2, 10, -3], false, -0.2), ([2.5, 9.75, -2.5], [0.0, -0.25, 0.0]));
        let (at, velocity) = ejection([2, 10, -3], true, 0.1);
        assert_eq!(at, [2.5, 11.0, -2.5]);
        assert!((velocity[1] - 0.2).abs() < 1e-7);
        let mut chute = Chute { air: Air { push: 256.0, pull: 0.0 }, ..Chute::default() };
        chute.accept(ItemStack::new("minecraft:stone", 8), 0.0);
        let mut ends = TestEnds { drop: true, ..TestEnds::default() };
        for _ in 0..6 {
            chute.tick(&mut ends);
        }
        assert_eq!(ends.dropped, [ItemStack::new("minecraft:stone", 8)]);
        assert!(chute.item.is_none());
    }

    #[test]
    fn upward_air_takes_below_and_gives_above() {
        let mut chute = Chute { air: Air { push: 256.0, pull: 0.0 }, ..Chute::default() };
        let mut ends =
            TestEnds { below: Some(ItemStack::new("minecraft:stone", 16)), ..TestEnds::default() };
        chute.tick(&mut ends);
        assert_eq!(chute.position, 0.0);
        assert!(ends.below.is_none());
        for _ in 0..6 {
            chute.tick(&mut ends);
        }
        assert!(chute.item.is_none());
        assert_eq!(ends.above, Some(ItemStack::new("minecraft:stone", 16)));
    }

    #[test]
    fn fast_upward_air_can_pass_a_newly_received_entity_in_the_same_tick() {
        let mut chute = Chute { air: Air { push: 1280.0, pull: 0.0 }, ..Chute::default() };
        let mut ends = TestEnds::default();
        assert!(chute.accept(ItemStack::new("minecraft:stone", 8), 0.4));
        chute.tick(&mut ends);
        assert!(chute.item.is_none());
        assert_eq!(ends.above, Some(ItemStack::new("minecraft:stone", 8)));
    }

    #[test]
    fn a_chute_never_merges_a_second_stack_and_keeps_oversized_remainders() {
        let mut chute = Chute::default();
        assert_eq!(chute.insert(ItemStack::new("minecraft:stone", 80), 64).unwrap().count, 16);
        assert_eq!(chute.insert(ItemStack::new("minecraft:stone", 8), 64).unwrap().count, 8);
        assert_eq!(chute.item.unwrap().count, 64);
    }

    #[test]
    fn hopper_extraction_resets_the_remainders_height_in_the_current_direction() {
        let mut chute = Chute::default();
        chute.accept(ItemStack::new("minecraft:stone", 8), 0.25);
        assert!(chute.extract(0).is_none());
        assert_eq!(chute.extract(3).unwrap().count, 3);
        assert_eq!(chute.item.as_ref().unwrap().count, 5);
        assert_eq!((chute.previous, chute.position), (1.0, 1.0));
        chute.air.pull = 128.0;
        assert_eq!(chute.extract(1).unwrap().count, 1);
        assert_eq!((chute.previous, chute.position), (0.0, 0.0));
    }

    #[test]
    fn fan_push_splits_between_branches_and_pull_adds_them() {
        let chute = |d| Part::Chute(d, ChuteShape::Normal);
        let parts = HashMap::from([
            ([0, 0, 0], chute(Dir::Down)),
            ([0, 1, 0], chute(Dir::Down)),
            ([1, 1, 0], chute(Dir::East)),
            ([0, 2, 0], chute(Dir::Down)),
            ([5, 1, 0], chute(Dir::Down)),
        ]);
        let air = airflow(
            &parts,
            &[([0, 0, 0], false, 256.0), ([0, 2, 0], true, 64.0), ([1, 1, 0], true, 32.0)],
        );
        assert_eq!(air[&[0, 1, 0]].push, 128.0);
        assert_eq!(air[&[1, 1, 0]].push, 128.0);
        assert_eq!(air[&[0, 2, 0]].push, 128.0);
        assert_eq!(air[&[0, 0, 0]].pull, 96.0);
        assert_eq!(air[&[5, 1, 0]], Air::default());
        assert_eq!(target([1, 1, 0], Dir::East), [0, 0, 0]);
    }

    #[test]
    fn inputs_keep_creates_order_for_equal_speed_upward_outputs() {
        let chute = |d| Part::Chute(d, ChuteShape::Normal);
        let parts = HashMap::from([
            ([0, 1, 0], chute(Dir::Down)),
            ([0, 1, 1], chute(Dir::South)),
            ([0, 1, -1], chute(Dir::North)),
            ([1, 1, 0], chute(Dir::East)),
            ([-1, 1, 0], chute(Dir::West)),
            ([2, 1, 0], chute(Dir::East)),
        ]);
        assert_eq!(
            inputs(&parts, [0, 0, 0]),
            [[0, 1, 0], [0, 1, 1], [0, 1, -1], [1, 1, 0], [-1, 1, 0]]
        );
    }

    #[test]
    fn diagonal_inputs_make_intersections_and_missing_targets_make_vertical_chutes() {
        let down = Part::Chute(Dir::Down, ChuteShape::Normal);
        let east = Part::Chute(Dir::East, ChuteShape::Normal);
        let parts = HashMap::from([([0, 0, 0], down), ([1, 1, 0], east)]);
        assert_eq!(
            updated([0, 0, 0], down, |p| parts.get(&p).copied()),
            Part::Chute(Dir::Down, ChuteShape::Intersection)
        );
        assert_eq!(
            updated([1, 1, 0], east, |p| parts.get(&p).copied()),
            Part::Chute(Dir::East, ChuteShape::Intersection)
        );
        assert_eq!(updated([1, 1, 0], east, |_| None), down);
        assert!(open(Dir::Down, ChuteShape::Window));
        assert!(!open(Dir::East, ChuteShape::Normal));
    }

    #[test]
    fn saved_chutes_keep_damage_and_components() {
        let mut item = ItemStack::new("minecraft:iron_pickaxe", 1);
        let tag = horizon_world::nbt::Tag::compound([
            ("minecraft:damage", horizon_world::nbt::Tag::Int(7)),
            (
                "minecraft:custom_data",
                horizon_world::nbt::Tag::compound([("chute", horizon_world::nbt::Tag::Int(42))]),
            ),
        ]);
        (item.damage, item.extra) = horizon_client::item_data::decode_components(Some(&tag));
        let mut chute = Chute::default();
        chute.accept(item.clone(), 0.51);
        let read: Chute = serde_json::from_str(&serde_json::to_string(&chute).unwrap()).unwrap();
        assert_eq!(read.item, Some(item));
        assert_eq!(read.position, 0.51);
    }

    #[test]
    fn saved_chutes_restart_at_their_height_and_a_bad_row_does_not_erase_the_rest() {
        let p = [0, 10, 0];
        let q = [2, 10, 0];
        let mut first = Chute::default();
        first.accept(ItemStack::new("minecraft:stone", 8), 0.51);
        let mut second = Chute {
            air: Air { push: 0.0, pull: 128.0 },
            bottom_pull_distance: 3.0,
            ..Chute::default()
        };
        second.accept(ItemStack::new("minecraft:iron_ingot", 16), -0.5);
        let rows = serde_json::json!([
            [p, &first],
            [[1, 10, 0], {"item": [255]}],
            ["not a position", &first],
            [q, &second],
        ]);
        let loaded = read(&rows);
        assert_eq!(loaded.len(), 2);
        for (pos, chute) in [(p, first), (q, second)] {
            let restored = &loaded[&pos];
            assert_eq!(restored.item, chute.item);
            assert_eq!(restored.position, chute.position);
            assert_eq!(restored.previous, chute.position);
            assert_eq!(restored.air, chute.air);
            assert_eq!(restored.bottom_pull_distance, chute.bottom_pull_distance);
        }
        assert!(read(&serde_json::Value::Null).is_empty());
    }
}
