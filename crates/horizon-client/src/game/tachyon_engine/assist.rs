//! Create's placement helpers (PoleHelper, CogwheelBlockItem's
//! SmallCogHelper/LargeCogHelper/DiagonalCogHelper; MIT,
//! THIRD_PARTY_NOTICES.md) on the ported create:* blocks, after Kinetics for
//! Horizon's assist (sakurartro, MIT). A shaft in hand aimed at a shaft
//! shows the free cell at the end of its row and a use puts it there; a
//! cogwheel in hand aimed at a cogwheel shows the meshing cell. The ghost is
//! drawn in the HUD from the camera's view (no depth test, no bobbing), as
//! Kinetics does.
use super::water_wheel::replaceable;
use horizon_client::hud::HudBatch;
use horizon_client::items::ItemStack;

pub(super) const SHAFT: &str = "create:shaft";
const COG: &str = "create:cogwheel";
const LARGE_COG: &str = "create:large_cogwheel";
/// How far along a row the helper reaches (Create's `placementAssistRange`).
const RANGE: i32 = 12;
/// How near the aimed block must be.
const REACH: f32 = 6.0;

/// 0, 1, 2 for the axis `x`, `y`, `z`.
fn axis_index(axis: &str) -> Option<usize> {
    ["x", "y", "z"].iter().position(|a| *a == axis)
}

/// `PoleHelper.getOffset`: the free cell along `axis` past the shafts in a
/// row from `pos`, on the side the hit is nearer to (else the other).
fn pole_target(
    axis: usize,
    pos: [i32; 3],
    hit: [f64; 3],
    is_pole: impl Fn([i32; 3]) -> bool,
    free: impl Fn([i32; 3]) -> bool,
) -> Option<[i32; 3]> {
    let at = |dir: i32, n: i32| {
        let mut p = pos;
        p[axis] += dir * n;
        p
    };
    let distance = |dir: i32| {
        let c = at(dir, 1);
        (0..3)
            .map(|i| (f64::from(c[i]) + 0.5 - hit[i]).powi(2))
            .sum::<f64>()
    };
    let mut dirs = [1, -1];
    dirs.sort_by(|a, b| distance(*a).total_cmp(&distance(*b)));
    for dir in dirs {
        let mut poles = 0;
        while poles < RANGE && is_pole(at(dir, poles + 1)) {
            poles += 1;
        }
        if poles < RANGE && free(at(dir, poles + 1)) {
            return Some(at(dir, poles + 1));
        }
    }
    None
}

/// The shaft's box in `cell` (4 px thick), min and max corners.
fn shaft_box(axis: usize, cell: [i32; 3]) -> ([f64; 3], [f64; 3]) {
    let mut min = cell.map(|c| f64::from(c) + 0.375);
    let mut max = cell.map(|c| f64::from(c) + 0.625);
    min[axis] = f64::from(cell[axis]);
    max[axis] = f64::from(cell[axis]) + 1.0;
    (min, max)
}

/// What the cog rules know of a cell.
#[derive(Clone, Copy, Default)]
struct Cell {
    /// `Some(large)` for a cogwheel.
    cog: Option<bool>,
    axis: Option<usize>,
    free: bool,
}

/// `Some(true)` for a large cogwheel (bare or cased), `Some(false)` a small one.
fn cog_kind(block: &str) -> Option<bool> {
    match block.strip_prefix("create:")? {
        "cogwheel" | "andesite_encased_cogwheel" | "brass_encased_cogwheel" => Some(false),
        "large_cogwheel" | "andesite_encased_large_cogwheel" | "brass_encased_large_cogwheel" => {
            Some(true)
        }
        _ => None,
    }
}

/// Axis and sign, in `Direction.values()` order.
type Dir = (usize, i32);
const DIRECTIONS: [Dir; 6] = [(1, -1), (1, 1), (2, -1), (2, 1), (0, -1), (0, 1)];

/// `IPlacementHelper.orderedByDistance`: the kept directions, nearest the hit first.
fn ordered(pos: [i32; 3], hit: [f64; 3], keep: impl Fn(Dir) -> bool) -> Vec<Dir> {
    let off: [f64; 3] = std::array::from_fn(|i| hit[i] - (f64::from(pos[i]) + 0.5));
    let dist = |d: Dir| {
        (0..3)
            .map(|i| (if i == d.0 { f64::from(d.1) } else { 0.0 } - off[i]).powi(2))
            .sum::<f64>()
    };
    let mut v: Vec<Dir> = DIRECTIONS.into_iter().filter(|d| keep(*d)).collect();
    v.sort_by(|a, b| dist(*a).total_cmp(&dist(*b)));
    v
}

fn step(p: [i32; 3], d: Dir) -> [i32; 3] {
    let mut q = p;
    q[d.0] += d.1;
    q
}

/// `CogWheelBlock.isValidCogwheelPosition`.
fn cog_position_ok(
    large: bool,
    cell: [i32; 3],
    axis: usize,
    info: &impl Fn([i32; 3]) -> Cell,
) -> bool {
    DIRECTIONS.into_iter().filter(|d| d.0 != axis).all(|d| {
        let n = info(step(cell, d));
        n.axis == Some(d.0) || !(n.cog == Some(true) || (large && n.cog == Some(false)))
    })
}

/// The cell and axis for a cog of size `hand_large` aimed at the cog at
/// `pos`; nothing when the hit is on its shaft.
fn cog_target(
    hand_large: bool,
    pos: [i32; 3],
    hit: [f64; 3],
    info: impl Fn([i32; 3]) -> Cell,
) -> Option<([i32; 3], usize)> {
    let here = info(pos);
    let (aimed_large, axis) = (here.cog?, here.axis?);
    let on_shaft = (0..3).filter(|i| *i != axis).all(|i| {
        let l = hit[i] - hit[i].floor();
        (5.0 / 16.0 - 0.001..=11.0 / 16.0 + 0.001).contains(&l)
    });
    if on_shaft {
        return None;
    }
    if hand_large != aimed_large {
        let closest = *ordered(pos, hit, |d| d.0 != axis).first()?;
        return ordered(pos, hit, |d| d.0 != axis && d.0 != closest.0)
            .into_iter()
            .find_map(|d| {
                let cell = step(step(pos, d), closest);
                (info(cell).free && cog_position_ok(aimed_large, cell, axis, &info))
                    .then_some((cell, axis))
            });
    }
    if !hand_large {
        return ordered(pos, hit, |d| d.0 != axis)
            .into_iter()
            .find_map(|d| {
                let cell = step(pos, d);
                (cog_position_ok(false, cell, axis, &info) && info(cell).free)
                    .then_some((cell, axis))
            });
    }
    let side = *ordered(pos, hit, |d| d.0 == axis).first()?;
    ordered(pos, hit, |d| d.0 != axis)
        .into_iter()
        .find_map(|d| {
            let cell = step(step(pos, d), side);
            (cog_position_ok(true, cell, d.0, &info) && info(cell).free).then_some((cell, d.0))
        })
}

/// A cogwheel's plate in `cell` on `axis`.
fn cog_box(large: bool, axis: usize, cell: [i32; 3]) -> ([f64; 3], [f64; 3]) {
    let m = if large { 0.0 } else { 0.125 };
    let mut min = cell.map(|c| f64::from(c) + m);
    let mut max = cell.map(|c| f64::from(c) + 1.0 - m);
    min[axis] = f64::from(cell[axis]) + 0.375;
    max[axis] = f64::from(cell[axis]) + 0.625;
    (min, max)
}

/// Where `p` falls on a `size` GUI-pixel screen for a camera at `eye`
/// (Minecraft yaw/pitch, vertical `fov`, degrees); none behind it.
fn project(
    eye: [f64; 3],
    yaw: f32,
    pitch: f32,
    fov: f32,
    size: [f32; 2],
    p: [f64; 3],
) -> Option<[f32; 2]> {
    let (y, q) = (f64::from(yaw).to_radians(), f64::from(pitch).to_radians());
    let forward = [-y.sin() * q.cos(), -q.sin(), y.cos() * q.cos()];
    let right = [-y.cos(), 0.0, -y.sin()];
    let up = [
        right[1] * forward[2] - right[2] * forward[1],
        right[2] * forward[0] - right[0] * forward[2],
        right[0] * forward[1] - right[1] * forward[0],
    ];
    let d = [p[0] - eye[0], p[1] - eye[1], p[2] - eye[2]];
    let dot = |a: [f64; 3]| a[0] * d[0] + a[1] * d[1] + a[2] * d[2];
    let depth = dot(forward);
    if depth < 0.05 {
        return None;
    }
    let tan = (f64::from(fov).to_radians() / 2.0).tan();
    let aspect = f64::from(size[0] / size[1]);
    let x = dot(right) / (depth * tan * aspect);
    let v = dot(up) / (depth * tan);
    Some([
        ((x * 0.5 + 0.5) as f32) * size[0],
        ((0.5 - v * 0.5) as f32) * size[1],
    ])
}

type Quad = ([[f32; 2]; 4], u32);

/// The ghost box `lo`..`hi` (shaded faces toward the camera, outline) and
/// an arrow by the crosshair from `pos` toward `target`; ARGB colours.
#[allow(clippy::too_many_arguments)]
fn overlay(
    eye: [f64; 3],
    yaw: f32,
    pitch: f32,
    fov: f32,
    size: [f32; 2],
    lo: [f64; 3],
    hi: [f64; 3],
    pos: [i32; 3],
    target: [i32; 3],
) -> Vec<Quad> {
    let project = |p: [f64; 3]| project(eye, yaw, pitch, fov, size, p);
    let mut out = Vec::new();
    let corner = |i: usize| {
        [
            if i & 1 == 0 { lo[0] } else { hi[0] },
            if i & 2 == 0 { lo[1] } else { hi[1] },
            if i & 4 == 0 { lo[2] } else { hi[2] },
        ]
    };
    let screen: Vec<Option<[f32; 2]>> = (0..8).map(|i| project(corner(i))).collect();
    let faces: [([usize; 4], usize, bool, f32); 6] = [
        ([0, 2, 6, 4], 0, false, 0.6),
        ([1, 3, 7, 5], 0, true, 0.6),
        ([0, 1, 5, 4], 1, false, 0.5),
        ([2, 3, 7, 6], 1, true, 1.0),
        ([0, 1, 3, 2], 2, false, 0.8),
        ([4, 5, 7, 6], 2, true, 0.8),
    ];
    for (ids, axis, high, shade) in faces {
        let toward = if high {
            eye[axis] > hi[axis]
        } else {
            eye[axis] < lo[axis]
        };
        let corners: Option<Vec<[f32; 2]>> = ids.iter().map(|i| screen[*i]).collect();
        let Some(c) = corners.filter(|_| toward) else {
            continue;
        };
        let grey = (0xB8 as f32 * shade) as u32;
        out.push((
            [c[0], c[1], c[2], c[3]],
            0x90_000000 | grey << 16 | grey << 8 | grey,
        ));
    }
    let edges = (0..8usize)
        .flat_map(|i| [(i, i | 1), (i, i | 2), (i, i | 4)])
        .filter(|(i, j)| i != j);
    for (from, to) in edges {
        if let (Some(p), Some(q)) = (screen[from], screen[to]) {
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let len = (dx * dx + dy * dy).sqrt().max(0.001);
            let n = [-dy / len * 0.5, dx / len * 0.5];
            out.push((
                [
                    [p[0] + n[0], p[1] + n[1]],
                    [q[0] + n[0], q[1] + n[1]],
                    [q[0] - n[0], q[1] - n[1]],
                    [p[0] - n[0], p[1] - n[1]],
                ],
                0xC0_101010,
            ));
        }
    }
    let centre = |c: [i32; 3]| {
        project([
            f64::from(c[0]) + 0.5,
            f64::from(c[1]) + 0.5,
            f64::from(c[2]) + 0.5,
        ])
    };
    let (Some(from), Some(to)) = (centre(pos), centre(target)) else {
        return out;
    };
    let (dx, dy) = (to[0] - from[0], to[1] - from[1]);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.5 {
        return out;
    }
    let (d, n) = ([dx / len, dy / len], [-dy / len, dx / len]);
    let (cx, cy) = (size[0] / 2.0 + d[0] * 14.0, size[1] / 2.0 + d[1] * 14.0);
    let at = |along: f32, across: f32| {
        [
            cx + d[0] * along + n[0] * across,
            cy + d[1] * along + n[1] * across,
        ]
    };
    for (grow, color) in [(1.0, 0xFF_101010_u32), (0.0, 0xFF_FFFFFF)] {
        out.push((
            [
                at(-5.0 - grow, -1.5 - grow),
                at(1.0, -1.5 - grow),
                at(1.0, 1.5 + grow),
                at(-5.0 - grow, 1.5 + grow),
            ],
            color,
        ));
        out.push((
            [
                at(0.0, -4.0 - grow),
                at(5.0 + grow * 1.5, 0.0),
                at(5.0 + grow * 1.5, 0.0),
                at(0.0, 4.0 + grow),
            ],
            color,
        ));
    }
    out
}

/// What the helper offers: the aimed block, the cell, its axis and the ghost box.
struct Offer {
    pos: [i32; 3],
    cell: [i32; 3],
    axis: usize,
    lo: [f64; 3],
    hi: [f64; 3],
}

impl super::super::Game<'_> {
    fn tachyon_assist_offer(&self, item: &str) -> Option<Offer> {
        let hand_cog = match item {
            COG => Some(false),
            LARGE_COG => Some(true),
            SHAFT => None,
            _ => return None,
        };
        let (distance, hit) = self.native_view_hit()?;
        if distance > REACH {
            return None;
        }
        let pos = [hit.block_x, hit.block_y, hit.block_z];
        let point = [hit.x, hit.y, hit.z];
        let free = |p: [i32; 3]| {
            replaceable(
                &self.native_block(p),
                self.native_block_property(p, "layers").as_deref(),
            )
        };
        if let Some(large) = hand_cog {
            let info = |p: [i32; 3]| Cell {
                cog: cog_kind(&self.native_block(p)),
                axis: self
                    .native_block_property(p, "axis")
                    .and_then(|a| axis_index(&a)),
                free: free(p),
            };
            let (cell, axis) = cog_target(large, pos, point, info)?;
            let (lo, hi) = cog_box(large, axis, cell);
            return Some(Offer {
                pos,
                cell,
                axis,
                lo,
                hi,
            });
        }
        if hit.block != SHAFT {
            return None;
        }
        let axis_name = self.native_block_property(pos, "axis")?;
        let axis = axis_index(&axis_name)?;
        let is_pole = |p: [i32; 3]| {
            self.native_block(p) == SHAFT
                && self.native_block_property(p, "axis").as_deref() == Some(axis_name.as_str())
        };
        let cell = pole_target(axis, pos, point, is_pole, free)?;
        let (lo, hi) = shaft_box(axis, cell);
        Some(Offer {
            pos,
            cell,
            axis,
            lo,
            hi,
        })
    }

    /// A shaft or cogwheel used on its kind goes where the helper shows.
    pub(in crate::game) fn tachyon_assist_use(
        &mut self,
        stack: &ItemStack,
        pos: [i32; 3],
        claimed: &mut bool,
    ) {
        if *claimed || !self.may_build() || self.block_action_restricted(pos) {
            return;
        }
        let item = stack.item.to_string();
        let Some(offer) = self.tachyon_assist_offer(&item) else {
            return;
        };
        // The ray is the local player's: a click it doesn't agree with places as usual.
        if offer.pos != pos || self.block_action_restricted(offer.cell) {
            return;
        }
        let registry = &self.pack.assets.registry;
        let Some(id) = registry.default_state(&item).and_then(|default| {
            let mut state = registry.state(default)?.clone();
            state
                .properties
                .insert("axis".into(), ["x", "y", "z"][offer.axis].into());
            registry.id(&state)
        }) else {
            return;
        };
        let dimension = self.native_dimension();
        if self.native_set_block(dimension, offer.cell, id, 3) {
            if !self.creative {
                self.native_take_items(&item, 1);
            }
            *claimed = true;
        }
    }

    pub(in crate::game) fn tachyon_assist_hud(&mut self, batch: &mut HudBatch) {
        if !self.native_window() {
            return;
        }
        let Some((item, _, _)) = self.native_held() else {
            return;
        };
        let Some(offer) = self.tachyon_assist_offer(&item) else {
            return;
        };
        let Some(eye) = self.eye_position() else {
            return;
        };
        let fov = self.fov_degrees(self.clock.camera_fraction() as f32, eye);
        let size = [batch.width, batch.height];
        let quads = overlay(
            eye, self.yaw, self.pitch, fov, size, offer.lo, offer.hi, offer.pos, offer.cell,
        );
        // ARGB to the batch's ABGR.
        let abgr = |argb: u32| (argb & 0xFF00_FF00) | (argb >> 16 & 0xFF) | (argb & 0xFF) << 16;
        for (quad, argb) in quads {
            batch.fill_quad(&self.gui.atlas, quad, abgr(argb));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shaft_goes_past_its_row_on_the_nearer_side() {
        let free = |_| true;
        assert_eq!(
            pole_target(0, [0, 0, 0], [0.9, 0.5, 0.5], |_| false, free),
            Some([1, 0, 0])
        );
        assert_eq!(
            pole_target(0, [0, 0, 0], [0.1, 0.5, 0.5], |_| false, free),
            Some([-1, 0, 0])
        );
        let row = |p: [i32; 3]| (1..=3).contains(&p[2]);
        assert_eq!(
            pole_target(2, [0, 0, 0], [0.5, 0.5, 0.9], row, |p| !row(p)),
            Some([0, 0, 4])
        );
        let long = |p: [i32; 3]| p[0] >= 1;
        assert_eq!(
            pole_target(0, [0, 0, 0], [0.9, 0.5, 0.5], long, |_| true),
            Some([-1, 0, 0])
        );
        assert_eq!(
            pole_target(0, [0, 0, 0], [0.9, 0.5, 0.5], |_| false, |_| false),
            None
        );
        assert_eq!(
            shaft_box(0, [2, 0, 0]),
            ([2.0, 0.375, 0.375], [3.0, 0.625, 0.625])
        );
    }

    fn world(cogs: Vec<([i32; 3], bool, usize)>) -> impl Fn([i32; 3]) -> Cell {
        move |p| match cogs.iter().find(|c| c.0 == p) {
            Some(c) => Cell {
                cog: Some(c.1),
                axis: Some(c.2),
                free: false,
            },
            None => Cell {
                cog: None,
                axis: None,
                free: true,
            },
        }
    }

    #[test]
    fn cogs_mesh_beside_or_diagonally() {
        let w = world(vec![([0, 0, 0], false, 1)]);
        assert_eq!(
            cog_target(false, [0, 0, 0], [0.95, 0.5, 0.5], &w),
            Some(([1, 0, 0], 1))
        );
        assert_eq!(cog_target(false, [0, 0, 0], [0.5, 0.5, 0.5], &w), None);
        assert_eq!(
            cog_target(true, [0, 0, 0], [0.95, 0.5, 0.8], &w),
            Some(([1, 0, 1], 1))
        );
        let w = world(vec![([0, 0, 0], true, 1)]);
        assert_eq!(
            cog_target(true, [0, 0, 0], [0.95, 0.9, 0.5], &w),
            Some(([1, 1, 0], 0))
        );
        assert_eq!(
            cog_target(false, [0, 0, 0], [0.95, 0.5, 0.8], &w),
            Some(([1, 0, 1], 1))
        );
        let w = world(vec![([0, 0, 0], false, 1), ([2, 0, 0], true, 1)]);
        assert_ne!(
            cog_target(false, [0, 0, 0], [0.95, 0.5, 0.5], &w).map(|t| t.0),
            Some([1, 0, 0])
        );
    }

    #[test]
    fn kinds_boxes_and_projection() {
        assert_eq!(cog_kind("create:cogwheel"), Some(false));
        assert_eq!(cog_kind("create:brass_encased_large_cogwheel"), Some(true));
        assert_eq!(cog_kind("create:shaft"), None);
        assert_eq!(
            cog_box(true, 1, [0, 0, 0]),
            ([0.0, 0.375, 0.0], [1.0, 0.625, 1.0])
        );
        let c = project([0.0; 3], 0.0, 0.0, 70.0, [320.0, 240.0], [0.0, 0.0, 5.0]).unwrap();
        assert!((c[0] - 160.0).abs() < 1e-3 && (c[1] - 120.0).abs() < 1e-3);
        assert!(project([0.0; 3], 0.0, 0.0, 70.0, [320.0, 240.0], [0.0, 0.0, -5.0]).is_none());
        let quads = overlay(
            [0.5, 0.5, -3.0],
            0.0,
            0.0,
            70.0,
            [320.0, 240.0],
            [0.0; 3],
            [1.0; 3],
            [0, 0, 0],
            [1, 0, 0],
        );
        assert!(quads.len() > 12);
    }
}
