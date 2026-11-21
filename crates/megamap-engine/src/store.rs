//! The regions in memory for one world and dimension, with an LRU limit, and their files on disk:
//! `<base>/<world key>/<dimension>/r.<rx>.<rz>.mmr`.

use super::baker::BakedChunk;
use super::region::Region;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use super::reveal::Front;

pub struct Store {
    pub regions: HashMap<(i32, i32), Region>,
    used: HashMap<(i32, i32), u64>,
    tick: u64,
    pub limit: usize,
    /// Regions asked from disk and not answered yet.
    pub pending_load: std::collections::HashSet<(i32, i32)>,
    /// Regions whose file was looked for already (found or not).
    pub tried: std::collections::HashSet<(i32, i32)>,
    /// Bumped whenever any region changes, so a view knows to redraw.
    pub generation: u64,
    /// Chunks seen for the first time (baked live, never on disk): they come out of the fog behind the front
    /// (`reveal::Front`). Only fed while `track_fresh` (the surface and cave layers, not the far one).
    pub front: Front,
    pub track_fresh: bool,
    /// No region files behind this store (the cave slice): every chunk explored first counts as new.
    pub memory_only: bool,
    /// The limit given at creation; `fit_visible` raises `limit` above it while a wide view needs more.
    pub base_limit: usize,
    /// Reads that failed: when to try again and how many tries so far (`load_failed`).
    pub failed: HashMap<(i32, i32), (std::time::Instant, u8)>,
    /// Load counters for the debug log.
    pub stats: LoadStats,
    /// The region rectangle `[x0, z0, x1, z1]` (inclusive) the last view touched (not capped): an eviction inside
    /// it counts as `evicted_visible`.
    pub shown: Option<[i32; 4]>,
}

/// Region load counters (debug log): asked from disk, answered with data, failed reads, and regions dropped
/// while a view still showed them (should stay 0).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LoadStats {
    pub queued: u64,
    pub loaded: u64,
    pub failed: u64,
    pub evicted_visible: u64,
}

/// Most regions a view keeps loaded at once (nearest first); farther ones are drawn from the coarse far
/// terrain. About 2 MB each with heights and mips, so this bounds a far-zoomed map to about 270 MB per layer.
pub const VISIBLE_CAP: usize = 128;
/// First wait before reading a region again after a failed read (doubles per try).
pub const RETRY_AFTER: std::time::Duration = std::time::Duration::from_secs(3);
/// Tries per region before giving up until it leaves the view.
pub const RETRY_TRIES: u8 = 4;

impl Store {
    pub fn new(limit: usize) -> Self {
        Store {
            regions: HashMap::new(),
            used: HashMap::new(),
            tick: 0,
            limit,
            pending_load: Default::default(),
            tried: Default::default(),
            generation: 1,
            front: Front::default(),
            track_fresh: false,
            memory_only: false,
            base_limit: limit,
            failed: HashMap::new(),
            stats: LoadStats::default(),
            shown: None,
        }
    }

    /// Whether to ask for region `key` from disk now (marks it asked): not asked yet, or a failed read whose
    /// retry time has come.
    pub fn want_load(&mut self, key: (i32, i32), now: std::time::Instant) -> bool {
        if let Some(&(at, _)) = self.failed.get(&key) {
            if now < at || self.tried.contains(&key) {
                return false;
            }
        }
        if !self.tried.insert(key) {
            return false;
        }
        self.pending_load.insert(key);
        self.stats.queued += 1;
        true
    }

    /// A read of `key` failed (not a missing file): try again later, a few times. Stays pending meanwhile, so
    /// what memory holds of the region is never saved over a file that could not be read.
    pub fn load_failed(&mut self, key: (i32, i32), now: std::time::Instant) {
        self.stats.failed += 1;
        let tries = self.failed.get(&key).map_or(0, |f| f.1) + 1;
        if tries > RETRY_TRIES {
            self.failed.remove(&key);
            self.pending_load.remove(&key);
            return;
        }
        self.failed.insert(key, (now + RETRY_AFTER * (1 << (tries - 1)), tries));
        self.tried.remove(&key);
    }

    /// No region file there (a fine answer).
    pub fn load_missing(&mut self, key: (i32, i32)) {
        self.pending_load.remove(&key);
        self.failed.remove(&key);
    }

    /// Raises the limit while a view shows more regions than it (never below the base limit), so nothing
    /// visible has to go.
    pub fn fit_visible(&mut self, visible: usize) {
        self.limit = self.base_limit.max(visible.min(VISIBLE_CAP) + 8);
    }

    fn is_shown(&self, k: (i32, i32)) -> bool {
        self.shown.is_some_and(|r| k.0 >= r[0] && k.0 <= r[2] && k.1 >= r[1] && k.1 <= r[3])
    }

    /// Regions in the rectangle the last view touched.
    pub fn shown_count(&self) -> u64 {
        self.shown.map_or(0, |r| (r[2] as i64 - r[0] as i64 + 1).max(0) as u64 * (r[3] as i64 - r[1] as i64 + 1).max(0) as u64)
    }

    /// The regions a view of `w` x `h` keeps loaded, into `out`: the nearest to its centre first, at most
    /// `VISIBLE_CAP`; returns the whole rectangle it touches (`view::visible_rect`). The same pick as listing the
    /// rectangle and sorting it (`nearest_visible`), without the list: a map zoomed all the way out touches
    /// tens of thousands of regions, and listing, copying and sorting them was up to a millisecond of every frame.
    pub fn plan(v: &super::view::View, w: usize, h: usize, out: &mut Vec<(i32, i32)>) -> [i32; 4] {
        let rect = super::view::visible_rect(v, w, h);
        let (cx, cz) = ((v.center[0].floor() as i32) >> 9, (v.center[1].floor() as i32) >> 9);
        out.clear();
        let cells = (rect[2] as i64 - rect[0] as i64 + 1) * (rect[3] as i64 - rect[1] as i64 + 1);
        // Rings (Chebyshev) around the centre until they hold enough, then as far again as a ring's corner is
        // from its side (sqrt 2), so nothing nearer in a straight line is left out.
        let mut reach = i64::MAX;
        if cells > 4 * VISIBLE_CAP as i64 {
            let within = |r: i64| {
                let span = |lo: i32, hi: i32, c: i32| ((hi as i64).min(c as i64 + r) - (lo as i64).max(c as i64 - r) + 1).max(0);
                span(rect[0], rect[2], cx) * span(rect[1], rect[3], cz)
            };
            let mut r = 1i64;
            while within(r) < VISIBLE_CAP as i64 {
                r *= 2;
            }
            reach = r * 3 / 2 + 1;
        }
        let lo = |a: i32, c: i32| (a as i64).max(c as i64 - reach.min(1 << 40)) as i32;
        let hi = |a: i32, c: i32| (a as i64).min(c as i64 + reach.min(1 << 40)) as i32;
        for rz in lo(rect[1], cz)..=hi(rect[3], cz) {
            for rx in lo(rect[0], cx)..=hi(rect[2], cx) {
                out.push((rx, rz));
            }
        }
        Self::nearest_visible(out, (cx, cz));
        rect
    }

    /// The visible regions to keep loaded: nearest to `center` first, at most `VISIBLE_CAP`.
    pub fn nearest_visible(visible: &mut Vec<(i32, i32)>, center: (i32, i32)) {
        visible.sort_unstable_by_key(|k| ((k.0 - center.0) as i64).pow(2) + ((k.1 - center.1) as i64).pow(2));
        visible.truncate(VISIBLE_CAP);
    }

    /// The rectangle `[x0, z0, x1, z1]` (inclusive) around every region in memory, `None` when there is none.
    pub fn bounds(&self) -> Option<[i32; 4]> {
        let mut it = self.regions.keys();
        let &(x, z) = it.next()?;
        Some(it.fold([x, z, x, z], |b, &(x, z)| [b[0].min(x), b[1].min(z), b[2].max(x), b[3].max(z)]))
    }

    pub fn touch(&mut self, key: (i32, i32)) {
        self.tick += 1;
        self.used.insert(key, self.tick);
    }

    pub fn get(&self, rx: i32, rz: i32) -> Option<&Region> {
        self.regions.get(&(rx, rz))
    }

    /// Stores a baked chunk; returns whether pixels changed.
    pub fn put(&mut self, c: &BakedChunk) -> bool {
        let key = Region::of_chunk(c.cx, c.cz);
        self.touch(key);
        // New ground only once the region's file has answered: a chunk explored in an earlier session is not news.
        let answered = self.memory_only || (self.tried.contains(&key) && !self.pending_load.contains(&key));
        let r = self.regions.entry(key).or_insert_with(|| Region::new(key.0, key.1));
        let first = !r.chunk_explored((c.cx & 31) as usize, (c.cz & 31) as usize);
        let changed = r.put_chunk(c);
        if first && answered && self.track_fresh {
            self.front.add(c.cx, c.cz);
        }
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Whether chunk (`cx`, `cz`) has been explored (is in memory and marked).
    pub fn chunk_explored(&self, cx: i32, cz: i32) -> bool {
        self.get(cx >> 5, cz >> 5).is_some_and(|r| r.chunk_explored((cx & 31) as usize, (cz & 31) as usize))
    }

    /// A region read from disk: kept as is when nothing is in memory, else it fills the chunks memory lacks.
    pub fn loaded(&mut self, mut r: Region) {
        let key = (r.rx, r.rz);
        self.pending_load.remove(&key);
        self.failed.remove(&key);
        self.stats.loaded += 1;
        self.touch(key);
        match self.regions.get_mut(&key) {
            Some(mem) => mem.merge_missing(&r),
            None => {
                r.saved_generation = r.generation;
                self.regions.insert(key, r);
            }
        }
        self.generation += 1;
    }

    /// Drops the least recently used regions beyond the limit (never one whose file is still being read: saving it
    /// first would write over the file with less than it holds); returns the unsaved ones' encoded bytes to write.
    pub fn evict(&mut self, keep: &[(i32, i32)]) -> Vec<((i32, i32), Vec<u8>)> {
        let mut out = Vec::new();
        while self.regions.len() > self.limit {
            let victim = self
                .regions
                .keys()
                .filter(|k| !keep.contains(k) && !self.pending_load.contains(k))
                .min_by_key(|k| self.used.get(k).copied().unwrap_or(0))
                .copied();
            let Some(k) = victim else { break };
            if self.is_shown(k) {
                self.stats.evicted_visible += 1;
            }
            let r = self.regions.remove(&k).unwrap();
            self.used.remove(&k);
            self.tried.remove(&k);
            if r.generation != r.saved_generation {
                out.push((k, r.encode()));
            }
        }
        out
    }

    /// Like `evict`, but hands the unsaved regions over whole, so the worker encodes them instead of the frame.
    pub fn evict_regions(&mut self, keep: &[(i32, i32)]) -> Vec<Region> {
        let mut out = Vec::new();
        while self.regions.len() > self.limit {
            let victim = self
                .regions
                .keys()
                .filter(|k| !keep.contains(k) && !self.pending_load.contains(k))
                .min_by_key(|k| self.used.get(k).copied().unwrap_or(0))
                .copied();
            let Some(k) = victim else { break };
            if self.is_shown(k) {
                self.stats.evicted_visible += 1;
            }
            let r = self.regions.remove(&k).unwrap();
            self.used.remove(&k);
            self.tried.remove(&k);
            if r.generation != r.saved_generation {
                out.push(r);
            }
        }
        out
    }

    /// Copies of every unsaved region (marked saved), for the worker to encode and write.
    pub fn dirty_copies(&mut self) -> Vec<Region> {
        let mut out = Vec::new();
        for r in self.regions.values_mut() {
            if r.generation != r.saved_generation {
                r.saved_generation = r.generation;
                out.push(r.copy_pixels());
            }
        }
        out
    }

    /// Every unsaved region, encoded, marked saved.
    pub fn dirty(&mut self) -> Vec<((i32, i32), Vec<u8>)> {
        let mut out = Vec::new();
        for (k, r) in self.regions.iter_mut() {
            if r.generation != r.saved_generation {
                out.push((*k, r.encode()));
                r.saved_generation = r.generation;
            }
        }
        out
    }
}

pub fn region_path(dir: &Path, rx: i32, rz: i32) -> PathBuf {
    dir.join(format!("r.{rx}.{rz}.mmr"))
}

/// Writes atomically (temporary file, then rename) so a crash never leaves half a region.
pub fn write_region(dir: &Path, rx: i32, rz: i32, bytes: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let path = region_path(dir, rx, rz);
    let tmp = path.with_extension("mmr.tmp");
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, &path)
}

/// `Ok(None)` when there is no file; a corrupt file is an error the caller logs and ignores.
pub fn read_region(dir: &Path, rx: i32, rz: i32) -> Result<Option<Region>, String> {
    match std::fs::read(region_path(dir, rx, rz)) {
        Ok(b) => Region::decode(&b).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

/// A folder name from a world or server name: lowercase `[a-z0-9_.-]`, others become `_`.
pub fn sanitize(s: &str) -> String {
    let mut out: String = s
        .chars()
        .map(|c| {
            let c = c.to_ascii_lowercase();
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' { c } else { '_' }
        })
        .collect();
    if out.is_empty() || out.chars().all(|c| c == '.') {
        out = "_".into();
    }
    out.truncate(80);
    out
}

/// The explored-chunk mask of a saved region without decoding it (bytes 14..142 of the file, see region.rs);
/// all zero when there is no file or it is not a megamap region.
pub fn explored_mask(dir: &Path, rx: i32, rz: i32, need_blocks: bool) -> [u64; 16] {
    use std::io::Read;
    let mut out = [0u64; 16];
    let Ok(mut f) = std::fs::File::open(region_path(dir, rx, rz)) else { return out };
    let mut head = [0u8; 142];
    if f.read_exact(&mut head).is_err() || !(&head[..4] == b"MMR2" || &head[..4] == b"MMR1") {
        return out;
    }
    // Only a file with block ids (v3, flags bit 0) when asked for: older ones are baked again for their textures.
    if need_blocks && (head[4] != 3 || head[5] & 1 == 0) {
        return out;
    }
    for (i, w) in out.iter_mut().enumerate() {
        *w = u64::from_le_bytes(head[14 + i * 8..22 + i * 8].try_into().unwrap());
    }
    out
}
