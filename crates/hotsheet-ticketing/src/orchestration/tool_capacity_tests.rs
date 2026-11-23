//! HS2-AHG7HW tool capacity: transitions, backoff, budget, selection and the store.
//! Synthetic tools, accounts and homes only; every time is an argument.

use super::*;

const MIN: u64 = 60 * 1000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;
/// Noon UTC on day 20 000.
const T0: u64 = 20_000 * DAY + 12 * HOUR;

const FULL: SignalSupport = SignalSupport {
    limits: true,
    reset_times: true,
    usage: true,
    cost: true,
};
const NONE: SignalSupport = SignalSupport {
    limits: false,
    reset_times: false,
    usage: false,
    cost: false,
};

type Reading = (&'static str, Option<CapacityReason>, Option<u64>);

fn signal(kind: SignalKind, at_ms: u64) -> LimitSignal {
    LimitSignal {
        tool: "alpha".into(),
        account: None,
        kind,
        at_ms,
        resets_at_ms: None,
        cost_cents: None,
        session: None,
    }
}
fn limit(at_ms: u64) -> LimitSignal {
    signal(SignalKind::UsageLimit, at_ms)
}
fn limit_until(at_ms: u64, resets_at_ms: u64) -> LimitSignal {
    LimitSignal {
        resets_at_ms: Some(resets_at_ms),
        ..limit(at_ms)
    }
}
fn recovered(at_ms: u64) -> LimitSignal {
    signal(SignalKind::Recovered, at_ms)
}
fn usage(at_ms: u64, cost_cents: Option<u64>) -> LimitSignal {
    LimitSignal {
        cost_cents,
        ..signal(SignalKind::Usage, at_ms)
    }
}
fn fold(signals: &[LimitSignal]) -> CapacityEntry {
    let mut entry = None;
    for signal in signals {
        let (next, applied) = apply_signal(entry, signal);
        assert_eq!(applied, Applied::Applied, "{signal:?}");
        entry = Some(next);
    }
    entry.unwrap()
}
/// `(state code, reason, until)` at one instant, with a fully reporting plugin.
fn read(entry: &CapacityEntry, now_ms: u64) -> Reading {
    describe(state_at(Some(entry), FULL, None, now_ms))
}
fn describe(status: CapacityStatus) -> Reading {
    let until = match status.state {
        CapacityState::Limited { until_ms } => Some(until_ms),
        _ => None,
    };
    (status.state.code(), status.reason, until)
}

#[test]
fn hs2_ahg7hw_transition_table_covers_available_limited_degraded_and_recovered() {
    use CapacityReason as R;
    let backoff = BACKOFF_BASE_MS;
    // Each row: the signals so far, the instant read, the expected state.
    let table: Vec<(&str, Vec<LimitSignal>, u64, Reading)> = vec![
        (
            "a completed turn makes an observed account available",
            vec![usage(T0, Some(5))],
            T0 + 1,
            ("available", None, None),
        ),
        (
            "a usage limit without a reset time backs off from its own time",
            vec![usage(T0, Some(5)), limit(T0 + MIN)],
            T0 + 2 * MIN,
            ("limited", Some(R::UsageLimit), Some(T0 + MIN + backoff)),
        ),
        (
            "a rate limit keeps its own reason",
            vec![signal(SignalKind::RateLimit, T0)],
            T0 + 1,
            ("limited", Some(R::RateLimit), Some(T0 + backoff)),
        ),
        (
            "an explicit recovery ends the limit at once",
            vec![limit(T0), recovered(T0 + MIN)],
            T0 + MIN,
            ("available", None, None),
        ),
        (
            "a backoff that ends without a completed turn is only a probe",
            vec![limit(T0)],
            T0 + backoff,
            ("degraded", Some(R::RecoveryUnconfirmed), None),
        ),
        (
            "a turn completed after the backoff confirms the recovery",
            vec![limit(T0), usage(T0 + backoff + MIN, Some(1))],
            T0 + backoff + MIN,
            ("available", None, None),
        ),
        (
            "a reported reset time is believed exactly",
            vec![limit_until(T0, T0 + 3 * HOUR)],
            T0 + 3 * HOUR - 1,
            ("limited", Some(R::UsageLimit), Some(T0 + 3 * HOUR)),
        ),
        (
            "after the provider's own reset time the account is available",
            vec![limit_until(T0, T0 + 3 * HOUR)],
            T0 + 3 * HOUR,
            ("available", None, None),
        ),
        (
            "a turn completed during a guessed backoff contradicts it",
            vec![limit(T0), usage(T0 + MIN, Some(1))],
            T0 + MIN,
            ("degraded", Some(R::InconsistentSignals), None),
        ),
        (
            "a second completed turn settles the contradiction",
            vec![
                limit(T0),
                usage(T0 + MIN, Some(1)),
                usage(T0 + 2 * MIN, Some(1)),
            ],
            T0 + 2 * MIN,
            ("available", None, None),
        ),
        (
            "a late turn does not outrank the provider's reset time",
            vec![limit_until(T0, T0 + HOUR), usage(T0 + MIN, Some(1))],
            T0 + 2 * MIN,
            ("limited", Some(R::UsageLimit), Some(T0 + HOUR)),
        ),
        (
            "a limit after a recovery starts a new, short backoff",
            vec![limit(T0), recovered(T0 + MIN), limit(T0 + 2 * MIN)],
            T0 + 3 * MIN,
            ("limited", Some(R::UsageLimit), Some(T0 + 2 * MIN + backoff)),
        ),
    ];
    for (name, signals, now_ms, expected) in table {
        assert_eq!(read(&fold(&signals), now_ms), expected, "{name}");
    }
    // Nothing observed is unknown, never available.
    assert_eq!(
        describe(state_at(None, FULL, None, T0)),
        ("unknown", Some(R::NotObserved), None)
    );
}

#[test]
fn hs2_ahg7hw_repeated_limits_lengthen_the_backoff_up_to_the_cap() {
    assert_eq!(BACKOFF_BASE_MS, 5 * MIN);
    assert_eq!(BACKOFF_FACTOR, 2);
    assert_eq!(BACKOFF_CAP_MS, 4 * HOUR);
    let minutes: Vec<u64> = (0..=9).map(|n| backoff_ms(n) / MIN).collect();
    assert_eq!(minutes, [5, 5, 10, 20, 40, 80, 160, 240, 240, 240]);
    assert_eq!(backoff_ms(u32::MAX), BACKOFF_CAP_MS);

    // Each probe that hits the limit again doubles the wait, up to the cap.
    let mut entry = None;
    let mut at = T0;
    for expected in [5, 10, 20, 40, 80, 160, 240, 240] {
        let (next, applied) = apply_signal(entry, &limit(at));
        assert_eq!(applied, Applied::Applied);
        let until = next.limit.as_ref().unwrap().until_ms;
        assert_eq!((until - at) / MIN, expected);
        assert_eq!(read(&next, until).0, "degraded");
        at = until;
        entry = Some(next);
    }
    let entry = entry.unwrap();
    assert_eq!(entry.consecutive_limits, 8);

    // More reports during one backoff are the same episode and do not lengthen it.
    let once = fold(&[limit(T0)]);
    let (again, applied) = apply_signal(Some(once.clone()), &limit(T0 + MIN));
    assert_eq!(applied, Applied::Applied);
    assert_eq!(again.consecutive_limits, 1);
    assert_eq!(again.limit, once.limit);
    // A reset time learned during the episode replaces the guess.
    let (stated, _) = apply_signal(Some(again), &limit_until(T0 + 2 * MIN, T0 + 2 * HOUR));
    assert_eq!(
        read(&stated, T0 + HOUR),
        (
            "limited",
            Some(CapacityReason::UsageLimit),
            Some(T0 + 2 * HOUR)
        )
    );

    // A confirmed recovery resets the sequence; a contradiction does not.
    let (confirmed, _) = apply_signal(Some(entry.clone()), &usage(at + MIN, Some(1)));
    assert_eq!(confirmed.consecutive_limits, 0);
    let (fresh, _) = apply_signal(Some(confirmed), &limit(at + 2 * MIN));
    assert_eq!(fresh.limit.unwrap().until_ms, at + 2 * MIN + 5 * MIN);
    let contradicted = fold(&[limit(T0), usage(T0 + MIN, Some(1))]);
    assert!(contradicted.unconfirmed);
    let (longer, _) = apply_signal(Some(contradicted), &limit(T0 + 2 * MIN));
    assert_eq!(longer.consecutive_limits, 2);
    assert_eq!(longer.limit.unwrap().until_ms, T0 + 2 * MIN + 10 * MIN);
    // After the provider's own reset time, the next limit is a new sequence.
    let (after_reset, _) = apply_signal(
        Some(fold(&[limit(T0), limit_until(T0 + MIN, T0 + HOUR)])),
        &limit(T0 + 2 * HOUR),
    );
    assert_eq!(after_reset.consecutive_limits, 1);
}

#[test]
fn hs2_ahg7hw_a_plugin_that_cannot_report_stays_unknown() {
    use CapacityReason as R;
    // Completed turns and even an explicit recovery never make it available.
    for signals in [
        vec![usage(T0, Some(1))],
        vec![recovered(T0)],
        vec![limit(T0), recovered(T0 + MIN)],
        vec![limit(T0), usage(T0 + HOUR, Some(1))],
    ] {
        let entry = fold(&signals);
        assert_eq!(
            describe(state_at(Some(&entry), NONE, None, T0 + 2 * HOUR)),
            ("unknown", Some(R::NotReported), None),
            "{signals:?}"
        );
    }
    assert_eq!(
        describe(state_at(None, NONE, None, T0)),
        ("unknown", Some(R::NotReported), None)
    );
    // A limit somebody did observe is still a limit, and its end is still unknown.
    let entry = fold(&[limit(T0)]);
    assert_eq!(
        describe(state_at(Some(&entry), NONE, None, T0 + MIN)),
        ("limited", Some(R::UsageLimit), Some(T0 + BACKOFF_BASE_MS))
    );
    assert_eq!(
        describe(state_at(Some(&entry), NONE, None, T0 + BACKOFF_BASE_MS)),
        ("unknown", Some(R::NotReported), None)
    );
    // Usage without limit reporting is not evidence of availability either.
    let usage_only = SignalSupport {
        usage: true,
        ..NONE
    };
    assert_eq!(
        state_at(Some(&fold(&[usage(T0, None)])), usage_only, None, T0).state,
        CapacityState::Unknown
    );
    assert!(!state_at(None, NONE, None, T0).verified());
    assert!(state_at(Some(&fold(&[usage(T0, None)])), FULL, None, T0).verified());
}

#[test]
fn hs2_ahg7hw_repeated_signals_change_nothing() {
    for signal in [
        limit(T0),
        limit_until(T0, T0 + HOUR),
        recovered(T0),
        usage(T0, Some(7)),
        usage(T0, None),
    ] {
        let (first, applied) = apply_signal(None, &signal);
        assert_eq!(applied, Applied::Applied);
        let (second, applied) = apply_signal(Some(first.clone()), &signal);
        assert_eq!(applied, Applied::Duplicate, "{signal:?}");
        assert_eq!(second, first);
    }
    // The same turn reported twice is counted once; two sessions' turns both count.
    let mut entry = fold(&[usage(T0, Some(7))]);
    let other = LimitSignal {
        session: Some("session-b".into()),
        ..usage(T0, Some(7))
    };
    for _ in 0..3 {
        entry = apply_signal(Some(entry), &other).0;
    }
    assert_eq!(entry.spend.as_ref().unwrap().cents, 14);
    assert_eq!(entry.recent.len(), 2);
    // A repeat is recognised even after newer signals arrived.
    let entry = fold(&[limit(T0), recovered(T0 + MIN), usage(T0 + 2 * MIN, Some(1))]);
    assert_eq!(
        apply_signal(Some(entry.clone()), &limit(T0)),
        (entry, Applied::Duplicate)
    );
}

#[test]
fn hs2_ahg7hw_out_of_order_signals_never_move_the_state_backwards() {
    // An old limit delivered after the recovery that ended it.
    let recovered_first = fold(&[recovered(T0 + MIN)]);
    let (entry, applied) = apply_signal(Some(recovered_first.clone()), &limit(T0));
    assert_eq!(applied, Applied::Stale);
    assert_eq!(entry, recovered_first);
    assert_eq!(read(&entry, T0 + 2 * MIN).0, "available");
    // An old limit delivered after a newer completed turn.
    let worked = fold(&[usage(T0 + MIN, Some(1))]);
    assert_eq!(
        apply_signal(Some(worked.clone()), &limit(T0)),
        (worked, Applied::Stale)
    );
    // An old recovery delivered after a newer limit.
    let limited = fold(&[limit(T0 + MIN)]);
    let (entry, applied) = apply_signal(Some(limited.clone()), &recovered(T0));
    assert_eq!(applied, Applied::Stale);
    assert_eq!(entry, limited);
    assert_eq!(read(&entry, T0 + 2 * MIN).0, "limited");
    // An old completed turn delivered after a newer limit: its cost counts, the limit stays.
    let (entry, applied) = apply_signal(Some(limited), &usage(T0, Some(9)));
    assert_eq!(applied, Applied::Applied);
    assert_eq!(read(&entry, T0 + 2 * MIN).0, "limited");
    assert!(!entry.unconfirmed);
    assert_eq!(entry.spend.as_ref().unwrap().cents, 9);
    assert_eq!(entry.updated_at_ms, T0 + MIN);
    // A recovery that arrives after the limit that followed it changes nothing.
    let in_order = fold(&[limit(T0), recovered(T0 + MIN), limit(T0 + 2 * MIN)]);
    let (late, applied) = apply_signal(
        Some(fold(&[limit(T0), limit(T0 + 6 * MIN)])),
        &recovered(T0 + MIN),
    );
    assert_eq!(applied, Applied::Stale);
    assert_eq!(read(&late, T0 + 7 * MIN).0, "limited");
    assert_eq!(read(&in_order, T0 + 3 * MIN).0, "limited");
    // Yesterday's cost delivered today does not count against today.
    let today = fold(&[usage(T0, Some(5))]);
    let (entry, applied) = apply_signal(Some(today), &usage(T0 - DAY, Some(900)));
    assert_eq!(applied, Applied::Applied);
    assert_eq!(entry.spend.unwrap().cents, 5);

    // A full window cannot tell an older signal from a repeat, so it is stale.
    let mut entry = None;
    for step in 0..MAX_RECENT_SIGNALS as u64 + 4 {
        entry = Some(apply_signal(entry, &usage(T0 + step * MIN, Some(1))).0);
    }
    let entry = entry.unwrap();
    assert_eq!(entry.recent.len(), MAX_RECENT_SIGNALS);
    assert_eq!(entry.recent[0].at_ms, T0 + 4 * MIN);
    let (after, applied) = apply_signal(Some(entry.clone()), &usage(T0, Some(1)));
    assert_eq!(applied, Applied::Stale);
    assert_eq!(after, entry);
    assert_eq!(entry.spend.unwrap().cents, MAX_RECENT_SIGNALS as u64 + 4);
}

#[test]
fn hs2_ahg7hw_one_accounts_limit_never_changes_another() {
    let on = |account: &str, mut signal: LimitSignal| {
        signal.account = Some(account.into());
        signal
    };
    let mut entries = vec![
        fold(&[on("first", usage(T0, Some(1)))]),
        fold(&[on("second", usage(T0, Some(1)))]),
    ];
    let before = entries[1].clone();
    entries[0] = apply_signal(Some(entries[0].clone()), &on("first", limit(T0 + MIN))).0;
    assert_eq!(entries[1], before);
    let status = |entries: &[CapacityEntry], account: Option<&str>| {
        describe(status_for(
            entries,
            "alpha",
            account,
            FULL,
            None,
            T0 + 2 * MIN,
        ))
    };
    assert_eq!(status(&entries, Some("first")).0, "limited");
    assert_eq!(status(&entries, Some("second")), ("available", None, None));
    // Another tool with the same account name is a different account.
    let mut other_tool = on("first", usage(T0, Some(1)));
    other_tool.tool = "beta".into();
    entries.push(fold(&[other_tool]));
    assert_eq!(
        status_for(&entries, "beta", Some("first"), FULL, None, T0 + 2 * MIN).state,
        CapacityState::Available
    );

    // A limit nobody could attribute limits no account; it makes each a probe.
    entries.push(fold(&[limit(T0 + MIN)]));
    assert_eq!(status(&entries, Some("first")).0, "limited");
    assert_eq!(
        status(&entries, Some("second")),
        ("degraded", Some(CapacityReason::UnattributedLimit), None)
    );
    assert_eq!(status(&entries, None).0, "limited");
    assert_eq!(entries[1], before, "the account's own record is untouched");
    // When the unattributed limit ends, the account reads from its own record again.
    assert_eq!(
        describe(status_for(
            &entries,
            "alpha",
            Some("second"),
            FULL,
            None,
            T0 + HOUR
        )),
        ("available", None, None)
    );
}

#[test]
fn hs2_ahg7hw_a_daily_budget_limits_until_the_next_utc_day_or_is_unknown() {
    use CapacityReason as R;
    let next_day = 20_001 * DAY;
    let spent = fold(&[usage(T0, Some(60)), usage(T0 + MIN, Some(40))]);
    assert_eq!(known_spend_cents(Some(&spent), FULL, T0 + HOUR), Some(100));
    let at = |entry: &CapacityEntry, support, budget, now_ms| {
        describe(state_at(Some(entry), support, budget, now_ms))
    };
    assert_eq!(
        at(&spent, FULL, Some(101), T0 + HOUR),
        ("available", None, None)
    );
    assert_eq!(
        at(&spent, FULL, Some(100), T0 + HOUR),
        ("limited", Some(R::BudgetExceeded), Some(next_day))
    );
    assert_eq!(at(&spent, FULL, None, T0 + HOUR), ("available", None, None));
    // The next UTC day starts from zero.
    assert_eq!(
        at(&spent, FULL, Some(100), next_day),
        ("available", None, None)
    );
    assert_eq!(known_spend_cents(Some(&spent), FULL, next_day), Some(0));
    // The later of a limit and the budget decides the end and the reason.
    let both = apply_signal(
        Some(spent.clone()),
        &limit_until(T0 + 2 * MIN, T0 + 2 * DAY),
    )
    .0;
    assert_eq!(
        at(&both, FULL, Some(100), T0 + HOUR),
        ("limited", Some(R::UsageLimit), Some(T0 + 2 * DAY))
    );
    let short = apply_signal(Some(spent.clone()), &limit(T0 + 2 * MIN)).0;
    assert_eq!(
        at(&short, FULL, Some(100), T0 + 3 * MIN),
        ("limited", Some(R::BudgetExceeded), Some(next_day))
    );

    // One turn without a cost makes the day's total unknown: not available.
    let unpriced = apply_signal(Some(fold(&[usage(T0, Some(10))])), &usage(T0 + MIN, None)).0;
    assert_eq!(known_spend_cents(Some(&unpriced), FULL, T0 + HOUR), None);
    assert_eq!(
        at(&unpriced, FULL, Some(100), T0 + HOUR),
        ("unknown", Some(R::BudgetUnknown), None)
    );
    // Without a budget the unknown cost does not matter.
    assert_eq!(
        at(&unpriced, FULL, None, T0 + HOUR),
        ("available", None, None)
    );
    // Cost that was observed still exceeds a budget, whatever else is unknown.
    assert_eq!(
        at(&unpriced, FULL, Some(10), T0 + HOUR),
        ("limited", Some(R::BudgetExceeded), Some(next_day))
    );
    // A plugin that reports no cost can never show a budget as respected.
    let no_cost = SignalSupport {
        cost: false,
        ..FULL
    };
    assert_eq!(known_spend_cents(Some(&spent), no_cost, T0 + HOUR), None);
    assert_eq!(
        at(&fold(&[usage(T0, None)]), no_cost, Some(100), T0 + HOUR),
        ("unknown", Some(R::BudgetUnknown), None)
    );
    assert_eq!(
        describe(state_at(None, no_cost, Some(100), T0)),
        ("unknown", Some(R::BudgetUnknown), None)
    );
    // A zero budget allows nothing.
    assert_eq!(
        describe(state_at(None, FULL, Some(0), T0)),
        ("limited", Some(R::BudgetExceeded), Some(next_day))
    );
}

#[test]
fn hs2_ahg7hw_signals_are_validated_and_carry_no_text() {
    let now = T0;
    assert!(limit(T0).validate(now).is_ok());
    assert!(
        limit_until(T0, T0 + MAX_RESET_AHEAD_MS)
            .validate(now)
            .is_ok()
    );
    assert!(
        limit_until(T0, T0 - MAX_SIGNAL_SKEW_MS)
            .validate(now)
            .is_ok()
    );
    assert!(
        usage(T0 + MAX_SIGNAL_SKEW_MS, Some(MAX_SIGNAL_COST_CENTS))
            .validate(now)
            .is_ok()
    );
    let bad = [
        LimitSignal {
            tool: "two words".into(),
            ..limit(T0)
        },
        LimitSignal {
            tool: String::new(),
            ..limit(T0)
        },
        LimitSignal {
            account: Some("person@example.test".into()),
            ..limit(T0)
        },
        LimitSignal {
            session: Some("a session".into()),
            ..limit(T0)
        },
        limit(0),
        limit(T0 + MAX_SIGNAL_SKEW_MS + 1),
        limit_until(T0, T0 + MAX_RESET_AHEAD_MS + 1),
        limit_until(T0, T0 - MAX_SIGNAL_SKEW_MS - 1),
        LimitSignal {
            resets_at_ms: Some(T0 + HOUR),
            ..recovered(T0)
        },
        LimitSignal {
            resets_at_ms: Some(T0 + HOUR),
            ..usage(T0, None)
        },
        LimitSignal {
            cost_cents: Some(1),
            ..limit(T0)
        },
        LimitSignal {
            cost_cents: Some(1),
            ..recovered(T0)
        },
        usage(T0, Some(MAX_SIGNAL_COST_CENTS + 1)),
    ];
    for signal in bad {
        assert!(
            matches!(signal.validate(now), Err(OrchestrationError::Invalid(_))),
            "{signal:?}"
        );
    }
    // The wire shape has no free-text member, and unknown members are refused.
    let wire = serde_json::to_value(LimitSignal {
        account: Some("team-a".into()),
        session: Some("session-1".into()),
        ..limit_until(T0, T0 + HOUR)
    })
    .unwrap();
    assert_eq!(
        wire,
        serde_json::json!({"tool":"alpha","account":"team-a","kind":"usage_limit",
            "at_ms":T0,"resets_at_ms":T0 + HOUR,"session":"session-1"})
    );
    for extra in ["message", "error", "detail"] {
        let mut value = wire.clone();
        value[extra] = "synthetic".into();
        assert!(serde_json::from_value::<LimitSignal>(value).is_err());
    }
    for kind in [
        SignalKind::RateLimit,
        SignalKind::UsageLimit,
        SignalKind::Recovered,
        SignalKind::Usage,
    ] {
        assert_eq!(serde_json::to_value(kind).unwrap(), kind.code());
        assert_eq!(kind.is_limit(), kind.code().ends_with("_limit"));
    }
    assert_eq!(
        serde_json::to_value(state_at(Some(&fold(&[limit(T0)])), FULL, None, T0)).unwrap(),
        serde_json::json!({"state":"limited","until_ms":T0 + BACKOFF_BASE_MS,
            "reason":"usage_limit"})
    );
    assert_eq!(
        serde_json::to_value(state_at(Some(&fold(&[recovered(T0)])), FULL, None, T0)).unwrap(),
        serde_json::json!({"state":"available"})
    );
}

fn tool(
    name: &str,
    max_sessions: u32,
    weight: u32,
    accounts: &[(&str, u32, Option<u64>)],
) -> ToolPolicy {
    ToolPolicy {
        tool: name.into(),
        max_sessions: Some(max_sessions),
        weight: Some(weight),
        accounts: accounts
            .iter()
            .map(|(account, max_sessions, budget)| AccountPolicy {
                account: (*account).into(),
                max_sessions: *max_sessions,
                budget_cents_per_day: *budget,
            })
            .collect(),
    }
}
fn target_id() -> String {
    ulid::Ulid::from(7u128).to_string()
}
fn policy(tools: Vec<ToolPolicy>, scopes: &[(&str, &[&str])]) -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 8,
            ..MachineLimits::default()
        },
        tools,
        targets: vec![TargetPolicy {
            target_id: target_id(),
            enabled: true,
            actions: None,
            tools: scopes
                .iter()
                .map(|(tool, models)| ToolScope {
                    tool: (*tool).into(),
                    models: models.iter().map(|model| (*model).into()).collect(),
                })
                .collect(),
            max_sessions: Some(4),
            machine: None,
            speculation: None,
        }],
        ..PolicyBody::default()
    }
}
fn observed(tool: &str, account: Option<&str>, signals: &[LimitSignal]) -> CapacityEntry {
    let signals: Vec<LimitSignal> = signals
        .iter()
        .cloned()
        .map(|mut signal| {
            signal.tool = tool.into();
            signal.account = account.map(Into::into);
            signal
        })
        .collect();
    fold(&signals)
}
fn count(tool: &str, account: Option<&str>, sessions: u32) -> SessionCount {
    SessionCount {
        tool: tool.into(),
        account: account.map(Into::into),
        sessions,
    }
}
/// `gamma` is a tool whose plugin reports nothing.
fn support(tool: &str) -> SignalSupport {
    if tool == "gamma" { NONE } else { FULL }
}
fn choose(
    policy: &PolicyBody,
    tool: Option<&str>,
    model: Option<&str>,
    entries: &[CapacityEntry],
    load: &[SessionCount],
) -> Selection {
    select(
        &SelectionRequest {
            policy,
            target_id: &target_id(),
            tool,
            model,
            entries,
            load,
            now_ms: T0 + HOUR,
            auto_tools: &[],
        },
        &support,
    )
}
fn names(selection: &Selection) -> Vec<(&str, Option<&str>, &'static str)> {
    selection
        .candidates
        .iter()
        .map(|candidate| {
            (
                candidate.tool.as_str(),
                candidate.account.as_deref(),
                candidate.status.state.code(),
            )
        })
        .collect()
}
fn excluded(selection: &Selection) -> Vec<(&str, Option<&str>, ExclusionReason)> {
    selection
        .excluded
        .iter()
        .map(|entry| {
            (
                entry.tool.as_str(),
                entry.account.as_deref(),
                entry.excluded,
            )
        })
        .collect()
}

#[test]
fn hs2_ahg7hw_a_limited_tool_fails_over_to_another_allowed_tool() {
    let body = policy(
        vec![
            tool("alpha", 2, 80, &[]),
            tool("beta", 2, 40, &[]),
            tool("gamma", 2, 90, &[]),
        ],
        &[("alpha", &[]), ("beta", &[]), ("gamma", &[])],
    );
    assert!(body.problems(None).is_empty());
    let working = [usage(T0, Some(1))];
    let healthy = vec![
        observed("alpha", None, &working),
        observed("beta", None, &working),
        observed("gamma", None, &working),
    ];
    // Both verified: the higher weight leads. The unverifiable tool ranks last even
    // with the highest weight, and is marked unverified.
    let selection = choose(&body, None, None, &healthy, &[]);
    assert_eq!(selection.denied, None);
    assert_eq!(
        names(&selection),
        [
            ("alpha", None, "available"),
            ("beta", None, "available"),
            ("gamma", None, "unknown"),
        ]
    );
    assert_eq!(
        selection
            .candidates
            .iter()
            .map(|c| c.verified)
            .collect::<Vec<_>>(),
        [true, true, false]
    );
    assert_eq!(
        selection.candidates[2].status.reason,
        Some(CapacityReason::NotReported)
    );
    assert!(selection.excluded.is_empty());
    // A requested tool leads its rank whatever the weights say.
    let selection = choose(&body, Some("beta"), None, &healthy, &[]);
    assert_eq!(names(&selection)[0].0, "beta");
    assert!(selection.candidates[0].requested && !selection.candidates[1].requested);

    // The requested tool is limited: the next allowed tool takes the work, and the
    // answer says why and until when the requested one is out.
    let limited = vec![
        observed("alpha", None, &[limit_until(T0, T0 + 3 * HOUR)]),
        observed("beta", None, &working),
    ];
    let selection = choose(&body, Some("alpha"), None, &limited, &[]);
    assert_eq!(
        names(&selection),
        [("beta", None, "available"), ("gamma", None, "unknown")]
    );
    assert_eq!(
        excluded(&selection),
        [("alpha", None, ExclusionReason::Limited)]
    );
    assert_eq!(
        serde_json::to_value(&selection.excluded[0]).unwrap(),
        serde_json::json!({"tool":"alpha","excluded":"limited","state":"limited",
            "until_ms":T0 + 3 * HOUR,"reason":"usage_limit"})
    );
    assert_eq!(
        serde_json::to_value(&selection.candidates[0]).unwrap(),
        serde_json::json!({"tool":"beta","state":"available","verified":true,
            "requested":false,"weight":40,"free_sessions":2})
    );
    // A probe ranks between verified and unknown.
    let probing = vec![
        observed("alpha", None, &[limit(T0)]),
        observed("beta", None, &working),
    ];
    assert_eq!(
        names(&choose(&body, Some("alpha"), None, &probing, &[])),
        [
            ("beta", None, "available"),
            ("alpha", None, "degraded"),
            ("gamma", None, "unknown"),
        ]
    );
    // Every tool limited: nothing is offered.
    let until = limit_until(T0, T0 + 3 * HOUR);
    let all = vec![
        observed("alpha", None, std::slice::from_ref(&until)),
        observed("beta", None, std::slice::from_ref(&until)),
        observed("gamma", None, std::slice::from_ref(&until)),
    ];
    let selection = choose(&body, None, None, &all, &[]);
    assert!(selection.candidates.is_empty());
    assert_eq!(selection.excluded.len(), 3);
    // Unobserved tools are offered as unknown, never as available.
    let selection = choose(&body, None, None, &[], &[]);
    assert!(selection.candidates.iter().all(|c| !c.verified));
    assert_eq!(
        names(&selection)
            .iter()
            .map(|name| name.2)
            .collect::<Vec<_>>(),
        ["unknown"; 3]
    );
}

#[test]
fn hs2_ahg7hw_selection_respects_caps_accounts_models_and_the_target() {
    let body = policy(
        vec![
            tool(
                "alpha",
                3,
                50,
                &[("first", 2, Some(100)), ("second", 1, None)],
            ),
            tool("beta", 1, 50, &[]),
        ],
        &[("alpha", &["large"]), ("beta", &[])],
    );
    assert!(body.problems(None).is_empty());
    let working = [usage(T0, Some(1))];
    let entries = vec![
        observed("alpha", Some("first"), &working),
        observed("alpha", Some("second"), &working),
        observed("beta", None, &working),
    ];
    // Free sessions are the smaller of the tool's and the account's remainder.
    let selection = choose(
        &body,
        None,
        None,
        &entries,
        &[count("alpha", Some("first"), 1)],
    );
    assert_eq!(
        selection
            .candidates
            .iter()
            .map(|c| (c.tool.as_str(), c.account.as_deref(), c.free_sessions))
            .collect::<Vec<_>>(),
        [
            ("alpha", Some("first"), 1),
            ("alpha", Some("second"), 1),
            ("beta", None, 1)
        ]
    );
    // A full account is out; its sibling and the other tool remain.
    let selection = choose(
        &body,
        None,
        None,
        &entries,
        &[count("alpha", Some("second"), 1), count("beta", None, 1)],
    );
    assert_eq!(names(&selection), [("alpha", Some("first"), "available")]);
    assert_eq!(
        excluded(&selection),
        [
            ("alpha", Some("second"), ExclusionReason::AccountCap),
            ("beta", None, ExclusionReason::ToolCap),
        ]
    );
    // The tool cap counts every account's sessions, including unattributed ones.
    let selection = choose(
        &body,
        None,
        None,
        &entries,
        &[count("alpha", Some("first"), 1), count("alpha", None, 2)],
    );
    assert_eq!(
        excluded(&selection),
        [
            ("alpha", Some("first"), ExclusionReason::ToolCap),
            ("alpha", Some("second"), ExclusionReason::ToolCap),
        ]
    );
    // One account over budget fails over to its sibling account.
    let mut spent = entries.clone();
    spent[0] = observed("alpha", Some("first"), &[usage(T0, Some(100))]);
    let selection = choose(&body, Some("alpha"), None, &spent, &[]);
    assert_eq!(names(&selection)[0], ("alpha", Some("second"), "available"));
    assert_eq!(
        excluded(&selection),
        [("alpha", Some("first"), ExclusionReason::Limited)]
    );
    assert_eq!(
        selection.excluded[0].status.unwrap().reason,
        Some(CapacityReason::BudgetExceeded)
    );
    // A model outside the target's list rules the tool out for this request.
    let selection = choose(&body, None, Some("small"), &entries, &[]);
    assert_eq!(names(&selection), [("beta", None, "available")]);
    assert_eq!(
        excluded(&selection),
        [("alpha", None, ExclusionReason::ModelNotPermitted)]
    );
    assert_eq!(
        choose(&body, None, Some("large"), &entries, &[])
            .candidates
            .len(),
        3
    );
    // A requested tool the target does not list is named, and the rest still answer.
    let selection = choose(&body, Some("delta"), None, &entries, &[]);
    assert_eq!(
        excluded(&selection),
        [("delta", None, ExclusionReason::ToolNotPermitted)]
    );
    assert_eq!(selection.candidates.len(), 3);
    assert!(selection.candidates.iter().all(|c| !c.requested));

    // The target itself must be permitted, enabled and allowed to manage sessions.
    let ask = |policy: &PolicyBody, target: &str| {
        select(
            &SelectionRequest {
                policy,
                target_id: target,
                tool: None,
                model: None,
                entries: &entries,
                load: &[],
                now_ms: T0,
                auto_tools: &[],
            },
            &support,
        )
    };
    // HS2-N5BDE4: a target the policy does not list is Auto: every listed tool.
    let other = ulid::Ulid::from(8u128).to_string();
    let auto = ask(&body, &other);
    assert_eq!(auto.denied, None);
    assert!(!auto.candidates.is_empty());
    let mut disabled = body.clone();
    disabled.targets[0].enabled = false;
    assert_eq!(
        ask(&disabled, &target_id()).denied,
        Some("policy_target_disabled")
    );
    // HS2-N3HWP9: a target tool the policy does not list is Auto, not excluded; one
    // the policy turns off (Sessions 0) is excluded.
    let mut auto = body.clone();
    auto.tools.remove(1);
    let selection = ask(&auto, &target_id());
    assert!(selection.excluded.is_empty());
    assert!(selection.candidates.iter().any(|c| c.tool == "beta"));
    let mut off = body.clone();
    off.tools[1].max_sessions = Some(0);
    assert_eq!(
        excluded(&ask(&off, &target_id())),
        [("beta", None, ExclusionReason::ToolNotConfigured)]
    );
}

#[test]
fn hs2_n3hwp9_auto_tools_share_evenly_and_zero_turns_a_tool_off() {
    let auto = |name: &str| ToolPolicy::auto(name);
    let body = policy(
        vec![auto("alpha"), auto("beta")],
        &[("alpha", &[]), ("beta", &[])],
    );
    assert!(body.problems(None).is_empty());
    let working = [usage(T0, Some(1))];
    let healthy = vec![
        observed("alpha", None, &working),
        observed("beta", None, &working),
    ];
    // Equal Auto weights: the tool with fewer running sessions is tried first, so
    // starts alternate between the tools.
    let selection = choose(&body, None, None, &healthy, &[count("alpha", None, 1)]);
    assert_eq!(selection.candidates[0].tool, "beta");
    assert_eq!(selection.candidates[0].weight, AUTO_TOOL_WEIGHT);
    assert_eq!(selection.candidates[0].free_sessions, MAX_POLICY_SESSIONS);
    assert_eq!(
        selection.candidates[1].free_sessions,
        MAX_POLICY_SESSIONS - 1
    );
    let selection = choose(
        &body,
        None,
        None,
        &healthy,
        &[count("alpha", None, 1), count("beta", None, 2)],
    );
    assert_eq!(selection.candidates[0].tool, "alpha");
    // An unlisted tool is Auto too.
    let unlisted = policy(Vec::new(), &[("alpha", &[]), ("beta", &[])]);
    assert_eq!(
        choose(&unlisted, None, None, &healthy, &[])
            .candidates
            .len(),
        2
    );
    // An explicit weight still ranks above Auto's equal share.
    let mut weighted = body.clone();
    weighted.tools[0].weight = Some(90);
    let selection = choose(&weighted, None, None, &healthy, &[count("alpha", None, 3)]);
    assert_eq!(selection.candidates[0].tool, "alpha");
    // Sessions 0 or Weight 0: the tool is off and the other one takes the work.
    for off in [
        ToolPolicy {
            max_sessions: Some(0),
            ..auto("alpha")
        },
        ToolPolicy {
            weight: Some(0),
            ..auto("alpha")
        },
    ] {
        let mut body = body.clone();
        body.tools[0] = off;
        let selection = choose(&body, Some("alpha"), None, &healthy, &[]);
        assert_eq!(names(&selection), [("beta", None, "available")]);
        assert_eq!(
            excluded(&selection),
            [("alpha", None, ExclusionReason::ToolNotConfigured)]
        );
    }
}

fn file(home: &std::path::Path) -> std::path::PathBuf {
    home.join("orchestration-tool-capacity.json")
}

#[test]
fn hs2_ahg7hw_the_registry_is_idempotent_private_and_never_created_by_a_read() {
    let home = tempfile::tempdir().unwrap();
    let registry = ToolCapacityRegistry::new(home.path());
    assert!(registry.entries().unwrap().is_empty());
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());

    let outcome = registry.record(&limit(T0), T0).unwrap();
    assert_eq!(outcome.applied, Applied::Applied);
    assert_eq!(
        registry.entries().unwrap(),
        std::slice::from_ref(&outcome.entry)
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(file(home.path()))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    let stored: serde_json::Value =
        serde_json::from_slice(&std::fs::read(file(home.path())).unwrap()).unwrap();
    assert_eq!(stored["schema_version"], 1);
    assert_eq!(stored["entries"][0]["tool"], "alpha");

    // A repeat and a stale signal write nothing at all.
    let before = std::fs::read(file(home.path())).unwrap();
    let modified = || {
        std::fs::metadata(file(home.path()))
            .unwrap()
            .modified()
            .unwrap()
    };
    let written = modified();
    assert_eq!(
        registry.record(&limit(T0), T0 + MIN).unwrap().applied,
        Applied::Duplicate
    );
    assert_eq!(
        registry
            .record(&recovered(T0 - MIN), T0 + MIN)
            .unwrap()
            .applied,
        Applied::Stale
    );
    assert_eq!(std::fs::read(file(home.path())).unwrap(), before);
    assert_eq!(modified(), written);

    // Accounts and tools are separate records; a second registry sees the same file.
    let second = LimitSignal {
        account: Some("team-a".into()),
        ..usage(T0 + MIN, Some(3))
    };
    registry.record(&second, T0 + MIN).unwrap();
    registry
        .record(&recovered(T0 + 2 * MIN), T0 + 2 * MIN)
        .unwrap();
    let entries = ToolCapacityRegistry::new(home.path()).entries().unwrap();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].limit, None);
    assert_eq!(entries[0].recent.len(), 2);
    assert_eq!(entries[1].account.as_deref(), Some("team-a"));
    // An invalid signal is refused before anything is read or written.
    let before = std::fs::read(file(home.path())).unwrap();
    assert!(matches!(
        registry.record(&limit(T0 + DAY), T0),
        Err(OrchestrationError::Invalid(_))
    ));
    assert_eq!(std::fs::read(file(home.path())).unwrap(), before);
}

#[test]
fn hs2_ahg7hw_corrupt_or_newer_capacity_data_fails_closed_and_is_kept() {
    let valid = serde_json::to_value(fold(&[limit(T0)])).unwrap();
    let with = |change: &dyn Fn(&mut serde_json::Value)| {
        let mut entry = valid.clone();
        change(&mut entry);
        serde_json::json!({"schema_version":1,"entries":[entry]}).to_string()
    };
    let corrupt = "corrupt_orchestration_data";
    let cases: Vec<(String, &str)> = vec![
        ("not json".into(), corrupt),
        ("{}".into(), corrupt),
        (
            r#"{"schema_version":2,"entries":[]}"#.into(),
            "upgrade_required",
        ),
        (
            r#"{"schema_version":1,"entries":[],"extra":true}"#.into(),
            corrupt,
        ),
        (with(&|entry| entry["tool"] = "two words".into()), corrupt),
        (with(&|entry| entry["account"] = "a b".into()), corrupt),
        (
            with(&|entry| entry["limit"]["kind"] = "usage".into()),
            corrupt,
        ),
        (
            with(&|entry| entry["limit"]["until_ms"] = 1.into()),
            corrupt,
        ),
        (
            with(&|entry| entry["recent"][0]["tool"] = "beta".into()),
            corrupt,
        ),
        (
            with(&|entry| entry["message"] = "synthetic".into()),
            corrupt,
        ),
        (
            serde_json::json!({"schema_version":1,"entries":[valid.clone(), valid.clone()]})
                .to_string(),
            corrupt,
        ),
    ];
    for (bytes, code) in cases {
        let home = tempfile::tempdir().unwrap();
        std::fs::write(file(home.path()), &bytes).unwrap();
        let registry = ToolCapacityRegistry::new(home.path());
        assert_eq!(registry.entries().unwrap_err().code(), code, "{bytes}");
        assert_eq!(
            registry.record(&limit(T0), T0).unwrap_err().code(),
            code,
            "{bytes}"
        );
        assert_eq!(std::fs::read_to_string(file(home.path())).unwrap(), bytes);
    }
    // The valid shape these cases were derived from does load.
    let home = tempfile::tempdir().unwrap();
    std::fs::write(
        file(home.path()),
        serde_json::json!({"schema_version":1,"entries":[valid]}).to_string(),
    )
    .unwrap();
    assert_eq!(
        ToolCapacityRegistry::new(home.path())
            .entries()
            .unwrap()
            .len(),
        1
    );
}

#[test]
fn hs2_ahg7hw_a_full_registry_forgets_only_records_with_nothing_in_force() {
    let home = tempfile::tempdir().unwrap();
    let registry = ToolCapacityRegistry::new(home.path());
    let named = |index: usize, signal: LimitSignal| LimitSignal {
        tool: format!("tool-{index}"),
        ..signal
    };
    // Every record holds a limit or today's spend.
    for index in 0..MAX_CAPACITY_ENTRIES {
        let signal = match index % 2 {
            0 => named(index, limit_until(T0, T0 + 2 * DAY)),
            _ => named(index, usage(T0, Some(1))),
        };
        registry.record(&signal, T0).unwrap();
    }
    assert_eq!(registry.entries().unwrap().len(), MAX_CAPACITY_ENTRIES);
    let before = std::fs::read(file(home.path())).unwrap();
    let extra = named(MAX_CAPACITY_ENTRIES, limit(T0));
    assert_eq!(
        registry.record(&extra, T0).unwrap_err().code(),
        "capacity_reached"
    );
    assert_eq!(std::fs::read(file(home.path())).unwrap(), before);
    // An existing record still takes signals at capacity.
    assert_eq!(
        registry
            .record(&named(1, usage(T0 + MIN, Some(1))), T0 + MIN)
            .unwrap()
            .applied,
        Applied::Applied
    );
    // The next day, records that only held yesterday's spend can make room; the
    // quietest goes first, and records with a limit in force stay.
    let tomorrow = T0 + DAY;
    registry.record(&extra, tomorrow).unwrap();
    let entries = registry.entries().unwrap();
    assert_eq!(entries.len(), MAX_CAPACITY_ENTRIES);
    let has = |tool: &str| entries.iter().any(|entry| entry.tool == tool);
    assert!(!has("tool-3"), "the longest-quiet spend-only record went");
    assert!(has("tool-1") && has("tool-0") && has("tool-2") && has("tool-256"));
}
