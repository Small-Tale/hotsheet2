//! Assignment authority separate from integration (HS2-TWZW7X).
//!
//! Exactly one role reserves assignments. Schema three and older pools keep the legacy
//! integrator authority and are written back unchanged, so a frozen schema-three artifact
//! keeps operating them. Only the exact live integrator's explicit switch persists schema
//! four with dispatcher authority, which every schema-three artifact then refuses.

use serde::Serialize;

use super::scope::{entry, normalized_scope, normalized_shared, scopes_overlap};
use super::*;
use contract::{Authority, IntegrationReport};

/// Pool support this executable reports without opening (and so without migrating) a pool.
#[derive(Debug, Serialize)]
pub struct Capabilities {
    pub schemas_read: &'static [u32],
    pub schema_written_with_integrator_authority: u32,
    pub schema_written_with_dispatcher_authority: u32,
    /// Integrator-reviewed shared documents and section scopes (HS2-YHYNJ6).
    pub schema_written_with_shared_sections: u32,
    /// Suspended integration and prerequisite gate evidence (HS2-2D3099).
    pub schema_written_with_suspended_integration: u32,
    pub schema_written_with_next_plan: u32,
    pub scope_section_kinds: &'static [&'static str],
    pub assignment_authorities: &'static [&'static str],
    pub roles: &'static [&'static str],
}

pub const CAPABILITIES: Capabilities = Capabilities {
    schemas_read: &[1, 2, 3, 4, 5, 6, 7],
    schema_written_with_integrator_authority: 3,
    schema_written_with_dispatcher_authority: 4,
    schema_written_with_shared_sections: 5,
    schema_written_with_suspended_integration: 6,
    schema_written_with_next_plan: 7,
    scope_section_kinds: scope::SECTION_KINDS,
    assignment_authorities: &["integrator", "dispatcher"],
    roles: &["integrator", "worker", "dispatcher"],
};

impl Coordinator {
    /// Schema three must stay exactly what a schema-three artifact writes, and schema four
    /// exactly what a schema-four artifact can read without dropping shared sections.
    pub(super) fn schema_fence(state: &State) -> Result<()> {
        if state.version < 7 {
            ensure!(
                state.next_plan.is_empty()
                    && state.plan_revision == 0
                    && state.plan_events.is_empty(),
                "next plans require schema seven; upgrade every participant first"
            );
        }
        if state.version < 6 {
            ensure!(
                state.assignments.values().all(|a| {
                    a.phase != Phase::SuspendedIntegration
                        && a.suspension.is_none()
                        && a.suspension_history.is_empty()
                }),
                "suspended integration evidence requires schema six; upgrade every participant first"
            );
        }
        if state.version < 5 {
            let sectioned = |scopes: &[String]| scopes.iter().any(|s| s.contains('#'));
            ensure!(
                state.integration.as_ref().is_none_or(|report| {
                    report.shared.is_empty() && !sectioned(&report.in_flight)
                }) && !state.assignments.values().any(|a| sectioned(&a.scope)),
                "shared sections require schema five; publish them with --all-participants-upgraded first"
            );
        }
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
            let declared = entry(path);
            ensure!(
                declared.section.is_none()
                    || report.shared.iter().any(|shared| shared == declared.path),
                "scope {path} names a section of {}, which the integrator has not published as a reviewed shared document",
                declared.path
            );
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

    /// Publish the safe base, blocked tickets, in-flight scopes and reviewed shared documents
    /// the dispatcher must read before reserving work. Replaces the previous report
    /// atomically. The first shared document persists schema five, which every schema-four
    /// artifact refuses, so the caller asserts every participant was upgraded (HS2-YHYNJ6).
    /// `clock` is read only inside the state lock.
    #[allow(clippy::too_many_arguments)] // HS2-YHYNJ6: one atomic report, every field explicit.
    pub fn publish(
        &self,
        integrator: &str,
        generation: &str,
        clock: &dyn Fn() -> i64,
        base: &str,
        blocked: Vec<TicketRef>,
        in_flight: &[String],
        shared: &[String],
        all_participants_upgraded: bool,
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
        let shared = normalized_shared(shared)?;
        for held in &in_flight {
            let held = entry(held);
            ensure!(
                held.section.is_none() || shared.iter().any(|path| path == held.path),
                "in-flight section of {} requires publishing it as a shared document",
                held.path
            );
        }
        self.change(|state| {
            // Sampled at the serialized boundary, after any blocking validation and lock
            // wait, so an integrator that expired meanwhile cannot publish (HS2-YHYNJ6).
            Self::integrator(state, integrator, generation, clock())?;
            ensure!(
                state.version >= 4,
                "integration reports require dispatcher authority (schema four)"
            );
            if !shared.is_empty() && state.version < 5 {
                ensure!(
                    all_participants_upgraded,
                    "stop old waiters and switch every participant to a schema-five artifact, then pass --all-participants-upgraded"
                );
                state.version = 5;
            }
            state.integration = Some(IntegrationReport {
                integrator: integrator.into(),
                generation: generation.into(),
                base: base.into(),
                blocked,
                in_flight,
                shared,
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
                    state.version = state.version.max(4);
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
