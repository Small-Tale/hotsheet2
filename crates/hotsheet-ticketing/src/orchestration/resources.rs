//! HS2-2FG91X: the resources orchestrated work declares, and how a pass saw them.
//!
//! A requirement names a resource of the resource-claim service (`docs/36`) as
//! `<namespace>/<resource>`. It is declared on a plan step or, for every piece of
//! work on a target, in the policy's `placement.resources`. The scheduler never
//! claims: it reads who holds what on the machine that hosts the target, treats a
//! held resource as unavailable capacity, and leaves the claim to the session.
//!
//! Names follow the claim service's own rules, restated here because this crate
//! does not depend on it; the runtime's tests pin the two against each other.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::contract::valid_token;
use super::{OrchestrationError, ResourceNeed};

/// Resources one plan step may require.
pub const MAX_STEP_RESOURCES: usize = 4;
/// Resources the policy may require of every piece of work on one target.
pub const MAX_TARGET_RESOURCES: usize = 4;
const MAX_NAMESPACE_BYTES: usize = 64;
const MAX_RESOURCE_BYTES: usize = 128;

fn starts_alphanumeric(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
}

impl ResourceNeed {
    /// The claim service's naming rules for a namespace and a resource.
    pub fn validate(&self) -> Result<(), OrchestrationError> {
        let namespace = &self.namespace;
        let namespace_ok = !namespace.is_empty()
            && namespace.len() <= MAX_NAMESPACE_BYTES
            && starts_alphanumeric(namespace)
            && !namespace.contains("..")
            && namespace.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_' | '-')
            });
        let resource = &self.resource;
        let resource_ok = !resource.is_empty()
            && resource.len() <= MAX_RESOURCE_BYTES
            && starts_alphanumeric(resource)
            && resource
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
        if namespace_ok && resource_ok {
            Ok(())
        } else {
            Err(OrchestrationError::Invalid(
                "a resource is <namespace>/<resource> in the claim service's names",
            ))
        }
    }

    /// HS2-NPAZQW: a requirement of the policy may name a pattern: `*` in the resource
    /// part matches any run of resource characters (`vm/*`, `vm/macos-*`). The work
    /// must hold any one declared resource that matches. The namespace stays exact.
    pub fn is_pattern(&self) -> bool {
        self.resource.contains('*')
    }

    /// HS2-NPAZQW: [`Self::validate`] for a policy requirement, which may be a
    /// pattern: the resource part may contain `*` and may start with one.
    pub fn validate_requirement(&self) -> Result<(), OrchestrationError> {
        if !self.is_pattern() {
            return self.validate();
        }
        let concrete = Self {
            namespace: self.namespace.clone(),
            resource: self.resource.replace('*', "x"),
        };
        concrete.validate()
    }

    /// HS2-NPAZQW: whether the concrete `other` satisfies this requirement: the same
    /// name, or for a pattern the same namespace and a resource the pattern matches.
    pub fn satisfied_by(&self, other: &ResourceNeed) -> bool {
        self.namespace == other.namespace
            && !other.is_pattern()
            && wildcard_match(&self.resource, &other.resource)
    }

    /// `<namespace>/<resource>`, as every surface writes it.
    pub fn name(&self) -> String {
        format!("{}/{}", self.namespace, self.resource)
    }

    /// Parse `<namespace>/<resource>`.
    pub fn parse(text: &str) -> Result<Self, OrchestrationError> {
        let (namespace, resource) = text.split_once('/').unwrap_or((text, ""));
        let need = Self {
            namespace: namespace.into(),
            resource: resource.into(),
        };
        need.validate()?;
        Ok(need)
    }
}

/// HS2-1X1TXC, HS2-NPAZQW: whether `pattern` matches `text`, where `*` in the pattern
/// matches any run of characters (none included) and every other character matches
/// itself. A pattern without `*` matches only the same text.
pub fn wildcard_match(pattern: &str, text: &str) -> bool {
    let Some((first, rest)) = pattern.split_once('*') else {
        return pattern == text;
    };
    let Some(mut remaining) = text.strip_prefix(first) else {
        return false;
    };
    let mut parts: Vec<&str> = rest.split('*').collect();
    let last = parts.pop().unwrap_or("");
    for part in parts {
        match remaining.find(part) {
            Some(at) => remaining = &remaining[at + part.len()..],
            None => return false,
        }
    }
    remaining.len() >= last.len() && remaining.ends_with(last)
}

/// Bounded, valid and each named once.
pub fn validate_needs(needs: &[ResourceNeed], max: usize) -> Result<(), OrchestrationError> {
    if needs.len() > max {
        return Err(OrchestrationError::Invalid("at most 4 resources"));
    }
    for (index, need) in needs.iter().enumerate() {
        need.validate()?;
        if needs[..index].contains(need) {
            return Err(OrchestrationError::Invalid("a resource is named once"));
        }
    }
    Ok(())
}

/// The policy's requirement for every piece of work on one target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetResources {
    pub target_id: String,
    pub resources: Vec<ResourceNeed>,
}

impl TargetResources {
    /// The stable code of what is wrong with this entry, if anything.
    pub fn problem(&self) -> Option<(&'static str, &'static str)> {
        if valid_token(&self.target_id).is_err() {
            return Some(("invalid_target", "target needs a canonical ULID"));
        }
        if self.resources.is_empty() || self.resources.len() > MAX_TARGET_RESOURCES {
            return Some((
                "invalid_resource_count",
                "a target requires 1 to 4 resources",
            ));
        }
        for (index, need) in self.resources.iter().enumerate() {
            // HS2-NPAZQW: a policy requirement may be a pattern.
            if need.validate_requirement().is_err() {
                return Some((
                    "invalid_resource",
                    "a resource is <namespace>/<resource> in the claim service's names",
                ));
            }
            if self.resources[..index].contains(need) {
                return Some(("duplicate_resource", "resource is listed twice"));
            }
        }
        None
    }
}

/// What a claim backend's configuration says to do while it cannot be reached
/// (`docs/36` §36.7).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OfflinePolicy {
    /// Nothing is assumed: work that needs the resource waits.
    #[default]
    FailClosed,
    /// Sessions take provisional, advisory claims that are reconciled later, so
    /// work may start. Such a claim is never reported as exclusive.
    Provisional,
}

/// Why the claims of one namespace are not known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnreadableReason {
    /// The machine answered that it could not read the namespace's backend.
    BackendUnreachable,
    /// One read lists a bounded number of namespaces and this one was left out.
    NamespaceLimit,
}

/// One namespace on one machine whose live claims a pass could not read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnreadableClaims {
    pub server_id: String,
    pub namespace: String,
    pub offline: OfflinePolicy,
    pub reason: UnreadableReason,
}

/// How one pass saw one required resource. The wire spelling equals `code()`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResourceState {
    /// Nobody holds it and no better-ranked work took it.
    Free,
    /// A live claim holds it until `expires_at_ms` unless it is renewed or released.
    Held,
    /// Nobody holds it yet; better-ranked work was given it in this pass.
    Taken,
    /// Its claims could not be read and the backend fails closed.
    Unreadable,
    /// Its claims could not be read and the backend's offline policy lets work
    /// start on a provisional claim.
    Unverified,
    /// The machine that hosts the target was not read in this pass.
    NotRead,
}

impl ResourceState {
    pub fn code(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Held => "held",
            Self::Taken => "taken",
            Self::Unreadable => "unreadable",
            Self::Unverified => "unverified",
            Self::NotRead => "not_read",
        }
    }
}

/// One resource that deferred work in a pass, or that work was started on without
/// its claims being readable. Identifiers and a time only: never a holder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResourceContention {
    /// The machine whose claim backend holds the resource; absent for `not_read`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    pub namespace: String,
    pub resource: String,
    pub state: ResourceState,
    /// When the live claim lapses unless it is renewed; only for `held`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<UnreadableReason>,
    /// The step this pass gave the resource to.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub taken_by: Option<String>,
    /// Steps this pass deferred for it, best first.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub waiting: Vec<String>,
}

/// One required resource as it stands for one step.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResourceStanding {
    pub namespace: String,
    pub resource: String,
    pub state: ResourceState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expires_at_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unreadable: Option<UnreadableReason>,
    /// The better-ranked step that was given it in this pass.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub taken_by: Option<String>,
}

/// Collects contention while a pass packs its work.
#[derive(Default)]
pub(super) struct ContentionLog {
    entries: BTreeMap<(Option<String>, String, String), ResourceContention>,
}

impl ContentionLog {
    pub(super) fn entry(
        &mut self,
        machine: Option<&str>,
        need: &ResourceNeed,
        state: ResourceState,
    ) -> &mut ResourceContention {
        self.entries
            .entry((
                machine.map(Into::into),
                need.namespace.clone(),
                need.resource.clone(),
            ))
            .or_insert_with(|| ResourceContention {
                server_id: machine.map(Into::into),
                namespace: need.namespace.clone(),
                resource: need.resource.clone(),
                state,
                expires_at_ms: None,
                unreadable: None,
                taken_by: None,
                waiting: Vec::new(),
            })
    }

    pub(super) fn finish(self) -> Vec<ResourceContention> {
        self.entries.into_values().collect()
    }
}

/// How each of a step's resources stands in a pass's contention list. `evaluated`
/// is false for work the pass ranked without looking at its resources.
pub fn resource_standings(
    contention: &[ResourceContention],
    step_id: &str,
    needs: &[ResourceNeed],
    evaluated: bool,
) -> Vec<ResourceStanding> {
    needs
        .iter()
        .map(|need| {
            let entry = contention.iter().find(|entry| {
                entry.namespace == need.namespace
                    && entry.resource == need.resource
                    && (entry.taken_by.as_deref() == Some(step_id)
                        || entry.waiting.iter().any(|waiting| waiting == step_id))
            });
            let mut standing = ResourceStanding {
                namespace: need.namespace.clone(),
                resource: need.resource.clone(),
                state: if evaluated {
                    ResourceState::Free
                } else {
                    ResourceState::NotRead
                },
                server_id: None,
                expires_at_ms: None,
                unreadable: None,
                taken_by: None,
            };
            let Some(entry) = entry else {
                return standing;
            };
            standing.server_id.clone_from(&entry.server_id);
            if entry.taken_by.as_deref() == Some(step_id) {
                // This step was given it: free, or unverified when nothing could be read.
                if entry.state == ResourceState::Unverified {
                    standing.state = ResourceState::Unverified;
                    standing.unreadable = entry.unreadable;
                }
                return standing;
            }
            standing.state = match entry.state {
                // Another step holds the provisional start: for this one it is taken.
                ResourceState::Unverified => ResourceState::Taken,
                state => state,
            };
            standing.expires_at_ms = entry.expires_at_ms;
            standing.unreadable = entry.unreadable;
            standing.taken_by.clone_from(&entry.taken_by);
            standing
        })
        .collect()
}
