//! Shared transport adapters select a route only; all transitions live in core.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{AccessRequest, SetPolicy};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::tickets::identifier;
use super::*;

pub fn is_path(path: &str) -> bool {
    let parts = parts(path);
    matches!(
        parts.as_slice(),
        ["checkouts", _, "orchestration", "targets", ..]
            | ["checkouts", _, "orchestration", "inspect" | "audit"]
            // HS2-CRBR8X: this machine's journal of the sessions it hosted.
            | ["checkouts", _, "orchestration", "hosted-audit"]
            | ["checkouts", _, "orchestration", "policy"]
            // HS2-W27N1D: the project type and the projects an orchestration project adds.
            | ["checkouts", _, "orchestration", "project-type" | "projects"]
            | ["checkouts", _, "orchestration", "projects", _]
            // HS2-92FJXM: this server's open checkouts, which another machine may add.
            | ["checkouts", _, "orchestration", "hosted-projects"]
            | [
                "checkouts",
                _,
                "orchestration",
                "policy",
                "validate" | "dry-run"
            ]
            // HS2-XHQ1EH: permission requests and remembered rules.
            | ["checkouts", _, "orchestration", "permissions"]
            | ["checkouts", _, "orchestration", "permissions", _]
            | ["checkouts", _, "orchestration", "permission-rules"]
            | ["checkouts", _, "orchestration", "permission-rules", _]
            // HS2-JWV5S1: the retired grant and approval files.
            | ["checkouts", _, "orchestration", "retired-permissions"]
            // HS2-AZ15GA: orchestrating-side ticket operations and the hosted half.
            | ["checkouts", _, "orchestration", "tickets", _]
            | ["checkouts", _, "target-tickets"]
            // HS2-D85G19: one bounded page of a hosted source's open tickets.
            | ["checkouts", _, "target-tickets", "list"]
            // HS2-B6236S: one hosted ticket's cost reading.
            | ["checkouts", _, "target-tickets", "cost"]
            // HS2-EBHDSB: runs (orchestrating) and the hosted exact claim.
            | ["checkouts", _, "orchestration", "runs"]
            | ["checkouts", _, "orchestration", "runs", _]
            | ["checkouts", _, "orchestration", "runs", _, _]
            | ["checkouts", _, "orchestration", "run-events"]
            // HS2-5JBYPV: gates of runs. Linking, syncing and deployment requests are
            // `runs/{run}/{gates,gate-sync,deployments}` above.
            | ["checkouts", _, "orchestration", "gates"]
            | ["checkouts", _, "orchestration", "gates", _]
            // HS2-WBREDM: reconcile every unfinished run of the owner.
            | ["checkouts", _, "orchestration", "run-reconcile"]
            // HS2-S13719: the progress and cost summary of the owner's runs.
            | ["checkouts", _, "orchestration", "summary"]
            // HS2-J89JC7: the plan as dependency waves, a long poll by version.
            | ["checkouts", _, "orchestration", "timeline"]
            // HS2-S3FKJ7: the orchestration lines of a target checkout's tickets.
            | ["checkouts", _, "orchestration", "target-lines"]
            // HS2-3D8D6V: the same lines for a checkout on a remote server bound here.
            | ["remotes", _, "checkouts", _, "orchestration", "target-lines"]
            // HS2-KB2Y51: one push pass (orchestrating) and the hosted observations.
            | ["checkouts", _, "orchestration", "target-lines", "push"]
            | ["checkouts", _, "target-line-observations"]
            | ["checkouts", _, "target-claims"]
            | ["checkouts", _, "target-events"]
            // HS2-AHG7HW: tool capacity states, signals and selection.
            | ["checkouts", _, "orchestration", "tool-capacity"]
            | [
                "checkouts",
                _,
                "orchestration",
                "tool-capacity",
                "signals" | "select"
            ]
            // HS2-J8PRTV: the hosted capacity snapshot and admission answer.
            | ["checkouts", _, "target-capacity" | "target-admission"]
            // HS2-FJ6MDW: the hosted project's speculation setting alone.
            | ["checkouts", _, "target-speculation"]
            // HS2-G1PKK2: a long poll on the hosting machine's resource wait.
            | ["checkouts", _, "target-resource-wait"]
            // HS2-WN0MXZ: validation steps (orchestrating) and the hosted execution.
            | ["checkouts", _, "orchestration", "validations", _]
            | ["checkouts", _, "target-validations"]
            | ["checkouts", _, "target-validation-profiles"]
            // HS2-WDQZ37: a target's CI gate watched on the machine that hosts it.
            | ["checkouts", _, "target-gates"]
            // HS2-G3VAK4: a deployment run on the machine that hosts the target.
            | ["checkouts", _, "target-deployments"]
            // HS2-ZCR3Y7: kept step output, machine-local.
            | ["checkouts", _, "target-validation-output", _]
            // HS2-42TARA: the plan of prerequisites and its readiness.
            | ["checkouts", _, "orchestration", "readiness", _]
            // HS2-GDJXGZ: orchestrator-owned sessions (orchestrating) and the hosted half.
            | ["checkouts", _, "orchestration", "sessions"]
            | ["checkouts", _, "orchestration", "sessions", _]
            | ["checkouts", _, "orchestration", "sessions", _, _]
            | ["checkouts", _, "orchestration", "session-sync"]
            | ["checkouts", _, "target-sessions"]
            // HS2-ECWG50: the scheduler's plan, explanation, decision log and tick.
            | ["checkouts", _, "orchestration", "scheduler", _]
            // HS2-HCW7MD: Propose Plan and where planning stands.
            | ["checkouts", _, "orchestration", "planning", _]
            | ["remote-checkouts", _, "resolution"]
    )
}

/// Used by the serving-machine ACL and the actual HTTP-backend origin guard.
/// Lists/declarations/hosted metadata are ordinary authenticated project data.
pub fn is_machine_path(path: &str) -> bool {
    matches!(
        parts(path).as_slice(),
        [
            "checkouts",
            _,
            "orchestration",
            "targets",
            _,
            // HS2-J8PRTV: capacity and admission pass this machine's gate and
            // policy. The hosted `target-capacity` and `target-admission` routes stay
            // ordinary authenticated data so a remote orchestrator can reach them.
            "binding" | "resolve" | "access" | "capacity" | "admission"
        ] | ["checkouts", _, "orchestration", "inspect" | "audit"]
            // HS2-CRBR8X: only this machine's owner reads what it hosted.
            | ["checkouts", _, "orchestration", "hosted-audit"]
            // HS2-W27N1D: adding binds on this machine and removing unbinds here. The
            // project type is a checked-in setting and stays ordinary project data.
            | ["checkouts", _, "orchestration", "projects"]
            | ["checkouts", _, "orchestration", "projects", _]
            // HS2-YCJDEB: a dry run reads this machine's bindings and remembered rules.
            | ["checkouts", _, "orchestration", "policy", "dry-run"]
            // HS2-XHQ1EH: permission requests and remembered rules are this machine's.
            | ["checkouts", _, "orchestration", "permissions"]
            | ["checkouts", _, "orchestration", "permissions", _]
            | ["checkouts", _, "orchestration", "permission-rules"]
            | ["checkouts", _, "orchestration", "permission-rules", _]
            // HS2-JWV5S1: so are the retired files.
            | ["checkouts", _, "orchestration", "retired-permissions"]
            // HS2-AZ15GA: orchestrating uses this machine's gate and bindings. The
            // hosted `target-tickets` routes stay ordinary authenticated project data,
            // because a remote orchestrator must reach them.
            | ["checkouts", _, "orchestration", "tickets", _]
            // HS2-EBHDSB: run records are this machine's.
            | ["checkouts", _, "orchestration", "runs"]
            | ["checkouts", _, "orchestration", "runs", _]
            | ["checkouts", _, "orchestration", "runs", _, _]
            | ["checkouts", _, "orchestration", "run-events"]
            // HS2-5JBYPV: gate records are this machine's.
            | ["checkouts", _, "orchestration", "gates"]
            | ["checkouts", _, "orchestration", "gates", _]
            // HS2-WBREDM
            | ["checkouts", _, "orchestration", "run-reconcile"]
            // HS2-S13719: computed from this machine's records.
            | ["checkouts", _, "orchestration", "summary"]
            // HS2-J89JC7: computed from this machine's records.
            | ["checkouts", _, "orchestration", "timeline"]
            // HS2-S3FKJ7: computed from this machine's bindings and timelines.
            | ["checkouts", _, "orchestration", "target-lines"]
            // HS2-3D8D6V: and so are the lines of a remote checkout bound here.
            | ["remotes", _, "checkouts", _, "orchestration", "target-lines"]
            // HS2-KB2Y51: a push pass uses this machine's bindings and credentials.
            // The hosted `target-line-observations` route stays ordinary
            // authenticated data so the owner's machine can reach it.
            | ["checkouts", _, "orchestration", "target-lines", "push"]
            // HS2-AHG7HW: observed tool capacity is this machine's.
            | ["checkouts", _, "orchestration", "tool-capacity"]
            | [
                "checkouts",
                _,
                "orchestration",
                "tool-capacity",
                "signals" | "select"
            ]
            // HS2-WN0MXZ: step definitions and results are this machine's, and a run
            // passes its gate. The hosted `target-validations` route stays ordinary
            // authenticated data so a remote orchestrator can reach it.
            | ["checkouts", _, "orchestration", "validations", _]
            // HS2-ZCR3Y7: kept step output never leaves the machine that ran the step.
            | ["checkouts", _, "target-validation-output", _]
            // HS2-42TARA: the plan and what it is computed from are this machine's.
            | ["checkouts", _, "orchestration", "readiness", _]
            // HS2-GDJXGZ: session records are this machine's and a start passes its
            // gate. The hosted `target-sessions` route stays ordinary authenticated
            // data so a remote orchestrator can reach it.
            | ["checkouts", _, "orchestration", "sessions"]
            | ["checkouts", _, "orchestration", "sessions", _]
            | ["checkouts", _, "orchestration", "sessions", _, _]
            | ["checkouts", _, "orchestration", "session-sync"]
            // HS2-ECWG50: scheduling reads and writes this machine's records.
            | ["checkouts", _, "orchestration", "scheduler", _]
            // HS2-HCW7MD: the planner runs on this machine, in the project itself.
            | ["checkouts", _, "orchestration", "planning", _]
    )
}

/// The only project-wrapper requests that bypass reopen/relink/startup recovery.
pub fn is_cleanup_request(method: &str, path: &str) -> bool {
    matches!(method, "GET" | "DELETE")
        && matches!(
            parts(path).as_slice(),
            ["checkouts", _, "orchestration", "targets", _, "binding"]
        )
}

fn parts(path: &str) -> Vec<&str> {
    let path = path.split_once('?').map_or(path, |(path, _)| path);
    path.trim_start_matches('/').split('/').collect()
}
/// HS2-SBQ3T8: a single unsigned query value; a malformed or repeated key is invalid.
fn query_number(path: &str, key: &str, default: u64) -> Result<u64, OrchestrationError> {
    let Some((_, query)) = path.split_once('?') else {
        return Ok(default);
    };
    let mut found = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == key {
            if found.is_some() {
                return Err(OrchestrationError::Invalid("repeated query parameter"));
            }
            found = Some(
                value
                    .parse()
                    .map_err(|_| OrchestrationError::Invalid("unsigned query value required"))?,
            );
        }
    }
    Ok(found.unwrap_or(default))
}
/// HS2-AZ15GA: one percent-decoded text query value; a repeated key is invalid.
fn query_text(path: &str, key: &str) -> Result<Option<String>, OrchestrationError> {
    let Some((_, query)) = path.split_once('?') else {
        return Ok(None);
    };
    let mut found = None;
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
        if name == key {
            if found.is_some() {
                return Err(OrchestrationError::Invalid("repeated query parameter"));
            }
            found = Some(super::tickets::decode_query(value)?);
        }
    }
    Ok(found)
}
/// HS2-SBQ3T8: the optional acting role and id every adapter may attach to a body.
fn actor(body: &Value) -> Result<Option<MutationActor>, OrchestrationError> {
    let Some(actor) = body.get("actor").filter(|actor| !actor.is_null()) else {
        return Ok(None);
    };
    let role = actor
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let id = match actor.get("id") {
        None | Some(Value::Null) => None,
        Some(Value::String(id)) => Some(id.clone()),
        Some(_) => return Err(OrchestrationError::Invalid("actor id must be a string")),
    };
    MutationActor::parse(role, id)
        .map(Some)
        .map_err(|_| OrchestrationError::Invalid("actor role must be human, ai, or system"))
}
fn value(result: impl serde::Serialize) -> Result<Value, OrchestrationError> {
    serde_json::to_value(result).map_err(|_| OrchestrationError::Corrupt)
}
fn decode<T: DeserializeOwned>(body: &Value) -> Result<T, OrchestrationError> {
    let mut body = body.clone();
    if let Some(object) = body.as_object_mut() {
        object.remove("actor");
    }
    serde_json::from_value(body).map_err(|_| OrchestrationError::Invalid("invalid target request"))
}
/// HS2-AZ15GA: hosted bodies keep their `actor`; it attributes the target's write.
fn hosted<T: DeserializeOwned>(body: &Value) -> Result<T, OrchestrationError> {
    serde_json::from_value(body.clone())
        .map_err(|_| OrchestrationError::Invalid("invalid target ticket request"))
}
fn reference(owner: &str, id: &str) -> Result<TargetRef, OrchestrationError> {
    let target = TargetRef {
        owner_checkout_id: owner.into(),
        target_id: id.into(),
    };
    target.validate()?;
    Ok(target)
}
fn checked_target(target: &TargetRef, owner: &str, id: &str) -> Result<(), OrchestrationError> {
    if *target != reference(owner, id)? {
        return Err(OrchestrationError::Invalid("target route/body mismatch"));
    }
    Ok(())
}

pub(super) struct Clock;
impl hotsheet_ticketing::Clock for Clock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_millis().try_into().unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}
pub(super) struct Rng;
impl hotsheet_ticketing::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        ulid::Ulid::new().into()
    }
}

/// Must be called BEFORE runtime/identity/provider construction by every adapter.
/// None means this is not the independent machine-binding cleanup subset.
pub fn request_cleanup_now(
    home: &Path,
    method: &str,
    path: &str,
    body: &Value,
) -> Option<Result<Value, OrchestrationError>> {
    let mut rng = Rng;
    cleanup(
        home,
        method,
        path,
        body,
        &mut Transaction {
            clock: &Clock,
            rng: &mut rng,
        },
    )
}
fn cleanup(
    home: &Path,
    method: &str,
    path: &str,
    body: &Value,
    tx: &mut Transaction<'_>,
) -> Option<Result<Value, OrchestrationError>> {
    match (method, parts(path).as_slice()) {
        (
            "GET",
            [
                "checkouts",
                owner,
                "orchestration",
                "targets",
                id,
                "binding",
            ],
        ) => Some((|| {
            value(OrchestrationRuntime::binding_at(
                home,
                &reference(owner, id)?,
            )?)
        })()),
        (
            "DELETE",
            [
                "checkouts",
                owner,
                "orchestration",
                "targets",
                id,
                "binding",
            ],
        ) => Some((|| {
            let request: UnbindTarget = decode(body)?;
            checked_target(&request.target, owner, id)?;
            value(OrchestrationRuntime::unbind_at(home, request, tx)?)
        })()),
        _ => None,
    }
}

impl OrchestrationRuntime {
    /// HS2-7QHWHJ: one watch step on the real clock, for a host's background follower.
    pub fn watch_step_now(
        &self,
        owner_id: &str,
        target_id: &str,
        state: &mut super::WatchState,
        wait_ms: u64,
    ) -> super::WatchStep {
        let mut rng = Rng;
        self.watch_step(
            owner_id,
            target_id,
            state,
            wait_ms,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }

    /// HS2-2DPGDY: try a pending role prompt again on the real clock, for the host's
    /// readiness events.
    pub fn hosted_role_retry_now(
        &self,
        terminal_id: &str,
    ) -> Result<Option<super::RolePrompt>, OrchestrationError> {
        let mut rng = Rng;
        self.hosted_role_retry(
            terminal_id,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }

    /// HS2-ECWG50: one scheduling step on the real clock, for a host's follower.
    pub fn scheduler_follow_step_now(
        &self,
        owner_id: &str,
        state: &mut super::SchedulerFollowState,
        wait_ms: u64,
    ) -> super::SchedulerStep {
        let mut rng = Rng;
        self.scheduler_follow_step(
            owner_id,
            state,
            wait_ms,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }

    /// HS2-5JBYPV: one gate follow step on the real clock, for a host's follower.
    pub fn gate_follow_step_now(
        &self,
        owner_id: &str,
        state: &mut super::GateFollowState,
        wait_ms: u64,
    ) -> super::GateFollowStep {
        let mut rng = Rng;
        self.gate_follow_step(
            owner_id,
            state,
            wait_ms,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }

    pub fn request_now(
        &self,
        method: &str,
        path: &str,
        body: &Value,
    ) -> Result<Value, OrchestrationError> {
        let mut rng = Rng;
        self.request(
            method,
            path,
            body,
            &mut Transaction {
                clock: &Clock,
                rng: &mut rng,
            },
        )
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        body: &Value,
        tx: &mut Transaction<'_>,
    ) -> Result<Value, OrchestrationError> {
        if let Some(result) = cleanup(&self.home, method, path, body, tx) {
            return result;
        }
        match (method, parts(path).as_slice()) {
            ("POST", ["checkouts", owner, "orchestration", "inspect"]) => {
                value(self.inspect(owner, decode(body)?)?)
            }
            ("GET", ["remote-checkouts", id, "resolution"]) => value(self.checkout_projection(id)?),
            ("GET", ["checkouts", owner, "orchestration", "targets"]) => {
                value(self.declarations(owner)?)
            }
            ("PUT", ["checkouts", owner, "orchestration", "targets"]) => {
                value(self.replace(owner, decode(body)?, tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "targets"]) => {
                value(self.add(owner, decode(body)?, tx)?)
            }
            ("PATCH", ["checkouts", owner, "orchestration", "targets", id]) => {
                value(self.rename(&reference(owner, id)?, decode(body)?, tx)?)
            }
            ("DELETE", ["checkouts", owner, "orchestration", "targets", id]) => {
                value(self.remove(&reference(owner, id)?, decode(body)?, tx)?)
            }
            (
                "PUT",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "targets",
                    id,
                    "binding",
                ],
            ) => {
                let request: BindTarget = decode(body)?;
                checked_target(&request.target, owner, id)?;
                value(self.bind(request, tx)?)
            }
            (
                "GET",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "targets",
                    id,
                    "resolve",
                ],
            ) => value(self.resolve(&reference(owner, id)?, tx)?),
            ("POST", ["checkouts", owner, "orchestration", "targets", id, "access"]) => {
                let person_acts = access::is_person(actor(body)?.as_ref());
                let request: AccessRequest = decode(body)?;
                value(self.access(&reference(owner, id)?, &request, person_acts, tx)?)
            }
            // HS2-AZ15GA hosted half: provider-neutral ticket operations on one source
            // of a checkout this machine hosts.
            ("GET", ["checkouts", checkout, "target-tickets"]) => {
                let connection = query_text(path, "connection_id")?
                    .ok_or(OrchestrationError::Invalid("connection_id is required"))?;
                match (
                    query_text(path, "native_id")?,
                    query_text(path, "operation_id")?,
                ) {
                    (Some(native_id), None) => {
                        value(self.hosted_ticket(checkout, &connection, &native_id)?)
                    }
                    (None, Some(operation_id)) => value(self.hosted_ticket_by_operation(
                        checkout,
                        &connection,
                        &operation_id,
                    )?),
                    _ => Err(OrchestrationError::Invalid(
                        "exactly one of native_id or operation_id is required",
                    )),
                }
            }
            // HS2-B6236S hosted half: what one ticket of a hosted checkout cost.
            ("GET", ["checkouts", checkout, "target-tickets", "cost"]) => {
                let connection = query_text(path, "connection_id")?
                    .ok_or(OrchestrationError::Invalid("connection_id is required"))?;
                let native_id = query_text(path, "native_id")?
                    .ok_or(OrchestrationError::Invalid("native_id is required"))?;
                value(self.hosted_ticket_cost(checkout, &connection, &native_id)?)
            }
            // HS2-D85G19 hosted half: one bounded page of a source's open tickets.
            ("GET", ["checkouts", checkout, "target-tickets", "list"]) => {
                let connection = query_text(path, "connection_id")?
                    .ok_or(OrchestrationError::Invalid("connection_id is required"))?;
                let limit = match query_text(path, "limit")? {
                    None => None,
                    Some(_) => Some(
                        u32::try_from(query_number(path, "limit", 0)?)
                            .map_err(|_| OrchestrationError::Invalid("page limit too large"))?,
                    ),
                };
                let search = TargetTicketSearch {
                    text: query_text(path, "text")?,
                    category: query_text(path, "category")?,
                    tag: query_text(path, "tag")?,
                    cursor: query_text(path, "cursor")?,
                    limit,
                };
                value(self.hosted_ticket_list(checkout, &connection, &search)?)
            }
            ("POST", ["checkouts", checkout, "target-tickets"]) => {
                value(self.hosted_ticket_create(checkout, hosted(body)?, tx)?)
            }
            ("PATCH", ["checkouts", checkout, "target-tickets"]) => {
                value(self.hosted_ticket_update(checkout, hosted(body)?, tx)?)
            }
            // HS2-G1PKK2 hosted half: wait on this machine's claim backend.
            ("GET", ["checkouts", checkout, "target-resource-wait"]) => {
                let namespace = query_text(path, "namespace")?
                    .ok_or(OrchestrationError::Invalid("namespace is required"))?;
                let resource = query_text(path, "resource")?
                    .ok_or(OrchestrationError::Invalid("resource is required"))?;
                value(self.hosted_resource_wait(
                    checkout,
                    &namespace,
                    &resource,
                    query_number(path, "wait_ms", 0)?,
                )?)
            }
            // HS2-7QHWHJ hosted half: ticket changes after a cursor, with a bounded wait.
            // HS2-FJ6MDW hosted half: the project's own speculation setting alone.
            ("GET", ["checkouts", checkout, "target-speculation"]) => {
                value(self.hosted_speculation(checkout)?)
            }
            ("GET", ["checkouts", checkout, "target-events"]) => {
                let since = match query_text(path, "since")? {
                    None => None,
                    Some(_) => Some(query_number(path, "since", 0)?),
                };
                value(self.hosted_events(checkout, since, query_number(path, "wait_ms", 0)?)?)
            }
            // HS2-EBHDSB hosted half: the exact ticket's own claim and lease.
            ("POST", ["checkouts", checkout, "target-claims"]) => {
                value(self.hosted_claim(checkout, hosted(body)?, tx)?)
            }
            ("PATCH", ["checkouts", checkout, "target-claims"]) => {
                value(self.hosted_claim_renew(checkout, hosted(body)?, tx)?)
            }
            ("DELETE", ["checkouts", checkout, "target-claims"]) => {
                value(self.hosted_claim_release(checkout, hosted(body)?, tx)?)
            }
            // HS2-J8PRTV hosted half: this machine's capacity and its own admission answer.
            ("GET", ["checkouts", checkout, "target-capacity"]) => {
                // HS2-P5GTSW: with `after` and `wait_ms` the read is a long poll.
                value(self.hosted_capacity_wait(
                    checkout,
                    &super::capacity::claim_namespaces(
                        query_text(path, "claim_namespaces")?.as_deref(),
                    )?,
                    query_text(path, "after")?.as_deref(),
                    query_number(path, "wait_ms", 0)?,
                )?)
            }
            ("POST", ["checkouts", checkout, "target-admission"]) => {
                value(self.hosted_admission(checkout, decode(body)?)?)
            }
            // HS2-WN0MXZ hosted half: run a validation step where the target lives.
            ("POST", ["checkouts", checkout, "target-validations"]) => {
                value(self.hosted_validation_start(checkout, decode(body)?, tx)?)
            }
            // HS2-G3VAK4 hosted half: run a deployment here, and read it.
            ("POST", ["checkouts", checkout, "target-deployments"]) => {
                value(self.hosted_deployment_start(checkout, decode(body)?, tx)?)
            }
            ("GET", ["checkouts", checkout, "target-deployments"]) => {
                let execution = query_text(path, "execution_id")?
                    .ok_or(OrchestrationError::Invalid("execution_id is required"))?;
                value(self.hosted_validation(
                    checkout,
                    &execution,
                    query_number(path, "wait_ms", 0)?,
                )?)
            }
            // HS2-WDQZ37 hosted half: watch a CI gate here, and long-poll its reading.
            ("POST", ["checkouts", checkout, "target-gates"]) => {
                value(self.hosted_gate_watch(checkout, decode(body)?, tx)?)
            }
            ("GET", ["checkouts", checkout, "target-gates"]) => {
                let text = |key| {
                    query_text(path, key)?.ok_or(OrchestrationError::Invalid(
                        "commit, repository, workflow and branch are required",
                    ))
                };
                let watch = HostedGateWatch {
                    commit: text("commit")?,
                    repository: text("repository")?,
                    workflow: text("workflow")?,
                    branch: text("branch")?,
                    artifact: query_text(path, "artifact")?,
                    provider: query_text(path, "provider")?,
                    again: false,
                };
                let after = match query_text(path, "after")? {
                    Some(_) => Some(query_number(path, "after", 0)?),
                    None => None,
                };
                value(self.hosted_gate(
                    checkout,
                    &watch,
                    after,
                    query_number(path, "wait_ms", 0)?,
                )?)
            }
            // HS2-YYNW9F: the profiles this checkout offers, checked.
            ("GET", ["checkouts", checkout, "target-validation-output", digest]) => {
                value(self.validation_output(checkout, digest)?)
            }
            ("GET", ["checkouts", checkout, "target-validation-profiles"]) => {
                value(self.hosted_validation_profiles(checkout)?)
            }
            // HS2-17ABM2: stop it; only lowers activity.
            ("DELETE", ["checkouts", checkout, "target-validations"]) => {
                value(self.hosted_validation_cancel(checkout, decode(body)?, tx)?)
            }
            ("GET", ["checkouts", checkout, "target-validations"]) => {
                let execution = query_text(path, "execution_id")?
                    .ok_or(OrchestrationError::Invalid("execution_id is required"))?;
                value(self.hosted_validation(
                    checkout,
                    &execution,
                    query_number(path, "wait_ms", 0)?,
                )?)
            }
            // HS2-GDJXGZ hosted half: sessions an orchestrator started on this machine.
            ("GET", ["checkouts", checkout, "target-sessions"]) => value(self.hosted_sessions(
                checkout,
                query_text(path, "operation_id")?.as_deref(),
                query_text(path, "safe_point")?.as_deref() == Some("true"),
                tx,
            )?),
            ("POST", ["checkouts", checkout, "target-sessions"]) => {
                value(self.hosted_session_start(checkout, hosted(body)?, tx)?)
            }
            ("DELETE", ["checkouts", checkout, "target-sessions"]) => {
                value(self.hosted_session_stop(checkout, hosted(body)?, tx)?)
            }
            ("PATCH", ["checkouts", checkout, "target-sessions"]) => {
                value(self.hosted_session_control(checkout, hosted(body)?, tx)?)
            }
            // HS2-R7QH35: tell a running hosted session its exact ticket.
            ("PUT", ["checkouts", checkout, "target-sessions"]) => {
                value(self.hosted_session_request(checkout, hosted(body)?, tx)?)
            }
            // HS2-GDJXGZ orchestrating half.
            ("GET", ["checkouts", owner, "orchestration", "sessions"]) => {
                value(self.session_list(
                    owner,
                    query_text(path, "active")?.as_deref() == Some("true"),
                )?)
            }
            ("POST", ["checkouts", owner, "orchestration", "sessions"]) => {
                let actor = actor(body)?;
                value(self.session_start(owner, decode(body)?, actor.as_ref(), tx)?)
            }
            ("GET", ["checkouts", owner, "orchestration", "sessions", session]) => {
                value(self.session(owner, session)?)
            }
            (
                "POST",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "sessions",
                    session,
                    operation,
                ],
            ) => {
                let actor = actor(body)?;
                match *operation {
                    "sync" => value(self.session_sync(owner, session, actor.as_ref(), tx)?),
                    // HS2-R7QH35
                    "request" => {
                        value(self.session_tell_ticket(owner, session, actor.as_ref(), tx)?)
                    }
                    "safe-point" => {
                        value(self.session_safe_point(owner, session, actor.as_ref(), tx)?)
                    }
                    "stop" => value(self.session_stop(
                        owner,
                        session,
                        decode(body)?,
                        actor.as_ref(),
                        tx,
                    )?),
                    "takeover" => value(self.session_takeover(
                        owner,
                        session,
                        decode(body)?,
                        actor.as_ref(),
                        tx,
                    )?),
                    // HS2-KR28FS
                    "hand-back" => value(self.session_hand_back(
                        owner,
                        session,
                        decode(body)?,
                        actor.as_ref(),
                        tx,
                    )?),
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            ("POST", ["checkouts", owner, "orchestration", "session-sync"]) => {
                value(self.sessions_sync(owner, actor(body)?.as_ref(), tx)?)
            }
            // HS2-WN0MXZ orchestrating half.
            (
                "POST",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "validations",
                    operation,
                ],
            ) => {
                let actor = actor(body)?;
                match *operation {
                    "define" => {
                        value(self.validation_define(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "list" => {
                        let list: super::ListValidations = decode(body)?;
                        value(self.validation_list(owner, list.ticket.as_ref(), tx)?)
                    }
                    "evidence" => {
                        let step: super::ValidationStepRef = decode(body)?;
                        value(self.validation_evidence(owner, &step.ticket, &step.step_id, tx)?)
                    }
                    "run" => {
                        value(self.validation_run(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "remove" => {
                        value(self.validation_remove(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    // HS2-YYNW9F
                    "profiles" => {
                        value(self.validation_profiles(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    // HS2-17ABM2
                    "cancel" => {
                        value(self.validation_cancel(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            // HS2-42TARA: the plan of prerequisites. The reads contact no target.
            ("POST", ["checkouts", owner, "orchestration", "readiness", operation]) => {
                let actor = actor(body)?;
                match *operation {
                    "define" => {
                        value(self.readiness_define(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "show" => {
                        let step: super::PlanStepRef = decode(body)?;
                        value(self.readiness(owner, &step.step_id, tx)?)
                    }
                    "plan" => value(self.readiness_plan(owner, tx)?),
                    // HS2-F1BKQ0: the plan's state where every read would fail.
                    "health" => value(self.readiness_health(owner)?),
                    "ready" => value(self.ready_work(owner, tx)?),
                    "refresh" => {
                        value(self.readiness_refresh(owner, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    "remove" => {
                        value(self.readiness_remove(owner, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    // HS2-YDY5HJ: move a machine-local plan into the checked-in one.
                    "migrate" => {
                        value(self.readiness_migrate(owner, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    // HS2-JSF985: which work is provisional on which gate, and the
                    // reopening of work a failed prerequisite's fix invalidated.
                    "speculation" => value(self.provisional_status(owner)?),
                    "reopen" => {
                        value(self.readiness_reopen(owner, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            // HS2-HCW7MD: Propose Plan starts the project's planner; `show` reads
            // where planning stands.
            ("POST", ["checkouts", owner, "orchestration", "planning", operation]) => {
                let actor = actor(body)?;
                match *operation {
                    "propose" => {
                        value(self.plan_propose(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "show" => {
                        decode::<super::PlanRequest>(body)?;
                        value(self.plan_status(owner, tx)?)
                    }
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            // HS2-ECWG50: the scheduler. `plan` and `explain` change nothing.
            ("POST", ["checkouts", owner, "orchestration", "scheduler", operation]) => {
                let actor = actor(body)?;
                match *operation {
                    "plan" => {
                        decode::<super::PlanRequest>(body)?;
                        value(self.scheduler_plan(owner, actor.as_ref(), tx)?)
                    }
                    "explain" => {
                        value(self.scheduler_explain(owner, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    "decisions" => value(self.scheduler_decisions(owner, &decode(body)?)?),
                    "tick" => {
                        value(self.scheduler_tick(owner, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            // HS2-J8PRTV orchestrating half.
            (
                "GET",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "targets",
                    id,
                    "capacity",
                ],
            ) => {
                // A read carries no body, so its actor travels in the query.
                let actor = match query_text(path, "actor_id")? {
                    Some(id) => Some(
                        MutationActor::parse(
                            query_text(path, "actor_role")?.as_deref().unwrap_or("ai"),
                            Some(id),
                        )
                        .map_err(|_| {
                            OrchestrationError::Invalid("actor role must be human, ai, or system")
                        })?,
                    ),
                    None => None,
                };
                value(self.target_capacity_wait(
                    &reference(owner, id)?,
                    &super::capacity::claim_namespaces(
                        query_text(path, "claim_namespaces")?.as_deref(),
                    )?,
                    query_text(path, "after")?.as_deref(),
                    query_number(path, "wait_ms", 0)?,
                    actor.as_ref(),
                    tx,
                )?)
            }
            // The same read for adapters that attach the actor to a body (CLI, MCP).
            (
                "POST",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "targets",
                    id,
                    "capacity",
                ],
            ) => {
                let actor = actor(body)?;
                let read: super::capacity::ReadCapacity = decode(body)?;
                value(self.target_capacity_wait(
                    &reference(owner, id)?,
                    &super::capacity::claim_namespaces(Some(&read.claim_namespaces.join(",")))?,
                    read.after.as_deref(),
                    read.wait_ms.unwrap_or(0),
                    actor.as_ref(),
                    tx,
                )?)
            }
            (
                "POST",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "targets",
                    id,
                    "admission",
                ],
            ) => {
                let actor = actor(body)?;
                value(self.target_admission(
                    &reference(owner, id)?,
                    decode(body)?,
                    actor.as_ref(),
                    tx,
                )?)
            }
            // HS2-EBHDSB orchestrating half.
            ("GET", ["checkouts", owner, "orchestration", "runs"]) => value(self.run_list(
                owner,
                query_text(path, "active")?.as_deref() == Some("true"),
            )?),
            ("POST", ["checkouts", owner, "orchestration", "runs"]) => {
                let actor = actor(body)?;
                value(self.run_dispatch(owner, decode(body)?, actor.as_ref(), tx)?)
            }
            ("GET", ["checkouts", owner, "orchestration", "runs", run]) => {
                value(self.run(owner, run)?)
            }
            ("GET", ["checkouts", owner, "orchestration", "runs", run, "status"]) => {
                value(self.run_status(owner, run)?)
            }
            // HS2-7QHWHJ: cursor replay over the owner's run events, with an optional
            // bounded wait instead of client polling.
            ("GET", ["checkouts", owner, "orchestration", "run-events"]) => {
                let run = query_text(path, "run")?;
                value(self.run_events(
                    owner,
                    query_number(path, "after", 0)?,
                    usize::try_from(query_number(path, "limit", 100)?).unwrap_or(usize::MAX),
                    run.as_deref(),
                    query_number(path, "wait_ms", 0)?,
                )?)
            }
            ("POST", ["checkouts", owner, "orchestration", "runs", run, operation]) => {
                let actor = actor(body)?;
                match *operation {
                    "renew" => {
                        let minutes = match body.get("lease_minutes") {
                            None | Some(Value::Null) => None,
                            Some(value) => Some(
                                value
                                    .as_u64()
                                    .and_then(|minutes| u32::try_from(minutes).ok())
                                    .ok_or(OrchestrationError::Invalid(
                                        "lease_minutes must be an unsigned integer",
                                    ))?,
                            ),
                        };
                        value(self.run_renew(owner, run, minutes, actor.as_ref(), tx)?)
                    }
                    "cancel" => value(self.run_cancel(owner, run, actor.as_ref(), tx)?),
                    "sync" => value(self.run_sync(owner, run, actor.as_ref(), tx)?),
                    // HS2-5JBYPV
                    "gates" => {
                        value(self.gate_link(owner, run, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "gate-sync" => {
                        value(self.gate_sync(owner, run, &decode(body)?, actor.as_ref(), tx)?)
                    }
                    "deployments" => {
                        value(self.gate_deploy(owner, run, decode(body)?, actor.as_ref(), tx)?)
                    }
                    // HS2-WBREDM: recovery and the control actions.
                    "reconcile" => value(self.run_reconcile(owner, run, actor.as_ref(), tx)?),
                    "pause" => {
                        value(self.run_pause(owner, run, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "resume" => value(self.run_resume(owner, run, actor.as_ref(), tx)?),
                    "handoff" => {
                        value(self.run_handoff(owner, run, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "correct" => {
                        value(self.run_correct(owner, run, decode(body)?, actor.as_ref(), tx)?)
                    }
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            // HS2-5JBYPV: gate reads answer from this machine's records alone.
            ("GET", ["checkouts", owner, "orchestration", "gates"]) => {
                let commit = query_text(path, "commit")?.map(|sha| sha.to_ascii_lowercase());
                value(self.gate_summary(
                    owner,
                    &hotsheet_ticketing::orchestration::GateFilter {
                        run_id: query_text(path, "run")?,
                        target_id: query_text(path, "target_id")?,
                        commit,
                        current_only: false,
                    },
                )?)
            }
            ("GET", ["checkouts", owner, "orchestration", "gates", gate]) => {
                value(self.gate(owner, gate)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "run-reconcile"]) => {
                value(self.runs_reconcile(owner, actor(body)?.as_ref(), tx)?)
            }
            // HS2-J89JC7: answers from this machine's records alone; `after` (a version)
            // with `wait_ms` waits for the next change.
            ("GET", ["checkouts", owner, "orchestration", "timeline"]) => {
                let after = query_text(path, "after")?;
                if let Some(after) = &after {
                    identifier(after, "bounded timeline version required")?;
                }
                value(self.orchestration_timeline(
                    owner,
                    after.as_deref(),
                    query_number(path, "wait_ms", 0)?,
                    tx,
                )?)
            }
            // HS2-S3FKJ7: from the target side, the same records as the timeline.
            ("GET", ["checkouts", checkout, "orchestration", "target-lines"]) => {
                let after = query_text(path, "after")?;
                if let Some(after) = &after {
                    identifier(after, "bounded target lines version required")?;
                }
                value(self.orchestration_target_lines(
                    checkout,
                    after.as_deref(),
                    query_number(path, "wait_ms", 0)?,
                    tx,
                )?)
            }
            // HS2-KB2Y51: one push pass of this machine's lines to remote targets.
            ("POST", ["checkouts", owner, "orchestration", "target-lines", "push"]) => value(
                self.push_target_lines(&mut super::LinePushState::default(), Some(owner), tx)?,
            ),
            // HS2-KB2Y51: the hosted half; another machine's display-only lines.
            ("PUT", ["checkouts", checkout, "target-line-observations"]) => {
                value(self.target_line_observations_put(checkout, decode(body)?, tx)?)
            }
            // HS2-3D8D6V: a remote checkout's lines, from this machine's records alone.
            (
                "GET",
                [
                    "remotes",
                    registration,
                    "checkouts",
                    checkout,
                    "orchestration",
                    "target-lines",
                ],
            ) => {
                let after = query_text(path, "after")?;
                if let Some(after) = &after {
                    identifier(after, "bounded target lines version required")?;
                }
                value(self.orchestration_remote_target_lines(
                    registration,
                    checkout,
                    after.as_deref(),
                    query_number(path, "wait_ms", 0)?,
                    tx,
                )?)
            }
            // HS2-S13719: answers from this machine's records alone.
            ("GET", ["checkouts", owner, "orchestration", "summary"]) => {
                let ticket = query_text(path, "ticket")?.map(|native_id| {
                    Ok::<_, OrchestrationError>(hotsheet_ticketing::orchestration::RunTicket {
                        connection_id: query_text(path, "connection_id")?.unwrap_or_default(),
                        native_id,
                    })
                });
                value(self.progress_summary(owner, ticket.transpose()?.as_ref(), tx)?)
            }
            // HS2-AZ15GA orchestrating half.
            ("POST", ["checkouts", owner, "orchestration", "tickets", operation]) => {
                let actor = actor(body)?;
                match *operation {
                    "create" => value(self.target_ticket_create(
                        owner,
                        decode(body)?,
                        actor.as_ref(),
                        tx,
                    )?),
                    "read" => {
                        value(self.target_ticket_read(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "update" => value(self.target_ticket_update(
                        owner,
                        decode(body)?,
                        actor.as_ref(),
                        tx,
                    )?),
                    "link" => {
                        value(self.target_ticket_link(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    "unlink" => value(self.target_ticket_unlink(owner, decode(body)?, tx)?),
                    "links" => {
                        value(self.target_ticket_links(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    // HS2-D85G19
                    "list" => {
                        value(self.target_ticket_list(owner, decode(body)?, actor.as_ref(), tx)?)
                    }
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
            }
            // HS2-AHG7HW
            ("GET", ["checkouts", owner, "orchestration", "tool-capacity"]) => {
                value(self.tool_capacity(owner, tx)?)
            }
            (
                "POST",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "tool-capacity",
                    "signals",
                ],
            ) => {
                let actor = actor(body)?;
                value(self.tool_capacity_signal(owner, decode(body)?, actor.as_ref(), tx)?)
            }
            (
                "POST",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "tool-capacity",
                    "select",
                ],
            ) => value(self.tool_select(owner, decode(body)?, tx)?),
            ("GET", ["checkouts", owner, "orchestration", "policy"]) => value(self.policy(owner)?),
            // HS2-W27N1D
            ("GET", ["checkouts", owner, "orchestration", "project-type"]) => {
                value(self.project_type(owner)?)
            }
            ("PUT", ["checkouts", owner, "orchestration", "project-type"]) => {
                let actor = actor(body)?;
                value(self.set_project_type(owner, decode(body)?, actor.as_ref(), tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "projects"]) => {
                let actor = actor(body)?;
                value(self.add_project(owner, decode(body)?, actor.as_ref(), tx)?)
            }
            // HS2-92FJXM: not machine-local, so a read-write device on another machine
            // may add one of this server's open checkouts; nothing else is reachable.
            ("GET", ["checkouts", owner, "orchestration", "hosted-projects"]) => {
                value(self.hosted_project_choices(owner)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "hosted-projects"]) => {
                let actor = actor(body)?;
                value(self.add_hosted_project(owner, decode(body)?, actor.as_ref(), tx)?)
            }
            ("DELETE", ["checkouts", owner, "orchestration", "projects", id]) => {
                let actor = actor(body)?;
                let request: super::RemoveProject = if body.is_null() {
                    super::RemoveProject::default()
                } else {
                    decode(body)?
                };
                value(self.remove_project(&reference(owner, id)?, request, actor.as_ref(), tx)?)
            }
            ("PUT", ["checkouts", owner, "orchestration", "policy"]) => {
                let actor = actor(body)?;
                let request: SetPolicy = decode(body)?;
                value(self.set_policy(owner, request, actor.as_ref(), tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "policy", "validate"]) => {
                value(self.validate_policy(owner, decode(body)?)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "policy", "dry-run"]) => {
                value(self.dry_run_policy(owner, decode(body)?)?)
            }
            // HS2-XHQ1EH
            ("GET", ["checkouts", owner, "orchestration", "permissions"]) => {
                value(self.permission_list(owner, tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "permissions", id]) => {
                let actor = actor(body)?;
                let request: super::AnswerPermission = decode(body)?;
                value(self.permission_answer(owner, id, request.answer, actor.as_ref(), tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "permission-rules"]) => {
                let actor = actor(body)?;
                let request: super::RememberPermission = decode(body)?;
                value(self.permission_remember(owner, request, actor.as_ref(), tx)?)
            }
            ("GET", ["checkouts", owner, "orchestration", "retired-permissions"]) => {
                value(self.retired_permissions(owner)?)
            }
            ("DELETE", ["checkouts", owner, "orchestration", "retired-permissions"]) => {
                value(self.retired_permissions_remove(owner, actor(body)?.as_ref(), tx)?)
            }
            ("DELETE", ["checkouts", owner, "orchestration", "permission-rules", id]) => {
                value(self.permission_forget(owner, id, actor(body)?.as_ref(), tx)?)
            }
            ("GET", ["checkouts", checkout, "orchestration", "hosted-audit"]) => {
                value(self.hosted_audit(
                    checkout,
                    query_number(path, "after", 0)?,
                    usize::try_from(query_number(path, "limit", 100)?).unwrap_or(usize::MAX),
                )?)
            }
            ("GET", ["checkouts", owner, "orchestration", "audit"]) => value(self.audit(
                owner,
                query_number(path, "after", 0)?,
                usize::try_from(query_number(path, "limit", 100)?).unwrap_or(usize::MAX),
            )?),
            _ => Err(OrchestrationError::Invalid("unsupported target route")),
        }
    }
}
