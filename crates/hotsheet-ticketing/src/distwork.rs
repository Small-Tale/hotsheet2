//! Server-hosted **distributed work loop** (`docs/08` §8.5, HS2-E7RXXR) — the git-native
//! self-claim fan-out that lets many independent workers (servers, CLIs) drain a shared
//! project **without any coordinator or server-to-server protocol**. Coordination is the
//! git ref CAS from [`crate::distclaim`]; this layer picks *which* ticket to attempt and
//! turns a won CAS into a real local claim.
//!
//! [`select_and_claim`] is the primitive: pick the next candidate (open, unblocked,
//! Up-Next-first — the same order as the local [`ops::claim_next`]) and try to claim it on
//! the remote; if the CAS is **lost** (another machine got it), move to the next candidate.
//! Because the remote arbitrates, two workers scanning the same queue never take the same
//! ticket — the property a single machine's in-process claim can't give across machines.

use hotsheet_model::{Timestamp, Ulid};

use crate::distclaim::{self, ClaimMarker, ClaimResult, DistError};
use crate::ops::{self, priority_rank};
use crate::store::{FsStore, StoreError};

fn store_err(e: StoreError) -> DistError {
    DistError::Store(e.to_string())
}

/// What driving a claimed ticket produced — decides whether to keep the lease or drop it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkOutcome {
    /// The ticket's work is finished — release the claim marker.
    Completed,
    /// More turns to go — renew the lease and keep it.
    Continued,
    /// The turn failed — release so another worker can retry (poison-count guards spins).
    Failed,
}

/// Self-claim the next available ticket for `worker` over the shared remote (a lease of
/// `lease_minutes`). Scans candidates in worklist order and attempts the distributed CAS on
/// each; the first **won** claim is recorded locally (claim frontmatter) and its marker
/// returned. `Ok(None)` when nothing is claimable (queue drained, or every candidate lost
/// the race).
pub fn select_and_claim(
    store: &FsStore,
    worker: &str,
    lease_minutes: i64,
    now: &Timestamp,
) -> Result<Option<ClaimMarker>, DistError> {
    let expires = now.plus_minutes(lease_minutes);
    for id in candidates(store, now)? {
        match distclaim::claim(store.root(), &id, worker, expires.clone(), now)? {
            ClaimResult::Won(marker) => {
                // The distributed CAS is ours — reflect the claim in the ticket file too, so
                // the local view (and the index) shows the holder + lease.
                record_local_claim(store, &id, worker, &expires, now)?;
                return Ok(Some(marker));
            }
            ClaimResult::Lost => continue, // another worker holds it — try the next
        }
    }
    Ok(None)
}

/// One distributed work cycle: self-claim the next ticket, drive it (the injected `drive`
/// does the actual AI turn — `live::run_trigger` in production, a fake in tests), then keep
/// or drop the lease per the outcome. `Ok(None)` when the queue is drained. Renewing on
/// `Continued` uses the marker we just took (force-with-lease), so a lease we already lost
/// isn't clobbered.
pub fn work_once<F>(
    store: &FsStore,
    worker: &str,
    lease_minutes: i64,
    now: &Timestamp,
    drive: F,
) -> Result<Option<(Ulid, WorkOutcome)>, DistError>
where
    F: FnOnce(&Ulid) -> WorkOutcome,
{
    let Some(marker) = select_and_claim(store, worker, lease_minutes, now)? else {
        return Ok(None);
    };
    let id = marker.id.expect("a won marker carries its id");
    let outcome = drive(&id);
    match outcome {
        WorkOutcome::Continued => {
            let expires = now.plus_minutes(lease_minutes);
            let _ = distclaim::renew(store.root(), &id, worker, &marker.oid, expires, now)?;
        }
        WorkOutcome::Completed | WorkOutcome::Failed => {
            distclaim::release(store.root(), &id)?;
            // Drop the local claim too when we're done with it (Failed leaves it claimable).
            let _ = crate::ops::release(store, &id, now.clone(), worker, true);
        }
    }
    Ok(Some((id, outcome)))
}

/// The candidate ticket ids, in the same order [`ops::claim_next`] prefers: open + unblocked
/// + locally-claimable, Up Next first, then priority, then creation order.
fn candidates(store: &FsStore, now: &Timestamp) -> Result<Vec<Ulid>, DistError> {
    let tickets = store.list_tickets().map_err(store_err)?;
    let done: std::collections::HashSet<Ulid> = tickets
        .iter()
        .filter(|t| ops::is_done(t))
        .map(|t| t.id)
        .collect();

    let mut c: Vec<_> = tickets
        .into_iter()
        .filter(|t| ops::is_open(t) && !ops::is_blocked(t, &done) && ops::claim_available(t, now))
        .collect();
    c.sort_by(|a, b| {
        b.up_next
            .cmp(&a.up_next)
            .then(priority_rank(a.priority).cmp(&priority_rank(b.priority)))
            .then(a.id.cmp(&b.id))
    });
    Ok(c.into_iter().map(|t| t.id).collect())
}

/// Write the claim frontmatter for a ticket we just won on the remote.
fn record_local_claim(
    store: &FsStore,
    id: &Ulid,
    worker: &str,
    expires: &Timestamp,
    now: &Timestamp,
) -> Result<(), DistError> {
    let mut t = store.read_ticket(id).map_err(store_err)?;
    t.claimed_by = Some(worker.to_string());
    t.claim_lease_expires_at = Some(expires.clone());
    t.claim_count += 1;
    t.updated_at = now.clone();
    store.write_ticket_committing(&t).map_err(store_err)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ops::{NewTicket, create};
    use crate::store::StoreMetadata;

    fn ts(s: &str) -> Timestamp {
        Timestamp::new(format!("2026-08-22T00:00:{s}Z"))
    }

    /// A bare remote + two clones that are each Hot Sheet stores holding the SAME two Up
    /// Next tickets (so both workers see the same queue and race over it).
    fn two_workers_sharing_a_queue() -> (tempfile::TempDir, FsStore, FsStore, Ulid, Ulid) {
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("remote.git");
        let base = dir.path().join("base");
        let run = |args: &[&str]| {
            assert!(
                std::process::Command::new("git")
                    .args(args)
                    .status()
                    .unwrap()
                    .success(),
                "git {args:?}"
            );
        };
        run(&["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
        run(&["init", "-q", "-b", "main", base.to_str().unwrap()]);
        let base_store = FsStore::init(&base, &StoreMetadata::new("HS")).unwrap();

        let x = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap();
        let y = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB1").unwrap();
        for id in [x, y] {
            create(
                &base_store,
                id,
                "HS",
                ts("00"),
                NewTicket {
                    title: format!("t-{id}"),
                    up_next: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        run(&[
            "-C",
            base.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            bare.to_str().unwrap(),
        ]);
        run(&[
            "-C",
            base.to_str().unwrap(),
            "push",
            "-q",
            "-u",
            "origin",
            "main",
        ]);

        let a = dir.path().join("a");
        let b = dir.path().join("b");
        run(&["clone", "-q", bare.to_str().unwrap(), a.to_str().unwrap()]);
        run(&["clone", "-q", bare.to_str().unwrap(), b.to_str().unwrap()]);
        (
            dir,
            FsStore::open(&a).unwrap(),
            FsStore::open(&b).unwrap(),
            x,
            y,
        )
    }

    #[test]
    fn two_workers_never_take_the_same_ticket() {
        let (_d, a, b, x, y) = two_workers_sharing_a_queue();
        let now = ts("10");

        // Worker A claims the first candidate.
        let a_got = select_and_claim(&a, "worker-a", 30, &now)
            .unwrap()
            .unwrap()
            .id
            .unwrap();
        // Worker B, scanning the SAME local queue, tries A's ticket first, loses the CAS,
        // and falls through to the other one.
        let b_got = select_and_claim(&b, "worker-b", 30, &now)
            .unwrap()
            .unwrap()
            .id
            .unwrap();

        assert_ne!(a_got, b_got, "the remote CAS prevents a double-claim");
        assert!([x, y].contains(&a_got) && [x, y].contains(&b_got));
        // A's win is recorded locally (the ticket file shows the holder).
        assert_eq!(
            a.read_ticket(&a_got).unwrap().claimed_by.as_deref(),
            Some("worker-a")
        );

        // The queue is now exhausted for a third worker.
        assert!(
            select_and_claim(&a, "worker-c", 30, &now)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn work_once_completed_releases_the_marker_continued_keeps_it() {
        let (_d, a, _b, _x, _y) = two_workers_sharing_a_queue();
        let now = ts("10");

        // A Continued turn keeps the claim (marker still on the remote).
        let (id1, out1) = work_once(&a, "w", 30, &now, |_id| WorkOutcome::Continued)
            .unwrap()
            .unwrap();
        assert_eq!(out1, WorkOutcome::Continued);
        let claims = distclaim::list_claims(a.root()).unwrap();
        assert!(
            claims.iter().any(|c| c.id == Some(id1)),
            "Continued keeps the marker"
        );

        // A Completed turn on the next ticket releases its marker. A real drive marks the
        // ticket done; the fake does the same so it leaves the queue.
        let drive_complete = |id: &Ulid| {
            crate::ops::update(
                &a,
                id,
                ts("11"),
                crate::ops::TicketPatch {
                    status: Some(hotsheet_model::Status::Completed),
                    ..Default::default()
                },
            )
            .unwrap();
            WorkOutcome::Completed
        };
        let (id2, out2) = work_once(&a, "w", 30, &now, drive_complete)
            .unwrap()
            .unwrap();
        assert_eq!(out2, WorkOutcome::Completed);
        assert_ne!(id1, id2);
        let claims = distclaim::list_claims(a.root()).unwrap();
        assert!(
            !claims.iter().any(|c| c.id == Some(id2)),
            "Completed released the marker"
        );

        // The drive callback is skipped when nothing is claimable.
        let mut driven = false;
        let none = work_once(&a, "w", 30, &now, |_id| {
            driven = true;
            WorkOutcome::Completed
        })
        .unwrap();
        assert!(none.is_none() && !driven, "no claim → no drive");
    }

    #[test]
    fn no_remote_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        std::process::Command::new("git")
            .args(["init", "-q", dir.path().to_str().unwrap()])
            .status()
            .unwrap();
        create(
            &store,
            Ulid::new(),
            "HS",
            ts("00"),
            NewTicket {
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert!(matches!(
            select_and_claim(&store, "w", 30, &ts("10")),
            Err(DistError::NoRemote)
        ));
    }
}
