//! Kinetica's belt and funnel. Belt segments in a line turn together, on
//! pulleys a shaft beside any of them turns; turning, they carry the dropped
//! items on them along the line (Create's speed, a block in 480 / RPM ticks)
//! and keep them to its middle. Past its end an item falls off, or goes into
//! a funnel there.
//!
//! A funnel sits on a container's face (a chest, a furnace, a depot, a
//! basin...), facing out of it. It puts the dropped items that come into it
//! into the container, through that face; used, it turns to taking out
//! instead: up to a stack every 8 ticks
//! while nothing lies at its mouth, dropped out of it (onto a belt, say).
//!
//! The speed and directions follow `BeltBlockEntity` (Create's MIT code,
//! THIRD_PARTY_NOTICES.md). Unlike `BeltInventory`, these belts carry ordinary
//! dropped items: a rejected funnel insertion does not hold them at the end.
//! `FunnelBlockEntity` has per-funnel cooldowns and one-item andesite
//! extraction; this module keeps Tachyon's global clock and stack limit.

use super::super::Game;
use super::super::machines::dir_from_name;
use super::kinetic::{Axis, Dir, Part};
use super::{OWNERS, Owner};
use horizon_client::{entities::ItemEntity, items::ItemStack};
use horizon_core::geometry::Direction;
use std::collections::HashMap;

/// How often a funnel may take out, on the owner's clock.
const FUNNEL_TICKS: u64 = 8;
/// The most a funnel takes out at once: a stack, Tachyon's current limit.
const FUNNEL_STACK: u32 = 64;
/// How far round its block a funnel reaches for the items to put in.
const FUNNEL_REACH: f64 = 0.25;

/// What a belt along `axis` turning at `speed` (RPM) carries an item by in a
/// tick: Create's speed, the way its pulleys' tops go (by the right hand
/// about their axis, the other level one).
pub(super) fn carry(axis: Axis, speed: f32) -> [f64; 3] {
    let v = f64::from(speed) / 480.0;
    match axis {
        Axis::X => [-v, 0.0, 0.0],
        _ => [0.0, 0.0, v],
    }
}

/// The axis of a belt's pulleys: the other level one.
pub(super) fn pulleys(axis: Axis) -> Axis {
    if axis == Axis::X { Axis::Z } else { Axis::X }
}

/// The belts turning, and what each carries by.
fn moving(parts: &HashMap<[i32; 3], Part>, speeds: &super::kinetic::Speeds) -> HashMap<[i32; 3], [f64; 3]> {
    (parts.iter())
        .filter_map(|(p, part)| {
            let Part::Belt(axis) = part else {
                return None;
            };
            let speed = speeds.get(p)?[0];
            (speed != 0.0).then(|| (*p, carry(*axis, speed)))
        })
        .collect()
}

/// Whether an item's box `lo`..`hi` meets the block at `pos` grown by
/// `grow` all round.
fn meets(pos: [i32; 3], lo: [f64; 3], hi: [f64; 3], grow: f64) -> bool {
    (0..3).all(|i| {
        let min = f64::from(pos[i]) - grow;
        lo[i] < min + 1.0 + 2.0 * grow && hi[i] > min
    })
}

/// Carries the dropped items by the turning belt under their middle, else
/// under their edge. Their ordinary item ticks move them and let them fall.
fn convey(belts: &HashMap<[i32; 3], [f64; 3]>, dropped: &mut [ItemEntity]) {
    if belts.is_empty() {
        return;
    }
    for item in dropped {
        let p = item.position;
        let y = p.y.floor();
        // On the belt's top (9/16), not over it.
        if p.y - y > 0.75 {
            continue;
        }
        let cell = |dx: f64, dz: f64| [(p.x + dx).floor() as i32, y as i32, (p.z + dz).floor() as i32];
        let under = [(0.0, 0.0), (-0.125, 0.0), (0.125, 0.0), (0.0, -0.125), (0.0, 0.125)]
            .into_iter()
            .map(|(dx, dz)| cell(dx, dz))
            .find_map(|c| Some((c, *belts.get(&c)?)));
        let Some((at, way)) = under else {
            continue;
        };
        // Along the belt at its speed; across, to its middle.
        let v = &mut item.velocity;
        if way[0] != 0.0 {
            v.x = way[0];
            v.z = (f64::from(at[2]) + 0.5 - p.z) * 0.2;
        } else {
            v.z = way[2];
            v.x = (f64::from(at[0]) + 0.5 - p.x) * 0.2;
        }
    }
}

/// The dropped items and containers a funnel moves between. The game keeps
/// its usual container rules and spawning; tests need neither its resources
/// nor a running window.
trait FunnelWorld {
    fn dropped(&mut self) -> &mut Vec<ItemEntity>;
    fn put(&mut self, at: (i32, i32, i32), stack: ItemStack, face: Direction) -> Option<ItemStack>;
    fn take(&mut self, at: (i32, i32, i32), face: Direction, most: u32) -> Option<ItemStack>;
    fn spawn(&mut self, stack: ItemStack, at: [f64; 3], velocity: [f64; 3], delay: u32);
}

/// A funnel's tick: collect nearby dropped stacks, or extract while its
/// mouth is empty. Compared with `FunnelBlockEntity`, its reach, global
/// clock, stack limit and dropped-item motion are Tachyon's current rules.
fn funnel(world: &mut impl FunnelWorld, pos: [i32; 3], facing: Dir, out: bool, ticks: u64) {
    let o = facing.offset();
    let behind = (pos[0] - o[0], pos[1] - o[1], pos[2] - o[2]);
    let Some(face) = dir_from_name(facing.name()) else {
        return;
    };
    let near: Vec<usize> = (world.dropped().iter().enumerate())
        .filter(|(_, d)| {
            let b = d.bounds();
            let (lo, hi) = (b.min_position(), b.max_position());
            meets(pos, [lo.x, lo.y, lo.z], [hi.x, hi.y, hi.z], FUNNEL_REACH)
        })
        .map(|(i, _)| i)
        .collect();
    if !out {
        // Last first, so the others keep their places.
        for i in near.into_iter().rev() {
            let stack = world.dropped()[i].stack.clone();
            match world.put(behind, stack, face) {
                None => {
                    world.dropped().remove(i);
                }
                Some(rest) => world.dropped()[i].stack = rest,
            }
        }
        return;
    }
    if !near.is_empty() || ticks % FUNNEL_TICKS != 0 {
        return;
    }
    let Some(stack) = world.take(behind, face, FUNNEL_STACK) else {
        return;
    };
    // Out of its mouth: over its rod when it faces the side.
    let reach = if facing.axis() == Axis::Y { 0.8 } else { 0.3 };
    let mut at = [0usize, 1, 2].map(|i| f64::from(pos[i]) + 0.5 + f64::from(o[i]) * reach);
    if facing.axis() != Axis::Y {
        at[1] = f64::from(pos[1]) + 0.7;
    }
    let velocity = o.map(|v| f64::from(v) * 0.12);
    world.spawn(stack, at, velocity, 10);
}

impl Game<'_> {
    /// The owner's belts carry the dropped items on them: each by the belt
    /// under its middle, else under its edge (so it goes off the end).
    pub(super) fn tachyon_belts(&mut self) {
        let dimension = self.dim.id();
        let belts = OWNERS.with_borrow(|owners| {
            (owners.get(dimension))
                .map(|o| moving(&o.index.parts, &o.shown.speeds))
                .unwrap_or_default()
        });
        convey(&belts, &mut self.dropped);
    }

    /// The owner's funnels: putting the items that come into them into the
    /// container behind, or taking stacks out of it.
    pub(super) fn tachyon_funnels(&mut self) {
        let dimension = self.dim.id();
        let (funnels, ticks) = OWNERS.with_borrow_mut(|owners| {
            let Some(owner): Option<&mut Owner> = owners.get_mut(dimension) else {
                return (Vec::new(), 0);
            };
            let funnels: Vec<([i32; 3], Dir, bool)> = (owner.index.parts.iter())
                .filter_map(|(p, part)| match part {
                    Part::Funnel(d) => Some((*p, *d, owner.funnels_out.contains(p))),
                    _ => None,
                })
                .collect();
            owner.funnel_work = (funnels.iter())
                .map(|(p, _, out)| (*p, (if *out { "extracting" } else { "inserting" }).to_owned()))
                .collect();
            (funnels, owner.ticks)
        });
        for (pos, facing, out) in funnels {
            funnel(self, pos, facing, out, ticks);
        }
    }
}

impl FunnelWorld for Game<'_> {
    fn dropped(&mut self) -> &mut Vec<ItemEntity> {
        &mut self.dropped
    }
    fn put(&mut self, at: (i32, i32, i32), stack: ItemStack, face: Direction) -> Option<ItemStack> {
        self.tachyon_container_put(at, stack, face)
    }
    fn take(&mut self, at: (i32, i32, i32), face: Direction, most: u32) -> Option<ItemStack> {
        self.tachyon_container_take(at, face, most)
    }
    fn spawn(&mut self, stack: ItemStack, at: [f64; 3], velocity: [f64; 3], delay: u32) {
        self.spawn_item(stack, at, velocity, delay);
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::machines::{tachyon_funnel_put, tachyon_funnel_take};
    use super::super::kinetic::solve;
    use super::super::millstone::{Held, Mill};
    use super::*;
    use horizon_client::{
        assets::{Assets, Properties},
        blocks::{BlockFacts, Fluid, RenderShape, Sounds, StateFacts},
        chunk::Chunk,
        motion::FluidResources,
        world::World,
    };
    use horizon_core::geometry::Vec3;
    use horizon_world::{
        DATA_VERSION,
        block_collision::BlockCollisionRegistry,
        collision::CachedCollisionRegistry,
        nbt::Tag,
        registry::{BlockRegistry, BlockStateId},
    };
    use std::sync::{Arc, OnceLock};

    const TOP: f64 = 9.0 / 16.0;
    type Parts = HashMap<[i32; 3], Part>;
    type Belts = HashMap<[i32; 3], [f64; 3]>;

    fn item(id: u32, position: Vec3, count: u32) -> ItemEntity {
        ItemEntity {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            vertical_collision: false,
            horizontal_collision: false,
            needs_sync: false,
            stuck: Vec3::ZERO,
            was_touching_water: false,
            health: 5,
            fire_ticks: 0,
            id,
            stack: ItemStack::new("minecraft:stone", count),
            thrower: None,
            target: None,
            position,
            previous: position,
            velocity: Vec3::ZERO,
            on_ground: true,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
        }
    }

    fn point(axis: Axis, along: f64) -> Vec3 {
        if axis == Axis::X {
            Vec3::new(along, TOP, 0.5)
        } else {
            Vec3::new(0.5, TOP, along)
        }
    }

    fn close(actual: Vec3, expected: [f64; 3]) {
        for (got, want) in [actual.x, actual.y, actual.z].into_iter().zip(expected) {
            assert!((got - want).abs() < 1e-9, "{got} != {want}");
        }
    }

    /// A motor beside the first pulley drives the whole line, in either
    /// direction; motion is checked against `BeltBlockEntity`'s speed.
    fn line(axis: Axis, length: i32, speed: f32) -> (Parts, Belts) {
        let mut parts: Parts = (0..length)
            .map(|i| {
                let at = if axis == Axis::X { [i, 0, 0] } else { [0, 0, i] };
                (at, Part::Belt(axis))
            })
            .collect();
        let (motor, facing) = if axis == Axis::X {
            ([0, 0, -1], Dir::South)
        } else {
            ([-1, 0, 0], Dir::East)
        };
        parts.insert(motor, Part::Motor(facing));
        let solved = solve(&parts, &HashMap::from([(motor, speed)]), &Default::default(), &Default::default());
        let belts = moving(&parts, &solved.speeds);
        (parts, belts)
    }

    /// Only air, a belt's 9/16 top and a solid end. No resource files: the
    /// real `ItemEntity::tick` supplies movement, gravity, collision and drag.
    struct Scene {
        world: World,
        facts: BlockFacts,
        collision: BlockCollisionRegistry<'static>,
        fluids: FluidResources,
    }
    impl Scene {
        fn new(parts: &Parts) -> Self {
            const NAMES: [&str; 3] = ["minecraft:air", "kinetica:belt", "minecraft:stone"];
            static REGISTRY: OnceLock<BlockRegistry> = OnceLock::new();
            let states: Vec<Tag> = NAMES.map(|s| Tag::String(s.into())).into();
            let registry = REGISTRY.get_or_init(|| {
                BlockRegistry::from_nbt(&Tag::compound([
                    ("DataVersion", Tag::Int(DATA_VERSION)),
                    ("states", Tag::List(states.clone())),
                    (
                        "defaults",
                        Tag::compound(
                            NAMES
                                .into_iter()
                                .enumerate()
                                .map(|(i, s)| (s, Tag::Int(i as i32))),
                        ),
                    ),
                ]))
                .unwrap()
            });
            let shape = |top: f64, canonical| {
                let empty = top == 0.0;
                let cube = top == 1.0;
                let coords = |end: f64| {
                    Tag::LongArray(if empty { vec![0] } else { vec![0, end.to_bits() as i64] })
                };
                Tag::compound([
                    ("canonical", Tag::Byte(canonical)),
                    ("x", coords(1.0)),
                    ("y", coords(top)),
                    ("z", coords(1.0)),
                    (
                        "occupied",
                        Tag::ByteArray(if empty { vec![] } else { vec![1] }),
                    ),
                    ("cube", Tag::Byte(if cube { 1 } else { 0 })),
                    ("cube_axes", Tag::Byte(if cube { 7 } else { 0 })),
                    ("discrete_min", Tag::IntArray(vec![0; 3])),
                    ("discrete_max", Tag::IntArray(vec![if empty { 0 } else { 1 }; 3])),
                ])
            };
            let cached = CachedCollisionRegistry::from_nbt(
                &Tag::compound([
                    ("DataVersion", Tag::Int(DATA_VERSION)),
                    ("states", Tag::List(states)),
                    (
                        "shapes",
                        Tag::List(vec![shape(0.0, 1), shape(TOP, 0), shape(1.0, 2)]),
                    ),
                    ("cached_shapes", Tag::IntArray(vec![0, 1, 2])),
                ]),
                registry,
            )
            .unwrap();
            let facts = BlockFacts {
                air: BlockStateId(0),
                states: NAMES
                    .into_iter()
                    .map(|name| StateFacts {
                        name: name.into(),
                        emission: 0,
                        dampening: 0,
                        skylight_down: true,
                        shape_occlusion: false,
                        can_occlude: false,
                        solid_render: false,
                        hardness: 0.0,
                        requires_tool: false,
                        replaceable: name == "minecraft:air",
                        random_ticks: false,
                        block_entity: false,
                        render_shape: RenderShape::Model,
                        full_collision: name == "minecraft:stone",
                        friction: 0.6,
                        explosion_resistance: 0.0,
                        item: name.into(),
                        sounds: Sounds {
                            volume: 1.0,
                            pitch: 1.0,
                            break_sound: "".into(),
                            step: "".into(),
                            place: "".into(),
                            hit: "".into(),
                            fall: "".into(),
                        },
                        fluid: Fluid::None,
                        fluid_level: 0,
                        fluid_falling: false,
                        waterlogged: false,
                        random_offset: false,
                        outline_offset: false,
                        legacy_solid: false,
                        sturdy_full: 0,
                        classes: Default::default(),
                        outline: Default::default(),
                        outline_edges: Default::default(),
                        map_color: 0,
                    })
                    .collect(),
                woods: Default::default(),
            };
            let assets = Assets {
                registry,
                properties: Properties::for_tests(&[
                    (true, false, false, false),
                    (false, false, true, true),
                    (false, false, true, true),
                ]),
                material_states: [BlockStateId(2); 6],
                blocks_fluid_flow: Default::default(),
            };
            let mut scene = Self {
                world: World::new(BlockStateId(0), 1),
                fluids: FluidResources::new(&assets, &facts).unwrap(),
                facts,
                collision: BlockCollisionRegistry::new(Arc::new(cached)).unwrap(),
            };
            for (&at, part) in parts {
                if matches!(part, Part::Belt(_)) {
                    scene.set(at, BlockStateId(1));
                }
            }
            scene
        }
        fn set(&mut self, at: [i32; 3], state: BlockStateId) {
            let key = (at[0] >> 4, at[2] >> 4);
            if !self.world.chunks.contains_key(&key) {
                self.world
                    .chunks
                    .insert(key, Arc::new(Chunk::empty(key.0, key.1)));
            }
            self.world.set(at[0], at[1], at[2], state).unwrap();
        }
        fn tick(&self, dropped: &mut [ItemEntity]) {
            for d in dropped {
                // ItemEntity.tick without fluids: the move, then the settle.
                let _ = &self.fluids;
                let step = d.tick_move(
                    &self.world,
                    &self.facts,
                    &self.collision,
                    false,
                    (0.0, 0.0),
                    &[],
                    None,
                    None,
                );
                assert!(d.tick_settle(&self.world, &self.facts, false, step));
            }
        }
    }

    /// Plain stacks through the one-item funnel adapters in `machines.rs`.
    /// `Mill` supplies storage; container lookup and spawning stand in for `Game`.
    #[derive(Default)]
    struct Funnels {
        dropped: Vec<ItemEntity>,
        depots: HashMap<(i32, i32, i32), Mill>,
        puts: Vec<((i32, i32, i32), Direction, ItemStack)>,
        takes: Vec<((i32, i32, i32), Direction, u32)>,
        next_id: u32,
    }
    impl FunnelWorld for Funnels {
        fn dropped(&mut self) -> &mut Vec<ItemEntity> {
            &mut self.dropped
        }
        fn put(
            &mut self,
            at: (i32, i32, i32),
            stack: ItemStack,
            face: Direction,
        ) -> Option<ItemStack> {
            self.puts.push((at, face, stack.clone()));
            let Some(mill) = self.depots.get_mut(&at) else {
                return Some(stack);
            };
            tachyon_funnel_put(stack, |one| mill.put(&one.item, one.count) == one.count)
        }
        fn take(&mut self, at: (i32, i32, i32), face: Direction, most: u32) -> Option<ItemStack> {
            self.takes.push((at, face, most));
            tachyon_funnel_take(
                self.depots.get_mut(&at)?,
                most,
                |mill| {
                    mill.take_one(true)
                        .map(|(h, input)| (ItemStack::new(&h.item, h.count), input))
                },
                |mill, s, input| {
                    mill.give_back(
                        Held {
                            item: s.item.to_string(),
                            count: s.count,
                        },
                        input,
                    )
                },
            )
        }
        fn spawn(&mut self, stack: ItemStack, at: [f64; 3], velocity: [f64; 3], delay: u32) {
            self.next_id += 1;
            let mut d = item(self.next_id, Vec3::new(at[0], at[1], at[2]), stack.count);
            d.stack = stack;
            d.velocity = Vec3::new(velocity[0], velocity[1], velocity[2]);
            d.pickup_delay = i64::from(delay);
            d.on_ground = false;
            self.dropped.push(d);
        }
    }

    #[test]
    fn a_belt_carries_the_way_its_pulleys_tops_go() {
        // Pulleys along X turning forward: their tops go south.
        assert_eq!(carry(Axis::Z, 48.0), [0.0, 0.0, 0.1]);
        // Pulleys along Z: west.
        assert_eq!(carry(Axis::X, 48.0), [-0.1, 0.0, 0.0]);
        assert_eq!(carry(Axis::X, -48.0), [0.1, 0.0, 0.0]);
        assert_eq!(carry(Axis::Z, -48.0), [0.0, 0.0, -0.1]);
        assert_eq!(carry(Axis::X, 0.0), [0.0; 3]);
        assert_eq!(pulleys(Axis::X), Axis::Z);
        assert_eq!(pulleys(Axis::Z), Axis::X);
    }

    #[test]
    fn belt_lines_of_different_lengths_carry_at_the_solved_speed_in_both_directions() {
        for axis in [Axis::X, Axis::Z] {
            for length in [1, 3, 20] {
                // Expected distances are independent of `carry`.
                for (speed, per_tick) in [
                    (16.0f32, 1.0f64 / 30.0),
                    (48.0, 0.1),
                    (256.0, 8.0 / 15.0),
                    (-16.0, -1.0 / 30.0),
                    (-48.0, -0.1),
                    (-256.0, -8.0 / 15.0),
                ] {
                    let (parts, belts) = line(axis, length, speed);
                    assert_eq!(belts.len(), length as usize);
                    let way = if axis == Axis::X {
                        [-per_tick, 0.0, 0.0]
                    } else {
                        [0.0, 0.0, per_tick]
                    };
                    let positive = way[0] + way[2] > 0.0;
                    let start = point(axis, if positive { 0.5 } else { f64::from(length) - 0.5 });
                    let mut dropped = [item(7, start, 12)];
                    let scene = Scene::new(&parts);
                    let ticks = ((f64::from(length - 1) / per_tick.abs()) as u32).max(1);
                    for tick in 1..=ticks {
                        convey(&belts, &mut dropped);
                        close(dropped[0].velocity, way);
                        scene.tick(&mut dropped);
                        close(
                            dropped[0].position,
                            [
                                start.x + way[0] * f64::from(tick),
                                TOP,
                                start.z + way[2] * f64::from(tick),
                            ],
                        );
                    }
                    assert_eq!(dropped[0].stack, ItemStack::new("minecraft:stone", 12));
                }
            }
        }
    }

    #[test]
    fn the_belt_under_the_middle_wins_and_only_horizontal_velocity_changes() {
        for (axis, at, position, expected) in [
            (Axis::X, [1, 0, 0], Vec3::new(1.05, TOP, 0.75), [-0.2, -0.3, -0.05]),
            (Axis::Z, [0, 0, 1], Vec3::new(0.75, TOP, 1.05), [-0.05, -0.3, 0.2]),
        ] {
            let belts = HashMap::from([
                ([0, 0, 0], carry(axis, 48.0)),
                (at, carry(axis, 96.0)),
            ]);
            let mut dropped = [item(7, position, 3)];
            dropped[0].velocity = Vec3::new(9.0, -0.3, 7.0);
            convey(&belts, &mut dropped);
            close(dropped[0].velocity, expected);
            close(dropped[0].position, [position.x, position.y, position.z]);
            close(dropped[0].previous, [position.x, position.y, position.z]);
            // Above the 3/4 cutoff, away from a belt, or without power: untouched.
            for at in [Vec3::new(0.5, 0.750_001, 0.5), point(axis, 4.5)] {
                dropped[0].position = at;
                convey(&belts, &mut dropped);
                close(dropped[0].velocity, expected);
            }
            let (parts, stopped) = line(axis, 3, 0.0);
            assert!(stopped.is_empty());
            assert!(moving(&parts, &HashMap::new()).is_empty());
            dropped[0].position = point(axis, 0.5);
            convey(&stopped, &mut dropped);
            close(dropped[0].velocity, expected);
        }
    }

    #[test]
    fn an_items_edge_keeps_it_moving_past_the_last_segment() {
        for axis in [Axis::X, Axis::Z] {
            let (_, belts) = line(axis, 1, 48.0);
            for (along, caught) in [
                (-0.125, true),
                (-0.125_001, false),
                (1.124_999, true),
                (1.125, false),
            ] {
                let mut dropped = [item(7, point(axis, along), 1)];
                convey(&belts, &mut dropped);
                close(
                    dropped[0].velocity,
                    if caught { carry(axis, 48.0) } else { [0.0; 3] },
                );
            }
        }
    }

    #[test]
    fn a_solid_end_stops_the_item_but_the_belt_keeps_turning() {
        for axis in [Axis::X, Axis::Z] {
            for speed in [-48.0, 48.0] {
                let (parts, belts) = line(axis, 3, speed);
                let way = carry(axis, speed);
                let positive = way[0] + way[2] > 0.0;
                let mut scene = Scene::new(&parts);
                let wall = if positive { 3 } else { -1 };
                scene.set(
                    if axis == Axis::X { [wall, 0, 0] } else { [0, 0, wall] },
                    BlockStateId(2),
                );
                let mut dropped = [item(7, point(axis, if positive { 0.5 } else { 2.5 }), 12)];
                for _ in 0..40 {
                    convey(&belts, &mut dropped);
                    close(dropped[0].velocity, way);
                    scene.tick(&mut dropped);
                }
                let stop = point(axis, if positive { 2.875 } else { 0.125 });
                close(dropped[0].position, [stop.x, stop.y, stop.z]);
                close(dropped[0].velocity, [0.0; 3]);
                assert!(dropped[0].on_ground);
                assert_eq!(dropped[0].stack.count, 12);
                assert_eq!(belts.len(), 3);
            }
        }
    }

    #[test]
    fn an_open_end_drops_the_item_instead_of_holding_or_removing_it() {
        for axis in [Axis::X, Axis::Z] {
            for length in [1, 3, 20] {
                for speed in [-48.0, 48.0] {
                    let (parts, belts) = line(axis, length, speed);
                    let way = carry(axis, speed);
                    let positive = way[0] + way[2] > 0.0;
                    let start = point(axis, if positive { 0.5 } else { f64::from(length) - 0.5 });
                    let mut dropped = [item(7, start, 12)];
                    let scene = Scene::new(&parts);
                    for _ in 0..length * 10 + 8 {
                        convey(&belts, &mut dropped);
                        scene.tick(&mut dropped);
                    }
                    let d = &dropped[0];
                    let along = if axis == Axis::X { d.position.x } else { d.position.z };
                    assert!(if positive {
                        along > f64::from(length) + 0.125
                    } else {
                        along < -0.125
                    });
                    assert!(d.position.y < TOP && d.velocity.y < 0.0 && !d.on_ground);
                    let velocity = d.velocity;
                    convey(&belts, &mut dropped);
                    close(dropped[0].velocity, [velocity.x, velocity.y, velocity.z]);
                    assert_eq!(dropped[0].stack.count, 12);
                }
            }
        }
    }

    #[test]
    fn funnels_put_onto_and_take_from_a_belt_without_losing_items() {
        let (parts, belts) = line(Axis::X, 4, -48.0);
        let scene = Scene::new(&parts);
        let mut funnels = Funnels::default();
        let mut source = Mill::default();
        source.put("minecraft:stone", 12);
        funnels.depots.insert((-1, 0, 0), source);
        funnels.depots.insert((5, 0, 0), Mill::default());
        funnel(&mut funnels, [0, 0, 0], Dir::East, true, 8);
        assert_eq!(funnels.dropped.len(), 1);
        close(funnels.dropped[0].position, [0.8, 0.7, 0.5]);
        close(funnels.dropped[0].velocity, [0.12, 0.0, 0.0]);
        assert_eq!(funnels.dropped[0].pickup_delay, 10);
        for tick in 9..=48 {
            convey(&belts, &mut funnels.dropped);
            funnel(&mut funnels, [4, 0, 0], Dir::West, false, tick);
            scene.tick(&mut funnels.dropped);
        }
        assert!(funnels.dropped.is_empty());
        assert!(funnels.depots[&(-1, 0, 0)].is_empty());
        assert_eq!(funnels.depots[&(5, 0, 0)].input.as_ref().unwrap().count, 12);
        assert_eq!(funnels.takes, [((-1, 0, 0), Direction::East, 64)]);
        assert_eq!(funnels.puts.len(), 1);
        assert_eq!(funnels.puts[0].0, (5, 0, 0));
        assert_eq!(funnels.puts[0].1, Direction::West);
    }

    #[test]
    fn a_full_funnel_rejects_the_stack_but_does_not_hold_it_on_the_belt() {
        // `BeltInventory` holds a rejected insertion. Tachyon does not yet:
        // without a solid obstacle the dropped stack carries on and falls.
        let (parts, belts) = line(Axis::X, 3, -48.0);
        let scene = Scene::new(&parts);
        let mut funnels = Funnels::default();
        let mut full = Mill::default();
        full.put("minecraft:stone", 64);
        funnels.depots.insert((4, 0, 0), full);
        funnels.dropped.push(item(7, point(Axis::X, 2.5), 12));
        for tick in 1..=20 {
            convey(&belts, &mut funnels.dropped);
            funnel(&mut funnels, [3, 0, 0], Dir::West, false, tick);
            scene.tick(&mut funnels.dropped);
        }
        assert!(!funnels.puts.is_empty());
        assert_eq!(funnels.depots[&(4, 0, 0)].input.as_ref().unwrap().count, 64);
        assert_eq!(funnels.dropped.len(), 1);
        assert_eq!(funnels.dropped[0].stack, ItemStack::new("minecraft:stone", 12));
        assert!(funnels.dropped[0].position.x > 3.125);
        assert!(funnels.dropped[0].position.y < TOP);
    }

    #[test]
    fn insertion_keeps_remainders_and_removes_accepted_items_last_first() {
        let mut funnels = Funnels::default();
        let mut depot = Mill::default();
        depot.put("minecraft:stone", 59);
        funnels.depots.insert((1, 0, 0), depot);
        let near = Vec3::new(0.5, TOP, 0.5);
        let far = Vec3::new(3.5, TOP, 0.5);
        funnels.dropped = vec![
            item(1, far, 1),
            item(2, near, 4),
            item(3, far, 1),
            item(4, near, 3),
            item(5, near, 2),
        ];
        funnels.dropped[4].stack = ItemStack::new("minecraft:dirt", 2);
        funnel(&mut funnels, [0, 0, 0], Dir::West, false, 1);
        assert_eq!(
            funnels.dropped.iter().map(|d| (d.id, d.stack.count)).collect::<Vec<_>>(),
            [(1, 1), (2, 2), (3, 1), (5, 2)],
        );
        assert_eq!(
            funnels.puts.iter().map(|(_, _, s)| s.count).collect::<Vec<_>>(),
            [2, 3, 4],
        );
        assert_eq!(funnels.dropped[1].stack, ItemStack::new("minecraft:stone", 2));
        assert_eq!(funnels.dropped[3].stack, ItemStack::new("minecraft:dirt", 2));
        assert_eq!(funnels.depots[&(1, 0, 0)].input.as_ref().unwrap().count, 64);
        // No container behind it: nothing is removed or changed.
        let stacks: Vec<_> = funnels.dropped.iter().map(|d| d.stack.clone()).collect();
        funnel(&mut funnels, [0, 0, 0], Dir::East, false, 2);
        assert_eq!(
            funnels.dropped.iter().map(|d| d.stack.clone()).collect::<Vec<_>>(),
            stacks,
        );
    }

    #[test]
    fn extraction_waits_for_the_eighth_tick_and_an_empty_mouth_and_takes_at_most_64() {
        let mut funnels = Funnels::default();
        let source = Mill {
            outputs: vec![
                Held { item: "minecraft:stone".into(), count: 40 },
                Held { item: "minecraft:stone".into(), count: 30 },
            ],
            ..Mill::default()
        };
        funnels.depots.insert((-1, 0, 0), source);
        for tick in 1..8 {
            funnel(&mut funnels, [0, 0, 0], Dir::East, true, tick);
        }
        assert!(funnels.takes.is_empty());
        funnel(&mut funnels, [0, 0, 0], Dir::East, true, 8);
        assert_eq!(funnels.dropped[0].stack.count, 64);
        assert_eq!(funnels.depots[&(-1, 0, 0)].outputs[0].count, 6);
        funnel(&mut funnels, [0, 0, 0], Dir::East, true, 16);
        assert_eq!(funnels.takes.len(), 1);
        funnels.dropped[0].position.x = 3.0;
        funnel(&mut funnels, [0, 0, 0], Dir::East, true, 17);
        assert_eq!(funnels.takes.len(), 1);
        funnel(&mut funnels, [0, 0, 0], Dir::East, true, 24);
        assert_eq!(funnels.dropped[1].stack.count, 6);
        assert!(funnels.depots[&(-1, 0, 0)].is_empty());
        funnels.dropped.clear();
        funnel(&mut funnels, [0, 0, 0], Dir::East, true, 32);
        assert!(funnels.dropped.is_empty());
    }

    #[test]
    fn a_machine_funnel_returns_the_first_different_item_to_its_original_slot() {
        let mut funnels = Funnels::default();
        let source = Mill {
            input: Some(Held { item: "minecraft:dirt".into(), count: 3 }),
            outputs: vec![Held { item: "minecraft:stone".into(), count: 2 }],
            ..Mill::default()
        };
        funnels.depots.insert((-1, 0, 0), source);
        assert_eq!(funnels.take((-1, 0, 0), Direction::East, 1).unwrap().count, 1);
        assert_eq!(
            funnels.take((-1, 0, 0), Direction::East, 64).unwrap(),
            ItemStack::new("minecraft:stone", 1),
        );
        let source = &funnels.depots[&(-1, 0, 0)];
        assert_eq!(
            source.input.as_ref().unwrap(),
            &Held { item: "minecraft:dirt".into(), count: 3 },
        );
        assert!(source.outputs.is_empty());
        assert_eq!(
            funnels.take((-1, 0, 0), Direction::East, 64).unwrap(),
            ItemStack::new("minecraft:dirt", 3),
        );
    }

    #[test]
    fn a_funnels_container_face_and_output_follow_all_six_directions() {
        for (facing, face, behind, at, velocity) in [
            (Dir::Down, Direction::Down, (4, 6, 6), [4.5, 4.7, 6.5], [0.0, -0.12, 0.0]),
            (Dir::Up, Direction::Up, (4, 4, 6), [4.5, 6.3, 6.5], [0.0, 0.12, 0.0]),
            (Dir::North, Direction::North, (4, 5, 7), [4.5, 5.7, 6.2], [0.0, 0.0, -0.12]),
            (Dir::South, Direction::South, (4, 5, 5), [4.5, 5.7, 6.8], [0.0, 0.0, 0.12]),
            (Dir::West, Direction::West, (5, 5, 6), [4.2, 5.7, 6.5], [-0.12, 0.0, 0.0]),
            (Dir::East, Direction::East, (3, 5, 6), [4.8, 5.7, 6.5], [0.12, 0.0, 0.0]),
        ] {
            let mut funnels = Funnels::default();
            let mut source = Mill::default();
            source.put("minecraft:stone", 2);
            funnels.depots.insert(behind, source);
            funnel(&mut funnels, [4, 5, 6], facing, true, 8);
            assert_eq!(funnels.takes, [(behind, face, 64)]);
            assert_eq!(funnels.dropped.len(), 1);
            close(funnels.dropped[0].position, at);
            close(funnels.dropped[0].velocity, velocity);
            assert_eq!(funnels.dropped[0].pickup_delay, 10);
            funnels.dropped[0].position = Vec3::new(4.5, 5.5, 6.5);
            funnel(&mut funnels, [4, 5, 6], facing, false, 9);
            assert_eq!(funnels.puts, [(behind, face, ItemStack::new("minecraft:stone", 2))]);
            assert!(funnels.dropped.is_empty());
            assert_eq!(funnels.depots[&behind].input.as_ref().unwrap().count, 2);
        }
    }

    #[test]
    fn a_funnel_reaches_a_little_round_its_block() {
        let lo = [1.6, 0.5, 0.4];
        let hi = [1.85, 0.75, 0.65];
        assert!(meets([1, 0, 0], lo, hi, 0.25));
        // Just past its west side.
        assert!(meets([2, 0, 0], lo, hi, 0.25));
        assert!(!meets([2, 0, 0], lo, hi, 0.0));
        assert!(!meets([3, 0, 0], lo, hi, 0.25));
        // Touching the grown block is not enough: all axes need overlap.
        for axis in 0..3 {
            let mut lo = [0.4; 3];
            let mut hi = [0.65; 3];
            lo[axis] = 1.25;
            hi[axis] = 1.5;
            assert!(!meets([0; 3], lo, hi, 0.25));
            lo[axis] = 1.249_999;
            assert!(meets([0; 3], lo, hi, 0.25));
            lo[axis] = -0.5;
            hi[axis] = -0.25;
            assert!(!meets([0; 3], lo, hi, 0.25));
            hi[axis] = -0.249_999;
            assert!(meets([0; 3], lo, hi, 0.25));
        }
    }
}
