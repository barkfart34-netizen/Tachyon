//! Create's cuckoo clock (CuckooClockBlockEntity, CuckooClockRenderer,
//! CuckooClockBlock; MIT, THIRD_PARTY_NOTICES.md) on create:cuckoo_clock,
//! after Kinetics for Horizon's clock (sakurartro, MIT). Turning, it ticks
//! and its hands chase the time of day; at noon the pig comes out, at dusk
//! the creeper, and the mysterious clock's surprise blows it up.
//!
//! Unlike Create, the animation isn't saved: it is read from the time of
//! day (it starts at the first tick of its window), on the server for the
//! sounds and the blast and on each client for the doors and the figure. A
//! world whose time stands still has no animation.
use super::kinetic::Dir;
use glam::{Mat4, Vec3};
use std::cell::RefCell;
use std::collections::HashMap;

pub(super) const NONE: i32 = 0;
pub(super) const PIG: i32 = 1;
pub(super) const CREEPER: i32 = 2;
pub(super) const SURPRISE: i32 = 3;

/// `DimensionType.natural`: elsewhere time runs 24 times faster on the face.
pub(super) fn natural(dimension: &str) -> bool {
    dimension == "minecraft:overworld"
}

/// The hour and minute shown at `day_time`.
pub(super) fn time(day_time: i64, natural: bool) -> (i32, i32) {
    let day = ((day_time * if natural { 1 } else { 24 }).rem_euclid(24000)) as i32;
    ((day / 1000 + 6) % 24, (day % 1000) * 60 / 1000)
}

/// The animation running at `day_time` and how many ticks it has run (0
/// on its first tick), in the overworld. Create starts the pig at the
/// first tick of 12:00-12:04 and the creeper at the first of 18:32-18:35,
/// and ends each once it has run past 101 ticks.
pub(super) fn animation(day_time: i64, mysterious: bool) -> Option<(i32, i32)> {
    let day = day_time.rem_euclid(24000) as i32;
    // 12:00 is 6000; 18:32 the first tick whose minute is 32.
    let (start, anim) = if (6000..6000 + 102).contains(&day) {
        (6000, PIG)
    } else if (12534..12534 + 102).contains(&day) {
        (12534, CREEPER)
    } else {
        return None;
    };
    Some((if mysterious { SURPRISE } else { anim }, day - start))
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Sound {
    pub(super) event: &'static str,
    pub(super) volume: f32,
    pub(super) pitch: f32,
}

const HAT: &str = "minecraft:block.note_block.hat";
const CHIME: &str = "minecraft:block.note_block.chime";

const fn sound(event: &'static str, volume: f32, pitch: f32) -> Sound {
    Sound {
        event,
        volume,
        pitch,
    }
}

/// The ticking of a clock with nothing to announce.
pub(super) fn tick_sound(ticks: i64, natural: bool) -> Option<Sound> {
    let (long, short) = if natural { (32, 16) } else { (6, 3) };
    if ticks % long == 0 {
        Some(sound(HAT, 1.0 / 16.0, 2.0))
    } else if ticks % short == 0 {
        Some(sound(HAT, 1.0 / 16.0, 1.5))
    } else {
        None
    }
}

/// Ticks between the figure's pops.
fn step_of(anim: i32) -> i32 {
    if anim == SURPRISE { 3 } else { 15 }
}

/// The sounds of an animation `value` ticks in.
pub(super) fn animation_sounds(anim: i32, value: i32) -> Vec<Sound> {
    let mut out = Vec::new();
    if value == 1 {
        out.push(sound(CHIME, 2.0, 0.5));
    }
    if value == 21 {
        out.push(sound(CHIME, 2.0, 0.793701));
    }
    if value == 40 && anim == SURPRISE {
        out.push(sound("minecraft:entity.tnt.primed", 1.0, 1.0));
    }
    let step = step_of(anim);
    for phase in (30..=60).step_by(step as usize) {
        if value == phase - step / 3 {
            out.push(sound("minecraft:block.chest.open", 1.0 / 16.0, 2.0));
        }
        if value == phase {
            out.push(if anim == PIG {
                sound("minecraft:entity.pig.ambient", 0.25, 1.0)
            } else {
                sound("minecraft:entity.creeper.hurt", 0.25, 3.0)
            });
        }
        if value == phase + step / 3 {
            out.push(sound("minecraft:block.chest.close", 1.0 / 16.0, 2.0));
        }
    }
    out
}

/// What a turning clock does this tick.
#[derive(Debug, Default, PartialEq)]
pub(super) struct Step {
    pub(super) sounds: Vec<Sound>,
    pub(super) smoke: bool,
    pub(super) explode: bool,
}

/// One server tick of a turning clock, at `game_time` and `day_time`.
pub(super) fn step(game_time: i64, day_time: i64, natural: bool, mysterious: bool) -> Step {
    let mut out = Step::default();
    let running = natural.then(|| animation(day_time, mysterious)).flatten();
    let Some((anim, value)) = running else {
        out.sounds.extend(tick_sound(game_time, natural));
        return out;
    };
    out.sounds = animation_sounds(anim, value);
    out.smoke = anim == SURPRISE && value > 30;
    out.explode = anim == SURPRISE && value == 50;
    out
}

/// How far the doors are open (degrees) `value` ticks into an animation.
pub(super) fn door_angle(anim: i32, value: f32) -> f32 {
    let step = step_of(anim);
    let mut angle = 0.0;
    for phase in (30..=60).step_by(step as usize) {
        let local = value - phase as f32;
        if local < -((step / 3) as f32) {
            continue;
        } else if local < 0.0 {
            angle = lerp((value - (phase - 5) as f32) / 5.0, 0.0, 135.0);
        } else if local < (step / 3) as f32 {
            angle = 135.0;
        } else if local < (2 * step / 3) as f32 {
            angle = lerp((value - (phase + 5) as f32) / 5.0, 135.0, 0.0);
        }
    }
    angle
}

/// How far out the figure is along the model's x: 10/16 (inside) shut.
pub(super) fn figure_offset(angle: f32) -> f32 {
    -(angle / 135.0) / 2.0 + 10.0 / 16.0
}

fn lerp(t: f32, a: f32, b: f32) -> f32 {
    a + t.clamp(0.0, 1.0) * (b - a)
}

/// The turn (degrees, right-handed about +Y) taking Create's partials,
/// built on the west side, to a clock facing `facing`.
pub(super) fn west_to(facing: Dir) -> Option<f32> {
    Some(match facing {
        Dir::West => 0.0,
        Dir::South => 90.0,
        Dir::East => 180.0,
        Dir::North => -90.0,
        _ => return None,
    })
}

#[derive(Clone, Copy, Default)]
struct Hand {
    value: f32,
    previous: f32,
}

#[derive(Clone, Copy)]
struct Hands {
    tick: i64,
    hour: Hand,
    minute: Hand,
}

thread_local! {
    /// The hands as this client draws them, by position.
    static HANDS: RefCell<HashMap<[i32; 3], Hands>> = RefCell::new(HashMap::new());
}

fn shortest(current: f32, target: f32) -> f32 {
    (((target % 360.0 - current % 360.0) % 360.0 + 540.0) % 360.0) - 180.0
}

/// `moveHands`: a fifth of the gap a tick, the short way round.
fn chase(hand: &mut Hand, target: f32) {
    let target = hand.value + shortest(hand.value, target);
    hand.previous = hand.value;
    let diff = target - hand.value;
    hand.value = if diff.abs() < 1.0 / 4096.0 {
        target
    } else {
        hand.value + diff * 0.2
    };
}

fn at(hand: Hand, partial: f32) -> f32 {
    hand.previous + shortest(hand.previous, hand.value) * partial
}

/// The hour and minute hands' angles (degrees) of the clock at `pos` at
/// `ticks`; they move only while it turns.
fn hands(pos: [i32; 3], ticks: f64, running: bool, hours: i32, minutes: i32) -> (f32, f32) {
    let whole = ticks.floor() as i64;
    HANDS.with_borrow_mut(|all| {
        if all.len() > 4096 {
            all.clear();
        }
        let h = all.entry(pos).or_insert(Hands {
            tick: whole,
            hour: Hand::default(),
            minute: Hand::default(),
        });
        if running {
            let (hour, minute) = ((30 * (hours % 12)) as f32, (6 * minutes) as f32);
            for _ in 0..(whole - h.tick).clamp(0, 100) {
                chase(&mut h.hour, hour);
                chase(&mut h.minute, minute);
            }
        }
        h.tick = whole;
        let partial = (ticks - whole as f64) as f32;
        (at(h.hour, partial), at(h.minute, partial))
    })
}

const HOUR: &str = "create:block/cuckoo_clock/hour_hand";
const MINUTE: &str = "create:block/cuckoo_clock/minute_hand";
const LEFT_DOOR: &str = "create:block/cuckoo_clock/left_door";
const RIGHT_DOOR: &str = "create:block/cuckoo_clock/right_door";
const PIG_MODEL: &str = "create:block/cuckoo_clock/pig";
const CREEPER_MODEL: &str = "create:block/cuckoo_clock/creeper";

/// A turn about `axis` through `pivot` (block units).
fn about(axis: Vec3, degrees: f32, pivot: Vec3) -> Mat4 {
    Mat4::from_translation(pivot)
        * Mat4::from_axis_angle(axis, degrees.to_radians())
        * Mat4::from_translation(-pivot)
}

/// The moving parts of a clock facing `facing` (the body is the chunk's):
/// models and their poses inside the block.
#[allow(clippy::too_many_arguments)]
pub(super) fn parts(
    pos: [i32; 3],
    facing: Dir,
    turning: bool,
    ticks: f64,
    day_time: f64,
    natural: bool,
    mysterious: bool,
) -> Vec<(&'static str, Mat4)> {
    let Some(west) = west_to(facing) else {
        return Vec::new();
    };
    let centre = Vec3::splat(0.5);
    let face = about(Vec3::Y, west, centre);
    let (hours, minutes) = time(day_time.floor() as i64, natural);
    let (hour, minute) = hands(pos, ticks, turning, hours, minutes);
    let pivot = Vec3::new(2.0, 6.0, 8.0) / 16.0;
    let mut out = vec![
        (HOUR, face * about(Vec3::X, hour, pivot)),
        (MINUTE, face * about(Vec3::X, minute, pivot)),
    ];
    let running = (natural && turning)
        .then(|| animation(day_time.floor() as i64, mysterious))
        .flatten();
    let (anim, value) = match running {
        Some((anim, value)) => (anim, value as f32 + day_time.fract() as f32),
        None => (NONE, 0.0),
    };
    let angle = if anim == NONE {
        0.0
    } else {
        door_angle(anim, value)
    };
    for (model, z, sign) in [(LEFT_DOOR, 6.0, -1.0), (RIGHT_DOOR, 10.0, 1.0)] {
        let hinge = Vec3::new(2.0, 0.0, z) / 16.0;
        out.push((model, face * about(Vec3::Y, angle * sign, hinge)));
    }
    if anim != NONE {
        let figure = if anim == PIG {
            PIG_MODEL
        } else {
            CREEPER_MODEL
        };
        let out_of = Mat4::from_translation(Vec3::new(figure_offset(angle), 0.0, 0.0));
        out.push((figure, face * out_of));
    }
    out
}

thread_local! {
    /// The time of day each dimension's clocks last saw: a world whose time
    /// stands still plays no animation (it would repeat every tick).
    static LAST_DAY: RefCell<HashMap<String, i64>> = RefCell::new(HashMap::new());
}

impl super::super::GameLevel<'_> {
    /// `CuckooClockBlock.getStateForPlacement`: its back to a shaft beside
    /// it (none when shafts come from both horizontal axes), else the
    /// template's facing toward the player.
    pub(in crate::game) fn tachyon_clock_place(
        &self,
        state: horizon_world::registry::BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
    ) {
        use super::kinetic::Axis;
        let clock = self.native_state_block(state);
        if answer.is_some()
            || !matches!(
                clock,
                Some("create:cuckoo_clock" | "create:mysterious_cuckoo_clock")
            )
        {
            return;
        }
        let p = context.pos;
        let mut found: Option<Dir> = None;
        for d in [Dir::North, Dir::South, Dir::West, Dir::East] {
            let o = d.offset();
            let there = self.world.block(p.x + o[0], p.y + o[1], p.z + o[2]);
            let axis = if d.axis() == Axis::X { "x" } else { "z" };
            let along = self.native_state_with_property(there, "axis", axis) == Some(there);
            let kinetic = (self.native_state_block(there))
                .and_then(|n| n.strip_prefix("create:"))
                .is_some_and(|n| {
                    super::create_part(n, |k| (k == "axis").then_some(axis)).is_some()
                });
            if !(along && kinetic) {
                continue;
            }
            if found.is_some_and(|f| f.axis() != d.axis()) {
                return;
            }
            found = Some(d);
        }
        let Some(shaft) = found else { return };
        if let Some(placed) =
            self.native_state_with_property(state, "facing", shaft.opposite().name())
        {
            *answer = Some(Some(placed));
        }
    }
}

impl super::super::GameLevel<'_> {
    /// `SawBlock`'s placement: the blade away from the look, its shaft
    /// across the player's way (`saw::placement`).
    pub(in crate::game) fn tachyon_saw_place(
        &self,
        state: horizon_world::registry::BlockStateId,
        context: &horizon_world::level::PlacementContext,
        answer: &mut Option<Option<horizon_world::registry::BlockStateId>>,
    ) {
        if answer.is_some() || self.native_state_block(state) != Some(super::saw::SAW) {
            return;
        }
        let (facing, along_first, flipped) = super::saw::placement(context.yaw, context.pitch);
        let placed = self
            .native_state_with_property(state, "facing", facing.name())
            .and_then(|s| self.native_state_with_property(s, "axis_along_first", if along_first { "true" } else { "false" }))
            .and_then(|s| self.native_state_with_property(s, "flipped", if flipped { "true" } else { "false" }));
        if let Some(placed) = placed {
            *answer = Some(Some(placed));
        }
    }
}

impl super::super::Game<'_> {
    /// Each turning clock ticks, chimes and smokes; a mysterious one blows
    /// up (`CuckooClockBlockEntity.tick`). The owner's alone.
    pub(super) fn tachyon_clocks(&mut self) {
        use super::kinetic::Part;
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let clocks: Vec<([i32; 3], bool)> = super::OWNERS.with_borrow(|owners| {
            let Some(owner) = owners.get(dimension) else {
                return Vec::new();
            };
            (owner.index.parts.iter())
                .filter_map(|(p, part)| match part {
                    Part::Clock(_, mysterious)
                        if owner.shown.speeds.get(p).is_some_and(|s| s[0] != 0.0) =>
                    {
                        Some((*p, *mysterious))
                    }
                    _ => None,
                })
                .collect()
        });
        let (now, day) = (self.world.game_time as i64, self.world.day_time);
        let moving =
            LAST_DAY.with_borrow_mut(|last| last.insert(dimension.to_owned(), day) != Some(day));
        let natural = natural(dimension);
        for (pos, mysterious) in clocks {
            let s = if moving {
                step(now, day, natural, mysterious)
            } else {
                Step {
                    sounds: tick_sound(now, natural).into_iter().collect(),
                    ..Step::default()
                }
            };
            let at = pos.map(|c| f64::from(c) + 0.5);
            for x in &s.sounds {
                // SoundSource 4 is `block`.
                self.native_level_play_sound(x.event, at, 4, x.volume, x.pitch);
            }
            if s.smoke {
                use horizon_client::particles::{ParticleOptions, ParticleType};
                let smoke = ParticleOptions::simple(ParticleType::LargeSmoke);
                self.native_level_send_particles(&smoke, None, at, [0.3; 3], 0.0, 1);
            }
            if s.explode
                && let Some(air) = self.pack.assets.registry.default_state("minecraft:air")
            {
                // `destroyBlock(pos, false)`, then a blast of 3 that breaks blocks.
                self.native_set_block(dimension, pos, air, 3);
                self.native_explode(
                    dimension,
                    at,
                    3.0,
                    false,
                    super::super::native_explosions::NativeBlast::Block,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_day_starts_at_six_and_runs_faster_away_from_the_overworld() {
        assert_eq!(time(0, true), (6, 0));
        assert_eq!(time(6000, true), (12, 0));
        assert_eq!(time(6500, true), (12, 30));
        assert_eq!(time(18999, true), (0, 59));
        assert_eq!(time(125, false), (9, 0));
        assert!(natural("minecraft:overworld") && !natural("minecraft:the_nether"));
    }

    #[test]
    fn noon_brings_the_pig_and_dusk_the_creeper() {
        assert_eq!(animation(5999, false), None);
        assert_eq!(animation(6000, false), Some((PIG, 0)));
        assert_eq!(animation(24000 + 6101, false), Some((PIG, 101)));
        assert_eq!(animation(6102, false), None);
        // 12533 is still minute 31.
        assert_eq!(time(12533, true), (18, 31));
        assert_eq!(time(12534, true), (18, 32));
        assert_eq!(animation(12534, false), Some((CREEPER, 0)));
        assert_eq!(animation(6030, true), Some((SURPRISE, 30)));
    }

    #[test]
    fn a_noon_chimes_and_oinks_three_times() {
        let mut heard = Vec::new();
        for value in 0..110 {
            let s = step(1, 6000 + value, true, false);
            assert!(!s.explode && !s.smoke);
            heard.extend(s.sounds.iter().map(|x| (value, x.event)));
        }
        assert!(heard.contains(&(1, CHIME)) && heard.contains(&(21, CHIME)));
        let oinks: Vec<i64> = (heard.iter())
            .filter(|s| s.1 == "minecraft:entity.pig.ambient")
            .map(|s| s.0)
            .collect();
        assert_eq!(oinks, vec![30, 45, 60]);
    }

    #[test]
    fn the_surprise_smokes_and_blows_up_once() {
        let booms: Vec<i64> = (0..110)
            .filter(|v| step(1, 6000 + v, true, true).explode)
            .collect();
        assert_eq!(booms, vec![50]);
        assert!(step(1, 6031, true, true).smoke && !step(1, 6030, true, true).smoke);
        // Away from the overworld it only ticks.
        let s = step(6, 6000, false, true);
        assert_eq!(s.sounds, vec![sound(HAT, 1.0 / 16.0, 2.0)]);
        assert!(!s.explode);
    }

    #[test]
    fn the_doors_open_for_each_pop_and_the_figure_comes_out() {
        assert_eq!(door_angle(PIG, 0.0), 0.0);
        assert_eq!(door_angle(PIG, 30.0), 135.0);
        assert_eq!(door_angle(PIG, 40.0), 0.0);
        assert_eq!(figure_offset(0.0), 10.0 / 16.0);
        assert_eq!(figure_offset(135.0), 2.0 / 16.0);
        let shut = parts([0, 0, 0], Dir::North, false, 0.0, 0.0, true, false);
        assert_eq!(shut.len(), 4);
        let out = parts([0, 1, 0], Dir::North, true, 30.0, 6030.0, true, false);
        assert_eq!(out.len(), 5);
        assert_eq!(out[4].0, PIG_MODEL);
        assert!(parts([0, 0, 0], Dir::Up, true, 0.0, 0.0, true, false).is_empty());
    }

    #[test]
    fn the_hands_chase_the_time_while_turning() {
        let (h0, m0) = hands([9, 9, 9], 0.0, true, 3, 30);
        assert_eq!((h0, m0), (0.0, 0.0));
        let (h, m) = hands([9, 9, 9], 200.0, true, 3, 30);
        assert!((h - 90.0).abs() < 0.5, "{h}");
        assert!((m.rem_euclid(360.0) - 180.0).abs() < 1.0, "{m}");
        let still = hands([9, 9, 9], 400.0, false, 9, 0);
        assert_eq!(still, (h, m));
    }
}
