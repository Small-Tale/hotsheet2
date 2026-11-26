//! HS2-KB2Y51 pushed line observations: the lifetime transitions (absent, stored,
//! refreshed, stale, expired, withdrawn, refilled), bounds, two owner machines
//! interleaved, and the merged fold. Synthetic ids only.

use serde_json::json;

use super::*;

const OWNER_A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA0";
const OWNER_B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FB0";
const SOURCE: &str = "source-a";

fn line(owner: &str, step: &str, native: &str, state: &str, connection: &str) -> TargetLine {
    serde_json::from_value(json!({
        "owner_checkout_id": owner,
        "owner_label": owner.to_uppercase(),
        "proposed": false,
        "item": {
            "step_id": step,
            "goal": {"connection_id": "goals", "native_id": format!("goal-{step}")},
            "goal_slug": "LCH-1",
            "target": {"target_id": "api", "connection_id": connection, "native_id": native},
            "project_label": "API",
            "location": "remote",
            "machine_label": "build-box",
            "session_id": "session-1",
            "state": state,
            "level": "confirmed",
            "reasons": ["held"],
            "run": {"run_id": "run-1", "state": "dispatched", "paused": false, "cancel_requested": false, "created_at_ms": 1},
            "permission": {"kind": "permission", "request_id": "req-1", "action": "session_start"},
            "decision": {"note_id": "note-1"},
        },
    }))
    .unwrap()
}

fn post(owner: &str, sequence: u64, lines: Vec<TargetLine>) -> ObservedLines {
    ObservedLines {
        owner_server_id: owner.into(),
        sequence,
        ttl_ms: Some(60_000),
        lines,
    }
}

fn sources() -> Vec<String> {
    vec![SOURCE.to_owned()]
}

fn shown(file: &ObservationFile, now: u64) -> Vec<(String, usize)> {
    file.live(now)
        .iter()
        .map(|entry| (entry.owner_server_id.clone(), entry.lines.len()))
        .collect()
}

#[test]
fn hs2_kb2y51_a_push_lives_until_refreshed_withdrawn_or_expired() {
    let mut file = ObservationFile::default();
    assert!(file.live(0).is_empty());
    let first = line("launch", "s1", "t1", "ready", SOURCE);
    // Absent -> stored.
    let stored = file
        .apply(post(OWNER_A, 1, vec![first.clone()]), &sources(), 1_000)
        .unwrap();
    assert_eq!(stored.outcome, "stored");
    assert_eq!((stored.accepted, stored.dropped), (1, 0));
    assert_eq!(stored.expires_at_ms, Some(61_000));
    assert_eq!(shown(&file, 1_000), [(OWNER_A.to_owned(), 1)]);
    // Stored -> refreshed by a newer sequence; the lifetime restarts.
    let refreshed = file
        .apply(post(OWNER_A, 2, vec![first.clone()]), &sources(), 30_000)
        .unwrap();
    assert_eq!(refreshed.expires_at_ms, Some(90_000));
    // A repeated or older sequence is stale and changes nothing.
    for sequence in [2, 1] {
        let before = file.clone();
        let stale = file
            .apply(post(OWNER_A, sequence, vec![]), &sources(), 31_000)
            .unwrap();
        assert_eq!(stale.outcome, "stale");
        assert_eq!(file, before);
    }
    // Refreshed -> expired at its deadline, never before.
    assert_eq!(shown(&file, 89_999).len(), 1);
    assert!(file.live(90_000).is_empty());
    // Expired -> refilled: an expired entry is pruned, so any sequence starts again.
    file.apply(post(OWNER_A, 1, vec![first.clone()]), &sources(), 100_000)
        .unwrap();
    assert_eq!(shown(&file, 100_000).len(), 1);
    // Live -> withdrawn by an empty push; then refilled again.
    let withdrawn = file
        .apply(post(OWNER_A, 5, vec![]), &sources(), 100_001)
        .unwrap();
    assert_eq!(withdrawn.outcome, "withdrawn");
    assert!(file.live(100_001).is_empty() && file.entries.is_empty());
    file.apply(post(OWNER_A, 6, vec![first]), &sources(), 100_002)
        .unwrap();
    assert_eq!(shown(&file, 100_002).len(), 1);
    // Withdrawing what was never pushed is a no-op answer.
    let none = file
        .apply(post(OWNER_B, 1, vec![]), &sources(), 100_003)
        .unwrap();
    assert_eq!(none.outcome, "withdrawn");
    assert_eq!(file.entries.len(), 1);
}

#[test]
fn hs2_kb2y51_only_this_checkouts_tickets_are_kept_as_display_facts() {
    let mut file = ObservationFile::default();
    let outcome = file
        .apply(
            post(
                OWNER_A,
                1,
                vec![
                    line("launch", "s1", "t1", "running", SOURCE),
                    line("launch", "s2", "t2", "ready", "someone-elses-source"),
                ],
            ),
            &sources(),
            0,
        )
        .unwrap();
    assert_eq!((outcome.accepted, outcome.dropped), (1, 1));
    let kept = &file.live(0)[0].lines[0];
    assert_eq!(kept.observed_from.as_deref(), Some(OWNER_A));
    let item = &kept.item;
    assert_eq!(item.state, ItemState::Running);
    assert_eq!(item.goal_slug.as_deref(), Some("LCH-1"));
    // Runs where the target lives: the receiving machine.
    assert_eq!(item.location, TargetLocation::Local);
    assert!(item.machine_label.is_none());
    // No owner-side identifiers cross machines.
    assert!(item.session_id.is_none() && item.run.is_none());
    assert!(item.permission.is_none() && item.decision.is_none());
    assert!(item.reasons.is_empty());
    // Everything dropped still stores an empty entry that shows nothing.
    let mut empty = ObservationFile::default();
    empty
        .apply(
            post(OWNER_A, 1, vec![line("launch", "s2", "t2", "ready", "x")]),
            &sources(),
            0,
        )
        .unwrap();
    assert!(empty.live(0).is_empty());
}

#[test]
fn hs2_kb2y51_pushes_are_bounded_in_size_owners_rate_and_lifetime() {
    let mut file = ObservationFile::default();
    let one = || vec![line("launch", "s1", "t1", "ready", SOURCE)];
    // Malformed owner ids.
    for owner in ["", "not-a-ulid", "01arz3ndektsv4rrffq69g5fa0"] {
        assert!(matches!(
            file.apply(post(owner, 1, one()), &sources(), 0),
            Err(OrchestrationError::Invalid(_))
        ));
    }
    // Too many lines, and one line too large.
    let many = (0..=MAX_OBSERVED_LINES)
        .map(|n| {
            line(
                "launch",
                &format!("s{n}"),
                &format!("t{n}"),
                "ready",
                SOURCE,
            )
        })
        .collect();
    assert!(file.apply(post(OWNER_A, 1, many), &sources(), 0).is_err());
    let mut huge = line("launch", "s1", "t1", "ready", SOURCE);
    huge.item.goal_title = Some("x".repeat(5_000));
    assert!(
        file.apply(post(OWNER_A, 1, vec![huge]), &sources(), 0)
            .is_err()
    );
    assert!(file.entries.is_empty());
    // The lifetime is clamped both ways, and defaults when not named.
    for (ttl, expected) in [
        (Some(1), MIN_OBSERVATION_TTL_MS),
        (Some(u64::MAX), MAX_OBSERVATION_TTL_MS),
        (None, DEFAULT_OBSERVATION_TTL_MS),
    ] {
        let mut file = ObservationFile::default();
        let mut push = post(OWNER_A, 1, one());
        push.ttl_ms = ttl;
        assert_eq!(
            file.apply(push, &sources(), 0).unwrap().expires_at_ms,
            Some(expected)
        );
    }
    // At most MAX_OBSERVATION_OWNERS machines; an expired one frees its slot.
    let owner = |n: u128| ulid::Ulid::from(n + 1).to_string();
    for n in 0..MAX_OBSERVATION_OWNERS as u128 {
        file.apply(post(&owner(n), 1, one()), &sources(), 0)
            .unwrap();
    }
    let extra = owner(100);
    assert!(matches!(
        file.apply(post(&extra, 1, one()), &sources(), 1),
        Err(OrchestrationError::Capacity)
    ));
    // A known owner still refreshes at capacity.
    file.apply(post(&owner(0), 2, one()), &sources(), 1)
        .unwrap();
    assert!(
        file.apply(post(&extra, 1, one()), &sources(), 60_001)
            .is_ok()
    );
    // MAX_PUSHES_PER_MINUTE per owner and minute, then 429 until the minute passes.
    let mut file = ObservationFile::default();
    for sequence in 1..=MAX_PUSHES_PER_MINUTE as u64 {
        file.apply(post(OWNER_A, sequence, one()), &sources(), sequence)
            .unwrap();
    }
    assert!(matches!(
        file.apply(post(OWNER_A, 100, one()), &sources(), 59_000),
        Err(OrchestrationError::TargetOperation(
            TargetOpError::RateLimited
        ))
    ));
    // Another owner is not limited by this one.
    file.apply(post(OWNER_B, 1, one()), &sources(), 59_000)
        .unwrap();
    assert!(
        file.apply(post(OWNER_A, 100, one()), &sources(), 60_001 + 30)
            .is_ok()
    );
}

fn timeline(items: &[serde_json::Value]) -> Timeline {
    serde_json::from_value(json!({
        "version": "v", "now_ms": 1, "declared": true, "phase": "running",
        "automatic": true, "kick_off": {"can_start": false, "can_pause": false},
        "sessions": 0, "targets": [],
        "waves": [{"kind": "next", "ordinal": 0, "items": items}],
        "counts": {}, "wave_minutes": 60, "estimate": "default",
    }))
    .unwrap()
}

#[test]
fn hs2_kb2y51_pushed_lines_merge_after_this_machines_own_by_rank() {
    let local_item = serde_json::to_value(&line("here", "s9", "t1", "ready", SOURCE).item).unwrap();
    let view = timeline(&[local_item]);
    let api = vec!["api".to_owned()];
    let local = [TargetLineSource {
        owner_checkout_id: "here",
        owner_label: "Here",
        target_ids: &api,
        timeline: &view,
        sites: &ProjectSites::default(),
    }];
    let a = [line("launch", "s1", "t1", "running", SOURCE)];
    let b = [
        line("launch", "s1", "t1", "ready", SOURCE),
        line("launch", "s2", "t2", "ready", SOURCE),
    ];
    let observed = |order: &[(&'static str, &'static [TargetLine])]| {
        order
            .iter()
            .map(|(owner, lines)| ObservedOwner {
                owner_server_id: owner,
                lines,
            })
            .collect::<Vec<_>>()
    };
    let a: &'static [TargetLine] = Box::leak(Box::new(a));
    let b: &'static [TargetLine] = Box::leak(Box::new(b));
    let both = target_lines_observed(&local, &observed(&[(OWNER_A, a), (OWNER_B, b)]));
    assert!(both.orchestrated);
    let t1 = both
        .lines
        .iter()
        .find(|line| line.item.target.native_id == "t1")
        .unwrap();
    // Running work outranks the local ready step; the same owner checkout id and step
    // on two machines are two steps, not one.
    assert_eq!(t1.item.state, ItemState::Running);
    assert_eq!(t1.observed_from.as_deref(), Some(OWNER_A));
    assert_eq!(t1.also_planned, 2);
    let t2 = both
        .lines
        .iter()
        .find(|line| line.item.target.native_id == "t2")
        .unwrap();
    assert_eq!(t2.observed_from.as_deref(), Some(OWNER_B));
    // Repeating an owner, or interleaving the owners, changes nothing in what shows.
    let repeated = target_lines_observed(
        &local,
        &observed(&[(OWNER_A, a), (OWNER_B, b), (OWNER_A, a)]),
    );
    assert_eq!(repeated.lines, both.lines);
    let swapped = target_lines_observed(&local, &observed(&[(OWNER_B, b), (OWNER_A, a)]));
    assert_eq!(swapped.lines.len(), both.lines.len());
    assert_eq!(swapped.lines[0].item.state, ItemState::Running);
    // Without pushed lines the fold is exactly the local one.
    assert_eq!(target_lines_observed(&local, &[]), target_lines(&local));
    assert!(target_lines(&local).lines[0].observed_from.is_none());
    // Only pushed lines still make the checkout orchestrated; none at all does not.
    let only = target_lines_observed(&[], &observed(&[(OWNER_A, a)]));
    assert!(only.orchestrated && only.lines.len() == 1);
    let empty: &'static [TargetLine] = &[];
    let none = target_lines_observed(&[], &observed(&[(OWNER_A, empty)]));
    assert!(!none.orchestrated);
    assert_ne!(none.version, only.version);
}

#[test]
fn hs2_kb2y51_the_registry_keeps_one_private_file_per_checkout() {
    let home = tempfile::tempdir().unwrap();
    let registry = LineObservations::new(home.path());
    assert!(registry.live("api-0123456789ab", 0).is_empty());
    assert!(!registry.directory().exists());
    let pushed = vec![line("launch", "s1", "t1", "ready", SOURCE)];
    registry
        .put(
            "api-0123456789ab",
            post(OWNER_A, 1, pushed.clone()),
            &sources(),
            0,
        )
        .unwrap();
    assert_eq!(registry.live("api-0123456789ab", 0).len(), 1);
    assert!(registry.live("other-0123456789", 0).is_empty());
    // A stale push writes nothing.
    let file = registry.directory().join("api-0123456789ab.json");
    let before = std::fs::read(&file).unwrap();
    let stale = registry
        .put("api-0123456789ab", post(OWNER_A, 1, pushed), &sources(), 1)
        .unwrap();
    assert_eq!(stale.outcome, "stale");
    assert_eq!(std::fs::read(&file).unwrap(), before);
    assert!(registry.live("api-0123456789ab", 60_000).is_empty());
    registry
        .put(
            "api-0123456789ab",
            post(
                OWNER_A,
                2,
                vec![line("launch", "s1", "t1", "ready", SOURCE)],
            ),
            &sources(),
            60_000,
        )
        .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    // A path-like checkout id is refused; a damaged file shows nothing.
    assert!(
        registry
            .put("../x", post(OWNER_A, 9, vec![]), &sources(), 0)
            .is_err()
    );
    std::fs::write(&file, b"not json").unwrap();
    assert!(registry.live("api-0123456789ab", 0).is_empty());
}

const CHECKOUT: &str = "api-0123456789ab";

fn stored(registry: &LineObservations) -> Option<ObservationFile> {
    let path = registry.directory().join(format!("{CHECKOUT}.json"));
    std::fs::read(path)
        .ok()
        .map(|bytes| serde_json::from_slice(&bytes).unwrap())
}

fn stored_owners(registry: &LineObservations) -> Option<Vec<String>> {
    stored(registry).map(|file| {
        file.entries
            .into_iter()
            .map(|entry| entry.owner_server_id)
            .collect()
    })
}

fn push(registry: &LineObservations, owner: &str, sequence: u64, at: u64) -> String {
    registry
        .put(
            CHECKOUT,
            post(
                owner,
                sequence,
                vec![line("launch", "s1", "t1", "ready", SOURCE)],
            ),
            &sources(),
            at,
        )
        .unwrap()
        .outcome
}

/// HS2-2WMQER: a read that finds an expired entry prunes it under the lock, keeps the
/// live owners, and removes the file once no entry is left; reads that find nothing
/// expired write nothing, however often they repeat.
#[test]
fn hs2_2wmqer_a_read_prunes_expired_entries_and_removes_the_empty_file() {
    let home = tempfile::tempdir().unwrap();
    let registry = LineObservations::new(home.path());
    // Owner A expires at 60 000, owner B (pushed later) at 90 000.
    assert_eq!(push(&registry, OWNER_A, 1, 0), "stored");
    assert_eq!(push(&registry, OWNER_B, 1, 30_000), "stored");
    let path = registry.directory().join(format!("{CHECKOUT}.json"));
    // Repeated reads with nothing expired leave the file byte for byte.
    let before = std::fs::read(&path).unwrap();
    for _ in 0..3 {
        assert_eq!(registry.live(CHECKOUT, 59_999).len(), 2);
    }
    assert_eq!(std::fs::read(&path).unwrap(), before);
    // A's expiry is pruned by the read itself, not left for the next push.
    assert_eq!(registry.live(CHECKOUT, 60_000).len(), 1);
    assert_eq!(stored_owners(&registry), Some(vec![OWNER_B.to_owned()]));
    let pruned = std::fs::read(&path).unwrap();
    assert_eq!(registry.live(CHECKOUT, 60_001).len(), 1);
    assert_eq!(
        std::fs::read(&path).unwrap(),
        pruned,
        "nothing more to prune"
    );
    // B expires too: the read shows nothing and the file is gone.
    assert!(registry.live(CHECKOUT, 90_000).is_empty());
    assert!(!path.exists());
    // Reading again finds no file and creates none.
    assert!(registry.live(CHECKOUT, 90_001).is_empty());
    assert!(!path.exists());
    // Empty, then refilled: a push after the removal stores again, and the sequence
    // an expired owner used is forgotten with its entry (as a push's prune does).
    assert_eq!(push(&registry, OWNER_A, 1, 100_000), "stored");
    assert_eq!(stored_owners(&registry), Some(vec![OWNER_A.to_owned()]));
    assert_eq!(registry.live(CHECKOUT, 100_000).len(), 1);
}

/// HS2-2WMQER: withdrawing the last entry removes the file; withdrawing one of two keeps
/// the other; a withdrawal for an owner with no entry leaves no file behind.
#[test]
fn hs2_2wmqer_withdrawing_the_last_entry_removes_the_file() {
    let home = tempfile::tempdir().unwrap();
    let registry = LineObservations::new(home.path());
    let path = registry.directory().join(format!("{CHECKOUT}.json"));
    let withdraw = |owner: &str, sequence: u64, at: u64| {
        registry
            .put(CHECKOUT, post(owner, sequence, vec![]), &sources(), at)
            .unwrap()
            .outcome
    };
    // A withdrawal with nothing stored writes no empty file.
    assert_eq!(withdraw(OWNER_A, 1, 0), "withdrawn");
    assert!(!path.exists());
    assert_eq!(push(&registry, OWNER_A, 2, 0), "stored");
    assert_eq!(push(&registry, OWNER_B, 1, 0), "stored");
    assert_eq!(withdraw(OWNER_A, 3, 1), "withdrawn");
    assert_eq!(stored_owners(&registry), Some(vec![OWNER_B.to_owned()]));
    assert_eq!(withdraw(OWNER_B, 2, 2), "withdrawn");
    assert!(!path.exists());
    // Repeated withdrawal of an owner already gone, then a refill.
    assert_eq!(withdraw(OWNER_B, 3, 3), "withdrawn");
    assert!(!path.exists());
    assert_eq!(push(&registry, OWNER_B, 4, 4), "stored");
    assert_eq!(registry.live(CHECKOUT, 4).len(), 1);
}

/// HS2-2WMQER: expiry and pushes interleaved, in both orders. A push that lands
/// between a read's expiry and its prune is kept (the prune reads again under the
/// lock), a push prunes an expired owner just as a read does, and a file left empty
/// by an older version is removed by the next read.
#[test]
fn hs2_2wmqer_interleaved_expiry_and_pushes_keep_every_live_entry() {
    let home = tempfile::tempdir().unwrap();
    let registry = LineObservations::new(home.path());
    let path = registry.directory().join(format!("{CHECKOUT}.json"));
    assert_eq!(push(&registry, OWNER_A, 1, 0), "stored");
    // A expired at 60 000, but the owner refreshes at 70 000 before anyone reads: the
    // read at 70 000 finds it live and prunes nothing.
    assert_eq!(push(&registry, OWNER_A, 2, 70_000), "stored");
    assert_eq!(registry.live(CHECKOUT, 70_000).len(), 1);
    assert_eq!(stored(&registry).unwrap().entries[0].sequence, 2);
    // B pushes after A expired again: the push itself prunes A.
    assert_eq!(push(&registry, OWNER_B, 1, 130_000), "stored");
    assert_eq!(stored_owners(&registry), Some(vec![OWNER_B.to_owned()]));
    // The prune works on the file as it is under the lock: a refresh stored after a
    // read saw the old, expired copy survives the prune that read started.
    let seen = LineObservations::read_file(&registry.storage(CHECKOUT).unwrap())
        .unwrap()
        .unwrap();
    assert!(seen.needs_pruning(190_000));
    assert_eq!(push(&registry, OWNER_B, 2, 190_000), "stored");
    LineObservations::prune_stored(&registry.storage(CHECKOUT).unwrap(), 190_000).unwrap();
    assert_eq!(stored_owners(&registry), Some(vec![OWNER_B.to_owned()]));
    assert_eq!(registry.live(CHECKOUT, 190_000).len(), 1);
    // Interleaved owners expiring one after the other, read between each.
    assert_eq!(push(&registry, OWNER_A, 1, 200_000), "stored");
    assert_eq!(
        registry.live(CHECKOUT, 250_000).len(),
        1,
        "B expired at 250 000"
    );
    assert_eq!(stored_owners(&registry), Some(vec![OWNER_A.to_owned()]));
    assert!(registry.live(CHECKOUT, 260_000).is_empty());
    assert!(!path.exists());
    // An empty file an older version left behind is removed by a read.
    std::fs::create_dir_all(registry.directory()).unwrap();
    std::fs::write(&path, br#"{"schema_version":1,"entries":[]}"#).unwrap();
    assert!(registry.live(CHECKOUT, 0).is_empty());
    assert!(!path.exists());
}
