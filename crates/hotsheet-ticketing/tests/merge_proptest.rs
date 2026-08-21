//! Property tests for the semantic ticket merge (`docs/02` §2.7, HS2-18): the invariants
//! the field-by-field merge must hold for *any* pair of edits over a base.

use std::collections::BTreeSet;

use hotsheet_model::{Status, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::merge_tickets;
use proptest::prelude::*;

const STATUSES: [Status; 4] = [
    Status::NotStarted,
    Status::Started,
    Status::Completed,
    Status::Backlog,
];

/// A ticket differing from others only in the fields we vary (status, tags, body, clock).
fn tk(status_ix: usize, tag_ids: &[u8], details: &str, updated: &str) -> Ticket {
    let mut t = Ticket::new(
        Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap(),
        "HS-1",
        "title",
        "bug",
        Timestamp::from("2026-01-01T00:00:00Z"),
        Timestamp::from(updated),
    );
    t.status = STATUSES[status_ix % STATUSES.len()];
    // Dedup + stable order so equal sets compare equal.
    t.tags = tag_ids
        .iter()
        .map(|b| format!("t{}", b % 6))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    t.details = details.to_string();
    t
}

proptest! {
    /// merge(x, x, x) == x on every field we merge — no spurious change.
    #[test]
    fn merge_of_identical_sides_is_the_input(
        sx in 0usize..4,
        tags in prop::collection::vec(0u8..6, 0..6),
        body in "[a-z ]{0,24}",
    ) {
        let x = tk(sx, &tags, &body, "2026-01-02T00:00:00Z");
        let m = merge_tickets(&x, &x, &x).ticket;
        prop_assert_eq!(m.status, x.status);
        prop_assert_eq!(&m.tags, &x.tags);
        prop_assert_eq!(&m.details, &x.details);
    }

    /// A scalar changed on exactly one side takes that side's value (no base wins).
    #[test]
    fn one_sided_scalar_change_wins(bs in 0usize..4, os in 0usize..4) {
        let base = tk(bs, &[], "body", "2026-01-01T00:00:00Z");
        let mut ours = base.clone();
        ours.status = STATUSES[os];
        ours.updated_at = Timestamp::from("2026-01-02T00:00:00Z");
        let theirs = base.clone(); // untouched
        let m = merge_tickets(&base, &ours, &theirs).ticket;
        prop_assert_eq!(m.status, ours.status);
    }

    /// When both sides change a scalar to different values, the newer `updated_at` wins.
    #[test]
    fn both_sided_scalar_change_is_last_writer_wins(
        bs in 0usize..4, os in 0usize..4, tsx in 0usize..4, ours_newer in any::<bool>(),
    ) {
        let base = tk(bs, &[], "body", "2026-01-01T00:00:00Z");
        let (ots, tts) = if ours_newer {
            ("2026-01-03T00:00:00Z", "2026-01-02T00:00:00Z")
        } else {
            ("2026-01-02T00:00:00Z", "2026-01-03T00:00:00Z")
        };
        let mut ours = base.clone();
        ours.status = STATUSES[os];
        ours.updated_at = Timestamp::from(ots);
        let mut theirs = base.clone();
        theirs.status = STATUSES[tsx];
        theirs.updated_at = Timestamp::from(tts);

        let m = merge_tickets(&base, &ours, &theirs).ticket;
        // Only a real both-sides-differ conflict is governed by last-writer-wins.
        if ours.status != base.status && theirs.status != base.status && ours.status != theirs.status {
            let want = if ours_newer { ours.status } else { theirs.status };
            prop_assert_eq!(m.status, want);
        }
    }

    /// Tag set union: a tag is in the result iff some side has it AND it wasn't deleted
    /// (present in base, removed on a side). No duplicates.
    #[test]
    fn tag_union_honors_adds_and_deletes(
        base_t in prop::collection::vec(0u8..6, 0..6),
        ours_t in prop::collection::vec(0u8..6, 0..6),
        theirs_t in prop::collection::vec(0u8..6, 0..6),
    ) {
        let base = tk(0, &base_t, "b", "2026-01-01T00:00:00Z");
        let ours = tk(0, &ours_t, "b", "2026-01-02T00:00:00Z");
        let theirs = tk(0, &theirs_t, "b", "2026-01-02T00:00:00Z");
        let m = merge_tickets(&base, &ours, &theirs).ticket;

        let has = |v: &[String], k: &str| v.iter().any(|x| x == k);
        for i in 0..6u8 {
            let k = format!("t{i}");
            let removed_ours = has(&base.tags, &k) && !has(&ours.tags, &k);
            let removed_theirs = has(&base.tags, &k) && !has(&theirs.tags, &k);
            let present = has(&ours.tags, &k) || has(&theirs.tags, &k);
            let expected = present && !removed_ours && !removed_theirs;
            prop_assert_eq!(has(&m.tags, &k), expected, "tag {} membership", k);
        }
        // No duplicates.
        let uniq: BTreeSet<&String> = m.tags.iter().collect();
        prop_assert_eq!(uniq.len(), m.tags.len());
    }
}
