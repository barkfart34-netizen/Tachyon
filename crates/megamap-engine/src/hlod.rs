//! Read-only reader of Horizon's far terrain disk cache (`<dimension>/data/horizon_far_terrain/r.X.Z.hlod`): the
//! decorated far chunks the level-of-detail generator saved in earlier sessions. Reading it needs no engine
//! change and no running LOD renderer: the files stay after the far terrain is switched off.
//!
//! The format (Horizon `lod/cache.rs`, `FORMAT` 1), all little endian:
//!
//! ```text
//! "HLOD" u32 format(1) u64 build key                    16 byte header
//! records: i32 chunk x, i32 chunk z, u32 len, len bytes   raw deflate of the payload; a torn last record ends
//!                                                         the file (the game cuts it off on its next open)
//! payload: 256 columns (index z * 16 + x), each
//!   u8 tag (0 none, 1 surface, 2 surface + water)
//!   tag > 0: i32 height, u32 state, u16 biome; tag 2: i32 water height, u32 water state
//!   u16 span count, spans of (i16 min, i16 max, u32 state, u16 biome, u16 light)
//! ```
//!
//! This reader never opens a file for writing (the game's `DiskCache::open` truncates files with another build
//! key; a mod must not). The build key is not checked: a file from another build may carry other block state
//! ids, which shows as wrong colours until the game rewrites it. Inflating is injected (`Inflate`): the glue
//! passes `flate2`, which Horizon already links.

use super::far::Surface;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};

const MAGIC: &[u8; 4] = b"HLOD";
const FORMAT: u32 = 1;
const HEADER: u64 = 16;
const RECORD: u64 = 12;
const MAX_RECORD: u32 = 1 << 22;
/// Longest inflated payload: 256 columns of 21 bytes plus a span table each (bounded by the u16 count).
pub const MAX_PAYLOAD: usize = 256 * (21 + 2 + 65535 * 12);

/// Inflates raw deflate `data` into `out` (cleared first), reading at most `limit` bytes; false on damage.
pub type Inflate = fn(data: &[u8], out: &mut Vec<u8>, limit: usize) -> bool;

/// One open region file: its chunks' record offsets.
pub struct HlodRegion<R> {
    file: R,
    pub key: u64,
    index: HashMap<(i32, i32), (u64, u32)>,
}

impl<R: Read + Seek> HlodRegion<R> {
    /// Indexes the records of an open file; `None` if it is not a current-format cache file.
    pub fn open(mut file: R) -> Option<Self> {
        let mut header = [0u8; HEADER as usize];
        file.seek(SeekFrom::Start(0)).ok()?;
        file.read_exact(&mut header).ok()?;
        if &header[..4] != MAGIC || header[4..8] != FORMAT.to_le_bytes() {
            return None;
        }
        let key = u64::from_le_bytes(header[8..16].try_into().unwrap());
        let size = file.seek(SeekFrom::End(0)).ok()?;
        let mut index = HashMap::new();
        let mut at = HEADER;
        let mut rec = [0u8; RECORD as usize];
        while at + RECORD <= size {
            file.seek(SeekFrom::Start(at)).ok()?;
            if file.read_exact(&mut rec).is_err() {
                break;
            }
            let field = |i: usize| u32::from_le_bytes(rec[i..i + 4].try_into().unwrap());
            let len = field(8);
            if len > MAX_RECORD || at + RECORD + len as u64 > size {
                break;
            }
            // Later records of a chunk replace earlier ones, as in the game.
            index.insert((field(0) as i32, field(4) as i32), (at + RECORD, len));
            at += RECORD + len as u64;
        }
        Some(HlodRegion { file, key, index })
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn contains(&self, cx: i32, cz: i32) -> bool {
        self.index.contains_key(&(cx, cz))
    }

    pub fn chunks(&self) -> impl Iterator<Item = (i32, i32)> + '_ {
        self.index.keys().copied()
    }

    /// Reads chunk (`cx`, `cz`)'s surfaces into `out` (256 entries). `buf` and `raw` are scratch buffers kept by
    /// the caller so a scan allocates nothing per chunk.
    pub fn read(
        &mut self,
        cx: i32,
        cz: i32,
        inflate: Inflate,
        buf: &mut Vec<u8>,
        raw: &mut Vec<u8>,
        out: &mut [Option<Surface>; 256],
    ) -> bool {
        let Some(&(offset, len)) = self.index.get(&(cx, cz)) else {
            return false;
        };
        buf.resize(len as usize, 0);
        if self.file.seek(SeekFrom::Start(offset)).is_err() || self.file.read_exact(buf).is_err() {
            return false;
        }
        inflate(buf, raw, MAX_PAYLOAD) && parse(raw, out)
    }
}

/// Parses an inflated payload into the 256 column surfaces; false (and `out` unspecified) on damage.
pub fn parse(raw: &[u8], out: &mut [Option<Surface>; 256]) -> bool {
    let mut at = 0usize;
    let mut take = |n: usize| -> Option<&[u8]> {
        let s = raw.get(at..at + n)?;
        at += n;
        Some(s)
    };
    let i32_ = |b: &[u8]| i32::from_le_bytes(b.try_into().unwrap());
    let u32_ = |b: &[u8]| u32::from_le_bytes(b.try_into().unwrap());
    let u16_ = |b: &[u8]| u16::from_le_bytes(b.try_into().unwrap());
    for slot in out.iter_mut() {
        let Some(tag) = take(1).map(|b| b[0]) else { return false };
        if tag > 2 {
            return false;
        }
        *slot = None;
        if tag > 0 {
            let (Some(h), Some(st), Some(bi)) = (take(4).map(i32_), take(4).map(u32_), take(2).map(u16_)) else {
                return false;
            };
            let water = if tag == 2 {
                let (Some(wh), Some(ws)) = (take(4).map(i32_), take(4).map(u32_)) else { return false };
                Some((wh, ws))
            } else {
                None
            };
            *slot = Some(Surface { height: h, state: st, biome: bi, water });
        }
        let Some(count) = take(2).map(u16_) else { return false };
        // Spans (solid runs below the surface) are not drawn by the map: skip them.
        if take(count as usize * 12).is_none() {
            return false;
        }
    }
    at == raw.len()
}

/// The cache directory of a dimension save folder (`<world>/dimensions/<ns>/<path>`).
pub fn dir_of(dimension_root: &std::path::Path) -> std::path::PathBuf {
    dimension_root.join("data").join("horizon_far_terrain")
}

/// The file holding chunk (`cx`, `cz`).
pub fn file_name(cx: i32, cz: i32) -> String {
    format!("r.{}.{}.hlod", cx >> 5, cz >> 5)
}
