//! HS2-7QHWHJ run event stream: ordering, duplicates, replay and retention.

use super::*;

const OWNER: &str = "owner-0123456789ab";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn event(run: u128, at_ms: u64, kind: RunEventKind, dedupe: Option<&str>) -> RunEvent {
    RunEvent {
        at_ms,
        run_id: id(run),
        kind,
        source: RunEventSource::Target,
        state: None,
        reason: None,
        actor_id: None,
        dedupe: dedupe.map(Into::into),
    }
}

#[test]
fn hs2_7qhwhj_duplicate_and_out_of_order_reports_are_recorded_once_in_arrival_order() {
    let home = tempfile::tempdir().unwrap();
    let journal = RunEventJournal::new(home.path(), OWNER).unwrap();
    assert!(journal.read(0, 10, None).unwrap().entries.is_empty());
    assert!(std::fs::read_dir(home.path()).unwrap().next().is_none());
    let (first, appended) = journal
        .record(event(1, 500, RunEventKind::Observed, Some("obs-a")))
        .unwrap();
    assert!(appended);
    assert_eq!(first.seq, 1);
    // The same fact reported again, by anyone and at any later time, is not a new event.
    for at_ms in [500, 900] {
        let (again, appended) = journal
            .record(event(1, at_ms, RunEventKind::Observed, Some("obs-a")))
            .unwrap();
        assert!(!appended);
        assert_eq!(again, first);
    }
    // The same key on another run is that run's own event.
    let (other, appended) = journal
        .record(event(2, 500, RunEventKind::Observed, Some("obs-a")))
        .unwrap();
    assert!(appended);
    assert_eq!(other.seq, 2);
    // An older fact arriving late is appended in arrival order and keeps its own time.
    let (late, appended) = journal
        .record(event(1, 100, RunEventKind::Observed, Some("obs-earlier")))
        .unwrap();
    assert!(appended);
    assert_eq!((late.seq, late.record.at_ms), (3, 100));
    // Events without a key are always distinct.
    for _ in 0..2 {
        assert!(
            journal
                .record(event(1, 600, RunEventKind::Lease, None))
                .unwrap()
                .1
        );
    }
    let all = journal.read(0, 100, None).unwrap();
    assert_eq!(
        all.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [1, 2, 3, 4, 5]
    );
    assert!(!all.gap);
    // Free text is refused at the boundary.
    let mut text = event(1, 700, RunEventKind::State, None);
    text.reason = Some("the worker gave up".into());
    assert!(matches!(
        journal.record(text),
        Err(OrchestrationError::Invalid(_))
    ));
    let mut bad_run = event(1, 700, RunEventKind::State, None);
    bad_run.run_id = "label".into();
    assert!(journal.record(bad_run).is_err());
    assert_eq!(journal.read(0, 100, None).unwrap().entries.len(), 5);
}

#[test]
fn hs2_7qhwhj_a_run_filter_pages_and_resumes_past_other_runs_events() {
    let home = tempfile::tempdir().unwrap();
    let journal = RunEventJournal::new(home.path(), OWNER).unwrap();
    for index in 0..10u64 {
        journal
            .record(event(
                u128::from(index % 2) + 1,
                index,
                RunEventKind::Lease,
                None,
            ))
            .unwrap();
    }
    let run_one = id(1);
    let first = journal.read(0, 2, Some(&run_one)).unwrap();
    assert_eq!(
        first.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [1, 3]
    );
    assert!(first.more);
    assert_eq!(first.cursor, 3);
    let rest = journal.read(first.cursor, 10, Some(&run_one)).unwrap();
    assert_eq!(
        rest.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        [5, 7, 9]
    );
    // Caught up: the cursor moves past the other run's trailing event too, so the
    // next read examines nothing twice.
    assert!(!rest.more);
    assert_eq!(rest.cursor, 10);
    let idle = journal.read(rest.cursor, 10, Some(&run_one)).unwrap();
    assert!(idle.entries.is_empty());
    assert_eq!(idle.cursor, 10);
    // Empty then refill resumes from the same cursor.
    journal
        .record(event(1, 99, RunEventKind::State, None))
        .unwrap();
    assert_eq!(
        journal
            .read(idle.cursor, 10, Some(&run_one))
            .unwrap()
            .entries[0]
            .seq,
        11
    );
    // A run with no events answers empty and still advances.
    let none = journal.read(0, 10, Some(&id(9))).unwrap();
    assert!(none.entries.is_empty());
    assert_eq!(none.cursor, 11);
    assert!(journal.read(0, 10, Some("not-a-run")).is_err());
}

#[test]
fn hs2_7qhwhj_retention_reports_a_gap_and_dedupe_still_holds_for_retained_events() {
    let home = tempfile::tempdir().unwrap();
    let journal = RunEventJournal::new(home.path(), OWNER)
        .unwrap()
        .with_segment_limit(700);
    for index in 0..20u64 {
        journal
            .record(event(
                1,
                index,
                RunEventKind::Observed,
                Some(&format!("obs-{index}")),
            ))
            .unwrap();
    }
    let all = journal.read(0, 500, None).unwrap();
    let seqs: Vec<_> = all.entries.iter().map(|entry| entry.seq).collect();
    assert_eq!(*seqs.last().unwrap(), 20);
    assert!(seqs.len() < 20);
    assert!(seqs.windows(2).all(|pair| pair[1] == pair[0] + 1));
    // A reader that fell behind retention is told so; one inside it is not.
    assert!(all.gap);
    assert!(!journal.read(seqs[0], 500, None).unwrap().gap);
    // A retained fact reported again is still recognized.
    let (_, appended) = journal
        .record(event(1, 19, RunEventKind::Observed, Some("obs-19")))
        .unwrap();
    assert!(!appended);
    // A fact older than retention is appended again rather than silently dropped:
    // the journal cannot prove it was seen, and consumers apply events idempotently.
    let (again, appended) = journal
        .record(event(1, 0, RunEventKind::Observed, Some("obs-0")))
        .unwrap();
    assert!(appended);
    assert_eq!(again.seq, 21);
}

#[test]
fn hs2_7qhwhj_concurrent_reporters_of_one_fact_record_it_once() {
    let home = tempfile::tempdir().unwrap();
    let path = home.path().to_owned();
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let path = path.clone();
            std::thread::spawn(move || {
                let journal = RunEventJournal::new(&path, OWNER).unwrap();
                (0..10u64)
                    .filter(|index| {
                        journal
                            .record(event(
                                1,
                                *index,
                                RunEventKind::Observed,
                                Some(&format!("obs-{index}")),
                            ))
                            .unwrap()
                            .1
                    })
                    .count()
            })
        })
        .collect();
    let appended: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();
    assert_eq!(appended, 10);
    let page = RunEventJournal::new(&path, OWNER)
        .unwrap()
        .read(0, 500, None)
        .unwrap();
    assert_eq!(
        page.entries.iter().map(|e| e.seq).collect::<Vec<_>>(),
        (1..=10).collect::<Vec<_>>()
    );
}
