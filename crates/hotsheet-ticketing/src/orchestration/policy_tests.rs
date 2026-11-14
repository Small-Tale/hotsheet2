//! HS2-YCJDEB policy format, validation and evaluation; synthetic roots only.

use std::collections::BTreeSet;

use super::*;

struct Clock(u64);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn tool(name: &str, max_sessions: u32) -> ToolPolicy {
    ToolPolicy {
        tool: name.into(),
        max_sessions,
        weight: 50,
        accounts: Vec::new(),
    }
}
fn scope(name: &str, models: &[&str]) -> ToolScope {
    ToolScope {
        tool: name.into(),
        models: models.iter().map(|model| (*model).into()).collect(),
    }
}
fn target(value: u128, tools: Vec<ToolScope>) -> TargetPolicy {
    TargetPolicy {
        target_id: id(value),
        enabled: true,
        actions: None,
        tools,
        max_sessions: 2,
        machine: None,
        speculation: None,
    }
}
/// Two tools, one target allowed everything, machine cap 3.
fn policy() -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 3,
            ..MachineLimits::default()
        },
        tools: vec![tool("claude", 2), tool("codex", 2)],
        targets: vec![target(
            1,
            vec![scope("claude", &["model-a"]), scope("codex", &[])],
        )],
        ..PolicyBody::default()
    }
}
fn codes(body: &PolicyBody, declared: Option<&BTreeSet<String>>) -> Vec<&'static str> {
    body.problems(declared)
        .into_iter()
        .map(|problem| problem.code)
        .collect()
}
fn start(tool: &str, model: Option<&str>) -> SessionRequest {
    SessionRequest {
        tool: tool.into(),
        model: model.map(Into::into),
        running: 0,
        starting: true,
    }
}
fn ask<'a>(
    action: TargetAction,
    session: Option<&'a SessionRequest>,
    load: Option<&'a SessionLoad>,
) -> PolicyQuestion<'a> {
    PolicyQuestion {
        target_id: "00000000000000000000000001",
        action,
        session,
        load,
        provisional: false,
        role_session: false,
    }
}

#[test]
fn hs2_ycjdeb_absent_read_set_noop_and_revision_authority() {
    let root = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let absent = store.read().unwrap();
    assert!(!absent.enabled && absent.revision.is_none() && absent.policy.is_none());
    assert!(!root.path().join(".hotsheet2").exists());
    // A set against an absent policy must not name a revision.
    assert!(matches!(
        store.set(
            SetPolicy {
                expected_revision: Some(id(5)),
                policy: policy()
            },
            None,
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    let first = store
        .set(
            SetPolicy {
                expected_revision: None,
                policy: policy(),
            },
            None,
            &mut tx,
        )
        .unwrap();
    let path = root.path().join(".hotsheet2/orchestration-policy.json");
    let bytes = std::fs::read(&path).unwrap();
    // Checked-in shape: versioned, revisioned, readable diffs.
    let text = String::from_utf8(bytes.clone()).unwrap();
    assert!(text.starts_with("{\n  \"schema_version\": 1,\n  \"revision\": \""));
    assert!(text.ends_with("}\n"));
    let repeat = store
        .set(
            SetPolicy {
                expected_revision: first.revision.clone(),
                policy: policy(),
            },
            None,
            &mut tx,
        )
        .unwrap();
    assert_eq!(repeat, first);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(matches!(
        store.set(
            SetPolicy {
                expected_revision: None,
                policy: policy()
            },
            None,
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    let mut changed = policy();
    changed.wind_down.idle_minutes = 30;
    let second = store
        .set(
            SetPolicy {
                expected_revision: first.revision.clone(),
                policy: changed,
            },
            None,
            &mut tx,
        )
        .unwrap();
    assert_ne!(second.revision, first.revision);
    // Changing back still rotates: a revision is not a content hash.
    let third = store
        .set(
            SetPolicy {
                expected_revision: second.revision.clone(),
                policy: policy(),
            },
            None,
            &mut tx,
        )
        .unwrap();
    assert_ne!(third.revision, first.revision);
    assert_eq!(third.policy, first.policy);
    assert!(matches!(
        store.set(
            SetPolicy {
                expected_revision: first.revision,
                policy: policy()
            },
            None,
            &mut tx
        ),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(store.read().unwrap(), third);
}

#[test]
fn hs2_ycjdeb_version_shape_fills_defaults_and_refuses_newer_or_foreign_files() {
    let root = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let directory = root.path().join(".hotsheet2");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("orchestration-policy.json");
    // A version-1 file that names only what it changes reads with every default.
    std::fs::write(
        &path,
        format!(
            r#"{{"schema_version":1,"revision":"{}","policy":{{}}}}"#,
            id(9)
        ),
    )
    .unwrap();
    let minimal = store.read().unwrap().policy.unwrap();
    assert_eq!(minimal, PolicyBody::default());
    assert_eq!(
        minimal.machine,
        MachineLimits {
            max_sessions: 2,
            min_free_ram_mb: 2048,
            max_cpu_percent: 90,
            min_free_disk_mb: 5120
        }
    );
    assert_eq!(minimal.wind_down.idle_minutes, 15);
    assert_eq!(minimal.wind_down.keep_warm, 0);
    assert!(!minimal.speculation.allowed);
    assert_eq!(minimal.speculation.max_depth, 1);
    assert_eq!(minimal.speculation.risk_weight, 50);
    assert_eq!(minimal.speculation.opt_out_tag, "speculate:never");
    assert!(minimal.approval.is_none());
    assert!(minimal.ignored_fields().is_empty());
    assert!(minimal.problems(None).is_empty());
    let cases: [(String, bool); 5] = [
        (
            r#"{"schema_version":2,"revision":"x","policy":{}}"#.into(),
            true,
        ),
        ("{not json".into(), false),
        (r#"{"revision":"x","policy":{}}"#.into(), false),
        (
            format!(
                r#"{{"schema_version":1,"revision":"{}","policy":{{"surprise":1}}}}"#,
                id(9)
            ),
            false,
        ),
        // Stored content that no longer validates is not evaluated as if it did.
        (
            format!(
                r#"{{"schema_version":1,"revision":"{}","policy":{{"machine":{{"max_sessions":0}}}}}}"#,
                id(9)
            ),
            false,
        ),
    ];
    for (bytes, newer) in cases {
        std::fs::write(&path, &bytes).unwrap();
        let read = store.read();
        let set = store.set(
            SetPolicy {
                expected_revision: Some(id(9)),
                policy: policy(),
            },
            None,
            &mut tx,
        );
        if newer {
            assert!(matches!(
                read,
                Err(OrchestrationError::UnsupportedVersion { found: 2 })
            ));
            assert!(matches!(
                set,
                Err(OrchestrationError::UnsupportedVersion { found: 2 })
            ));
        } else {
            assert!(matches!(read, Err(OrchestrationError::Corrupt)), "{bytes}");
            assert!(matches!(set, Err(OrchestrationError::Corrupt)), "{bytes}");
        }
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_ycjdeb_malformed_and_conflicting_policies_report_every_problem() {
    assert!(codes(&policy(), None).is_empty());
    type Change = Box<dyn Fn(&mut PolicyBody)>;
    let cases: Vec<(&str, Change)> = vec![
        (
            "invalid_machine_cap",
            Box::new(|p| p.machine.max_sessions = 0),
        ),
        (
            "invalid_machine_cap",
            Box::new(|p| p.machine.max_sessions = 65),
        ),
        (
            "invalid_cpu_percent",
            Box::new(|p| p.machine.max_cpu_percent = 0),
        ),
        (
            "invalid_cpu_percent",
            Box::new(|p| p.machine.max_cpu_percent = 101),
        ),
        (
            "invalid_resource_floor",
            Box::new(|p| p.machine.min_free_ram_mb = 8 * 1024 * 1024),
        ),
        (
            "invalid_tool",
            Box::new(|p| p.tools[0].tool = "has space".into()),
        ),
        (
            "duplicate_tool",
            Box::new(|p| p.tools.push(tool("codex", 1))),
        ),
        (
            "invalid_tool_cap",
            Box::new(|p| p.tools[1].max_sessions = 0),
        ),
        ("invalid_tool_weight", Box::new(|p| p.tools[1].weight = 0)),
        ("invalid_tool_weight", Box::new(|p| p.tools[1].weight = 101)),
        (
            "account_cap_exceeds_tool",
            Box::new(|p| {
                p.tools[1].accounts.push(AccountPolicy {
                    account: "team".into(),
                    max_sessions: 3,
                    budget_cents_per_day: None,
                });
            }),
        ),
        (
            "duplicate_account",
            Box::new(|p| {
                for _ in 0..2 {
                    p.tools[1].accounts.push(AccountPolicy {
                        account: "team".into(),
                        max_sessions: 1,
                        budget_cents_per_day: Some(500),
                    });
                }
            }),
        ),
        (
            "invalid_target",
            Box::new(|p| p.targets[0].target_id = "label".into()),
        ),
        (
            "duplicate_target",
            Box::new(|p| p.targets.push(target(1, Vec::new()))),
        ),
        (
            "unconfigured_tool",
            Box::new(|p| p.targets[0].tools.push(scope("opencode", &[]))),
        ),
        (
            "invalid_models",
            Box::new(|p| p.targets[0].tools[0].models = vec!["m".into(), "m".into()]),
        ),
        (
            "invalid_target_cap",
            Box::new(|p| p.targets[0].max_sessions = 0),
        ),
        (
            "target_cap_exceeds_machine",
            Box::new(|p| p.targets[0].max_sessions = 4),
        ),
        (
            "target_cap_exceeds_machine",
            Box::new(|p| {
                p.targets[0].machine = Some(MachineLimits {
                    max_sessions: 1,
                    ..MachineLimits::default()
                });
            }),
        ),
        (
            "invalid_idle_minutes",
            Box::new(|p| p.wind_down.idle_minutes = 0),
        ),
        (
            "invalid_keep_warm",
            Box::new(|p| p.wind_down.keep_warm = 65),
        ),
        (
            "invalid_speculation_depth",
            Box::new(|p| p.speculation.max_depth = 0),
        ),
        (
            "invalid_speculation_depth",
            Box::new(|p| p.speculation.max_depth = 9),
        ),
        (
            "invalid_risk_weight",
            Box::new(|p| p.speculation.risk_weight = 101),
        ),
        (
            "invalid_opt_out_tag",
            Box::new(|p| p.speculation.opt_out_tag = "two words".into()),
        ),
    ];
    for (expected, change) in &cases {
        let mut body = policy();
        change(&mut body);
        assert!(codes(&body, None).contains(expected), "{expected}");
    }
    // A target override that raises the machine cap makes a larger target cap legal.
    let mut raised = policy();
    raised.targets[0].max_sessions = 6;
    raised.targets[0].machine = Some(MachineLimits {
        max_sessions: 8,
        ..MachineLimits::default()
    });
    assert!(codes(&raised, None).is_empty());
    // Several problems are all reported, each naming its subject.
    let mut broken = policy();
    broken.machine.max_cpu_percent = 0;
    broken.tools[0].max_sessions = 0;
    broken.targets[0].tools.push(scope("opencode", &[]));
    let problems = broken.problems(None);
    assert_eq!(
        problems.iter().map(|p| p.code).collect::<Vec<_>>(),
        [
            "invalid_cpu_percent",
            "invalid_tool_cap",
            "unconfigured_tool"
        ]
    );
    assert_eq!(problems[0].subject, None);
    assert_eq!(problems[1].subject.as_deref(), Some("claude"));
    assert_eq!(problems[2].subject.as_deref(), Some("opencode"));
    // Against the manifest, a target that is not actively declared is a conflict.
    let declared: BTreeSet<String> = [id(1)].into();
    assert!(codes(&policy(), Some(&declared)).is_empty());
    let other: BTreeSet<String> = [id(2)].into();
    assert_eq!(codes(&policy(), Some(&other)), ["undeclared_target"]);
    // `set` refuses the first problem and writes nothing.
    let root = tempfile::tempdir().unwrap();
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    assert!(matches!(
        PolicyStore::new(root.path()).set(
            SetPolicy {
                expected_revision: None,
                policy: policy()
            },
            Some(&other),
            &mut tx
        ),
        Err(OrchestrationError::Invalid(
            "policy names a target the manifest does not actively declare"
        ))
    ));
    assert!(!root.path().join(".hotsheet2").exists());
}

#[test]
fn hs2_xhq1eh_limits_only_sessions_need_a_listed_target_and_tool() {
    use PolicyDenial as D;
    use PolicyVerdict as V;
    let body = policy();
    let load = SessionLoad::default();
    // HS2-XHQ1EH: the policy holds limits only. Acting actions other than a session
    // start are not limited on an enabled or unlisted target; a person is asked
    // through the permission bridge instead.
    for action in [
        TargetAction::TicketsRead,
        TargetAction::TicketsWrite,
        TargetAction::WorkStart,
        TargetAction::GatesObserve,
        TargetAction::ValidationRun,
        TargetAction::DeploymentsRun,
    ] {
        assert_eq!(body.evaluate(&ask(action, None, None)), V::Allowed);
        let unlisted = PolicyQuestion {
            target_id: "00000000000000000000000002",
            ..ask(action, None, None)
        };
        assert_eq!(body.evaluate(&unlisted), V::Allowed);
    }
    // A session start needs the target listed: its tools and caps are the limits.
    let codex = start("codex", None);
    let unlisted = PolicyQuestion {
        target_id: "00000000000000000000000002",
        ..ask(TargetAction::SessionsManage, Some(&codex), Some(&load))
    };
    assert_eq!(body.evaluate(&unlisted), V::Denied(D::TargetNotPermitted));
    // A disabled target limits every acting action.
    let mut disabled = policy();
    disabled.targets[0].enabled = false;
    for action in [TargetAction::TicketsWrite, TargetAction::WorkStart] {
        assert_eq!(
            disabled.evaluate(&ask(action, None, None)),
            V::Denied(D::TargetDisabled)
        );
    }
    let sessions = |session: Option<&SessionRequest>, load: Option<&SessionLoad>| {
        body.evaluate(&ask(TargetAction::SessionsManage, session, load))
    };
    assert_eq!(
        sessions(Some(&start("claude", Some("model-a"))), Some(&load)),
        V::Allowed
    );
    assert_eq!(
        sessions(Some(&start("codex", None)), Some(&load)),
        V::Allowed
    );
    assert_eq!(
        sessions(Some(&start("codex", Some("any"))), Some(&load)),
        V::Allowed
    );
    assert_eq!(sessions(None, Some(&load)), V::Denied(D::ToolNotPermitted));
    assert_eq!(
        sessions(Some(&start("opencode", None)), Some(&load)),
        V::Denied(D::ToolNotPermitted)
    );
    assert_eq!(
        sessions(Some(&start("claude", Some("model-b"))), Some(&load)),
        V::Denied(D::ModelNotPermitted)
    );
    assert_eq!(
        sessions(Some(&start("claude", None)), Some(&load)),
        V::Denied(D::ModelNotPermitted)
    );
    // Without the current load, a start cannot be proven within any cap.
    assert_eq!(
        sessions(Some(&start("codex", None)), None),
        V::Denied(D::LoadUnknown)
    );
    // Stopping needs neither a load nor headroom.
    let stop = SessionRequest {
        starting: false,
        ..start("codex", None)
    };
    assert_eq!(sessions(Some(&stop), None), V::Allowed);
}

#[test]
fn hs2_xhq1eh_retired_actions_and_approval_read_ignored_and_reported() {
    let root = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(root.path());
    let directory = root.path().join(".hotsheet2");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("orchestration-policy.json");
    let old = serde_json::json!({
        "schema_version": 1,
        "revision": id(9),
        "policy": {
            "tools": [{"tool": "codex", "max_sessions": 1}],
            "targets": [{
                "target_id": id(1),
                "actions": ["tickets_read", "sessions_manage"],
                "tools": [{"tool": "codex"}]
            }],
            "approval": {"required_for": ["work_start", "session_start"]}
        }
    });
    std::fs::write(&path, serde_json::to_vec(&old).unwrap()).unwrap();
    let body = store.read().unwrap().policy.unwrap();
    assert!(body.problems(None).is_empty());
    assert_eq!(
        body.ignored_fields(),
        [
            IgnoredField {
                code: "approval_ignored",
                field: "approval",
                subject: None
            },
            IgnoredField {
                code: "target_actions_ignored",
                field: "targets[].actions",
                subject: Some(id(1))
            }
        ]
    );
    // Ignored means ignored: the old approval list asks nobody here.
    let load = SessionLoad::default();
    let codex = start("codex", None);
    assert_eq!(
        body.evaluate(&PolicyQuestion {
            target_id: &id(1),
            ..ask(TargetAction::SessionsManage, Some(&codex), Some(&load))
        }),
        PolicyVerdict::Allowed
    );
    // Writing the same limits back drops the retired fields from the file.
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let view = store
        .set(
            SetPolicy {
                expected_revision: Some(id(9)),
                policy: body.clone(),
            },
            None,
            &mut tx,
        )
        .unwrap();
    // Content equal but for the retired fields is a no-op on the limits, yet the file
    // still held them, so nothing about the limits changed.
    assert_eq!(view.revision.as_deref(), Some(id(9).as_str()));
    let mut changed = body.without_retired();
    changed.wind_down.idle_minutes = 20;
    store
        .set(
            SetPolicy {
                expected_revision: Some(id(9)),
                policy: changed,
            },
            None,
            &mut tx,
        )
        .unwrap();
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(!text.contains("required_for") && !text.contains("\"actions\""));
    assert!(
        store
            .read()
            .unwrap()
            .policy
            .unwrap()
            .ignored_fields()
            .is_empty()
    );
}

#[test]
fn hs2_ycjdeb_each_concurrency_cap_denies_exactly_at_its_bound() {
    use PolicyDenial as D;
    use PolicyVerdict as V;
    let mut body = policy();
    body.tools[1].accounts = vec![
        AccountPolicy {
            account: "team".into(),
            max_sessions: 1,
            budget_cents_per_day: None,
        },
        AccountPolicy {
            account: "spare".into(),
            max_sessions: 2,
            budget_cents_per_day: Some(1_000),
        },
    ];
    assert!(body.problems(None).is_empty());
    let codex = start("codex", None);
    let run = |load: SessionLoad| {
        body.evaluate(&ask(
            TargetAction::SessionsManage,
            Some(&codex),
            Some(&load),
        ))
    };
    let team = |target, machine, tool, account| SessionLoad {
        target_sessions: target,
        machine_sessions: machine,
        tool_sessions: tool,
        account: Some("team".into()),
        account_sessions: account,
    };
    // Caps: target 2, machine 3, tool 2, account "team" 1.
    assert_eq!(run(team(1, 2, 1, 0)), V::Allowed);
    assert_eq!(run(team(2, 2, 1, 0)), V::Denied(D::TargetCap));
    assert_eq!(run(team(1, 3, 1, 0)), V::Denied(D::MachineCap));
    assert_eq!(run(team(1, 2, 2, 0)), V::Denied(D::ToolCap));
    assert_eq!(run(team(1, 2, 1, 1)), V::Denied(D::AccountCap));
    // Another account of the same tool has its own headroom.
    assert_eq!(
        run(SessionLoad {
            account: Some("spare".into()),
            account_sessions: 1,
            ..team(1, 2, 1, 0)
        }),
        V::Allowed
    );
    // A tool with configured accounts runs only under one of them, and a tool
    // without accounts under none.
    for account in [None, Some("unknown".to_owned())] {
        assert_eq!(
            run(SessionLoad {
                account,
                ..team(0, 0, 0, 0)
            }),
            V::Denied(D::AccountNotPermitted)
        );
    }
    let claude = start("claude", Some("model-a"));
    assert_eq!(
        body.evaluate(&ask(
            TargetAction::SessionsManage,
            Some(&claude),
            Some(&team(0, 0, 0, 0))
        )),
        V::Denied(D::AccountNotPermitted)
    );
    // A target's machine override replaces the default machine cap for that target.
    body.targets[0].machine = Some(MachineLimits {
        max_sessions: 2,
        min_free_ram_mb: 8192,
        ..MachineLimits::default()
    });
    assert_eq!(
        body.evaluate(&ask(
            TargetAction::SessionsManage,
            Some(&codex),
            Some(&team(1, 2, 1, 0))
        )),
        V::Denied(D::MachineCap)
    );
    assert_eq!(body.machine_limits(&id(1)).min_free_ram_mb, 8192);
    assert_eq!(body.machine_limits(&id(2)).min_free_ram_mb, 2048);
}

#[test]
fn hs2_ycjdeb_speculation_settings() {
    use PolicyVerdict as V;
    // Speculation: off by default and switchable per target. It is a limit; whether
    // a person is asked is the permission bridge's business (HS2-XHQ1EH).
    let provisional = |body: &PolicyBody| {
        body.evaluate(&PolicyQuestion {
            provisional: true,
            ..ask(TargetAction::WorkStart, None, None)
        })
    };
    let mut body = policy();
    assert_eq!(
        provisional(&body),
        V::Denied(PolicyDenial::SpeculationNotAllowed)
    );
    assert!(!body.speculation_allowed(&id(1)));
    body.targets[0].speculation = Some(true);
    assert_eq!(provisional(&body), V::Allowed);
    body.targets[0].speculation = None;
    body.speculation.allowed = true;
    assert_eq!(provisional(&body), V::Allowed);
    body.targets[0].speculation = Some(false);
    assert_eq!(
        provisional(&body),
        V::Denied(PolicyDenial::SpeculationNotAllowed)
    );
}
