//! The store that records a terminal AI session's usage (HS2-B4EQ87).
//!
//! A machine server's primary store is its bootstrap store, not a project. A terminal's
//! usage belongs to the project the terminal works in: the registered checkout the AI
//! session reported (`checkout_id`), else the checkout containing the terminal's working
//! directory, the same rule that keeps the checkout's stores hosted while the terminal lives
//! (HS2-R5KV1Q). Within that checkout, the store holding the terminal worker's unique live
//! claim records the turn and supplies its ticket; with no claim, the checkout's usage store
//! ([`Checkout::usage_source`]) does. A terminal outside every registered checkout searches
//! every hosted store for its claim and otherwise keeps the primary store.

use crate::{AppState, now};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::checkouts::{Checkout, TicketSource};
use hotsheet_ticketing::{FsStore, StoreError, metrics};
use std::path::Path;

/// Where one turn's usage is recorded, and the ticket it is attributed to.
#[derive(Clone)]
pub(crate) struct UsageTarget {
    pub(crate) store: FsStore,
    pub(crate) ticket: Option<String>,
}

/// The registered checkout whose root contains `dir` (the deepest one when roots nest).
pub(crate) fn checkout_containing(state: &AppState, dir: &Path) -> Option<Checkout> {
    let dir = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    state
        .checkout_registry
        .list()
        .unwrap_or_default()
        .into_iter()
        .filter(|checkout| dir.starts_with(Path::new(&checkout.root)))
        .max_by_key(|checkout| checkout.root.len())
}

/// The usage checkout id recorded for terminal `id`'s reported AI session, if any.
pub(crate) fn terminal_usage_checkout(state: &AppState, id: &str) -> Option<String> {
    state
        .terminal_ai_connections
        .lock_or_recover()
        .get(id)
        .and_then(|connection| connection.usage_checkout.clone())
}

/// A linked Git source's store: the hosted entry when hosted, else opened from its path.
fn source_store(state: &AppState, source: &TicketSource) -> Option<FsStore> {
    if let Some(entry) = state.host.get(&source.connection_id) {
        return Some(entry.store);
    }
    FsStore::open(&source.locator)
        .inspect_err(|error| {
            tracing::warn!(
                %error,
                store = %source.connection_id,
                "opening a terminal usage store failed"
            );
        })
        .ok()
}

/// Resolve where terminal `id`'s turn records usage now, attributing it to the ticket that
/// one of `workers` uniquely holds under a live claim. Blocking: lists tickets.
pub(crate) fn terminal_usage_target(
    state: &AppState,
    id: &str,
    workers: &[&str],
) -> Result<UsageTarget, StoreError> {
    usage_target_for_checkout(
        state,
        terminal_usage_checkout(state, id).as_deref(),
        workers,
    )
}

/// [`terminal_usage_target`] for a known usage checkout id, as a Claude transcript follow
/// resumed after a restart persisted it before any session reported again (HS2-SSMDQ1).
pub(crate) fn usage_target_for_checkout(
    state: &AppState,
    checkout_id: Option<&str>,
    workers: &[&str],
) -> Result<UsageTarget, StoreError> {
    let checkout =
        checkout_id.and_then(|checkout_id| state.checkout_registry.describe(checkout_id).ok());
    let (candidates, fallback) = match &checkout {
        Some(checkout) => {
            let stores: Vec<FsStore> = checkout
                .claim_next_sources()
                .unwrap_or_default()
                .into_iter()
                .filter_map(|source| source_store(state, source))
                .collect();
            // `claim_next_sources` puts the usage source first.
            let fallback = stores.first().cloned();
            (stores, fallback)
        }
        None => {
            let primary = crate::multistore::store_url_id(&state.store);
            let mut stores = vec![state.store.clone()];
            stores.extend(
                state
                    .host
                    .locations()
                    .into_iter()
                    .filter(|(id, _)| *id != primary)
                    .filter_map(|(id, _)| state.host.get(&id))
                    .map(|entry| entry.store),
            );
            (stores, None)
        }
    };
    let now = now();
    let mut held = Vec::new();
    for store in &candidates {
        if let Some(ticket) = metrics::active_ticket_for_workers(store, workers, &now)? {
            held.push((store, ticket));
        }
    }
    if let [(store, ticket)] = held.as_slice() {
        return Ok(UsageTarget {
            store: (*store).clone(),
            ticket: Some(ticket.clone()),
        });
    }
    Ok(UsageTarget {
        store: fallback.unwrap_or_else(|| state.store.clone()),
        ticket: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::terminals::TerminalAiConnection;
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::checkouts::CheckoutRegistry;
    use hotsheet_ticketing::{NewTicket, StoreMetadata, ops};

    const WORKER: &str = "terminal-t1";

    fn claim(store: &FsStore, prefix: &str) -> String {
        let ticket = ops::create(
            store,
            Ulid::new(),
            prefix,
            Timestamp::new("2026-11-06T00:00:00Z"),
            NewTicket {
                title: "terminal work".into(),
                category: "task".into(),
                ..Default::default()
            },
        )
        .unwrap();
        ops::claim(
            store,
            &ticket.id,
            &Timestamp::new("2026-11-06T00:00:00Z"),
            Timestamp::new("2099-01-01T00:00:00Z"),
            WORKER,
            None,
        )
        .unwrap();
        ticket.id.to_string()
    }

    fn connect(state: &AppState, checkout: Option<String>) {
        state.terminal_ai_connections.lock_or_recover().insert(
            "t1".into(),
            TerminalAiConnection {
                agent: Some("claude".into()),
                checkout_id: None,
                usage_checkout: checkout,
                at: String::new(),
                source: Default::default(),
                session_id: Some("s1".into()),
                codex_home: None,
                claude_inbox: None,
            },
        );
    }

    /// HS2-B4EQ87 transitions: no checkout keeps the primary store; a checkout with no
    /// claim uses its usage source; a claim in a later linked store moves the turn there;
    /// releasing it returns to the usage source.
    #[test]
    fn hs2_b4eq87_target_follows_the_terminal_checkout_and_its_claim() {
        let root = tempfile::tempdir().unwrap();
        let primary =
            FsStore::init_git(root.path().join("primary.hs2"), &StoreMetadata::new("HS")).unwrap();
        let first =
            FsStore::init_git(root.path().join("first.hs2"), &StoreMetadata::new("FA")).unwrap();
        let second =
            FsStore::init_git(root.path().join("second.hs2"), &StoreMetadata::new("SB")).unwrap();
        let project = root.path().join("project");
        std::fs::create_dir_all(project.join("nested")).unwrap();
        let registry = root.path().join("checkouts.json");
        let checkout = CheckoutRegistry::new(&registry)
            .register(
                &project,
                None,
                None,
                vec![first.root().to_path_buf(), second.root().to_path_buf()],
            )
            .unwrap();
        let state = AppState::new(primary.clone(), "secret".into())
            .unwrap()
            .with_checkout_registry(&registry);
        assert_eq!(
            checkout_containing(&state, &project.join("nested")).map(|found| found.id),
            Some(checkout.id.clone())
        );
        assert!(checkout_containing(&state, root.path()).is_none());

        let target = |state: &AppState| terminal_usage_target(state, "t1", &[WORKER]).unwrap();
        // No reported connection, then a connection outside every checkout.
        assert_eq!(target(&state).store.root(), primary.root());
        connect(&state, None);
        assert_eq!(target(&state).store.root(), primary.root());

        connect(&state, Some(checkout.id.clone()));
        let usage_root = checkout.usage_source().unwrap().locator.clone();
        let unclaimed = target(&state);
        assert_eq!(
            unclaimed.store.root().canonicalize().unwrap(),
            Path::new(&usage_root)
        );
        assert_eq!(unclaimed.ticket, None);

        let other = if first.root().canonicalize().unwrap() == Path::new(&usage_root) {
            &second
        } else {
            &first
        };
        let ticket = claim(other, "XX");
        let claimed = target(&state);
        assert_eq!(
            claimed.store.root().canonicalize().unwrap(),
            other.root().canonicalize().unwrap()
        );
        assert_eq!(claimed.ticket.as_deref(), Some(ticket.as_str()));

        ops::release(
            other,
            &Ulid::from_string(&ticket).unwrap(),
            Timestamp::new("2026-11-06T00:00:01Z"),
            WORKER,
            false,
        )
        .unwrap();
        let released = target(&state);
        assert_eq!(
            released.store.root().canonicalize().unwrap(),
            Path::new(&usage_root)
        );
        assert_eq!(released.ticket, None);

        // A primary-store claim never attributes a turn of a terminal in another project.
        claim(&primary, "HS");
        assert_eq!(target(&state).ticket, None);
    }
}
