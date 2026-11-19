//! HS2-WJF0D8: the Git-ref compare-and-swap against a real bare remote and two clones.

use std::path::PathBuf;

use super::*;

fn repos() -> (tempfile::TempDir, PathBuf, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("remote.git");
    let run = |args: &[&str]| {
        assert!(
            crate::git::command().args(args).status().unwrap().success(),
            "{args:?}"
        );
    };
    run(&[
        "init",
        "--quiet",
        "--bare",
        "-b",
        "main",
        bare.to_str().unwrap(),
    ]);
    let mut clones = Vec::new();
    for name in ["a", "b"] {
        let path = dir.path().join(name);
        run(&["init", "--quiet", path.to_str().unwrap()]);
        run(&[
            "-C",
            path.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            bare.to_str().unwrap(),
        ]);
        run(&[
            "-C",
            path.to_str().unwrap(),
            "config",
            "maintenance.auto",
            "false",
        ]);
        clones.push(path);
    }
    run(&[
        "-C",
        bare.to_str().unwrap(),
        "config",
        "maintenance.auto",
        "false",
    ]);
    let b = clones.pop().unwrap();
    let a = clones.pop().unwrap();
    (dir, a, b)
}

const REF: &str = "refs/hotsheet/resource-claims/vm/runner";

fn won(result: Acquired) -> RefMarker {
    match result {
        Acquired::Won(marker) => marker,
        Acquired::Held(holder) => panic!("held by {holder:?}"),
    }
}

#[test]
fn hs2_wjf0d8_first_acquire_wins_and_a_live_claim_is_held() {
    let (_d, a, b) = repos();
    let (a, b) = (GitRefCas::new(a, "origin"), GitRefCas::new(b, "origin"));
    let first = won(a
        .acquire(REF, RefMarker::new("w-a", "c1", 1_000, 60_000), 1_000)
        .unwrap());
    assert_eq!(first.fencing, 1_000);
    match b
        .acquire(REF, RefMarker::new("w-b", "c2", 2_000, 60_000), 2_000)
        .unwrap()
    {
        Acquired::Held(holder) => assert_eq!(holder.claim_id, "c1"),
        Acquired::Won(_) => panic!("a live claim must refuse another"),
    }
    // The same claim again is an idempotent retry that keeps its fencing token.
    let retry = won(a
        .acquire(REF, RefMarker::new("w-a", "c1", 3_000, 90_000), 3_000)
        .unwrap());
    assert_eq!((retry.fencing, retry.expiry_ms()), (1_000, 90_000));
}

#[test]
fn hs2_wjf0d8_a_lapsed_claim_is_taken_with_a_larger_fencing_token() {
    let (_d, a, b) = repos();
    let (a, b) = (GitRefCas::new(a, "origin"), GitRefCas::new(b, "origin"));
    won(a
        .acquire(REF, RefMarker::new("w-a", "c1", 1_000, 5_000), 1_000)
        .unwrap());
    let taken = won(b
        .acquire(REF, RefMarker::new("w-b", "c2", 4_000, 9_000), 6_000)
        .unwrap());
    assert!(taken.fencing > 1_000);
    // The lapsed holder can neither renew nor release the new claim.
    assert!(matches!(a.renew(REF, "c1", 20_000).unwrap(), Err(Some(m)) if m.claim_id == "c2"));
    assert!(!a.release(REF, "c1").unwrap());
    assert!(b.release(REF, "c2").unwrap());
    assert!(a.read(REF).unwrap().is_none());
    // After a deletion the next grant still gets a larger token from its clock.
    let again = won(a
        .acquire(REF, RefMarker::new("w-a", "c3", 7_000, 9_000), 7_000)
        .unwrap());
    assert!(again.fencing > taken.fencing);
}

#[test]
fn hs2_wjf0d8_renew_moves_the_expiry_and_list_reads_every_marker() {
    let (_d, a, _b) = repos();
    let a = GitRefCas::new(a, "origin");
    won(a
        .acquire(REF, RefMarker::new("w-a", "c1", 1_000, 5_000), 1_000)
        .unwrap());
    let other = "refs/hotsheet/resource-claims/vm/other";
    won(a
        .acquire(other, RefMarker::new("w-a", "c9", 1_000, 5_000), 1_000)
        .unwrap());
    let renewed = a.renew(REF, "c1", 50_000).unwrap().unwrap();
    assert_eq!(renewed.expiry_ms(), 50_000);
    let listed = a.list("refs/hotsheet/resource-claims/vm/").unwrap();
    assert_eq!(listed.len(), 2);
    assert!(matches!(
        a.renew("refs/hotsheet/resource-claims/vm/none", "c1", 1)
            .unwrap(),
        Err(None)
    ));
}

#[test]
fn hs2_wjf0d8_a_distclaim_marker_and_an_exact_claim_exclude_each_other() {
    let (_d, a, b) = repos();
    let id = hotsheet_model::Ulid::new();
    let far = Timestamp::new("2999-01-01T00:00:00Z");
    crate::distclaim::claim(
        &a,
        &id,
        "loop-worker",
        far,
        &Timestamp::new("2026-01-01T00:00:00Z"),
    )
    .unwrap();
    let cas = GitRefCas::new(&b, "origin");
    match cas
        .acquire(
            &GitRefCas::ticket_ref(&id),
            RefMarker::new("w-b", "c1", 1_000, 60_000),
            1_000,
        )
        .unwrap()
    {
        Acquired::Held(holder) => assert_eq!(holder.worker, "loop-worker"),
        Acquired::Won(_) => panic!("the work loop's claim must hold"),
    }
}

#[test]
fn hs2_wjf0d8_an_unreachable_remote_is_reported_as_such() {
    let (dir, a, _b) = repos();
    let cas = GitRefCas::new(&a, dir.path().join("gone.git").to_str().unwrap());
    assert!(matches!(cas.read(REF), Err(CasError::Unreachable(_))));
    assert!(matches!(
        cas.acquire(REF, RefMarker::new("w", "c", 1, 2), 1),
        Err(CasError::Unreachable(_))
    ));
}
