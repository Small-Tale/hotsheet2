//! Leaves the standby snapshot for the macOS screen saver (HS2-MZM988,
//! `docs/34-glance-surfaces.md` §34.12).
//!
//! The saver runs in the system's sandboxed screen saver engine, which gives it no server secret and
//! no network, so the server mirrors what clients already receive (`GET /live-state`) into one JSON
//! file in the engine's container and the saver watches it. This writes only where a snapshot
//! directory already exists, which `hotsheet-cli screensaver install` (or the saver's first run)
//! creates, so a machine that never installed the saver gets no file and no work. It adds no timer:
//! the notification router calls it when a `live_state` event passes and on its existing minute tick,
//! which keeps `valid_until_ms` ahead of the clock while the server runs.

use hotsheet_ticketing::screensaver::{self, Locations, WriteReport};

use crate::AppState;
use crate::live_state;

/// Write the snapshot for this process's saver locations. Never fails the caller: a saver that
/// cannot be fed must not disturb the server.
pub(crate) fn write(state: &AppState) {
    let Some(locations) = screensaver::locations_from_env() else {
        return;
    };
    write_to(state, &locations, unix_ms());
}

/// [`write`] into explicit `locations` at `now_ms` (tests).
pub fn write_to(state: &AppState, locations: &Locations, now_ms: i64) -> WriteReport {
    if locations.active_snapshot_dirs().is_empty() {
        return WriteReport::default();
    }
    let list = live_state::current_all(state);
    let text = match screensaver::snapshot_json(&list, now_ms) {
        Ok(text) => text,
        Err(error) => {
            tracing::warn!(%error, "could not serialize the screen saver snapshot");
            return WriteReport::default();
        }
    };
    let report = screensaver::write_snapshots(locations, &text);
    for (dir, error) in &report.failed {
        tracing::warn!(dir = %dir.display(), %error, "could not write the screen saver snapshot");
    }
    report
}

fn unix_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |elapsed| elapsed.as_millis()),
    )
    .unwrap_or(i64::MAX)
}
