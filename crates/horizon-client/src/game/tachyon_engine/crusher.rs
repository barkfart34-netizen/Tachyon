//! Create's crushing wheels, turned by Kinetica's engine, working as
//! Create's (`CrushingWheelBlock`, `CrushingWheelControllerBlock`,
//! `CrushingWheelControllerBlockEntity`; Create's code is MIT-licensed,
//! THIRD_PARTY_NOTICES.md): two wheels on parallel shafts with a block of
//! room between them make a gap there. While they turn against each other
//! the gap takes in a dropped item that comes into it, a whole stack, and
//! crushes it by Create's crushing recipes (else its milling ones), slower
//! the bigger the stack; what it made comes out of the gap the way their
//! rims go there, and passes the gap. A stack no recipe crushes is lost, as
//! in Create. Stopped, the gap keeps what it holds; with its wheels gone it
//! drops it. A window shows four fragments a tick of the stack being
//! crushed. An empty turning gap keeps one living entity and hurts it by
//! Create's crushing damage. Creative players, spectators and entities in
//! peaceful worlds are not hurt.
//!
//! Unlike Create's, no block fills the gap: the owner keeps the stack it
//! took in (saved with the machines), the items waiting for it are held in
//! the gap, a stack with components passes (the owner keeps items by name
//! and count), and a living entity is hurt in place: pulling it through and
//! putting it out before death are not ported yet.
use super::super::Game;
use super::create_recipes::Recipes;
use super::kinetic::{Axis, Dir, Part, Speeds};
use super::{OWNERS, Owner, add, positive};
use horizon_client::entities::ItemEntity;
use horizon_client::items::ItemStack;
use horizon_client::lan::data::SyncData;
use horizon_client::mobs::{Damage, DamageKind};
use horizon_client::particles::{ParticleOptions, ParticleType};
use horizon_core::geometry::{Aabb, Vec3};
use horizon_core::random::RandomSource;
use horizon_world::damage_source::{Catalog, Definition, Scaling};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// The countdown of a stack no recipe crushes (Create's).
const NO_RECIPE: f32 = 100.0;
/// Under this the stack is crushed and what it made comes out (Create's
/// applies the recipe then, emptying its timer, and puts it out the next
/// tick).
const DONE: f32 = 20.0;
/// The damage each tick, before the living entity's hurt cooldown
/// (Create's `CKinetics.crushingDamage`).
const DAMAGE: f32 = 4.0;
/// `CreateDamageSources.crush`: the registry id, not its message id `create.crush`.
const DAMAGE_TYPE: &str = "create:crush";
/// `DamageTypeTagGen`: crushing bypasses armor, but not the hurt cooldown.
const DAMAGE_TAGS: &[&str] = &["minecraft:bypasses_armor"];

/// `AllDamageTypes` and `DamageTypeBuilder`: the crushing source's defaults.
fn damage_definition() -> Definition {
    Definition {
        name: DAMAGE_TYPE.into(),
        message_id: "create.crush".into(),
        exhaustion: 0.0,
        scaling: Scaling::Always,
        effects: "hurt".into(),
        death_message_type: "default".into(),
    }
}

/// A turning gap: the way it sends out what it crushed, and how fast it
/// crushes (Create's: its wheel's RPM / 50, times 4).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Gap {
    pub out: Dir,
    pub speed: f32,
}

/// What a gap holds: the stack it took in, and what is left of its
/// countdown.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(super) struct Inside {
    pub item: String,
    pub count: u32,
    pub left: f32,
}

/// A living occupant of a gap: a mob's id, or a player's UUID (the host and
/// guests alike).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum Living {
    Mob(u32),
    Player([i32; 4]),
}

/// What came out of a gap: the gap, the stacks, where, and how fast.
type Out = ([i32; 3], Vec<ItemStack>, [f64; 3], [f64; 3]);

/// The wheels round a gap at `gap`: their axis, and the side of the gap the
/// first of them is on in Create's order of directions (the other is
/// across). None when no two wheels on one axis face each other over it.
pub(super) fn pair_round(parts: &HashMap<[i32; 3], Part>, gap: [i32; 3]) -> Option<(Axis, Dir)> {
    Dir::ALL.into_iter().find_map(|d| {
        let Some(Part::CrushingWheel(axis)) = parts.get(&add(gap, d.offset())).copied() else {
            return None;
        };
        let across = parts.get(&add(gap, d.opposite().offset()));
        (d.axis() != axis && across == Some(&Part::CrushingWheel(axis))).then_some((axis, d))
    })
}

/// The gaps between wheels, by position: each turning (Some) or not.
pub(super) fn gaps(parts: &HashMap<[i32; 3], Part>, speeds: &Speeds) -> HashMap<[i32; 3], Option<Gap>> {
    let mut gaps = HashMap::new();
    for (p, part) in parts {
        let Part::CrushingWheel(axis) = *part else {
            continue;
        };
        for d in Dir::ALL.into_iter().filter(|d| d.axis() != axis) {
            let gap = add(*p, d.offset());
            if gaps.contains_key(&gap) {
                continue;
            }
            if let Some(pair) = pair_round(parts, gap) {
                gaps.insert(gap, turning(gap, pair, speeds));
            }
        }
    }
    gaps
}

/// How a gap with wheels along `axis` on its side `d` and across works, as
/// Create's controller: only while they turn against each other, at the
/// speed of the wheel on side `d`, the way its rim goes in the gap.
fn turning(gap: [i32; 3], (axis, d): (Axis, Dir), speeds: &Speeds) -> Option<Gap> {
    let speed = |p: [i32; 3]| speeds.get(&p).map_or(0.0, |s| s[0]);
    let (s1, s2) = (speed(add(gap, d.offset())), speed(add(gap, d.opposite().offset())));
    if s1 == 0.0 || s2 == 0.0 || (s1 > 0.0) == (s2 > 0.0) {
        return None;
    }
    Some(Gap {
        out: rim(axis, d.opposite(), s1),
        speed: s1.abs() / 50.0 * 4.0,
    })
}

/// The way the rim of a wheel along `axis` turning at `speed` goes on its
/// side `d` (Create's controller facing: positive axis × side, by the
/// speed's sign).
fn rim(axis: Axis, d: Dir, speed: f32) -> Dir {
    let a = positive(axis).offset();
    let o = d.offset();
    let s = if speed > 0.0 { 1 } else { -1 };
    let way = [
        (a[1] * o[2] - a[2] * o[1]) * s,
        (a[2] * o[0] - a[0] * o[2]) * s,
        (a[0] * o[1] - a[1] * o[0]) * s,
    ];
    Dir::ALL
        .into_iter()
        .find(|d| d.offset() == way)
        .unwrap_or(Dir::Down)
}

/// A tick of crushing a stack of `count` at `speed`: what is left of the
/// countdown, down by `speed / log2(count)` (a lone item's by 20), 0.25 to
/// 20 a tick (Create's).
pub(super) fn step(left: f32, speed: f32, count: u32) -> f32 {
    let per = if count <= 1 {
        f32::INFINITY
    } else {
        speed / (count as f32).log2()
    };
    left - per.clamp(0.25, 20.0)
}

/// Where what was crushed comes out of a gap at `gap` sent `out`, and how
/// fast (Create's: just past the gap's middle that way; 0.25 a tick
/// sideways, 0.5 up, dropped down).
fn exit(gap: [i32; 3], out: Dir) -> ([f64; 3], [f64; 3]) {
    let o = out.offset().map(f64::from);
    let at = [0, 1, 2].map(|i| f64::from(gap[i]) + 0.5 + 0.55 * o[i]);
    let velocity = match out {
        Dir::Up => [0.0, 0.5, 0.0],
        Dir::Down => [0.0; 3],
        _ => [o[0] * 0.25, 0.0, o[2] * 0.25],
    };
    (at, velocity)
}

/// The stacks of `count` of `item`, at most 64 each.
fn stacks(item: &str, mut count: u32) -> Vec<ItemStack> {
    let mut out = Vec::new();
    while count > 0 {
        let n = count.min(64);
        out.push(ItemStack::new(item, n));
        count -= n;
    }
    out
}

/// The controller's block box (`CrushingWheelControllerBlockEntity`).
fn bounds(p: [i32; 3]) -> Aabb {
    let [x, y, z] = p.map(f64::from);
    Aabb::unit_cube(Vec3::new(x, y, z))
}

/// `CrushingWheelControllerBlock.checkEntityForProcessing`: no creative
/// player enters. Peaceful excludes mobs too here; spectators never touch
/// the controller.
fn vulnerable(alive: bool, creative: bool, spectator: bool, peaceful: bool) -> bool {
    alive && !creative && !spectator && !peaceful
}

/// The living occupants hurt this tick. As Create's controller, a gap
/// holding a stack or another entity takes no new entity, and keeps its
/// living occupant until it dies or leaves the block box inflated by 0.5.
fn living(
    owner: &mut Owner,
    pairs: &HashSet<[i32; 3]>,
    turning: &HashMap<[i32; 3], Gap>,
    entities: &[(Living, Aabb)],
) -> HashSet<Living> {
    let occupied = &owner.crushers;
    owner.crushing_entities.retain(|p, who| {
        pairs.contains(p)
            && !occupied.contains_key(p)
            && entities.iter().any(|(e, b)| {
                *e == *who && b.intersects(bounds(*p).inflate(Vec3::new(0.5, 0.5, 0.5)))
            })
    });
    let mut hurt = HashSet::new();
    for p in turning.keys() {
        if occupied.contains_key(p) {
            continue;
        }
        let who = owner.crushing_entities.get(p).copied().or_else(|| {
            (entities.iter())
                .find(|(_, b)| b.intersects(bounds(*p)))
                .map(|(e, _)| *e)
        });
        if let Some(who) = who {
            owner.crushing_entities.insert(*p, who);
            hurt.insert(who);
        }
    }
    hurt
}

/// The stacks whose turning gaps a window shows, from the owner's view or
/// the speeds and held item names a guest received.
fn particle_stacks(
    parts: &HashMap<[i32; 3], Part>,
    speeds: &Speeds,
    held: &HashMap<[i32; 3], String>,
) -> Vec<([i32; 3], String)> {
    let turning = gaps(parts, speeds);
    (held.iter())
        .filter(|(p, _)| turning.get(*p).is_some_and(Option::is_some))
        .map(|(p, item)| (*p, item.clone()))
        .collect()
}

/// `CrushingWheelControllerBlockEntity.spawnParticles`: four random points
/// in the controller's block, drawing floats in X, Y, Z order and adding
/// them as floats before converting to the particle engine's doubles.
fn particle_positions(gap: [i32; 3], random: &mut impl RandomSource) -> [[f64; 3]; 4] {
    std::array::from_fn(|_| gap.map(|v| f64::from(v as f32 + random.next_float())))
}

/// A tick of the turning gaps: the stacks in them crushed (what they made
/// into `out`), the dropped items in them taken in by an empty one, else
/// held there till it is. Whether a stack went in or out.
fn crush(
    owner: &mut Owner,
    create: &Recipes,
    turning: &HashMap<[i32; 3], Gap>,
    dropped: &mut [ItemEntity],
    random: &mut impl RandomSource,
    out: &mut Vec<Out>,
) -> bool {
    let mut moved = false;
    for (p, gap) in turning {
        let Some(inside) = owner.crushers.get_mut(p) else {
            continue;
        };
        inside.left = step(inside.left, gap.speed, inside.count);
        owner.mills_changed = true;
        if inside.left >= DONE {
            continue;
        }
        let Some(inside) = owner.crushers.remove(p) else {
            continue;
        };
        moved = true;
        // Each item of the stack by the recipe's chances; nothing of a
        // stack no recipe crushes.
        let recipe = create.crushing(&inside.item).or_else(|| create.milling(&inside.item));
        let mut made: Vec<ItemStack> = Vec::new();
        for o in recipe.map_or(&[][..], |r| r.outputs.as_slice()) {
            let times = if o.chance >= 1.0 {
                inside.count
            } else {
                (0..inside.count).filter(|_| random.next_float() <= o.chance).count() as u32
            };
            made.extend(stacks(&o.item, o.count * times));
        }
        let (at, velocity) = exit(*p, gap.out);
        out.push((*p, made, at, velocity));
    }
    for item in dropped.iter_mut() {
        let at = item.position;
        let cell = [at.x.floor() as i32, at.y.floor() as i32, at.z.floor() as i32];
        if !turning.contains_key(&cell) || owner.passed.get(&item.id) == Some(&cell) {
            continue;
        }
        if item.stack.extra.is_some() {
            owner.passed.insert(item.id, cell);
            continue;
        }
        if owner.crushers.contains_key(&cell) || owner.crushing_entities.contains_key(&cell) {
            item.position.x = f64::from(cell[0]) + 0.5;
            item.position.z = f64::from(cell[2]) + 0.5;
            item.position.y = item.position.y.max(f64::from(cell[1]) + 0.25);
            item.previous = item.position;
            item.velocity.x = 0.0;
            item.velocity.y = 0.0;
            item.velocity.z = 0.0;
            continue;
        }
        let name = item.stack.item.to_string();
        let recipe = create.crushing(&name).or_else(|| create.milling(&name));
        owner.crushers.insert(cell, Inside {
            item: name,
            count: item.stack.count,
            left: recipe.map_or(NO_RECIPE, |r| r.time),
        });
        moved = true;
        // Gone into the gap.
        item.stack.count = 0;
    }
    moved
}

impl Game<'_> {
    /// Create's crushing source (`AllDamageTypes`, `DamageTypeBuilder`,
    /// `DamageTypeTagGen`), added before the movement resources are shared.
    pub(in crate::game) fn tachyon_crushing_damage(damage: Catalog) -> std::io::Result<Catalog> {
        if damage.id(DAMAGE_TYPE).is_some() {
            return Ok(damage);
        }
        damage.with_definition(damage_definition(), DAMAGE_TAGS)
    }

    /// The same crushing source on the wire, so server damage events reach
    /// the host's window and LAN guests with Create's source name.
    pub(in crate::game) fn tachyon_crushing_sync(sync: &mut SyncData) {
        if sync.id("minecraft:block", "create:crushing_wheel").is_some() {
            sync.add_damage_type(&damage_definition(), DAMAGE_TAGS);
        }
    }

    /// The owner's crushing wheels: each turning gap takes in a dropped item
    /// and crushes it, or hurts a living occupant; the other items coming
    /// into it wait there. What a gap held whose wheels are gone (their
    /// chunks loaded) drops there.
    pub(super) fn tachyon_crushers(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let found = OWNERS.with_borrow(|owners| {
            (owners.get(dimension))
                .map(|o| gaps(&o.index.parts, &o.shown.speeds))
                .unwrap_or_default()
        });
        let pairs: HashSet<[i32; 3]> = found.keys().copied().collect();
        // A gap with a block in it is none (Create's controller takes the
        // place of what was there).
        let facts = self.res.facts();
        let turning: HashMap<[i32; 3], Gap> = (found.into_iter())
            .filter_map(|(p, gap)| Some((p, gap?)))
            .filter(|(p, _)| facts.get(self.world.block(p[0], p[1], p[2])).replaceable)
            .collect();
        let mut entities = Vec::new();
        if !pairs.is_empty() {
            let peaceful = self.ui.difficulty == 0;
            entities.extend(
                (self.mobs.iter())
                    .filter(|m| vulnerable(m.alive() && !m.removed, false, false, peaceful))
                    .map(|m| (Living::Mob(m.id), m.bounds())),
            );
            self.for_each_player(|g| {
                if let Some(p) = g.player.as_ref()
                    && vulnerable(!p.health.dead_or_dying(), g.creative, g.spectator(), peaceful)
                {
                    entities.push((Living::Player(g.acting_uuid()), p.bounds()));
                }
            });
        }
        let present: HashSet<u32> = self.dropped.iter().map(|d| d.id).collect();
        let create = (!turning.is_empty()).then(|| self.tachyon_create_recipes());
        let chunks = &self.world.chunks;
        let loaded = |p: [i32; 3]| {
            [[0, 0], [-1, 0], [1, 0], [0, -1], [0, 1]]
                .iter()
                .all(|[x, z]| chunks.contains_key(&((p[0] + x) >> 4, (p[2] + z) >> 4)))
        };
        let dropped = &mut self.dropped;
        let random = &mut self.world.random;
        let mut out: Vec<Out> = Vec::new();
        // What the gaps whose wheels are gone held.
        let mut lost: Vec<([i32; 3], ItemStack)> = Vec::new();
        let hurt = OWNERS.with_borrow_mut(|owners| {
            let Some(owner) = owners.get_mut(dimension) else {
                return HashSet::new();
            };
            owner.passed.retain(|id, _| present.contains(id));
            owner.gaps = turning.keys().copied().collect();
            let gone: Vec<[i32; 3]> = (owner.crushers.keys())
                .filter(|p| !pairs.contains(*p) && loaded(**p))
                .copied()
                .collect();
            for p in &gone {
                if let Some(inside) = owner.crushers.remove(p) {
                    lost.extend(stacks(&inside.item, inside.count).into_iter().map(|s| (*p, s)));
                }
            }
            let hurt = living(owner, &pairs, &turning, &entities);
            let moved = create
                .is_some_and(|create| crush(owner, &create, &turning, dropped, random, &mut out));
            // Saved at once when items left the world or came back to it.
            if moved || !gone.is_empty() {
                owner.save(&root, dimension);
            }
            hurt
        });
        for (p, stack) in lost {
            self.pop_resource(p, stack);
        }
        self.dropped.retain(|d| d.stack.count > 0);
        // What came out passes the gap it came from.
        for (gap, made, at, velocity) in out {
            for stack in made {
                self.spawn_item(stack, at, velocity, 0);
                if let Some(id) = self.dropped.last().map(|d| d.id) {
                    OWNERS.with_borrow_mut(|owners| {
                        if let Some(owner) = owners.get_mut(dimension) {
                            owner.passed.insert(id, gap);
                        }
                    });
                }
            }
        }
        if hurt.is_empty() {
            return;
        }
        for mob in &mut self.mobs {
            if hurt.contains(&Living::Mob(mob.id)) {
                mob.damage(DAMAGE, Damage::of(DamageKind::Crush));
            }
        }
        self.for_each_player(|g| {
            if hurt.contains(&Living::Player(g.acting_uuid())) && !g.creative && !g.spectator() {
                g.hurt_player_from(DAMAGE, None, DAMAGE_TYPE, Some(None));
            }
        });
    }

    /// A window's held stacks (`CrushingWheelControllerBlockEntity.spawnParticles`):
    /// block fragments for block items, item fragments otherwise, four a
    /// tick with no initial velocity. Cosmetic draws stay off the world RNG.
    pub(super) fn tachyon_crusher_particles(&mut self) {
        if self.window.is_none() {
            return;
        }
        let held = self.tachyon_view(|index, shown| {
            particle_stacks(&index.parts, &shown.speeds, &shown.crushers)
        });
        for (gap, item) in held {
            if !self.res.facts().get(self.world.block(gap[0], gap[1], gap[2])).replaceable {
                continue;
            }
            let catalog = self.res.items();
            let Some(info) = catalog.get(&item) else {
                continue;
            };
            let options = if let Some(state) = info.block {
                let Some((sprite, tint)) = self.particle_sprite(state, gap) else {
                    continue;
                };
                ParticleOptions::block(ParticleType::Block, sprite, tint)
            } else {
                self.item_particle(&ItemStack::new(&item, 1))
            };
            for at in particle_positions(gap, self.particles.random()) {
                self.particles.add(&options, at, [0.0; 3]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheels_turning_against_each_other_make_a_gap_between_them() {
        let parts = HashMap::from([
            ([0, 0, 0], Part::CrushingWheel(Axis::Z)),
            ([2, 0, 0], Part::CrushingWheel(Axis::Z)),
        ]);
        let speeds = HashMap::from([([0, 0, 0], [-32.0; 2]), ([2, 0, 0], [32.0; 2])]);
        let found = gaps(&parts, &speeds);
        assert_eq!(found.len(), 1);
        let gap = found[&[1, 0, 0]].unwrap();
        // Their rims go down in the gap: what they crush drops.
        assert_eq!(gap.out, Dir::Down);
        assert!((gap.speed - 2.56).abs() < 1e-4);
        // Both turning the same way: a gap that doesn't work.
        let same = HashMap::from([([0, 0, 0], [32.0; 2]), ([2, 0, 0], [32.0; 2])]);
        assert_eq!(gaps(&parts, &same)[&[1, 0, 0]], None);
        // The other way round, up.
        let up = HashMap::from([([0, 0, 0], [32.0; 2]), ([2, 0, 0], [-32.0; 2])]);
        assert_eq!(gaps(&parts, &up)[&[1, 0, 0]].unwrap().out, Dir::Up);
        // At two speeds, the west wheel's (Create's order: down, up, north,
        // south, west, east).
        let two = HashMap::from([([0, 0, 0], [-8.0; 2]), ([2, 0, 0], [256.0; 2])]);
        assert!((gaps(&parts, &two)[&[1, 0, 0]].unwrap().speed - 0.64).abs() < 1e-4);
        // Wheels on different axes make none.
        let crossed = HashMap::from([
            ([0, 0, 0], Part::CrushingWheel(Axis::Z)),
            ([2, 0, 0], Part::CrushingWheel(Axis::Y)),
        ]);
        assert!(gaps(&crossed, &speeds).is_empty());
    }

    #[test]
    fn a_stack_is_crushed_slower_the_bigger_it_is() {
        // A lone item: 20 a tick, whatever the speed.
        assert_eq!(step(400.0, 2.56, 1), 380.0);
        // Four: 2.56 / 2 a tick.
        assert!((step(400.0, 2.56, 4) - 398.72).abs() < 1e-3);
        // Never under 0.25.
        assert_eq!(step(400.0, 0.1, 64), 399.75);
        let (at, velocity) = exit([1, 5, 0], Dir::Down);
        assert!((at[1] - 4.95).abs() < 1e-9 && at[0] == 1.5 && at[2] == 0.5);
        assert_eq!(velocity, [0.0; 3]);
        assert_eq!(exit([0, 0, 0], Dir::East).1, [0.25, 0.0, 0.0]);
        assert_eq!(stacks("x:a", 130).len(), 3);
    }

    #[test]
    fn four_particles_draw_float_positions_inside_the_gap() {
        let gap = [-3, 64, 7];
        let mut random = horizon_core::random::LegacyRandom::new(17);
        let points = particle_positions(gap, &mut random);
        let mut expected = horizon_core::random::LegacyRandom::new(17);
        for p in points {
            assert_eq!(p, gap.map(|v| f64::from(v as f32 + expected.next_float())));
            for i in 0..3 {
                assert!((f64::from(gap[i])..f64::from(gap[i]) + 1.0).contains(&p[i]));
            }
        }
        // Exactly twelve float draws, without touching the world's random.
        assert_eq!(random.next_float(), expected.next_float());
    }

    #[test]
    fn only_held_stacks_between_turning_wheels_have_particles() {
        let mut parts = HashMap::from([
            ([0, 0, 0], Part::CrushingWheel(Axis::Z)),
            ([2, 0, 0], Part::CrushingWheel(Axis::Z)),
        ]);
        let speeds = HashMap::from([([0, 0, 0], [-32.0; 2]), ([2, 0, 0], [32.0; 2])]);
        let held = HashMap::from([
            ([1, 0, 0], "minecraft:cobblestone".to_owned()),
            ([5, 0, 0], "minecraft:raw_iron".to_owned()),
        ]);
        assert_eq!(
            particle_stacks(&parts, &speeds, &held),
            [([1, 0, 0], "minecraft:cobblestone".to_owned())],
        );
        assert!(particle_stacks(&parts, &HashMap::new(), &held).is_empty());
        assert!(particle_stacks(&parts, &speeds, &HashMap::new()).is_empty());
        parts.remove(&[2, 0, 0]);
        assert!(particle_stacks(&parts, &speeds, &held).is_empty());
    }

    #[test]
    fn a_gap_keeps_one_living_occupant_until_it_leaves_the_enlarged_box() {
        let mut owner = Owner::default();
        let gap = [1, 0, 0];
        let pairs = HashSet::from([gap]);
        let turning = HashMap::from([(gap, Gap { out: Dir::Down, speed: 2.56 })]);
        let mob = Living::Mob(7);
        let player = Living::Player([1, 2, 3, 4]);
        let near = Aabb::new(Vec3::new(1.2, 0.2, 0.2), Vec3::new(1.8, 2.0, 0.8));
        let outside = Aabb::new(Vec3::new(2.1, 0.2, 0.2), Vec3::new(2.4, 2.0, 0.8));
        // Not already captured: the inflated box alone takes nobody in.
        assert!(living(&mut owner, &pairs, &turning, &[(mob, outside)]).is_empty());
        assert_eq!(
            living(&mut owner, &pairs, &turning, &[(mob, near), (player, near)]),
            HashSet::from([mob]),
        );
        // The captured mob is kept even with another entity first, and after
        // moving outside the block but still within the half-block margin.
        assert_eq!(
            living(&mut owner, &pairs, &turning, &[(player, near), (mob, outside)]),
            HashSet::from([mob]),
        );
        assert_eq!(owner.crushing_entities[&gap], mob);
        let gone = outside.move_by(Vec3::new(0.4, 0.0, 0.0));
        assert_eq!(
            living(&mut owner, &pairs, &turning, &[(mob, gone), (player, near)]),
            HashSet::from([player]),
        );
        // A dead entity is absent from the candidates and is released.
        assert!(living(&mut owner, &pairs, &turning, &[]).is_empty());
        assert!(owner.crushing_entities.is_empty());
    }

    #[test]
    fn a_stopped_or_stack_holding_gap_hurts_nobody() {
        let mut owner = Owner::default();
        let gap = [1, 0, 0];
        let pairs = HashSet::from([gap]);
        let turning = HashMap::from([(gap, Gap { out: Dir::Down, speed: 2.56 })]);
        let mob = Living::Mob(7);
        let entities = [(mob, bounds(gap))];
        assert_eq!(
            living(&mut owner, &pairs, &turning, &entities),
            HashSet::from([mob]),
        );
        assert!(living(&mut owner, &pairs, &HashMap::new(), &entities).is_empty());
        assert_eq!(owner.crushing_entities[&gap], mob);
        owner.crushers.insert(gap, Inside {
            item: "minecraft:stone".into(),
            count: 1,
            left: NO_RECIPE,
        });
        assert!(living(&mut owner, &pairs, &turning, &entities).is_empty());
        assert!(owner.crushing_entities.is_empty());
        owner.crushers.clear();
        living(&mut owner, &pairs, &turning, &entities);
        assert!(living(&mut owner, &HashSet::new(), &HashMap::new(), &entities).is_empty());
        assert!(owner.crushing_entities.is_empty());
    }

    #[test]
    fn creative_spectator_dead_and_peaceful_entities_are_not_crushed() {
        assert!(vulnerable(true, false, false, false));
        assert!(!vulnerable(false, false, false, false));
        assert!(!vulnerable(true, true, false, false));
        assert!(!vulnerable(true, false, true, false));
        assert!(!vulnerable(true, false, false, true));
    }

    #[test]
    fn crushing_has_creates_source_and_tags_without_changing_vanilla_ids() {
        let damage = Catalog::from_json(
            &serde_json::json!({
                "data_version": horizon_world::DATA_VERSION,
                "types": [{
                    "id": "minecraft:generic", "message_id": "generic",
                    "exhaustion": "00000000", "scaling": "never",
                    "effects": "hurt", "death_message_type": "default",
                }],
            }),
            &serde_json::json!({
                "minecraft:bypasses_armor": r#"{"values":["minecraft:generic"]}"#,
            }),
        )
        .unwrap();
        let generic = damage.id("minecraft:generic").unwrap();
        let damage = Game::tachyon_crushing_damage(damage).unwrap();
        assert_eq!(damage.id("minecraft:generic"), Some(generic));
        assert!(damage.in_tag(generic, "minecraft:bypasses_armor"));
        let crush = damage.id(DAMAGE_TYPE).unwrap();
        let source = damage.source::<()>(crush, None, None, None).unwrap();
        let def = source.definition();
        assert_eq!(def.message_id, "create.crush");
        assert_eq!(def.scaling, Scaling::Always);
        assert_eq!(def.exhaustion, 0.0);
        assert_eq!(def.effects, "hurt");
        assert_eq!(def.death_message_type, "default");
        assert!(source.in_tag("minecraft:bypasses_armor"));
        assert!(!source.in_tag("minecraft:no_knockback"));
        assert!(!source.in_tag("minecraft:bypasses_invulnerability"));
        assert_eq!(DAMAGE, 4.0);
        let mob_source = Damage::of(DamageKind::Crush);
        assert!(mob_source.bypasses_armor());
        assert!(!mob_source.no_knockback());
        assert!(!mob_source.causes_panic());
        assert!(!mob_source.kind.bypasses_invulnerability());
        assert_eq!(
            crate::game::creatures::damage_type(mob_source.kind, false),
            DAMAGE_TYPE,
        );
        let damage = Game::tachyon_crushing_damage(damage).unwrap();
        assert_eq!(damage.id(DAMAGE_TYPE), Some(crush));
        assert_eq!(damage.entries().count(), 2);
    }

    #[test]
    fn a_gap_takes_in_a_stack_crushes_it_and_lets_out_what_it_made() {
        let mut owner = Owner::default();
        let create = Recipes::default();
        let turning = HashMap::from([([1, 0, 0], Gap { out: Dir::Down, speed: 2.56 })]);
        let mut random = horizon_core::random::LegacyRandom::new(1);
        let mut out = Vec::new();
        let at = horizon_core::geometry::Vec3::new(1.5, 0.3, 0.5);
        let mut dropped = vec![ItemEntity {
            rotation: [0.0, 0.0],
            join: Default::default(),
            no_gravity: false,
            vertical_collision: false,
            horizontal_collision: false,
            needs_sync: false,
            stuck: horizon_core::geometry::Vec3::ZERO,
            was_touching_water: false,
            health: 5,
            fire_ticks: 0,
            id: 7,
            stack: ItemStack::new("x:stone", 2),
            thrower: None,
            target: None,
            position: at,
            previous: at,
            velocity: horizon_core::geometry::Vec3::ZERO,
            on_ground: true,
            age: 0,
            pickup_delay: 0,
            bob_offset: 0.0,
            ticks: 0,
        }];
        // A living occupant takes the gap: an item waits, without being lost.
        owner.crushing_entities.insert([1, 0, 0], Living::Mob(42));
        assert!(!crush(
            &mut owner, &create, &turning, &mut dropped, &mut random, &mut out,
        ));
        assert_eq!(dropped[0].stack.count, 2);
        assert!(owner.crushers.is_empty());
        owner.crushing_entities.clear();
        // Taken in: gone from the world, kept by the gap.
        assert!(crush(&mut owner, &create, &turning, &mut dropped, &mut random, &mut out));
        assert_eq!(dropped[0].stack.count, 0);
        assert_eq!(owner.crushers[&[1, 0, 0]].left, NO_RECIPE);
        // No recipe crushes it: lost when its countdown runs out (100 - 20
        // at 2.56 a tick).
        let mut ticks = 0;
        while owner.crushers.contains_key(&[1, 0, 0]) {
            crush(&mut owner, &create, &turning, &mut [], &mut random, &mut out);
            ticks += 1;
        }
        assert_eq!(ticks, 32);
        assert_eq!(out.len(), 1);
        assert!(out[0].1.is_empty());
    }
}
