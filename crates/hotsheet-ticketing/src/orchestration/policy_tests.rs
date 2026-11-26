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
        max_sessions: Some(max_sessions),
        weight: Some(50),
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
        max_sessions: Some(2),
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
            Box::new(|p| p.tools[1].max_sessions = Some(65)),
        ),
        (
            "invalid_tool_weight",
            Box::new(|p| p.tools[1].weight = Some(101)),
        ),
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
            "invalid_tool",
            Box::new(|p| p.targets[0].tools.push(scope("has space", &[]))),
        ),
        (
            "invalid_models",
            Box::new(|p| p.targets[0].tools[0].models = vec!["m".into(), "m".into()]),
        ),
        (
            "invalid_target_cap",
            Box::new(|p| p.targets[0].max_sessions = Some(65)),
        ),
        (
            "target_cap_exceeds_machine",
            Box::new(|p| p.targets[0].max_sessions = Some(4)),
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
    raised.targets[0].max_sessions = Some(6);
    raised.targets[0].machine = Some(MachineLimits {
        max_sessions: 8,
        ..MachineLimits::default()
    });
    assert!(codes(&raised, None).is_empty());
    // Several problems are all reported, each naming its subject.
    let mut broken = policy();
    broken.machine.max_cpu_percent = 0;
    broken.tools[0].max_sessions = Some(65);
    broken.targets[0].tools.push(scope("has space", &[]));
    let problems = broken.problems(None);
    assert_eq!(
        problems.iter().map(|p| p.code).collect::<Vec<_>>(),
        ["invalid_cpu_percent", "invalid_tool_cap", "invalid_tool"]
    );
    assert_eq!(problems[0].subject, None);
    assert_eq!(problems[1].subject.as_deref(), Some("claude"));
    assert_eq!(problems[2].subject.as_deref(), Some("has space"));
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
    // HS2-N5BDE4: an unlisted target is Auto: any tool the policy does not turn off,
    // any model, up to the machine's sessions limit.
    let codex = start("codex", None);
    let unlisted = PolicyQuestion {
        target_id: "00000000000000000000000002",
        ..ask(TargetAction::SessionsManage, Some(&codex), Some(&load))
    };
    assert_eq!(body.evaluate(&unlisted), V::Allowed);
    let any_model = start("opencode", Some("any"));
    assert_eq!(
        body.evaluate(&PolicyQuestion {
            session: Some(&any_model),
            ..unlisted
        }),
        V::Allowed
    );
    let full = SessionLoad {
        target_sessions: 3,
        machine_sessions: 0,
        ..SessionLoad::default()
    };
    assert_eq!(
        body.evaluate(&PolicyQuestion {
            load: Some(&full),
            ..unlisted
        }),
        V::Denied(D::TargetCap)
    );
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
        integrator: false,
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

#[test]
fn hs2_n3hwp9_tool_sessions_and_weight_read_auto_zero_or_a_number() {
    let parse = |json: &str| serde_json::from_str::<ToolPolicy>(json);
    // Absent, null and "auto" (any case) are Auto; numbers are kept, 0 included.
    for json in [
        r#"{"tool":"codex"}"#,
        r#"{"tool":"codex","max_sessions":null,"weight":null}"#,
        r#"{"tool":"codex","max_sessions":"auto","weight":"Auto"}"#,
    ] {
        let tool = parse(json).unwrap();
        assert_eq!((tool.max_sessions, tool.weight), (None, None), "{json}");
        assert!(!tool.is_off());
        assert_eq!(tool.session_cap(), MAX_POLICY_SESSIONS);
        assert_eq!(tool.effective_weight(), AUTO_TOOL_WEIGHT);
        // Auto is written by leaving the field out.
        assert_eq!(serde_json::to_string(&tool).unwrap(), r#"{"tool":"codex"}"#);
    }
    let explicit = parse(r#"{"tool":"codex","max_sessions":3,"weight":70}"#).unwrap();
    assert_eq!(
        (explicit.max_sessions, explicit.weight),
        (Some(3), Some(70))
    );
    assert_eq!(
        (explicit.session_cap(), explicit.effective_weight()),
        (3, 70)
    );
    assert!(!explicit.is_off());
    for off in [
        r#"{"tool":"codex","max_sessions":0}"#,
        r#"{"tool":"codex","weight":0}"#,
        r#"{"tool":"codex","max_sessions":4,"weight":0}"#,
    ] {
        assert!(parse(off).unwrap().is_off(), "{off}");
    }
    for bad in [
        r#"{"tool":"codex","max_sessions":"many"}"#,
        r#"{"tool":"codex","weight":-1}"#,
        r#"{"tool":"codex","weight":1.5}"#,
    ] {
        assert!(parse(bad).is_err(), "{bad}");
    }
    // 0 is a valid value now; only the upper bounds are problems.
    let mut body = policy();
    body.tools[0].max_sessions = Some(0);
    body.tools[1].weight = Some(0);
    assert!(codes(&body, None).is_empty());
    body.tools[0].max_sessions = Some(64);
    body.tools[1].weight = Some(100);
    assert!(codes(&body, None).is_empty());
    // An account under an Auto or off tool is bounded only by itself.
    body.tools[1].max_sessions = None;
    body.tools[1].accounts.push(AccountPolicy {
        account: "team".into(),
        max_sessions: 9,
        budget_cents_per_day: None,
    });
    assert!(codes(&body, None).is_empty());
    body.tools[1].max_sessions = Some(0);
    assert!(codes(&body, None).is_empty());
}

#[test]
fn hs2_n3hwp9_existing_policy_files_read_unchanged_and_unlisted_tools_are_auto() {
    let root = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(root.path());
    let directory = root.path().join(".hotsheet2");
    std::fs::create_dir(&directory).unwrap();
    let path = directory.join("orchestration-policy.json");
    // A file written before Auto: explicit numbers and an empty-handed `tools: []` era
    // tool left out. Existing caps and weights keep their meaning.
    std::fs::write(
        &path,
        format!(
            r#"{{"schema_version":1,"revision":"{}","policy":{{"tools":[{{"tool":"claude","max_sessions":2,"weight":50}}],"targets":[{{"target_id":"{}","tools":[{{"tool":"claude"}},{{"tool":"codex"}}]}}]}}}}"#,
            id(9),
            id(1)
        ),
    )
    .unwrap();
    let body = store.read().unwrap().policy.unwrap();
    assert!(body.problems(None).is_empty());
    let claude = body.tool_policy("claude");
    assert_eq!((claude.max_sessions, claude.weight), (Some(2), Some(50)));
    // The unlisted tool is Auto, not "do not start".
    let codex = body.tool_policy("codex");
    assert_eq!(codex, ToolPolicy::auto("codex"));
    // `"tools": []` makes every tool Auto.
    std::fs::write(
        &path,
        format!(
            r#"{{"schema_version":1,"revision":"{}","policy":{{"tools":[]}}}}"#,
            id(9)
        ),
    )
    .unwrap();
    let empty = store.read().unwrap().policy.unwrap();
    assert!(empty.tools.is_empty());
    assert!(!empty.tool_policy("claude").is_off());
}

#[test]
fn hs2_n3hwp9_admission_treats_auto_as_machine_bounded_and_zero_as_off() {
    use PolicyDenial as D;
    use PolicyVerdict as V;
    let load = |tool| SessionLoad {
        target_sessions: 0,
        machine_sessions: 0,
        tool_sessions: tool,
        account: None,
        account_sessions: 0,
        integrator: false,
    };
    let mut body = policy();
    body.tools.clear();
    body.targets[0].max_sessions = Some(64);
    body.machine.max_sessions = 64;
    let codex = start("codex", None);
    let run = |body: &PolicyBody, load: SessionLoad| {
        body.evaluate(&ask(
            TargetAction::SessionsManage,
            Some(&codex),
            Some(&load),
        ))
    };
    // Auto: an unlisted tool starts; only the policy-wide bound caps it.
    assert_eq!(run(&body, load(10)), V::Allowed);
    assert_eq!(run(&body, load(63)), V::Allowed);
    assert_eq!(run(&body, load(64)), V::Denied(D::ToolCap));
    // The machine limit binds an Auto tool.
    body.machine.max_sessions = 3;
    assert_eq!(
        run(
            &body,
            SessionLoad {
                machine_sessions: 3,
                ..load(0)
            }
        ),
        V::Denied(D::MachineCap)
    );
    // Sessions 0 or weight 0 turn the tool off.
    for off in [
        ToolPolicy {
            max_sessions: Some(0),
            ..ToolPolicy::auto("codex")
        },
        ToolPolicy {
            weight: Some(0),
            ..ToolPolicy::auto("codex")
        },
    ] {
        body.tools = vec![off];
        assert_eq!(run(&body, load(0)), V::Denied(D::ToolNotPermitted));
    }
    // An explicit number is still a cap.
    body.tools = vec![tool("codex", 2)];
    assert_eq!(run(&body, load(1)), V::Allowed);
    assert_eq!(run(&body, load(2)), V::Denied(D::ToolCap));
}

/// HS2-1X1TXC: a target's models may be patterns. `*` matches any run of characters;
/// a start names the first exact model, or none when every entry is a pattern.
#[test]
fn hs2_1x1txc_model_patterns_allow_matching_models_and_pick_the_start_model() {
    let scope = |models: &[&str]| scope("claude", models);
    let any = scope(&[]);
    assert!(any.allows(None) && any.allows(Some("model-a")));
    assert_eq!(any.start_model(), None);

    let exact = scope(&["model-a"]);
    assert!(exact.allows(Some("model-a")));
    assert!(!exact.allows(Some("model-a2")) && !exact.allows(None));
    assert_eq!(exact.start_model().as_deref(), Some("model-a"));

    let family = scope(&["opus-*"]);
    for allowed in ["opus-", "opus-4", "opus-4-2"] {
        assert!(family.allows(Some(allowed)), "{allowed}");
    }
    for refused in ["sonnet-4", "xopus-4", "opus"] {
        assert!(!family.allows(Some(refused)), "{refused}");
    }
    // Only patterns: the start names no model and the tool's own default runs.
    assert!(family.allows(None));
    assert_eq!(family.start_model(), None);

    let mixed = scope(&["*-mini", "model-b", "gpt-*-codex"]);
    assert!(mixed.allows(Some("gpt-5-mini")));
    assert!(mixed.allows(Some("gpt-5.1-codex")));
    assert!(!mixed.allows(Some("gpt-5")));
    // An exact entry is listed, so a session must name a model.
    assert!(!mixed.allows(None));
    assert_eq!(mixed.start_model().as_deref(), Some("model-b"));

    // The policy check and validation accept patterns like names.
    let mut body = policy();
    body.targets[0].tools = vec![scope(&["opus-*"])];
    assert!(
        body.problems(None)
            .iter()
            .all(|problem| problem.code != "invalid_models"),
        "{:?}",
        body.problems(None)
    );
    let ask = |model: Option<&str>| {
        let session = SessionRequest {
            tool: "claude".into(),
            model: model.map(str::to_owned),
            running: 0,
            starting: false,
        };
        body.evaluate(&PolicyQuestion {
            target_id: &id(1),
            action: TargetAction::SessionsManage,
            session: Some(&session),
            load: None,
            provisional: false,
            role_session: false,
        })
    };
    assert_eq!(ask(Some("opus-4-2")), PolicyVerdict::Allowed);
    assert_eq!(
        ask(Some("sonnet-4")),
        PolicyVerdict::Denied(PolicyDenial::ModelNotPermitted)
    );
}

/// HS2-0VTQFF: a start resolves a pattern-only list to the first model the host
/// reports that a pattern matches (the catalog default first, then the catalog's own
/// newest-first order). Exact and mixed lists keep their first exact model; an
/// unreported catalog or one with no match resolves to nothing, with a stable code.
#[test]
fn hs2_0vtqff_a_pattern_only_list_resolves_to_the_newest_reported_model() {
    let scope = |models: &[&str]| scope("claude", models);
    let reported = |models: &[&str]| models.iter().map(|m| (*m).to_owned()).collect::<Vec<_>>();
    let catalog = reported(&[
        "model-sonnet-5",
        "model-opus-5",
        "model-opus-4",
        "model-haiku-4",
    ]);

    // Patterns only, with matches: the first reported match, not the tool default.
    let family = scope(&["model-opus-*"]);
    assert_eq!(
        family.resolve_start_model(Some(&catalog)),
        Ok(Some("model-opus-5".into()))
    );
    // Two patterns: the catalog's order decides, not the order of the patterns.
    let two = scope(&["model-haiku-*", "model-opus-*"]);
    assert_eq!(
        two.resolve_start_model(Some(&catalog)),
        Ok(Some("model-opus-5".into()))
    );
    // The catalog default is listed first, so it wins whenever it matches.
    let sonnet_default = reported(&["model-opus-4", "model-opus-5"]);
    assert_eq!(
        family.resolve_start_model(Some(&sonnet_default)),
        Ok(Some("model-opus-4".into()))
    );
    // Reported, no match: refused with its code.
    assert_eq!(
        scope(&["gpt-*"]).resolve_start_model(Some(&catalog)),
        Err(ModelUnresolved::NoMatch)
    );
    assert_eq!(ModelUnresolved::NoMatch.code(), "no_matching_model");
    // Reported empty is reported: still no match.
    assert_eq!(
        family.resolve_start_model(Some(&[])),
        Err(ModelUnresolved::NoMatch)
    );
    // Unknown catalog: nothing starts on a guess.
    assert_eq!(
        family.resolve_start_model(None),
        Err(ModelUnresolved::CatalogUnknown)
    );
    assert_eq!(ModelUnresolved::CatalogUnknown.code(), "model_unresolved");

    // Exact lists are unchanged, with or without a catalog, even one without them.
    let exact = scope(&["model-a"]);
    for reported in [None, Some(&catalog[..]), Some(&[][..])] {
        assert_eq!(
            exact.resolve_start_model(reported),
            Ok(Some("model-a".into()))
        );
    }
    // Mixed lists keep their first exact model.
    let mixed = scope(&["model-opus-*", "model-b"]);
    assert_eq!(
        mixed.resolve_start_model(Some(&catalog)),
        Ok(Some("model-b".into()))
    );
    assert_eq!(mixed.resolve_start_model(None), Ok(Some("model-b".into())));
    // No models: any model, so the tool's own default, catalog or not.
    assert_eq!(scope(&[]).resolve_start_model(None), Ok(None));
    assert_eq!(scope(&[]).resolve_start_model(Some(&catalog)), Ok(None));
    // A resolved model always passes the policy's own check.
    let resolved = family.resolve_start_model(Some(&catalog)).unwrap();
    assert!(family.allows(resolved.as_deref()));
}

/// HS2-ZF5H5M: the first start is metadata beside the shared policy: absent until a
/// change records it, written in the same write as that change, kept by every later
/// change (with or without a time, from any writer), left out of what the policy
/// answers, and absent from a file written before it existed.
#[test]
fn hs2_zf5h5m_the_first_start_is_kept_beside_the_policy() {
    let root = tempfile::tempdir().unwrap();
    let store = PolicyStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let path = root.path().join(".hotsheet2/orchestration-policy.json");
    let on_disk =
        || -> serde_json::Value { serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap() };
    // Absent file: nothing recorded, and a read creates nothing.
    assert_eq!(store.started_at().unwrap(), None);
    let set = |store: &PolicyStore,
               revision: Option<String>,
               body: PolicyBody,
               at: Option<u64>,
               tx: &mut Transaction<'_>| {
        store
            .set_started(
                SetPolicy {
                    expected_revision: revision,
                    policy: body,
                },
                None,
                at,
                tx,
            )
            .unwrap()
    };
    let first = set(&store, None, policy(), None, &mut tx);
    assert_eq!(store.started_at().unwrap(), None);
    assert!(on_disk().get("placement_started_at_ms").is_none());
    // A change that records it, in the same write that rotates the revision.
    let mut changed = policy();
    changed.speculation.allowed = !changed.speculation.allowed;
    let second = set(
        &store,
        first.revision.clone(),
        changed.clone(),
        Some(500),
        &mut tx,
    );
    assert_ne!(second.revision, first.revision);
    assert_eq!(store.started_at().unwrap(), Some(500));
    assert_eq!(on_disk()["placement_started_at_ms"], 500);
    // Kept by later changes: with no time, with a later time, and by plain `set`.
    let third = set(&store, second.revision.clone(), policy(), None, &mut tx);
    let fourth = set(
        &store,
        third.revision.clone(),
        changed.clone(),
        Some(900),
        &mut tx,
    );
    let fifth = store
        .set(
            SetPolicy {
                expected_revision: fourth.revision.clone(),
                policy: policy(),
            },
            None,
            &mut tx,
        )
        .unwrap();
    assert_eq!(store.started_at().unwrap(), Some(500));
    // An identical policy is still a no-op that writes nothing.
    let bytes = std::fs::read(&path).unwrap();
    let same = set(&store, fifth.revision.clone(), policy(), Some(1), &mut tx);
    assert_eq!(same.revision, fifth.revision);
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    // Not part of what the policy answers.
    let answered = serde_json::to_value(store.read().unwrap()).unwrap();
    assert!(!answered.to_string().contains("placement_started_at_ms"));
    // A file written before the record existed reads, with nothing recorded.
    let mut older = on_disk();
    older
        .as_object_mut()
        .unwrap()
        .remove("placement_started_at_ms");
    std::fs::write(&path, serde_json::to_vec(&older).unwrap()).unwrap();
    assert!(store.read().unwrap().enabled);
    assert_eq!(store.started_at().unwrap(), None);
}
