//! Privacy-preserving milestone distillation for the local activity stream.
//!
//! This module deliberately separates three concerns:
//! - [`DistillationPipeline`] deterministically selects bounded event windows;
//! - [`LocalActivitySummarizer`] sees only [`SafeActivityEvent`] projections;
//! - [`DistilledActivityNote`] carries a provenance-derived note id, making provider
//!   writes idempotent across retries and collaborating clients.
//!
//! Nothing runs unless the machine-local [`DistillationPolicy`] is enabled. In
//! particular, shared project settings cannot opt another collaborator into sending
//! activity through a summarizer.

use std::collections::{HashMap, HashSet, VecDeque};

use hotsheet_model::{NoteKind, Timestamp, Ulid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::activity::{ActivityEvent, ActivityKind, Importance};
use crate::provider::{MutationContext, ProviderError, TicketProvider};
use crate::settings::{Scope, Settings, SettingsError};
use crate::wire::ApiTicket;

/// Machine-local settings key. The value is intentionally one object so consent,
/// adapter selection, and fallback behavior are reviewed together.
pub const DISTILLATION_SETTINGS_KEY: &str = "activity_distillation";

fn default_sequence_threshold() -> usize {
    3
}

fn default_max_window_events() -> usize {
    64
}

fn default_adapter() -> String {
    "deterministic".into()
}

fn default_max_seen_event_ids() -> usize {
    2_048
}

/// Explicit local policy. `enabled` defaults to false even when the object exists.
/// `adapter` names a client-owned implementation (for example
/// `apple_foundation_models`); it is not interpreted or loaded by the server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillationPolicy {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_adapter")]
    pub adapter: String,
    #[serde(default)]
    pub deterministic_fallback: bool,
    #[serde(default = "default_sequence_threshold")]
    pub substantial_sequence_events: usize,
    #[serde(default = "default_max_window_events")]
    pub max_window_events: usize,
    #[serde(default = "default_max_seen_event_ids")]
    pub max_seen_event_ids: usize,
}

impl Default for DistillationPolicy {
    fn default() -> Self {
        Self {
            enabled: false,
            adapter: default_adapter(),
            deterministic_fallback: false,
            substantial_sequence_events: default_sequence_threshold(),
            max_window_events: default_max_window_events(),
            max_seen_event_ids: default_max_seen_event_ids(),
        }
    }
}

impl DistillationPolicy {
    /// Read consent from the local scope only. A committed/shared setting with the same
    /// key is intentionally ignored so a project cannot opt in every collaborator.
    pub fn from_local_settings(settings: &Settings) -> Result<Self, SettingsError> {
        let Some(value) = settings.get(DISTILLATION_SETTINGS_KEY, Scope::Local)? else {
            return Ok(Self::default());
        };
        serde_json::from_value(value).map_err(|source| SettingsError::Invalid {
            key: DISTILLATION_SETTINGS_KEY.into(),
            source,
        })
    }

    fn normalized(&self) -> Self {
        let mut policy = self.clone();
        policy.substantial_sequence_events = policy.substantial_sequence_events.max(1);
        policy.max_window_events = policy.max_window_events.max(1);
        policy.max_seen_event_ids = policy.max_seen_event_ids.max(1);
        policy
    }
}

/// Why a bounded window became a candidate. Closed values keep adapters portable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateTrigger {
    TurnEnd,
    Decision,
    PlanChange,
    Blocked,
    Unblocked,
    SubstantialSequence,
    TicketStatus,
}

/// The only event representation exposed to a summarizer. Raw `summary` and `detail`
/// are absent by construction. `status` is populated only from a closed allow-list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafeActivityEvent {
    pub id: String,
    pub tool: String,
    pub kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
}

/// Stable identity for one ticket/session/event window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityWindowProvenance {
    pub version: u8,
    pub ticket: String,
    pub session: String,
    pub first_event_id: String,
    pub last_event_id: String,
    pub event_count: usize,
    pub digest: String,
}

/// Provider-neutral input for a local summarizer. This type is suitable for an Apple
/// Foundation Models client adapter, another on-device model, or a deterministic adapter.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DistillationRequest {
    pub trigger: CandidateTrigger,
    pub provenance: ActivityWindowProvenance,
    pub events: Vec<SafeActivityEvent>,
}

/// A local adapter may create a concise note (`Some`), suppress the candidate (`None`),
/// or fail. Failures use deterministic fallback only when local policy permits it.
pub trait LocalActivitySummarizer: Send + Sync {
    fn summarize(&self, request: &DistillationRequest) -> Result<Option<String>, String>;
}

/// A no-network baseline and deterministic fallback.
#[derive(Debug, Default, Clone, Copy)]
pub struct DeterministicActivitySummarizer;

impl LocalActivitySummarizer for DeterministicActivitySummarizer {
    fn summarize(&self, request: &DistillationRequest) -> Result<Option<String>, String> {
        Ok(Some(deterministic_summary(request)))
    }
}

/// A candidate's final shared note. `note_id` is stable for the provenance window.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DistilledActivityNote {
    pub note_id: Ulid,
    pub text: String,
    pub provenance: ActivityWindowProvenance,
}

#[derive(Debug, Default)]
struct PendingWindow {
    events: Vec<ActivityEvent>,
}

/// Bounded, deterministic candidate selection keyed by ticket + session.
#[derive(Debug, Default)]
pub struct DistillationPipeline {
    windows: HashMap<(String, String), PendingWindow>,
    seen: HashSet<(String, String, String)>,
    seen_order: VecDeque<(String, String, String)>,
}

impl DistillationPipeline {
    /// Forget every in-memory candidate window. Hosts call this immediately on opt-out
    /// or when handing ownership to a different client adapter.
    pub fn clear(&mut self) {
        self.windows.clear();
        self.seen.clear();
        self.seen_order.clear();
    }

    /// Observe one normalized event. Events without both ticket and session provenance
    /// cannot safely produce a shared, idempotent note and are ignored.
    pub fn observe(
        &mut self,
        event: &ActivityEvent,
        policy: &DistillationPolicy,
    ) -> Option<DistillationRequest> {
        if !policy.enabled || event.kind == ActivityKind::Note {
            return None;
        }
        let ticket = event.ticket.clone()?;
        let session = event.session.clone()?;
        let policy = policy.normalized();
        let seen_key = (ticket.clone(), session.clone(), event.id.clone());
        if self.seen.contains(&seen_key) {
            return None;
        }
        self.seen.insert(seen_key.clone());
        self.seen_order.push_back(seen_key);
        while self.seen_order.len() > policy.max_seen_event_ids {
            if let Some(expired) = self.seen_order.pop_front() {
                self.seen.remove(&expired);
            }
        }
        let window = self.windows.entry((ticket, session)).or_default();

        window.events.push(event.clone());
        if window.events.len() > policy.max_window_events {
            let excess = window.events.len() - policy.max_window_events;
            window.events.drain(..excess);
        }

        let trigger = candidate_trigger(&window.events, &policy)?;
        let events = std::mem::take(&mut window.events);
        Some(build_request(trigger, &events))
    }
}

/// Run a redacted request through an adapter and finalize an idempotent note.
pub fn distill(
    request: &DistillationRequest,
    policy: &DistillationPolicy,
    summarizer: &dyn LocalActivitySummarizer,
) -> Option<DistilledActivityNote> {
    if !policy.enabled {
        return None;
    }
    let summary = match summarizer.summarize(request) {
        Ok(Some(summary)) => clean_summary(&summary),
        Ok(None) => return None,
        Err(_) if policy.deterministic_fallback => deterministic_summary(request),
        Err(_) => return None,
    };
    if summary.is_empty() {
        return None;
    }
    let note_id = provenance_note_id(&request.provenance);
    let marker = format!(
        "<!-- hotsheet:activity-distillation:v1:{} -->",
        request.provenance.digest
    );
    Some(DistilledActivityNote {
        note_id,
        text: format!("{summary}\n\n{marker}"),
        provenance: request.provenance.clone(),
    })
}

/// Append a distilled note through the ordinary provider boundary. Provider adapters
/// receive the provenance-derived id as `generated_id`; adapters that cannot choose a
/// remote note id should treat it (or the embedded marker) as their idempotency key.
/// A read-before-write avoids a redundant mutation on ordinary retries, while the stable
/// id remains authoritative for races and semantic git merges.
pub fn write_distilled_note(
    provider: &dyn TicketProvider,
    native_id: &str,
    now: Timestamp,
    note: &DistilledActivityNote,
) -> Result<ApiTicket, ProviderError> {
    let current = provider.get(native_id)?;
    let marker = format!(
        "hotsheet:activity-distillation:v1:{}",
        note.provenance.digest
    );
    if current
        .notes
        .iter()
        .any(|existing| existing.id == note.note_id.to_string() || existing.text.contains(&marker))
    {
        return Ok(current);
    }
    provider.add_note(
        native_id,
        MutationContext {
            now,
            generated_id: note.note_id,
        },
        NoteKind::Activity,
        note.text.clone(),
    )
}

fn candidate_trigger(
    events: &[ActivityEvent],
    policy: &DistillationPolicy,
) -> Option<CandidateTrigger> {
    let event = events.last()?;
    let prior_substance = events[..events.len() - 1].iter().any(is_substantive);
    if event.kind == ActivityKind::Blocked {
        return Some(if is_unblocked(event) {
            CandidateTrigger::Unblocked
        } else {
            CandidateTrigger::Blocked
        });
    }
    if event.kind == ActivityKind::Decision
        && (event.importance == Importance::High || boolean_hint(event, "meaningful"))
    {
        return Some(CandidateTrigger::Decision);
    }
    if event.kind == ActivityKind::Plan
        && (boolean_hint(event, "changed") || boolean_hint(event, "meaningful"))
    {
        return Some(CandidateTrigger::PlanChange);
    }
    let edit_commands = events
        .iter()
        .filter(|item| matches!(item.kind, ActivityKind::Edit | ActivityKind::Command))
        .count();
    if matches!(event.kind, ActivityKind::Edit | ActivityKind::Command)
        && edit_commands >= policy.substantial_sequence_events
    {
        return Some(CandidateTrigger::SubstantialSequence);
    }
    if event.kind == ActivityKind::TicketStatus && prior_substance {
        return Some(CandidateTrigger::TicketStatus);
    }
    if event.kind == ActivityKind::TurnEnd && prior_substance {
        return Some(CandidateTrigger::TurnEnd);
    }
    None
}

fn is_substantive(event: &ActivityEvent) -> bool {
    matches!(
        event.kind,
        ActivityKind::Plan
            | ActivityKind::Edit
            | ActivityKind::Command
            | ActivityKind::ToolCall
            | ActivityKind::Decision
            | ActivityKind::Blocked
    )
}

fn boolean_hint(event: &ActivityEvent, key: &str) -> bool {
    event.detail.get(key).and_then(serde_json::Value::as_bool) == Some(true)
}

fn is_unblocked(event: &ActivityEvent) -> bool {
    event
        .detail
        .get("blocked")
        .and_then(serde_json::Value::as_bool)
        == Some(false)
        || event
            .detail
            .get("state")
            .and_then(serde_json::Value::as_str)
            == Some("unblocked")
        || event
            .detail
            .get("transition")
            .and_then(serde_json::Value::as_str)
            == Some("unblocked")
}

fn build_request(trigger: CandidateTrigger, events: &[ActivityEvent]) -> DistillationRequest {
    let first = &events[0];
    let last = events.last().expect("candidate windows are non-empty");
    let ticket = first.ticket.clone().unwrap_or_default();
    let session = first.session.clone().unwrap_or_default();
    let mut hasher = Sha256::new();
    hasher.update(b"hotsheet-activity-distillation-v1\0");
    for value in [&ticket, &session] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    for event in events {
        hasher.update(event.id.as_bytes());
        hasher.update([0]);
    }
    hasher.update(events.len().to_be_bytes());
    let digest = format!("{:x}", hasher.finalize());
    let provenance = ActivityWindowProvenance {
        version: 1,
        ticket,
        session,
        first_event_id: first.id.clone(),
        last_event_id: last.id.clone(),
        event_count: events.len(),
        digest,
    };
    DistillationRequest {
        trigger,
        provenance,
        events: events.iter().map(sanitize_event).collect(),
    }
}

fn sanitize_event(event: &ActivityEvent) -> SafeActivityEvent {
    SafeActivityEvent {
        id: event.id.clone(),
        tool: safe_tool_name(&event.tool),
        kind: event.kind,
        status: (event.kind == ActivityKind::TicketStatus)
            .then(|| allowlisted_status(&event.detail))
            .flatten(),
    }
}

fn safe_tool_name(value: &str) -> String {
    let safe = !value.is_empty()
        && value.len() <= 40
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'));
    if safe {
        value.to_string()
    } else {
        "ai_tool".into()
    }
}

fn allowlisted_status(detail: &serde_json::Value) -> Option<String> {
    const STATUSES: &[&str] = &[
        "not_started",
        "started",
        "completed",
        "verified",
        "backlog",
        "archive",
        "deleted",
        "moved",
    ];
    ["status", "to"]
        .into_iter()
        .find_map(|key| detail.get(key).and_then(serde_json::Value::as_str))
        .filter(|value| STATUSES.contains(value))
        .map(str::to_string)
}

fn deterministic_summary(request: &DistillationRequest) -> String {
    let mut counts = Vec::new();
    for (kind, label) in [
        (ActivityKind::Edit, "edit"),
        (ActivityKind::Command, "command"),
        (ActivityKind::ToolCall, "tool call"),
        (ActivityKind::Plan, "plan change"),
        (ActivityKind::Decision, "decision"),
    ] {
        let count = request
            .events
            .iter()
            .filter(|event| event.kind == kind)
            .count();
        if count > 0 {
            counts.push(format!(
                "{count} {label}{}",
                if count == 1 { "" } else { "s" }
            ));
        }
    }
    let activity = if counts.is_empty() {
        "a work milestone".into()
    } else {
        counts.join(", ")
    };
    match request.trigger {
        CandidateTrigger::Blocked => format!("Reached a blocker after {activity}."),
        CandidateTrigger::Unblocked => format!("Resumed work after {activity}."),
        CandidateTrigger::TicketStatus => format!("Changed ticket status after {activity}."),
        CandidateTrigger::Decision => format!("Recorded a decision after {activity}."),
        CandidateTrigger::PlanChange => format!("Changed the plan after {activity}."),
        CandidateTrigger::SubstantialSequence => format!("Completed {activity}."),
        CandidateTrigger::TurnEnd => format!("Finished a work turn with {activity}."),
    }
}

fn clean_summary(summary: &str) -> String {
    let one_line = summary.split_whitespace().collect::<Vec<_>>().join(" ");
    one_line
        .replace("<!--", "")
        .replace("-->", "")
        .chars()
        .take(500)
        .collect::<String>()
        .trim()
        .to_string()
}

fn provenance_note_id(provenance: &ActivityWindowProvenance) -> Ulid {
    let digest = Sha256::digest(provenance.digest.as_bytes());
    let timestamp = Ulid::from_string(&provenance.first_event_id)
        .map(|id| id.timestamp_ms())
        .unwrap_or(0);
    let mut randomness = [0_u8; 16];
    randomness[6..].copy_from_slice(&digest[..10]);
    Ulid::from_parts(timestamp, u128::from_be_bytes(randomness))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{FsStore, GitProvider, NewTicket, StoreMetadata, ops};
    use serde_json::json;

    fn event(id: &str, kind: ActivityKind, detail: serde_json::Value) -> ActivityEvent {
        let mut event = ActivityEvent::new(id, "2026-09-02T01:00:00Z", "codex", kind, detail);
        event.ticket = Some("ticket-1".into());
        event.session = Some("session-1".into());
        event
    }

    fn enabled() -> DistillationPolicy {
        DistillationPolicy {
            enabled: true,
            ..Default::default()
        }
    }

    #[test]
    fn opt_out_and_noise_do_not_create_candidates() {
        let mut pipeline = DistillationPipeline::default();
        let edit = event("01ARZ3NDEKTSV4RRFFQ69G5FAV", ActivityKind::Edit, json!({}));
        assert!(
            pipeline
                .observe(&edit, &DistillationPolicy::default())
                .is_none()
        );

        let mut pipeline = DistillationPipeline::default();
        let policy = enabled();
        assert!(
            pipeline
                .observe(&event("a", ActivityKind::TurnStart, json!({})), &policy)
                .is_none()
        );
        assert!(
            pipeline
                .observe(&event("b", ActivityKind::TurnEnd, json!({})), &policy)
                .is_none()
        );
        assert!(
            pipeline
                .observe(
                    &event("c", ActivityKind::TicketStatus, json!({"to":"started"})),
                    &policy
                )
                .is_none()
        );
    }

    #[test]
    fn deterministic_triggers_cover_milestones() {
        let policy = enabled();

        let mut sequence = DistillationPipeline::default();
        assert!(
            sequence
                .observe(&event("a", ActivityKind::Edit, json!({})), &policy)
                .is_none()
        );
        assert!(
            sequence
                .observe(&event("b", ActivityKind::Command, json!({})), &policy)
                .is_none()
        );
        assert_eq!(
            sequence
                .observe(&event("c", ActivityKind::Edit, json!({})), &policy)
                .unwrap()
                .trigger,
            CandidateTrigger::SubstantialSequence
        );

        let mut decision = DistillationPipeline::default();
        assert_eq!(
            decision
                .observe(
                    &event("d", ActivityKind::Decision, json!({"text":"secret"})),
                    &policy
                )
                .unwrap()
                .trigger,
            CandidateTrigger::Decision
        );

        let mut plan = DistillationPipeline::default();
        assert!(
            plan.observe(
                &event("e", ActivityKind::Plan, json!({"text":"noise"})),
                &policy
            )
            .is_none()
        );
        assert_eq!(
            plan.observe(
                &event("f", ActivityKind::Plan, json!({"changed":true})),
                &policy
            )
            .unwrap()
            .trigger,
            CandidateTrigger::PlanChange
        );

        let mut blocked = DistillationPipeline::default();
        assert_eq!(
            blocked
                .observe(&event("g", ActivityKind::Blocked, json!({})), &policy)
                .unwrap()
                .trigger,
            CandidateTrigger::Blocked
        );
        assert_eq!(
            blocked
                .observe(
                    &event("h", ActivityKind::Blocked, json!({"blocked":false})),
                    &policy
                )
                .unwrap()
                .trigger,
            CandidateTrigger::Unblocked
        );

        let mut turn = DistillationPipeline::default();
        assert!(
            turn.observe(&event("i", ActivityKind::ToolCall, json!({})), &policy)
                .is_none()
        );
        assert_eq!(
            turn.observe(&event("j", ActivityKind::TurnEnd, json!({})), &policy)
                .unwrap()
                .trigger,
            CandidateTrigger::TurnEnd
        );

        let mut status = DistillationPipeline::default();
        assert!(
            status
                .observe(&event("k", ActivityKind::ToolCall, json!({})), &policy)
                .is_none()
        );
        assert_eq!(
            status
                .observe(
                    &event("l", ActivityKind::TicketStatus, json!({"to":"completed"}),),
                    &policy,
                )
                .unwrap()
                .trigger,
            CandidateTrigger::TicketStatus
        );
    }

    #[test]
    fn summarizer_input_excludes_sensitive_raw_fields() {
        let policy = enabled();
        let mut pipeline = DistillationPipeline::default();
        let mut private = event(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            ActivityKind::Decision,
            json!({
                "prompt":"password=hunter2",
                "command":"deploy --token secret-token",
                "output":"private command output",
                "path":"/Users/private/customer.rs",
                "contents":"source text"
            }),
        );
        private.summary = "send customer secret to production".into();
        private.tool = "codex; secret-token".into();
        let request = pipeline.observe(&private, &policy).unwrap();
        let encoded = serde_json::to_string(&request).unwrap();
        for secret in [
            "hunter2",
            "secret-token",
            "private command output",
            "customer.rs",
            "source text",
            "send customer",
        ] {
            assert!(!encoded.contains(secret), "leaked {secret}: {encoded}");
        }
        assert_eq!(request.events[0].tool, "ai_tool");
    }

    #[test]
    fn provenance_and_note_id_are_stable_across_clients_and_retries() {
        let policy = enabled();
        let source = event(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            ActivityKind::Decision,
            json!({}),
        );
        let request_a = DistillationPipeline::default()
            .observe(&source, &policy)
            .unwrap();
        let request_b = DistillationPipeline::default()
            .observe(&source, &policy)
            .unwrap();
        let a = distill(&request_a, &policy, &DeterministicActivitySummarizer).unwrap();
        let b = distill(&request_b, &policy, &DeterministicActivitySummarizer).unwrap();
        assert_eq!(a, b);
        assert!(a.text.contains(&request_a.provenance.digest));
    }

    #[test]
    fn provenance_digest_covers_every_event_in_the_window() {
        let policy = enabled();
        let request = |middle: &str| {
            let mut pipeline = DistillationPipeline::default();
            pipeline.observe(&event("first", ActivityKind::ToolCall, json!({})), &policy);
            pipeline.observe(&event(middle, ActivityKind::ToolCall, json!({})), &policy);
            pipeline
                .observe(&event("last", ActivityKind::TurnEnd, json!({})), &policy)
                .unwrap()
        };
        assert_ne!(
            request("middle-a").provenance.digest,
            request("middle-b").provenance.digest
        );
    }

    struct Failing;
    impl LocalActivitySummarizer for Failing {
        fn summarize(&self, _: &DistillationRequest) -> Result<Option<String>, String> {
            Err("model unavailable".into())
        }
    }

    struct Suppressing;
    impl LocalActivitySummarizer for Suppressing {
        fn summarize(&self, _: &DistillationRequest) -> Result<Option<String>, String> {
            Ok(None)
        }
    }

    #[test]
    fn failure_uses_deterministic_fallback_but_explicit_suppression_does_not() {
        let mut policy = enabled();
        policy.deterministic_fallback = true;
        let request = DistillationPipeline::default()
            .observe(&event("a", ActivityKind::Blocked, json!({})), &policy)
            .unwrap();
        let note = distill(&request, &policy, &Failing).unwrap();
        assert_eq!(
            note.text.lines().next(),
            Some("Reached a blocker after a work milestone.")
        );
        assert!(distill(&request, &policy, &Suppressing).is_none());
    }

    #[test]
    fn local_policy_cannot_be_enabled_by_shared_settings() {
        let root = tempfile::tempdir().unwrap();
        let settings = Settings::new(root.path());
        settings
            .set(
                DISTILLATION_SETTINGS_KEY,
                json!({"enabled":true}),
                Scope::Shared,
            )
            .unwrap();
        assert!(
            !DistillationPolicy::from_local_settings(&settings)
                .unwrap()
                .enabled
        );
        settings
            .set(
                DISTILLATION_SETTINGS_KEY,
                json!({"enabled":true,"adapter":"apple_foundation_models"}),
                Scope::Local,
            )
            .unwrap();
        let policy = DistillationPolicy::from_local_settings(&settings).unwrap();
        assert!(policy.enabled);
        assert_eq!(policy.adapter, "apple_foundation_models");
        assert!(!policy.deterministic_fallback);
    }

    #[test]
    fn windows_are_bounded_and_duplicate_events_do_not_inflate_sequences() {
        let mut policy = enabled();
        policy.max_window_events = 2;
        policy.substantial_sequence_events = 3;
        let mut pipeline = DistillationPipeline::default();
        let a = event("a", ActivityKind::Edit, json!({}));
        assert!(pipeline.observe(&a, &policy).is_none());
        assert!(pipeline.observe(&a, &policy).is_none());
        assert!(
            pipeline
                .observe(&event("b", ActivityKind::Edit, json!({})), &policy)
                .is_none()
        );
        assert!(
            pipeline
                .observe(&event("c", ActivityKind::Edit, json!({})), &policy)
                .is_none()
        );
    }

    #[test]
    fn clearing_on_opt_out_discards_the_prior_private_window() {
        let mut policy = enabled();
        policy.substantial_sequence_events = 2;
        let mut pipeline = DistillationPipeline::default();
        assert!(
            pipeline
                .observe(&event("a", ActivityKind::Edit, json!({})), &policy)
                .is_none()
        );
        pipeline.clear();
        assert!(
            pipeline
                .observe(&event("b", ActivityKind::Edit, json!({})), &policy)
                .is_none()
        );
    }

    #[test]
    fn completed_window_replay_does_not_seed_the_next_window() {
        let policy = enabled();
        let mut pipeline = DistillationPipeline::default();
        for id in ["a", "b"] {
            assert!(
                pipeline
                    .observe(&event(id, ActivityKind::Edit, json!({})), &policy)
                    .is_none()
            );
        }
        assert!(
            pipeline
                .observe(&event("c", ActivityKind::Edit, json!({})), &policy)
                .is_some()
        );
        assert!(
            pipeline
                .observe(&event("c", ActivityKind::Edit, json!({})), &policy)
                .is_none()
        );
        for id in ["d", "e"] {
            assert!(
                pipeline
                    .observe(&event(id, ActivityKind::Edit, json!({})), &policy)
                    .is_none()
            );
        }
        assert!(
            pipeline
                .observe(&event("f", ActivityKind::Edit, json!({})), &policy)
                .is_some()
        );
    }

    #[test]
    fn completed_event_dedupe_memory_is_bounded() {
        let mut policy = enabled();
        policy.max_seen_event_ids = 3;
        policy.substantial_sequence_events = usize::MAX;
        let mut pipeline = DistillationPipeline::default();
        for id in ["a", "b", "c", "d", "e"] {
            assert!(
                pipeline
                    .observe(&event(id, ActivityKind::Edit, json!({})), &policy)
                    .is_none()
            );
        }
        assert_eq!(pipeline.seen.len(), 3);
        assert_eq!(pipeline.seen_order.len(), 3);
        assert!(
            !pipeline
                .seen
                .iter()
                .any(|(_, _, id)| id == "a" || id == "b")
        );
    }

    #[test]
    fn provider_write_is_idempotent_for_retries() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let policy = enabled();
        let request = DistillationPipeline::default()
            .observe(
                &event(
                    "01ARZ3NDEKTSV4RRFFQ69G5FAV",
                    ActivityKind::Decision,
                    json!({}),
                ),
                &policy,
            )
            .unwrap();
        let note = distill(&request, &policy, &DeterministicActivitySummarizer).unwrap();
        let provider = GitProvider::new("git-test", store);
        let first = write_distilled_note(
            &provider,
            &ticket_id.to_string(),
            Timestamp::new("2026-09-02T01:00:00Z"),
            &note,
        )
        .unwrap();
        let retry = write_distilled_note(
            &provider,
            &ticket_id.to_string(),
            Timestamp::new("2026-09-02T01:01:00Z"),
            &note,
        )
        .unwrap();
        assert_eq!(first.notes.len(), 1);
        assert_eq!(retry.notes.len(), 1);
        assert_eq!(retry.notes[0].id, note.note_id.to_string());
    }
}
