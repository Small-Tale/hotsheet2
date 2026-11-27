//! HS2-VQ92XZ: the headless machine read of a paired remote server with no open project,
//! `hotsheet-cli remote machine <binding>`. It is the CLI side of the desktop bridge's
//! Systems path (HS2-CMXKK2, docs/08): the same private binding connection the CLI already
//! uses for a remote's checkouts and targets, a binding re-checked by fresh authenticated
//! discovery on every run, and only the same three read-only machine routes with only their
//! own query parameters. Capacity is read through the first checkout discovery reports
//! available. Only the machine readings are printed: never a session, cookie or credential.

use hotsheet_ticketing::remote_servers::{
    RemoteCheckoutState, RemoteDiscovery, RemoteError, RemoteFailure, RemoteView,
};
use serde_json::{Map, Value};

/// The longest a single machine long poll waits, the same bound as a target's capacity wait.
pub const MAX_MACHINE_WAIT_MS: u64 = 8_000;

/// One of the three machine routes a paired remote answers without an open project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MachineRead {
    /// `GET /host-health`: the latest evaluated host sample.
    Health,
    /// `GET /host-health/cpu`: the live CPU frame; with `after` (a frame `seq`) and a wait,
    /// the first newer frame.
    Cpu {
        after: Option<u64>,
        wait_ms: Option<u64>,
    },
    /// `GET /checkouts/{checkout}/target-capacity`: the machine's capacity; with `after` (a
    /// snapshot `version`) and a wait, the next changed snapshot.
    Capacity {
        after: Option<String>,
        wait_ms: Option<u64>,
        claim_namespaces: Vec<String>,
    },
}

impl MachineRead {
    /// The output key of this reading.
    pub fn key(&self) -> &'static str {
        match self {
            Self::Health => "health",
            Self::Cpu { .. } => "cpu",
            Self::Capacity { .. } => "capacity",
        }
    }

    /// The server path this reading asks for, through `checkout` for capacity. `None` when a
    /// capacity read has no checkout to go through.
    pub fn server_path(&self, checkout: Option<&str>) -> Option<String> {
        let mut query = Vec::new();
        let path = match self {
            Self::Health => "/host-health".to_owned(),
            Self::Cpu { after, wait_ms } => {
                if let Some(after) = after {
                    query.push(format!("after={after}"));
                    query.push(format!(
                        "timeout_ms={}",
                        wait_ms.unwrap_or(0).min(MAX_MACHINE_WAIT_MS)
                    ));
                }
                "/host-health/cpu".to_owned()
            }
            Self::Capacity {
                after,
                wait_ms,
                claim_namespaces,
            } => {
                if !claim_namespaces.is_empty() {
                    query.push(format!(
                        "claim_namespaces={}",
                        encode(&claim_namespaces.join(","))
                    ));
                }
                if let Some(after) = after {
                    query.push(format!("after={}", encode(after)));
                    query.push(format!(
                        "wait_ms={}",
                        wait_ms.unwrap_or(0).min(MAX_MACHINE_WAIT_MS)
                    ));
                }
                format!("/checkouts/{}/target-capacity", encode(checkout?))
            }
        };
        Some(if query.is_empty() {
            path
        } else {
            format!("{path}?{}", query.join("&"))
        })
    }
}

/// HS2-SW03MK: which machine readings to take, as `remote machine`'s flags, the
/// `POST /remotes/{id}/machine` body and the `hotsheet_remotes` `machine` action name them.
/// No reading named means all three, with capacity skipped (not failed) when the remote has
/// no available project.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MachineQuery {
    /// The latest evaluated host sample.
    #[serde(default)]
    pub health: bool,
    /// The live CPU frame.
    #[serde(default)]
    pub cpu: bool,
    /// The machine's capacity, read through the first available project.
    #[serde(default)]
    pub capacity: bool,
    /// Also list live claims of these resource-claim namespaces (with `capacity`).
    #[serde(default)]
    pub claim_namespaces: Vec<String>,
    /// The CPU frame `seq` or capacity `version` the caller has: with `wait_ms`, answer
    /// when a newer one arrives. Needs exactly one of `cpu` or `capacity`.
    #[serde(default, deserialize_with = "cursor")]
    pub after: Option<String>,
    /// How long to wait for a newer reading, capped at [`MAX_MACHINE_WAIT_MS`].
    #[serde(default)]
    pub wait_ms: Option<u64>,
}

/// A cursor given as a number (a CPU frame `seq`) or as text.
fn cursor<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    use serde::Deserialize;
    #[derive(serde::Deserialize)]
    #[serde(untagged)]
    enum Cursor {
        Number(u64),
        Text(String),
    }
    Ok(
        Option::<Cursor>::deserialize(deserializer)?.map(|cursor| match cursor {
            Cursor::Number(number) => number.to_string(),
            Cursor::Text(text) => text,
        }),
    )
}

impl MachineQuery {
    /// The reads to make, in order, and whether capacity may be skipped for want of a
    /// project (only when no reading was named). An ambiguous or malformed cursor, or
    /// claim namespaces without capacity, is refused before any request.
    pub fn reads(&self) -> Result<(Vec<MachineRead>, bool), MachineError> {
        if self.after.is_some() && usize::from(self.cpu) + usize::from(self.capacity) != 1 {
            return Err(MachineError::Invalid(
                "after (a cursor) needs exactly one of cpu or capacity",
            ));
        }
        if self.wait_ms.is_some() && self.after.is_none() {
            return Err(MachineError::Invalid(
                "a wait needs after, the cursor it waits past",
            ));
        }
        if !self.claim_namespaces.is_empty() && !self.capacity {
            return Err(MachineError::Invalid(
                "claim namespaces need the capacity read",
            ));
        }
        let cpu_after = match (&self.after, self.cpu) {
            (Some(after), true) => Some(
                after
                    .parse::<u64>()
                    .map_err(|_| MachineError::Invalid("after with cpu is a frame seq number"))?,
            ),
            _ => None,
        };
        let all = !(self.health || self.cpu || self.capacity);
        let mut reads = Vec::new();
        if all || self.health {
            reads.push(MachineRead::Health);
        }
        if all || self.cpu {
            reads.push(MachineRead::Cpu {
                after: cpu_after,
                wait_ms: cpu_after.and(self.wait_ms),
            });
        }
        if all || self.capacity {
            let after = self.after.clone().filter(|_| self.capacity);
            reads.push(MachineRead::Capacity {
                wait_ms: after.as_ref().and(self.wait_ms),
                after,
                claim_namespaces: self.claim_namespaces.clone(),
            });
        }
        Ok((reads, all))
    }
}

/// Percent-encode everything but RFC 3986 unreserved bytes, so no value can add a query
/// parameter or move the request to another route.
fn encode(value: &str) -> String {
    value
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

/// Why a machine read could not be answered. Only codes and statuses are kept: a peer's
/// message never reaches the caller.
#[derive(Debug)]
pub enum MachineError {
    /// The registry, its credentials or discovery failed.
    Remote(RemoteError),
    /// The request did not reach the remote, or its reply was refused before any status.
    Transport(hotsheet_ticketing::orchestration::TargetFailure),
    /// The binding changed between discovery and the read: review and reconnect it.
    Changed,
    /// The remote server has no available project to read capacity through.
    NoCheckout,
    /// HS2-SW03MK: the readings asked for do not fit together; nothing was requested.
    Invalid(&'static str),
    /// The remote answered the read with an error status (and its stable code, if any).
    Refused {
        read: &'static str,
        status: u16,
        code: Option<String>,
    },
}

impl std::fmt::Display for MachineError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Remote(error) => write!(formatter, "{error}"),
            Self::Transport(failure) => write!(formatter, "remote machine read failed: {failure}"),
            Self::Changed => formatter.write_str(
                "remote_changed: the remote server changed; review and reconnect it",
            ),
            Self::NoCheckout => formatter.write_str(
                "no_available_checkout: this remote server has no project this computer can read its capacity through",
            ),
            Self::Invalid(message) => formatter.write_str(message),
            Self::Refused { read, status, code } => {
                write!(formatter, "the remote server refused the {read} read ({status}")?;
                if let Some(code) = code {
                    write!(formatter, ", {code}")?;
                }
                formatter.write_str(")")
            }
        }
    }
}

impl std::error::Error for MachineError {}

impl From<RemoteError> for MachineError {
    fn from(error: RemoteError) -> Self {
        Self::Remote(error)
    }
}

/// The binding this run reads through, after discovery re-checked it: its view, as the
/// private connection must still match, and the checkout capacity goes through.
pub(super) fn checked_binding(
    discovery: &RemoteDiscovery,
    connection: &RemoteView,
) -> Result<Option<String>, MachineError> {
    if discovery.remote.revoked {
        return Err(RemoteError::Revoked.into());
    }
    if let Some(failure) = discovery.failure {
        return Err(RemoteError::Verification(failure).into());
    }
    if connection.revoked {
        return Err(RemoteError::Revoked.into());
    }
    if connection.revision != discovery.remote.revision
        || connection.identity.server_id != discovery.remote.identity.server_id
    {
        return Err(MachineError::Changed);
    }
    Ok(discovery.checkouts.as_ref().and_then(|checkouts| {
        checkouts
            .iter()
            .find(|checkout| checkout.state == RemoteCheckoutState::Available)
            .map(|checkout| checkout.id.clone())
    }))
}

/// One reading from the remote's reply: its JSON on success, or the refusal with the peer's
/// stable `code` (never its message).
pub(super) fn reading(read: &MachineRead, status: u16, body: &[u8]) -> Result<Value, MachineError> {
    if status == 200 {
        return serde_json::from_slice(body).map_err(|_| {
            MachineError::Remote(RemoteError::Verification(RemoteFailure::InvalidResponse))
        });
    }
    let code = serde_json::from_slice::<Value>(body)
        .ok()
        .and_then(|value| value.get("code")?.as_str().map(str::to_owned))
        .filter(|code| {
            code.len() <= 64
                && code
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte == b'_' || byte.is_ascii_digit())
        });
    Err(MachineError::Refused {
        read: read.key(),
        status,
        code,
    })
}

/// The printed result: the binding, the machine's name, and each reading under its key. A
/// capacity read nobody asked for by name is left out, with why, when the remote has no
/// available project.
pub(super) fn report(
    discovery: &RemoteDiscovery,
    readings: Vec<(&'static str, Value)>,
    capacity_unavailable: bool,
) -> Value {
    let mut output = Map::new();
    output.insert("binding_id".into(), discovery.remote.id.clone().into());
    output.insert("label".into(), discovery.remote.label.clone().into());
    output.insert(
        "server_id".into(),
        discovery.remote.identity.server_id.clone().into(),
    );
    output.insert(
        "host_name".into(),
        discovery
            .host_name
            .clone()
            .map_or(Value::Null, Value::String),
    );
    for (key, value) in readings {
        output.insert(key.into(), value);
    }
    if capacity_unavailable {
        output.insert(
            "capacity_unavailable".into(),
            "no_available_checkout".into(),
        );
    }
    Value::Object(output)
}

#[cfg(test)]
#[path = "machine_tests.rs"]
mod tests;
