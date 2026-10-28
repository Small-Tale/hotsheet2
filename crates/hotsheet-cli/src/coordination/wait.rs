//! Filesystem subscriptions wake active calls. Lease deadlines renew availability;
//! they never query the ticket queue or issue network requests (HS2-76VMYG).

use std::fs::OpenOptions;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::{Result, bail};
use hotsheet_ticketing::FileLock;
use notify::{Event, RecursiveMode, Watcher};
use sha2::{Digest, Sha256};

use super::{Coordinator, Role, Snapshot, WaitRequest};

fn now() -> i64 {
    time::OffsetDateTime::now_utc().unix_timestamp()
}

impl Coordinator {
    pub fn next(&self, request: &WaitRequest) -> Result<Snapshot> {
        // The OS releases this lock on SIGKILL too. A duplicate waiter must not renew
        // another process's availability or consume the same assignment.
        let key = format!(
            "{:x}",
            Sha256::digest(format!("{:?}:{}", request.role, request.id))
        );
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(self.directory.join(format!("wait-{key}.lock")))?;
        let _waiter = FileLock::try_acquire(file)?
            .ok_or_else(|| anyhow::anyhow!("another active wait owns this session"))?;
        let (sender, receiver) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |event: notify::Result<Event>| {
            let _ = sender.send(event);
        })?;
        // Subscribe before reading/registering: a producer may arrive at any startup
        // point, and durable state closes the watch/reconnect race.
        watcher.watch(&self.directory, RecursiveMode::NonRecursive)?;
        watcher.watch(&self.source, RecursiveMode::NonRecursive)?;
        let registry_home = hotsheet_plugins::hotsheet_home();
        std::fs::create_dir_all(&registry_home)?;
        watcher.watch(&registry_home, RecursiveMode::NonRecursive)?;
        let registry_file = registry_home.join("checkouts.json");
        let providers_file = self.source.join("providers.json");
        let tickets = self.source.join("tickets");
        if tickets.is_dir() {
            watcher.watch(&tickets, RecursiveMode::Recursive)?;
        }
        let mut renew_at = now() + request.lease_seconds / 2;
        // HS2-TWZW7X: a dispatcher may start before dispatcher authority exists. It then
        // waits on genuine state events without registering, and says why.
        let mut registered = self.try_register(
            request.role,
            &request.id,
            &request.generation,
            now(),
            request.lease_seconds,
            request.return_route.clone(),
        )?;
        let mut snapshot_checked_at = now();
        let mut snapshot = self.snapshot(snapshot_checked_at)?;
        let mut state_cursor = Self::state_cursor(&snapshot.state, snapshot_checked_at)?;
        // A stopped run reaches every waiter, registered or not.
        if (registered || snapshot.state.stopped.is_some()) && ready(request, &snapshot) {
            return Ok(snapshot);
        }
        if registered {
            eprintln!("Waiting on local assignment/ticket events; availability registered.");
        } else {
            eprintln!(
                "Waiting on local assignment/ticket events; dispatcher not registered: assignment authority is the integrator (schema {}).",
                snapshot.state.version
            );
            #[cfg(test)]
            if let Some(hook) = self
                .waiting_unregistered
                .lock()
                .expect("test hook lock")
                .take()
            {
                hook();
            }
        }
        loop {
            let current = now();
            // HS2-TWZW7X: only real deadlines wake a wait. An unregistered dispatcher has no
            // lease to renew, and without peers or claims it blocks on genuine events alone.
            let peer_expiry = snapshot
                .state
                .integrator
                .iter()
                .chain(snapshot.state.dispatcher.iter())
                .chain(snapshot.state.workers.values())
                .filter(|s| s.expires_at > snapshot_checked_at)
                .map(|s| s.expires_at)
                .min();
            let claim_expiry = claim_deadline(request.role, &snapshot, snapshot_checked_at);
            let deadline = [registered.then_some(renew_at), peer_expiry, claim_expiry]
                .into_iter()
                .flatten()
                .min();
            let mut ticket_changed = false;
            let mut relevant_event = false;
            let received = match deadline {
                Some(deadline) => receiver.recv_timeout(Duration::from_secs(u64::try_from(
                    (deadline - current).max(0),
                )?)),
                None => receiver
                    .recv()
                    .map_err(|_| mpsc::RecvTimeoutError::Disconnected),
            };
            match received {
                Ok(Ok(event))
                    if event.need_rescan()
                        || matches!(
                            event.kind,
                            notify::EventKind::Create(_)
                                | notify::EventKind::Modify(_)
                                | notify::EventKind::Remove(_)
                        ) =>
                {
                    relevant_event = event.need_rescan()
                        || event.paths.iter().any(|p| {
                            p.starts_with(&tickets)
                                || p == &registry_file
                                || p == &providers_file
                                || p.file_name().is_some_and(|n| n == "state.json")
                        });
                    ticket_changed = event.need_rescan()
                        || event.paths.iter().any(|p| {
                            p.starts_with(&tickets) || p == &registry_file || p == &providers_file
                        });
                }
                Ok(Ok(_)) => {}
                Ok(Err(error)) => bail!(
                    "coordination watch failed; reconnect and reconcile durable state: {error}"
                ),
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("coordination watch disconnected; reconnect")
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
            }
            let current = now();
            if registered && current >= renew_at {
                self.register(
                    request.role,
                    &request.id,
                    &request.generation,
                    current,
                    request.lease_seconds,
                    request.return_route.clone(),
                )?;
                renew_at = current + request.lease_seconds / 2;
            }
            let claim_expired = claim_expiry.is_some_and(|at| current >= at);
            if !relevant_event && !claim_expired && peer_expiry.is_none_or(|at| current < at) {
                continue;
            }
            let mut state = self.state()?;
            if !registered && Self::require_dispatcher_authority(&state).is_ok() {
                #[cfg(test)]
                if let Some(hook) = self
                    .before_dispatcher_register
                    .lock()
                    .expect("test hook lock")
                    .take()
                {
                    hook();
                }
                // Atomic: a concurrent return to integrator authority leaves this waiter
                // unregistered and waiting, rather than ending it with a refusal.
                registered = self.try_register(
                    request.role,
                    &request.id,
                    &request.generation,
                    current,
                    request.lease_seconds,
                    request.return_route.clone(),
                )?;
                #[cfg(test)]
                if let Some(hook) = self
                    .after_dispatcher_register
                    .lock()
                    .expect("test hook lock")
                    .take()
                {
                    hook();
                }
                renew_at = current + request.lease_seconds / 2;
                state = self.state()?;
            }
            // A lease extension changes no semantic state. In particular, it must not
            // cause a timer-driven queue scan in this or another waiting process.
            let next_cursor = Self::state_cursor(&state, current)?;
            if next_cursor != state_cursor
                || (request.role != Role::Worker && (ticket_changed || claim_expired))
            {
                snapshot = self.snapshot(current)?;
                snapshot_checked_at = current;
                state_cursor = next_cursor;
                if (registered || snapshot.state.stopped.is_some()) && ready(request, &snapshot) {
                    return Ok(snapshot);
                }
            } else {
                // Elapsed peer deadlines were reconciled; never reuse them as a wake.
                snapshot.state = state;
                snapshot_checked_at = current;
            }
        }
    }
}

// Workers wait for their mailbox, not queue eligibility. Scheduling an unrelated
// claim deadline without refreshing queue facts would leave an elapsed zero timeout.
pub(super) fn claim_deadline(role: Role, snapshot: &Snapshot, checked_at: i64) -> Option<i64> {
    if role == Role::Worker {
        return None;
    }
    snapshot
        .live_claims
        .iter()
        .map(|t| t.expires_at)
        .filter(|at| *at > checked_at)
        .min()
}

fn ready(request: &WaitRequest, snapshot: &Snapshot) -> bool {
    if snapshot.state.stopped.is_some() {
        return true;
    }
    match request.role {
        Role::Worker => snapshot
            .state
            .active_assignment(&request.id)
            .is_some_and(|a| a.generation == request.generation),
        Role::Integrator | Role::Dispatcher => request
            .after
            .as_ref()
            .is_none_or(|after| after != &snapshot.cursor),
    }
}
