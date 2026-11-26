//! HS2-T2KS8G: which orchestration types accept fields a newer peer adds.
//!
//! - An answer read from a peer accepts unknown fields, so an additive field of a newer
//!   host never breaks an older orchestrator's read of the whole answer.
//! - A request a host reads refuses them: ignoring a field it does not know could
//!   change what the request means.
//! - A machine-local or committed file refuses them, and so does a type that is also
//!   part of a request or such a file (a policy limit or a validation profile an older
//!   binary silently ignored would loosen what it enforces).
use hotsheet_ticketing::orchestration::{
    Coverage, ExecutionEnd, ExecutionOutcome, GateEvidence, GateState, ProfileCheck,
    ProfileProblem, StreamRef, TargetSpeculation,
};
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use super::*;
use crate::orchestration::HostedGate;

const CLI_SESSIONS: &str = include_str!("../sessions.rs");
const CLI_WATCH: &str = include_str!("../watch.rs");
const CLI_VALIDATION: &str = include_str!("../validation.rs");
const CLI_HOSTED_GATES: &str = include_str!("../hosted_gates.rs");
const CLI_RUNS: &str = include_str!("../runs.rs");
const CLI_TICKETS: &str = include_str!("../tickets.rs");
const CORE_CAPACITY: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/capacity.rs");
const CORE_VALIDATION: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/validation.rs");
const CORE_SESSIONS: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/sessions.rs");
const CORE_GATES: &str = include_str!("../../../../hotsheet-ticketing/src/orchestration/gates.rs");
const CORE_POLICY: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/policy.rs");
const CORE_SCHEDULER: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/scheduler.rs");
const CORE_CONTRACT: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/contract.rs");
const CORE_SITES: &str =
    include_str!("../../../../hotsheet-ticketing/src/orchestration/contract/sites.rs");

/// Answers read from a peer: they accept unknown fields.
const ANSWERS: &[(&str, &str, &str)] = &[
    ("watch.rs", CLI_WATCH, "TargetEventPage"),
    ("watch.rs", CLI_WATCH, "TargetTicketEvent"),
    ("watch.rs", CLI_WATCH, "TargetSessionEvent"),
    ("watch.rs", CLI_WATCH, "HostedSpeculation"),
    ("validation.rs", CLI_VALIDATION, "HostedValidation"),
    ("hosted_gates.rs", CLI_HOSTED_GATES, "HostedGate"),
    ("core capacity.rs", CORE_CAPACITY, "TargetSpeculation"),
    ("core validation.rs", CORE_VALIDATION, "ExecutionOutcome"),
    ("core validation.rs", CORE_VALIDATION, "StreamRef"),
    ("core validation.rs", CORE_VALIDATION, "ProfileCheck"),
    ("core validation.rs", CORE_VALIDATION, "ProfileProblem"),
    ("core sessions.rs", CORE_SESSIONS, "HostedSession"),
    ("core sessions.rs", CORE_SESSIONS, "HostedRequest"),
    ("core sessions.rs", CORE_SESSIONS, "SafePoint"),
    ("core gates.rs", CORE_GATES, "GateEvidence"),
    // HS2-SHH7WW: the sites view.
    ("core sites.rs", CORE_SITES, "SitesView"),
    ("core sites.rs", CORE_SITES, "SiteProject"),
    ("core sites.rs", CORE_SITES, "SiteView"),
];

/// Requests a host reads, files, and types shared with either: they refuse them.
const REFUSING: &[(&str, &str, &str)] = &[
    ("sessions.rs", CLI_SESSIONS, "HostedSessionStart"),
    ("sessions.rs", CLI_SESSIONS, "HostedSessionRequest"),
    ("sessions.rs", CLI_SESSIONS, "HostedSessionStop"),
    ("sessions.rs", CLI_SESSIONS, "HostedSessionControl"),
    ("runs.rs", CLI_RUNS, "HostedClaim"),
    ("validation.rs", CLI_VALIDATION, "HostedValidationStart"),
    ("validation.rs", CLI_VALIDATION, "HostedValidationCancel"),
    ("tickets.rs", CLI_TICKETS, "HostedCreate"),
    ("tickets.rs", CLI_TICKETS, "HostedUpdate"),
    ("hosted_gates.rs", CLI_HOSTED_GATES, "HostedGateWatch"),
    ("core capacity.rs", CORE_CAPACITY, "AdmissionRequest"),
    ("core sessions.rs", CORE_SESSIONS, "NewHostedSession"),
    // Shared with a request or a file: the stricter rule wins.
    ("core sessions.rs", CORE_SESSIONS, "SessionOrigin"),
    ("core policy.rs", CORE_POLICY, "MachineLimits"),
    ("core scheduler.rs", CORE_SCHEDULER, "ResourceNeed"),
    ("core validation.rs", CORE_VALIDATION, "ValidationProfile"),
    // Machine-local and committed files.
    ("core policy.rs", CORE_POLICY, "PolicyBody"),
    ("core contract.rs", CORE_CONTRACT, "TargetManifest"),
    // HS2-SHH7WW: `site_of` is new in a declaration and in the add and rename
    // requests; an older host refuses it rather than add a target as its own project.
    ("core contract.rs", CORE_CONTRACT, "TargetDeclaration"),
    ("core contract.rs", CORE_CONTRACT, "AddTarget"),
    ("core contract.rs", CORE_CONTRACT, "RenameTarget"),
    ("core validation.rs", CORE_VALIDATION, "ValidationStep"),
    ("core validation.rs", CORE_VALIDATION, "StepDefinition"),
];

/// Whether the attributes directly above `pub struct name` refuse unknown fields.
fn refuses(source: &str, name: &str) -> Option<bool> {
    let header = format!("pub struct {name} ");
    let alone = format!("pub struct {name}\n");
    let at = source.find(&header).or_else(|| source.find(&alone))?;
    let above: Vec<&str> = source[..at]
        .lines()
        .rev()
        .take_while(|line| {
            let line = line.trim();
            line.starts_with("#[") || line.starts_with("//")
        })
        .collect();
    Some(
        above
            .iter()
            .any(|line| line.contains("deny_unknown_fields")),
    )
}

#[test]
fn hs2_t2ks8g_answers_accept_and_requests_and_files_refuse_unknown_fields() {
    for (file, source, name) in ANSWERS {
        assert_eq!(
            refuses(source, name),
            Some(false),
            "{name} in {file} is an answer read from a peer: it must accept unknown fields"
        );
    }
    for (file, source, name) in REFUSING {
        assert_eq!(
            refuses(source, name),
            Some(true),
            "{name} in {file} is a request, a file or shared with one: it must refuse unknown fields"
        );
    }
}

/// Add a field a newer peer might send to every object of `value`.
fn newer(mut value: Value) -> Value {
    fn walk(value: &mut Value) {
        match value {
            Value::Object(map) => {
                for child in map.values_mut() {
                    walk(child);
                }
                map.insert("added_by_a_newer_peer".into(), json!({"any": [1, 2]}));
            }
            Value::Array(items) => items.iter_mut().for_each(walk),
            _ => {}
        }
    }
    walk(&mut value);
    value
}

/// This build, as the older reader, parses what a newer peer sent: the answer it
/// knows, with the newer fields ignored.
fn reads_newer<T: Serialize + DeserializeOwned>(answer: T) {
    let known = serde_json::to_value(&answer).unwrap();
    let read: T = serde_json::from_value(newer(known.clone()))
        .expect("an older reader parses a newer answer");
    assert_eq!(serde_json::to_value(read).unwrap(), known);
}

#[test]
fn hs2_t2ks8g_an_older_reader_parses_a_newer_answer_with_an_extra_field() {
    let speculation = TargetSpeculation {
        enabled: true,
        max_depth: 2,
        integration: hotsheet_ticketing::provisional::IntegrationRule::Parked,
    };
    reads_newer(TargetEventPage {
        cursor: 7,
        events: vec![TargetTicketEvent {
            cursor: 6,
            connection_id: "tickets".into(),
            native_id: "TGT-1".into(),
        }],
        sessions: vec![TargetSessionEvent {
            cursor: 5,
            session_id: "terminal-1".into(),
        }],
        overflow: false,
        speculation: Some(HostedSpeculation {
            speculation: Some(speculation),
        }),
        remote_refs_changed: true,
        pool_changed: true,
    });
    let digest = "a".repeat(64);
    reads_newer(HostedValidation {
        execution_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        state: HostedExecutionState::Finished,
        outcome: Some(ExecutionOutcome {
            end: ExecutionEnd::Exited,
            exit_code: Some(0),
            signal: None,
            started_at_ms: 1,
            duration_ms: 2,
            stdout: Some(StreamRef {
                sha256: digest.clone(),
                bytes: 2,
            }),
            stderr: None,
        }),
    });
    reads_newer(HostedGate {
        state: GateState::Passed,
        reason: None,
        evidence: Some(GateEvidence {
            provider: "github_actions".into(),
            reference: "41".into(),
            order: 41,
            attempt: 1,
            commit: "c".repeat(40),
            coverage: Coverage::Exact,
            artifact: None,
            exit_code: None,
            duration_ms: None,
            at_ms: 3,
        }),
        watch_ref: Some("job-1".into()),
        cursor: 9,
        watching: Some(true),
    });
    reads_newer(ProfileCheck {
        present: true,
        valid: false,
        profiles: Vec::new(),
        problems: vec![ProfileProblem {
            code: "duplicate_name".into(),
            index: Some(1),
            name: Some("smoke".into()),
        }],
    });
}

#[test]
fn hs2_t2ks8g_a_host_refuses_a_request_field_it_does_not_know() {
    let request = serde_json::to_value(HostedValidationCancel {
        execution_id: "01ARZ3NDEKTSV4RRFFQ69G5FAV".into(),
        wait_ms: 0,
    })
    .unwrap();
    assert!(serde_json::from_value::<HostedValidationCancel>(request.clone()).is_ok());
    assert!(serde_json::from_value::<HostedValidationCancel>(newer(request)).is_err());
}
