//! HS2-FMHH9Y: `hotsheet_resource_claim`, the MCP surface of the shared resource-claim
//! request workflow (`docs/36-resource-claims.md` §36.10).
use std::time::{Duration, Instant};

use crate::{Backend, BackendError};
use hotsheet_resource_claims::{
    ClaimError, Namespace, ROUTE_PREFIX, ResourceRef, http_status, parse_duration,
    parse_renewal_token,
};
use serde_json::{Value, json};

/// Longest wait one tool call accepts. An agent that needs longer calls again.
pub(crate) const MAX_TOOL_WAIT: Duration = Duration::from_secs(10 * 60);
/// Longest wait one backend request carries; under the server's per-request bound, so
/// each request is one genuine long poll.
const MAX_REQUEST_WAIT: Duration = Duration::from_secs(50);
/// HS2-MP402X: how much earlier than asked a server may end a wait by this caller's clock
/// (it counts whole wall-clock milliseconds) and still count as having held it.
const CLOCK_ALLOWANCE: Duration = Duration::from_secs(1);

pub(crate) fn definition() -> Value {
    json!({
        "name": "hotsheet_resource_claim",
        "description": "Claim a named machine resource (for example a VM that allows one automation process) for a duration, renew it, release it, or inspect it. acquire returns claim_id, fencing_token, renewal_token and expires_at; hand the renewal_token to the task that uses the resource so it renews its own lease, and let the lease lapse if the task is stuck. renew and release need only the renewal_token. acquire answers { acquired: false, code, holder } when the resource is held.",
        "inputSchema": { "type": "object", "required": ["action"], "properties": {
            "action": { "type": "string", "enum": ["acquire", "renew", "release", "show", "list"] },
            "target": { "type": "string", "description": "<namespace>/<resource>; required for acquire and show, optional check for renew and release" },
            "namespace": { "type": "string", "description": "namespace to list" },
            "duration": { "type": "string", "description": "lease length for acquire and renew, such as 90s, 30m or 1h30m (at most 24h)" },
            "wait": { "type": "string", "description": "acquire: wait this long for a release or expiry instead of failing at once; show: wait this long for the resource to become free (at most 10m per call)" },
            "metadata": { "type": "object", "additionalProperties": { "type": "string" }, "description": "holder metadata for acquire; a plain:, hmac-sha256: or enc:v1: prefix on a value selects its codec. Never put customer content here." },
            "renewal_token": { "type": "string", "description": "the token acquire returned; required for renew and release" }
        }}
    })
}

pub(crate) fn backend_error(error: ClaimError) -> BackendError {
    BackendError {
        status: Some(http_status(&error)),
        message: format!("{}: {error}", error.code()),
    }
}

fn claim_error(error: ClaimError) -> String {
    format!("{}: {error}", error.code())
}

fn optional<'a>(args: &'a Value, name: &str) -> Result<Option<&'a str>, String> {
    match args.get(name) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{name} must be a string when present")),
    }
}

fn required<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    optional(args, name)?.ok_or_else(|| format!("{name} is required"))
}

fn duration(args: &Value, name: &str) -> Result<Option<Duration>, String> {
    optional(args, name)?
        .map(|text| parse_duration(text).map_err(claim_error))
        .transpose()
}

fn millis(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn target_path(target: &str, action: Option<&str>) -> Result<String, String> {
    let target = ResourceRef::parse(target).map_err(claim_error)?;
    Ok(match action {
        Some(action) => format!("{ROUTE_PREFIX}/{target}/{action}"),
        None => format!("{ROUTE_PREFIX}/{target}"),
    })
}

/// The targeted renew or release route for a token. The token names its own claim, so
/// `target` is only an optional cross-check; the backend's secret, not the token,
/// authenticates this shim to a server.
fn token_path(args: &Value, action: &str) -> Result<String, String> {
    let named = parse_renewal_token(required(args, "renewal_token")?)
        .map_err(claim_error)?
        .target;
    if let Some(target) = optional(args, "target")? {
        if ResourceRef::parse(target).map_err(claim_error)? != named {
            return Err(claim_error(ClaimError::TokenInvalid));
        }
    }
    Ok(format!("{ROUTE_PREFIX}/{named}/{action}"))
}

/// Park on the backend until `settled` accepts an answer or `wait` runs out. Each
/// request is one long poll; the next is issued only when the previous one returns, so
/// a waiting tool call sends one request per [`MAX_REQUEST_WAIT`] and none in between.
///
/// HS2-MP402X: a request that asked for the whole remaining wait is final. The server
/// judges that wait by its own clock (whole wall-clock milliseconds), so its timeout can
/// land a little before this caller's deadline; asking again then sent a second request
/// during an idle wait, and a server answering early repeatedly would have been asked in
/// a loop. Only a request cut short by [`MAX_REQUEST_WAIT`] is followed by another.
fn long_poll(
    backend: &dyn Backend,
    path: &str,
    body: Value,
    wait: Duration,
    settled: impl Fn(&Value) -> bool,
) -> Result<Value, String> {
    long_poll_capped(backend, path, body, wait, MAX_REQUEST_WAIT, settled)
}

/// [`long_poll`] with each request's wait capped at `max_request`.
fn long_poll_capped(
    backend: &dyn Backend,
    path: &str,
    mut body: Value,
    wait: Duration,
    max_request: Duration,
    settled: impl Fn(&Value) -> bool,
) -> Result<Value, String> {
    if wait > MAX_TOOL_WAIT {
        return Err(format!(
            "invalid: wait is at most {} minutes per call; call again to keep waiting",
            MAX_TOOL_WAIT.as_secs() / 60
        ));
    }
    let deadline = Instant::now() + wait;
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        let asked = remaining.min(max_request);
        // Rounded up so the truncated fraction of a millisecond is not lost from the wait.
        body["wait_ms"] = millis(asked + Duration::from_nanos(999_999)).into();
        let sent = Instant::now();
        let answer = backend.send("POST", path, &body).map_err(crate::be_msg)?;
        // A capped request that the server held for its whole wait (allowing for its
        // clock) is followed by another; any other unsettled answer is the verdict.
        let held = sent.elapsed() + CLOCK_ALLOWANCE >= asked;
        if settled(&answer) || remaining <= max_request || !held || Instant::now() >= deadline {
            return Ok(answer);
        }
    }
}

pub(crate) fn dispatch(args: &Value, backend: &dyn Backend) -> Result<Value, String> {
    // tools/call does not enforce inputSchema; every argument is checked here.
    match required(args, "action")? {
        "acquire" => {
            let path = target_path(required(args, "target")?, Some("acquire"))?;
            let lease = duration(args, "duration")?.ok_or("duration is required")?;
            let mut body = json!({ "duration_ms": millis(lease) });
            match args.get("metadata") {
                None | Some(Value::Null) => {}
                Some(metadata @ Value::Object(_)) => body["metadata"] = metadata.clone(),
                Some(_) => return Err("metadata must be an object of strings".into()),
            }
            match duration(args, "wait")? {
                None => backend.send("POST", &path, &body).map_err(crate::be_msg),
                Some(wait) => long_poll(backend, &path, body, wait, |answer| {
                    answer["acquired"] == true
                }),
            }
        }
        "renew" => {
            let lease = duration(args, "duration")?.ok_or("duration is required")?;
            let body = json!({
                "renewal_token": required(args, "renewal_token")?,
                "duration_ms": millis(lease),
            });
            backend
                .send("POST", &token_path(args, "renew")?, &body)
                .map_err(crate::be_msg)
        }
        "release" => {
            let body = json!({ "renewal_token": required(args, "renewal_token")? });
            backend
                .send("POST", &token_path(args, "release")?, &body)
                .map_err(crate::be_msg)
        }
        "show" => {
            let target = required(args, "target")?;
            match duration(args, "wait")? {
                None => backend
                    .get(&target_path(target, None)?, &[])
                    .map_err(crate::be_msg),
                Some(wait) => long_poll(
                    backend,
                    &target_path(target, Some("wait"))?,
                    json!({}),
                    wait,
                    |answer| answer["held"] == false,
                ),
            }
        }
        "list" => {
            let namespace = Namespace::parse(required(args, "namespace")?).map_err(claim_error)?;
            backend
                .get(&format!("{ROUTE_PREFIX}/{namespace}"), &[])
                .map_err(crate::be_msg)
        }
        _ => Err("unsupported resource claim action".into()),
    }
}

#[cfg(test)]
mod tests {
    //! HS2-MP402X: how many requests a waiting acquire sends, against a server that answers
    //! its timeout early by this caller's clock.
    use super::*;
    use crate::BackendError;
    use std::sync::Mutex;

    /// Answers every acquire with `wait_timeout` at once and records the asked wait.
    struct EarlyTimeout(Mutex<Vec<u64>>);

    impl Backend for EarlyTimeout {
        fn get(&self, _: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
            unreachable!("acquire sends only")
        }
        fn send(&self, _: &str, _: &str, body: &Value) -> Result<Value, BackendError> {
            self.0
                .lock()
                .unwrap()
                .push(body["wait_ms"].as_u64().unwrap());
            Ok(json!({"acquired": false, "code": "wait_timeout", "holder": {"claim_id": "c1"}}))
        }
    }

    fn acquire(backend: &EarlyTimeout, wait: &str) -> Value {
        dispatch(
            &json!({"action":"acquire","target":"vms/mac-1","duration":"10m","wait":wait}),
            backend,
        )
        .unwrap()
    }

    #[test]
    fn hs2_mp402x_a_full_wait_timeout_is_final_even_when_it_lands_early() {
        let backend = EarlyTimeout(Mutex::new(Vec::new()));
        // The server's timeout lands before this caller's deadline (its clock rounds to
        // whole milliseconds, or runs a little ahead): one request, never a loop.
        let answer = acquire(&backend, "1500ms");
        assert_eq!(answer["code"], "wait_timeout");
        let asked = backend.0.lock().unwrap().clone();
        assert_eq!(asked.len(), 1, "{asked:?}");
        assert!((1_400..=1_500).contains(&asked[0]), "{asked:?}");
    }

    #[test]
    fn hs2_mp402x_a_capped_request_answered_early_is_not_asked_again_in_a_loop() {
        let backend = EarlyTimeout(Mutex::new(Vec::new()));
        // Longer than one request may wait: the first is cut at MAX_REQUEST_WAIT. A server
        // that ends it at once did not hold it, so its answer is the verdict, not a reason
        // to ask again and again until the caller's deadline.
        let started = Instant::now();
        let answer = acquire(&backend, "90s");
        assert_eq!(answer["code"], "wait_timeout");
        assert!(started.elapsed() < Duration::from_secs(5));
        let asked = backend.0.lock().unwrap().clone();
        assert_eq!(asked, [millis(MAX_REQUEST_WAIT)]);
    }

    /// A server that holds each capped request for its whole wait is asked again for what
    /// is left, once per request; here its clock is simulated by a backend that reports
    /// having held the wait (the caller measures by its own clock, so the fake sleeps).
    struct HoldsWait(Mutex<Vec<u64>>);

    impl Backend for HoldsWait {
        fn get(&self, _: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
            unreachable!("acquire sends only")
        }
        fn send(&self, _: &str, _: &str, body: &Value) -> Result<Value, BackendError> {
            let wait = body["wait_ms"].as_u64().unwrap();
            self.0.lock().unwrap().push(wait);
            std::thread::sleep(Duration::from_millis(wait));
            Ok(json!({"acquired": false, "code": "wait_timeout", "holder": {"claim_id": "c1"}}))
        }
    }

    #[test]
    fn hs2_mp402x_a_held_capped_request_is_followed_by_one_for_what_is_left() {
        // With requests capped at 1.5 s, a 1.8 s wait is one held, capped request and one
        // more for what is left.
        let backend = HoldsWait(Mutex::new(Vec::new()));
        let answer = long_poll_capped(
            &backend,
            "/resource-claims/vms/mac-1/acquire",
            json!({"duration_ms": 600_000}),
            Duration::from_millis(1_800),
            Duration::from_millis(1_500),
            |answer| answer["acquired"] == true,
        )
        .unwrap();
        assert_eq!(answer["code"], "wait_timeout");
        let asked = backend.0.lock().unwrap().clone();
        assert_eq!(asked.len(), 2, "{asked:?}");
        assert_eq!(asked[0], 1_500);
        assert!(asked[1] <= 300, "{asked:?}");
    }
}
