//! Authoritative 20 TPS server simulation, assembly/disassembly, interactions, and `/aero` commands.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use aeronautics_physics::math::{Vec3, v_len, v_sub};
use aeronautics_physics::{
    BlockCell, Facing, PilotInput, Pos, RigidContraption, StepTelemetry, TireKind, WorldBlockSample,
    assemble_from_world, burner_state_for_heat,
};
use mod_api::{
    BlockMetadata, BlockState, BlockStateRecord, BlockUpdateFlags, Command as GameCommand,
    CommandInvocation, CommandSource, Flow, Hand, HostBlockStates, InteractionResult, PayloadBytes,
    PayloadId, PlayerRef, RightClickBlockEvent, RightClickItemEvent, ServerHost, ServerTickPhase,
    Text,
};

use crate::net::{
    ACTION_BURNER_DOWN, ACTION_BURNER_UP, ACTION_DISASSEMBLE, ACTION_TOGGLE_PILOT, PAYLOAD_ID,
    PilotPacket, ServerMsg, snapshot_from,
};

const USAGES: [(&str, u8); 5] = [
    ("aero", 0),
    ("aero assemble", 0),
    ("aero land", 0),
    ("aero pilot", 0),
    ("aero status", 0),
];

struct ActiveVessel {
    dimension: String,
    body: RigidContraption,
    telemetry: StepTelemetry,
    pilot: Option<u128>,
    last_input: PilotInput,
}

#[derive(Default)]
struct State {
    next_id: u64,
    vessels: HashMap<u64, ActiveVessel>,
    pilots: HashMap<u128, u64>,
    mounted_tires: HashMap<(String, Pos), TireKind>,
    burner_heat: HashMap<(String, Pos), u8>,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(Mutex::default);

fn lock() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn clear() {
    let mut s = lock();
    s.vessels.clear();
    s.pilots.clear();
    s.mounted_tires.clear();
    s.burner_heat.clear();
}

pub(crate) fn player_logged_in(host: &mut dyn ServerHost, player_uuid: u128) {
    let snaps: Vec<_> = {
        let s = lock();
        s.vessels
            .values()
            .map(|v| snapshot_from(&v.body, &v.dimension, v.pilot, v.telemetry))
            .collect()
    };
    for snap in snaps {
        send_to_player(host, player_uuid, &ServerMsg::Snapshot(snap));
    }
}

pub(crate) fn player_logged_out(host: &mut dyn ServerHost, player_uuid: u128) {
    let updated = {
        let mut s = lock();
        if let Some(vid) = s.pilots.remove(&player_uuid)
            && let Some(v) = s.vessels.get_mut(&vid)
            && v.pilot == Some(player_uuid)
        {
            v.pilot = None;
            v.last_input = PilotInput::default();
            Some(snapshot_from(&v.body, &v.dimension, None, v.telemetry))
        } else {
            None
        }
    };
    if let Some(snap) = updated {
        broadcast_msg(host, &ServerMsg::Snapshot(snap));
    }
}

pub(crate) fn server_stopping(host: &mut dyn ServerHost) {
    let ids: Vec<u64> = lock().vessels.keys().copied().collect();
    for id in ids {
        let _ = disassemble_vessel(host, id, None);
    }
}

fn player_dim(host: &dyn ServerHost, player: PlayerRef) -> String {
    player
        .dimension(host)
        .map_or_else(|| "minecraft:overworld".to_owned(), |id| id.to_string())
}

fn with_prop(host: &dyn ServerHost, state: BlockState, key: &str, value: &str) -> Option<BlockState> {
    host.block_states()
        .with(BlockStateRecord::from(&state), key, value)
        .ok()
        .map(Into::into)
}

fn tell(host: &mut dyn ServerHost, uuid: u128, msg: &str) {
    if let Some(player) = host.player(uuid) {
        let _ = player.tell(host, Text::literal(format!("[Create Aeronautics] {msg}")));
    }
}

fn send_to_player(host: &mut dyn ServerHost, uuid: u128, msg: &ServerMsg) {
    if let Some(player) = host.player(uuid) {
        let channel = PayloadId(PAYLOAD_ID.to_owned());
        let _ = player.send_payload(host, &channel, PayloadBytes(msg.encode()));
    }
}

fn parse_state_props(state_text: &str) -> (String, Facing, bool, String) {
    let mut parts = state_text.splitn(2, '[');
    let id = parts.next().unwrap_or("").to_owned();
    let mut facing = Facing::North;
    let mut powered = false;
    let mut variant = String::new();
    if let Some(props) = parts.next().and_then(|p| p.strip_suffix(']')) {
        for kv in props.split(',') {
            if let Some((k, v)) = kv.split_once('=') {
                if k == "facing"
                    && let Some(f) = Facing::from_name(v)
                {
                    facing = f;
                } else if k == "axis" {
                    facing = match v {
                        "x" => Facing::East,
                        "y" => Facing::Up,
                        _ => Facing::North,
                    };
                } else if k == "powered" {
                    powered = v == "true";
                } else if k == "variant" {
                    variant = v.to_owned();
                }
            }
        }
    }
    (id, facing, powered, variant)
}

fn broadcast_msg(host: &mut dyn ServerHost, msg: &ServerMsg) {
    let bytes = msg.encode();
    let channel = PayloadId(PAYLOAD_ID.to_owned());
    let players = host.players().server_players();
    for player in players {
        let _ = player.send_payload(host, &channel, PayloadBytes(bytes.clone()));
    }
}

fn set_world_block(
    host: &mut dyn ServerHost,
    dimension: &str,
    pos: Pos,
    id: &str,
    facing: Facing,
) {
    let Some(level) = host.level(dimension) else { return };
    let Some(mut state) = host.block_state_id(id) else { return };
    if let Some(updated) = with_prop(host, state, "facing", facing.as_str()) {
        state = updated;
    }
    let _ = level.set_block(
        host,
        pos,
        state,
        BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS,
    );
}

pub(crate) fn assemble_at(
    host: &mut dyn ServerHost,
    dimension: &str,
    origin: Pos,
    pilot_uuid: Option<u128>,
) -> Result<(u64, usize), String> {
    let Some(level) = host.level(dimension) else {
        return Err("Dimension tidak tersedia.".to_owned());
    };
    let (id, tires, heats) = {
        let mut s = lock();
        s.next_id += 1;
        (s.next_id, s.mounted_tires.clone(), s.burner_heat.clone())
    };

    let read_block = |pos: Pos| -> Option<WorldBlockSample> {
        let state = level.block_state(&*host, pos)?;
        let text = host.block_state_string(state)?;
        let (block_id, facing, powered, variant) = parse_state_props(&text);
        if block_id.is_empty() {
            return None;
        }
        let key = (dimension.to_owned(), pos);
        let tire = tires.get(&key).copied();
        let heat = heats.get(&key).copied().unwrap_or_else(|| {
            if block_id == "aeronautics:adjustable_burner" {
                if !powered {
                    0
                } else if variant == "soulful" {
                    14
                } else {
                    8
                }
            } else if block_id == "create:blaze_burner" {
                8
            } else {
                0
            }
        });
        Some(WorldBlockSample {
            id: block_id,
            facing,
            powered,
            heat,
            tire,
        })
    };

    let contraption = assemble_from_world(id, origin, 2048, read_block).map_err(|e| match e {
        aeronautics_physics::AssemblyError::Empty => "Tidak ada blok yang bisa dirakit di titik ini.".to_owned(),
        aeronautics_physics::AssemblyError::TooLarge { limit } => {
            format!("Struktur terlalu besar (> {limit} blok). Pisahkan dari tanah dulu.")
        }
        aeronautics_physics::AssemblyError::BlockedByUnmovable { id, .. } => {
            format!("Terhalang blok permanen ({id}).")
        }
    })?;

    if let Some(air) = host.block_state_id("minecraft:air") {
        let flags = BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS;
        for cell in &contraption.blocks {
            let wp = [
                origin[0] + cell.local[0],
                origin[1] + cell.local[1],
                origin[2] + cell.local[2],
            ];
            let _ = level.set_block(host, wp, air, flags);
        }
    }

    let count = contraption.blocks.len();
    let telemetry = StepTelemetry {
        altitude: contraption.position[1] + contraption.mass.center_of_mass[1],
        ..Default::default()
    };
    let snap = snapshot_from(&contraption, dimension, pilot_uuid, telemetry);

    {
        let mut s = lock();
        for cell in &contraption.blocks {
            let wp = [
                origin[0] + cell.local[0],
                origin[1] + cell.local[1],
                origin[2] + cell.local[2],
            ];
            s.mounted_tires.remove(&(dimension.to_owned(), wp));
            s.burner_heat.remove(&(dimension.to_owned(), wp));
        }
        if let Some(uuid) = pilot_uuid
            && let Some(old_id) = s.pilots.insert(uuid, id)
            && let Some(old_v) = s.vessels.get_mut(&old_id)
            && old_v.pilot == Some(uuid)
        {
            old_v.pilot = None;
        }
        s.vessels.insert(
            id,
            ActiveVessel {
                dimension: dimension.to_owned(),
                body: contraption,
                telemetry,
                pilot: pilot_uuid,
                last_input: PilotInput::default(),
            },
        );
    }

    broadcast_msg(host, &ServerMsg::Snapshot(snap));
    if let Some(uuid) = pilot_uuid {
        send_to_player(host, uuid, &ServerMsg::SetLocalPilot(id));
    }
    Ok((id, count))
}

pub(crate) fn disassemble_vessel(
    host: &mut dyn ServerHost,
    vessel_id: u64,
    actor_uuid: Option<u128>,
) -> Result<usize, String> {
    let vessel = {
        let mut s = lock();
        let Some(v) = s.vessels.remove(&vessel_id) else {
            return Err("Kendaraan tidak ditemukan.".to_owned());
        };
        if let Some(pilot) = v.pilot {
            s.pilots.remove(&pilot);
        }
        v
    };

    let snapped = vessel.body.snap_disassembly();
    let count = snapped.len();
    let mut burner_restore = Vec::new();
    let mut tire_restore = Vec::new();
    for (world_pos, cell, _steps) in &snapped {
        set_world_block(host, &vessel.dimension, *world_pos, &cell.id, cell.facing);
        if cell.id == "aeronautics:adjustable_burner" {
            if let Some(level) = host.level(&vessel.dimension)
                && let Some(mut state) = level.block_state(&*host, *world_pos)
            {
                let (powered, variant) = burner_state_for_heat(cell.heat);
                if let Some(next) = with_prop(host, state, "powered", if powered { "true" } else { "false" }) {
                    state = next;
                }
                if let Some(next) = with_prop(host, state, "variant", variant) {
                    state = next;
                }
                let _ = level.set_block(
                    host,
                    *world_pos,
                    state,
                    BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS,
                );
            }
            burner_restore.push(((vessel.dimension.clone(), *world_pos), cell.heat));
        }
        if let Some(tire) = cell.tire {
            tire_restore.push(((vessel.dimension.clone(), *world_pos), tire));
        }
    }

    {
        let mut s = lock();
        for (k, heat) in burner_restore {
            s.burner_heat.insert(k, heat);
        }
        for (k, tire) in tire_restore {
            s.mounted_tires.insert(k, tire);
        }
    }

    if let Some(pilot) = vessel.pilot {
        send_to_player(host, pilot, &ServerMsg::SetLocalPilot(0));
    }
    broadcast_msg(host, &ServerMsg::Remove(vessel_id));
    if let Some(uuid) = actor_uuid {
        tell(host, uuid, &format!("Kendaraan didaratkan ({count} blok dipasang kembali)."));
    }
    Ok(count)
}

pub(crate) fn on_client_packet(host: &mut dyn ServerHost, player_uuid: u128, data: &[u8]) {
    let Some(pkt) = PilotPacket::decode(data) else { return };
    match pkt.action {
        ACTION_DISASSEMBLE => {
            let target_id = lock().pilots.get(&player_uuid).copied().unwrap_or(pkt.vessel_id);
            let _ = disassemble_vessel(host, target_id, Some(player_uuid));
        }
        ACTION_TOGGLE_PILOT => {
            toggle_pilot_nearest(host, player_uuid);
        }
        ACTION_BURNER_UP | ACTION_BURNER_DOWN => {
            let mut snap_to_send = None;
            let mut new_heat = None;
            {
                let mut s = lock();
                if let Some(&vid) = s.pilots.get(&player_uuid)
                    && let Some(v) = s.vessels.get_mut(&vid)
                {
                    if pkt.action == ACTION_BURNER_UP {
                        v.body.burner_heat = (v.body.burner_heat + 3).min(15);
                    } else {
                        v.body.burner_heat = v.body.burner_heat.saturating_sub(3);
                    }
                    new_heat = Some(v.body.burner_heat);
                    snap_to_send = Some(snapshot_from(&v.body, &v.dimension, v.pilot, v.telemetry));
                }
            }
            if let Some(snap) = snap_to_send {
                broadcast_msg(host, &ServerMsg::Snapshot(snap));
            }
            if let Some(heat) = new_heat {
                let (_, variant) = burner_state_for_heat(heat);
                tell(host, player_uuid, &format!("Daya burner: {heat}/15 ({variant})"));
            }
        }
        _ => {
            let mut s = lock();
            if let Some(v) = s.vessels.get_mut(&pkt.vessel_id)
                && v.pilot == Some(player_uuid)
            {
                v.last_input = pkt.input.clamped();
            }
        }
    }
}

fn toggle_pilot_nearest(host: &mut dyn ServerHost, player_uuid: u128) {
    let Some(player) = host.player(player_uuid) else { return };
    let dim = player_dim(host, player);
    let Some(ppos) = player.entity().position(host) else { return };

    let mut snap_to_send = None;
    let mut pilot_vid: Option<u64> = None;
    let msg: Option<String>;

    {
        let mut s = lock();
        if let Some(cur_id) = s.pilots.remove(&player_uuid) {
            if let Some(v) = s.vessels.get_mut(&cur_id) {
                v.pilot = None;
                v.last_input = PilotInput::default();
                snap_to_send = Some(snapshot_from(&v.body, &v.dimension, None, v.telemetry));
            }
            pilot_vid = Some(0);
            msg = Some("Keluar dari kendali kemudi.".to_owned());
        } else {
            let best_id = s
                .vessels
                .values()
                .filter(|v| v.dimension == dim)
                .map(|v| (v.body.id, v_len(v_sub(ppos, v.body.position))))
                .filter(|(_, d)| *d <= 14.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(id, _)| id);
            if let Some(vid) = best_id {
                if let Some(v) = s.vessels.get_mut(&vid) {
                    v.pilot = Some(player_uuid);
                    let blocks_len = v.body.blocks.len();
                    snap_to_send = Some(snapshot_from(&v.body, &v.dimension, Some(player_uuid), v.telemetry));
                    pilot_vid = Some(vid);
                    msg = Some(format!("Mengendalikan kendaraan #{vid} ({blocks_len} blok)."));
                    s.pilots.insert(player_uuid, vid);
                } else {
                    msg = None;
                }
            } else {
                msg = Some("Tidak ada kendaraan aktif dalam jangkauan 14 blok. Klik physics_assembler atau jalankan /aero assemble.".to_owned());
            }
        }
    }

    if let Some(vid) = pilot_vid {
        send_to_player(host, player_uuid, &ServerMsg::SetLocalPilot(vid));
    }
    if let Some(snap) = snap_to_send {
        broadcast_msg(host, &ServerMsg::Snapshot(snap));
    }
    if let Some(text) = msg {
        tell(host, player_uuid, &text);
    }
}

pub(crate) fn right_click_block(
    host: &mut dyn ServerHost,
    event: &mut RightClickBlockEvent<'_>,
) -> Flow {
    if event.click.hand != Hand::Main {
        return Flow::Continue;
    }
    let pos = event.click.pos;
    let player_uuid = event.click.player;
    let Some(player) = host.player(player_uuid) else { return Flow::Continue };
    let dimension = player_dim(host, player);
    let Some(level) = host.level(&dimension) else { return Flow::Continue };
    let Some(state) = level.block_state(&*host, pos) else { return Flow::Continue };
    let Some(state_str) = host.block_state_string(state) else { return Flow::Continue };
    let (block_id, _, _, _) = parse_state_props(&state_str);
    let held_id = event
        .click
        .stack
        .map(|st| st.item().id(&*host))
        .unwrap_or_default();

    if block_id == "offroad:wheel_mount"
        && let Some(tire) = TireKind::from_item_id(&held_id)
    {
        lock()
            .mounted_tires
            .insert((dimension, pos), tire);
        tell(host, player_uuid, &format!("Memasang {held_id} pada Wheel Mount."));
        *event.cancellation_result = InteractionResult::Consume;
        return Flow::Cancel;
    }

    if block_id == "aeronautics:adjustable_burner" {
        let next_heat = {
            let mut s = lock();
            let entry = s.burner_heat.entry((dimension.clone(), pos)).or_insert(0);
            *entry = match *entry {
                0 => 5,
                1..=5 => 10,
                6..=10 => 15,
                _ => 0,
            };
            *entry
        };
        let (powered, variant) = burner_state_for_heat(next_heat);
        let mut updated_state = state;
        if let Some(next) = with_prop(host, updated_state, "powered", if powered { "true" } else { "false" }) {
            updated_state = next;
        }
        if let Some(next) = with_prop(host, updated_state, "variant", variant) {
            updated_state = next;
        }
        let _ = level.set_block(
            host,
            pos,
            updated_state,
            BlockUpdateFlags::NEIGHBORS | BlockUpdateFlags::CLIENTS,
        );
        tell(host, player_uuid, &format!("Burner level: {next_heat}/15 ({variant})."));
        *event.cancellation_result = InteractionResult::Consume;
        return Flow::Cancel;
    }

    if block_id == "simulated:physics_assembler" || held_id == "simulated:creative_physics_staff" {
        match assemble_at(host, &dimension, pos, Some(player_uuid)) {
            Ok((id, count)) => {
                tell(
                    host,
                    player_uuid,
                    &format!("Kapal #{id} dirakit ({count} blok)! W/S Throttle, A/D Kemudi, Space/Shift Naik/Turun, /aero land untuk mendarat."),
                );
            }
            Err(err) => tell(host, player_uuid, &err),
        }
        *event.cancellation_result = InteractionResult::Consume;
        return Flow::Cancel;
    }

    if block_id == "simulated:steering_wheel" {
        if lock().vessels.values().any(|v| v.dimension == dimension) {
            toggle_pilot_nearest(host, player_uuid);
        } else {
            match assemble_at(host, &dimension, pos, Some(player_uuid)) {
                Ok((id, count)) => {
                    tell(
                        host,
                        player_uuid,
                        &format!("Kendaraan #{id} dirakit dari kemudi ({count} blok)! Gunakan /aero land untuk mendarat."),
                    );
                }
                Err(err) => tell(host, player_uuid, &err),
            }
        }
        *event.cancellation_result = InteractionResult::Consume;
        return Flow::Cancel;
    }

    Flow::Continue
}

pub(crate) fn right_click_item(
    host: &mut dyn ServerHost,
    event: &mut RightClickItemEvent<'_>,
) -> Flow {
    if event.click.hand != Hand::Main {
        return Flow::Continue;
    }
    let held_id = event.click.stack.item().id(&*host);
    if held_id != "simulated:creative_physics_staff" && held_id != "create:wrench" {
        return Flow::Continue;
    }
    let player_uuid = event.click.player;
    let sneaking = host
        .player(player_uuid)
        .and_then(|p| p.is_secondary_use_active(&*host))
        .unwrap_or(false);
    if !sneaking {
        return Flow::Continue;
    }
    let active_vid = lock().pilots.get(&player_uuid).copied();
    if let Some(vid) = active_vid {
        let _ = disassemble_vessel(host, vid, Some(player_uuid));
        *event.cancellation_result = InteractionResult::Consume;
        return Flow::Cancel;
    }
    Flow::Continue
}

pub(crate) fn server_tick(host: &mut dyn ServerHost, phase: ServerTickPhase) {
    if !matches!(phase, ServerTickPhase::ServerTickEnd { .. }) {
        return;
    }

    let vessel_ids: Vec<u64> = lock().vessels.keys().copied().collect();
    if vessel_ids.is_empty() {
        return;
    }

    let mut snapshots = Vec::with_capacity(vessel_ids.len());
    let mut teleports: Vec<(u128, Vec3)> = Vec::new();

    for vid in vessel_ids {
        let (dimension, mut body, pilot, input) = {
            let mut s = lock();
            let Some(v) = s.vessels.get_mut(&vid) else { continue };
            let inp = if v.pilot.is_some() { v.last_input } else { PilotInput::default() };
            (v.dimension.clone(), v.body.clone(), v.pilot, inp)
        };

        let telemetry = if let Some(level) = host.level(&dimension) {
            body.step(0.05, input, |pos| {
                let Some(state) = level.block_state(&*host, pos) else {
                    return false;
                };
                if host.block_states().replaceable(state).unwrap_or(false) {
                    return false;
                }
                host.block_state_string(state)
                    .is_some_and(|s| !s.starts_with("minecraft:air"))
            })
        } else {
            StepTelemetry::default()
        };

        let snap = snapshot_from(&body, &dimension, pilot, telemetry);
        if let Some(uuid) = pilot {
            let helm_local = helm_local_cell(&body.blocks, body.mass.center_of_mass);
            let seat_world = body.local_to_world(helm_local);
            teleports.push((uuid, seat_world));
        }
        snapshots.push(snap);

        let mut s = lock();
        if let Some(v) = s.vessels.get_mut(&vid) {
            v.body = body;
            v.telemetry = telemetry;
        }
    }

    for (uuid, seat) in teleports {
        if let Some(player) = host.player(uuid) {
            let entity = player.entity();
            let _ = entity.teleport_to(host, seat, None);
            let _ = entity.set_velocity(host, [0.0, 0.0, 0.0]);
        }
    }

    for snap in snapshots {
        broadcast_msg(host, &ServerMsg::Snapshot(snap));
    }
}

fn helm_local_cell(blocks: &[BlockCell], fallback: Vec3) -> Vec3 {
    if let Some(b) = blocks
        .iter()
        .find(|b| b.id == "simulated:steering_wheel" || b.id == "simulated:physics_assembler")
    {
        [
            f64::from(b.local[0]) + 0.5,
            f64::from(b.local[1]) + 1.02,
            f64::from(b.local[2]) + 0.5,
        ]
    } else {
        fallback
    }
}

pub(crate) fn register_commands(commands: &mut Vec<GameCommand>) {
    for (usage, level) in USAGES {
        commands.push(if level == 0 {
            GameCommand::new(usage)
        } else {
            GameCommand::op(usage, level)
        });
    }
}

pub(crate) fn run_mod_command(
    host: &mut dyn ServerHost,
    source: &CommandSource,
    invocation: &CommandInvocation,
) -> Flow {
    if invocation.literals.first().map(String::as_str) != Some("aero") {
        return Flow::Continue;
    }
    let CommandSource::Player { player, .. } = source else {
        let _ = host
            .commands()
            .feedback(Text::literal("Jalankan /aero sebagai pemain."));
        return Flow::Cancel;
    };
    let uuid = player.uuid();
    let sub = invocation.literals.get(1).map(String::as_str).unwrap_or("help");
    let reply = match sub {
        "assemble" => {
            let dim = player_dim(host, *player);
            if let Some(pos) = player.entity().position(host) {
                let target = [
                    pos[0].floor() as i32,
                    (pos[1] - 1.0).floor() as i32,
                    pos[2].floor() as i32,
                ];
                match assemble_at(host, &dim, target, Some(uuid)) {
                    Ok((id, count)) => {
                        format!("Kapal #{id} dirakit ({count} blok)! W/S Throttle, A/D Kemudi, Space/Shift Naik/Turun.")
                    }
                    Err(e) => e,
                }
            } else {
                "Posisi pemain tidak terbaca.".to_owned()
            }
        }
        "land" => {
            let vid = lock().pilots.get(&uuid).copied().or_else(|| {
                let dim = player_dim(host, *player);
                let ppos = player.entity().position(host)?;
                lock()
                    .vessels
                    .values()
                    .filter(|v| v.dimension == dim)
                    .min_by(|a, b| {
                        v_len(v_sub(ppos, a.body.position))
                            .total_cmp(&v_len(v_sub(ppos, b.body.position)))
                    })
                    .map(|v| v.body.id)
            });
            match vid {
                Some(id) => match disassemble_vessel(host, id, Some(uuid)) {
                    Ok(c) => format!("Kendaraan #{id} mendarat ({c} blok)."),
                    Err(e) => e,
                },
                None => "Tidak ada kendaraan aktif.".to_owned(),
            }
        }
        "pilot" => {
            toggle_pilot_nearest(host, uuid);
            String::new()
        }
        "status" => {
            let s = lock();
            if let Some(vid) = s.pilots.get(&uuid).copied()
                && let Some(v) = s.vessels.get(&vid)
            {
                let t = &v.telemetry;
                format!(
                    "Kapal #{vid}: {} blok | ALT {:.1}m | SPD {:.1} m/s | LIFT {:.2}x | Grounded={}",
                    v.body.blocks.len(),
                    t.altitude,
                    t.speed_mps,
                    t.lift_ratio,
                    t.grounded
                )
            } else {
                format!("Total kendaraan aktif di server: {}", s.vessels.len())
            }
        }
        _ => "Create Aeronautics: /aero assemble | /aero pilot | /aero land | /aero status".to_owned(),
    };
    if !reply.is_empty() {
        let _ = host.commands().feedback(Text::literal(reply));
    }
    Flow::Cancel
}
