//! Index Git baselines come only from the repository that owns the store (HS2-ABV2J3).
//!
//! Each test builds a synthetic store with real Git: beneath an unrelated parent
//! checkout, as a tracked designated root, as its own (unborn, then committed)
//! repository, and as a linked worktree, then walks create, update, restart and
//! owner-transition sequences through `Index::reconcile`.

use std::path::Path;

use hotsheet_model::{Ticket, Timestamp, Ulid};
use hotsheet_ticketing::store_git::StoreGitOwner;
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, TicketQuery, ops};

use super::*;

const A: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA0";
const B: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA1";
const C: &str = "01ARZ3NDEKTSV4RRFFQ69G5FA2";

fn ulid(s: &str) -> Ulid {
    Ulid::from_string(s).unwrap()
}

fn git(dir: &Path, args: &[&str]) -> String {
    let out = hotsheet_ticketing::git::command_in(dir)
        .args([
            "-c",
            "user.name=Hot Sheet Test",
            "-c",
            "user.email=test@localhost",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A parent checkout with one commit that ignores `target/`.
fn parent_repo(dir: &Path) {
    git(dir, &["init", "-q", "-b", "main"]);
    std::fs::write(dir.join(".gitignore"), "target/\n").unwrap();
    std::fs::write(dir.join("README.md"), "parent\n").unwrap();
    git(dir, &["add", "--", ".gitignore", "README.md"]);
    git(dir, &["commit", "-q", "-m", "parent"]);
}

/// A synthetic ticket built in a throwaway Git-less store, so writing it into the
/// store under test never triggers an autocommit there.
fn ticket(id: &str, title: &str) -> Ticket {
    let scratch = tempfile::tempdir().unwrap();
    let store = FsStore::init(scratch.path(), &StoreMetadata::new("HS")).unwrap();
    ops::create(
        &store,
        ulid(id),
        "HS",
        Timestamp::new("2026-11-03T00:00:00Z"),
        NewTicket {
            title: title.into(),
            category: "task".into(),
            ..Default::default()
        },
    )
    .unwrap();
    store.read_ticket(&ulid(id)).unwrap()
}

fn retitle(store: &FsStore, id: &str, title: &str) {
    let mut t = store.read_ticket(&ulid(id)).unwrap();
    t.title = title.into();
    store.write_ticket(&t).unwrap();
}

fn titles(ix: &Index) -> Vec<String> {
    let mut titles: Vec<String> = ix
        .query(&TicketQuery::default())
        .unwrap()
        .into_iter()
        .map(|t| t.title)
        .collect();
    titles.sort();
    titles
}

fn index(store: &FsStore) -> Index {
    Index::open_in_memory(store.root().display().to_string()).unwrap()
}

#[test]
fn hs2_abv2j3_store_under_ignored_parent_path_never_borrows_parent_head() {
    let parent = tempfile::tempdir().unwrap();
    parent_repo(parent.path());
    let root = parent.path().join("target/store");
    let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();

    assert!(matches!(store.git_owner(), StoreGitOwner::Unowned { .. }));
    assert_eq!(
        store.head_commit(),
        None,
        "the parent's HEAD is not the store's"
    );
    assert!(store.git_owner().diagnostic(&root).is_some());

    // Empty → create → update are each visible, and parent commits change nothing.
    let ix = index(&store);
    assert_eq!(ix.reconcile(&store).unwrap(), (0, 0));
    store.write_ticket(&ticket(A, "created")).unwrap();
    assert_eq!(titles(&ix), Vec::<String>::new());
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["created"]);
    retitle(&store, A, "started");
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["started"]);

    std::fs::write(parent.path().join("README.md"), "moved\n").unwrap();
    git(parent.path(), &["commit", "-q", "-am", "unrelated"]);
    store
        .write_ticket(&ticket(B, "after parent commit"))
        .unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["after parent commit", "started"]);
    assert_eq!(ix.get_meta("head").unwrap(), None);
    assert_eq!(ix.get_meta(GIT_OWNER_META).unwrap(), None);

    let restarted = index(&store);
    restarted.reconcile(&store).unwrap();
    assert_eq!(titles(&restarted), ["after parent commit", "started"]);
}

#[test]
fn hs2_abv2j3_untracked_store_inside_parent_is_unowned() {
    let parent = tempfile::tempdir().unwrap();
    parent_repo(parent.path());
    let root = parent.path().join("tickets-here");
    let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
    let owner = store.git_owner();
    assert!(matches!(owner, StoreGitOwner::Unowned { .. }), "{owner:?}");
    let diagnostic = owner.diagnostic(&root).unwrap();
    assert!(diagnostic.contains("does not track it"), "{diagnostic}");
    assert!(diagnostic.contains("git -C"), "{diagnostic}");
}

#[test]
fn hs2_abv2j3_designated_root_uses_only_its_own_subtree() {
    let parent = tempfile::tempdir().unwrap();
    parent_repo(parent.path());
    let root = parent.path().join("ops/tickets-root");
    let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
    git(&root, &["add", "--", "hotsheet-store.json"]);
    git(parent.path(), &["commit", "-q", "-m", "designate store"]);

    let StoreGitOwner::Designated { head, prefix, .. } = store.git_owner() else {
        panic!(
            "tracked metadata makes a designated root: {:?}",
            store.git_owner()
        );
    };
    assert_eq!(prefix, "ops/tickets-root/");
    assert_eq!(
        head.as_deref(),
        Some(git(parent.path(), &["rev-parse", "HEAD"]).as_str())
    );
    assert_eq!(store.git_owner().diagnostic(&root), None);

    let ix = index(&store);
    ix.reconcile(&store).unwrap();
    assert!(ix.get_meta(GIT_OWNER_META).unwrap().is_some());

    // Untracked write → visible through the dirty hint.
    store.write_ticket(&ticket(A, "untracked")).unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["untracked"]);

    // A sibling ticket-shaped file elsewhere in the parent never feeds this store.
    let sibling = parent.path().join("other/tickets/01");
    std::fs::create_dir_all(&sibling).unwrap();
    std::fs::write(sibling.join(format!("{B}.md")), "not this store\n").unwrap();
    git(parent.path(), &["add", "--", "other"]);
    git(parent.path(), &["commit", "-q", "-m", "sibling"]);
    assert_eq!(ix.reconcile(&store).unwrap(), (0, 0));

    // Committing the store's ticket moves HEAD; then an edit and an unrelated parent
    // commit reconcile only what changed.
    git(&root, &["add", "--", "tickets"]);
    git(parent.path(), &["commit", "-q", "-m", "commit ticket"]);
    assert_eq!(ix.reconcile(&store).unwrap(), (0, 0));
    retitle(&store, A, "edited");
    std::fs::write(parent.path().join("README.md"), "moved\n").unwrap();
    git(parent.path(), &["commit", "-q", "-am", "unrelated"]);
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));
    assert_eq!(titles(&ix), ["edited"]);
}

#[test]
fn hs2_abv2j3_owned_unborn_repo_then_committed() {
    let parent = tempfile::tempdir().unwrap();
    parent_repo(parent.path());
    let root = parent.path().join("target/own");
    let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
    git(&root, &["init", "-q", "-b", "main"]);

    assert!(matches!(
        store.git_owner(),
        StoreGitOwner::Owned { head: None, .. }
    ));
    assert_eq!(store.head_commit(), None, "unborn, not the parent's HEAD");

    let ix = index(&store);
    store.write_ticket(&ticket(A, "first")).unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["first"]);
    assert_eq!(ix.get_meta("head").unwrap(), None);

    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "first commit"]);
    let head = git(&root, &["rev-parse", "HEAD"]);
    assert_eq!(store.head_commit().as_deref(), Some(head.as_str()));
    ix.reconcile(&store).unwrap();
    assert_eq!(ix.get_meta("head").unwrap().as_deref(), Some(head.as_str()));

    retitle(&store, A, "updated");
    store.write_ticket(&ticket(B, "second")).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (2, 0));
    assert_eq!(titles(&ix), ["second", "updated"]);
    assert_eq!(ix.reconcile(&store).unwrap(), (0, 0), "warm no-op");
}

#[test]
fn hs2_abv2j3_linked_worktree_owns_its_store() {
    let main = tempfile::tempdir().unwrap();
    let store = FsStore::init(main.path(), &StoreMetadata::new("HS")).unwrap();
    git(main.path(), &["init", "-q", "-b", "main"]);
    store.write_ticket(&ticket(A, "shared")).unwrap();
    git(main.path(), &["add", "-A"]);
    git(main.path(), &["commit", "-q", "-m", "base"]);

    let holder = tempfile::tempdir().unwrap();
    let linked = holder.path().join("linked");
    git(
        main.path(),
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "side",
            linked.to_str().unwrap(),
        ],
    );
    assert!(linked.join(".git").is_file());
    let linked_store = FsStore::open(&linked).unwrap();
    let (StoreGitOwner::Owned { key: main_key, .. }, StoreGitOwner::Owned { key, head }) =
        (store.git_owner(), linked_store.git_owner())
    else {
        panic!("both checkouts own their store");
    };
    assert_ne!(key, main_key, "each worktree is its own owner");
    assert!(head.is_some());

    let ix = index(&linked_store);
    ix.reconcile(&linked_store).unwrap();
    retitle(&linked_store, A, "linked edit");
    assert_eq!(ix.reconcile(&linked_store).unwrap(), (1, 0));
    assert_eq!(titles(&ix), ["linked edit"]);
}

#[test]
fn hs2_abv2j3_owner_change_and_legacy_cache_force_a_full_walk() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = FsStore::init(root, &StoreMetadata::new("HS")).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    store.write_ticket(&ticket(A, "base")).unwrap();
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);

    let ix = index(&store);
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["base"]);

    // Commit an edit, then fake a legacy cache: HEAD already current but no owner key.
    // A trusted delta would be empty and leave "base"; the full walk sees the edit.
    retitle(&store, A, "committed edit");
    git(root, &["commit", "-q", "-am", "edit"]);
    let head = git(root, &["rev-parse", "HEAD"]);
    ix.set_meta("head", &head).unwrap();
    ix.conn
        .execute(
            "DELETE FROM index_meta WHERE key=?1",
            params![GIT_OWNER_META],
        )
        .unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));
    assert_eq!(titles(&ix), ["committed edit"]);

    // A different owner key (as after the repository at this path is replaced) is
    // treated the same way.
    retitle(&store, A, "second edit");
    git(root, &["commit", "-q", "-am", "edit 2"]);
    ix.set_meta("head", &git(root, &["rev-parse", "HEAD"]))
        .unwrap();
    ix.set_meta(GIT_OWNER_META, "some other repository")
        .unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));
    assert_eq!(titles(&ix), ["second edit"]);

    // A stored HEAD the repository does not have falls back to the full walk.
    retitle(&store, A, "third edit");
    git(root, &["commit", "-q", "-am", "edit 3"]);
    ix.set_meta("head", "0000000000000000000000000000000000000000")
        .unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (1, 0));
    assert_eq!(titles(&ix), ["third edit"]);
    assert_eq!(
        ix.reconcile(&store).unwrap(),
        (0, 0),
        "baseline re-recorded"
    );
}

#[test]
fn hs2_abv2j3_unowned_store_that_gains_its_own_repo_records_a_baseline() {
    let parent = tempfile::tempdir().unwrap();
    parent_repo(parent.path());
    let root = parent.path().join("target/store");
    let store = FsStore::init(&root, &StoreMetadata::new("HS")).unwrap();
    store.write_ticket(&ticket(A, "before init")).unwrap();

    let ix = index(&store);
    ix.reconcile(&store).unwrap();
    assert_eq!(ix.get_meta("head").unwrap(), None);

    // The recovery step from the diagnostic: make the store its own repository.
    git(&root, &["init", "-q", "-b", "main"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "own"]);
    store.write_ticket(&ticket(B, "after init")).unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["after init", "before init"]);
    assert_eq!(
        ix.get_meta("head").unwrap().as_deref(),
        Some(git(&root, &["rev-parse", "HEAD"]).as_str())
    );

    // Back to unowned (the store's repository removed): the old baseline is cleared.
    std::fs::remove_dir_all(root.join(".git")).unwrap();
    store.write_ticket(&ticket(C, "after removal")).unwrap();
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["after init", "after removal", "before init"]);
    assert_eq!(ix.get_meta("head").unwrap(), None);
    assert_eq!(ix.get_meta(GIT_OWNER_META).unwrap(), None);
}

#[test]
fn hs2_abv2j3_ignored_ticket_files_in_an_owned_repo_stay_visible() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let store = FsStore::init(root, &StoreMetadata::new("HS")).unwrap();
    git(root, &["init", "-q", "-b", "main"]);
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "-m", "base"]);
    std::fs::write(root.join(".git/info/exclude"), "tickets/\n").unwrap();

    let ix = index(&store);
    ix.reconcile(&store).unwrap();
    assert!(ix.get_meta("head").unwrap().is_some(), "warm baseline");

    store.write_ticket(&ticket(A, "ignored create")).unwrap();
    assert_eq!(store.changed_ticket_ids_in_worktree().unwrap(), [ulid(A)]);
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["ignored create"]);
    retitle(&store, A, "ignored update");
    ix.reconcile(&store).unwrap();
    assert_eq!(titles(&ix), ["ignored update"]);
    std::fs::remove_file(store.ticket_path(&ulid(A))).unwrap();
    assert_eq!(ix.reconcile(&store).unwrap(), (0, 1));
}
