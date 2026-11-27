//! Client-side registry of live vessels + local player riding & deck collision.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

use aeronautics_physics::math::{
    Quat, Vec3, quat_rotate, quat_rotate_inv, quat_slerp, v_add, v_sub,
};
use aeronautics_physics::BlockRole;

use crate::net::{VesselSnapshot, role_from_code};

/// Local player state handed over by `remote_send_state` (`remote/mod.rs`).
pub struct LocalPlayer {
    pub position: [f64; 3],
    pub velocity: [f64; 3],
    pub width: f64,
    pub height: f64,
    pub max_up_step: f32,
    pub on_ground: bool,
    pub fall_distance: f64,
    pub passenger: bool,
}

#[derive(Clone, Debug)]
pub(crate) struct ClientVessel {
    pub snapshot: VesselSnapshot,
    pub prev_position: Vec3,
    pub prev_orientation: Quat,
    pub prev_center_of_mass: Vec3,
}

static VESSELS: LazyLock<Mutex<HashMap<u64, ClientVessel>>> = LazyLock::new(Mutex::default);
static LOCAL_PILOT_VESSEL: AtomicU64 = AtomicU64::new(0);

fn vessels() -> MutexGuard<'static, HashMap<u64, ClientVessel>> {
    VESSELS.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn set_local_pilot_vessel(id: u64) {
    LOCAL_PILOT_VESSEL.store(id, Ordering::Relaxed);
}

pub(crate) fn local_pilot_vessel_id() -> Option<u64> {
    let id = LOCAL_PILOT_VESSEL.load(Ordering::Relaxed);
    (id != 0).then_some(id)
}

pub(crate) fn apply_snapshot(snap: VesselSnapshot) {
    if snap.pilot_uuid.is_none() && LOCAL_PILOT_VESSEL.load(Ordering::Relaxed) == snap.id {
        LOCAL_PILOT_VESSEL.store(0, Ordering::Relaxed);
    }

    let mut map = vessels();
    let (prev_position, prev_orientation, prev_center_of_mass) = match map.get(&snap.id) {
        Some(prev) => (
            prev.snapshot.position,
            prev.snapshot.orientation,
            prev.snapshot.center_of_mass,
        ),
        None => (snap.position, snap.orientation, snap.center_of_mass),
    };
    map.insert(
        snap.id,
        ClientVessel {
            snapshot: snap,
            prev_position,
            prev_orientation,
            prev_center_of_mass,
        },
    );
}

pub(crate) fn remove_vessel(id: u64) {
    if LOCAL_PILOT_VESSEL.load(Ordering::Relaxed) == id {
        LOCAL_PILOT_VESSEL.store(0, Ordering::Relaxed);
    }
    vessels().remove(&id);
}

pub fn clear() {
    LOCAL_PILOT_VESSEL.store(0, Ordering::Relaxed);
    vessels().clear();
}

pub(crate) fn snapshot_for_render(dimension: &str) -> Vec<ClientVessel> {
    vessels()
        .values()
        .filter(|v| v.snapshot.dimension == dimension)
        .cloned()
        .collect()
}

pub(crate) fn interpolated_pose(v: &ClientVessel, partial: f64) -> (Vec3, Quat) {
    let t = partial.clamp(0.0, 1.0);
    let pos = [
        v.prev_position[0] + (v.snapshot.position[0] - v.prev_position[0]) * t,
        v.prev_position[1] + (v.snapshot.position[1] - v.prev_position[1]) * t,
        v.prev_position[2] + (v.snapshot.position[2] - v.prev_position[2]) * t,
    ];
    let ori = quat_slerp(v.prev_orientation, v.snapshot.orientation, t);
    (pos, ori)
}

pub(crate) fn to_world(pos: Vec3, ori: Quat, com: Vec3, local: Vec3) -> Vec3 {
    let cm_world = v_add(pos, com);
    let rel = v_sub(local, com);
    v_add(cm_world, quat_rotate(ori, rel))
}

pub(crate) fn to_local(pos: Vec3, ori: Quat, com: Vec3, world: Vec3) -> Vec3 {
    let cm_world = v_add(pos, com);
    let rel = quat_rotate_inv(ori, v_sub(world, cm_world));
    v_add(rel, com)
}

pub(crate) fn helm_local_pos(snap: &VesselSnapshot) -> Vec3 {
    if let Some(helm) = snap
        .blocks
        .iter()
        .find(|b| matches!(role_from_code(b.role_code), BlockRole::Helm))
    {
        return [
            f64::from(helm.local[0]) + 0.5,
            f64::from(helm.local[1]) + 1.02,
            f64::from(helm.local[2]) + 0.5,
        ];
    }
    if let Some(asm) = snap
        .blocks
        .iter()
        .find(|b| matches!(role_from_code(b.role_code), BlockRole::Assembler))
    {
        return [
            f64::from(asm.local[0]) + 0.5,
            f64::from(asm.local[1]) + 1.02,
            f64::from(asm.local[2]) + 0.5,
        ];
    }
    snap.center_of_mass
}

/// Keeps the local player riding cleanly on the helm seat when piloting or standing on a vessel deck.
pub fn local_player_pass(player: &mut LocalPlayer) -> bool {
    if player.passenger {
        return false;
    }
    let map = vessels();
    if map.is_empty() {
        return false;
    }

    if let Some(pilot_id) = local_pilot_vessel_id()
        && let Some(v) = map.get(&pilot_id)
    {
        let seat = to_world(
            v.snapshot.position,
            v.snapshot.orientation,
            v.snapshot.center_of_mass,
            helm_local_pos(&v.snapshot),
        );
        player.position = seat;
        player.velocity = [0.0, 0.0, 0.0];
        player.on_ground = true;
        player.fall_distance = 0.0;
        return true;
    }

    // Non-pilot passengers standing on deck: resolve top-surface support & carry by tick delta.
    for v in map.values() {
        let local = to_local(
            v.snapshot.position,
            v.snapshot.orientation,
            v.snapshot.center_of_mass,
            player.position,
        );
        let half_w = (player.width * 0.45).max(0.25);
        for b in &v.snapshot.blocks {
            let bx0 = f64::from(b.local[0]) - half_w;
            let bx1 = f64::from(b.local[0] + 1) + half_w;
            let bz0 = f64::from(b.local[2]) - half_w;
            let bz1 = f64::from(b.local[2] + 1) + half_w;
            let top = f64::from(b.local[1] + 1);
            if local[0] >= bx0
                && local[0] <= bx1
                && local[2] >= bz0
                && local[2] <= bz1
                && local[1] >= top - 0.65
                && local[1] <= top + 0.25
                && player.velocity[1] <= 0.15
            {
                let old_world = to_world(
                    v.prev_position,
                    v.prev_orientation,
                    v.prev_center_of_mass,
                    [local[0], top, local[2]],
                );
                let new_world = to_world(
                    v.snapshot.position,
                    v.snapshot.orientation,
                    v.snapshot.center_of_mass,
                    [local[0], top, local[2]],
                );
                let delta = v_sub(new_world, old_world);
                player.position = [
                    player.position[0] + delta[0],
                    new_world[1],
                    player.position[2] + delta[2],
                ];
                player.velocity[1] = 0.0;
                player.on_ground = true;
                player.fall_distance = 0.0;
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::net::{RenderBlock, encode_role};
    use aeronautics_physics::{Facing, IDENTITY_QUAT, StepTelemetry};

    #[test]
    fn standing_on_vessel_deck_carries_player_with_vessel() {
        clear();
        let snap1 = VesselSnapshot {
            id: 1,
            dimension: "minecraft:overworld".into(),
            position: [0.0, 70.0, 0.0],
            velocity: [1.0, 0.5, 0.0],
            orientation: IDENTITY_QUAT,
            angular_velocity: [0.0; 3],
            center_of_mass: [0.5, 0.5, 0.5],
            engine_rpm: 64.0,
            burner_heat: 8,
            pilot_uuid: None,
            telemetry: StepTelemetry::default(),
            blocks: vec![RenderBlock {
                local: [0, 0, 0],
                role_code: encode_role(BlockRole::Hull),
                facing: Facing::North,
                rgb: 0xAA_AAAA,
                tire: None,
            }],
        };
        apply_snapshot(snap1.clone());

        let mut snap2 = snap1;
        snap2.position = [0.5, 70.25, 0.0];
        apply_snapshot(snap2);

        let mut player = LocalPlayer {
            position: [0.5, 71.2, 0.5],
            velocity: [0.0, -0.2, 0.0],
            width: 0.6,
            height: 1.8,
            max_up_step: 0.6,
            on_ground: false,
            fall_distance: 2.0,
            passenger: false,
        };
        assert!(local_player_pass(&mut player));
        assert!(player.on_ground);
        assert_eq!(player.fall_distance, 0.0);
        assert!((player.position[0] - 1.0).abs() < 1e-6);
        assert!((player.position[1] - 71.25).abs() < 1e-6);
        clear();
    }
}
