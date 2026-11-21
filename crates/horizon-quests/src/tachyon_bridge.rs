//! Thread-safe bridge from Tachyon's kinetic/structure engine into Horizon Quests.
//!
//! `tachyon_engine::chronicles` polls nearby Kinetica RPM and Lost Realms structures
//! once per second only when the active quest pack uses `kinetic_speed` or `structure`
//! tasks (`needs_poll()`), pushing per-player snapshots here; `hq_server_glue::server_tick`
//! drains them into `QuestEvent::KineticSpeed` and `QuestEvent::Structure`.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

/// One player's kinetic speed and structure snapshot reported from Tachyon's server tick.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlayerReport {
    pub uuid: u128,
    pub max_rpm: u32,
    pub structures: Vec<String>,
}

static ACTIVE: AtomicBool = AtomicBool::new(false);
static WANTED_STRUCTURES: Mutex<Vec<String>> = Mutex::new(Vec::new());
static REPORTS: Mutex<Vec<PlayerReport>> = Mutex::new(Vec::new());

/// Whether the active quest pack contains any `kinetic_speed` or `structure` task.
pub fn needs_poll() -> bool {
    ACTIVE.load(Ordering::Relaxed)
}

/// Structure resource IDs requested by the active quest pack (`TaskKind::Structure`).
pub fn wanted_structures() -> Vec<String> {
    WANTED_STRUCTURES
        .lock()
        .unwrap_or_else(|p| p.into_inner())
        .clone()
}

/// Called by `hq_server_glue` when a quest pack is opened, reloaded, or closed.
pub(crate) fn set_active(active: bool, structures: Vec<String>) {
    *WANTED_STRUCTURES
        .lock()
        .unwrap_or_else(|p| p.into_inner()) = structures;
    ACTIVE.store(active, Ordering::Relaxed);
    if !active {
        REPORTS
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clear();
    }
}

/// Pushes a player's observed maximum nearby machine speed (RPM) and enclosing structures.
pub fn report_player(uuid: u128, max_rpm: u32, structures: Vec<String>) {
    if max_rpm == 0 && structures.is_empty() {
        return;
    }
    let mut guard = REPORTS.lock().unwrap_or_else(|p| p.into_inner());
    if guard.len() < 256 {
        guard.push(PlayerReport {
            uuid,
            max_rpm,
            structures,
        });
    }
}

/// Drains all queued player reports for the current server tick.
pub(crate) fn drain() -> Vec<PlayerReport> {
    std::mem::take(&mut *REPORTS.lock().unwrap_or_else(|p| p.into_inner()))
}
