//! HS2-BQN0M5, HS2-G6F5BP: the automatic re-plan memory's transitions and its
//! machine-local store. Synthetic homes and ids only.

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
