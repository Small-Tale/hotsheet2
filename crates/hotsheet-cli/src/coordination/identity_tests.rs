//! HS2-67N8SJ: a pool written before its checkout and ticket store moved follows the move
//! only when the checkout registry proves both ids; everything else is refused. Each test
//! uses its own temporary repos, stores and registry home.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use hotsheet_ticketing::StoreMetadata;
use hotsheet_ticketing::checkouts::{CheckoutRegistry, TicketSource};

use super::identity::MIGRATION_LOG;
use super::tests::{NOW, register};
use super::*;

struct Moved {
    root: tempfile::TempDir,
    project: PathBuf,
    store: PathBuf,
    old_project_id: String,
    old_source_id: String,
}

impl Moved {
    fn home(&self) -> PathBuf {
        self.root.path().join("home")
    }

    fn pool(&self) -> PathBuf {
        self.project.join(".git/hotsheet-coordination/state.json")
    }

    fn log(&self) -> String {
        fs::read_to_string(
            self.project
                .join(".git/hotsheet-coordination")
                .join(MIGRATION_LOG),
        )
        .unwrap_or_default()
    }

    fn open(&self) -> Result<Coordinator> {
        Coordinator::open_with_registry_home(&self.project, &self.store, self.home())
    }

    fn repair(&self, dry_run: bool) -> Result<IdentityReport> {
        Coordinator::repair_identity(&self.project, &self.store, self.home(), dry_run)
    }

    fn connection(path: &Path) -> String {
        TicketSource::git(path).connection_id
    }
}

fn init_project(path: &Path) {
    fs::create_dir(path).unwrap();
    assert!(
        hotsheet_ticketing::git::launch("git")
            .args(["init", "-q"])
            .arg(path)
            .status()
            .unwrap()
            .success()
    );
}

/// A pool with a worker, created at `old-project` + `old-store`, then both directories
/// renamed. When `registry` is set, the checkout was registered at the old paths and then
/// relocated and relinked through the registry's own API, as the maintainer's move was.
fn moved(registry: bool) -> Moved {
    let root = tempfile::tempdir().unwrap();
    let old_project = root.path().join("old-project");
    let old_store = root.path().join("old-store");
    init_project(&old_project);
    FsStore::init_git(&old_store, &StoreMetadata::new("HS")).unwrap();
    let home = root.path().join("home");
    let c = Coordinator::open_with_registry_home(&old_project, &old_store, home.clone()).unwrap();
    register(&c, Role::Worker, "worker", "worker-1", NOW);
    let (old_project_id, old_source_id) = (c.project_id.clone(), c.source_id.clone());
    let old_connection = Moved::connection(&old_store);
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    if registry {
        let entry = checkouts
            .register_sources(
                &old_project,
                None,
                None,
                vec![TicketSource::git(&old_store)],
                None,
            )
            .unwrap();
        assert_eq!(entry.id, old_project_id);
    }
    let project = root.path().join("project");
    let store = root.path().join("store");
    fs::rename(&old_project, &project).unwrap();
    fs::rename(&old_store, &store).unwrap();
    if registry {
        checkouts.relocate(&old_project_id, &project, None).unwrap();
        checkouts
            .relink_git_source(&old_project_id, &old_connection, &store)
            .unwrap();
    }
    Moved {
        root,
        project,
        store,
        old_project_id,
        old_source_id,
    }
}

/// Replace the registry with one entry at the moved root and the given aliases.
fn write_registry(m: &Moved, id: &str, aliases: &[(&str, &str)]) {
    let root = hotsheet_sync::paths::canonicalize(&m.project).unwrap();
    let store = hotsheet_sync::paths::canonicalize(&m.store).unwrap();
    let aliases: serde_json::Map<String, serde_json::Value> = aliases
        .iter()
        .map(|(from, to)| ((*from).to_owned(), (*to).into()))
        .collect();
    let file = serde_json::json!({
        "schemaVersion": 4,
        "checkouts": [{
            "id": id,
            "root": root,
            "alias": "project",
            "sources": [{
                "connection_id": Moved::connection(&store),
                "provider": "git",
                "locator": store,
            }],
        }],
        "sourceAliases": { id: aliases },
    });
    fs::create_dir_all(m.home()).unwrap();
    fs::write(
        m.home().join("checkouts.json"),
        serde_json::to_vec(&file).unwrap(),
    )
    .unwrap();
}

#[test]
fn hs2_67n8sj_relocated_checkout_and_renamed_store_migrate_automatically() {
    let m = moved(true);
    let before: State = serde_json::from_slice(&fs::read(m.pool()).unwrap()).unwrap();
    let c = m.open().unwrap();
    let state = c.state().unwrap();
    assert_ne!(state.project_id, m.old_project_id);
    assert_ne!(state.source_id, m.old_source_id);
    assert_eq!(state.project_id, c.project_id);
    assert_eq!(state.source_id, c.source_id);
    // Every other field is kept; only the revision records the write.
    assert_eq!(state.workers, before.workers);
    assert_eq!(state.version, before.version);
    assert_eq!(state.revision, before.revision + 1);
    let log = m.log();
    assert_eq!(log.lines().count(), 1, "{log}");
    assert!(log.contains("automatic identity migration"), "{log}");
    assert!(log.contains(&m.old_project_id) && log.contains(&state.project_id));
    assert!(log.contains("source alias chain"), "{log}");
}

#[test]
fn hs2_67n8sj_project_match_with_a_foreign_source_chain_is_refused() {
    let m = moved(false);
    // The project id is this checkout's durable id, but the old store's connection id
    // aliases to a store this checkout does not link.
    let old = &m.old_source_id[..16];
    write_registry(&m, &m.old_project_id, &[(old, "ffffffffffffffff")]);
    let bytes = fs::read(m.pool()).unwrap();
    let message = m.open().err().unwrap().to_string();
    assert!(
        message.contains("incompatible coordination state"),
        "{message}"
    );
    assert!(message.contains("does not reach"), "{message}");
    assert!(message.contains("repair-identity --dry-run"), "{message}");
    let report = m.repair(false).unwrap();
    assert_eq!(report.status, IdentityStatus::Refused);
    assert!(!report.migrated);
    assert_eq!(fs::read(m.pool()).unwrap(), bytes, "a refusal never writes");
    assert!(m.log().is_empty());
}

#[test]
fn hs2_67n8sj_an_unrelated_pool_is_refused() {
    let m = moved(false);
    let old = &m.old_source_id[..16];
    let current = Moved::connection(&m.store);
    // The registry's checkout at this root has another id; the alias chain alone is no proof.
    write_registry(&m, "someone-else-000000000000", &[(old, &current)]);
    let bytes = fs::read(m.pool()).unwrap();
    assert!(
        m.open()
            .err()
            .unwrap()
            .to_string()
            .contains("neither this checkout's id")
    );
    let report = m.repair(false).unwrap();
    assert_eq!(report.status, IdentityStatus::Refused);
    assert_eq!(fs::read(m.pool()).unwrap(), bytes);
    // No registry at all is refused too.
    fs::remove_file(m.home().join("checkouts.json")).unwrap();
    let report = m.repair(false).unwrap();
    assert_eq!(report.status, IdentityStatus::Refused);
    assert!(
        report
            .refusal
            .unwrap()
            .contains("no checkout is registered")
    );
    assert_eq!(fs::read(m.pool()).unwrap(), bytes);
}

#[test]
fn hs2_67n8sj_an_alias_cycle_is_refused_without_looping() {
    let m = moved(false);
    let old = &m.old_source_id[..16];
    write_registry(
        &m,
        &m.old_project_id,
        &[(old, "1111111111111111"), ("1111111111111111", old)],
    );
    let report = m.repair(false).unwrap();
    assert_eq!(report.status, IdentityStatus::Refused);
    assert!(report.refusal.unwrap().contains("source alias cycle"));
    assert!(m.open().err().unwrap().to_string().contains("cycle"));
}

#[test]
fn hs2_67n8sj_only_the_source_moved_names_the_source_error_and_repair() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    init_project(&project);
    let old_store = root.path().join("old-store");
    FsStore::init_git(&old_store, &StoreMetadata::new("HS")).unwrap();
    let home = root.path().join("home");
    Coordinator::open_with_registry_home(&project, &old_store, home.clone()).unwrap();
    // The connection id of the store's former canonical path, taken before it moves.
    let old = TicketSource::git(&old_store).connection_id;
    let store = root.path().join("store");
    fs::rename(&old_store, &store).unwrap();
    // No alias record: refused with the historic source message plus the repair command.
    let message = Coordinator::open_with_registry_home(&project, &store, home.clone())
        .err()
        .unwrap()
        .to_string();
    assert!(message.contains("bound to another source"), "{message}");
    assert!(message.contains("repair-identity"), "{message}");
    // A registry entry under the current checkout id with the alias proves the move.
    let m = Moved {
        root,
        project: project.clone(),
        store: store.clone(),
        old_project_id: checkouts::checkout_id(&project).unwrap(),
        old_source_id: String::new(),
    };
    write_registry(&m, &m.old_project_id, &[(&old, &Moved::connection(&store))]);
    let report = m.repair(false).unwrap();
    assert_eq!(report.status, IdentityStatus::Equivalent);
    assert_eq!(
        report.project_evidence.as_deref(),
        Some("current checkout id")
    );
    assert!(report.migrated);
    m.open().unwrap();
}

#[test]
fn hs2_67n8sj_dry_run_reports_without_writing() {
    let m = moved(true);
    let bytes = fs::read(m.pool()).unwrap();
    let report = m.repair(true).unwrap();
    assert_eq!(report.status, IdentityStatus::Equivalent);
    assert!(report.dry_run && !report.migrated);
    assert_eq!(
        report.stored_project_id.as_deref(),
        Some(&*m.old_project_id)
    );
    assert_eq!(report.stored_source_id.as_deref(), Some(&*m.old_source_id));
    assert_ne!(report.current_project_id, m.old_project_id);
    assert!(
        report
            .source_evidence
            .unwrap()
            .contains("source alias chain")
    );
    assert_eq!(fs::read(m.pool()).unwrap(), bytes);
    assert!(m.log().is_empty());
}

#[test]
fn hs2_67n8sj_a_current_pool_is_a_no_op_and_an_absent_pool_is_not_created() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    init_project(&project);
    let store = root.path().join("store");
    FsStore::init_git(&store, &StoreMetadata::new("HS")).unwrap();
    let home = root.path().join("home");
    let absent = Coordinator::repair_identity(&project, &store, home.clone(), false).unwrap();
    assert_eq!(absent.status, IdentityStatus::Absent);
    assert!(
        !project
            .join(".git/hotsheet-coordination/state.json")
            .exists()
    );
    Coordinator::open_with_registry_home(&project, &store, home.clone()).unwrap();
    let pool = project.join(".git/hotsheet-coordination/state.json");
    let bytes = fs::read(&pool).unwrap();
    let report = Coordinator::repair_identity(&project, &store, home, false).unwrap();
    assert_eq!(report.status, IdentityStatus::Current);
    assert!(!report.migrated);
    assert_eq!(fs::read(&pool).unwrap(), bytes);
}

#[test]
fn hs2_67n8sj_concurrent_opens_migrate_once_under_the_lock() {
    let m = moved(true);
    let handles: Vec<_> = (0..8)
        .map(|_| {
            let (project, store, home) = (m.project.clone(), m.store.clone(), m.home());
            std::thread::spawn(move || {
                Coordinator::open_with_registry_home(&project, &store, home)
                    .map(|c| c.state().unwrap().project_id)
            })
        })
        .collect();
    let ids: BTreeSet<String> = handles
        .into_iter()
        .map(|handle| handle.join().unwrap().unwrap())
        .collect();
    assert_eq!(ids.len(), 1);
    assert_eq!(m.log().lines().count(), 1, "exactly one migration");
}

#[test]
fn hs2_67n8sj_repeated_repairs_are_idempotent() {
    let m = moved(true);
    let first = m.repair(false).unwrap();
    assert_eq!(first.status, IdentityStatus::Equivalent);
    assert!(first.migrated);
    assert!(m.log().contains("repair identity migration"));
    let bytes = fs::read(m.pool()).unwrap();
    for _ in 0..2 {
        let again = m.repair(false).unwrap();
        assert_eq!(again.status, IdentityStatus::Current);
        assert!(!again.migrated);
    }
    m.open().unwrap();
    assert_eq!(fs::read(m.pool()).unwrap(), bytes);
    assert_eq!(m.log().lines().count(), 1);
}
