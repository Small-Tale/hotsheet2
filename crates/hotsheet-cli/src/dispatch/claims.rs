//! Live ticket claim leases: claim, claim-next, renew, and release.

use anyhow::Result;

use super::{Ctx, misrouted};
use crate::*;

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
        _ => misrouted("claims"),
    }
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
        Some(ticket) => println!(
            "Claimed {} for {worker} (lease {lease_minutes}m{})",
            ticket.slug,
            eta_suffix(&ticket)
        ),
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
    Ok(())
}

pub(crate) fn cmd_release(path: &PathBuf, id: &str, worker: &str, force: bool) -> Result<()> {
    let store = FsStore::open(path)?;
    let ticket = resolve(&store, id)?;
    let released = ops::release(&store, &ticket.id, now_ts(), worker, force)?;
    println!("Released {}", released.slug);
    Ok(())
}

pub(crate) fn cmd_release_worker(path: &PathBuf, worker: &str) -> Result<()> {
    let store = FsStore::open(path)?;
    let released = ops::release_worker(&store, now_ts(), worker)?;
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
    println!(
        "Renewed {} (lease {lease_minutes}m{})",
        renewed.slug,
        eta_suffix(&renewed)
    );
    // Prompt a re-estimate at the natural renewal touchpoint (HS2-DQQ0AX).
    if ops::claim_eta_expired(&renewed, &now) {
        println!("ETA passed; pass --eta with a new estimate.");
    }
    Ok(())
}
