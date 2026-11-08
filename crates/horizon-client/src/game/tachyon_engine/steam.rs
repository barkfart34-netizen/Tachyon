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
//! Not as Create: a boiler is one tank column (no wider tanks, so one burner heats it), the
//! water is measured over a second rather than Create's sampled maximum, the capacity follows the
//! speed's step, whistles aren't there and the engine's piston isn't drawn moving.
use super::kinetic::{Axis, Dir, Part};
use std::collections::HashMap;

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

/// A boiler: its tank column's bottom, its size and its engines (each with its facing).
#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct Boiler {
    pub bottom: [i32; 3],
    pub tanks: Vec<[i32; 3]>,
    pub engines: Vec<([i32; 3], Dir)>,
}

/// The boilers the engines in `parts` make: each engine on the tank behind it, by column.
pub(super) fn boilers(parts: &HashMap<[i32; 3], Part>) -> Vec<Boiler> {
    let tank = |p: [i32; 3]| parts.get(&p) == Some(&Part::Tank);
    let mut out: HashMap<[i32; 3], Boiler> = HashMap::new();
    let mut engines: Vec<([i32; 3], Dir)> = (parts.iter())
        .filter_map(|(p, part)| match part {
            Part::SteamEngine(toward) => Some((*p, *toward)),
            _ => None,
        })
        .collect();
    engines.sort_by_key(|e| e.0);
    for (pos, toward) in engines {
        let o = toward.offset();
        let mut t = [pos[0] - o[0], pos[1] - o[1], pos[2] - o[2]];
        if !tank(t) {
            continue;
        }
        while tank([t[0], t[1] - 1, t[2]]) {
            t[1] -= 1;
        }
        let boiler = out.entry(t).or_insert_with(|| {
            let mut tanks = Vec::new();
            let mut p = t;
            while tank(p) {
                tanks.push(p);
                p[1] += 1;
            }
            Boiler { bottom: t, tanks, engines: Vec::new() }
        });
        boiler.engines.push((pos, toward));
    }
    let mut list: Vec<Boiler> = out.into_values().collect();
    list.sort_by_key(|b| b.bottom);
    list
}

/// `BoilerHeaters`: what a block under a boiler gives, by its name and the burner's heat.
fn heating(name: &str, heat: super::burner::Heat) -> Heating {
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
            let under = [b.bottom[0], b.bottom[1] - 1, b.bottom[2]];
            let state = self.world.block(under[0], under[1], under[2]);
            let name = self.pack.assets.registry.state(state).map(|s| s.name.clone()).unwrap_or_default();
            let heat = heating(&name, self.tachyon_heat(under));
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
        assert_eq!(b[0].tanks.len(), 3);
        assert_eq!(b[0].engines.len(), 2);
    }
}
