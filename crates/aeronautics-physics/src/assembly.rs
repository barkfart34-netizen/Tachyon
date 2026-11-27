//! Flood-fill contraption scanner (`simulated:physics_assembler`) converting world blocks into a `RigidContraption`.

use std::collections::{HashSet, VecDeque};

use crate::body::RigidContraption;
use crate::catalog::{self, TireKind};
use crate::mass::BlockCell;
use crate::math::{Facing, Pos};

/// World block snapshot returned by the caller during BFS contraption assembly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WorldBlockSample {
    pub id: String,
    pub facing: Facing,
    pub powered: bool,
    pub heat: u8,
    pub tire: Option<TireKind>,
}

/// Reasons a contraption assembly scan may refuse to lift off.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssemblyError {
    /// No movable blocks were found at or connected to `origin`.
    Empty,
    /// Exceeded `max_blocks` limit while flood-filling connected hull blocks.
    TooLarge { limit: usize },
    /// Encountered an unmovable anchor block (e.g. obsidian/bedrock) directly on the assembler.
    BlockedByUnmovable { pos: Pos, id: String },
}

/// Scans 6-connected movable blocks starting at `origin` up to `max_blocks` and builds a `RigidContraption`.
pub fn assemble_from_world(
    id: u64,
    origin: Pos,
    max_blocks: usize,
    read_block: impl Fn(Pos) -> Option<WorldBlockSample>,
) -> Result<RigidContraption, AssemblyError> {
    let limit = max_blocks.max(1);
    let Some(root) = read_block(origin) else {
        return Err(AssemblyError::Empty);
    };
    if catalog::is_unmovable(&root.id) {
        return Err(AssemblyError::BlockedByUnmovable {
            pos: origin,
            id: root.id,
        });
    }

    let mut visited: HashSet<Pos> = HashSet::new();
    let mut queue: VecDeque<Pos> = VecDeque::new();
    let mut cells: Vec<BlockCell> = Vec::new();

    visited.insert(origin);
    queue.push_back(origin);

    while let Some(pos) = queue.pop_front() {
        let Some(sample) = read_block(pos) else {
            continue;
        };
        if catalog::is_unmovable(&sample.id) {
            continue;
        }
        if cells.len() >= limit {
            return Err(AssemblyError::TooLarge { limit });
        }

        let local = [
            pos[0] - origin[0],
            pos[1] - origin[1],
            pos[2] - origin[2],
        ];
        cells.push(BlockCell {
            local,
            id: sample.id,
            facing: sample.facing,
            powered: sample.powered,
            heat: sample.heat,
            tire: sample.tire,
        });

        for delta in [
            [1, 0, 0],
            [-1, 0, 0],
            [0, 1, 0],
            [0, -1, 0],
            [0, 0, 1],
            [0, 0, -1],
        ] {
            let np = [pos[0] + delta[0], pos[1] + delta[1], pos[2] + delta[2]];
            if visited.contains(&np) {
                continue;
            }
            if let Some(next_sample) = read_block(np)
                && !catalog::is_unmovable(&next_sample.id)
            {
                visited.insert(np);
                queue.push_back(np);
            }
        }
    }

    if cells.is_empty() {
        return Err(AssemblyError::Empty);
    }

    let world_origin = [f64::from(origin[0]), f64::from(origin[1]), f64::from(origin[2])];
    Ok(RigidContraption::from_blocks(id, world_origin, cells))
}
