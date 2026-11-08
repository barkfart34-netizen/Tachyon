//! Kinetica's rotation: which parts turn which, and how fast.
//!
//! Every part turns about an axis. Its speed is in turns per minute (RPM)
//! about the axis's positive direction, by the right-hand rule: shafts pass
//! it on unchanged; meshing cogwheels reverse it (a large one drives a small
//! one twice as fast); a gearbox turns it through a right angle. A clutch
//! or a gearshift is two half shafts: powered by redstone, a clutch parts
//! them and a gearshift turns one against the other. The rules follow
//! Create's (MIT); the code is Tachyon's. Cogwheel links follow ICogWheel
//! and RotationPropagator, including EncasedCogwheelBlock's closed shafts.
//! A rotation speed controller sets the speed across its link to the large
//! cogwheel above it ([`super::speed_controller`]), without becoming a motor.
//! Encased chain drives beside each other across their axes turn alike
//! (ChainDriveBlock); an adjustable chain gearshift's redstone signal
//! speeds up what it passes along the chain (ChainGearshiftBlockEntity), as
//! Kinetics for Horizon ports them (sakurartro, MIT).
//!
//! [`solve`] gives every half of every part its speed from the sources it
//! is joined to. A network whose parts disagree (two motors at odds, a loop
//! of gears that can't turn) or that would turn too fast stays still: it is
//! jammed.
//!
//! Machines load a network: each takes stress for every RPM it turns at
//! ([`Part::impact`]), and its sources can take so much ([`capacity`], in
//! stress units). A network loaded past its capacity is overstressed and
//! stays still too.
use super::speed_controller;
use std::collections::{HashMap, HashSet, VecDeque};

/// The fastest a part may turn.
pub const MAX_SPEED: f32 = 256.0;
/// A creative motor's capacity for each RPM it turns at (stress units), as
/// Create's.
pub const CREATIVE_CAPACITY: f32 = 16384.0;

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
    /// The axis's two directions, negative first.
    pub fn dirs(self) -> [Dir; 2] {
        match self {
            Axis::X => [Dir::West, Dir::East],
            Axis::Y => [Dir::Down, Dir::Up],
            Axis::Z => [Dir::North, Dir::South],
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

/// Create's `ChuteBlock.Shape`, carried by the stair template's shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChuteShape {
    Normal,
    Window,
    Intersection,
    Encased,
}

impl ChuteShape {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "normal" | "straight" => Some(Self::Normal),
            "window" | "inner_left" => Some(Self::Window),
            "intersection" | "inner_right" => Some(Self::Intersection),
            "encased" | "outer_left" | "outer_right" => Some(Self::Encased),
            _ => None,
        }
    }
    pub fn property(self) -> &'static str {
        match self {
            Self::Normal => "straight",
            Self::Window => "inner_left",
            Self::Intersection => "inner_right",
            Self::Encased => "outer_left",
        }
    }
}

/// A part, as its block and state make it; trays and chutes don't turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Part {
    /// A shaft along its axis.
    Shaft(Axis),
    /// Create's speedometer, as GaugeBlock and DirectionalAxisKineticBlock:
    /// a through shaft with no stress impact (AllBlocks).
    SpeedGauge(Axis),
    /// Create's stressometer: the same through shaft, with no stress impact
    /// (DirectionalAxisKineticBlock and AllBlocks).
    StressGauge(Axis),
    /// A small cogwheel on a shaft along its axis.
    Cog(Axis),
    /// A large cogwheel on a shaft along its axis.
    LargeCog(Axis),
    /// Create's small encased cogwheel, as EncasedCogwheelBlock's default:
    /// both shaft ends closed, its teeth still mesh.
    EncasedCog(Axis),
    /// Create's large encased cogwheel, as EncasedCogwheelBlock's default:
    /// both shaft ends closed, its teeth still mesh.
    EncasedLargeCog(Axis),
    /// A gearbox: shafts out of its four sides around its axis.
    Gearbox(Axis),
    /// A motor turning its shaft out of one side.
    Motor(Dir),
    /// Create's SpeedControllerBlock: a horizontal shaft under a large
    /// cogwheel on the other horizontal axis, driven at a saved target RPM.
    SpeedController(Axis),
    /// A hand crank turning the shaft it sits on, out of this side.
    Crank(Dir),
    /// Two half shafts along the axis, parted while powered.
    Clutch(Axis),
    /// Two half shafts along the axis, one turning against the other while
    /// powered.
    Gearshift(Axis),
    /// A water wheel on a shaft along its axis, turned by the water flowing
    /// past it.
    WaterWheel(Axis),
    /// Create's large water wheel, as LargeWaterWheelBlock: its shaft along
    /// the axis, its rim two blocks from the center.
    LargeWaterWheel(Axis),
    /// Create's encased chain drive: a shaft along its axis, and a chain to
    /// the drives beside it across the axis.
    ChainDrive(Axis),
    /// Create's flywheel (FlywheelBlock): a wheel on a shaft along its axis,
    /// with no stress of its own.
    Flywheel(Axis),
    /// Create's mechanical saw: its blade's facing and, facing up or down,
    /// whether its shaft runs along x (DirectionalAxisKineticBlock).
    Saw { facing: Dir, along_first: bool },
    /// Create's mechanical drill (DrillBlock, a DirectionalKineticBlock): its head's facing, its
    /// shaft out of the back.
    Drill(Dir),
    /// Create's deployer (DeployerBlock, a DirectionalAxisKineticBlock): its hand's facing
    /// and whether its shaft runs along the first other axis (`deployer::shaft_axis`).
    Deployer { facing: Dir, along_first: bool },
    /// Create's fluid pipe: the sides it is open to (`fluids::bit`).
    Pipe(u8),
    /// Create's mechanical pump: pushing out of its facing, turned as a small cogwheel along it.
    Pump(Dir),
    /// Create's fluid valve (DirectionalAxisKineticBlock): a pipe along the axis neither its
    /// facing nor its shaft's, open while its shaft turns (`fluids.rs`).
    Valve { facing: Dir, along_first: bool },
    /// Create's smart fluid pipe: a straight pipe along its axis (`fluids.rs`).
    SmartPipe(Axis),
    /// Create's hose pulley, facing: its shaft out of its clockwise side, its pipe out of the
    /// other, its hose down (`hose.rs`).
    HosePulley(Dir),
    /// Create's item vault, along its axis (`vault.rs`).
    Vault(Axis),
    /// Create's analog lever, powered latch and toggle latch, facing (`redstone.rs`).
    AnalogLever,
    Latch(Dir),
    ToggleLatch(Dir),
    /// Create's brass diodes, facing (`redstone.rs`).
    Diode(Dir, super::redstone::Pulse),
    /// Create's steam engine, pointing away from its boiler's tank (`steam.rs`).
    SteamEngine(Dir),
    /// Create's powered shaft: a shaft a steam engine turns, a source.
    PoweredShaft(Axis),
    /// Create's fluid tank, spout and item drain (`fluids.rs`).
    Tank,
    Spout,
    Drain,
    /// Create's cuckoo clock: a shaft out of its back (the side given), and
    /// whether it is the mysterious one.
    Clock(Dir, bool),
    /// Create's adjustable chain gearshift: a chain drive whose redstone
    /// signal changes the speed it passes along the chain.
    ChainGearshift(Axis),
    /// Sails on a shaft along its axis, turned by the wind under open sky.
    Windmill(Axis),
    /// A millstone: turned by a shaft from below, or by small cogwheels
    /// beside it (it meshes as one, about Y).
    Millstone,
    /// A mechanical press, turned by a shaft along its axis (not from below:
    /// its head comes down there, on the depot under it).
    Press(Axis),
    /// A depot: a tray for one stack, which doesn't turn.
    Depot,
    /// A mechanical mixer, turned by a shaft from above; its whisk mixes in
    /// the basin under it.
    Mixer,
    /// Create's mechanical arm (ArmBlock): a small cogwheel about Y, turned
    /// by a shaft from below.
    Arm,
    /// A basin: the items to mix, which doesn't turn.
    Basin,
    /// Create's blaze burner under a basin, which doesn't turn
    /// (tachyon_engine/burner.rs, BlazeBurnerBlock).
    Burner,
    /// A lit empty cage: passive heat, not a fueled blaze
    /// (LitBlazeBurnerBlock).
    LitBurner,
    /// An encased fan, on a shaft along its axis; it blows out of one end
    /// (tachyon_engine/fan.rs).
    Fan(Axis),
    /// A belt segment carrying along its axis (level), on pulleys turned by
    /// a shaft beside it; the segments in a line turn together
    /// (tachyon_engine/belt.rs).
    Belt(Axis),
    /// A funnel on a container's face, facing out of it, which doesn't turn.
    Funnel(Dir),
    /// Create's crushing wheel on a shaft along its axis: two a block apart
    /// turning against each other crush what falls between them
    /// (tachyon_engine/crusher.rs).
    CrushingWheel(Axis),
    /// Create's chute, facing down or toward its diagonal input, holding
    /// one stack (tachyon_engine/chute.rs).
    Chute(Dir, ChuteShape),
}

impl Part {
    /// Whether the part has a shaft out of side `d`.
    pub fn shaft_toward(self, d: Dir) -> bool {
        match self {
            Part::Shaft(a)
            | Part::PoweredShaft(a)
            | Part::SpeedGauge(a)
            | Part::StressGauge(a)
            | Part::Cog(a)
            | Part::LargeCog(a)
            | Part::Clutch(a)
            | Part::Gearshift(a)
            | Part::WaterWheel(a)
            | Part::LargeWaterWheel(a)
            | Part::Windmill(a)
            | Part::Fan(a)
            | Part::ChainDrive(a)
            | Part::ChainGearshift(a)
            | Part::Flywheel(a)
            | Part::CrushingWheel(a) => d.axis() == a,
            Part::Gearbox(a) => d.axis() != a,
            Part::Motor(out) | Part::Crank(out) | Part::Clock(out, _) => d == out,
            Part::Millstone | Part::Arm => d == Dir::Down,
            Part::Press(a) => d.axis() == a && d != Dir::Down,
            Part::Mixer => d == Dir::Up,
            Part::Saw { facing, along_first } => {
                if facing.axis() == Axis::Y {
                    d.axis() == if along_first { Axis::X } else { Axis::Z }
                } else {
                    d == facing.opposite()
                }
            }
            Part::SpeedController(a) => a != Axis::Y && d.axis() == a,
            Part::Drill(facing) => d == facing.opposite(),
            Part::Deployer { facing, along_first } | Part::Valve { facing, along_first } => {
                d.axis() == super::deployer::shaft_axis(facing, along_first)
            }
            Part::HosePulley(facing) => d == super::hose::clockwise(facing),
            // Its pulleys' shafts, out of its sides.
            Part::Belt(a) => d.axis() != a && d.axis() != Axis::Y,
            Part::EncasedCog(_)
            | Part::EncasedLargeCog(_)
            | Part::Depot
            | Part::Basin
            | Part::Funnel(_)
            | Part::Burner
            | Part::LitBurner
            | Part::Chute(_, _)
            | Part::Pipe(_)
            | Part::Pump(_)
            | Part::Tank
            | Part::Spout
            | Part::Drain
            | Part::SmartPipe(_)
            | Part::Vault(_)
            | Part::AnalogLever
            | Part::Latch(_)
            | Part::ToggleLatch(_)
            | Part::Diode(_, _)
            | Part::SteamEngine(_) => false,
        }
    }
    /// The stress the part takes for each RPM it turns at (stress units):
    /// a machine's; nothing for what only passes the turning on.
    pub fn impact(self) -> f32 {
        match self {
            // AllBlocks: CStress.setImpact(4.0) for both.
            Part::Millstone | Part::Saw { .. } | Part::Drill(_) | Part::Deployer { .. } => 4.0,
            // AllBlocks: CStress.setImpact(4.0) for the pump.
            Part::Pump(_) => 4.0,
            // AllBlocks: CStress.setImpact(4.0) for the hose pulley.
            Part::HosePulley(_) => 4.0,
            Part::Press(_) => 8.0,
            Part::Mixer => 4.0,
            Part::Arm => 2.0,
            Part::Fan(_) => 2.0,
            Part::CrushingWheel(_) => 8.0,
            // AllBlocks: CStress.setImpact(1.0).
            Part::Clock(_, _) => 1.0,
            // AllBlocks registers CStress.setNoImpact for the controller.
            Part::SpeedController(_) => 0.0,
            _ => 0.0,
        }
    }
    /// The axis of a chain drive or an adjustable chain gearshift.
    pub fn chain(self) -> Option<Axis> {
        match self {
            Part::ChainDrive(a) | Part::ChainGearshift(a) => Some(a),
            _ => None,
        }
    }
    /// Whether the part is two halves (a clutch, a gearshift).
    pub fn split(self) -> bool {
        matches!(self, Part::Clutch(_) | Part::Gearshift(_))
    }
    /// The half of the part on side `d`: of a split part, 0 on the
    /// negative side and 1 on the positive; of any other, 0.
    pub fn half(self, d: Dir) -> usize {
        usize::from(self.split() && d.positive())
    }
    /// The part's cogwheel: its axis and whether it is large.
    fn cog(self) -> Option<(Axis, bool)> {
        match self {
            Part::Cog(a) | Part::EncasedCog(a) => Some((a, false)),
            Part::LargeCog(a) | Part::EncasedLargeCog(a) => Some((a, true)),
            // A mixer meshes as one too (Create's).
            Part::Millstone | Part::Mixer | Part::Arm => Some((Axis::Y, false)),
            // PumpBlock is an ICogWheel along its facing.
            Part::Pump(facing) => Some((facing.axis(), false)),
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

/// One half of a part: its position and half ([`Part::half`]).
pub type Node = ([i32; 3], usize);

/// What a part at `to` turns at for each turn of the part at `from`, `diff`
/// being `to - from`, through their shafts or their cogwheels, and the
/// halves that touch; None if they don't. Speeds are the halves' own (for
/// a gearbox, that of its reference side).
pub fn link(from: Part, to: Part, diff: [i32; 3]) -> Option<(usize, usize, f32)> {
    let manhattan = diff.iter().map(|v| v.abs()).sum::<i32>();
    if manhattan == 1 {
        let d = Dir::ALL.into_iter().find(|d| d.offset() == diff)?;
        let back = d.opposite();
        // Shaft to shaft: the same speed at the touching sides.
        if from.shaft_toward(d) && to.shaft_toward(back) {
            let factor = from.side_factor(d) / to.side_factor(back);
            return Some((from.half(d), to.half(back), factor));
        }
        // Chain drives side by side across their shared axis: the same
        // speed (an adjustable gearshift's signal is applied by `solve`).
        if let (Some(a), Some(b)) = (from.chain(), to.chain())
            && a == b
            && a.pick(diff) == 0
        {
            return Some((0, 0, 1.0));
        }
        // Belt segments in a line, along it: the same speed.
        if let (Part::Belt(a), Part::Belt(b)) = (from, to)
            && a == b
            && a.pick(diff) != 0
        {
            return Some((0, 0, 1.0));
        }
        // Small cogwheels side by side on parallel axes mesh, reversed.
        if let (Some((a, false)), Some((b, false))) = (from.cog(), to.cog())
            && a == b
            && a.pick(diff) == 0
        {
            return Some((0, 0, -1.0));
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
        return Some((0, 0, if a_large { -2.0 } else { -0.5 }));
    }
    // Two large cogwheels at right angles, diagonally.
    if a_large && b_large && a.pick(diff) != 0 && b.pick(diff) != 0 {
        let factor = if (a.pick(diff) > 0) ^ (b.pick(diff) > 0) {
            -1.0
        } else {
            1.0
        };
        return Some((0, 0, factor));
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

/// Each half's speed, by position: both the same but for a split part.
pub type Speeds = HashMap<[i32; 3], [f32; 2]>;

/// What a source can take turning at `speed` (its own RPM), in stress
/// units: a creative motor any load; the others more the faster they turn.
/// Water wheel capacities follow Create's AllBlocks defaults (CStress).
pub fn capacity(part: Part, speed: f32) -> f32 {
    match part {
        Part::Motor(_) => CREATIVE_CAPACITY * speed.abs(),
        Part::Crank(_) => 8.0 * speed.abs(),
        Part::WaterWheel(_) => 32.0 * speed.abs(),
        Part::LargeWaterWheel(_) => 128.0 * speed.abs(),
        Part::Windmill(_) => 32.0 * speed.abs(),
        Part::PoweredShaft(_) => super::steam::capacity_per_rpm(speed) * speed.abs(),
        _ => 0.0,
    }
}

/// A network's stress: what its machines take and what its sources can
/// (stress units).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Stress {
    pub load: f32,
    pub capacity: f32,
}

impl Stress {
    /// Loaded past its capacity: the network stands still.
    pub fn over(self) -> bool {
        self.load > self.capacity + 1e-3
    }
}

/// The speeds of the parts joined to a source ([`Speeds`]: a gearbox's is
/// that of its reference side; [`Part::side_factor`] gives the others).
/// Parts not joined to a source, and the parts of a jammed or an
/// overstressed network, are left out (they stand still); `jammed` gets the
/// jammed ones, `stress` every part's network's stress but a jammed one's.
pub struct Solved {
    pub speeds: Speeds,
    pub jammed: HashSet<[i32; 3]>,
    pub stress: HashMap<[i32; 3], Stress>,
}

/// `ChainGearshiftBlockEntity.getModifierForSignal`: what an adjustable
/// chain gearshift with redstone `signal` multiplies along its chain.
pub fn chain_factor(signal: u8) -> f32 {
    if signal == 0 {
        1.0
    } else {
        1.0 + (f32::from(signal) + 1.0) / 16.0
    }
}

/// Solves the speeds of `parts`, with `sources` (positions of motors and
/// cranks among them) turning at their own speeds, and the clutches and
/// gearshifts at `powered` powered. A controller's target is at `targets`,
/// or SpeedControllerBlockEntity.DEFAULT_SPEED when absent.
pub fn solve(
    parts: &HashMap<[i32; 3], Part>,
    sources: &HashMap<[i32; 3], f32>,
    powered: &HashSet<[i32; 3]>,
    targets: &HashMap<[i32; 3], i32>,
) -> Solved {
    solve_with(parts, sources, powered, targets, &HashMap::new())
}

/// [`solve`], with the adjustable chain gearshifts' redstone `signals`.
pub fn solve_with(
    parts: &HashMap<[i32; 3], Part>,
    sources: &HashMap<[i32; 3], f32>,
    powered: &HashSet<[i32; 3]>,
    targets: &HashMap<[i32; 3], i32>,
    signals: &HashMap<[i32; 3], u8>,
) -> Solved {
    // RotationPropagator: a gearshift multiplies what leaves it along the
    // chain and divides what comes in.
    let chained = |at: [i32; 3], part: Part| match part {
        Part::ChainGearshift(_) => chain_factor(signals.get(&at).copied().unwrap_or(0)),
        _ => 1.0,
    };
    let mut halves: HashMap<Node, f32> = HashMap::new();
    let mut jammed: HashSet<[i32; 3]> = HashSet::new();
    let mut stress: HashMap<[i32; 3], Stress> = HashMap::new();
    let mut done: HashSet<Node> = HashSet::new();
    let mut order: Vec<_> = sources.iter().collect();
    order.sort_by_key(|(p, _)| **p);
    for (&start, &speed) in order {
        if done.contains(&(start, 0)) || !parts.contains_key(&start) {
            continue;
        }
        // One network: every half reached from this source.
        let mut network: HashMap<Node, f32> = HashMap::new();
        // The first neighbour feeding each controller: its shaft or wheel.
        let mut controller_sources: HashMap<[i32; 3], [i32; 3]> = HashMap::new();
        let mut bad = speed.abs() > MAX_SPEED;
        network.insert((start, 0), speed);
        let mut queue = VecDeque::from([(start, 0)]);
        let reached = |next: Node, want: f32, network: &mut HashMap<Node, f32>, bad: &mut bool| {
            match network.get(&next) {
                Some(&have) => {
                    if (have - want).abs() > 1e-3 * have.abs().max(1.0) {
                        *bad = true;
                    }
                    false
                }
                None => {
                    if want.abs() > MAX_SPEED + 1e-3 {
                        *bad = true;
                    }
                    // Another source in the network turns at its own speed.
                    if let Some(&own) = sources.get(&next.0)
                        && (own - want).abs() > 1e-3 * own.abs().max(1.0)
                    {
                        *bad = true;
                    }
                    network.insert(next, want);
                    true
                }
            }
        };
        while let Some((pos, half)) = queue.pop_front() {
            let part = parts[&pos];
            let here = network[&(pos, half)];
            // The other half of a clutch or a gearshift.
            if part.split() {
                let on = powered.contains(&pos);
                let inside = match part {
                    Part::Clutch(_) => (!on).then_some(1.0),
                    _ => Some(if on { -1.0 } else { 1.0 }),
                };
                if let Some(factor) = inside
                    && reached((pos, 1 - half), here * factor, &mut network, &mut bad)
                {
                    queue.push_back((pos, 1 - half));
                }
            }
            for diff in reach(part) {
                let next = add(pos, diff);
                let Some(&other) = parts.get(&next) else {
                    continue;
                };
                let (from, to, want) = if let Some(targeting) = speed_controller::connection(part, other, diff) {
                    let (controller, wheel) = if targeting { (next, pos) } else { (pos, next) };
                    let target = targets.get(&controller).copied()
                        .unwrap_or(speed_controller::DEFAULT_SPEED);
                    // A target does not generate rotation. At zero the two
                    // sides are disconnected, as Create's stopped output.
                    if here == 0.0 || target == 0 {
                        continue;
                    }
                    let shaft_speed = network.get(&(controller, 0)).copied().unwrap_or(0.0);
                    let wheel_speed = network.get(&(wheel, 0)).copied().unwrap_or(0.0);
                    let drive = match controller_sources.get(&controller) {
                        None => speed_controller::Drive::None,
                        Some(p) if *p == wheel => speed_controller::Drive::Wheel,
                        Some(_) => speed_controller::Drive::Shaft,
                    };
                    (0, 0, speed_controller::conveyed(
                        target, shaft_speed, wheel_speed, drive, targeting,
                    ))
                } else {
                    let Some((from, to, mut factor)) = link(part, other, diff) else {
                        continue;
                    };
                    if let (Some(a), Some(_)) = (part.chain(), other.chain())
                        && a.pick(diff) == 0
                    {
                        factor *= chained(pos, part) / chained(next, other);
                    }
                    (from, to, here * factor)
                };
                if from != half {
                    continue;
                }
                if reached((next, to), want, &mut network, &mut bad) {
                    if matches!(other, Part::SpeedController(_)) {
                        controller_sources.insert(next, pos);
                    }
                    queue.push_back((next, to));
                }
            }
        }
        done.extend(network.keys().copied());
        if bad {
            jammed.extend(network.keys().map(|n| n.0));
            continue;
        }
        // The machines' load against what the sources in it can take.
        let mut s = Stress::default();
        for (&(pos, _), &speed) in &network {
            let part = parts[&pos];
            s.load += part.impact() * speed.abs();
            if let Some(&own) = sources.get(&pos) {
                s.capacity += capacity(part, own);
            }
        }
        stress.extend(network.keys().map(|n| (n.0, s)));
        if !s.over() {
            halves.extend(network.into_iter().filter(|(_, s)| *s != 0.0));
        }
    }
    let mut speeds: Speeds = HashMap::new();
    for ((pos, half), speed) in halves {
        let split = parts.get(&pos).is_some_and(|p| p.split());
        let entry = speeds.entry(pos).or_insert([0.0; 2]);
        if split {
            entry[half] = speed;
        } else {
            *entry = [speed; 2];
        }
    }
    Solved {
        speeds,
        jammed,
        stress,
    }
}

/// A part's angle in degrees at `ticks` (game time with the partial tick),
/// as Create turns them: 0.3 degrees a tick for each RPM, cogwheels offset
/// by half a tooth on alternate positions so neighbours' teeth interleave.
/// The tooth offsets follow Create's KineticBlockEntityVisual.rotationOffset.
pub fn angle(part: Part, pos: [i32; 3], speed: f32, ticks: f64) -> f32 {
    let base = (ticks * f64::from(speed) * 0.3).rem_euclid(360.0) as f32;
    // ArmRenderer's cog has the same tooth offset as a small cogwheel.
    let part = if part == Part::Arm { Part::Cog(Axis::Y) } else { part };
    let offset = match part {
        Part::Cog(axis)
        | Part::LargeCog(axis)
        | Part::EncasedCog(axis)
        | Part::EncasedLargeCog(axis) => {
            let sum: i32 = [Axis::X, Axis::Y, Axis::Z]
                .into_iter()
                .filter(|a| *a != axis)
                .map(|a| a.pick(pos))
                .sum();
            let large = matches!(part, Part::LargeCog(_) | Part::EncasedLargeCog(_));
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

    fn solve1(p: &HashMap<[i32; 3], Part>, sources: &[([i32; 3], f32)]) -> Solved {
        solve(p, &sources.iter().copied().collect(), &HashSet::new(), &HashMap::new())
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
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds.get(&[2, 0, 0]), Some(&[32.0; 2]));
        assert_eq!(s.speeds.get(&[3, 0, 0]), None);
        assert!(s.jammed.is_empty());
    }

    #[test]
    fn gauges_pass_rotation_from_both_ends_without_meshing() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let beside = Dir::ALL.into_iter().find(|d| d.axis() != axis).unwrap();
            for gauge in [Part::SpeedGauge(axis), Part::StressGauge(axis)] {
                assert_eq!(gauge.impact(), 0.0);
                assert!(!gauge.split());
                assert_eq!(gauge.cog(), None);
                for side in Dir::ALL {
                    assert_eq!(gauge.shaft_toward(side), side.axis() == axis);
                    assert_eq!(gauge.half(side), 0);
                }
                assert_eq!(link(gauge, Part::Cog(axis), beside.offset()), None);
                for speed in [-32.0, 0.0, 32.0] {
                    assert_eq!(capacity(gauge, speed), 0.0);
                }
                for end in axis.dirs() {
                    assert_eq!(link(gauge, Part::Shaft(beside.axis()), end.offset()), None);
                    let p = parts(&[
                        (end.offset(), Part::Motor(end.opposite())),
                        ([0, 0, 0], gauge),
                        (end.opposite().offset(), Part::Shaft(axis)),
                        (beside.offset(), Part::Shaft(beside.axis())),
                    ]);
                    assert!(solve1(&p, &[]).speeds.is_empty());
                    for speed in [-32.0, 32.0] {
                        let s = solve1(&p, &[(end.offset(), speed)]);
                        assert_eq!(s.speeds.get(&[0, 0, 0]), Some(&[speed; 2]));
                        assert_eq!(s.speeds.get(&end.opposite().offset()), Some(&[speed; 2]));
                        assert_eq!(s.speeds.get(&beside.offset()), None);
                        assert!(s.jammed.is_empty());
                    }
                }
            }
        }
    }

    #[test]
    fn gauges_do_not_change_a_networks_stress() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let [neg, pos] = axis.dirs();
            for speed in [-32.0, 32.0] {
                let mut p = parts(&[
                    (neg.offset(), Part::Motor(pos)),
                    ([0, 0, 0], Part::Shaft(axis)),
                    (pos.offset(), Part::Fan(axis)),
                ]);
                let shaft = solve1(&p, &[(neg.offset(), speed)]);
                assert_eq!(shaft.stress[&[0, 0, 0]].load, 64.0);
                for gauge in [Part::SpeedGauge(axis), Part::StressGauge(axis)] {
                    p.insert([0, 0, 0], gauge);
                    let s = solve1(&p, &[(neg.offset(), speed)]);
                    assert_eq!(s.stress, shaft.stress);
                    assert_eq!(s.speeds, shaft.speeds);
                }
            }
        }
    }

    #[test]
    fn a_water_wheel_drives_the_shafts_along_its_axis() {
        let p = parts(&[
            ([0, 0, 0], Part::WaterWheel(Axis::X)),
            ([1, 0, 0], Part::Shaft(Axis::X)),
            ([-1, 0, 0], Part::Shaft(Axis::X)),
            // Beside it, under its paddles: not joined.
            ([0, 0, 1], Part::Shaft(Axis::Z)),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 16.0)]);
        assert_eq!(s.speeds.get(&[1, 0, 0]), Some(&[16.0; 2]));
        assert_eq!(s.speeds.get(&[-1, 0, 0]), Some(&[16.0; 2]));
        assert_eq!(s.speeds.get(&[0, 0, 1]), None);
    }

    #[test]
    fn a_large_water_wheel_drives_both_ends_of_its_axis() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            let [neg, pos] = axis.dirs();
            let beside = Dir::ALL.into_iter().find(|d| d.axis() != axis).unwrap();
            let p = parts(&[
                ([0, 0, 0], Part::LargeWaterWheel(axis)),
                (neg.offset(), Part::Shaft(axis)),
                (pos.offset(), Part::Shaft(axis)),
                (beside.offset(), Part::Shaft(beside.axis())),
            ]);
            for speed in [-4.0, 4.0] {
                let s = solve1(&p, &[([0, 0, 0], speed)]);
                assert_eq!(s.speeds.get(&neg.offset()), Some(&[speed; 2]));
                assert_eq!(s.speeds.get(&pos.offset()), Some(&[speed; 2]));
                assert_eq!(s.speeds.get(&beside.offset()), None);
                assert_eq!(s.stress[&[0, 0, 0]].capacity, 512.0);
                assert!(s.jammed.is_empty());
            }
        }
    }

    #[test]
    fn water_wheels_capacity_is_for_each_rpm() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for (part, rate) in [
                (Part::WaterWheel(axis), 32.0),
                (Part::LargeWaterWheel(axis), 128.0),
            ] {
                assert_eq!(capacity(part, 0.0), 0.0);
                assert_eq!(capacity(part, 1.0), rate);
                assert_eq!(capacity(part, 4.0), rate * 4.0);
                assert_eq!(capacity(part, -4.0), rate * 4.0);
                assert_eq!(part.impact(), 0.0);
            }
        }
        assert_eq!(capacity(Part::WaterWheel(Axis::X), 8.0), 256.0);
        assert_eq!(capacity(Part::LargeWaterWheel(Axis::X), 4.0), 512.0);
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
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[0, 1, 0]][0], 32.0);
        assert_eq!(s.speeds[&[1, 1, 0]][0], -32.0);
        assert_eq!(s.speeds[&[2, 1, 1]][0], 16.0);
    }

    #[test]
    fn encased_cogwheels_reverse_and_a_large_one_halves() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::Up)),
            ([0, 1, 0], Part::Cog(Axis::Y)),
            ([1, 1, 0], Part::EncasedCog(Axis::Y)),
            // Diagonal to the encased small one, in the same plane.
            ([2, 1, 1], Part::EncasedLargeCog(Axis::Y)),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[0, 1, 0]], [32.0; 2]);
        assert_eq!(s.speeds[&[1, 1, 0]], [-32.0; 2]);
        assert_eq!(s.speeds[&[2, 1, 1]], [16.0; 2]);
        assert!(s.jammed.is_empty());
    }

    #[test]
    fn encased_cogs_use_the_existing_mesh_ratios() {
        for (axis, side, diagonal) in [
            (Axis::X, [0, 1, 0], [0, 1, 1]),
            (Axis::Y, [1, 0, 0], [1, 0, 1]),
            (Axis::Z, [1, 0, 0], [1, 1, 0]),
        ] {
            for (from, to) in [
                (Part::EncasedCog(axis), Part::EncasedCog(axis)),
                (Part::EncasedCog(axis), Part::Cog(axis)),
                (Part::Cog(axis), Part::EncasedCog(axis)),
            ] {
                assert_eq!(link(from, to, side), Some((0, 0, -1.0)));
                assert_eq!(link(from, to, diagonal), None);
                assert_eq!(link(from, to, side.map(|v| v * 2)), None);
                assert_eq!(link(from, to, axis.dirs()[1].offset()), None);
            }
            for (large, small) in [
                (Part::EncasedLargeCog(axis), Part::EncasedCog(axis)),
                (Part::EncasedLargeCog(axis), Part::Cog(axis)),
                (Part::LargeCog(axis), Part::EncasedCog(axis)),
            ] {
                assert_eq!(link(large, small, diagonal), Some((0, 0, -2.0)));
                assert_eq!(link(small, large, diagonal.map(|v| -v)), Some((0, 0, -0.5)));
                assert_eq!(link(large, small, side), None);
                assert_eq!(link(large, small, diagonal.map(|v| v * 2)), None);
                assert_eq!(link(large, small, axis.dirs()[1].offset()), None);
            }
        }
        assert_eq!(link(Part::EncasedCog(Axis::Y), Part::Cog(Axis::Z), [1, 0, 0]), None);
        assert_eq!(
            link(Part::EncasedLargeCog(Axis::Y), Part::LargeCog(Axis::Y), [1, 0, 1]),
            None
        );
    }

    #[test]
    fn encased_large_cogs_mesh_at_right_angles() {
        for (from, to) in [
            (Part::EncasedLargeCog(Axis::X), Part::EncasedLargeCog(Axis::Z)),
            (Part::EncasedLargeCog(Axis::X), Part::LargeCog(Axis::Z)),
            (Part::LargeCog(Axis::X), Part::EncasedLargeCog(Axis::Z)),
        ] {
            for (offset, factor) in [([1, 0, 1], 1.0), ([1, 0, -1], -1.0)] {
                assert_eq!(link(from, to, offset), Some((0, 0, factor)));
                assert_eq!(link(to, from, offset.map(|v| -v)), Some((0, 0, factor)));
            }
            assert_eq!(link(from, to, [1, 1, 0]), None);
            assert_eq!(link(from, to, [1, 1, 1]), None);
            assert_eq!(link(from, to, [0, 0, 0]), None);
        }
    }

    #[test]
    fn encased_cogwheels_have_closed_shafts_and_no_stress() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for part in [Part::EncasedCog(axis), Part::EncasedLargeCog(axis)] {
                for d in Dir::ALL {
                    assert!(!part.shaft_toward(d));
                    let shaft = Part::Shaft(d.axis());
                    assert_eq!(link(part, shaft, d.offset()), None);
                    assert_eq!(link(shaft, part, d.opposite().offset()), None);
                }
                assert_eq!(part.impact(), 0.0);
                for speed in [-32.0, 0.0, 32.0] {
                    assert_eq!(capacity(part, speed), 0.0);
                }
                assert!(!part.split());
            }
        }
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
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        // Straight through: reversed. Round the corners: each side its way.
        assert_eq!(s.speeds[&[2, 0, 0]][0], -32.0);
        assert_eq!(s.speeds[&[1, 0, 1]][0].abs(), 32.0);
        assert_eq!(s.speeds[&[1, 0, -1]][0], -s.speeds[&[1, 0, 1]][0]);
    }

    #[test]
    fn a_square_of_cogs_turns_and_motors_at_odds_jam() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::Up)),
            ([0, 1, 0], Part::Cog(Axis::Y)),
            ([1, 1, 0], Part::Cog(Axis::Y)),
            ([1, 1, 1], Part::Cog(Axis::Y)),
            ([0, 1, 1], Part::Cog(Axis::Y)),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 16.0)]);
        assert!(s.jammed.is_empty());
        let mut p2 = p.clone();
        p2.insert([0, 2, 1], Part::Motor(Dir::Down));
        let s = solve1(&p2, &[([0, 0, 0], 16.0), ([0, 2, 1], 16.0)]);
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
        let s = solve1(&p, &[([0, 0, 0], 200.0)]);
        assert!(s.jammed.contains(&[0, 1, 0]));
    }

    #[test]
    fn a_saw_takes_its_shaft_behind_a_sideways_blade_or_across_an_upward_one() {
        let side = Part::Saw { facing: Dir::East, along_first: false };
        assert!(side.shaft_toward(Dir::West) && !side.shaft_toward(Dir::East) && !side.shaft_toward(Dir::Up));
        let up = Part::Saw { facing: Dir::Up, along_first: true };
        assert!(up.shaft_toward(Dir::East) && up.shaft_toward(Dir::West) && !up.shaft_toward(Dir::North));
        assert_eq!(up.impact(), 4.0);
        let line = parts(&[([0, 0, 0], Part::Motor(Dir::East)), ([1, 0, 0], side)]);
        let s = solve1(&line, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[1, 0, 0]][0], 32.0);
    }

    #[test]
    fn a_flywheel_passes_its_shaft_on_without_load() {
        let line = parts(&[
            ([0, 0, 0], Part::Shaft(Axis::X)),
            ([1, 0, 0], Part::Flywheel(Axis::X)),
            ([2, 0, 0], Part::Shaft(Axis::X)),
            ([1, 1, 0], Part::Shaft(Axis::Y)),
        ]);
        let s = solve1(&line, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[2, 0, 0]][0], 32.0);
        assert!(!s.speeds.contains_key(&[1, 1, 0]));
        assert_eq!(Part::Flywheel(Axis::X).impact(), 0.0);
    }

    #[test]
    fn chain_drives_turn_alike_across_their_axis_and_a_signal_speeds_them() {
        let row = parts(&[
            ([0, 0, 0], Part::Shaft(Axis::Y)),
            ([0, 1, 0], Part::ChainDrive(Axis::Y)),
            ([1, 1, 0], Part::ChainGearshift(Axis::Y)),
            ([2, 1, 0], Part::ChainDrive(Axis::Y)),
            ([2, 2, 0], Part::Shaft(Axis::Y)),
            // Along the axis is a shaft's link, not a chain's.
            ([0, 2, 0], Part::ChainDrive(Axis::X)),
        ]);
        let sources = HashMap::from([([0, 0, 0], 16.0)]);
        let plain = solve(&row, &sources, &HashSet::new(), &HashMap::new());
        assert_eq!(plain.speeds[&[2, 2, 0]][0], 16.0);
        assert!(!plain.speeds.contains_key(&[0, 2, 0]));
        let signals = HashMap::from([([1, 1, 0], 15)]);
        let fast = solve_with(&row, &sources, &HashSet::new(), &HashMap::new(), &signals);
        // Into the gearshift slower, out of it faster: the same at the ends.
        assert_eq!(fast.speeds[&[1, 1, 0]][0], 8.0);
        assert_eq!(fast.speeds[&[2, 2, 0]][0], 16.0);
        assert_eq!(chain_factor(0), 1.0);
        assert_eq!(chain_factor(15), 2.0);
        // A drive driven from the gearshift's end alone turns faster.
        let one = parts(&[
            ([0, 0, 0], Part::Shaft(Axis::Y)),
            ([0, 1, 0], Part::ChainGearshift(Axis::Y)),
            ([1, 1, 0], Part::ChainDrive(Axis::Y)),
        ]);
        let s = solve_with(&one, &sources, &HashSet::new(), &HashMap::new(), &signals_at([0, 1, 0]));
        assert_eq!(s.speeds[&[1, 1, 0]][0], 32.0);
    }

    fn signals_at(p: [i32; 3]) -> HashMap<[i32; 3], u8> {
        HashMap::from([(p, 15)])
    }

    #[test]
    fn a_powered_clutch_parts_and_a_powered_gearshift_reverses() {
        let line = |middle: Part| {
            parts(&[
                ([0, 0, 0], Part::Motor(Dir::East)),
                ([1, 0, 0], middle),
                ([2, 0, 0], Part::Shaft(Axis::X)),
            ])
        };
        let sources: HashMap<[i32; 3], f32> = HashMap::from([([0, 0, 0], 32.0)]);
        let on = HashSet::from([[1, 0, 0]]);
        let off = HashSet::new();
        let clutch = line(Part::Clutch(Axis::X));
        assert_eq!(solve(&clutch, &sources, &off, &HashMap::new()).speeds[&[2, 0, 0]][0], 32.0);
        let parted = solve(&clutch, &sources, &on, &HashMap::new());
        assert_eq!(parted.speeds.get(&[2, 0, 0]), None);
        // The motor's half turns, the other doesn't.
        assert_eq!(parted.speeds[&[1, 0, 0]], [32.0, 0.0]);
        let shift = line(Part::Gearshift(Axis::X));
        assert_eq!(solve(&shift, &sources, &off, &HashMap::new()).speeds[&[2, 0, 0]][0], 32.0);
        let reversed = solve(&shift, &sources, &on, &HashMap::new());
        assert_eq!(reversed.speeds[&[2, 0, 0]][0], -32.0);
        assert_eq!(reversed.speeds[&[1, 0, 0]], [32.0, -32.0]);
    }

    #[test]
    fn a_millstone_turns_from_below_and_beside_and_loads_its_network() {
        let p = parts(&[
            ([0, 0, 0], Part::Crank(Dir::Up)),
            ([0, 1, 0], Part::Millstone),
            // A small cogwheel beside it meshes, reversed.
            ([1, 1, 0], Part::Cog(Axis::Y)),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[0, 1, 0]][0], 32.0);
        assert_eq!(s.speeds[&[1, 1, 0]][0], -32.0);
        // 4 for each RPM against the crank's 8.
        let at = s.stress[&[1, 1, 0]];
        assert_eq!((at.load, at.capacity), (128.0, 256.0));
        assert!(!at.over());
    }

    #[test]
    fn an_overstressed_network_stands_still() {
        // A crank driving three millstones: 384 against 256.
        let p = parts(&[
            ([0, 0, 0], Part::Crank(Dir::Up)),
            ([0, 1, 0], Part::Millstone),
            ([1, 1, 0], Part::Cog(Axis::Y)),
            ([2, 1, 0], Part::Millstone),
            ([1, 1, 1], Part::Millstone),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert!(s.speeds.is_empty());
        assert!(s.jammed.is_empty());
        assert!(s.stress[&[2, 1, 0]].over());
        // A creative motor takes it.
        let mut p2 = p.clone();
        p2.insert([0, 0, 0], Part::Motor(Dir::Up));
        let s = solve1(&p2, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[2, 1, 0]][0], 32.0);
        assert_eq!(s.stress[&[0, 0, 0]].capacity, CREATIVE_CAPACITY * 32.0);
    }

    #[test]
    fn gauges_keep_an_overstressed_networks_stress_without_speed() {
        for gauge in [Part::SpeedGauge(Axis::Y), Part::StressGauge(Axis::Y)] {
            let p = parts(&[
                ([0, 0, 0], Part::Crank(Dir::Up)),
                ([0, 1, 0], gauge),
                ([0, 2, 0], Part::Millstone),
                ([1, 2, 0], Part::Cog(Axis::Y)),
                ([2, 2, 0], Part::Millstone),
                ([1, 2, 1], Part::Millstone),
            ]);
            let s = solve1(&p, &[([0, 0, 0], 32.0)]);
            assert!(s.speeds.is_empty());
            assert!(s.jammed.is_empty());
            assert_eq!(
                s.stress[&[0, 1, 0]],
                Stress { load: 384.0, capacity: 256.0 }
            );
            assert!(s.stress[&[0, 1, 0]].over());
        }
    }

    #[test]
    fn a_press_turns_along_its_axis_and_a_depot_not_at_all() {
        let p = parts(&[
            ([0, 1, 0], Part::Motor(Dir::East)),
            ([1, 1, 0], Part::Press(Axis::X)),
            ([1, 0, 0], Part::Depot),
            ([2, 1, 0], Part::Shaft(Axis::X)),
        ]);
        let s = solve1(&p, &[([0, 1, 0], 32.0)]);
        assert_eq!(s.speeds[&[1, 1, 0]][0], 32.0);
        assert_eq!(s.speeds[&[2, 1, 0]][0], 32.0);
        assert_eq!(s.speeds.get(&[1, 0, 0]), None);
        assert_eq!(s.stress[&[1, 1, 0]].load, 256.0);
        // Not from a shaft under it.
        let p = parts(&[([0, 0, 0], Part::Motor(Dir::Up)), ([0, 1, 0], Part::Press(Axis::Y))]);
        assert_eq!(solve1(&p, &[([0, 0, 0], 32.0)]).speeds.get(&[0, 1, 0]), None);
    }

    #[test]
    fn a_mixer_turns_from_above_only() {
        let p = parts(&[
            ([0, 2, 0], Part::Motor(Dir::Down)),
            ([0, 1, 0], Part::Mixer),
            ([0, 0, 0], Part::Basin),
            ([1, 1, 0], Part::Shaft(Axis::X)),
        ]);
        let s = solve1(&p, &[([0, 2, 0], 32.0)]);
        assert_eq!(s.speeds[&[0, 1, 0]][0].abs(), 32.0);
        assert_eq!(s.speeds.get(&[0, 0, 0]), None);
        assert_eq!(s.speeds.get(&[1, 1, 0]), None);
        assert_eq!(s.stress[&[0, 1, 0]].load, 128.0);
    }

    #[test]
    fn an_arm_turns_from_below_or_a_cog_beside_it() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::Up)),
            ([0, 1, 0], Part::Arm),
            ([1, 1, 0], Part::Cog(Axis::Y)),
            ([0, 2, 0], Part::Shaft(Axis::Y)),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[0, 1, 0]], [32.0; 2]);
        assert_eq!(s.speeds[&[1, 1, 0]], [-32.0; 2]);
        assert!(!s.speeds.contains_key(&[0, 2, 0]));
        assert_eq!(s.stress[&[0, 1, 0]].load, 64.0);
        assert_eq!(capacity(Part::Arm, 32.0), 0.0);
        for pos in [[0, 1, 0], [1, 1, 0], [-2, 0, 3]] {
            assert_eq!(angle(Part::Arm, pos, 32.0, 12.5), angle(Part::Cog(Axis::Y), pos, 32.0, 12.5));
        }
    }

    #[test]
    fn a_belt_turns_from_a_shaft_beside_it_and_along_its_line() {
        let p = parts(&[
            ([0, 0, 0], Part::Motor(Dir::East)),
            ([1, 0, 0], Part::Belt(Axis::Z)),
            ([1, 0, 1], Part::Belt(Axis::Z)),
            ([1, 0, 2], Part::Belt(Axis::Z)),
            // Across the line's end: not joined.
            ([1, 0, 3], Part::Belt(Axis::X)),
            // Nor a funnel.
            ([1, 1, 2], Part::Funnel(Dir::Up)),
        ]);
        let s = solve1(&p, &[([0, 0, 0], 32.0)]);
        assert_eq!(s.speeds[&[1, 0, 0]][0], 32.0);
        assert_eq!(s.speeds[&[1, 0, 2]][0], 32.0);
        assert_eq!(s.speeds.get(&[1, 0, 3]), None);
        assert_eq!(s.speeds.get(&[1, 1, 2]), None);
        // Not from a shaft at its end.
        let p = parts(&[([0, 0, 0], Part::Motor(Dir::East)), ([1, 0, 0], Part::Belt(Axis::X))]);
        assert_eq!(solve1(&p, &[([0, 0, 0], 32.0)]).speeds.get(&[1, 0, 0]), None);
    }

    #[test]
    fn neighbouring_cogs_are_half_a_tooth_apart() {
        let a = angle(Part::Cog(Axis::Y), [0, 0, 0], 0.0, 0.0);
        let b = angle(Part::Cog(Axis::Y), [1, 0, 0], 0.0, 0.0);
        assert_eq!((a - b).abs(), 22.5);
    }

    #[test]
    fn encased_cogs_keep_the_uncased_tooth_offsets() {
        for axis in [Axis::X, Axis::Y, Axis::Z] {
            for (encased, uncased) in [
                (Part::EncasedCog(axis), Part::Cog(axis)),
                (Part::EncasedLargeCog(axis), Part::LargeCog(axis)),
            ] {
                for pos in [
                    [0, 0, 0],
                    [1, 0, 0],
                    [0, 1, 0],
                    [0, 0, 1],
                    [-1, -2, -3],
                    [-2, -1, -3],
                ] {
                    for (speed, ticks) in [(0.0, 0.0), (32.0, 17.5), (-32.0, 123.25)] {
                        assert_eq!(
                            angle(encased, pos, speed, ticks),
                            angle(uncased, pos, speed, ticks)
                        );
                    }
                }
            }
        }
    }
}
