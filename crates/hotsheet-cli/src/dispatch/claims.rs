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
