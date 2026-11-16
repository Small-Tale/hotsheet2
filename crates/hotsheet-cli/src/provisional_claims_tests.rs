//! HS2-XYF4YT: the claim answer's provisional prerequisites across the transitions of
//! the policy, the prerequisite and its gate, with the warn-once record.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

use hotsheet_model::{Note, NoteKind, StartedPhase, Status, Timestamp, Ulid};
use hotsheet_ticketing::provisional::{GateResult, SPECULATION_POLICY_SETTING};
use hotsheet_ticketing::{Scope, StoreMetadata, ops};

use super::*;

const GATE: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct Facts {
    gates: RefCell<BTreeMap<String, GateObservation>>,
    reads: Cell<u32>,
}

struct Shared(Rc<Facts>);

impl GateProbe for Shared {
    fn reachable(&self, _commit: &str) -> Option<bool> {
        self.0.reads.set(self.0.reads.get() + 1);
        Some(true)
    }

    fn gate(&self, commit: &str) -> GateObservation {
        self.0.reads.set(self.0.reads.get() + 1);
        self.0
            .gates
            .borrow()
            .get(commit)
            .cloned()
            .unwrap_or_default()
    }
}

struct Fixture {
    _root: tempfile::TempDir,
    context: ProvisionalContext,
    store: FsStore,
    facts: Rc<Facts>,
    prerequisite: Ulid,
}

fn note(text: &str) -> Note {
    Note {
        id: Ulid::new(),
        kind: NoteKind::Regular,
        created_at: Timestamp::new("2026-11-16T00:00:00Z"),
        edited_at: Timestamp::new("2026-11-16T00:00:00Z"),
        summary: None,
        confidence: None,
        feedback_for: None,
        kind_explicit: false,
        feedback_request: false,
        response_to: None,
        ai_feedback: None,
        human_edited: false,
        actor: None,
        text: text.to_owned(),
    }
}

fn now() -> Timestamp {
    Timestamp::new("2026-11-16T12:00:00Z")
}

fn lease() -> Timestamp {
    Timestamp::new("2099-01-01T00:00:00Z")
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let project = root.path().join("project");
        fs::create_dir(&project).unwrap();
        let store = FsStore::init(root.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let mut prerequisite = Ticket {
            id: Ulid::new(),
            slug: "HS-PREREQ".into(),
            title: "Prerequisite".into(),
            status: Status::Started,
            started_phase: Some(StartedPhase::FinalTesting),
            ..Ticket::default()
        };
        prerequisite
            .notes
            .push(note(&format!("Pushed.\n\nCI gate commit: {GATE}\n")));
        store.write_ticket(&prerequisite).unwrap();
        Self {
            context: ProvisionalContext {
                project,
                checkout_id: "project-0000".into(),
                home: root.path().join("home"),
            },
            store,
            facts: Rc::default(),
            prerequisite: prerequisite.id,
            _root: root,
        }
    }

    fn policy(&self, value: Option<serde_json::Value>) {
        let settings = Settings::for_project(&self.context.project);
        match value {
            Some(value) => settings
                .set(SPECULATION_POLICY_SETTING, value, Scope::Shared)
                .unwrap(),
            None => {
                let _ = settings.unset(SPECULATION_POLICY_SETTING, Scope::Shared);
            }
        }
    }

    fn gate(&self, result: GateResult, run: Option<&str>) {
        self.facts.gates.borrow_mut().insert(
            GATE.into(),
            GateObservation {
                result,
                run: run.map(str::to_owned),
            },
        );
    }

    fn dependent(&self, slug: &str, blocked: bool) -> Ulid {
        let ticket = Ticket {
            id: Ulid::new(),
            slug: slug.into(),
            title: slug.into(),
            status: Status::NotStarted,
            blocked_by: if blocked {
                vec![self.prerequisite]
            } else {
                Vec::new()
            },
            ..Ticket::default()
        };
        self.store.write_ticket(&ticket).unwrap();
        ticket.id
    }

    fn claim(&self, id: &Ulid) -> Ticket {
        ops::claim(&self.store, id, &now(), lease(), "agent", None).unwrap()
    }

    fn renew(&self, id: &Ulid) -> Ticket {
        ops::renew_with_eta(&self.store, id, now(), lease(), "agent", None).unwrap()
    }

    fn answer(&self, claimed: &Ticket) -> Option<ClaimProvisional> {
        let facts = self.facts.clone();
        claim_provisional_with(&self.context, &self.store, claimed, &move |_| {
            Box::new(Shared(facts.clone()))
        })
    }

    fn set_prerequisite(&self, update: impl FnOnce(&mut Ticket)) {
        let mut ticket = self.store.read_ticket(&self.prerequisite).unwrap();
        update(&mut ticket);
        self.store.write_ticket(&ticket).unwrap();
    }
}

fn enabled() -> Option<serde_json::Value> {
    Some(serde_json::json!({ "enabled": true }))
}

fn awaiting(answer: &Option<ClaimProvisional>) -> Vec<(String, String, GateResult)> {
    answer
        .as_ref()
        .map(|answer| {
            answer
                .prerequisites
                .iter()
                .map(|base| {
                    (
                        base.slug.clone(),
                        base.base_commit.clone(),
                        base.gate.result,
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

fn red(answer: &Option<ClaimProvisional>) -> Vec<(String, Option<String>)> {
    answer
        .as_ref()
        .map(|answer| {
            answer
                .red_gates
                .iter()
                .map(|red| (red.slug.clone(), red.gate.run.clone()))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn hs2_xyf4yt_policy_off_reports_nothing_and_probes_nothing() {
    let f = Fixture::new();
    f.gate(GateResult::Pending, None);
    let id = f.dependent("HS-DEP", true);
    // No setting, then an explicit disabled policy: the answer is unchanged.
    let claimed = f.claim(&id);
    assert_eq!(f.answer(&claimed), None);
    f.policy(Some(serde_json::json!({ "enabled": false })));
    assert_eq!(f.answer(&f.renew(&id)), None);
    assert_eq!(
        f.facts.reads.get(),
        0,
        "an off policy never probes Git or CI"
    );
    // An unreadable policy is treated as off rather than failing the claim.
    f.policy(Some(serde_json::json!({ "enabled": true, "bogus": 1 })));
    assert_eq!(f.answer(&f.renew(&id)), None);
    assert!(!f.context.home.join(WARNINGS_FILE).exists());
}

#[test]
fn hs2_xyf4yt_enabled_without_prerequisite_reports_nothing() {
    let f = Fixture::new();
    f.policy(enabled());
    f.gate(GateResult::Pending, None);
    let id = f.dependent("HS-FREE", false);
    assert_eq!(f.answer(&f.claim(&id)), None);
    assert_eq!(f.answer(&f.renew(&id)), None);
    assert_eq!(f.facts.reads.get(), 0);
}

#[test]
fn hs2_xyf4yt_transitions_pending_red_quiet_green_closed() {
    let f = Fixture::new();
    f.policy(Some(
        serde_json::json!({ "enabled": true, "integration": "parked" }),
    ));
    let id = f.dependent("HS-DEP", true);

    // Awaiting CI: every claim and renew names the prerequisite, commit and rule.
    f.gate(GateResult::Pending, None);
    let claimed = f.claim(&id);
    let answer = f.answer(&claimed);
    assert_eq!(
        awaiting(&answer),
        vec![("HS-PREREQ".into(), GATE.into(), GateResult::Pending)]
    );
    assert_eq!(
        answer.as_ref().unwrap().integration,
        IntegrationRule::Parked
    );
    assert!(red(&answer).is_empty());
    let lines = answer.as_ref().unwrap().lines();
    assert_eq!(lines.len(), 1);
    assert!(lines[0].contains("Prerequisite HS-PREREQ awaits CI (pending)"));
    assert!(lines[0].contains(GATE));
    assert!(lines[0].contains("parked"));
    assert_eq!(
        awaiting(&f.answer(&f.renew(&id))),
        awaiting(&answer),
        "awaiting CI is reported on every renew, not once"
    );

    // Red: the first renew warns, a repeat renew under the same claim is quiet.
    f.gate(GateResult::Red, Some("41.1"));
    let first = f.answer(&f.renew(&id));
    assert_eq!(red(&first), vec![("HS-PREREQ".into(), Some("41.1".into()))]);
    assert!(awaiting(&first).is_empty());
    assert!(first.as_ref().unwrap().lines()[0].starts_with("Warning:"));
    assert_eq!(f.answer(&f.renew(&id)), None);
    assert_eq!(f.answer(&f.renew(&id)), None);

    // A new red run of the same commit is a new reading: it warns once more.
    f.gate(GateResult::Red, Some("41.2"));
    assert_eq!(
        red(&f.answer(&f.renew(&id))),
        vec![("HS-PREREQ".into(), Some("41.2".into()))]
    );
    assert_eq!(f.answer(&f.renew(&id)), None);

    // A new claim attempt after a release warns again for the same reading.
    ops::release(&f.store, &id, now(), "agent", false).unwrap();
    let reclaimed = f.claim(&id);
    assert_eq!(
        red(&f.answer(&reclaimed)),
        vec![("HS-PREREQ".into(), Some("41.2".into()))]
    );
    assert_eq!(f.answer(&f.renew(&id)), None);

    // Pending again (a rerun): reported as awaiting CI.
    f.gate(GateResult::Pending, None);
    assert_eq!(awaiting(&f.answer(&f.renew(&id))).len(), 1);

    // Green: the prerequisite counts as confirmed and drops out of the answer.
    f.gate(GateResult::Green, Some("42.1"));
    assert_eq!(f.answer(&f.renew(&id)), None);

    // Closed: confirmed by status, whatever the gate reads.
    f.gate(GateResult::Red, Some("43.1"));
    f.set_prerequisite(|ticket| ticket.status = Status::Completed);
    assert_eq!(f.answer(&f.renew(&id)), None);
}

#[test]
fn hs2_xyf4yt_other_blocked_reasons_stay_quiet() {
    let f = Fixture::new();
    f.policy(enabled());
    f.gate(GateResult::Pending, None);
    let id = f.dependent("HS-DEP", true);
    // Not yet in final_testing.
    f.set_prerequisite(|ticket| ticket.started_phase = Some(StartedPhase::Working));
    assert_eq!(f.answer(&f.claim(&id)), None);
    // Superseded and cancelled gates are not red.
    f.set_prerequisite(|ticket| ticket.started_phase = Some(StartedPhase::FinalTesting));
    for result in [GateResult::Superseded, GateResult::Cancelled] {
        f.gate(result, None);
        assert_eq!(f.answer(&f.renew(&id)), None, "{result:?}");
    }
    // An opted-out dependent is never told to build ahead.
    f.gate(GateResult::Pending, None);
    let mut dependent = f.store.read_ticket(&id).unwrap();
    dependent.tags.push("speculate:never".into());
    f.store.write_ticket(&dependent).unwrap();
    assert_eq!(f.answer(&f.renew(&id)), None);
}

#[test]
fn hs2_xyf4yt_interleaved_claims_and_an_emptied_record() {
    let f = Fixture::new();
    f.policy(enabled());
    f.gate(GateResult::Red, Some("7.1"));
    let one = f.dependent("HS-ONE", true);
    let two = f.dependent("HS-TWO", true);
    // Each claim warns once, independently and in either order.
    assert_eq!(red(&f.answer(&f.claim(&one))).len(), 1);
    assert_eq!(red(&f.answer(&f.claim(&two))).len(), 1);
    assert_eq!(f.answer(&f.renew(&one)), None);
    assert_eq!(f.answer(&f.renew(&two)), None);
    // The record emptied (or unreadable): a warning repeats rather than being lost.
    fs::remove_file(f.context.home.join(WARNINGS_FILE)).unwrap();
    assert_eq!(red(&f.answer(&f.renew(&two))).len(), 1);
    assert_eq!(f.answer(&f.renew(&two)), None);
    fs::write(f.context.home.join(WARNINGS_FILE), b"not json").unwrap();
    assert_eq!(red(&f.answer(&f.renew(&one))).len(), 1);
    assert_eq!(f.answer(&f.renew(&one)), None);
}

#[test]
fn hs2_xyf4yt_warn_once_is_bounded_and_ordered() {
    let root = tempfile::tempdir().unwrap();
    let home = root.path();
    let keys: Vec<String> = (0..MAX_WARNINGS + 10).map(|n| format!("k{n}")).collect();
    assert_eq!(warn_once(home, &keys), keys);
    assert!(warn_once(home, &keys[10..]).is_empty());
    // The oldest records were evicted to keep the file bounded.
    assert_eq!(warn_once(home, &keys[..1]), vec!["k0".to_owned()]);
    assert!(warn_once(home, &[]).is_empty());
    // Duplicate keys in one call are reported once, in order.
    assert_eq!(
        warn_once(home, &["x".to_owned(), "y".to_owned()]),
        vec!["x".to_owned(), "y".to_owned()]
    );
}

#[test]
fn hs2_xyf4yt_attach_keeps_an_answer_byte_identical_without_news() {
    let ticket = serde_json::json!({ "id": "01", "slug": "HS-1", "status": "started" });
    let mut unchanged = ticket.clone();
    attach(&mut unchanged, None);
    assert_eq!(
        serde_json::to_string(&unchanged).unwrap(),
        serde_json::to_string(&ticket).unwrap()
    );
    let mut answered = ticket.clone();
    attach(
        &mut answered,
        Some(&ClaimProvisional {
            integration: IntegrationRule::PushPerTicket,
            prerequisites: Vec::new(),
            red_gates: vec![RedGate {
                slug: "HS-P".into(),
                gate_commit: GATE.into(),
                gate: GateObservation::with_run(GateResult::Red, "1.1"),
            }],
        }),
    );
    assert_eq!(
        answered["provisional"],
        serde_json::json!({
            "integration": "push-per-ticket",
            "red_gates": [{ "slug": "HS-P", "gate_commit": GATE, "gate": { "result": "red", "run": "1.1" } }]
        })
    );
}

#[test]
fn hs2_xyf4yt_contexts_resolve_a_checkout_or_nothing() {
    let root = tempfile::tempdir().unwrap();
    let plain = root.path().join("plain");
    fs::create_dir(&plain).unwrap();
    assert_eq!(ProvisionalContext::for_dir(&plain, root.path()), None);
    let project = root.path().join("code");
    fs::create_dir(&project).unwrap();
    hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(&project)
        .args(["init", "-q"])
        .status()
        .unwrap();
    let nested = project.join("sub");
    fs::create_dir(&nested).unwrap();
    let context = ProvisionalContext::for_dir(&nested, root.path()).unwrap();
    assert_eq!(context.project, project.canonicalize().unwrap());
    assert_eq!(
        context.checkout_id,
        checkouts::checkout_id(&project).unwrap()
    );
    // No registered checkout links an arbitrary store.
    assert_eq!(ProvisionalContext::for_store(&plain, root.path()), None);
}
