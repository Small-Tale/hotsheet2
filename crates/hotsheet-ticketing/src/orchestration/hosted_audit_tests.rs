//! HS2-CRBR8X: the hosting machine's session journal; synthetic homes only.

use super::*;

fn record(at_ms: u64, event: HostedSessionEvent) -> HostedSessionAuditRecord {
    HostedSessionAuditRecord {
        at_ms,
        event,
        outcome: AuditOutcome::Succeeded,
        reason: None,
        operation_id: "op-1".into(),
        session_id: Some(ulid::Ulid::from(9u128).to_string()),
        tool: Some("codex".into()),
        origin_checkout_id: Some("release-train-0123456789ab".into()),
        requester: Some(format!("cert-{}", "ab".repeat(32))),
        actor_role: Some("ai".into()),
        actor_id: Some("scheduler-1".into()),
    }
}

#[test]
fn hs2_crbr8x_the_host_journal_is_its_own_file_beside_the_orchestration_audit() {
    let home = tempfile::tempdir().unwrap();
    let hosted = HostedSessionAuditJournal::new(home.path(), "peer-project-0123456789ab").unwrap();
    hosted
        .append(record(10, HostedSessionEvent::Start))
        .unwrap();
    assert!(
        home.path()
            .join("hosted-session-audit/peer-project-0123456789ab.jsonl")
            .is_file()
    );
    // The orchestration audit of the same checkout is untouched.
    let audit = AuditJournal::new(home.path(), "peer-project-0123456789ab").unwrap();
    assert!(audit.read(0, 10).unwrap().entries.is_empty());
    assert!(!home.path().join("orchestration-audit").exists());
}

#[test]
fn hs2_crbr8x_entries_replay_in_order_and_survive_a_new_reader() {
    let home = tempfile::tempdir().unwrap();
    let journal = HostedSessionAuditJournal::new(home.path(), "peer-0123456789ab").unwrap();
    let mut refused = record(5, HostedSessionEvent::Start);
    refused.outcome = AuditOutcome::Denied;
    refused.reason = Some("max_sessions".into());
    refused.session_id = None;
    journal.append(refused.clone()).unwrap();
    journal
        .append(record(10, HostedSessionEvent::Start))
        .unwrap();
    let mut stop = record(20, HostedSessionEvent::Stop);
    stop.reason = Some("wound_down".into());
    journal.append(stop.clone()).unwrap();
    // A process that starts later (a restart) reads the same history.
    let again = HostedSessionAuditJournal::new(home.path(), "peer-0123456789ab").unwrap();
    let page = again.read(0, 10).unwrap();
    let seen: Vec<_> = page
        .entries
        .iter()
        .map(|entry| (entry.seq, entry.record.clone()))
        .collect();
    assert_eq!(
        seen,
        [
            (1, refused),
            (2, record(10, HostedSessionEvent::Start)),
            (3, stop)
        ]
    );
    assert_eq!(page.cursor, 3);
    assert!(again.read(3, 10).unwrap().entries.is_empty());
    assert_eq!(again.read_newest(None, 1).unwrap().entries[0].seq, 3);
}

#[test]
fn hs2_crbr8x_entries_hold_identifiers_and_codes_only() {
    let home = tempfile::tempdir().unwrap();
    let journal = HostedSessionAuditJournal::new(home.path(), "peer-0123456789ab").unwrap();
    let free_text = [
        |r: &mut HostedSessionAuditRecord| r.reason = Some("the user asked nicely".into()),
        |r: &mut HostedSessionAuditRecord| r.tool = Some("codex --yolo".into()),
        |r: &mut HostedSessionAuditRecord| r.requester = Some("https://peer".into()),
        |r: &mut HostedSessionAuditRecord| r.session_id = Some("terminal one".into()),
        |r: &mut HostedSessionAuditRecord| r.operation_id = String::new(),
    ];
    for change in free_text {
        let mut bad = record(1, HostedSessionEvent::Start);
        change(&mut bad);
        assert!(journal.append(bad).is_err());
    }
    assert!(journal.read(0, 10).unwrap().entries.is_empty());
}

#[test]
fn hs2_crbr8x_the_host_journal_is_bounded_like_the_orchestration_audit() {
    let home = tempfile::tempdir().unwrap();
    let journal = HostedSessionAuditJournal::new(home.path(), "peer-0123456789ab")
        .unwrap()
        .with_segment_limit(1024);
    for at in 0..40 {
        journal
            .append(record(at, HostedSessionEvent::Start))
            .unwrap();
    }
    let dir = home.path().join("hosted-session-audit");
    let current = std::fs::metadata(dir.join("peer-0123456789ab.jsonl")).unwrap();
    let previous = std::fs::metadata(dir.join("peer-0123456789ab.1.jsonl")).unwrap();
    assert!(current.len() <= 1024 && previous.len() <= 1024);
    // Rotation drops the oldest entries; the sequence keeps counting.
    let page = journal.read(0, MAX_AUDIT_PAGE).unwrap();
    assert!(page.gap);
    assert_eq!(page.entries.last().unwrap().seq, 40);
    assert!(page.entries.len() < 40);
}
