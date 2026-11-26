//! HS2-BQN0M5, HS2-G6F5BP: the automatic re-plan memory's transitions and its
//! machine-local store. Synthetic homes and ids only.

use super::replan_memory::{CatchUp, MAX_REPLAN_TARGETS};
use super::*;

const OWNER: &str = "owner-0123456789ab";

fn key(native: &str) -> ReplanKey {
    ("conn".into(), native.into())
}

/// HS2-BQN0M5: the memory's transitions: new, repeated, closed then reopened,
/// covered by a newer update, named by a step, and asked.
#[test]
fn hs2_bqn0m5_replan_memory_transitions() {
    let mut memory = ReplanMemory::default();
    assert!(memory.observe(key("a"), true, None));
    assert!(!memory.observe(key("a"), true, None), "handed once");
    assert!(memory.observe(key("b"), true, None));
    assert_eq!(memory.pending(), 2);
    // A step names `a` now; `b` still waits on the same latest update.
    memory.settle(|candidate| candidate == &key("a"), None);
    assert_eq!(memory.pending(), 1);
    // A newer update started after `b` was seen: it is covered.
    memory.settle(|_| false, Some("u1"));
    assert_eq!(memory.pending(), 0);
    // `b` closes and reopens: it counts again, recording the current update.
    assert!(!memory.observe(key("b"), false, Some("u1")));
    assert!(memory.observe(key("b"), true, Some("u1")));
    memory.settle(|_| false, Some("u1"));
    assert_eq!(memory.pending(), 1, "the same update did not cover it");
    memory.asked();
    assert_eq!(memory.pending(), 0);
    assert!(!memory.observe(key("b"), true, Some("u2")), "still handed");
    // A closed ticket never pending.
    assert!(!memory.observe(key("c"), false, None));
    assert_eq!(memory.pending(), 0);
}

#[test]
fn hs2_bqn0m5_handed_tickets_are_bounded() {
    let mut memory = ReplanMemory::default();
    for n in 0..=MAX_REPLAN_HANDED {
        memory.observe(key(&format!("{n:05}")), true, None);
    }
    assert_eq!(memory.handed(), MAX_REPLAN_HANDED);
    // The oldest key went first: it counts as new again.
    memory.asked();
    assert!(memory.observe(key("00000"), true, None));
}

/// HS2-G6F5BP: the store keeps handed and pending tickets with the update each pending
/// one was seen after; an absent file is empty and a read never creates it; an empty
/// memory removes the file; every owner has its own file.
#[test]
fn hs2_g6f5bp_the_memory_round_trips_per_owner() {
    let home = tempfile::tempdir().unwrap();
    let store = ReplanMemoryStore::new(home.path(), OWNER).unwrap();
    let path = home
        .path()
        .join(REPLAN_MEMORY_DIR)
        .join(format!("{OWNER}.json"));
    assert_eq!(store.read().unwrap(), ReplanMemory::default());
    assert!(!path.exists(), "a read creates nothing");

    let mut memory = ReplanMemory::default();
    memory.observe(key("handed"), true, None);
    memory.asked();
    memory.observe(key("pending-new"), true, None);
    memory.observe(key("pending-after"), true, Some("update-1"));
    let _lock = store.lock().unwrap();
    store.write(&memory).unwrap();
    assert!(path.is_file());
    let read = store.read().unwrap();
    assert_eq!(read, memory);
    assert_eq!((read.handed(), read.pending()), (3, 2));
    // The update a pending ticket was seen after survives: a newer one covers it.
    let mut settled = read.clone();
    settled.settle(|_| false, Some("update-1"));
    assert_eq!(settled.pending(), 1);

    // Another owner has its own memory.
    let other = ReplanMemoryStore::new(home.path(), "owner-other").unwrap();
    assert_eq!(other.read().unwrap(), ReplanMemory::default());

    // Forgetting everything removes the file; removing it again is fine.
    store.write(&ReplanMemory::default()).unwrap();
    assert!(!path.exists());
    store.write(&ReplanMemory::default()).unwrap();
    assert_eq!(store.read().unwrap(), ReplanMemory::default());
}

/// HS2-G6F5BP: a file that does not decode, names an unknown field, holds an empty or
/// oversized id, or is from another schema version is refused by the read, so the
/// caller can fall back; an owner id outside the identifier rule is refused at once.
#[test]
fn hs2_g6f5bp_an_unreadable_memory_is_refused() {
    let home = tempfile::tempdir().unwrap();
    let store = ReplanMemoryStore::new(home.path(), OWNER).unwrap();
    let path = home
        .path()
        .join(REPLAN_MEMORY_DIR)
        .join(format!("{OWNER}.json"));
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let long = "x".repeat(513);
    for (text, version) in [
        ("not json".to_owned(), false),
        (r#"{"schema_version":1}"#.to_owned(), false),
        (
            r#"{"schema_version":1,"handed":[],"pending":[],"extra":1}"#.to_owned(),
            false,
        ),
        (
            r#"{"schema_version":1,"handed":[{"connection_id":"","native_id":"a"}],"pending":[]}"#
                .to_owned(),
            false,
        ),
        (
            format!(
                r#"{{"schema_version":1,"handed":[{{"connection_id":"c","native_id":"{long}"}}],"pending":[]}}"#
            ),
            false,
        ),
        (
            r#"{"schema_version":1,"handed":[],"pending":[{"connection_id":"c","native_id":"a","after":"not an id!"}]}"#
                .to_owned(),
            false,
        ),
        (
            r#"{"schema_version":2,"handed":[],"pending":[]}"#.to_owned(),
            true,
        ),
    ] {
        std::fs::write(&path, &text).unwrap();
        match store.read() {
            Err(OrchestrationError::UnsupportedVersion { found: 2 }) => assert!(version),
            Err(OrchestrationError::Corrupt) => assert!(!version, "{text}"),
            other => panic!("{text}: {other:?}"),
        }
    }
    assert!(ReplanMemoryStore::new(home.path(), "../escape").is_err());
    // An implausible memory is never written.
    let mut memory = ReplanMemory::default();
    memory.observe(("c".into(), long), true, None);
    assert!(store.write(&memory).is_err());
}

fn listing<'a>(target_id: &'a str, connections: &'a [String], complete: bool) -> CatchUp<'a> {
    CatchUp {
        target_id,
        connections,
        complete,
    }
}

/// HS2-04WW8B: the seen baseline's transitions. A target's first catch-up only seeds;
/// a later one hands what nobody saw, nor a step names, to one pending update; a
/// repeated catch-up hands nothing new; a ticket closed meanwhile is forgotten by a
/// complete listing (and counts again when it reopens) but kept by an incomplete one;
/// an event's ticket and a named ticket are seen; another target seeds separately.
#[test]
fn hs2_04ww8b_catch_up_transitions() {
    let conn = vec!["conn".to_owned()];
    let none = |_: &ReplanKey| false;
    let mut memory = ReplanMemory::default();
    assert!(!memory.seeded("t1"));
    // Seed: everything open is known, nothing pending.
    let open = [key("a"), key("b")];
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &open, none, None),
        0
    );
    assert!(memory.seeded("t1"));
    assert_eq!(
        (memory.pending(), memory.handed(), memory.seen()),
        (0, 0, 2)
    );
    // Repeated: nothing new.
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &open, none, None),
        0
    );
    // Down-time: `c` and `d` appeared; `d` a step names already.
    let later = [key("a"), key("b"), key("c"), key("d")];
    let named = |candidate: &ReplanKey| candidate == &key("d");
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &later, named, Some("u1")),
        1
    );
    assert_eq!(
        (memory.pending(), memory.handed(), memory.seen()),
        (1, 1, 4)
    );
    // The pending one records the latest update: a newer one covers it.
    let mut covered = memory.clone();
    covered.settle(none, Some("u2"));
    assert_eq!(covered.pending(), 0);
    // Repeated catch-ups hand nothing more.
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &later, none, Some("u1")),
        0
    );
    assert_eq!(memory.pending(), 1);
    // `c` closes meanwhile: an incomplete listing that misses it keeps it...
    let without_c = [key("a"), key("b"), key("d")];
    memory.catch_up(listing("t1", &conn, false), &without_c, none, Some("u1"));
    assert_eq!(
        (memory.pending(), memory.handed(), memory.seen()),
        (1, 1, 4)
    );
    // ...a complete one forgets it, pending and all.
    memory.catch_up(listing("t1", &conn, true), &without_c, none, Some("u1"));
    assert_eq!(
        (memory.pending(), memory.handed(), memory.seen()),
        (0, 0, 3)
    );
    // Reopened: new again.
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &later, none, None),
        1
    );
    memory.asked();
    // A ticket an event showed is seen: no catch-up hands it.
    memory.saw(key("e"));
    let with_e = [key("a"), key("b"), key("c"), key("d"), key("e")];
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &with_e, none, None),
        0
    );
    // An event of a closed ticket forgets it from the baseline.
    memory.observe(key("e"), false, None);
    assert_eq!(memory.seen(), 4);
    // Another target over another source seeds on its own, and its complete listing
    // forgets nothing of the first target's source.
    let other = vec!["other".to_owned()];
    let theirs = [("other".to_owned(), "x".to_owned())];
    assert_eq!(
        memory.catch_up(listing("t2", &other, true), &theirs, none, None),
        0
    );
    assert!(memory.seeded("t2"));
    assert_eq!(memory.seen(), 5);
    // An empty, complete listing: every remembered ticket of that source closed.
    memory.catch_up(listing("t1", &conn, true), &[], none, None);
    assert_eq!((memory.handed(), memory.seen()), (0, 1));
    // Empty, then refilled: what appears afterwards is new.
    assert_eq!(
        memory.catch_up(listing("t1", &conn, true), &[key("f")], none, None),
        1
    );
}

/// HS2-04WW8B: the baseline and the seeded targets are bounded; the smallest key goes
/// first.
#[test]
fn hs2_04ww8b_the_baseline_is_bounded() {
    let mut memory = ReplanMemory::default();
    for n in 0..=MAX_REPLAN_SEEN {
        memory.saw(key(&format!("{n:05}")));
    }
    assert_eq!(memory.seen(), MAX_REPLAN_SEEN);
    let conn = vec!["conn".to_owned()];
    for n in 0..=MAX_REPLAN_TARGETS {
        memory.catch_up(
            listing(&format!("target-{n:04}"), &conn, false),
            &[],
            |_| false,
            None,
        );
    }
    assert!(
        !memory.seeded("target-0000"),
        "the smallest target went first"
    );
    assert!(memory.seeded(&format!("target-{MAX_REPLAN_TARGETS:04}")));
    // The forgotten key counts as new at the next catch-up of a seeded target.
    let target = format!("target-{MAX_REPLAN_TARGETS:04}");
    assert_eq!(
        memory.catch_up(
            listing(&target, &conn, false),
            &[key("00000")],
            |_| false,
            None
        ),
        1
    );
}

/// HS2-04WW8B: the baseline survives the store, and a file written before it existed
/// still reads (as unseeded, so the next catch-up seeds).
#[test]
fn hs2_04ww8b_the_baseline_round_trips() {
    let home = tempfile::tempdir().unwrap();
    let store = ReplanMemoryStore::new(home.path(), OWNER).unwrap();
    let conn = vec!["conn".to_owned()];
    let mut memory = ReplanMemory::default();
    memory.catch_up(
        listing("target-1", &conn, true),
        &[key("a")],
        |_| false,
        None,
    );
    assert!(!memory.is_empty(), "a seeded baseline is remembered");
    let _lock = store.lock().unwrap();
    store.write(&memory).unwrap();
    let read = store.read().unwrap();
    assert_eq!(read, memory);
    assert!(read.seeded("target-1"));
    assert_eq!(read.seen(), 1);
    let path = home
        .path()
        .join(REPLAN_MEMORY_DIR)
        .join(format!("{OWNER}.json"));
    std::fs::write(
        &path,
        r#"{"schema_version":1,"handed":[{"connection_id":"c","native_id":"a"}],"pending":[]}"#,
    )
    .unwrap();
    let older = store.read().unwrap();
    assert_eq!((older.handed(), older.seen()), (1, 0));
    assert!(!older.seeded("target-1"));
    // An implausible seeded target is refused.
    std::fs::write(
        &path,
        r#"{"schema_version":1,"handed":[],"pending":[],"seeded":["../x"]}"#,
    )
    .unwrap();
    assert!(matches!(store.read(), Err(OrchestrationError::Corrupt)));
}
