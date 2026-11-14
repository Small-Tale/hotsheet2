//! HS2-SBQ3T8 audit journal sequences; synthetic homes only.

use super::*;

fn record(at_ms: u64, action: &str, outcome: AuditOutcome) -> AuditRecord {
    AuditRecord {
        at_ms,
        actor_role: Some("ai".into()),
        actor_id: Some("worker-1".into()),
        target_id: Some(ulid::Ulid::from(7u128).to_string()),
        action: action.into(),
        operation_id: Some("op-1".into()),
        outcome,
        reason: None,
        permission_id: None,
    }
}
fn journal(home: &std::path::Path) -> AuditJournal {
    AuditJournal::new(home, "owner-0123456789ab").unwrap()
}

#[test]
fn hs2_sbq3t8_absent_journal_reads_empty_without_creating_files() {
    let home = tempfile::tempdir().unwrap();
    let page = journal(home.path()).read(0, 10).unwrap();
    assert_eq!(
        page,
        AuditPage {
            entries: Vec::new(),
            cursor: 0,
            gap: false,
            more: false
        }
    );
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
}

#[test]
fn hs2_sbq3t8_appends_are_ordered_and_cursor_replay_pages_without_loss() {
    let home = tempfile::tempdir().unwrap();
    let log = journal(home.path());
    for index in 1..=5u64 {
        let entry = log
            .append(record(index, "tickets_write", AuditOutcome::Allowed))
            .unwrap();
        assert_eq!(entry.seq, index);
    }
    let first = log.read(0, 2).unwrap();
    assert_eq!(
        first.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(first.more);
    assert!(!first.gap);
    let second = log.read(first.cursor, 2).unwrap();
    assert_eq!(
        second.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [3, 4]
    );
    let last = log.read(second.cursor, 2).unwrap();
    assert_eq!(last.entries.len(), 1);
    assert!(!last.more);
    // Caught up: the cursor holds still and a repeat returns nothing new.
    let idle = log.read(last.cursor, 2).unwrap();
    assert!(idle.entries.is_empty());
    assert_eq!(idle.cursor, 5);
    // Empty then refill resumes from the same cursor.
    log.append(record(6, "work_start", AuditOutcome::Denied))
        .unwrap();
    let refilled = log.read(idle.cursor, 2).unwrap();
    assert_eq!(refilled.entries[0].seq, 6);
    assert_eq!(refilled.entries[0].record.outcome, AuditOutcome::Denied);
    // Owners do not share a journal.
    let other = AuditJournal::new(home.path(), "other-0123456789ab").unwrap();
    assert!(other.read(0, 10).unwrap().entries.is_empty());
    assert_eq!(
        other
            .append(record(1, "tickets_read", AuditOutcome::Allowed))
            .unwrap()
            .seq,
        1
    );
}

#[test]
fn hs2_sbq3t8_rotation_keeps_sequence_and_reports_a_gap_to_a_stale_cursor() {
    let home = tempfile::tempdir().unwrap();
    let log = journal(home.path()).with_segment_limit(600);
    for index in 1..=12u64 {
        log.append(record(index, "tickets_write", AuditOutcome::Succeeded))
            .unwrap();
    }
    let all = log.read(0, 500).unwrap();
    let seqs: Vec<_> = all.entries.iter().map(|entry| entry.seq).collect();
    // Only the current and one previous segment are retained, contiguous to the end.
    assert_eq!(*seqs.last().unwrap(), 12);
    assert!(seqs.len() < 12);
    assert!(seqs.windows(2).all(|pair| pair[1] == pair[0] + 1));
    assert!(all.gap);
    // A cursor inside retained history sees no gap.
    let recent = log.read(seqs[0], 500).unwrap();
    assert!(!recent.gap);
    assert_eq!(recent.entries.len(), seqs.len() - 1);
    assert!(
        std::fs::metadata(
            home.path()
                .join("orchestration-audit/owner-0123456789ab.jsonl")
        )
        .unwrap()
        .len()
            <= 600
    );
}

#[test]
fn hs2_sbq3t8_torn_final_line_is_skipped_and_the_next_append_stays_parseable() {
    let home = tempfile::tempdir().unwrap();
    let log = journal(home.path());
    log.append(record(1, "tickets_write", AuditOutcome::Allowed))
        .unwrap();
    log.append(record(2, "tickets_write", AuditOutcome::Succeeded))
        .unwrap();
    let path = home
        .path()
        .join("orchestration-audit/owner-0123456789ab.jsonl");
    let mut bytes = std::fs::read(&path).unwrap();
    bytes.extend_from_slice(br#"{"seq":3,"at_ms":3,"action":"tick"#);
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(log.read(0, 10).unwrap().entries.len(), 2);
    let next = log
        .append(record(4, "work_start", AuditOutcome::Denied))
        .unwrap();
    assert_eq!(next.seq, 3);
    let page = log.read(0, 10).unwrap();
    assert_eq!(
        page.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [1, 2, 3]
    );
    assert_eq!(page.entries[2].record.action, "work_start");
    // The torn bytes are still there: history is never rewritten.
    assert!(std::fs::read(&path).unwrap().starts_with(&bytes));
}

#[test]
fn hs2_sbq3t8_records_carry_codes_only_and_reject_free_text() {
    let home = tempfile::tempdir().unwrap();
    let log = journal(home.path());
    let mut free_text = record(1, "tickets_write", AuditOutcome::Failed);
    free_text.reason = Some("the customer said no".into());
    let mut bad_action = record(1, "create ticket", AuditOutcome::Allowed);
    bad_action.reason = None;
    let mut bad_target = record(1, "tickets_write", AuditOutcome::Allowed);
    bad_target.target_id = Some("label".into());
    let mut bad_operation = record(1, "tickets_write", AuditOutcome::Allowed);
    bad_operation.operation_id = Some("a/b".into());
    for bad in [free_text, bad_action, bad_target, bad_operation] {
        assert!(matches!(
            log.append(bad),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    assert!(log.read(0, 10).unwrap().entries.is_empty());
    assert!(AuditJournal::new(home.path(), "../escape").is_err());
    assert!(AuditJournal::new(home.path(), "a/b").is_err());
}

#[test]
fn hs2_sbq3t8_concurrent_writers_never_duplicate_or_lose_a_sequence() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().to_owned();
    let handles: Vec<_> = (0..8)
        .map(|writer| {
            let path = path.clone();
            std::thread::spawn(move || {
                let log = journal(&path);
                for index in 0..25u64 {
                    log.append(record(
                        writer * 100 + index,
                        "tickets_write",
                        AuditOutcome::Allowed,
                    ))
                    .unwrap();
                }
            })
        })
        .collect();
    for handle in handles {
        handle.join().unwrap();
    }
    let page = journal(&path).read(0, 500).unwrap();
    assert_eq!(
        page.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        (1..=200).collect::<Vec<_>>()
    );
}

#[cfg(unix)]
#[test]
fn hs2_sbq3t8_journal_refuses_symlinked_storage() {
    let home = tempfile::tempdir().unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(elsewhere.path(), home.path().join("orchestration-audit")).unwrap();
    let log = journal(home.path());
    assert!(
        log.append(record(1, "tickets_write", AuditOutcome::Allowed))
            .is_err()
    );
    assert!(log.read(0, 10).is_err());
    assert!(
        std::fs::read_dir(elsewhere.path())
            .unwrap()
            .next()
            .is_none()
    );
}
