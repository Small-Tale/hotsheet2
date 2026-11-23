//! HS2-AHG7HW: tool capacity through the shared runtime and its one request router,
//! against the bundled plugins' own declarations. Synthetic accounts and times only.
use std::cell::Cell;

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    AccountPolicy, Applied, AuditOutcome, LimitSignal, MachineLimits, PolicyBody, SessionCount,
    SetPolicy, SignalKind, TargetPolicy, ToolCapacityRegistry, ToolPolicy, ToolScope,
};
use serde_json::{Value, json};

use super::*;

const MIN: u64 = 60 * 1000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;
const T0: u64 = 20_000 * DAY + 12 * HOUR;

struct At(Cell<u64>);
impl hotsheet_ticketing::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}
fn human() -> MutationActor {
    MutationActor::parse("human", Some("maintainer".into())).unwrap()
}
fn tool(name: &str, weight: u32, accounts: &[(&str, Option<u64>)]) -> ToolPolicy {
    ToolPolicy {
        tool: name.into(),
        max_sessions: Some(2),
        weight: Some(weight),
        accounts: accounts
            .iter()
            .map(|(account, budget)| AccountPolicy {
                account: (*account).into(),
                max_sessions: 1,
                budget_cents_per_day: *budget,
            })
            .collect(),
    }
}
/// Every bundled tool, allowed on the one declared target.
fn policy() -> PolicyBody {
    let tools = vec![
        tool("claude", 80, &[]),
        tool("codex", 60, &[]),
        tool("opencode", 90, &[]),
        tool("antigravity", 90, &[]),
    ];
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 4,
            ..MachineLimits::default()
        },
        targets: vec![TargetPolicy {
            target_id: id(1),
            enabled: true,
            actions: None,
            tools: tools
                .iter()
                .map(|tool| ToolScope {
                    tool: tool.tool.clone(),
                    models: Vec::new(),
                })
                .collect(),
            max_sessions: Some(2),
            machine: None,
            speculation: None,
        }],
        tools,
        ..PolicyBody::default()
    }
}
fn set(f: &Fixture, body: PolicyBody, tx: &mut Transaction<'_>) {
    let expected_revision = f.runtime.policy(&f.owner.id).unwrap().revision;
    f.runtime
        .set_policy(
            &f.owner.id,
            SetPolicy {
                expected_revision,
                policy: body,
            },
            Some(&human()),
            tx,
        )
        .unwrap();
}
fn signal(tool: &str, kind: SignalKind, at_ms: u64) -> LimitSignal {
    LimitSignal {
        tool: tool.into(),
        account: None,
        kind,
        at_ms,
        resets_at_ms: None,
        cost_cents: None,
        session: None,
    }
}
fn report(f: &Fixture, signal: LimitSignal, tx: &mut Transaction<'_>) -> Value {
    serde_json::to_value(
        f.runtime
            .tool_capacity_signal(&f.owner.id, signal, Some(&human()), tx)
            .unwrap(),
    )
    .unwrap()
}
/// `tool[/account]` to its `state[:reason]`.
fn states(f: &Fixture, tx: &mut Transaction<'_>) -> Vec<(String, String)> {
    let view = serde_json::to_value(f.runtime.tool_capacity(&f.owner.id, tx).unwrap()).unwrap();
    view["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| {
            let name = match row["account"].as_str() {
                Some(account) => format!("{}/{account}", row["tool"].as_str().unwrap()),
                None => row["tool"].as_str().unwrap().to_owned(),
            };
            let state = match row["reason"].as_str() {
                Some(reason) => format!("{}:{reason}", row["state"].as_str().unwrap()),
                None => row["state"].as_str().unwrap().to_owned(),
            };
            (name, state)
        })
        .collect()
}
fn state_of(f: &Fixture, name: &str, tx: &mut Transaction<'_>) -> String {
    states(f, tx)
        .into_iter()
        .find(|(tool, _)| tool == name)
        .unwrap()
        .1
}

#[test]
fn hs2_ahg7hw_each_plugins_declared_signals_drive_its_capacity_state() {
    let f = fixture();
    let clock = At(T0.into());
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    // No policy: nothing is configured, and nothing is written.
    let empty = f.runtime.tool_capacity(&f.owner.id, &mut tx).unwrap();
    assert!(!empty.enabled && empty.tools.is_empty());
    declaration(&f, &mut tx);
    set(&f, policy(), &mut tx);
    let view =
        serde_json::to_value(f.runtime.tool_capacity(&f.owner.id, &mut tx).unwrap()).unwrap();
    assert_eq!(view["enabled"], true);
    assert_eq!(view["now_ms"], T0);
    assert!(!f.home.join("orchestration-tool-capacity.json").exists());
    // What each bundled plugin can report, straight from its manifest.
    let declared: Vec<(String, Value)> = view["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|row| (row["tool"].as_str().unwrap().into(), row["signals"].clone()))
        .collect();
    assert_eq!(
        declared,
        [
            (
                "claude".to_owned(),
                json!({"limits":true,"reset_times":true,"usage":true,"cost":true})
            ),
            (
                "codex".to_owned(),
                json!({"limits":true,"reset_times":true,"usage":true,"cost":true})
            ),
            (
                "opencode".to_owned(),
                json!({"limits":false,"reset_times":false,"usage":true,"cost":false})
            ),
            (
                "antigravity".to_owned(),
                json!({"limits":true,"reset_times":false,"usage":false,"cost":false})
            ),
        ]
    );
    // Before anything is observed nothing is available: unobserved or unreportable.
    assert_eq!(
        states(&f, &mut tx),
        [
            ("claude".into(), "unknown:not_observed".into()),
            ("codex".into(), "unknown:not_observed".into()),
            ("opencode".into(), "unknown:not_reported".into()),
            ("antigravity".into(), "unknown:not_observed".into()),
        ]
    );

    // Claude reports a rate limit with its reset time.
    let limited = report(
        &f,
        LimitSignal {
            resets_at_ms: Some(T0 + 2 * HOUR),
            ..signal("claude", SignalKind::RateLimit, T0)
        },
        &mut tx,
    );
    assert_eq!(
        limited,
        json!({"applied":"applied","tool":"claude","state":"limited",
            "until_ms":T0 + 2 * HOUR,"reason":"rate_limit"})
    );
    // Codex reports a usage limit; this one states no reset time, so it backs off.
    let backoff = report(&f, signal("codex", SignalKind::UsageLimit, T0), &mut tx);
    assert_eq!(backoff["until_ms"], T0 + 5 * MIN);
    assert_eq!(backoff["reason"], "usage_limit");
    // OpenCode reports usage without a cost; Antigravity reports nothing.
    let unknown = report(&f, signal("opencode", SignalKind::Usage, T0), &mut tx);
    assert_eq!(
        (unknown["state"].as_str(), unknown["reason"].as_str()),
        (Some("unknown"), Some("not_reported"))
    );

    // The same report again changes nothing and is not audited twice.
    let audits = |f: &Fixture| f.runtime.audit(&f.owner.id, 0, 500).unwrap().entries;
    let recorded = audits(&f).len();
    let again = report(&f, signal("codex", SignalKind::UsageLimit, T0), &mut tx);
    assert_eq!(again["applied"], "duplicate");
    assert_eq!(again["until_ms"], T0 + 5 * MIN);
    assert_eq!(audits(&f).len(), recorded);
    let entry = audits(&f).pop().unwrap().record;
    assert_eq!(
        (
            entry.action.as_str(),
            entry.outcome,
            entry.reason.as_deref()
        ),
        (
            "tool_capacity_usage_limit",
            AuditOutcome::Changed,
            Some("limited")
        )
    );

    // Time passes: Codex's backoff ends unconfirmed, Claude's stated reset has not come.
    clock.0.set(T0 + 10 * MIN);
    assert_eq!(state_of(&f, "claude", &mut tx), "limited:rate_limit");
    assert_eq!(
        state_of(&f, "codex", &mut tx),
        "degraded:recovery_unconfirmed"
    );
    // A completed Codex turn confirms the recovery.
    let confirmed = report(
        &f,
        LimitSignal {
            cost_cents: Some(12),
            ..signal("codex", SignalKind::Usage, T0 + 10 * MIN)
        },
        &mut tx,
    );
    assert_eq!(confirmed["state"], "available");
    // A stale recovery for Claude (older than its limit) is ignored.
    let stale = report(
        &f,
        signal("claude", SignalKind::Recovered, T0 - MIN),
        &mut tx,
    );
    assert_eq!(
        (stale["applied"].as_str(), stale["state"].as_str()),
        (Some("stale"), Some("limited"))
    );
    // After Claude's reset time it is available without another signal.
    clock.0.set(T0 + 2 * HOUR);
    assert_eq!(
        states(&f, &mut tx),
        [
            ("claude".into(), "available".into()),
            ("codex".into(), "available".into()),
            ("opencode".into(), "unknown:not_reported".into()),
            ("antigravity".into(), "unknown:not_observed".into()),
        ]
    );
    let view =
        serde_json::to_value(f.runtime.tool_capacity(&f.owner.id, &mut tx).unwrap()).unwrap();
    assert_eq!(view["tools"][1]["spent_cents_today"], 12);
    assert_eq!(view["tools"][1]["verified"], true);
    assert_eq!(view["tools"][1]["last_ok_at_ms"], T0 + 10 * MIN);
    assert_eq!(view["tools"][2]["verified"], false);
    assert!(view["tools"][2].get("spent_cents_today").is_none());
    // The registry holds identifiers and numbers only.
    let stored = std::fs::read_to_string(f.home.join("orchestration-tool-capacity.json")).unwrap();
    let stored: Value = serde_json::from_str(&stored).unwrap();
    fn strings(value: &Value, found: &mut Vec<String>) {
        match value {
            Value::String(text) => found.push(text.clone()),
            Value::Array(items) => items.iter().for_each(|item| strings(item, found)),
            Value::Object(map) => map.values().for_each(|item| strings(item, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    strings(&stored, &mut found);
    found.sort();
    found.dedup();
    assert_eq!(
        found,
        [
            "claude",
            "codex",
            "opencode",
            "rate_limit",
            "usage",
            "usage_limit"
        ]
    );
}

#[test]
fn hs2_ahg7hw_selection_fails_over_and_keeps_accounts_and_budgets_apart() {
    let f = fixture();
    let clock = At(T0.into());
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    declaration(&f, &mut tx);
    let mut body = policy();
    body.tools[0] = tool("claude", 80, &[("team-a", Some(500)), ("team-b", None)]);
    set(&f, body, &mut tx);
    let on = |account: &str, kind, at_ms, cost_cents| LimitSignal {
        account: Some(account.into()),
        cost_cents,
        ..signal("claude", kind, at_ms)
    };
    for account in ["team-a", "team-b"] {
        report(&f, on(account, SignalKind::Usage, T0, Some(100)), &mut tx);
    }
    report(
        &f,
        LimitSignal {
            cost_cents: Some(1),
            ..signal("codex", SignalKind::Usage, T0)
        },
        &mut tx,
    );
    let choose = |tool: Option<&str>, load: Vec<SessionCount>, tx: &mut Transaction<'_>| {
        serde_json::to_value(
            f.runtime
                .tool_select(
                    &f.owner.id,
                    SelectTool {
                        target_id: id(1),
                        tool: tool.map(Into::into),
                        model: None,
                        load,
                    },
                    tx,
                )
                .unwrap(),
        )
        .unwrap()
    };
    let order = |selection: &Value| -> Vec<String> {
        selection["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| {
                format!(
                    "{}/{}:{}",
                    c["tool"].as_str().unwrap(),
                    c["account"].as_str().unwrap_or("-"),
                    c["state"].as_str().unwrap()
                )
            })
            .collect()
    };
    // Verified tools by weight; the tools that cannot report rank last, unverified.
    let selection = choose(None, Vec::new(), &mut tx);
    assert_eq!(selection["now_ms"], T0);
    assert!(selection.get("denied").is_none());
    assert_eq!(
        order(&selection),
        [
            "claude/team-a:available",
            "claude/team-b:available",
            "codex/-:available",
            "antigravity/-:unknown",
            "opencode/-:unknown",
        ]
    );
    assert_eq!(selection["candidates"][3]["verified"], false);

    // One account is limited: only that account is excluded.
    let limited = report(
        &f,
        on("team-a", SignalKind::RateLimit, T0 + MIN, None),
        &mut tx,
    );
    assert_eq!(limited["account"], "team-a");
    assert_eq!(state_of(&f, "claude/team-b", &mut tx), "available");
    let selection = choose(Some("claude"), Vec::new(), &mut tx);
    assert_eq!(order(&selection)[0], "claude/team-b:available");
    assert_eq!(
        selection["excluded"],
        json!([{"tool":"claude","account":"team-a","excluded":"limited","state":"limited",
            "until_ms":T0 + 6 * MIN,"reason":"rate_limit"}])
    );
    // The other account is at its cap: the requested tool fails over to Codex.
    let load = vec![SessionCount {
        tool: "claude".into(),
        account: Some("team-b".into()),
        sessions: 1,
    }];
    let selection = choose(Some("claude"), load, &mut tx);
    assert_eq!(order(&selection)[0], "codex/-:available");
    assert_eq!(selection["candidates"][0]["requested"], false);
    assert_eq!(selection["excluded"][1]["excluded"], "account_cap");

    // A person clears the limit; the budget then decides for that account alone.
    report(
        &f,
        on("team-a", SignalKind::Recovered, T0 + 2 * MIN, None),
        &mut tx,
    );
    assert_eq!(state_of(&f, "claude/team-a", &mut tx), "available");
    report(
        &f,
        on("team-a", SignalKind::Usage, T0 + 3 * MIN, Some(400)),
        &mut tx,
    );
    clock.0.set(T0 + 4 * MIN);
    assert_eq!(
        state_of(&f, "claude/team-a", &mut tx),
        "limited:budget_exceeded"
    );
    assert_eq!(state_of(&f, "claude/team-b", &mut tx), "available");
    let view =
        serde_json::to_value(f.runtime.tool_capacity(&f.owner.id, &mut tx).unwrap()).unwrap();
    assert_eq!(view["tools"][0]["until_ms"], 20_001 * DAY);
    assert_eq!(view["tools"][0]["spent_cents_today"], 500);
    assert_eq!(view["tools"][0]["budget_cents_per_day"], 500);
    // A turn without a cost on the next day makes that day's budget unknowable.
    clock.0.set(T0 + DAY);
    assert_eq!(state_of(&f, "claude/team-a", &mut tx), "available");
    report(&f, on("team-a", SignalKind::Usage, T0 + DAY, None), &mut tx);
    assert_eq!(
        state_of(&f, "claude/team-a", &mut tx),
        "unknown:budget_unknown"
    );
    assert_eq!(state_of(&f, "claude/team-b", &mut tx), "available");

    // A limit nobody can attribute makes each account a probe, never a limit.
    report(
        &f,
        signal("claude", SignalKind::UsageLimit, T0 + DAY),
        &mut tx,
    );
    assert_eq!(
        state_of(&f, "claude/team-b", &mut tx),
        "degraded:unattributed_limit"
    );
    assert_eq!(
        order(&choose(None, Vec::new(), &mut tx))[0],
        "codex/-:available"
    );

    // A target outside the policy is refused with the policy's own code.
    let denied = serde_json::to_value(
        f.runtime
            .tool_select(
                &f.owner.id,
                SelectTool {
                    target_id: id(2),
                    tool: None,
                    model: None,
                    load: Vec::new(),
                },
                &mut tx,
            )
            .unwrap(),
    )
    .unwrap();
    // HS2-N5BDE4: a project the policy does not list is Auto, not refused.
    assert!(denied["denied"].is_null());
}

#[test]
fn hs2_ahg7hw_signals_are_refused_closed_and_routed_like_every_other_adapter() {
    let f = fixture();
    let clock = At(T0.into());
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let base = format!("/checkouts/{}/orchestration/tool-capacity", f.owner.id);
    for leaf in ["", "/signals", "/select"] {
        let path = format!("{base}{leaf}");
        assert!(is_path(&path) && is_machine_path(&path), "{path}");
        assert!(!is_cleanup_request("GET", &path));
    }
    assert!(!is_path(&format!("{base}/other")));
    let limit = json!({"tool":"codex","kind":"usage_limit","at_ms":T0});
    // Without a policy there is nothing to report against or select from.
    for (leaf, body) in [
        ("signals", limit.clone()),
        ("select", json!({"target_id":id(1),"load":[]})),
    ] {
        let error = f
            .runtime
            .request("POST", &format!("{base}/{leaf}"), &body, &mut tx)
            .unwrap_err();
        assert_eq!(error.code(), "invalid_request", "{leaf}");
    }
    declaration(&f, &mut tx);
    let mut body = policy();
    body.tools.truncate(2);
    body.targets[0].tools.truncate(2);
    body.tools[0] = tool("claude", 80, &[("team-a", None)]);
    set(&f, body, &mut tx);

    let post = |leaf: &str, body: Value, tx: &mut Transaction<'_>| {
        f.runtime
            .request("POST", &format!("{base}/{leaf}"), &body, tx)
    };
    let applied = post("signals", limit.clone(), &mut tx).unwrap();
    assert_eq!(applied["applied"], "applied");
    assert_eq!(applied["state"], "limited");
    let shown = f
        .runtime
        .request("GET", &base, &Value::Null, &mut tx)
        .unwrap();
    assert_eq!(shown["tools"][1]["state"], "limited");
    let selected = post(
        "select",
        json!({"target_id":id(1),"tool":"codex","load":[{"tool":"claude","sessions":0}]}),
        &mut tx,
    )
    .unwrap();
    assert_eq!(selected["candidates"][0]["tool"], "claude");
    assert_eq!(selected["candidates"][0]["account"], "team-a");
    assert_eq!(selected["excluded"][0]["excluded"], "limited");

    // Refusals leave the registry byte-identical.
    let file = f.home.join("orchestration-tool-capacity.json");
    let before = std::fs::read(&file).unwrap();
    let ai = json!({"role":"ai","id":"worker-1"});
    let refused: Vec<(&str, Value, &str)> = vec![
        // The session a limit holds back cannot lift it.
        (
            "signals",
            json!({"tool":"codex","kind":"recovered","at_ms":T0,"actor":ai}),
            "forbidden",
        ),
        (
            "signals",
            // HS2-N3HWP9: an unlisted tool is Auto and takes signals; a malformed one does not.
            json!({"tool":"has space","kind":"usage","at_ms":T0}),
            "invalid_request",
        ),
        (
            "signals",
            json!({"tool":"claude","account":"team-z","kind":"usage","at_ms":T0}),
            "invalid_request",
        ),
        (
            "signals",
            json!({"tool":"codex","kind":"usage_limit","at_ms":T0,"message":"synthetic"}),
            "invalid_request",
        ),
        (
            "signals",
            json!({"tool":"codex","kind":"paused","at_ms":T0}),
            "invalid_request",
        ),
        (
            "signals",
            json!({"tool":"codex","kind":"usage_limit","at_ms":T0 + DAY}),
            "invalid_request",
        ),
        ("select", json!({"target_id":id(1)}), "invalid_request"),
        (
            "select",
            json!({"target_id":"not-a-ulid","load":[]}),
            "invalid_request",
        ),
        (
            "select",
            json!({"target_id":id(1),"load":[],"extra":1}),
            "invalid_request",
        ),
    ];
    for (leaf, body, code) in refused {
        let error = post(leaf, body.clone(), &mut tx).unwrap_err();
        assert_eq!(error.code(), code, "{leaf} {body}");
    }
    assert_eq!(std::fs::read(&file).unwrap(), before);
    // An AI session may report a limit or usage; a person or the system may clear.
    let mut reported = limit.clone();
    reported["at_ms"] = json!(T0 + MIN);
    reported["actor"] = ai;
    assert_eq!(
        post("signals", reported, &mut tx).unwrap()["applied"],
        "applied"
    );
    for (role, at) in [("system", T0 + 2 * MIN), ("human", T0 + 3 * MIN)] {
        let cleared = post(
            "signals",
            json!({"tool":"codex","kind":"recovered","at_ms":at,"actor":{"role":role,"id":"m"}}),
            &mut tx,
        )
        .unwrap();
        assert_eq!(
            (cleared["applied"].as_str(), cleared["state"].as_str()),
            (Some("applied"), Some("available"))
        );
    }

    // The server's own detection records without a project, for loaded plugins only.
    let detected = signal("codex", SignalKind::UsageLimit, T0 + 4 * MIN);
    assert_eq!(
        OrchestrationRuntime::record_tool_signal_at(&f.home, &detected, T0 + 4 * MIN).unwrap(),
        Applied::Applied
    );
    assert_eq!(
        OrchestrationRuntime::record_tool_signal_at(&f.home, &detected, T0 + 4 * MIN).unwrap(),
        Applied::Duplicate
    );
    let unknown_tool = signal("no-such-tool", SignalKind::UsageLimit, T0);
    assert_eq!(
        OrchestrationRuntime::record_tool_signal_at(&f.home, &unknown_tool, T0)
            .unwrap_err()
            .code(),
        "invalid_request"
    );
    assert_eq!(
        f.runtime
            .request("GET", &base, &Value::Null, &mut tx)
            .unwrap()["tools"][1]["state"],
        "limited"
    );

    // Corrupt or newer data fails closed on every route and is never rewritten.
    for (bytes, code) in [
        ("corrupt", "corrupt_orchestration_data"),
        (r#"{"schema_version":2,"entries":[]}"#, "upgrade_required"),
    ] {
        std::fs::write(&file, bytes).unwrap();
        assert_eq!(
            f.runtime
                .request("GET", &base, &Value::Null, &mut tx)
                .unwrap_err()
                .code(),
            code
        );
        assert_eq!(
            post("signals", limit.clone(), &mut tx).unwrap_err().code(),
            code
        );
        assert_eq!(
            post("select", json!({"target_id":id(1),"load":[]}), &mut tx)
                .unwrap_err()
                .code(),
            code
        );
        assert_eq!(
            OrchestrationRuntime::record_tool_signal_at(&f.home, &detected, T0 + 4 * MIN)
                .unwrap_err()
                .code(),
            code
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), bytes);
        assert!(ToolCapacityRegistry::new(&f.home).entries().is_err());
    }
}

/// HS2-XFMKHQ: usage arrives with every turn, so it wakes the scheduler only when it
/// changes the account's standing (a first completed turn, a confirmed recovery), never
/// when it only adds cost; limits and recoveries always wake it.
#[test]
fn hs2_xfmkhq_usage_wakes_the_scheduler_only_when_the_standing_changes() {
    use hotsheet_ticketing::orchestration::{LimitSignal, SignalKind};
    let home = tempfile::tempdir().unwrap();
    let marker = home
        .path()
        .join("orchestration-scheduler-wake")
        .join("token");
    let token = || std::fs::read_to_string(&marker).ok();
    let signal = |kind: SignalKind, at_ms: u64, cost: Option<u64>| LimitSignal {
        tool: "claude".into(),
        account: Some("team-a".into()),
        kind,
        at_ms,
        resets_at_ms: None,
        cost_cents: cost,
        session: Some(format!("s{at_ms}")),
    };
    let record = |signal: &LimitSignal| {
        OrchestrationRuntime::record_tool_signal_at(home.path(), signal, 10_000_000).unwrap()
    };
    // The first completed turn: not observed to available.
    record(&signal(SignalKind::Usage, 1_000_000, Some(5)));
    let first = token().expect("a first turn changes the standing");
    // More turns only add cost.
    for at in [1_100_000, 1_200_000] {
        record(&signal(SignalKind::Usage, at, Some(5)));
        assert_eq!(token().as_deref(), Some(first.as_str()));
    }
    // A limit wakes it.
    record(&signal(SignalKind::UsageLimit, 1_300_000, None));
    let limited = token().unwrap();
    assert_ne!(limited, first);
    // A turn after the guessed backoff ended confirms the recovery: a standing change.
    record(&signal(SignalKind::Usage, 1_300_000 + 6 * 60_000, Some(5)));
    let confirmed = token().unwrap();
    assert_ne!(confirmed, limited);
    // Repeated: the identical signal changes nothing and wakes nothing.
    record(&signal(SignalKind::Usage, 1_300_000 + 6 * 60_000, Some(5)));
    assert_eq!(token(), Some(confirmed));
}
