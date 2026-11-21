// megamap: a HUD minimap and a fullscreen world map with waypoints, for Horizon as a native mixin mod.
//
// This file is the glue. tools/pack.py inlines `src/engine/*` into it and writes the result as the bundle's
// `runtime.rs`, which horizon-mixins adds to `crates/horizon-client/src/game.rs` as `mod native_megamap`
// (horizon-mixin.toml, [[module]]). The bundle's small helpers (frame.rs, hud.rs, ...) are spliced into
// Horizon's hook points (native_hooks.rs) and call the `pub(super)` entry points at the end of this file.
// The game reads go through `Host`, which `Game` implements (game/mod_api_host.rs), so this code is the same
// as when megamap was a trait mod. Everything that does not touch Horizon lives in `engine` and is unit-tested
// by the `engine-tests` crate in the work folder.

use megamap_engine as engine;

use engine::baker::{bake_chunk, BakedChunk, ChunkSource, ColorTable, Layer, Tints};
use engine::color;
use engine::detail::Detail;
use engine::clouds::{self, Clouds};
use engine::entities;
use engine::far::{self, Spiral, TintTable};
use engine::hlod::{self, HlodRegion};
use engine::radar::Radar;
use engine::region::Region;
use engine::reveal;
use engine::settings::{self, Info, Mode, Settings, Shape};
use engine::store::{self, Store};
use engine::style;
use engine::trail::Trail;
use engine::autozoom::{self, AutoZoom};
use engine::icons;
use engine::infoblock;
use engine::lang::{self, Lang, Tr};
use engine::layers::{self, LayerMode};
use engine::palette;
use engine::preview;
use engine::portrait;
use engine::ui::{self, Action, MapUi, Rect, BUTTONS, EDIT_BUTTONS};
use engine::view::{self, Lookup, View};
use engine::waypoints::{self, Waypoint, Waypoints, ICONS};
use super::megamap_world::{
    MegamapHost as Host, NativeBiomeId, NativeClientBiomes, NativeClientChunks, NativeScreenKind,
    NativeWorldId,
};
use horizon_client::{
    biome::BiomeColors,
    extras::Corner as HCorner,
    extras_hud::Corners,
    hud::HudBatch,
    mod_list::{Installed, KeyConflictContext, KeyMapping, NativeSetting, NativeSettingValue},
    text::Text,
    world_primitives::WorldPrimitives,
};
use horizon_world::registry::BlockStateId;
use mod_api::ScreenView as NativeScreenView;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
use std::time::{Duration, Instant};

pub(super) const ID: &str = "megamap";
/// The mod's version (tools/pack.py checks it against horizon-native.json).
const VERSION: &str = "0.3.11";
/// Horizon's one shared mod image slot (`horizon-client-render/src/hud.rs`: `MOD_IMAGE_SPRITE`, 1024 square).
const MOD_IMAGE: &str = "horizon/modimage";
const MOD_IMAGE_SIZE: usize = 1024;

/// Surface snapshot radius in chunks (65 x 65 chunks, far below the 66,049 limit) and the cave layer's.
const SURFACE_RADIUS: i32 = 32;
const CAVE_RADIUS: i32 = 8;
/// How long the cave slice bakes after a rebuild before new ground in it counts as news (`State::step_fronts`).
const CAVE_SETTLE: Duration = Duration::from_secs(2);
/// Regions kept in memory per layer (about 1.9 MB each with mips).
const REGION_LIMIT: usize = 48;
const BAKE_BATCH: usize = 512;
const INTEGRATE_PER_FRAME: usize = 64;

/// The one megamap of the process. Hook points run on the game thread; the mutex is for the borrow checker
/// and for `Sync`, it is never contended.
static STATE: OnceLock<Mutex<State>> = OnceLock::new();

fn lock() -> MutexGuard<'static, State> {
    // A panic in one hook must not take the map down for the rest of the session.
    STATE.get_or_init(|| Mutex::new(State::new())).lock().unwrap_or_else(|e| e.into_inner())
}

/// Far terrain: chunks around the player, in chunks, that the far filler bakes from Horizon's level-of-detail
/// data (its disk cache, its source, or the live tiles), and the far regions kept in memory.
const FAR_RADIUS: i32 = 48;
/// The farthest the far thread fills around the open map's centre (chunks), and the generator's coarse pass.
const FAR_MAX_RADIUS: i32 = 160;
const FAR_COARSE_RADIUS: i32 = 64;
/// The coarse pass takes one generator column every `FAR_COARSE_STRIDE` blocks (4 a chunk).
const FAR_COARSE_STRIDE: i32 = 8;
const FAR_REGION_LIMIT: usize = 48;
/// A far chunk its source listed but that baked to nothing is planned again after this (times the tries) ...
const FAR_RETRY_AFTER: Duration = Duration::from_secs(3);
/// ... up to this many times.
const FAR_RETRY_TRIES: u8 = 3;
/// The live LOD tiles are sampled every `FAR_STRIDE` blocks on the game thread, within this budget a frame.
const FAR_STRIDE: i32 = 2;
/// How often the layer probe runs (sky light, roof check, room check).
const PROBE_EVERY: Duration = Duration::from_millis(150);
const FAR_LIVE_BUDGET: Duration = Duration::from_micros(1500);
/// A pass over the live tiles that baked nothing is followed by this pause: the passes ran back to back and
/// took their whole budget from every frame for as long as any chunk around the player had no tile (always).
const FAR_LIVE_IDLE: Duration = Duration::from_millis(1000);
/// Far batches taken a frame, within this budget.
/// Far tasks handed to the readers at once (between the checks for a moved view).
const FAR_BLOCK: usize = 96;
const FAR_RECEIVE_BUDGET: Duration = Duration::from_millis(3);

// ---------------------------------------------------------------------------------------------------------------
// Adapters from Horizon's snapshots to the engine's traits (worker side).

struct Chunks<'a>(&'a NativeClientChunks);

impl ChunkSource for Chunks<'_> {
    #[inline]
    fn block(&self, pos: [i32; 3]) -> u32 {
        self.0.block(pos).0
    }
    #[inline]
    fn biome(&self, pos: [i32; 3]) -> Option<u16> {
        self.0.biome(pos).map(|b| b.0)
    }
    fn section_top(&self, xz: [i32; 2]) -> i32 {
        self.0.section_top(xz)
    }
    fn loaded(&self, xz: [i32; 2]) -> bool {
        self.0.loaded(xz)
    }
    fn min_y(&self) -> i32 {
        self.0.height().start
    }
}

struct BiomeTints(Option<NativeClientBiomes>);

impl Tints for BiomeTints {
    #[inline]
    fn tint(&self, biome: u16, x: i32, z: i32) -> (u32, u32, u32) {
        match &self.0 {
            Some(b) => {
                let c: BiomeColors = b.colors(NativeBiomeId(biome), [x, z]);
                (c.grass & 0xFF_FFFF, c.foliage & 0xFF_FFFF, c.water & 0xFF_FFFF)
            }
            // Plains-like defaults when the game gave no table.
            None => (0x91BD59, 0x77AB2F, 0x3F76E4),
        }
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The worker: bakes, encodes, writes and reads region files, never touching the game.

enum Job {
    Bake {
        epoch: u64,
        cave: bool,
        chunks: NativeClientChunks,
        biomes: Option<NativeClientBiomes>,
        table: Arc<ColorTable>,
        list: Vec<[i32; 2]>,
        layer: Layer,
    },
    Save {
        dir: PathBuf,
        regions: Vec<Region>,
    },
    Load {
        epoch: u64,
        layer: u8,
        dir: PathBuf,
        key: (i32, i32),
    },
}

enum Out {
    Baked { epoch: u64, cave: bool, chunks: Vec<BakedChunk>, attempted: Vec<[i32; 2]> },
    /// `failed`: the file is there but could not be read (tried again later, `Store::load_failed`).
    Loaded { epoch: u64, layer: u8, key: (i32, i32), region: Option<Region>, failed: bool },
}

struct Worker {
    tx: Sender<Job>,
    rx: Receiver<Out>,
}

fn spawn_worker(gens: [Arc<AtomicU64>; 2]) -> Option<Worker> {
    let (tx, jobs) = channel::<Job>();
    let (out, rx) = channel::<Out>();
    std::thread::Builder::new()
        .name("megamap-baker".into())
        .spawn(move || {
            for job in jobs {
                match job {
                    Job::Bake { epoch, cave, chunks, biomes, table, list, layer } => {
                        let gen_now = &gens[cave as usize];
                        let src = Chunks(&chunks);
                        let tints = BiomeTints(biomes);
                        let mut done = Vec::with_capacity(32);
                        let mut attempted = Vec::with_capacity(32);
                        for [cx, cz] in list {
                            if gen_now.load(Ordering::Relaxed) != epoch {
                                break; // the world, dimension or cave slice changed: the rest is stale
                            }
                            attempted.push([cx, cz]);
                            if let Some(c) = bake_chunk(&src, &tints, &table, cx, cz, layer) {
                                done.push(c);
                            }
                            if attempted.len() == 32 {
                                let chunks = std::mem::replace(&mut done, Vec::with_capacity(32));
                                let att = std::mem::replace(&mut attempted, Vec::with_capacity(32));
                                if out.send(Out::Baked { epoch, cave, chunks, attempted: att }).is_err() {
                                    return;
                                }
                            }
                        }
                        if !attempted.is_empty() && out.send(Out::Baked { epoch, cave, chunks: done, attempted }).is_err() {
                            return;
                        }
                    }
                    Job::Save { dir, regions } => {
                        for r in regions {
                            let _ = store::write_region(&dir, r.rx, r.rz, &r.encode());
                        }
                    }
                    Job::Load { epoch, layer, dir, key } => {
                        let (region, failed) = match store::read_region(&dir, key.0, key.1) {
                            // The mips here, not on the game thread (about 1 ms a region: a far-zoomed pan
                            // takes in a dozen a frame).
                            Ok(r) => (r.map(|mut r| { r.ensure_mips(); r }), false),
                            Err(e) => {
                                log::debug!("megamap: region {key:?} read failed: {e}");
                                (None, true)
                            }
                        };
                        if out.send(Out::Loaded { epoch, layer, key, region, failed }).is_err() {
                            return;
                        }
                    }
                }
            }
        })
        .ok()?;
    Some(Worker { tx, rx })
}

// ---------------------------------------------------------------------------------------------------------------
// Far terrain: a thread that bakes chunks past the live ones from what Horizon keeps on disk, whether or not the
// game draws far terrain, nearest the view first. Work is planned from region headers alone (no chunk is read
// to find out it is missing):
//   1. the far terrain disk cache (`.hlod`, read only), single player;
//   2. the server-terrain folder (`r.X.Z.mca`, Horizon's own records, through `ServerTerrain::chunk`, read only);
//   3. the save's region files (`region/r.X.Z.mca`), single player, read through the level's LOD source;
//   4. single player only, while the game draws no far terrain of its own: a coarse pass of the generator
//      (`LodSampler::grid`, one column every `FAR_COARSE_STRIDE` blocks) for the rest.
// Chunks explored on the map already (surface `.mmr`) or baked into the saved far layer are skipped. The game
// thread adds the live LOD tiles (`Game::lod`) on a time budget, and saves the far layer next to the map.

/// What the game thread tells the far thread.
struct FarShared {
    /// Bumped on every world or dimension change: a thread with an older epoch ends.
    epoch: AtomicU64,
    /// Where to fill first (the open map's centre, else the player), in chunks, and how far (chunks).
    focus: [AtomicI32; 2],
    radius: AtomicI32,
    /// The far terrain setting is on (else the thread idles).
    active: AtomicBool,
    /// The generator may be run and the LOD source's disk cache written: only while the game draws no far
    /// terrain of its own, so the two never write the level's far terrain disk cache at once.
    sampler_ok: AtomicBool,
    /// Chunks baked since the thread started (for the log and the bench numbers).
    baked: AtomicU64,
    /// Readers wanted (the setting; 0 = auto).
    threads: AtomicU32,
    /// The world map is open (more readers on automatic).
    map_open: AtomicBool,
}

impl FarShared {
    fn new() -> Self {
        FarShared {
            epoch: AtomicU64::new(1),
            focus: [AtomicI32::new(0), AtomicI32::new(0)],
            radius: AtomicI32::new(FAR_RADIUS),
            active: AtomicBool::new(false),
            sampler_ok: AtomicBool::new(false),
            baked: AtomicU64::new(0),
            threads: AtomicU32::new(0),
            map_open: AtomicBool::new(false),
        }
    }
    fn focus(&self) -> [i32; 2] {
        [self.focus[0].load(Ordering::Relaxed), self.focus[1].load(Ordering::Relaxed)]
    }
}

/// Where one place's far terrain comes from (`pick_far`).
#[derive(Default)]
struct FarSources {
    hlod: Option<PathBuf>,
    /// The save's `region` folder (single player).
    save_regions: Option<PathBuf>,
    /// The level's LOD source (single player: the save over the generator).
    source: Option<Arc<dyn horizon_client::streaming::LodSource>>,
    /// The server-terrain folder of the level (a server).
    terrain: Option<PathBuf>,
}

impl FarSources {
    fn any(&self) -> bool {
        self.hlod.is_some() || self.source.is_some() || self.terrain.is_some()
    }
}

struct FarJob {
    epoch: u64,
    src: FarSources,
    /// Map folders whose explored chunks need no far bake (the surface layer, the saved far layer); `true`: only
    /// files that carry block ids count (a far layer saved before 0.3.5 is baked again, once, for its textures).
    skip: Vec<(PathBuf, bool)>,
    table: Arc<ColorTable>,
    tints: Arc<TintTable>,
}

/// Raw deflate through `flate2`, which Horizon links already (`hlod::Inflate`).
fn inflate(data: &[u8], out: &mut Vec<u8>, limit: usize) -> bool {
    use std::io::Read;
    out.clear();
    let mut d = flate2::read::DeflateDecoder::new(data).take(limit as u64 + 1);
    d.read_to_end(out).is_ok() && out.len() <= limit
}

fn from_lod(s: &horizon_client::lod::Surface) -> far::Surface {
    far::Surface { height: s.height, state: s.state.0, biome: s.biome, water: s.water.map(|(h, st)| (h, st.0)) }
}

fn from_columns(c: &[horizon_client::lod::Column; 256], out: &mut [Option<far::Surface>; 256]) -> bool {
    let mut any = false;
    for (o, col) in out.iter_mut().zip(c.iter()) {
        *o = col.surface.as_ref().map(from_lod);
        any |= o.is_some();
    }
    any
}

/// Fills `out` (index z * 16 + x) for chunk (`cx`, `cz`) from a column sampler taken every `stride` blocks;
/// true when any column was found.
fn sample_chunk(
    cx: i32,
    cz: i32,
    stride: i32,
    out: &mut [Option<far::Surface>; 256],
    mut f: impl FnMut(i32, i32) -> Option<far::Surface>,
) -> bool {
    let mut any = false;
    let stride = stride.clamp(1, 16);
    for lz in (0..16).step_by(stride as usize) {
        for lx in (0..16).step_by(stride as usize) {
            let s = f(cx * 16 + lx, cz * 16 + lz);
            any |= s.is_some();
            for dz in 0..stride {
                for dx in 0..stride {
                    let (x, z) = (lx + dx, lz + dz);
                    if x < 16 && z < 16 {
                        out[(z * 16 + x) as usize] = s;
                    }
                }
            }
        }
    }
    any
}

/// One column of the live LOD tiles. Stock Horizon gives the top block and its height (`Lod::ground`); built
/// with `--cfg megamap_far_source` and patches/far-surface-accessor.patch applied, the whole surface (water and
/// biome too).
// The cfg is unknown to the stock build's check-cfg list; the allow on the enclosing module covers it.
mod live {
    use super::far;

    pub(super) fn column(lod: &horizon_client::lod::Lod, x: i32, z: i32) -> Option<far::Surface> {
        lod.megamap_surface(x, z).map(|s| super::from_lod(&s))
    }
}

type FarOut = (u64, Vec<BakedChunk>);

fn spawn_far(shared: Arc<FarShared>, job: FarJob) -> Option<Receiver<FarOut>> {
    let (tx, rx) = channel::<FarOut>();
    std::thread::Builder::new()
        .name("megamap-far".into())
        .spawn(move || far_thread(&shared, job, &tx))
        .ok()?;
    Some(rx)
}

/// The readers the far thread keeps open between chunks of one pass.
struct FarReaders {
    hlod: HashMap<(i32, i32), Option<HlodRegion<std::fs::File>>>,
    terrain: Option<Arc<horizon_client::server_terrain::ServerTerrain>>,
    terrain_reads: u32,
    sampler: Option<Box<dyn horizon_client::streaming::LodSampler>>,
    buf: Vec<u8>,
    raw: Vec<u8>,
}

impl FarReaders {
    /// Forgets the open files: the game keeps writing them (new chunks, new region headers).
    fn refresh(&mut self) {
        self.hlod.clear();
        self.terrain = None;
        self.terrain_reads = 0;
    }
}

/// Reads chunk `c` from the first source that has it into `cols`.
fn far_read(job: &FarJob, rd: &mut FarReaders, t: far::FarTask, sampler_ok: bool, cols: &mut [Option<far::Surface>; 256]) -> bool {
    let [cx, cz] = t.chunk;
    cols.fill(None);
    if t.sources & far::SRC_HLOD != 0 {
        if let Some(dir) = &job.src.hlod {
            let file = rd
                .hlod
                .entry((cx >> 5, cz >> 5))
                .or_insert_with(|| std::fs::File::open(dir.join(hlod::file_name(cx, cz))).ok().and_then(HlodRegion::open));
            if let Some(f) = file {
                if f.read(cx, cz, inflate, &mut rd.buf, &mut rd.raw, cols) {
                    return true;
                }
            }
        }
    }
    if t.sources & far::SRC_TERRAIN != 0 {
        if let Some(dir) = &job.src.terrain {
            // A fresh reader now and then: its chunk cache only grows (8192 chunks), and region headers are read
            // once per open.
            if rd.terrain_reads >= 2048 {
                rd.terrain = None;
                rd.terrain_reads = 0;
            }
            let t = rd.terrain.get_or_insert_with(|| horizon_client::server_terrain::ServerTerrain::new(dir.clone(), None));
            rd.terrain_reads += 1;
            if let Some(c) = t.chunk(cx, cz) {
                if from_columns(&c, cols) {
                    return true;
                }
            }
        }
    }
    if t.sources & far::SRC_SAVE != 0 {
        if let Some(src) = &job.src.source {
            let s = rd.sampler.get_or_insert_with(|| src.clone().sampler());
            if sampler_ok {
                // The whole saved chunk at once (or the generated one the disk cache kept).
                s.request_detail(&[(cx, cz)], [cx, cz]);
                for (key, c) in s.drain_details() {
                    if key == (cx, cz) && from_columns(&c, cols) {
                        return true;
                    }
                }
            } else {
                // Column by column: the saved chunk is decoded once and cached by the sampler; nothing is written.
                return sample_chunk(cx, cz, 1, cols, |x, z| s.reduced_column(x, z, 1).surface.as_ref().map(from_lod));
            }
        }
    }
    false
}

/// What one reader made of its share of a block of tasks.
struct FarPart {
    baked: Vec<BakedChunk>,
    read: Duration,
    bake: Duration,
    chunks: usize,
}

/// Reads and bakes `tasks` with one reader (one of the far thread's helpers runs this on its share).
fn far_part(job: &FarJob, rd: &mut FarReaders, tasks: &[far::FarTask], sampler_ok: bool) -> FarPart {
    let mut out = FarPart { baked: Vec::with_capacity(tasks.len()), read: Duration::ZERO, bake: Duration::ZERO, chunks: tasks.len() };
    let mut cols = [None; 256];
    for &t in tasks {
        let a = Instant::now();
        let ok = far_read(job, rd, t, sampler_ok, &mut cols);
        let b = Instant::now();
        out.read += b - a;
        if ok {
            if let Some(c) = far::bake_surfaces(t.chunk[0], t.chunk[1], &cols, &job.table, &job.tints) {
                out.baked.push(c);
            }
            out.bake += b.elapsed();
        }
    }
    out
}

/// Readers working the far tasks at once: a quarter of the cores, 1 to 3 (each reads its own files; the read and
/// decode of a stored chunk is the cost, the bake is a few microseconds).
fn far_threads(setting: u32, map_open: bool) -> usize {
    if setting > 0 {
        return (setting as usize).min(3);
    }
    // Automatic: a quarter of the cores behind the game, half of them while the map is open (the player is
    // looking at the far terrain coming in, and the world behind the map matters less).
    let n = std::thread::available_parallelism().map_or(1, |n| n.get());
    if map_open { (n / 2).clamp(1, 6) } else { (n / 4).clamp(1, 3) }
}

fn far_thread(shared: &FarShared, job: FarJob, tx: &Sender<FarOut>) {
    let mut done: HashSet<[i32; 2]> = HashSet::new();
    let mut coarse_done: HashSet<[i32; 2]> = HashSet::new();
    // Chunks a source listed that came out with nothing (a file being written, a read error): planned again
    // after a while, a few times (`FAR_RETRY_*`), instead of staying done for good.
    let mut retry: HashMap<[i32; 2], (Instant, u8)> = HashMap::new();
    let new_reader = || FarReaders { hlod: HashMap::new(), terrain: None, terrain_reads: 0, sampler: None, buf: Vec::new(), raw: Vec::new() };
    let mut readers: Vec<FarReaders> = (0..far_threads(shared.threads.load(Ordering::Relaxed), shared.map_open.load(Ordering::Relaxed))).map(|_| new_reader()).collect();
    let mut parts: Vec<Vec<far::FarTask>> = vec![Vec::new(); readers.len()];
    // Time spent reading and baking (summed over the readers) and chunks looked at, for the log.
    let mut phase = (Duration::ZERO, Duration::ZERO, 0usize);
    let mut cols = [None; 256];
    let mut grid_cols: Vec<Option<far::Surface>> = Vec::new();
    let mut batch = Vec::with_capacity(32);
    let mut tasks: Vec<far::FarTask> = Vec::new();
    let started = Instant::now();
    let mut logged = Instant::now();
    let mut last_count = 0u64;
    let send = |batch: &mut Vec<BakedChunk>| -> bool {
        batch.is_empty() || tx.send((job.epoch, std::mem::replace(batch, Vec::with_capacity(32)))).is_ok()
    };
    'pass: loop {
        if shared.epoch.load(Ordering::Relaxed) != job.epoch {
            return;
        }
        if !shared.active.load(Ordering::Relaxed) {
            for rd in readers.iter_mut() {
                rd.sampler = None;
            }
            std::thread::sleep(Duration::from_millis(250));
            continue;
        }
        let want = far_threads(shared.threads.load(Ordering::Relaxed), shared.map_open.load(Ordering::Relaxed));
        readers.truncate(want);
        while readers.len() < want {
            readers.push(new_reader());
        }
        parts.resize(want, Vec::new());
        let focus = shared.focus();
        let radius = shared.radius.load(Ordering::Relaxed).clamp(8, FAR_MAX_RADIUS);
        let now = Instant::now();
        for (c, (at, _)) in retry.iter_mut() {
            if now >= *at {
                done.remove(c);
                *at = now + Duration::from_secs(3600); // planned again: waits for its result
            }
        }
        // Plan: every chunk some source has, from region headers, minus what is on the map already.
        tasks.clear();
        let mut work = 0usize;
        for key in far::regions_around(focus, radius) {
            let mut present: Vec<(u8, [bool; 1024])> = Vec::with_capacity(3);
            if let Some(dir) = &job.src.hlod {
                let f = readers[0]
                    .hlod
                    .entry(key)
                    .or_insert_with(|| std::fs::File::open(dir.join(hlod::file_name(key.0 * 32, key.1 * 32))).ok().and_then(HlodRegion::open));
                if let Some(f) = f {
                    let mut p = [false; 1024];
                    for (cx, cz) in f.chunks() {
                        p[((cz & 31) * 32 + (cx & 31)) as usize] = true;
                    }
                    present.push((far::SRC_HLOD, p));
                }
            }
            if let Some(dir) = &job.src.terrain {
                if let Some(p) = far::read_mca_present(&dir.join(format!("r.{}.{}.mca", key.0, key.1))) {
                    present.push((far::SRC_TERRAIN, p));
                }
            }
            if let Some(dir) = &job.src.save_regions {
                if let Some(p) = far::read_mca_present(&dir.join(format!("r.{}.{}.mca", key.0, key.1))) {
                    present.push((far::SRC_SAVE, p));
                }
            }
            if present.is_empty() {
                continue;
            }
            let mut skip = [0u64; 16];
            for (d, need_blocks) in &job.skip {
                for (s, m) in skip.iter_mut().zip(store::explored_mask(d, key.0, key.1, *need_blocks)) {
                    *s |= m;
                }
            }
            far::plan_region(key, &present, &skip, focus, radius, &|c| done.contains(&c), &mut tasks);
        }
        far::sort_tasks(&mut tasks, focus);
        let sampler_ok = shared.sampler_ok.load(Ordering::Relaxed);
        if !sampler_ok {
            for rd in readers.iter_mut() {
                rd.sampler = None; // let go of the generator's threads while the game runs its own
            }
        }
        for block in tasks.chunks(FAR_BLOCK) {
            if shared.epoch.load(Ordering::Relaxed) != job.epoch || !shared.active.load(Ordering::Relaxed) {
                continue 'pass;
            }
            // The view moved on: plan again around the new focus (what was baked stays done).
            let f = shared.focus();
            let r = shared.radius.load(Ordering::Relaxed).clamp(8, FAR_MAX_RADIUS);
            // Or the view zoomed out (or in) a step: plan for the new size, nearest first again.
            if far::dist2(f, focus) > (radius as i64 / 4).max(8).pow(2) || r * 4 > radius * 5 || r * 5 < radius * 4 {
                if !send(&mut batch) {
                    return;
                }
                continue 'pass;
            }
            for t in block {
                done.insert(t.chunk);
            }
            work += block.len();
            // Split over the readers: a chunk only the save has goes to the first one (one generator sampler),
            // the rest round robin; each reader keeps its own open files, so they never share a lock.
            let k = readers.len();
            for p in parts.iter_mut() {
                p.clear();
            }
            let mut rr = 0;
            for &t in block {
                if k == 1 || t.sources & !far::SRC_SAVE == 0 {
                    parts[0].push(t);
                } else {
                    parts[rr % k].push(t);
                    rr += 1;
                }
            }
            let results: Vec<FarPart> = if k == 1 {
                vec![far_part(&job, &mut readers[0], &parts[0], sampler_ok)]
            } else {
                let job = &job;
                std::thread::scope(|s| {
                    let hs: Vec<_> = readers
                        .iter_mut()
                        .zip(parts.iter())
                        .map(|(rd, p)| s.spawn(move || far_part(job, rd, p, sampler_ok)))
                        .collect();
                    hs.into_iter().filter_map(|h| h.join().ok()).collect()
                })
            };
            let got: HashSet<[i32; 2]> = results.iter().flat_map(|r| r.baked.iter().map(|b| [b.cx, b.cz])).collect();
            for t in block {
                if got.contains(&t.chunk) {
                    retry.remove(&t.chunk);
                } else {
                    let e = retry.entry(t.chunk).or_insert((now, 0));
                    e.1 += 1;
                    e.0 = Instant::now() + FAR_RETRY_AFTER * e.1 as u32;
                    if e.1 > FAR_RETRY_TRIES {
                        retry.remove(&t.chunk); // given up: stays done
                    }
                }
            }
            for r in results {
                shared.baked.fetch_add(r.baked.len() as u64, Ordering::Relaxed);
                phase.0 += r.read;
                phase.1 += r.bake;
                phase.2 += r.chunks;
                batch.extend(r.baked);
            }
            if batch.len() >= 32 && !send(&mut batch) {
                return;
            }
            std::thread::yield_now();
        }
        if !send(&mut batch) {
            return;
        }
        // The rest, single player, while the game generates no far terrain itself: a coarse pass of the
        // generator over groups of 4 x 4 chunks, nearest first, a bounded number per pass.
        if sampler_ok && work == 0 {
            if let Some(src) = &job.src.source {
                let s = readers[0].sampler.get_or_insert_with(|| src.clone().sampler());
                let gr = (radius.min(FAR_COARSE_RADIUS) + 3) / 4;
                let center = [focus[0].div_euclid(4), focus[1].div_euclid(4)];
                let mut groups = 0;
                for g in Spiral::new(center, gr) {
                    if groups >= 16 {
                        break;
                    }
                    if coarse_done.contains(&g) || far::dist2(g, center) > (gr as i64).pow(2) {
                        continue;
                    }
                    coarse_done.insert(g);
                    let (x0, z0) = (g[0] * 64, g[1] * 64);
                    let side = (64 / FAR_COARSE_STRIDE) as usize;
                    let grid = s.grid(x0, z0, FAR_COARSE_STRIDE, side);
                    grid_cols.clear();
                    grid_cols.extend(grid.iter().map(|c| c.surface.as_ref().map(from_lod)));
                    for dz in 0..4 {
                        for dx in 0..4 {
                            let c = [g[0] * 4 + dx, g[1] * 4 + dz];
                            if done.contains(&c) {
                                continue;
                            }
                            done.insert(c);
                            if far::spread_grid(c[0], c[1], x0, z0, FAR_COARSE_STRIDE, side, &grid_cols, &mut cols) {
                                if let Some(b) = far::bake_surfaces(c[0], c[1], &cols, &job.table, &job.tints) {
                                    batch.push(b);
                                    shared.baked.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    }
                    groups += 1;
                    work += 1;
                    if shared.epoch.load(Ordering::Relaxed) != job.epoch {
                        return;
                    }
                }
                if !send(&mut batch) {
                    return;
                }
            }
        }
        if logged.elapsed() >= Duration::from_secs(10) {
            let n = shared.baked.load(Ordering::Relaxed);
            if n != last_count {
                let secs = logged.elapsed().as_secs_f64();
                let per = |d: Duration| if phase.2 == 0 { 0.0 } else { d.as_secs_f64() * 1e6 / phase.2 as f64 };
                log::info!(
                    "megamap: far terrain {} chunks ({:.2} regions/s over the last {:.0} s, {:.0} s since start; \
                     {} readers, read+decode {:.0} us/chunk, bake {:.1} us/chunk)",
                    n,
                    (n - last_count) as f64 / 1024.0 / secs,
                    secs,
                    started.elapsed().as_secs_f64(),
                    readers.len(),
                    per(phase.0),
                    per(phase.1)
                );
                phase = (Duration::ZERO, Duration::ZERO, 0);
                last_count = n;
            }
            logged = Instant::now();
        }
        if work == 0 {
            // Nothing new around the focus: look again later with fresh headers (the game keeps writing).
            for rd in readers.iter_mut() {
                rd.refresh();
            }
            if done.len() > 400_000 {
                let f = shared.focus();
                done.retain(|c| far::dist2(*c, f) <= (FAR_MAX_RADIUS as i64 * 2).pow(2));
            }
            for _ in 0..8 {
                std::thread::sleep(Duration::from_millis(125));
                let r = shared.radius.load(Ordering::Relaxed).clamp(8, FAR_MAX_RADIUS);
                if far::dist2(shared.focus(), focus) > 64
                    || r != radius
                    || shared.epoch.load(Ordering::Relaxed) != job.epoch
                {
                    break;
                }
            }
        }
    }
}

/// Where this world's far terrain comes from. Single player: the far terrain disk cache, the save's region
/// files and the level's LOD source. A server (not a friend's world, not as a splitscreen guest): the folder
/// Horizon keeps the server's chunks in (`server-terrain/...`), whether or not the server offers far terrain
/// and whether or not the folder exists yet (it is created once the game's far terrain sees a chunk).
fn pick_far(game: &crate::game::Game<'_>) -> FarSources {
    let dim = game.dim;
    let cwd = Some(crate::game::Game::game_dir());
    if let Some(server) = game.integrated.as_ref() {
        let folder = game.integrated.as_ref().map(|s| s.save_folder.clone());
        let root = cwd.zip(folder).map(|(c, f)| horizon_client::save::dimension_dir(&c.join("saves").join(f), dim.id()));
        let source = server.call(move |g| g.lod_source_in(dim)).flatten();
        return FarSources {
            hlod: root.as_ref().map(|r| hlod::dir_of(r)),
            save_regions: root.map(|r| r.join("region")),
            source,
            terrain: None,
        };
    }
    let Some(remote) = game.remote.as_ref() else { return FarSources::default() };
    if game.options.split_guest || remote.session.address.starts_with(horizon_client::net::FRIEND) {
        return FarSources::default();
    }
    let Some(cwd) = cwd else { return FarSources::default() };
    use horizon_client::server_terrain as terrain;
    let dir = terrain::level_dir(&cwd.join("server-terrain"), &remote.session.address, remote.hashed_seed, &remote.level);
    FarSources { terrain: Some(dir), ..Default::default() }
}

// ---------------------------------------------------------------------------------------------------------------
// Main-thread state.

/// One of the state's map layers by id, borrowing only that field (so the image buffers stay free to write).
macro_rules! lay {
    ($s:expr, $id:expr) => {
        match $id {
            0 => &$s.surface,
            1 => &$s.cave,
            2 => &$s.far,
            3 => &$s.other,
            _ => &$s.other_far,
        }
    };
}

/// `lay!` for writing: borrows only the one layer field, so other fields stay usable.
macro_rules! lay_mut {
    ($s:expr, $id:expr) => {
        match $id {
            0 => &mut $s.surface,
            1 => &mut $s.cave,
            2 => &mut $s.far,
            3 => &mut $s.other,
            _ => &mut $s.other_far,
        }
    };
}

/// The last minimap image of the layer left behind (0.3.7). It stays on screen, turned and moved with the camera
/// (`layers::crossfade_warped`), while the new layer is still baking, then fades out: no frozen frame, no empty
/// slice filling in chunk by chunk.
struct LayerFade {
    old: Vec<u8>,
    view: View,
    /// When the switch happened.
    since: Instant,
    /// When the cross-fade started (`None`: still holding the old image).
    start: Option<Instant>,
}

/// One map layer (the surface, or the cave slice around the player) with its region cache.
struct MapLayer {
    store: Store,
    epoch: Arc<AtomicU64>,
    queued: HashSet<[i32; 2]>,
    baked: HashSet<[i32; 2]>,
    snap: Option<NativeClientChunks>,
    /// Region folder; `None` keeps the layer in memory only (the cave slice).
    dir: Option<PathBuf>,
    /// The cave slice's y bucket.
    bucket: i32,
    /// 0 surface, 1 cave, 2 far terrain (`Job::Load`).
    id: u8,
    /// When the layer was last reset (the cave slice tracks new ground only once it has settled).
    since: Instant,
}

impl MapLayer {
    fn new(id: u8) -> Self {
        MapLayer {
            store: Store::new(if id == 2 { FAR_REGION_LIMIT } else { REGION_LIMIT }),
            epoch: Arc::new(AtomicU64::new(1)),
            queued: HashSet::new(),
            baked: HashSet::new(),
            snap: None,
            dir: None,
            bucket: i32::MIN,
            id,
            since: Instant::now(),
        }
        .tracking()
    }
    /// The surface and the cave slice bring newly explored ground out of the fog behind the front
    /// (`Store::front`); the cave slice has no files, and right after a rebuild all of it would be news, so it
    /// starts tracking only once settled (`State::step_fronts`).
    fn tracking(mut self) -> Self {
        self.store.track_fresh = self.id == 0;
        self.store.memory_only = self.id == 1;
        self.since = Instant::now();
        self
    }
    fn reset(&mut self, dir: Option<PathBuf>) {
        self.epoch.fetch_add(1, Ordering::Relaxed);
        self.store = Store::new(self.store.limit);
        self.store.track_fresh = self.id == 0;
        self.store.memory_only = self.id == 1;
        self.since = Instant::now();
        self.queued.clear();
        self.baked.clear();
        self.snap = None;
        self.dir = dir;
    }
}

struct Look<'a> {
    store: &'a Store,
    far: Option<&'a Store>,
    /// Ramp width (blocks) of newly explored ground coming out of the fog behind the front (`reveal::Front`);
    /// `None` shows it as it is.
    reveal: Option<f32>,
    /// Block textures: the layer's chunk snapshot (when it has one) and the loaded patterns (`TexLoader`); where
    /// no chunk is loaded the block ids saved with the regions stand in (0.3.5).
    tex: Option<(Option<&'a NativeClientChunks>, &'a TexLoader)>,
}

impl<'a> Look<'a> {
    fn plain(store: &'a Store, far: Option<&'a Store>) -> Self {
        Look { store, far, reveal: None, tex: None }
    }
}

impl Lookup for Look<'_> {
    #[inline]
    fn region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.store.get(rx, rz)
    }
    #[inline]
    fn far_region(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.far.and_then(|f| f.get(rx, rz))
    }
    #[inline]
    fn reveal(&self, x: i32, z: i32) -> u32 {
        match self.reveal {
            Some(soft) if !self.store.front.is_empty() => self.store.front.amount(x, z, soft),
            _ => 256,
        }
    }
    #[inline]
    fn has_coarse_far(&self) -> bool {
        false
    }
    fn bounds(&self) -> Option<[i32; 4]> {
        // Around both stores; an empty rectangle when neither holds a region.
        let b = [self.store.bounds(), self.far.and_then(|f| f.bounds())];
        Some(match (b[0], b[1]) {
            (Some(a), Some(b)) => [a[0].min(b[0]), a[1].min(b[1]), a[2].max(b[2]), a[3].max(b[3])],
            (Some(a), None) | (None, Some(a)) => a,
            (None, None) => [0, 0, -1, -1],
        })
    }
    fn detail(&self, x: i32, y: i32, z: i32) -> Option<&Detail> {
        let (snap, tex) = self.tex?;
        if let Some(snap) = snap.filter(|s| s.loaded([x, z])) {
            return tex.get(snap.block([x, y, z]).0);
        }
        // Not loaded: the block saved for the column, from the explored map, else from the far terrain (only
        // where the explored map has nothing, so a texture never belongs to another surface than the colour).
        let (rx, rz) = (x >> 9, z >> 9);
        tex.get(engine::region::saved_block(self.store.get(rx, rz), self.far.and_then(|f| f.get(rx, rz)), x, z)?)
    }
}

/// Block texture patterns (`detail::Detail`) by block state, loaded on a worker from the resources as the map
/// asks for them (`Lookup::detail`); started again when the resources change.
#[derive(Default)]
struct TexLoader {
    /// `native_block_sprites_version` the worker loads from (0: none).
    version: u64,
    tx: Option<Sender<u32>>,
    /// In a mutex only so the loader is `Sync` (the map renders on several threads); never contended.
    rx: Option<Mutex<Receiver<(u32, Option<Arc<Detail>>)>>>,
    /// Answered states (`None`: no texture), and the asked ones.
    map: HashMap<u32, Option<Arc<Detail>>>,
    asked: HashSet<u32>,
    /// States a render wanted and did not find (from any of its threads).
    want: Mutex<Vec<u32>>,
    /// Bumped when patterns arrive (the image key, so the map redraws with them).
    loaded: u64,
}

impl TexLoader {
    fn get(&self, id: u32) -> Option<&Detail> {
        match self.map.get(&id) {
            Some(d) => d.as_deref(),
            None => {
                if !self.asked.contains(&id) {
                    if let Ok(mut w) = self.want.try_lock() {
                        if w.len() < 256 && !w.contains(&id) {
                            w.push(id);
                        }
                    }
                }
                None
            }
        }
    }

    fn clear(&mut self) {
        let loaded = self.loaded;
        *self = TexLoader::default();
        self.loaded = loaded + 1;
    }

    /// Starts a loader for the resources as they are (when they changed), passes on what the renders wanted and
    /// takes in what arrived.
    fn upkeep(&mut self, game: &crate::game::Game<'_>, on: bool) {
        if !on {
            if self.version != 0 {
                self.clear();
            }
            return;
        }
        let version = game.megamap_block_sprites_version().max(1);
        if version != self.version {
            self.clear();
            self.version = version;
            let states = game.megamap_block_states();
            let res = game.megamap_client_resources();
            let (tx, jobs) = channel::<u32>();
            let (done, rx) = channel();
            let _ = std::thread::Builder::new().name("megamap-textures".into()).spawn(move || {
                let mut by_name: HashMap<String, Option<Arc<Detail>>> = HashMap::new();
                while let Ok(id) = jobs.recv() {
                    let name = states
                        .sprites(BlockStateId(id))
                        .and_then(|sp| sp.sides[1].first().or_else(|| sp.sides.iter().find_map(|s| s.first())).map(|f| f.sprite))
                        .and_then(|sprite| states.texture(sprite).map(str::to_owned));
                    let d = name.and_then(|n| {
                        by_name
                            .entry(n)
                            .or_insert_with_key(|n| res.texture(n).and_then(|t| Detail::build(t.width, t.height, &t.rgba)).map(Arc::new))
                            .clone()
                    });
                    if done.send((id, d)).is_err() {
                        break;
                    }
                }
            });
            self.tx = Some(tx);
            self.rx = Some(Mutex::new(rx));
        }
        if let (Some(tx), Ok(want)) = (&self.tx, self.want.get_mut()) {
            for id in want.drain(..) {
                if self.asked.insert(id) {
                    let _ = tx.send(id);
                }
            }
        }
        let mut got = false;
        if let Some(Ok(rx)) = self.rx.as_mut().map(Mutex::get_mut) {
            while let Ok((id, d)) = rx.try_recv() {
                self.map.insert(id, d);
                got = true;
            }
        }
        if got {
            self.loaded += 1;
        }
    }
}

/// Square map pixels outside the inscribed circle made transparent (the cloud pass renders the round minimap
/// square, so the clouds never take the corners for unexplored ground).
fn mask_round(buf: &mut [u8], w: usize, h: usize) {
    if w == 0 {
        return;
    }
    let (hw, hh) = (w as f32 / 2.0, h as f32 / 2.0);
    let r2 = hw.min(hh).powi(2);
    let outside = |x: usize, dy: f32| {
        let dx = x as f32 + 0.5 - hw;
        dx * dx + dy * dy > r2
    };
    // The pixels kept in a row are one run (the test is monotone in |dx|): clear the two ends in place of a
    // test and a 4-byte store per pixel, same pixels as the per-pixel loop.
    for (y, row) in buf[..w * h * 4].chunks_exact_mut(w * 4).enumerate() {
        let dy = y as f32 + 0.5 - hh;
        let a = (0..w).find(|&x| !outside(x, dy)).unwrap_or(w);
        let b = (a..w).rev().find(|&x| !outside(x, dy)).map_or(w, |x| x + 1);
        row[..a * 4].fill(0);
        row[b * 4..].fill(0);
    }
}

fn force_opaque(buf: &mut [u8]) {
    for a in buf.iter_mut().skip(3).step_by(4) {
        *a = 255;
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Slot {
    None,
    Mini,
    World,
}

/// What an image was last rendered from; equal keys skip the render and the upload.
/// Entity portraits (see `engine::portrait`): decoded textures, 8 x 8 faces and small 3D renders, filled lazily
/// from the frame hook (the only place with the game's resources) under a per-frame budget; drawing reads the
/// caches and shows the plain blip until an entry is ready.
#[derive(Default)]
struct Portraits {
    /// Decoded textures by name (None: the game could not give it).
    tex: HashMap<String, Option<Arc<portrait::Tex>>>,
    /// Faces by `kind|texture`.
    faces: HashMap<String, Option<portrait::Face>>,
    /// 3D renders by `kind|texture|size`: RGBA8 and its fill runs.
    renders: HashMap<String, Option<Arc<Render3d>>>,
    /// The default body texture of a kind (for the entities window, where there is no live entity).
    kind_tex: HashMap<String, Option<String>>,
    /// The texture of each radar blip, parallel to `radar.blips`.
    blip_tex: Vec<Option<String>>,
    /// The entity id of each radar blip, parallel to `radar.blips` (`track_blips` finds it again every frame).
    blip_ids: Vec<u32>,
    /// Each tracked entity's glide (the same `Follow` the player's position goes through).
    blip_follow: HashMap<u32, view::Follow>,
    /// Kinds that failed to render, with the reason (for the report / debug line).
    failed: Vec<(String, String)>,
    bench_done: bool,
    /// The entities window's turning 3D preview.
    spin: Option<Spin>,
}

/// The live preview of the selected kind: its model, kept to re-rasterise as it turns (by itself, and with the
/// mouse), and the latest render.
struct Spin {
    key: String,
    model: Option<Arc<EntityModel>>,
    render: Option<Render3d>,
    /// The angles `render` was made at.
    yaw: f32,
    pitch: f32,
    zoom: f32,
    /// The auto spin's angle, degrees.
    auto: f32,
    rendered: Instant,
    ticked: Instant,
    /// The last frame the mouse turned it: the auto spin rests a moment after that.
    held: Instant,
}

struct Render3d {
    n: usize,
    runs: Vec<(u16, u16, u16, u32)>,
    ms: f32,
}

/// The cache key of a face / render.
fn pkey(kind: &str, tex: Option<&str>) -> String {
    format!("{kind}|{}", tex.unwrap_or(""))
}

/// The live preview of the settings window (see `engine::preview`): its own clouds and pixels, uploaded into the
/// top `preview::SIZE` rows of the mod image above the world map while the settings window is open.
struct Preview {
    clouds: Clouds,
    buf: Vec<u8>,
    /// Seconds (of `State::t0`) of the last preview frame.
    last: f64,
    /// A frame was made and not uploaded yet.
    dirty: bool,
    /// The upload that holds the world map also holds the preview rows (the world sits `SIZE` rows lower).
    world_off: usize,
    /// Mean preview frame cost, ms (for the report and the budget).
    ms: f32,
    /// The view of the last frame (blocks per preview pixel, rotation), for the overlay.
    bpp: f64,
    rot: f64,
    combo: Vec<u8>,
}

impl Preview {
    fn new() -> Self {
        Preview { clouds: Clouds::new(), buf: vec![0; preview::SIZE * preview::SIZE * 4], last: -1.0, dirty: false, world_off: 0, ms: 0.0, bpp: 1.0, rot: 0.0, combo: Vec::new() }
    }
}

/// Drag frames faster than this keep the cloud time of the previous frame (see `State::cloud_t`).
const DRAG_TICK: Duration = Duration::from_millis(33);

#[derive(Clone, Copy, PartialEq)]
struct ImageKey {
    /// Fog fades and texture loads (`State::anim`).
    anim: u64,
    store_gen: u64,
    far_gen: u64,
    layer_gen: u64,
    view: [i64; 4],
    size: [usize; 2],
    flags: u32,
}

impl ImageKey {
    /// Whether `self` differs from `old` by new map data alone (chunks baked, far tiles streamed in): the same
    /// view, look, fades and layer.
    fn data_only(&self, old: &ImageKey) -> bool {
        *self != *old && ImageKey { store_gen: old.store_gen, far_gen: old.far_gen, ..*self } == *old
    }
}

/// Redraws for new map data alone come at most every `RENDER_SHARE` render costs (`State::data_tick`), within
/// these bounds: at a far zoom a redraw costs tens of milliseconds and streaming far tiles changes the data
/// every frame, which redrew the whole map every frame.
const RENDER_SHARE: f32 = 8.0;
const DATA_TICK_MIN: Duration = Duration::from_millis(33);
const DATA_TICK_MAX: Duration = Duration::from_millis(200);

/// The cloud rate "every frame" (`cloud_fps` 0) caps itself at `CLOUD_PX_FPS / bpp` frames a second, at least
/// `CLOUD_MIN_FPS` (no cap from `CLOUD_PX_FPS / CLOUD_FULL_FPS` blocks a pixel in): the deck drifts about 6 blocks a
/// second and changes shape over tens of seconds, so at 60 blocks a pixel it moves a tenth of a pixel a second
/// and a cloud frame every game frame (about 15 ms at full size) redrew the same image.
const CLOUD_PX_FPS: f64 = 120.0;
const CLOUD_MIN_FPS: f64 = 10.0;
const CLOUD_FULL_FPS: f64 = 60.0;

/// The cloud frame interval of `cloud_fps` 0 at `bpp` (see `CLOUD_PX_FPS`): every frame near in, under lightning
/// (the flashes), and in the Nether and the End (their embers and sparkles move by the frame).
fn auto_cloud_tick(bpp: f64, p: &clouds::Params) -> Duration {
    let fps = (CLOUD_PX_FPS / bpp.max(1e-3)).max(CLOUD_MIN_FPS);
    if fps >= CLOUD_FULL_FPS || p.look != clouds::Look::Overworld || (p.lightning > 0 && p.thunder > 0.0) {
        return Duration::ZERO;
    }
    Duration::from_secs_f64(1.0 / fps)
}

struct Keys {
    map: String,
    waypoint_here: String,
    waypoints: String,
    zoom: String,
    size: String,
    mode: String,
    rotate: String,
}

/// The eight compass points from south (yaw 0) clockwise, as lang keys.
const COMPASS: [&str; 8] = [
    "megamap.compass.s",
    "megamap.compass.sw",
    "megamap.compass.w",
    "megamap.compass.nw",
    "megamap.compass.n",
    "megamap.compass.ne",
    "megamap.compass.e",
    "megamap.compass.se",
];

const KEYS: [(&str, &str, &str); 7] = [
    ("map", "megamap.key.map", "key.keyboard.m"),
    ("waypoint_here", "megamap.key.waypoint_here", "key.keyboard.b"),
    ("waypoints", "megamap.key.waypoints", "key.keyboard.u"),
    ("zoom", "megamap.key.zoom", "key.keyboard.left.bracket"),
    ("size", "megamap.key.size", "key.keyboard.right.bracket"),
    ("mode", "megamap.key.mode", "key.keyboard.k"),
    ("rotate", "megamap.key.rotate", "key.keyboard.n"),
];

enum Queued {
    Ui(Action),
    OpenMap { list: bool, new_wp: bool },
    CloseMap,
}

struct Toast {
    text: String,
    until: Instant,
}

struct State {
    started: bool,
    data: PathBuf,
    settings: Settings,
    worker: Option<Worker>,
    surface: MapLayer,
    cave: MapLayer,
    pending: VecDeque<(bool, BakedChunk)>,
    // color table, built a slice per frame on the main thread
    table_build: ColorTable,
    table_next: u32,
    table_empty_run: u32,
    table: Option<Arc<ColorTable>>,
    /// The game's own map colours, before the resource pack palette (`palette_upkeep`) replaces `table`.
    table_vanilla: Option<Arc<ColorTable>>,
    /// `palette::pack_hash` of the palette in `table` (0: none asked yet).
    pal_hash: u64,
    /// The palette the far layer is kept in (`far_palette_sync`).
    pal_id: u64,
    /// A palette being built on a worker: its hash, the table, textures decoded, milliseconds.
    pal_rx: Option<Receiver<(u64, ColorTable, usize, f32)>>,
    /// Whether `table_vanilla` has the block light sources (`ColorTable::set_emit`).
    emit_done: bool,
    tex: TexLoader,
    /// The player's position glided between ticks (`view::Follow`).
    follow: view::Follow,
    /// Bumped every frame a chunk is fading out of the fog, and when block textures arrive (`ImageKey::anim`).
    anim: u64,
    /// The last frame the reveal fronts stepped (`step_fronts`).
    front_t: Option<Instant>,
    /// Shown entity names by id for the language they were looked up in (the entities window).
    ent_names: std::cell::RefCell<(Option<Lang>, HashMap<String, String>)>,
    biomes: Option<NativeClientBiomes>,
    // world / dimension
    world_dir: Option<PathBuf>,
    world_id: Option<String>,
    dim: String,
    player: [f64; 3],
    yaw: f32,
    in_world: bool,
    waypoints: Waypoints,
    wp_saved: u64,
    trail: Trail,
    trail_saved: u64,
    // far terrain (see the far thread)
    /// The far layer: what the far thread and the live tiles bake, saved next to the map (`<dim>_far`).
    far: MapLayer,
    far_shared: Arc<FarShared>,
    far_rx: Option<Receiver<FarOut>>,
    /// When the far sources were last looked for (none found yet: looked for again every few seconds).
    far_picked: Option<Instant>,
    /// The far folder and palette `far_palette_sync` last checked.
    far_pal_synced: Option<(PathBuf, u64)>,
    last_probe: Instant,
    /// The height the player stands at for the layer probe, and when they last stood there: a jump (up to two
    /// blocks up, back within a second) is probed from the floor it started on.
    stand: (i32, Instant),
    far_live: Spiral,
    far_live_done: HashSet<[i32; 2]>,
    /// Chunks the running pass over the live tiles baked, and when the next pass may start (`FAR_LIVE_IDLE`).
    far_live_new: u32,
    far_live_wait: Option<Instant>,
    tints: Option<Arc<TintTable>>,
    far_cols: Box<[Option<far::Surface>; 256]>,
    // cloud fog and the light
    clouds: Clouds,
    base: Vec<u8>,
    base_key: Option<ImageKey>,
    last_cloud: Instant,
    /// When `compose` last drew the map, and the smoothed cost of that, ms (`data_tick`).
    base_drawn: Instant,
    render_ms: f32,
    /// Cloud time of the last cloud frame and when it was taken: drag frames that come faster than `DRAG_TICK`
    /// reuse it, so the cloud noise cache hits and a drag only translates the field.
    cloud_t: (f32, Instant),
    t0: Instant,
    weather: (f32, f32),
    day_time: u32,
    light: u32,
    map_opened: Instant,
    radar: Radar,
    // timers
    last_snapshot: Instant,
    last_radar: Instant,
    last_info: Instant,
    /// The block position and whole-degree yaw the info lines were made for (remade at once when they change).
    info_key: Option<([i32; 3], i32)>,
    last_keys: Instant,
    last_save: Instant,
    last_evict: Instant,
    last_persist: Instant,
    last_mini: Instant,
    last_world: Instant,
    // images
    buf: Vec<u8>,
    slot: Slot,
    mini_key: Option<ImageKey>,
    mini_px: usize,
    world_key: Option<ImageKey>,
    /// The world map image as rendered: centre, GUI blocks per pixel, size in GUI pixels and in image pixels.
    world_img: ([f64; 2], f64, [f32; 2], [f32; 2]),
    visible: Vec<(i32, i32)>,
    // world map
    ui: MapUi,
    map_open: bool,
    /// Surface / cave detector of the auto layer, fed in `schedule`.
    detector: layers::Detector,
    /// The cave slice's ceiling, following the player in `cave_step` blocks with a margin (set with each probe).
    slice_track: layers::SliceTracker,
    slice_top: Option<i32>,
    /// Indoors (0.3.4): the room detector (its `cave` flag means "in a room"), the cut height and the minimap zoom
    /// multiplier easing toward the indoor zoom.
    indoor_det: layers::Detector,
    indoor_top: Option<i32>,
    indoor_k: f64,
    last_indoor_k: Instant,
    /// The last roof check, for the layer badge's detail.
    roof: layers::Roof,
    /// The cave slice shown last (`None` the surface): a change starts the cross-fade.
    shown_slice: Option<Option<i32>>,
    /// The minimap image before a layer switch, held (moved with the camera) until the new layer is baked, then
    /// cross-faded out.
    fade_from: Option<LayerFade>,
    /// The view the minimap image on screen was drawn under.
    mini_view: Option<View>,
    /// The dimension the world map shows when it is not the player's (a folder name under the world's data);
    /// read from disk only, through `other` and `other_far`.
    view_dim: Option<String>,
    /// Entity kinds the radar has met this session, for the entities window.
    seen: Vec<String>,
    other: MapLayer,
    other_far: MapLayer,
    explored: f32,
    rows: Vec<usize>,
    rows_key: (u64, String, String),
    // misc
    keys: Keys,
    queue: Vec<Queued>,
    toasts: Vec<Toast>,
    info: Vec<String>,
    was_dead: bool,
    last_shared: Option<(String, [i32; 3], String)>,
    gui: [f32; 2],
    gui_hidden: bool,
    // auto-zoom
    autozoom: AutoZoom,
    az_radius: f64,
    last_az_scan: Instant,
    last_az: Instant,
    // settings preview
    preview: Preview,
    /// The minimap scale last drawn (blocks per GUI pixel), auto-zoom included: the overlay places blips with it.
    mini_bpp: f64,
    portraits: Portraits,
}

impl State {
    fn new() -> Self {
        let now = Instant::now();
        let past = now.checked_sub(Duration::from_secs(60)).unwrap_or(now);
        State {
            started: false,
            data: PathBuf::new(),
            settings: Settings::default(),
            worker: None,
            surface: MapLayer::new(0),
            cave: MapLayer::new(1),
            pending: VecDeque::new(),
            table_build: ColorTable::default(),
            table_next: 0,
            table_empty_run: 0,
            table: None,
            table_vanilla: None,
            pal_hash: 0,
            pal_id: 0,
            pal_rx: None,
            emit_done: false,
            tex: TexLoader::default(),
            follow: view::Follow::default(),
            anim: 0,
            front_t: None,
            ent_names: Default::default(),
            biomes: None,
            world_dir: None,
            world_id: None,
            dim: String::new(),
            player: [0.0; 3],
            yaw: 0.0,
            in_world: false,
            waypoints: Waypoints::from_text(""),
            wp_saved: 0,
            trail: Trail::new(4000, 4),
            trail_saved: 0,
            far: MapLayer::new(2),
            far_shared: Arc::new(FarShared::new()),
            far_rx: None,
            far_picked: None,
            far_pal_synced: None,
            last_probe: past,
            stand: (i32::MIN, past),
            far_live: Spiral::new([0, 0], FAR_RADIUS * 2),
            far_live_done: HashSet::new(),
            far_live_new: 0,
            far_live_wait: None,
            tints: None,
            far_cols: Box::new([None; 256]),
            clouds: Clouds::new(),
            base: Vec::new(),
            base_key: None,
            last_cloud: past,
            base_drawn: past,
            render_ms: 0.0,
            cloud_t: (0.0, past),
            t0: now,
            weather: (0.0, 0.0),
            day_time: 6000,
            light: 256,
            map_opened: past,
            radar: Radar::new(48.0),
            last_snapshot: past,
            last_radar: past,
            last_info: past,
            info_key: None,
            last_keys: past,
            last_save: now,
            last_evict: now,
            last_persist: now,
            last_mini: past,
            last_world: past,
            buf: Vec::new(),
            slot: Slot::None,
            mini_key: None,
            mini_px: 0,
            world_key: None,
            world_img: ([0.0; 2], 1.0, [0.0; 2], [0.0; 2]),
            visible: Vec::with_capacity(64),
            ui: MapUi::new(),
            map_open: false,
            detector: layers::Detector::default(),
            slice_track: layers::SliceTracker::default(),
            slice_top: None,
            indoor_det: layers::Detector::default(),
            indoor_top: None,
            indoor_k: 1.0,
            last_indoor_k: Instant::now(),
            roof: layers::Roof::default(),
            shown_slice: None,
            fade_from: None,
            mini_view: None,
            view_dim: None,
            seen: Vec::new(),
            other: MapLayer::new(3),
            other_far: MapLayer::new(4),
            explored: 0.0,
            rows: Vec::new(),
            rows_key: (u64::MAX, String::new(), String::new()),
            keys: Keys {
                map: KEYS[0].2.into(),
                waypoint_here: KEYS[1].2.into(),
                waypoints: KEYS[2].2.into(),
                zoom: KEYS[3].2.into(),
                size: KEYS[4].2.into(),
                mode: KEYS[5].2.into(),
                rotate: KEYS[6].2.into(),
            },
            queue: Vec::new(),
            toasts: Vec::new(),
            info: Vec::new(),
            was_dead: false,
            last_shared: None,
            gui: [480.0, 270.0],
            gui_hidden: false,
            autozoom: AutoZoom::default(),
            az_radius: 0.0,
            last_az_scan: past,
            last_az: now,
            preview: Preview::new(),
            mini_bpp: 1.0,
            portraits: Portraits::default(),
        }
    }

    /// The minimap scale (blocks per GUI pixel of a `s`-pixel minimap): the fixed zoom, or with auto-zoom the
    /// scale that fits the known data around the player (the fixed zoom then acts as a multiplier, 1x neutral).
    fn minimap_bpp(&mut self, s: usize, cave: bool) -> f64 {
        let indoor = self.indoor_slice().is_some();
        let target = if indoor { layers::indoor_zoom(self.settings.indoor) } else { 1.0 };
        let dt = self.last_indoor_k.elapsed().as_secs_f64();
        self.last_indoor_k = Instant::now();
        self.indoor_k = layers::ease_zoom(self.indoor_k, target, dt);
        // Indoors the auto-zoom keeps measuring the surface layer: the indoor slice is the cave layer, whose data
        // around a house is a few chunks or none, so the auto level changed 1.5 s after the indoor zoom (in, then
        // back out) — the minimap pumped inside a building. The indoor zoom alone is the multiplier on top.
        self.minimap_bpp_base(s, cave && !indoor) * self.indoor_k
    }

    fn minimap_bpp_base(&mut self, s: usize, cave: bool) -> f64 {
        let st = &self.settings;
        if !st.autozoom {
            return st.zoom_bpp();
        }
        let (near, far) = (settings::ZOOMS[st.az_near.min(4)], settings::ZOOMS[st.az_far.min(4)]);
        let mult = st.zoom_bpp() / settings::ZOOMS[1];
        if self.last_az_scan.elapsed() >= Duration::from_millis(250) {
            self.last_az_scan = Instant::now();
            let sel = cave as u8;
            let store = &lay!(self, sel).store;
            let far_store = self.far_for(sel).map(|f| &lay!(self, f).store);
            // The cave slice counts the surface's explored chunks too: right after the switch the slice has only a
            // few chunks baked, so the scale zoomed in and 1.5 s later back out as the bake caught up (the "pump"
            // on entering a closed room or a cave). The loaded chunks get the slice baked anyway.
            let surf = (sel != 0).then(|| &self.surface.store);
            let (pcx, pcz) = ((self.player[0].floor() as i32) >> 4, (self.player[2].floor() as i32) >> 4);
            let known = |dx: i32, dz: i32| {
                let (cx, cz) = (pcx + dx, pcz + dz);
                let (rx, rz) = Region::of_chunk(cx, cz);
                let (lx, lz) = (cx.rem_euclid(32) as usize, cz.rem_euclid(32) as usize);
                store.get(rx, rz).is_some_and(|r| r.chunk_explored(lx, lz))
                    || surf.and_then(|f| f.get(rx, rz)).is_some_and(|r| r.chunk_explored(lx, lz))
                    || far_store.and_then(|f| f.get(rx, rz)).is_some_and(|r| r.chunk_explored(lx, lz))
            };
            let max = ((far * mult.max(1.0) * s as f64 / 2.0) / 16.0).ceil() as i32 + 1;
            self.az_radius = autozoom::known_radius(max.clamp(2, 96), 0.6, known);
        }
        let dt = self.last_az.elapsed().as_secs_f64();
        self.last_az = Instant::now();
        self.autozoom.update(self.az_radius, s as u32, near, far, mult, dt)
    }

    fn toast(&mut self, text: impl Into<String>) {
        if self.toasts.len() >= 4 {
            self.toasts.remove(0);
        }
        self.toasts.push(Toast { text: text.into(), until: Instant::now() + Duration::from_secs(5) });
    }

    /// The top bar's layer note: " (Auto: Cave Y 9..-7)" and the like.
    fn layer_tag(&self) -> String {
        let tr = self.tr();
        let m = self.settings.layer_mode();
        let auto = if m == LayerMode::Auto { tr.t("megamap.ui.layer_auto") } else { String::new() };
        let word = |b: Badge| tr.t(b.key());
        if let Some(cut) = self.indoor_slice() {
            return tr.f("megamap.ui.layer_tag_indoor", &[&word(Badge::Indoor), &cut]) + &auto;
        }
        match self.cave_slice() {
            Some(top) => {
                let (t, f) = layers::cave_window(top, self.settings.cave_depth as i32);
                let b = if m == LayerMode::FixedY { Badge::Slice } else { Badge::Cave };
                tr.f("megamap.ui.layer_tag_cave", &[&word(b), &f, &t]) + &auto
            }
            None if m == LayerMode::Auto => tr.f("megamap.ui.layer_tag", &[&word(Badge::Surface)]) + &auto,
            None => tr.f("megamap.ui.layer_tag", &[&tr.t(&format!("megamap.layer.{}", m.key()))]),
        }
    }

    /// The map's word table: its language, with the game's own translations for the keys it shares with the
    /// game when the map speaks the game's language.
    fn tr(&self) -> Tr {
        let l = self.lang();
        let same = self.settings.lang == 0 || Lang::from_code(&horizon_client::lang::code()) == l;
        Tr { lang: l, game: same.then_some(game_tr as lang::GameFn) }
    }

    /// The language of the map's words: the setting, or the game's.
    fn lang(&self) -> Lang {
        Lang::pick(self.settings.lang, &horizon_client::lang::code())
    }

    /// A registry id's shown name (`kind`: entity, biome, dimension): the game's translation when the map speaks
    /// the game's language, else the built-in tables.
    fn tname(&self, kind: &str, id: &str) -> String {
        let l = self.lang();
        let same = self.settings.lang == 0 || Lang::from_code(&horizon_client::lang::code()) == l;
        // Pre-1.21.2 ids (the game still reports a plain boat as one) have no lang key of their own any more.
        let id = match (kind, id) {
            ("entity", "minecraft:boat") => "minecraft:oak_boat",
            ("entity", "minecraft:chest_boat") => "minecraft:oak_chest_boat",
            _ => id,
        };
        lang::name(l, kind, id,&|k| (same && horizon_client::lang::has(k)).then(|| horizon_client::lang::get(k)))
    }

    fn block_pos(&self) -> [i32; 3] {
        [self.player[0].floor() as i32, self.player[1].floor() as i32, self.player[2].floor() as i32]
    }

    /// The dimension whose map the world map shows (its clouds follow it too).
    fn viewed_dim(&self) -> &str {
        self.view_dim.as_deref().unwrap_or(&self.dim)
    }

    /// Dimension folders saved for this world (the far layers' `_far` folders left out), the player's own first.
    fn dims_on_disk(&self) -> Vec<String> {
        let own = store::sanitize(&self.dim);
        let mut v: Vec<String> = self
            .world_dir
            .as_ref()
            .and_then(|d| std::fs::read_dir(d).ok())
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
                    .filter_map(|e| e.file_name().to_str().map(str::to_string))
                    .filter(|n| !n.ends_with("_far") && *n != own)
                    .collect()
            })
            .unwrap_or_default();
        v.sort();
        v.insert(0, own);
        v
    }

    /// Shows another dimension's saved map on the world map (`None` or the player's own: back to the live map).
    fn set_view_dim(&mut self, dim: Option<String>) {
        let dim = dim.filter(|d| *d != store::sanitize(&self.dim) && *d != self.dim);
        if dim == self.view_dim {
            return;
        }
        self.view_dim = dim;
        let (a, b) = match (&self.view_dim, &self.world_dir) {
            (Some(d), Some(w)) => (Some(w.join(d)), Some(w.join(format!("{d}_far")))),
            _ => (None, None),
        };
        self.other.reset(a);
        self.other_far.reset(b);
        self.world_key = None;
        self.base_key = None;
    }

    fn layer(&self, id: u8) -> &MapLayer {
        match id {
            0 => &self.surface,
            1 => &self.cave,
            2 => &self.far,
            3 => &self.other,
            _ => &self.other_far,
        }
    }

    fn layer_mut(&mut self, id: u8) -> &mut MapLayer {
        match id {
            0 => &mut self.surface,
            1 => &mut self.cave,
            2 => &mut self.far,
            3 => &mut self.other,
            _ => &mut self.other_far,
        }
    }

    /// The far layer drawn under map layer `sel` (none for the cave slice or with far terrain off).
    fn far_for(&self, sel: u8) -> Option<u8> {
        match sel {
            _ if !self.settings.far => None,
            0 => Some(2),
            3 => Some(4),
            _ => None,
        }
    }

    /// The layer the world map shows: another dimension's, the cave slice or the surface.
    fn world_sel(&self) -> u8 {
        if self.view_dim.is_some() {
            3
        } else if self.cave_active() {
            1
        } else {
            0
        }
    }

    /// Whether the cave layer is the one shown now.
    fn cave_active(&self) -> bool {
        self.cave_slice().is_some()
    }

    /// The ceiling of the cave slice shown now (`None` for the surface): the layer mode, the detector's verdict in
    /// auto, the player's height.
    fn cave_slice(&self) -> Option<i32> {
        if let Some(cut) = self.indoor_slice() {
            return Some(cut);
        }
        let top = self.slice_top.unwrap_or_else(|| layers::bucket_top(self.block_pos()[1]));
        layers::slice_at(self.settings.layer_mode(), self.detector.cave, top, self.settings.fixed_y)
    }

    /// The indoor cut shown now: auto layer, the surface, the indoor setting on and the room detector sure.
    fn indoor_slice(&self) -> Option<i32> {
        let on = self.settings.indoor > 0 && self.settings.layer_mode() == LayerMode::Auto && !self.detector.cave;
        self.indoor_top.filter(|_| on && self.indoor_det.cave)
    }

    /// Feeds the auto-layer detector one probe: the roof check over the player (a 3x3 chunk snapshot), the sky
    /// light at the feet and the surface map's top block of the player's column; moves the cave slice with the
    /// player. Called every `PROBE_EVERY`.
    fn probe_layer(&mut self, host: &dyn Host) {
        let mut p = self.block_pos();
        // A jump is probed from the floor: in the air the head is at the ceiling, the roof scan starts inside or
        // above it, and the room read as left.
        let now = Instant::now();
        let (sy, st) = self.stand;
        if p[1] > sy && p[1] <= sy + 2 && now.duration_since(st) < Duration::from_millis(1000) {
            p[1] = sy;
        } else {
            self.stand = (p[1], now);
        }
        let (_, sky) = host.light(p);
        let ground = self.surface.store.get(p[0] >> 9, p[2] >> 9).and_then(|r| {
            let h = r.heights[((p[2] & 511) * 512 + (p[0] & 511)) as usize];
            (h != i16::MIN).then_some(h as i32)
        });
        let probe = layers::Probe { y: p[1], sky: Some(sky as u8), ground };
        let st = &self.settings;
        // The roof check answers the default rule; "sky only" / "height only" stay as they were.
        let roof = match (&self.table, st.surface_rule, st.roof_size) {
            (Some(table), 0, size) if size > 0 => {
                let (cx, cz) = (p[0] >> 4, p[2] >> 4);
                host.client_chunks([cx - 1, cz - 1], [cx + 1, cz + 1])
                    .ok()
                    .map(|snap| layers::roof_probe(&Chunks(&snap), table, p, size, st.roof_reach as i32))
            }
            _ => None,
        };
        let ev = match roof {
            Some(r) => {
                self.roof = r;
                layers::roof_evidence(r, probe.sky)
            }
            None => layers::evidence(probe, st.surface_rule),
        };
        let (enter, exit) = layers::dwell(st.layer_delay);
        let t = self.t0.elapsed().as_secs_f32();
        // A room: the roof check alone says so (sky light shines in through windows and doors).
        // Already indoors the check is looser (`is_room_keep`); no roof data at all (the snapshot failed or the
        // chunks are not there yet) is "unsure": the view stays as it is.
        let was_in = self.indoor_det.cave;
        let room = roof.is_some_and(|r| if was_in { layers::is_room_keep(r, p[1]) } else { layers::is_room(r, p[1]) });
        let room_ev = match roof {
            None => layers::Evidence::Unsure,
            Some(_) if room => layers::Evidence::Cave,
            Some(_) => layers::Evidence::Surface,
        };
        self.indoor_det.step(t, room_ev, layers::INDOOR_ENTER_S, layers::INDOOR_EXIT_S);
        // A room seen from the surface stays a room: a closed one (no window, no sky light) also reads as a cave a
        // moment later, and the cave layer took over (the minimap zoomed in for the room, then back out to the
        // cave layer's auto scale). While the indoor view is on (or a probe already says "room",
        // so a short layer delay cannot beat the indoor dwell), the cave detector hears "surface"; a room dug
        // inside a cave (the cave layer already on) is left to the cave layer.
        let hold = (self.indoor_det.cave || room) && !self.detector.cave && self.settings.indoor > 0 && self.settings.layer_mode() == LayerMode::Auto;
        let ev = if hold { layers::Evidence::Surface } else { ev };
        self.detector.step(t, ev, enter, exit);
        if room {
            self.indoor_top = Some(layers::indoor_cut(self.indoor_top, p[1], self.roof.lowest));
        } else if !self.indoor_det.cave {
            self.indoor_top = None;
        }
        self.slice_top = Some(self.slice_track.top(p[1], self.settings.cave_step as i32));
    }

    /// The layer badge on the minimap and the map: nothing on the plain surface layer, the surface / cave / slice /
    /// building in the other modes. Heights only go in the map's title (`layer_tag`) and the badge's tooltip.
    fn layer_badge(&self) -> Option<Badge> {
        if self.settings.badge >= 2 {
            return None;
        }
        if self.indoor_slice().is_some() {
            return Some(Badge::Indoor);
        }
        match (self.settings.layer_mode(), self.cave_slice()) {
            (LayerMode::Surface, _) => None,
            (LayerMode::FixedY, Some(_)) => Some(Badge::Slice),
            (_, Some(_)) => Some(Badge::Cave),
            (_, None) => Some(Badge::Surface),
        }
    }

    /// Draws the layer badge with its top-left corner at `at` (GUI px): an icon on a dark tile, or the word.
    /// Returns its rectangle.
    fn draw_badge(&self, host: &dyn Host, b: &mut HudBatch, badge: Badge, at: [f32; 2]) -> [f32; 4] {
        let cave = matches!(badge, Badge::Cave | Badge::Slice);
        let (bg, ink) = if cave { (0xB040_2A10, 0xFFFF_D28A) } else { (0xA000_0000, 0xFFE8_E8E8) };
        let (x, y) = (at[0].round(), at[1].round());
        if self.settings.badge == 0 {
            let r = [x, y, 18.0, 18.0];
            host.fill(b, r, bg);
            draw_icon(host, b, badge.icon(), [x + 1.0, y + 1.0], ink);
            r
        } else {
            let t = self.tr().t(badge.key());
            let w = host.text_width(&t);
            let r = [x, y, w + 4.0, 10.0];
            host.fill(b, r, bg);
            host.text(b, &t, [x + 2.0, y + 1.0], ink & 0xFF_FFFF, false);
            r
        }
    }

    // ---- persistence ----

    fn start(&mut self, host: &mut dyn Host) {
        self.started = true;
        self.data = host.data_dir(ID);
        let _ = std::fs::create_dir_all(&self.data);
        if let Ok(t) = std::fs::read_to_string(self.data.join("settings.txt")) {
            self.settings = Settings::from_text(&t);
        }
        self.ui.settings_cat = (self.settings.ui_cat as usize).min(settings::CATEGORIES.len() - 1);
        self.worker = spawn_worker([self.surface.epoch.clone(), self.cave.epoch.clone()]);
        self.refresh_keys(host);
    }

    fn save_settings(&self) {
        let _ = std::fs::write(self.data.join("settings.txt"), self.settings.to_text());
    }

    fn refresh_keys(&mut self, host: &dyn Host) {
        let k = |i: usize| host.mod_key(ID, KEYS[i].0, KEYS[i].2);
        self.keys = Keys { map: k(0), waypoint_here: k(1), waypoints: k(2), zoom: k(3), size: k(4), mode: k(5), rotate: k(6) };
    }

    fn trail_path(&self) -> Option<PathBuf> {
        Some(self.world_dir.as_ref()?.join(format!("trail_{}.txt", store::sanitize(&self.dim))))
    }

    fn region_dir(&self) -> Option<PathBuf> {
        Some(self.world_dir.as_ref()?.join(store::sanitize(&self.dim)))
    }

    /// The far layer's folder, next to the map's (`<dim>_far`): kept apart so the map's own regions stay what the
    /// player explored.
    fn far_dir(&self) -> Option<PathBuf> {
        Some(self.world_dir.as_ref()?.join(format!("{}_far", store::sanitize(&self.dim))))
    }

    /// Queues every unsaved region of the surface and far layers for the worker to write.
    fn flush_regions(&mut self) {
        for layer in [&mut self.surface, &mut self.far] {
            let Some(dir) = layer.dir.clone() else { continue };
            let regions = layer.store.dirty_copies();
            if regions.is_empty() {
                continue;
            }
            match &self.worker {
                Some(w) => {
                    let _ = w.tx.send(Job::Save { dir, regions });
                }
                None => write_now(&dir, regions),
            }
        }
    }

    fn persist_small(&mut self, force: bool) {
        let Some(wd) = self.world_dir.clone() else { return };
        if force || self.waypoints.generation != self.wp_saved {
            let _ = std::fs::create_dir_all(&wd);
            let _ = std::fs::write(wd.join("waypoints.txt"), self.waypoints.to_text());
            self.wp_saved = self.waypoints.generation;
        }
        if force || self.trail.generation != self.trail_saved {
            if let Some(p) = self.trail_path() {
                let _ = std::fs::write(p, self.trail.to_text());
            }
            self.trail_saved = self.trail.generation;
        }
    }

    /// The player entered a world (another save or server) or another dimension.
    fn enter(&mut self, world_id: Option<String>, dim: &str) {
        let world_changed = world_id != self.world_id;
        // Save what belongs to the old place first.
        self.flush_regions();
        self.persist_small(false);
        if world_changed {
            self.world_id = world_id;
            self.world_dir = self.world_id.as_ref().map(|w| self.data.join(w));
            self.waypoints = self
                .world_dir
                .as_ref()
                .and_then(|d| std::fs::read_to_string(d.join("waypoints.txt")).ok())
                .map_or_else(|| Waypoints::from_text(""), |t| Waypoints::from_text(&t));
            self.wp_saved = self.waypoints.generation;
            // Block ids are per world (a server's registries), so the colour table is rebuilt.
            self.table = None;
            self.table_vanilla = None;
            engine::region::set_block_stamp(0);
            self.pal_hash = 0;
            self.pal_rx = None;
            self.emit_done = false;
            self.tex.clear();
            self.follow.reset();
            self.table_build = ColorTable::default();
            self.table_next = 0;
            self.table_empty_run = 0;
        }
        self.dim = dim.to_string();
        self.trail.clear();
        if let Some(t) = self.trail_path().and_then(|p| std::fs::read_to_string(p).ok()) {
            self.trail.from_text(&t);
        }
        self.trail_saved = self.trail.generation;
        let dir = self.region_dir();
        if let Some(d) = &dir {
            let _ = std::fs::create_dir_all(d);
        }
        self.surface.reset(dir);
        self.cave.reset(None);
        self.cave.bucket = i32::MIN;
        self.pending.clear();
        self.far_reset();
        self.mini_key = None;
        self.world_key = None;
        self.ui.dimension = self.dim.clone();
        self.view_dim = Some(String::new());
        self.set_view_dim(None);
    }

    // ---- baking ----

    fn build_table(&mut self, host: &dyn Host) {
        if self.table.is_some() {
            return;
        }
        let end = self.table_next + 8192;
        let mut any = false;
        for id in self.table_next..end {
            if let Some((mc, rgb)) = host.block_map_color(BlockStateId(id)) {
                self.table_build.set(id, mc, rgb);
                any = true;
            }
        }
        self.table_next = end;
        self.table_empty_run = if any { 0 } else { self.table_empty_run + 1 };
        // A whole empty slice past the registry ends the table (or 2M ids as a hard stop).
        if self.table_empty_run >= 1 || self.table_next >= 1 << 21 {
            let t = Arc::new(std::mem::take(&mut self.table_build));
            // Block ids saved with the regions are read back only under this same registry.
            engine::region::set_block_stamp(t.fingerprint());
            self.table_vanilla = Some(t.clone());
            self.table = Some(t);
            self.pal_hash = 0;
            self.surface.reset(self.surface.dir.clone());
            self.cave.reset(None);
        }
    }

    fn schedule(&mut self, host: &dyn Host) {
        let (Some(table), Some(worker)) = (self.table.clone(), self.worker.as_ref()) else { return };
        let biomes_changed = match &self.biomes {
            Some(b) => host.client_biomes_changed(b),
            None => true,
        };
        if biomes_changed {
            self.biomes = host.client_biomes();
            self.tints = None;
            // New tints: everything baked is stale (the pixels stay until rebaked).
            self.surface.baked.clear();
            self.cave.baked.clear();
        }
        let p = self.block_pos();
        let (pcx, pcz) = (p[0] >> 4, p[2] >> 4);
        let slice = self.cave_slice();
        let cave_on = slice.is_some();
        let indoor = self.indoor_slice().is_some();
        if let Some(top) = slice {
            // The slice key folds the window depth in: another depth is another bake; indoors is another bake too.
            let key = (top * 64 + self.settings.cave_depth as i32) * 2 + indoor as i32;
            if key != self.cave.bucket {
                self.cave.reset(None);
                self.cave.bucket = key;
            }
        }
        let window = slice.map(|top| layers::cave_window(top, self.settings.cave_depth as i32));
        for (cave, layer, radius) in [(false, &mut self.surface, SURFACE_RADIUS), (true, &mut self.cave, CAVE_RADIUS)] {
            if cave && !cave_on {
                continue;
            }
            let Ok(snap) = host.client_chunks([pcx - radius, pcz - radius], [pcx + radius, pcz + radius]) else { continue };
            if let Some(old) = layer.snap.take() {
                for c in host.client_chunks_changed(&old) {
                    layer.baked.remove(&c);
                }
            }
            let mut todo: Vec<[i32; 2]> = Vec::new();
            for cz in pcz - radius..=pcz + radius {
                for cx in pcx - radius..=pcx + radius {
                    let c = [cx, cz];
                    // A chunk is only worth baking with its north neighbour loaded too (the slope shading), except
                    // at the snapshot's edge.
                    if snap.loaded([cx * 16, cz * 16]) && !layer.baked.contains(&c) && !layer.queued.contains(&c) {
                        todo.push(c);
                    }
                }
            }
            todo.sort_unstable_by_key(|c| (c[0] - pcx).pow(2) + (c[1] - pcz).pow(2));
            todo.truncate(BAKE_BATCH);
            if !todo.is_empty() {
                layer.queued.extend(todo.iter().copied());
                let lay = match window {
                    Some((top, _)) if cave && indoor => Layer::Indoor(top),
                    Some((top, floor)) if cave => Layer::Cave(top, floor),
                    _ => Layer::Surface,
                };
                let _ = worker.tx.send(Job::Bake {
                    epoch: layer.epoch.load(Ordering::Relaxed),
                    cave,
                    chunks: snap.clone(),
                    biomes: self.biomes.clone(),
                    table: table.clone(),
                    list: todo,
                    layer: lay,
                });
            }
            if layer.baked.len() > 20000 {
                // Far behind the player: forget, they rebake if the player comes back.
                layer.baked.retain(|c| (c[0] - pcx).abs() <= radius * 2 && (c[1] - pcz).abs() <= radius * 2);
            }
            layer.snap = Some(snap);
        }
    }

    fn receive(&mut self) {
        let Some(worker) = self.worker.as_ref() else { return };
        while let Ok(out) = worker.rx.try_recv() {
            match out {
                Out::Baked { epoch, cave, chunks, attempted } => {
                    let layer = if cave { &mut self.cave } else { &mut self.surface };
                    if layer.epoch.load(Ordering::Relaxed) != epoch {
                        continue;
                    }
                    for c in &attempted {
                        layer.queued.remove(c);
                    }
                    for c in chunks {
                        layer.baked.insert([c.cx, c.cz]);
                        self.pending.push_back((cave, c));
                    }
                }
                Out::Loaded { epoch, layer, key, region, failed } => {
                    let layer = lay_mut!(self, layer);
                    if layer.epoch.load(Ordering::Relaxed) != epoch {
                        continue;
                    }
                    match region {
                        Some(r) if (r.rx, r.rz) == key => layer.store.loaded(r),
                        _ if failed => layer.store.load_failed(key, Instant::now()),
                        _ => layer.store.load_missing(key),
                    }
                }
            }
        }
        // Integrate a bounded number per frame so a burst never shows up as a hitch.
        for _ in 0..INTEGRATE_PER_FRAME {
            let Some((cave, c)) = self.pending.pop_front() else { break };
            let layer = if cave { &mut self.cave } else { &mut self.surface };
            let key = Region::of_chunk(c.cx, c.cz);
            layer.store.put(&c);
            // A region first seen through a bake still has to merge its file before it is ever saved over it.
            request_load(layer, self.worker.as_ref(), key);
        }
    }

    // ---- images ----

    fn ensure_buf(&mut self) {
        if self.buf.len() < MOD_IMAGE_SIZE * MOD_IMAGE_SIZE * 4 {
            self.buf = vec![0; MOD_IMAGE_SIZE * MOD_IMAGE_SIZE * 4];
        }
    }

    /// Asks for visible regions from disk, keeps them warm in the LRU and builds the mips the zoom needs.
    fn prepare(&mut self, sel: u8, v: &View, w: usize, h: usize) {
        // Every zoom, pan, resize and still frame asks again for what the view shows, nearest first (the far
        // ones only while the view stays within `VISIBLE_CAP`); the stores grow to hold all of it, so a far zoom
        // never evicts what it shows (v0.3.5: the list was cut to 40 nearest while the far store held 48).
        let shown = store::Store::plan(v, w, h, &mut self.visible);
        let mip = view::mip_for(v.bpp);
        let visible = std::mem::take(&mut self.visible);
        for id in [self.far_for(sel), Some(sel)].into_iter().flatten() {
            let worker = self.worker.take();
            let layer = self.layer_mut(id);
            layer.store.fit_visible(visible.len());
            layer.store.shown = Some(shown);
            for &k in &visible {
                request_load(layer, worker.as_ref(), k);
                if layer.store.regions.contains_key(&k) {
                    layer.store.touch(k);
                    if mip > 0 {
                        if let Some(r) = layer.store.regions.get_mut(&k) {
                            r.ensure_mips();
                        }
                    }
                }
            }
            self.worker = worker;
        }
        self.visible = visible;
    }

    fn pal(&self) -> Pal {
        // Auto: light by day, dark from dusk on, unless the interface is pinned (`ui_fixed`, default on).
        let day = clouds::daylight(self.day_time).0;
        if style::ui_light(self.settings.ui_theme, self.settings.ui_fixed, day, self.weather.0, self.weather.1) { LIGHT } else { DARK }
    }

    /// Advances the reveal fronts of the surface and the cave slice (`reveal::Front`) by the time since the last
    /// frame; whether any ground is still coming out of the fog. The reveal setting off lets it all out at once.
    fn step_fronts(&mut self, now: Instant, player: [f64; 2]) -> bool {
        let dt = self.front_t.map_or(0.0, |t| now.saturating_duration_since(t).as_secs_f32());
        self.front_t = Some(now);
        let speed = reveal::speed(self.settings.reveal).filter(|_| self.settings.fog);
        // The cave slice tracks new ground once it has settled after a rebuild (a new slice is all news).
        let settled = self.cave.since.elapsed() > CAVE_SETTLE;
        if self.cave.store.track_fresh != settled {
            self.cave.store.track_fresh = settled;
            self.cave.store.front.clear();
        }
        let soft = reveal::soft_for(self.mini_bpp.max(self.ui.bpp));
        let mut busy = false;
        for layer in [&mut self.surface, &mut self.cave] {
            let st = &mut layer.store;
            if st.front.is_empty() {
                continue;
            }
            let Some(sp) = speed else {
                st.front.clear();
                busy = true;
                continue;
            };
            let mut front = std::mem::take(&mut st.front);
            busy |= front.step(dt, player, sp, soft, |cx, cz| st.chunk_explored(cx, cz));
            st.front = front;
        }
        busy
    }

    /// Whether the cloud fog draws over this layer now.
    fn clouds_on(&self, cave: bool) -> bool {
        self.settings.fog && self.settings.clouds && !cave
    }

    fn cloud_params(&self) -> clouds::Params {
        // The look follows the dimension the map shows (or the one picked in the settings).
        let mut p = self.settings.cloud_params_for(self.viewed_dim());
        (p.rain, p.thunder) = self.weather;
        p.day_time = self.day_time;
        p
    }

    /// Composes the image for `key` into `self.buf` (`w` x `h`): the map, then, with the cloud fog on, the clouds
    /// at their own frame rate over a kept copy of the map. Returns false when nothing changed since the last
    /// upload (same key, and no cloud frame due). `mini`: the minimap (its own cloud reach).
    #[allow(clippy::too_many_arguments)]
    fn compose(&mut self, key: ImageKey, shown: bool, v: &View, w: usize, h: usize, sel: u8, round: bool, mini: bool) -> bool {
        let n = w * h * 4;
        let cloudy = self.clouds_on(sel == 1);
        self.ensure_buf();
        let far = self.far_for(sel).map(|f| &lay!(self, f).store);
        let layer = &lay!(self, sel);
        let tex = v.style.textures.then(|| (layer.snap.as_ref(), &self.tex));
        // The ramp behind the front spans at least three pixels, so it never shows a hard line at a far zoom.
        let soft = reveal::soft_for(v.bpp);
        let look = Look { store: &layer.store, far, reveal: Some(soft), tex };
        if !cloudy {
            if shown {
                return false;
            }
            let t0 = Instant::now();
            view::render(&mut self.buf, w, h, v, &look);
            self.drawn(t0);
            if v.style.mark_unknown {
                force_opaque(&mut self.buf[..n]);
            }
            self.base_key = None;
            return true;
        }
        // The hard reach is in world blocks, so it holds at any zoom.
        let p = clouds::Params { reach: self.settings.reach_blocks(mini) as f32, ..self.cloud_params() };
        // 0 = a cloud frame with every game frame, but no faster than the deck visibly moves at this zoom.
        let tick = match self.settings.cloud_fps {
            0 => auto_cloud_tick(v.bpp, &p),
            f => Duration::from_micros(1_000_000 / f.clamp(5, 240) as u64),
        };
        let fresh = self.base_key != Some(key);
        // Fog thinning out over far terrain that has just arrived: at least 30 cloud frames a second meanwhile.
        let tick = if self.clouds.easing() { tick.min(Duration::from_millis(33)) } else { tick };
        if !fresh && shown && self.last_cloud.elapsed() < tick {
            return false;
        }
        // Same layer and look, only the view moved: a pan or zoom frame.
        let moved = fresh
            && self.base_key.is_some_and(|b| {
                b.layer_gen == key.layer_gen && b.size == key.size && b.flags == key.flags && b.view != key.view
            });
        if fresh && !moved && self.base_key.is_some_and(|b| b.layer_gen != key.layer_gen || b.flags != key.flags) {
            // Another layer or world: the kept explored-flags outside the image no longer apply.
            self.clouds.forget();
        }
        if fresh {
            if self.base.len() < MOD_IMAGE_SIZE * MOD_IMAGE_SIZE * 4 {
                self.base = vec![0; MOD_IMAGE_SIZE * MOD_IMAGE_SIZE * 4];
            }
            let t0 = Instant::now();
            view::render(&mut self.base, w, h, v, &look);
            // Only a running reveal front puts ramp alphas into the image.
            if !layer.store.front.is_empty() {
                view::settle_revealing(&mut self.base[..n]);
            }
            self.drawn(t0);
            self.base_key = Some(key);
        }
        self.last_cloud = Instant::now();
        let t = if moved && self.cloud_t.1.elapsed() < DRAG_TICK.max(tick) {
            self.cloud_t.0
        } else {
            let t = self.t0.elapsed().as_secs_f32();
            self.cloud_t = (t, Instant::now());
            t
        };
        self.buf[..n].copy_from_slice(&self.base[..n]);
        if self.clouds.update(&self.base[..n], w, h, v, t, &p) {
            self.clouds.composite(&mut self.buf[..n], w, h, &p, t);
        } else {
            force_opaque(&mut self.buf[..n]);
        }
        if round {
            mask_round(&mut self.buf[..n], w, h);
        }
        true
    }

    /// Notes a map draw started at `t0` (`data_tick`).
    fn drawn(&mut self, t0: Instant) {
        let ms = t0.elapsed().as_secs_f32() * 1e3;
        self.render_ms = if self.render_ms == 0.0 { ms } else { self.render_ms * 0.75 + ms * 0.25 };
        self.base_drawn = Instant::now();
    }

    /// The image to draw for `key` when `shown` (the key on screen) is in this slot: `shown` itself while only
    /// the map data changed and the last draw is younger than the data tick, so streaming data redraws a few
    /// times a second instead of every frame. A pan, zoom, fade or look change draws at once.
    fn data_throttled(&self, key: ImageKey, shown: Option<ImageKey>) -> ImageKey {
        let tick = Duration::from_secs_f32(self.render_ms * RENDER_SHARE / 1e3).clamp(DATA_TICK_MIN, DATA_TICK_MAX);
        match shown {
            Some(old) if key.data_only(&old) && self.base_drawn.elapsed() < tick => old,
            _ => key,
        }
    }

    fn style_flags(&self, cave: bool) -> u32 {
        let st = &self.settings;
        (st.relief as u32) << 8
            | (st.contours as u32) << 10
            | (st.grid as u32) << 16
            | (if st.day_night { self.light / 8 } else { 0 }) << 18
            | (self.clouds_on(cave) as u32) << 24
            | (st.far as u32) << 25
    }

    /// Whether the minimap layer has its ground around the player: the surface always; the cave slice once the
    /// bake it was reset for has come back and been taken in (a layer switch holds the old image until then).
    fn layer_ready(&self, cave: bool) -> bool {
        !cave || (self.cave.queued.is_empty() && !self.pending.iter().any(|(c, _)| *c) && self.cave.since.elapsed() > Duration::from_millis(100))
    }

    fn render_minimap(&mut self, host: &mut dyn Host) {
        let s = self.settings.size_px() as usize;
        let px = (s * 2).min(MOD_IMAGE_SIZE);
        let rotation = if self.settings.north_lock { 0.0 } else { (self.yaw as f64).to_radians() + std::f64::consts::PI };
        let slice = self.cave_slice();
        let cave = slice.is_some();
        // A layer switch: keep the last minimap image to hold it over the new layer and fade it out.
        if self.shown_slice.is_some_and(|old| old != slice) && self.slot == Slot::Mini && self.mini_px == px && self.buf.len() >= px * px * 4 {
            if let Some(view) = self.mini_view {
                let mut old = self.fade_from.take().map(|f| f.old).unwrap_or_default();
                old.clear();
                old.extend_from_slice(&self.buf[..px * px * 4]);
                self.fade_from = Some(LayerFade { old, view, since: Instant::now(), start: None });
            }
        }
        self.shown_slice = Some(slice);
        let ready = self.layer_ready(cave);
        let fading = self.fade_from.as_mut().map(|f| {
            if f.start.is_none() && (ready || f.since.elapsed().as_secs_f32() >= layers::HOLD_MAX_S) {
                f.start = Some(Instant::now());
            }
            f.start.map_or(0.0, |t| layers::fade(t.elapsed().as_secs_f32()))
        });
        if fading.is_some_and(|k| k >= 1.0) {
            self.fade_from = None;
        }
        let cloudy = self.clouds_on(cave);
        let round = self.settings.shape == Shape::Round;
        self.mini_bpp = self.minimap_bpp(s, cave);
        let v = View {
            center: [self.player[0], self.player[2]],
            bpp: self.mini_bpp * s as f64 / px as f64,
            rotation,
            // The cloud pass needs the whole square; the circle is cut afterwards.
            round: round && !cloudy,
            fog: self.settings.fog,
            style: self.settings.style(self.light, cloudy),
        };
        let sel = cave as u8;
        self.prepare(sel, &v, px, px);
        let layer = self.layer(sel);
        // Eighth of a pixel steps, so a gliding map redraws smoothly at any zoom.
        let q = 8.0 / v.bpp.max(1e-3);
        let key = ImageKey {
            anim: self.anim,
            store_gen: layer.store.generation,
            far_gen: self.far_for(sel).map_or(0, |f| self.layer(f).store.generation),
            layer_gen: layer.epoch.load(Ordering::Relaxed),
            view: [(v.center[0] * q) as i64, (v.center[1] * q) as i64, (rotation * 4000.0) as i64, (v.bpp * 1000.0) as i64],
            size: [px, px],
            flags: round as u32 | (v.fog as u32) << 1 | self.settings.opacity << 2 | self.style_flags(cave) << 9,
        };
        let key = self.data_throttled(key, self.mini_key.filter(|_| self.slot == Slot::Mini));
        let shown = self.slot == Slot::Mini && self.mini_key == Some(key);
        // Optional cap; by default the minimap follows the game's frame rate.
        if self.slot == Slot::Mini && self.settings.mini_fps > 0 && self.last_mini.elapsed() < Duration::from_micros(1_000_000 / self.settings.mini_fps as u64) {
            return;
        }
        if !self.compose(key, shown && self.fade_from.is_none(), &v, px, px, sel, round && cloudy, true) {
            return;
        }
        let n = px * px * 4;
        if self.settings.opacity < 100 {
            let o = self.settings.opacity;
            for a in self.buf[..n].iter_mut().skip(3).step_by(4) {
                *a = (*a as u32 * o / 100) as u8;
            }
        }
        if let (Some(f), Some(k)) = (&self.fade_from, fading) {
            if f.old.len() == n {
                layers::crossfade_warped(&mut self.buf[..n], px, px, &v, &f.old, &f.view, k);
            }
        }
        if host.mod_image(px as u32, px as u32, &self.buf[..n]) {
            self.slot = Slot::Mini;
            self.mini_key = Some(key);
            self.mini_view = Some(v);
            self.mini_px = px;
            self.last_mini = Instant::now();
        }
    }

    /// Makes a settings preview frame when one is due (at most `preview::FPS` a second): the minimap look (tab 0)
    /// or a map fragment (tab 1) around the player from the real map data, with its own clouds under the weather
    /// picked in the preview.
    fn render_preview(&mut self) {
        let now = self.t0.elapsed().as_secs_f64();
        if !preview::due(self.preview.last, now) {
            return;
        }
        let Some(p) = self.ui.panel.as_ref() else { return };
        let (tab, weather) = (p.tab, p.preview_weather);
        self.preview.last = now;
        let t_start = Instant::now();
        let n = preview::SIZE;
        let (sel, bpp, rotation, round) = if tab == 0 {
            let s = self.settings.size_px() as usize;
            let cave = self.cave_active();
            let rot = if self.settings.north_lock { 0.0 } else { (self.yaw as f64).to_radians() + std::f64::consts::PI };
            // The same ground the minimap shows, squeezed into the preview.
            (cave as u8, self.minimap_bpp(s, cave) * s as f64 / n as f64, rot, self.settings.shape == Shape::Round)
        } else {
            (self.world_sel(), self.ui.bpp, 0.0, false)
        };
        let cloudy = self.clouds_on(sel == 1);
        let v = View {
            center: [self.player[0], self.player[2]],
            bpp,
            rotation,
            round: false,
            fog: self.settings.fog,
            style: self.settings.style(self.light, cloudy),
        };
        let mut buf = std::mem::take(&mut self.preview.buf);
        {
            let far = self.far_for(sel).map(|f| &lay!(self, f).store);
            let look = Look::plain(&lay!(self, sel).store, far);
            view::render(&mut buf, n, n, &v, &look);
        }
        if cloudy {
            preview::fog_edge(&mut buf, n);
            let base = buf.clone();
            let st = &self.settings;
            // Tab 0 previews the minimap, so its reach; the map tabs, the map's.
            let base_p = clouds::Params { reach: st.reach_blocks(tab == 0) as f32, ..self.cloud_params() };
            let cp = preview::weather_params(base_p, weather, st.nether_intensity as f32 / 100.0, st.end_intensity as f32 / 100.0);
            let t = now as f32;
            if self.preview.clouds.update(&base, n, n, &v, t, &cp) {
                self.preview.clouds.composite(&mut buf, n, n, &cp, t);
            } else {
                force_opaque(&mut buf);
            }
        } else if v.style.mark_unknown {
            force_opaque(&mut buf);
        }
        if round {
            mask_round(&mut buf, n, n);
        }
        if tab == 0 && self.settings.opacity < 100 {
            let o = self.settings.opacity;
            for a in buf.iter_mut().skip(3).step_by(4) {
                *a = (*a as u32 * o / 100) as u8;
            }
        }
        self.preview.buf = buf;
        self.preview.dirty = true;
        self.preview.bpp = bpp;
        self.preview.rot = rotation;
        let ms = t_start.elapsed().as_secs_f32() * 1e3;
        self.preview.ms = if self.preview.ms == 0.0 { ms } else { self.preview.ms * 0.95 + ms * 0.05 };
    }

    /// Uploads a pending preview frame alone (a small patch over the top rows), when the image already holds the
    /// world map below the preview rows.
    fn upload_preview(&mut self, host: &mut dyn Host) {
        let n = preview::SIZE;
        if self.preview.dirty && self.slot == Slot::World && self.preview.world_off == n && host.mod_image(n as u32, n as u32, &self.preview.buf) {
            self.preview.dirty = false;
        }
    }

    fn render_world(&mut self, host: &mut dyn Host) {
        let (w, h) = (self.ui.screen.w.max(1.0), self.ui.screen.h.max(1.0));
        // The settings preview takes the top rows of the image while the settings window shows it.
        let pv = self.ui.settings_preview_image().is_some();
        if pv {
            self.render_preview();
        }
        let off = if pv { preview::SIZE } else { 0 };
        // One mod image of at most 1024 x 1024 (Horizon's limit): a big GUI is drawn from fewer pixels than it has.
        let s = (MOD_IMAGE_SIZE as f32 / w).min((MOD_IMAGE_SIZE - off) as f32 / h).min(2.0);
        let (iw, ih) = (((w * s) as usize).clamp(1, MOD_IMAGE_SIZE), ((h * s) as usize).clamp(1, MOD_IMAGE_SIZE - off));
        let sel = self.world_sel();
        let cave = sel == 1;
        let cloudy = self.clouds_on(cave);
        let v = View {
            center: self.ui.center,
            bpp: self.ui.bpp * w as f64 / iw as f64,
            rotation: 0.0,
            round: false,
            fog: self.settings.fog,
            style: self.settings.style(self.light, cloudy),
        };
        self.prepare(sel, &v, iw, ih);
        let layer = self.layer(sel);
        let q = 4.0 / v.bpp.max(1e-3);
        let key = ImageKey {
            anim: self.anim,
            store_gen: layer.store.generation,
            far_gen: self.far_for(sel).map_or(0, |f| self.layer(f).store.generation),
            layer_gen: layer.epoch.load(Ordering::Relaxed) << 3 | sel as u64,
            view: [(v.center[0] * q) as i64, (v.center[1] * q) as i64, 0, (v.bpp * 10000.0) as i64],
            size: [iw, ih + off],
            flags: v.fog as u32 | self.style_flags(cave) << 9,
        };
        let key = self.data_throttled(key, self.world_key.filter(|_| self.slot == Slot::World));
        let shown = self.slot == Slot::World && self.world_key == Some(key);
        let fresh = self.world_key != Some(key);
        if shown && self.settings.map_fps > 0 && self.last_world.elapsed() < Duration::from_micros(1_000_000 / self.settings.map_fps as u64) {
            self.upload_preview(host);
            return;
        }
        if !self.compose(key, shown, &v, iw, ih, sel, false, false) {
            self.upload_preview(host);
            return;
        }
        if fresh {
            let far = self.far_for(sel).map(|f| &self.layer(f).store);
            self.explored = view::explored_percent(&v, iw, ih, &Look::plain(&self.layer(sel).store, far));
        }
        let ok = if off == 0 {
            host.mod_image(iw as u32, ih as u32, &self.buf[..iw * ih * 4])
        } else {
            // Preview rows on top, the world map under them, in one upload.
            let (n, ww) = (preview::SIZE, iw.max(preview::SIZE));
            let mut combo = std::mem::take(&mut self.preview.combo);
            combo.clear();
            combo.resize(ww * (off + ih) * 4, 0);
            for y in 0..n {
                combo[y * ww * 4..y * ww * 4 + n * 4].copy_from_slice(&self.preview.buf[y * n * 4..(y + 1) * n * 4]);
            }
            for y in 0..ih {
                let o = (off + y) * ww * 4;
                combo[o..o + iw * 4].copy_from_slice(&self.buf[y * iw * 4..(y + 1) * iw * 4]);
            }
            let ok = host.mod_image(ww as u32, (off + ih) as u32, &combo);
            self.preview.combo = combo;
            ok
        };
        if ok {
            self.preview.world_off = off;
            self.preview.dirty = false;
            self.slot = Slot::World;
            self.world_key = Some(key);
            self.world_img = (v.center, self.ui.bpp, [w, h], [iw as f32, ih as f32]);
            self.last_world = Instant::now();
        }
    }

    // ---- per-frame upkeep ----

    fn info_lines(&mut self, host: &dyn Host) {
        self.info.clear();
        let p = self.block_pos();
        let l = self.lang();
        let w = |k: &str| lang::builtin(l, k).unwrap_or("");
        // In the order the player set, only the switched-on lines.
        for (kind, on) in self.settings.info.clone() {
            if !on {
                continue;
            }
            match kind {
                Info::Coords => self.info.push(format!("{} {} {}", p[0], p[1], p[2])),
                Info::Biome => {
                    if let Some(b) = host.biome(p) {
                        let b = self.tname("biome", &b);
                        self.info.push(b);
                    }
                }
                Info::Time => {
                    let t = host.day_time().rem_euclid(24000);
                    // "Day 2480 · 14:21"; the period word only for morning, evening and night ("День ... День" before).
                    let period = (!(3000..11000).contains(&t)).then(|| lang::time_of_day(l, t as u32));
                    self.info.push(infoblock::time_line(w("megamap.day"), host.day_time().div_euclid(24000) + 1, t, period));
                }
                Info::Light => {
                    let (block, sky) = host.light(p);
                    self.info.push(format!("{} {block} ({} {sky})", w("megamap.light"), w("megamap.sky")));
                }
                Info::Facing => {
                    // Yaw 0 looks south (+z), 90 west.
                    let y = (self.yaw as f64).rem_euclid(360.0);
                    let n = w(COMPASS[(((y + 22.5) / 45.0) as usize) % 8]);
                    self.info.push(format!("{} {n} ({y:.0})", w("megamap.facing")));
                }
                Info::Dimension => {
                    let d = self.tname("dimension", &self.dim.clone());
                    self.info.push(d);
                }
            }
        }
    }

    fn apply(&mut self, host: &mut dyn Host, q: Queued) {
        match q {
            Queued::OpenMap { list, new_wp } => {
                if host.mod_screen_open(ID) {
                    self.map_open = true;
                    self.ui.screen = Rect::new(0.0, 0.0, self.gui[0], self.gui[1]);
                    self.ui.open(self.player, &self.dim, list);
                    self.map_opened = Instant::now();
                    if new_wp {
                        let pos = self.block_pos();
                        self.ui.new_waypoint(&self.waypoints, pos);
                    }
                    self.world_key = None;
                }
            }
            Queued::CloseMap => {
                host.mod_screen_close(ID);
                self.map_open = false;
            }
            Queued::Ui(a) => match a {
                Action::None => {}
                Action::Close => {
                    host.mod_screen_close(ID);
                    self.map_open = false;
                }
                Action::ChatPrefill(text) => {
                    // The chat box only opens with no other screen up, so the map goes first.
                    host.mod_screen_close(ID);
                    self.map_open = false;
                    if let Err(e) = host.open_chat(&text) {
                        self.toast(format!("{text}  ({e})"));
                    }
                }
                Action::SaveWaypoint(i, w) => {
                    let name = w.name.clone();
                    match i {
                        Some(i) if i < self.waypoints.list.len() => self.waypoints.replace(i, w),
                        _ => {
                            self.waypoints.add(w);
                        }
                    }
                    self.toast(self.tr().f("megamap.toast.wp_saved", &[&name]));
                }
                Action::DeleteWaypoint(i) => {
                    if let Some(w) = self.waypoints.remove(i) {
                        self.toast(self.tr().f("megamap.toast.wp_deleted", &[&w.name]));
                    }
                }
                Action::ShareWaypoint(i) => {
                    if let Some(w) = self.waypoints.list.get(i) {
                        let text = w.share_text();
                        if let Err(e) = host.send_chat(&text) {
                            self.toast(self.tr().f("megamap.toast.share_failed", &[&e]));
                        }
                    }
                }
                Action::ToggleCave => {
                    // Auto -> Surface -> Cave -> Fixed Y -> Auto.
                    self.settings.cycle_layer();
                    self.save_settings();
                    self.world_key = None;
                    self.mini_key = None;
                    let tr = self.tr();
                    let m = self.settings.layer_mode();
                    let mut t = tr.f("megamap.toast.layer", &[&tr.t(&format!("megamap.layer.{}", m.key()))]);
                    if m == LayerMode::FixedY {
                        t += &format!(" {}", self.settings.fixed_y);
                    }
                    self.toast(t);
                }
                Action::ToggleTrail => {
                    self.settings.trail = !self.settings.trail;
                    self.save_settings();
                }
                Action::ToggleClouds => {
                    self.settings.clouds = !self.settings.clouds;
                    if self.settings.clouds && !self.settings.fog {
                        self.settings.fog = true;
                    }
                    self.save_settings();
                    self.world_key = None;
                    self.toast(self.tr().t(if self.settings.clouds { "megamap.toast.clouds_on" } else { "megamap.toast.clouds_off" }));
                }
                Action::CycleRelief => {
                    self.settings.relief = (self.settings.relief + 1) % 4;
                    self.save_settings();
                    self.world_key = None;
                    let tr = self.tr();
                    self.toast(tr.f("megamap.toast.relief", &[&tr.t(&format!("megamap.relief.{}", self.settings.relief))]));
                }
                Action::CycleGrid => {
                    self.settings.grid = (self.settings.grid + 1) % 4;
                    self.save_settings();
                    self.world_key = None;
                    let tr = self.tr();
                    self.toast(tr.f("megamap.toast.grid", &[&tr.t(&format!("megamap.grid.{}", self.settings.grid))]));
                }
                Action::SettingClick(i) => {
                    self.settings.click(i);
                    if i == settings::AUTOZOOM_FIRST {
                        self.autozoom.reset();
                    }
                    self.save_settings();
                    self.mini_key = None;
                    self.world_key = None;
                    self.base_key = None;
                }
                Action::SettingSlider(i, f) => {
                    if let Some((_, steps)) = self.settings.slider(i) {
                        self.settings.set_slider(i, settings::slider_step(f, steps));
                        self.save_settings();
                        self.mini_key = None;
                        self.world_key = None;
                    }
                }
                Action::InfoMove(pos, up) => {
                    if self.settings.info_move(pos, up) {
                        self.save_settings();
                    }
                }
                Action::EntityEdit(kind, field) => {
                    let mut st = self.settings.entity_style(&kind);
                    let cycle = |v: u8, n: usize| ((v as usize + 1) % n) as u8;
                    match field {
                        ui::EntityField::Mini => st.mini = !st.mini,
                        ui::EntityField::Map => st.map = !st.map,
                        ui::EntityField::Color => {
                            let i = entities::COLORS.iter().position(|&c| c == st.color & 0xFF_FFFF);
                            st.color = entities::COLORS[i.map_or(0, |i| (i + 1) % entities::COLORS.len())];
                        }
                        ui::EntityField::Icon => st.icon = cycle(st.icon, entities::ICONS.len()),
                        ui::EntityField::Size => st.size = cycle(st.size, entities::ICON_PX.len()),
                        ui::EntityField::Reset => st = entities::default_style(&kind),
                    }
                    self.settings.set_entity_style(&kind, st);
                    self.save_settings();
                }
                Action::SettingsCat(c) => {
                    if self.settings.ui_cat as usize != c {
                        self.settings.ui_cat = c as u8;
                        self.save_settings();
                    }
                }
                Action::ResetCategory(c) => {
                    self.settings.reset_category(c);
                    self.save_settings();
                    self.autozoom.reset();
                    self.mini_key = None;
                    self.world_key = None;
                    self.base_key = None;
                    let tr = self.tr();
                    self.toast(tr.f("megamap.toast.reset_cat", &[&tr.t(settings::CATEGORIES[c].name)]));
                }
                Action::ViewDimension(d) => {
                    self.set_view_dim(d);
                    let shown = waypoints::short_dim(self.viewed_dim()).to_string();
                    self.toast(self.tr().f("megamap.toast.map_dim", &[&shown]));
                }
            },
        }
    }

    fn frame(&mut self, host: &mut dyn Host) {
        if !self.started {
            self.start(host);
        }
        let now = Instant::now();
        let state = host.client_state();
        let player = host.player();
        let (Some(state), Some(player)) = (state, player) else {
            if self.in_world {
                self.leave();
            }
            return;
        };
        self.in_world = true;
        self.gui = state.gui_size;
        self.gui_hidden = state.gui_hidden;
        let glide = if self.settings.smooth { self.follow.update([player.x, player.z], now) } else { [player.x, player.z] };
        self.player = [glide[0], player.y, glide[1]];
        self.yaw = player.yaw;
        // New ground coming out of the fog redraws the map every frame until the front has passed it.
        if self.step_fronts(now, [player.x, player.z]) {
            self.anim += 1;
        }
        self.ui.player = self.player;
        let tr = self.tr();
        if tr.lang != self.ui.tr.lang {
            self.ui.wp_name = tr.t("megamap.toast.wp_default");
        }
        self.ui.tr = tr;
        let world_id = state.world.as_ref().map(world_key);
        if world_id != self.world_id || player.dimension != self.dim {
            self.enter(world_id, player.dimension);
        }
        if self.map_open && host.mod_screen() != Some(ID) {
            // Escape (or the game) closed it.
            self.map_open = false;
            self.ui.edit = None;
            self.ui.menu = None;
        }
        // Death waypoint on the edge into the death screen.
        let dead = state.screen == NativeScreenKind::Death;
        if dead && !self.was_dead {
            let day = host.day_time().div_euclid(24000) + 1;
            let pos = self.block_pos();
            let dim = self.dim.clone();
            let tr = self.tr();
            self.waypoints.add_death(pos, &dim, self.settings.max_deaths, &tr.f("megamap.toast.death_name", &[&day]));
            self.toast(tr.f("megamap.toast.death", &[&pos[0], &pos[1], &pos[2]]));
        }
        self.was_dead = dead;
        if self.settings.trail && !dead {
            // Saved with the waypoints by persist_small.
            let p = self.block_pos();
            self.trail.push(p);
        }

        self.build_table(host);
        self.receive();
        // The layer probe on its own, faster clock (0.3.11: it came with the chunk snapshot twice a second, so the
        // minimap took up to a second and a half to notice a doorway).
        if self.in_world && now.duration_since(self.last_probe) >= PROBE_EVERY {
            self.last_probe = now;
            self.probe_layer(host);
        }
        if now.duration_since(self.last_snapshot) >= Duration::from_millis(500) {
            self.last_snapshot = now;
            self.schedule(host);
        }
        self.day_time = host.day_time().rem_euclid(24000) as u32;
        self.light = (clouds::daylight(self.day_time).0 * 200.0) as u32 + 56;

        if now.duration_since(self.last_radar) >= Duration::from_millis(250) {
            self.last_radar = now;
            if self.settings.radar {
                let ents = host.client_entities();
                let me = self.player;
                // Every kind is scanned; which ones show (and how) is the per-kind entity style's call.
                self.radar.scan(me, ents.iter().map(|e| (e.kind.as_str(), e.position, e.alive && !e.invisible)), true);
                self.portraits.blip_tex = self.radar.blips.iter().map(|b| ents.get(b.idx).and_then(|e| e.texture.clone())).collect();
                self.portraits.blip_ids = self.radar.blips.iter().map(|b| ents.get(b.idx).map_or(u32::MAX, |e| e.id)).collect();
                let ids = &self.portraits.blip_ids;
                self.portraits.blip_follow.retain(|id, _| ids.contains(id));
                for bl in &self.radar.blips {
                    if self.seen.len() < 512 && !self.seen.iter().any(|k| *k == bl.kind) {
                        self.seen.push(bl.kind.clone());
                    }
                }
            } else {
                self.radar.blips.clear();
                self.portraits.blip_tex.clear();
                self.portraits.blip_ids.clear();
                self.portraits.blip_follow.clear();
                self.radar.hostile_count = 0;
                self.radar.nearest = None;
            }
        }
        self.track_blips(host, now);
        // The info lines follow the player at once (coordinates, facing); the rest (biome, light, time) at 10 Hz.
        let info_key = (self.block_pos(), ((self.yaw as f64).rem_euclid(360.0)) as i32);
        if self.info_key != Some(info_key) || now.duration_since(self.last_info) >= Duration::from_millis(100) {
            self.info_key = Some(info_key);
            self.last_info = now;
            self.info_lines(host);
        }
        if now.duration_since(self.last_keys) >= Duration::from_secs(2) {
            self.last_keys = now;
            self.refresh_keys(host);
        }
        for q in std::mem::take(&mut self.queue) {
            self.apply(host, q);
        }
        self.toasts.retain(|t| t.until > now);

        if self.map_open {
            self.render_world(host);
        } else if self.settings.mode != Mode::Hidden && !self.gui_hidden {
            self.render_minimap(host);
        }

        if now.duration_since(self.last_persist) >= Duration::from_secs(2) {
            self.last_persist = now;
            self.persist_small(false);
        }
        if now.duration_since(self.last_save) >= Duration::from_secs(30) {
            self.last_save = now;
            self.flush_regions();
        }
        if now.duration_since(self.last_evict) >= Duration::from_secs(10) {
            self.last_evict = now;
            self.evict();
        }
    }

    /// Moves the radar blips every frame (v0.3.5 fix: they took the entity positions of the last scan, 4 a
    /// second, so a walking mob jumped a block at a time over a map that glides with the player). The scan still
    /// picks who shows; here each blip reads its entity's position again and goes through the same `Follow` as
    /// the player's own position, so icons and map move alike, in the same frame.
    fn track_blips(&mut self, host: &dyn Host, now: Instant) {
        let visible = self.map_open || (self.settings.mode != Mode::Hidden && !self.gui_hidden);
        if !self.settings.radar || self.radar.blips.is_empty() || !visible {
            return;
        }
        let ents = host.client_entities();
        let smooth = self.settings.smooth;
        let pt = &mut self.portraits;
        for (i, bl) in self.radar.blips.iter_mut().enumerate() {
            let Some(&id) = pt.blip_ids.get(i) else { break };
            let e = ents.get(bl.idx).filter(|e| e.id == id).or_else(|| ents.iter().find(|e| e.id == id));
            let raw = match e {
                Some(e) => {
                    bl.idx = ents.iter().position(|x| std::ptr::eq(x, e)).unwrap_or(bl.idx);
                    bl.y = e.position[1];
                    [e.position[0], e.position[2]]
                }
                None => [bl.x, bl.z], // gone since the scan: stays put until the next one drops it
            };
            let p = if smooth { pt.blip_follow.entry(id).or_default().update(raw, now) } else { raw };
            bl.x = p[0];
            bl.z = p[1];
        }
    }

    /// Drops the far terrain of the old place (saving it first); the far thread (if any) ends on the epoch change.
    fn far_reset(&mut self) {
        self.far_shared.epoch.fetch_add(1, Ordering::Relaxed);
        if let (Some(dir), Some(w)) = (self.far.dir.clone(), self.worker.as_ref()) {
            let regions = self.far.store.dirty_copies();
            if !regions.is_empty() {
                let _ = w.tx.send(Job::Save { dir, regions });
            }
        }
        let dir = self.far_dir();
        self.far.reset(dir);
        self.far_rx = None;
        self.far_picked = None;
        self.far_live_done.clear();
        self.far_live.restart([i32::MIN / 2, i32::MIN / 2]);
        self.far_live_new = 0;
        self.far_live_wait = None;
        self.base_key = None;
    }

    /// Takes a baked far chunk: into the far layer, whose saved file is read first (merged under it).
    fn far_put(&mut self, b: &BakedChunk) {
        self.far.store.put(b);
        request_load(&mut self.far, self.worker.as_ref(), Region::of_chunk(b.cx, b.cz));
    }

    /// The game-side half of the far terrain, run after `frame` with the game itself: tells the far thread where
    /// to fill (the open map's view, else around the player) and whether it may run the generator, starts it
    /// once its sources are known (looking again every few seconds while there are none), takes its chunks
    /// within a time budget, and bakes columns of the live LOD tiles within another when the game draws far
    /// terrain.
    /// Keeps the colour table in step with the resource packs: on a change of the resources generation, the pack
    /// list, the shader pack or the colour style, a worker averages every state's top texture
    /// (`native_block_states` + `native_client_resources`) and the result replaces the table; the surface and cave
    /// layers rebake with it. No cost while nothing changes (one hash of a few strings).
    fn palette_upkeep(&mut self, game: &crate::game::Game<'_>) {
        if let Some(rx) = &self.pal_rx {
            match rx.try_recv() {
                Ok((hash, table, decoded, ms)) => {
                    self.pal_rx = None;
                    if hash == self.pal_hash {
                        log::info!("megamap: resource pack palette: {decoded} textures in {ms:.0} ms");
                        self.set_table(Arc::new(table));
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => self.pal_rx = None,
                Err(_) => {}
            }
        }
        let loaded = self.tex.loaded;
        self.tex.upkeep(game, self.settings.textures);
        if self.tex.loaded != loaded {
            self.anim += 1;
        }
        let Some(mut vanilla) = self.table_vanilla.clone() else { return };
        if !self.emit_done {
            // Light sources from the block facts, once per table: the bake lights the ground around them.
            self.emit_done = true;
            let states = game.megamap_block_states();
            let mut t = (*vanilla).clone();
            for (id, facts) in states.iter() {
                if facts.emission > 0 {
                    t.set_emit(id.0, facts.emission);
                }
            }
            if t.has_emitters() {
                let t = Arc::new(t);
                let was = self.table.as_ref().is_some_and(|c| Arc::ptr_eq(c, &vanilla));
                self.table_vanilla = Some(t.clone());
                vanilla = t;
                if was {
                    self.set_table(vanilla.clone());
                }
                // The pack palette is made from the vanilla table: again, with the light in it.
                self.pal_hash = 0;
            }
        }
        let packs = &game.settings.resource_packs;
        let shader = &game.settings.horizon.shader_pack;
        let style = self.settings.color_style;
        let hash = palette::pack_hash(game.megamap_block_sprites_version(), packs, shader, style).max(1);
        if hash == self.pal_hash {
            return;
        }
        self.pal_hash = hash;
        // Without the resources generation (a counter of this session): the same packs are the same palette
        // the next time the game starts.
        self.pal_id = palette::pack_hash(0, packs, shader, style);
        if !palette::use_pack(style, !packs.is_empty()) {
            self.pal_rx = None;
            if !self.table.as_ref().is_some_and(|t| Arc::ptr_eq(t, &vanilla)) {
                self.set_table(vanilla);
            }
            return;
        }
        let states = game.megamap_block_states();
        let res = game.megamap_client_resources();
        let shader_on = !shader.is_empty();
        let (tx, rx) = channel();
        self.pal_rx = Some(rx);
        let _ = std::thread::Builder::new().name("megamap-palette".into()).spawn(move || {
            let t0 = Instant::now();
            let mut cache = palette::AverageCache::default();
            let mut samples = Vec::new();
            for (id, _) in states.iter() {
                if vanilla.get(id.0).is_none() {
                    continue;
                }
                let Some(sprites) = states.sprites(id) else { continue };
                let Some(face) = sprites.sides[1].first() else { continue };
                let Some(name) = states.texture(face.sprite) else { continue };
                let Some(rgb) = cache.get(name, |n| res.texture(n).map(|t| (t.width, t.height, t.rgba))) else { continue };
                use horizon_client::mesher::TintSource as T;
                let tint = match face.tint {
                    None => palette::Tint::None,
                    Some(T::Grass) => palette::Tint::Grass,
                    Some(T::Foliage) | Some(T::DryFoliage) => palette::Tint::Foliage,
                    Some(T::Water) => palette::Tint::Water,
                    Some(T::Fixed(c)) => palette::Tint::Fixed(c),
                    Some(_) => palette::Tint::None,
                };
                samples.push(palette::Sample { id: id.0, rgb, tint });
            }
            let table = palette::override_table(&vanilla, &samples, shader_on);
            let _ = tx.send((hash, table, cache.decoded, t0.elapsed().as_secs_f32() * 1e3));
        });
    }

    /// Keeps the far layer in the colours of the palette in use (0.3.11): the far layer's folder remembers the
    /// palette it was baked with (the resource packs, the shader pack, the colour style); under another one its
    /// regions are dropped, in memory and on disk, and the far thread bakes them again. Before, only the loaded
    /// chunks took a pack's colours and the far terrain kept the old ones.
    fn far_palette_sync(&mut self) {
        let Some(dir) = self.far.dir.clone() else { return };
        let want = (dir.clone(), self.pal_id);
        if self.far_pal_synced.as_ref() == Some(&want) {
            return;
        }
        self.far_pal_synced = Some(want);
        let stamp = dir.join("palette");
        let text = self.pal_id.to_string();
        if std::fs::read_to_string(&stamp).is_ok_and(|s| s.trim() == text) {
            return;
        }
        let mut dropped = 0;
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                if e.file_name().to_string_lossy().ends_with(".mmr") && std::fs::remove_file(e.path()).is_ok() {
                    dropped += 1;
                }
            }
        }
        self.far_shared.epoch.fetch_add(1, Ordering::Relaxed);
        self.far.reset(Some(dir.clone()));
        self.far_live_done.clear();
        self.far_live.restart([i32::MIN / 2, i32::MIN / 2]);
        self.base_key = None;
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(&stamp, text);
        log::info!("megamap: far layer rebakes for a new palette ({dropped} region files dropped)");
    }

    /// A new colour table: the surface and cave layers rebake with it (their pixels stay until then).
    fn set_table(&mut self, t: Arc<ColorTable>) {
        self.table = Some(t);
        // The far thread bakes with the table it started with: it starts again, and what the far layer holds in
        // the old colours goes (`far_palette_sync`).
        self.far_shared.epoch.fetch_add(1, Ordering::Relaxed);
        self.far_rx = None;
        self.far_picked = None;
        self.far_pal_synced = None;
        self.surface.baked.clear();
        self.cave.baked.clear();
        self.mini_key = None;
        self.world_key = None;
    }

    fn far_upkeep(&mut self, game: &mut crate::game::Game<'_>) {
        self.weather = game.weather_levels(1.0);
        self.palette_upkeep(game);
        if !self.in_world {
            return;
        }
        let on = self.settings.far;
        let p = self.block_pos();
        let c = [p[0] >> 4, p[2] >> 4];
        let (focus, radius) = if self.map_open && !self.cave_active() {
            let (w, h) = (self.ui.screen.w.max(1.0) as f64, self.ui.screen.h.max(1.0) as f64);
            // Half the view's diagonal, in chunks.
            let r = (w.hypot(h) * 0.5 * self.ui.bpp / 16.0).ceil() as i32;
            let f = [(self.ui.center[0].floor() as i32) >> 4, (self.ui.center[1].floor() as i32) >> 4];
            (f, r.clamp(FAR_RADIUS / 2, FAR_MAX_RADIUS))
        } else {
            (c, FAR_RADIUS)
        };
        if on && self.far_rx.is_none() && self.pal_rx.is_none() && self.pal_hash != 0 {
            self.far_palette_sync();
        }
        let sh = &self.far_shared;
        sh.focus[0].store(focus[0], Ordering::Relaxed);
        sh.focus[1].store(focus[1], Ordering::Relaxed);
        sh.radius.store(radius, Ordering::Relaxed);
        sh.active.store(on, Ordering::Relaxed);
        sh.threads.store(self.settings.far_threads, Ordering::Relaxed);
        sh.map_open.store(self.map_open, Ordering::Relaxed);
        sh.sampler_ok.store(on && game.lod.is_none(), Ordering::Relaxed);
        if !on {
            return;
        }
        let Some(table) = self.table.clone() else { return };
        if self.tints.is_none() {
            let Some(b) = self.biomes.as_ref() else { return };
            let tints = (0..256u16)
                .map(|i| {
                    let c: BiomeColors = b.colors(NativeBiomeId(i), [0, 0]);
                    (c.grass & 0xFF_FFFF, c.foliage & 0xFF_FFFF, c.water & 0xFF_FFFF)
                })
                .collect();
            self.tints = Some(Arc::new(TintTable { tints }));
        }
        let tints = self.tints.clone().unwrap();
        // Not before the pack palette is in: its table replaces this one a moment later.
        if self.far_rx.is_none() && self.pal_rx.is_none() && self.pal_hash != 0 && self.far_picked.map_or(true, |t| t.elapsed() >= Duration::from_secs(3)) {
            self.far_picked = Some(Instant::now());
            let src = pick_far(game);
            if src.any() {
                let mut skip: Vec<(PathBuf, bool)> = Vec::new();
                // Explored chunks saved without block ids (before 0.3.5) are baked into the far layer too: their
                // columns' blocks texture the explored map where its surface is the same (`region::saved_block`).
                skip.extend(self.surface.dir.clone().map(|d| (d, true)));
                skip.extend(self.far.dir.clone().map(|d| (d, true)));
                let job = FarJob { epoch: sh.epoch.load(Ordering::Relaxed), src, skip, table: table.clone(), tints: tints.clone() };
                self.far_rx = spawn_far(sh.clone(), job);
            }
        }
        let t0 = Instant::now();
        let epoch = self.far_shared.epoch.load(Ordering::Relaxed);
        while t0.elapsed() < FAR_RECEIVE_BUDGET {
            let Some(rx) = &self.far_rx else { break };
            let Ok((e, chunks)) = rx.try_recv() else { break };
            if e == epoch {
                for b in &chunks {
                    self.far_put(b);
                }
            }
        }
        // The live tiles: what the game streams for its own far terrain (a server's offer included).
        let Some(lod) = game.lod.as_ref() else { return };
        if (c[0] - self.far_live.center[0]).abs() > 4 || (c[1] - self.far_live.center[1]).abs() > 4 {
            self.far_live.restart(c);
            self.far_live_new = 0;
            self.far_live_wait = None;
            let keep = FAR_RADIUS * 2 + 8;
            self.far_live_done.retain(|d| (d[0] - c[0]).abs() <= keep && (d[1] - c[1]).abs() <= keep);
        }
        let t0 = Instant::now();
        if self.far_live_wait.is_some_and(|t| t0 < t) {
            return;
        }
        self.far_live_wait = None;
        let mut looked = 0;
        while t0.elapsed() < FAR_LIVE_BUDGET && looked < 4096 {
            let Some([cx, cz]) = self.far_live.next() else {
                if self.far_live.finished() {
                    // Tiles keep arriving: another pass later over what is still missing.
                    if self.last_snapshot.elapsed() < Duration::from_millis(20) {
                        if self.far_live_new == 0 {
                            self.far_live_wait = Some(t0 + FAR_LIVE_IDLE);
                        }
                        self.far_live_new = 0;
                        self.far_live.restart(c);
                    }
                }
                break;
            };
            looked += 1;
            if self.far_live_done.contains(&[cx, cz]) {
                continue;
            }
            // Nothing at the chunk's middle and corners: no tile over it (tiles are whole chunks) or all but void,
            // found without asking for all 64 columns of every chunk the game has not streamed.
            if [(8, 8), (0, 0), (15, 0), (0, 15), (15, 15)].iter().all(|&(dx, dz)| live::column(lod, cx * 16 + dx, cz * 16 + dz).is_none()) {
                continue;
            }
            let got = sample_chunk(cx, cz, FAR_STRIDE, &mut self.far_cols, |x, z| live::column(lod, x, z));
            if !got {
                continue;
            }
            if let Some(b) = far::bake_surfaces(cx, cz, &self.far_cols[..], &table, &tints) {
                self.far_live_done.insert([cx, cz]);
                self.far_live_new += 1;
                self.far.store.put(&b);
                request_load(&mut self.far, self.worker.as_ref(), Region::of_chunk(cx, cz));
            }
        }
    }

    fn evict(&mut self) {
        let keep = self.visible.clone();
        // Load counters: in the log at info level while the map is open (every 10 s), else at debug.
        let lvl = if self.map_open { log::Level::Info } else { log::Level::Debug };
        for (name, st) in [("surface", &self.surface.store), ("far", &self.far.store)] {
            log::log!(
                lvl,
                "megamap: {name} regions: {} visible, {} kept (limit {}), {} in memory, {} pending; queued {}, \
                 loaded {}, failed {}, evicted while visible {}",
                st.shown_count(),
                keep.len(),
                st.limit,
                st.regions.len(),
                st.pending_load.len(),
                st.stats.queued,
                st.stats.loaded,
                st.stats.failed,
                st.stats.evicted_visible
            );
        }
        let regions = self.surface.store.evict_regions(&keep);
        if !regions.is_empty() {
            if let (Some(dir), Some(w)) = (self.surface.dir.clone(), self.worker.as_ref()) {
                let _ = w.tx.send(Job::Save { dir, regions });
            }
        }
        let regions = self.far.store.evict_regions(&keep);
        if !regions.is_empty() {
            if let (Some(dir), Some(w)) = (self.far.dir.clone(), self.worker.as_ref()) {
                let _ = w.tx.send(Job::Save { dir, regions });
            }
        }
        // The cave slice and another dimension's map are read only here: their evictions are just dropped.
        let _ = self.cave.store.evict_regions(&keep);
        let _ = self.other.store.evict_regions(&keep);
        let _ = self.other_far.store.evict_regions(&keep);
    }

    /// Left the world: everything saved, caches dropped.
    fn leave(&mut self) {
        self.flush_regions();
        self.persist_small(false);
        self.in_world = false;
        self.map_open = false;
        self.world_id = None;
        self.world_dir = None;
        self.dim.clear();
        self.view_dim = Some(String::new());
        self.set_view_dim(None);
        self.surface.reset(None);
        self.cave.reset(None);
        self.pending.clear();
        self.far_reset();
        self.far_shared.active.store(false, Ordering::Relaxed);
        self.tints = None;
        self.slot = Slot::None;
        self.table = None;
        self.table_vanilla = None;
        self.pal_hash = 0;
        self.pal_rx = None;
        self.emit_done = false;
        self.tex.clear();
        self.follow.reset();
        self.table_build = ColorTable::default();
        self.table_next = 0;
        self.table_empty_run = 0;
        self.biomes = None;
    }

    // ---- commands ----

    fn command(&mut self, host: &mut dyn Host, line: &str) {
        let line = line.trim().trim_start_matches('/');
        let mut parts = line.splitn(4, ' ');
        let _ = parts.next();
        let (a, b) = (parts.next().unwrap_or("help"), parts.next().unwrap_or(""));
        let rest = parts.next().unwrap_or("").trim();
        match (a, b) {
            ("wp", "add") => {
                let pos = self.block_pos();
                let tr = self.tr();
                let name = if rest.is_empty() { self.waypoints.next_name(&tr.t("megamap.toast.wp_default")) } else { rest.to_string() };
                let mut w = Waypoint::new(&name, pos, &self.dim);
                w.color = self.waypoints.list.len() % color::PALETTE.len();
                self.waypoints.add(w);
                self.toast(tr.f("megamap.toast.wp_added", &[&name, &pos[0], &pos[1], &pos[2]]));
            }
            ("wp", "del") => match self.find_wp(rest) {
                Some(i) => {
                    let w = self.waypoints.remove(i).unwrap();
                    self.toast(self.tr().f("megamap.toast.wp_deleted", &[&w.name]));
                }
                None => self.toast(self.tr().f("megamap.toast.no_wp", &[&rest])),
            },
            ("wp", "share") => match self.find_wp(rest) {
                Some(i) => {
                    let text = self.waypoints.list[i].share_text();
                    if let Err(e) = host.send_chat(&text) {
                        self.toast(self.tr().f("megamap.toast.share_failed", &[&e]));
                    }
                }
                None => self.toast(self.tr().f("megamap.toast.no_wp", &[&rest])),
            },
            ("wp", "import") => match self.last_shared.take() {
                Some((name, pos, dim)) => {
                    let dim = if dim.contains(':') { dim } else { format!("minecraft:{dim}") };
                    let mut w = Waypoint::new(&name, pos, &dim);
                    w.color = 5;
                    w.icon = 4;
                    self.waypoints.add(w);
                    self.toast(self.tr().f("megamap.toast.imported", &[&name]));
                }
                None => self.toast(self.tr().t("megamap.toast.no_shared")),
            },
            ("wp", _) => {
                let rows = self.waypoints.filter("", Some(&self.dim), self.player);
                let names: Vec<String> = rows.iter().take(8).map(|&i| self.waypoints.list[i].name.clone()).collect();
                self.toast(if names.is_empty() { self.tr().t("megamap.toast.no_wp_here") } else { names.join(", ") });
            }
            ("trail", "clear") => {
                self.trail.clear();
                self.toast(self.tr().t("megamap.toast.trail_cleared"));
            }
            _ => self.toast(self.tr().t("megamap.toast.usage")),
        }
    }

    fn find_wp(&self, name: &str) -> Option<usize> {
        let n = name.to_lowercase();
        self.waypoints.list.iter().position(|w| w.dimension == self.dim && w.name.to_lowercase() == n)
    }
}

fn request_load(layer: &mut MapLayer, worker: Option<&Worker>, key: (i32, i32)) {
    let (Some(dir), Some(w)) = (layer.dir.as_ref(), worker) else { return };
    if layer.store.want_load(key, Instant::now()) {
        let _ = w.tx.send(Job::Load { epoch: layer.epoch.load(Ordering::Relaxed), layer: layer.id, dir: dir.clone(), key });
    }
}

fn write_now(dir: &Path, regions: Vec<Region>) {
    for r in regions {
        let _ = store::write_region(dir, r.rx, r.rz, &r.encode());
    }
}

fn world_key(w: &NativeWorldId) -> String {
    match w {
        NativeWorldId::Singleplayer(s) => format!("sp_{}", store::sanitize(s)),
        NativeWorldId::Server(s) => format!("mp_{}", store::sanitize(s)),
        #[allow(unreachable_patterns)]
        _ => "other".into(),
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Drawing helpers.

const PANEL: u32 = 0xC010_1216;
const PANEL_LIGHT: u32 = 0xE02A_2E36;
const ACCENT: u32 = 0xFF4F_C3F7;
const WHITE: u32 = 0xFFFFFF;
const GREY: u32 = 0xA0A4AC;

/// The UI colours of one theme (Options > Mods > megamap > UI theme).
#[derive(Clone, Copy)]
struct Pal {
    bg: u32,
    panel: u32,
    panel_hi: u32,
    field: u32,
    border: u32,
    text: u32,
    dim: u32,
    shadow: bool,
    dark: bool,
}

const DARK: Pal = Pal {
    bg: 0xFF12_1418,
    panel: PANEL,
    panel_hi: PANEL_LIGHT,
    field: 0xFF20_242A,
    border: style::FRAME_MID,
    text: WHITE,
    dim: GREY,
    shadow: true,
    dark: true,
};

const LIGHT: Pal = Pal {
    bg: 0xFFD8_DCE2,
    panel: 0xE0EE_F0F4,
    panel_hi: 0xF0FF_FFFF,
    field: 0xFFDD_E1E8,
    border: style::FRAME_MID,
    text: 0x1E2228,
    dim: 0x5A606A,
    shadow: false,
    dark: false,
};

/// A panel with a soft drop shadow (two offset translucent rects) under it.
fn shadowed(host: &dyn Host, b: &mut HudBatch, r: [f32; 4], fill: u32) {
    let [x, y, w, h] = r;
    host.fill(b, [x + 2.0, y + 2.0, w, h], 0x3000_0000);
    host.fill(b, [x + 1.0, y + 1.0, w, h], 0x3000_0000);
    host.fill(b, r, fill);
}


/// What the layer badge shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Badge {
    Surface,
    Cave,
    Slice,
    Indoor,
}

impl Badge {
    fn icon(self) -> &'static [u16; 16] {
        match self {
            Badge::Surface => &icons::SURFACE,
            Badge::Cave => &icons::CAVE,
            Badge::Slice => &icons::SLICE,
            Badge::Indoor => &icons::HOUSE,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Badge::Surface => "megamap.ui.layer_badge_surface",
            Badge::Cave => "megamap.ui.layer_badge_cave",
            Badge::Slice => "megamap.ui.layer_badge_fixed",
            Badge::Indoor => "megamap.ui.layer_badge_indoor",
        }
    }
}

/// A generated 16x16 line icon at `at` (GUI px), one fill per horizontal run.
/// Icon corners go to the minimap image's pixel grid (half a GUI pixel), not whole GUI pixels: the map glides in
/// eighths of an image pixel, and whole-pixel icons stepped a full GUI pixel against it (v0.3.5). The face's own
/// pixels stay whole GUI pixels, so its shape never changes as it moves.
fn snap_half(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

fn draw_icon(host: &dyn Host, b: &mut HudBatch, icon: &[u16; 16], at: [f32; 2], argb: u32) {
    let (x0, y0) = (at[0].round(), at[1].round());
    icons::runs(icon, |x, y, n| host.fill(b, [x0 + x as f32, y0 + y as f32, n as f32, 1.0], argb));
}

/// The hostile warning's red (text and skull).
const HOSTILE_RED: u32 = 0xFF55_5555;

/// The info block: one dark panel, then each row (the skull before the first row of the hostile warning).
fn draw_info_block(host: &dyn Host, b: &mut HudBatch, bl: &infoblock::Block) {
    host.fill(b, bl.rect, 0x7000_0000);
    for r in &bl.rows {
        if r.icon {
            let at = [r.x - infoblock::ICON_W, r.y];
            draw_icon(host, b, &icons::SKULL, [at[0] + 1.0, at[1] + 1.0], 0xFF3A_0000);
            draw_icon(host, b, &icons::SKULL, at, r.color | 0xFF00_0000);
        }
        host.text(b, &r.text, [r.x, r.y], r.color & 0xFF_FFFF, true);
    }
}

fn frame_rect(host: &dyn Host, b: &mut HudBatch, r: [f32; 4], argb: u32) {
    let [x, y, w, h] = r;
    host.fill(b, [x, y, w, 1.0], argb);
    host.fill(b, [x, y + h - 1.0, w, 1.0], argb);
    host.fill(b, [x, y, 1.0, h], argb);
    host.fill(b, [x + w - 1.0, y, 1.0, h], argb);
}

/// An icon frame `width` GUI px wide (quarter steps) around inner rect `r`, anti-aliased (`style::frame_rings`).
fn aa_frame(host: &dyn Host, b: &mut HudBatch, r: [f32; 4], width: f32, argb: u32) {
    let a0 = (argb >> 24) as f32;
    for (off, cov) in style::frame_rings(width) {
        let c = ((a0 * cov).round() as u32) << 24 | (argb & 0xFF_FFFF);
        frame_rect(host, b, [r[0] - off, r[1] - off, r[2] + 2.0 * off, r[3] + 2.0 * off], c);
    }
}

fn diamond(host: &dyn Host, b: &mut HudBatch, c: [f32; 2], r: f32, argb: u32) {
    host.quad(b, [[c[0], c[1] - r], [c[0] + r, c[1]], [c[0], c[1] + r], [c[0] - r, c[1]]], argb);
}

/// An arrowhead at `c` pointing along unit `d` (GUI pixels, y down).
fn arrow(host: &dyn Host, b: &mut HudBatch, c: [f32; 2], d: [f32; 2], size: f32, argb: u32) {
    let rot = |a: f32| {
        let (s, co) = a.sin_cos();
        [d[0] * co - d[1] * s, d[0] * s + d[1] * co]
    };
    let (l, r) = (rot(2.45), rot(-2.45));
    let tip = [c[0] + d[0] * size, c[1] + d[1] * size];
    let notch = [c[0] - d[0] * size * 0.3, c[1] - d[1] * size * 0.3];
    let left = [c[0] + l[0] * size * 0.8, c[1] + l[1] * size * 0.8];
    let right = [c[0] + r[0] * size * 0.8, c[1] + r[1] * size * 0.8];
    // An outline first, a little larger, so the arrow reads on any terrain.
    let grow = |p: [f32; 2], k: f32| [c[0] + (p[0] - c[0]) * k, c[1] + (p[1] - c[1]) * k];
    host.quad(b, [grow(tip, 1.3), grow(right, 1.3), grow(notch, 1.3), grow(left, 1.3)], 0xFF00_0000);
    host.quad(b, [tip, right, notch, left], argb);
}

/// A waypoint icon: the generated sprite when the resource pack has it, else a diamond and the letter.
fn waypoint_icon(host: &dyn Host, b: &mut HudBatch, w: &Waypoint, c: [f32; 2], big: bool) {
    let r = if big { 5.0 } else { 3.0 };
    let glyph = icons::WP_GLYPHS[w.icon % icons::WP_GLYPHS.len()].filter(|_| big);
    let Some(glyph) = glyph else {
        // The diamond, and every mark on the minimap (too small there for a shape).
        diamond(host, b, c, r + 1.0, 0xFF00_0000);
        diamond(host, b, c, r, w.argb());
        return;
    };
    // The mark's own shape in the waypoint's colour, a black edge around it (0.3.11: a letter on a diamond).
    let (x0, y0) = ((c[0] - 4.0).round(), (c[1] - 4.0).round());
    for (dx, dy) in [(-1.0, 0.0), (1.0, 0.0), (0.0, -1.0), (0.0, 1.0)] {
        icons::runs(&glyph, |x, y, n| host.fill(b, [x0 + dx + x as f32, y0 + dy + y as f32, n as f32, 1.0], 0xFF00_0000));
    }
    icons::runs(&glyph, |x, y, n| host.fill(b, [x0 + x as f32, y0 + y as f32, n as f32, 1.0], w.argb()));
}

#[allow(dead_code)]
const ICON_SPRITES: [&str; 6] = [
    "megamap:gui/wp_diamond",
    "megamap:gui/wp_star",
    "megamap:gui/wp_home",
    "megamap:gui/wp_skull",
    "megamap:gui/wp_flag",
    "megamap:gui/wp_cross",
];

fn hcorner(c: settings::Corner) -> HCorner {
    match c {
        settings::Corner::TopLeft => HCorner::TopLeft,
        settings::Corner::TopRight => HCorner::TopRight,
        settings::Corner::BottomLeft => HCorner::BottomLeft,
        settings::Corner::BottomRight => HCorner::BottomRight,
    }
}

/// One radar blip in its kind's icon, colour and size (white while an urgent one blinks).
fn blip(host: &dyn Host, b: &mut HudBatch, st: &entities::Style, flash: bool, c: [f32; 2]) {
    let fill = 0xFF00_0000 | if flash { 0xFF_FFFF } else { st.color & 0xFF_FFFF };
    // The whole icon moves on one grid, so its edges never round apart (a shimmering width) as it glides.
    let c = [snap_half(c[0]), snap_half(c[1])];
    for (r, outline) in entities::icon_rects(st.icon, c, st.px()) {
        host.fill(b, r, if outline { 0xE000_0000 } else { fill });
    }
}

fn distance_label(d: f64) -> String {
    if d >= 1000.0 { format!("{:.1}km", d / 1000.0) } else { format!("{}m", d as i64) }
}

impl State {
    fn draw_hud(&mut self, host: &mut dyn Host, batch: &mut HudBatch, corners: &mut Corners) {
        // Toasts: top centre, independent of the minimap.
        for (i, t) in self.toasts.iter().enumerate() {
            let w = host.text_width(&t.text);
            let x = (batch.width - w) / 2.0;
            let y = 4.0 + i as f32 * 11.0;
            host.fill(batch, [x - 3.0, y - 2.0, w + 6.0, 11.0], 0xA000_0000);
            host.text(batch, &t.text, [x, y], WHITE, true);
        }
        if self.map_open || self.settings.mode == Mode::Hidden || self.gui_hidden || !self.in_world {
            set_last_mini_rect(None);
            return;
        }
        let s = self.settings.size_px() as f32;
        let block_lines = self.info_block_lines(false);
        let width = |t: &str| host.text_width(t);
        let (bw, rows) = infoblock::measure(&block_lines, s + 4.0, batch.width, &width);
        let bh = if rows > 0 { rows as f32 * infoblock::ROW_H + 2.0 } else { 0.0 };
        let (x, y) = corners.place(hcorner(self.settings.corner), (s + 4.0).max(bw), s + 4.0 + bh);
        // A block wider than the frame grows away from the screen edge, the frame stays in the corner.
        let right = matches!(self.settings.corner, settings::Corner::TopRight | settings::Corner::BottomRight);
        let x = if right && bw > s + 4.0 { x + bw - (s + 4.0) } else { x };
        set_last_mini_rect(Some([x, y, s + 4.0, s + 4.0]));
        let (mx, my) = (x + 2.0, y + 2.0);
        let round = self.settings.shape == Shape::Round;
        // The frame is one fixed designer grey bevel, always opaque (engine::style): not the minimap opacity, the
        // theme, the time of day or the weather.
        let fc = style::frame_colors(self.settings.opacity, self.day_time, self.weather.0, self.weather.1, self.settings.ui_theme);
        if !round {
            let (fx, fy, fs) = (x, y, s + 4.0);
            host.fill(batch, [fx + 2.0, fy + 2.0, fs, fs], fc.shadow);
            host.fill(batch, [fx + 1.0, fy + 1.0, fs, fs], fc.shadow);
            frame_rect(host, batch, [fx, fy, fs, fs], fc.edge);
            // Highlight on the top and left, shade on the bottom and right.
            host.fill(batch, [fx + 1.0, fy + 1.0, fs - 2.0, 1.0], fc.light);
            host.fill(batch, [fx + 1.0, fy + 1.0, 1.0, fs - 2.0], fc.light);
            host.fill(batch, [fx + 1.0, fy + fs - 2.0, fs - 2.0, 1.0], fc.dark);
            host.fill(batch, [fx + fs - 2.0, fy + 1.0, 1.0, fs - 2.0], fc.dark);
        }
        if self.slot == Slot::Mini && self.mini_px > 0 {
            let p = self.mini_px as f32;
            host.sprite(batch, MOD_IMAGE, [0.0, 0.0, p, p], [mx, my, s, s]);
        }
        if round {
            let (cx, cy, r) = (mx + s / 2.0, my + s / 2.0, s / 2.0);
            let n = 72;
            for i in 0..n {
                let (a0, a1) = (i as f32 / n as f32 * std::f32::consts::TAU, (i + 1) as f32 / n as f32 * std::f32::consts::TAU);
                let p = |a: f32, rr: f32, o: f32| [cx + o + a.cos() * rr, cy + o + a.sin() * rr];
                // Shadow, then the outer edge, then the bevel ring shaded by its facing to the top-left light.
                host.quad(batch, [p(a0, r + 1.0, 1.5), p(a1, r + 1.0, 1.5), p(a1, r + 3.0, 1.5), p(a0, r + 3.0, 1.5)], fc.shadow);
                host.quad(batch, [p(a0, r + 1.0, 0.0), p(a1, r + 1.0, 0.0), p(a1, r + 2.0, 0.0), p(a0, r + 2.0, 0.0)], fc.edge);
                host.quad(batch, [p(a0, r - 0.5, 0.0), p(a1, r - 0.5, 0.0), p(a1, r + 1.0, 0.0), p(a0, r + 1.0, 0.0)], style::ring_color((a0 + a1) / 2.0));
            }
        }
        let rotation = if self.settings.north_lock { 0.0 } else { (self.yaw as f64).to_radians() + std::f64::consts::PI };
        let v = View { center: [self.player[0], self.player[2]], bpp: self.mini_bpp, rotation, round, fog: false, style: Default::default() };
        let su = s as usize;
        let (cx, cy) = (mx + s / 2.0, my + s / 2.0);
        let half = s / 2.0;
        let place = |wx: f64, wz: f64, margin: f32| -> ([f32; 2], bool) {
            let p = v.to_image(su, su, wx, wz);
            let (dx, dy) = (p[0] as f32 - half, p[1] as f32 - half);
            let lim = half - margin;
            if round {
                let d = (dx * dx + dy * dy).sqrt();
                if d > lim { ([cx + dx / d * lim, cy + dy / d * lim], false) } else { ([cx + dx, cy + dy], true) }
            } else {
                let inside = dx.abs() <= lim && dy.abs() <= lim;
                ([cx + dx.clamp(-lim, lim), cy + dy.clamp(-lim, lim)], inside)
            }
        };
        // Trail, the newest few hundred points.
        if self.settings.trail {
            let n = self.trail.points.len();
            for p in self.trail.points.iter().skip(n.saturating_sub(300)) {
                if p[1] == i32::MIN {
                    continue;
                }
                let (q, inside) = place(p[0] as f64 + 0.5, p[2] as f64 + 0.5, 1.0);
                if inside {
                    host.fill(batch, [q[0] - 0.5, q[1] - 0.5, 1.0, 1.0], 0xC0FF_E082);
                }
            }
        }
        // Radar blips.
        if self.settings.radar {
            let blink = (self.t0.elapsed().as_millis() / 250) % 2 == 0;
            for (i, bl) in self.radar.blips.iter().enumerate() {
                let st = self.settings.entity_style(&bl.kind);
                if !st.mini {
                    continue;
                }
                let (q, inside) = place(bl.x, bl.z, 2.0);
                if !inside {
                    continue;
                }
                let live = self.portraits.blip_tex.get(i).and_then(|t| t.as_deref());
                self.draw_entity(host, batch, &st, &bl.kind, live, bl.urgent && blink, q);
            }
        }
        // Waypoints, clamped to the edge when out of range.
        for w in self.waypoints.in_dimension(&self.dim) {
            let (q, _) = place(w.x as f64 + 0.5, w.z as f64 + 0.5, 4.0);
            waypoint_icon(host, batch, w, q, false);
        }
        // The player.
        let d = if self.settings.north_lock {
            let r = (self.yaw as f32).to_radians();
            [-r.sin(), r.cos()]
        } else {
            [0.0, -1.0]
        };
        arrow(host, batch, [cx, cy], d, 4.5, 0xFFFF_FFFF);
        // Compass: N always, E / S / W too with the compass on; they ride the rim as the map turns.
        let letters: &[(&str, f32, u32)] = if self.settings.compass {
            &[(COMPASS[4], 0.0, 0xFF5555), (COMPASS[6], -0.5, 0xE0E0E0), (COMPASS[0], 1.0, 0xE0E0E0), (COMPASS[2], 0.5, 0xE0E0E0)]
        } else {
            &[(COMPASS[4], 0.0, 0xFF5555)]
        };
        let tr = self.tr();
        for &(key, turn, rgb) in letters {
            let t = &tr.t(key);
            // World north shows at (-sin r, -cos r) on screen; the others a quarter turn apart.
            let (sn, cn) = (rotation as f32 + turn * std::f32::consts::PI).sin_cos();
            let p = [cx - sn * (half - 5.0), cy - cn * (half - 5.0)];
            let w = host.text_width(t);
            host.fill(batch, [p[0] - w / 2.0 - 1.5, p[1] - 5.0, w + 3.0, 9.0], 0x9000_0000);
            host.text(batch, t, [p[0] - w / 2.0, p[1] - 4.0], rgb, false);
        }
        // The layer badge in the minimap's top-left corner (the cave tint tells it at a glance; this says which).
        if let Some(badge) = self.layer_badge() {
            let at = if round { [mx + s * 0.15 - 1.0, my + s * 0.15 - 1.0] } else { [mx + 1.0, my + 1.0] };
            self.draw_badge(host, batch, badge, at);
        }
        // Info block under the map: one panel, centred under the frame or flush with one of its edges.
        if !block_lines.is_empty() {
            let bl = infoblock::layout(&block_lines, [x, y, s + 4.0, s + 4.0], infoblock::Align::of(self.settings.info_align), true, batch.width, &width);
            draw_info_block(host, batch, &bl);
        }
    }

    /// The info block's lines: the switched-on info lines, then the hostile warning (`sample`: a made-up one for
    /// the settings preview when no hostile is near).
    fn info_block_lines(&self, sample: bool) -> Vec<infoblock::Line> {
        let mut out: Vec<infoblock::Line> = self.info.iter().map(|t| infoblock::Line { text: t.clone(), icon: false, color: WHITE }).collect();
        let (n, near) = match (self.radar.hostile_count, self.radar.nearest) {
            (n, Some(d)) if n > 0 && self.settings.radar => (n, d),
            _ if sample => (2, 21.0),
            _ => (0, 0.0),
        };
        if n > 0 && self.settings.hostile < 2 {
            let l = self.lang();
            let unit = lang::builtin(l, "megamap.unit.m").unwrap_or("m");
            let word = (self.settings.hostile == 1).then(|| lang::builtin(l, "megamap.hostile_word").unwrap_or("Hostile"));
            out.push(infoblock::Line { text: infoblock::hostile_line(n, near, unit, word), icon: self.settings.hostile == 0, color: HOSTILE_RED });
        }
        out
    }

    fn draw_world(&mut self, host: &dyn Host, b: &mut HudBatch, mouse: [f32; 2], rect: [f32; 4]) {
        self.ui.screen = Rect::new(rect[0], rect[1], rect[2], rect[3]);
        self.ui.cursor = mouse;
        self.refresh_panel_rows();
        let tr = self.tr();
        let labels = self.settings.buttons_in(&tr);
        self.ui.refresh_settings_rows(&labels);
        let sc = self.ui.screen;
        let pal = self.pal();
        host.fill(b, sc.arr(), pal.bg);
        // The map image, placed from the view it was rendered with (a pan this frame moves it until the next render).
        if self.slot == Slot::World {
            let (c, bpp, size, img) = self.world_img;
            let k = bpp / self.ui.bpp;
            let (w, h) = (size[0] as f64 * k, size[1] as f64 * k);
            let tl = self.ui.screen_of(c[0] - size[0] as f64 / 2.0 * bpp, c[1] - size[1] as f64 / 2.0 * bpp);
            let off = self.preview.world_off as f32;
            host.sprite(b, MOD_IMAGE, [0.0, off, img[0], img[1]], [tl[0], tl[1], w as f32, h as f32]);
        }
        let ui = &self.ui;
        let inside = |p: [f32; 2]| p[0] >= sc.x && p[1] >= sc.y + 14.0 && p[0] < sc.x + sc.w && p[1] < sc.y + sc.h - 12.0;
        // Trail.
        if self.settings.trail {
            // Zoomed out, many points fall on one pixel: a dot is drawn once it is a pixel away from the last.
            let mut last = [f32::MIN; 2];
            for (a, c) in self.trail.segments() {
                let (pa, pc) = (ui.screen_of(a[0] as f64 + 0.5, a[2] as f64 + 0.5), ui.screen_of(c[0] as f64 + 0.5, c[2] as f64 + 0.5));
                if (inside(pa) || inside(pc)) && ((pa[0] - last[0]).abs() >= 1.0 || (pa[1] - last[1]).abs() >= 1.0) {
                    host.fill(b, [pa[0] - 0.5, pa[1] - 0.5, 1.5, 1.5], 0xD0FF_E082);
                    last = pa;
                }
            }
        }
        // Waypoints (of the dimension shown) with their names when zoomed in enough not to clutter.
        let vd = self.view_dim.as_deref();
        let wps = self.waypoints.list.iter().filter(|w| {
            w.visible && vd.map_or(w.dimension == self.dim, |d| store::sanitize(&w.dimension) == d)
        });
        for w in wps {
            let p = ui.screen_of(w.x as f64 + 0.5, w.z as f64 + 0.5);
            if !inside(p) {
                continue;
            }
            waypoint_icon(host, b, w, p, true);
            if ui.bpp <= 8.0 {
                let tw = host.text_width(&w.name);
                host.fill(b, [p[0] - tw / 2.0 - 2.0, p[1] + 7.0, tw + 4.0, 10.0], 0x8000_0000);
                host.text(b, &w.name, [p[0] - tw / 2.0, p[1] + 8.0], WHITE, true);
            }
        }
        // Radar blips around the player.
        if self.settings.radar {
            let blink = (self.t0.elapsed().as_millis() / 250) % 2 == 0;
            for (i, bl) in self.radar.blips.iter().enumerate() {
                let st = self.settings.entity_style(&bl.kind);
                let p = ui.screen_of(bl.x, bl.z);
                if st.map && inside(p) && self.view_dim.is_none() {
                    let live = self.portraits.blip_tex.get(i).and_then(|t| t.as_deref());
                    self.draw_entity(host, b, &st, &bl.kind, live, bl.urgent && blink, p);
                }
            }
        }
        // The player, unless the map shows another dimension.
        let pp = ui.screen_of(self.player[0], self.player[2]);
        if inside(pp) && self.view_dim.is_none() {
            let r = self.yaw.to_radians();
            arrow(host, b, pp, [-r.sin(), r.cos()], 5.0, 0xFFFF_FFFF);
        }
        // Top bar.
        host.fill(b, [sc.x, sc.y, sc.w, 14.0], pal.panel);
        let cb = ui.cursor_block();
        let shown_dim = match &self.view_dim {
            Some(d) => tr.f("megamap.ui.viewing", &[&waypoints::short_dim(d), &waypoints::short_dim(&self.dim)]),
            None => self.tname("dimension", &self.dim),
        };
        let scale = if ui.bpp >= 1.0 {
            tr.f("megamap.ui.scale_blocks", &[&format!("{:.0}", ui.bpp)])
        } else {
            tr.f("megamap.ui.scale_frac", &[&format!("{:.0}", 1.0 / ui.bpp)])
        };
        let tag = if self.view_dim.is_none() { self.layer_tag() } else { String::new() };
        let top = tr.f("megamap.ui.top", &[&format!("{shown_dim}{tag}"), &cb[0], &cb[1], &format!("{:.0}", self.explored), &scale]);
        host.text(b, &top, [sc.x + 4.0, sc.y + 3.0], pal.text, pal.shadow);
        // The layer badge under the top bar's left end, as on the minimap.
        let mut tooltip: Option<String> = None;
        if let Some(badge) = self.layer_badge().filter(|_| self.view_dim.is_none()) {
            let r = self.draw_badge(host, b, badge, [sc.x + 3.0, sc.y + 16.0]);
            if (r[0]..r[0] + r[2]).contains(&mouse[0]) && (r[1]..r[1] + r[3]).contains(&mouse[1]) {
                tooltip = Some(self.layer_tag().trim_start_matches([' ', '·']).to_string());
            }
        }
        // Buttons with tooltips.
        for (i, (bt, label, tip)) in BUTTONS.iter().enumerate() {
            let r = ui.button_rect(i);
            let on = match bt {
                ui::Button::List => ui.list_open,
                ui::Button::Cave => self.settings.layer_mode() != LayerMode::Surface,
                ui::Button::Trail => self.settings.trail,
                ui::Button::Clouds => self.settings.clouds && self.settings.fog,
                ui::Button::Relief => self.settings.relief > 0,
                ui::Button::Grid => self.settings.grid > 0,
                ui::Button::Settings => ui.panel.as_ref().is_some_and(|p| p.kind == ui::PanelKind::Settings),
                ui::Button::Entities => ui.panel.as_ref().is_some_and(|p| p.kind == ui::PanelKind::Entities),
                ui::Button::Dimension => self.view_dim.is_some() || ui.panel.as_ref().is_some_and(|p| p.kind == ui::PanelKind::Dimensions),
                _ => false,
            };
            let hover = r.contains(mouse);
            let look = style::button_look(hover, on, pal.dark, ACCENT);
            if pal.shadow {
                shadowed(host, b, r.arr(), look.fill);
            } else {
                host.fill(b, r.arr(), look.fill);
            }
            frame_rect(host, b, r.arr(), look.border);
            // A 16x16 line icon from the code (the .hnmod has no textures); the letter is only the tooltip's.
            let _ = label;
            draw_icon(host, b, icons::TOOLBAR[i], [r.x + (r.w - 16.0) / 2.0, r.y + (r.h - 16.0) / 2.0], look.icon);
            if on {
                host.fill(b, [r.x + 2.0, r.y + r.h - 2.0, r.w - 4.0, 1.0], ACCENT);
            }
            if hover {
                tooltip = Some(tr.t(tip));
            }
        }
        // Waypoint list.
        if ui.list_open {
            let key_changed = self.rows_key.0 != self.waypoints.generation || self.rows_key.1 != ui.search || self.rows_key.2 != self.dim;
            if key_changed {
                self.rows = ui.filtered(&self.waypoints);
                self.rows_key = (self.waypoints.generation, ui.search.clone(), self.dim.clone());
            }
            let ui = &self.ui;
            let l = ui.list_rect();
            shadowed(host, b, l.arr(), pal.panel);
            frame_rect(host, b, l.arr(), pal.border);
            let sr = ui.search_rect();
            host.fill(b, sr.arr(), 0xFF08_0A0C);
            frame_rect(host, b, sr.arr(), if ui.search_focus { ACCENT } else { 0xFF30_3540 });
            if ui.search.is_empty() && !ui.search_focus {
                host.text(b, &tr.t("megamap.ui.search"), [sr.x + 3.0, sr.y + 3.0], 0x707480, false);
            } else {
                let caret = if ui.search_focus && (now_ms() / 500) % 2 == 0 { "_" } else { "" };
                host.text(b, &format!("{}{caret}", ui.search), [sr.x + 3.0, sr.y + 3.0], WHITE, false);
            }
            // Deaths sort last: a thin rule marks where their group starts.
            let first_death = self.rows.iter().position(|&i| self.waypoints.list.get(i).is_some_and(|w| w.kind == waypoints::Kind::Death));
            for slot in 0..ui.list_rows() {
                let Some(&i) = self.rows.get(ui.scroll + slot) else { break };
                let Some(w) = self.waypoints.list.get(i) else { break };
                let r = ui.row_rect(slot);
                if first_death == Some(ui.scroll + slot) && ui.scroll + slot > 0 {
                    host.fill(b, [r.x + 2.0, r.y, r.w - 4.0, 1.0], 0xFF80_3030);
                }
                if r.contains(mouse) {
                    host.fill(b, r.arr(), 0x40FF_FFFF);
                }
                host.fill(b, [r.x + 2.0, r.y + 3.0, 6.0, 6.0], w.argb());
                let d = ((w.x as f64 + 0.5 - self.player[0]).powi(2) + (w.z as f64 + 0.5 - self.player[2]).powi(2)).sqrt();
                let dist = distance_label(d);
                let dw = host.text_width(&dist);
                let mut name = w.name.clone();
                while !name.is_empty() && host.text_width(&name) > r.w - dw - 28.0 {
                    name.pop();
                }
                host.text(b, &name, [r.x + 11.0, r.y + 2.0], if w.visible { pal.text } else { 0x707480 }, false);
                host.text(b, &dist, [r.x + r.w - dw - 14.0, r.y + 2.0], pal.dim, false);
                let e = ui.row_edit_rect(slot);
                host.fill(b, e.arr(), if e.contains(mouse) { ACCENT } else { 0xFF30_3540 });
                host.text(b, "e", [e.x + 2.5, e.y + 1.0], WHITE, false);
            }
            if self.rows.is_empty() {
                host.text(b, &tr.t("megamap.ui.no_waypoints"), [l.x + 6.0, l.y + 20.0], 0x707480, false);
            }
            let nb = ui.new_button_rect();
            host.fill(b, nb.arr(), if nb.contains(mouse) { pal.panel_hi } else { pal.field });
            let t = &tr.t("megamap.ui.new_wp");
            host.text(b, t, [nb.x + (nb.w - host.text_width(t)) / 2.0, nb.y + 3.0], pal.text, false);
        }
        self.draw_panel(host, b, mouse, &pal);
        let ui = &self.ui;
        // Right-click menu.
        if let Some(m) = ui.menu {
            let r = ui.menu_rect(&m);
            host.fill(b, r.arr(), 0xF018_1B20);
            frame_rect(host, b, r.arr(), 0xFF40_4650);
            let wp = m.wp.and_then(|i| self.waypoints.list.get(i));
            if let Some(w) = wp {
                // The waypoint's name over its entries, its colour beside it.
                host.fill(b, [r.x + 2.0, r.y + 2.0, r.w - 4.0, ui::ROW - 1.0], 0xFF24_2830);
                host.fill(b, [r.x + 5.0, r.y + 4.0, 6.0, 6.0], w.argb());
                let (name, _) = ui::fit(&w.name, r.w - 20.0, &|t| host.text_width(t));
                host.text(b, &name, [r.x + 14.0, r.y + 4.0], WHITE, false);
            }
            for (i, item) in m.items().iter().enumerate() {
                let ir = ui.menu_item_rect(&m, i);
                if ir.contains(mouse) {
                    host.fill(b, ir.arr(), 0x40FF_FFFF);
                }
                let ink = if *item == ui::MenuItem::WpDelete { 0xFF_8A80 } else { WHITE };
                host.text(b, &tr.t(item.key(wp.is_none_or(|w| w.visible))), [ir.x + 3.0, ir.y + 2.0], ink, false);
            }
        }
        // The toolbar's tooltip over the open panel and the menu (drawn before them it hid under the panel's rows).
        // Over the toolbar: every button's name beside it at once (0.3.11), the hovered one bright, so the icons
        // need no guessing and no hunting one tooltip at a time.
        let over_bar = ui.edit.is_none() && (0..BUTTONS.len()).any(|i| ui.button_rect(i).contains(mouse));
        if over_bar {
            for (i, (_, _, tip)) in BUTTONS.iter().enumerate() {
                let r = ui.button_rect(i);
                let t = tr.t(tip);
                let tw = host.text_width(&t);
                let (tx, ty) = (r.x - tw - 7.0, r.y + (r.h - 8.0) / 2.0);
                let hover = r.contains(mouse);
                host.fill(b, [tx - 3.0, ty - 2.0, tw + 6.0, 12.0], if hover { 0xF020_3040 } else { 0xD010_1010 });
                host.text(b, &t, [tx, ty], if hover { WHITE } else { GREY }, true);
            }
        }
        if let Some(t) = tooltip.as_deref().filter(|_| !over_bar) {
            let tw = host.text_width(t);
            let (tx, ty) = ((mouse[0] - tw - 8.0).max(sc.x + 4.0), mouse[1] + 2.0);
            let ty = if tx > mouse[0] - tw - 8.0 { ty + 12.0 } else { ty };
            host.fill(b, [tx - 3.0, ty - 2.0, tw + 6.0, 12.0], 0xF010_1010);
            host.text(b, t, [tx, ty], WHITE, true);
        }
        // Edit dialog.
        if let Some(e) = &ui.edit {
            host.fill(b, sc.arr(), 0x6000_0000);
            let d = ui.dialog_rect();
            host.fill(b, d.arr(), 0xF818_1B20);
            frame_rect(host, b, d.arr(), ACCENT);
            let title = &tr.t(if e.index.is_some() { "megamap.ui.edit_wp" } else { "megamap.ui.new_wp_title" });
            host.text(b, title, [d.x + 6.0, d.y + 5.0], WHITE, true);
            let coords = format!("{} {} {}", e.draft.x, e.draft.y, e.draft.z);
            host.text(b, &coords, [d.x + d.w - host.text_width(&coords) - 6.0, d.y + 5.0], GREY, false);
            let caret = if (now_ms() / 500) % 2 == 0 { "_" } else { "" };
            // The name field, then the colours and the icons laid out to pick with one click, then the box.
            let f = ui.dialog_field(0);
            host.fill(b, f.arr(), 0xFF08_0A0C);
            frame_rect(host, b, f.arr(), 0xFF40_4650);
            if e.draft.name.is_empty() {
                host.text(b, &tr.t("megamap.edit.name_label"), [f.x + 4.0, f.y + 4.0], 0x606470, false);
            }
            host.text(b, &format!("{}{caret}", e.draft.name), [f.x + 4.0, f.y + 4.0], WHITE, false);
            for (row, key) in [(1usize, "megamap.edit.color_label"), (2, "megamap.edit.icon_label")] {
                let f = ui.dialog_field(row);
                host.text(b, &tr.t(key), [f.x + 1.0, f.y + 4.0], GREY, false);
            }
            for i in 0..color::PALETTE.len() {
                let r = ui.dialog_swatch(i);
                let on = e.draft.color % color::PALETTE.len() == i;
                if on || r.contains(mouse) {
                    host.fill(b, [r.x - 1.0, r.y - 1.0, r.w + 2.0, r.h + 2.0], if on { WHITE | 0xFF00_0000 } else { 0xFF70_7480 });
                }
                host.fill(b, r.arr(), color::PALETTE[i]);
            }
            for i in 0..ICONS.len() {
                let r = ui.dialog_icon(i);
                let on = e.draft.icon % ICONS.len() == i;
                host.fill(b, r.arr(), if on { 0xFF30_3642 } else if r.contains(mouse) { PANEL_LIGHT } else { 0xFF20_242A });
                if on {
                    frame_rect(host, b, r.arr(), ACCENT);
                }
                let mut shown = e.draft.clone();
                shown.icon = i;
                // Without the sprite the fallback is the name's first letter, the same on every choice:
                // drop the name so each choice shows its own icon letter (D S H X F +).
                shown.name.clear();
                waypoint_icon(host, b, &shown, [r.x + r.w / 2.0, r.y + r.h / 2.0], true);
            }
            let f = ui.dialog_field(3);
            if f.contains(mouse) {
                host.fill(b, f.arr(), 0xFF20_242A);
            }
            let bx = [f.x + 2.0, f.y + 3.0, 10.0, 10.0];
            host.fill(b, bx, 0xFF08_0A0C);
            frame_rect(host, b, bx, 0xFF70_7480);
            if e.draft.visible {
                host.fill(b, [bx[0] + 2.0, bx[1] + 2.0, 6.0, 6.0], ACCENT);
            }
            host.text(b, &tr.t("megamap.edit.visible_label"), [f.x + 16.0, f.y + 4.0], WHITE, false);
            for (i, label) in EDIT_BUTTONS.iter().enumerate() {
                let label = &tr.t(label);
                let r = ui.dialog_button(i);
                let disabled = i == 1 && e.index.is_none();
                host.fill(b, r.arr(), if r.contains(mouse) && !disabled { PANEL_LIGHT } else { 0xFF20_242A });
                frame_rect(host, b, r.arr(), 0xFF40_4650);
                host.text(b, label, [r.x + (r.w - host.text_width(label)) / 2.0, r.y + 3.0], if disabled { 0x606470 } else { WHITE }, false);
            }
        }
        // Hint line.
        host.fill(b, [sc.x, sc.y + sc.h - 12.0, sc.w, 12.0], pal.panel);
        host.text(
            b,
            // What the mouse can do where it is.
            &tr.t(if ui.entity_preview_rect().is_some_and(|r| r.contains(mouse)) {
                "megamap.ui.help_preview"
            } else if ui.list_open && ui.list_rect().contains(mouse) {
                "megamap.ui.help_list"
            } else if ui.panel.is_some() && ui.panel_rect().contains(mouse) {
                "megamap.ui.help_panel"
            } else if ui.waypoint_at(mouse, &self.waypoints).is_some() {
                "megamap.ui.help_wp"
            } else {
                "megamap.ui.help"
            }),
            [sc.x + 4.0, sc.y + sc.h - 10.0],
            pal.dim,
            false,
        );
        // Opening fade: the backdrop colour over everything, gone after 150 ms.
        if self.settings.animations {
            let t = self.map_opened.elapsed().as_secs_f32() / 0.15;
            if t < 1.0 {
                let a = ((1.0 - t) * 255.0) as u32;
                host.fill(b, sc.arr(), (a << 24) | (pal.bg & 0x00FF_FFFF));
            }
        }
    }
}

impl State {
    /// Keeps the open panel's rows current (the press handler reads the same list the panel drew).
    fn refresh_panel_rows(&mut self) {
        let rows = match &self.ui.panel {
            Some(p) if p.kind == ui::PanelKind::Entities => {
                // Names are looked up once per kind and language, not every frame.
                let l = self.lang();
                let mut names = self.ent_names.borrow_mut();
                if names.0 != Some(l) {
                    *names = (Some(l), HashMap::new());
                }
                entities::list_named(&self.seen, &p.search, p.category(), &mut |k| {
                    names.1.entry(k.to_string()).or_insert_with(|| self.tname("entity", k)).clone()
                })
            }
            Some(p) if p.kind == ui::PanelKind::Dimensions => {
                // Scanned once per opening: a directory listing every frame is not worth it.
                if !self.ui.panel_rows.is_empty() {
                    return;
                }
                self.dims_on_disk()
            }
            _ => Vec::new(),
        };
        self.ui.panel_rows = rows;
    }

    fn draw_panel(&self, host: &dyn Host, b: &mut HudBatch, mouse: [f32; 2], pal: &Pal) {
        let ui = &self.ui;
        let tr = self.tr();
        let width = |t: &str| host.text_width(t);
        let Some(p) = &ui.panel else { return };
        let r = ui.panel_rect();
        if pal.shadow {
            shadowed(host, b, r.arr(), pal.panel);
        } else {
            host.fill(b, r.arr(), pal.panel);
        }
        frame_rect(host, b, r.arr(), pal.border);
        // Header: tabs, or a title.
        let tabs: Vec<String> = match p.kind {
            ui::PanelKind::Settings => settings::TAB_NAMES.iter().map(|t| tr.t(t)).collect(),
            ui::PanelKind::Entities if p.selected.is_none() => {
                std::iter::once(tr.t("megamap.ui.all")).chain(entities::Category::ALL.iter().map(|c| tr.t(c.name_key()))).collect()
            }
            _ => Vec::new(),
        };
        if tabs.is_empty() {
            let title = match (&p.kind, &p.selected) {
                (ui::PanelKind::Entities, Some(k)) => self.tname("entity", k),
                _ => tr.t("megamap.ui.dimension"),
            };
            host.text(b, &title, [r.x + 4.0, r.y + 4.0], pal.text, pal.shadow);
        }
        for (i, t) in tabs.iter().enumerate() {
            let tb = ui.panel_tab_rect(i);
            let on = p.tab == i;
            host.fill(b, tb.arr(), if on { pal.panel_hi } else if tb.contains(mouse) { pal.field } else { pal.panel });
            if on {
                host.fill(b, [tb.x, tb.y + tb.h - 1.0, tb.w, 1.0], ACCENT);
            }
            let (t, _) = ui::fit(t, tb.w - 2.0, &width);
            host.text(b, &t, [tb.x + (tb.w - host.text_width(&t)) / 2.0, tb.y + 2.0], pal.text, false);
        }
        let cr = ui.panel_close_rect();
        host.fill(b, cr.arr(), if cr.contains(mouse) { 0xFFC6_2828 } else { pal.field });
        host.text(b, "x", [cr.x + 3.0, cr.y + 1.0], WHITE, false);
        if let Some(sr) = ui.panel_search_rect() {
            host.fill(b, sr.arr(), pal.field);
            frame_rect(host, b, sr.arr(), if p.search_focus { ACCENT } else { pal.border });
            let caret = if p.search_focus && (now_ms() / 500) % 2 == 0 { "_" } else { "" };
            let hint = tr.t(if p.kind == ui::PanelKind::Settings { "megamap.ui.search_settings" } else { "megamap.ui.search" });
            let t = if p.search.is_empty() && !p.search_focus { hint } else { format!("{}{caret}", p.search) };
            host.text(b, &t, [sr.x + 3.0, sr.y + 2.0], if p.search.is_empty() { pal.dim } else { pal.text }, false);
        }
        // The canonical (English) labels tell what differs from the defaults; the rows show the words in the map's
        // language.
        let labels = self.settings.buttons();
        let defaults = if p.kind == ui::PanelKind::Settings { Settings::default().buttons() } else { Vec::new() };
        let mut tooltip: Option<String> = None;
        let len = ui.panel_len();
        for slot in 0..ui.panel_slots() {
            let i = p.scroll + slot;
            if i >= len {
                break;
            }
            let rr = ui.panel_row_rect(slot);
            if rr.contains(mouse) {
                host.fill(b, rr.arr(), 0x30FF_FFFF);
            }
            let boxes = |host: &dyn Host, b: &mut HudBatch, marks: [(&str, bool); 2]| {
                for (k, (m, on)) in marks.iter().enumerate() {
                    let br = ui.panel_row_box(slot, k);
                    host.fill(b, br.arr(), if br.contains(mouse) { pal.panel_hi } else if *on { 0xFF2E_7D32 } else { pal.field });
                    host.text(b, m, [br.x + (br.w - host.text_width(m)) / 2.0, br.y + 1.0], WHITE, false);
                }
            };
            let text_w = rr.w - 30.0;
            let clip = |t: String| ui::fit(&t, text_w, &width).0;
            match p.kind {
                ui::PanelKind::Settings => match ui.settings_rows.get(i) {
                    Some(&settings::Row::Header(h)) => {
                        // A fold mark, the name, and folded: how many settings are inside.
                        let folded = ui.collapsed.contains(&h);
                        if rr.contains(mouse) {
                            host.fill(b, rr.arr(), pal.panel_hi);
                        }
                        let (cx, cy) = (rr.x + 5.0, rr.y + ROW_PX / 2.0);
                        let ink = ACCENT | 0xFF00_0000;
                        if folded {
                            // Pointing right.
                            for k in 0..3 {
                                host.fill(b, [cx - 2.0 + k as f32, cy - 2.5 + k as f32, 1.0, 5.0 - 2.0 * k as f32], ink);
                            }
                        } else {
                            // Pointing down.
                            for k in 0..3 {
                                host.fill(b, [cx - 2.5 + k as f32, cy - 1.5 + k as f32, 5.0 - 2.0 * k as f32, 1.0], ink);
                            }
                        }
                        let name = if folded { format!("{} ({})", tr.t(h), settings::group_len(p.cat, h)) } else { tr.t(h) };
                        let (h, _) = ui::fit(&name, rr.w - 14.0, &width);
                        let tw = host.text_width(&h);
                        host.text(b, &h, [rr.x + 11.0, rr.y + 3.0], ACCENT & 0x00FF_FFFF, false);
                        host.fill(b, [rr.x + tw + 15.0, rr.y + ROW_PX / 2.0 + 1.0, (rr.w - tw - 17.0).max(0.0), 1.0], pal.border);
                    }
                    Some(&settings::Row::Item(flat)) => {
                        // A dot marks a setting that differs from the default.
                        if labels.get(flat) != defaults.get(flat) {
                            host.fill(b, [rr.x + 3.0, rr.y + 5.0, 3.0, 3.0], ACCENT);
                        }
                        let (name, value) = self.settings.item(flat, &tr);
                        let boxed = (settings::INFO_FIRST..settings::INFO_FIRST + Info::ALL.len()).contains(&flat);
                        let slider = self.settings.slider(flat);
                        // Name left, value right-aligned before the slider track or the arrow boxes.
                        let right = match slider {
                            Some(_) => ui.panel_slider_rect(slot).x - 4.0,
                            None if boxed => rr.x + rr.w - 27.0,
                            None => rr.x + rr.w - 3.0,
                        };
                        let row = ui::row_text(&name, &value, right - rr.x - 9.0, &width);
                        host.text(b, &row.name, [rr.x + 9.0, rr.y + 2.0], pal.text, false);
                        if !row.value.is_empty() {
                            let vx = right - host.text_width(&row.value);
                            host.text(b, &row.value, [vx, rr.y + 2.0], pal.dim | 0xFF00_0000, false);
                        }
                        if row.cut && rr.contains(mouse) {
                            tooltip = Some(if value.is_empty() { name.clone() } else { format!("{name}: {value}") });
                        }
                        if let Some((pos, steps)) = slider {
                            // A track with the knob right; a click on the track sets the step.
                            let tr = ui.panel_slider_rect(slot);
                            let cy = (tr.y + tr.h / 2.0).round();
                            host.fill(b, [tr.x, cy - 1.0, tr.w, 2.0], pal.border);
                            let f = if steps > 1 { pos as f32 / (steps - 1) as f32 } else { 0.0 };
                            host.fill(b, [tr.x, cy - 1.0, tr.w * f, 2.0], ACCENT);
                            // Ticks only where they stay apart (the 65-step reach slider gets none).
                            for k in 0..if steps <= 17 { steps } else { 0 } {
                                let x = (tr.x + tr.w * k as f32 / (steps - 1).max(1) as f32).round();
                                host.fill(b, [x, cy + 2.0, 1.0, 2.0], pal.dim | 0xFF00_0000);
                            }
                            let kx = (tr.x + tr.w * f - 2.0).round();
                            let hot = tr.contains(mouse);
                            host.fill(b, [kx, tr.y, 4.0, tr.h], if hot { WHITE | 0xFF00_0000 } else { 0xFFD0_D4DA });
                        }
                        if boxed {
                            boxes(host, b, [("^", false), ("v", false)]);
                        }
                    }
                    Some(&settings::Row::ResetCategory(c)) => {
                        let n = self.settings.changed_in(c, &labels, &defaults);
                        let t = if n > 0 { tr.f("megamap.ui.reset_cat_n", &[&n]) } else { tr.t("megamap.ui.reset_cat") };
                        host.text(b, &clip(t), [rr.x + 9.0, rr.y + 2.0], if n > 0 { 0xFFB0_74 } else { pal.dim }, false);
                    }
                    None => break,
                },
                ui::PanelKind::Entities => match &p.selected {
                    Some(kind) => {
                        let st = self.settings.entity_style(kind);
                        let onoff = |v: bool| tr.t(if v { "megamap.on" } else { "megamap.off" });
                        let t = if i == 0 {
                            tr.t("megamap.ui.back")
                        } else {
                            match ui::ENTITY_FIELDS[i - 1] {
                                ui::EntityField::Mini => tr.f("megamap.ent.mini", &[&onoff(st.mini)]),
                                ui::EntityField::Map => tr.f("megamap.ent.map", &[&onoff(st.map)]),
                                ui::EntityField::Color => tr.f("megamap.ent.color", &[&format!("{:06X}", st.color & 0xFF_FFFF)]),
                                ui::EntityField::Icon => {
                                    tr.f("megamap.ent.icon", &[&tr.t(entities::ICONS[(st.icon as usize).min(entities::ICONS.len() - 1)])])
                                }
                                ui::EntityField::Size => tr.f("megamap.ent.size", &[&st.px()]),
                                ui::EntityField::Reset => tr.t("megamap.ent.reset"),
                            }
                        };
                        let t = ui::fit(&t, rr.w - 26.0, &width).0;
                        host.text(b, &t, [rr.x + 3.0, rr.y + 2.0], pal.text, false);
                        if i == 1 {
                            // A preview of the current look.
                            host.fill(b, [rr.x + rr.w - 20.0, rr.y - 0.0, 18.0, ROW_PX * 1.0], 0xFF10_1214);
                            let pc = [rr.x + rr.w - 11.0, rr.y + ROW_PX / 2.0];
                            if !self.draw_face(host, b, kind, None, pc, 1.0) {
                                blip(host, b, &st, false, pc);
                            }
                        }
                    }
                    None => {
                        let Some(kind) = ui.panel_rows.get(i) else { break };
                        let st = self.settings.entity_style(kind);
                        host.fill(b, [rr.x + 1.0, rr.y + 1.0, 10.0, ROW_PX - 2.0], 0xFF10_1214);
                        // Rows show the head whatever the radar mode (the blip until the face is cached).
                        let row_st = entities::Style { size: st.size.min(2), ..st };
                        let rc = [rr.x + 6.0, rr.y + ROW_PX / 2.0];
                        if !self.draw_face(host, b, kind, None, rc, 1.0) {
                            blip(host, b, &row_st, false, rc);
                        }
                        let custom = self.settings.entities.contains_key(kind);
                        let shown = self.ent_names.borrow().1.get(kind).cloned().unwrap_or_else(|| entities::display_name(kind));
                        let name = format!("{}{}", shown, if custom { " *" } else { "" });
                        host.text(b, &clip(name), [rr.x + 14.0, rr.y + 2.0], pal.text, false);
                        boxes(host, b, [("M", st.mini), ("W", st.map)]);
                    }
                },
                ui::PanelKind::Dimensions => {
                    let Some(d) = ui.panel_rows.get(i) else { break };
                    let cur = match &self.view_dim {
                        Some(v) => v == d,
                        None => i == 0,
                    };
                    let you = if i == 0 { tr.t("megamap.ui.you") } else { String::new() };
                    let t = format!("{}{}{you}", if cur { "> " } else { "  " }, waypoints::short_dim(d));
                    host.text(b, &clip(t), [rr.x + 3.0, rr.y + 2.0], if cur { 0x4FC3F7 } else { pal.text }, false);
                }
            }
        }
        if len == 0 {
            let t = tr.t(match p.kind {
                ui::PanelKind::Dimensions => "megamap.ui.no_dims",
                _ => "megamap.ui.nothing",
            });
            host.text(b, &t, [r.x + 6.0, ui.panel_row_rect(0).y + 2.0], pal.dim, false);
        }
        if p.kind == ui::PanelKind::Settings {
            self.draw_settings_side(host, b, mouse, pal, &labels, &defaults);
            // A shortened row's full text, over everything.
            if let Some(t) = tooltip {
                let (t, _) = ui::fit(&t, ui.screen.w - 8.0, &width);
                let tw = host.text_width(&t);
                let tx = (mouse[0] + 8.0).min(ui.screen.x + ui.screen.w - tw - 4.0).max(ui.screen.x + 4.0);
                let ty = mouse[1] + 10.0;
                host.fill(b, [tx - 3.0, ty - 2.0, tw + 6.0, 12.0], 0xF010_1010);
                frame_rect(host, b, [tx - 3.0, ty - 2.0, tw + 6.0, 12.0], pal.border);
                host.text(b, &t, [tx, ty], WHITE, true);
            }
            return;
        }
        if let (ui::PanelKind::Entities, Some(k)) = (p.kind, &p.selected) {
            self.draw_entity_preview(host, b, pal, k);
            return;
        }
        // Scroll hint.
        if len > ui.panel_slots() {
            let t = format!("{}-{} / {}", p.scroll + 1, (p.scroll + ui.panel_slots()).min(len), len);
            // Left of the rows' M/W toggles (24 px), on its own backing: over them it hid the last row's toggles.
            let tw = host.text_width(&t);
            let at = [r.x + r.w - tw - 30.0, r.y + r.h - 10.0];
            host.fill(b, [at[0] - 2.0, at[1] - 1.0, tw + 4.0, 10.0], pal.panel);
            host.text(b, &t, at, pal.dim, false);
        }
    }
}

impl State {
    /// The settings window around its content: the category nav, the hint line with the changed counter and the
    /// live preview with its weather switch and the highlight of what the hovered setting changes.
    fn draw_settings_side(&self, host: &dyn Host, b: &mut HudBatch, mouse: [f32; 2], pal: &Pal, labels: &[String], defaults: &[String]) {
        let ui = &self.ui;
        let tr = self.tr();
        let width = |t: &str| host.text_width(t);
        let Some(p) = &ui.panel else { return };
        // Nav.
        for k in 0.. {
            let (Some(nr), Some(c)) = (ui.settings_nav_rect(k), ui.settings_nav_cat(k)) else { break };
            let cat = &settings::CATEGORIES[c];
            let on = p.cat == c && p.search.trim().is_empty();
            let hover = nr.contains(mouse);
            host.fill(b, nr.arr(), if on { pal.panel_hi } else if hover { pal.field } else { pal.panel });
            if on {
                host.fill(b, [nr.x, nr.y, 2.0, nr.h], ACCENT);
            }
            let ic = if on || hover { ACCENT } else { pal.dim | 0xFF00_0000 };
            draw_icon(host, b, cat.icon, [nr.x + 2.0, nr.y + (nr.h - 16.0) / 2.0], ic);
            let n = self.settings.changed_in(c, labels, defaults);
            let badge = if n > 0 { n.to_string() } else { String::new() };
            let bw = if n > 0 { host.text_width(&badge) + 4.0 } else { 0.0 };
            let (name, _) = ui::fit(&tr.t(cat.name), nr.w - 21.0 - bw, &width);
            host.text(b, &name, [nr.x + 20.0, nr.y + (nr.h - 8.0) / 2.0], pal.text, false);
            if n > 0 {
                let br = [nr.x + nr.w - bw - 1.0, nr.y + (nr.h - 10.0) / 2.0, bw, 10.0];
                host.fill(b, br, 0xFF3A_3F48);
                host.text(b, &badge, [br[0] + 2.0, br[1] + 1.0], ACCENT & 0x00FF_FFFF, false);
            }
        }
        // Hint line: the hovered setting's hint, and how many settings differ from the defaults.
        let hover = ui.settings_hover();
        if let Some(hr) = ui.settings_hint_rect() {
            host.fill(b, hr.arr(), pal.field);
            let total = (0..labels.len()).filter(|&i| i != settings::RESET_ALL && labels.get(i) != defaults.get(i)).count();
            let count = tr.f("megamap.ui.changed", &[&total]);
            let cw = host.text_width(&count);
            host.text(b, &count, [hr.x + hr.w - cw - 3.0, hr.y + 2.0], if total > 0 { ACCENT & 0x00FF_FFFF } else { pal.dim }, false);
            let t = tr.t(match hover {
                Some(i) => settings::hint(i),
                None => "megamap.ui.hover_hint",
            });
            let (t, _) = ui::fit(&t, hr.w - cw - 10.0, &width);
            host.text(b, &t, [hr.x + 3.0, hr.y + 2.0], if hover.is_some() { pal.text } else { pal.dim }, false);
        }
        // Preview.
        let (Some(pr), Some(img)) = (ui.settings_preview_rect(), ui.settings_preview_image()) else { return };
        host.fill(b, pr.arr(), 0xFF0C_0E10);
        frame_rect(host, b, pr.arr(), pal.border);
        let n = preview::SIZE as f32;
        let ready = self.slot == Slot::World && self.preview.world_off == preview::SIZE;
        let (cx, cy, half) = (img.x + img.w / 2.0, img.y + img.h / 2.0, img.w / 2.0);
        if ready {
            host.sprite(b, MOD_IMAGE, [0.0, 0.0, n, n], img.arr());
            // Overlay: entity dots and the player, placed with the preview's own view.
            let v = View { center: [self.player[0], self.player[2]], bpp: self.preview.bpp, rotation: self.preview.rot, round: false, fog: false, style: Default::default() };
            let round = p.tab == 0 && self.settings.shape == Shape::Round;
            let inside = |q: [f64; 2]| {
                let (dx, dy) = (q[0] as f32 - half, q[1] as f32 - half);
                if round { dx * dx + dy * dy < (half - 2.0).powi(2) } else { dx.abs() < half - 2.0 && dy.abs() < half - 2.0 }
            };
            if self.settings.radar {
                for (i, bl) in self.radar.blips.iter().enumerate() {
                    let st = self.settings.entity_style(&bl.kind);
                    let q = v.to_image(preview::SIZE, preview::SIZE, bl.x, bl.z);
                    if (if p.tab == 0 { st.mini } else { st.map }) && inside(q) {
                        let live = self.portraits.blip_tex.get(i).and_then(|t| t.as_deref());
                        self.draw_entity(host, b, &st, &bl.kind, live, false, [img.x + q[0] as f32, img.y + q[1] as f32]);
                    }
                }
            }
            let d = if p.tab == 1 || self.settings.north_lock {
                let r = self.yaw.to_radians();
                [-r.sin(), r.cos()]
            } else {
                [0.0, -1.0]
            };
            arrow(host, b, [cx, cy], d, 4.0, 0xFFFF_FFFF);
            if p.tab == 0 && round {
                for i in 0..48 {
                    let (a0, a1) = (i as f32 / 48.0 * std::f32::consts::TAU, (i + 1) as f32 / 48.0 * std::f32::consts::TAU);
                    let q = |a: f32, rr: f32| [cx + a.cos() * rr, cy + a.sin() * rr];
                    host.quad(b, [q(a0, half - 0.5), q(a1, half - 0.5), q(a1, half + 1.0), q(a0, half + 1.0)], style::ring_color((a0 + a1) / 2.0));
                }
            } else {
                frame_rect(host, b, [img.x - 1.0, img.y - 1.0, img.w + 2.0, img.h + 2.0], pal.border);
            }
        } else {
            host.text(b, "...", [cx - 6.0, cy - 4.0], pal.dim, false);
        }
        let tab_name = &tr.t(if p.tab == 0 { "megamap.ui.minimap" } else { "megamap.ui.map" });
        host.text(b, tab_name, [img.x + 2.0, img.y + 2.0], 0xC0C0C0, true);
        // Weather switch (preview only).
        let mut last_y = img.y + img.h;
        for (k, name) in ui::PREVIEW_WEATHERS.iter().enumerate() {
            let Some(wr) = ui.settings_weather_rect(k) else { break };
            let on = p.preview_weather == k;
            host.fill(b, wr.arr(), if on { pal.panel_hi } else if wr.contains(mouse) { pal.field } else { 0xFF16_191D });
            if on {
                host.fill(b, [wr.x, wr.y + wr.h - 1.0, wr.w, 1.0], ACCENT);
            }
            let (name, _) = ui::fit(&tr.t(name), wr.w - 2.0, &width);
            host.text(b, &name, [wr.x + (wr.w - host.text_width(&name)) / 2.0, wr.y + 2.0], pal.text, false);
            last_y = wr.y + wr.h;
        }
        // Info lines as the minimap shows them.
        let info_r = [pr.x + 3.0, last_y + 4.0, pr.w - 6.0, (pr.y + pr.h - last_y - 6.0).max(0.0)];
        let mut ty = info_r[1];
        if p.tab == 0 {
            // The minimap's own block, laid out against the preview image as its frame (a sample warning shows
            // what the hostile setting does when none is near).
            let lines = self.info_block_lines(true);
            // Laid out in the panel's own coordinates (its width is the "screen"), then moved into place.
            let frame = [img.x - 1.0 - info_r[0], -1.0, img.w + 2.0, 0.0];
            let mut shown = infoblock::layout(&lines, frame, infoblock::Align::of(self.settings.info_align), true, info_r[2], &width);
            shown.rect[0] += info_r[0];
            shown.rect[1] += info_r[1];
            for r in &mut shown.rows {
                r.x += info_r[0];
                r.y += info_r[1];
            }
            shown.rows.retain(|r| r.y + 9.0 <= info_r[1] + info_r[3]);
            shown.rect[3] = shown.rect[3].min(info_r[1] + info_r[3] - shown.rect[1]).max(0.0);
            draw_info_block(host, b, &shown);
            ty = shown.rows.last().map_or(ty, |r| r.y - 1.0 + infoblock::ROW_H);
        }
        // Legend: the kinds near the player, by head and shown name.
        let kinds = self.legend_kinds();
        if !kinds.is_empty() && ty + 12.0 <= info_r[1] + info_r[3] {
            ty += 2.0;
            host.fill(b, [info_r[0], ty, info_r[2], 1.0], pal.border);
            ty += 3.0;
            for k in kinds {
                if ty + 11.0 > info_r[1] + info_r[3] {
                    break;
                }
                let c = [info_r[0] + 6.0, ty + 5.0];
                if !self.draw_face(host, b, &k, None, c, 1.0) {
                    let st = self.settings.entity_style(&k);
                    blip(host, b, &entities::Style { size: st.size.min(2), ..st }, false, c);
                }
                let mut name = self.ent_names.borrow().1.get(&k).cloned().unwrap_or_else(|| self.tname("entity", &k));
                while !name.is_empty() && host.text_width(&name) > info_r[2] - 16.0 {
                    name.pop();
                }
                host.text(b, &name, [info_r[0] + 14.0, ty + 1.0], pal.text, false);
                ty += 11.0;
            }
        }
        if !self.clouds_on(false) {
            host.text(b, &tr.t("megamap.ui.clouds_off"), [img.x + 2.0, img.y + img.h - 10.0], 0xA0A0A0, true);
        }
        // What the hovered setting changes, outlined with a slow pulse.
        if let Some(i) = hover {
            let t = settings::target(i);
            let rect = if t == settings::Target::MiniInfo { Some(info_r) } else { preview::target_rect(t, img.arr(), 10.0) };
            if let Some(r) = rect {
                let a = 0.55 + 0.45 * (self.t0.elapsed().as_secs_f32() * 5.0).sin();
                let c = ((a * 255.0) as u32) << 24 | (ACCENT & 0x00FF_FFFF);
                frame_rect(host, b, r, c);
                frame_rect(host, b, [r[0] + 1.0, r[1] + 1.0, r[2] - 2.0, r[3] - 2.0], c);
            }
        }
    }
}

const ROW_PX: f32 = ui::ROW;

fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

// ---------------------------------------------------------------------------------------------------------------
// Entity portraits: faces and 3D renders (see `engine::portrait`).

/// Side of the bench render (`MEGAMAP_PORTRAIT_BENCH`), image pixels.
const ENT_PREVIEW_PX: usize = 96;
const ENT_THUMB_PX: usize = 16;
/// The turning preview's raster side: twice the GUI side, drawn at half a GUI pixel per image pixel.
const ENT_SPIN_PX: usize = 192;
/// The preview's auto spin, degrees a second.
const SPIN_DEG_S: f32 = 36.0;
/// The 3/4 view.
const VIEW_YAW: f32 = -35.0;
const VIEW_PITCH: f32 = 20.0;

fn to_tex(t: Option<horizon_client_render::client_resources::TexturePixels>) -> Option<Arc<portrait::Tex>> {
    let t = t?;
    let tex = portrait::Tex { w: t.width as usize, h: t.height as usize, rgba: t.rgba };
    (tex.w > 0 && tex.h > 0 && tex.rgba.len() >= tex.w * tex.h * 4).then(|| Arc::new(tex))
}

/// The front face (model -Z normal) of the largest polygon of a part tree, as a texture UV rect with the screen
/// left / top first (so a mirrored face comes out mirrored).
fn front_uv(part: &horizon_client::mobs::Part) -> Option<([f32; 4], f32)> {
    let mut best: Option<([f32; 4], f32)> = None;
    for poly in &part.polygons {
        if poly.normal[2] > -0.9 {
            continue;
        }
        let v = &poly.vertices;
        let (mut lo, mut hi) = (0, 0);
        for k in 1..4 {
            if v[k][0] + v[k][1] < v[lo][0] + v[lo][1] {
                lo = k;
            }
            if v[k][0] + v[k][1] > v[hi][0] + v[hi][1] {
                hi = k;
            }
        }
        let area = (v[hi][0] - v[lo][0]).abs() * (v[hi][1] - v[lo][1]).abs();
        if best.is_none_or(|b| area > b.1) {
            best = Some(([v[lo][3], v[lo][4], v[hi][3], v[hi][4]], area));
        }
    }
    best
}

fn find_part<'a>(part: &'a horizon_client::mobs::Part, name: &str) -> Option<&'a horizon_client::mobs::Part> {
    if part.name == name {
        return Some(part);
    }
    part.children.iter().find_map(|c| find_part(c, name))
}

/// A mob's face from its model: the head's front, with a `hat` child laid over it.
fn mob_face(model: &horizon_client::mobs::Part, tex: &portrait::Tex) -> Option<portrait::Face> {
    let head = find_part(model, "head")?;
    let (uv, _) = front_uv(head).or_else(|| head.children.iter().filter(|c| c.name != "hat").find_map(front_uv))?;
    let mut f = portrait::crop_face(tex, uv)?;
    if let Some(hat) = find_part(head, "hat").and_then(front_uv).and_then(|(uv, _)| portrait::crop_face(tex, uv)) {
        portrait::overlay_face(&mut f, &hat);
    }
    // A face with holes (an eye socket) shows the frame behind; make it opaque like the player's.
    if f.iter().filter(|p| *p >> 24 >= 0x80).count() < 24 {
        return None;
    }
    Some(f)
}

/// Renders an entity: players as the skin's box model, mobs through the game's own posed geometry. `live` is the
/// texture the live entity draws with (its variant / skin), if known. Returns RGBA8 `n` x `n` and the time.
fn render_entity(game: &crate::game::Game<'_>, kind: &str, live: Option<&str>, n: usize, yaw: f32, pitch: f32) -> Result<(Vec<u8>, f32), String> {
    let t0 = Instant::now();
    let m = entity_model(game, kind, live)?;
    let img = m.raster(n, yaw, pitch)?;
    Ok((img, t0.elapsed().as_secs_f32() * 1000.0))
}

/// An entity's posed geometry and textures, ready to rasterise at any angle (the preview turns it live).
struct EntityModel {
    verts: Vec<portrait::RVert>,
    texs: Vec<portrait::Tex>,
}

impl EntityModel {
    fn raster(&self, n: usize, yaw: f32, pitch: f32) -> Result<Vec<u8>, String> {
        self.raster_fit(n, yaw, pitch, None)
    }

    /// `zoom`: the preview's fixed frame (`portrait::raster_fit`).
    fn raster_fit(&self, n: usize, yaw: f32, pitch: f32, zoom: Option<f32>) -> Result<Vec<u8>, String> {
        let (img, cov) = portrait::raster_fit(&self.verts, &self.texs, n, yaw, pitch, zoom);
        if cov == 0 {
            return Err(if self.texs.iter().all(|t| t.w == 0) { "no texture".into() } else { "empty".into() });
        }
        Ok(img)
    }
}

fn entity_model(game: &crate::game::Game<'_>, kind: &str, live: Option<&str>) -> Result<EntityModel, String> {
    if kind == "minecraft:player" {
        let name = live.unwrap_or("entity/player/wide/steve");
        let tex = to_tex(game.megamap_client_texture(name)).ok_or_else(|| format!("no skin {name}"))?;
        let verts = portrait::player_model(name.contains("/slim/"), tex.h * 2 == tex.w);
        return Ok(EntityModel { verts, texs: vec![(*tex).clone()] });
    }
    if let Some(r) = object_model(game, kind) {
        return r;
    }
    let kt = game.res.mob_types().get(kind).cloned().ok_or_else(|| "no model".to_string())?;
    let mob = horizon_client::mobs::Mob::new(u32::MAX, kt, horizon_core::geometry::Vec3::ZERO, 0.0);
    let data = game.res.mob_data();
    let names = horizon_client::mobs::render::drawn_textures(&mob, data);
    let body = horizon_client::mobs::render::body(&mob).1.into_owned();
    let texs: Vec<portrait::Tex> = names
        .iter()
        .map(|nm| {
            let src = if *nm == body { live.unwrap_or(nm) } else { nm.as_str() };
            to_tex(game.megamap_client_texture(src)).map(|t| (*t).clone()).unwrap_or_default()
        })
        .collect();
    let lookup = |name: &str| names.iter().position(|n| n == name).map(portrait::virtual_rect);
    let mut out = Vec::new();
    horizon_client::mobs::render::render_posed(&mob, 0.0, glam::Mat4::IDENTITY, &lookup, 0xF0F0, data, &mut out);
    if out.is_empty() {
        return Err("no geometry".into());
    }
    let rv: Vec<portrait::RVert> = out
        .chunks_exact(4)
        .filter(|q| q[0].layer != 13 && q[0].layer != horizon_client::mobs::render::ARMOR_GLINT)
        .flatten()
        .map(|v| portrait::RVert { pos: v.pos, uv: v.uv, color: v.color, normal: v.normal })
        .collect();
    Ok(EntityModel { verts: rv, texs })
}

/// The model part, base transform and texture of an entity that is not a mob type: boats and rafts, minecarts
/// (the bare cart, without the block it carries), armour stands, arrows and tridents. The transforms are the
/// game's own renderers' at yaw 0, without the world position.
fn object_part<'g>(game: &'g crate::game::Game<'_>, kind: &str) -> Option<(&'g horizon_client::mobs::Part, glam::Mat4, String)> {
    use glam::{Mat4, Vec3};
    let flip = Mat4::from_scale(Vec3::new(-1.0, -1.0, 1.0));
    let boat = match kind {
        "minecraft:boat" => Some(("oak", false)),
        "minecraft:chest_boat" => Some(("oak", true)),
        k => crate::game::boats::any_boat_wood(k),
    };
    if let Some((wood, chest)) = boat {
        let models = if chest { &game.boat_models.chest_models } else { &game.boat_models.models };
        let base = Mat4::from_translation(Vec3::new(0.0, 0.375, 0.0)) * Mat4::from_rotation_y(180f32.to_radians()) * flip * Mat4::from_rotation_y(90f32.to_radians());
        let folder = if chest { "chest_boat" } else { "boat" };
        return Some((models.get(wood)?, base, format!("entity/{folder}/{wood}")));
    }
    if let Some(ck) = crate::game::minecarts::CartKind::from_id(kind) {
        let base = Mat4::from_translation(Vec3::new(0.0, 0.375, 0.0)) * Mat4::from_rotation_y(180f32.to_radians()) * flip;
        return Some((game.cart_models.models.get(&ck)?, base, "entity/minecart/minecart".into()));
    }
    match kind {
        "minecraft:armor_stand" => {
            let base = Mat4::from_rotation_y(180f32.to_radians()) * flip * Mat4::from_translation(Vec3::new(0.0, -1.501, 0.0));
            Some((game.armor_stand_assets.main.as_deref()?, base, "entity/armorstand/armorstand".into()))
        }
        "minecraft:arrow" | "minecraft:spectral_arrow" => {
            let tex = if kind == "minecraft:arrow" { "entity/projectiles/arrow" } else { "entity/projectiles/arrow_spectral" };
            Some((game.arrow_model.as_ref()?, Mat4::from_rotation_y((-90f32).to_radians()), tex.into()))
        }
        "minecraft:trident" => {
            let base = Mat4::from_rotation_y((-90f32).to_radians()) * Mat4::from_rotation_z(90f32.to_radians());
            Some((game.trident_model.as_ref()?, base, "entity/trident/trident".into()))
        }
        _ => None,
    }
}

/// `entity_model` for `object_part`'s kinds; `None` for any other kind.
fn object_model(game: &crate::game::Game<'_>, kind: &str) -> Option<Result<EntityModel, String>> {
    if let Some((verts, names)) = plain_model(kind) {
        let mut texs = Vec::with_capacity(names.len());
        for nm in names {
            let Some(t) = to_tex(game.megamap_client_texture(nm)) else {
                return Some(Err(format!("no texture {nm}")));
            };
            texs.push((*t).clone());
        }
        return Some(Ok(EntityModel { verts, texs }));
    }
    let (model, base, tex_name) = object_part(game, kind)?;
    let Some(tex) = to_tex(game.megamap_client_texture(&tex_name)) else {
        return Some(Err(format!("no texture {tex_name}")));
    };
    let mut poses = horizon_client::mobs::anim::Poses::new(model);
    // ArmorStand: the arms are hidden unless ShowArms is set (vanilla's default).
    if kind == "minecraft:armor_stand" {
        for name in ["left_arm", "right_arm"] {
            if poses.has(name) {
                let p = poses.get(name);
                p[6] = 0.0;
                p[7] = 0.0;
                p[8] = 0.0;
            }
        }
    }
    let mut out = Vec::new();
    horizon_client::mobs::render::draw(model, base, &poses, portrait::virtual_rect(0), 0xFFFF_FFFF, 0xF0F0, 0, &mut out);
    if out.is_empty() {
        return Some(Err("no geometry".into()));
    }
    let verts = out.iter().map(|v| portrait::RVert { pos: v.pos, uv: v.uv, color: v.color, normal: v.normal }).collect();
    Some(Ok(EntityModel { verts, texs: vec![(*tex).clone()] }))
}

/// The item texture a thrown or flying entity is drawn with (ThrownItemRenderer and the like).
fn thrown_item_texture(kind: &str) -> Option<&'static str> {
    Some(match kind.strip_prefix("minecraft:")? {
        "snowball" => "item/snowball",
        "egg" => "item/egg",
        "ender_pearl" => "item/ender_pearl",
        "eye_of_ender" => "item/ender_eye",
        "experience_bottle" => "item/experience_bottle",
        "fireball" | "small_fireball" => "item/fire_charge",
        "firework_rocket" => "item/firework_rocket",
        "wind_charge" => "item/wind_charge",
        "potion" | "splash_potion" => "item/splash_potion",
        "lingering_potion" => "item/lingering_potion",
        _ => return None,
    })
}

/// Entities drawn from block textures or as a flat sprite rather than an entity model: primed TNT and item
/// frames as their block models, experience orbs and thrown items as a camera-facing card. The vertices and the
/// texture of each slot. A dropped item (`minecraft:item`) has no look of its own, so it is not here.
fn plain_model(kind: &str) -> Option<(Vec<portrait::RVert>, Vec<&'static str>)> {
    let white = 0xFFFF_FFFF;
    match kind {
        "minecraft:tnt" => {
            let mut v = Vec::new();
            portrait::block_box(&mut v, [0.0; 3], [16.0; 3], [0, 0, 0, 0, 1, 2], white);
            Some((v, vec!["block/tnt_side", "block/tnt_top", "block/tnt_bottom"]))
        }
        "minecraft:item_frame" | "minecraft:glow_item_frame" => {
            // item_frame.json: the back plate and four birch plank bars, against the wall at z 15..16.
            let back = if kind == "minecraft:item_frame" { "block/item_frame" } else { "block/glow_item_frame" };
            let mut v = Vec::new();
            portrait::block_box(&mut v, [3.0, 3.0, 15.5], [13.0, 13.0, 16.0], [0, 0, 1, 1, 1, 1], white);
            for (from, to) in [
                ([2.0, 2.0, 15.0], [14.0, 3.0, 16.0]),
                ([2.0, 13.0, 15.0], [14.0, 14.0, 16.0]),
                ([2.0, 3.0, 15.0], [3.0, 13.0, 16.0]),
                ([13.0, 3.0, 15.0], [14.0, 13.0, 16.0]),
            ] {
                portrait::block_box(&mut v, from, to, [1; 6], white);
            }
            Some((v, vec![back, "block/birch_planks"]))
        }
        "minecraft:experience_orb" => {
            // ExperienceOrbRenderer: the first of the 4 x 4 frames, tinted at its colour's start.
            let mut v = portrait::flat_card();
            for p in &mut v {
                p.uv = [p.uv[0] * 0.25, p.uv[1] * 0.25];
                p.color = 0xFF03_FF80;
            }
            // 26.x keeps it under entity/experience/ (older packs: entity/experience_orb).
            Some((v, vec!["entity/experience/experience_orb"]))
        }
        k => thrown_item_texture(k).map(|t| (portrait::flat_card(), vec![t])),
    }
}

/// The texture an `object_part` / `plain_model` kind draws with (its icon's source).
fn object_texture(game: &crate::game::Game<'_>, kind: &str) -> Option<String> {
    if let Some((_, names)) = plain_model(kind) {
        return names.first().map(|n| n.to_string());
    }
    object_part(game, kind).map(|(_, _, t)| t)
}

/// The angle of an object's 8 x 8 icon: flat cards face the camera, models are seen at the preview's 3/4 angle;
/// `None` for mobs.
fn object_icon_view(game: &crate::game::Game<'_>, kind: &str) -> Option<(f32, f32)> {
    let flat = kind == "minecraft:experience_orb" || thrown_item_texture(kind).is_some();
    object_texture(game, kind).map(|_| if flat { (0.0, 0.0) } else { (VIEW_YAW, VIEW_PITCH) })
}

/// Development bench (`MEGAMAP_PORTRAIT_BENCH=<file>`): renders every mob type once at the preview size and
/// writes the time per mob and the failures to the file.
fn portrait_bench(game: &crate::game::Game<'_>, path: &str) {
    let mut kinds: Vec<String> = game.res.mob_types().keys().cloned().collect();
    kinds.sort();
    kinds.insert(0, "minecraft:player".into());
    for k in ["minecraft:boat", "minecraft:oak_boat", "minecraft:spruce_chest_boat", "minecraft:bamboo_raft", "minecraft:bamboo_chest_raft", "minecraft:minecart", "minecraft:chest_minecart", "minecraft:hopper_minecart", "minecraft:armor_stand", "minecraft:arrow", "minecraft:spectral_arrow", "minecraft:trident", "minecraft:item", "minecraft:experience_orb", "minecraft:item_frame", "minecraft:glow_item_frame", "minecraft:tnt", "minecraft:snowball", "minecraft:ender_pearl", "minecraft:fireball", "minecraft:splash_potion"] {
        if !kinds.iter().any(|x| x == k) {
            kinds.push(k.into());
        }
    }
    let mut lines = Vec::new();
    let (mut ok, mut total_ms, mut worst) = (0, 0.0f32, (0.0f32, String::new()));
    let mut faces = 0;
    for k in &kinds {
        match render_entity(game, k, None, ENT_PREVIEW_PX, VIEW_YAW, VIEW_PITCH) {
            Ok((_, ms)) => {
                ok += 1;
                total_ms += ms;
                if ms > worst.0 {
                    worst = (ms, k.clone());
                }
                lines.push(format!("ok   {k} {ms:.2} ms"));
            }
            Err(e) => lines.push(format!("FAIL {k}: {e}")),
        }
        if let Some(kt) = game.res.mob_types().get(k) {
            let mob = horizon_client::mobs::Mob::new(u32::MAX, kt.clone(), horizon_core::geometry::Vec3::ZERO, 0.0);
            let body = horizon_client::mobs::render::body(&mob).1.into_owned();
            if to_tex(game.megamap_client_texture(&body)).is_some_and(|t| mob_face(&kt.model, &t).is_some()) {
                faces += 1;
            } else {
                lines.push(format!("face-fallback {k}"));
            }
        }
    }
    let head = format!(
        "portraits: {ok}/{} rendered at {ENT_PREVIEW_PX}px, mean {:.2} ms, worst {:.2} ms ({}); model faces {faces}/{}\n", // lang-ok: diagnostics file
        kinds.len(),
        total_ms / ok.max(1) as f32,
        worst.0,
        worst.1,
        game.res.mob_types().len()
    );
    let _ = std::fs::write(path, head + &lines.join("\n"));
}

impl State {
    fn ent_mode(&self, kind: &str) -> u8 {
        self.settings.ent_mode[entities::category(kind).index()].min(2)
    }

    /// The default body texture of a kind (players: Steve).
    fn kind_texture(&mut self, game: &crate::game::Game<'_>, kind: &str) -> Option<String> {
        if let Some(t) = self.portraits.kind_tex.get(kind) {
            return t.clone();
        }
        let t = if kind == "minecraft:player" {
            Some("entity/player/wide/steve".to_string())
        } else {
            game.res.mob_types().get(kind).map(|kt| {
                let mob = horizon_client::mobs::Mob::new(u32::MAX, kt.clone(), horizon_core::geometry::Vec3::ZERO, 0.0);
                horizon_client::mobs::render::body(&mob).1.into_owned()
            })
            .or_else(|| object_texture(game, kind))
        };
        self.portraits.kind_tex.insert(kind.to_string(), t.clone());
        t
    }

    /// Fills the portrait caches for what is on screen: radar icons in face / 3D mode, the entities window's rows
    /// and its selected kind. At most two texture decodes and one 3D render a frame.
    fn portraits_tick(&mut self, game: &crate::game::Game<'_>) {
        if !self.started {
            return;
        }
        if !self.portraits.bench_done {
            self.portraits.bench_done = true;
            if let Ok(path) = std::env::var("MEGAMAP_PORTRAIT_BENCH") {
                portrait_bench(game, &path);
            }
        }
        let mut want: Vec<(String, Option<String>, usize)> = Vec::new();
        if self.settings.radar {
            for (i, bl) in self.radar.blips.iter().enumerate() {
                let m = self.ent_mode(&bl.kind);
                if m > 0 {
                    let t = self.portraits.blip_tex.get(i).cloned().flatten();
                    want.push((bl.kind.clone(), t, if m == 1 { 0 } else { ENT_THUMB_PX }));
                }
            }
        }
        if self.map_open {
            if let Some(p) = self.ui.panel.as_ref().filter(|p| p.kind == ui::PanelKind::Entities) {
                match &p.selected {
                    Some(k) => {
                        let k = k.clone();
                        self.spin_tick(game, &k);
                        want.push((k, None, 0));
                    }
                    None => {
                        for i in p.scroll..(p.scroll + self.ui.panel_slots()).min(self.ui.panel_rows.len()) {
                            let k = &self.ui.panel_rows[i];
                            want.push((k.clone(), None, 0));
                        }
                    }
                }
            }
        }
        // The settings preview's legend shows heads whatever the radar mode.
        if self.map_open && self.ui.is_settings() {
            for k in self.legend_kinds() {
                want.push((k, None, 0));
            }
        }
        let mut decode = portrait::Budget::new(2);
        let mut render = portrait::Budget::new(1);
        for (kind, live, size) in want {
            let tex = match live {
                Some(t) => Some(t),
                None => self.kind_texture(game, &kind),
            };
            if size > 0 {
                let key = format!("{}|{size}", pkey(&kind, tex.as_deref()));
                if self.portraits.renders.contains_key(&key) || !render.take() {
                    continue;
                }
                let r = match render_entity(game, &kind, tex.as_deref(), size, VIEW_YAW, VIEW_PITCH) {
                    Ok((img, ms)) => Some(Arc::new(Render3d { n: size, runs: portrait::image_runs(&img, size), ms })),
                    Err(e) => {
                        if !self.portraits.failed.iter().any(|(k, _)| *k == kind) {
                            self.portraits.failed.push((kind.clone(), e));
                        }
                        None
                    }
                };
                self.portraits.renders.insert(key, r);
                continue;
            }
            let key = pkey(&kind, tex.as_deref());
            if self.portraits.faces.contains_key(&key) {
                continue;
            }
            let Some(name) = tex else {
                self.portraits.faces.insert(key, None);
                continue;
            };
            let t = match self.portraits.tex.get(&name) {
                Some(t) => t.clone(),
                None => {
                    if !decode.take() {
                        continue;
                    }
                    let t = to_tex(game.megamap_client_texture(&name));
                    self.portraits.tex.insert(name.clone(), t.clone());
                    t
                }
            };
            let face = t.and_then(|t| {
                if kind == "minecraft:player" {
                    portrait::player_face(&t)
                } else {
                    game.res.mob_types().get(&kind).and_then(|kt| mob_face(&kt.model, &t))
                }
            });
            // No head on the model (or no texture): an 8 x 8 front render of the whole mob, if one may run now.
            let face = match face {
                Some(f) => Some(f),
                None => {
                    if !render.take() {
                        continue;
                    }
                    // Boats, carts and the like read better at the preview's 3/4 angle.
                    let (yaw, pitch) = object_icon_view(game, &kind).unwrap_or((0.0, 0.0));
                    render_entity(game, &kind, Some(&name), 8, yaw, pitch).ok().map(|(img, _)| {
                        let mut f = [0u32; 64];
                        for (i, px) in img.chunks_exact(4).enumerate().take(64) {
                            f[i] = (px[3] as u32) << 24 | (px[0] as u32) << 16 | (px[1] as u32) << 8 | px[2] as u32;
                        }
                        f
                    })
                }
            };
            self.portraits.faces.insert(key, face);
        }
        // Keep the caches bounded (a long session meets many skins).
        if self.portraits.faces.len() > 512 {
            self.portraits.faces.clear();
        }
        if self.portraits.renders.len() > 256 {
            self.portraits.renders.clear();
        }
        if self.portraits.tex.len() > 256 {
            self.portraits.tex.clear();
        }
    }

    /// Keeps the entities window's preview turning: the auto spin plus the mouse's turn, re-rasterised when the
    /// angle moved, at most every 33 ms (16 ms while dragged) and never more than a third of the frame time.
    fn spin_tick(&mut self, game: &crate::game::Game<'_>, kind: &str) {
        let tex = self.kind_texture(game, kind);
        let key = pkey(kind, tex.as_deref());
        let now = Instant::now();
        if self.portraits.spin.as_ref().is_none_or(|s| s.key != key) {
            let model = match entity_model(game, kind, tex.as_deref()) {
                Ok(m) => Some(Arc::new(m)),
                Err(e) => {
                    if !self.portraits.failed.iter().any(|(k, _)| k == kind) {
                        self.portraits.failed.push((kind.to_string(), e));
                    }
                    None
                }
            };
            let old = Instant::now() - Duration::from_secs(10);
            self.portraits.spin = Some(Spin { key, model, render: None, yaw: 0.0, pitch: 0.0, zoom: 1.0, auto: 0.0, rendered: old, ticked: now, held: old });
        }
        let drag = self.ui.spin_drag;
        let (uy, up) = (self.ui.spin_yaw, self.ui.spin_pitch);
        let failed = &mut self.portraits.failed;
        let Some(sp) = self.portraits.spin.as_mut() else { return };
        let dt = now.duration_since(sp.ticked).as_secs_f32().min(0.1);
        sp.ticked = now;
        if drag {
            sp.held = now;
        } else if sp.held.elapsed().as_secs_f32() > 1.2 {
            sp.auto = (sp.auto + dt * SPIN_DEG_S).rem_euclid(360.0);
        }
        let Some(model) = sp.model.clone() else { return };
        let (yaw, pitch) = (VIEW_YAW + sp.auto + uy, VIEW_PITCH + up);
        let turned = (yaw - sp.yaw + 180.0).rem_euclid(360.0) - 180.0;
        let zoom = self.ui.spin_zoom;
        let moved = sp.render.is_none() || turned.abs() >= 1.0 || (pitch - sp.pitch).abs() >= 0.5 || zoom != sp.zoom;
        let last_ms = sp.render.as_ref().map_or(0.0, |r| r.ms);
        let gap = (if drag { 16.0 } else { 33.0f32 }).max(last_ms * 3.0);
        if !moved || sp.rendered.elapsed().as_secs_f32() * 1000.0 < gap {
            return;
        }
        let t0 = Instant::now();
        match model.raster_fit(ENT_SPIN_PX, yaw, pitch, Some(zoom)) {
            Ok(img) => {
                let ms = t0.elapsed().as_secs_f32() * 1000.0;
                sp.render = Some(Render3d { n: ENT_SPIN_PX, runs: portrait::image_runs(&img, ENT_SPIN_PX), ms });
            }
            Err(e) => {
                if sp.render.is_none() {
                    sp.model = None;
                    if !failed.iter().any(|(k, _)| k == kind) {
                        failed.push((kind.to_string(), e));
                    }
                }
            }
        }
        sp.yaw = yaw;
        sp.pitch = pitch;
        sp.zoom = zoom;
        sp.rendered = now;
    }

    /// Draws one entity at `c` in its category's mode: the face (framed in the category colour), the 3D icon, or
    /// the blip (also while the portrait is not ready, and for a flashing urgent blip).
    fn draw_entity(&self, host: &dyn Host, b: &mut HudBatch, st: &entities::Style, kind: &str, live: Option<&str>, flash: bool, c: [f32; 2]) {
        let m = self.ent_mode(kind);
        if m == 0 || flash {
            blip(host, b, st, flash, c);
            return;
        }
        let tex = live.map(str::to_string).or_else(|| self.portraits.kind_tex.get(kind).cloned().flatten());
        let key = pkey(kind, tex.as_deref());
        let frame = self.settings.frame_px();
        let fc = entities::category(kind).frame_color();
        if m == 1 {
            let px = (st.px() * 2.0 / 8.0).round().max(1.0);
            if !self.draw_face(host, b, kind, tex.as_deref(), c, px) {
                blip(host, b, st, false, c);
            }
            return;
        }
        let Some(Some(r)) = self.portraits.renders.get(&format!("{key}|{ENT_THUMB_PX}")) else {
            blip(host, b, st, false, c);
            return;
        };
        let side = r.n as f32;
        let (x0, y0) = (snap_half(c[0] - side / 2.0), snap_half(c[1] - side / 2.0));
        aa_frame(host, b, [x0, y0, side, side], frame, fc);
        for &(x, y, len, col) in &r.runs {
            host.fill(b, [x0 + x as f32, y0 + y as f32, len as f32, 1.0], col);
        }
    }

    /// The settings preview legend: the distinct kinds on the radar now, hostile first, at most five.
    fn legend_kinds(&self) -> Vec<String> {
        if !self.settings.radar {
            return Vec::new();
        }
        let mut v: Vec<String> = Vec::new();
        for bl in &self.radar.blips {
            if !v.contains(&bl.kind) {
                v.push(bl.kind.clone());
            }
        }
        v.sort_by_key(|k| (entities::category(k), k.clone()));
        v.truncate(5);
        v
    }

    /// The 8 x 8 face of `kind` (skin `tex`, else its default) centred at `c`, `px` GUI px per face pixel, in the
    /// icon frame setting's anti-aliased frame (none: a soft dark edge). False while the face is not cached.
    fn draw_face(&self, host: &dyn Host, b: &mut HudBatch, kind: &str, tex: Option<&str>, c: [f32; 2], px: f32) -> bool {
        let tex = tex.map(str::to_string).or_else(|| self.portraits.kind_tex.get(kind).cloned().flatten());
        let Some(Some(face)) = self.portraits.faces.get(&pkey(kind, tex.as_deref())) else { return false };
        let side = px * 8.0;
        let (x0, y0) = (snap_half(c[0] - side / 2.0), snap_half(c[1] - side / 2.0));
        let frame = self.settings.frame_px();
        if frame > 0.0 {
            aa_frame(host, b, [x0, y0, side, side], frame, entities::category(kind).frame_color());
        } else {
            host.fill(b, [x0 - 0.5, y0 - 0.5, side + 1.0, side + 1.0], 0xA000_0000);
        }
        for (x, y, len, col) in portrait::runs(8, 8, |x, y| face[y * 8 + x]) {
            host.fill(b, [x0 + x as f32 * px, y0 + y as f32 * px, len as f32 * px, px], col);
        }
        true
    }

    /// The entities window's detail view: the selected kind in 3D under the rows.
    fn draw_entity_preview(&self, host: &dyn Host, b: &mut HudBatch, pal: &Pal, kind: &str) {
        let r = self.ui.panel_rect();
        let Some(pr) = self.ui.entity_preview_rect() else { return };
        let (x0, y0, side) = (pr.x, pr.y, pr.w);
        host.fill(b, [x0, y0, side, side], 0xFF10_1214);
        let fc = entities::category(kind).frame_color();
        frame_rect(host, b, [x0 - 1.0, y0 - 1.0, side + 2.0, side + 2.0], fc);
        let tex = self.portraits.kind_tex.get(kind).cloned().flatten();
        let key = pkey(kind, tex.as_deref());
        let spin = self.portraits.spin.as_ref().filter(|s| s.key == key);
        let note = match spin.map(|s| (s.model.is_some(), s.render.as_ref())) {
            Some((_, Some(rd))) => {
                let k = side / rd.n as f32;
                for &(x, y, len, col) in &rd.runs {
                    host.fill(b, [x0 + x as f32 * k, y0 + y as f32 * k, len as f32 * k, k], col);
                }
                format!("3D {:.1} ms", rd.ms)
            }
            Some((false, None)) => {
                let why = self.portraits.failed.iter().find(|(k, _)| k == kind).map_or("?", |(_, e)| e.as_str());
                let pc = [x0 + side / 2.0, y0 + side / 2.0];
                if !self.draw_face(host, b, kind, tex.as_deref(), pc, 6.0) {
                    blip(host, b, &self.settings.entity_style(kind), false, pc);
                }
                self.tr().f("megamap.ui.no3d", &[&why])
            }
            _ => self.tr().t("megamap.ui.rendering"),
        };
        // The face beside the render, as the radar shows it.
        if let Some(Some(face)) = self.portraits.faces.get(&pkey(kind, tex.as_deref())) {
            let (fx, fy) = (x0 + side + 6.0, y0);
            if fx + 18.0 < r.x + r.w {
                host.fill(b, [fx - 1.0, fy - 1.0, 18.0, 18.0], fc);
                for (x, y, len, col) in portrait::runs(8, 8, |x, y| face[y * 8 + x]) {
                    host.fill(b, [fx + x as f32 * 2.0, fy + y as f32 * 2.0, len as f32 * 2.0, 2.0], col);
                }
            }
        }
        // A long reason ("no texture entity/...") is cut at the panel's edge.
        let mut note = note;
        let room = r.x + r.w - x0 - 4.0;
        while note.chars().count() > 1 && host.text_width(&note) > room {
            note.pop();
        }
        host.text(b, &note, [x0, y0 + side + 2.0], pal.dim, false);
    }
}

// ---------------------------------------------------------------------------------------------------------------
// Hooks.

// Entry points for the bundle's helpers, one per hook point. The helpers check Options > Mods
// (`Game::native_on`) before calling any of these; a turned-off megamap does nothing, draws nothing
// and lets every screen event through.

pub(super) fn frame(game: &mut crate::game::Game<'_>) {
    let mut s = lock();
    s.frame(&mut *game);
    s.far_upkeep(game);
    s.portraits_tick(game);
}

pub(super) fn level_load(host: &mut dyn Host, dimension: &str) {
    // The frame notices the change too; this makes a dimension switch take effect before its first frame.
    let mut s = lock();
    if s.started && dimension != s.dim {
        let world = host.client_state().and_then(|st| st.world.as_ref().map(world_key));
        s.enter(world, dimension);
    }
}

pub(super) fn disconnecting() {
    let mut s = lock();
    // Written here and now: the worker may not get another chance.
    if let Some(dir) = s.surface.dir.clone() {
        let regions = s.surface.store.dirty_copies();
        write_now(&dir, regions);
    }
    s.persist_small(true);
}

pub(super) fn hud(host: &mut dyn Host, batch: &mut HudBatch, corners: &mut Corners) {
    lock().draw_hud(host, batch, corners);
}

pub(super) fn key(host: &mut dyn Host, key: &str, pressed: bool) {
    if !pressed {
        return;
    }
    let mut s = lock();
    if !s.in_world || !host.client_state().is_some_and(|st| st.screen == NativeScreenKind::Playing) {
        return;
    }
    let k = key;
    let mut save = true;
    if k == s.keys.map {
        s.queue.push(Queued::OpenMap { list: false, new_wp: false });
        save = false;
    } else if k == s.keys.waypoint_here {
        s.queue.push(Queued::OpenMap { list: false, new_wp: true });
        save = false;
    } else if k == s.keys.waypoints {
        s.queue.push(Queued::OpenMap { list: true, new_wp: false });
        save = false;
    } else if k == s.keys.zoom {
        s.settings.cycle_zoom();
        let z = 1.0 / s.settings.zoom_bpp();
        let msg = s.tr().f(if s.settings.autozoom { "megamap.toast.autozoom" } else { "megamap.toast.mini_zoom" }, &[&z]);
        s.toast(msg);
    } else if k == s.keys.size {
        s.settings.cycle_size();
    } else if k == s.keys.mode {
        s.settings.cycle_mode();
        let tr = s.tr();
        let shown = tr.t(if s.settings.mode == Mode::Hidden { "megamap.v.hidden" } else { "megamap.v.shown" });
        s.toast(tr.f("megamap.toast.mini_mode", &[&shown]));
    } else if k == s.keys.rotate {
        s.settings.north_lock = !s.settings.north_lock;
        let msg = s.tr().t(if s.settings.north_lock { "megamap.toast.north_up" } else { "megamap.toast.rotating" });
        s.toast(msg);
    } else {
        return;
    }
    if save {
        s.save_settings();
    }
}

pub(super) fn commands(commands: &mut Vec<&'static str>) {
    commands.push(ID);
}

/// True when the line was megamap's (and has run).
pub(super) fn command(host: &mut dyn Host, command: &str) -> bool {
    let c = command.trim().trim_start_matches('/');
    if c != ID && !c.starts_with("megamap ") {
        return false;
    }
    lock().command(host, c);
    true
}

pub(super) fn chat(message: &Text) {
    let text = message.plain();
    if let Some(found) = waypoints::parse_share(&text) {
        let mut s = lock();
        let msg = s.tr().f("megamap.toast.wp_shared", &[&found.0]);
        s.last_shared = Some(found);
        s.toast(msg);
    }
}

static LAST_MINI_RECT: Mutex<Option<[f32; 4]>> = Mutex::new(None);

fn set_last_mini_rect(rect: Option<[f32; 4]>) {
    *LAST_MINI_RECT.lock().unwrap_or_else(|e| e.into_inner()) = rect;
}

/// Last drawn minimap frame `[x, y, w, h]` in GUI pixels (for Android tap-to-open).
#[cfg_attr(not(feature = "android"), allow(dead_code))]
pub(in crate::game) fn minimap_hud_rect() -> Option<[f32; 4]> {
    *LAST_MINI_RECT.lock().unwrap_or_else(|e| e.into_inner())
}

/// Opens the fullscreen world map on the next frame (used by Android tap-on-minimap).
#[cfg_attr(not(feature = "android"), allow(dead_code))]
pub(in crate::game) fn open_world_map() {
    let mut s = lock();
    if s.in_world && !s.map_open {
        s.queue.push(Queued::OpenMap { list: false, new_wp: false });
    }
}

/// Whether Megamap currently has a focused text box (search or waypoint editor).
pub(super) fn is_typing() -> bool {
    lock().ui.typing()
}

/// Closes Megamap screen if open (when player disables Megamap in Options > Mods).
pub(super) fn close_if_open(host: &mut dyn Host) {
    if host.mod_screen() == Some(ID) {
        host.mod_screen_close(ID);
    }
    set_last_mini_rect(None);
}

/// Options > Mods row and key mappings for Megamap.
pub(super) fn installed_row() -> (Installed, Vec<KeyMapping>) {
    let tr = lock().tr();
    let row = Installed {
        id: ID.into(),
        title: format!("Megamap {VERSION}"), // lang-ok
        description: tr.t("megamap.mod.desc"),
        problem: None,
        dev: false,
        native: None,
    };
    let mappings = KEYS
        .iter()
        .map(|(id, label_key, default_key)| KeyMapping {
            id: (*id).to_owned(),
            label: tr.t(label_key),
            key: (*default_key).to_owned(),
            modifiers: 0,
            context: KeyConflictContext::Universal,
            category: String::new(),
        })
        .collect();
    (row, mappings)
}

/// Curated top-level Options > Mods settings synced with Megamap settings.txt
/// (full 75-item categorized UI with live preview remains in-map via M -> S).
pub(super) fn page_settings() -> Vec<NativeSetting> {
    let s = lock();
    let tr = s.tr();
    let st = &s.settings;
    let toggle = |key: &str, label: String, on: bool| NativeSetting {
        key: key.to_owned(),
        label,
        tooltip: None,
        value: NativeSettingValue::Toggle(on),
    };
    let choice = |key: &str, label: String, options: Vec<String>, index: usize| NativeSetting {
        key: key.to_owned(),
        label,
        tooltip: None,
        value: NativeSettingValue::Choice { options, index },
    };
    let corner_idx = match st.corner {
        settings::Corner::TopLeft => 0,
        settings::Corner::TopRight => 1,
        settings::Corner::BottomLeft => 2,
        settings::Corner::BottomRight => 3,
    };
    vec![
        toggle("shown", tr.t("megamap.set.mode"), st.mode == Mode::Shown),
        choice(
            "shape",
            tr.t("megamap.set.shape"),
            vec![tr.t("megamap.v.square"), tr.t("megamap.v.round")],
            usize::from(st.shape == Shape::Round),
        ),
        choice(
            "corner",
            tr.t("megamap.set.corner"),
            vec![
                tr.t("megamap.corner.top_left"),
                tr.t("megamap.corner.top_right"),
                tr.t("megamap.corner.bottom_left"),
                tr.t("megamap.corner.bottom_right"),
            ],
            corner_idx,
        ),
        choice(
            "size",
            tr.t("megamap.set.size"),
            settings::SIZES.iter().map(|px| format!("{px} px")).collect(),
            st.size.min(settings::SIZES.len() - 1),
        ),
        choice(
            "zoom",
            tr.t("megamap.set.zoom"),
            settings::ZOOMS.iter().map(|z| format!("{}x", 1.0 / z)).collect(),
            st.zoom.min(settings::ZOOMS.len() - 1),
        ),
        toggle("north_lock", tr.t("megamap.set.north_lock"), st.north_lock),
        choice(
            "layer",
            tr.t("megamap.set.layer"),
            vec![
                tr.t("megamap.layer.auto"),
                tr.t("megamap.layer.surface"),
                tr.t("megamap.layer.cave"),
                tr.t("megamap.layer.fixed_y"),
            ],
            (st.layer as usize).min(3),
        ),
        toggle("fog", tr.t("megamap.set.fog"), st.fog),
        toggle("clouds", tr.t("megamap.set.clouds"), st.clouds),
        toggle("far", tr.t("megamap.set.far"), st.far),
        toggle("radar", tr.t("megamap.set.radar"), st.radar),
        toggle("beams", tr.t("megamap.set.beams"), st.beams),
        toggle("trail", tr.t("megamap.set.trail"), st.trail),
        choice(
            "lang",
            tr.t("megamap.set.lang"),
            vec![tr.t("megamap.v.as_game"), "English".into(), "Русский".into()], // lang-ok
            (st.lang as usize).min(2),
        ),
    ]
}

/// Applies an Options > Mods change to Megamap and persists settings.txt.
pub(super) fn setting_event(key: &str, value: &NativeSettingValue) -> bool {
    let mut s = lock();
    match (key, value) {
        ("shown", NativeSettingValue::Toggle(on)) => {
            s.settings.mode = if *on { Mode::Shown } else { Mode::Hidden };
        }
        ("shape", NativeSettingValue::Choice { index, .. }) => {
            s.settings.shape = if *index == 1 { Shape::Round } else { Shape::Square };
        }
        ("corner", NativeSettingValue::Choice { index, .. }) => {
            s.settings.corner = match *index {
                0 => settings::Corner::TopLeft,
                1 => settings::Corner::TopRight,
                2 => settings::Corner::BottomLeft,
                _ => settings::Corner::BottomRight,
            };
        }
        ("size", NativeSettingValue::Choice { index, .. }) => {
            s.settings.size = (*index).min(settings::SIZES.len() - 1);
        }
        ("zoom", NativeSettingValue::Choice { index, .. }) => {
            s.settings.zoom = (*index).min(settings::ZOOMS.len() - 1);
        }
        ("north_lock", NativeSettingValue::Toggle(on)) => s.settings.north_lock = *on,
        ("layer", NativeSettingValue::Choice { index, .. }) => {
            s.settings.layer = (*index).min(3) as u8;
        }
        ("fog", NativeSettingValue::Toggle(on)) => s.settings.fog = *on,
        ("clouds", NativeSettingValue::Toggle(on)) => s.settings.clouds = *on,
        ("far", NativeSettingValue::Toggle(on)) => s.settings.far = *on,
        ("radar", NativeSettingValue::Toggle(on)) => s.settings.radar = *on,
        ("beams", NativeSettingValue::Toggle(on)) => s.settings.beams = *on,
        ("trail", NativeSettingValue::Toggle(on)) => s.settings.trail = *on,
        ("lang", NativeSettingValue::Choice { index, .. }) => {
            s.settings.lang = (*index).min(2) as u8;
        }
        _ => return false,
    }
    s.save_settings();
    s.mini_key = None;
    true
}

/// The game's translation of a key it knows.
fn game_tr(key: &str) -> Option<String> {
    horizon_client::lang::has(key).then(|| horizon_client::lang::get(key))
}

/// True when megamap's world map is the open screen: every screen hook below acts only then.
fn ours(host: &dyn Host) -> bool {
    host.mod_screen() == Some(ID)
}

pub(super) fn screen_draw(host: &dyn Host, batch: &mut HudBatch, screen: &NativeScreenView<'_>) {
    if !ours(host) || screen.id != "mod" {
        return;
    }
    lock().draw_world(host, batch, screen.mouse, screen.rect);
}

/// The screen hooks return true to cancel the event (megamap took it).
pub(super) fn screen_press(host: &dyn Host, screen: &NativeScreenView<'_>, button: i32) -> bool {
    if !ours(host) {
        return false;
    }
    let mut s = lock();
    let s = &mut *s;
    let a = s.ui.press(screen.mouse, button, &s.waypoints);
    if a != Action::None {
        s.queue.push(Queued::Ui(a));
    }
    true
}

pub(super) fn screen_release(host: &dyn Host, button: i32) -> bool {
    if !ours(host) {
        return false;
    }
    lock().ui.release(button);
    true
}

pub(super) fn screen_drag(host: &dyn Host, screen: &NativeScreenView<'_>, delta: [f32; 2]) -> bool {
    if !ours(host) {
        return false;
    }
    let mut s = lock();
    s.ui.mouse_moved(screen.mouse);
    s.ui.drag(delta);
    true
}

pub(super) fn screen_scroll(host: &dyn Host, screen: &NativeScreenView<'_>, scroll: [f64; 2]) -> bool {
    if !ours(host) {
        return false;
    }
    let mut s = lock();
    let s = &mut *s;
    s.ui.scroll(screen.mouse, scroll[1], &s.waypoints);
    true
}

pub(super) fn screen_key(host: &dyn Host, key: &str) -> bool {
    if !ours(host) {
        return false;
    }
    let mut s = lock();
    let (consumed, a) = s.ui.key(key);
    if consumed {
        if a != Action::None {
            s.queue.push(Queued::Ui(a));
        }
        return true;
    }
    if a == Action::Close {
        // Escape with nothing to close inside: the game closes the screen itself.
        return false;
    }
    if !s.ui.typing() {
        if key == s.keys.map {
            s.queue.push(Queued::CloseMap);
            return true;
        }
        if key == s.keys.waypoints {
            s.ui.list_open = !s.ui.list_open;
            s.ui.search_focus = s.ui.list_open;
            return true;
        }
    }
    false
}

pub(super) fn screen_char(host: &dyn Host, c: char) -> bool {
    ours(host) && lock().ui.char_typed(c)
}

pub(super) fn primitives(camera: [f64; 3], primitives: &mut WorldPrimitives) {
    let s = lock();
    if !s.in_world {
        return;
    }
    let cam = camera;
    if s.settings.beams {
        for w in s.waypoints.in_dimension(&s.dim) {
            let base = [w.x as f64 + 0.5, w.y as f64, w.z as f64 + 0.5];
            let (dx, dy, dz) = (base[0] - cam[0], base[1] + 1.0 - cam[1], base[2] - cam[2]);
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();
            if dist < 2.0 {
                continue;
            }
            let argb = w.argb();
            if dist < 256.0 {
                primitives.beam(base, 256, (argb & 0xFF_FFFF) | 0xA000_0000);
            }
            // The label sits at the waypoint up close, and is pulled to 48 blocks from the camera further out,
            // so it stays readable and never gets clipped by the far plane.
            let k = if dist > 48.0 { 48.0 / dist } else { 1.0 };
            let at = [cam[0] + dx * k, cam[1] + (dy + 1.0) * k, cam[2] + dz * k];
            primitives
                .text(at, format!("{} ({})", w.name, distance_label(dist)), argb | 0xFF00_0000, 1.0)
                .through_walls()
                .scale_with_distance()
                .background();
        }
    }
    if s.settings.trail {
        let n = s.trail.points.len();
        let start = n.saturating_sub(400);
        let pts = s.trail.points.range(start..);
        let mut prev: Option<[i32; 3]> = None;
        for p in pts {
            if p[1] == i32::MIN {
                prev = None;
                continue;
            }
            if let Some(a) = prev {
                let f = |q: [i32; 3]| [q[0] as f64 + 0.5, q[1] as f64 + 0.1, q[2] as f64 + 0.5];
                let d2 = (a[0] as f64 - cam[0]).powi(2) + (a[2] as f64 - cam[2]).powi(2);
                if d2 < 96.0 * 96.0 {
                    primitives.line(f(a), f(*p), 0xB0FF_E082, 2.0);
                }
            }
            prev = Some(*p);
        }
    }
}
