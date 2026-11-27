//! Rigid-body mass, center of mass, and moment of inertia from contraption blocks.

use crate::catalog::{self, TireKind};
use crate::math::{Facing, Pos, Vec3};

/// A single block cell belonging to an assembled 6-DOF contraption.
#[derive(Clone, Debug, PartialEq)]
pub struct BlockCell {
    pub local: Pos,
    pub id: String,
    pub facing: Facing,
    pub powered: bool,
    /// Burner heat level (`0..=15`) when `role == BlockRole::Burner`.
    pub heat: u8,
    /// Mounted tire on `offroad:wheel_mount` or `offroad:borehead_bearing`.
    pub tire: Option<TireKind>,
}

impl BlockCell {
    /// Convenience constructor for tests and assemblers.
    pub fn new(local: Pos, id: impl Into<String>, facing: Facing) -> Self {
        Self {
            local,
            id: id.into(),
            facing,
            powered: false,
            heat: 0,
            tire: None,
        }
    }
}

/// Aggregate rigid-body mass distribution computed via the parallel-axis theorem.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    pub total_mass: f64,
    pub center_of_mass: Vec3,
    pub inertia_diag: Vec3,
    pub bounds_min: Vec3,
    pub bounds_max: Vec3,
}

impl Default for MassProperties {
    fn default() -> Self {
        Self {
            total_mass: 100.0,
            center_of_mass: [0.5, 0.5, 0.5],
            inertia_diag: [100.0, 100.0, 100.0],
            bounds_min: [0.0, 0.0, 0.0],
            bounds_max: [1.0, 1.0, 1.0],
        }
    }
}

/// Computes mass, center of mass, diagonal inertia tensor, and local bounding box.
pub fn compute_mass_properties(blocks: &[BlockCell]) -> MassProperties {
    if blocks.is_empty() {
        return MassProperties::default();
    }

    let mut total_mass = 0.0_f64;
    let mut weighted_pos = [0.0_f64; 3];
    let mut bmin = [f64::INFINITY; 3];
    let mut bmax = [f64::NEG_INFINITY; 3];

    for cell in blocks {
        let m = catalog::classify(&cell.id).mass_kg;
        let c = [
            f64::from(cell.local[0]) + 0.5,
            f64::from(cell.local[1]) + 0.5,
            f64::from(cell.local[2]) + 0.5,
        ];
        total_mass += m;
        for i in 0..3 {
            weighted_pos[i] += m * c[i];
            bmin[i] = bmin[i].min(f64::from(cell.local[i]));
            bmax[i] = bmax[i].max(f64::from(cell.local[i]) + 1.0);
        }
    }

    let safe_mass = total_mass.max(10.0);
    let com = [
        weighted_pos[0] / safe_mass,
        weighted_pos[1] / safe_mass,
        weighted_pos[2] / safe_mass,
    ];

    // Unit cube box inertia around its own center is m / 6, plus m * d^2 parallel axis term.
    let mut inertia = [0.0_f64; 3];
    for cell in blocks {
        let m = catalog::classify(&cell.id).mass_kg;
        let dx = f64::from(cell.local[0]) + 0.5 - com[0];
        let dy = f64::from(cell.local[1]) + 0.5 - com[1];
        let dz = f64::from(cell.local[2]) + 0.5 - com[2];
        let self_i = m / 6.0;
        inertia[0] += self_i + m * (dy * dy + dz * dz);
        inertia[1] += self_i + m * (dx * dx + dz * dz);
        inertia[2] += self_i + m * (dx * dx + dy * dy);
    }

    // Clamp minimum inertia so single-block contraptions never suffer singular spin spikes.
    let min_i = safe_mass * 0.5;
    inertia[0] = inertia[0].max(min_i);
    inertia[1] = inertia[1].max(min_i);
    inertia[2] = inertia[2].max(min_i);

    MassProperties {
        total_mass: safe_mass,
        center_of_mass: com,
        inertia_diag: inertia,
        bounds_min: bmin,
        bounds_max: bmax,
    }
}
