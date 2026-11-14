//! HS2-GDJXGZ session state machine, ownership, safe points and both registries;
//! synthetic homes only.

use super::*;

struct Clock(std::cell::Cell<u64>);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
const OWNER: &str = "owner-0123456789ab";
const HOSTED: &str = "target-0123456789ab";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn new(operation: &str, tool: &str) -> NewSession {
    NewSession {
        operation_id: operation.into(),
        target_id: id(1),
        role: SessionRole::Worker,
        tool: tool.into(),
        model: Some("model-a".into()),
        effort: Some("high".into()),
        account: None,
        run_id: None,
        replaces: None,
    }
}
fn to(state: SessionState, reason: &str) -> SessionUpdate {
    SessionUpdate {
        to: Some(state),
        reason: Some(reason.into()),
        ..SessionUpdate::default()
    }
}
fn takeover(paused: bool) -> SessionUpdate {
    SessionUpdate {
        owner: Some(SessionOwner::Human),
        paused: Some(paused),
        control: Some(if paused {
            SessionControl::Pause
        } else {
            SessionControl::Takeover
        }),
        reason: Some("person".into()),
        actor_id: Some("maintainer".into()),
        ..SessionUpdate::default()
    }
}
fn session_error(result: Result<impl std::fmt::Debug, OrchestrationError>) -> SessionError {
    match result {
        Err(OrchestrationError::Session(error)) => error,
        other => panic!("expected a session error, got {other:?}"),
    }
}

/// Run `body` with a registry in a fresh home, a settable clock and a counting rng.
fn with_registry(body: impl FnOnce(&SessionRegistry, &Clock, &mut Rng, &std::path::Path)) {
    let home = tempfile::tempdir().unwrap();
    let clock = Clock(std::cell::Cell::new(1_000));
    let mut rng = Rng(100);
    body(
        &SessionRegistry::new(home.path()),
        &clock,
        &mut rng,
        home.path(),
    );
}

#[test]
fn hs2_gdjxgz_every_state_edge_is_allowed_or_refused_exactly() {
    use SessionState::{Failed, Lost, Requested, Running, Stopped};
    let allowed = [
        (Requested, Running),
        (Requested, Stopped),
        (Requested, Failed),
        (Running, Stopped),
        (Running, Lost),
    ];
    for from in SessionState::ALL {
        for next in SessionState::ALL {
            assert_eq!(
                from.may_become(next),
                allowed.contains(&(from, next)),
                "{from:?} -> {next:?}"
            );
        }
        assert_eq!(from.is_active(), matches!(from, Requested | Running));
        assert_eq!(from.is_terminal(), !from.is_active());
    }
    // The registry enforces the same matrix from every reachable state.
    for from in SessionState::ALL {
        for next in SessionState::ALL {
            with_registry(|registry, clock, rng, _| {
                let mut tx = Transaction { clock, rng };
                let (session, _) = registry
                    .begin(OWNER, new("op-1", "codex"), None, &mut tx)
                    .unwrap();
                // Reach `from` by its shortest legal path.
                let path: &[SessionState] = match from {
                    Requested => &[],
                    Running => &[Running],
                    Stopped => &[Stopped],
                    Failed => &[Failed],
                    Lost => &[Running, Lost],
                };
                for step in path {
                    registry
                        .update(OWNER, &session.session_id, to(*step, "setup"), &mut tx)
                        .unwrap();
                }
                let result = registry.update(OWNER, &session.session_id, to(next, "edge"), &mut tx);
                if from == next && from.is_active() {
                    // The same state is not a transition and writes nothing.
                    assert_eq!(result.unwrap().history.len(), path.len() + 1);
                } else if from.is_terminal() {
                    assert_eq!(session_error(result), SessionError::NotActive);
                } else if from.may_become(next) {
                    let updated = result.unwrap();
                    assert_eq!(updated.state, next);
                    assert_eq!(updated.history.last().unwrap().from, Some(from));
                } else {
                    assert_eq!(session_error(result), SessionError::InvalidTransition);
                }
            });
        }
    }
}

#[test]
fn hs2_gdjxgz_a_repeated_operation_id_is_the_same_session_and_never_a_second() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let (first, created) = registry
            .begin(
                OWNER,
                new("op-1", "codex"),
                Some("scheduler".into()),
                &mut tx,
            )
            .unwrap();
        assert!(created);
        assert_eq!(first.state, SessionState::Requested);
        assert_eq!(first.owner, SessionOwner::Orchestrator);
        assert_eq!(
            first.history[0].actor_id.as_deref(),
            Some("scheduler"),
            "the first entry records who asked"
        );
        // Repeated, whoever asks and however often.
        for _ in 0..3 {
            let (again, created) = registry
                .begin(OWNER, new("op-1", "codex"), Some("other".into()), &mut tx)
                .unwrap();
            assert!(!created);
            assert_eq!(again, first);
        }
        assert_eq!(registry.list(OWNER, false).unwrap().len(), 1);
        // The same id for anything else is a conflict, field by field.
        let different: [fn(&mut NewSession); 8] = [
            |new| new.tool = "claude".into(),
            |new| new.model = None,
            |new| new.effort = Some("low".into()),
            |new| new.role = SessionRole::Integrator,
            |new| new.account = Some("team".into()),
            |new| new.run_id = Some(id(9)),
            |new| new.target_id = id(2),
            |new| new.replaces = Some(id(8)),
        ];
        for change in different {
            let mut other = new("op-1", "codex");
            change(&mut other);
            assert!(matches!(
                registry.begin(OWNER, other, None, &mut tx),
                Err(OrchestrationError::Conflict)
            ));
        }
        // A start that failed keeps answering with the failed session.
        registry
            .update(
                OWNER,
                &first.session_id,
                to(SessionState::Failed, "session_cap_reached"),
                &mut tx,
            )
            .unwrap();
        let (failed, created) = registry
            .begin(OWNER, new("op-1", "codex"), None, &mut tx)
            .unwrap();
        assert!(!created);
        assert_eq!(failed.state, SessionState::Failed);
        // Another owner's identical operation id is its own session.
        let (other_owner, created) = registry
            .begin("other-0123456789ab", new("op-1", "codex"), None, &mut tx)
            .unwrap();
        assert!(created);
        assert_ne!(other_owner.session_id, first.session_id);
        assert!(registry.get(OWNER, &other_owner.session_id).is_err());
    });
}

#[test]
fn hs2_gdjxgz_a_target_has_one_active_session_per_role_and_any_number_of_workers() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let role = |operation: &str, role: SessionRole, target: u128| NewSession {
            role,
            target_id: id(target),
            ..new(operation, "codex")
        };
        for (index, operation) in ["w-1", "w-2", "w-3"].into_iter().enumerate() {
            let (session, created) = registry
                .begin(
                    OWNER,
                    role(operation, SessionRole::Worker, 1),
                    None,
                    &mut tx,
                )
                .unwrap();
            assert!(created, "worker {index}");
            assert_eq!(session.role, SessionRole::Worker);
        }
        for kind in [SessionRole::Integrator, SessionRole::Dispatcher] {
            let first = format!("{}-1", kind.code());
            let (held, _) = registry
                .begin(OWNER, role(&first, kind, 1), None, &mut tx)
                .unwrap();
            assert_eq!(
                session_error(registry.begin(OWNER, role("second", kind, 1), None, &mut tx)),
                SessionError::RoleActive
            );
            // Another target has its own.
            registry
                .begin(
                    OWNER,
                    role(&format!("{}-other", kind.code()), kind, 2),
                    None,
                    &mut tx,
                )
                .unwrap();
            // Empty, then refill: once the first is gone the role is free again.
            registry
                .update(
                    OWNER,
                    &held.session_id,
                    to(SessionState::Failed, "gone"),
                    &mut tx,
                )
                .unwrap();
            let (next, created) = registry
                .begin(
                    OWNER,
                    role(&format!("{}-2", kind.code()), kind, 1),
                    None,
                    &mut tx,
                )
                .unwrap();
            assert!(created);
            assert_ne!(next.session_id, held.session_id);
        }
    });
}

#[test]
fn hs2_gdjxgz_only_a_session_that_is_gone_can_be_replaced() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let (lost, _) = registry
            .begin(OWNER, new("op-1", "codex"), None, &mut tx)
            .unwrap();
        let replacement = |operation: &str, replaces: &str| NewSession {
            replaces: Some(replaces.into()),
            ..new(operation, "codex")
        };
        // Unknown, then still requested, then still running: none can be replaced.
        assert_eq!(
            session_error(registry.begin(OWNER, replacement("op-2", &id(77)), None, &mut tx)),
            SessionError::NotFound
        );
        assert_eq!(
            session_error(registry.begin(
                OWNER,
                replacement("op-2", &lost.session_id),
                None,
                &mut tx
            )),
            SessionError::InvalidTransition
        );
        registry
            .update(
                OWNER,
                &lost.session_id,
                to(SessionState::Running, "started"),
                &mut tx,
            )
            .unwrap();
        assert_eq!(
            session_error(registry.begin(
                OWNER,
                replacement("op-2", &lost.session_id),
                None,
                &mut tx
            )),
            SessionError::InvalidTransition
        );
        registry
            .update(
                OWNER,
                &lost.session_id,
                to(SessionState::Lost, "host_lost"),
                &mut tx,
            )
            .unwrap();
        let (second, created) = registry
            .begin(OWNER, replacement("op-2", &lost.session_id), None, &mut tx)
            .unwrap();
        assert!(created);
        assert_eq!(second.replaces.as_deref(), Some(lost.session_id.as_str()));
        // Asking again is the same replacement, never a third session.
        let (again, created) = registry
            .begin(OWNER, replacement("op-2", &lost.session_id), None, &mut tx)
            .unwrap();
        assert!(!created);
        assert_eq!(again.session_id, second.session_id);
        assert_eq!(registry.list(OWNER, true).unwrap().len(), 1);
    });
}

#[test]
fn hs2_gdjxgz_ownership_moves_to_a_person_once_and_never_returns() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let (session, _) = registry
            .begin(OWNER, new("op-1", "codex"), None, &mut tx)
            .unwrap();
        let sid = session.session_id.clone();
        // The orchestrator cannot pause its own session: a pause is a person's.
        let unchanged = registry
            .update(
                OWNER,
                &sid,
                SessionUpdate {
                    paused: Some(true),
                    control: Some(SessionControl::Pause),
                    ..SessionUpdate::default()
                },
                &mut tx,
            )
            .unwrap();
        assert_eq!(unchanged, session);
        registry
            .update(OWNER, &sid, to(SessionState::Running, "started"), &mut tx)
            .unwrap();
        clock.0.set(2_000);
        let taken = registry
            .update(OWNER, &sid, takeover(false), &mut tx)
            .unwrap();
        assert_eq!(taken.owner, SessionOwner::Human);
        assert!(!taken.paused);
        assert_eq!(taken.state, SessionState::Running);
        let entry = taken.history.last().unwrap();
        assert_eq!(entry.control, Some(SessionControl::Takeover));
        assert_eq!((entry.from, entry.to), (Some(taken.state), taken.state));
        assert_eq!(taken.updated_at_ms, 2_000);
        // Repeating the takeover writes nothing.
        clock.0.set(3_000);
        assert_eq!(
            registry
                .update(OWNER, &sid, takeover(false), &mut tx)
                .unwrap(),
            taken
        );
        // Handing it back is not a thing: the owner stays a person.
        let back = registry
            .update(
                OWNER,
                &sid,
                SessionUpdate {
                    owner: Some(SessionOwner::Orchestrator),
                    control: Some(SessionControl::Takeover),
                    ..SessionUpdate::default()
                },
                &mut tx,
            )
            .unwrap();
        assert_eq!(back, taken);
        // A pause after the takeover is its own entry, and stays.
        let paused = registry
            .update(OWNER, &sid, takeover(true), &mut tx)
            .unwrap();
        assert!(paused.paused);
        assert_eq!(
            paused.history.last().unwrap().control,
            Some(SessionControl::Pause)
        );
        let still = registry
            .update(
                OWNER,
                &sid,
                SessionUpdate {
                    paused: Some(false),
                    control: Some(SessionControl::Pause),
                    ..SessionUpdate::default()
                },
                &mut tx,
            )
            .unwrap();
        assert!(still.paused);
        assert_eq!(still.history.len(), paused.history.len());
        // The person's session still ends like any other.
        let stopped = registry
            .update(
                OWNER,
                &sid,
                SessionUpdate {
                    control: Some(SessionControl::Stop),
                    ..to(SessionState::Stopped, "person")
                },
                &mut tx,
            )
            .unwrap();
        assert_eq!(stopped.owner, SessionOwner::Human);
        assert_eq!(
            session_error(registry.update(OWNER, &sid, takeover(false), &mut tx)),
            SessionError::NotActive
        );
    });
}

#[test]
fn hs2_gdjxgz_history_keeps_the_first_entry_and_the_most_recent_ones() {
    let home = tempfile::tempdir().unwrap();
    let registry = SessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(1));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (session, _) = registry
        .begin(OWNER, new("op-1", "codex"), Some("first".into()), &mut tx)
        .unwrap();
    registry
        .update(
            OWNER,
            &session.session_id,
            to(SessionState::Running, "started"),
            &mut tx,
        )
        .unwrap();
    // Only real changes add entries, so the bound is checked on a stored record.
    let mut stored = registry.get(OWNER, &session.session_id).unwrap();
    while stored.history.len() < MAX_SESSION_HISTORY {
        stored.history.push(stored.history[1].clone());
    }
    let path = home.path().join("orchestration-sessions.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&serde_json::json!({"schema_version":1,"sessions":[stored]})).unwrap(),
    )
    .unwrap();
    let taken = registry
        .update(OWNER, &session.session_id, takeover(false), &mut tx)
        .unwrap();
    assert_eq!(taken.history.len(), MAX_SESSION_HISTORY);
    assert_eq!(taken.history[0].actor_id.as_deref(), Some("first"));
    assert_eq!(
        taken.history.last().unwrap().control,
        Some(SessionControl::Takeover)
    );
}

#[test]
fn hs2_gdjxgz_load_counts_what_still_occupies_a_machine_and_a_tool() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let server = id(900);
        let load = |registry: &SessionRegistry, tool: &str, account: Option<&str>| {
            registry
                .load(OWNER, &id(1), Some(&server), tool, account)
                .unwrap()
        };
        // Empty.
        assert_eq!(load(registry, "codex", None), SessionLoad::default());
        let start = |tx: &mut Transaction<'_>, operation: &str, target: u128, tool: &str| {
            registry
                .begin(
                    OWNER,
                    NewSession {
                        target_id: id(target),
                        account: (tool == "claude").then(|| "team".into()),
                        ..new(operation, tool)
                    },
                    None,
                    tx,
                )
                .unwrap()
                .0
        };
        let a = start(&mut tx, "a", 1, "codex");
        let b = start(&mut tx, "b", 1, "claude");
        let c = start(&mut tx, "c", 2, "codex");
        // A requested session already counts: it may be running by now.
        let seen = load(registry, "codex", None);
        assert_eq!(
            (
                seen.target_sessions,
                seen.machine_sessions,
                seen.tool_sessions
            ),
            (2, 2, 2)
        );
        // The other target's session lands on the same machine.
        registry
            .update(
                OWNER,
                &c.session_id,
                SessionUpdate {
                    hosted: Some(HostedRef {
                        server_id: server.clone(),
                        session_id: id(700),
                        worker_id: "codex-x".into(),
                    }),
                    ..to(SessionState::Running, "started")
                },
                &mut tx,
            )
            .unwrap();
        assert_eq!(load(registry, "codex", None).machine_sessions, 3);
        let claude = load(registry, "claude", Some("team"));
        assert_eq!((claude.tool_sessions, claude.account_sessions), (1, 1));
        assert_eq!(claude.account.as_deref(), Some("team"));
        assert_eq!(load(registry, "claude", Some("other")).account_sessions, 0);
        // A session a person took over still runs, so it still counts.
        registry
            .update(OWNER, &b.session_id, takeover(true), &mut tx)
            .unwrap();
        assert_eq!(load(registry, "claude", Some("team")).tool_sessions, 1);
        // Every way of ending frees its place.
        registry
            .update(
                OWNER,
                &a.session_id,
                to(SessionState::Failed, "refused"),
                &mut tx,
            )
            .unwrap();
        registry
            .update(
                OWNER,
                &b.session_id,
                to(SessionState::Stopped, "person"),
                &mut tx,
            )
            .unwrap();
        registry
            .update(
                OWNER,
                &c.session_id,
                to(SessionState::Lost, "host_lost"),
                &mut tx,
            )
            .unwrap();
        assert_eq!(load(registry, "codex", None), SessionLoad::default());
        assert!(registry.list(OWNER, true).unwrap().is_empty());
        assert_eq!(registry.list(OWNER, false).unwrap().len(), 3);
        // Refill after empty.
        start(&mut tx, "d", 1, "codex");
        assert_eq!(load(registry, "codex", None).target_sessions, 1);
    });
}

#[test]
fn hs2_gdjxgz_invalid_requests_and_damaged_files_are_refused() {
    with_registry(|registry, clock, rng, home| {
        let mut tx = Transaction { clock, rng };
        let invalid: [fn(&mut NewSession); 6] = [
            |new| new.operation_id = "has space".into(),
            |new| new.target_id = "not-a-ulid".into(),
            |new| new.tool = String::new(),
            |new| new.model = Some("two words".into()),
            |new| new.effort = Some("a/b".into()),
            |new| new.run_id = Some("run".into()),
        ];
        for change in invalid {
            let mut request = new("op-1", "codex");
            change(&mut request);
            assert!(matches!(
                registry.begin(OWNER, request, None, &mut tx),
                Err(OrchestrationError::Invalid(_))
            ));
        }
        // Nothing was written by a refused request, and a read creates nothing.
        assert!(registry.list(OWNER, false).unwrap().is_empty());
        let path = home.join("orchestration-sessions.json");
        assert!(!path.exists());
        assert_eq!(
            session_error(registry.get(OWNER, &id(5))),
            SessionError::NotFound
        );
        assert_eq!(
            session_error(registry.update(OWNER, &id(5), to(SessionState::Running, "x"), &mut tx)),
            SessionError::NotFound
        );
        std::fs::write(&path, br#"{"schema_version":2,"sessions":[]}"#).unwrap();
        assert!(matches!(
            registry.list(OWNER, false),
            Err(OrchestrationError::UnsupportedVersion { found: 2 })
        ));
        std::fs::write(
            &path,
            br#"{"schema_version":1,"sessions":[{"session_id":"x"}]}"#,
        )
        .unwrap();
        assert!(matches!(
            registry.list(OWNER, false),
            Err(OrchestrationError::Corrupt)
        ));
        // A stored pause on an orchestrator-owned session cannot be real.
        std::fs::remove_file(&path).unwrap();
        let (session, _) = registry
            .begin(OWNER, new("op-1", "codex"), None, &mut tx)
            .unwrap();
        let mut stored = serde_json::to_value(&session).unwrap();
        stored["paused"] = true.into();
        std::fs::write(
            &path,
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"sessions":[stored]}))
                .unwrap(),
        )
        .unwrap();
        assert!(matches!(
            registry.list(OWNER, false),
            Err(OrchestrationError::Corrupt)
        ));
    });
}

#[test]
fn hs2_gdjxgz_a_safe_point_needs_every_fact_known_and_clear() {
    let options = [None, Some(0), Some(2)];
    let flags = [None, Some(false), Some(true)];
    let mut seen_safe = 0;
    for alive in flags {
        for busy in flags {
            for held_claims in options {
                for dirty_worktrees in options {
                    for pending_handoffs in options {
                        let facts = SafePointFacts {
                            alive,
                            busy,
                            held_claims,
                            dirty_worktrees,
                            pending_handoffs,
                            idle_since_ms: None,
                        };
                        let decided = decide_safe_point(&facts);
                        let expected = alive == Some(false)
                            || (alive == Some(true)
                                && busy == Some(false)
                                && held_claims == Some(0)
                                && dirty_worktrees == Some(0)
                                && pending_handoffs == Some(0));
                        assert_eq!(decided.safe, expected, "{facts:?}");
                        assert_eq!(decided.safe, decided.blockers.is_empty());
                        assert_eq!(decided.facts, facts);
                        seen_safe += usize::from(decided.safe);
                        if alive == Some(false) {
                            continue;
                        }
                        use SafePointBlocker as B;
                        let has = |blocker| decided.blockers.contains(&blocker);
                        assert_eq!(has(B::SessionUnknown), alive.is_none() || busy.is_none());
                        assert_eq!(
                            has(B::MidTurn),
                            alive.is_some() && busy == Some(true),
                            "{facts:?}"
                        );
                        assert_eq!(has(B::HeldClaim), held_claims == Some(2));
                        assert_eq!(has(B::ClaimsUnknown), held_claims.is_none());
                        assert_eq!(has(B::UncommittedChanges), dirty_worktrees == Some(2));
                        assert_eq!(has(B::WorktreeUnknown), dirty_worktrees.is_none());
                        assert_eq!(has(B::PendingHandoff), pending_handoffs == Some(2));
                        assert_eq!(has(B::HandoffUnknown), pending_handoffs.is_none());
                        // A fixed order, and each blocker once.
                        let order: Vec<usize> = decided
                            .blockers
                            .iter()
                            .map(|blocker| B::ALL.iter().position(|b| b == blocker).unwrap())
                            .collect();
                        assert!(order.windows(2).all(|pair| pair[0] < pair[1]), "{order:?}");
                    }
                }
            }
        }
    }
    // A dead session, whatever else is known (81 cases), and one clean live one.
    assert_eq!(seen_safe, 82);
}

#[test]
fn hs2_gdjxgz_a_stop_is_decided_by_owner_asker_force_and_safe_point() {
    let safe = decide_safe_point(&SafePointFacts {
        alive: Some(true),
        busy: Some(false),
        held_claims: Some(0),
        dirty_worktrees: Some(0),
        pending_handoffs: Some(0),
        idle_since_ms: None,
    });
    let held = decide_safe_point(&SafePointFacts {
        held_claims: Some(1),
        ..safe.facts
    });
    assert!(safe.safe && !held.safe);
    for owner in [SessionOwner::Orchestrator, SessionOwner::Human] {
        for force in [false, true] {
            for human in [false, true] {
                for point in [&safe, &held] {
                    let expected = if force && !human {
                        Err(SessionError::ForceNotPermitted)
                    } else if owner == SessionOwner::Human && !human {
                        Err(SessionError::HumanOwned)
                    } else if !force && !point.safe {
                        Err(SessionError::NotSafe)
                    } else {
                        Ok(())
                    };
                    assert_eq!(
                        decide_stop(owner, StopRequest { force, human }, point),
                        expected,
                        "{owner:?} force={force} human={human} safe={}",
                        point.safe
                    );
                }
            }
        }
    }
    // The orchestrator never stops a session mid-work, and never a person's.
    assert!(decide_stop(SessionOwner::Orchestrator, StopRequest::default(), &held).is_err());
    assert!(decide_stop(SessionOwner::Human, StopRequest::default(), &safe).is_err());
    // A person can always stop one, explicitly.
    let person = StopRequest {
        force: true,
        human: true,
    };
    assert!(decide_stop(SessionOwner::Human, person, &held).is_ok());
    assert!(decide_stop(SessionOwner::Orchestrator, person, &held).is_ok());
}

#[test]
fn hs2_gdjxgz_codes_are_stable_distinct_and_round_trip() {
    let mut codes: Vec<&str> = SessionError::ALL.iter().map(|error| error.code()).collect();
    codes.extend(SafePointBlocker::ALL.iter().map(|blocker| blocker.code()));
    codes.extend(SessionState::ALL.iter().map(|state| state.code()));
    codes.extend(SessionControl::ALL.iter().map(|control| control.code()));
    let distinct: std::collections::BTreeSet<&str> = codes.iter().copied().collect();
    assert_eq!(distinct.len(), codes.len());
    for error in SessionError::ALL {
        assert_eq!(SessionError::from_code(error.code()), Some(error));
        assert!(error.code().starts_with("session_"));
        assert_eq!(OrchestrationError::Session(error).code(), error.code());
    }
    assert_eq!(SessionError::from_code("conflict"), None);
    // Serialized names are the codes.
    for state in SessionState::ALL {
        assert_eq!(serde_json::to_value(state).unwrap(), state.code());
    }
    for blocker in SafePointBlocker::ALL {
        assert_eq!(serde_json::to_value(blocker).unwrap(), blocker.code());
    }
    for role in SessionRole::ALL {
        assert_eq!(serde_json::to_value(role).unwrap(), role.code());
        assert_eq!(role.skill(), format!("hotsheet-{}", role.code()));
        assert_eq!(role.is_role_session(), role != SessionRole::Worker);
    }
}

#[test]
fn hs2_gdjxgz_the_label_names_the_project_and_the_run_or_role() {
    let run = id(0x1234_5678_9abc);
    let label = session_label("Release train", Some(&run), SessionRole::Worker);
    assert_eq!(
        label,
        format!("Release train: run {}", &run[run.len() - 8..])
    );
    assert_eq!(
        session_label("Release train", None, SessionRole::Integrator),
        "Release train: integrator"
    );
    // Control characters never reach a tab name, and a blank name has a stand-in.
    assert_eq!(
        session_label("  \u{7}\n ", None, SessionRole::Worker),
        "Orchestration: worker"
    );
    let long = "p".repeat(500);
    let bounded = session_label(&long, Some(&run), SessionRole::Worker);
    assert!(bounded.chars().count() <= MAX_SESSION_PROJECT_CHARS + 16);
}

fn hosted(operation: &str, tool: &str) -> NewHostedSession {
    NewHostedSession {
        operation_id: operation.into(),
        tool: tool.into(),
        model: Some("model-a".into()),
        effort: None,
        role: SessionRole::Worker,
        origin: SessionOrigin {
            owner_checkout_id: OWNER.into(),
            project: "Release train".into(),
            run_id: Some(id(40)),
            ticket: None,
        },
    }
}

#[test]
fn hs2_gdjxgz_a_host_reserves_one_terminal_per_operation_and_never_launches_twice() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(10));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(registry.list(HOSTED).unwrap().is_empty());
    assert!(
        !home
            .path()
            .join("orchestration-hosted-sessions.json")
            .exists()
    );
    let (first, created) = registry
        .reserve(HOSTED, &hosted("op-1", "codex"), &mut tx)
        .unwrap();
    assert!(created && !first.started);
    assert_eq!(first.owner, SessionOwner::Orchestrator);
    assert_eq!(
        first.label(),
        format!("Release train: run {}", &id(40)[18..])
    );
    // The same request again is the same reservation and the same terminal id.
    let (again, created) = registry
        .reserve(HOSTED, &hosted("op-1", "codex"), &mut tx)
        .unwrap();
    assert!(!created);
    assert_eq!(again, first);
    // The same id for another tool, or another orchestration project, conflicts.
    assert!(matches!(
        registry.reserve(HOSTED, &hosted("op-1", "claude"), &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    let mut foreign = hosted("op-1", "codex");
    foreign.origin.owner_checkout_id = "elsewhere-0123456789ab".into();
    assert!(matches!(
        registry.reserve(HOSTED, &foreign, &mut tx),
        Err(OrchestrationError::Conflict)
    ));
    // Another checkout's identical operation id is its own reservation.
    let (other, created) = registry
        .reserve("other-0123456789ab", &hosted("op-1", "codex"), &mut tx)
        .unwrap();
    assert!(created);
    assert_ne!(other.session_id, first.session_id);

    // Release forgets an unstarted reservation, and the start can be asked again.
    registry.release(HOSTED, "op-1").unwrap();
    assert!(registry.get(HOSTED, "op-1").unwrap().is_none());
    registry.release(HOSTED, "op-1").unwrap();
    let (second, created) = registry
        .reserve(HOSTED, &hosted("op-1", "codex"), &mut tx)
        .unwrap();
    assert!(created);
    assert_ne!(second.session_id, first.session_id);
    // Started, then released: a launched session is never forgotten.
    let started = registry.mark_started(HOSTED, "op-1").unwrap().unwrap();
    assert!(started.started);
    assert_eq!(
        registry.mark_started(HOSTED, "op-1").unwrap().unwrap(),
        started
    );
    registry.release(HOSTED, "op-1").unwrap();
    assert_eq!(registry.get(HOSTED, "op-1").unwrap().unwrap(), started);
    assert_eq!(
        registry
            .reserve(HOSTED, &hosted("op-1", "codex"), &mut tx)
            .unwrap(),
        (started.clone(), false)
    );

    // Out of order and interleaved: takeover, pause, end, then everything again.
    let taken = registry.take_over(HOSTED, "op-1", false).unwrap().unwrap();
    assert_eq!((taken.owner, taken.paused), (SessionOwner::Human, false));
    let paused = registry.take_over(HOSTED, "op-1", true).unwrap().unwrap();
    assert!(paused.paused);
    assert!(
        registry
            .take_over(HOSTED, "op-1", false)
            .unwrap()
            .unwrap()
            .paused
    );
    clock.0.set(50);
    let ended = registry
        .end(HOSTED, "op-1", "stopped", 50)
        .unwrap()
        .unwrap();
    assert_eq!(ended.ended_at_ms, Some(50));
    // The first reason stands; nothing changes an ended session.
    let later = registry
        .end(HOSTED, "op-1", "host_lost", 90)
        .unwrap()
        .unwrap();
    assert_eq!(later, ended);
    assert_eq!(
        registry.take_over(HOSTED, "op-1", true).unwrap().unwrap(),
        ended
    );
    assert_eq!(
        registry.mark_started(HOSTED, "op-1").unwrap().unwrap(),
        ended
    );
    // An unknown operation is an absence, not an error.
    assert!(
        registry
            .end(HOSTED, "nope", "stopped", 1)
            .unwrap()
            .is_none()
    );
    assert!(registry.take_over(HOSTED, "nope", true).unwrap().is_none());
    assert!(registry.mark_started(HOSTED, "nope").unwrap().is_none());
    assert!(registry.end(HOSTED, "op-1", "not a code", 1).is_err());
    assert_eq!(registry.list(HOSTED).unwrap().len(), 1);
    assert_eq!(registry.list("other-0123456789ab").unwrap().len(), 1);
}

#[test]
fn hs2_gdjxgz_a_host_refuses_malformed_starts_and_damaged_records() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(10));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let invalid: [fn(&mut NewHostedSession); 6] = [
        |new| new.operation_id = String::new(),
        |new| new.tool = "a tool".into(),
        |new| new.model = Some(String::new()),
        |new| new.origin.project = "p".repeat(MAX_SESSION_PROJECT_CHARS + 1),
        |new| new.origin.project = "line\nbreak".into(),
        |new| new.origin.run_id = Some("run".into()),
    ];
    for change in invalid {
        let mut request = hosted("op-1", "codex");
        change(&mut request);
        assert!(matches!(
            registry.reserve(HOSTED, &request, &mut tx),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    let path = home.path().join("orchestration-hosted-sessions.json");
    assert!(!path.exists());
    std::fs::write(&path, br#"{"schema_version":9,"sessions":[]}"#).unwrap();
    assert!(matches!(
        registry.reserve(HOSTED, &hosted("op-1", "codex"), &mut tx),
        Err(OrchestrationError::UnsupportedVersion { found: 9 })
    ));
    std::fs::write(&path, b"{").unwrap();
    assert!(matches!(
        registry.list(HOSTED),
        Err(OrchestrationError::Corrupt)
    ));
}

#[test]
fn hs2_2dpgdy_the_host_records_role_tries_and_finds_a_started_session_by_terminal() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(10));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (reserved, _) = registry
        .reserve(HOSTED, &hosted("op-1", "codex"), &mut tx)
        .unwrap();
    // A reservation that never launched is not found by its terminal.
    assert_eq!(registry.by_session(&reserved.session_id).unwrap(), None);
    registry.mark_started(HOSTED, "op-1").unwrap();
    let found = registry.by_session(&reserved.session_id).unwrap().unwrap();
    assert_eq!(found.operation_id, "op-1");
    assert!(found.role_delivery.is_unasked());
    // Unasked is left out of the stored record.
    let stored =
        std::fs::read_to_string(home.path().join("orchestration-hosted-sessions.json")).unwrap();
    assert!(!stored.contains("role_delivery"), "{stored}");

    // Not ready, not ready again, then delivered: every try counted.
    let at = |attempt, now| {
        registry
            .role_attempt(HOSTED, "op-1", Some(attempt), now)
            .unwrap()
            .unwrap()
            .role_delivery
    };
    assert_eq!(
        at(PushAttempt::NotReady, 20).prompt,
        Some(RolePrompt::Pending)
    );
    assert_eq!(at(PushAttempt::NotReady, 30).attempts, 2);
    let delivered = at(PushAttempt::Delivered, 40);
    assert_eq!(delivered.prompt, Some(RolePrompt::Delivered));
    assert_eq!(delivered.delivered_at_ms, Some(40));
    // A late try changes nothing.
    assert_eq!(at(PushAttempt::Rejected, 50), delivered);

    // At launch: recorded once, never over an earlier answer.
    registry
        .reserve(HOSTED, &hosted("op-2", "codex"), &mut tx)
        .unwrap();
    registry.mark_started(HOSTED, "op-2").unwrap();
    let launched = registry
        .role_attempt(HOSTED, "op-2", None, 60)
        .unwrap()
        .unwrap();
    assert_eq!(launched.role_delivery, RoleDelivery::at_launch(60));
    let again = registry
        .role_attempt(HOSTED, "op-2", None, 70)
        .unwrap()
        .unwrap();
    assert_eq!(again.role_delivery, RoleDelivery::at_launch(60));
    // An ended session's delivery is left as it was; an unknown operation is none.
    registry.end(HOSTED, "op-2", "wound_down", 80).unwrap();
    let ended = registry
        .role_attempt(HOSTED, "op-2", Some(PushAttempt::Rejected), 90)
        .unwrap()
        .unwrap();
    assert_eq!(ended.role_delivery, RoleDelivery::at_launch(60));
    assert_eq!(
        registry
            .role_attempt(HOSTED, "op-x", Some(PushAttempt::Delivered), 90)
            .unwrap(),
        None
    );
}

#[test]
fn hs2_2dpgdy_a_session_keeps_its_latest_role_answer_and_its_first_join() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let (session, _) = registry
            .begin(OWNER, new("op-1", "codex"), None, &mut tx)
            .unwrap();
        let update = |prompt, joined| SessionUpdate {
            role_prompt: prompt,
            joined_at_ms: joined,
            ..SessionUpdate::default()
        };
        let mut tx = Transaction { clock, rng };
        let after = registry
            .update(
                OWNER,
                &session.session_id,
                update(Some(RolePrompt::Pending), None),
                &mut tx,
            )
            .unwrap();
        assert_eq!(after.role_prompt, Some(RolePrompt::Pending));
        assert_eq!(after.history.len(), 1, "not a state change");
        clock.0.set(2_000);
        let joined = registry
            .update(
                OWNER,
                &session.session_id,
                update(Some(RolePrompt::Delivered), Some(2_000)),
                &mut tx,
            )
            .unwrap();
        assert_eq!(joined.role_prompt, Some(RolePrompt::Delivered));
        assert_eq!(joined.joined_at_ms, Some(2_000));
        // A later sighting keeps the first time; an unchanged update writes nothing.
        clock.0.set(3_000);
        let later = registry
            .update(
                OWNER,
                &session.session_id,
                update(None, Some(3_000)),
                &mut tx,
            )
            .unwrap();
        assert_eq!(later, joined);
        assert_eq!(later.updated_at_ms, 2_000);
    });
}

#[test]
fn hs2_yc2p7k_a_session_is_linked_to_a_run_without_a_state_change() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let (session, _) = registry
            .begin(OWNER, new("op-1", "codex"), None, &mut tx)
            .unwrap();
        assert_eq!(session.run_id, None);
        let link = |run: &str, tx: &mut Transaction<'_>| {
            registry
                .update(
                    OWNER,
                    &session.session_id,
                    SessionUpdate {
                        run_id: Some(run.into()),
                        ..SessionUpdate::default()
                    },
                    tx,
                )
                .unwrap()
        };
        let linked = link(&id(40), &mut tx);
        assert_eq!(linked.run_id, Some(id(40)));
        assert_eq!(linked.history.len(), 1, "not a state change");
        // The same link again writes nothing; a refill moves it to the next run.
        assert_eq!(link(&id(40), &mut tx), linked);
        assert_eq!(link(&id(41), &mut tx).run_id, Some(id(41)));
        // A finished session is never relinked.
        registry
            .update(
                OWNER,
                &session.session_id,
                to(SessionState::Failed, "refused"),
                &mut tx,
            )
            .unwrap();
        assert_eq!(
            session_error(registry.update(
                OWNER,
                &session.session_id,
                SessionUpdate {
                    run_id: Some(id(42)),
                    ..SessionUpdate::default()
                },
                &mut tx,
            )),
            SessionError::NotActive
        );
    });
}

#[test]
fn hs2_yc2p7k_a_hosted_origin_names_its_ticket_and_it_is_validated() {
    let mut start = hosted("op-1", "codex");
    start.origin.ticket = Some(id(700));
    assert!(start.validate().is_ok());
    start.origin.ticket = Some("not a ticket id".into());
    assert!(start.validate().is_err());
}

fn for_run(operation: &str, run: &str) -> NewSession {
    NewSession {
        run_id: Some(run.into()),
        ..new(operation, "codex")
    }
}

/// HS2-DG0YPA: a run has at most one live session. A start or a link for a run
/// another `requested` or `running` session holds is refused, whatever the holder's
/// state, and a finished holder holds nothing.
#[test]
fn hs2_dg0ypa_a_run_held_by_a_live_session_refuses_a_second_one_in_every_state() {
    use SessionState::{Failed, Lost, Requested, Running, Stopped};
    // (the holder's state, whether it still holds the run)
    for (state, holds) in [
        (Requested, true),
        (Running, true),
        (Stopped, false),
        (Failed, false),
        (Lost, false),
    ] {
        with_registry(|registry, clock, rng, _| {
            let mut tx = Transaction { clock, rng };
            let run = id(50);
            let (holder, _) = registry
                .begin(OWNER, for_run("op-holder", &run), None, &mut tx)
                .unwrap();
            // A failed start never ran; every other end ran first.
            if !matches!(state, Requested | Failed) {
                registry
                    .update(OWNER, &holder.session_id, to(Running, "started"), &mut tx)
                    .unwrap();
            }
            if !matches!(state, Requested | Running) {
                registry
                    .update(OWNER, &holder.session_id, to(state, "ended"), &mut tx)
                    .unwrap();
            }
            let records = registry.list(OWNER, false).unwrap();
            assert_eq!(
                run_holder(&records, &run).map(|s| s.session_id.clone()),
                holds.then(|| holder.session_id.clone()),
                "{state:?}"
            );
            // A second start for the run.
            let check = registry.check(OWNER, &for_run("op-second", &run));
            let begun = registry.begin(OWNER, for_run("op-second", &run), None, &mut tx);
            if holds {
                assert_eq!(session_error(check), SessionError::RunHeld, "{state:?}");
                assert_eq!(session_error(begun), SessionError::RunHeld, "{state:?}");
                assert_eq!(registry.list(OWNER, false).unwrap().len(), 1);
            } else {
                assert_eq!(check.unwrap(), None, "{state:?}");
                assert!(begun.unwrap().1, "{state:?}");
            }
            // A running session without a run is linked to it.
            let (free, _) = registry
                .begin(OWNER, new("op-free", "codex"), None, &mut tx)
                .unwrap();
            let link = registry.update(
                OWNER,
                &free.session_id,
                SessionUpdate {
                    run_id: Some(run.clone()),
                    ..SessionUpdate::default()
                },
                &mut tx,
            );
            // Whoever started second now holds it in the free case.
            assert_eq!(session_error(link), SessionError::RunHeld, "{state:?}");
            // The same operation id is still the same start, whatever holds the run.
            let (again, created) = registry
                .begin(OWNER, for_run("op-holder", &run), None, &mut tx)
                .unwrap();
            assert!(!created);
            assert_eq!(again.session_id, holder.session_id);
        });
    }
}

/// HS2-DG0YPA: repeated, interleaved, out-of-order and empty-then-refill sequences on
/// one run, with another run and another owner alongside.
#[test]
fn hs2_dg0ypa_one_live_session_per_run_across_repeats_losses_and_refills() {
    with_registry(|registry, clock, rng, _| {
        let mut tx = Transaction { clock, rng };
        let (run, other_run) = (id(60), id(61));
        // Two starts race for the run: the first is recorded, the second refused, and
        // repeating either changes nothing.
        let (a, _) = registry
            .begin(OWNER, for_run("op-a", &run), None, &mut tx)
            .unwrap();
        for _ in 0..2 {
            assert_eq!(
                session_error(registry.begin(OWNER, for_run("op-b", &run), None, &mut tx)),
                SessionError::RunHeld
            );
            assert!(
                !registry
                    .begin(OWNER, for_run("op-a", &run), None, &mut tx)
                    .unwrap()
                    .1
            );
        }
        // Another run and another owner's run of the same id are unaffected.
        assert!(
            registry
                .begin(OWNER, for_run("op-c", &other_run), None, &mut tx)
                .unwrap()
                .1
        );
        assert!(
            registry
                .begin("owner-ba9876543210", for_run("op-b", &run), None, &mut tx)
                .unwrap()
                .1
        );
        registry
            .update(
                OWNER,
                &a.session_id,
                to(SessionState::Running, "started"),
                &mut tx,
            )
            .unwrap();
        assert_eq!(
            session_error(registry.begin(OWNER, for_run("op-b", &run), None, &mut tx)),
            SessionError::RunHeld
        );
        // It crashes: the run is empty, and the replacement that names it takes it.
        registry
            .update(
                OWNER,
                &a.session_id,
                to(SessionState::Lost, "host_lost"),
                &mut tx,
            )
            .unwrap();
        let replacement = NewSession {
            replaces: Some(a.session_id.clone()),
            ..for_run("op-b", &run)
        };
        let (b, created) = registry
            .begin(OWNER, replacement.clone(), None, &mut tx)
            .unwrap();
        assert!(created);
        assert_eq!(
            run_holder(&registry.list(OWNER, false).unwrap(), &run).map(|s| s.session_id.clone()),
            Some(b.session_id.clone())
        );
        // Out of order: the lost session's own start is repeated late; it is the same
        // finished session and holds nothing.
        let (late, created) = registry
            .begin(OWNER, for_run("op-a", &run), None, &mut tx)
            .unwrap();
        assert!(!created);
        assert_eq!(late.state, SessionState::Lost);
        // Refill: the replacement ends too, and the run takes a third session; the
        // second's repeat is still itself.
        registry
            .update(
                OWNER,
                &b.session_id,
                to(SessionState::Failed, "refused"),
                &mut tx,
            )
            .unwrap();
        assert!(
            registry
                .begin(OWNER, for_run("op-d", &run), None, &mut tx)
                .unwrap()
                .1
        );
        assert_eq!(
            registry
                .begin(OWNER, replacement, None, &mut tx)
                .unwrap()
                .0
                .session_id,
            b.session_id
        );
        assert_eq!(
            session_error(registry.begin(OWNER, for_run("op-e", &run), None, &mut tx)),
            SessionError::RunHeld
        );
    });
}

/// HS2-R7QH35: a hosted session's request naming one ticket: recorded once per ticket
/// and kind, replaced by another, delivered within the role prompt's bounds counted
/// from the request, and left alone once the session ended.
#[test]
fn hs2_r7qh35_a_request_is_recorded_once_replaced_by_another_and_delivered_in_bounds() {
    let home = tempfile::tempdir().unwrap();
    let registry = HostedSessionRegistry::new(home.path());
    let clock = Clock(std::cell::Cell::new(10));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    registry
        .reserve(HOSTED, &hosted("op-1", "codex"), &mut tx)
        .unwrap();
    registry.mark_started(HOSTED, "op-1").unwrap();
    let request = |ticket: &str, kind, now| {
        registry
            .request(HOSTED, "op-1", ticket, kind, now)
            .unwrap()
            .unwrap()
            .request
            .unwrap()
    };
    let attempt = |answer, now| {
        registry
            .request_attempt(HOSTED, "op-1", answer, now)
            .unwrap()
            .unwrap()
            .request
            .unwrap()
    };
    // No request yet: an attempt records nothing.
    assert_eq!(
        registry
            .request_attempt(HOSTED, "op-1", PushAttempt::Delivered, 15)
            .unwrap()
            .unwrap()
            .request,
        None
    );
    let first = request(&id(700), RequestKind::Assigned, 20);
    assert_eq!((first.requested_at_ms, first.at(20)), (20, None));
    // Not ready, then the same request again: kept, not reset.
    assert_eq!(
        attempt(PushAttempt::NotReady, 30).at(30),
        Some(RolePrompt::Pending)
    );
    let repeated = request(&id(700), RequestKind::Assigned, 40);
    assert_eq!(
        (repeated.requested_at_ms, repeated.delivery.attempts),
        (20, 1)
    );
    // Delivered; a late try changes nothing.
    let delivered = attempt(PushAttempt::Delivered, 50);
    assert_eq!(delivered.at(50), Some(RolePrompt::Delivered));
    assert_eq!(attempt(PushAttempt::Rejected, 60), delivered);
    // The same ticket claimed for it is a new request; so is another ticket.
    let claimed = request(&id(700), RequestKind::Claimed, 70);
    assert_eq!(
        (claimed.kind, claimed.requested_at_ms, claimed.at(70)),
        (RequestKind::Claimed, 70, None)
    );
    let next = request(&id(701), RequestKind::Assigned, 80);
    assert_eq!((next.ticket.clone(), next.requested_at_ms), (id(701), 80));
    // Pending past its window, counted from the request, reads failed.
    attempt(PushAttempt::NotReady, 90);
    let pending = attempt(PushAttempt::NotReady, 100);
    assert_eq!(pending.at(100), Some(RolePrompt::Pending));
    assert_eq!(
        pending.at(80 + ROLE_PROMPT_WINDOW_MS),
        Some(RolePrompt::Failed)
    );
    // An ended session keeps what it had; a bad ticket id is refused; an unknown
    // operation is none.
    registry.end(HOSTED, "op-1", "wound_down", 110).unwrap();
    let ended = registry
        .request(HOSTED, "op-1", &id(702), RequestKind::Assigned, 120)
        .unwrap()
        .unwrap();
    assert_eq!(ended.request.unwrap().ticket, id(701));
    assert!(
        registry
            .request(HOSTED, "op-1", "not a ticket", RequestKind::Assigned, 120)
            .is_err()
    );
    assert_eq!(
        registry
            .request(HOSTED, "op-x", &id(700), RequestKind::Assigned, 120)
            .unwrap(),
        None
    );
    assert_eq!(
        serde_json::to_value(RequestKind::Claimed).unwrap(),
        "claimed"
    );
}

/// HS2-RG91SK: the host's idle time rides on the facts without deciding anything, and
/// facts from a host that reports more read leniently.
#[test]
fn hs2_rg91sk_the_idle_time_is_a_fact_not_a_blocker() {
    let base = SafePointFacts {
        alive: Some(true),
        busy: Some(false),
        held_claims: Some(0),
        dirty_worktrees: Some(0),
        pending_handoffs: Some(0),
        idle_since_ms: None,
    };
    let stamped = SafePointFacts {
        idle_since_ms: Some(5_000),
        ..base
    };
    assert_eq!(
        decide_safe_point(&base).safe,
        decide_safe_point(&stamped).safe
    );
    assert_eq!(decide_safe_point(&stamped).facts.idle_since_ms, Some(5_000));
    let wire = serde_json::to_value(stamped).unwrap();
    assert_eq!(wire["idle_since_ms"], 5_000);
    assert!(
        serde_json::to_value(base)
            .unwrap()
            .get("idle_since_ms")
            .is_none()
    );
    let newer: SafePointFacts = serde_json::from_value(serde_json::json!({
        "alive": true, "busy": false, "held_claims": 0, "dirty_worktrees": 0,
        "pending_handoffs": 0, "idle_since_ms": 7, "a_later_fact": 1
    }))
    .unwrap();
    assert_eq!(newer.idle_since_ms, Some(7));
}
