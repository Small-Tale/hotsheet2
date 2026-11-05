//! Headless batch integration commands (HS2-9G0M3V). Git and ticket-store facts are read
//! here; the fenced transitions live in `hotsheet_cli::coordination::batch`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Result, bail, ensure};
use hotsheet_cli::coordination::batch::{
    BatchCandidate, BatchFailure, priority_rank, select_batch,
};
use hotsheet_cli::coordination::{
    Assignment, Coordinator, HandoffEvidence, Phase, StrategyPolicy, TransitionKind,
};
use hotsheet_ticketing::{FsStore, TicketRef, git_connection_id};
use serde::Deserialize;

use super::{CoordinationCmd, Ctx, git_output, require_current_main, require_pushed_main};

pub(super) fn run(
    coordinator: &Coordinator,
    ctx: &Ctx,
    project: &Path,
    command: CoordinationCmd,
    now: i64,
) -> Result<serde_json::Value> {
    let limits = || -> Result<_> {
        Ok(
            StrategyPolicy::read(&hotsheet_ticketing::Settings::for_project(project))?
                .circuit_breaker(),
        )
    };
    Ok(match command {
        CoordinationCmd::BatchPlan { max } => {
            let candidates = candidates(coordinator, ctx, project, now)?;
            let plan = select_batch(candidates, max);
            serde_json::to_value(plan)?
        }
        CoordinationCmd::BatchOpen {
            integrator,
            batch,
            assignments,
        } => {
            let state = coordinator.state()?;
            let mut members = Vec::with_capacity(assignments.len());
            for id in assignments {
                let a = state
                    .assignments
                    .get(&id)
                    .ok_or_else(|| anyhow::anyhow!("unknown assignment {id}"))?;
                members.push((id.clone(), changed_paths(project, a)?));
            }
            serde_json::to_value(coordinator.batch_open(
                &integrator.id,
                &integrator.session,
                now,
                &batch,
                members,
                limits()?,
            )?)?
        }
        CoordinationCmd::BatchAbort {
            integrator,
            batch,
            reason,
        } => serde_json::to_value(coordinator.batch_abort(
            &integrator.id,
            &integrator.session,
            now,
            &batch,
            &reason,
        )?)?,
        CoordinationCmd::BatchPushed {
            integrator,
            batch,
            main_commit,
            shipping,
            smoke_checks,
        } => {
            require_current_main(project, &main_commit)?;
            require_pushed_main(project, None)?;
            let mut mapped: BTreeMap<String, Vec<String>> = BTreeMap::new();
            for raw in shipping {
                let (assignment, sha) = raw.split_once('=').ok_or_else(|| {
                    anyhow::anyhow!("shipping must be <assignment>=<full sha>, got {raw}")
                })?;
                let ancestry =
                    git_output(project, &["merge-base", "--is-ancestor", sha, &main_commit])?;
                ensure!(
                    ancestry.status.success(),
                    "shipping commit {sha} is not contained in the batch main commit"
                );
                mapped
                    .entry(assignment.to_owned())
                    .or_default()
                    .push(sha.to_owned());
            }
            serde_json::to_value(coordinator.batch_pushed(
                &integrator.id,
                &integrator.session,
                now,
                &batch,
                &main_commit,
                mapped,
                smoke_checks,
            )?)?
        }
        CoordinationCmd::BatchResult {
            integrator,
            batch,
            run,
            conclusion,
        } => serde_json::to_value(coordinator.batch_result(
            &integrator.id,
            &integrator.session,
            now,
            &batch,
            &run,
            conclusion,
            limits()?,
        )?)?,
        CoordinationCmd::BatchFollowUps {
            integrator,
            batch,
            failures,
            attribution_file,
        } => {
            let mut recorded = Vec::new();
            for raw in failures {
                let (key, follow_up) = raw.split_once('=').ok_or_else(|| {
                    anyhow::anyhow!("failure must be <key>=<follow-up slug>, got {raw}")
                })?;
                recorded.push(BatchFailure {
                    key: key.to_owned(),
                    follow_up: follow_up.to_owned(),
                    follow_up_ticket: None,
                    suspects: Vec::new(),
                });
            }
            if let Some(path) = attribution_file {
                recorded.extend(attributed_failures(&std::fs::read(path)?)?);
            }
            for failure in &mut recorded {
                failure.follow_up_ticket = Some(follow_up_ticket(ctx, &failure.follow_up)?);
            }
            serde_json::to_value(coordinator.batch_follow_ups(
                &integrator.id,
                &integrator.session,
                now,
                &batch,
                recorded,
            )?)?
        }
        CoordinationCmd::BatchRecover {
            integrator,
            run,
            main_commit,
        } => {
            let ledger = coordinator.batch_ledger()?.unwrap_or_default();
            if let Some(red) = &ledger.breaker.last_red_main
                && ledger.breaker.tripped.is_some()
            {
                let ancestry =
                    git_output(project, &["merge-base", "--is-ancestor", red, &main_commit])?;
                ensure!(
                    ancestry.status.success(),
                    "the green run's commit {main_commit} does not contain the last red batch push {red}"
                );
            }
            serde_json::to_value(coordinator.batch_recover(
                &integrator.id,
                &integrator.session,
                now,
                &run,
                &main_commit,
            )?)?
        }
        CoordinationCmd::BatchStatus => {
            serde_json::to_value(coordinator.batch_ledger()?.unwrap_or_default())?
        }
        _ => unreachable!("only batch commands are routed here"),
    })
}

/// Ready code handoffs with the facts batch selection needs.
fn candidates(
    coordinator: &Coordinator,
    ctx: &Ctx,
    project: &Path,
    now: i64,
) -> Result<Vec<BatchCandidate>> {
    let snapshot = coordinator.snapshot(now)?;
    let store = ctx
        .path
        .join(hotsheet_ticketing::STORE_METADATA_FILE)
        .exists()
        .then(|| FsStore::open_without_maintenance(&ctx.path))
        .transpose()?;
    let connection = store.as_ref().map(git_connection_id);
    let mut candidates = Vec::new();
    for a in snapshot.state.assignments.values() {
        if a.phase != Phase::Handoff
            || a.returned_for_review()
            || !matches!(a.handoff, Some(HandoffEvidence::Code { .. }))
        {
            continue;
        }
        let mut blocker = None;
        let mut rank = priority_rank(hotsheet_model::Priority::Default);
        match (&store, &connection) {
            (Some(store), Some(connection)) if *connection == a.ticket.connection_id => {
                let ticket = store.read_ticket(&a.ticket.native_id.parse()?)?;
                rank = priority_rank(ticket.priority);
            }
            _ => {
                blocker = Some(format!(
                    "ticket source {} is not the selected local store",
                    a.ticket.connection_id
                ));
            }
        }
        if snapshot
            .live_claims
            .iter()
            .any(|claim| claim.ticket == a.ticket)
        {
            blocker = Some("its ticket claim is still live".into());
        }
        let paths = match changed_paths(project, a) {
            Ok(paths) => paths,
            Err(error) => {
                blocker.get_or_insert(format!("its commit cannot be inspected: {error}"));
                Vec::new()
            }
        };
        candidates.push(BatchCandidate {
            assignment: a.id.clone(),
            ticket: a.ticket.clone(),
            slug: a.slug.clone(),
            priority_rank: rank,
            handoff_at: handoff_at(a),
            paths,
            blocker,
        });
    }
    Ok(candidates)
}

fn handoff_at(a: &Assignment) -> String {
    a.transitions
        .iter()
        .rev()
        .find(|t| t.kind == TransitionKind::Handoff)
        .map(|t| t.at.clone())
        .unwrap_or_default()
}

/// The declared scope plus every path the handoff commit changes since its base.
fn changed_paths(project: &Path, a: &Assignment) -> Result<Vec<String>> {
    let Some(HandoffEvidence::Code { commit }) = &a.handoff else {
        bail!("assignment {} is not a code handoff", a.id);
    };
    let diff = git_output(
        project,
        &["diff", "--name-only", "-z", "--no-renames", &a.base, commit],
    )?;
    ensure!(
        diff.status.success(),
        "could not diff {}..{commit}: {}",
        a.base,
        String::from_utf8_lossy(&diff.stderr).trim()
    );
    let mut paths: Vec<String> = String::from_utf8(diff.stdout)?
        .split('\0')
        .filter(|p| !p.is_empty())
        .map(str::to_owned)
        .chain(a.scope.iter().cloned())
        .collect();
    paths.sort();
    paths.dedup();
    Ok(paths)
}

#[derive(Deserialize)]
struct AttributionOutput {
    filed: Vec<FiledFailure>,
}

#[derive(Deserialize)]
struct FiledFailure {
    key: String,
    #[serde(default)]
    kind: Option<String>,
    #[serde(default)]
    slug: Option<String>,
    #[serde(default)]
    links: Vec<String>,
}

/// The non-flaky failures an HS2-HAHX20 attribution run filed. A flaky failure is not caused
/// by the batch; it gets its own flake bug and no batch debt.
fn attributed_failures(bytes: &[u8]) -> Result<Vec<BatchFailure>> {
    let output: AttributionOutput = serde_json::from_slice(bytes)
        .map_err(|error| anyhow::anyhow!("invalid attribution output: {error}"))?;
    let mut failures = Vec::new();
    for filed in output.filed {
        if filed.kind.as_deref() == Some("flaky") {
            continue;
        }
        let Some(slug) = filed.slug else {
            bail!(
                "attribution failure {} has no filed follow-up; rerun the attribution",
                filed.key
            );
        };
        failures.push(BatchFailure {
            key: filed.key,
            follow_up: slug,
            follow_up_ticket: None,
            suspects: filed.links,
        });
    }
    Ok(failures)
}

fn follow_up_ticket(ctx: &Ctx, slug: &str) -> Result<TicketRef> {
    ensure!(
        ctx.path
            .join(hotsheet_ticketing::STORE_METADATA_FILE)
            .exists(),
        "follow-up verification needs the selected local ticket store"
    );
    let store = FsStore::open_without_maintenance(&ctx.path)?;
    let ticket = store
        .find_ticket_by_slug(slug)?
        .ok_or_else(|| anyhow::anyhow!("follow-up {slug} is not in the selected ticket store"))?;
    Ok(TicketRef {
        connection_id: git_connection_id(&store),
        native_id: ticket.id.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_9g0m3v_attribution_output_maps_failures_and_skips_flaky_entries() {
        let failures = attributed_failures(
            br#"{"filed":[
                {"key":"k1","kind":"failure","slug":"HS2-AAA111","links":["HS2-X"],"action":"created"},
                {"key":"k2","kind":"flaky","slug":"HS2-BBB222","links":[]}
            ]}"#,
        )
        .unwrap();
        assert_eq!(failures.len(), 1);
        assert_eq!(failures[0].key, "k1");
        assert_eq!(failures[0].follow_up, "HS2-AAA111");
        assert_eq!(failures[0].suspects, ["HS2-X"]);
        assert!(
            attributed_failures(br#"{"filed":[{"key":"k3","kind":"failure"}]}"#)
                .unwrap_err()
                .to_string()
                .contains("no filed follow-up")
        );
        assert!(attributed_failures(b"{}").is_err());
    }
}
