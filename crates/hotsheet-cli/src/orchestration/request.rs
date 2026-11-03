//! Shared transport adapters select a route only; all transitions live in core.
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::*;

pub fn is_path(path: &str) -> bool {
    let parts = parts(path);
    matches!(
        parts.as_slice(),
        ["checkouts", _, "orchestration", "targets", ..]
            | ["checkouts", _, "orchestration", "inspect"]
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
            "binding" | "resolve"
        ] | ["checkouts", _, "orchestration", "inspect"]
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
    path.trim_start_matches('/').split('/').collect()
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
            _ => Err(OrchestrationError::Invalid("unsupported target route")),
        }
    }
}
