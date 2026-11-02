use std::collections::BTreeMap;

use hotsheet_model::Ulid;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::{ProviderConnection, ProviderError};

pub(super) const RESERVED: &str = "github_endpoint";

/// Content-free errors: provider config/credential paths and raw input are never displayed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum EndpointError {
    #[error("GitHub endpoint metadata or intent is invalid; review this source")]
    Invalid,
    #[error("GitHub endpoint decision changed; inspect this source again")]
    Conflict,
    #[error("GitHub endpoint operation is awaiting its checkout link; resume or compensate it")]
    Pending,
    #[error("GitHub sign-in site differs from the reviewed endpoint decision")]
    Drift,
    #[error("GitHub sign-in site cannot be verified; review this source")]
    Unavailable,
    #[error("GitHub endpoint source proof belongs to an unsupported owner")]
    UnsupportedOwner,
    #[error("Provider configuration cannot be read or published")]
    Storage,
}

impl From<ProviderError> for EndpointError {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Endpoint(error) => error,
            _ => Self::Storage,
        }
    }
}

/// A required nullable field, not an optional field that silently defaults when missing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Endpoint {
    Public,
    Url(String),
}

impl Endpoint {
    pub fn validate(&self) -> Result<(), EndpointError> {
        let Self::Url(raw) = self else {
            return Ok(());
        };
        if raw.is_empty()
            || raw.chars().any(|c| c.is_control() || c.is_whitespace())
            || raw.contains('\\')
            || raw.contains('?')
            || raw.contains('#')
        {
            return Err(EndpointError::Invalid);
        }
        // HS2-6H8ZY6: a special-scheme URL parser may normalize shorthand,
        // empty userinfo or an empty port. Validate the preserved raw spelling first.
        let after_scheme = if raw
            .get(..8)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("https://"))
        {
            &raw[8..]
        } else if raw
            .get(..7)
            .is_some_and(|scheme| scheme.eq_ignore_ascii_case("http://"))
        {
            &raw[7..]
        } else {
            return Err(EndpointError::Invalid);
        };
        let authority = after_scheme.split('/').next().unwrap_or_default();
        if authority.is_empty() || authority.contains('@') || authority.ends_with(':') {
            return Err(EndpointError::Invalid);
        }
        let parsed = url::Url::parse(raw).map_err(|_| EndpointError::Invalid)?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(EndpointError::Invalid);
        }
        // URL validates only; preserve original ports, path case/encodings and trailing slash.
        Ok(())
    }

    pub fn from_record(record: &ProviderConnection) -> Result<Self, EndpointError> {
        match record.settings.get("api_base") {
            None => Ok(Self::Public),
            Some(Value::String(value)) => {
                let endpoint = Self::Url(value.clone());
                endpoint.validate()?;
                Ok(endpoint)
            }
            _ => Err(EndpointError::Invalid),
        }
    }

    #[cfg(test)]
    pub(super) fn write(&self, record: &mut ProviderConnection) -> Result<(), EndpointError> {
        let settings = record
            .settings
            .as_object_mut()
            .ok_or(EndpointError::Invalid)?;
        match self {
            Self::Public => {
                settings.remove("api_base");
            }
            Self::Url(value) => {
                settings.insert("api_base".into(), Value::String(value.clone()));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Decision {
    LegacyUnknown {
        credential_ref: String,
        endpoint: Endpoint,
    },
    Explicit {
        credential_ref: String,
        endpoint: Endpoint,
    },
    Inferred {
        credential_ref: String,
        endpoint: Endpoint,
        origin: String,
    },
}

impl Decision {
    pub fn endpoint(&self) -> &Endpoint {
        match self {
            Self::LegacyUnknown { endpoint, .. }
            | Self::Explicit { endpoint, .. }
            | Self::Inferred { endpoint, .. } => endpoint,
        }
    }
    pub fn credential_ref(&self) -> &str {
        match self {
            Self::LegacyUnknown { credential_ref, .. }
            | Self::Explicit { credential_ref, .. }
            | Self::Inferred { credential_ref, .. } => credential_ref,
        }
    }
    pub(super) fn validate(&self) -> Result<(), EndpointError> {
        if self.credential_ref().is_empty() {
            return Err(EndpointError::Invalid);
        }
        self.endpoint().validate()?;
        if let Self::Inferred { origin, .. } = self {
            if crate::github_origin::normalize_web_base(origin)
                .map_err(|_| EndpointError::Invalid)?
                != *origin
            {
                return Err(EndpointError::Invalid);
            }
        }
        // Persisted decisions bind a fixed endpoint: never re-derive a different route on read.
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Identity {
    Legacy,
    Known { incarnation: Ulid, revision: Ulid },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expected {
    pub identity: Identity,
    pub record_sha256: String,
}

/// Required JSON null/value, with no missing-field compatibility default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Nullable<T> {
    Null,
    Value(T),
}

impl<T> Nullable<T> {
    pub fn as_ref(&self) -> Option<&T> {
        match self {
            Self::Null => None,
            Self::Value(value) => Some(value),
        }
    }
    pub fn is_none(&self) -> bool {
        self.as_ref().is_none()
    }
}
impl<T> From<Option<T>> for Nullable<T> {
    fn from(value: Option<T>) -> Self {
        value.map_or(Self::Null, Self::Value)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ExpectedWire {
    incarnation: Nullable<Ulid>,
    revision: Nullable<Ulid>,
    record_sha256: String,
}

impl Serialize for Expected {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let (incarnation, revision) = match &self.identity {
            Identity::Legacy => (None, None),
            Identity::Known {
                incarnation,
                revision,
            } => (Some(*incarnation), Some(*revision)),
        };
        ExpectedWire {
            incarnation: incarnation.into(),
            revision: revision.into(),
            record_sha256: self.record_sha256.clone(),
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Expected {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let wire = ExpectedWire::deserialize(deserializer)?;
        let identity = match (wire.incarnation, wire.revision) {
            (Nullable::Null, Nullable::Null) => Identity::Legacy,
            (Nullable::Value(incarnation), Nullable::Value(revision)) => Identity::Known {
                incarnation,
                revision,
            },
            _ => return Err(serde::de::Error::custom("invalid endpoint expected state")),
        };
        Ok(Self {
            identity,
            record_sha256: wire.record_sha256,
        })
    }
}

impl Expected {
    pub fn capture(record: &ProviderConnection) -> Result<Self, EndpointError> {
        let identity = match State::load(record)? {
            None => Identity::Legacy,
            Some(state) => Identity::Known {
                incarnation: state.incarnation,
                revision: state.revision,
            },
        };
        Ok(Self {
            identity,
            record_sha256: record_hash(record)?,
        })
    }
    pub(super) fn check(&self, record: &ProviderConnection) -> Result<(), EndpointError> {
        if !valid_hash(&self.record_sha256) || Self::capture(record)? != *self {
            return Err(EndpointError::Conflict);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Link {
    pub checkout_id: String,
    pub make_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Choice {
    Legacy,
    Keep,
    Explicit {
        credential_ref: String,
        endpoint: Endpoint,
    },
    InferSite {
        credential_ref: String,
        endpoint: Endpoint,
        origin: String,
    },
    Copy {
        source_id: String,
        expected: Expected,
        unknown: bool,
    },
}

impl Choice {
    fn validate(&self) -> Result<(), EndpointError> {
        match self {
            Self::Explicit {
                credential_ref,
                endpoint,
            }
            | Self::InferSite {
                credential_ref,
                endpoint,
                ..
            } => {
                if credential_ref.is_empty() {
                    return Err(EndpointError::Invalid);
                }
                endpoint.validate()?;
                if let Self::InferSite { origin, .. } = self {
                    if crate::github_origin::normalize_web_base(origin)
                        .map_err(|_| EndpointError::Invalid)?
                        != *origin
                    {
                        return Err(EndpointError::Invalid);
                    }
                }
            }
            Self::Copy {
                source_id,
                expected,
                ..
            } => {
                if source_id.is_empty() || !valid_hash(&expected.record_sha256) {
                    return Err(EndpointError::Invalid);
                }
            }
            Self::Legacy | Self::Keep => {}
        }
        Ok(())
    }
}

/// Typed intent. Copy authority is an opaque same-owner proof, never a deserialized claim.
#[derive(Debug, Clone)]
pub enum Intent {
    Legacy,
    Keep,
    Explicit {
        credential_ref: String,
        endpoint: Endpoint,
    },
    InferSite {
        credential_ref: String,
        endpoint: Endpoint,
        origin: String,
    },
    Copy {
        proof: super::registry::SourceProof,
        unknown: bool,
    },
}

impl Intent {
    pub(super) fn choice(&self) -> Choice {
        match self {
            Self::Legacy => Choice::Legacy,
            Self::Keep => Choice::Keep,
            Self::Explicit {
                credential_ref,
                endpoint,
            } => Choice::Explicit {
                credential_ref: credential_ref.clone(),
                endpoint: endpoint.clone(),
            },
            Self::InferSite {
                credential_ref,
                endpoint,
                origin,
            } => Choice::InferSite {
                credential_ref: credential_ref.clone(),
                endpoint: endpoint.clone(),
                origin: origin.clone(),
            },
            Self::Copy { proof, unknown } => Choice::Copy {
                source_id: proof.source_id.clone(),
                expected: proof.expected.clone(),
                unknown: *unknown,
            },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Completed,
    AwaitingLink,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Operation {
    pub id: Ulid,
    pub request_sha256: String,
    pub expected: Nullable<Expected>,
    pub(super) choice: Choice,
    pub state: OperationState,
    pub result_revision: Ulid,
    pub result_sha256: String,
    pub link: Nullable<Link>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Box<ProviderConnection>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_sha256: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub version: u8,
    pub incarnation: Ulid,
    pub revision: Ulid,
    pub decision: Decision,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<Operation>,
}

impl State {
    pub fn load(record: &ProviderConnection) -> Result<Option<Self>, EndpointError> {
        let Some(raw) = record.settings.get(RESERVED) else {
            return Ok(None);
        };
        let state: Self =
            serde_json::from_value(raw.clone()).map_err(|_| EndpointError::Invalid)?;
        if state.version != 1 || record.provider != "github" {
            return Err(EndpointError::Invalid);
        }
        state.decision.validate()?;
        if state.decision.credential_ref() != credential(record)?
            || *state.decision.endpoint() != Endpoint::from_record(record)?
        {
            return Err(EndpointError::Invalid);
        }
        if let Some(operation) = &state.operation {
            operation.choice.validate()?;
            if !valid_hash(&operation.request_sha256)
                || !valid_hash(&operation.result_sha256)
                || operation
                    .expected
                    .as_ref()
                    .is_some_and(|expected| !valid_hash(&expected.record_sha256))
                || operation.result_revision != state.revision
            {
                return Err(EndpointError::Invalid);
            }
            if operation.state == OperationState::Completed
                && (operation.before.is_some() || operation.before_sha256.is_some())
            {
                return Err(EndpointError::Invalid);
            }
            if operation.state == OperationState::AwaitingLink && operation.link.is_none() {
                return Err(EndpointError::Invalid);
            }
            if operation.state == OperationState::AwaitingLink
                && (operation.result_sha256 != record_hash(record)?
                    || operation.expected.is_none() != operation.before.is_none())
            {
                return Err(EndpointError::Invalid);
            }
            if operation
                .link
                .as_ref()
                .is_some_and(|link| link.checkout_id.is_empty())
            {
                return Err(EndpointError::Invalid);
            }
            if operation.request_sha256
                != request_hash(
                    record,
                    operation.id,
                    &operation.expected.as_ref().cloned(),
                    &operation.choice,
                    &operation.link.as_ref().cloned(),
                )?
                || operation.result_sha256 != record_hash(record)?
            {
                return Err(EndpointError::Invalid);
            }
            if let Some(before) = &operation.before {
                if before.id != record.id || before.provider != "github" {
                    return Err(EndpointError::Invalid);
                }
                if before
                    .settings
                    .get(RESERVED)
                    .and_then(|value| value.get("operation"))
                    .is_some()
                {
                    return Err(EndpointError::Invalid);
                }
                if let Some(previous) = Self::load(before)? {
                    if previous.operation.is_some() {
                        return Err(EndpointError::Invalid);
                    }
                }
                if operation.before_sha256.as_deref() != Some(hash(before)?.as_str()) {
                    return Err(EndpointError::Invalid);
                }
                operation
                    .expected
                    .as_ref()
                    .ok_or(EndpointError::Invalid)?
                    .check(before)
                    .map_err(|_| EndpointError::Invalid)?;
            } else if operation.before_sha256.is_some() {
                return Err(EndpointError::Invalid);
            }
        }
        Ok(Some(state))
    }
    pub fn is_pending(&self) -> bool {
        self.operation
            .as_ref()
            .is_some_and(|op| op.state == OperationState::AwaitingLink)
    }
    pub(super) fn write(&self, record: &mut ProviderConnection) -> Result<(), EndpointError> {
        record
            .settings
            .as_object_mut()
            .ok_or(EndpointError::Invalid)?
            .insert(
                RESERVED.into(),
                serde_json::to_value(self).map_err(|_| EndpointError::Invalid)?,
            );
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Observation {
    Available {
        credential_ref: String,
        site: String,
    },
    Unavailable,
}

pub fn admit_read(
    record: &ProviderConnection,
    observation: &Observation,
) -> Result<(), EndpointError> {
    let Some(state) = State::load(record)? else {
        return Ok(());
    };
    // Pending blocks every kind, including LegacyUnknown, before compatibility.
    if state.is_pending() {
        return Err(EndpointError::Pending);
    }
    check_decision(&state.decision, observation)
}

pub(super) fn check_decision(
    decision: &Decision,
    observation: &Observation,
) -> Result<(), EndpointError> {
    let Decision::Inferred {
        credential_ref,
        origin,
        ..
    } = decision
    else {
        return Ok(());
    };
    match observation {
        Observation::Unavailable => Err(EndpointError::Unavailable),
        Observation::Available {
            credential_ref: current,
            site,
        } => {
            let site = crate::github_origin::normalize_web_base(site)
                .map_err(|_| EndpointError::Unavailable)?;
            if current != credential_ref || site != *origin {
                Err(EndpointError::Drift)
            } else {
                Ok(())
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleOrigin {
    Missing,
    Known(String),
    Invalid,
}

/// For an already-read bundle only. This function never obtains or refreshes a credential.
pub fn validate_bundle_origin(
    record: &ProviderConnection,
    origin: &BundleOrigin,
) -> Result<(), EndpointError> {
    let Some(state) = State::load(record)? else {
        return Ok(());
    };
    if state.is_pending() {
        return Err(EndpointError::Pending);
    }
    let Decision::Inferred {
        origin: expected, ..
    } = &state.decision
    else {
        return Ok(());
    };
    match origin {
        BundleOrigin::Missing => Ok(()),
        BundleOrigin::Invalid => Err(EndpointError::Unavailable),
        BundleOrigin::Known(site) => {
            let site = crate::github_origin::normalize_web_base(site)
                .map_err(|_| EndpointError::Unavailable)?;
            if site == *expected {
                Ok(())
            } else {
                Err(EndpointError::Drift)
            }
        }
    }
}

/// Core guard only: all actual generic edit/disable/webhook callers still need adoption.
pub fn guard_ordinary(
    before: &ProviderConnection,
    incoming: &mut ProviderConnection,
) -> Result<(), EndpointError> {
    if before.id != incoming.id
        || before.provider != "github"
        || before.provider != incoming.provider
    {
        return Err(EndpointError::Conflict);
    }
    let state = State::load(before)?;
    if state.as_ref().is_some_and(State::is_pending) {
        return Err(EndpointError::Pending);
    }
    if let Some(supplied) = incoming.settings.get(RESERVED) {
        if Some(supplied) != before.settings.get(RESERVED) {
            return Err(EndpointError::Invalid);
        }
    }
    // Unmarked legacy nonmaterial edits retain existing bytes, even invalid historical
    // endpoint data. Only a new material decision is validated/stamped here.
    let changed = before.settings.get("api_base") != incoming.settings.get("api_base")
        || before.settings.get("credential") != incoming.settings.get("credential");
    if changed
        && state
            .as_ref()
            .is_some_and(|state| !matches!(state.decision, Decision::LegacyUnknown { .. }))
    {
        return Err(EndpointError::Conflict);
    }
    if changed {
        let state = State {
            version: 1,
            incarnation: state.map_or_else(Ulid::new, |state| state.incarnation),
            revision: Ulid::new(),
            decision: Decision::LegacyUnknown {
                credential_ref: credential(incoming)?.into(),
                endpoint: Endpoint::from_record(incoming)?,
            },
            operation: None,
        };
        state.write(incoming)?;
    } else if let Some(mut state) = state {
        // A nonmaterial edit keeps authority but retires an exact-result receipt.
        if request_connection(before)? != request_connection(incoming)? {
            state.operation = None;
        }
        state.write(incoming)?;
    }
    Ok(())
}

#[derive(Debug, Clone)]
pub struct Request {
    pub operation_id: Ulid,
    /// None is an explicit new-create branch, never fallback from a failed existing review.
    pub expected: Option<Expected>,
    pub connection: ProviderConnection,
    pub intent: Intent,
    pub link: Option<Link>,
}

pub(super) fn credential(record: &ProviderConnection) -> Result<&str, EndpointError> {
    record
        .settings
        .get("credential")
        .and_then(|value| value.get("secret"))
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or(EndpointError::Invalid)
}

pub(super) fn without_operation(
    record: &ProviderConnection,
) -> Result<ProviderConnection, EndpointError> {
    let mut result = record.clone();
    if let Some(mut state) = State::load(record)? {
        state.operation = None;
        state.write(&mut result)?;
    }
    Ok(result)
}

pub(super) fn record_hash(record: &ProviderConnection) -> Result<String, EndpointError> {
    let mut record = record.clone();
    if let Some(value) = record
        .settings
        .get_mut(RESERVED)
        .and_then(Value::as_object_mut)
    {
        value.remove("operation");
    }
    hash(&record)
}

pub(super) fn request_connection(
    record: &ProviderConnection,
) -> Result<ProviderConnection, EndpointError> {
    let mut result = record.clone();
    result
        .settings
        .as_object_mut()
        .ok_or(EndpointError::Invalid)?
        .remove(RESERVED);
    Ok(result)
}

pub(super) fn request_hash(
    record: &ProviderConnection,
    operation_id: Ulid,
    expected: &Option<Expected>,
    choice: &Choice,
    link: &Option<Link>,
) -> Result<String, EndpointError> {
    #[derive(Serialize)]
    struct Payload<'a> {
        version: u8,
        operation_id: Ulid,
        expected: &'a Option<Expected>,
        connection: ProviderConnection,
        choice: &'a Choice,
        link: &'a Option<Link>,
    }
    hash(&Payload {
        version: 1,
        operation_id,
        expected,
        connection: request_connection(record)?,
        choice,
        link,
    })
}

pub(super) fn hash(value: &impl Serialize) -> Result<String, EndpointError> {
    fn canonical(value: Value) -> Value {
        match value {
            Value::Object(map) => Value::Object(
                map.into_iter()
                    .map(|(k, v)| (k, canonical(v)))
                    .collect::<BTreeMap<_, _>>()
                    .into_iter()
                    .collect(),
            ),
            Value::Array(values) => Value::Array(values.into_iter().map(canonical).collect()),
            other => other,
        }
    }
    let bytes = serde_json::to_vec(&canonical(
        serde_json::to_value(value).map_err(|_| EndpointError::Invalid)?,
    ))
    .map_err(|_| EndpointError::Invalid)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn valid_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}
