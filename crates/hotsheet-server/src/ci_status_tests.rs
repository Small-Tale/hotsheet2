use super::*;

fn report(status: &str) -> CiReport {
    CiReport {
        status: status.into(),
        ..CiReport::default()
    }
}

fn run(status: CiStatus, started: Option<i64>, reported: i64) -> CiRun {
    CiRun {
        status,
        branch: Some("main".into()),
        sha: Some("abcdef1".into()),
        run_url: None,
        workflow: Some("CI".into()),
        started_at_ms: started,
        finished_at_ms: None,
        reported_at_ms: reported,
        verification: Verification::Verified,
        verify_by_ms: None,
    }
}

fn workflow_run(workflow: &str, status: CiStatus, started: i64) -> CiRun {
    CiRun {
        workflow: Some(workflow.into()),
        ..run(status, Some(started), started)
    }
}

fn single(home: &Path, reports: &CiReports, checkout: &str) -> LiveCi {
    reports.live(home, &[checkout.to_owned()]).0
}

#[test]
fn hs2_m8hhea_report_validation_accepts_a_full_run_and_normalizes_it() {
    let full = CiReport {
        status: " success ".into(),
        branch: Some(" main ".into()),
        sha: Some("ABCDEF0123456789".into()),
        run_url: Some("https://github.com/o/r/actions/runs/1".into()),
        workflow: Some("CI".into()),
        started_at_ms: Some(10),
        finished_at_ms: Some(20),
    };
    let run = full.into_run(30).unwrap();
    assert_eq!(run.status, CiStatus::Success);
    assert_eq!(run.branch.as_deref(), Some("main"));
    assert_eq!(run.sha.as_deref(), Some("abcdef0123456789"));
    assert_eq!(run.reported_at_ms, 30);
    // HS2-D3JJ49: a finished run is verified with no time limit...
    assert_eq!(
        (run.verification, run.verify_by_ms),
        (Verification::Verified, None)
    );
    let minimal = report("queued").into_run(1).unwrap();
    assert_eq!(minimal.status, CiStatus::Queued);
    assert_eq!(minimal.sha, None);
    // An active report alone carries no verified provider observation.
    assert_eq!(minimal.verification(), Verification::Unverifiable);
    assert_eq!(minimal.verify_by_ms, None);
}

#[test]
fn hs2_m8hhea_report_validation_rejects_bad_fields() {
    let cases: Vec<(CiReport, &str)> = vec![
        (report("unavailable"), "status must be"),
        (report("passed"), "status must be"),
        (
            CiReport {
                sha: Some("xyz1234".into()),
                ..report("success")
            },
            "sha must be",
        ),
        (
            CiReport {
                sha: Some("abc".into()),
                ..report("success")
            },
            "sha must be",
        ),
        (
            CiReport {
                run_url: Some("file:///etc/passwd".into()),
                ..report("success")
            },
            "run_url must be",
        ),
        (
            CiReport {
                run_url: Some("https://x/ y".into()),
                ..report("success")
            },
            "run_url must be",
        ),
        (
            CiReport {
                branch: Some(String::new()),
                ..report("success")
            },
            "branch must be",
        ),
        (
            CiReport {
                workflow: Some("a\nb".into()),
                ..report("success")
            },
            "workflow must not",
        ),
        (
            CiReport {
                started_at_ms: Some(-1),
                ..report("queued")
            },
            "must not be negative",
        ),
        (
            CiReport {
                finished_at_ms: Some(5),
                ..report("in_progress")
            },
            "only valid for",
        ),
        (
            CiReport {
                started_at_ms: Some(10),
                finished_at_ms: Some(5),
                ..report("failure")
            },
            "before started_at_ms",
        ),
    ];
    for (case, expected) in cases {
        let error = case.clone().into_run(1).unwrap_err();
        assert!(error.contains(expected), "{case:?}: {error}");
    }
}

#[test]
fn hs2_m8hhea_staleness_keeps_the_newest_run_and_terminal_states() {
    let stored = run(CiStatus::Success, Some(100), 200);
    // An older run is stale; a newer one supersedes.
    assert!(!run(CiStatus::Failure, Some(99), 300).supersedes(&stored));
    assert!(run(CiStatus::Queued, Some(101), 300).supersedes(&stored));
    // The same run going back to in progress after success is stale...
    assert!(!run(CiStatus::InProgress, Some(100), 300).supersedes(&stored));
    // ...but a terminal correction of the same run applies (a re-run's conclusion).
    assert!(run(CiStatus::Failure, Some(100), 300).supersedes(&stored));
    // Same start but another commit is another run.
    let mut other = run(CiStatus::InProgress, Some(100), 300);
    other.sha = Some("1234567".into());
    assert!(other.supersedes(&stored));
    // Without both start times, the later report wins.
    assert!(run(CiStatus::Queued, None, 300).supersedes(&stored));
    assert!(run(CiStatus::Queued, Some(1), 300).supersedes(&run(CiStatus::Success, None, 1)));
    // A queued run moving to in progress then success applies in order.
    let queued = run(CiStatus::Queued, Some(500), 1);
    let running = run(CiStatus::InProgress, Some(500), 2);
    let done = run(CiStatus::Success, Some(500), 3);
    assert!(running.supersedes(&queued));
    assert!(done.supersedes(&running));
    assert!(!running.supersedes(&done));
}

/// HS2-D3JJ49: an initial outage cannot reveal a reported run. A later one retains an
/// established state until a successful observation or an unsettled end.
#[test]
fn hs2_d3jj49_verification_transitions() {
    use VerificationEvent::*;
    let mut flight = report("in_progress").into_run(1_000).unwrap();
    assert_eq!(flight.verification(), Verification::Unverifiable);
    assert!(!flight.verify(Unreachable));
    assert!(!flight.visible());

    assert!(flight.verify(Confirmed));
    assert!(flight.visible());
    assert!(!flight.verify(Confirmed));
    assert!(flight.verify(Unreachable));
    assert_eq!(flight.verification(), Verification::Transient);
    assert!(flight.visible());
    assert!(!flight.verify(Unreachable));
    assert!(flight.verify(Confirmed));
    assert_eq!(flight.verification(), Verification::Verified);
    assert!(flight.verify(Unreachable));
    assert!(flight.verify(Unsettled));
    assert!(!flight.visible());

    for event in [Recheck, Unfollowed] {
        assert!(flight.verify(Confirmed));
        assert!(flight.verify(event));
        assert!(!flight.visible());
        assert!(!flight.verify(Unreachable));
    }
    let mut done = report("success").into_run(5).unwrap();
    for event in [Confirmed, Unfollowed, Unreachable, Recheck, Unsettled] {
        assert!(!done.verify(event), "{event:?}");
        assert_eq!(done.verification(), Verification::Verified);
    }
}

#[test]
fn hs2_d3jj49_an_active_refresh_keeps_evidence_only_for_the_same_flight() {
    let mut known = workflow_run("CI", CiStatus::InProgress, 10);
    known.run_url = Some("https://github.com/o/r/actions/runs/1".into());
    known.verify(VerificationEvent::Unreachable);
    let mut repeat = known.clone();
    repeat.reported_at_ms += 1;
    repeat.verify(VerificationEvent::Unfollowed);
    let mut runs = vec![known];
    assert_eq!(
        record_run(&mut runs, repeat.clone()).1.verification(),
        Verification::Transient
    );

    repeat.run_url = Some("https://github.com/o/r/actions/runs/2".into());
    assert!(!record_run(&mut runs, repeat).1.visible());
}

#[test]
fn hs2_d3jj49_an_old_watch_cannot_confirm_or_finish_a_replacement() {
    use VerificationEvent::Confirmed;
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let reports = CiReports::default();
    let mut previous = report("in_progress").into_run(1).unwrap();
    previous.run_url = Some("https://github.com/o/r/actions/runs/1".into());
    reports.record(home, "a", previous.clone());
    let replacement = CiRun {
        run_url: Some("https://github.com/o/r/actions/runs/2".into()),
        reported_at_ms: 2,
        ..previous.clone()
    };
    reports.record(home, "a", replacement.clone());
    assert!(!reports.verify(home, "a", &previous, Confirmed));
    assert_eq!(
        reports.with_current_flight(home, "a", &previous, || "old stop"),
        None
    );
    assert_eq!(
        reports.with_current_flight(home, "a", &replacement, || "current stop"),
        Some("current stop")
    );
    let old_verdict = CiRun {
        status: CiStatus::Success,
        ..previous.clone()
    };
    assert!(
        !reports
            .record_matching(home, "a", old_verdict, Some(&previous))
            .0
    );
    assert_eq!(single(home, &reports, "a").status, CiStatus::Unavailable);
    assert!(reports.verify(home, "a", &replacement, Confirmed));
    assert_eq!(single(home, &reports, "a").run_url, replacement.run_url);
}

/// HS2-D3JJ49: one run per branch and workflow per checkout; a stale report of one pair
/// never touches another, and the oldest pair is dropped past the cap.
#[test]
fn hs2_d3jj49_several_runs_are_kept_per_branch_and_workflow() {
    let mut runs = Vec::new();
    assert!(record_run(&mut runs, workflow_run("CI", CiStatus::InProgress, 10)).0);
    assert!(
        record_run(
            &mut runs,
            workflow_run("Platforms", CiStatus::InProgress, 20)
        )
        .0
    );
    let mut docs = workflow_run("CI", CiStatus::Queued, 30);
    docs.branch = Some("docs".into());
    assert!(record_run(&mut runs, docs).0);
    assert_eq!(runs.len(), 3);
    // CI finishes: it replaces only its own pair.
    let (applied, stored) = record_run(&mut runs, workflow_run("CI", CiStatus::Success, 10));
    assert!(applied);
    assert_eq!(stored.status, CiStatus::Success);
    assert_eq!(runs.len(), 3);
    assert_eq!(runs[1].status, CiStatus::InProgress, "Platforms untouched");
    // An older Platforms run is stale.
    let (applied, stored) = record_run(&mut runs, workflow_run("Platforms", CiStatus::Failure, 5));
    assert!(!applied);
    assert_eq!(stored.started_at_ms, Some(20));
    // Past the cap, the oldest pair goes.
    for index in 0..MAX_RUNS_PER_CHECKOUT {
        let started = 100 + i64::try_from(index).unwrap();
        record_run(
            &mut runs,
            workflow_run(&format!("W{index}"), CiStatus::Success, started),
        );
    }
    assert_eq!(runs.len(), MAX_RUNS_PER_CHECKOUT);
    assert!(runs.iter().all(|run| run.started_at_ms >= Some(100)));
}

#[test]
fn hs2_d3jj49_live_runs_show_each_visible_run_newest_first() {
    let mut runs = CiRunsByCheckout::new();
    assert!(live_runs(&runs, &["a".into()]).is_empty());
    assert_eq!(newest(&runs, &["a".into()]), LiveCi::unavailable());
    let hidden = {
        let mut run = workflow_run("Release", CiStatus::InProgress, 900);
        run.verify(VerificationEvent::Unsettled);
        run
    };
    runs.insert(
        "a".to_owned(),
        vec![
            workflow_run("CI", CiStatus::InProgress, 300),
            workflow_run("Platforms", CiStatus::Queued, 200),
            hidden,
        ],
    );
    // b reports the same branch and workflow as a, newer: it wins the pair.
    runs.insert(
        "b".to_owned(),
        vec![workflow_run("CI", CiStatus::InProgress, 400)],
    );
    runs.insert(
        "c".to_owned(),
        vec![workflow_run("Docs", CiStatus::Queued, 999)],
    );
    let shown = live_runs(&runs, &["b".into(), "a".into(), "a".into()]);
    let summary: Vec<(Option<&str>, Option<&str>, Option<i64>)> = shown
        .iter()
        .map(|ci| {
            (
                ci.checkout.as_deref(),
                ci.workflow.as_deref(),
                ci.started_at_ms,
            )
        })
        .collect();
    assert_eq!(
        summary,
        vec![
            (Some("b"), Some("CI"), Some(400)),
            (Some("a"), Some("Platforms"), Some(200)),
        ],
        "unverifiable and unlinked runs never show"
    );
    assert!(
        shown
            .iter()
            .all(|ci| ci.verification == Some(Verification::Verified))
    );
    assert_eq!(newest(&runs, &["a".into(), "b".into()]), shown[0]);
    // An unlinked checkout list reports unavailable explicitly.
    let none = newest(&runs, &[]);
    assert_eq!(none.status, CiStatus::Unavailable);
    assert_eq!(
        serde_json::to_value(&none).unwrap(),
        serde_json::json!({"status": "unavailable", "checkout": null, "branch": null,
            "sha": null, "run_url": null, "workflow": null, "started_at_ms": null,
            "finished_at_ms": null, "reported_at_ms": null, "verification": null,
            "verify_by_ms": null})
    );
}

#[test]
fn hs2_m8hhea_reports_persist_and_bad_files_are_ignored() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let reports = CiReports::default();
    assert_eq!(single(&home, &reports, "a").status, CiStatus::Unavailable);
    let (applied, _) = reports.record(&home, "a", run(CiStatus::InProgress, Some(10), 1));
    assert!(applied);
    let (applied, stored) = reports.record(&home, "a", run(CiStatus::Queued, Some(5), 2));
    assert!(!applied, "an older run is stale");
    assert_eq!(stored.status, CiStatus::InProgress);
    let (applied, _) = reports.record(&home, "a", run(CiStatus::Success, Some(10), 3));
    assert!(applied);

    // A fresh registry (a restarted server) reads the persisted run.
    let restarted = CiReports::default();
    let live = single(&home, &restarted, "a");
    assert_eq!(live.status, CiStatus::Success);
    assert_eq!(live.reported_at_ms, Some(3));
    let written: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ci_status_path(&home)).unwrap()).unwrap();
    assert_eq!(written["version"], CI_STATUS_VERSION);
    assert!(written["checkouts"]["a"].is_array());

    // An unreadable file or another version starts empty.
    let path = ci_status_path(&home);
    std::fs::write(&path, b"{not json").unwrap();
    assert!(load_runs(&path).is_empty());
    std::fs::write(&path, br#"{"version": 99, "checkouts": {}}"#).unwrap();
    assert!(load_runs(&path).is_empty());
    assert!(load_runs(&dir.path().join("missing.json")).is_empty());
}

/// HS2-D3JJ49: a version 1 file (one run per checkout, no verification) migrates on load.
/// Its active run starts hidden and its finished run stays visible. The next save writes v2.
#[test]
fn hs2_d3jj49_an_old_single_record_file_migrates() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    let legacy = serde_json::json!({"version": 1, "checkouts": {
        "a": {"status": "in_progress", "branch": "main", "sha": "abcdef1", "run_url": null,
              "workflow": "CI", "started_at_ms": 10, "finished_at_ms": null,
              "reported_at_ms": 1_000},
        "b": {"status": "success", "branch": "main", "sha": "abcdef2", "run_url": null,
              "workflow": "Docs", "started_at_ms": 20, "finished_at_ms": 30,
              "reported_at_ms": 40}}});
    std::fs::write(ci_status_path(&home), serde_json::to_vec(&legacy).unwrap()).unwrap();
    let runs = load_runs(&ci_status_path(&home));
    assert_eq!(runs["a"].len(), 1);
    assert_eq!(runs["a"][0].verification(), Verification::Unverifiable);
    assert_eq!(runs["b"][0].verification(), Verification::Verified);
    let shown = live_runs(&runs, &["a".to_owned(), "b".to_owned()]);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].checkout.as_deref(), Some("b"));

    // The next save writes version 2 with both runs.
    let reports = CiReports::default();
    reports.record(&home, "c", run(CiStatus::Queued, Some(50), 60));
    let written = load_runs(&ci_status_path(&home));
    assert_eq!(written.len(), 3);
    let raw: serde_json::Value =
        serde_json::from_slice(&std::fs::read(ci_status_path(&home)).unwrap()).unwrap();
    assert_eq!(raw["version"], 2);
    assert_eq!(raw["checkouts"]["a"][0]["verification"], "unverifiable");
    assert!(raw["checkouts"]["a"][0]["verify_by_ms"].is_null());
}

#[test]
fn hs2_d3jj49_v2_report_only_rows_hide_even_before_the_old_deadline() {
    let mut old = workflow_run("CI", CiStatus::InProgress, 10);
    old.verify_by_ms = Some(i64::MAX);
    let finished = CiRun {
        status: CiStatus::Success,
        workflow: Some("Done".into()),
        ..old.clone()
    };
    let observed = workflow_run("Platforms", CiStatus::Queued, 20);
    let mut missing_evidence =
        serde_json::to_value(workflow_run("Missing", CiStatus::Queued, 30)).unwrap();
    missing_evidence
        .as_object_mut()
        .unwrap()
        .remove("verification");
    let bytes = serde_json::to_vec(&serde_json::json!({"version": 2, "checkouts": {
        "a": [old, finished, observed, missing_evidence]
    }}))
    .unwrap();
    let mut runs = parse_runs(&bytes).unwrap();
    let shown = live_runs(&runs, &["a".into()]);
    assert_eq!(shown.len(), 2);
    assert_eq!(shown[0].workflow.as_deref(), Some("Platforms"));
    assert_eq!(shown[1].workflow.as_deref(), Some("Done"));
    let old = &mut runs.get_mut("a").unwrap()[0];
    assert!(!old.verify(VerificationEvent::Unreachable));
    assert!(!old.visible());
    assert!(old.verify(VerificationEvent::Confirmed));
    assert_eq!(old.verify_by_ms, None);
    assert!(old.visible());
}

#[test]
fn hs2_d3jj49_verify_reaches_only_the_followed_run_in_flight() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let reports = CiReports::default();
    let followed = workflow_run("CI", CiStatus::InProgress, 10);
    reports.record(&home, "a", followed.clone());
    reports.record(
        &home,
        "a",
        workflow_run("Platforms", CiStatus::InProgress, 20),
    );
    // Another checkout, or another commit of the pair, is not the followed run.
    assert!(!reports.verify(&home, "b", &followed, VerificationEvent::Unreachable));
    let mut other = followed.clone();
    other.sha = Some("1234567".into());
    assert!(!reports.verify(&home, "a", &other, VerificationEvent::Unreachable));
    assert!(reports.verify(&home, "a", &followed, VerificationEvent::Unreachable));
    let runs = load_runs(&ci_status_path(&home));
    assert_eq!(runs["a"][0].verification, Verification::Transient);
    assert_eq!(runs["a"][1].verification, Verification::Verified);
    // Once finished, the run ignores the follower's events.
    reports.record(&home, "a", workflow_run("CI", CiStatus::Success, 10));
    assert!(!reports.verify(&home, "a", &followed, VerificationEvent::Unsettled));
}

#[test]
fn hs2_7psbhq_clear_removes_only_the_run_the_caller_saw_and_persists() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let reports = CiReports::default();
    let at = |reported_at_ms: i64| CiClearQuery {
        reported_at_ms: Some(reported_at_ms),
        ..CiClearQuery::default()
    };
    // Nothing stored: nothing cleared, repeatedly.
    assert_eq!(
        reports.clear(&home, "a", &CiClearQuery::default()),
        (false, Vec::new())
    );
    reports.record(&home, "a", run(CiStatus::InProgress, Some(10), 100));
    reports.record(&home, "b", run(CiStatus::Success, Some(10), 100));
    // A newer report than the one the caller saw is kept.
    let (cleared, kept) = reports.clear(&home, "a", &at(99));
    assert!(!cleared);
    assert_eq!(kept[0].reported_at_ms, 100);
    // The run the caller saw is removed, the other checkout's run is untouched.
    assert_eq!(reports.clear(&home, "a", &at(100)), (true, Vec::new()));
    assert_eq!(reports.clear(&home, "a", &at(100)), (false, Vec::new()));
    assert_eq!(
        reports
            .live(&home, &["a".into(), "b".into()])
            .0
            .checkout
            .as_deref(),
        Some("b")
    );
    // Persisted: a restarted server no longer has it.
    let restarted = CiReports::default();
    assert_eq!(single(&home, &restarted, "a").status, CiStatus::Unavailable);
    // Without a filter, whatever is stored is cleared; a new report stores again.
    assert!(restarted.clear(&home, "b", &CiClearQuery::default()).0);
    let (applied, _) = restarted.record(&home, "a", run(CiStatus::Queued, Some(1), 5));
    assert!(applied, "a cleared checkout accepts any later report");
}

/// HS2-D3JJ49: Clear applies per run: a branch or workflow filter leaves the other runs.
#[test]
fn hs2_d3jj49_clear_applies_per_run() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let reports = CiReports::default();
    reports.record(&home, "a", workflow_run("CI", CiStatus::InProgress, 10));
    reports.record(
        &home,
        "a",
        workflow_run("Platforms", CiStatus::InProgress, 10),
    );
    let mut docs = workflow_run("CI", CiStatus::Queued, 30);
    docs.branch = Some("docs".into());
    reports.record(&home, "a", docs);
    // Same time, but the workflow names one run.
    let (cleared, left) = reports.clear(
        &home,
        "a",
        &CiClearQuery {
            reported_at_ms: Some(10),
            workflow: Some("Platforms".into()),
            ..CiClearQuery::default()
        },
    );
    assert!(cleared);
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|run| run.workflow.as_deref() == Some("CI")));
    // A branch filter that matches nothing clears nothing.
    let (cleared, left) = reports.clear(
        &home,
        "a",
        &CiClearQuery {
            branch: Some("release".into()),
            ..CiClearQuery::default()
        },
    );
    assert!(!cleared);
    assert_eq!(left.len(), 2);
    let (cleared, left) = reports.clear(
        &home,
        "a",
        &CiClearQuery {
            branch: Some("docs".into()),
            ..CiClearQuery::default()
        },
    );
    assert!(cleared);
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].branch.as_deref(), Some("main"));
}
