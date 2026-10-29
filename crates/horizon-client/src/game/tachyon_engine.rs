//! The Tachyon Content Engine (docs/tachyon/rencana-modpack.md, phase 1):
//! what the modpack's blocks do beyond their vanilla templates. For now
//! Kinetica's rotation ([`kinetic`]).
//!
//! The world's owner (the server thread, a LAN host, or the window when the
//! world runs in the frame) keeps an index of the kinetic parts of its
//! loaded chunks, solves their speeds when a part comes or goes, and tells
//! its players (`tachyon:kinetic` payloads). A client keeps its own index
//! of its chunks, takes the speeds, and draws the turning parts with the
//! entities: their blocks' own models are empty but for their casings.
//!
//! The state lives per thread (each `Game` of a process runs on its own:
//! the window's, the server thread's), by dimension on the owner's side.
//! The game calls in at a few places: [`Game::tachyon_tick_blocks`] in the
//! block entity phase, [`Game::tachyon_tick`] from the native tick hook,
//! [`Game::tachyon_payload`] for a payload, [`Game::tachyon_render`] with the
//! block entities, [`Game::tachyon_engine_hud`] from the HUD hook.
pub(super) mod chronicles;
pub(crate) mod kinetic;
mod millstone;

use super::Game;
use glam::{DVec3, Mat4, Vec3};
use horizon_client::{
    extras::Corner,
    extras_hud::Corners,
    hud::HudBatch,
    item_render::{ItemRenderState, LayerGeometry, LayerRender},
    mesher::Vertex,
};
use horizon_client::items::ItemStack;
use horizon_core::random::RandomSource;
use horizon_world::registry::{BlockRegistry, BlockStateId};
use kinetic::{Axis, Dir, Part};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// The payload channel for the speeds.
const KINETIC: &str = "tachyon:kinetic";
/// A creative motor's speed (RPM) until a player sets it, and a hand
/// crank's.
const MOTOR_SPEED: f32 = 32.0;
/// What shows a part's speed when worn, or held in either hand.
const GOGGLES: &str = "kinetica:engineers_goggles";
/// Ticks a hand crank turns after each use (holding use repeats it).
const CRANK_TICKS: u64 = 10;
/// Ticks between looks at the water round the wheels and the sky above
/// the sails.
const NATURE_TICKS: u64 = 20;
/// A water wheel's speed for each side water flows past squarely.
const WATER_SPEED: f32 = 8.0;
/// Ticks between full sends of the speeds, for players who just came.
const RESEND: u64 = 100;
/// How far turning parts are drawn, in blocks.
const VIEW: f64 = 64.0;

/// The kinetic part of each block state (by id), for one registry.
struct Kinds {
    registry: usize,
    parts: Vec<Option<Part>>,
    any: bool,
}

impl Kinds {
    fn new(registry: &BlockRegistry) -> Self {
        let mut parts = Vec::with_capacity(registry.len());
        for id in 0..registry.len() {
            let part = registry
                .state(BlockStateId(id as u32))
                .and_then(|s| part_of(&s.name, |k| s.properties.get(k).map(String::as_str)));
            parts.push(part);
        }
        let any = parts.iter().any(Option::is_some);
        Self {
            registry: std::ptr::from_ref(registry) as usize,
            parts,
            any,
        }
    }
    fn part(&self, state: u32) -> Option<Part> {
        self.parts.get(state as usize).copied().flatten()
    }
}

/// The part a block state is: Kinetica's blocks by name and properties.
fn part_of<'a>(name: &str, property: impl Fn(&str) -> Option<&'a str>) -> Option<Part> {
    let axis = || property("axis").and_then(Axis::parse);
    match name.strip_prefix("kinetica:")? {
        "shaft" => Some(Part::Shaft(axis()?)),
        "cogwheel" => Some(Part::Cog(axis()?)),
        "large_cogwheel" => Some(Part::LargeCog(axis()?)),
        "gearbox" => Some(Part::Gearbox(axis()?)),
        // Its template's facing points at the player who placed it; the
        // shaft points away (tachyon/art/kinetica.py).
        "creative_motor" => Some(Part::Motor(
            property("facing").and_then(Dir::parse)?.opposite(),
        )),
        // Its template's facing points out of the block it was put on,
        // which the crank turns.
        "hand_crank" => Some(Part::Crank(
            property("facing").and_then(Dir::parse)?.opposite(),
        )),
        "clutch" => Some(Part::Clutch(axis()?)),
        "water_wheel" => Some(Part::WaterWheel(axis()?)),
        "windmill" => Some(Part::Windmill(axis()?)),
        "gearshift" => Some(Part::Gearshift(axis()?)),
        "millstone" => Some(Part::Millstone),
        _ => None,
    }
}

/// The kinetic parts of a world's loaded chunks.
#[derive(Default)]
struct Index {
    parts: HashMap<[i32; 3], Part>,
    known: HashSet<(i32, i32)>,
    revision: Option<u64>,
}

impl Index {
    /// Follows the world: chunks that came or went, and block writes since
    /// the last call (taken when `take`: no one else reads them). Whether a
    /// part came or went.
    fn sync(
        &mut self,
        world: &mut horizon_client::world::World,
        kinds: &Kinds,
        take: bool,
        removed: &mut Vec<[i32; 3]>,
    ) -> bool {
        // The world records writes of the states it watches (sculk's, and these).
        if world.watched.len() < kinds.parts.len() {
            world.watched.resize(kinds.parts.len(), false);
        }
        for (watched, part) in world.watched.iter_mut().zip(&kinds.parts) {
            *watched |= part.is_some();
        }
        let mut changed = false;
        let revision = world.chunks.membership_revision();
        // Another world's chunks (their count starts again): start over.
        if self.revision.is_some_and(|r| revision < r) {
            changed |= !self.parts.is_empty();
            *self = Index::default();
        }
        if self.revision != Some(revision) {
            self.revision = Some(revision);
            let loaded: HashSet<(i32, i32)> = world.chunks.keys().copied().collect();
            let gone: HashSet<(i32, i32)> = self.known.difference(&loaded).copied().collect();
            if !gone.is_empty() {
                let before = self.parts.len();
                self.parts
                    .retain(|p, _| !gone.contains(&(p[0] >> 4, p[2] >> 4)));
                changed |= self.parts.len() != before;
            }
            let arrived: Vec<(i32, i32)> = loaded.difference(&self.known).copied().collect();
            for key in arrived {
                changed |= self.scan(world, key, kinds);
            }
            self.known = loaded;
        }
        let writes = if take {
            std::mem::take(&mut world.watched_writes)
        } else {
            world.watched_writes.clone()
        };
        for ((x, y, z), old, new) in writes {
            let (was, is) = (kinds.part(old), kinds.part(new));
            if was.is_none() && is.is_none() {
                continue;
            }
            match is {
                Some(part) => self.parts.insert([x, y, z], part),
                None => {
                    removed.push([x, y, z]);
                    self.parts.remove(&[x, y, z])
                }
            };
            changed = true;
        }
        changed
    }
    /// The parts of one chunk: sections without a kinetic state in their
    /// palette are passed over.
    fn scan(
        &mut self,
        world: &horizon_client::world::World,
        key: (i32, i32),
        kinds: &Kinds,
    ) -> bool {
        use horizon_client::chunk::MIN_Y;
        let Some(chunk) = world.chunks.get(&key) else {
            return false;
        };
        let mut found = false;
        for (s, section) in chunk.sections.iter().enumerate() {
            if !section.blocks.maybe_has(|v| kinds.part(v).is_some()) {
                continue;
            }
            let y0 = MIN_Y + s as i32 * 16;
            let mut a = [0u32; 4096];
            section.unpack(&mut a);
            for (i, v) in a.iter().enumerate() {
                if let Some(part) = kinds.part(*v) {
                    let (x, z, y) = (i & 15, (i >> 4) & 15, i >> 8);
                    let pos = [key.0 * 16 + x as i32, y0 + y as i32, key.1 * 16 + z as i32];
                    self.parts.insert(pos, part);
                    found = true;
                }
            }
        }
        found
    }
}

/// What a world's owner keeps for a dimension.
#[derive(Default)]
struct Owner {
    index: Index,
    shown: Shown,
    ticks: u64,
    /// The creative motors' speeds the players set (RPM; the default is
    /// [`MOTOR_SPEED`]), kept in the world's folder.
    motors: HashMap<[i32; 3], f32>,
    /// `motors` was read from the world's folder.
    loaded: bool,
    /// The speeds need solving again (a motor's speed changed).
    dirty: bool,
    /// The hand cranks turning: until which tick, and their speed.
    cranks: HashMap<[i32; 3], (u64, f32)>,
    /// The clutches and gearshifts powered by redstone.
    powered: HashSet<[i32; 3]>,
    /// The water wheels and windmills turning, at their own speeds.
    natural: HashMap<[i32; 3], f32>,
    /// What the millstones hold, kept with the motors' speeds.
    mills: HashMap<[i32; 3], millstone::Mill>,
    /// `mills` changed since the file was written.
    mills_changed: bool,
}

impl Owner {
    /// The motors' and millstones' file in the world's folder, for this
    /// dimension.
    fn file(root: &std::path::Path, dimension: &str) -> std::path::PathBuf {
        let name = dimension.replace([':', '/'], "_");
        root.join("tachyon").join(format!("kinetica-{name}.json"))
    }
    fn load(&mut self, root: &std::path::Path, dimension: &str) {
        if std::mem::replace(&mut self.loaded, true) {
            return;
        }
        let Ok(text) = std::fs::read_to_string(Self::file(root, dimension)) else {
            return;
        };
        let value: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let rows: Vec<[f64; 4]> = serde_json::from_value(value["motors"].clone()).unwrap_or_default();
        self.motors = rows
            .into_iter()
            .map(|[x, y, z, s]| ([x as i32, y as i32, z as i32], s as f32))
            .collect();
        let mills: Vec<([i32; 3], millstone::Mill)> =
            serde_json::from_value(value["mills"].clone()).unwrap_or_default();
        self.mills = mills.into_iter().collect();
        self.dirty = true;
    }
    fn save(&self, root: &std::path::Path, dimension: &str) {
        let mut rows: Vec<[f64; 4]> = self
            .motors
            .iter()
            .map(|(p, s)| {
                [
                    f64::from(p[0]),
                    f64::from(p[1]),
                    f64::from(p[2]),
                    f64::from(*s),
                ]
            })
            .collect();
        rows.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let mut mills: Vec<(&[i32; 3], &millstone::Mill)> =
            self.mills.iter().filter(|(_, m)| !m.is_empty()).collect();
        mills.sort_by_key(|(p, _)| **p);
        let file = Self::file(root, dimension);
        let text = serde_json::json!({ "motors": rows, "mills": mills }).to_string();
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&file, text));
        if let Err(e) = written {
            log::warn!("Tachyon: {} not written: {e}", file.display());
        }
    }
}

/// A creative motor's next speed when used: twice as fast, from 256 back
/// to 8; sneaking, the other way round.
fn next_motor_speed(speed: f32, sneaking: bool) -> f32 {
    if sneaking {
        return -speed;
    }
    let faster = speed.abs() * 2.0;
    let faster = if faster > kinetic::MAX_SPEED {
        8.0
    } else {
        faster
    };
    faster.copysign(speed)
}

/// What a client keeps.
#[derive(Default)]
struct Client {
    index: Index,
    shown: Shown,
    dimension: &'static str,
    /// Baked models, for the item renderer at `renderer` (a resource
    /// reload makes another).
    models: HashMap<&'static str, Arc<ItemRenderState>>,
    renderer: usize,
}

thread_local! {
    static KINDS: RefCell<Option<Arc<Kinds>>> = const { RefCell::new(None) };
    static OWNERS: RefCell<HashMap<&'static str, Owner>> = RefCell::new(HashMap::new());
    static CLIENT: RefCell<Client> = RefCell::new(Client::default());
}

/// The millstone's recipes, for the recipe viewer: each input, its work,
/// and its outputs (item, count, chance).
pub(in crate::game) fn milling() -> Vec<(String, f32, Vec<(String, u32, f32)>)> {
    millstone::recipes()
        .iter()
        .map(|r| {
            let outputs = (r.outputs.iter())
                .map(|o| (o.item.clone(), o.count, o.chance))
                .collect();
            (r.input.clone(), r.time, outputs)
        })
        .collect()
}

/// What a game shows of the parts: their speeds, the jammed ones, their
/// networks' stress, and the millstones at work (on what).
#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    speeds: kinetic::Speeds,
    jammed: HashSet<[i32; 3]>,
    stress: HashMap<[i32; 3], kinetic::Stress>,
    work: HashMap<[i32; 3], String>,
}

/// What the parts show as a payload: the turning parts (each half's
/// speed), the jammed ones, the stress of each part's network, the
/// millstones at work.
fn encode(shown: &Shown) -> Vec<u8> {
    let Shown {
        speeds,
        jammed,
        stress,
        work,
    } = shown;
    let mut out = Vec::with_capacity(17 + speeds.len() * 20 + jammed.len() * 12 + stress.len() * 20);
    let pos = |out: &mut Vec<u8>, p: &[i32; 3]| {
        for v in p {
            out.extend_from_slice(&v.to_le_bytes());
        }
    };
    out.push(VERSION);
    out.extend_from_slice(&(speeds.len() as u32).to_le_bytes());
    for (p, s) in speeds {
        pos(&mut out, p);
        for v in s {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out.extend_from_slice(&(jammed.len() as u32).to_le_bytes());
    for p in jammed {
        pos(&mut out, p);
    }
    out.extend_from_slice(&(stress.len() as u32).to_le_bytes());
    for (p, s) in stress {
        pos(&mut out, p);
        out.extend_from_slice(&s.load.to_le_bytes());
        out.extend_from_slice(&s.capacity.to_le_bytes());
    }
    out.extend_from_slice(&(work.len() as u32).to_le_bytes());
    for (p, item) in work {
        pos(&mut out, p);
        let name = &item.as_bytes()[..item.len().min(255)];
        out.push(name.len() as u8);
        out.extend_from_slice(name);
    }
    out
}

/// The payload's form.
const VERSION: u8 = 3;

fn decode(data: &[u8]) -> Option<Shown> {
    let mut at = 0usize;
    let mut next = |n: usize| {
        let s = data.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let word = |b: &[u8]| -> [u8; 4] { [b[0], b[1], b[2], b[3]] };
    let pos = |b: &[u8]| {
        [
            i32::from_le_bytes(word(&b[0..4])),
            i32::from_le_bytes(word(&b[4..8])),
            i32::from_le_bytes(word(&b[8..12])),
        ]
    };
    if next(1)? != [VERSION] {
        return None;
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut speeds = HashMap::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        let b = next(20)?;
        let p = [
            i32::from_le_bytes(word(&b[0..4])),
            i32::from_le_bytes(word(&b[4..8])),
            i32::from_le_bytes(word(&b[8..12])),
        ];
        let halves = [
            f32::from_le_bytes(word(&b[12..16])),
            f32::from_le_bytes(word(&b[16..20])),
        ];
        speeds.insert(p, halves);
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut jammed = HashSet::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        jammed.insert(pos(next(12)?));
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut stress = HashMap::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        let b = next(20)?;
        let s = kinetic::Stress {
            load: f32::from_le_bytes(word(&b[12..16])),
            capacity: f32::from_le_bytes(word(&b[16..20])),
        };
        stress.insert(pos(b), s);
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut work = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let p = pos(next(12)?);
        let len = usize::from(next(1)?[0]);
        let item = std::str::from_utf8(next(len)?).ok()?;
        work.insert(p, item.to_owned());
    }
    Some(Shown {
        speeds,
        jammed,
        stress,
        work,
    })
}

/// A turning piece of a part: its model, where its shaft points (+Y in
/// the model), its speed for each turn of its half's, and the half.
fn pieces(part: Part) -> Vec<(&'static str, Dir, f32, usize)> {
    const HALF: &str = "kinetica:block/shaft_half";
    match part {
        Part::Shaft(a) => vec![("kinetica:block/shaft", positive(a), 1.0, 0)],
        Part::Cog(a) => vec![("kinetica:block/cogwheel", positive(a), 1.0, 0)],
        Part::LargeCog(a) => vec![("kinetica:block/large_cogwheel", positive(a), 1.0, 0)],
        Part::WaterWheel(a) => vec![("kinetica:block/water_wheel", positive(a), 1.0, 0)],
        Part::Windmill(a) => vec![("kinetica:block/windmill", positive(a), 1.0, 0)],
        Part::Gearbox(_) => Dir::ALL
            .into_iter()
            .filter(|d| part.shaft_toward(*d))
            .map(|d| (HALF, d, part.side_factor(d), 0))
            .collect(),
        Part::Motor(out) => vec![(HALF, out, 1.0, 0)],
        // The handle stands out of the side the crank was put on.
        Part::Crank(out) => vec![("kinetica:block/hand_crank_handle", out.opposite(), 1.0, 0)],
        Part::Clutch(a) | Part::Gearshift(a) => {
            let [neg, pos] = a.dirs();
            vec![(HALF, neg, 1.0, 0), (HALF, pos, 1.0, 1)]
        }
        Part::Millstone => vec![("kinetica:block/millstone_top", Dir::Up, 1.0, 0)],
    }
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn positive(axis: Axis) -> Dir {
    match axis {
        Axis::X => Dir::East,
        Axis::Y => Dir::Up,
        Axis::Z => Dir::South,
    }
}

/// Turns the model's +Y to `d`.
fn point(d: Dir) -> Mat4 {
    use std::f32::consts::{FRAC_PI_2, PI};
    match d {
        Dir::Up => Mat4::IDENTITY,
        Dir::Down => Mat4::from_rotation_x(PI),
        Dir::North => Mat4::from_rotation_x(-FRAC_PI_2),
        Dir::South => Mat4::from_rotation_x(FRAC_PI_2),
        Dir::East => Mat4::from_rotation_z(-FRAC_PI_2),
        Dir::West => Mat4::from_rotation_z(FRAC_PI_2),
    }
}

/// A turn of `degrees` about `axis`'s positive direction.
fn spin(axis: Axis, degrees: f32) -> Mat4 {
    let r = degrees.to_radians();
    match axis {
        Axis::X => Mat4::from_rotation_x(r),
        Axis::Y => Mat4::from_rotation_y(r),
        Axis::Z => Mat4::from_rotation_z(r),
    }
}

impl Game<'_> {
    /// The kinetic kinds of this game's registry, made once per registry;
    /// None when it has no kinetic block (Kinetica isn't there).
    fn tachyon_kinds(&self) -> Option<Arc<Kinds>> {
        let registry = self.pack.assets.registry;
        let at = std::ptr::from_ref(registry) as usize;
        KINDS.with_borrow_mut(|kinds| {
            if kinds.as_ref().is_none_or(|k| k.registry != at) {
                *kinds = Some(Arc::new(Kinds::new(registry)));
            }
            kinds.clone().filter(|k| k.any)
        })
    }

    /// The owner's turn, in the block entity phase: the index follows the
    /// world, the speeds follow the index, the redstone and the cranks, and
    /// the players hear of them.
    pub(super) fn tachyon_tick_blocks(&mut self) {
        let Some(kinds) = self.tachyon_kinds() else {
            return;
        };
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let (split, nature, mut changed, drops) = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            owner.ticks += 1;
            // Sculk reads the writes after this: they stay.
            let mut removed = Vec::new();
            let changed = owner
                .index
                .sync(&mut self.world, &kinds, false, &mut removed);
            // A motor broken forgets its speed.
            let before = owner.motors.len();
            owner.motors.retain(|p, _| !removed.contains(p));
            if owner.motors.len() != before {
                owner.save(&root, dimension);
            }
            // A millstone broken drops what it held.
            let mut drops: Vec<([i32; 3], millstone::Held)> = Vec::new();
            for p in &removed {
                if let Some(mut mill) = owner.mills.remove(p) {
                    drops.extend(mill.take_all().into_iter().map(|h| (*p, h)));
                    owner.save(&root, dimension);
                }
            }
            let split: Vec<[i32; 3]> = owner
                .index
                .parts
                .iter()
                .filter(|(_, part)| part.split())
                .map(|(p, _)| *p)
                .collect();
            let nature: Option<Vec<([i32; 3], Part)>> = (owner.ticks % NATURE_TICKS == 1)
                .then(|| {
                    owner
                        .index
                        .parts
                        .iter()
                        .filter(|(_, part)| matches!(part, Part::WaterWheel(_) | Part::Windmill(_)))
                        .map(|(p, part)| (*p, *part))
                        .collect()
                });
            (split, nature, changed, drops)
        });
        for (p, h) in drops {
            self.pop_resource(p, ItemStack::new(&h.item, h.count));
        }
        let natural = nature.map(|parts| self.tachyon_natural(&parts));
        // Redstone at the clutches and gearshifts.
        let powered: HashSet<[i32; 3]> = if split.is_empty() {
            HashSet::new()
        } else {
            self.with_level(|b, level, ticks| {
                split
                    .into_iter()
                    .filter(|p| {
                        let at = horizon_core::position::BlockPos::new(p[0], p[1], p[2]);
                        b.has_neighbor_signal(level, ticks, at)
                    })
                    .collect()
            })
        };
        let random = &mut self.world.random;
        let send = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            if powered != owner.powered {
                owner.powered = powered;
                changed = true;
            }
            if let Some(natural) = natural
                && natural != owner.natural
            {
                owner.natural = natural;
                changed = true;
            }
            // Cranks no one turns any more stop.
            let now = owner.ticks;
            let before = owner.cranks.len();
            owner.cranks.retain(|p, (until, _)| {
                *until > now && matches!(owner.index.parts.get(p), Some(Part::Crank(_)))
            });
            changed |= owner.cranks.len() != before;
            changed |= std::mem::take(&mut owner.dirty);
            let mut send = owner.ticks % RESEND == 1;
            if changed {
                let sources: HashMap<[i32; 3], f32> = owner
                    .index
                    .parts
                    .iter()
                    .filter_map(|(p, part)| {
                        // The set speed is the source's as its front shows
                        // it; its own is along the axis.
                        let (out, set) = match part {
                            Part::Motor(out) => {
                                (out, owner.motors.get(p).copied().unwrap_or(MOTOR_SPEED))
                            }
                            Part::Crank(out) => (out, owner.cranks.get(p)?.1),
                            // Their speeds are their own already.
                            Part::WaterWheel(_) | Part::Windmill(_) => {
                                return owner.natural.get(p).map(|s| (*p, *s));
                            }
                            _ => return None,
                        };
                        Some((*p, if out.positive() { set } else { -set }))
                    })
                    .collect();
                let solved = kinetic::solve(&owner.index.parts, &sources, &owner.powered);
                let shown = &mut owner.shown;
                if solved.speeds != shown.speeds
                    || solved.jammed != shown.jammed
                    || solved.stress != shown.stress
                {
                    shown.speeds = solved.speeds;
                    shown.jammed = solved.jammed;
                    shown.stress = solved.stress;
                    send = true;
                }
            }
            // The millstones turning grind.
            let mut work = HashMap::new();
            for (p, mill) in &mut owner.mills {
                let Some(item) = mill.input.as_ref().map(|h| h.item.clone()) else {
                    continue;
                };
                let speed = owner.shown.speeds.get(p).map_or(0.0, |s| s[0]);
                if speed == 0.0 || owner.index.parts.get(p) != Some(&Part::Millstone) {
                    continue;
                }
                if mill.grind(speed, millstone::recipe(&item), &mut || random.next_float()) {
                    owner.mills_changed = true;
                    work.insert(*p, item);
                }
            }
            if work != owner.shown.work {
                owner.shown.work = work;
                send = true;
            }
            if owner.mills_changed && owner.ticks % RESEND == 0 {
                owner.mills_changed = false;
                owner.save(&root, dimension);
            }
            send.then(|| encode(&owner.shown))
        });
        if let Some(data) = send
            && let Some(host) = self.lan.as_ref()
        {
            use horizon_protocol::packets::{configuration::CustomPayload, play::Clientbound};
            host.hub
                .broadcast_level(&Clientbound::CustomPayload(CustomPayload {
                    channel: KINETIC.into(),
                    data,
                }));
        }
    }

    /// The fastest part turning within `radius` blocks of `pos` in the
    /// owner's `dimension` (RPM, either way).
    pub(super) fn tachyon_fastest_near(dimension: &str, pos: [f64; 3], radius: f64) -> f32 {
        OWNERS.with_borrow(|owners| {
            let Some(owner) = owners.get(dimension) else {
                return 0.0;
            };
            owner
                .shown
                .speeds
                .iter()
                .filter(|(p, _)| {
                    let d = [0, 1, 2].map(|i| f64::from(p[i]) + 0.5 - pos[i]);
                    d[0] * d[0] + d[1] * d[1] + d[2] * d[2] <= radius * radius
                })
                .map(|(_, [a, b])| a.abs().max(b.abs()))
                .fold(0.0, f32::max)
        })
    }

    /// Whether the player wears engineer's goggles, or holds them.
    fn tachyon_goggles(&self) -> bool {
        [self.armor[0].as_ref(), self.held(), self.offhand.as_ref()]
            .into_iter()
            .flatten()
            .any(|s| &*s.item == GOGGLES)
    }

    /// The speeds of the water wheels and windmills among `parts`, each
    /// about its axis: a wheel from the water flowing past its sides (its
    /// turn about the axis), sails from the open sky above them, faster
    /// higher up.
    fn tachyon_natural(&self, parts: &[([i32; 3], Part)]) -> HashMap<[i32; 3], f32> {
        let mut out = HashMap::new();
        for &(p, part) in parts {
            let speed = match part {
                Part::WaterWheel(axis) => {
                    let along = Vec3::from(positive(axis).offset().map(|c| c as f32));
                    let turn: f32 = Dir::ALL
                        .into_iter()
                        .filter(|d| d.axis() != axis)
                        .map(|d| {
                            let r = Vec3::from(d.offset().map(|c| c as f32));
                            let w = add(p, d.offset());
                            r.cross(self.tachyon_water_flow(w)).dot(along)
                        })
                        .sum();
                    (turn * WATER_SPEED).clamp(-32.0, 32.0)
                }
                Part::Windmill(_) => {
                    let above = self.world.light(p[0], p[1] + 1, p[2]) >> 4;
                    if above >= 15 && p[1] >= 60 {
                        (8.0 + ((p[1] - 64).max(0) / 16 * 4) as f32).min(24.0)
                    } else {
                        0.0
                    }
                }
                _ => 0.0,
            };
            if speed.abs() >= 0.5 {
                out.insert(p, speed.round());
            }
        }
        out
    }

    /// Which way the water at `w` flows, roughly: down where it falls, else
    /// from its level toward lower neighbours; nothing for still water and
    /// for no water.
    fn tachyon_water_flow(&self, w: [i32; 3]) -> Vec3 {
        use horizon_client::blocks::Fluid;
        let facts = self.res.facts();
        let here = facts.get(self.world.block(w[0], w[1], w[2]));
        if here.fluid != Fluid::Water {
            return Vec3::ZERO;
        }
        if here.fluid_falling {
            return Vec3::NEG_Y;
        }
        let level = f32::from(here.fluid_level);
        let mut v = Vec3::ZERO;
        for d in [Dir::North, Dir::South, Dir::West, Dir::East] {
            let n = add(w, d.offset());
            let there = facts.get(self.world.block(n[0], n[1], n[2]));
            let lower = if there.fluid == Fluid::Water {
                if there.fluid_falling { 0.0 } else { f32::from(there.fluid_level) }
            } else if there.replaceable {
                0.0
            } else {
                continue;
            };
            v += Vec3::from(d.offset().map(|c| c as f32)) * (level - lower);
        }
        v.normalize_or_zero()
    }

    /// The player used the block at `pos` (the world's owner, before the
    /// block's own use): a creative motor's speed changes, a hand crank
    /// turns a moment (sneaking, the other way), a millstone takes the held
    /// stack or gives what it ground. Whether it was Tachyon's to use.
    pub(super) fn tachyon_use(&mut self, pos: [i32; 3]) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(kinds) = self.tachyon_kinds() else {
            return false;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]).0;
        let part = kinds.part(state);
        if !matches!(part, Some(Part::Motor(_) | Part::Crank(_) | Part::Millstone)) {
            return false;
        }
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        if part == Some(Part::Millstone) {
            let mut mill = OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                owner.load(&root, dimension);
                owner.mills.remove(&pos).unwrap_or_default()
            });
            let moved = self.tachyon_use_millstone(&mut mill);
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                if !mill.is_empty() {
                    owner.mills.insert(pos, mill);
                }
                if moved {
                    owner.save(&root, dimension);
                }
            });
            return true;
        }
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            if matches!(part, Some(Part::Crank(_))) {
                let speed = if sneaking { -MOTOR_SPEED } else { MOTOR_SPEED };
                let until = owner.ticks + CRANK_TICKS;
                let was = owner.cranks.insert(pos, (until, speed));
                owner.dirty |= was.is_none_or(|(_, s)| s != speed);
                return;
            }
            let speed = owner.motors.get(&pos).copied().unwrap_or(MOTOR_SPEED);
            owner.motors.insert(pos, next_motor_speed(speed, sneaking));
            owner.dirty = true;
            owner.save(&root, dimension);
        });
        true
    }

    /// Whether the block at `pos` is one of Tachyon's a use goes to: a
    /// player's window sends the use and places nothing (the owner does the
    /// rest, `tachyon_use`).
    pub(super) fn tachyon_takes_use(&self, pos: [i32; 3]) -> bool {
        let Some(kinds) = self.tachyon_kinds() else {
            return false;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]).0;
        matches!(
            kinds.part(state),
            Some(Part::Motor(_) | Part::Crank(_) | Part::Millstone)
        )
    }

    /// Each tick of every game: a client's index follows its world.
    pub(super) fn tachyon_tick(&mut self) {
        self.tachyon_quests_tick();
        if self.remote.is_none() {
            return;
        }
        let Some(kinds) = self.tachyon_kinds() else {
            return;
        };
        let dimension = self.dim.id();
        CLIENT.with_borrow_mut(|client| {
            if client.dimension != dimension {
                client.index = Index::default();
                client.shown = Shown::default();
                client.dimension = dimension;
            }
            client
                .index
                .sync(&mut self.world, &kinds, true, &mut Vec::new());
        });
    }

    /// A `tachyon:*` payload from the server.
    pub(super) fn tachyon_payload(&mut self, channel: &str, data: &[u8]) {
        if channel == chronicles::PROGRESS {
            self.tachyon_quests_payload(data);
            return;
        }
        if channel != KINETIC {
            return;
        }
        match decode(data) {
            Some(shown) => CLIENT.with_borrow_mut(|client| client.shown = shown),
            None => log::warn!("Tachyon: a {KINETIC} payload that doesn't read"),
        }
    }

    /// The parts this game shows and their speeds: a client's, or the
    /// owner's when the world runs in the window's frame.
    fn tachyon_view<R>(&self, f: impl FnOnce(&Index, &Shown) -> R) -> R {
        if self.remote.is_some() {
            CLIENT.with_borrow(|c| f(&c.index, &c.shown))
        } else {
            let dimension = self.dim.id();
            OWNERS.with_borrow(|owners| match owners.get(dimension) {
                Some(o) => f(&o.index, &o.shown),
                None => f(&Index::default(), &Shown::default()),
            })
        }
    }

    /// The turning parts near the camera, with the block entities.
    pub(super) fn tachyon_render(&self, camera: DVec3, alpha: f64, out: &mut Vec<Vertex>) {
        if self.tachyon_kinds().is_none() {
            return;
        }
        let ticks = self.world.game_time as f64 + alpha;
        let view = self.view_frustum;
        let mut draws: Vec<([i32; 3], Part, [f32; 2])> = Vec::new();
        self.tachyon_view(|index, shown| {
            for (pos, part) in &index.parts {
                let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
                if (at + DVec3::splat(0.5)).distance_squared(camera) > VIEW * VIEW {
                    continue;
                }
                let o = (at - camera).as_vec3();
                // Large cogwheels reach a block past their own.
                if view.is_some_and(|v| !v.intersects(o - Vec3::splat(1.5), o + Vec3::splat(2.5))) {
                    continue;
                }
                draws.push((*pos, *part, shown.speeds.get(pos).copied().unwrap_or([0.0; 2])));
            }
        });
        for (pos, part, speed) in draws {
            let packed = self.world.light(pos[0], pos[1], pos[2]);
            let light = horizon_client::block_entity_render::vertex_light(packed);
            let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
            let origin = (at - camera).as_vec3();
            for (model, toward, factor, half) in pieces(part) {
                let speed = speed[half] * factor;
                let axis = toward.axis();
                let degrees = kinetic::angle(part, pos, speed, ticks);
                let pose = Mat4::from_translation(origin + Vec3::splat(0.5))
                    * spin(axis, degrees)
                    * point(toward)
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let state = self.tachyon_model(model);
                self.submit_item_world(&state, pose, light, out);
            }
        }
    }

    /// A model to draw as an item draws one, baked once.
    fn tachyon_model(&self, model: &'static str) -> Arc<ItemRenderState> {
        let renderer = Arc::as_ptr(&self.pack.item_renderer) as usize;
        CLIENT.with_borrow_mut(|client| {
            if client.renderer != renderer {
                client.models.clear();
                client.renderer = renderer;
            }
            client
                .models
                .entry(model)
                .or_insert_with(|| {
                    let baked = self.pack.item_renderer.bake(model);
                    Arc::new(ItemRenderState {
                        display: horizon_assets::item_model::DisplayContext::None,
                        layers: vec![LayerRender {
                            geometry: LayerGeometry::Quads(baked),
                            tints: Vec::new(),
                            foil: horizon_assets::item_model::FoilType::None,
                            matrix: Mat4::IDENTITY,
                            particle: String::new(),
                            uses_block_light: true,
                            extents: Arc::new(Vec::new()),
                        }],
                        oversized_in_gui: false,
                        animated: false,
                    })
                })
                .clone()
        })
    }

    /// What the part the player looks at does, in the top-left corner (as
    /// Create's goggles tell).
    pub(super) fn tachyon_engine_hud(&mut self, batch: &mut HudBatch, corners: &mut Corners) {
        if !self.native_corner_shown(Corner::TopLeft)
            || self.tachyon_kinds().is_none()
            || !self.tachyon_goggles()
        {
            return;
        }
        let Some(hit) = self.pick() else {
            return;
        };
        let seen = self.tachyon_view(|index, shown| {
            let part = *index.parts.get(&hit.pos)?;
            let [a, b] = shown.speeds.get(&hit.pos).copied().unwrap_or([0.0; 2]);
            let speed = if a.abs() >= b.abs() { a } else { b };
            Some((
                part,
                speed,
                shown.jammed.contains(&hit.pos),
                shown.stress.get(&hit.pos).copied(),
                shown.work.get(&hit.pos).cloned(),
            ))
        });
        let Some((part, speed, jammed, stress, work)) = seen else {
            return;
        };
        let (block, english) = match part {
            Part::Shaft(_) => ("shaft", "Shaft"),
            Part::Cog(_) => ("cogwheel", "Cogwheel"),
            Part::LargeCog(_) => ("large_cogwheel", "Large Cogwheel"),
            Part::Gearbox(_) => ("gearbox", "Gearbox"),
            Part::Motor(_) => ("creative_motor", "Creative Motor"),
            Part::Crank(_) => ("hand_crank", "Hand Crank"),
            Part::Clutch(_) => ("clutch", "Clutch"),
            Part::Gearshift(_) => ("gearshift", "Gearshift"),
            Part::WaterWheel(_) => ("water_wheel", "Water Wheel"),
            Part::Windmill(_) => ("windmill", "Windmill"),
            Part::Millstone => ("millstone", "Millstone"),
        };
        let what = self.translate(&format!("block.kinetica.{block}"), english);
        let mut state = if jammed {
            self.translate("kinetica.hud.jammed", "jammed")
        } else if stress.is_some_and(kinetic::Stress::over) {
            self.translate("kinetica.hud.overstressed", "overstressed")
        } else if speed == 0.0 {
            self.translate("kinetica.hud.still", "still")
        } else {
            self.translate("kinetica.hud.rpm", "%s RPM")
                .replace("%s", &speed.abs().round().to_string())
        };
        // The network's load against its capacity, and what a millstone
        // grinds.
        if let Some(s) = stress {
            let su = self
                .translate("kinetica.hud.stress", "%s / %s SU")
                .replacen("%s", &(s.load.round() as i64).to_string(), 1)
                .replacen("%s", &(s.capacity.round() as i64).to_string(), 1);
            state = format!("{state} | {su}");
        }
        if let Some(item) = work {
            state = format!("{state} | {}", self.tachyon_item_name(&item));
        }
        let line = format!("{what}: {state}");
        let w = self.native_text_width(&line) + 4.0;
        let (x, y) = corners.place(Corner::TopLeft, w, 12.0);
        self.native_fill(batch, [x, y, w, 12.0], 0x7800_0000);
        self.native_text(batch, &line, [x + 2.0, y + 2.0], 0xFFD27A, true);
    }

    /// An item's name in the player's language: its item key's, else its
    /// block key's.
    fn tachyon_item_name(&self, item: &str) -> String {
        let key = item.replacen(':', ".", 1);
        let name = self.translate(&format!("item.{key}"), "");
        if !name.is_empty() {
            return name;
        }
        self.translate(&format!("block.{key}"), item)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_go_and_come_back() {
        let shown = Shown {
            speeds: HashMap::from([([1, -64, 3], [32.0; 2]), ([-5, 70, 9], [-16.0, 16.0])]),
            jammed: HashSet::from([[0, 0, 0]]),
            stress: HashMap::from([(
                [1, -64, 3],
                kinetic::Stress {
                    load: 128.0,
                    capacity: 256.0,
                },
            )]),
            work: HashMap::from([([2, 64, 2], "minecraft:cobblestone".to_owned())]),
        };
        assert_eq!(decode(&encode(&shown)), Some(shown.clone()));
        assert!(decode(&[1]).is_none());
        assert!(decode(&encode(&shown)[..10]).is_none());
    }

    #[test]
    fn a_motor_doubles_up_to_256_then_starts_again() {
        assert_eq!(next_motor_speed(32.0, false), 64.0);
        assert_eq!(next_motor_speed(256.0, false), 8.0);
        assert_eq!(next_motor_speed(-128.0, false), -256.0);
        assert_eq!(next_motor_speed(64.0, true), -64.0);
    }

    #[test]
    fn kinetica_blocks_are_parts() {
        let props = |axis: &'static str| move |k: &str| (k == "axis").then_some(axis);
        assert_eq!(
            part_of("kinetica:shaft", props("x")),
            Some(Part::Shaft(Axis::X))
        );
        assert_eq!(part_of("minecraft:stone", props("x")), None);
        let facing = |k: &str| (k == "facing").then_some("north");
        assert_eq!(
            part_of("kinetica:creative_motor", facing),
            Some(Part::Motor(Dir::South))
        );
        assert_eq!(
            part_of("kinetica:hand_crank", facing),
            Some(Part::Crank(Dir::South))
        );
        assert_eq!(
            part_of("kinetica:gearshift", props("y")),
            Some(Part::Gearshift(Axis::Y))
        );
        assert_eq!(
            part_of("kinetica:water_wheel", props("x")),
            Some(Part::WaterWheel(Axis::X))
        );
        assert_eq!(
            part_of("kinetica:windmill", props("z")),
            Some(Part::Windmill(Axis::Z))
        );
        assert_eq!(part_of("kinetica:millstone", facing), Some(Part::Millstone));
    }

    #[test]
    fn a_gearbox_shows_four_turning_shafts_a_shaft_one() {
        assert_eq!(pieces(Part::Gearbox(Axis::Y)).len(), 4);
        assert_eq!(pieces(Part::Shaft(Axis::Z)).len(), 1);
        assert_eq!(pieces(Part::Motor(Dir::East)).len(), 1);
        let halves: Vec<usize> = pieces(Part::Clutch(Axis::X)).iter().map(|p| p.3).collect();
        assert_eq!(halves, [0, 1]);
    }

    #[test]
    fn pointing_turns_up_to_the_side() {
        for d in Dir::ALL {
            let v = point(d).transform_vector3(Vec3::Y);
            let [x, y, z] = d.offset().map(|c| c as f32);
            assert!((v - Vec3::new(x, y, z)).length() < 1e-5, "{d:?}: {v}");
        }
    }
}
