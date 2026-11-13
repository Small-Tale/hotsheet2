//! Shared transport adapters select a route only; all transitions live in core.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{AccessRequest, CreateGrant, SetPolicy};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;

pub fn is_path(path: &str) -> bool {
    let parts = parts(path);
    matches!(
        parts.as_slice(),
        ["checkouts", _, "orchestration", "targets", ..]
            | ["checkouts", _, "orchestration", "inspect" | "audit"]
            | ["checkouts", _, "orchestration", "policy"]
            | [
                "checkouts",
                _,
                "orchestration",
                "policy",
                "validate" | "dry-run"
            ]
            | ["checkouts", _, "orchestration", "approvals"]
            | ["checkouts", _, "orchestration", "approvals", _]
            // HS2-AZ15GA: orchestrating-side ticket operations and the hosted half.
            | ["checkouts", _, "orchestration", "tickets", _]
            | ["checkouts", _, "target-tickets"]
            // HS2-EBHDSB: runs (orchestrating) and the hosted exact claim.
            | ["checkouts", _, "orchestration", "runs"]
            | ["checkouts", _, "orchestration", "runs", _]
            | ["checkouts", _, "orchestration", "runs", _, _]
            | ["checkouts", _, "orchestration", "run-events"]
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
            // HS2-J8PRTV: capacity and admission pass this machine's grants and
            // policy. The hosted `target-capacity` and `target-admission` routes stay
            // ordinary authenticated data so a remote orchestrator can reach them.
            "binding" | "resolve" | "grants" | "access" | "capacity" | "admission"
        ] | [
            "checkouts",
            _,
            "orchestration",
            "targets",
            _,
            "grants",
            _
        ] | ["checkouts", _, "orchestration", "inspect" | "audit"]
            // HS2-YCJDEB: a dry run reads this machine's grants; approvals live here.
            | ["checkouts", _, "orchestration", "policy", "dry-run"]
            | ["checkouts", _, "orchestration", "approvals"]
            | ["checkouts", _, "orchestration", "approvals", _]
            // HS2-AZ15GA: orchestrating uses this machine's grants and bindings. The
            // hosted `target-tickets` routes stay ordinary authenticated project data,
            // because a remote orchestrator must reach them.
            | ["checkouts", _, "orchestration", "tickets", _]
            // HS2-EBHDSB: run records are this machine's.
            | ["checkouts", _, "orchestration", "runs"]
            | ["checkouts", _, "orchestration", "runs", _]
            | ["checkouts", _, "orchestration", "runs", _, _]
            | ["checkouts", _, "orchestration", "run-events"]
            // HS2-AHG7HW: observed tool capacity is this machine's.
            | ["checkouts", _, "orchestration", "tool-capacity"]
            | [
                "checkouts",
                _,
                "orchestration",
                "tool-capacity",
                "signals" | "select"
            ]
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

struct Clock;
impl hotsheet_ticketing::Clock for Clock {
    fn now_ms(&self) -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|time| time.as_millis().try_into().unwrap_or(u64::MAX))
            .unwrap_or(0)
    }
}
struct Rng;
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
            ("GET", ["checkouts", owner, "orchestration", "targets", id, "grants"]) => {
                value(self.grants(&reference(owner, id)?, tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "targets", id, "grants"]) => {
                let actor = actor(body)?;
                let request: CreateGrant = decode(body)?;
                value(self.grant(&reference(owner, id)?, request, actor.as_ref(), tx)?)
            }
            (
                "DELETE",
                [
                    "checkouts",
                    owner,
                    "orchestration",
                    "targets",
                    id,
                    "grants",
                    grant,
                ],
            ) => value(self.revoke(&reference(owner, id)?, grant, actor(body)?.as_ref(), tx)?),
            ("POST", ["checkouts", owner, "orchestration", "targets", id, "access"]) => {
                let request: AccessRequest = decode(body)?;
                value(self.access(&reference(owner, id)?, &request, tx)?)
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
            ("POST", ["checkouts", checkout, "target-tickets"]) => {
                value(self.hosted_ticket_create(checkout, hosted(body)?, tx)?)
            }
            ("PATCH", ["checkouts", checkout, "target-tickets"]) => {
                value(self.hosted_ticket_update(checkout, hosted(body)?, tx)?)
            }
            // HS2-7QHWHJ hosted half: ticket changes after a cursor, with a bounded wait.
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
            ("GET", ["checkouts", checkout, "target-capacity"]) => value(self.hosted_capacity(
                checkout,
                &super::capacity::claim_namespaces(
                    query_text(path, "claim_namespaces")?.as_deref(),
                )?,
            )?),
            ("POST", ["checkouts", checkout, "target-admission"]) => {
                value(self.hosted_admission(checkout, decode(body)?)?)
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
                value(self.target_capacity(
                    &reference(owner, id)?,
                    &super::capacity::claim_namespaces(
                        query_text(path, "claim_namespaces")?.as_deref(),
                    )?,
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
                value(self.target_capacity(
                    &reference(owner, id)?,
                    &super::capacity::claim_namespaces(Some(&read.claim_namespaces.join(",")))?,
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
                    _ => Err(OrchestrationError::Invalid("unsupported target route")),
                }
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
            ("PUT", ["checkouts", owner, "orchestration", "policy"]) => {
                let actor = actor(body)?;
                let request: SetPolicy = decode(body)?;
                value(self.set_policy(owner, request, actor.as_ref(), tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "policy", "validate"]) => {
                value(self.validate_policy(owner, decode(body)?)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "policy", "dry-run"]) => {
                value(self.dry_run_policy(owner, decode(body)?, tx)?)
            }
            ("GET", ["checkouts", owner, "orchestration", "approvals"]) => {
                value(self.approvals(owner, tx)?)
            }
            ("POST", ["checkouts", owner, "orchestration", "approvals", id]) => {
                let actor = actor(body)?;
                let approve = match body.get("approve") {
                    Some(Value::Bool(approve)) => *approve,
                    _ => return Err(OrchestrationError::Invalid("approve must be a boolean")),
                };
                value(self.decide_approval(owner, id, approve, actor.as_ref(), tx)?)
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
