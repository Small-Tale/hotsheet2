//! Source-scoped queue facts and semantic cursors (HS2-76VMYG).

use super::*;

impl Coordinator {
    pub fn snapshot(&self, now: i64) -> Result<Snapshot> {
        let state = self.state()?;
        let mut live_workers: Vec<String> = state
            .workers
            .values()
            .filter(|s| s.live(now) && state.active_assignment(&s.id).is_none())
            .map(|s| s.id.clone())
            .collect();
        let live_integrator = state.integrator.as_ref().is_some_and(|s| s.live(now));
        let live_dispatcher = state.dispatcher.as_ref().is_some_and(|s| s.live(now));
        let mut live_claims = Vec::new();
        let mut selected_connection = None;
        let mut selected_tickets = None;
        let local_queue = if self
            .source
            .join(hotsheet_ticketing::STORE_METADATA_FILE)
            .exists()
        {
            let store = FsStore::open_without_maintenance(&self.source)?;
            let connection_id = git_connection_id(&store);
            selected_connection = Some(connection_id.clone());
            let now = hotsheet_model::Timestamp::from_datetime(
                time::OffsetDateTime::from_unix_timestamp(now)?,
            );
            let mut tickets = store.list_tickets()?;
            selected_tickets = Some(tickets.clone());
            live_claims = tickets
                .iter()
                .filter(|t| !ops::claim_available(t, &now))
                .filter_map(|t| {
                    let worker = t.claimed_by.as_ref()?;
                    let expiry = t.claim_lease_expires_at.as_ref()?.instant()?;
                    Some(contract::LiveClaim {
                        ticket: TicketRef {
                            connection_id: connection_id.clone(),
                            native_id: t.id.to_string(),
                        },
                        worker: worker.clone(),
                        expires_at: expiry.unix_timestamp() + i64::from(expiry.nanosecond() != 0),
                        started_phase: t.started_phase,
                        eta_at: t.claim_eta_at.as_ref().map(|at| at.as_str().to_owned()),
                    })
                })
                .collect();
            tickets.sort_by_key(|t| t.id);
            Some(
                tickets
                    .into_iter()
                    .filter(|t| t.up_next && ops::is_open(t))
                    .map(|t| {
                        let claimed_by = if ops::claim_available(&t, &now) {
                            None
                        } else {
                            t.claimed_by.clone()
                        };
                        contract::QueueEntry {
                            ticket: TicketRef {
                                connection_id: connection_id.clone(),
                                native_id: t.id.to_string(),
                            },
                            slug: t.slug,
                            updated_at: t.updated_at.as_str().to_owned(),
                            phase: t.started_phase,
                            blocked: t
                                .blocked_reason
                                .as_ref()
                                .is_some_and(|r| !r.trim().is_empty()),
                            claimed_by,
                            claim_expires_at: t
                                .claim_lease_expires_at
                                .as_ref()
                                .and_then(|at| at.instant())
                                // Ceiling preserves fractional deadlines until this
                                // whole-second clock can observe their expiry.
                                .map(|at| at.unix_timestamp() + i64::from(at.nanosecond() != 0)),
                        }
                    })
                    .collect::<Vec<_>>(),
            )
        } else {
            None
        };
        live_claims.sort_by_key(|claim| claim.ticket.qualified());
        live_workers.retain(|worker| !live_claims.iter().any(|claim| &claim.worker == worker));
        let connections =
            hotsheet_ticketing::ProviderConfigRegistry::new(self.source.join("providers.json"))
                .load()?;
        let disabled: std::collections::HashSet<_> = connections
            .iter()
            .filter(|s| s.disabled)
            .map(|s| s.id.clone())
            .collect();
        let mut uncovered_sources: Vec<String> = connections
            .iter()
            .filter(|s| !s.disabled)
            .map(|s| s.id.clone())
            .collect();
        let registry = checkouts::CheckoutRegistry::new(self.registry_home.join("checkouts.json"));
        let checkouts: Vec<_> = registry
            .list()?
            .into_iter()
            .filter(|c| c.id == self.project_id)
            .collect();
        let source_support = super::source_support::reports(
            selected_connection.as_deref(),
            &self.source,
            &connections,
            &checkouts,
        );
        for checkout in &checkouts {
            for source in &checkout.sources {
                if disabled.contains(&source.connection_id) {
                    continue;
                }
                if !(source.provider == "git"
                    && Path::new(&source.locator).canonicalize().ok().as_ref()
                        == Some(&self.source))
                {
                    uncovered_sources.push(source.connection_id.clone());
                }
            }
            for store in &checkout.stores {
                if Path::new(&store).canonicalize().ok().as_ref() != Some(&self.source) {
                    uncovered_sources.push(format!("{:x}", Sha256::digest(store.as_bytes())));
                }
            }
        }
        uncovered_sources.sort();
        uncovered_sources.dedup();
        let queue_support = if local_queue.is_some() {
            "selected_local_source_only; uncovered_sources require provider-specific query/claim/event adapters"
        } else {
            "provider_query_required; use provider-specific query/claim/event capabilities"
        }
        .to_owned();
        let dispatch_blockers =
            Self::dispatch_blockers(&state, now, &live_workers, local_queue.is_some());
        let planned = state
            .next_plan
            .iter()
            .map(|entry| contract::PlannedTicket {
                entry: entry.clone(),
                reasons: plan_reasons(
                    &state,
                    entry,
                    selected_connection.as_deref(),
                    selected_tickets.as_deref(),
                    now,
                ),
            })
            .collect::<Vec<_>>();
        let cursor = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&(
                Self::state_cursor(&state, now)?,
                &local_queue,
                &uncovered_sources,
                &live_claims,
                &source_support
            ))?)
        );
        Ok(Snapshot {
            cursor,
            state,
            live_workers,
            live_integrator,
            live_dispatcher,
            dispatch_blockers,
            local_queue,
            queue_support,
            uncovered_sources,
            live_claims,
            source_support,
            planned,
        })
    }

    pub(crate) fn state_cursor(state: &State, now: i64) -> Result<String> {
        let mut semantic = state.clone();
        semantic.revision = 0;
        for session in semantic
            .integrator
            .iter_mut()
            .chain(semantic.dispatcher.iter_mut())
            .chain(semantic.workers.values_mut())
        {
            session.expires_at = i64::from(session.live(now));
        }
        Ok(format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&semantic)?)
        ))
    }
}

fn plan_reasons(
    state: &State,
    entry: &contract::NextPlanEntry,
    selected_connection: Option<&str>,
    selected_tickets: Option<&[hotsheet_model::Ticket]>,
    now: i64,
) -> Vec<String> {
    let mut reasons = Vec::new();
    match state.workers.get(&entry.worker) {
        None => reasons.push("worker_departed".into()),
        Some(session) if session.generation != entry.generation => {
            reasons.push("worker_replaced".into())
        }
        Some(session) if !session.live(now) => reasons.push("worker_disconnected".into()),
        _ => {}
    }
    if state.active_assignment(&entry.worker).is_some() {
        reasons.push("worker_has_active_assignment".into());
    }
    if state.assignments.values().any(|assignment| {
        assignment.ticket == entry.ticket
            && assignment.phase.pending()
            && !assignment.returned_for_review()
    }) {
        reasons.push("ticket_already_assigned".into());
    }
    if selected_connection != Some(entry.ticket.connection_id.as_str()) {
        reasons.push("source_capability_or_availability_unknown".into());
        return reasons;
    }
    let Some(tickets) = selected_tickets else {
        reasons.push("source_unavailable".into());
        return reasons;
    };
    let Some(ticket) = tickets
        .iter()
        .find(|ticket| ticket.id.to_string() == entry.ticket.native_id)
    else {
        reasons.push("ticket_missing".into());
        return reasons;
    };
    if !ticket.up_next {
        reasons.push("not_up_next".into());
    }
    if !ops::is_open(ticket) {
        reasons.push("ticket_closed".into());
    }
    if ticket.slug != entry.slug {
        reasons.push("ticket_slug_changed".into());
    }
    if ticket
        .blocked_reason
        .as_ref()
        .is_some_and(|reason| !reason.trim().is_empty())
    {
        reasons.push("ticket_blocked".into());
    }
    if entry
        .priority_at_plan
        .is_some_and(|priority| priority != ticket.priority)
    {
        reasons.push("priority_changed".into());
    }
    let at = hotsheet_model::Timestamp::from_datetime(
        time::OffsetDateTime::from_unix_timestamp(now).expect("valid snapshot time"),
    );
    if !ops::claim_available(ticket, &at) {
        reasons.push("ticket_claim_held".into());
    }
    if tickets.iter().any(|candidate| {
        candidate.claimed_by.as_deref() == Some(&entry.worker)
            && !ops::claim_available(candidate, &at)
    }) {
        reasons.push("worker_claim_held".into());
    }
    for dependency in &ticket.blocked_by {
        if !tickets
            .iter()
            .find(|candidate| candidate.id == *dependency)
            .is_some_and(|candidate| !ops::is_open(candidate))
        {
            reasons.push("dependency_open_or_unknown".into());
            break;
        }
    }
    if state.authority == Authority::Dispatcher {
        match &state.integration {
            None => reasons.push("safe_base_unpublished".into()),
            Some(report) => {
                if !state.integrator.as_ref().is_some_and(|session| {
                    session.id == report.integrator && session.generation == report.generation
                }) {
                    reasons.push("safe_base_stale".into());
                }
                if report.blocked.contains(&entry.ticket) {
                    reasons.push("integrator_blocked".into());
                }
                if entry.scope.is_empty() {
                    reasons.push("scope_missing".into());
                }
                if entry.scope.iter().any(|path| {
                    report
                        .in_flight
                        .iter()
                        .any(|held| scope::scopes_overlap(path, held))
                }) {
                    reasons.push("scope_in_flight".into());
                }
            }
        }
        if entry.scope.iter().any(|path| {
            state
                .assignments
                .values()
                .filter(|assignment| {
                    assignment.phase.pending() && !assignment.returned_for_review()
                })
                .any(|assignment| {
                    assignment
                        .scope
                        .iter()
                        .any(|held| scope::scopes_overlap(path, held))
                })
        }) {
            reasons.push("scope_assigned".into());
        }
        if entry.scope.iter().any(|path| {
            state
                .next_plan
                .iter()
                .filter(|other| other.operation != entry.operation && other.worker != entry.worker)
                .any(|other| {
                    other
                        .scope
                        .iter()
                        .any(|held| scope::scopes_overlap(path, held))
                })
        }) {
            reasons.push("scope_planned_elsewhere".into());
        }
    }
    reasons
}
