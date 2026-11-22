//! Create's steam engine and boiler (SteamEngineBlock, SteamEngineBlockEntity, PoweredShaftBlock,
//! PoweredShaftBlockEntity, BoilerData, BoilerHeaters; MIT, THIRD_PARTY_NOTICES.md). An engine on
//! a fluid tank's side (its `face`/`facing` pointing away from the tank) makes the tank's column a
//! boiler, and drives the shaft two blocks out (`getShaftPos`) when it runs across the engine's
//! way: that shaft becomes a powered shaft, a source. The boiler's level is the least of its heat
//! (a blaze burner under its bottom: kindled or fading 1, seething 2), its size (blocks / 4) and
//! its water (mB pumped in a tick / 10), each at most 18; with only passive heat (a smouldering
//! burner, a campfire, magma, fire, lava) engines share 1/8. An engine turns its shaft at
//! `16 * (1 + min(3, ⌊4 × efficiency⌋))` RPM, 64 at full efficiency, with 1024 stress units for
//! each RPM at that efficiency. Water pumped into a running boiler is used up.
//!
//! Not as Create: water is measured over a second rather than Create's sampled maximum, the
//! capacity follows the speed's step, and whistles aren't there.
use super::kinetic::{Axis, Dir, Part};
use std::collections::{HashMap, HashSet, VecDeque};

/// `CStress.setCapacity(1024.0)` for the steam engine.
const CAPACITY: f32 = 1024.0;
/// How often a boiler is measured, in ticks.
pub(super) const SAMPLE: u64 = 20;
/// `passiveEngineEfficiency`.
const PASSIVE: f32 = 1.0 / 8.0;

/// What heats a boiler from under it: its active heat, or passive heat alone.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Heating {
    pub active: u32,
    pub passive: bool,
}

/// `BoilerData.getEngineEfficiency`: each of `engines` engines' efficiency for a boiler of
/// `size` blocks taking `water` mB a tick.
pub(super) fn efficiency(heat: Heating, size: u32, water: f32, engines: u32) -> f32 {
    if engines == 0 {
        return 0.0;
    }
    let for_size = (size / 4).min(18);
    let for_water = ((water.ceil() as u32) / 10).min(18);
    if heat.active == 0 {
        return if heat.passive && for_size > 0 && for_water > 0 { PASSIVE / engines as f32 } else { 0.0 };
    }
    let actual = heat.active.min(for_size).min(for_water);
    if engines <= actual { 1.0 } else { actual as f32 / engines as f32 }
}

/// `PoweredShaftBlockEntity.getGeneratedSpeed`'s size for an efficiency.
pub(super) fn speed(efficiency: f32) -> f32 {
    if efficiency <= 0.0 {
        return 0.0;
    }
    let step = if efficiency >= 1.0 { 3.0 } else { (efficiency * 4.0).floor().min(2.0) };
    16.0 * (1.0 + step)
}

/// A powered shaft's capacity for each RPM, by its speed's step.
pub(super) fn capacity_per_rpm(speed: f32) -> f32 {
    let efficiency = match speed.abs() {
        s if s >= 64.0 => 1.0,
        s if s >= 48.0 => 0.5,
        s if s >= 32.0 => 0.25,
        s if s > 0.0 => PASSIVE,
        _ => 0.0,
    };
    CAPACITY * efficiency
}

/// SteamEngineBlockEntity's direction for the shaft: a vertical shaft turns one way, others by
/// the engine's facing (`convertToDirection`), reversed along z.
pub(super) fn direction(toward: Dir, axis: Axis) -> f32 {
    if axis == Axis::Y {
        return 1.0;
    }
    let d = if toward.positive() { 1.0 } else { -1.0 };
    if axis == Axis::Z { -d } else { d }
}

/// A multiblock boiler: its canonical base (`bottom`, lowest `(x, y, z)` in `bases`), all column
/// bases under its footprint (`bases`), all tank blocks (`tanks`), and its attached engines
/// (`engines`, each `(pos, facing)`).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Boiler {
    pub bottom: [i32; 3],
    pub bases: Vec<[i32; 3]>,
    pub tanks: Vec<[i32; 3]>,
    pub engines: Vec<([i32; 3], Dir)>,
}

/// The boilers the engines in `parts` make: horizontally adjacent `Part::Tank` columns of
/// matching bottom Y and height merge into connected footprints up to 3x3 on XZ, collecting
/// all tank blocks, column bases (`bases`), and attached engines across each footprint.
pub(super) fn boilers(parts: &HashMap<[i32; 3], Part>) -> Vec<Boiler> {
    let is_tank = |p: [i32; 3]| parts.get(&p) == Some(&Part::Tank);

    let mut columns: HashMap<[i32; 3], i32> = HashMap::new();
    for &p in parts.keys() {
        if is_tank(p) && !is_tank([p[0], p[1] - 1, p[2]]) {
            let mut h = 1;
            while is_tank([p[0], p[1] + h, p[2]]) {
                h += 1;
            }
            columns.insert(p, h);
        }
    }

    let mut sorted_bases: Vec<[i32; 3]> = columns.keys().copied().collect();
    sorted_bases.sort_by_key(|p| (p[1], p[0], p[2]));

    let mut claimed: HashSet<[i32; 3]> = HashSet::new();
    let mut footprints: Vec<Vec<[i32; 3]>> = Vec::new();
    let mut block_to_footprint: HashMap<[i32; 3], usize> = HashMap::new();

    for seed in sorted_bases {
        if !claimed.insert(seed) {
            continue;
        }
        let h = columns[&seed];
        let (mut min_x, mut max_x) = (seed[0], seed[0]);
        let (mut min_z, mut max_z) = (seed[2], seed[2]);
        let mut group = vec![seed];
        let mut queue = VecDeque::from([seed]);

        while let Some(c) = queue.pop_front() {
            for [dx, dz] in [[-1, 0], [1, 0], [0, -1], [0, 1]] {
                let nb = [c[0] + dx, c[1], c[2] + dz];
                if claimed.contains(&nb) || columns.get(&nb) != Some(&h) {
                    continue;
                }
                let nx_min = min_x.min(nb[0]);
                let nx_max = max_x.max(nb[0]);
                let nz_min = min_z.min(nb[2]);
                let nz_max = max_z.max(nb[2]);
                if nx_max - nx_min + 1 > 3 || nz_max - nz_min + 1 > 3 {
                    continue;
                }
                min_x = nx_min;
                max_x = nx_max;
                min_z = nz_min;
                max_z = nz_max;
                claimed.insert(nb);
                group.push(nb);
                queue.push_back(nb);
            }
        }

        group.sort();
        let fp_idx = footprints.len();
        for &base in &group {
            for dy in 0..h {
                block_to_footprint.insert([base[0], base[1] + dy, base[2]], fp_idx);
            }
        }
        footprints.push(group);
    }

    let mut engines: Vec<([i32; 3], Dir)> = (parts.iter())
        .filter_map(|(p, part)| match part {
            Part::SteamEngine(toward) => Some((*p, *toward)),
            _ => None,
        })
        .collect();
    engines.sort_by_key(|e| e.0);

    let mut out: HashMap<usize, Boiler> = HashMap::new();
    for (pos, toward) in engines {
        let o = toward.offset();
        let t = [pos[0] - o[0], pos[1] - o[1], pos[2] - o[2]];
        let Some(&fp_idx) = block_to_footprint.get(&t) else {
            continue;
        };
        let boiler = out.entry(fp_idx).or_insert_with(|| {
            let bases = footprints[fp_idx].clone();
            let bottom = bases[0];
            let mut tanks = Vec::new();
            for &base in &bases {
                for dy in 0..columns[&base] {
                    tanks.push([base[0], base[1] + dy, base[2]]);
                }
            }
            tanks.sort();
            Boiler { bottom, bases, tanks, engines: Vec::new() }
        });
        boiler.engines.push((pos, toward));
    }

    let mut list: Vec<Boiler> = out.into_values().collect();
    list.sort_by_key(|b| b.bottom);
    list
}

/// `BoilerHeaters`: what a block under a boiler gives, by its name and the burner's heat.
pub(super) fn heating(name: &str, heat: super::burner::Heat) -> Heating {
    use super::burner::Heat;
    match heat {
        Heat::Seething => Heating { active: 2, passive: false },
        Heat::Kindled | Heat::Fading => Heating { active: 1, passive: false },
        Heat::Smouldering => Heating { active: 0, passive: true },
        Heat::None => Heating {
            active: 0,
            passive: matches!(
                name,
                "minecraft:magma_block" | "minecraft:fire" | "minecraft:soul_fire" | "minecraft:lava"
                    | "minecraft:campfire" | "minecraft:soul_campfire"
            ),
        },
    }
}

/// Combines base block heats under a multiblock boiler footprint (`BoilerData.updateTemperature`):
/// active heat sums across all bases; passive heat applies only when total active heat is zero.
pub(super) fn sum_heating(heats: impl IntoIterator<Item = Heating>) -> Heating {
    let mut active = 0u32;
    let mut passive = false;
    for h in heats {
        active += h.active;
        passive |= h.passive;
    }
    Heating { active, passive: passive && active == 0 }
}

/// `SteamEngineRenderer`'s piston Y translation offset in blocks (`piston + 20/16`, range
/// `0.0..=0.75`) and linkage X-rotation angle in degrees (`angle2`) for shaft angle `angle_deg`.
pub(super) fn piston_travel(angle_deg: f32) -> (f32, f32) {
    const CRANK: f32 = 6.0 / 16.0;
    const ROD: f32 = 14.0 / 16.0;
    let rad = angle_deg.to_radians();
    let sin = rad.sin();
    let cos = rad.cos();
    let piston = CRANK * sin - (ROD * ROD - CRANK * CRANK * cos * cos).max(0.0).sqrt();
    let distance = (piston - CRANK * sin).abs();
    let linkage_rad = (distance / ROD).clamp(-1.0, 1.0).acos() * if cos >= 0.0 { 1.0 } else { -1.0 };
    (piston + 20.0 / 16.0, linkage_rad.to_degrees())
}

impl super::super::Game<'_> {
    /// The owner's boilers, measured every `SAMPLE` ticks: their engines turn the shafts before
    /// them (made powered shafts) or let them go.
    pub(super) fn tachyon_steam(&mut self) {
        if self.remote.is_some() {
            return;
        }
        let dimension = self.native_dimension();
        let due = super::OWNERS.with_borrow(|owners| owners.get(dimension).is_some_and(|o| o.ticks % SAMPLE == 0));
        if !due {
            return;
        }
        let parts: HashMap<[i32; 3], Part> = super::OWNERS.with_borrow(|owners| {
            owners.get(dimension).map(|o| o.index.parts.clone()).unwrap_or_default()
        });
        let mut steam: HashMap<[i32; 3], f32> = HashMap::new();
        let mut boiling: HashMap<[i32; 3], [i32; 3]> = HashMap::new();
        for b in boilers(&parts) {
            let heat = sum_heating(b.bases.iter().map(|base| {
                let under = [base[0], base[1] - 1, base[2]];
                let state = self.world.block(under[0], under[1], under[2]);
                let name = self.pack.assets.registry.state(state).map(|s| s.name.clone()).unwrap_or_default();
                heating(&name, self.tachyon_heat(under))
            }));
            let water = super::OWNERS.with_borrow_mut(|owners| {
                owners.get_mut(dimension).and_then(|o| o.boiled.remove(&b.bottom)).unwrap_or(0)
            }) as f32
                / SAMPLE as f32;
            let size = b.tanks.len() as u32;
            let engines = b.engines.len() as u32;
            let e = efficiency(heat, size, water, engines);
            // Heated, it boils the water it takes.
            if heat.active > 0 || heat.passive {
                for t in &b.tanks {
                    boiling.insert(*t, b.bottom);
                }
            }
            for (pos, toward) in &b.engines {
                let o = toward.offset();
                let shaft = [pos[0] + o[0] * 2, pos[1] + o[1] * 2, pos[2] + o[2] * 2];
                let axis = match parts.get(&shaft) {
                    Some(Part::Shaft(a) | Part::PoweredShaft(a)) if *a != toward.axis() => *a,
                    _ => continue,
                };
                if e > 0.0 {
                    steam.insert(shaft, speed(e) * direction(*toward, axis));
                }
            }
        }
        // Shafts made powered, or let go.
        let shafts: Vec<([i32; 3], Axis, bool)> = (parts.iter())
            .filter_map(|(p, part)| match part {
                Part::Shaft(a) if steam.contains_key(p) => Some((*p, *a, true)),
                Part::PoweredShaft(a) if !steam.contains_key(p) => Some((*p, *a, false)),
                _ => None,
            })
            .collect();
        for (p, axis, power) in shafts {
            let name = if power { "create:powered_shaft" } else { "create:shaft" };
            self.tachyon_set_with_axis(p, name, axis);
        }
        super::OWNERS.with_borrow_mut(|owners| {
            let Some(o) = owners.get_mut(dimension) else { return };
            o.boiling = boiling;
            o.boiled.clear();
            if steam != o.steam {
                o.steam = steam;
                o.dirty = true;
            }
        });
    }

    /// Sets the block at `p` to `name` along `axis`.
    fn tachyon_set_with_axis(&mut self, p: [i32; 3], name: &str, axis: Axis) {
        let registry = self.pack.assets.registry;
        let Some(mut state) = registry.default_state(name).and_then(|s| registry.state(s)).cloned() else {
            return;
        };
        let axis = match axis {
            Axis::X => "x",
            Axis::Y => "y",
            Axis::Z => "z",
        };
        state.properties.insert("axis".into(), axis.into());
        let Ok(id) = registry.resolve(&state) else { return };
        let dimension = self.native_dimension();
        self.native_set_block(dimension, p, id, 3);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_boiler_runs_its_engines_by_heat_size_and_water() {
        let heated = Heating { active: 1, passive: false };
        // Four tanks, ten mB a tick, a lit burner: level 1, one engine at full speed.
        assert_eq!(efficiency(heated, 4, 10.0, 1), 1.0);
        assert_eq!(speed(1.0), 64.0);
        // Two engines on level 1 share it.
        assert_eq!(efficiency(heated, 4, 10.0, 2), 0.5);
        assert_eq!(speed(0.5), 48.0);
        // Too little water or too small: nothing.
        assert_eq!(efficiency(heated, 4, 5.0, 1), 0.0);
        assert_eq!(efficiency(heated, 3, 10.0, 1), 0.0);
        // Passive heat: an eighth.
        let passive = Heating { active: 0, passive: true };
        assert_eq!(efficiency(passive, 4, 10.0, 1), 0.125);
        assert_eq!(speed(0.125), 16.0);
        assert_eq!(capacity_per_rpm(64.0), 1024.0);
        assert_eq!(capacity_per_rpm(16.0), 128.0);
        assert_eq!(direction(Dir::Up, Axis::Y), 1.0);
        assert_eq!(direction(Dir::West, Axis::Z), 1.0);
    }

    #[test]
    fn engines_find_the_column_of_tanks_behind_them() {
        let parts = HashMap::from([
            ([0, 0, 0], Part::Tank),
            ([0, 1, 0], Part::Tank),
            ([0, 2, 0], Part::Tank),
            ([1, 1, 0], Part::SteamEngine(Dir::East)),
            ([0, 3, 0], Part::SteamEngine(Dir::Up)),
            ([5, 0, 0], Part::SteamEngine(Dir::East)),
        ]);
        let b = boilers(&parts);
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].bottom, [0, 0, 0]);
        assert_eq!(b[0].bases, vec![[0, 0, 0]]);
        assert_eq!(b[0].tanks.len(), 3);
        assert_eq!(b[0].engines.len(), 2);
    }

    #[test]
    fn multiblock_boilers_merge_up_to_3x3_and_sum_base_heat() {
        // 2x2x2 boiler with engines on opposite columns merges into one boiler.
        let mut parts_2x2 = HashMap::new();
        for x in 0..2 {
            for y in 0..2 {
                for z in 0..2 {
                    parts_2x2.insert([x, y, z], Part::Tank);
                }
            }
        }
        parts_2x2.insert([-1, 0, 0], Part::SteamEngine(Dir::West));
        parts_2x2.insert([2, 1, 1], Part::SteamEngine(Dir::East));
        let b2 = boilers(&parts_2x2);
        assert_eq!(b2.len(), 1);
        assert_eq!(b2[0].bottom, [0, 0, 0]);
        assert_eq!(b2[0].bases.len(), 4);
        assert_eq!(b2[0].tanks.len(), 8);
        assert_eq!(b2[0].engines.len(), 2);

        // 3x3x2 boiler merges all 9 bases and 18 tanks.
        let mut parts_3x3 = HashMap::new();
        for x in 0..3 {
            for y in 0..2 {
                for z in 0..3 {
                    parts_3x3.insert([x, y, z], Part::Tank);
                }
            }
        }
        parts_3x3.insert([0, 2, 0], Part::SteamEngine(Dir::Up));
        parts_3x3.insert([3, 1, 2], Part::SteamEngine(Dir::East));
        let b3 = boilers(&parts_3x3);
        assert_eq!(b3.len(), 1);
        assert_eq!(b3[0].bases.len(), 9);
        assert_eq!(b3[0].tanks.len(), 18);
        assert_eq!(b3[0].engines.len(), 2);

        // >3x3 footprint (4x1 and 4x4) stays split so no boiler exceeds 3x3 on XZ.
        let mut parts_4x1 = HashMap::from([
            ([0, 0, 0], Part::Tank),
            ([1, 0, 0], Part::Tank),
            ([2, 0, 0], Part::Tank),
            ([3, 0, 0], Part::Tank),
            ([-1, 0, 0], Part::SteamEngine(Dir::West)),
            ([4, 0, 0], Part::SteamEngine(Dir::East)),
        ]);
        let b4x1 = boilers(&parts_4x1);
        assert_eq!(b4x1.len(), 2);
        assert_eq!(b4x1[0].bases.len(), 3);
        assert_eq!(b4x1[1].bases, vec![[3, 0, 0]]);
        parts_4x1.clear();

        let mut parts_4x4 = HashMap::new();
        for x in 0..4 {
            for z in 0..4 {
                parts_4x4.insert([x, 0, z], Part::Tank);
            }
        }
        parts_4x4.insert([-1, 0, 0], Part::SteamEngine(Dir::West));
        parts_4x4.insert([4, 0, 3], Part::SteamEngine(Dir::East));
        let b4x4 = boilers(&parts_4x4);
        assert_eq!(b4x4.len(), 2);
        assert!(b4x4.iter().all(|b| b.bases.len() <= 9));
        assert_eq!(b4x4[0].bases.len(), 9);
        assert_eq!(b4x4[1].bases, vec![[3, 0, 3]]);

        // Base heating sum: active adds across all bases; passive only applies when active == 0.
        assert_eq!(
            sum_heating([Heating { active: 2, passive: false }; 9]),
            Heating { active: 18, passive: false }
        );
        assert_eq!(
            sum_heating([
                Heating { active: 1, passive: false },
                Heating { active: 0, passive: true },
            ]),
            Heating { active: 1, passive: false }
        );
        assert_eq!(
            sum_heating([Heating { active: 0, passive: true }; 4]),
            Heating { active: 0, passive: true }
        );
        assert_eq!(efficiency(sum_heating([Heating { active: 2, passive: false }; 9]), 72, 180.0, 18), 1.0);
    }

    #[test]
    fn piston_travel_matches_steam_engine_renderer() {
        let (top_y, top_deg) = piston_travel(90.0);
        assert!((top_y - 0.75).abs() < 1e-5);
        assert!(top_deg.abs() < 1e-4);

        let (bot_y, bot_deg) = piston_travel(270.0);
        assert!(bot_y.abs() < 1e-5);
        assert!(bot_deg.abs() < 1e-4);

        let (mid_y0, mid_deg0) = piston_travel(0.0);
        let (mid_y180, mid_deg180) = piston_travel(180.0);
        let expected_y = 20.0 / 16.0 - ((14.0_f32 / 16.0).powi(2) - (6.0_f32 / 16.0).powi(2)).sqrt();
        let expected_deg = (6.0_f32 / 14.0).asin().to_degrees();
        assert!((mid_y0 - expected_y).abs() < 1e-5);
        assert!((mid_y180 - expected_y).abs() < 1e-5);
        assert!((mid_deg0 - expected_deg).abs() < 1e-4);
        assert!((mid_deg180 + expected_deg).abs() < 1e-4);
    }
}
