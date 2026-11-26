//! HS2-CBZV9W: the discovery cache answers exactly what a fresh `git rev-parse` would,
//! across every transition that can change the answer, and spawns no `git` while
//! nothing changed.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

use super::{DiscoveryCache, Watch};
use crate::store_git::{OwnerProbe, probe_owner};
use crate::worklist::probe_enclosing_git_exclude;

fn git(dir: &Path, args: &[&str]) {
    let output = crate::git::command_in(dir)
        .args(["-c", "user.name=Synthetic"])
        .args(["-c", "user.email=synthetic@example.invalid"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn init(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"]);
}

fn commit(dir: &Path, message: &str) {
    git(dir, &["commit", "-q", "--allow-empty", "-m", message]);
}

/// A cache of real owner probes that counts every `git` spawn it makes.
struct Owners {
    cache: DiscoveryCache<OwnerProbe>,
    probes: AtomicUsize,
}

impl Owners {
    fn new() -> Self {
        Self {
            cache: DiscoveryCache::new(),
            probes: AtomicUsize::new(0),
        }
    }

    /// The cached answer, asserted equal to a fresh uncached probe.
    fn check(&self, path: &Path) -> OwnerProbe {
        let cached = self
            .cache
            .get_or_probe(path, Watch::LocationAndHead, |path| {
                self.probes.fetch_add(1, Ordering::SeqCst);
                probe_owner(path)
            });
        assert_eq!(cached, probe_owner(path).0, "{}", path.display());
        cached
    }

    fn probes(&self) -> usize {
        self.probes.load(Ordering::SeqCst)
    }
}

fn head(probe: &OwnerProbe) -> Option<String> {
    match probe {
        OwnerProbe::Located { head, .. } => head.clone(),
        OwnerProbe::Classified(_) => None,
    }
}

#[test]
fn hs2_cbzv9w_repeated_lookups_spawn_git_once() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init(&repo);
    commit(&repo, "one");
    let owners = Owners::new();
    let first = owners.check(&repo);
    for _ in 0..5 {
        assert_eq!(owners.check(&repo), first);
    }
    assert_eq!(owners.probes(), 1);
    // Another spelling of the same directory shares the entry.
    owners.check(&repo.join("."));
    assert_eq!(owners.probes(), 1);
    // A ticket write inside the working tree changes nothing Git discovery reads.
    std::fs::write(repo.join("ticket.md"), "synthetic").unwrap();
    owners.check(&repo);
    assert_eq!(owners.probes(), 1);
}

#[test]
fn hs2_cbzv9w_head_changes_are_seen() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init(&repo);
    let owners = Owners::new();
    // Unborn → born.
    assert_eq!(head(&owners.check(&repo)), None);
    commit(&repo, "one");
    let one = head(&owners.check(&repo)).unwrap();
    // A commit, then a branch switch, a reset and packing the refs.
    commit(&repo, "two");
    let two = head(&owners.check(&repo)).unwrap();
    assert_ne!(one, two);
    git(&repo, &["switch", "-q", "-c", "side", &one]);
    assert_eq!(head(&owners.check(&repo)).unwrap(), one);
    git(&repo, &["reset", "-q", "--hard", &two]);
    assert_eq!(head(&owners.check(&repo)).unwrap(), two);
    git(&repo, &["pack-refs", "--all"]);
    owners.check(&repo);
    git(&repo, &["update-ref", "refs/heads/side", &one]);
    assert_eq!(head(&owners.check(&repo)).unwrap(), one);
    git(&repo, &["checkout", "-q", "--detach", &two]);
    assert_eq!(head(&owners.check(&repo)).unwrap(), two);
    // Nothing changed since the last lookup: no spawn.
    let probes = owners.probes();
    owners.check(&repo);
    assert_eq!(owners.probes(), probes);
}

#[test]
fn hs2_cbzv9w_a_repository_initialized_after_the_first_lookup_is_found() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path().join("outer").join("store");
    std::fs::create_dir_all(&store).unwrap();
    let owners = Owners::new();
    assert_eq!(
        owners.check(&store),
        OwnerProbe::Classified(crate::store_git::StoreGitOwner::NoRepository)
    );
    owners.check(&store);
    assert_eq!(
        owners.probes(),
        1,
        "NoRepository is reused while nothing changes"
    );
    // An enclosing repository appears above the store: now a nested prefix.
    init(&dir.path().join("outer"));
    let OwnerProbe::Located { prefix, .. } = owners.check(&store) else {
        panic!("expected a located store");
    };
    assert_eq!(prefix, "store/");
    // The store becomes its own repository below the enclosing one.
    init(&store);
    let OwnerProbe::Located { prefix, .. } = owners.check(&store) else {
        panic!("expected a located store");
    };
    assert_eq!(prefix, "");
    // Its `.git` removed again: back to the enclosing repository.
    std::fs::remove_dir_all(store.join(".git")).unwrap();
    let OwnerProbe::Located { prefix, .. } = owners.check(&store) else {
        panic!("expected a located store");
    };
    assert_eq!(prefix, "store/");
}

#[test]
fn hs2_cbzv9w_a_path_removed_then_recreated_is_probed_again() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init(&repo);
    commit(&repo, "one");
    let owners = Owners::new();
    let before = owners.check(&repo);
    std::fs::remove_dir_all(&repo).unwrap();
    // A missing path is never cached: Git reports it on every lookup.
    owners.check(&repo);
    owners.check(&repo);
    std::fs::create_dir_all(&repo).unwrap();
    assert_eq!(
        owners.check(&repo),
        OwnerProbe::Classified(crate::store_git::StoreGitOwner::NoRepository)
    );
    init(&repo);
    commit(&repo, "again");
    assert_ne!(owners.check(&repo), before);
    // The HS2-86ES0B relocation: the repository moved to a new path.
    let moved = dir.path().join("moved");
    std::fs::rename(&repo, &moved).unwrap();
    let OwnerProbe::Located { toplevel, .. } = owners.check(&moved) else {
        panic!("expected the moved repository");
    };
    assert_eq!(
        hotsheet_sync::paths::canonicalize(hotsheet_sync::paths::from_git(&toplevel)).unwrap(),
        hotsheet_sync::paths::canonicalize(&moved).unwrap()
    );
    owners.check(&repo);
}

#[test]
fn hs2_cbzv9w_linked_worktrees_are_tracked() {
    let dir = tempfile::tempdir().unwrap();
    let main = dir.path().join("main");
    init(&main);
    commit(&main, "one");
    let owners = Owners::new();
    owners.check(&main);
    let worktree = dir.path().join("wt");
    let worktree_arg = worktree.to_str().unwrap();
    git(&main, &["worktree", "add", "-q", "-b", "wt", worktree_arg]);
    let OwnerProbe::Located { git_dir, .. } = owners.check(&worktree) else {
        panic!("expected a linked worktree");
    };
    assert!(git_dir.contains("worktrees"), "{git_dir}");
    // A commit in the worktree moves its HEAD, not the main checkout's.
    commit(&worktree, "two");
    owners.check(&worktree);
    owners.check(&main);
    // Removed and re-added at the same path on another branch.
    git(&main, &["worktree", "remove", "--force", worktree_arg]);
    owners.check(&main);
    git(
        &main,
        &["worktree", "add", "-q", "-b", "again", worktree_arg],
    );
    owners.check(&worktree);
    // Pruned behind the checkout's back: the gitfile now points nowhere.
    std::fs::remove_dir_all(Path::new(&git_dir)).unwrap();
    owners.check(&worktree);
}

#[cfg(unix)]
#[test]
fn hs2_cbzv9w_a_symlinked_path_follows_its_target() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (dir.path().join("first"), dir.path().join("second"));
    init(&first);
    commit(&first, "one");
    init(&second);
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&first, &link).unwrap();
    let owners = Owners::new();
    assert_eq!(owners.check(&link), owners.check(&first));
    assert_eq!(owners.probes(), 1, "a link and its target share one entry");
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&second, &link).unwrap();
    assert_eq!(owners.check(&link), owners.check(&second));
    assert_ne!(owners.check(&link), owners.check(&first));
}

#[test]
fn hs2_cbzv9w_concurrent_lookups_agree() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    init(&repo);
    commit(&repo, "one");
    let owners = Arc::new(Owners::new());
    let expected = probe_owner(&repo).0;
    let threads = 8;
    let barrier = Arc::new(Barrier::new(threads));
    let handles: Vec<_> = (0..threads)
        .map(|_| {
            let (owners, barrier, repo) = (Arc::clone(&owners), Arc::clone(&barrier), repo.clone());
            std::thread::spawn(move || {
                barrier.wait();
                owners.check(&repo)
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap(), expected);
    }
    assert!((1..=threads).contains(&owners.probes()));
    let probes = owners.probes();
    owners.check(&repo);
    assert_eq!(owners.probes(), probes);
}

#[test]
fn hs2_cbzv9w_an_undescribable_repository_is_never_reused() {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-q", "--bare"]);
    let owners = Owners::new();
    assert!(matches!(
        owners.check(dir.path()),
        OwnerProbe::Classified(crate::store_git::StoreGitOwner::Unavailable { .. })
    ));
    owners.check(dir.path());
    assert_eq!(owners.probes(), 2);
}

#[test]
fn hs2_cbzv9w_enclosing_exclude_follows_the_checkout_location() {
    let dir = tempfile::tempdir().unwrap();
    let checkout = dir.path().join("outer").join("app");
    std::fs::create_dir_all(&checkout).unwrap();
    let cache = DiscoveryCache::new();
    let probes = AtomicUsize::new(0);
    let lookup = || {
        let cached = cache.get_or_probe(&checkout, Watch::Location, |path| {
            probes.fetch_add(1, Ordering::SeqCst);
            probe_enclosing_git_exclude(path)
        });
        assert_eq!(cached, probe_enclosing_git_exclude(&checkout).0);
        cached
    };
    assert_eq!(lookup(), None);
    assert_eq!(lookup(), None);
    assert_eq!(probes.load(Ordering::SeqCst), 1);
    init(&dir.path().join("outer"));
    let (_, pattern) = lookup().unwrap();
    assert_eq!(
        pattern,
        format!("/app/{}", crate::worklist::CHECKOUT_WORKLIST)
    );
    // A commit in the enclosing repository does not move the checkout.
    commit(&dir.path().join("outer"), "one");
    lookup();
    assert_eq!(probes.load(Ordering::SeqCst), 2);
}
