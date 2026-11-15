//! Live ticket claim leases: claim, claim-next, renew, and release.

use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, bail, ensure};
use hotsheet_model::{ClaimEventKind, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::{FsStore, metrics, ops};
use time::{Duration, OffsetDateTime};

use crate::{Cmd, now_ts, resolve};

use super::claim_process::GuardedChild;
use super::{Ctx, misrouted};

pub(super) fn run(cmd: Cmd, ctx: &Ctx) -> Result<()> {
    let path = &ctx.path;
    match cmd {
        Cmd::ClaimNext {
            worker,
            label,
            lease_minutes,
            eta,
        } => cmd_claim_next(path, &worker, label, lease_minutes, eta.as_deref()),
        Cmd::Claim {
            id,
            worker,
            label,
            lease_minutes,
            start,
            eta,
        } => cmd_claim(
            path,
            &id,
            &worker,
            label,
            lease_minutes,
            start,
            eta.as_deref(),
        ),
        Cmd::Release {
            id,
            worker,
            force,
            all,
        } => match id {
            Some(id) if !all => cmd_release(path, &id, &worker, force),
            _ => cmd_release_worker(path, &worker),
        },
        Cmd::Renew {
            id,
            worker,
            lease_minutes,
            eta,
        } => cmd_renew(path, &id, &worker, lease_minutes, eta.as_deref()),
        Cmd::ClaimRun {
            id,
            worker,
            lease_seconds,
            command,
        } => cmd_claim_run(path, &id, &worker, lease_seconds, &command),
        #[cfg(windows)]
        Cmd::ClaimRunChild { command } => super::claim_process::run_child(&command),
        _ => misrouted("claims"),
    }
}

/// Guard one owned process tree with the exact live claim attempt (HS2-MFYYJ9).
fn cmd_claim_run(
    path: &PathBuf,
    id: &str,
    worker: &str,
    lease_seconds: u64,
    command: &[OsString],
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let now = Timestamp::from_datetime(OffsetDateTime::now_utc());
    ensure!(
        ticket.claimed_by.as_deref() == Some(worker) && !ops::claim_available(&ticket, &now),
        "exact live ticket claim required before running a command"
    );
    let attempt = ticket
        .claim_history
        .iter()
        .rev()
        .find(|event| event.kind == ClaimEventKind::Claim)
        .filter(|event| event.worker == worker)
        .map(|event| event.id)
        .context(
            "live claim has no exact attempt event; inspect and reacquire before continuing",
        )?;
    // Preflight can wait for the lock: no requested command has started yet.
    let mut held = renew_attempt(&store, &ticket.id, worker, &attempt, lease_seconds)
        .context("claim guard preflight renewal failed")?;
    let mut child = GuardedChild::spawn(command)?;
    // HS2-MFYYJ9 native Windows fixture: let the helper open its event only
    // after this call has returned, reproducing the event lifetime race.
    #[cfg(debug_assertions)]
    if let Ok(marker) = std::env::var("HOTSHEET_TEST_CLAIM_RUN_PARENT_READY_PATH") {
        std::fs::write(marker, b"spawn returned")
            .context("could not mark guarded parent spawn return")?;
    }
    let margin = Duration::seconds((lease_seconds / 3).clamp(1, 60) as i64);
    let mut pending: Option<Receiver<Result<Ticket, ops::OpError>>> = None;
    loop {
        if let Some(receiver) = pending.as_ref() {
            match receiver.try_recv() {
                Ok(result) => {
                    pending = None;
                    held = match result {
                        Ok(renewed) => renewed,
                        Err(error) => {
                            child
                                .stop()
                                .context("could not stop guarded process tree")?;
                            return Err(error).context("claim guard renewal failed; tree stopped");
                        }
                    };
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    child
                        .stop()
                        .context("could not stop guarded process tree")?;
                    bail!("claim guard renewal worker ended without a result; tree stopped");
                }
            }
        }
        let now = OffsetDateTime::now_utc();
        let expiry = lease_deadline(&held)?;
        if now >= expiry {
            child
                .stop()
                .context("could not stop guarded process tree")?;
            bail!("claim lease deadline passed before renewal completed; tree stopped");
        }
        match child.try_wait() {
            Ok(Some(status)) => {
                // A shell can exit while a grandchild still retains stdio and writes.
                // Stop that tree before any blocking final authority check.
                child
                    .stop()
                    .context("could not stop guarded process tree")?;
                if !status.success() {
                    bail!("guarded command failed with {status}; claim authority is not inferred");
                }
                if let Some(receiver) = pending.take() {
                    held = await_renewal(receiver, expiry)
                        .context("scheduled renewal did not complete before lease deadline")?;
                }
                let final_check = spawn_renew(&store, ticket.id, worker, attempt, lease_seconds);
                await_renewal(final_check, lease_deadline(&held)?)
                    .context("claim guard final authority check failed")?;
                return Ok(());
            }
            Ok(None) => {}
            Err(error) => {
                child
                    .stop()
                    .context("could not stop guarded process tree")?;
                return Err(error).context("could not observe guarded command");
            }
        }
        let renew_at = expiry - margin;
        if now >= renew_at && pending.is_none() {
            pending = Some(spawn_renew(
                &store,
                ticket.id,
                worker,
                attempt,
                lease_seconds,
            ));
        }
        let wake_at = if now < renew_at { renew_at } else { expiry };
        let until = (wake_at - now).whole_milliseconds().max(1) as u64;
        thread::sleep(StdDuration::from_millis(until.min(250)));
    }
}

fn lease_deadline(ticket: &Ticket) -> Result<OffsetDateTime> {
    ticket
        .claim_lease_expires_at
        .as_ref()
        .and_then(Timestamp::instant)
        .context("renewal did not return a valid lease deadline")
}

fn renew_attempt(
    store: &FsStore,
    id: &Ulid,
    worker: &str,
    attempt: &Ulid,
    lease_seconds: u64,
) -> Result<Ticket, ops::OpError> {
    ops::renew_live_attempt_with_clock(store, id, worker, attempt, || {
        let now = OffsetDateTime::now_utc();
        (
            Timestamp::from_datetime(now),
            Timestamp::from_datetime(now + Duration::seconds(lease_seconds as i64)),
        )
    })
}

fn spawn_renew(
    store: &FsStore,
    id: Ulid,
    worker: &str,
    attempt: Ulid,
    lease_seconds: u64,
) -> Receiver<Result<Ticket, ops::OpError>> {
    let (sender, receiver) = mpsc::channel();
    let store = store.clone();
    let worker = worker.to_owned();
    thread::spawn(move || {
        let _ = sender.send(renew_attempt(&store, &id, &worker, &attempt, lease_seconds));
    });
    receiver
}

fn await_renewal(
    receiver: Receiver<Result<Ticket, ops::OpError>>,
    expiry: OffsetDateTime,
) -> Result<Ticket> {
    await_renewal_with_clock(&receiver, expiry, OffsetDateTime::now_utc)
}

/// Wait for one renewal result until the previous lease deadline `expiry`.
///
/// A result already delivered is read before `expiry` applies (HS2-DG53Y3).
/// The store samples its clock under the ticket lock and refuses to renew an
/// expired lease, so a delivered `Ok` proves the lease stayed live; rejecting
/// it because the guard itself observed the old deadline late (a host stall
/// after the command exited) would report a confirmed renewal as missed. A
/// renewal that has not been delivered by `expiry` still fails, and every
/// accepted renewal must still hold a lease that ends in the future.
fn await_renewal_with_clock(
    receiver: &Receiver<Result<Ticket, ops::OpError>>,
    expiry: OffsetDateTime,
    now: impl Fn() -> OffsetDateTime,
) -> Result<Ticket> {
    let result = match receiver.try_recv() {
        Ok(result) => result,
        Err(TryRecvError::Disconnected) => {
            bail!("claim guard renewal worker ended without a result")
        }
        Err(TryRecvError::Empty) => {
            let current = now();
            ensure!(
                current < expiry,
                "claim lease deadline passed while renewal waited"
            );
            let until = (expiry - current).whole_milliseconds().max(1) as u64;
            receiver
                .recv_timeout(StdDuration::from_millis(until))
                .context("claim lease deadline passed while renewal waited")?
        }
    };
    let ticket = result?;
    ensure!(
        now() < lease_deadline(&ticket)?,
        "renewal completed with an expired lease"
    );
    Ok(ticket)
}

pub(crate) fn cmd_claim_next(
    path: &PathBuf,
    worker: &str,
    label: Option<String>,
    lease_minutes: i64,
    eta: Option<&str>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let now_dt = OffsetDateTime::now_utc();
    let now = Timestamp::from_datetime(now_dt);
    let lease = lease_until(now_dt, lease_minutes);
    let eta = eta.map(|raw| ops::parse_claim_eta(&now, raw)).transpose()?;
    match ops::claim_next_with_eta(&store, &now, lease, worker, label, eta)? {
        Some(ticket) => {
            println!(
                "Claimed {} for {worker} (lease {lease_minutes}m{})",
                ticket.slug,
                eta_suffix(&ticket)
            );
            println!("{}", phase_hint(&ticket));
        }
        None => println!("No claimable tickets."),
    }
    Ok(())
}

pub(crate) fn cmd_claim(
    path: &PathBuf,
    id: &str,
    worker: &str,
    label: Option<String>,
    lease_minutes: i64,
    start: bool,
    eta: Option<&str>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let now_dt = OffsetDateTime::now_utc();
    let now = Timestamp::from_datetime(now_dt);
    let lease = lease_until(now_dt, lease_minutes);
    let _ = start;
    let eta = eta.map(|raw| ops::parse_claim_eta(&now, raw)).transpose()?;
    let claimed = ops::claim_with_eta(&store, &ticket.id, &now, lease, worker, label, eta)?;
    println!(
        "Claimed {} for {worker} (lease {lease_minutes}m{})",
        claimed.slug,
        eta_suffix(&claimed)
    );
    println!("{}", phase_hint(&claimed));
    // HS2-BAK4BC: claims take blocked tickets; say so instead of claiming silently.
    if let Some(notice) = ops::blocked_claim_notice(&claimed) {
        println!("{notice}");
    }
    Ok(())
}

pub(crate) fn cmd_release(path: &PathBuf, id: &str, worker: &str, force: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let released = ops::release(&store, &ticket.id, now_ts(), worker, force)?;
    if !force && ticket.claimed_by.as_deref() == Some(worker) {
        if let Err(error) = metrics::codex_work::capture_current_session(&store, worker) {
            eprintln!("Codex Work usage capture failed: {error}");
        }
    }
    println!("Released {}", released.slug);
    Ok(())
}

pub(crate) fn cmd_release_worker(path: &PathBuf, worker: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let released = ops::release_worker(&store, now_ts(), worker)?;
    if !released.is_empty() {
        if let Err(error) = metrics::codex_work::capture_current_session(&store, worker) {
            eprintln!("Codex Work usage capture failed: {error}");
        }
    }
    if released.is_empty() {
        println!("{worker} holds no claims");
    }
    for ticket in released {
        println!("Released {}", ticket.slug);
    }
    Ok(())
}

pub(crate) fn cmd_renew(
    path: &PathBuf,
    id: &str,
    worker: &str,
    lease_minutes: i64,
    eta: Option<&str>,
) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let now_dt = OffsetDateTime::now_utc();
    let now = Timestamp::from_datetime(now_dt);
    let lease = lease_until(now_dt, lease_minutes);
    let eta = eta.map(|raw| ops::parse_claim_eta(&now, raw)).transpose()?;
    let renewed = ops::renew_with_eta(&store, &ticket.id, now.clone(), lease, worker, eta)?;
    if let Err(error) = metrics::codex_work::capture_current_session(&store, worker) {
        eprintln!("Codex Work usage capture failed: {error}");
    }
    println!(
        "Renewed {} (lease {lease_minutes}m{})",
        renewed.slug,
        eta_suffix(&renewed)
    );
    // Prompt a re-estimate at the natural renewal touchpoint (HS2-DQQ0AX).
    if ops::claim_eta_expired(&renewed, &now) {
        println!("ETA passed; pass --eta with a new estimate.");
    }
    println!("{}", phase_hint(&renewed));
    if let Some(notice) = ops::blocked_claim_notice(&renewed) {
        println!("{notice}");
    }
    Ok(())
}

/// Nudge the worker to keep `started_phase` current at every claim touchpoint
/// (HS2-7P14DC): agents otherwise leave a ticket at `working` while they run tests.
pub(crate) fn phase_hint(ticket: &hotsheet_model::Ticket) -> String {
    let phase = ticket
        .started_phase
        .and_then(|phase| serde_json::to_value(phase).ok())
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_else(|| "unset".to_owned());
    format!(
        "Phase: {phase}. Record each transition as it happens with `edit {} --started-phase <phase>` \
         (set initial_testing before running tests, integrating before merging/pushing, \
         final_testing while waiting on CI).",
        ticket.slug
    )
}

/// The suffix that reports a claim's ETA, if it has one.
pub(crate) fn eta_suffix(ticket: &hotsheet_model::Ticket) -> String {
    ticket
        .claim_eta_at
        .as_ref()
        .map(|eta| format!(", ETA {}", eta.as_str()))
        .unwrap_or_default()
}

pub(crate) fn lease_until(now: OffsetDateTime, minutes: i64) -> Timestamp {
    Timestamp::from_datetime(now + Duration::minutes(minutes))
}

#[cfg(test)]
mod tests {
    //! HS2-DG53Y3: transitions of the guard's renewal wait after the command
    //! exits. A fixed clock and hand-fed channels replace wall-clock windows.
    use super::*;
    use hotsheet_model::derive_slug;

    type Delivery = Result<Ticket, ops::OpError>;

    /// The previous lease deadline every case waits against.
    fn old_deadline() -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(1_800_000_000).expect("fixed instant")
    }

    fn renewed(lease_ends: OffsetDateTime) -> Ticket {
        let id = Ulid::new();
        let created = Timestamp::new("2026-11-16T00:00:00Z");
        let mut ticket = Ticket::new(
            id,
            derive_slug(&id, "HS2"),
            "guarded",
            "task",
            created.clone(),
            created,
        );
        ticket.claim_lease_expires_at = Some(Timestamp::from_datetime(lease_ends));
        ticket
    }

    #[test]
    fn delivered_renewal_is_accepted_after_the_old_deadline_passed() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        let new_deadline = old_deadline() + Duration::seconds(30);
        sender.send(Ok(renewed(new_deadline))).expect("deliver");
        // The guard reaches the wait 5 s after the old deadline (a host stall
        // while it stopped the exited tree). The store already confirmed.
        let late = old_deadline() + Duration::seconds(5);
        let ticket = await_renewal_with_clock(&receiver, old_deadline(), || late)
            .expect("an already-confirmed renewal is not reported as missed");
        assert_eq!(lease_deadline(&ticket).expect("deadline"), new_deadline);
    }

    #[test]
    fn missing_renewal_is_rejected_once_the_old_deadline_passed() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        let late = old_deadline() + Duration::seconds(5);
        let error = await_renewal_with_clock(&receiver, old_deadline(), || late)
            .expect_err("no renewal by the deadline must fail");
        assert!(
            format!("{error:#}").contains("deadline passed while renewal waited"),
            "{error:#}"
        );
        // The worker is still running: its later result is never consulted.
        drop(sender);
    }

    #[test]
    fn renewal_arriving_after_the_deadline_is_rejected() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        // One millisecond of lease remains; nothing is delivered within it.
        let just_before = old_deadline() - Duration::milliseconds(1);
        let error = await_renewal_with_clock(&receiver, old_deadline(), || just_before)
            .expect_err("a renewal that lands after the deadline must fail");
        assert!(
            format!("{error:#}").contains("deadline passed while renewal waited"),
            "{error:#}"
        );
        // It arrives only now, after the guard already gave up.
        sender
            .send(Ok(renewed(old_deadline() + Duration::seconds(30))))
            .expect("late delivery");
    }

    #[test]
    fn renewal_delivered_while_waiting_before_the_deadline_is_accepted() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        let new_deadline = old_deadline() + Duration::seconds(30);
        // A generous remaining lease; the worker delivers during the wait.
        let early = old_deadline() - Duration::seconds(60);
        let worker = thread::spawn(move || {
            sender.send(Ok(renewed(new_deadline))).expect("deliver");
        });
        let ticket = await_renewal_with_clock(&receiver, old_deadline(), || early)
            .expect("a renewal delivered before the deadline is accepted");
        worker.join().expect("worker");
        assert_eq!(lease_deadline(&ticket).expect("deadline"), new_deadline);
    }

    #[test]
    fn delivered_store_refusal_is_rejected() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        sender
            .send(Err(ops::OpError::NotClaimed("HS2-GUARD".into())))
            .expect("deliver");
        let late = old_deadline() + Duration::seconds(5);
        let error = await_renewal_with_clock(&receiver, old_deadline(), || late)
            .expect_err("a store refusal is never authority");
        assert!(format!("{error:#}").contains("HS2-GUARD"), "{error:#}");
    }

    #[test]
    fn delivered_renewal_whose_new_lease_also_expired_is_rejected() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        let new_deadline = old_deadline() + Duration::seconds(3);
        sender.send(Ok(renewed(new_deadline))).expect("deliver");
        let later = new_deadline + Duration::seconds(1);
        let error = await_renewal_with_clock(&receiver, old_deadline(), || later)
            .expect_err("an expired renewed lease proves nothing now");
        assert!(
            format!("{error:#}").contains("renewal completed with an expired lease"),
            "{error:#}"
        );
    }

    #[test]
    fn renewal_worker_ending_without_a_result_is_rejected() {
        let (sender, receiver) = mpsc::channel::<Delivery>();
        drop(sender);
        let late = old_deadline() + Duration::seconds(5);
        let error = await_renewal_with_clock(&receiver, old_deadline(), || late)
            .expect_err("a vanished worker is not authority");
        assert!(
            format!("{error:#}").contains("ended without a result"),
            "{error:#}"
        );
        let early = old_deadline() - Duration::seconds(60);
        let (sender, receiver) = mpsc::channel::<Delivery>();
        let worker = thread::spawn(move || drop(sender));
        await_renewal_with_clock(&receiver, old_deadline(), || early)
            .expect_err("a worker that ends during the wait is not authority");
        worker.join().expect("worker");
    }
}
