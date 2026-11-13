//! HS2-9TV6A2: a cycle registers the semantic merge driver before it integrates, so a
//! store whose upstream has no `.gitattributes` and whose checkout never ran `init` still
//! merges ticket files through the driver. Synthetic stores and bodies only.

use std::path::{Path, PathBuf};

use super::tests::{clean, ok, remote_with_two_clones};
use super::*;
use crate::merge_driver::{MergeDriverStatus, TEST_PROGRAM};
use crate::sync_health::{health, read_record};

/// A stand-in for `hotsheet-cli merge-driver %O %A %B`: a line-union merge that also
/// records each call, so a test sees that Git consulted the driver.
#[cfg(unix)]
fn recording_driver(dir: &Path) -> (PathBuf, PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let calls = dir.join("driver-calls");
    let program = dir.join("fake-hotsheet-cli");
    std::fs::write(
        &program,
        format!(
            "#!/bin/sh\n[ \"$1\" = merge-driver ] || exit 2\necho call >> '{}'\nexec git \
             merge-file --union \"$3\" \"$2\" \"$4\"\n",
            calls.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o755)).unwrap();
    (program, calls)
}

/// A ticket file both sides changed on adjacent frontmatter lines: Git's text merge
/// conflicts on it; the driver merges it.
fn ticket(tags: &str, priority: &str) -> String {
    format!("---\nid: 01TEST\ntitle: shared\ntags: [{tags}]\npriority: {priority}\n---\nbody\n")
}

fn write_ticket(root: &Path, body: &str, message: &str) {
    let path = root.join("tickets/st/01TEST.md");
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, body).unwrap();
    ok(root, &["add", "-A"]);
    ok(root, &["commit", "--quiet", "-m", message]);
}

#[cfg(unix)]
#[test]
fn hs2_9tv6a2_a_cycle_registers_the_driver_before_it_replays() {
    for mode in [ReplayMode::Auto, ReplayMode::LiveRebase] {
        let (dir, store, peer) = remote_with_two_clones();
        let (program, calls) = recording_driver(dir.path());
        // The shared history has the ticket but no `.gitattributes`, and this checkout
        // has no driver configuration: the incident's starting point.
        write_ticket(&peer, &ticket("a", "default"), "ticket");
        ok(&peer, &["push", "--quiet"]);
        ok(&store, &["pull", "--quiet"]);
        assert!(!store.join(".gitattributes").exists());
        assert_eq!(
            crate::merge_driver::status(&store),
            MergeDriverStatus::Missing
        );
        write_ticket(&store, &ticket("a, local-tag", "default"), "local tag");
        write_ticket(&peer, &ticket("a", "high"), "peer priority");
        ok(&peer, &["push", "--quiet"]);

        TEST_PROGRAM.with(|slot| *slot.borrow_mut() = Some(program.clone()));
        let limits = SyncLimits {
            replay: mode,
            ..SyncLimits::default()
        };
        let report = sync_once_with(&store, limits);
        TEST_PROGRAM.with(|slot| slot.borrow_mut().take());
        assert_eq!(
            report,
            SyncReport::Synced {
                pulled: true,
                pushed: true
            },
            "{mode:?}"
        );
        assert!(
            std::fs::read_to_string(&calls).unwrap().lines().count() >= 1,
            "{mode:?}: Git consulted the driver"
        );
        let merged = std::fs::read_to_string(store.join("tickets/st/01TEST.md")).unwrap();
        assert!(
            merged.contains("local-tag") && merged.contains("high"),
            "{merged}"
        );
        assert!(!merged.contains("<<<<<<<"), "{merged}");
        assert!(clean(&store));
        // Registered locally only: nothing was committed for it.
        assert!(!store.join(".gitattributes").exists());
        let health = health(&store);
        assert_eq!(health.merge_driver, MergeDriverStatus::Registered);
        assert!(health.merge_driver_repaired_at.is_some());
        assert!(
            read_record(&store)
                .unwrap()
                .merge_driver_repaired_at
                .is_some()
        );
    }
}

/// Without any driver program the same edit is a text-merge conflict, and health says
/// why and how to fix it.
#[test]
fn hs2_9tv6a2_without_a_driver_the_conflict_names_the_missing_registration() {
    let (_dir, store, peer) = remote_with_two_clones();
    write_ticket(&peer, &ticket("a", "default"), "ticket");
    ok(&peer, &["push", "--quiet"]);
    ok(&store, &["pull", "--quiet"]);
    write_ticket(&store, &ticket("a, local-tag", "default"), "local tag");
    write_ticket(&peer, &ticket("a", "high"), "peer priority");
    ok(&peer, &["push", "--quiet"]);
    assert_eq!(sync_once(&store), SyncReport::Conflict);
    let health = health(&store);
    assert_eq!(health.merge_driver, MergeDriverStatus::Missing);
    assert!(
        health
            .next_action
            .unwrap()
            .contains("merge driver is not registered"),
    );
    assert!(clean(&store));
}
