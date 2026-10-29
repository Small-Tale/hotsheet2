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
