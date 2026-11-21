//! Tachyon kinetic & structure bridge into Horizon Quests (`crates/horizon-quests`).
//!
//! Chronicles' quests live in `tachyon/content/chronicles/data/chronicles/horizon_quests/`
//! and are executed and rendered by the bundled `horizon_quests` mod. This module only
//! forwards Tachyon-specific observations (nearby Kinetica machine RPM and enclosing
//! Lost Realms structures) once a second when the active quest pack uses them.

use super::super::Game;
use std::cell::Cell;
use std::time::{Duration, Instant};

/// How often the owner polls nearby Kinetica RPM and Lost Realms structures.
const LOOK: Duration = Duration::from_secs(1);
/// How near a turning machine counts for a `kinetic_speed` task (blocks).
const NEAR: f64 = 16.0;

thread_local! {
    static LAST_LOOK: Cell<Option<Instant>> = const { Cell::new(None) };
}

impl Game<'_> {
    /// Polls connected players' nearby Kinetica RPM and enclosing structures once a second
    /// when the active Horizon Quests pack contains `kinetic_speed` or `structure` tasks.
    pub(super) fn tachyon_quests_tick(&mut self) {
        if !horizon_quests::tachyon_bridge::needs_poll() {
            return;
        }
        let Some(host) = self.lan.as_ref() else {
            return;
        };
        let now = Instant::now();
        let due = LAST_LOOK.with(|last| match last.get() {
            Some(prev) if now.saturating_duration_since(prev) < LOOK => false,
            _ => {
                last.set(Some(now));
                true
            }
        });
        if !due {
            return;
        }
        let wanted = horizon_quests::tachyon_bridge::wanted_structures();
        let here = self.dim.id();
        for g in &host.hub.guests {
            let max_rpm = Self::tachyon_fastest_near(&g.dimension, g.position, NEAR).max(0.0) as u32;
            let structures = if wanted.is_empty() || g.dimension != here {
                Vec::new()
            } else {
                let at = g.position.map(|c| c.floor() as i32);
                self.structures_reaching_where(at, &|id| wanted.iter().any(|w| w == id))
                    .into_iter()
                    .map(|(id, _)| id)
                    .collect()
            };
            horizon_quests::tachyon_bridge::report_player(g.uuid, max_rpm, structures);
        }
    }
}
