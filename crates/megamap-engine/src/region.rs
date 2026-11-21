//! A 512x512 block region (32x32 chunks): pixels, heights, which chunks were explored, and lazily built mip
//! levels (colours and heights) for zoomed-out views. `encode`/`decode` are the compact on-disk format `MMR2`
//! (`MMR1` files from 0.1 still decode, as land-only):
//!
//! ```text
//! "MMR2" u8 version u8 flags i32 rx i32 rz      (little endian)
//! 16 x u64 explored bits (bit = chunk lz*32+lx)
//! varint palette size N, N x ARGB32 (v1: RGB24) (palette index 0 = unknown, entries start at 1;
//!                                               alpha is the material, see `color::water`)
//! pixels: (varint run-1, varint index)*        until 512*512 pixels
//! heights: tokens, varint t                    t even: (t>>1)+1 zero deltas; t odd: one delta zigzag(t>>1)
//!          delta against the left pixel, the first column against the pixel above
//! v3 only (flags bit 0): the shown block of each column, for block textures where no chunk is loaded:
//!          u32 block stamp (`block_stamp`, the block registry the ids belong to)
//!          varint palette size M, M x varint block state id (index 0 = unknown, entries start at 1)
//!          (varint run-1, varint index)*      until 512*512 columns
//! u32 FNV-1a of everything before it
//! ```
//! A region with no block ids is still written as v2, so older versions keep reading it.
//! Terrain repeats colours a lot, so the palette + run length pass stores a typical explored region in
//! 60-200 KiB instead of the 1.5 MiB of raw pixels and heights.

use super::baker::BakedChunk;
use super::color;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};

pub const SIZE: usize = 512;
pub const CHUNKS: usize = SIZE / 16;
pub const MIP_LEVELS: usize = 5; // 0 = full, 4 = 16 blocks per texel
const MAGIC: &[u8; 4] = b"MMR2";
const MAGIC_V1: &[u8; 4] = b"MMR1";
const VERSION: u8 = 2;
/// v2 plus the column block ids (0.3.5).
const VERSION_BLOCKS: u8 = 3;

/// The block registry the game runs now (`ColorTable::fingerprint`, 0 unknown): block ids saved under another
/// one are ignored (they would name other blocks after a game or mod change).
static BLOCK_STAMP: AtomicU32 = AtomicU32::new(0);

pub fn set_block_stamp(stamp: u32) {
    BLOCK_STAMP.store(stamp, Ordering::Relaxed);
}

pub fn block_stamp() -> u32 {
    BLOCK_STAMP.load(Ordering::Relaxed)
}

/// The saved block for column (`x`, `z`) where no chunk is loaded: from the explored region, else from the far
/// region, the far one only where the explored map has nothing (so the texture always belongs to the surface the
/// colour shows).
pub fn saved_block(explored: Option<&Region>, far: Option<&Region>, x: i32, z: i32) -> Option<u32> {
    let i = (z & (SIZE as i32 - 1)) as usize * SIZE + (x & (SIZE as i32 - 1)) as usize;
    match explored {
        // Explored ground saved without ids (before 0.3.5): the far column's block when it is the same surface
        // (the same height), so a texture never belongs to another surface than the colour.
        Some(r) if r.heights[i] != i16::MIN => r.block_at(x, z).or_else(|| far.filter(|f| f.heights[i] == r.heights[i])?.block_at(x, z)),
        _ => far?.block_at(x, z),
    }
}

pub struct Region {
    pub rx: i32,
    pub rz: i32,
    pub colors: Box<[u32]>,
    pub heights: Box<[i16]>,
    pub explored: [u64; 16],
    /// Bumped on every change: the view and the uploader compare it instead of diffing pixels.
    pub generation: u32,
    /// Generation the region was last saved at.
    pub saved_generation: u32,
    mips: [Box<[u32]>; MIP_LEVELS - 1],
    /// Height mips (mean of the known heights below), for relief shading of zoomed-out views.
    hmips: [Box<[i16]>; MIP_LEVELS - 1],
    mips_generation: u32,
    /// Chunks written since the mips were built (bit = `lz * CHUNKS + lx`), and how many `generation` bumps
    /// those writes made: when every bump since `mips_generation` is one of them, only these chunks' texels are
    /// rebuilt (a chunk covers whole texels on every level, so the result is the same as a full rebuild).
    mips_dirty: [u64; 16],
    mips_dirty_bumps: u32,
    /// The shown block of each column as an index into `block_pal` (0 unknown); `None` until a chunk brings one.
    pub blocks: Option<Box<[u16]>>,
    /// Block state ids by index (`block_pal[0]` = 0, unknown).
    pub block_pal: Vec<u32>,
    /// The registry `block_pal` belongs to (`block_stamp`).
    pub block_stamp: u32,
    block_index: HashMap<u32, u16>,
}

impl Region {
    pub fn new(rx: i32, rz: i32) -> Self {
        let mip = |l: usize| vec![color::UNKNOWN; (SIZE >> l) * (SIZE >> l)].into_boxed_slice();
        let hmip = |l: usize| vec![i16::MIN; (SIZE >> l) * (SIZE >> l)].into_boxed_slice();
        Region {
            rx,
            rz,
            colors: vec![color::UNKNOWN; SIZE * SIZE].into_boxed_slice(),
            heights: vec![i16::MIN; SIZE * SIZE].into_boxed_slice(),
            explored: [0; 16],
            generation: 1,
            saved_generation: 0,
            mips: [mip(1), mip(2), mip(3), mip(4)],
            hmips: [hmip(1), hmip(2), hmip(3), hmip(4)],
            mips_generation: 0,
            mips_dirty: [0; 16],
            mips_dirty_bumps: 0,
            blocks: None,
            block_pal: vec![0],
            block_stamp: 0,
            block_index: HashMap::new(),
        }
    }

    /// The block state id shown at world column (`x`, `z`) as saved, when it belongs to the running registry.
    #[inline]
    pub fn block_at(&self, x: i32, z: i32) -> Option<u32> {
        let b = self.blocks.as_ref()?;
        if self.block_stamp == 0 || self.block_stamp != block_stamp() {
            return None;
        }
        let i = (z & (SIZE as i32 - 1)) as usize * SIZE + (x & (SIZE as i32 - 1)) as usize;
        Some(self.block_pal[b[i] as usize]).filter(|&id| id != 0)
    }

    /// A copy for tests (the struct holds caches that are not `Clone`).
    pub fn clone_for_test(&self) -> Region {
        let mut r = self.copy_pixels();
        r.explored = self.explored;
        r
    }

    /// Starts the block ids over when the registry changed since they were saved.
    fn blocks_for(&mut self, stamp: u32) {
        if self.block_stamp != stamp {
            self.blocks = None;
            self.block_pal = vec![0];
            self.block_index.clear();
            self.block_stamp = stamp;
        }
    }

    /// The palette index of block `id` (0 for unknown, or when the palette is full).
    fn block_index_of(pal: &mut Vec<u32>, index: &mut HashMap<u32, u16>, id: u32) -> u16 {
        if id == 0 {
            return 0;
        }
        if index.is_empty() && pal.len() > 1 {
            for (i, &b) in pal.iter().enumerate().skip(1) {
                index.insert(b, i as u16);
            }
        }
        if let Some(&i) = index.get(&id) {
            return i;
        }
        if pal.len() >= u16::MAX as usize {
            return 0;
        }
        pal.push(id);
        let i = (pal.len() - 1) as u16;
        index.insert(id, i);
        i
    }

    /// Writes 16 column ids of one chunk row at pixel offset `o`.
    fn put_blocks(&mut self, o: usize, ids: &[u32]) {
        let (pal, index) = (&mut self.block_pal, &mut self.block_index);
        let b = self.blocks.get_or_insert_with(|| vec![0u16; SIZE * SIZE].into_boxed_slice());
        for (k, &id) in ids.iter().enumerate() {
            b[o + k] = Self::block_index_of(pal, index, id);
        }
    }

    /// The saved part (pixels, heights, explored mask) without the mips, for the worker to encode off the main
    /// thread: a copy of about 1.5 MB, a fraction of the encode's cost.
    /// The mips are left unallocated (`ensure_mips` builds them if the copy is ever drawn): the copy only goes to
    /// `encode`, and allocating and filling 0.6 MB of mips nobody reads was a third of its cost.
    pub fn copy_pixels(&self) -> Region {
        Region {
            rx: self.rx,
            rz: self.rz,
            colors: self.colors.clone(),
            heights: self.heights.clone(),
            explored: self.explored,
            generation: self.generation,
            saved_generation: self.saved_generation,
            mips: Default::default(),
            hmips: Default::default(),
            mips_generation: 0,
            mips_dirty: [0; 16],
            mips_dirty_bumps: 0,
            blocks: self.blocks.clone(),
            block_pal: self.block_pal.clone(),
            block_stamp: self.block_stamp,
            block_index: HashMap::new(),
        }
    }

    #[inline]
    pub fn of_chunk(cx: i32, cz: i32) -> (i32, i32) {
        (cx >> 5, cz >> 5)
    }

    pub fn chunk_explored(&self, lx: usize, lz: usize) -> bool {
        let bit = lz * CHUNKS + lx;
        self.explored[bit / 64] >> (bit % 64) & 1 != 0
    }

    pub fn explored_count(&self) -> u32 {
        self.explored.iter().map(|w| w.count_ones()).sum()
    }

    /// Copies a baked chunk in; returns whether anything changed (an unchanged re-bake costs no upload).
    pub fn put_chunk(&mut self, c: &BakedChunk) -> bool {
        let (lx, lz) = ((c.cx & 31) as usize, (c.cz & 31) as usize);
        let mut changed = !self.chunk_explored(lx, lz);
        let stamp = block_stamp();
        let with_blocks = stamp != 0 && c.block.iter().any(|&b| b != 0);
        let keep_blocks = stamp != 0 && (with_blocks || self.blocks.is_some());
        if keep_blocks {
            self.blocks_for(stamp);
        }
        for row in 0..16 {
            if keep_blocks {
                self.put_blocks((lz * 16 + row) * SIZE + lx * 16, &c.block[row * 16..row * 16 + 16]);
            }
            let o = (lz * 16 + row) * SIZE + lx * 16;
            let src = &c.color[row * 16..row * 16 + 16];
            if &self.colors[o..o + 16] != src {
                self.colors[o..o + 16].copy_from_slice(src);
                changed = true;
            }
            self.heights[o..o + 16].copy_from_slice(&c.height[row * 16..row * 16 + 16]);
        }
        let bit = lz * CHUNKS + lx;
        self.explored[bit / 64] |= 1 << (bit % 64);
        // Marked even when unchanged: the heights were copied all the same, and the next rebuild must see them.
        self.mips_dirty[bit / 64] |= 1 << (bit % 64);
        if changed {
            self.generation = self.generation.wrapping_add(1);
            self.mips_dirty_bumps = self.mips_dirty_bumps.wrapping_add(1);
        }
        changed
    }

    /// Fills the chunks of `other` that this region has not explored (a region loaded from disk merging with
    /// chunks baked before the load finished).
    pub fn merge_missing(&mut self, other: &Region) {
        for lz in 0..CHUNKS {
            for lx in 0..CHUNKS {
                if self.chunk_explored(lx, lz) || !other.chunk_explored(lx, lz) {
                    continue;
                }
                for row in 0..16 {
                    let o = (lz * 16 + row) * SIZE + lx * 16;
                    self.colors[o..o + 16].copy_from_slice(&other.colors[o..o + 16]);
                    self.heights[o..o + 16].copy_from_slice(&other.heights[o..o + 16]);
                    // Block ids come along when both sides use the same registry (or this side has none yet).
                    if let Some(ob) = other.blocks.as_ref().filter(|_| other.block_stamp != 0) {
                        if self.blocks.is_some() && self.block_stamp != other.block_stamp {
                            continue;
                        }
                        self.blocks_for(other.block_stamp);
                        let ids: Vec<u32> = ob[o..o + 16].iter().map(|&i| other.block_pal[i as usize]).collect();
                        self.put_blocks(o, &ids);
                    }
                }
                let bit = lz * CHUNKS + lx;
                self.explored[bit / 64] |= 1 << (bit % 64);
                self.mips_dirty[bit / 64] |= 1 << (bit % 64);
            }
        }
        self.generation = self.generation.wrapping_add(1);
        self.mips_dirty_bumps = self.mips_dirty_bumps.wrapping_add(1);
    }

    /// Rebuilds the mip chain if the pixels changed since; cheap no-op otherwise. After chunk writes only the
    /// written chunks are rebuilt (about 1 us a chunk against 1 ms for the whole region).
    pub fn ensure_mips(&mut self) {
        if self.mips_generation == self.generation && !self.mips[0].is_empty() {
            return;
        }
        let partial = !self.mips[0].is_empty()
            && self.mips_generation != 0
            && self.generation.wrapping_sub(self.mips_generation) == self.mips_dirty_bumps;
        if self.mips[0].is_empty() {
            // A `copy_pixels` copy: its mips were never allocated.
            for l in 1..MIP_LEVELS {
                let n = (SIZE >> l) * (SIZE >> l);
                self.mips[l - 1] = vec![color::UNKNOWN; n].into_boxed_slice();
                self.hmips[l - 1] = vec![i16::MIN; n].into_boxed_slice();
            }
        }
        if partial {
            let dirty = self.mips_dirty;
            for (w, &word) in dirty.iter().enumerate() {
                let mut m = word;
                while m != 0 {
                    let bit = w * 64 + m.trailing_zeros() as usize;
                    m &= m - 1;
                    let (lx, lz) = (bit % CHUNKS, bit / CHUNKS);
                    for l in 1..MIP_LEVELS {
                        let n = 16 >> l;
                        self.mip_block(l, lx * n, lz * n, n);
                    }
                }
            }
        } else {
            for l in 1..MIP_LEVELS {
                self.mip_block(l, 0, 0, SIZE >> l);
            }
        }
        self.mips_dirty = [0; 16];
        self.mips_dirty_bumps = 0;
        self.mips_generation = self.generation;
    }

    /// Rebuilds the `n` x `n` texels at (`x0`, `y0`) of mip `l` from level `l - 1`.
    fn mip_block(&mut self, l: usize, x0: usize, y0: usize, n: usize) {
        let s = SIZE >> l;
        let ss = s * 2;
        let (lower, upper) = self.mips.split_at_mut(l - 1);
        let src: &[u32] = if l == 1 { &self.colors } else { &lower[l - 2] };
        let dst = &mut upper[0];
        for y in y0..y0 + n {
            for x in x0..x0 + n {
                let i = y * 2 * ss + x * 2;
                dst[y * s + x] = color::average4([src[i], src[i + 1], src[i + ss], src[i + ss + 1]]);
            }
        }
        let (lower, upper) = self.hmips.split_at_mut(l - 1);
        let src: &[i16] = if l == 1 { &self.heights } else { &lower[l - 2] };
        let dst = &mut upper[0];
        for y in y0..y0 + n {
            for x in x0..x0 + n {
                let i = y * 2 * ss + x * 2;
                let (mut k, mut sum) = (0i32, 0i32);
                for h in [src[i], src[i + 1], src[i + ss], src[i + ss + 1]] {
                    if h != i16::MIN {
                        k += 1;
                        sum += h as i32;
                    }
                }
                dst[y * s + x] = if k == 0 { i16::MIN } else { (sum / k) as i16 };
            }
        }
    }

    /// Heights of mip `level` (`i16::MIN` = unknown); call `ensure_mips` first for level > 0.
    #[inline]
    pub fn height_level(&self, level: usize) -> &[i16] {
        if level == 0 { &self.heights } else { &self.hmips[level - 1] }
    }

    /// Texels of mip `level` (0 = full resolution); call `ensure_mips` first for level > 0.
    #[inline]
    pub fn level(&self, level: usize) -> &[u32] {
        if level == 0 { &self.colors } else { &self.mips[level - 1] }
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 * 1024);
        out.extend_from_slice(MAGIC);
        let blocks = self.blocks.as_ref().filter(|_| self.block_stamp != 0 && self.block_pal.len() > 1);
        out.push(if blocks.is_some() { VERSION_BLOCKS } else { VERSION });
        out.push(blocks.is_some() as u8);
        out.extend_from_slice(&self.rx.to_le_bytes());
        out.extend_from_slice(&self.rz.to_le_bytes());
        for w in self.explored {
            out.extend_from_slice(&w.to_le_bytes());
        }
        // Palette: index 0 is "unknown", the rest in first-seen order.
        let mut index: HashMap<u32, u32> = HashMap::new();
        let mut palette: Vec<u32> = Vec::new();
        let mut idx = Vec::with_capacity(SIZE * SIZE);
        for &c in self.colors.iter() {
            if c >> 24 == 0 {
                idx.push(0u32);
                continue;
            }
            let i = *index.entry(c).or_insert_with(|| {
                palette.push(c);
                palette.len() as u32
            });
            idx.push(i);
        }
        varint(&mut out, palette.len() as u64);
        for p in &palette {
            out.extend_from_slice(&p.to_le_bytes());
        }
        let mut i = 0;
        while i < idx.len() {
            let v = idx[i];
            let mut run = 1;
            while i + run < idx.len() && idx[i + run] == v {
                run += 1;
            }
            varint(&mut out, run as u64 - 1);
            varint(&mut out, v as u64);
            i += run;
        }
        let mut zeros = 0u64;
        for (i, &h) in self.heights.iter().enumerate() {
            let prev = if i % SIZE != 0 {
                self.heights[i - 1]
            } else if i >= SIZE {
                self.heights[i - SIZE]
            } else {
                i16::MIN
            };
            let d = h as i32 - prev as i32;
            if d == 0 {
                zeros += 1;
                continue;
            }
            if zeros > 0 {
                varint(&mut out, (zeros - 1) << 1);
                zeros = 0;
            }
            varint(&mut out, zigzag(d) << 1 | 1);
        }
        if zeros > 0 {
            varint(&mut out, (zeros - 1) << 1);
        }
        if let Some(b) = blocks {
            out.extend_from_slice(&self.block_stamp.to_le_bytes());
            // Only the indices in use, renumbered in first-seen order (a re-baked chunk can leave unused ones).
            let mut remap = vec![0u32; self.block_pal.len()];
            let mut pal: Vec<u32> = Vec::new();
            for &i in b.iter() {
                if i != 0 && remap[i as usize] == 0 {
                    pal.push(self.block_pal[i as usize]);
                    remap[i as usize] = pal.len() as u32;
                }
            }
            varint(&mut out, pal.len() as u64);
            for &id in &pal {
                varint(&mut out, id as u64);
            }
            let mut i = 0;
            while i < b.len() {
                let v = b[i];
                let mut run = 1;
                while i + run < b.len() && b[i + run] == v {
                    run += 1;
                }
                varint(&mut out, run as u64 - 1);
                varint(&mut out, remap[v as usize] as u64);
                i += run;
            }
        }
        let sum = fnv1a(&out);
        out.extend_from_slice(&sum.to_le_bytes());
        out
    }

    pub fn decode(data: &[u8]) -> Result<Region, String> {
        if data.len() < 4 + 2 + 8 + 128 + 4 || (&data[..4] != MAGIC && &data[..4] != MAGIC_V1) {
            return Err("not a megamap region".into());
        }
        let (body, sum) = data.split_at(data.len() - 4);
        if fnv1a(body) != u32::from_le_bytes(sum.try_into().unwrap()) {
            return Err("region checksum mismatch".into());
        }
        let v1 = &data[..4] == MAGIC_V1;
        let ok = if v1 { body[4] == 1 } else { body[4] == VERSION || body[4] == VERSION_BLOCKS };
        if !ok {
            return Err(format!("region version {} unsupported", body[4]));
        }
        let with_blocks = !v1 && body[4] == VERSION_BLOCKS && body[5] & 1 != 0;
        let mut r = Reader { data: body, at: 6 };
        let rx = i32::from_le_bytes(r.bytes(4)?.try_into().unwrap());
        let rz = i32::from_le_bytes(r.bytes(4)?.try_into().unwrap());
        let mut region = Region::new(rx, rz);
        for w in region.explored.iter_mut() {
            *w = u64::from_le_bytes(r.bytes(8)?.try_into().unwrap());
        }
        let n = r.varint()? as usize;
        if n > SIZE * SIZE {
            return Err("palette too large".into());
        }
        let mut palette = Vec::with_capacity(n + 1);
        palette.push(color::UNKNOWN);
        for _ in 0..n {
            if v1 {
                let b = r.bytes(3)?;
                palette.push(color::rgb(b[0], b[1], b[2]));
            } else {
                let c = u32::from_le_bytes(r.bytes(4)?.try_into().unwrap());
                palette.push(if c >> 24 == 0 { color::OPAQUE | c } else { c });
            }
        }
        let mut at = 0usize;
        while at < SIZE * SIZE {
            let run = r.varint()? as usize + 1;
            let v = r.varint()? as usize;
            let c = *palette.get(v).ok_or("palette index out of range")?;
            if at + run > SIZE * SIZE {
                return Err("pixel run past the region".into());
            }
            region.colors[at..at + run].fill(c);
            at += run;
        }
        let mut i = 0usize;
        while i < SIZE * SIZE {
            let t = r.varint()?;
            let (count, delta) = if t & 1 == 0 { ((t >> 1) as usize + 1, 0) } else { (1, unzigzag(t >> 1)) };
            if i + count > SIZE * SIZE {
                return Err("height run past the region".into());
            }
            for _ in 0..count {
                let prev = if i % SIZE != 0 {
                    region.heights[i - 1]
                } else if i >= SIZE {
                    region.heights[i - SIZE]
                } else {
                    i16::MIN
                };
                region.heights[i] = (prev as i32 + delta) as i16;
                i += 1;
            }
        }
        if with_blocks {
            region.block_stamp = u32::from_le_bytes(r.bytes(4)?.try_into().unwrap());
            let m = r.varint()? as usize;
            if m >= u16::MAX as usize {
                return Err("block palette too large".into());
            }
            for _ in 0..m {
                region.block_pal.push(r.varint()? as u32);
            }
            let mut b = vec![0u16; SIZE * SIZE].into_boxed_slice();
            let mut at = 0usize;
            while at < SIZE * SIZE {
                let run = r.varint()? as usize + 1;
                let v = r.varint()? as usize;
                if v > m || at + run > SIZE * SIZE {
                    return Err("block run out of range".into());
                }
                b[at..at + run].fill(v as u16);
                at += run;
            }
            region.blocks = Some(b);
        }
        if r.at != body.len() {
            return Err("trailing bytes in region".into());
        }
        region.saved_generation = region.generation;
        Ok(region)
    }
}

struct Reader<'a> {
    data: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn bytes(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self.data.get(self.at..self.at + n).ok_or("region truncated")?;
        self.at += n;
        Ok(s)
    }
    fn varint(&mut self) -> Result<u64, String> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = *self.data.get(self.at).ok_or("region truncated")?;
            self.at += 1;
            v |= ((b & 0x7F) as u64) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err("varint too long".into())
    }
}

fn varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push(v as u8 | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

fn zigzag(v: i32) -> u64 {
    ((v << 1) ^ (v >> 31)) as u32 as u64
}

fn unzigzag(v: u64) -> i32 {
    let v = v as u32;
    (v >> 1) as i32 ^ -((v & 1) as i32)
}

pub fn fnv1a(data: &[u8]) -> u32 {
    let mut h = 0x811C_9DC5u32;
    for &b in data {
        h ^= b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    h
}
