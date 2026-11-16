//! HS2-KB2Y51: pushing this machine's orchestration lines to the remote servers that
//! host its targets, so anyone who views such a target sees them.
//!
//! Part A (HS2-3D8D6V) answers a remote checkout's lines here, from this machine's
//! records. A pass recomputes them for every remote destination a reviewed binding
//! names and sends one replace to the target's server only when they changed, when
//! half their lifetime passed (a lease renewal of display facts, not a poll), or when
//! an earlier send failed and its bounded backoff elapsed: the re-assert after a
//! reconnect. A destination no longer bound is withdrawn with an empty push. The
//! lines carry display facts only (`display_line`), never run, session, permission or
//! decision identifiers. Nothing here writes this machine's records.

use std::collections::BTreeMap;

use hotsheet_ticketing::orchestration::{
    BindingRegistry, DEFAULT_OBSERVATION_TTL_MS, MAX_OBSERVATION_TTL_MS, ObservedLines,
    TargetDestination, display_line,
};
use hotsheet_ticketing::remote_servers::ReviewedRemoteIdentity;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::request::{Clock, Rng};
use super::target_lines::LinesOf;
use super::*;

/// The hosted route a push replaces.
pub const LINE_OBSERVATION_LEAF: &str = "/target-line-observations";
/// The lifetime each push asks for; a pass re-asserts at half of it.
pub const LINE_PUSH_TTL_MS: u64 = DEFAULT_OBSERVATION_TTL_MS;
/// At least this long between two pushes to one destination.
pub const LINE_PUSH_MIN_GAP_MS: u64 = 2_000;
/// The first retry after a failure; it doubles up to [`LINE_PUSH_MAX_BACKOFF_MS`].
pub const LINE_PUSH_FIRST_BACKOFF_MS: u64 = 2_000;
pub const LINE_PUSH_MAX_BACKOFF_MS: u64 = 5 * 60_000;
/// At most this many remote destinations are pushed to in one pass.
pub const MAX_LINE_PUSH_DESTINATIONS: usize = 64;

#[derive(Debug, Clone)]
struct Pushed {
    registration_id: String,
    identity: ReviewedRemoteIdentity,
    checkout_id: String,
    /// The version the target last acknowledged.
    version: Option<String>,
    lines: usize,
    pushed_at_ms: u64,
    failures: u32,
    retry_at_ms: u64,
}

/// What one machine remembers between passes. A fresh state pushes everything once.
#[derive(Debug, Clone, Default)]
pub struct LinePushState {
    destinations: BTreeMap<(String, String), Pushed>,
    sequence: u64,
}

/// What a pass did for one destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinePushOutcome {
    pub registration_id: String,
    pub checkout_id: String,
    pub lines: u32,
    /// `pushed`, `withdrawn`, `unchanged`, `waiting` (coalescing or backing off) or
    /// `failed`.
    pub outcome: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
}

/// One pass over every remote destination.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinePushReport {
    pub destinations: Vec<LinePushOutcome>,
    /// When the next pass has something to do even if nothing changes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_due_ms: Option<u64>,
}

fn backoff(failures: u32) -> u64 {
    LINE_PUSH_FIRST_BACKOFF_MS
        .saturating_mul(1 << failures.saturating_sub(1).min(16))
        .min(LINE_PUSH_MAX_BACKOFF_MS)
}

impl OrchestrationRuntime {
    fn send_lines(&self, pushed: &Pushed, body: &ObservedLines) -> Result<(), String> {
        let bytes = serde_json::to_vec(body).map_err(|_| "corrupt".to_owned())?;
        let reply = self
            .remotes
            .call_checkout(
                &pushed.registration_id,
                &pushed.identity,
                &pushed.checkout_id,
                self.secrets.as_ref(),
                self.transport.as_ref(),
                "PUT",
                LINE_OBSERVATION_LEAF,
                Some(&bytes),
            )
            .map_err(|failure| {
                serde_json::to_value(failure)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "observation_failed".into())
            })?;
        if (200..300).contains(&reply.status) {
            return Ok(());
        }
        // The peer's own code when it names one; its message text is never kept.
        let parsed: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
        Err(parsed
            .get("code")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| match reply.status {
                401 | 403 => "unauthorized".into(),
                404 | 405 => "peer_unsupported".into(),
                429 => "rate_limited".into(),
                _ => format!("status_{}", reply.status),
            }))
    }

    /// HS2-KB2Y51: one push pass. With `owner`, only the destinations that owner's
    /// bindings name (each still carries every owner's lines for it, since a push
    /// replaces this machine's whole entry).
    pub fn push_target_lines(
        &self,
        state: &mut LinePushState,
        owner: Option<&str>,
        tx: &mut Transaction<'_>,
    ) -> Result<LinePushReport, OrchestrationError> {
        if let Some(owner) = owner {
            self.owner(owner)?;
        }
        let now = tx.clock.now_ms();
        let mut current: BTreeMap<(String, String), ReviewedRemoteIdentity> = BTreeMap::new();
        for (target, destination) in BindingRegistry::new(&self.home).remote_destinations()? {
            if owner.is_some_and(|owner| owner != target.owner_checkout_id) {
                continue;
            }
            let TargetDestination::Remote {
                registration_id,
                identity,
                checkout_id,
                ..
            } = destination
            else {
                continue;
            };
            if current.len() >= MAX_LINE_PUSH_DESTINATIONS {
                break;
            }
            current
                .entry((registration_id, checkout_id))
                .or_insert(identity);
        }
        let mut report = LinePushReport {
            destinations: Vec::new(),
            next_due_ms: None,
        };
        let mut due = |at: u64| {
            report.next_due_ms = Some(report.next_due_ms.map_or(at, |next| next.min(at)));
        };
        let mut outcomes = Vec::new();
        for ((registration_id, checkout_id), identity) in &current {
            let (lines, ..) = self.target_lines_now(
                LinesOf::Remote {
                    registration_id,
                    checkout_id,
                },
                tx,
            )?;
            let key = (registration_id.clone(), checkout_id.clone());
            let entry = state.destinations.entry(key).or_insert_with(|| Pushed {
                registration_id: registration_id.clone(),
                identity: identity.clone(),
                checkout_id: checkout_id.clone(),
                version: None,
                lines: 0,
                pushed_at_ms: 0,
                failures: 0,
                retry_at_ms: 0,
            });
            // A reviewed identity change replaces the one remembered.
            entry.identity = identity.clone();
            let count = u32::try_from(lines.lines.len()).unwrap_or(u32::MAX);
            let outcome = |outcome: &str, code: Option<String>| LinePushOutcome {
                registration_id: registration_id.clone(),
                checkout_id: checkout_id.clone(),
                lines: count,
                outcome: outcome.into(),
                code,
            };
            let changed = entry.version.as_deref() != Some(lines.version.as_str());
            let renew = !lines.lines.is_empty()
                && now >= entry.pushed_at_ms.saturating_add(LINE_PUSH_TTL_MS / 2);
            if !changed && !renew && entry.failures == 0 {
                if !lines.lines.is_empty() {
                    due(entry.pushed_at_ms.saturating_add(LINE_PUSH_TTL_MS / 2));
                }
                outcomes.push(outcome("unchanged", None));
                continue;
            }
            if entry.failures > 0 && now < entry.retry_at_ms {
                due(entry.retry_at_ms);
                outcomes.push(outcome("waiting", None));
                continue;
            }
            let gap = entry.pushed_at_ms.saturating_add(LINE_PUSH_MIN_GAP_MS);
            if entry.pushed_at_ms > 0 && now < gap {
                due(gap);
                outcomes.push(outcome("waiting", None));
                continue;
            }
            state.sequence = state.sequence.saturating_add(1).max(now);
            let body = ObservedLines {
                owner_server_id: self.server_id.clone(),
                sequence: state.sequence,
                ttl_ms: Some(LINE_PUSH_TTL_MS),
                lines: lines.lines.iter().map(display_line).collect(),
            };
            match self.send_lines(entry, &body) {
                Ok(()) => {
                    entry.version = Some(lines.version.clone());
                    entry.lines = lines.lines.len();
                    entry.pushed_at_ms = now;
                    entry.failures = 0;
                    if !lines.lines.is_empty() {
                        due(now.saturating_add(LINE_PUSH_TTL_MS / 2));
                    }
                    outcomes.push(outcome("pushed", None));
                }
                Err(code) => {
                    entry.failures = entry.failures.saturating_add(1);
                    entry.retry_at_ms = now.saturating_add(backoff(entry.failures));
                    due(entry.retry_at_ms);
                    outcomes.push(outcome("failed", Some(code)));
                }
            }
        }
        // Destinations no longer bound (in this pass's scope) are withdrawn once;
        // one the target cannot be told about expires there on its own.
        let gone: Vec<(String, String)> = state
            .destinations
            .iter()
            .filter(|(key, pushed)| {
                !current.contains_key(*key)
                    && owner.is_none()
                    && (pushed.version.is_some() || pushed.failures > 0)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in gone {
            let Some(entry) = state.destinations.get_mut(&key) else {
                continue;
            };
            let expired = now >= entry.pushed_at_ms.saturating_add(MAX_OBSERVATION_TTL_MS);
            let report_of = |outcome: &str, code: Option<String>| LinePushOutcome {
                registration_id: key.0.clone(),
                checkout_id: key.1.clone(),
                lines: 0,
                outcome: outcome.into(),
                code,
            };
            if entry.lines == 0 && entry.failures == 0 || expired {
                state.destinations.remove(&key);
                continue;
            }
            if entry.failures > 0 && now < entry.retry_at_ms {
                due(entry.retry_at_ms);
                outcomes.push(report_of("waiting", None));
                continue;
            }
            state.sequence = state.sequence.saturating_add(1).max(now);
            let body = ObservedLines {
                owner_server_id: self.server_id.clone(),
                sequence: state.sequence,
                ttl_ms: Some(LINE_PUSH_TTL_MS),
                lines: Vec::new(),
            };
            match self.send_lines(entry, &body) {
                Ok(()) => {
                    state.destinations.remove(&key);
                    outcomes.push(report_of("withdrawn", None));
                }
                Err(code) => {
                    entry.failures = entry.failures.saturating_add(1);
                    entry.retry_at_ms = now.saturating_add(backoff(entry.failures));
                    due(entry.retry_at_ms);
                    outcomes.push(report_of("failed", Some(code)));
                }
            }
        }
        report.destinations = outcomes;
        Ok(report)
    }

    /// [`Self::push_target_lines`] on the system clock, for the server's follower.
    pub fn push_target_lines_now(
        &self,
        state: &mut LinePushState,
    ) -> Result<LinePushReport, OrchestrationError> {
        let mut rng = Rng;
        self.push_target_lines(
            state,
            None,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }

    /// Whether any reviewed binding on this machine names a remote destination.
    pub fn has_remote_targets(&self) -> bool {
        BindingRegistry::new(&self.home)
            .remote_destinations()
            .is_ok_and(|destinations| !destinations.is_empty())
    }

    /// HS2-KB2Y51: the hosted half. Keep what another machine pushed for checkout
    /// `checkout_id` hosted here: display-only, expiring, never a plan record, never
    /// written to git or a ticket store. Lines of tickets outside the checkout's own
    /// sources are dropped.
    pub fn target_line_observations_put(
        &self,
        checkout_id: &str,
        post: ObservedLines,
        tx: &mut Transaction<'_>,
    ) -> Result<hotsheet_ticketing::orchestration::ObservationOutcome, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        let sources: Vec<String> = checkout
            .sources
            .iter()
            .map(|source| source.connection_id.clone())
            .collect();
        hotsheet_ticketing::orchestration::LineObservations::new(&self.home).put(
            &checkout.id,
            post,
            &sources,
            tx.clock.now_ms(),
        )
    }
}
