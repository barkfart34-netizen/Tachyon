//! Kinetica's rotation: which parts turn which, and how fast.
//!
//! Every part turns about an axis. Its speed is in turns per minute (RPM)
//! about the axis's positive direction, by the right-hand rule: shafts pass
//! it on unchanged; meshing cogwheels reverse it (a large one drives a small
//! one twice as fast); a gearbox turns it through a right angle. The rules
//! follow Create's (MIT); the code is Tachyon's.
//!
//! [`solve`] gives every part its speed from the sources it is joined to.
//! A network whose parts disagree (two motors at odds, a loop of gears that
//! can't turn) or that would turn too fast stays still: it is jammed.
use std::collections::{HashMap, HashSet, VecDeque};

/// The fastest a part may turn.
pub const MAX_SPEED: f32 = 256.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Axis {
    X,
    Y,
    Z,
}

impl Axis {
    /// The component of `v` along this axis.
    pub fn pick(self, v: [i32; 3]) -> i32 {
        match self {
            Axis::X => v[0],
            Axis::Y => v[1],
            Axis::Z => v[2],
        }
    }
    pub fn parse(s: &str) -> Option<Axis> {
        match s {
            "x" => Some(Axis::X),
            "y" => Some(Axis::Y),
            "z" => Some(Axis::Z),
            _ => None,
        }
    }
}

/// The six directions, with vanilla's names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Dir {
    Down,
    Up,
    North,
    South,
    West,
    East,
}

impl Dir {
    pub const ALL: [Dir; 6] = [
        Dir::Down,
        Dir::Up,
        Dir::North,
        Dir::South,
        Dir::West,
        Dir::East,
    ];
    pub fn offset(self) -> [i32; 3] {
        match self {
            Dir::Down => [0, -1, 0],
            Dir::Up => [0, 1, 0],
            Dir::North => [0, 0, -1],
            Dir::South => [0, 0, 1],
            Dir::West => [-1, 0, 0],
            Dir::East => [1, 0, 0],
        }
    }
    pub fn axis(self) -> Axis {
        match self {
            Dir::Down | Dir::Up => Axis::Y,
            Dir::North | Dir::South => Axis::Z,
            Dir::West | Dir::East => Axis::X,
        }
    }
    pub fn positive(self) -> bool {
        matches!(self, Dir::Up | Dir::South | Dir::East)
    }
    pub fn opposite(self) -> Dir {
        match self {
            Dir::Down => Dir::Up,
            Dir::Up => Dir::Down,
            Dir::North => Dir::South,
            Dir::South => Dir::North,
            Dir::West => Dir::East,
            Dir::East => Dir::West,
        }
    }
    pub fn parse(s: &str) -> Option<Dir> {
        Dir::ALL.into_iter().find(|d| d.name() == s)
    }
    pub fn name(self) -> &'static str {
        match self {
            Dir::Down => "down",
            Dir::Up => "up",
            Dir::North => "north",
            Dir::South => "south",
            Dir::West => "west",
            Dir::East => "east",
        }
    }
}

/// A part that turns, as its block and state make it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// A shaft along its axis.
    Shaft(Axis),
    /// A small cogwheel on a shaft along its axis.
    Cog(Axis),
    /// A large cogwheel on a shaft along its axis.
    LargeCog(Axis),
    /// A gearbox: shafts out of its four sides around its axis.
    Gearbox(Axis),
    /// A motor turning its shaft out of one side.
    Motor(Dir),
}

impl Part {
    /// Whether the part has a shaft out of side `d`.
    pub fn shaft_toward(self, d: Dir) -> bool {
        match self {
            Part::Shaft(a) | Part::Cog(a) | Part::LargeCog(a) => d.axis() == a,
            Part::Gearbox(a) => d.axis() != a,
            Part::Motor(out) => d == out,
        }
    }
    /// The part's cogwheel: its axis and whether it is large.
    fn cog(self) -> Option<(Axis, bool)> {
        match self {
            Part::Cog(a) => Some((a, false)),
            Part::LargeCog(a) => Some((a, true)),
            _ => None,
        }
    }
    /// The speed of side `d`'s shaft for every turn of the part's own
    /// speed: 1 but for a gearbox, whose sides turn each its own way (as
    /// Create's: straight through reverses, a right angle depends on the
    /// sides' directions).
    pub fn side_factor(self, d: Dir) -> f32 {
        let Part::Gearbox(axis) = self else {
            return 1.0;
        };
        // The first side round the axis is the reference.
        let reference = Dir::ALL
            .into_iter()
            .find(|s| s.axis() != axis)
            .unwrap_or(Dir::North);
        gearbox_factor(reference, d)
    }
}

/// Create's gearbox: the speed out of side `out` for a speed coming in at
/// side `source`, both along their own axes.
fn gearbox_factor(source: Dir, out: Dir) -> f32 {
    if out.axis() == source.axis() {
        if out == source { 1.0 } else { -1.0 }
    } else if out.positive() == source.positive() {
        -1.0
    } else {
        1.0
    }
}

/// The speed a part at `to` turns at for each turn of the part at `from`,
/// `diff` being `to - from`, through their shafts or their cogwheels; None
/// if they don't touch. Speeds are the parts' own (for a gearbox, that of
/// its reference side).
pub fn link(from: Part, to: Part, diff: [i32; 3]) -> Option<f32> {
    let manhattan = diff.iter().map(|v| v.abs()).sum::<i32>();
    if manhattan == 1 {
        let d = Dir::ALL.into_iter().find(|d| d.offset() == diff)?;
        // Shaft to shaft: the same speed at the touching sides.
        if from.shaft_toward(d) && to.shaft_toward(d.opposite()) {
            return Some(from.side_factor(d) / to.side_factor(d.opposite()));
        }
        // Small cogwheels side by side on parallel axes mesh, reversed.
        if let (Some((a, false)), Some((b, false))) = (from.cog(), to.cog())
            && a == b
            && a.pick(diff) == 0
        {
            return Some(-1.0);
        }
        return None;
    }
    let (Some((a, a_large)), Some((b, b_large))) = (from.cog(), to.cog()) else {
        return None;
    };
    if manhattan != 2 || diff.iter().any(|v| v.abs() > 1) {
        return None;
    }
    if a == b {
        // A large cogwheel and a small one diagonally in their plane.
        if a.pick(diff) != 0 || a_large == b_large {
            return None;
        }
        return Some(if a_large { -2.0 } else { -0.5 });
    }
    // Two large cogwheels at right angles, diagonally.
    if a_large && b_large && a.pick(diff) != 0 && b.pick(diff) != 0 {
        return Some(if (a.pick(diff) > 0) ^ (b.pick(diff) > 0) {
            -1.0
        } else {
            1.0
        });
    }
    None
}

/// The positions a part may link with: its six neighbours, and for a
/// cogwheel the twelve diagonals too.
fn reach(part: Part) -> impl Iterator<Item = [i32; 3]> {
    let diagonals = part.cog().is_some();
    (-1..=1).flat_map(move |x| {
        (-1..=1).flat_map(move |y| {
            (-1..=1).filter_map(move |z| {
                let m = i32::abs(x) + i32::abs(y) + i32::abs(z);
                (m == 1 || (diagonals && m == 2)).then_some([x, y, z])
            })
        })
    })
}

fn add(a: [i32; 3], b: [i32; 3]) -> [i32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

/// The speeds of the parts joined to a source: each part's own speed (a
/// gearbox's is that of its reference side; [`Part::side_factor`] gives the
/// others). Parts not joined to a source, and the parts of a jammed network,
/// are left out (they stand still); `jammed` gets the latter.
pub struct Solved {
    pub speeds: HashMap<[i32; 3], f32>,
    pub jammed: HashSet<[i32; 3]>,
}

/// Solves the speeds of `parts`, with `sources` (positions of motors among
/// them) turning at their own speeds.
pub fn solve(parts: &HashMap<[i32; 3], Part>, sources: &HashMap<[i32; 3], f32>) -> Solved {
    let mut speeds: HashMap<[i32; 3], f32> = HashMap::new();
    let mut jammed: HashSet<[i32; 3]> = HashSet::new();
    let mut done: HashSet<[i32; 3]> = HashSet::new();
    let mut order: Vec<_> = sources.iter().collect();
    order.sort_by_key(|(p, _)| **p);
    for (&start, &speed) in order {
        if done.contains(&start) || !parts.contains_key(&start) {
            continue;
        }
        // One network: every part reached from this source.
        let mut network: HashMap<[i32; 3], f32> = HashMap::new();
        let mut bad = speed.abs() > MAX_SPEED;
        network.insert(start, speed);
        let mut queue = VecDeque::from([start]);
        while let Some(pos) = queue.pop_front() {
            let part = parts[&pos];
            let here = network[&pos];
            for diff in reach(part) {
                let next = add(pos, diff);
                let Some(&other) = parts.get(&next) else {
                    continue;
                };
                let Some(factor) = link(part, other, diff) else {
                    continue;
                };
                let want = here * factor;
                match network.get(&next) {
                    Some(&have) => {
                        if (have - want).abs() > 1e-3 * have.abs().max(1.0) {
                            bad = true;
                        }
                    }
                    None => {
                        if want.abs() > MAX_SPEED + 1e-3 {
                            bad = true;
                        }
                        // Another source in the network turns at its own speed.
                        if let Some(&own) = sources.get(&next)
                            && (own - want).abs() > 1e-3 * own.abs().max(1.0)
                        {
                            bad = true;
                        }
                        network.insert(next, want);
                        queue.push_back(next);
                    }
                }
            }
        }
        done.extend(network.keys().copied());
        if bad {
            jammed.extend(network.keys().copied());
        } else {
            speeds.extend(network.into_iter().filter(|(_, s)| *s != 0.0));
        }
    }
    Solved { speeds, jammed }
}

/// A part's angle in degrees at `ticks` (game time with the partial tick),
/// as Create turns them: 0.3 degrees a tick for each RPM, cogwheels offset
/// by half a tooth on alternate positions so neighbours' teeth interleave.
pub fn angle(part: Part, pos: [i32; 3], speed: f32, ticks: f64) -> f32 {
    let base = (ticks * f64::from(speed) * 0.3).rem_euclid(360.0) as f32;
    let offset = match part {
        Part::Cog(axis) | Part::LargeCog(axis) => {
            let sum: i32 = [Axis::X, Axis::Y, Axis::Z]
                .into_iter()
                .filter(|a| *a != axis)
                .map(|a| a.pick(pos))
                .sum();
            let large = matches!(part, Part::LargeCog(_));
            match (sum.rem_euclid(2) == 0, large) {
                (true, _) => 22.5,
                (false, true) => 11.25,
                (false, false) => 0.0,
            }
        }
        _ => 0.0,
    };
    base + offset
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts(list: &[([i32; 3], Part)]) -> HashMap<[i32; 3], Part> {
        list.iter().copied().collect()
    }

    #[test]
    fn a_shaft_line_turns_with_its_motor() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::East)),
            ([1, 0, 0], Part::Shaft(Axis::X)),
            ([2, 0, 0], Part::Shaft(Axis::X)),
            // Across the line, not joined.
            ([3, 0, 0], Part::Shaft(Axis::Z)),
        ]);
        let s = solve(&p, &HashMap::from([([0, 0, 0], 32.0)]));
        assert_eq!(s.speeds.get(&[2, 0, 0]), Some(&32.0));
        assert_eq!(s.speeds.get(&[3, 0, 0]), None);
        assert!(s.jammed.is_empty());
    }

    #[test]
    fn cogwheels_reverse_and_a_large_one_halves() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::Up)),
            ([0, 1, 0], Part::Cog(Axis::Y)),
            ([1, 1, 0], Part::Cog(Axis::Y)),
            // Diagonal to the second small one, in the same plane.
            ([2, 1, 1], Part::LargeCog(Axis::Y)),
        ]);
        let s = solve(&p, &HashMap::from([([0, 0, 0], 32.0)]));
        assert_eq!(s.speeds[&[0, 1, 0]], 32.0);
        assert_eq!(s.speeds[&[1, 1, 0]], -32.0);
        assert_eq!(s.speeds[&[2, 1, 1]], 16.0);
    }

    #[test]
    fn a_gearbox_turns_the_drive_round_a_corner() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::East)),
            ([1, 0, 0], Part::Gearbox(Axis::Y)),
            ([2, 0, 0], Part::Shaft(Axis::X)),
            ([1, 0, 1], Part::Shaft(Axis::Z)),
            ([1, 0, -1], Part::Shaft(Axis::Z)),
        ]);
        let s = solve(&p, &HashMap::from([([0, 0, 0], 32.0)]));
        // Straight through: reversed. Round the corners: each side its way.
        assert_eq!(s.speeds[&[2, 0, 0]], -32.0);
        assert_eq!(s.speeds[&[1, 0, 1]].abs(), 32.0);
        assert_eq!(s.speeds[&[1, 0, -1]], -s.speeds[&[1, 0, 1]]);
    }

    #[test]
    fn a_loop_of_three_cogs_jams() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::Up)),
            ([0, 1, 0], Part::Cog(Axis::Y)),
            ([1, 1, 0], Part::Cog(Axis::Y)),
            ([1, 1, 1], Part::Cog(Axis::Y)),
            ([0, 1, 1], Part::Cog(Axis::Y)),
        ]);
        // Four in a square mesh evenly: they turn.
        let s = solve(&p, &HashMap::from([([0, 0, 0], 16.0)]));
        assert!(s.jammed.is_empty());
        // Two motors at odds jam the network.
        let mut p2 = p.clone();
        p2.insert([0, 2, 1], Part::Motor(Dir::Down));
        let s = solve(&p2, &HashMap::from([([0, 0, 0], 16.0), ([0, 2, 1], 16.0)]));
        assert!(s.jammed.contains(&[1, 1, 0]));
        assert!(s.speeds.is_empty());
    }

    #[test]
    fn too_fast_jams() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::Up)),
            ([0, 1, 0], Part::LargeCog(Axis::Y)),
            ([1, 1, 1], Part::Cog(Axis::Y)),
        ]);
        let s = solve(&p, &HashMap::from([([0, 0, 0], 200.0)]));
        assert!(s.jammed.contains(&[0, 1, 0]));
    }

    #[test]
    fn neighbouring_cogs_are_half_a_tooth_apart() {
        let a = angle(Part::Cog(Axis::Y), [0, 0, 0], 0.0, 0.0);
        let b = angle(Part::Cog(Axis::Y), [1, 0, 0], 0.0, 0.0);
        assert_eq!((a - b).abs(), 22.5);
    }
}
