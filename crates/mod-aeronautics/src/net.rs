//! Binary wire codec (`tachyon_aeronautics:net`) and block role/color helpers.

use aeronautics_physics::{
    BlockCell, BlockRole, Facing, PilotInput, Pos, Quat, RigidContraption, StepTelemetry, TireKind,
    Vec3, catalog,
};

pub const PAYLOAD_ID: &str = "tachyon_aeronautics:net";

pub const ACTION_CONTROL: u8 = 0;
pub const ACTION_DISASSEMBLE: u8 = 1;
pub const ACTION_TOGGLE_PILOT: u8 = 2;
pub const ACTION_BURNER_UP: u8 = 3;
pub const ACTION_BURNER_DOWN: u8 = 4;

/// Compact block descriptor sent in vessel snapshots for 6-DOF client rendering & solid deck collision.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderBlock {
    pub local: Pos,
    pub role_code: u8,
    pub facing: Facing,
    pub rgb: u32,
    pub tire: Option<TireKind>,
}

/// Client -> Server pilot input & action frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PilotPacket {
    pub vessel_id: u64,
    pub input: PilotInput,
    pub action: u8,
}

/// Server -> Client authoritative 6-DOF vessel snapshot.
#[derive(Clone, Debug, PartialEq)]
pub struct VesselSnapshot {
    pub id: u64,
    pub dimension: String,
    pub position: Vec3,
    pub velocity: Vec3,
    pub orientation: Quat,
    pub angular_velocity: Vec3,
    pub center_of_mass: Vec3,
    pub engine_rpm: f32,
    pub burner_heat: u8,
    pub pilot_uuid: Option<u128>,
    pub telemetry: StepTelemetry,
    pub blocks: Vec<RenderBlock>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ServerMsg {
    Snapshot(VesselSnapshot),
    Remove(u64),
    SetLocalPilot(u64),
}

pub fn encode_role(role: BlockRole) -> u8 {
    match role {
        BlockRole::Hull => 0,
        BlockRole::Envelope => 1,
        BlockRole::Burner => 2,
        BlockRole::Levitite => 3,
        BlockRole::Propeller => 4,
        BlockRole::GyroPropeller => 5,
        BlockRole::SteamVent => 6,
        BlockRole::Sail => 7,
        BlockRole::Engine => 8,
        BlockRole::Helm => 9,
        BlockRole::WheelMount => 10,
        BlockRole::Assembler => 11,
        BlockRole::Sensor => 12,
    }
}

pub fn role_from_code(code: u8) -> BlockRole {
    match code {
        1 => BlockRole::Envelope,
        2 => BlockRole::Burner,
        3 => BlockRole::Levitite,
        4 => BlockRole::Propeller,
        5 => BlockRole::GyroPropeller,
        6 => BlockRole::SteamVent,
        7 => BlockRole::Sail,
        8 => BlockRole::Engine,
        9 => BlockRole::Helm,
        10 => BlockRole::WheelMount,
        11 => BlockRole::Assembler,
        12 => BlockRole::Sensor,
        _ => BlockRole::Hull,
    }
}

fn facing_to_byte(f: Facing) -> u8 {
    match f {
        Facing::North => 0,
        Facing::South => 1,
        Facing::West => 2,
        Facing::East => 3,
        Facing::Down => 4,
        Facing::Up => 5,
    }
}

fn facing_from_byte(b: u8) -> Facing {
    match b {
        1 => Facing::South,
        2 => Facing::West,
        3 => Facing::East,
        4 => Facing::Down,
        5 => Facing::Up,
        _ => Facing::North,
    }
}

fn tire_to_byte(t: Option<TireKind>) -> u8 {
    match t {
        None => 0,
        Some(TireKind::Small) => 1,
        Some(TireKind::Standard) => 2,
        Some(TireKind::Large) => 3,
        Some(TireKind::Monstrous) => 4,
    }
}

fn tire_from_byte(b: u8) -> Option<TireKind> {
    match b {
        1 => Some(TireKind::Small),
        2 => Some(TireKind::Standard),
        3 => Some(TireKind::Large),
        4 => Some(TireKind::Monstrous),
        _ => None,
    }
}

fn dye_color(id: &str) -> Option<u32> {
    if id.contains("white_") { Some(0xEE_ECE8) }
    else if id.contains("orange_") { Some(0xD8_7F33) }
    else if id.contains("magenta_") { Some(0xB2_4CD8) }
    else if id.contains("light_blue_") { Some(0x66_99D8) }
    else if id.contains("yellow_") { Some(0xE5_E533) }
    else if id.contains("lime_") { Some(0x7F_CC19) }
    else if id.contains("pink_") { Some(0xF2_7FA5) }
    else if id.contains("light_gray_") { Some(0x99_9999) }
    else if id.contains("gray_") { Some(0x4C_4C4C) }
    else if id.contains("cyan_") { Some(0x4C_7F99) }
    else if id.contains("purple_") { Some(0x7F_3FB2) }
    else if id.contains("blue_") { Some(0x33_4CB2) }
    else if id.contains("brown_") { Some(0x66_4C33) }
    else if id.contains("green_") { Some(0x66_7F33) }
    else if id.contains("red_") { Some(0x99_3333) }
    else if id.contains("black_") { Some(0x28_282C) }
    else { None }
}

/// Assigns a crisp 24-bit RGB color for 6-DOF block rendering.
pub fn block_rgb(id: &str, role: BlockRole) -> u32 {
    if let Some(c) = dye_color(id) {
        return c;
    }
    match role {
        BlockRole::Envelope => 0xE6_DFD0,
        BlockRole::Burner => 0xE8_7A28,
        BlockRole::Levitite => 0x8F_5CE6,
        BlockRole::Propeller | BlockRole::GyroPropeller => 0xC9_9A4B,
        BlockRole::SteamVent => 0xB8_8446,
        BlockRole::Sail => 0xF0_E6CC,
        BlockRole::Engine => 0x7A_5238,
        BlockRole::Helm => 0xD8_A840,
        BlockRole::WheelMount => 0x3D_4048,
        BlockRole::Assembler => 0x52_88A8,
        BlockRole::Sensor => 0x9E_3A36,
        BlockRole::Hull => {
            if id.contains("spruce") || id.contains("dark_oak") {
                0x6E_4F2E
            } else if id.contains("oak") || id.contains("wood") || id.contains("plank") {
                0xA6_8352
            } else if id.contains("brass") {
                0xC8_9B42
            } else if id.contains("copper") {
                0xB8_6846
            } else if id.contains("andesite") || id.contains("stone") {
                0x7D_8085
            } else if id.contains("glass") || id.contains("window") {
                0x9E_CDE0
            } else {
                0x8C_9098
            }
        }
    }
}

pub fn render_blocks_from(cells: &[BlockCell]) -> Vec<RenderBlock> {
    cells
        .iter()
        .map(|cell| {
            let role = catalog::classify(&cell.id).role;
            RenderBlock {
                local: cell.local,
                role_code: encode_role(role),
                facing: cell.facing,
                rgb: block_rgb(&cell.id, role),
                tire: cell.tire,
            }
        })
        .collect()
}

pub fn snapshot_from(
    vessel: &RigidContraption,
    dimension: &str,
    pilot_uuid: Option<u128>,
    telemetry: StepTelemetry,
) -> VesselSnapshot {
    VesselSnapshot {
        id: vessel.id,
        dimension: dimension.to_owned(),
        position: vessel.position,
        velocity: vessel.velocity,
        orientation: vessel.orientation,
        angular_velocity: vessel.angular_velocity,
        center_of_mass: vessel.mass.center_of_mass,
        engine_rpm: vessel.engine_rpm,
        burner_heat: vessel.burner_heat,
        pilot_uuid,
        telemetry,
        blocks: render_blocks_from(&vessel.blocks),
    }
}

impl PilotPacket {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(30);
        out.extend_from_slice(&self.vessel_id.to_le_bytes());
        out.extend_from_slice(&self.input.throttle.to_le_bytes());
        out.extend_from_slice(&self.input.pitch.to_le_bytes());
        out.extend_from_slice(&self.input.yaw.to_le_bytes());
        out.extend_from_slice(&self.input.roll.to_le_bytes());
        out.extend_from_slice(&self.input.vertical.to_le_bytes());
        out.push(u8::from(self.input.brake));
        out.push(self.action);
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 30 {
            return None;
        }
        let vessel_id = u64::from_le_bytes(data[0..8].try_into().ok()?);
        let f = |i: usize| -> Option<f32> {
            Some(f32::from_le_bytes(data[i..i + 4].try_into().ok()?))
        };
        let input = PilotInput {
            throttle: f(8)?,
            pitch: f(12)?,
            yaw: f(16)?,
            roll: f(20)?,
            vertical: f(24)?,
            brake: data[28] != 0,
        }
        .clamped();
        Some(Self {
            vessel_id,
            input,
            action: data[29],
        })
    }
}

fn push_f64(out: &mut Vec<u8>, v: f64) {
    out.extend_from_slice(&v.to_le_bytes());
}

fn push_vec3(out: &mut Vec<u8>, v: Vec3) {
    push_f64(out, v[0]);
    push_f64(out, v[1]);
    push_f64(out, v[2]);
}

struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let head = self.0.get(..n)?;
        self.0 = &self.0[n..];
        Some(head)
    }
    fn u8(&mut self) -> Option<u8> {
        Some(self.take(1)?[0])
    }
    fn i32(&mut self) -> Option<i32> {
        Some(i32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn u128(&mut self) -> Option<u128> {
        Some(u128::from_le_bytes(self.take(16)?.try_into().ok()?))
    }
    fn f32(&mut self) -> Option<f32> {
        Some(f32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }
    fn f64(&mut self) -> Option<f64> {
        Some(f64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }
    fn vec3(&mut self) -> Option<Vec3> {
        Some([self.f64()?, self.f64()?, self.f64()?])
    }
}

impl ServerMsg {
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        match self {
            ServerMsg::Remove(id) => {
                out.push(0);
                out.extend_from_slice(&id.to_le_bytes());
            }
            ServerMsg::SetLocalPilot(id) => {
                out.push(2);
                out.extend_from_slice(&id.to_le_bytes());
            }
            ServerMsg::Snapshot(s) => {
                out.push(1);
                out.extend_from_slice(&s.id.to_le_bytes());
                let dim = s.dimension.as_bytes();
                let dim_len = (dim.len().min(255)) as u8;
                out.push(dim_len);
                out.extend_from_slice(&dim[..dim_len as usize]);
                push_vec3(&mut out, s.position);
                push_vec3(&mut out, s.velocity);
                for q in s.orientation {
                    push_f64(&mut out, q);
                }
                push_vec3(&mut out, s.angular_velocity);
                push_vec3(&mut out, s.center_of_mass);
                out.extend_from_slice(&s.engine_rpm.to_le_bytes());
                out.push(s.burner_heat);
                match s.pilot_uuid {
                    Some(u) => {
                        out.push(1);
                        out.extend_from_slice(&u.to_le_bytes());
                    }
                    None => out.push(0),
                }
                push_f64(&mut out, s.telemetry.altitude);
                push_f64(&mut out, s.telemetry.speed_mps);
                push_f64(&mut out, s.telemetry.vertical_speed);
                push_f64(&mut out, s.telemetry.lift_ratio);
                out.extend_from_slice(&s.telemetry.throttle.to_le_bytes());
                push_f64(&mut out, s.telemetry.yaw_deg);
                push_f64(&mut out, s.telemetry.pitch_deg);
                push_f64(&mut out, s.telemetry.roll_deg);
                out.push(u8::from(s.telemetry.grounded));

                let count = s.blocks.len().min(4096) as u32;
                out.extend_from_slice(&count.to_le_bytes());
                for b in s.blocks.iter().take(count as usize) {
                    for c in b.local {
                        out.extend_from_slice(&c.to_le_bytes());
                    }
                    out.push(b.role_code);
                    out.push(facing_to_byte(b.facing));
                    out.extend_from_slice(&b.rgb.to_le_bytes());
                    out.push(tire_to_byte(b.tire));
                }
            }
        }
        out
    }

    pub fn decode(data: &[u8]) -> Option<Self> {
        let mut r = Reader(data);
        match r.u8()? {
            0 => Some(ServerMsg::Remove(r.u64()?)),
            2 => Some(ServerMsg::SetLocalPilot(r.u64()?)),
            1 => {
                let id = r.u64()?;
                let dim_len = usize::from(r.u8()?);
                let dimension = std::str::from_utf8(r.take(dim_len)?).ok()?.to_owned();
                let position = r.vec3()?;
                let velocity = r.vec3()?;
                let orientation = [r.f64()?, r.f64()?, r.f64()?, r.f64()?];
                let angular_velocity = r.vec3()?;
                let center_of_mass = r.vec3()?;
                let engine_rpm = r.f32()?;
                let burner_heat = r.u8()?;
                let pilot_uuid = if r.u8()? != 0 { Some(r.u128()?) } else { None };
                let telemetry = StepTelemetry {
                    altitude: r.f64()?,
                    speed_mps: r.f64()?,
                    vertical_speed: r.f64()?,
                    lift_ratio: r.f64()?,
                    throttle: r.f32()?,
                    yaw_deg: r.f64()?,
                    pitch_deg: r.f64()?,
                    roll_deg: r.f64()?,
                    grounded: r.u8()? != 0,
                };
                let count = (r.u32()? as usize).min(4096);
                let mut blocks = Vec::with_capacity(count);
                for _ in 0..count {
                    let local = [r.i32()?, r.i32()?, r.i32()?];
                    let role_code = r.u8()?;
                    let facing = facing_from_byte(r.u8()?);
                    let rgb = r.u32()?;
                    let tire = tire_from_byte(r.u8()?);
                    blocks.push(RenderBlock {
                        local,
                        role_code,
                        facing,
                        rgb,
                        tire,
                    });
                }
                Some(ServerMsg::Snapshot(VesselSnapshot {
                    id,
                    dimension,
                    position,
                    velocity,
                    orientation,
                    angular_velocity,
                    center_of_mass,
                    engine_rpm,
                    burner_heat,
                    pilot_uuid,
                    telemetry,
                    blocks,
                }))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pilot_packet_roundtrip() {
        let pkt = PilotPacket {
            vessel_id: 42,
            input: PilotInput {
                throttle: 0.75,
                pitch: -0.25,
                yaw: 0.5,
                roll: 0.0,
                vertical: 1.0,
                brake: true,
            },
            action: ACTION_CONTROL,
        };
        let decoded = PilotPacket::decode(&pkt.encode()).expect("decode PilotPacket");
        assert_eq!(pkt, decoded);
    }

    #[test]
    fn server_snapshot_roundtrip() {
        let snap = VesselSnapshot {
            id: 7,
            dimension: "minecraft:overworld".into(),
            position: [12.5, 72.25, -4.0],
            velocity: [0.2, 0.5, -1.3],
            orientation: [1.0, 0.0, 0.0, 0.0],
            angular_velocity: [0.0, 0.1, 0.0],
            center_of_mass: [0.5, 0.5, 0.5],
            engine_rpm: 96.0,
            burner_heat: 10,
            pilot_uuid: Some(0x1234_5678_9ABC_DEF0_1111_2222_3333_4444),
            telemetry: StepTelemetry {
                altitude: 72.75,
                speed_mps: 1.4,
                vertical_speed: 0.5,
                lift_ratio: 1.12,
                throttle: 0.8,
                yaw_deg: 15.0,
                pitch_deg: 2.0,
                roll_deg: -1.0,
                grounded: false,
            },
            blocks: vec![RenderBlock {
                local: [0, 0, 0],
                role_code: encode_role(BlockRole::Helm),
                facing: Facing::North,
                rgb: 0xD8_A840,
                tire: Some(TireKind::Standard),
            }],
        };
        let decoded = ServerMsg::decode(&ServerMsg::Snapshot(snap.clone()).encode())
            .expect("decode ServerMsg");
        assert_eq!(ServerMsg::Snapshot(snap), decoded);
    }
}
