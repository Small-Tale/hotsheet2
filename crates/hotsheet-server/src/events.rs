//! Change-event publication: the long-poll ring, live bus, and index-maintaining
//! write notifications (`emit`, `changed_many_in`, `removed_in`).

use crate::*;

impl AppState {
    pub(crate) fn emit(&self, event: ChangeEvent) {
        emit_change(&self.event_log, &self.events, event);
    }

    /// The current long-poll cursor (the last emitted event's seq; 0 if none).
    pub(crate) fn event_cursor(&self) -> u64 {
        self.event_log.with_lock(|l| l.seq)
    }

    /// Run one server-side index write, surfacing a SQLite failure instead of swallowing
    /// it (HS2-JD7TK0). A poisoned index lock is recovered (HS2-ZGQJZP).
    pub(crate) fn index_write(
        &self,
        entry: &StoreEntry,
        write: impl FnOnce(&Index) -> Result<(), IndexError>,
    ) -> Result<(), String> {
        #[cfg(test)]
        if self
            .index_write_faults
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
            .is_ok()
        {
            return Err("injected index write fault".into());
        }
        let index = entry.index.lock_or_recover();
        write(&index).map_err(|error| error.to_string())
    }

    /// Log a failed index write and queue that ticket for re-indexing from its file.
    pub(crate) fn schedule_index_repair(&self, store_id: &str, id: Ulid, error: &str) {
        tracing::warn!(ticket = %id, store = store_id, error, "index write failed; scheduled repair");
        self.pending_index_repairs
            .lock_or_recover()
            .insert((store_id.to_string(), id));
    }

    /// Re-index every queued ticket of `entry` from its current file (or drop its row
    /// when the file is gone). A still-failing repair stays queued.
    pub(crate) fn repair_pending_index_rows(&self, entry: &StoreEntry, store_id: &str) {
        let queued: Vec<Ulid> = self.pending_index_repairs.with_lock(|pending| {
            let ids = pending
                .iter()
                .filter(|(store, _)| store == store_id)
                .map(|(_, id)| *id)
                .collect::<Vec<_>>();
            for id in &ids {
                pending.remove(&(store_id.to_string(), *id));
            }
            ids
        });
        for id in queued {
            let path = entry.store.ticket_path(&id);
            let result = match std::fs::read(&path) {
                Ok(bytes) => match parse_file(&String::from_utf8_lossy(&bytes)) {
                    Ok(ticket) => {
                        let hash = hash_bytes(&bytes);
                        let path = path.display().to_string();
                        self.index_write(entry, |index| index.upsert(&ticket, &path, &hash))
                    }
                    // A corrupt file is the watcher's recovery path, not an index fault.
                    Err(_) => Ok(()),
                },
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    self.index_write(entry, |index| index.delete(&id))
                }
                Err(error) => Err(error.to_string()),
            };
            if let Err(error) = result {
                self.schedule_index_repair(store_id, id, &error);
            }
        }
    }

    /// Reindex a ticket the server just wrote into `entry`'s index, then broadcast a
    /// change tagged with the store it happened in. The index now carries the file's
    /// hash, so the watcher sees "no change" and won't re-emit.
    pub(crate) fn changed_in(&self, entry: &StoreEntry, kind: &str, t: &Ticket) {
        // The phase timings only feed request-performance reporting for batches.
        let _ = self.changed_many_in(entry, kind, std::slice::from_ref(t));
    }

    /// Publish a batch after every file has been written. Rebuild each affected checkout
    /// worklist once instead of once per ticket.
    pub(crate) fn changed_many_in(
        &self,
        entry: &StoreEntry,
        kind: &str,
        tickets: &[Ticket],
    ) -> [Duration; 3] {
        let store_id = multistore::store_url_id(&entry.store);
        self.repair_pending_index_rows(entry, &store_id);
        let mut index_time = Duration::ZERO;
        let mut event_time = Duration::ZERO;
        for t in tickets {
            let text = to_file_string(t);
            let path = entry.store.ticket_path(&t.id).display().to_string();
            let hash = hash_bytes(text.as_bytes());
            let index_started = Instant::now();
            let indexed = self.index_write(entry, |index| index.upsert(t, &path, &hash));
            index_time += index_started.elapsed();
            match indexed {
                // Only a successfully indexed write may suppress its watcher echo; a
                // failed one leaves the echo free to re-index the file (HS2-JD7TK0).
                Ok(()) => {
                    let mut writes = self.local_write_hashes.lock_or_recover();
                    writes.insert(
                        (store_id.clone(), t.id.to_string(), hash.clone()),
                        std::time::Instant::now(),
                    );
                }
                Err(error) => self.schedule_index_repair(&store_id, t.id, &error),
            }
            let event_started = Instant::now();
            self.emit(ChangeEvent {
                cursor: None,
                store: store_id.clone(),
                kind: kind.to_string(),
                id: t.id.to_string(),
                slug: t.slug.clone(),
                message: None,
                activity: None,
                assignment: None,
                turn: None,
            });
            event_time += event_started.elapsed();
        }
        let worklist_started = Instant::now();
        if let Ok(checkouts) = self.checkout_registry.list() {
            for checkout in checkouts.into_iter().filter(|checkout| {
                checkout
                    .stores
                    .iter()
                    .any(|root| same_path(FsPath::new(root), entry.store.root()))
            }) {
                if let Err(error) = regenerate_checkout_worklist_indexed(&self.host, &checkout) {
                    tracing::warn!("worklist regenerate failed for {}: {error}", checkout.root);
                }
            }
        }
        let worklist_time = worklist_started.elapsed();
        // A write is worth pushing promptly — wake the background sync loop (HS2-731C2X).
        self.kick_sync();
        [index_time, event_time, worklist_time]
    }

    /// Remove a hard-purged ticket from the live index and publish one deletion event.
    /// The empty hash marker suppresses the filesystem watcher's echo of this local write.
    pub(crate) fn removed_in(&self, entry: &StoreEntry, ticket: &Ticket) {
        let store_id = multistore::store_url_id(&entry.store);
        self.repair_pending_index_rows(entry, &store_id);
        match self.index_write(entry, |index| index.delete(&ticket.id)) {
            Ok(()) => {
                let mut writes = self.local_write_hashes.lock_or_recover();
                writes.insert(
                    (store_id.clone(), ticket.id.to_string(), String::new()),
                    std::time::Instant::now(),
                );
            }
            Err(error) => self.schedule_index_repair(&store_id, ticket.id, &error),
        }
        self.emit(ChangeEvent {
            cursor: None,
            store: store_id,
            kind: "deleted".into(),
            id: ticket.id.to_string(),
            slug: ticket.slug.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
        if let Ok(checkouts) = self.checkout_registry.list() {
            for checkout in checkouts.into_iter().filter(|checkout| {
                checkout
                    .stores
                    .iter()
                    .any(|root| same_path(FsPath::new(root), entry.store.root()))
            }) {
                if let Err(error) = regenerate_checkout_worklist_indexed(&self.host, &checkout) {
                    tracing::warn!("worklist regenerate failed for {}: {error}", checkout.root);
                }
            }
        }
        self.kick_sync();
    }

    /// Broadcast an **ephemeral announcement** to live `/ws/sync` subscribers (HS2-HHDNTH):
    /// a store-level message that is **not** persisted — it rides the WS bus only, so it is
    /// NOT recorded in the long-poll ring and never replayed. A client not connected when it
    /// fires simply misses it. `store` is the target store's URL id (empty = the default).
    pub fn announce(&self, store: String, message: String) {
        // WS-only: intentionally skip the EventLog ring (ephemeral, unlike `emit`).
        let _ = self.events.send(ChangeEvent {
            cursor: None,
            store,
            kind: "announce".to_string(),
            id: String::new(),
            slug: String::new(),
            message: Some(message),
            activity: None,
            assignment: None,
            turn: None,
        });
    }

    pub(crate) fn emit_drive_updated(&self, info: &client_drive::ClientConnectionInfo) {
        self.emit(ChangeEvent {
            cursor: None,
            store: info.source.clone(),
            kind: "drive_updated".into(),
            id: info.id.clone(),
            slug: info.tool.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
    }

    pub fn emit_turn_event(
        &self,
        store: &FsStore,
        connection_id: &str,
        ticket: Option<String>,
        tool: &str,
        event: turn_stream::ClientTurnEvent,
    ) {
        self.emit(ChangeEvent {
            cursor: None,
            store: multistore::store_url_id(store),
            kind: "turn_event".into(),
            id: connection_id.into(),
            slug: tool.into(),
            message: None,
            activity: None,
            assignment: None,
            turn: Some(turn_stream::TurnStreamEnvelope {
                connection_id: connection_id.into(),
                ticket,
                event,
            }),
        });
    }
}

/// A live-change event pushed over `/ws/sync`.
#[derive(Clone, Debug, Serialize)]
pub struct ChangeEvent {
    /// Monotonic replay cursor. Present for durable/replayable events; absent for ephemeral
    /// WS-only announcements.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<u64>,
    /// The URL id of the store the change happened in (multi-store, HS2-87).
    pub store: String,
    pub kind: String,
    pub id: String,
    pub slug: String,
    /// For `kind == "announce"` (HS2-HHDNTH): the broadcast message text. `None` for
    /// ticket-change events (omitted on the wire).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    /// For `kind == "activity"`: the complete event persisted to the rolling timeline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity: Option<hotsheet_ticketing::ActivityEvent>,
    /// For `kind == "assignment"`: newly assigned/requested recipients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<AssignmentEvent>,
    /// For `kind == "turn_event"`: bounded raw tool output/usage/activity/done.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn: Option<turn_stream::TurnStreamEnvelope>,
}

#[derive(Clone, Debug, Serialize)]
pub struct AssignmentEvent {
    pub newly_assigned: Vec<String>,
    pub review_requested: Vec<String>,
    pub requested_by: Option<String>,
}

/// How many recent events the long-poll ring retains. A poller whose cursor falls behind
/// this many events gets an `overflow` signal and should re-sync via a full list.
pub(crate) const EVENT_LOG_CAP: usize = 512;

/// A bounded, monotonically-sequenced ring of recent [`ChangeEvent`]s, so a long-poll
/// client can ask "everything since cursor N" without holding a socket (HS2-P3P3CC). Each
/// event gets a `seq`; the ring keeps the newest [`EVENT_LOG_CAP`].
#[derive(Default)]
pub(crate) struct EventLog {
    /// The last sequence number assigned (0 = nothing emitted yet).
    pub(crate) seq: u64,
    /// `(seq, event)`, oldest first, capped at [`EVENT_LOG_CAP`].
    pub(crate) ring: std::collections::VecDeque<(u64, ChangeEvent)>,
}

pub(crate) fn emit_change(
    event_log: &Arc<Mutex<EventLog>>,
    events: &broadcast::Sender<ChangeEvent>,
    event: ChangeEvent,
) {
    // Record first so a long poll racing the broadcast can always replay by cursor.
    let event = event_log.with_lock(|log| log.push(event.clone()));
    let _ = events.send(event); // Err just means no live subscribers.
}

impl EventLog {
    /// Record an event, assigning it the next seq.
    pub(crate) fn push(&mut self, mut event: ChangeEvent) -> ChangeEvent {
        self.seq += 1;
        event.cursor = Some(self.seq);
        self.ring.push_back((self.seq, event.clone()));
        while self.ring.len() > EVENT_LOG_CAP {
            self.ring.pop_front();
        }
        event
    }

    /// Events with `seq > since`, plus whether `since` fell off the back of the ring
    /// (the caller lost events and should re-sync). `since >= seq` (caught up / future) is
    /// not an overflow — it just yields no events.
    pub(crate) fn since(&self, since: u64) -> (Vec<ChangeEvent>, bool) {
        let oldest = self.ring.front().map(|(s, _)| *s);
        // Overflow only when we've dropped events the caller hadn't seen: they ask for
        // `since` strictly before our oldest retained event, and we have emitted past it.
        let overflow = matches!(oldest, Some(o) if since.saturating_add(1) < o);
        let events = self
            .ring
            .iter()
            .filter(|(s, _)| *s > since)
            .map(|(_, e)| e.clone())
            .collect();
        (events, overflow)
    }
}

#[cfg(test)]
mod event_log_tests {
    use super::{ChangeEvent, EVENT_LOG_CAP, EventLog};

    fn ev(n: usize) -> ChangeEvent {
        ChangeEvent {
            cursor: None,
            store: "s".into(),
            kind: "created".into(),
            id: format!("id-{n}"),
            slug: format!("HS-{n}"),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        }
    }

    #[test]
    fn since_returns_the_tail_and_advances_the_cursor() {
        let mut log = EventLog::default();
        for i in 0..3 {
            log.push(ev(i));
        }
        assert_eq!(log.seq, 3);
        // Everything since 0 = all three; since 2 = just the last; caught up = none.
        let (all, of) = log.since(0);
        assert_eq!(all.len(), 3);
        assert!(!of);
        assert_eq!(all[0].cursor, Some(1));
        assert_eq!(log.since(2).0.len(), 1);
        assert!(log.since(3).0.is_empty(), "caught up → none");
        // A future/equal cursor is not an overflow.
        assert!(!log.since(3).1);
        assert!(!log.since(99).1);
        assert!(
            log.since(u64::MAX).0.is_empty(),
            "the largest possible future cursor is safe and caught up"
        );
        assert!(!log.since(u64::MAX).1);
    }

    #[test]
    fn falling_behind_the_ring_signals_overflow() {
        let mut log = EventLog::default();
        // Emit more than the ring holds, so the oldest retained seq > 1.
        for i in 0..(EVENT_LOG_CAP + 10) {
            log.push(ev(i));
        }
        // A poller stuck at cursor 1 lost events that aged out → overflow.
        let (_, overflow) = log.since(1);
        assert!(
            overflow,
            "cursor before the oldest retained event overflows"
        );
        // A poller within the retained window does not overflow.
        let recent = log.seq - 5;
        assert!(!log.since(recent).1);
        assert_eq!(log.since(recent).0.len(), 5);
    }
}

#[cfg(test)]
mod index_repair_tests {
    use super::{AppState, multistore};
    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_sync::LockExt;
    use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
    use std::sync::atomic::Ordering;

    fn indexed_hash(entry: &super::StoreEntry, id: &Ulid) -> Option<String> {
        let index = entry.index.lock_or_recover();
        index.content_hash(id).unwrap()
    }

    /// HS2-JD7TK0: a failed server index write is logged, does not suppress the
    /// watcher echo, and is repaired from the file on the store's next write.
    #[test]
    fn failed_index_writes_are_repaired_on_the_next_store_write() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let first = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        let second = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAB").unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let store_id = multistore::store_url_id(&store);
        let entry = state.host.get(&store_id).expect("primary entry");

        let created = ops::create(
            &store,
            first,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        state.index_write_faults.store(1, Ordering::SeqCst);
        state.changed_in(&entry, "created", &created);
        assert_eq!(indexed_hash(&entry, &first), None);
        assert!(
            state
                .pending_index_repairs
                .lock_or_recover()
                .contains(&(store_id.clone(), first))
        );
        // The failed write must not mark its watcher echo as a local no-op.
        assert!(state.local_write_hashes.lock_or_recover().is_empty());

        // A later write to the same store repairs the queued row from disk.
        let other = ops::create(
            &store,
            second,
            "HS",
            Timestamp::new("2026-09-02T00:00:01Z"),
            NewTicket::default(),
        )
        .unwrap();
        state.changed_in(&entry, "created", &other);
        assert!(indexed_hash(&entry, &first).is_some());
        assert!(indexed_hash(&entry, &second).is_some());
        assert!(state.pending_index_repairs.lock_or_recover().is_empty());

        // A failed delete is queued too; once the file is gone the repair drops the row.
        state.index_write_faults.store(1, Ordering::SeqCst);
        state.removed_in(&entry, &created);
        std::fs::remove_file(store.ticket_path(&first)).unwrap();
        assert!(indexed_hash(&entry, &first).is_some());
        state.changed_in(&entry, "changed", &other);
        assert_eq!(indexed_hash(&entry, &first), None);
        assert!(state.pending_index_repairs.lock_or_recover().is_empty());
    }

    /// HS2-PD8NJ6: the failure is a structured `tracing` warning naming the ticket.
    #[test]
    fn a_failed_index_write_emits_a_structured_warning() {
        #[derive(Clone, Default)]
        struct Buffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);
        impl std::io::Write for Buffer {
            fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
                self.0.lock_or_recover().extend_from_slice(bytes);
                Ok(bytes.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let entry = state.host.get(&multistore::store_url_id(&store)).unwrap();
        let ticket = ops::create(
            &store,
            id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let buffer = Buffer::default();
        let writer = buffer.clone();
        let subscriber = tracing_subscriber::fmt()
            .with_writer(move || writer.clone())
            .with_ansi(false)
            .finish();
        state.index_write_faults.store(1, Ordering::SeqCst);
        tracing::subscriber::with_default(subscriber, || {
            state.changed_in(&entry, "created", &ticket);
        });
        let logged = String::from_utf8(buffer.0.lock_or_recover().clone()).unwrap();
        assert!(logged.contains("WARN"), "{logged}");
        assert!(
            logged.contains("index write failed; scheduled repair"),
            "{logged}"
        );
        assert!(logged.contains(&format!("ticket={id}")), "{logged}");
        assert!(logged.contains("injected index write fault"), "{logged}");
    }

    /// HS2-ZGQJZP: a panic while holding a server lock must not wedge later writes.
    #[test]
    fn poisoned_server_locks_are_recovered_by_later_writes() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let entry = state.host.get(&multistore::store_url_id(&store)).unwrap();
        let ticket = ops::create(
            &store,
            id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let index = entry.index.clone();
        let writes = state.local_write_hashes.clone();
        let log = state.event_log.clone();
        let _ = std::thread::spawn(move || {
            let _a = index.lock().unwrap();
            let _b = writes.lock().unwrap();
            let _c = log.lock().unwrap();
            panic!("poison");
        })
        .join();
        assert!(entry.index.is_poisoned());
        state.changed_in(&entry, "created", &ticket);
        assert!(indexed_hash(&entry, &id).is_some());
        assert_eq!(state.local_write_hashes.lock_or_recover().len(), 1);
        assert!(state.event_cursor() > 0);
    }

    #[test]
    fn a_repair_that_fails_again_stays_queued() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let store_id = multistore::store_url_id(&store);
        let entry = state.host.get(&store_id).unwrap();
        let ticket = ops::create(
            &store,
            id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        // The write fails, its repair fails, and the write itself fails again.
        state.index_write_faults.store(3, Ordering::SeqCst);
        state.changed_in(&entry, "created", &ticket);
        state.changed_in(&entry, "changed", &ticket);
        assert!(
            state
                .pending_index_repairs
                .lock_or_recover()
                .contains(&(store_id.clone(), id))
        );
        state.changed_in(&entry, "changed", &ticket);
        assert!(indexed_hash(&entry, &id).is_some());
        assert!(state.pending_index_repairs.lock_or_recover().is_empty());
    }
}
