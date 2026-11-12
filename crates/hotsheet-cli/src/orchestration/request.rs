//! Shared transport adapters select a route only; all transitions live in core.
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{AccessRequest, CreateGrant};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;

pub fn is_path(path: &str) -> bool {
    let parts = parts(path);
    matches!(
        parts.as_slice(),
        ["checkouts", _, "orchestration", "targets", ..]
            | ["checkouts", _, "orchestration", "inspect" | "audit"]
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
            "binding" | "resolve" | "grants" | "access"
        ] | ["checkouts", _, "orchestration", "targets", _, "grants", _]
            | ["checkouts", _, "orchestration", "inspect" | "audit"]
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
            ("GET", ["checkouts", owner, "orchestration", "audit"]) => value(self.audit(
                owner,
                query_number(path, "after", 0)?,
                usize::try_from(query_number(path, "limit", 100)?).unwrap_or(usize::MAX),
            )?),
            _ => Err(OrchestrationError::Invalid("unsupported target route")),
        }
    }
}
