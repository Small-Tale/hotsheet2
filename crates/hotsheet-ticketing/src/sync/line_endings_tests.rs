//! HS2-WNPX98: a store shared by an LF site and a CRLF (`core.autocrlf=true`, the Git for
//! Windows default) site syncs byte for byte. The CRLF clone is made by plain Git before
//! Hot Sheet ever pinned it, as a store created before the fix was. Both replay modes run;
//! every round makes the CRLF site replay its own write onto an incoming one, so Git checks
//! files out during the replay. Synthetic bare remotes and synthetic bodies only.

use std::path::{Path, PathBuf};

use super::tests::{head, identity, ok};
use super::*;
use crate::store::{FsStore, StoreMetadata};

fn live() -> SyncLimits {
    SyncLimits {
        replay: ReplayMode::LiveRebase,
        ..SyncLimits::default()
    }
}

fn clone(dir: &Path, remote: &Path, name: &str, autocrlf: &str) -> PathBuf {
    let clone = dir.join(name);
    let config = format!("core.autocrlf={autocrlf}");
    ok(
        dir,
        &[
            "-c",
            &config,
            "clone",
            "--quiet",
            "--config",
            &config,
            remote.to_str().unwrap(),
            clone.to_str().unwrap(),
        ],
    );
    identity(&clone);
    clone
}

fn tracked(root: &Path) -> Vec<String> {
    git_stdout(root, &["ls-files"])
        .unwrap()
        .lines()
        .map(str::to_string)
        .collect()
}

/// Every tracked file is identical on both sites and has no CRLF; both trees are clean.
fn assert_same_bytes(lf: &Path, crlf: &Path) {
    assert_eq!(head(lf), head(crlf));
    let files = tracked(lf);
    assert_eq!(files, tracked(crlf));
    for file in &files {
        let a = std::fs::read(lf.join(file)).unwrap();
        let b = std::fs::read(crlf.join(file)).unwrap();
        assert_eq!(
            String::from_utf8_lossy(&b),
            String::from_utf8_lossy(&a),
            "{file} differs between the sites"
        );
        assert!(!b.windows(2).any(|w| w == b"\r\n"), "{file} has CRLF");
    }
    for root in [lf, crlf] {
        let status = git_stdout(root, &["status", "--porcelain"]).unwrap();
        assert!(status.is_empty(), "{}: {status}", root.display());
    }
}

fn synced(report: &SyncReport) -> bool {
    matches!(report, SyncReport::Synced { .. } | SyncReport::UpToDate)
}

fn run(limits: SyncLimits) {
    let dir = tempfile::tempdir().unwrap();
    let remote = dir.path().join("remote.git");
    ok(
        dir.path(),
        &["init", "--quiet", "--bare", remote.to_str().unwrap()],
    );
    let lf = clone(dir.path(), &remote, "lf", "false");
    let lf_store = FsStore::init(&lf, &StoreMetadata::new("HS"))
        .unwrap()
        .with_deferred_push();
    std::fs::create_dir_all(lf.join("tickets")).unwrap();
    std::fs::write(lf.join("tickets/one.md"), "---\nslug: HS-1\n---\nfirst\n").unwrap();
    assert!(lf_store.autocommit("init store").unwrap());
    let report = sync_once_with(&lf, limits);
    assert!(synced(&report), "{report:?}");

    // A checkout made before the fix: Git rewrote every text file as CRLF.
    let crlf = clone(dir.path(), &remote, "crlf", "true");
    let one = std::fs::read(crlf.join("tickets/one.md")).unwrap();
    assert!(
        one.windows(2).any(|w| w == b"\r\n"),
        "the precondition did not convert"
    );

    // HS2-457FGS: opening keeps the managed `.gitignore` free of duplicates whatever its
    // line endings, and the store's lasting locks stay out of history.
    let crlf_store = FsStore::open(&crlf).unwrap().with_deferred_push();
    crlf_store
        .with_ticket_transaction(|| Ok::<_, crate::store::StoreError>(()))
        .unwrap();
    std::fs::write(crlf.join("providers.json.lock"), "").unwrap();

    // The first sync pins the endings, repairs the copies, and commits nothing.
    let before = head(&crlf);
    let report = sync_once_with(&crlf, limits);
    assert_eq!(report, SyncReport::UpToDate);
    assert_eq!(head(&crlf), before);
    assert_same_bytes(&lf, &crlf);
    let ignore = std::fs::read_to_string(crlf.join(".gitignore")).unwrap();
    for managed in crate::store::MANAGED_IGNORES {
        assert_eq!(
            ignore.lines().filter(|l| l.trim() == managed).count(),
            1,
            "{ignore}"
        );
    }

    for round in 0..3 {
        // The LF site publishes first, so the CRLF site's own write is replayed on it.
        std::fs::write(
            lf.join("tickets/one.md"),
            format!("---\nslug: HS-1\n---\nfirst\nlf round {round}\n"),
        )
        .unwrap();
        std::fs::write(
            lf.join(format!("tickets/lf-{round}.md")),
            "from lf\nsecond line\n",
        )
        .unwrap();
        assert!(synced(&sync_once_with(&lf, limits)));
        std::fs::write(
            crlf.join(format!("tickets/crlf-{round}.md")),
            "from crlf\nsecond line\n",
        )
        .unwrap();
        let report = sync_once_with(&crlf, limits);
        assert_eq!(
            report,
            SyncReport::Synced {
                pulled: true,
                pushed: true
            }
        );
        assert!(synced(&sync_once_with(&lf, limits)));
        assert_same_bytes(&lf, &crlf);
    }

    // Repeated sync on both sites changes nothing.
    for _ in 0..2 {
        assert_eq!(sync_once_with(&crlf, limits), SyncReport::UpToDate);
        assert_eq!(sync_once_with(&lf, limits), SyncReport::UpToDate);
    }
    assert_same_bytes(&lf, &crlf);
    for lock in [".hotsheet-note-mutation.lock", "providers.json.lock"] {
        assert!(crlf.join(lock).exists());
        assert!(
            !tracked(&crlf).iter().any(|file| file == lock),
            "{lock} tracked"
        );
    }
}

#[test]
fn hs2_wnpx98_an_lf_and_a_crlf_clone_sync_byte_for_byte() {
    run(SyncLimits::default());
}

#[test]
fn hs2_wnpx98_an_lf_and_a_crlf_clone_sync_byte_for_byte_with_a_live_rebase() {
    run(live());
}
