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
mod application;
mod arm;
mod assist;
mod basin;
mod bracket;
mod burner;
mod belt;
pub(in crate::game) mod chute;
mod clock;
mod create_recipes;
mod crusher;
mod fan;
mod girder;
pub(super) mod loading;
mod millstone;
mod obj_model;
mod press;
mod deployer;
mod drill;
mod create_belt;
mod fluids;
mod hose;
mod redstone;
mod vault;
mod steam;
mod saw;
mod speed_controller;
mod water_wheel;
mod wrench;

// The render crate's touch logic, tested by the desktop Tachyon target.
#[cfg(test)]
mod touch;
#[cfg(test)]
mod touch_hud;
#[cfg(test)]
mod touch_input;

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
use kinetic::{Axis, ChuteShape, Dir, Part};
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
/// Ticks between full sends of the speeds, for players who just came.
const RESEND: u64 = 100;
/// How far turning parts are drawn, in blocks.
const VIEW: f64 = 64.0;

/// The kinetic part of each block state (by id), for one registry.
struct Kinds {
    registry: usize,
    parts: Vec<Option<Part>>,
    /// The states of Create's blocks (ported from its own file), drawn with
    /// its models.
    create: Vec<bool>,
    any: bool,
}

impl Kinds {
    fn new(registry: &BlockRegistry) -> Self {
        let mut parts = Vec::with_capacity(registry.len());
        let mut create = Vec::with_capacity(registry.len());
        for id in 0..registry.len() {
            let state = registry.state(BlockStateId(id as u32));
            let part = state
                .and_then(|s| part_of(&s.name, |k| s.properties.get(k).map(String::as_str)));
            parts.push(part);
            create.push(state.is_some_and(|s| s.name.starts_with("create:")));
        }
        let any = parts.iter().any(Option::is_some);
        Self {
            registry: std::ptr::from_ref(registry) as usize,
            parts,
            create,
            any,
        }
    }
    fn part(&self, state: u32) -> Option<Part> {
        self.parts.get(state as usize).copied().flatten()
    }
    /// Whether the state is a block of Create's.
    fn create(&self, state: u32) -> bool {
        self.create.get(state as usize).copied().unwrap_or(false)
    }
}

/// The part a block state is: Kinetica's blocks by name and properties.
fn part_of<'a>(name: &str, property: impl Fn(&str) -> Option<&'a str>) -> Option<Part> {
    if let Some(block) = name.strip_prefix("create:") {
        return create_part(block, property);
    }
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
        "mechanical_press" => Some(Part::Press(axis()?)),
        "depot" => Some(Part::Depot),
        "mechanical_mixer" => Some(Part::Mixer),
        "basin" => Some(Part::Basin),
        "encased_fan" => Some(Part::Fan(axis()?)),
        // A stonecutter's facing: the line it carries along.
        "belt" => Some(Part::Belt(property("facing").and_then(Dir::parse)?.axis())),
        // A lightning rod's facing points out of the block it was put on.
        "funnel" => Some(Part::Funnel(property("facing").and_then(Dir::parse)?)),
        _ => None,
    }
}

/// The part a block of Create's is (ported from its own file on the
/// player's device, `tachyon/create_port.rs` choosing its templates), by
/// the properties those templates give it.
fn create_part<'a>(block: &str, property: impl Fn(&str) -> Option<&'a str>) -> Option<Part> {
    let axis = || property("axis").and_then(Axis::parse);
    let facing = || property("facing").and_then(Dir::parse);
    Some(match block {
        "shaft" | "andesite_encased_shaft" | "brass_encased_shaft" => Part::Shaft(axis()?),
        // GirderEncasedShaftBlock: a shaft along its axis (x or z).
        "metal_girder_encased_shaft" => Part::Shaft(axis()?),
        "cogwheel" => Part::Cog(axis()?),
        "large_cogwheel" => Part::LargeCog(axis()?),
        // EncasedCogwheelBlock's default closes both shaft ends.
        "andesite_encased_cogwheel" | "brass_encased_cogwheel" => Part::EncasedCog(axis()?),
        "andesite_encased_large_cogwheel" | "brass_encased_large_cogwheel" => {
            Part::EncasedLargeCog(axis()?)
        }
        "gearbox" => Part::Gearbox(axis()?),
        // GaugeBlock's facing isn't its shaft; create_port keeps an axis.
        "speedometer" => Part::SpeedGauge(axis()?),
        "stressometer" => Part::StressGauge(axis()?),
        "clutch" => Part::Clutch(axis()?),
        "gearshift" => Part::Gearshift(axis()?),
        // Its facing is where its shaft points.
        "creative_motor" => Part::Motor(facing()?),
        // Its facing points out of the block it turns.
        "hand_crank" => Part::Crank(facing()?.opposite()),
        "millstone" => Part::Millstone,
        // SawBlock: its blade's facing; `axis_along_first` its shaft's way.
        "mechanical_saw" => Part::Saw {
            facing: facing()?,
            along_first: property("axis_along_first")? == "true",
        },
        // DrillBlock: its head's facing.
        "mechanical_drill" => Part::Drill(facing()?),
        // FluidPipeBlock's six sides.
        "fluid_pipe" => Part::Pipe(
            Dir::ALL
                .into_iter()
                .filter(|d| property(d.name()) == Some("true"))
                .fold(0, |m, d| m | fluids::bit(d)),
        ),
        "mechanical_pump" => Part::Pump(facing()?),
        // FluidValveBlock: a DirectionalAxisKineticBlock, its pipe across both.
        "fluid_valve" => Part::Valve {
            facing: facing()?,
            along_first: property("axis_along_first")? == "true",
        },
        // SmartFluidPipeBlock.getPipeAxis: up and down on a wall, else along its facing.
        "smart_fluid_pipe" => Part::SmartPipe(match property("face")? {
            "wall" => Axis::Y,
            _ => facing()?.axis(),
        }),
        "hose_pulley" => Part::HosePulley(facing()?),
        "item_vault" => Part::Vault(axis()?),
        "analog_lever" => Part::AnalogLever,
        "powered_latch" => Part::Latch(facing()?),
        "powered_toggle_latch" => Part::ToggleLatch(facing()?),
        // BeltBlock, level ones: run as Kinetica's belt along its facing (`create_belt.rs`).
        "belt" if property("slope") == Some("horizontal") => Part::Belt(facing()?.axis()),
        // FaceAttachedHorizontalDirectionalBlock's connected way: up from a floor, down from a
        // ceiling, else its facing (the grindstone template keeps `face` and `facing`).
        "steam_engine" => Part::SteamEngine(match property("face")? {
            "floor" => Dir::Up,
            "ceiling" => Dir::Down,
            _ => facing()?,
        }),
        "powered_shaft" => Part::PoweredShaft(axis()?),
        "fluid_tank" => Part::Tank,
        "spout" => Part::Spout,
        "item_drain" => Part::Drain,
        // DeployerBlock: its hand's facing; `axis_along_first` its shaft's way.
        "deployer" => Part::Deployer {
            facing: facing()?,
            along_first: property("axis_along_first")? == "true",
        },
        "depot" => Part::Depot,
        // The stair template keeps four horizontal facings. Its bottom
        // half means down, its top half the diagonal chute's facing.
        "chute" => {
            let dir = if property("half") == Some("bottom") { Dir::Down } else { facing()? };
            if dir == Dir::Up {
                return None;
            }
            Part::Chute(dir, ChuteShape::parse(property("shape")?)?)
        }
        // Its shaft goes through it the way it faces.
        "mechanical_press" => Part::Press(facing()?.axis()),
        "mechanical_mixer" => Part::Mixer,
        "mechanical_arm" => Part::Arm,
        "basin" => Part::Basin,
        "blaze_burner" => Part::Burner,
        "lit_blaze_burner" => Part::LitBurner,
        "encased_fan" => Part::Fan(axis()?),
        "andesite_funnel" | "brass_funnel" => Part::Funnel(facing()?),
        "crushing_wheel" => Part::CrushingWheel(axis()?),
        // Its facing is its shaft's way.
        "water_wheel" => Part::WaterWheel(facing()?.axis()),
        // LargeWaterWheelBlock uses an axis, not a facing.
        "large_water_wheel" => Part::LargeWaterWheel(axis()?),
        // ChainDriveBlock's part and axis_along_first follow from the
        // drives beside it; create_port keeps the axis.
        "encased_chain_drive" => Part::ChainDrive(axis()?),
        "flywheel" => Part::Flywheel(axis()?),
        // CuckooClockBlock: its shaft out of the back; the face is its facing.
        "cuckoo_clock" => Part::Clock(facing()?.opposite(), false),
        "mysterious_cuckoo_clock" => Part::Clock(facing()?.opposite(), true),
        "adjustable_chain_gearshift" => Part::ChainGearshift(axis()?),
        // The four-way template's facing gives HorizontalAxisKineticBlock's
        // shaft across the player's facing, below the cogwheel.
        "rotation_speed_controller" => Part::SpeedController(match facing()?.axis() {
            Axis::X => Axis::Z,
            Axis::Z => Axis::X,
            Axis::Y => return None,
        }),
        _ => return None,
    })
}

/// The kinetic parts of a world's loaded chunks.
#[derive(Default)]
struct Index {
    parts: HashMap<[i32; 3], Part>,
    /// The parts that are Create's blocks: they work by Create's recipes.
    create: HashSet<[i32; 3]>,
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
                self.create
                    .retain(|p| !gone.contains(&(p[0] >> 4, p[2] >> 4)));
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
            if is.is_some() && kinds.create(new) {
                self.create.insert([x, y, z]);
            } else {
                self.create.remove(&[x, y, z]);
            }
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
                    if kinds.create(*v) {
                        self.create.insert(pos);
                    }
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
    /// Rotation speed controllers' signed targets, kept per block as
    /// SpeedControllerBlockEntity's scroll value (RPM, initially 16).
    controllers: HashMap<[i32; 3], i32>,
    /// The saved state was read from the world's folder.
    loaded: bool,
    /// The speeds need solving again (a motor's speed or a target changed).
    dirty: bool,
    /// The hand cranks turning: until which tick, and their speed.
    cranks: HashMap<[i32; 3], (u64, f32)>,
    /// The clutches and gearshifts powered by redstone.
    powered: HashSet<[i32; 3]>,
    /// The adjustable chain gearshifts' redstone signals, where above 0.
    signals: HashMap<[i32; 3], u8>,
    /// The water wheels and windmills turning, at their own speeds.
    natural: HashMap<[i32; 3], f32>,
    /// What the millstones hold, kept with the motors' speeds.
    mills: HashMap<[i32; 3], millstone::Mill>,
    /// What the basins hold.
    basins: HashMap<[i32; 3], basin::Basin>,
    /// What each chute holds, with its item components and height.
    chutes: HashMap<[i32; 3], chute::Chute>,
    /// The fans' air through connected chutes, recomputed each tick.
    chute_air: HashMap<[i32; 3], chute::Air>,
    /// The arms' points, movement and held stacks.
    arms: HashMap<[i32; 3], arm::Arm>,
    /// The filled blaze burners' fuel and remaining burn time.
    burners: HashMap<[i32; 3], burner::Burner>,
    /// A machine's contents or motion changed since the file was written.
    mills_changed: bool,
    /// What each deployer holds and how far its stroke is (`deployer.rs`).
    deployers: HashMap<[i32; 3], deployer::Deployer>,
    /// The fluid each tank, spout and item drain holds (`fluids.rs`).
    tanks: HashMap<[i32; 3], fluids::Tank>,
    /// What open pipe ends hold on the way in or out of the world.
    open_ends: HashMap<[i32; 3], fluids::Tank>,
    /// The ticks each spout and item drain has worked on its item.
    spouting: HashMap<[i32; 3], u32>,
    /// The speed each steam engine turns its powered shaft at.
    steam: HashMap<[i32; 3], f32>,
    /// The tanks of running boilers, with their column's bottom, and the water each boiler
    /// took since it was last measured (mB).
    boiling: HashMap<[i32; 3], [i32; 3]>,
    boiled: HashMap<[i32; 3], u32>,
    /// Parts gone since the belts last looked (a belt broken in part breaks whole).
    belt_removed: Vec<[i32; 3]>,
    /// Each hose pulley's hose (`hose.rs`).
    hoses: HashMap<[i32; 3], hose::Hose>,
    /// What each item vault block holds (`vault.rs`).
    vaults: HashMap<[i32; 3], vault::Vault>,
    /// What each fan turning does, for the goggles ("washing", "pull:air").
    fan_work: HashMap<[i32; 3], String>,
    /// The dropped items and the depots where a fan's air works them: how,
    /// for how many ticks, and when it is due.
    fan_items: HashMap<u32, (fan::Process, u32, u32)>,
    fan_depots: HashMap<[i32; 3], (fan::Process, u32, u32)>,
    /// The funnels taking out of their containers (the others put in).
    funnels_out: HashSet<[i32; 3]>,
    /// What each funnel does, for the goggles ("inserting").
    funnel_work: HashMap<[i32; 3], String>,
    /// What the crushing wheels' gaps hold, by the gap.
    crushers: HashMap<[i32; 3], crusher::Inside>,
    /// The living entity each crushing gap keeps until it leaves.
    crushing_entities: HashMap<[i32; 3], crusher::Living>,
    /// The dropped items a gap doesn't take in (what it made, or a stack
    /// with components), by the gap.
    passed: HashMap<u32, [i32; 3]>,
    /// The crushing wheels' gaps, as of the last tick.
    gaps: HashSet<[i32; 3]>,
    /// The cycles of the presses pressing items lying under them, and on
    /// what, for the goggles.
    press_cycles: HashMap<[i32; 3], i32>,
    press_work: HashMap<[i32; 3], String>,
    /// The items an upward saw is cutting: ticks of work left, by entity.
    saw_items: HashMap<u32, f32>,
    /// What each sideways saw is breaking.
    saw_breakers: HashMap<[i32; 3], saw::Breaker>,
}

impl Owner {
    /// New controllers start at Create's default. Broken ones forget their
    /// target; unloaded ones keep it until their chunks come back.
    fn sync_controllers(&mut self, removed: &[[i32; 3]]) -> bool {
        let before = self.controllers.len();
        let parts = &self.index.parts;
        let known = &self.index.known;
        self.controllers.retain(|p, _| {
            !removed.contains(p) && match parts.get(p) {
                Some(Part::SpeedController(_)) => true,
                Some(_) => false,
                None => !known.contains(&(p[0] >> 4, p[2] >> 4)),
            }
        });
        let mut changed = self.controllers.len() != before;
        for (&p, part) in parts {
            if matches!(part, Part::SpeedController(_))
                && let std::collections::hash_map::Entry::Vacant(entry) = self.controllers.entry(p)
            {
                entry.insert(speed_controller::DEFAULT_SPEED);
                changed = true;
            }
        }
        self.dirty |= changed;
        changed
    }

    /// The motors', controllers' and machines' file in the world's folder,
    /// for this dimension.
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
        let controllers: Vec<([i32; 3], i32)> =
            serde_json::from_value(value["controllers"].clone()).unwrap_or_default();
        self.controllers = controllers
            .into_iter()
            .map(|(p, s)| (p, speed_controller::limit(s)))
            .collect();
        let mills: Vec<([i32; 3], millstone::Mill)> =
            serde_json::from_value(value["mills"].clone()).unwrap_or_default();
        self.mills = mills.into_iter().collect();
        let basins: Vec<([i32; 3], basin::Basin)> =
            serde_json::from_value(value["basins"].clone()).unwrap_or_default();
        self.basins = basins.into_iter().collect();
        let arms: Vec<([i32; 3], arm::Arm)> =
            serde_json::from_value(value["arms"].clone()).unwrap_or_default();
        self.arms = arms.into_iter().collect();
        let burners: Vec<([i32; 3], burner::Burner)> =
            serde_json::from_value(value["burners"].clone()).unwrap_or_default();
        self.burners = burners.into_iter().collect();
        let funnels: Vec<[i32; 3]> = serde_json::from_value(value["funnels"].clone()).unwrap_or_default();
        self.funnels_out = funnels.into_iter().collect();
        let crushers: Vec<([i32; 3], crusher::Inside)> =
            serde_json::from_value(value["crushers"].clone()).unwrap_or_default();
        self.crushers = crushers.into_iter().collect();
        self.chutes = chute::read(&value["chutes"]);
        let deployers: Vec<([i32; 3], deployer::Deployer)> =
            serde_json::from_value(value["deployers"].clone()).unwrap_or_default();
        self.deployers = deployers.into_iter().collect();
        let tanks: Vec<([i32; 3], fluids::Tank)> = serde_json::from_value(value["tanks"].clone()).unwrap_or_default();
        self.tanks = tanks.into_iter().collect();
        let hoses: Vec<([i32; 3], hose::Hose)> = serde_json::from_value(value["hoses"].clone()).unwrap_or_default();
        self.hoses = hoses.into_iter().collect();
        let vaults: Vec<([i32; 3], vault::Vault)> =
            serde_json::from_value(value["vaults"].clone()).unwrap_or_default();
        self.vaults = vaults.into_iter().collect();
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
        let mut controllers: Vec<(&[i32; 3], &i32)> = self.controllers.iter().collect();
        controllers.sort_by_key(|(p, _)| **p);
        let mut mills: Vec<(&[i32; 3], &millstone::Mill)> =
            self.mills.iter().filter(|(_, m)| !m.is_empty()).collect();
        mills.sort_by_key(|(p, _)| **p);
        let mut basins: Vec<(&[i32; 3], &basin::Basin)> =
            self.basins.iter().filter(|(_, b)| !b.is_empty()).collect();
        basins.sort_by_key(|(p, _)| **p);
        let mut arms: Vec<(&[i32; 3], &arm::Arm)> = self.arms.iter().collect();
        arms.sort_by_key(|(p, _)| **p);
        let mut burners: Vec<(&[i32; 3], &burner::Burner)> = self.burners.iter().collect();
        burners.sort_by_key(|(p, _)| **p);
        let mut funnels: Vec<&[i32; 3]> = self.funnels_out.iter().collect();
        funnels.sort();
        let mut crushers: Vec<(&[i32; 3], &crusher::Inside)> = self.crushers.iter().collect();
        crushers.sort_by_key(|(p, _)| **p);
        let mut chutes: Vec<(&[i32; 3], &chute::Chute)> =
            self.chutes.iter().filter(|(_, c)| c.item.is_some()).collect();
        chutes.sort_by_key(|(p, _)| **p);
        let mut deployers: Vec<(&[i32; 3], &deployer::Deployer)> =
            self.deployers.iter().filter(|(_, d)| d.held.is_some()).collect();
        deployers.sort_by_key(|(p, _)| **p);
        let file = Self::file(root, dimension);
        let mut tanks: Vec<(&[i32; 3], &fluids::Tank)> = self.tanks.iter().filter(|(_, t)| t.amount > 0).collect();
        tanks.sort_by_key(|(p, _)| **p);
        let mut hoses: Vec<(&[i32; 3], &hose::Hose)> = self.hoses.iter().filter(|(_, h)| h.offset > 0.0).collect();
        hoses.sort_by_key(|(p, _)| **p);
        let mut vaults: Vec<(&[i32; 3], &vault::Vault)> = self.vaults.iter().filter(|(_, v)| !v.is_empty()).collect();
        vaults.sort_by_key(|(p, _)| **p);
        let text = serde_json::json!({
            "hoses": hoses,
            "vaults": vaults,
            "tanks": tanks,
            "deployers": deployers,
            "motors": rows,
            "controllers": controllers,
            "mills": mills,
            "basins": basins,
            "arms": arms,
            "burners": burners,
            "funnels": funnels,
            "crushers": crushers,
            "chutes": chutes,
        })
        .to_string();
        let written = file
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&file, text));
        if let Err(e) = written {
            log::warn!("Tachyon: {} not written: {e}", file.display());
        }
    }
}

/// The machine working the depot or basin at `p`: the one `is` finds right
/// above it, else one block higher with nothing between (as Create's press
/// and mixer stand, their head down through that block).
fn worker(
    parts: &HashMap<[i32; 3], Part>,
    p: [i32; 3],
    is: impl Fn(&Part) -> bool,
) -> Option<[i32; 3]> {
    let up = |d: i32| [p[0], p[1] + d, p[2]];
    match parts.get(&up(1)) {
        Some(q) if is(q) => Some(up(1)),
        Some(_) => None,
        None => parts.get(&up(2)).filter(|&q| is(q)).map(|_| up(2)),
    }
}

/// How far down a press's or mixer's work is: 1 for the block right under
/// it, else 2 (`worker`).
fn reach(parts: &HashMap<[i32; 3], Part>, pos: [i32; 3], under: Part) -> f32 {
    if parts.get(&[pos[0], pos[1] - 1, pos[2]]) == Some(&under) {
        1.0
    } else {
        2.0
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

/// The goggles Kinetica and Create supply, as AllItems.GOGGLES and
/// GogglesItem (worn in the head slot, or held for Tachyon's HUD).
fn is_goggles_item(item: &str) -> bool {
    matches!(item, GOGGLES | "create:goggles")
}

/// A gauge's compact goggles text, as GaugeBlockEntity,
/// SpeedGaugeBlockEntity, StressGaugeBlockEntity and IRotate: absolute RPM
/// or utilization and remaining capacity. Shown keeps an overloaded
/// network's stress even though its parts have no speed entry.
fn gauge_readout(
    part: Part,
    speed: f32,
    jammed: bool,
    stress: Option<kinetic::Stress>,
    translate: impl Fn(&str, &str) -> String,
) -> Option<String> {
    if !matches!(part, Part::SpeedGauge(_) | Part::StressGauge(_)) {
        return None;
    }
    if jammed {
        return Some(translate("kinetica.hud.jammed", "jammed"));
    }
    let over = stress.is_some_and(kinetic::Stress::over);
    if matches!(part, Part::SpeedGauge(_)) {
        return Some(if over {
            translate("kinetica.hud.overstressed", "overstressed")
        } else {
            translate("kinetica.hud.rpm", "%s RPM")
                .replace("%s", &speed.abs().round().to_string())
        });
    }
    let Some(s) = stress else {
        return Some(translate("create.gui.stressometer.no_rotation", "No Rotation"));
    };
    if speed == 0.0 && s.load == 0.0 && s.capacity == 0.0 {
        return Some(translate("create.gui.stressometer.no_rotation", "No Rotation"));
    }
    let capacity = f64::from(s.capacity);
    let load = f64::from(s.load);
    let divisor = if capacity == 0.0 { 1.0 } else { capacity };
    let percent = (load / divisor * 100.0) as i32;
    let su = translate("kinetica.hud.stress", "%s / %s SU")
        .replacen("%s", &((capacity - load).round() as i64).to_string(), 1)
        .replacen("%s", &(capacity.round() as i64).to_string(), 1);
    let label = translate("create.gui.stressometer.capacity", "Remaining Capacity");
    let mut state = format!("{percent}% | {label}: {su}");
    if over {
        let status = translate("kinetica.hud.overstressed", "overstressed");
        state = format!("{state} | {status}");
    }
    Some(state)
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
    /// The presses' heads, by their cycles (a guest's and the owner's
    /// window alike).
    heads: press::PressHeads,
}

thread_local! {
    static KINDS: RefCell<Option<Arc<Kinds>>> = const { RefCell::new(None) };
    /// Create's recipes, read once per registry (by its address).
    static CREATE: RefCell<Option<(usize, Arc<create_recipes::Recipes>)>> = const { RefCell::new(None) };
    static OWNERS: RefCell<HashMap<&'static str, Owner>> = RefCell::new(HashMap::new());
    static CLIENT: RefCell<Client> = RefCell::new(Client::default());
}

/// A machine's recipe for the recipe viewer: its inputs (item, count), its
/// work, and its outputs (item, count, chance).
pub(in crate::game) type Processing = (Vec<(String, u32)>, f32, Vec<(String, u32, f32)>);

/// The millstone's recipes, the press's, the mixer's and a fan's washing,
/// for the recipe viewer.
pub(in crate::game) fn processing() -> [Vec<Processing>; 4] {
    let outputs = |list: &[millstone::Output]| -> Vec<(String, u32, f32)> {
        (list.iter())
            .map(|o| (o.item.clone(), o.count, o.chance))
            .collect()
    };
    let list = |recipes: &[millstone::Milling]| -> Vec<Processing> {
        (recipes.iter())
            .map(|r| (vec![(r.input.clone(), 1)], r.time, outputs(&r.outputs)))
            .collect()
    };
    let mixing: Vec<Processing> = (basin::recipes().iter())
        .map(|r| {
            let inputs: Vec<(String, u32)> =
                r.inputs.iter().map(|i| (i.item.clone(), i.count)).collect();
            (inputs, r.time, outputs(&r.outputs))
        })
        .collect();
    // A fan's air works an item in the same ticks whatever its speed.
    let washing = (fan::washing().iter())
        .map(|r| (vec![(r.input.clone(), 1)], fan::time(1) as f32, outputs(&r.outputs)))
        .collect();
    [list(millstone::recipes()), list(millstone::pressing()), mixing, washing]
}

/// Create's recipe categories for the recipe viewer, as JEI shows them (Create's
/// `CreateJEI`): milling, crushing, pressing, mixing, compacting, fan washing, fan
/// haunting, cutting, item application, filling and emptying, in that order.
pub(in crate::game) const CREATE_CATEGORIES: usize = 11;

/// A Create recipe for the viewer: its category (`CREATE_CATEGORIES`' order), each input
/// slot's items (a tag's several) and count, its work in ticks, its outputs, and a note
/// (heat needed, a held item kept).
pub(in crate::game) type CreateShown = (usize, Vec<(Vec<String>, u32)>, f32, Vec<(String, u32, f32)>, String);

impl Game<'_> {
    /// Create's recipes the machines work (read from the Create the player fetched), for the
    /// recipe viewer; none without Create.
    pub(in crate::game) fn tachyon_create_shown(&self) -> Vec<CreateShown> {
        let r = self.tachyon_create_recipes();
        let outputs = |list: &[millstone::Output]| -> Vec<(String, u32, f32)> {
            list.iter().map(|o| (o.item.clone(), o.count, o.chance)).collect()
        };
        // A fluid shows as its bucket, its amount in the note (JEI draws the fluid itself).
        let fluid = |item: &str, count: u32, note: &mut Vec<String>| -> Option<(Vec<String>, u32)> {
            match item.strip_prefix(fluids::FLUID) {
                Some(f) => {
                    note.push(format!("{count} mB {}", fluids::fluid_name(f)));
                    Some((vec![fluids::bucket_of(f)?.to_owned()], 1))
                }
                None => Some((vec![item.to_owned()], count)),
            }
        };
        let mut out = Vec::new();
        let singles: [(usize, &Vec<millstone::Milling>); 6] = [
            (0, &r.milling),
            (1, &r.crushing),
            (2, &r.pressing),
            (5, &r.splashing),
            (6, &r.haunting),
            (7, &r.cutting),
        ];
        for (category, list) in singles {
            for m in list {
                out.push((category, vec![(vec![m.input.clone()], 1)], m.time, outputs(&m.outputs), String::new()));
            }
        }
        for (category, list) in [(3, &r.mixing), (4, &r.compacting)] {
            for m in list {
                let heat = match m.heat_requirement {
                    basin::HeatRequirement::None => "",
                    basin::HeatRequirement::Heated => "Heated",
                    basin::HeatRequirement::Superheated => "Superheated",
                };
                let mut note: Vec<String> = Vec::new();
                if !heat.is_empty() {
                    note.push(heat.to_owned());
                }
                let inputs: Option<Vec<(Vec<String>, u32)>> =
                    m.inputs.iter().map(|i| fluid(&i.item, i.count, &mut note)).collect();
                let made: Option<Vec<(String, u32, f32)>> = (m.outputs.iter())
                    .map(|o| fluid(&o.item, o.count, &mut note).map(|(i, n)| (i[0].clone(), n, o.chance)))
                    .collect();
                let (Some(inputs), Some(made)) = (inputs, made) else { continue };
                out.push((category, inputs, m.time, made, note.join(", ")));
            }
        }
        for a in &r.application {
            let mut made = vec![(a.result.clone(), 1, 1.0)];
            made.extend(outputs(&a.extra));
            let note = if a.keep { "Held item kept" } else { "" };
            out.push((8, vec![(a.on.clone(), 1), (a.held.clone(), 1)], 0.0, made, note.to_owned()));
        }
        for f in &r.filling {
            let Some(bucket) = fluids::bucket_of(&f.fluid) else { continue };
            let note = format!("{} mB {}", f.amount, fluids::fluid_name(&f.fluid));
            let inputs = vec![(f.input.clone(), 1), (vec![bucket.to_owned()], 1)];
            out.push((9, inputs, 0.0, outputs(&f.outputs), note));
        }
        for e in &r.emptying {
            let mut note = Vec::new();
            let made: Option<Vec<(String, u32, f32)>> = (e.outputs.iter())
                .map(|o| fluid(&o.item, o.count, &mut note).map(|(i, n)| (i[0].clone(), n, o.chance)))
                .collect();
            let Some(made) = made else { continue };
            out.push((10, vec![(e.input.clone(), 1)], 0.0, made, note.join(", ")));
        }
        out
    }
}

/// What a game shows of the parts: their speeds, the jammed ones, their
/// networks' stress, the machines at work (a millstone, a press, a mixer:
/// on what), what each depot, basin and chute holds, and the item in each
/// crushing gap.
#[derive(Clone, Debug, Default, PartialEq)]
struct Shown {
    speeds: kinetic::Speeds,
    jammed: HashSet<[i32; 3]>,
    stress: HashMap<[i32; 3], kinetic::Stress>,
    work: HashMap<[i32; 3], String>,
    held: HashMap<[i32; 3], Vec<(String, u32)>>,
    crushers: HashMap<[i32; 3], String>,
    /// Chute items' previous and current heights, and the open pull below.
    chutes: HashMap<[i32; 3], [f32; 3]>,
    targets: HashMap<[i32; 3], i32>,
    /// The mechanical arms' movement and held stacks.
    arms: HashMap<[i32; 3], arm::Snapshot>,
    /// The deployers' strokes: their state (0 waiting, 1 out, 2 back) and timer.
    deployers: HashMap<[i32; 3], [i32; 2]>,
    /// How far each hose pulley's hose is down, in thousandths of a block.
    hoses: HashMap<[i32; 3], i32>,
}

/// What the parts show as a payload: the turning parts (each half's
/// speed), the jammed ones, the stress of each part's network, the
/// machines at work, the stacks they hold and the controllers' targets.
fn encode(shown: &Shown) -> Vec<u8> {
    let Shown {
        speeds,
        jammed,
        stress,
        work,
        held,
        crushers,
        chutes,
        targets,
        arms,
        deployers,
        hoses,
    } = shown;
    let mut out = Vec::with_capacity(
        37 + speeds.len() * 20 + jammed.len() * 12 + stress.len() * 20 + targets.len() * 16,
    );
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
    let name = |out: &mut Vec<u8>, item: &str| {
        let name = &item.as_bytes()[..item.len().min(255)];
        out.push(name.len() as u8);
        out.extend_from_slice(name);
    };
    out.extend_from_slice(&(work.len() as u32).to_le_bytes());
    for (p, item) in work {
        pos(&mut out, p);
        name(&mut out, item);
    }
    out.extend_from_slice(&(held.len() as u32).to_le_bytes());
    for (p, stacks) in held {
        pos(&mut out, p);
        let stacks = &stacks[..stacks.len().min(255)];
        out.push(stacks.len() as u8);
        for (item, count) in stacks {
            out.extend_from_slice(&count.to_le_bytes());
            name(&mut out, item);
        }
    }
    out.extend_from_slice(&(crushers.len() as u32).to_le_bytes());
    for (p, item) in crushers {
        pos(&mut out, p);
        name(&mut out, item);
    }
    out.extend_from_slice(&(chutes.len() as u32).to_le_bytes());
    for (p, offsets) in chutes {
        pos(&mut out, p);
        for v in offsets {
            out.extend_from_slice(&v.to_le_bytes());
        }
    }
    out.extend_from_slice(&(targets.len() as u32).to_le_bytes());
    for (p, target) in targets {
        pos(&mut out, p);
        out.extend_from_slice(&target.to_le_bytes());
    }
    out.extend_from_slice(&(arms.len() as u32).to_le_bytes());
    for (p, arm) in arms {
        pos(&mut out, p);
        // A length-delimited snapshot includes the held stack's components.
        let data = serde_json::to_vec(arm).unwrap_or_default();
        out.extend_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(&data);
    }
    out.extend_from_slice(&(deployers.len() as u32).to_le_bytes());
    for (p, [state, timer]) in deployers {
        pos(&mut out, p);
        out.extend_from_slice(&state.to_le_bytes());
        out.extend_from_slice(&timer.to_le_bytes());
    }
    out.extend_from_slice(&(hoses.len() as u32).to_le_bytes());
    for (p, offset) in hoses {
        pos(&mut out, p);
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out
}

/// The payload's form.
const VERSION: u8 = 11;

fn decode(data: &[u8]) -> Option<Shown> {
    let mut at = 0usize;
    let mut next = |n: usize| {
        let end = at.checked_add(n)?;
        let s = data.get(at..end)?;
        at = end;
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
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut held = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let p = pos(next(12)?);
        let stacks = usize::from(next(1)?[0]);
        let mut list = Vec::with_capacity(stacks);
        for _ in 0..stacks {
            let n = u32::from_le_bytes(word(next(4)?));
            let len = usize::from(next(1)?[0]);
            let item = std::str::from_utf8(next(len)?).ok()?;
            list.push((item.to_owned(), n));
        }
        held.insert(p, list);
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut crushers = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let p = pos(next(12)?);
        let len = usize::from(next(1)?[0]);
        let item = std::str::from_utf8(next(len)?).ok()?;
        crushers.insert(p, item.to_owned());
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut chutes = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let p = pos(next(12)?);
        let b = next(12)?;
        chutes.insert(p, [
            f32::from_le_bytes(word(&b[0..4])),
            f32::from_le_bytes(word(&b[4..8])),
            f32::from_le_bytes(word(&b[8..12])),
        ]);
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut targets = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let b = next(16)?;
        targets.insert(pos(b), speed_controller::limit(i32::from_le_bytes(word(&b[12..16]))));
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut arms = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let p = pos(next(12)?);
        let len = u32::from_le_bytes(word(next(4)?)) as usize;
        let arm = serde_json::from_slice(next(len)?).ok()?;
        arms.insert(p, arm);
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut deployers = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let b = next(20)?;
        let stroke = [i32::from_le_bytes(word(&b[12..16])), i32::from_le_bytes(word(&b[16..20]))];
        deployers.insert(pos(b), stroke);
    }
    let count = u32::from_le_bytes(word(next(4)?)) as usize;
    let mut hoses = HashMap::with_capacity(count.min(1 << 10));
    for _ in 0..count {
        let b = next(16)?;
        hoses.insert(pos(b), i32::from_le_bytes(word(&b[12..16])));
    }
    Some(Shown {
        speeds,
        jammed,
        stress,
        work,
        held,
        crushers,
        chutes,
        targets,
        arms,
        deployers,
        hoses,
    })
}

/// A turning piece of a part: its model, where its shaft points, its speed
/// for each turn of its half's, the half, and where its shaft points in the
/// model. Create's blocks are drawn with Create's own models.
fn pieces(part: Part, create: bool) -> Vec<(&'static str, Dir, f32, usize, Dir)> {
    if create {
        return create_pieces(part);
    }
    (kinetica_pieces(part).into_iter())
        .map(|(model, toward, factor, half)| (model, toward, factor, half, Dir::Up))
        .collect()
}

/// Create's partial models (AllPartialModels): a half shaft points south,
/// a propeller turns about Z, the others about Y.
/// EncasedCogRenderer and EncasedCogVisual (Create, MIT) turn shaftless
/// cogwheels when EncasedCogwheelBlock's ends are closed. A gauge has a
/// shaft, as GaugeRenderer extends ShaftRenderer; its casing stays in the
/// chunk mesh. SpeedControllerRenderer draws a whole shaft through its
/// controller's casing.
fn create_pieces(part: Part) -> Vec<(&'static str, Dir, f32, usize, Dir)> {
    const HALF: &str = "create:block/shaft_half";
    const SHAFT: &str = "create:block/shaft";
    match part {
        // ChainDriveBlock's and ChainGearshiftBlock's ShaftRenderer.
        Part::Shaft(a)
        | Part::PoweredShaft(a)
        | Part::Press(a)
        | Part::SpeedGauge(a)
        | Part::StressGauge(a)
        | Part::ChainDrive(a)
        | Part::ChainGearshift(a)
        | Part::SpeedController(a) => {
            vec![(SHAFT, positive(a), 1.0, 0, Dir::Up)]
        }
        Part::Cog(a) => vec![("create:block/cogwheel", positive(a), 1.0, 0, Dir::Up)],
        Part::LargeCog(a) => vec![("create:block/large_cogwheel", positive(a), 1.0, 0, Dir::Up)],
        Part::EncasedCog(a) => vec![("create:block/cogwheel_shaftless", positive(a), 1.0, 0, Dir::Up)],
        Part::EncasedLargeCog(a) => {
            vec![("create:block/large_cogwheel_shaftless", positive(a), 1.0, 0, Dir::Up)]
        }
        Part::Gearbox(_) => Dir::ALL
            .into_iter()
            .filter(|d| part.shaft_toward(*d))
            .map(|d| (HALF, d, part.side_factor(d), 0, Dir::South))
            .collect(),
        // CuckooClockRenderer's shaft; the hands and doors are `clock.rs`'s.
        Part::Motor(out) | Part::Clock(out, _) => vec![(HALF, out, 1.0, 0, Dir::South)],
        // HAND_CRANK_HANDLE is modelled pointing north (its base on the south side, as item.json's
        // axis): it points out of the crank's facing.
        Part::Crank(out) => vec![("create:block/hand_crank/handle", out.opposite(), 1.0, 0, Dir::North)],
        Part::Clutch(a) | Part::Gearshift(a) => {
            let [neg, pos] = a.dirs();
            vec![(HALF, neg, 1.0, 0, Dir::South), (HALF, pos, 1.0, 1, Dir::South)]
        }
        Part::Millstone => vec![("create:block/millstone/inner", Dir::Up, 1.0, 0, Dir::Up)],
        // SawRenderer's shaft: out of the back, or across an upward blade.
        Part::Saw { facing, .. } if facing.axis() != Axis::Y => {
            vec![(HALF, facing.opposite(), 1.0, 0, Dir::South)]
        }
        // DrillRenderer's head, by partialFacing: modelled facing south.
        Part::Drill(facing) => vec![("create:block/mechanical_drill/head", facing, 1.0, 0, Dir::South)],
        // PumpRenderer's cog, by partialFacing: modelled facing south.
        Part::Pump(facing) => vec![("create:block/mechanical_pump/cog", facing, 1.0, 0, Dir::South)],
        Part::Pipe(_)
        | Part::Tank
        | Part::Spout
        | Part::Drain
        | Part::SteamEngine(_)
        | Part::SmartPipe(_)
        | Part::AnalogLever
        | Part::Latch(_)
        | Part::ToggleLatch(_)
        | Part::Vault(_) => Vec::new(),
        // FluidValveRenderer's and HosePulleyRenderer's shaft through them.
        Part::Valve { facing, along_first } => {
            vec![(SHAFT, positive(deployer::shaft_axis(facing, along_first)), 1.0, 0, Dir::Up)]
        }
        Part::HosePulley(facing) => vec![(SHAFT, positive(hose::clockwise(facing).axis()), 1.0, 0, Dir::Up)],
        // DeployerRenderer's shaft through it; the pole and hand are drawn with its stroke.
        Part::Deployer { facing, along_first } => {
            vec![(SHAFT, positive(deployer::shaft_axis(facing, along_first)), 1.0, 0, Dir::Up)]
        }
        Part::Saw { along_first, .. } => {
            vec![(SHAFT, positive(if along_first { Axis::X } else { Axis::Z }), 1.0, 0, Dir::Up)]
        }
        // Its cogwheel; the head is drawn as it mixes.
        Part::Mixer => vec![("create:block/cogwheel_shaftless", Dir::Up, 1.0, 0, Dir::Up)],
        // ArmRenderer's cog; the jointed bodies are drawn separately.
        Part::Arm => vec![("create:block/mechanical_arm/cog", Dir::Up, 1.0, 0, Dir::Up)],
        Part::Fan(a) => vec![("create:block/encased_fan/propeller", positive(a), 1.0, 0, Dir::South)],
        // Their OBJ models (`obj_model.rs`), about Y.
        Part::CrushingWheel(a) => vec![("create:block/crushing_wheel/block", positive(a), 1.0, 0, Dir::Up)],
        Part::WaterWheel(a) => vec![("create:block/water_wheel/wheel", positive(a), 1.0, 0, Dir::Up)],
        // FlywheelRenderer: the shaftless wheel (OBJ, about Y) on a shaft.
        Part::Flywheel(a) => vec![
            (SHAFT, positive(a), 1.0, 0, Dir::Up),
            ("create:block/flywheel/block", positive(a), 1.0, 0, Dir::Up),
        ],
        // WaterWheelRenderer's LARGE partial, about Y too.
        Part::LargeWaterWheel(a) => vec![(water_wheel::LARGE_MODEL, positive(a), 1.0, 0, Dir::Up)],
        Part::Windmill(_)
        | Part::Belt(_)
        | Part::Depot
        | Part::Basin
        | Part::Burner
        | Part::LitBurner
        | Part::Chute(_, _)
        | Part::Funnel(_) => Vec::new(),
    }
}

/// Kinetica's pieces, their shafts +Y in their models.
fn kinetica_pieces(part: Part) -> Vec<(&'static str, Dir, f32, usize)> {
    const HALF: &str = "kinetica:block/shaft_half";
    // Out of a casing's face a hair, not in it.
    const CASED: &str = "kinetica:block/shaft_half_cased";
    match part {
        Part::Shaft(a) | Part::PoweredShaft(a) => vec![("kinetica:block/shaft", positive(a), 1.0, 0)],
        Part::Cog(a) => vec![("kinetica:block/cogwheel", positive(a), 1.0, 0)],
        Part::LargeCog(a) => vec![("kinetica:block/large_cogwheel", positive(a), 1.0, 0)],
        Part::WaterWheel(a) => vec![("kinetica:block/water_wheel", positive(a), 1.0, 0)],
        Part::Windmill(a) => vec![("kinetica:block/windmill", positive(a), 1.0, 0)],
        Part::Gearbox(_) => Dir::ALL
            .into_iter()
            .filter(|d| part.shaft_toward(*d))
            .map(|d| (CASED, d, part.side_factor(d), 0))
            .collect(),
        Part::Motor(out) => vec![(HALF, out, 1.0, 0)],
        // The handle stands out of the side the crank was put on.
        Part::Crank(out) => vec![("kinetica:block/hand_crank_handle", out.opposite(), 1.0, 0)],
        Part::Clutch(a) | Part::Gearshift(a) => {
            let [neg, pos] = a.dirs();
            vec![(CASED, neg, 1.0, 0), (CASED, pos, 1.0, 1)]
        }
        Part::Millstone => vec![("kinetica:block/millstone_top", Dir::Up, 1.0, 0)],
        // The head is drawn as it presses (`tachyon_render`).
        Part::Press(_) => Dir::ALL
            .into_iter()
            .filter(|d| part.shaft_toward(*d))
            .map(|d| (CASED, d, 1.0, 0))
            .collect(),
        // The whisk is drawn as it mixes (`tachyon_render`).
        Part::Mixer => vec![(CASED, Dir::Up, 1.0, 0)],
        // Its blades, in each end's grille.
        Part::Fan(a) => {
            let [neg, pos] = a.dirs();
            const BLADES: &str = "kinetica:block/fan_blades";
            vec![(BLADES, neg, 1.0, 0), (BLADES, pos, 1.0, 0)]
        }
        Part::Belt(a) => {
            let [neg, pos] = belt::pulleys(a).dirs();
            const HUB: &str = "kinetica:block/belt_pulley";
            vec![(HUB, neg, 1.0, 0), (HUB, pos, 1.0, 0)]
        }
        Part::Depot
        | Part::Arm
        | Part::Basin
        | Part::Burner
        | Part::LitBurner
        | Part::Funnel(_)
        | Part::CrushingWheel(_)
        | Part::LargeWaterWheel(_)
        | Part::Chute(_, _)
        | Part::EncasedCog(_)
        | Part::EncasedLargeCog(_)
        | Part::SpeedGauge(_)
        | Part::StressGauge(_)
        | Part::ChainDrive(_)
        | Part::ChainGearshift(_)
        | Part::Flywheel(_)
        | Part::Clock(_, _)
        | Part::Saw { .. }
        | Part::Drill(_)
        | Part::Deployer { .. }
        | Part::Pipe(_)
        | Part::Pump(_)
        | Part::Tank
        | Part::Spout
        | Part::Drain
        | Part::SteamEngine(_)
        | Part::Valve { .. }
        | Part::SmartPipe(_)
        | Part::HosePulley(_)
        | Part::Vault(_)
        | Part::AnalogLever
        | Part::Latch(_)
        | Part::ToggleLatch(_)
        | Part::SpeedController(_) => Vec::new(),
    }
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// Whether a use of the part is Tachyon's (`Game::tachyon_use`): a motor's,
/// a crank's, a machine's that holds items, a burner's, a belt's, a funnel's.
fn takes_use(part: Part) -> bool {
    matches!(
        part,
        Part::Motor(_)
            | Part::Crank(_)
            | Part::Millstone
            | Part::Depot
            | Part::Deployer { .. }
            | Part::Tank
            | Part::Spout
            | Part::Drain
            | Part::Basin
            | Part::Burner
            | Part::LitBurner
            | Part::Belt(_)
            | Part::Funnel(_)
            | Part::AnalogLever
            | Part::Latch(_)
            | Part::ToggleLatch(_)
    )
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

    /// Create's recipes this game's machines work by, read from the Create
    /// in the game folder once per registry; none without Create's blocks.
    fn tachyon_create_recipes(&self) -> Arc<create_recipes::Recipes> {
        let registry = self.pack.assets.registry;
        let at = std::ptr::from_ref(registry) as usize;
        if let Some(recipes) = CREATE.with_borrow(|c| c.as_ref().filter(|c| c.0 == at).map(|c| c.1.clone())) {
            return recipes;
        }
        let any = self
            .tachyon_kinds()
            .is_some_and(|k| k.create.iter().any(|c| *c));
        let recipes = if any {
            let dir = Self::options_path()
                .with_file_name(horizon_client::mod_list::DIR)
                .join("create/data/create/recipe");
            let items = self.res.items();
            let tagged = create_recipes::tagged_in(&items.item_tags);
            let known = |item: &str| items.get(item).is_some();
            let lookup = create_recipes::Lookup {
                tagged: &tagged,
                known: &known,
            };
            let r = create_recipes::Recipes::read(&dir, &lookup);
            log::info!(
                "Tachyon: Create's recipes: {} milling, {} crushing, {} pressing, {} splashing, {} haunting, {} mixing, {} compacting, {} item application",
                r.milling.len(),
                r.crushing.len(),
                r.pressing.len(),
                r.splashing.len(),
                r.haunting.len(),
                r.mixing.len(),
                r.compacting.len(),
                r.application.len()
            );
            r
        } else {
            create_recipes::Recipes::default()
        };
        let recipes = Arc::new(recipes);
        CREATE.with_borrow_mut(|c| *c = Some((at, recipes.clone())));
        recipes
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
            let revision = owner.index.revision;
            let changed = owner
                .index
                .sync(&mut self.world, &kinds, false, &mut removed);
            // A motor or controller broken forgets its speed or target, a
            // burner its fuel.
            let before = owner.motors.len();
            owner.motors.retain(|p, _| !removed.contains(p));
            let controllers = (changed || owner.dirty || owner.index.revision != revision)
                && owner.sync_controllers(&removed);
            let funnels = owner.funnels_out.len();
            owner.funnels_out.retain(|p| !removed.contains(p));
            let burners = owner.burners.len();
            owner.burners.retain(|p, _| !removed.contains(p));
            if owner.motors.len() != before
                || owner.funnels_out.len() != funnels
                || owner.burners.len() != burners
                || controllers
            {
                owner.save(&root, dimension);
            }
            // A millstone, a depot or a basin broken drops what it held.
            let mut drops: Vec<([i32; 3], ItemStack)> = Vec::new();
            for p in &removed {
                let held = (owner.mills.remove(p).map(|mut m| m.take_all()))
                    .or_else(|| owner.basins.remove(p).map(|mut b| b.take_all()))
                    .or_else(|| owner.vaults.remove(p).map(|v| v.slots));
                if let Some(held) = held {
                    drops.extend(held.into_iter().map(|h| (*p, ItemStack::new(&h.item, h.count))));
                    owner.save(&root, dimension);
                }
            }
            owner.belt_removed.extend(removed.iter().copied());
            // A broken chute, or one replaced by another engine part,
            // drops its full stack. Unloaded chunks keep their contents.
            let gone: Vec<[i32; 3]> = owner.chutes.keys().copied().filter(|p| {
                owner.index.known.contains(&(p[0] >> 4, p[2] >> 4))
                    && !matches!(owner.index.parts.get(p), Some(Part::Chute(_, _)))
            }).collect();
            for p in gone {
                if let Some(item) = owner.chutes.remove(&p).and_then(|c| c.item) {
                    drops.push((p, item));
                    owner.save(&root, dimension);
                }
            }
            // The second: whether the signal's strength counts.
            let split: Vec<([i32; 3], bool)> = owner
                .index
                .parts
                .iter()
                .filter(|(_, part)| {
                    part.split() || matches!(part, Part::Arm | Part::Funnel(_) | Part::ChainGearshift(_))
                })
                .map(|(p, part)| (*p, matches!(part, Part::ChainGearshift(_))))
                .collect();
            let nature: Option<Vec<([i32; 3], Part)>> = (owner.ticks % NATURE_TICKS == 1)
                .then(|| {
                    owner
                        .index
                        .parts
                        .iter()
                        .filter(|(_, part)| {
                            matches!(
                                part,
                                Part::WaterWheel(_) | Part::LargeWaterWheel(_) | Part::Windmill(_)
                            )
                        })
                        .map(|(p, part)| (*p, *part))
                        .collect()
                });
            (split, nature, changed, drops)
        });
        for (p, stack) in drops {
            self.pop_resource(p, stack);
        }
        let natural = nature.map(|parts| self.tachyon_natural(&parts));
        // Redstone at the clutches, gearshifts, arms and funnel points, and
        // its strength at the adjustable chain gearshifts.
        let (powered, signals) = if split.is_empty() {
            (HashSet::new(), HashMap::new())
        } else {
            self.with_level(|b, level, ticks| {
                let mut powered: HashSet<[i32; 3]> = HashSet::new();
                let mut signals: HashMap<[i32; 3], u8> = HashMap::new();
                for (p, strength) in split {
                    let at = horizon_core::position::BlockPos::new(p[0], p[1], p[2]);
                    if strength {
                        let signal = b.best_neighbor_signal(level, ticks, at).clamp(0, 15) as u8;
                        if signal > 0 {
                            signals.insert(p, signal);
                        }
                    } else if b.has_neighbor_signal(level, ticks, at) {
                        powered.insert(p);
                    }
                }
                (powered, signals)
            })
        };
        // The burners count down before the basins test their heat.
        self.tachyon_burners();
        // The fans' air on the items in it, the belts under them, the
        // funnels they come to.
        self.tachyon_chute_shapes();
        self.tachyon_fans();
        self.tachyon_chutes();
        self.tachyon_belts();
        self.tachyon_funnels();
        self.tachyon_crushers();
        self.tachyon_clocks();
        self.tachyon_saws();
        self.tachyon_deployers();
        self.tachyon_hoses();
        self.tachyon_latches();
        self.tachyon_fluids();
        self.tachyon_steam();
        let removed = OWNERS.with_borrow_mut(|owners| {
            owners.get_mut(dimension).map(|o| std::mem::take(&mut o.belt_removed)).unwrap_or_default()
        });
        if !removed.is_empty() {
            self.tachyon_belts_broken(&removed);
        }
        // BasinBlockEntity.getHeatLevel: the block directly below each
        // basin, read before borrowing the world's random for the outputs.
        let basin_heat: HashMap<[i32; 3], burner::Heat> = OWNERS.with_borrow(|owners| {
            owners
                .get(dimension)
                .map(|o| {
                    (o.basins.keys())
                        .map(|p| (*p, self.tachyon_heat([p[0], p[1] - 1, p[2]])))
                        .collect()
                })
                .unwrap_or_default()
        });
        self.tachyon_presses_on_ground();
        let create = self.tachyon_create_recipes();
        let send = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            if powered != owner.powered {
                owner.powered = powered;
                changed = true;
            }
            if signals != owner.signals {
                owner.signals = signals;
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
                            Part::WaterWheel(_) | Part::LargeWaterWheel(_) | Part::Windmill(_) => {
                                return owner.natural.get(p).map(|s| (*p, *s));
                            }
                            // A steam engine's shaft, at its boiler's speed (`steam.rs`).
                            Part::PoweredShaft(_) => return owner.steam.get(p).map(|s| (*p, *s)),
                            _ => return None,
                        };
                        Some((*p, if out.positive() { set } else { -set }))
                    })
                    .collect();
                let solved = kinetic::solve_with(
                    &owner.index.parts,
                    &sources,
                    &owner.powered,
                    &owner.controllers,
                    &owner.signals,
                );
                let targets: HashMap<[i32; 3], i32> = owner
                    .controllers
                    .iter()
                    .filter(|(p, _)| matches!(owner.index.parts.get(*p), Some(Part::SpeedController(_))))
                    .map(|(p, s)| (*p, *s))
                    .collect();
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
                if targets != shown.targets {
                    shown.targets = targets;
                    send = true;
                }
            }
            send
        });
        self.tachyon_arms();
        let send = OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            let mut send = send;
            let random = &mut self.world.random;
            // The millstones turning grind; a press turning works the depot
            // under it. What the depots hold shows on them.
            let mut work = HashMap::new();
            let mut held = HashMap::new();
            let mut chutes = HashMap::new();
            for (p, mill) in &mut owner.mills {
                let part = owner.index.parts.get(p).copied();
                if matches!(part, Some(Part::Depot | Part::Drain))
                    && let Some(h) = mill.outputs.first().or(mill.input.as_ref())
                {
                    held.insert(*p, vec![(h.item.clone(), h.count)]);
                }
                let Some(item) = mill.input.as_ref().map(|h| h.item.clone()) else {
                    // An empty depot: the next stack begins a press's cycle
                    // of its own.
                    mill.progress = 0.0;
                    continue;
                };
                // A Create machine by Create's recipes first, a Kinetica
                // one by Kinetica's.
                let (at, recipe) = match part {
                    Some(Part::Millstone) => {
                        let (ours, theirs) = (millstone::recipe(&item), create.milling(&item));
                        let r = if owner.index.create.contains(p) {
                            theirs.or(ours)
                        } else {
                            ours.or(theirs)
                        };
                        (*p, r)
                    }
                    Some(Part::Depot) => {
                        let press = |q: &Part| matches!(q, Part::Press(_));
                        let Some(at) = worker(&owner.index.parts, *p, press) else {
                            continue;
                        };
                        let (ours, theirs) = (millstone::press_recipe(&item), create.pressing(&item));
                        let r = if owner.index.create.contains(&at) {
                            theirs.or(ours)
                        } else {
                            ours.or(theirs)
                        };
                        (at, r)
                    }
                    _ => continue,
                };
                let speed = owner.shown.speeds.get(&at).map_or(0.0, |s| s[0]);
                if speed == 0.0 {
                    continue;
                }
                // A depot's press by Create's cycle, a millstone by work.
                let roll = &mut || random.next_float();
                let worked = if part == Some(Part::Depot) {
                    mill.press(speed, recipe, roll)
                } else {
                    mill.grind(speed, recipe, roll)
                };
                if worked {
                    owner.mills_changed = true;
                    work.insert(at, item);
                }
            }
            for (p, b) in &mut owner.basins {
                if owner.index.parts.get(p) != Some(&Part::Basin) {
                    continue;
                }
                let heat = basin_heat.get(p).copied().unwrap_or_default();
                let stacks: Vec<(String, u32)> = (b.outputs.iter().chain(&b.inputs))
                    .take(4)
                    .map(|h| (h.item.clone(), h.count))
                    .collect();
                if !stacks.is_empty() {
                    held.insert(*p, stacks);
                }
                // A mixer over a basin mixes: a Create one by Create's
                // recipes first, a Kinetica one by Kinetica's (the other's
                // when it holds none of these). A press over one compacts,
                // by Create's, once a cycle.
                let (at, recipes, pressing) = if let Some(at) = worker(&owner.index.parts, *p, |q| *q == Part::Mixer) {
                    let (mut first, mut second) = (basin::recipes(), create.mixing.as_slice());
                    if owner.index.create.contains(&at) {
                        std::mem::swap(&mut first, &mut second);
                    }
                    (at, if b.ready(first, heat).is_some() { first } else { second }, false)
                } else if let Some(at) = worker(&owner.index.parts, *p, |q| matches!(q, Part::Press(_))) {
                    (at, create.compacting.as_slice(), true)
                } else {
                    continue;
                };
                let speed = owner.shown.speeds.get(&at).map_or(0.0, |s| s[0]);
                let roll = &mut || random.next_float();
                let made = if pressing {
                    b.compact(speed, recipes, heat, roll)
                } else {
                    b.mix(speed, recipes, heat, roll)
                };
                if let Some(into) = made {
                    owner.mills_changed = true;
                    work.insert(at, into);
                }
            }
            for (p, t) in &owner.tanks {
                if t.amount > 0 {
                    held.insert(*p, vec![(fluids::stack_name(&t.fluid), t.amount)]);
                }
            }
            for (p, d) in &owner.deployers {
                if let Some(h) = &d.held {
                    held.insert(*p, vec![(h.item.clone(), h.count)]);
                }
            }
            for (p, v) in &owner.vaults {
                if !v.is_empty() {
                    held.insert(*p, v.slots.iter().map(|h| (h.item.clone(), h.count)).collect());
                }
            }
            for (p, chute) in &owner.chutes {
                if !matches!(owner.index.parts.get(p), Some(Part::Chute(_, _))) {
                    continue;
                }
                if let Some(item) = &chute.item {
                    held.insert(*p, vec![(item.item.to_string(), item.count)]);
                    chutes.insert(*p, [chute.previous, chute.position, chute.bottom_pull_distance]);
                }
            }
            work.extend(owner.fan_work.iter().map(|(p, w)| (*p, w.clone())));
            work.extend(owner.press_work.iter().map(|(p, w)| (*p, w.clone())));
            work.extend(owner.funnel_work.iter().map(|(p, w)| (*p, w.clone())));
            work.extend(owner.arms.iter().filter_map(|(p, a)| {
                a.held.as_ref().map(|h| (*p, h.item.to_string()))
            }));
            if work != owner.shown.work {
                owner.shown.work = work;
                send = true;
            }
            if held != owner.shown.held {
                owner.shown.held = held;
                send = true;
            }
            let crushers: HashMap<[i32; 3], String> = (owner.crushers.iter())
                .map(|(p, inside)| (*p, inside.item.clone()))
                .collect();
            if crushers != owner.shown.crushers {
                owner.shown.crushers = crushers;
                send = true;
            }
            if chutes != owner.shown.chutes {
                owner.shown.chutes = chutes;
                send = true;
            }
            let arms: HashMap<[i32; 3], arm::Snapshot> = (owner.arms.iter())
                .filter(|(p, _)| owner.index.parts.get(*p) == Some(&Part::Arm))
                .map(|(p, a)| (*p, a.snapshot(*p)))
                .collect();
            if arms != owner.shown.arms {
                owner.shown.arms = arms;
                send = true;
            }
            let strokes: HashMap<[i32; 3], [i32; 2]> = (owner.deployers.iter())
                .filter(|(p, _)| matches!(owner.index.parts.get(*p), Some(Part::Deployer { .. })))
                .map(|(p, d)| (*p, [d.state as i32, d.timer]))
                .collect();
            if strokes != owner.shown.deployers {
                owner.shown.deployers = strokes;
                send = true;
            }
            let hoses: HashMap<[i32; 3], i32> =
                owner.hoses.iter().map(|(p, h)| (*p, (h.offset * 1000.0).round() as i32)).collect();
            if hoses != owner.shown.hoses {
                owner.shown.hoses = hoses;
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
            .any(|s| is_goggles_item(&s.item))
    }

    /// The speeds of the water wheels and windmills among `parts`, each
    /// about its axis: a wheel from the water flowing past its sides (its
    /// turn about the axis), sails from the open sky above them, faster
    /// higher up.
    fn tachyon_natural(&self, parts: &[([i32; 3], Part)]) -> HashMap<[i32; 3], f32> {
        let mut out = HashMap::new();
        for &(p, part) in parts {
            let speed = match part {
                Part::WaterWheel(axis) | Part::LargeWaterWheel(axis) => {
                    let large = matches!(part, Part::LargeWaterWheel(_));
                    water_wheel::speed(axis, large, |offset| self.tachyon_water_flow(add(p, offset)))
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
        if !part.is_some_and(takes_use) {
            return false;
        }
        // Burners take their own uses, never their template's charging or
        // respawn interaction, even when the held item isn't fuel.
        if matches!(part, Some(Part::Burner | Part::LitBurner)) {
            self.tachyon_use_burner(pos, part == Some(Part::LitBurner));
            return true;
        }
        if let Some(p @ (Part::Tank | Part::Spout)) = part {
            let (dimension, root) = (self.dim.id(), self.save.root.clone());
            OWNERS.with_borrow_mut(|owners| owners.entry(dimension).or_default().load(&root, dimension));
            self.tachyon_use_tank(pos, p);
            return true;
        }
        if matches!(part, Some(Part::Deployer { .. })) {
            let (dimension, root) = (self.dim.id(), self.save.root.clone());
            OWNERS.with_borrow_mut(|owners| owners.entry(dimension).or_default().load(&root, dimension));
            self.tachyon_use_deployer(pos);
            return true;
        }
        let sneaking = self.keys.contains(&winit::keyboard::KeyCode::ShiftLeft);
        if matches!(part, Some(Part::AnalogLever | Part::Latch(_) | Part::ToggleLatch(_))) {
            return self.tachyon_use_redstone(pos, sneaking);
        }
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        // A belt: the held stack onto it, to be carried.
        if let Some(Part::Belt(_)) = part {
            if let Some(stack) = self.held().cloned() {
                let n = stack.count;
                let at = [0usize, 1, 2].map(|i| f64::from(pos[i]) + 0.5);
                self.spawn_item(stack, [at[0], at[1] + 0.2, at[2]], [0.0; 3], 20);
                self.tachyon_spend_held(n);
            }
            return true;
        }
        // A funnel: from putting in to taking out, and back.
        if let Some(Part::Funnel(_)) = part {
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                owner.load(&root, dimension);
                if !owner.funnels_out.remove(&pos) {
                    owner.funnels_out.insert(pos);
                }
                owner.save(&root, dimension);
            });
            return true;
        }
        if part == Some(Part::Basin) {
            let mut b = OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                owner.load(&root, dimension);
                owner.basins.remove(&pos).unwrap_or_default()
            });
            let moved = self.tachyon_use_basin_fluid(&mut b) || self.tachyon_use_basin(&mut b);
            OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                if !b.is_empty() {
                    owner.basins.insert(pos, b);
                }
                if moved {
                    owner.save(&root, dimension);
                }
            });
            return true;
        }
        if matches!(part, Some(Part::Millstone | Part::Depot | Part::Drain)) {
            let mut mill = OWNERS.with_borrow_mut(|owners| {
                let owner = owners.entry(dimension).or_default();
                owner.load(&root, dimension);
                owner.mills.remove(&pos).unwrap_or_default()
            });
            let moved = if matches!(part, Some(Part::Depot | Part::Drain)) {
                self.tachyon_use_depot(&mut mill)
            } else {
                self.tachyon_use_millstone(&mut mill)
            };
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
        kinds.part(state).is_some_and(takes_use)
    }

    /// Whether the block at `pos` is a machine a hopper fills and empties
    /// (a millstone, a depot, a basin): the world's owner's
    /// (game/machines.rs).
    pub(super) fn tachyon_machine(&self, pos: [i32; 3]) -> bool {
        let Some(kinds) = self.tachyon_kinds() else {
            return false;
        };
        let state = self.world.block(pos[0], pos[1], pos[2]).0;
        matches!(
            kinds.part(state),
            Some(Part::Millstone | Part::Depot | Part::Basin | Part::Drain | Part::Vault(_))
        )
    }

    /// Whether a hopper leaves the dropped `item` be: it lies in a crushing
    /// wheels' gap, to be crushed there (not what the gap passed).
    pub(super) fn tachyon_crushing(&self, item: &horizon_client::entities::ItemEntity) -> bool {
        let p = item.position;
        let cell = [p.x.floor() as i32, p.y.floor() as i32, p.z.floor() as i32];
        OWNERS.with_borrow(|owners| {
            owners
                .get(self.dim.id())
                .is_some_and(|o| o.gaps.contains(&cell) && o.passed.get(&item.id) != Some(&cell))
        })
    }

    /// A hopper's item into the machine at `pos`: a millstone takes what it
    /// grinds, a depot one stack while it holds nothing it made, a basin
    /// nine stacks of anything; none an item with components (they keep
    /// items by name and count). Whether it went in.
    pub(super) fn tachyon_machine_put(&mut self, pos: [i32; 3], one: &ItemStack) -> bool {
        let Some(kinds) = self.tachyon_kinds() else {
            return false;
        };
        if one.damage != 0 || one.extra.is_some() {
            return false;
        }
        let part = kinds.part(self.world.block(pos[0], pos[1], pos[2]).0);
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let create = self.tachyon_create_recipes();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            if let Some(Part::Vault(_)) = part {
                let into = vault::joined(&owner.index.parts, pos)
                    .into_iter()
                    .find(|p| owner.vaults.get(p).is_none_or(|v| v.room(&one.item, 1) == 1));
                let Some(into) = into else { return false };
                let took = owner.vaults.entry(into).or_default().put(&one.item, 1) == 1;
                owner.mills_changed |= took;
                return took;
            }
            if part == Some(Part::Basin) {
                let b = owner.basins.entry(pos).or_default();
                let took = b.put(&one.item, 1) == 1;
                if b.is_empty() {
                    owner.basins.remove(&pos);
                }
                owner.mills_changed |= took;
                return took;
            }
            let mill = owner.mills.entry(pos).or_default();
            let fits = match part {
                Some(Part::Millstone) => {
                    millstone::recipe(&one.item).is_some() || create.milling(&one.item).is_some()
                }
                Some(Part::Depot | Part::Drain) => mill.outputs.is_empty(),
                _ => false,
            };
            let took = fits && mill.put(&one.item, 1) == 1;
            if mill.is_empty() {
                owner.mills.remove(&pos);
            }
            owner.mills_changed |= took;
            took
        })
    }

    /// A chute checks the machine's room halfway through, without putting
    /// anything in it. The same rules as `tachyon_machine_put`, on a copy.
    pub(super) fn tachyon_machine_preview(&self, pos: [i32; 3], mut stack: ItemStack) -> Option<ItemStack> {
        let Some(kinds) = self.tachyon_kinds() else {
            return Some(stack);
        };
        if stack.damage != 0 || stack.extra.is_some() {
            return Some(stack);
        }
        let part = kinds.part(self.world.block(pos[0], pos[1], pos[2]).0);
        let create = self.tachyon_create_recipes();
        let count = OWNERS.with_borrow(|owners| {
            let owner = owners.get(self.dim.id());
            if let (Some(Part::Vault(_)), Some(o)) = (part, owner) {
                let blocks = vault::joined(&o.index.parts, pos);
                let room: u32 = (blocks.iter())
                    .map(|p| o.vaults.get(p).cloned().unwrap_or_default().room(&stack.item, stack.count))
                    .sum();
                return room.min(stack.count);
            }
            if part == Some(Part::Basin) {
                let mut basin = owner.and_then(|o| o.basins.get(&pos)).cloned().unwrap_or_default();
                return basin.put(&stack.item, stack.count);
            }
            let mut mill = owner.and_then(|o| o.mills.get(&pos)).cloned().unwrap_or_default();
            let fits = match part {
                Some(Part::Millstone) => {
                    millstone::recipe(&stack.item).is_some() || create.milling(&stack.item).is_some()
                }
                Some(Part::Depot | Part::Drain) => mill.outputs.is_empty(),
                _ => false,
            };
            if fits { mill.put(&stack.item, stack.count) } else { 0 }
        });
        stack.count -= count;
        (stack.count > 0).then_some(stack)
    }

    /// One item out of the machine at `pos` for a hopper under it: what it
    /// made, or a depot's stack no press works (and whether it was that).
    pub(super) fn tachyon_machine_take(&mut self, pos: [i32; 3]) -> Option<(ItemStack, bool)> {
        let kinds = self.tachyon_kinds()?;
        let part = kinds.part(self.world.block(pos[0], pos[1], pos[2]).0);
        let dimension = self.dim.id();
        let root = self.save.root.clone();
        let create = self.tachyon_create_recipes();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            owner.load(&root, dimension);
            if let Some(Part::Vault(_)) = part {
                let from = vault::joined(&owner.index.parts, pos)
                    .into_iter()
                    .find(|p| owner.vaults.get(p).is_some_and(|v| !v.is_empty()))?;
                let v = owner.vaults.get_mut(&from)?;
                let one = v.take_one()?;
                if v.is_empty() {
                    owner.vaults.remove(&from);
                }
                owner.mills_changed = true;
                return Some((ItemStack::new(&one.item, one.count), false));
            }
            // A basin: what was mixed only.
            if part == Some(Part::Basin) {
                let b = owner.basins.get_mut(&pos)?;
                let one = b.take_one()?;
                if b.is_empty() {
                    owner.basins.remove(&pos);
                }
                owner.mills_changed = true;
                return Some((ItemStack::new(&one.item, one.count), false));
            }
            let mill = owner.mills.get_mut(&pos)?;
            let pressed = mill
                .input
                .as_ref()
                .is_some_and(|h| {
                    millstone::press_recipe(&h.item).is_some() || create.pressing(&h.item).is_some()
                });
            let worked = || worker(&owner.index.parts, pos, |q| matches!(q, Part::Press(_)));
            // BeltDeployerCallbacks holds an item a deployer two blocks up has a recipe for.
            let above = [pos[0], pos[1] + 2, pos[2]];
            let deployed = matches!(owner.index.parts.get(&above), Some(Part::Deployer { facing: Dir::Down, .. }))
                && (owner.deployers.get(&above).and_then(|d| d.held.as_ref()))
                    .zip(mill.input.as_ref())
                    .is_some_and(|(held, input)| create.deploying(&input.item, &held.item).is_some());
            // A spout two blocks up holds the depot's item too (SpoutBlockEntity's BeltProcessingBehaviour).
            let spouted = owner.index.parts.get(&above) == Some(&Part::Spout);
            let input_too = part == Some(Part::Depot) && !(pressed && worked().is_some()) && !deployed && !spouted;
            let (one, input) = mill.take_one(input_too)?;
            if mill.is_empty() {
                owner.mills.remove(&pos);
            }
            owner.mills_changed = true;
            Some((ItemStack::new(&one.item, one.count), input))
        })
    }

    /// What a hopper took from the machine at `pos` and couldn't keep (of
    /// the items to work, `input`).
    pub(super) fn tachyon_machine_return(&mut self, pos: [i32; 3], rest: ItemStack, input: bool) {
        let state = self.world.block(pos[0], pos[1], pos[2]).0;
        let part = self.tachyon_kinds().and_then(|k| k.part(state));
        let basin = part == Some(Part::Basin);
        let dimension = self.dim.id();
        OWNERS.with_borrow_mut(|owners| {
            let owner = owners.entry(dimension).or_default();
            let held = millstone::Held {
                item: rest.item.to_string(),
                count: rest.count,
            };
            if let Some(Part::Vault(_)) = part {
                let v = owner.vaults.entry(pos).or_default();
                v.slots.insert(0, held);
            } else if basin {
                owner.basins.entry(pos).or_default().give_back(held);
            } else {
                owner.mills.entry(pos).or_default().give_back(held, input);
            }
        });
    }

    /// Each tick of every game: a client's index follows its world.
    pub(super) fn tachyon_tick(&mut self) {
        self.tachyon_quests_tick();
        self.tachyon_fan_particles();
        self.tachyon_burner_particles();
        self.tachyon_press_heads();
        self.tachyon_crusher_particles();
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

    /// A tick of the presses' heads this game draws: those at work run
    /// their cycles.
    fn tachyon_press_heads(&self) {
        let working: Vec<([i32; 3], f32)> = self.tachyon_view(|index, shown| {
            (shown.work.keys())
                .filter(|p| matches!(index.parts.get(*p), Some(Part::Press(_))))
                .map(|p| (*p, shown.speeds.get(p).map_or(0.0, |s| s[0])))
                .collect()
        });
        CLIENT.with_borrow_mut(|client| client.heads.tick(working.into_iter()));
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
        let Some(kinds) = self.tachyon_kinds() else {
            return;
        };
        let ticks = self.world.game_time as f64 + alpha;
        let view = self.view_frustum;
        // Each with whether it is Create's block.
        let mut draws: Vec<([i32; 3], Part, [f32; 2], bool)> = Vec::new();
        let mut heads: Vec<([i32; 3], Part, f32, f32, bool)> = Vec::new();
        let mut trays: Vec<([i32; 3], Part, Vec<ItemStack>, bool)> = Vec::new();
        let mut arms: Vec<([i32; 3], f32, arm::Snapshot)> = Vec::new();
        let mut strokes: HashMap<[i32; 3], ([i32; 2], bool)> = HashMap::new();
        let mut hoses: HashMap<[i32; 3], f32> = HashMap::new();
        self.tachyon_view(|index, shown| {
            for (pos, part) in &index.parts {
                let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
                if (at + DVec3::splat(0.5)).distance_squared(camera) > VIEW * VIEW {
                    continue;
                }
                let o = (at - camera).as_vec3();
                // Large cogwheels reach a block past their own. A large
                // water wheel uses WaterWheelBlockEntity's bounds inflated
                // by its size.
                let radius = if *part == Part::Arm { 3.0 } else if matches!(part, Part::LargeWaterWheel(_)) { 2.0 } else { 1.5 };
                if view.is_some_and(|v| !v.intersects(o - Vec3::splat(radius), o + Vec3::splat(radius + 1.0))) {
                    continue;
                }
                let speed = shown.speeds.get(pos).copied().unwrap_or([0.0; 2]);
                let create = kinds.create(self.world.block(pos[0], pos[1], pos[2]).0);
                if matches!(part, Part::HosePulley(_)) {
                    hoses.insert(*pos, shown.hoses.get(pos).map_or(0.0, |o| *o as f32 / 1000.0));
                }
                if matches!(part, Part::Deployer { .. }) {
                    let stroke = shown.deployers.get(pos).copied().unwrap_or([0, 0]);
                    strokes.insert(*pos, (stroke, shown.held.contains_key(pos)));
                }
                if *part == Part::Arm {
                    let arm = shown.arms.get(pos).cloned().unwrap_or_else(|| arm::Arm::default().snapshot(*pos));
                    arms.push((*pos, speed[0], arm));
                }
                if shown.work.contains_key(pos) {
                    match part {
                        Part::Press(_) => {
                            // With no depot or basin under it, onto what
                            // lies on the ground a block down (as if on a
                            // depot as high as that ground is low).
                            let under = |d: i32| index.parts.get(&[pos[0], pos[1] - d, pos[2]]);
                            let works = |q: Option<&Part>| matches!(q, Some(Part::Depot | Part::Basin));
                            let ground = !(works(under(1)) || (under(1).is_none() && works(under(2))));
                            let drop = if ground {
                                1.0 + if create { 13.0 / 16.0 } else { 9.0 / 16.0 }
                            } else {
                                reach(&index.parts, *pos, Part::Depot)
                            };
                            heads.push((*pos, *part, speed[0], drop, create));
                        }
                        Part::Mixer => {
                            let drop = reach(&index.parts, *pos, Part::Basin);
                            heads.push((*pos, *part, speed[0], drop, create));
                        }
                        _ => {}
                    }
                }
                if matches!(part, Part::Depot | Part::Basin | Part::Drain)
                    && let Some(stacks) = shown.held.get(pos)
                {
                    let stacks = (stacks.iter())
                        .filter(|(item, _)| !fluids::is_fluid(item))
                        .map(|(item, n)| ItemStack::new(item, *n));
                    trays.push((*pos, *part, stacks.collect(), create));
                }
                draws.push((*pos, *part, speed, create));
            }
        });
        for (pos, part, speed, create) in draws {
            let packed = self.world.light(pos[0], pos[1], pos[2]);
            let light = horizon_client::block_entity_render::vertex_light(packed);
            let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
            let origin = (at - camera).as_vec3();
            let large_model = match part {
                Part::LargeWaterWheel(axis) if create => {
                    Some(water_wheel::large_model(axis, |offset| {
                        let p = add(pos, offset);
                        matches!(
                            kinds.part(self.world.block(p[0], p[1], p[2]).0),
                            Some(Part::LargeWaterWheel(_))
                        )
                    }))
                }
                _ => None,
            };
            for (model, toward, factor, half, native) in pieces(part, create) {
                let model = large_model.unwrap_or(model);
                let speed = speed[half] * factor;
                let axis = toward.axis();
                let degrees = kinetic::angle(part, pos, speed, ticks);
                let pose = Mat4::from_translation(origin + Vec3::splat(0.5))
                    * spin(axis, degrees)
                    * point(toward)
                    * point(native).inverse()
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let state = self.tachyon_model(model);
                self.submit_item_world(&state, pose, light, out);
            }
            // BeltRenderer's belt, top and bottom (modelled facing south), and the pulley at an end.
            if create && let Part::Belt(axis) = part {
                let state = self.world.block(pos[0], pos[1], pos[2]);
                let props = self.pack.assets.registry.state(state).map(|s| s.properties.clone()).unwrap_or_default();
                let facing = props.get("facing").and_then(|f| Dir::parse(f)).unwrap_or(Dir::South);
                let belt_part = props.get("part").cloned().unwrap_or_default();
                let turn = match facing {
                    Dir::North => std::f32::consts::PI,
                    Dir::East => std::f32::consts::FRAC_PI_2,
                    Dir::West => -std::f32::consts::FRAC_PI_2,
                    _ => 0.0,
                };
                let pose = Mat4::from_translation(origin + Vec3::splat(0.5))
                    * Mat4::from_rotation_y(turn)
                    * Mat4::from_translation(Vec3::splat(-0.5));
                for model in create_belt::partials(&belt_part) {
                    let state = self.tachyon_model(model);
                    self.submit_item_world(&state, pose, light, out);
                }
                if create_belt::pulley(&belt_part) {
                    let across = if axis == Axis::X { Axis::Z } else { Axis::X };
                    let degrees = kinetic::angle(part, pos, speed[0], ticks);
                    let pose = Mat4::from_translation(origin + Vec3::splat(0.5))
                        * spin(across, degrees)
                        * point(positive(across))
                        * Mat4::from_translation(Vec3::splat(-0.5));
                    let state = self.tachyon_model("create:block/belt_pulley");
                    self.submit_item_world(&state, pose, light, out);
                }
            }
            // FluidValveRenderer's pointer, a quarter turn round while open.
            if create && let Part::Valve { facing, along_first } = part {
                use std::f32::consts::{FRAC_PI_2, PI};
                let pipe = fluids::valve_axis(facing, along_first);
                let shaft = deployer::shaft_axis(facing, along_first);
                let open = self.native_block_property(pos, "enabled").as_deref() == Some("true");
                let offset = if (pipe != Axis::Y && shaft == Axis::X) || pipe == Axis::Y { 90.0 } else { 0.0 };
                let turn = offset + if open { -90.0f32 } else { 0.0 };
                // AngleHelper.horizontalAngle, then up 0, down 180, else 90 about x.
                let yaw = match facing {
                    Dir::North => PI,
                    Dir::East => FRAC_PI_2,
                    Dir::West => -FRAC_PI_2,
                    _ => 0.0,
                };
                let pitch = match facing {
                    Dir::Up => 0.0,
                    Dir::Down => PI,
                    _ => FRAC_PI_2,
                };
                let pose = Mat4::from_translation(origin + Vec3::splat(0.5))
                    * Mat4::from_rotation_y(yaw)
                    * Mat4::from_rotation_x(pitch)
                    * Mat4::from_rotation_y(turn.to_radians())
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let state = self.tachyon_model("create:block/fluid_valve/pointer");
                self.submit_item_world(&state, pose, light, out);
            }
            // AnalogLeverRenderer's handle, a quarter turn at 15, and its indicator.
            if create && part == Part::AnalogLever {
                use std::f32::consts::{FRAC_PI_2, PI};
                let face = self.native_block_property(pos, "face").unwrap_or_default();
                let facing = self.native_block_property(pos, "facing").and_then(|f| Dir::parse(&f)).unwrap_or(Dir::North);
                let power: f32 = self.native_block_property(pos, "power").and_then(|p| p.parse().ok()).unwrap_or(0.0);
                let r_x = match face.as_str() {
                    "floor" => 0.0,
                    "wall" => FRAC_PI_2,
                    _ => PI,
                };
                let r_y = match facing {
                    Dir::North => PI,
                    Dir::East => FRAC_PI_2,
                    Dir::West => -FRAC_PI_2,
                    _ => 0.0,
                };
                let base = Mat4::from_translation(origin + Vec3::splat(0.5))
                    * Mat4::from_rotation_y(r_y)
                    * Mat4::from_rotation_x(r_x)
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let pivot = Vec3::new(0.5, 1.0 / 16.0, 0.5);
                let handle = base
                    * Mat4::from_translation(pivot)
                    * Mat4::from_rotation_x(power / 15.0 * FRAC_PI_2)
                    * Mat4::from_translation(-pivot);
                let state = self.tachyon_model("create:block/analog_lever/handle");
                self.submit_item_world(&state, handle, light, out);
                let state = self.tachyon_model("create:block/analog_lever/indicator");
                self.submit_item_world(&state, base, light, out);
            }
            // HosePulleyRenderer (AbstractPulleyRenderer): the coil, the hose down to its magnet.
            if create && let Part::HosePulley(facing) = part {
                let axis = hose::clockwise(facing).axis();
                let coil = Mat4::from_translation(origin + Vec3::splat(0.5))
                    * point(positive(axis))
                    * point(Dir::South).inverse()
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let state = self.tachyon_model("create:block/hose_pulley/hose_coil");
                self.submit_item_world(&state, coil, light, out);
                let offset = hoses.get(&pos).copied().unwrap_or(0.0);
                let at = |down: f32| Mat4::from_translation(origin - Vec3::new(0.0, down, 0.0));
                let magnet = if offset > 0.25 {
                    "create:block/hose_pulley/pulley_magnet"
                } else {
                    "create:block/hose_pulley/rope_half_magnet"
                };
                let state = self.tachyon_model(magnet);
                self.submit_item_world(&state, at(offset), light, out);
                let f = offset % 1.0;
                if offset > 0.75 && !(0.25..=0.75).contains(&f) {
                    let state = self.tachyon_model("create:block/hose_pulley/rope_half");
                    self.submit_item_world(&state, at(if f > 0.75 { f - 1.0 } else { f }), light, out);
                }
                let rope = self.tachyon_model("create:block/hose_pulley/rope");
                let mut i = 0.0;
                while i < offset - 1.25 {
                    self.submit_item_world(&rope, at(offset - i - 1.0), light, out);
                    i += 1.0;
                }
            }
            // DeployerRenderer's pole and hand, out by its stroke.
            if create && let Part::Deployer { facing, .. } = part {
                let ([state, timer], holding) = strokes.get(&pos).copied().unwrap_or(([0, 0], false));
                let state = match state {
                    1 => deployer::State::Expanding,
                    2 => deployer::State::Retracting,
                    _ => deployer::State::Waiting,
                };
                let out_by = deployer::hand_offset(state, timer, alpha as f32, speed[0], holding);
                let o = facing.offset().map(|c| c as f32 * out_by);
                let pose = Mat4::from_translation(origin + Vec3::splat(0.5) + Vec3::new(o[0], o[1], o[2]))
                    * point(facing)
                    * point(Dir::South).inverse()
                    * Mat4::from_translation(Vec3::splat(-0.5));
                for model in deployer::arm_models(holding) {
                    let state = self.tachyon_model(model);
                    self.submit_item_world(&state, pose, light, out);
                }
            }
            // SawRenderer's blade.
            if create && let Part::Saw { facing, along_first } = part {
                let (model, inner) = saw::blade(facing, along_first, speed[0]);
                let state = self.tachyon_model(model);
                self.submit_item_world(&state, Mat4::from_translation(origin) * inner, light, out);
            }
            // CuckooClockRenderer's hands, doors and figure.
            if create && let Part::Clock(back, mysterious) = part {
                let day = self.world.day_time as f64 + alpha;
                let natural = clock::natural(self.dim.id());
                let turning = speed[0] != 0.0;
                for (model, inner) in
                    clock::parts(pos, back.opposite(), turning, ticks, day, natural, mysterious)
                {
                    let state = self.tachyon_model(model);
                    self.submit_item_world(&state, Mat4::from_translation(origin) * inner, light, out);
                }
            }
            // SpeedControllerRenderer's bracket is stationary, one block
            // up, whenever a horizontal large cogwheel is present.
            if create
                && let Part::SpeedController(axis) = part
                && matches!(
                    kinds.part(self.world.block(pos[0], pos[1] + 1, pos[2]).0),
                    Some(Part::LargeCog(Axis::X | Axis::Z))
                )
            {
                let turn = if axis == Axis::X {
                    std::f32::consts::PI
                } else {
                    std::f32::consts::FRAC_PI_2
                };
                let pose = Mat4::from_translation(origin + Vec3::new(0.5, 1.5, 0.5))
                    * Mat4::from_rotation_y(turn)
                    * Mat4::from_translation(Vec3::splat(-0.5));
                let light = horizon_client::block_entity_render::vertex_light(
                    self.world.light(pos[0], pos[1] + 1, pos[2]),
                );
                let state = self.tachyon_model(speed_controller::BRACKET_MODEL);
                self.submit_item_world(&state, pose, light, out);
            }
        }
        for (pos, speed, arm) in arms {
            let at = DVec3::from_array(pos.map(f64::from));
            let light = horizon_client::block_entity_render::vertex_light(self.world.light(pos[0], pos[1], pos[2]));
            self.tachyon_render_arm((at - camera).as_vec3(), &arm, speed, alpha as f32, light, out);
        }
        // A press at work: its head down onto what lies on the depot under
        // it (`drop` blocks down) and up, by its cycle (`press.rs`). A mixer
        // at work: its whisk down in the basin, turning. Each on a stem up
        // into its block, as long as the head is low.
        for (pos, part, speed, drop, create) in heads {
            let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
            let origin = (at - camera).as_vec3();
            let packed = self.world.light(pos[0], pos[1] - 1, pos[2]);
            let light = horizon_client::block_entity_render::vertex_light(packed);
            let down = CLIENT.with_borrow(|client| client.heads.down(pos, alpha as f32));
            if create {
                self.tachyon_create_head(part, origin, speed, drop, down, ticks, light, out);
                continue;
            }
            // The head's bottom (blocks from the machine's), its height,
            // its turn.
            let (bottom, height, turn, head, stem) = if part == Part::Mixer {
                let degrees = kinetic::angle(part, pos, speed, ticks);
                let turn = Mat4::from_translation(Vec3::new(0.5, 0.0, 0.5))
                    * spin(Axis::Y, degrees)
                    * Mat4::from_translation(Vec3::new(-0.5, 0.0, -0.5));
                // The blades a quarter block over the basin's bottom.
                let whisk = "kinetica:block/mechanical_mixer_whisk";
                (4.0 / 16.0 - drop, 8.0 / 16.0, turn, whisk, "kinetica:block/mechanical_mixer_pole")
            } else {
                // From inside the press to the top of what lies on the tray.
                let rest = 1.0 / 64.0;
                let lowest = 9.0 / 16.0 + 1.0 / 32.0 - drop;
                let bottom = rest + down * (lowest - rest);
                let head = "kinetica:block/mechanical_press_head";
                (bottom, 2.0 / 16.0, Mat4::IDENTITY, head, "kinetica:block/mechanical_press_stem")
            };
            let pose = Mat4::from_translation(origin + Vec3::new(0.0, bottom, 0.0)) * turn;
            let state = self.tachyon_model(head);
            self.submit_item_world(&state, pose, light, out);
            let length = -(bottom + height);
            if length > 0.0 {
                let pose = Mat4::from_translation(origin + Vec3::new(0.0, bottom + height, 0.0))
                    * turn
                    * Mat4::from_scale(Vec3::new(1.0, length, 1.0));
                let state = self.tachyon_model(stem);
                self.submit_item_world(&state, pose, light, out);
            }
        }
        // What a depot holds, lying on its tray: turned face up, its middle
        // over the tray's. What a basin holds, smaller, on its floor, a
        // stack in each quarter.
        const QUARTERS: [(f32, f32); 4] = [(-0.2, -0.2), (0.2, 0.2), (0.2, -0.2), (-0.2, 0.2)];
        for (pos, part, stacks, create) in trays {
            let at = DVec3::new(f64::from(pos[0]), f64::from(pos[1]), f64::from(pos[2]));
            let packed = self.world.light(pos[0], pos[1], pos[2]);
            let light = horizon_client::block_entity_render::vertex_light(packed);
            for (i, stack) in stacks.iter().enumerate().take(QUARTERS.len()) {
                let state = self.resolve_item(
                    stack,
                    horizon_assets::item_model::DisplayContext::Ground,
                    None,
                    0,
                );
                if state.is_empty() {
                    continue;
                }
                let (lo, hi) = state.bounding_box();
                let (spot, scale) = if part == Part::Basin {
                    let (x, z) = QUARTERS[i];
                    (Vec3::new(0.5 + x, 2.0 / 16.0 + i as f32 / 64.0, 0.5 + z), 0.75)
                } else {
                    // Create's depot is 13 high, Kinetica's 9.
                    let top = if create { 13.0 } else { 9.0 } / 16.0;
                    (Vec3::new(0.5, top, 0.5), 1.0)
                };
                // Face up: (x, y, z) to (x, z, -y).
                let lift = Vec3::new(-(lo.x + hi.x) / 2.0, -lo.z, (lo.y + hi.y) / 2.0) * scale;
                let pose = Mat4::from_translation((at - camera).as_vec3() + spot + lift)
                    * Mat4::from_scale(Vec3::splat(scale))
                    * Mat4::from_rotation_x(-std::f32::consts::FRAC_PI_2);
                self.submit_item_world(&state, pose, light, out);
            }
        }
        self.tachyon_chute_items(camera, alpha, out);
    }

    /// A Create press's head at work, down onto what lies on the depot
    /// under it and up (its pole flush with the press's top when up), or a
    /// Create mixer's whisk down in the basin, turning, on its pole: Create's
    /// own models, moved as Kinetica's are.
    #[allow(clippy::too_many_arguments)]
    fn tachyon_create_head(
        &self,
        part: Part,
        origin: Vec3,
        speed: f32,
        drop: f32,
        down: f32,
        ticks: f64,
        light: u32,
        out: &mut Vec<Vertex>,
    ) {
        if part == Part::Mixer {
            let degrees = kinetic::angle(part, [0; 3], speed, ticks);
            let turn = Mat4::from_translation(Vec3::new(0.5, 0.0, 0.5))
                * spin(Axis::Y, degrees)
                * Mat4::from_translation(Vec3::new(-0.5, 0.0, -0.5));
            // The whisk's blades (4 under its origin) a quarter block over
            // the basin's floor; the pole (2 blocks long) no higher than
            // the mixer's top.
            let head = 8.0 / 16.0 - drop;
            let pose = Mat4::from_translation(origin + Vec3::new(0.0, head, 0.0)) * turn;
            let state = self.tachyon_model("create:block/mechanical_mixer/head");
            self.submit_item_world(&state, pose, light, out);
            let pole = Mat4::from_translation(origin + Vec3::new(0.0, head.min(-1.0), 0.0));
            let state = self.tachyon_model("create:block/mechanical_mixer/pole");
            self.submit_item_world(&state, pole, light, out);
            return;
        }
        // Its pole is 27 high over the plate's bottom: up, flush with the
        // press's top; down, on what lies on the depot (13 high).
        let rest = 1.0 - 27.0 / 16.0;
        let lowest = 13.0 / 16.0 + 1.0 / 32.0 - drop;
        let bottom = rest + down * (lowest - rest);
        let pose = Mat4::from_translation(origin + Vec3::new(0.0, bottom, 0.0));
        let state = self.tachyon_model("create:block/mechanical_press/head");
        self.submit_item_world(&state, pose, light, out);
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
                    // An OBJ model of a ported mod's (Create's wheels), else
                    // the item renderer's.
                    let mods = Self::options_path().with_file_name(horizon_client::mod_list::DIR);
                    let baked = (model.starts_with("create:"))
                        .then(|| obj_model::bake(&mods, model))
                        .flatten()
                        .map(Arc::new)
                        .unwrap_or_else(|| self.pack.item_renderer.bake(model));
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
                        cit_glint: None,
                        hdr_tier: 0,
                        hdr_light: 0,
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
                shown.held.get(&hit.pos).cloned(),
                shown.targets.get(&hit.pos).copied(),
            ))
        });
        let Some((part, speed, jammed, stress, work, held, target)) = seen else {
            return;
        };
        let (block, english) = match part {
            Part::Shaft(_) => ("shaft", "Shaft"),
            Part::SpeedGauge(_) => ("speedometer", "Speedometer"),
            Part::StressGauge(_) => ("stressometer", "Stressometer"),
            Part::Cog(_) | Part::EncasedCog(_) => ("cogwheel", "Cogwheel"),
            Part::LargeCog(_) | Part::EncasedLargeCog(_) => ("large_cogwheel", "Large Cogwheel"),
            Part::Gearbox(_) => ("gearbox", "Gearbox"),
            Part::Motor(_) => ("creative_motor", "Creative Motor"),
            Part::Crank(_) => ("hand_crank", "Hand Crank"),
            Part::Clutch(_) => ("clutch", "Clutch"),
            Part::Gearshift(_) => ("gearshift", "Gearshift"),
            Part::WaterWheel(_) => ("water_wheel", "Water Wheel"),
            Part::LargeWaterWheel(_) => ("large_water_wheel", "Large Water Wheel"),
            Part::ChainDrive(_) => ("encased_chain_drive", "Encased Chain Drive"),
            Part::Flywheel(_) => ("flywheel", "Flywheel"),
            Part::Clock(_, false) => ("cuckoo_clock", "Cuckoo Clock"),
            Part::Clock(_, true) => ("mysterious_cuckoo_clock", "Cuckoo Clock"),
            Part::ChainGearshift(_) => ("adjustable_chain_gearshift", "Adjustable Chain Gearshift"),
            Part::Windmill(_) => ("windmill", "Windmill"),
            Part::Millstone => ("millstone", "Millstone"),
            Part::Saw { .. } => ("mechanical_saw", "Mechanical Saw"),
            Part::Drill(_) => ("mechanical_drill", "Mechanical Drill"),
            Part::Deployer { .. } => ("deployer", "Deployer"),
            Part::Pipe(_) => ("fluid_pipe", "Fluid Pipe"),
            Part::Pump(_) => ("mechanical_pump", "Mechanical Pump"),
            Part::Tank => ("fluid_tank", "Fluid Tank"),
            Part::Spout => ("spout", "Spout"),
            Part::Drain => ("item_drain", "Item Drain"),
            Part::SteamEngine(_) => ("steam_engine", "Steam Engine"),
            Part::PoweredShaft(_) => ("shaft", "Shaft"),
            Part::Valve { .. } => ("fluid_valve", "Fluid Valve"),
            Part::SmartPipe(_) => ("smart_fluid_pipe", "Smart Fluid Pipe"),
            Part::HosePulley(_) => ("hose_pulley", "Hose Pulley"),
            Part::Vault(_) => ("item_vault", "Item Vault"),
            Part::AnalogLever => ("analog_lever", "Analog Lever"),
            Part::Latch(_) => ("powered_latch", "Powered Latch"),
            Part::ToggleLatch(_) => ("powered_toggle_latch", "Powered Toggle Latch"),
            Part::Press(_) => ("mechanical_press", "Mechanical Press"),
            Part::Depot => ("depot", "Depot"),
            Part::Mixer => ("mechanical_mixer", "Mechanical Mixer"),
            Part::Arm => ("mechanical_arm", "Mechanical Arm"),
            Part::Basin => ("basin", "Basin"),
            Part::Burner => ("blaze_burner", "Blaze Burner"),
            Part::LitBurner => ("lit_blaze_burner", "Lit Blaze Burner"),
            Part::Fan(_) => ("encased_fan", "Encased Fan"),
            Part::Belt(_) => ("belt", "Mechanical Belt"),
            Part::SpeedController(_) => ("rotation_speed_controller", "Rotation Speed Controller"),
            Part::Funnel(_) => ("funnel", "Funnel"),
            Part::CrushingWheel(_) => ("crushing_wheel", "Crushing Wheel"),
            Part::Chute(_, _) => ("chute", "Chute"),
        };
        // Create's blocks by Create's names.
        let state = self.world.block(hit.pos[0], hit.pos[1], hit.pos[2]);
        let name = self.pack.assets.registry.state(state).map(|s| s.name.clone());
        let what = match name.as_deref().and_then(|n| n.strip_prefix("create:")) {
            Some(create) => self.translate(&format!("block.create.{create}"), english),
            None => self.translate(&format!("block.kinetica.{block}"), english),
        };
        let mut state = if let Some(gauge) = gauge_readout(part, speed, jammed, stress, |k, fallback| {
            self.translate(k, fallback)
        }) {
            gauge
        } else {
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
            // Other parts show the network's load, not its remaining capacity.
        if matches!(part, Part::SpeedController(_)) {
            let target = target.unwrap_or(speed_controller::DEFAULT_SPEED);
            let text = self.translate("kinetica.hud.target", "target %s RPM")
                .replace("%s", &target.to_string());
            state = format!("{state} | {text}");
        }
            if let Some(s) = stress {
                let su = self
                    .translate("kinetica.hud.stress", "%s / %s SU")
                    .replacen("%s", &(s.load.round() as i64).to_string(), 1)
                    .replacen("%s", &(s.capacity.round() as i64).to_string(), 1);
                state = format!("{state} | {su}");
            }
            state
        };
        // A funnel: whether it puts in or takes out, only.
        if let (Part::Funnel(_), Some(w)) = (part, &work) {
            state = self.translate(&format!("kinetica.hud.{w}"), w);
        } else if let Some(item) = work {
            // A fan: what its air does ("pulling, washing").
            let what = if matches!(part, Part::Fan(_)) {
                (item.split(':'))
                    .map(|w| self.translate(&format!("kinetica.hud.{w}"), w))
                    .collect::<Vec<_>>()
                    .join(", ")
            } else {
                self.tachyon_item_name(&item)
            };
            state = format!("{state} | {what}");
        }
        // A depot, a basin, a chute: what it holds.
        if matches!(
            part,
            Part::Depot
                | Part::Basin
                | Part::Chute(_, _)
                | Part::Tank
                | Part::Spout
                | Part::Drain
                | Part::Deployer { .. }
                | Part::Vault(_)
                | Part::HosePulley(_)
        ) {
            let names: Vec<String> = (held.unwrap_or_default().iter())
                .map(|(item, n)| {
                    if fluids::is_fluid(item) {
                        format!("{} {n} mB", fluids::fluid_name(item))
                    } else {
                        format!("{} x{n}", self.tachyon_item_name(item))
                    }
                })
                .collect();
            state = if names.is_empty() {
                "-".to_owned()
            } else {
                names.join(", ")
            };
        }
        // A burner shows its heat rather than a speed; a basin shows the
        // heat of the block under it beside what it holds.
        let heat_at = match part {
            Part::Burner | Part::LitBurner => Some(hit.pos),
            Part::Basin => Some([hit.pos[0], hit.pos[1] - 1, hit.pos[2]]),
            _ => None,
        };
        if let Some(pos) = heat_at {
            let heat = self.tachyon_heat(pos).name();
            let heat = self.translate(&format!("kinetica.hud.heat.{heat}"), heat);
            state = if part == Part::Basin {
                format!("{state} | {heat}")
            } else {
                heat
            };
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

    fn english(_key: &str, fallback: &str) -> String {
        fallback.to_owned()
    }

    #[test]
    fn speeds_go_and_come_back() {
        let mut arm = arm::Arm::default();
        let mut stack = ItemStack { damage: 7, ..ItemStack::new("minecraft:iron_pickaxe", 1) };
        stack.enchant("minecraft:unbreaking", 2);
        stack.data_mut().custom_name = Some(horizon_world::nbt::Tag::String("Held pickaxe".into()));
        arm.held = Some(stack);
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
            held: HashMap::from([(
                [3, 64, 2],
                vec![("kinetica:iron_sheet".to_owned(), 5), ("x:y".to_owned(), 64)],
            )]),
            crushers: HashMap::from([
                ([1, -20, 3], "minecraft:cobblestone".to_owned()),
                ([-5, 70, 9], "minecraft:raw_iron".to_owned()),
            ]),
            arms: HashMap::from([([8, 64, 2], arm.snapshot([8, 64, 2]))]),
            deployers: HashMap::from([([9, 64, 2], [1, 500])]),
            hoses: HashMap::from([([10, 64, 2], 3500)]),
            chutes: HashMap::from([
                ([3, 64, 2], [0.8, 0.6, 0.0]),
                ([-3, 65, 2], [-0.5, -0.3, 3.0]),
            ]),
            targets: HashMap::from([([4, 64, 2], -64), ([5, 64, 2], 0), ([6, 64, 2], 256)]),
        };
        let data = encode(&shown);
        assert_eq!(data[0], VERSION);
        assert_eq!(decode(&data), Some(shown.clone()));
        assert_eq!(decode(&encode(&Shown::default())), Some(Shown::default()));
        assert!(decode(&[1]).is_none());
        for end in 0..data.len() {
            assert!(decode(&data[..end]).is_none(), "truncated at {end}");
        }
        let mut old = data;
        old[0] = VERSION - 1;
        assert!(decode(&old).is_none());
    }

    #[test]
    fn overstressed_stress_goes_and_comes_back_without_speed() {
        let pos = [2, 64, -3];
        let shown = Shown {
            stress: HashMap::from([(pos, kinetic::Stress { load: 384.0, capacity: 256.0 })]),
            ..Shown::default()
        };
        let data = encode(&shown);
        assert_eq!(data[0], VERSION);
        let back = decode(&data).unwrap();
        assert_eq!(back, shown);
        assert!(!back.speeds.contains_key(&pos));
        assert_eq!(
            gauge_readout(
                Part::StressGauge(Axis::Y),
                back.speeds.get(&pos).map_or(0.0, |s| s[0]),
                back.jammed.contains(&pos),
                back.stress.get(&pos).copied(),
                english,
            )
            .as_deref(),
            Some("150% | Remaining Capacity: -128 / 256 SU | overstressed")
        );
    }

    #[test]
    fn malformed_arm_snapshots_and_lengths_do_not_read() {
        let shown = Shown {
            arms: HashMap::from([([0; 3], arm::Arm::default().snapshot([0; 3]))]),
            ..Shown::default()
        };
        let mut data = encode(&shown);
        *data.last_mut().unwrap() = b'!';
        assert!(decode(&data).is_none());
        let mut data = encode(&Shown::default());
        let count = data.len() - 4;
        data[count..].copy_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&[0; 12]);
        data.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(decode(&data).is_none());
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
        assert_eq!(part_of("kinetica:depot", facing), Some(Part::Depot));
        assert_eq!(part_of("kinetica:basin", facing), Some(Part::Basin));
        assert_eq!(part_of("kinetica:mechanical_mixer", facing), Some(Part::Mixer));
        assert_eq!(
            part_of("kinetica:encased_fan", props("x")),
            Some(Part::Fan(Axis::X))
        );
        assert_eq!(part_of("kinetica:belt", facing), Some(Part::Belt(Axis::Z)));
        // Create's, by the properties of the templates create_port.rs gives.
        assert_eq!(part_of("create:shaft", props("y")), Some(Part::Shaft(Axis::Y)));
        assert_eq!(part_of("create:creative_motor", facing), Some(Part::Motor(Dir::North)));
        assert_eq!(part_of("create:hand_crank", facing), Some(Part::Crank(Dir::South)));
        assert_eq!(part_of("create:mechanical_press", facing), Some(Part::Press(Axis::Z)));
        assert_eq!(part_of("create:mechanical_arm", |_| None), Some(Part::Arm));
        assert_eq!(part_of("create:andesite_casing", facing), None);
        let chute = |k: &str| match k {
            "facing" => Some("east"),
            "half" => Some("bottom"),
            "shape" => Some("inner_left"),
            _ => None,
        };
        assert_eq!(part_of("create:chute", chute), Some(Part::Chute(Dir::Down, ChuteShape::Window)));
        assert_eq!(part_of("create:smart_chute", chute), None);
        assert!(pieces(Part::Chute(Dir::Down, ChuteShape::Normal), true).is_empty());
        assert!(Dir::ALL.into_iter().all(|d| !Part::Chute(Dir::Down, ChuteShape::Normal).shaft_toward(d)));
        assert_eq!(
            part_of("kinetica:funnel", facing),
            Some(Part::Funnel(Dir::North))
        );
        assert_eq!(
            part_of("kinetica:mechanical_press", props("z")),
            Some(Part::Press(Axis::Z))
        );
    }

    #[test]
    fn a_gearbox_shows_four_turning_shafts_a_shaft_one() {
        assert_eq!(pieces(Part::Gearbox(Axis::Y), false).len(), 4);
        assert_eq!(pieces(Part::Shaft(Axis::Z), false).len(), 1);
        assert_eq!(pieces(Part::Motor(Dir::East), false).len(), 1);
        // Create's gearbox: four half shafts pointing south in the model.
        let create = pieces(Part::Gearbox(Axis::Y), true);
        assert_eq!(create.len(), 4);
        assert!(create.iter().all(|p| p.0 == "create:block/shaft_half" && p.4 == Dir::South));
        assert_eq!(pieces(Part::Arm, true), vec![("create:block/mechanical_arm/cog", Dir::Up, 1.0, 0, Dir::Up)]);
        let halves: Vec<usize> = pieces(Part::Clutch(Axis::X), false).iter().map(|p| p.3).collect();
        assert_eq!(halves, [0, 1]);
    }

    #[test]
    fn a_large_water_wheel_uses_its_axis_and_creates_model() {
        for (axis, name) in [(Axis::X, "x"), (Axis::Y, "y"), (Axis::Z, "z")] {
            let props = |k: &str| (k == "axis").then_some(name);
            let part = Part::LargeWaterWheel(axis);
            assert_eq!(part_of("create:large_water_wheel", props), Some(part));
            assert_eq!(
                pieces(part, true),
                vec![(water_wheel::LARGE_MODEL, positive(axis), 1.0, 0, Dir::Up)]
            );
            assert!(pieces(part, false).is_empty());
        }
        let facing = |k: &str| (k == "facing").then_some("north");
        assert_eq!(part_of("create:large_water_wheel", facing), None);
        assert_eq!(part_of("create:large_water_wheel", |_| None), None);
    }

    #[test]
    fn encased_cogwheels_use_their_axis_and_shaftless_create_models() {
        for (axis, name) in [(Axis::X, "x"), (Axis::Y, "y"), (Axis::Z, "z")] {
            let props = |k: &str| (k == "axis").then_some(name);
            for (id, part, model) in [
                (
                    "create:andesite_encased_cogwheel",
                    Part::EncasedCog(axis),
                    "create:block/cogwheel_shaftless",
                ),
                (
                    "create:brass_encased_cogwheel",
                    Part::EncasedCog(axis),
                    "create:block/cogwheel_shaftless",
                ),
                (
                    "create:andesite_encased_large_cogwheel",
                    Part::EncasedLargeCog(axis),
                    "create:block/large_cogwheel_shaftless",
                ),
                (
                    "create:brass_encased_large_cogwheel",
                    Part::EncasedLargeCog(axis),
                    "create:block/large_cogwheel_shaftless",
                ),
            ] {
                assert_eq!(part_of(id, props), Some(part));
                assert_eq!(pieces(part, true), vec![(model, positive(axis), 1.0, 0, Dir::Up)]);
                assert!(pieces(part, false).is_empty());
                assert_eq!(part_of(id, |_| None), None);
                for invalid in ["", "north", "Y"] {
                    assert_eq!(part_of(id, |k| (k == "axis").then_some(invalid)), None);
                }
                assert_eq!(part_of(id, |k| (k == "facing").then_some("north")), None);
            }
        }
    }

    #[test]
    fn create_gauges_use_their_axes_and_creates_shaft() {
        for (axis, name) in [(Axis::X, "x"), (Axis::Y, "y"), (Axis::Z, "z")] {
            let props = |k: &str| (k == "axis").then_some(name);
            for (id, part) in [
                ("create:speedometer", Part::SpeedGauge(axis)),
                ("create:stressometer", Part::StressGauge(axis)),
            ] {
                assert_eq!(part_of(id, props), Some(part));
                assert_eq!(
                    pieces(part, true),
                    vec![("create:block/shaft", positive(axis), 1.0, 0, Dir::Up)]
                );
                assert!(pieces(part, false).is_empty());
                assert!(!takes_use(part));
            }
        }
        for block in ["speedometer", "stressometer"] {
            let id = format!("create:{block}");
            assert_eq!(part_of(&id, |_| None), None);
            assert_eq!(part_of(&id, |k| (k == "facing").then_some("north")), None);
            for invalid in ["", "north", "w"] {
                assert_eq!(part_of(&id, |k| (k == "axis").then_some(invalid)), None);
            }
            assert_eq!(
                part_of(&format!("kinetica:{block}"), |k| (k == "axis").then_some("x")),
                None
            );
        }
    }

    #[test]
    fn a_speedometer_shows_absolute_rpm_even_when_idle() {
        let part = Part::SpeedGauge(Axis::X);
        for speed in [-32.0, 32.0] {
            assert_eq!(
                gauge_readout(part, speed, false, None, english).as_deref(),
                Some("32 RPM")
            );
        }
        assert_eq!(
            gauge_readout(part, 0.0, false, None, english).as_deref(),
            Some("0 RPM")
        );
        let stress = kinetic::Stress { load: 128.0, capacity: 512.0 };
        assert_eq!(
            gauge_readout(part, -32.0, false, Some(stress), english).as_deref(),
            Some("32 RPM")
        );
        let over = kinetic::Stress { load: 384.0, capacity: 256.0 };
        assert_eq!(
            gauge_readout(part, 0.0, false, Some(over), english).as_deref(),
            Some("overstressed")
        );
    }

    #[test]
    fn a_stressometer_shows_utilization_and_remaining_capacity() {
        let part = Part::StressGauge(Axis::Z);
        for (load, capacity, speed, expected) in [
            (128.0, 512.0, 32.0, "25% | Remaining Capacity: 384 / 512 SU"),
            (129.0, 512.0, -32.0, "25% | Remaining Capacity: 383 / 512 SU"),
            (384.0, 256.0, 0.0, "150% | Remaining Capacity: -128 / 256 SU | overstressed"),
            (16.0, 0.0, 0.0, "1600% | Remaining Capacity: -16 / 0 SU | overstressed"),
            (0.0, 512.0, 0.0, "0% | Remaining Capacity: 512 / 512 SU"),
            (0.0, 0.0, 32.0, "0% | Remaining Capacity: 0 / 0 SU"),
        ] {
            let stress = Some(kinetic::Stress { load, capacity });
            assert_eq!(
                gauge_readout(part, speed, false, stress, english).as_deref(),
                Some(expected)
            );
        }
        for speed in [0.0, 32.0] {
            assert_eq!(
                gauge_readout(part, speed, false, None, english).as_deref(),
                Some("No Rotation")
            );
        }
        assert_eq!(
            gauge_readout(part, 0.0, false, Some(kinetic::Stress::default()), english).as_deref(),
            Some("No Rotation")
        );
    }

    #[test]
    fn gauges_keep_jammed_precedence_and_use_translations() {
        let over = Some(kinetic::Stress { load: 384.0, capacity: 256.0 });
        let text = |key: &str, fallback: &str| match key {
            "kinetica.hud.jammed" => "macet".to_owned(),
            "kinetica.hud.overstressed" => "kelebihan beban".to_owned(),
            "kinetica.hud.rpm" => "%s putaran".to_owned(),
            "kinetica.hud.stress" => "%s / %s beban".to_owned(),
            "create.gui.stressometer.capacity" => "Sisa kapasitas".to_owned(),
            "create.gui.stressometer.no_rotation" => "Tanpa putaran".to_owned(),
            _ => fallback.to_owned(),
        };
        for part in [Part::SpeedGauge(Axis::X), Part::StressGauge(Axis::Y)] {
            assert_eq!(
                gauge_readout(part, 0.0, true, over, english).as_deref(),
                Some("jammed")
            );
            assert_eq!(
                gauge_readout(part, 0.0, true, over, text).as_deref(),
                Some("macet")
            );
        }
        assert_eq!(gauge_readout(Part::Shaft(Axis::X), 32.0, true, over, text), None);
        assert_eq!(
            gauge_readout(Part::SpeedGauge(Axis::X), -32.0, false, None, text).as_deref(),
            Some("32 putaran")
        );
        assert_eq!(
            gauge_readout(Part::StressGauge(Axis::Y), 0.0, false, None, text).as_deref(),
            Some("Tanpa putaran")
        );
        assert_eq!(
            gauge_readout(Part::StressGauge(Axis::Y), 0.0, false, over, text).as_deref(),
            Some("150% | Sisa kapasitas: -128 / 256 beban | kelebihan beban")
        );
    }

    #[test]
    fn create_and_kinetica_goggles_are_not_ordinary_helmets() {
        assert!(is_goggles_item("create:goggles"));
        assert!(is_goggles_item("kinetica:engineers_goggles"));
        for item in [
            "minecraft:leather_helmet", "minecraft:iron_helmet", "minecraft:stick", "create:shaft",
        ] {
            assert!(!is_goggles_item(item));
        }
    }

    #[test]
    fn a_controller_uses_a_horizontal_template_and_creates_shaft() {
        for (facing, axis) in [("north", Axis::X), ("south", Axis::X), ("west", Axis::Z), ("east", Axis::Z)] {
            let props = |k: &str| (k == "facing").then_some(facing);
            let part = Part::SpeedController(axis);
            assert_eq!(part_of("create:rotation_speed_controller", props), Some(part));
            assert_eq!(pieces(part, true), vec![("create:block/shaft", positive(axis), 1.0, 0, Dir::Up)]);
            assert!(pieces(part, false).is_empty());
            assert!(!takes_use(part));
        }
        assert_eq!(part_of("create:rotation_speed_controller", |k| (k == "facing").then_some("up")), None);
        assert_eq!(part_of("create:rotation_speed_controller", |_| None), None);
    }

    #[test]
    fn controller_targets_survive_unloading_but_not_breaking_or_replacement() {
        let p = [1, 64, 3];
        let mut owner = Owner::default();
        owner.index.parts.insert(p, Part::SpeedController(Axis::X));
        assert!(owner.sync_controllers(&[]));
        assert_eq!(owner.controllers[&p], speed_controller::DEFAULT_SPEED);
        owner.controllers.insert(p, -64);
        owner.index.parts.clear();
        assert!(!owner.sync_controllers(&[]));
        assert_eq!(owner.controllers[&p], -64);
        // The same controller comes back, even if its facing changed.
        owner.index.known.insert((0, 0));
        owner.index.parts.insert(p, Part::SpeedController(Axis::Z));
        assert!(!owner.sync_controllers(&[]));
        assert_eq!(owner.controllers[&p], -64);
        owner.index.parts.insert(p, Part::Shaft(Axis::X));
        assert!(owner.sync_controllers(&[]));
        assert!(!owner.controllers.contains_key(&p));
        owner.index.parts.insert(p, Part::SpeedController(Axis::X));
        assert!(owner.sync_controllers(&[]));
        assert_eq!(owner.controllers[&p], speed_controller::DEFAULT_SPEED);
        owner.index.parts.remove(&p);
        assert!(owner.sync_controllers(&[p]));
        assert!(!owner.controllers.contains_key(&p));
        // A loaded chunk no longer has the saved controller in it.
        owner.controllers.insert(p, -64);
        assert!(owner.sync_controllers(&[]));
        assert!(!owner.controllers.contains_key(&p));
    }

    #[test]
    fn controller_targets_are_saved_per_block_and_old_files_get_the_default() {
        let root = std::env::temp_dir().join(format!("tachyon-controller-save-{}", std::process::id()));
        let dimension = "minecraft:overworld";
        let (a, b, c) = ([1, 64, 3], [-5, -20, 9], [9, 64, 9]);
        let mut owner = Owner::default();
        owner.motors.insert([0, 64, 0], 128.0);
        owner.controllers.extend([(a, -64), (b, 0), (c, 1000)]);
        owner.save(&root, dimension);
        let mut loaded = Owner::default();
        loaded.load(&root, dimension);
        assert_eq!(loaded.controllers, HashMap::from([(a, -64), (b, 0), (c, 256)]));
        assert_eq!(loaded.motors, owner.motors);
        let legacy_dimension = "minecraft:the_nether";
        std::fs::write(Owner::file(&root, legacy_dimension), r#"{"motors":[[0,64,0,32]]}"#).unwrap();
        let mut legacy = Owner::default();
        legacy.load(&root, legacy_dimension);
        assert!(legacy.controllers.is_empty());
        legacy.index.parts.insert(a, Part::SpeedController(Axis::X));
        assert!(legacy.sync_controllers(&[]));
        assert_eq!(legacy.controllers[&a], speed_controller::DEFAULT_SPEED);
        assert_eq!(legacy.motors[&[0, 64, 0]], 32.0);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn blaze_burners_take_uses_but_never_join_or_draw_a_turning_part() {
        for (name, part) in [
            ("create:blaze_burner", Part::Burner),
            ("create:lit_blaze_burner", Part::LitBurner),
        ] {
            assert_eq!(part_of(name, |_| None), Some(part));
            assert!(takes_use(part));
            assert!(pieces(part, true).is_empty());
            assert!(pieces(part, false).is_empty());
            for d in Dir::ALL {
                assert!(!part.shaft_toward(d));
                assert!(kinetic::link(Part::Shaft(d.axis()), part, d.offset()).is_none());
            }
            assert_eq!(part.impact(), 0.0);
        }
        assert_eq!(part_of("minecraft:respawn_anchor", |_| None), None);
    }

    #[test]
    fn a_press_works_the_depot_under_it_or_one_lower() {
        let press = |q: &Part| matches!(q, Part::Press(_));
        let p = HashMap::from([([0, 1, 0], Part::Press(Axis::X)), ([5, 2, 0], Part::Press(Axis::X))]);
        assert_eq!(worker(&p, [0, 0, 0], press), Some([0, 1, 0]));
        assert_eq!(worker(&p, [5, 0, 0], press), Some([5, 2, 0]));
        assert_eq!(reach(&p, [0, 1, 0], Part::Depot), 2.0);
        // A part between: not that one.
        let p = HashMap::from([([0, 2, 0], Part::Press(Axis::X)), ([0, 1, 0], Part::Shaft(Axis::X))]);
        assert_eq!(worker(&p, [0, 0, 0], press), None);
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
