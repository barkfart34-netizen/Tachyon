//! Hot-air balloon cavity detection, atmospheric buoyancy, and levitite anti-gravity lift.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::catalog::{self, BlockRole};
use crate::mass::BlockCell;
use crate::math::{Pos, Quat, Vec3, air_density, quat_rotate_inv, v_add, v_cross, v_scale};

const MAX_CAVITY_CELLS: usize = 512;
const HORIZONTAL_RADIUS: i32 = 6;
const VERTICAL_REACH: i32 = 12;

/// An enclosed/domed hot-air envelope pocket heated by a burner below it.
#[derive(Clone, Debug, PartialEq)]
pub struct BalloonPocket {
    pub volume: u32,
    pub center_local: Vec3,
    pub burner_pos: Pos,
    pub burner_heat: u8,
}

/// Net buoyant force (world space) and torque (local body space around CoM).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuoyancySummary {
    pub hot_air_lift_n: f64,
    pub levitite_lift_n: f64,
    pub force_world: Vec3,
    pub torque_local: Vec3,
}

/// Scans the contraption's block grid for domed envelope pockets above burners.
pub fn detect_balloons(blocks: &[BlockCell]) -> Vec<BalloonPocket> {
    if blocks.is_empty() {
        return Vec::new();
    }
    let by_pos: HashMap<Pos, &BlockCell> = blocks.iter().map(|b| (b.local, b)).collect();
    let max_y = blocks.iter().map(|b| b.local[1]).max().unwrap_or(0);

    let has_envelope_overhead = |p: Pos| -> bool {
        for y in (p[1] + 1)..=max_y {
            if let Some(cell) = by_pos.get(&[p[0], y, p[2]]) {
                return catalog::classify(&cell.id).role == BlockRole::Envelope;
            }
        }
        false
    };

    let mut pockets = Vec::new();
    let mut global_claimed: HashSet<Pos> = HashSet::new();

    for cell in blocks {
        if catalog::classify(&cell.id).role != BlockRole::Burner {
            continue;
        }
        let bpos = cell.local;
        let seed = [bpos[0], bpos[1] + 1, bpos[2]];
        if by_pos.contains_key(&seed) || !has_envelope_overhead(seed) || global_claimed.contains(&seed) {
            continue;
        }

        let mut visited: HashSet<Pos> = HashSet::new();
        let mut queue: VecDeque<Pos> = VecDeque::new();
        visited.insert(seed);
        queue.push_back(seed);
        let mut sum = [0.0_f64; 3];

        while let Some(pos) = queue.pop_front() {
            global_claimed.insert(pos);
            sum[0] += f64::from(pos[0]) + 0.5;
            sum[1] += f64::from(pos[1]) + 0.5;
            sum[2] += f64::from(pos[2]) + 0.5;
            if visited.len() >= MAX_CAVITY_CELLS {
                break;
            }
            for delta in [
                [1, 0, 0],
                [-1, 0, 0],
                [0, 1, 0],
                [0, -1, 0],
                [0, 0, 1],
                [0, 0, -1],
            ] {
                let np = [pos[0] + delta[0], pos[1] + delta[1], pos[2] + delta[2]];
                if np[1] <= bpos[1]
                    || np[1] > bpos[1] + VERTICAL_REACH
                    || (np[0] - bpos[0]).abs() > HORIZONTAL_RADIUS
                    || (np[2] - bpos[2]).abs() > HORIZONTAL_RADIUS
                {
                    continue;
                }
                if visited.contains(&np) || by_pos.contains_key(&np) {
                    continue;
                }
                if has_envelope_overhead(np) {
                    visited.insert(np);
                    queue.push_back(np);
                }
            }
        }

        if !visited.is_empty() {
            let count = visited.len() as f64;
            pockets.push(BalloonPocket {
                volume: visited.len() as u32,
                center_local: [sum[0] / count, sum[1] / count, sum[2] / count],
                burner_pos: bpos,
                burner_heat: cell.heat.max(4),
            });
        }
    }

    pockets
}

/// Evaluates world buoyancy force and righting torque for hot-air pockets and levitite blocks.
pub fn solve_buoyancy(
    world_y: f64,
    orientation: Quat,
    center_of_mass: Vec3,
    blocks: &[BlockCell],
    pockets: &[BalloonPocket],
    burner_override: Option<u8>,
    vertical_input: f32,
) -> BuoyancySummary {
    let rho = air_density(world_y);
    let up_local = quat_rotate_inv(orientation, [0.0, 1.0, 0.0]);
    let v_ctrl = f64::from(vertical_input.clamp(-1.0, 1.0));

    let mut hot_air_lift_n = 0.0_f64;
    let mut torque_local = [0.0_f64; 3];

    for pocket in pockets {
        let base_heat = f64::from(burner_override.unwrap_or(pocket.burner_heat).min(15));
        let effective_heat = (base_heat + v_ctrl * 6.0).clamp(0.0, 18.0);
        // Each trapped hot-air cell at standard heat lifts ~52 kg at sea level.
        let lift = f64::from(pocket.volume) * (160.0 + effective_heat * 62.0) * rho;
        hot_air_lift_n += lift;

        let r = [
            pocket.center_local[0] - center_of_mass[0],
            pocket.center_local[1] - center_of_mass[1],
            pocket.center_local[2] - center_of_mass[2],
        ];
        torque_local = v_add(torque_local, v_cross(r, v_scale(up_local, lift)));
    }

    let mut levitite_lift_n = 0.0_f64;
    for cell in blocks {
        let info = catalog::classify(&cell.id);
        if info.buoyant_lift_n > 0.0 {
            // Levitite also supports gentle pilot vertical trim when piloted directly.
            let boost = (1.0 + v_ctrl * 0.35).clamp(0.4, 1.5);
            let lift = info.buoyant_lift_n * boost * (0.4 + 0.6 * rho);
            levitite_lift_n += lift;
            let r = [
                f64::from(cell.local[0]) + 0.5 - center_of_mass[0],
                f64::from(cell.local[1]) + 0.5 - center_of_mass[1],
                f64::from(cell.local[2]) + 0.5 - center_of_mass[2],
            ];
            torque_local = v_add(torque_local, v_cross(r, v_scale(up_local, lift)));
        }
    }

    let total = hot_air_lift_n + levitite_lift_n;
    BuoyancySummary {
        hot_air_lift_n,
        levitite_lift_n,
        force_world: [0.0, total, 0.0],
        torque_local,
    }
}
