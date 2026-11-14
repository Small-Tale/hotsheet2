//! HS2-4J6V46: project target parity over the shared request workflow.
use crate::{Backend, BackendError};
use serde_json::{Value, json};

pub(crate) fn definition() -> Value {
    json!({"name":"hotsheet_targets", "description":"List/add/rename/remove shared project targets; inspect fresh candidates then explicitly bind/unbind/resolve this machine's reviewed target destinations. Lists make no requests. Inspect/bind/unbind/resolve and list with target_id require a machine-local backend. A fresh projection/capability is never a grant. grant_list, access_check and audit read this machine's delegated authority and decision journal (HS2-SBQ3T8); grants are issued and revoked by a person through the CLI, never through this tool. policy_show, policy_validate, policy_dry_run and approval_list read the throughput policy and pending approvals (HS2-YCJDEB); setting the policy and answering an approval are likewise a person's CLI actions. ticket_create/read/update/link/unlink/links act on tickets in a declared target for an orchestration ticket (HS2-AZ15GA): every call passes the grant and policy gate under your actor_id, the target's own source stays authoritative, and ticket_create needs an operation_id you retain so a retry never creates a second ticket. run_dispatch starts work on one exact target ticket and records a run (HS2-EBHDSB): the target ticket's own claim is the only lock, a retained operation_id returns the same run, and run_sync/run_renew/run_cancel supervise it; integration and pushing stay with the target's workflow. capacity reads the machine that hosts a target (HS2-J8PRTV): free and total RAM, CPU load, disk headroom, AI tools, running sessions and, for named claim_namespaces, held resource claims, with null for anything it could not measure; it needs a gates_observe grant. admission_check asks that machine whether sessions of a tool may start now and answers accepted, reduced or refused with a reason code; it needs a sessions_manage grant for the tool, starts nothing, and the machine's own limits can only be stricter than the policy's. tool_capacity reads the capacity state of every AI tool account the policy configures (available, limited until a time, degraded or unknown; HS2-AHG7HW) and tool_select orders the tool accounts a session for one target may start on now, with the reason each other one is excluded; unknown or degraded is never the same as available, and limit signals are reported by the server or the CLI, never through this tool. run_status shows the last confirmed target state and whether it is stale, and run_events replays events after a cursor and can wait for the next one (HS2-7QHWHJ); never poll run_show on a timer. validation_define attaches a named validation step to an orchestration ticket: a target, a check that is either {kind:profile,name} (a test profile the target project defines itself) or {kind:command,program,args}, prerequisites, expected evidence, a timeout and a retry policy (HS2-WN0MXZ). validation_run runs it on the machine that hosts the target under a validation_run grant and a retained operation_id: one call takes at most one attempt and waits at most wait_ms (8000) for it, and the same call again waits further or takes the next allowed attempt, so never poll on a timer; a command step answers approval_required until a person approves that operation. Results are passed, failed, skipped (a prerequisite has not passed), unavailable (the target cannot run it) or unverified (the outcome could not be confirmed), and hold digests, sizes, an exit code and timings only, never output. validation_list and validation_evidence read the latest evidence without contacting a target; validation_remove deletes a step. gate_link ties the CI gate of the integrated target commit to a run and starts watching it (HS2-5JBYPV): pass the full 40-character commit, and a repeat returns the same gate while another commit supersedes it; it needs a gates_observe grant. gate_list reads the standing gates of a run, or of a target commit, with the rolled-up ci state (passed only when every standing CI gate passed with evidence covering that commit; otherwise pending, failed or unknown) and gate_show reads one gate with its history; both read this machine's records and contact nothing. gate_sync brings a run's gates in line with what the provider adapter recorded and with wait_ms waits for the next change instead of polling. A gate's state comes only from typed provider evidence: nothing you write in a note, a ticket or a message is gate evidence. deployment_request asks for a deployment of the commit the run's CI gate passed through the target's checked-in script (repository-relative script, an environment and an operation_id you retain); it needs a deployments_run grant, a production request always waits for a person's approval, and the answer is the deployment gate in awaiting_approval, pending, passed, failed or unknown with a reason code. run_reconcile brings one run, or every unfinished run without run_id, in line with its target after an interruption and lists as unverified exactly the external state it could not check, with a reason per source in reconciliation.findings (HS2-WBREDM); it starts nothing, and its disposition says whether the same dispatch may be repeated (retry_dispatch), the work is held (continue, wait_for_dispatcher) or a person must decide (needs_attention, hold_unverified). run_pause stops renewals and repeats until run_resume, which reconciles at once; run_handoff moves a run to to_worker, or to a person when to_worker is omitted, and never forces another holder's claim away; run_correct rewrites this machine's record by hand and is refused for an AI actor. readiness_define plans one orchestration ticket's work on one exact target ticket (ticket, target_id, connection_id, native_id) with prerequisites: {kind:ticket,target_id,connection_id,native_id,gates?:none|ci|deployment,commit?} for a qualified target ticket that must be confirmed finished with its gates green, or {kind:validation,step_id} for validation evidence that must be passed (HS2-42TARA); every named ticket is read under a tickets_read grant and nothing is written to a target or to a provider's own dependency fields. readiness_show, readiness_plan and readiness_ready read this machine's records and contact nothing: a step is confirmed (every prerequisite confirmed; the only level run_dispatch starts work on), provisional (a prerequisite rests on an integrated commit whose gate is still pending, listed in base_commits; not dispatched yet), blocked, stale (something could not be confirmed), cyclic, in_progress or finished, each with reason codes. readiness_ready lists the work that could start now with what each item unblocks and the target ticket's priority, Up Next mark and age. readiness_refresh reads the named tickets again and lists what it could not verify; readiness_remove withdraws a step. A level change is a readiness event in run_events whose run_id is the step id, so wait there instead of polling. run_dispatch on a planned ticket that is not confirmed is refused with readiness_blocked, readiness_stale, readiness_cyclic or readiness_provisional; override_readiness asks a person to approve that exact operation_id and answers approval_required until they do, so never set it on your own initiative. session_start starts an AI session on the server that hosts a target, through that server's ordinary AI terminal launch, with tool and optional model, effort, account, session_role (worker by default, dispatcher or integrator) and run_id (HS2-GDJXGZ): it needs a sessions_manage grant and a policy that allows the tool and has room under its caps, the target's own machine may still refuse, and a retained operation_id returns the same session and never launches twice; the session appears in that server's drawer labelled with the orchestration project and run. session_list and session_show read this machine's records and contact nothing; session_sync reads one session, or every unfinished one without session_id, from its host, so a crashed session becomes lost and can be replaced with session_start and replaces. session_safe_point asks the host whether a session may be wound down and lists every blocker (mid_turn, held_claim, uncommitted_changes, pending_handoff, or an unknown). session_request tells a running session which exact ticket its run is for through its host's session push, retried when the tool becomes ready, as the scheduler does after a refill (HS2-R7QH35). session_stop winds a session down only at a safe point the host confirms and is refused with session_not_safe otherwise, or session_human_owned once a person took it over; stopping mid-turn, taking over and pausing are a person's CLI actions, never available here. scheduler_plan computes where and how many sessions the scheduler would start, keep or wind down per target, tool and model from the confirmed ready work, the policy, each target's capacity, tool capacity and held resources (HS2-ECWG50): it is a dry run that changes and logs nothing, lists every piece of ready work best first with its score parts, and gives each one not placed a reason code (target_cap, machine_refused with the limit, no_tool_available, resource_held, cooldown, refusal_backoff, policy_denied, target_unavailable, provisional_not_dispatched). scheduler_explain says why one piece of work is not running, by step_id or by target_id, connection_id and native_id. scheduler_tick runs one pass and logs every decision with its inputs; it starts or winds down sessions only when the policy sets placement.automatic, and otherwise, or with dry_run, applies nothing. A decision the policy wants approved answers approval_required and is not acted on until a person approves it. scheduler_decisions reads the decision log after a cursor. readiness_define also takes resources, each {namespace,resource} of the resource-claim service (for example {namespace:vm,resource:macos-vm}), and the policy can require resources of every piece of work on a target in placement.resources (HS2-2FG91X): the scheduler reads the claims of the machine that hosts the target, does not start work whose resource someone holds (resource_held, with the claim's expiry as retry_at_ms) or whose claims it could not read while the backend fails closed (resource_unknown), and never claims anything itself: the started session acquires the resource with hotsheet_resource_claim, keeps the renewal token, renews while it works and releases when done. scheduler_plan lists contention per resource (state held, taken, unreadable, unverified or not_read, the step given it and the steps waiting) and scheduler_explain lists each resource a step needs with how it stands. The server runs the scheduling loop on run events for a policy that enables it, so never call scheduler_tick on a timer. summary reads lead time, queue wait, blocked time, retries, CI and deployment outcomes and known AI cost of the project's runs, per run, work item and target (HS2-S13719): identifiers and metrics only, computed from this machine's records without contacting a target; each measure is {status:known,value} or {status:unknown,reason}, so never read an unknown as zero, and a cost total is exact only when cost.complete is true. A target ticket dispatched again or reached through two targets is one work item whose cost counts once, toward the target of its earliest Provisional work across targets is off unless the policy sets speculation.allowed (HS2-JSF985): then a provisional step may be dispatched, and placed by the scheduler below confirmed work of the same score, when the orchestration policy and the own speculation setting of every involved target allow it, no involved ticket carries the opt-out tag, and no more unconfirmed gates stack beneath it than the smallest max_depth; otherwise run_dispatch answers readiness_provisional_refused and readiness_show names the reason under speculation.refusal (for example target_speculation_disabled, target_speculation_unknown, speculation_depth_exceeded, speculation_opted_out). readiness_speculation lists the work started ahead of a pending gate with each gate and its result, the prerequisites to repair first and the gates to re-evaluate; a gate result changes that work in a provisional event of run_events (confirmed, at_risk, rebased, reevaluate, reopened), so wait there instead of polling. readiness_reopen records, with step_id and a reason code, that a failed prerequisite's fix invalidated a step's provisional work; it keeps the record and does not touch the run. timeline reads the project's plan as dependency waves (HS2-J89JC7): done, working_now, next, then and later once orchestration was kicked off, or planned waves (ordinal 0 is the first) for a proposed plan, each with an estimated start; every item names its goal ticket, target project and ticket, local or remote machine, tool, state (running, provisional, ready, waits_for_prerequisite, waits_for_permission with the request, waits_for_decision with the goal ticket's feedback note, done) and run. It reads this machine's records and contacts no target; pass the answer's version with wait_ms to wait for the next change instead of polling. Retain target_id and expected revisions for repeatable setup; stale revisions conflict, including otherwise identical retries. plan_propose starts the orchestration project's planning AI session in that project itself (HS2-HCW7MD), through the ordinary AI terminal launch so it shows in the drawer, with tool and optional model, effort, goal and a retained operation_id: the planner reads the open goal tickets and each declared project's tickets and proposes the dry-run plan with ticket_create/ticket_link and readiness_define (tool records the preferred AI tool of a step); one planner runs at a time, a live one answers instead (outcome existing), and the answer is plan_no_targets, plan_no_goals or plan_goal_not_open when there is nothing to plan. Nothing is placed until a person starts orchestration. plan_show reads where planning stands (idle, planning or ended), the open goal tickets with their plan steps and whether the policy places sessions yet.",
    "inputSchema":{"type":"object","required":["action"],"properties":{
        "action":{"type":"string","enum":["list","add","rename","remove","bind","unbind","resolve","inspect","grant_list","access_check","audit","policy_show","policy_validate","policy_dry_run","approval_list","ticket_create","ticket_read","ticket_update","ticket_link","ticket_unlink","ticket_links","run_dispatch","run_list","run_show","run_renew","run_cancel","run_sync","capacity","admission_check","tool_capacity","tool_select","run_status","run_events","validation_define","validation_list","validation_evidence","validation_run","validation_remove","gate_link","gate_list","gate_show","gate_sync","deployment_request","run_reconcile","run_pause","run_resume","run_handoff","run_correct","readiness_define","readiness_show","readiness_plan","readiness_ready","readiness_refresh","readiness_remove","readiness_speculation","readiness_reopen","session_start","session_list","session_show","session_sync","session_safe_point","session_request","session_stop","scheduler_plan","scheduler_explain","scheduler_decisions","scheduler_tick","summary","plan_propose","plan_show","timeline"]},
        "wait_ms":{"type":"integer","minimum":0,"maximum":20000,"description":"run_events: wait this long for the next event when there is none (use with after instead of polling); validation_run: wait this long for the attempt to finish, at most 8000; gate_sync: wait this long for the provider adapter's records to change"},
        "step":{"type":"object","description":"validation_define: {step_id, check:{kind:profile,name}|{kind:command,program,args}, prerequisites?, evidence?:{exit_code?,stdout_sha256?}, timeout_seconds?, retry?:{max_attempts}}; target_id is taken from the target_id argument"},
        "step_id":{"type":"string","description":"validation_evidence/validation_run/validation_remove: the step's name within its orchestration ticket; readiness_show/readiness_remove/readiness_reopen (required), readiness_refresh (optional): the plan step's id; scheduler_explain: the plan step to explain, instead of target_id, connection_id and native_id"},
        "prerequisites":{"type":"array","maxItems":16,"items":{"type":"object"},"description":"readiness_define: what must hold first, each {kind:ticket,target_id,connection_id,native_id,gates?,commit?} or {kind:validation,step_id,ticket?}"},
        "override_readiness":{"type":"boolean","description":"run_dispatch: ask a person to let this operation start although its plan step is not ready; answers approval_required until approved"},
        "tool":{"type":"string","description":"tool_select: the preferred tool id; omit for any tool the target allows; admission_check: the AI tool a session would run; session_start: the AI tool to launch; plan_propose (required): the AI tool the planner runs in; readiness_define: the AI tool preferred for the step's work"},
        "goal":{"type":"object","description":"plan_propose: plan only this goal ticket of the orchestration project, {native_id, connection_id?}; omit for every open goal ticket"},
        "model":{"type":"string","description":"tool_select: the model the session would use, checked against the target's model list; admission_check/session_start: optional model"},
        "effort":{"type":"string","description":"session_start/plan_propose: optional effort level the tool's plugin declares"},
        "account":{"type":"string","description":"session_start: the tool account, required when the policy configures accounts for the tool"},
        "session_role":{"type":"string","enum":["worker","dispatcher","integrator"],"description":"session_start: the part the session plays in the target's role model (default worker)"},
        "session_id":{"type":"string","description":"session_show/session_safe_point/session_request/session_stop: the session's id; session_sync: omit to sync every unfinished session"},
        "replaces":{"type":"string","description":"session_start: the lost or failed session this one replaces"},
        "resources":{"type":"array","maxItems":4,"items":{"type":"object"},"description":"readiness_define: resources the session working the step must hold, each {namespace,resource} in the resource-claim service's names (HS2-2FG91X)"},
        "dry_run":{"type":"boolean","description":"scheduler_tick: compute and log the decisions and apply none of them, whatever the policy says"},
        "tool_load":{"type":"array","items":{"type":"object","required":["tool","sessions"],"properties":{"tool":{"type":"string"},"account":{"type":"string"},"sessions":{"type":"integer","minimum":0}}},"description":"tool_select (required): sessions now running per tool account; an empty list states that none are running"},
        "claim_namespaces":{"type":"array","items":{"type":"string"},"maxItems":8,"description":"capacity: resource-claim namespaces whose live claims to list"},
        "sessions":{"type":"integer","minimum":1,"maximum":64,"description":"admission_check: how many sessions to ask for (default 1)"},
        "run_id":{"type":"string","description":"run_show/renew/cancel/sync/pause/resume/handoff/correct, gate_link/gate_sync/deployment_request, gate_list: the run's id; validation_run: optionally the dispatch run the validation belongs to; run_reconcile: omit to reconcile every unfinished run"},
        "commit":{"type":"string","description":"gate_link: the integrated target commit, all 40 hex characters; gate_list with target_id: the commit to read gates for"},
        "repository":{"type":"string","description":"gate_link: the provider repository as owner/name; defaults to, and must agree with, the GitHub repository the target declares"},
        "workflow":{"type":"string","description":"gate_link: the workflow name (default CI)"},
        "branch":{"type":"string","description":"gate_link: the shipping branch whose runs count (default main)"},
        "gate_provider":{"type":"string","description":"gate_link: the CI provider adapter (default github-actions); deployment_request: the deployment adapter (default target-script)"},
        "gate_id":{"type":"string","description":"gate_show: the gate's id"},
        "environment":{"type":"string","description":"deployment_request: the environment name"},
        "script":{"type":"string","description":"deployment_request: the target's checked-in deploy script, relative to its repository root"},
        "production":{"type":"boolean","description":"deployment_request: an irreversible production change; a person is always asked first"},
        "artifact":{"type":"object","description":"deployment_request: optional {name, digest} of the built artifact, digest as lowercase SHA-256"},
        "reason":{"type":"string","description":"run_pause (optional), run_correct (required), readiness_reopen (required): a stable reason code, never free text"},
        "to_worker":{"type":"string","description":"run_handoff: the worker that takes the run over; omit to hand it to a person"},
        "state":{"type":"string","enum":["requested","queued","dispatched","intervened","completed","failed","cancelled"],"description":"run_correct: the state the run is really in"},
        "mode":{"type":"string","enum":["direct","dispatcher"],"description":"run_dispatch: direct claims the ticket for worker_id; dispatcher marks it Up Next for the target's own dispatcher"},
        "worker_id":{"type":"string","description":"run_dispatch direct: the worker the ticket is claimed for; run_correct: the worker that really holds the work"},
        "worker_label":{"type":"string"},
        "lease_minutes":{"type":"integer","minimum":5,"maximum":240,"description":"run_dispatch/run_renew/run_handoff"},
        "active":{"type":"boolean","description":"run_list/session_list: leave out finished ones"},
        "ticket":{"type":"object","description":"ticket_create/link/unlink/links and validation_*: the orchestration ticket {native_id, connection_id?}; connection_id may be omitted when the project has one git ticket source; summary (optional): only the runs of this orchestration ticket"},
        "connection_id":{"type":"string","description":"ticket_*: the target's ticket source connection id"},
        "native_id":{"type":"string","description":"ticket_read/update/link/unlink: the target ticket's provider-native id"},
        "fields":{"type":"object","description":"ticket_create/ticket_update: {title, details, category, priority, status, tags, up_next}"},
        "expected_token":{"type":"string","description":"ticket_update: the provider's concurrency token from a previous read"},
        "relation":{"type":"string","enum":["observed","validated"],"description":"ticket_link"},
        "resolve":{"type":"boolean","description":"ticket_links: also read each linked target ticket"},
        "policy":{"type":"object","description":"policy_validate/policy_dry_run: a proposed throughput policy body; omit to examine the stored one"},
        "for_actor":{"type":"string","description":"access_check: the actor id the grant must cover (not the caller's own actor_id)"},
        "target_action":{"type":"string","enum":["tickets_read","tickets_write","work_start","gates_observe","validation_run","sessions_manage","deployments_run"],"description":"access_check: the delegated action"},
        "session":{"type":"object","description":"access_check for sessions_manage: {tool, model?, running?, starting?}"},
        "load":{"type":"object","description":"access_check for a session start: {target_sessions, machine_sessions, tool_sessions, account?, account_sessions?}; a start without it is denied policy_load_unknown"},
        "operation_id":{"type":"string","description":"access_check: the operation an approval would be for; validation_run/session_start/plan_propose: retained across repeats of one request"},
        "after":{"type":"integer","minimum":0,"description":"audit, scheduler_decisions: return entries after this sequence number"},
        "version":{"type":"string","maxLength":160,"description":"timeline: the version already seen; with wait_ms, wait for the next change instead of polling"},
        "limit":{"type":"integer","minimum":1,"maximum":500,"description":"audit: page size; scheduler_decisions: page size, at most 200"},
        "destination_checkout":{"type":"string","description":"registered local/remote checkout id to inspect"},
            "registration_id":{"type":"string","description":"reviewed remote registration for inspect; omit for local"},
            "checkout":{"type":"string","description":"owning registered checkout id; inherits selected project"},
        "target_id":{"type":"string","description":"immutable canonical target ULID; retain generated id for retries"},
        "expected_revision":{"type":["string","null"],"description":"shared opaque manifest revision; null only for an absent manifest"},
        "label":{"type":"string"},"clear_expected_repository_identity":{"type":"boolean"},"expected_repository_identity":{"type":["string","null"]},
        "expected_manifest_revision":{"type":"string"},"expected_binding_revision":{"type":"integer","minimum":0},
        "destination":{"type":"object","description":"explicit reviewed local/remote server, CA, checkout and source authority identity; never origin/credentials"},
        "previous_destination":{"type":["object","null"],"description":"exact previous reviewed identity, null for initial/unbound"}
    }}})
}
pub(crate) fn backend_error(
    error: hotsheet_ticketing::orchestration::OrchestrationError,
) -> BackendError {
    BackendError {
        status: Some(hotsheet_cli::orchestration::error_status(&error)),
        message: format!("{}: {}", error.code(), error),
    }
}
fn required<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} is required"))
}
fn optional_id<'a>(args: &'a Value, name: &str) -> Result<Option<&'a str>, String> {
    match args.get(name) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{name} must be a string when present")),
    }
}
pub(crate) fn dispatch(args: &Value, backend: &dyn Backend) -> Result<Value, String> {
    let action = required(args, "action")?;
    // HS2-4J6V46: tools/call does not enforce inputSchema; only omission selects defaults.
    let id = optional_id(args, "target_id")?;
    let registration = optional_id(args, "registration_id")?;
    let machine = matches!(
        action,
        "bind"
            | "unbind"
            | "resolve"
            | "inspect"
            | "grant_list"
            | "access_check"
            | "audit"
            | "policy_dry_run"
            | "approval_list"
            | "ticket_create"
            | "ticket_read"
            | "ticket_update"
            | "ticket_link"
            | "ticket_unlink"
            | "ticket_links"
            | "run_dispatch"
            | "run_list"
            | "run_show"
            | "run_renew"
            | "run_cancel"
            | "run_sync"
            | "run_status"
            | "run_events"
            | "gate_link"
            | "gate_list"
            | "gate_show"
            | "gate_sync"
            | "deployment_request"
            // HS2-WBREDM
            | "run_reconcile"
            | "run_pause"
            | "run_resume"
            | "run_handoff"
            | "run_correct"
            // HS2-S13719
            | "summary"
            // HS2-J89JC7
            | "timeline"
            | "tool_capacity"
            | "tool_select"
            | "capacity"
            | "admission_check"
            | "validation_define"
            | "validation_list"
            | "validation_evidence"
            | "validation_run"
            | "validation_remove"
            // HS2-42TARA
            | "readiness_define"
            | "readiness_show"
            | "readiness_plan"
            | "readiness_ready"
            | "readiness_refresh"
            | "readiness_remove"
            // HS2-JSF985
            | "readiness_speculation"
            | "readiness_reopen"
            // HS2-GDJXGZ
            | "session_start"
            | "session_list"
            | "session_show"
            | "session_sync"
            | "session_safe_point"
            | "session_request"
            | "session_stop"
            // HS2-ECWG50
            | "scheduler_plan"
            | "scheduler_explain"
            | "scheduler_decisions"
            | "scheduler_tick"
            // HS2-HCW7MD
            | "plan_propose"
            | "plan_show"
    ) || (action == "list" && id.is_some());
    // Preflight before any backend request; HttpBackend repeats it for actual origin.
    if machine && !backend.machine_local() {
        return Err("target binding administration requires a machine-local backend".into());
    }
    let owner = required(args, "checkout")?;
    if owner.is_empty()
        || owner.len() > 160
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("registered checkout id required".into());
    }
    if let Some(id) = id {
        if id
            .parse::<hotsheet_model::Ulid>()
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(id)
        {
            return Err("canonical target ULID required".into());
        }
    }
    let prefix = format!("/checkouts/{owner}/orchestration/targets");
    if action == "list" {
        return backend
            .get(
                &id.map(|id| format!("{prefix}/{id}/binding"))
                    .unwrap_or(prefix),
                &[],
            )
            .map_err(crate::be_msg);
    }
    if action == "audit" {
        let number = |name: &str, default: u64| match args.get(name) {
            None => Ok(default),
            Some(value) => value
                .as_u64()
                .ok_or_else(|| format!("{name} must be an unsigned integer when present")),
        };
        return backend
            .get(
                &format!(
                    "/checkouts/{owner}/orchestration/audit?after={}&limit={}",
                    number("after", 0)?,
                    number("limit", 100)?
                ),
                &[],
            )
            .map_err(crate::be_msg);
    }
    // HS2-J89JC7: the plan as dependency waves; a version and wait_ms wait for a change.
    if action == "timeline" {
        let mut path = format!("/checkouts/{owner}/orchestration/timeline");
        match args.get("version") {
            None | Some(Value::Null) => {}
            Some(Value::String(version))
                if !version.is_empty()
                    && version.len() <= 160
                    && version.bytes().all(|byte| byte.is_ascii_alphanumeric()) =>
            {
                let wait_ms = args
                    .get("wait_ms")
                    .and_then(Value::as_u64)
                    .unwrap_or(0)
                    .min(20_000);
                path = format!("{path}?after={version}&wait_ms={wait_ms}");
            }
            Some(_) => return Err("version must be a timeline version".into()),
        }
        return backend.get(&path, &[]).map_err(crate::be_msg);
    }
    // HS2-S13719: the read-only progress and cost summary of the project's runs.
    if action == "summary" {
        let mut path = format!("/checkouts/{owner}/orchestration/summary");
        if let Some(ticket) = args.get("ticket").filter(|ticket| !ticket.is_null()) {
            let part = |name: &str| match ticket.get(name) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(value))
                    if !value.is_empty()
                        && value.len() <= 160
                        && value.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        }) =>
                {
                    Ok(Some(value.as_str()))
                }
                Some(_) => Err(format!("ticket.{name} must be a bounded identifier")),
            };
            let native_id = part("native_id")?.ok_or("ticket.native_id is required")?;
            path = format!("{path}?ticket={native_id}");
            if let Some(connection_id) = part("connection_id")? {
                path = format!("{path}&connection_id={connection_id}");
            }
        }
        return backend.get(&path, &[]).map_err(crate::be_msg);
    }
    // HS2-YCJDEB: read-only policy and approval views.
    let base = format!("/checkouts/{owner}/orchestration");
    match action {
        "policy_show" => {
            return backend
                .get(&format!("{base}/policy"), &[])
                .map_err(crate::be_msg);
        }
        "approval_list" => {
            return backend
                .get(&format!("{base}/approvals"), &[])
                .map_err(crate::be_msg);
        }
        // HS2-AHG7HW: read-only tool capacity and selection.
        "tool_capacity" => {
            return backend
                .get(&format!("{base}/tool-capacity"), &[])
                .map_err(crate::be_msg);
        }
        "tool_select" => {
            let load = match args.get("tool_load") {
                Some(load @ Value::Array(_)) => load.clone(),
                _ => return Err("tool_load must be an array of observed session counts".into()),
            };
            let body = json!({"target_id":required(args, "target_id")?,
                "tool":optional_id(args, "tool")?,"model":optional_id(args, "model")?,
                "load":load});
            return backend
                .send("POST", &format!("{base}/tool-capacity/select"), &body)
                .map_err(crate::be_msg);
        }
        "policy_validate" | "policy_dry_run" => {
            let mut body = json!({"policy":args.get("policy").cloned().unwrap_or(Value::Null)});
            if action == "policy_dry_run" {
                body["actor_id"] = args.get("for_actor").cloned().unwrap_or(Value::Null);
            }
            let leaf = if action == "policy_validate" {
                "validate"
            } else {
                "dry-run"
            };
            return backend
                .send("POST", &format!("{base}/policy/{leaf}"), &body)
                .map_err(crate::be_msg);
        }
        _ => {}
    }
    // HS2-5JBYPV: gates of runs. State comes from provider adapters only.
    if action == "deployment_request" || action.starts_with("gate_") {
        let ulid = |name: &str| -> Result<&str, String> {
            let id = required(args, name)?;
            if id.len() != 26 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                return Err(format!("canonical {name} required"));
            }
            Ok(id)
        };
        let body = |keys: &[(&str, &str)]| {
            let mut body = serde_json::Map::new();
            for (argument, field) in keys {
                if let Some(value) = args.get(*argument) {
                    body.insert((*field).into(), value.clone());
                }
            }
            Value::Object(body)
        };
        return match action {
            "gate_list" => {
                let mut query = Vec::new();
                if args.get("run_id").is_some() {
                    query.push(format!("run={}", ulid("run_id")?));
                }
                if let Some(target) = id {
                    query.push(format!("target_id={target}"));
                }
                if let Some(commit) = optional_id(args, "commit")? {
                    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        return Err("commit must be all 40 hex characters".into());
                    }
                    query.push(format!("commit={commit}"));
                }
                backend.get(&format!("{base}/gates?{}", query.join("&")), &[])
            }
            "gate_show" => backend.get(&format!("{base}/gates/{}", ulid("gate_id")?), &[]),
            "gate_link" => backend.send(
                "POST",
                &format!("{base}/runs/{}/gates", ulid("run_id")?),
                &body(&[
                    ("commit", "commit"),
                    ("repository", "repository"),
                    ("workflow", "workflow"),
                    ("branch", "branch"),
                    ("gate_provider", "provider"),
                ]),
            ),
            "gate_sync" => backend.send(
                "POST",
                &format!("{base}/runs/{}/gate-sync", ulid("run_id")?),
                &body(&[("wait_ms", "wait_ms")]),
            ),
            "deployment_request" => backend.send(
                "POST",
                &format!("{base}/runs/{}/deployments", ulid("run_id")?),
                &body(&[
                    ("environment", "environment"),
                    ("script", "script"),
                    ("production", "production"),
                    ("operation_id", "operation_id"),
                    ("gate_provider", "provider"),
                    ("artifact", "artifact"),
                ]),
            ),
            _ => return Err("unsupported target action".into()),
        }
        .map_err(crate::be_msg);
    }
    // HS2-GDJXGZ: orchestrator-owned sessions. Forcing a stop, taking over and pausing
    // are a person's CLI actions and have no action here.
    if let Some(operation) = action.strip_prefix("session_") {
        let sessions = format!("{base}/sessions");
        let session = || -> Result<String, String> {
            let id = required(args, "session_id")?;
            if id.len() != 26 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                return Err("canonical session id required".into());
            }
            Ok(format!("{sessions}/{id}"))
        };
        return match operation {
            "list" => backend.get(
                &format!(
                    "{sessions}?active={}",
                    args.get("active").and_then(Value::as_bool).unwrap_or(false)
                ),
                &[],
            ),
            "show" => backend.get(&session()?, &[]),
            "start" => {
                let mut body = serde_json::Map::new();
                body.insert(
                    "target_id".into(),
                    id.ok_or("target_id is required")?.into(),
                );
                body.insert(
                    "operation_id".into(),
                    required(args, "operation_id")?.into(),
                );
                body.insert("tool".into(), required(args, "tool")?.into());
                for (argument, field) in [
                    ("model", "model"),
                    ("effort", "effort"),
                    ("account", "account"),
                    ("session_role", "role"),
                    ("run_id", "run_id"),
                    ("replaces", "replaces"),
                ] {
                    if let Some(value) = args.get(argument) {
                        body.insert(field.into(), value.clone());
                    }
                }
                backend.send("POST", &sessions, &Value::Object(body))
            }
            // Without a session id, every unfinished session of the checkout.
            "sync" => match args.get("session_id") {
                None | Some(Value::Null) => {
                    backend.send("POST", &format!("{base}/session-sync"), &json!({}))
                }
                Some(_) => backend.send("POST", &format!("{}/sync", session()?), &json!({})),
            },
            "safe_point" => backend.send("POST", &format!("{}/safe-point", session()?), &json!({})),
            // HS2-R7QH35: tell a running session which exact ticket its run is for.
            "request" => backend.send("POST", &format!("{}/request", session()?), &json!({})),
            "stop" => backend.send("POST", &format!("{}/stop", session()?), &json!({})),
            _ => return Err("unsupported target action".into()),
        }
        .map_err(crate::be_msg);
    }
    // HS2-EBHDSB: dispatch runs.
    if let Some(operation) = action.strip_prefix("run_") {
        let runs = format!("{base}/runs");
        let run = || -> Result<String, String> {
            let id = required(args, "run_id")?;
            if id.len() != 26 || !id.bytes().all(|byte| byte.is_ascii_alphanumeric()) {
                return Err("canonical run id required".into());
            }
            Ok(format!("{runs}/{id}"))
        };
        return match operation {
            "list" => backend.get(
                &format!(
                    "{runs}?active={}",
                    args.get("active").and_then(Value::as_bool).unwrap_or(false)
                ),
                &[],
            ),
            "show" => backend.get(&run()?, &[]),
            // HS2-7QHWHJ
            "status" => backend.get(&format!("{}/status", run()?), &[]),
            "events" => {
                let number = |name: &str, default: u64| match args.get(name) {
                    None => Ok(default),
                    Some(value) => value
                        .as_u64()
                        .ok_or_else(|| format!("{name} must be an unsigned integer when present")),
                };
                let filter = match args.get("run_id") {
                    None => String::new(),
                    Some(_) => format!("&run={}", run()?.rsplit('/').next().unwrap_or_default()),
                };
                backend.get(
                    &format!(
                        "{base}/run-events?after={}&limit={}&wait_ms={}{filter}",
                        number("after", 0)?,
                        number("limit", 100)?,
                        number("wait_ms", 0)?
                    ),
                    &[],
                )
            }
            "dispatch" => {
                let mut body = serde_json::Map::new();
                for key in [
                    "ticket",
                    "connection_id",
                    "native_id",
                    "operation_id",
                    "mode",
                    "worker_id",
                    "worker_label",
                    "lease_minutes",
                    // HS2-42TARA
                    "override_readiness",
                ] {
                    if let Some(value) = args.get(key) {
                        body.insert(key.into(), value.clone());
                    }
                }
                body.insert(
                    "target_id".into(),
                    id.ok_or("target_id is required")?.into(),
                );
                backend.send("POST", &runs, &Value::Object(body))
            }
            "renew" => backend.send(
                "POST",
                &format!("{}/renew", run()?),
                &json!({"lease_minutes":args.get("lease_minutes").cloned().unwrap_or(Value::Null)}),
            ),
            "cancel" | "sync" | "resume" => {
                backend.send("POST", &format!("{}/{operation}", run()?), &json!({}))
            }
            // HS2-WBREDM: without a run id, every unfinished run of the checkout.
            "reconcile" => match args.get("run_id") {
                None | Some(Value::Null) => {
                    backend.send("POST", &format!("{base}/run-reconcile"), &json!({}))
                }
                Some(_) => backend.send("POST", &format!("{}/reconcile", run()?), &json!({})),
            },
            "pause" | "handoff" | "correct" => {
                let keys: &[&str] = match operation {
                    "pause" => &["reason"],
                    "handoff" => &["to_worker", "worker_label", "lease_minutes"],
                    _ => &["state", "reason", "worker_id"],
                };
                let mut body = serde_json::Map::new();
                for key in keys {
                    if let Some(value) = args.get(*key) {
                        body.insert((*key).into(), value.clone());
                    }
                }
                backend.send(
                    "POST",
                    &format!("{}/{operation}", run()?),
                    &Value::Object(body),
                )
            }
            _ => return Err("unsupported target action".into()),
        }
        .map_err(crate::be_msg);
    }
    // HS2-AZ15GA: target tickets for an orchestration ticket.
    if let Some(operation) = action.strip_prefix("ticket_") {
        let mut body = serde_json::Map::new();
        if let Some(Value::Object(fields)) = args.get("fields") {
            body.extend(fields.clone());
        }
        // Named arguments win over anything a `fields` object tried to set.
        for key in [
            "ticket",
            "connection_id",
            "native_id",
            "operation_id",
            "expected_token",
            "relation",
            "resolve",
        ] {
            if let Some(value) = args.get(key) {
                body.insert(key.into(), value.clone());
            }
        }
        if operation != "links" {
            body.insert(
                "target_id".into(),
                id.ok_or("target_id is required")?.into(),
            );
        }
        if !matches!(
            operation,
            "create" | "read" | "update" | "link" | "unlink" | "links"
        ) {
            return Err("unsupported target action".into());
        }
        return backend
            .send(
                "POST",
                &format!("{base}/tickets/{operation}"),
                &Value::Object(body),
            )
            .map_err(crate::be_msg);
    }
    // HS2-WN0MXZ: validation steps of an orchestration ticket.
    if let Some(operation) = action.strip_prefix("validation_") {
        if !matches!(operation, "define" | "list" | "evidence" | "run" | "remove") {
            return Err("unsupported target action".into());
        }
        // Each operation's body is strict, so only its own arguments are forwarded.
        let keys: &[&str] = match operation {
            "define" | "list" => &["ticket"],
            "run" => &["ticket", "step_id", "operation_id", "run_id", "wait_ms"],
            _ => &["ticket", "step_id"],
        };
        let mut body = serde_json::Map::new();
        for key in keys.iter().copied() {
            if let Some(value) = args.get(key) {
                body.insert(key.into(), value.clone());
            }
        }
        if operation == "define" {
            let Some(Value::Object(step)) = args.get("step") else {
                return Err("step is required".into());
            };
            let mut step = step.clone();
            // The validated argument wins over anything the step object tried to set.
            step.insert(
                "target_id".into(),
                id.ok_or("target_id is required")?.into(),
            );
            body.insert("step".into(), Value::Object(step));
        }
        return backend
            .send(
                "POST",
                &format!("{base}/validations/{operation}"),
                &Value::Object(body),
            )
            .map_err(crate::be_msg);
    }
    // HS2-42TARA: the plan of prerequisites and its readiness.
    if let Some(operation) = action.strip_prefix("readiness_") {
        // Each operation's body is strict, so only its own arguments are forwarded.
        let keys: &[&str] = match operation {
            "define" => &[
                "ticket",
                "connection_id",
                "native_id",
                "prerequisites",
                "resources",
                // HS2-HCW7MD: the preferred AI tool of the step.
                "tool",
            ],
            "show" | "remove" | "refresh" => &["step_id"],
            "plan" | "ready" | "speculation" => &[],
            // HS2-JSF985
            "reopen" => &["step_id", "reason"],
            _ => return Err("unsupported target action".into()),
        };
        let mut body = serde_json::Map::new();
        for key in keys.iter().copied() {
            if let Some(value) = args.get(key) {
                body.insert(key.into(), value.clone());
            }
        }
        match operation {
            "define" => {
                body.insert(
                    "target_id".into(),
                    id.ok_or("target_id is required")?.into(),
                );
            }
            "show" | "remove" => {
                required(args, "step_id")?;
            }
            "reopen" => {
                required(args, "step_id")?;
                required(args, "reason")?;
            }
            _ => {}
        }
        return backend
            .send(
                "POST",
                &format!("{base}/readiness/{operation}"),
                &Value::Object(body),
            )
            .map_err(crate::be_msg);
    }
    // HS2-HCW7MD: Propose Plan starts the project's planning session; plan_show reads
    // where planning stands.
    if let Some(operation) = action.strip_prefix("plan_") {
        let keys: &[&str] = match operation {
            "propose" => &["tool", "model", "effort", "goal", "operation_id"],
            "show" => &[],
            _ => return Err("unsupported target action".into()),
        };
        if operation == "propose" {
            required(args, "tool")?;
        }
        let mut body = serde_json::Map::new();
        for key in keys.iter().copied() {
            if let Some(value) = args.get(key) {
                body.insert(key.into(), value.clone());
            }
        }
        return backend
            .send(
                "POST",
                &format!("{base}/planning/{operation}"),
                &Value::Object(body),
            )
            .map_err(crate::be_msg);
    }
    // HS2-ECWG50: the scheduler's plan, explanation, decision log and tick.
    if let Some(operation) = action.strip_prefix("scheduler_") {
        let keys: &[&str] = match operation {
            "plan" => &[],
            "explain" => &["step_id", "connection_id", "native_id"],
            "decisions" => &["after", "limit"],
            "tick" => &["dry_run"],
            _ => return Err("unsupported target action".into()),
        };
        let mut body = serde_json::Map::new();
        for key in keys.iter().copied() {
            if let Some(value) = args.get(key) {
                body.insert(key.into(), value.clone());
            }
        }
        if operation == "explain" {
            match (id, body.contains_key("step_id")) {
                (Some(target_id), false) => {
                    required(args, "connection_id")?;
                    required(args, "native_id")?;
                    body.insert("target_id".into(), target_id.into());
                }
                (None, true) => {
                    body.remove("connection_id");
                    body.remove("native_id");
                }
                _ => return Err("scheduler_explain takes step_id or one target ticket".into()),
            }
        }
        return backend
            .send(
                "POST",
                &format!("{base}/scheduler/{operation}"),
                &Value::Object(body),
            )
            .map_err(crate::be_msg);
    }
    if action == "inspect" {
        let checkout_id = required(args, "destination_checkout")?;
        let destination = match registration {
            Some(registration_id) => {
                json!({"kind":"remote","registration_id":registration_id,"checkout_id":checkout_id})
            }
            None => json!({"kind":"local","checkout_id":checkout_id}),
        };
        return backend
            .send(
                "POST",
                &format!("/checkouts/{owner}/orchestration/inspect"),
                &json!({"destination":destination}),
            )
            .map_err(crate::be_msg);
    }
    let target = || json!({"owner_checkout_id":owner,"target_id":id});
    let revision = args
        .get("expected_revision")
        .cloned()
        .unwrap_or(Value::Null);
    match action {
        "add" => backend.send("POST", &prefix, &json!({"expected_revision":revision,"target_id":id,
            "label":required(args,"label")?,"expected_repository_identity":args.get("expected_repository_identity").cloned().unwrap_or(Value::Null)})),
        "rename" => backend.send("PATCH", &format!("{prefix}/{}",id.ok_or("target_id is required")?), &json!({"expected_revision":required(args,"expected_revision")?,"label":required(args,"label")?,"expected_repository_identity":args.get("expected_repository_identity").cloned().unwrap_or(Value::Null),"clear_expected_repository_identity":args.get("clear_expected_repository_identity").cloned().unwrap_or(json!(false))})),
        "remove" => backend.send("DELETE", &format!("{prefix}/{}",id.ok_or("target_id is required")?), &json!({"expected_revision":required(args,"expected_revision")?})),
        "bind" | "unbind" => {
            let id = id.ok_or("target_id is required")?;
            let expected = args.get("expected_binding_revision").and_then(Value::as_u64).ok_or("expected_binding_revision is required")?;
            let mut body = json!({"target":target(),"expected_binding_revision":expected,
                "previous_destination":args.get("previous_destination").cloned().unwrap_or(Value::Null)});
            if action == "bind" {
                body["expected_manifest_revision"] = required(args,"expected_manifest_revision")?.into();
                body["destination"] = args.get("destination").cloned().ok_or("destination is required")?;
            }
            backend.send(if action == "bind" {"PUT"} else {"DELETE"}, &format!("{prefix}/{id}/binding"), &body)
        }
        "resolve" => backend.get(&format!("{prefix}/{}/resolve", id.ok_or("target_id is required")?), &[]),
        // HS2-J8PRTV: both are sent with a body so the caller's actor passes the gate.
        "capacity" => backend.send("POST", &format!("{prefix}/{}/capacity", id.ok_or("target_id is required")?), &json!({
            "claim_namespaces":args.get("claim_namespaces").cloned().unwrap_or_else(|| json!([]))})),
        "admission_check" => backend.send("POST", &format!("{prefix}/{}/admission", id.ok_or("target_id is required")?), &json!({
            "tool":required(args,"tool")?,"model":args.get("model").cloned().unwrap_or(Value::Null),
            "sessions":args.get("sessions").cloned().unwrap_or(Value::Null)})),
        "grant_list" => backend.get(&format!("{prefix}/{}/grants", id.ok_or("target_id is required")?), &[]),
        "access_check" => backend.send("POST", &format!("{prefix}/{}/access", id.ok_or("target_id is required")?), &json!({
            "actor_id":required(args,"for_actor")?,"action":required(args,"target_action")?,
            "session":args.get("session").cloned().unwrap_or(Value::Null),
            "load":args.get("load").cloned().unwrap_or(Value::Null),
            "operation_id":args.get("operation_id").cloned().unwrap_or(Value::Null)})),
        _ => return Err("unsupported target action".into()),
    }.map_err(crate::be_msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_sync::LockExt;
    use hotsheet_ticketing::orchestration::{CheckoutProjection, TargetFailure};
    use hotsheet_ticketing::remote_servers::{
        RemoteCheckoutTransport, RemoteCredentials, RemoteFailure, RemoteSnapshot, RemoteTransport,
    };
    use hotsheet_ticketing::{
        FsStore, ProviderConfigRegistry, ProviderConnection, SecretError, SecretStore,
        StoreMetadata,
    };
    use std::sync::{Arc, Mutex};
    struct OffBox {
        calls: Mutex<Vec<String>>,
    }
    impl Backend for OffBox {
        fn get(&self, path: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls.lock_or_recover().push(path.into());
            Ok(json!({"targets":[]}))
        }
        fn send(&self, _: &str, path: &str, _: &Value) -> Result<Value, BackendError> {
            self.calls.lock_or_recover().push(path.into());
            Ok(Value::Null)
        }
    }
    #[test]
    fn hs2_4j6v46_offbox_admin_preflight_precedes_backend_and_actual_origin_is_rechecked() {
        let backend = OffBox {
            calls: Mutex::new(Vec::new()),
        };
        for action in [
            "bind",
            "unbind",
            "resolve",
            "inspect",
            // HS2-SBQ3T8: delegated authority and its journal are machine-local reads.
            "grant_list",
            "access_check",
            "audit",
            // HS2-YCJDEB
            "policy_dry_run",
            "approval_list",
            // HS2-AZ15GA
            "ticket_create",
            "ticket_read",
            "ticket_update",
            "ticket_link",
            "ticket_unlink",
            "ticket_links",
            // HS2-EBHDSB
            "run_dispatch",
            "run_list",
            "run_show",
            "run_renew",
            "run_cancel",
            "run_sync",
            // HS2-7QHWHJ
            "run_status",
            "run_events",
            // HS2-5JBYPV
            "gate_link",
            "gate_list",
            "gate_show",
            "gate_sync",
            "deployment_request",
            // HS2-WBREDM
            "run_reconcile",
            "run_pause",
            "run_resume",
            "run_handoff",
            "run_correct",
            // HS2-S13719
            "summary",
            // HS2-J89JC7
            "timeline",
            // HS2-AHG7HW
            "tool_capacity",
            "tool_select",
            // HS2-J8PRTV
            "capacity",
            "admission_check",
            // HS2-WN0MXZ
            "validation_define",
            "validation_list",
            "validation_evidence",
            "validation_run",
            "validation_remove",
            // HS2-42TARA
            "readiness_define",
            "readiness_show",
            "readiness_plan",
            "readiness_ready",
            "readiness_refresh",
            "readiness_remove",
            // HS2-JSF985
            "readiness_speculation",
            "readiness_reopen",
            // HS2-GDJXGZ
            "session_start",
            "session_list",
            "session_show",
            "session_sync",
            "session_safe_point",
            "session_request",
            "session_stop",
            // HS2-ECWG50
            "scheduler_plan",
            "scheduler_explain",
            "scheduler_decisions",
            "scheduler_tick",
            // HS2-HCW7MD
            "plan_propose",
            "plan_show",
        ] {
            assert!(
                dispatch(&json!({"action":action}), &backend)
                    .unwrap_err()
                    .contains("machine-local")
            );
        }
        assert!(
            dispatch(
                &json!({"action":"list","target_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
                &backend
            )
            .is_err()
        );
        assert!(backend.calls.lock_or_recover().is_empty());
        dispatch(
            &json!({"action":"list","checkout":"owner-0123456789ab"}),
            &backend,
        )
        .unwrap();
        assert_eq!(backend.calls.lock_or_recover().len(), 1);
        let http = crate::HttpBackend::new("https://offbox.example.test", "synthetic");
        for path in [
            "/checkouts/owner/orchestration/targets/id/binding",
            "/checkouts/owner/orchestration/targets/id/resolve",
            "/checkouts/owner/orchestration/inspect",
            "/checkouts/owner/orchestration/targets/id/grants",
            "/checkouts/owner/orchestration/targets/id/access",
            "/checkouts/owner/orchestration/audit?after=0&limit=1",
        ] {
            assert_eq!(http.get(path, &[]).unwrap_err().status, Some(403));
            assert_eq!(
                http.send("POST", path, &Value::Null).unwrap_err().status,
                Some(403)
            );
        }
    }
    struct Never;
    impl SecretStore for Never {
        fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
            panic!("local flow read native credentials")
        }
        fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
            panic!("local flow wrote credentials")
        }
        fn delete(&self, _: &str) -> Result<bool, SecretError> {
            panic!("local flow deleted credentials")
        }
    }
    impl RemoteTransport for Never {
        fn probe(&self, _: &str, _: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
            panic!("local flow performed remote discovery")
        }
    }
    impl RemoteCheckoutTransport for Never {
        fn checkout_projection(
            &self,
            _: &str,
            _: &RemoteCredentials,
            _: &str,
        ) -> Result<CheckoutProjection, TargetFailure> {
            panic!("local flow performed remote metadata request")
        }
    }
    fn call(backend: &dyn Backend, owner: &str, args: Value) -> Value {
        let reply=crate::handle_message_for_checkout(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call", "params":{"name":"hotsheet_targets","arguments":args}}),backend,Some(owner)).unwrap();
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
    fn rejected(backend: &dyn Backend, owner: &str, args: Value) {
        let reply = crate::handle_message_for_checkout(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hotsheet_targets","arguments":args}}), backend, Some(owner)).unwrap();
        assert_eq!(reply["result"]["isError"], true, "{reply}");
    }
    struct Counted<'a> {
        inner: &'a dyn Backend,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Backend for Counted<'_> {
        fn machine_local(&self) -> bool {
            self.inner.machine_local()
        }
        fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.get(path, query)
        }
        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.send(method, path, body)
        }
    }
    #[test]
    fn hs2_4j6v46_tools_call_malformed_optional_ids_never_reach_backend() {
        let backend = OffBox {
            calls: Mutex::new(Vec::new()),
        };
        for bad in [
            json!(123),
            Value::Null,
            json!(true),
            json!({"id":"synthetic"}),
        ] {
            for _ in 0..2 {
                rejected(
                    &backend,
                    "owner",
                    json!({"action":"add","target_id":bad,"label":"Synthetic","expected_revision":null}),
                );
                rejected(
                    &backend,
                    "owner",
                    json!({"action":"inspect","registration_id":bad,"destination_checkout":"owner"}),
                );
            }
        }
        assert!(backend.calls.lock_or_recover().is_empty());
    }
    #[test]
    fn hs2_4j6v46_real_core_tool_parent_inherits_project_and_orphan_cleanup_avoids_live_constructor()
     {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let store = FsStore::init(temp.path().join("host"), &StoreMetadata::new("SYN")).unwrap();
        let providers = ProviderConfigRegistry::new(store.root().join("providers.json"));
        providers
            .save(&[ProviderConnection {
                id: "synthetic".into(),
                provider: "github".into(),
                locator: "synthetic/repo".into(),
                name: None,
                default: false,
                disabled: false,
                settings: Value::Null,
            }])
            .unwrap();
        let registry =
            hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let owner = registry
            .register_registered_sources(
                &providers,
                &project,
                None,
                None,
                vec![hotsheet_ticketing::checkouts::TicketSource {
                    connection_id: "synthetic".into(),
                    provider: "github".into(),
                    locator: "synthetic/repo".into(),
                }],
                None,
            )
            .unwrap();
        let runtime = hotsheet_cli::orchestration::OrchestrationRuntime::new(
            home.clone(),
            registry.clone(),
            providers.clone(),
            Arc::new(Never),
            Arc::new(Never),
            hotsheet_model::Ulid::from(10u128).to_string(),
        )
        .unwrap();
        let backend = crate::CoreBackend::new(store.clone())
            .with_checkout_registry(registry.clone())
            .with_orchestration_runtime(runtime);
        let inspect = call(
            &backend,
            &owner.id,
            json!({"action":"inspect","destination_checkout":owner.id}),
        );
        assert_eq!(inspect["usable"], false);
        assert!(!home.join("orchestration-bindings.json").exists());
        let manifest = project.join(".hotsheet2/orchestration.json");
        let counted = Counted {
            inner: &backend,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        for bad in [
            json!(123),
            Value::Null,
            json!(true),
            json!({"id":"synthetic"}),
        ] {
            for _ in 0..2 {
                rejected(
                    &counted,
                    &owner.id,
                    json!({"action":"add","target_id":bad,"label":"Synthetic","expected_revision":null}),
                );
                rejected(
                    &counted,
                    &owner.id,
                    json!({"action":"inspect","registration_id":bad,"destination_checkout":owner.id}),
                );
                assert!(!manifest.exists());
                assert!(!home.join("orchestration-bindings.json").exists());
            }
        }
        assert_eq!(counted.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let added = call(
            &backend,
            &owner.id,
            json!({"action":"add","target_id":id,"label":"Synthetic","expected_revision":null}),
        );
        let renamed = call(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":added["revision"],"expected_repository_identity":"https://repo.example.test/group/project"}),
        );
        assert_eq!(renamed["targets"][0]["target_id"], id);
        assert_eq!(renamed["targets"][0]["label"], "Renamed");
        assert_eq!(
            renamed["targets"][0]["expected_repository_identity"],
            "https://repo.example.test/group/project"
        );
        assert_ne!(renamed["revision"], added["revision"]);
        assert_eq!(call(&backend, &owner.id, json!({"action":"list"})), renamed);
        let bytes = std::fs::read(&manifest).unwrap();
        for bad in [json!(123), Value::Null, json!(false), json!({})] {
            for _ in 0..2 {
                rejected(
                    &counted,
                    &owner.id,
                    json!({"action":"add","target_id":bad,"label":"Synthetic","expected_revision":renamed["revision"]}),
                );
                assert_eq!(std::fs::read(&manifest).unwrap(), bytes);
            }
        }
        assert_eq!(counted.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        rejected(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Wrong","expected_revision":renamed["revision"],"expected_repository_identity":"https://repo.example.test/other","clear_expected_repository_identity":true}),
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), bytes);
        let cleared = call(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":renamed["revision"],"clear_expected_repository_identity":true}),
        );
        assert_eq!(cleared["targets"][0]["target_id"], id);
        assert_eq!(cleared["targets"][0]["label"], "Renamed");
        assert!(
            cleared["targets"][0]
                .get("expected_repository_identity")
                .is_none()
        );
        assert_ne!(cleared["revision"], renamed["revision"]);
        let before = std::fs::read(&manifest).unwrap();
        rejected(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Stale","expected_revision":renamed["revision"]}),
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), before);
        assert_eq!(
            call(
                &backend,
                &owner.id,
                json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":cleared["revision"],"clear_expected_repository_identity":true})
            ),
            cleared
        );
        let after_clear = call(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"After clear","expected_revision":cleared["revision"]}),
        );
        assert_eq!(after_clear["targets"][0]["target_id"], id);
        assert_eq!(after_clear["targets"][0]["label"], "After clear");
        assert!(
            after_clear["targets"][0]
                .get("expected_repository_identity")
                .is_none()
        );
        assert_ne!(after_clear["revision"], cleared["revision"]);
        let cleared = after_clear;
        assert!(!home.join("orchestration-bindings.json").exists());
        let bound = call(
            &backend,
            &owner.id,
            json!({"action":"bind","target_id":id,"expected_manifest_revision":cleared["revision"],"expected_binding_revision":0,"previous_destination":null,"destination":inspect["destination"]}),
        );
        assert_eq!(bound["usable"], true);
        assert_eq!(
            call(
                &backend,
                &owner.id,
                json!({"action":"resolve","target_id":id})
            )["usable"],
            true
        );
        assert_eq!(
            call(&backend, &owner.id, json!({"action":"list"}))["targets"][0]["target_id"],
            id
        );
        call(
            &backend,
            &owner.id,
            json!({"action":"remove","target_id":id,"expected_revision":cleared["revision"]}),
        );
        let current = call(&backend, &owner.id, json!({"action":"list"}));
        let generated = call(
            &backend,
            &owner.id,
            json!({"action":"add","label":"Generated","expected_revision":current["revision"]}),
        );
        let generated_id = generated["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["label"] == "Generated")
            .unwrap()["target_id"]
            .as_str()
            .unwrap();
        assert_ne!(generated_id, id);
        assert_eq!(
            generated_id
                .parse::<hotsheet_model::Ulid>()
                .unwrap()
                .to_string(),
            generated_id
        );
        std::fs::write(registry.path(), b"corrupt").unwrap();
        std::fs::write(providers.path(), b"corrupt").unwrap();
        std::fs::write(home.join("server-identity.json"), b"corrupt").unwrap();
        let cleanup = crate::CoreBackend::new(store).with_checkout_registry(registry);
        assert_eq!(
            call(&cleanup, &owner.id, json!({"action":"list","target_id":id}))["revision"],
            1
        );
        assert_eq!(
            call(
                &cleanup,
                &owner.id,
                json!({"action":"unbind","target_id":id,"expected_binding_revision":1,"previous_destination":inspect["destination"]})
            )["revision"],
            2
        );
        assert_eq!(
            std::fs::read(home.join("server-identity.json")).unwrap(),
            b"corrupt"
        );
    }
    /// A machine-local backend that records what the tool sent.
    struct Recorded(Mutex<Vec<(String, String, Value)>>);
    impl Backend for Recorded {
        fn machine_local(&self) -> bool {
            true
        }
        fn get(&self, path: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
            self.0
                .lock_or_recover()
                .push(("GET".into(), path.into(), Value::Null));
            Ok(Value::Null)
        }
        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            self.0
                .lock_or_recover()
                .push((method.into(), path.into(), body.clone()));
            Ok(json!({"outcome":"accepted"}))
        }
    }
    #[test]
    fn hs2_j8prtv_orchestration_capacity_and_admission_actions_carry_the_actor_in_a_body() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let prefix = format!("/checkouts/{owner}/orchestration/targets/{target}");
        call(
            &backend,
            owner,
            json!({"action":"capacity","target_id":target,"claim_namespaces":["builds"],
                "actor_role":"ai","actor_id":"scheduler-1"}),
        );
        call(
            &backend,
            owner,
            json!({"action":"capacity","target_id":target,"actor_role":"ai","actor_id":"scheduler-1"}),
        );
        call(
            &backend,
            owner,
            json!({"action":"admission_check","target_id":target,"tool":"codex","model":"model-a",
                "sessions":2,"actor_role":"ai","actor_id":"scheduler-1"}),
        );
        call(
            &backend,
            owner,
            json!({"action":"admission_check","target_id":target,"tool":"codex",
                "actor_role":"ai","actor_id":"scheduler-1"}),
        );
        let actor = json!({"role":"ai","id":"scheduler-1"});
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                (
                    "POST".to_owned(),
                    format!("{prefix}/capacity"),
                    json!({"claim_namespaces":["builds"],"actor":actor})
                ),
                (
                    "POST".to_owned(),
                    format!("{prefix}/capacity"),
                    json!({"claim_namespaces":[],"actor":actor})
                ),
                (
                    "POST".to_owned(),
                    format!("{prefix}/admission"),
                    json!({"tool":"codex","model":"model-a","sessions":2,"actor":actor})
                ),
                (
                    "POST".to_owned(),
                    format!("{prefix}/admission"),
                    json!({"tool":"codex","model":null,"sessions":null,"actor":actor})
                ),
            ]
        );
        // A missing target or tool never reaches the backend.
        backend.0.lock_or_recover().clear();
        rejected(&backend, owner, json!({"action":"capacity"}));
        rejected(
            &backend,
            owner,
            json!({"action":"admission_check","tool":"codex"}),
        );
        rejected(
            &backend,
            owner,
            json!({"action":"admission_check","target_id":target}),
        );
        assert!(backend.0.lock_or_recover().is_empty());
    }
    #[test]
    fn hs2_wn0mxz_validation_actions_send_one_body_shape_and_validate_first() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let base = format!("/checkouts/{owner}/orchestration/validations");
        let actor = json!({"role":"ai","id":"scheduler-1"});
        let ticket = json!({"native_id":"OWN-ABC123"});
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("ai");
            arguments["actor_id"] = json!("scheduler-1");
            arguments
        };
        call(
            &backend,
            owner,
            who(
                json!({"action":"validation_define","target_id":target,"ticket":ticket,
                // The validated argument replaces a target the step object names, and
                // arguments of other actions are not forwarded.
                "step":{"step_id":"smoke","target_id":"elsewhere","check":{"kind":"profile","name":"smoke"}},
                "wait_ms":5,"worker_id":"ignored"}),
            ),
        );
        call(
            &backend,
            owner,
            who(
                json!({"action":"validation_run","ticket":ticket,"step_id":"smoke",
                "operation_id":"op-1","run_id":target,"wait_ms":8000,"step":{"ignored":true}}),
            ),
        );
        call(&backend, owner, who(json!({"action":"validation_list"})));
        call(
            &backend,
            owner,
            who(json!({"action":"validation_evidence","ticket":ticket,"step_id":"smoke"})),
        );
        call(
            &backend,
            owner,
            who(json!({"action":"validation_remove","ticket":ticket,"step_id":"smoke"})),
        );
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                (
                    "POST".to_owned(),
                    format!("{base}/define"),
                    json!({"ticket":ticket,"actor":actor,
                        "step":{"step_id":"smoke","target_id":target,"check":{"kind":"profile","name":"smoke"}}})
                ),
                (
                    "POST".to_owned(),
                    format!("{base}/run"),
                    json!({"ticket":ticket,"step_id":"smoke","operation_id":"op-1",
                        "run_id":target,"wait_ms":8000,"actor":actor})
                ),
                (
                    "POST".to_owned(),
                    format!("{base}/list"),
                    json!({"actor":actor})
                ),
                (
                    "POST".to_owned(),
                    format!("{base}/evidence"),
                    json!({"ticket":ticket,"step_id":"smoke","actor":actor})
                ),
                (
                    "POST".to_owned(),
                    format!("{base}/remove"),
                    json!({"ticket":ticket,"step_id":"smoke","actor":actor})
                ),
            ]
        );
        // A missing step or target, a malformed target and an unknown operation never
        // reach the backend.
        backend.0.lock_or_recover().clear();
        for arguments in [
            json!({"action":"validation_define","target_id":target,"ticket":ticket}),
            json!({"action":"validation_define","target_id":target,"ticket":ticket,"step":"smoke"}),
            json!({"action":"validation_define","ticket":ticket,"step":{"step_id":"smoke"}}),
            json!({"action":"validation_define","target_id":"not-a-ulid","ticket":ticket,"step":{}}),
            json!({"action":"validation_explode","ticket":ticket}),
        ] {
            rejected(&backend, owner, arguments);
        }
        assert!(backend.0.lock_or_recover().is_empty());
    }
    #[test]
    fn hs2_2fg91x_readiness_define_forwards_resources_and_the_schema_names_them() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let ticket = json!({"native_id":"OWN-ABC123"});
        let resources = json!([{"namespace":"vm","resource":"macos-vm"}]);
        for arguments in [
            json!({"action":"readiness_define","target_id":target,"ticket":ticket,
                "connection_id":"tickets","native_id":"TGT-2","resources":resources}),
            // Only a definition takes them: other actions do not forward the argument.
            json!({"action":"readiness_plan","resources":resources}),
            json!({"action":"scheduler_plan","resources":resources}),
        ] {
            let mut arguments = arguments;
            arguments["actor_role"] = json!("ai");
            arguments["actor_id"] = json!("scheduler-1");
            call(&backend, owner, arguments);
        }
        let actor = json!({"role":"ai","id":"scheduler-1"});
        let post = |path: &str, body: Value| {
            (
                "POST".to_owned(),
                format!("/checkouts/{owner}/orchestration/{path}"),
                body,
            )
        };
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                post(
                    "readiness/define",
                    json!({"ticket":ticket,"connection_id":"tickets","native_id":"TGT-2",
                        "resources":resources,"target_id":target,"actor":actor})
                ),
                post("readiness/plan", json!({"actor":actor})),
                post("scheduler/plan", json!({"actor":actor})),
            ]
        );
        let definition = definition();
        let schema = &definition["inputSchema"]["properties"]["resources"];
        assert_eq!(
            (&schema["type"], &schema["maxItems"]),
            (&json!("array"), &json!(4))
        );
        let description = definition["description"].as_str().unwrap().to_owned();
        for word in ["resource_unknown", "placement.resources", "never claims"] {
            assert!(description.contains(word), "{word}");
        }
    }

    #[test]
    fn hs2_j89jc7_timeline_is_one_read_and_waits_only_with_a_version() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        for arguments in [
            json!({"action":"timeline"}),
            json!({"action":"timeline","version":null,"wait_ms":5000}),
            json!({"action":"timeline","version":"0a1b2c3d4e5f","wait_ms":5000}),
            json!({"action":"timeline","version":"0a1b2c3d4e5f","wait_ms":99_000}),
            json!({"action":"timeline","version":"0a1b2c3d4e5f"}),
        ] {
            call(&backend, owner, arguments);
        }
        let path = format!("/checkouts/{owner}/orchestration/timeline");
        let get = |query: &str| ("GET".to_owned(), format!("{path}{query}"), Value::Null);
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                get(""),
                get(""),
                get("?after=0a1b2c3d4e5f&wait_ms=5000"),
                get("?after=0a1b2c3d4e5f&wait_ms=20000"),
                get("?after=0a1b2c3d4e5f&wait_ms=0"),
            ]
        );
        backend.0.lock_or_recover().clear();
        for version in [json!(""), json!(7), json!("a&b=c"), json!("x".repeat(161))] {
            let mut arguments = json!({"action":"timeline","checkout":owner});
            arguments["version"] = version.clone();
            assert!(dispatch(&arguments, &backend).is_err(), "{version}");
        }
        assert!(backend.0.lock_or_recover().is_empty());
        let definition = definition();
        assert!(
            definition["inputSchema"]["properties"]["action"]["enum"]
                .as_array()
                .unwrap()
                .contains(&json!("timeline"))
        );
        let description = definition["description"].as_str().unwrap().to_owned();
        for word in ["waits_for_permission", "instead of polling", "working_now"] {
            assert!(description.contains(word), "{word}");
        }
    }

    #[test]
    fn hs2_s13719_summary_is_one_read_and_validates_its_ticket_first() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        for arguments in [
            json!({"action":"summary"}),
            json!({"action":"summary","ticket":null}),
            json!({"action":"summary","ticket":{"native_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}}),
            json!({"action":"summary","actor_role":"ai","actor_id":"scheduler-1",
                "ticket":{"native_id":"OWN-ABC123","connection_id":"tickets.1"}}),
        ] {
            call(&backend, owner, arguments);
        }
        let path = format!("/checkouts/{owner}/orchestration/summary");
        let get = |query: &str| ("GET".to_owned(), format!("{path}{query}"), Value::Null);
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                get(""),
                get(""),
                get("?ticket=01ARZ3NDEKTSV4RRFFQ69G5FAV"),
                get("?ticket=OWN-ABC123&connection_id=tickets.1"),
            ]
        );
        // A malformed narrowing never reaches the backend, and cannot extend the query.
        backend.0.lock_or_recover().clear();
        for ticket in [
            json!("OWN-ABC123"),
            json!({}),
            json!({"connection_id":"tickets"}),
            json!({"native_id":""}),
            json!({"native_id":7}),
            json!({"native_id":"a&ticket=b"}),
            json!({"native_id":"a/b"}),
            json!({"native_id":"OWN-ABC123","connection_id":"has space"}),
            json!({"native_id":"x".repeat(161)}),
        ] {
            let mut arguments = json!({"action":"summary","checkout":owner});
            arguments["ticket"] = ticket.clone();
            assert!(dispatch(&arguments, &backend).is_err(), "{ticket}");
        }
        assert!(backend.0.lock_or_recover().is_empty());
        let definition = definition();
        let actions = definition["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap();
        assert!(actions.contains(&json!("summary")));
        let description = definition["description"].as_str().unwrap().to_owned();
        for word in [
            "never read an unknown as zero",
            "cost.complete",
            "counts once",
        ] {
            assert!(description.contains(word), "{word}");
        }
    }

    #[test]
    fn hs2_42tara_readiness_actions_send_one_body_shape_and_validate_first() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let step = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
        let base = format!("/checkouts/{owner}/orchestration");
        let actor = json!({"role":"ai","id":"scheduler-1"});
        let ticket = json!({"native_id":"OWN-ABC123"});
        let needs = json!([{"kind":"ticket","target_id":target,"connection_id":"tickets",
            "native_id":"TGT-1","gates":"none"},{"kind":"validation","step_id":"smoke"}]);
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("ai");
            arguments["actor_id"] = json!("scheduler-1");
            arguments
        };
        for arguments in [
            // Arguments of other actions are not forwarded.
            json!({"action":"readiness_define","target_id":target,"ticket":ticket,
                "connection_id":"tickets","native_id":"TGT-2","prerequisites":needs,
                "step_id":step,"worker_id":"ignored"}),
            json!({"action":"readiness_show","step_id":step,"ticket":ticket}),
            json!({"action":"readiness_plan","step_id":step}),
            json!({"action":"readiness_ready"}),
            json!({"action":"readiness_refresh"}),
            json!({"action":"readiness_refresh","step_id":step}),
            json!({"action":"readiness_remove","step_id":step}),
            json!({"action":"run_dispatch","target_id":target,"ticket":ticket,
                "connection_id":"tickets","native_id":"TGT-2","operation_id":"op-1",
                "worker_id":"worker-a","override_readiness":true}),
        ] {
            call(&backend, owner, who(arguments));
        }
        let post = |path: &str, body: Value| ("POST".to_owned(), format!("{base}/{path}"), body);
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                post(
                    "readiness/define",
                    json!({"ticket":ticket,"connection_id":"tickets","native_id":"TGT-2",
                        "prerequisites":needs,"target_id":target,"actor":actor})
                ),
                post("readiness/show", json!({"step_id":step,"actor":actor})),
                post("readiness/plan", json!({"actor":actor})),
                post("readiness/ready", json!({"actor":actor})),
                post("readiness/refresh", json!({"actor":actor})),
                post("readiness/refresh", json!({"step_id":step,"actor":actor})),
                post("readiness/remove", json!({"step_id":step,"actor":actor})),
                post(
                    "runs",
                    json!({"ticket":ticket,"connection_id":"tickets","native_id":"TGT-2",
                        "operation_id":"op-1","worker_id":"worker-a","override_readiness":true,
                        "target_id":target,"actor":actor})
                ),
            ]
        );
        // A missing target or step and an unknown operation never reach the backend.
        backend.0.lock_or_recover().clear();
        for arguments in [
            json!({"action":"readiness_define","ticket":ticket,"connection_id":"tickets","native_id":"TGT-2"}),
            json!({"action":"readiness_define","target_id":"not-a-ulid","ticket":ticket}),
            json!({"action":"readiness_show"}),
            json!({"action":"readiness_remove"}),
            json!({"action":"readiness_explain","step_id":step}),
        ] {
            rejected(&backend, owner, arguments);
        }
        assert!(backend.0.lock_or_recover().is_empty());
        // Like every record of this machine, the plan is not served to a remote backend.
        struct Remote;
        impl Backend for Remote {
            fn machine_local(&self) -> bool {
                false
            }
            fn get(&self, _: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
                unreachable!("a remote backend is refused before any request")
            }
            fn send(&self, _: &str, _: &str, _: &Value) -> Result<Value, BackendError> {
                unreachable!("a remote backend is refused before any request")
            }
        }
        rejected(&Remote, owner, json!({"action":"readiness_ready"}));
    }
    #[test]
    fn hs2_jsf985_provisional_actions_send_one_body_shape_and_validate_first() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let step = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
        let base = format!("/checkouts/{owner}/orchestration");
        let actor = json!({"role":"ai","id":"scheduler-1"});
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("ai");
            arguments["actor_id"] = json!("scheduler-1");
            arguments
        };
        for arguments in [
            // Arguments of other actions are not forwarded.
            json!({"action":"readiness_speculation","step_id":step,"reason":"ignored"}),
            json!({"action":"readiness_reopen","step_id":step,
                "reason":"fix_changed_contract","worker_id":"ignored"}),
        ] {
            call(&backend, owner, who(arguments));
        }
        let post = |path: &str, body: Value| ("POST".to_owned(), format!("{base}/{path}"), body);
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                post("readiness/speculation", json!({"actor":actor})),
                post(
                    "readiness/reopen",
                    json!({"step_id":step,"reason":"fix_changed_contract","actor":actor})
                ),
            ]
        );
        // A reopen without its step or its reason never reaches the backend.
        backend.0.lock_or_recover().clear();
        for arguments in [
            json!({"action":"readiness_reopen","step_id":step}),
            json!({"action":"readiness_reopen","reason":"fix_changed_contract"}),
        ] {
            rejected(&backend, owner, arguments);
        }
        assert!(backend.0.lock_or_recover().is_empty());
        let definition = definition();
        let actions = definition["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap()
            .clone();
        for action in ["readiness_speculation", "readiness_reopen"] {
            assert!(actions.contains(&json!(action)), "{action}");
        }
        let description = definition["description"].as_str().unwrap().to_owned();
        for word in [
            "speculation.allowed",
            "readiness_provisional_refused",
            "target_speculation_disabled",
            "instead of polling",
        ] {
            assert!(description.contains(word), "{word}");
        }
    }

    #[test]
    fn hs2_ecwg50_scheduler_actions_send_one_body_shape_and_validate_first() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let step = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
        let base = format!("/checkouts/{owner}/orchestration");
        let actor = json!({"role":"ai","id":"scheduler-1"});
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("ai");
            arguments["actor_id"] = json!("scheduler-1");
            arguments
        };
        for arguments in [
            // Arguments of other actions are not forwarded.
            json!({"action":"scheduler_plan","step_id":step,"dry_run":true}),
            json!({"action":"scheduler_explain","step_id":step,"connection_id":"tickets"}),
            json!({"action":"scheduler_explain","target_id":target,"connection_id":"tickets",
                "native_id":"TGT-2","worker_id":"ignored"}),
            json!({"action":"scheduler_decisions"}),
            json!({"action":"scheduler_decisions","after":7,"limit":50,"dry_run":true}),
            json!({"action":"scheduler_tick"}),
            json!({"action":"scheduler_tick","dry_run":true,"after":3}),
        ] {
            call(&backend, owner, who(arguments));
        }
        let post = |path: &str, body: Value| ("POST".to_owned(), format!("{base}/{path}"), body);
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                post("scheduler/plan", json!({"actor":actor})),
                post("scheduler/explain", json!({"step_id":step,"actor":actor})),
                post(
                    "scheduler/explain",
                    json!({"connection_id":"tickets","native_id":"TGT-2","target_id":target,
                        "actor":actor})
                ),
                post("scheduler/decisions", json!({"actor":actor})),
                post(
                    "scheduler/decisions",
                    json!({"after":7,"limit":50,"actor":actor})
                ),
                post("scheduler/tick", json!({"actor":actor})),
                post("scheduler/tick", json!({"dry_run":true,"actor":actor})),
            ]
        );
        // Nothing to explain, both ways of naming it, a partial ticket and an unknown
        // operation never reach the backend.
        backend.0.lock_or_recover().clear();
        for arguments in [
            json!({"action":"scheduler_explain"}),
            json!({"action":"scheduler_explain","step_id":step,"target_id":target}),
            json!({"action":"scheduler_explain","target_id":target,"connection_id":"tickets"}),
            json!({"action":"scheduler_explain","target_id":"not-a-ulid","connection_id":"tickets",
                "native_id":"TGT-2"}),
            json!({"action":"scheduler_start"}),
        ] {
            rejected(&backend, owner, arguments);
        }
        assert!(backend.0.lock_or_recover().is_empty());
        // The schedule is this machine's and is not served to a remote backend.
        struct Remote;
        impl Backend for Remote {
            fn machine_local(&self) -> bool {
                false
            }
            fn get(&self, _: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
                unreachable!("a remote backend is refused before any request")
            }
            fn send(&self, _: &str, _: &str, _: &Value) -> Result<Value, BackendError> {
                unreachable!("a remote backend is refused before any request")
            }
        }
        rejected(&Remote, owner, json!({"action":"scheduler_plan"}));
    }
    #[test]
    fn hs2_hcw7md_plan_actions_forward_only_their_own_arguments() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let base = format!("/checkouts/{owner}/orchestration");
        let actor = json!({"role":"human","id":"maintainer"});
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("human");
            arguments["actor_id"] = json!("maintainer");
            arguments
        };
        for arguments in [
            json!({"action":"plan_propose","tool":"claude"}),
            json!({"action":"plan_propose","tool":"codex","model":"m","effort":"high",
                "goal":{"native_id":"OWN-1"},"operation_id":"press-1","step_id":"ignored",
                "session_role":"planner"}),
            json!({"action":"plan_show","tool":"ignored"}),
            // HS2-HCW7MD: a step's preferred tool is forwarded with its definition.
            json!({"action":"readiness_define","target_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV",
                "ticket":{"native_id":"OWN-1"},"connection_id":"tickets","native_id":"TGT-2",
                "tool":"claude"}),
        ] {
            call(&backend, owner, who(arguments));
        }
        let post = |path: &str, body: Value| ("POST".to_owned(), format!("{base}/{path}"), body);
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                post("planning/propose", json!({"tool":"claude","actor":actor})),
                post(
                    "planning/propose",
                    json!({"tool":"codex","model":"m","effort":"high",
                        "goal":{"native_id":"OWN-1"},"operation_id":"press-1","actor":actor})
                ),
                post("planning/show", json!({"actor":actor})),
                post(
                    "readiness/define",
                    json!({"ticket":{"native_id":"OWN-1"},"connection_id":"tickets",
                        "native_id":"TGT-2","tool":"claude",
                        "target_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV","actor":actor})
                ),
            ]
        );
        backend.0.lock_or_recover().clear();
        // A press needs its tool; an unknown planning action never reaches the backend.
        for arguments in [
            json!({"action":"plan_propose"}),
            json!({"action":"plan_start","tool":"claude"}),
        ] {
            rejected(&backend, owner, arguments);
        }
        assert!(backend.0.lock_or_recover().is_empty());
        let schema = definition();
        let actions = schema["inputSchema"]["properties"]["action"]["enum"]
            .as_array()
            .unwrap();
        for action in ["plan_propose", "plan_show"] {
            assert!(actions.contains(&json!(action)), "{action}");
        }
        assert!(
            schema["inputSchema"]["properties"]["goal"]["description"]
                .as_str()
                .unwrap()
                .contains("plan_propose")
        );
    }
    #[test]
    fn hs2_wbredm_orchestration_recovery_and_control_actions_reach_their_routes() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let run = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let base = format!("/checkouts/{owner}/orchestration");
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("human");
            arguments["actor_id"] = json!("maintainer");
            arguments
        };
        for arguments in [
            json!({"action":"run_reconcile"}),
            json!({"action":"run_reconcile","run_id":run}),
            json!({"action":"run_pause","run_id":run,"reason":"maintenance"}),
            json!({"action":"run_pause","run_id":run}),
            json!({"action":"run_resume","run_id":run}),
            json!({"action":"run_handoff","run_id":run,"to_worker":"worker-b",
                "worker_label":"Worker B","lease_minutes":45}),
            json!({"action":"run_handoff","run_id":run}),
            json!({"action":"run_correct","run_id":run,"state":"requested",
                "reason":"redispatch","worker_id":"worker-a"}),
        ] {
            call(&backend, owner, who(arguments));
        }
        let actor = json!({"role":"human","id":"maintainer"});
        let sent = |leaf: &str, mut body: Value| {
            body["actor"] = actor.clone();
            ("POST".to_owned(), format!("{base}/{leaf}"), body)
        };
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                sent("run-reconcile", json!({})),
                sent(&format!("runs/{run}/reconcile"), json!({})),
                sent(
                    &format!("runs/{run}/pause"),
                    json!({"reason":"maintenance"})
                ),
                sent(&format!("runs/{run}/pause"), json!({})),
                sent(&format!("runs/{run}/resume"), json!({})),
                sent(
                    &format!("runs/{run}/handoff"),
                    json!({"to_worker":"worker-b","worker_label":"Worker B","lease_minutes":45})
                ),
                sent(&format!("runs/{run}/handoff"), json!({})),
                sent(
                    &format!("runs/{run}/correct"),
                    json!({"state":"requested","reason":"redispatch","worker_id":"worker-a"})
                ),
            ]
        );
        // A control action names its run; a malformed id never reaches the backend.
        backend.0.lock_or_recover().clear();
        for action in ["run_pause", "run_resume", "run_handoff", "run_correct"] {
            rejected(&backend, owner, json!({"action":action}));
            rejected(
                &backend,
                owner,
                json!({"action":action,"run_id":"not-a-run"}),
            );
        }
        rejected(
            &backend,
            owner,
            json!({"action":"run_reconcile","run_id":"not-a-run"}),
        );
        assert!(backend.0.lock_or_recover().is_empty());
    }
    #[test]
    fn hs2_gdjxgz_session_actions_reach_their_routes_and_offer_no_person_only_act() {
        let backend = Recorded(Mutex::new(Vec::new()));
        let owner = "owner-0123456789ab";
        let target = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let session = "01ARZ3NDEKTSV4RRFFQ69G5FB0";
        let base = format!("/checkouts/{owner}/orchestration");
        let who = |mut arguments: Value| {
            arguments["actor_role"] = json!("ai");
            arguments["actor_id"] = json!("scheduler-1");
            arguments
        };
        for arguments in [
            json!({"action":"session_start","target_id":target,"operation_id":"op-1",
                "tool":"codex","model":"model-a","effort":"high","account":"team",
                "session_role":"integrator","run_id":session,"replaces":session}),
            json!({"action":"session_start","target_id":target,"operation_id":"op-2",
                "tool":"claude"}),
            json!({"action":"session_sync"}),
            json!({"action":"session_sync","session_id":session}),
            json!({"action":"session_safe_point","session_id":session}),
            json!({"action":"session_request","session_id":session}),
            // `force` is not an argument of this tool: it never reaches the route.
            json!({"action":"session_stop","session_id":session,"force":true}),
        ] {
            call(&backend, owner, who(arguments));
        }
        let actor = json!({"role":"ai","id":"scheduler-1"});
        let sent = |leaf: &str, mut body: Value| {
            body["actor"] = actor.clone();
            ("POST".to_owned(), format!("{base}/{leaf}"), body)
        };
        assert_eq!(
            *backend.0.lock_or_recover(),
            [
                sent(
                    "sessions",
                    json!({"target_id":target,"operation_id":"op-1","tool":"codex",
                        "model":"model-a","effort":"high","account":"team",
                        "role":"integrator","run_id":session,"replaces":session})
                ),
                sent(
                    "sessions",
                    json!({"target_id":target,"operation_id":"op-2","tool":"claude"})
                ),
                sent("session-sync", json!({})),
                sent(&format!("sessions/{session}/sync"), json!({})),
                sent(&format!("sessions/{session}/safe-point"), json!({})),
                sent(&format!("sessions/{session}/request"), json!({})),
                sent(&format!("sessions/{session}/stop"), json!({})),
            ]
        );
        // Missing or malformed arguments never reach the backend; nor does an act
        // only a person may take.
        backend.0.lock_or_recover().clear();
        for arguments in [
            json!({"action":"session_start","operation_id":"op-1","tool":"codex"}),
            json!({"action":"session_start","target_id":target,"tool":"codex"}),
            json!({"action":"session_start","target_id":target,"operation_id":"op-1"}),
            json!({"action":"session_show"}),
            json!({"action":"session_stop","session_id":"not-a-session"}),
            json!({"action":"session_safe_point"}),
            json!({"action":"session_request"}),
            json!({"action":"session_sync","session_id":"../x"}),
            json!({"action":"session_takeover","session_id":session}),
            json!({"action":"session_pause","session_id":session}),
        ] {
            rejected(&backend, owner, arguments);
        }
        assert!(backend.0.lock_or_recover().is_empty());
    }
}
