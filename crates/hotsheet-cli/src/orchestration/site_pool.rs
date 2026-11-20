//! HS2-5P112Z: one coordination pool per project. The project's home site (its own
//! target, `ProjectSites::home_of`) runs the dispatcher and the integrator; a worker on
//! another site of the project joins the home checkout's pool through the
//! orchestrator, which already reaches every site over the reviewed hosted routes.
//! Site machines never call each other and hold no credential for the home.
//!
//! This is the hosted half, on the home's machine: `GET /checkouts/{checkout}/target-pool`
//! reads the pool's role flags and one member's assignment; `POST` registers or renews
//! that member and syncs its assignment from ticket evidence. A worker accepts by
//! claiming the ticket in the shared store and hands off by pushing a branch to
//! `origin`, releasing its claim and writing an `## Integration handoff` note naming
//! `Branch:` and `Commit:`. The home fetches that branch into
//! `refs/hotsheet/site-handoffs/<assignment>`, checks the commit is on it and inside
//! the assignment's scope, and only then records the mailbox handoff, so its
//! integrator finds the commit locally. The scope reservation stays the pool's own
//! `assign` non-overlap check.
use std::path::Path;

use hotsheet_model::{ClaimEventKind, NoteKind, Ticket, Timestamp};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::TargetOpError;
use hotsheet_ticketing::{FsStore, git_connection_id, ops};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::sessions::{HostedRoles, coordination_state};
use super::tickets::identifier;
use super::*;
use crate::coordination::{
    Coordinator, Phase, PreferenceRequest, ReturnRoute, Role, State, WorkerPreferences,
};

/// Where the home keeps a fetched site handoff, one ref per assignment.
pub const SITE_HANDOFF_REFS: &str = "refs/hotsheet/site-handoffs";
/// The remote both sites push to and fetch from. Every site of a project declares the
/// same repository identity (§26.2), so this names one repository everywhere.
pub const SITE_HANDOFF_REMOTE: &str = "origin";
/// The member's availability lease, as `coordination register` bounds it.
pub const MIN_MEMBER_LEASE_SECONDS: u32 = 10;
pub const MAX_MEMBER_LEASE_SECONDS: u32 = 3600;
const MAX_BRANCH_CHARS: usize = 200;

/// A worker on another site of the project, joining (or renewing in) the home's pool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedPoolMember {
    pub worker_id: String,
    /// The worker's session generation in the pool: the orchestrator's session id.
    pub generation: String,
    pub lease_seconds: u32,
    /// The site the worker runs on. Descriptive: the pool records the worker only.
    pub site_target_id: String,
    /// The exact ticket the member's run is for, stated as its request where the pool
    /// already takes worker requests (schema nine); an older pool is never upgraded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub native_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
}

/// What the home's pool says to one member.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedPoolReply {
    /// The home checkout has a coordination pool. Without one nothing joins: its
    /// dispatcher's own setup creates it.
    pub pool: bool,
    /// The member holds a live registration after this call.
    pub joined: bool,
    pub roles: HostedRoles,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assignment: Option<HostedPoolAssignment>,
}

/// The member's unfinished assignment in the home pool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HostedPoolAssignment {
    pub assignment_id: String,
    pub connection_id: String,
    pub native_id: String,
    pub slug: String,
    pub phase: Phase,
    /// The published shipping base the work starts from.
    pub base: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub commit: Option<String>,
    /// What this call recorded from the ticket's evidence: `accepted` or `handoff`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded: Option<String>,
    /// Why the next phase is not recorded yet, as a stable code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub waiting: Option<String>,
}

/// What the ticket says a member did with its offered or accepted assignment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SiteEvidence {
    /// It holds the exact ticket's live claim: record `accepted`.
    Accept,
    /// It released the claim and named a pushed branch and commit: fetch, verify and
    /// record `handoff`. `accept_first` when the pool never saw it accept.
    Handoff {
        branch: String,
        commit: String,
        accept_first: bool,
    },
    /// Nothing to record yet, and why.
    Wait(&'static str),
}

/// The `Branch:` and `Commit:` lines of the newest regular `## Integration handoff`
/// note that names both, the same note a worker writes on its own machine.
pub fn handoff_note(ticket: &Ticket) -> Option<(String, String)> {
    ticket.notes.iter().rev().find_map(|note| {
        let mut lines = note.text.lines();
        if note.kind != NoteKind::Regular || lines.next() != Some("## Integration handoff") {
            return None;
        }
        let field = |name: &str| {
            note.text.lines().find_map(|line| {
                line.strip_prefix(name)
                    .map(|value| value.trim().trim_matches('`').to_owned())
            })
        };
        let branch = field("Branch:")?;
        let commit = field("Commit:")?;
        (commit.len() == 40 && commit.bytes().all(|byte| byte.is_ascii_hexdigit()))
            .then_some((branch, commit.to_ascii_lowercase()))
    })
}

/// A branch name the home may fetch: one plain `refs/heads/` name, no option, range,
/// pattern or revision syntax. `git check-ref-format --branch` checks it again.
pub fn valid_branch(branch: &str) -> bool {
    !branch.is_empty()
        && branch.chars().count() <= MAX_BRANCH_CHARS
        && !branch.starts_with(['-', '/', '.'])
        && !branch.ends_with(['/', '.'])
        && !branch.ends_with(".lock")
        && !branch.contains("..")
        && !branch.contains("//")
        && !branch.contains("@{")
        && !branch.chars().any(|c| {
            c.is_whitespace()
                || c.is_control()
                || matches!(c, ':' | '~' | '^' | '?' | '*' | '[' | '\\')
        })
}

/// The pure decision: what the shared ticket says the member did, given the phase the
/// home pool records for its assignment. `tickets` are the source's other tickets, so
/// a member holding another live claim never accepts this one.
pub fn site_evidence(
    phase: Phase,
    ticket: &Ticket,
    tickets: &[Ticket],
    worker: &str,
    now: &Timestamp,
) -> Option<SiteEvidence> {
    let holds =
        |t: &Ticket| t.claimed_by.as_deref() == Some(worker) && !ops::claim_available(t, now);
    let released_by_worker = ticket.claimed_by.is_none()
        && ticket
            .claim_history
            .last()
            .is_some_and(|event| event.kind == ClaimEventKind::Release && event.worker == worker);
    let handoff = |accept_first| match handoff_note(ticket) {
        Some((branch, commit)) if valid_branch(&branch) => SiteEvidence::Handoff {
            branch,
            commit,
            accept_first,
        },
        Some(_) => SiteEvidence::Wait("handoff_branch_invalid"),
        None => SiteEvidence::Wait("handoff_note_missing"),
    };
    match phase {
        Phase::Offered => Some(if tickets.iter().any(|t| t.id != ticket.id && holds(t)) {
            SiteEvidence::Wait("worker_holds_another_claim")
        } else if holds(ticket) {
            SiteEvidence::Accept
        } else if released_by_worker {
            // Claimed, worked and released between two relays: the release proves
            // the member held it.
            handoff(true)
        } else {
            SiteEvidence::Wait("claim_not_taken")
        }),
        Phase::Accepted => Some(if ticket.claimed_by.is_some() {
            SiteEvidence::Wait("claim_held")
        } else {
            handoff(false)
        }),
        _ => None,
    }
}

/// HS2-RCR4A6: the least time between two pulls the home makes for its members.
pub const FRESHEN_INTERVAL_MS: u64 = 2_000;

/// HS2-RCR4A6: pull the home's clone of the shared ticket store, at most once per
/// [`FRESHEN_INTERVAL_MS`] per store, so evidence a site published is read without
/// waiting for the home's next sync pass. True when new commits arrived.
fn freshen(root: &Path, now_ms: u64) -> bool {
    use std::collections::HashMap;
    use std::sync::{Mutex, OnceLock};
    static LAST: OnceLock<Mutex<HashMap<std::path::PathBuf, u64>>> = OnceLock::new();
    if !hotsheet_ticketing::sync::publishes_to_remote(root) {
        return false;
    }
    {
        let mut last = LAST
            .get_or_init(Default::default)
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let key = root.to_path_buf();
        if last
            .get(&key)
            .is_some_and(|at| now_ms.saturating_sub(*at) < FRESHEN_INTERVAL_MS)
        {
            return false;
        }
        last.insert(key, now_ms);
    }
    matches!(
        hotsheet_ticketing::sync::sync_once(root),
        hotsheet_ticketing::sync::SyncReport::Synced { pulled: true, .. }
    )
}

/// The pool's source id of a store root, as `Coordinator` derives it.
fn source_id(root: &Path) -> Option<String> {
    let canonical = hotsheet_sync::paths::canonicalize(root).ok()?;
    Some(format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    ))
}

/// The ref one assignment's fetched handoff is kept under.
pub fn handoff_ref(assignment_id: &str) -> String {
    let plain = !assignment_id.is_empty()
        && assignment_id.len() <= 80
        && assignment_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    let leaf = if plain {
        assignment_id.to_owned()
    } else {
        format!("{:x}", Sha256::digest(assignment_id.as_bytes()))[..32].to_owned()
    };
    format!("{SITE_HANDOFF_REFS}/{leaf}")
}

fn git(root: &Path, args: &[&str]) -> Option<std::process::Output> {
    hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .ok()
}

/// Fetch the member's pushed branch into the home checkout and check the named commit
/// is on it and inside the assignment's scope. `Err` is the waiting code.
fn fetch_handoff(
    root: &Path,
    assignment: &crate::coordination::Assignment,
    branch: &str,
    commit: &str,
) -> Result<(), &'static str> {
    let checked = git(root, &["check-ref-format", "--branch", branch])
        .is_some_and(|out| out.status.success());
    if !checked {
        return Err("handoff_branch_invalid");
    }
    let local = handoff_ref(&assignment.id);
    let spec = format!("+refs/heads/{branch}:{local}");
    let fetched = git(
        root,
        &["fetch", "--no-tags", "--quiet", SITE_HANDOFF_REMOTE, &spec],
    )
    .is_some_and(|out| out.status.success());
    if !fetched {
        return Err("handoff_fetch_failed");
    }
    let on_branch = git(root, &["merge-base", "--is-ancestor", commit, &local])
        .is_some_and(|out| out.status.success());
    if !on_branch {
        return Err("handoff_commit_not_on_branch");
    }
    if !assignment.scope.is_empty()
        && crate::coordination::scope::review_commit(
            root,
            &assignment.base,
            commit,
            &assignment.scope,
        )
        .is_err()
    {
        return Err("handoff_out_of_scope");
    }
    Ok(())
}

fn roles(state: &State, now_s: i64) -> HostedRoles {
    HostedRoles {
        integrator_live: Some(
            state
                .integrator
                .as_ref()
                .is_some_and(|s| s.expires_at > now_s),
        ),
        dispatcher_live: Some(
            state
                .dispatcher
                .as_ref()
                .is_some_and(|s| s.expires_at > now_s),
        ),
    }
}

/// The member's unfinished assignment: the one it holds (offered or accepted) first,
/// else its latest handed-off one still in the pool.
fn member_assignment<'a>(
    state: &'a State,
    worker: &str,
    generation: &str,
) -> Option<&'a crate::coordination::Assignment> {
    let mine = || {
        state
            .assignments
            .values()
            .filter(move |a| a.worker == worker && a.generation == generation && a.phase.pending())
    };
    mine()
        .find(|a| a.phase.holds_worker())
        .or_else(|| mine().max_by(|a, b| a.transitions.len().cmp(&b.transitions.len())))
}

fn view(a: &crate::coordination::Assignment) -> HostedPoolAssignment {
    HostedPoolAssignment {
        assignment_id: a.id.clone(),
        connection_id: a.ticket.connection_id.clone(),
        native_id: a.ticket.native_id.clone(),
        slug: a.slug.clone(),
        phase: a.phase,
        base: a.base.clone(),
        commit: a.commit.clone(),
        recorded: None,
        waiting: None,
    }
}

fn failed() -> OrchestrationError {
    OrchestrationError::TargetOperation(TargetOpError::Failed)
}

impl OrchestrationRuntime {
    /// The home checkout's pool and the linked Git store it is bound to. `None` when
    /// the checkout has no pool yet.
    fn site_pool_open(
        &self,
        checkout_id: &str,
    ) -> Result<Option<(Coordinator, FsStore)>, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        let root = Path::new(&checkout.root);
        let Some(state) = coordination_state(root).map_err(|()| failed())? else {
            return Ok(None);
        };
        let store = checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .filter_map(|source| checkout.verified_git_source(&source.connection_id).ok())
            .find(|store| source_id(store.root()).as_deref() == Some(state.source_id.as_str()))
            .ok_or(OrchestrationError::TargetOperation(
                TargetOpError::Unsupported,
            ))?;
        let coordinator =
            Coordinator::open_with_registry_home(root, store.root(), self.home.clone())
                .map_err(|_| failed())?;
        Ok(Some((coordinator, store)))
    }

    /// `GET /checkouts/{checkout}/target-pool?worker_id=&generation=`: the pool's role
    /// flags and the member's assignment. Reads only; joins and records nothing.
    pub fn hosted_pool_read(
        &self,
        checkout_id: &str,
        worker_id: Option<&str>,
        generation: Option<&str>,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedPoolReply, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        let now_s = i64::try_from(tx.clock.now_ms() / 1000).unwrap_or(i64::MAX);
        let Some(state) = coordination_state(Path::new(&checkout.root)).map_err(|()| failed())?
        else {
            return Ok(HostedPoolReply {
                pool: false,
                joined: false,
                roles: HostedRoles {
                    integrator_live: Some(false),
                    dispatcher_live: Some(false),
                },
                assignment: None,
            });
        };
        let (joined, assignment) = match (worker_id, generation) {
            (Some(worker), Some(generation)) => (
                state
                    .workers
                    .get(worker)
                    .is_some_and(|s| s.generation == generation && s.expires_at > now_s),
                member_assignment(&state, worker, generation).map(view),
            ),
            _ => (false, None),
        };
        Ok(HostedPoolReply {
            pool: true,
            joined,
            roles: roles(&state, now_s),
            assignment,
        })
    }

    /// `POST /checkouts/{checkout}/target-pool`: register or renew a member from
    /// another site, then record what the shared ticket says it did with its
    /// assignment. Repeating it changes nothing that did not change.
    pub fn hosted_pool_join(
        &self,
        checkout_id: &str,
        member: HostedPoolMember,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedPoolReply, OrchestrationError> {
        identifier(&member.worker_id, "bounded worker id required")?;
        identifier(&member.generation, "bounded generation required")?;
        identifier(&member.site_target_id, "bounded site target id required")?;
        if !(MIN_MEMBER_LEASE_SECONDS..=MAX_MEMBER_LEASE_SECONDS).contains(&member.lease_seconds) {
            return Err(OrchestrationError::Invalid(
                "lease seconds must be 10 to 3600",
            ));
        }
        if let Some(connection) = &member.connection_id {
            identifier(connection, "bounded connection id required")?;
        }
        if let Some(native) = &member.native_id {
            identifier(native, "bounded native id required")?;
        }
        let now_ms = tx.clock.now_ms();
        let now_s = i64::try_from(now_ms / 1000).unwrap_or(i64::MAX);
        let Some((coordinator, store)) = self.site_pool_open(checkout_id)? else {
            return self.hosted_pool_read(checkout_id, None, None, tx);
        };
        let before = coordinator.state().map_err(|_| failed())?;
        // The run's exact ticket as the member's request, only where the pool already
        // takes requests: joining never upgrades a pool its other members may not read.
        let request = match (&member.connection_id, &member.native_id) {
            (Some(connection), Some(native))
                if before.version >= 9 && *connection == git_connection_id(&store) =>
            {
                ops::resolve(&store, native)
                    .ok()
                    .flatten()
                    .map(|ticket| ticket.slug)
            }
            _ => None,
        };
        let preferences = match request {
            Some(slug) => PreferenceRequest::replace(
                WorkerPreferences::stated(Vec::new(), Vec::new(), Vec::new(), Some(slug))
                    .map_err(|_| failed())?,
                false,
            ),
            None => PreferenceRequest::keep(),
        };
        coordinator
            .register_full(
                Role::Worker,
                &member.worker_id,
                &member.generation,
                now_s,
                i64::from(member.lease_seconds),
                ReturnRoute::HumanRelay,
                crate::coordination::StrategyRequest::default(),
                preferences,
            )
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Conflict))?;
        let state = coordinator.state().map_err(|_| failed())?;
        let Some(assigned) = member_assignment(&state, &member.worker_id, &member.generation)
        else {
            return Ok(HostedPoolReply {
                pool: true,
                joined: true,
                roles: roles(&state, now_s),
                assignment: None,
            });
        };
        let mut reply = view(assigned);
        if assigned.ticket.connection_id != git_connection_id(&store) {
            reply.waiting = Some("source_unsupported".into());
        } else {
            let mut evidence = self.site_pool_evidence(&store, assigned, &member, tx)?;
            // HS2-RCR4A6: the member's claim, release and note reach this clone only
            // through the shared remote. Before waiting on them, pull once.
            if matches!(evidence, Some(SiteEvidence::Wait(_)))
                && freshen(store.root(), tx.clock.now_ms())
            {
                evidence = self.site_pool_evidence(&store, assigned, &member, tx)?;
            }
            if let Some(evidence) = evidence {
                self.site_pool_record(&coordinator, assigned, &member, evidence, &mut reply)?;
            }
        }
        let state = coordinator.state().map_err(|_| failed())?;
        if let Some(now) = state.assignments.get(&assigned.id) {
            reply.phase = now.phase;
            reply.commit = now.commit.clone();
        }
        Ok(HostedPoolReply {
            pool: true,
            joined: true,
            roles: roles(&state, now_s),
            assignment: Some(reply),
        })
    }

    fn site_pool_evidence(
        &self,
        store: &FsStore,
        assigned: &crate::coordination::Assignment,
        member: &HostedPoolMember,
        tx: &mut Transaction<'_>,
    ) -> Result<Option<SiteEvidence>, OrchestrationError> {
        let Ok(id) = assigned.ticket.native_id.parse() else {
            return Ok(Some(SiteEvidence::Wait("ticket_unreadable")));
        };
        let (Ok(ticket), Ok(tickets)) = (store.read_ticket(&id), store.list_tickets()) else {
            return Ok(Some(SiteEvidence::Wait("ticket_unreadable")));
        };
        let now = Timestamp::from_datetime(
            time::OffsetDateTime::from_unix_timestamp_nanos(
                i128::from(tx.clock.now_ms()) * 1_000_000,
            )
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
        );
        Ok(site_evidence(
            assigned.phase,
            &ticket,
            &tickets,
            &member.worker_id,
            &now,
        ))
    }

    fn site_pool_record(
        &self,
        coordinator: &Coordinator,
        assigned: &crate::coordination::Assignment,
        member: &HostedPoolMember,
        evidence: SiteEvidence,
        reply: &mut HostedPoolAssignment,
    ) -> Result<(), OrchestrationError> {
        let transition = |phase, commit| {
            coordinator
                .worker_transition(
                    &member.worker_id,
                    &member.generation,
                    &assigned.id,
                    phase,
                    commit,
                )
                .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Conflict))
        };
        match evidence {
            SiteEvidence::Wait(code) => reply.waiting = Some(code.into()),
            SiteEvidence::Accept => {
                transition(Phase::Accepted, None)?;
                reply.recorded = Some("accepted".into());
            }
            SiteEvidence::Handoff {
                branch,
                commit,
                accept_first,
            } => {
                // Its release proves it held the ticket: accepted, whatever the fetch.
                if accept_first {
                    transition(Phase::Accepted, None)?;
                    reply.recorded = Some("accepted".into());
                }
                match fetch_handoff(&coordinator.project, assigned, &branch, &commit) {
                    Err(code) => reply.waiting = Some(code.into()),
                    Ok(()) => {
                        transition(Phase::Handoff, Some(commit))?;
                        reply.recorded = Some("handoff".into());
                    }
                }
            }
        }
        Ok(())
    }
}
