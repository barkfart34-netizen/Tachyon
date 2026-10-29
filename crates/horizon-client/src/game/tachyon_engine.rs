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
pub(crate) mod kinetic;

use super::Game;
use glam::{DVec3, Mat4, Vec3};
use horizon_client::{
    extras::Corner,
    extras_hud::Corners,
    hud::HudBatch,
    item_render::{ItemRenderState, LayerGeometry, LayerRender},
    mesher::Vertex,
};
use horizon_world::registry::{BlockRegistry, BlockStateId};
use kinetic::{Axis, Dir, Part};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
};

/// The payload channel for the speeds.
const KINETIC: &str = "tachyon:kinetic";
/// A creative motor's speed (RPM), until it can be set.
const MOTOR_SPEED: f32 = 32.0;
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
    speeds: HashMap<[i32; 3], f32>,
    jammed: HashSet<[i32; 3]>,
    ticks: u64,
    /// The creative motors' speeds the players set (RPM; the default is
    /// [`MOTOR_SPEED`]), kept in the world's folder.
    motors: HashMap<[i32; 3], f32>,
    /// `motors` was read from the world's folder.
    loaded: bool,
    /// The speeds need solving again (a motor's speed changed).
    dirty: bool,
}

impl Owner {
    /// The motors' file in the world's folder, for this dimension.
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
        let rows: Vec<[f64; 4]> = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| serde_json::from_value(v["motors"].clone()).ok())
            .unwrap_or_default();
        self.motors = rows
            .into_iter()
            .map(|[x, y, z, s]| ([x as i32, y as i32, z as i32], s as f32))
            .collect();
        self.dirty = true;
    }
    fn save(&self, root: &std::path::Path, dimension: &str) {
        let mut rows: Vec<[f64; 4]> = self
            .motors
            .iter()
            .map(|(p, s)| [f64::from(p[0]), f64::from(p[1]), f64::from(p[2]), f64::from(*s)])
            .collect();
        rows.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let file = Self::file(root, dimension);
        let text = serde_json::json!({ "motors": rows }).to_string();
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
    let faster = if faster > kinetic::MAX_SPEED { 8.0 } else { faster };
    faster.copysign(speed)
}

/// What a client keeps.
#[derive(Default)]
struct Client {
    index: Index,
    speeds: HashMap<[i32; 3], f32>,
    jammed: HashSet<[i32; 3]>,
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

/// The speeds as a payload: the turning parts, then the jammed ones.
fn encode(speeds: &HashMap<[i32; 3], f32>, jammed: &HashSet<[i32; 3]>) -> Vec<u8> {
    let mut out = Vec::with_capacity(9 + speeds.len() * 16 + jammed.len() * 12);
    out.push(1);
    out.extend_from_slice(&(speeds.len() as u32).to_le_bytes());
    for (p, s) in speeds {
        for v in p {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&s.to_le_bytes());
    }
    out.extend_from_slice(&(jammed.len() as u32).to_le_bytes());
    for p in jammed {
        for v in p {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out
}

type Speeds = (HashMap<[i32; 3], f32>, HashSet<[i32; 3]>);

fn decode(data: &[u8]) -> Option<Speeds> {
    let mut at = 0usize;
    let mut next = |n: usize| {
        let s = data.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let word = |b: &[u8]| -> [u8; 4] { [b[0], b[1], b[2], b[3]] };
    if next(1)? != [1] {
        return None;
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut speeds = HashMap::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        let b = next(16)?;
        let p = [
            i32::from_le_bytes(word(&b[0..4])),
            i32::from_le_bytes(word(&b[4..8])),
            i32::from_le_bytes(word(&b[8..12])),
        ];
        speeds.insert(p, f32::from_le_bytes(word(&b[12..16])));
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut jammed = HashSet::with_capacity(count.min(1 << 16));
    for _ in 0..count {
        let b = next(12)?;
        jammed.insert([
            i32::from_le_bytes(word(&b[0..4])),
            i32::from_le_bytes(word(&b[4..8])),
            i32::from_le_bytes(word(&b[8..12])),
        ]);
    }
    Some((speeds, jammed))
}

/// A turning piece of a part: its model, where its shaft points (+Y in
/// the model), and its speed for each turn of the part's.
fn pieces(part: Part) -> impl Iterator<Item = (&'static str, Dir, f32)> {
    let (whole, axis_dir): (Option<&'static str>, Dir) = match part {
        Part::Shaft(a) => (Some("kinetica:block/shaft"), positive(a)),
        Part::Cog(a) => (Some("kinetica:block/cogwheel"), positive(a)),
        Part::LargeCog(a) => (Some("kinetica:block/large_cogwheel"), positive(a)),
        Part::Gearbox(_) | Part::Motor(_) => (None, Dir::Up),
    };
    let halves = Dir::ALL
        .into_iter()
        .filter(move |d| whole.is_none() && part.shaft_toward(*d));
    whole
        .map(|m| (m, axis_dir, 1.0))
        .into_iter()
        .chain(halves.map(move |d| ("kinetica:block/shaft_half", d, part.side_factor(d))))
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
    /// world, the speeds follow the index, and the players hear of them.
    pub(super) fn tachyon_tick_blocks(&mut self) {
        let Some(kinds) = self.tachyon_kinds() else {
            return;
        };
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let send = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            owner.ticks += 1;
            // Sculk reads the writes after this: they stay.
            let mut removed = Vec::new();
            let mut changed = owner
                .index
                .sync(&mut self.world, &kinds, false, &mut removed);
            // A motor broken forgets its speed.
            let before = owner.motors.len();
            owner.motors.retain(|p, _| !removed.contains(p));
            if owner.motors.len() != before {
                owner.save(&root, dimension);
            }
            changed |= std::mem::take(&mut owner.dirty);
            let mut send = owner.ticks % RESEND == 1;
            if changed {
                let sources: HashMap<[i32; 3], f32> = owner
                    .index
                    .parts
                    .iter()
                    .filter_map(|(p, part)| match part {
                        // The set speed is the motor's as its front shows
                        // it; its own is along the axis.
                        Part::Motor(out) => {
                            let set = owner.motors.get(p).copied().unwrap_or(MOTOR_SPEED);
                            Some((*p, if out.positive() { set } else { -set }))
                        }
                        _ => None,
                    })
                    .collect();
                let solved = kinetic::solve(&owner.index.parts, &sources);
                if solved.speeds != owner.speeds || solved.jammed != owner.jammed {
                    owner.speeds = solved.speeds;
                    owner.jammed = solved.jammed;
                    send = true;
                }
            }
            send.then(|| encode(&owner.speeds, &owner.jammed))
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

    /// The player used the block at `pos` (the world's owner, before the
    /// block's own use): a creative motor's speed changes. Whether it was
    /// Tachyon's to use.
    pub(super) fn tachyon_use(&mut self, pos: [i32; 3]) -> bool {
        if self.remote.is_some() {
            return false;
        }
        let Some(kinds) = self.tachyon_kinds() else {
            return false;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]).0;
        if !matches!(kinds.part(state), Some(Part::Motor(_))) {
            return false;
        }
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            let speed = owner.motors.get(&pos).copied().unwrap_or(MOTOR_SPEED);
            owner.motors.insert(pos, next_motor_speed(speed, sneaking));
            owner.dirty = true;
            owner.save(&root, dimension);
        });
        true
    }

    /// Each tick of every game: a client's index follows its world.
    pub(super) fn tachyon_tick(&mut self) {
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
                client.speeds.clear();
                client.jammed.clear();
                client.dimension = dimension;
            }
            client
                .index
                .sync(&mut self.world, &kinds, true, &mut Vec::new());
        });
    }

    /// A `tachyon:*` payload from the server.
    pub(super) fn tachyon_payload(&mut self, channel: &str, data: &[u8]) {
        if channel != KINETIC {
            return;
        }
        match decode(data) {
            Some((speeds, jammed)) => CLIENT.with_borrow_mut(|client| {
                client.speeds = speeds;
                client.jammed = jammed;
            }),
            None => log::warn!("Tachyon: a {KINETIC} payload that doesn't read"),
        }
    }

    /// The parts this game shows and their speeds: a client's, or the
    /// owner's when the world runs in the window's frame.
    fn tachyon_view<R>(
        &self,
        f: impl FnOnce(&Index, &HashMap<[i32; 3], f32>, &HashSet<[i32; 3]>) -> R,
    ) -> R {
        if self.remote.is_some() {
            CLIENT.with_borrow(|c| f(&c.index, &c.speeds, &c.jammed))
        } else {
            let dimension = self.dim.id();
            OWNERS.with_borrow(|owners| match owners.get(dimension) {
                Some(o) => f(&o.index, &o.speeds, &o.jammed),
                None => f(&Index::default(), &HashMap::new(), &HashSet::new()),
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
        let mut draws: Vec<([i32; 3], Part, f32)> = Vec::new();
        self.tachyon_view(|index, speeds, _| {
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
                draws.push((*pos, *part, speeds.get(pos).copied().unwrap_or(0.0)));
            }
        });
        for (pos, part, speed) in draws {
            let packed = self.world.light(pos[0], pos[1], pos[2]);
            let light = horizon_client::block_entity_render::vertex_light(packed);
            let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
            let origin = (at - camera).as_vec3();
            for (model, toward, factor) in pieces(part) {
                let speed = speed * factor;
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
        if !self.native_corner_shown(Corner::TopLeft) || self.tachyon_kinds().is_none() {
            return;
        }
        let Some(hit) = self.pick() else {
            return;
        };
        let line = self.tachyon_view(|index, speeds, jammed| {
            let part = index.parts.get(&hit.pos)?;
            let speed = speeds.get(&hit.pos).copied().unwrap_or(0.0);
            let what = match part {
                Part::Shaft(_) => "Shaft",
                Part::Cog(_) => "Cogwheel",
                Part::LargeCog(_) => "Large Cogwheel",
                Part::Gearbox(_) => "Gearbox",
                Part::Motor(_) => "Creative Motor",
            };
            Some(if jammed.contains(&hit.pos) {
                format!("{what}: jammed")
            } else if speed == 0.0 {
                format!("{what}: still")
            } else {
                format!("{what}: {} RPM", speed.abs().round())
            })
        });
        let Some(line) = line else {
            return;
        };
        let w = self.native_text_width(&line) + 4.0;
        let (x, y) = corners.place(Corner::TopLeft, w, 12.0);
        self.native_fill(batch, [x, y, w, 12.0], 0x7800_0000);
        self.native_text(batch, &line, [x + 2.0, y + 2.0], 0xFFD27A, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speeds_go_and_come_back() {
        let speeds = HashMap::from([([1, -64, 3], 32.0), ([-5, 70, 9], -16.0)]);
        let jammed = HashSet::from([[0, 0, 0]]);
        let (s, j) = decode(&encode(&speeds, &jammed)).unwrap();
        assert_eq!((s, j), (speeds, jammed));
        assert!(decode(&[2]).is_none());
        assert!(decode(&encode(&speeds, &jammed)[..10]).is_none());
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
    }

    #[test]
    fn a_gearbox_shows_four_turning_shafts_a_shaft_one() {
        assert_eq!(pieces(Part::Gearbox(Axis::Y)).count(), 4);
        assert_eq!(pieces(Part::Shaft(Axis::Z)).count(), 1);
        assert_eq!(pieces(Part::Motor(Dir::East)).count(), 1);
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
