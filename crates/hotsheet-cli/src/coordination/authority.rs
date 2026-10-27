//! Assignment authority separate from integration (HS2-TWZW7X).
//!
//! Exactly one role reserves assignments. Schema three and older pools keep the legacy
//! integrator authority and are written back unchanged, so a frozen schema-three artifact
//! keeps operating them. Only the exact live integrator's explicit switch persists schema
//! four with dispatcher authority, which every schema-three artifact then refuses.

use serde::Serialize;

use super::*;
use contract::{Authority, IntegrationReport};

/// Pool support this executable reports without opening (and so without migrating) a pool.
#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub schemas_read: &'static [u32],
    pub schema_written_with_integrator_authority: u32,
    pub schema_written_with_dispatcher_authority: u32,
    pub assignment_authorities: &'static [&'static str],
    pub roles: &'static [&'static str],
}

pub const CAPABILITIES: Capabilities = Capabilities {
    schemas_read: &[1, 2, 3, 4],
    schema_written_with_integrator_authority: 3,
    schema_written_with_dispatcher_authority: 4,
    assignment_authorities: &["integrator", "dispatcher"],
    roles: &["integrator", "worker", "dispatcher"],
};

/// Repository-relative scope paths as normalized component lists, joined with `/`.
pub(super) fn normalized_scope(scope: &[String]) -> Result<Vec<String>> {
    let mut normalized = Vec::with_capacity(scope.len());
    for raw in scope {
        let path = raw.replace('\\', "/");
        ensure!(
            !path.starts_with('/') && !path.contains(':'),
            "scope paths must be repository-relative: {raw}"
        );
        let components: Vec<&str> = path
            .split('/')
            .filter(|part| !part.is_empty() && *part != ".")
            .collect();
        ensure!(
            !components.is_empty() && !components.contains(&".."),
            "scope path must name a file or directory inside the repository: {raw}"
        );
        normalized.push(components.join("/"));
    }
    normalized.sort();
    normalized.dedup();
    Ok(normalized)
}

/// Two scopes overlap when one path is the other or contains it.
pub(super) fn scopes_overlap(left: &str, right: &str) -> bool {
    let contains = |outer: &str, inner: &str| {
        inner == outer
            || inner
                .strip_prefix(outer)
                .is_some_and(|rest| rest.starts_with('/'))
    };
    contains(left, right) || contains(right, left)
}

impl Coordinator {
    /// Schema three must stay exactly what a schema-three artifact writes.
    pub(super) fn schema_fence(state: &State) -> Result<()> {
        if state.version < 4 {
            ensure!(
                state.authority == Authority::Integrator
                    && state.dispatcher.is_none()
                    && state.integration.is_none()
                    && state.assignments.values().all(|a| a.scope.is_empty()),
                "schema-four coordination data requires enabling dispatcher authority first"
            );
        }
        Ok(())
    }

    pub(super) fn require_dispatcher_authority(state: &State) -> Result<()> {
        ensure!(
            state.version >= 4 && state.authority == Authority::Dispatcher,
            "assignment authority is the integrator (schema {}); the live integrator must run `coordination authority --mode dispatcher --all-participants-upgraded` after every participant uses a schema-four artifact",
            state.version
        );
        Ok(())
    }

    pub(super) fn assignment_authority(
        state: &State,
        id: &str,
        generation: &str,
        now: i64,
    ) -> Result<()> {
        match state.authority {
            Authority::Integrator => {
                ensure!(
                    !state
                        .dispatcher
                        .as_ref()
                        .is_some_and(|s| s.id == id && s.generation == generation),
                    "assignment authority is the integrator; a dispatcher cannot assign in legacy mode"
                );
                Self::integrator(state, id, generation, now)
            }
            Authority::Dispatcher => {
                ensure!(
                    state
                        .dispatcher
                        .as_ref()
                        .is_some_and(|s| s.id == id && s.generation == generation && s.live(now)),
                    "assignment authority is the dispatcher; register or renew the exact dispatcher session (the integrator cannot assign)"
                );
                Ok(())
            }
        }
    }

    /// Dispatcher reservations honour the integrator's published facts and file scopes.
    pub(super) fn dispatch_rules(state: &State, assignment: &Assignment) -> Result<()> {
        if state.authority == Authority::Integrator {
            ensure!(
                assignment.scope.is_empty(),
                "assignment scopes require dispatcher authority"
            );
            return Ok(());
        }
        let report = Self::current_report(state)?;
        ensure!(
            assignment.base == report.base,
            "base must be the integrator's published safe shipping base {}",
            report.base
        );
        ensure!(
            !report.blocked.contains(&assignment.ticket),
            "the integrator has blocked this ticket from new assignment"
        );
        ensure!(
            !assignment.scope.is_empty(),
            "dispatcher assignments must declare their repository path scope"
        );
        for path in &assignment.scope {
            if let Some(held) = report
                .in_flight
                .iter()
                .find(|held| scopes_overlap(path, held))
            {
                anyhow::bail!("scope {path} overlaps in-flight integration scope {held}");
            }
            for other in state.assignments.values() {
                if !other.phase.pending() || other.returned_for_review() {
                    continue;
                }
                if let Some(held) = other.scope.iter().find(|held| scopes_overlap(path, held)) {
                    anyhow::bail!(
                        "scope {path} overlaps {held} held by pending assignment {}",
                        other.id
                    );
                }
            }
        }
        Ok(())
    }

    fn current_report(state: &State) -> Result<&IntegrationReport> {
        let report = state.integration.as_ref().ok_or_else(|| {
            anyhow::anyhow!("the integrator has not published a safe shipping base")
        })?;
        ensure!(
            state
                .integrator
                .as_ref()
                .is_some_and(|s| s.id == report.integrator && s.generation == report.generation),
            "the published integration report belongs to a replaced integrator generation; wait for it to republish"
        );
        Ok(report)
    }

    /// Publish the safe base, blocked tickets and in-flight scopes the dispatcher must
    /// read before reserving work. Replaces the previous report atomically.
    pub fn publish(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        base: &str,
        blocked: Vec<TicketRef>,
        in_flight: &[String],
    ) -> Result<()> {
        ensure!(
            base.len() == 40 && base.bytes().all(|c| c.is_ascii_hexdigit()),
            "base must be a full shipping SHA"
        );
        ensure!(
            blocked
                .iter()
                .all(|t| !t.connection_id.is_empty() && !t.native_id.is_empty()),
            "blocked tickets must be qualified connection:native references"
        );
        let in_flight = normalized_scope(in_flight)?;
        self.change(|state| {
            Self::integrator(state, integrator, generation, now)?;
            ensure!(
                state.version >= 4,
                "integration reports require dispatcher authority (schema four)"
            );
            state.integration = Some(IntegrationReport {
                integrator: integrator.into(),
                generation: generation.into(),
                base: base.into(),
                blocked,
                in_flight,
            });
            Ok(())
        })
    }

    /// Switch assignment authority. Only the exact live integrator may do this. Enabling
    /// the dispatcher persists schema four, which fences every schema-three artifact; the
    /// caller asserts every participant already runs a schema-four artifact. Returning to
    /// the integrator requires that no dispatcher session is live and never downgrades.
    pub fn set_authority(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        mode: Authority,
        all_participants_upgraded: bool,
    ) -> Result<()> {
        self.change(|state| {
            Self::integrator(state, integrator, generation, now)?;
            match mode {
                Authority::Dispatcher => {
                    ensure!(
                        all_participants_upgraded || state.version >= 4,
                        "stop old waiters and switch every participant to a schema-four artifact, then pass --all-participants-upgraded"
                    );
                    state.version = 4;
                    state.authority = Authority::Dispatcher;
                }
                Authority::Integrator => {
                    ensure!(
                        !state.dispatcher.as_ref().is_some_and(|s| s.live(now)),
                        "a live dispatcher still holds assignment authority; it must leave or expire first"
                    );
                    state.authority = Authority::Integrator;
                    state.dispatcher = None;
                }
            }
            Ok(())
        })
    }

    /// Explicit reasons the assignment authority cannot reserve work now.
    pub(super) fn dispatch_blockers(
        state: &State,
        now: i64,
        live_workers: &[String],
        queue_supported: bool,
    ) -> Vec<String> {
        let mut reasons = Vec::new();
        if state.stopped.is_some() {
            reasons.push("stopped".to_owned());
        }
        match state.authority {
            Authority::Integrator => {
                reasons.push("assignment_authority_is_integrator".to_owned());
                if !state.integrator.as_ref().is_some_and(|s| s.live(now)) {
                    reasons.push("no_live_integrator".to_owned());
                }
            }
            Authority::Dispatcher => {
                if !state.dispatcher.as_ref().is_some_and(|s| s.live(now)) {
                    reasons.push("no_live_dispatcher".to_owned());
                }
                match (&state.integrator, &state.integration) {
                    (None, _) => reasons.push("no_integrator_registered".to_owned()),
                    (Some(_), None) => reasons.push("no_safe_base_published".to_owned()),
                    (Some(_), Some(_)) if Self::current_report(state).is_err() => {
                        reasons.push("integration_report_from_replaced_generation".to_owned());
                    }
                    _ => {}
                }
            }
        }
        if live_workers.is_empty() {
            reasons.push("no_available_worker".to_owned());
        }
        if !queue_supported {
            reasons.push("selected_source_queue_unsupported".to_owned());
        }
        reasons
    }
}
