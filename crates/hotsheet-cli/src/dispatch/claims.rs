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
    let now = OffsetDateTime::now_utc();
    ensure!(
        now < expiry,
        "claim lease deadline passed while renewal waited"
    );
    let until = (expiry - now).whole_milliseconds().max(1) as u64;
    let ticket = receiver
        .recv_timeout(StdDuration::from_millis(until))
        .context("claim lease deadline passed while renewal waited")??;
    ensure!(
        OffsetDateTime::now_utc() < lease_deadline(&ticket)?,
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
