//! HS2-SHH7WW: several targets declared as sites of one project. Membership
//! transitions, the version-2 manifest, binding to one ticket store, and one plan step
//! and one active run per ticket of a project. Synthetic roots and homes only.
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::*;

struct Clock(u64);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}
const OWNER: &str = "owner-0123456789ab";
fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn target(value: u128, label: &str, site_of: Option<u128>) -> TargetDeclaration {
    TargetDeclaration {
        target_id: id(value),
        state: DeclarationState::Active,
        label: label.into(),
        expected_repository_identity: None,
        site_of: site_of.map(id),
    }
}
fn rename(revision: Option<String>, label: &str) -> RenameTarget {
    RenameTarget {
        expected_revision: revision.unwrap(),
        label: label.into(),
        expected_repository_identity: None,
        clear_expected_repository_identity: false,
        site_of: None,
        clear_site_of: false,
    }
}
fn site_error(result: Result<ManifestView, OrchestrationError>) -> SiteError {
    match result {
        Err(OrchestrationError::Site(error)) => error,
        other => panic!("expected a site error, got {other:?}"),
    }
}
fn version(root: &Path) -> u64 {
    let bytes = std::fs::read(root.join(".hotsheet2/orchestration.json")).unwrap();
    serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["schema_version"]
        .as_u64()
        .unwrap()
}
fn ticket(target: u128, native: &str) -> RunTarget {
    RunTarget {
        target_id: id(target),
        connection_id: "target-source".into(),
        native_id: native.into(),
    }
}

#[test]
fn hs2_shh7ww_manifest_is_version_one_until_a_site_is_declared() {
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let one = store
        .add(None, target(1, "Widgets", None), &mut tx)
        .unwrap();
    let two = store
        .add(
            one.revision,
            target(2, "Widgets on the build host", None),
            &mut tx,
        )
        .unwrap();
    assert_eq!(version(root.path()), 1);
    let ungrouped = std::fs::read(root.path().join(".hotsheet2/orchestration.json")).unwrap();
    assert!(!String::from_utf8_lossy(&ungrouped).contains("site_of"));

    let grouped = store
        .rename(
            &id(2),
            RenameTarget {
                site_of: Some(id(1)),
                ..rename(two.revision, "Widgets on the build host")
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(version(root.path()), 2);
    assert_eq!(grouped.targets[1].site_of, Some(id(1)));
    // An older binary decodes with the shared version-1 reader: it reports an upgrade
    // instead of reading the file without the site, and so never overwrites it.
    let bytes = std::fs::read(root.path().join(".hotsheet2/orchestration.json")).unwrap();
    assert!(matches!(
        storage::decode::<TargetManifest>(&bytes),
        Err(OrchestrationError::UnsupportedVersion { found: 2 })
    ));
    let sites = store.sites().unwrap();
    assert_eq!(sites.project_of(&id(2)), id(1));
    assert_eq!(sites.sites_of(&id(1)), vec![id(1), id(2)]);

    let separated = store
        .rename(
            &id(2),
            RenameTarget {
                clear_site_of: true,
                ..rename(grouped.revision, "Widgets on the build host")
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(version(root.path()), 1);
    assert!(separated.targets.iter().all(|t| t.site_of.is_none()));
    assert!(store.sites().unwrap().is_empty());

    // A version-1 file that names a site was not written by any version: damaged.
    let forged = String::from_utf8(ungrouped).unwrap().replacen(
        "\"label\":\"Widgets on",
        &format!("\"site_of\":\"{}\",\"label\":\"Widgets on", id(1)),
        1,
    );
    std::fs::write(root.path().join(".hotsheet2/orchestration.json"), forged).unwrap();
    assert!(matches!(store.read(), Err(OrchestrationError::Corrupt)));
    // A newer version is refused, never overwritten.
    std::fs::write(
        root.path().join(".hotsheet2/orchestration.json"),
        format!(
            r#"{{"schema_version":3,"revision":"{}","targets":[]}}"#,
            id(9)
        ),
    )
    .unwrap();
    assert!(matches!(
        store.read(),
        Err(OrchestrationError::UnsupportedVersion { found: 3 })
    ));
}

/// Every membership transition, including the refused ones, and what each leaves.
#[test]
fn hs2_shh7ww_membership_transition_matrix() {
    let root = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let mut view = store
        .add(None, target(1, "Widgets", None), &mut tx)
        .unwrap();
    view = store
        .add(view.revision, target(3, "Gadgets", None), &mut tx)
        .unwrap();
    // Add as a site.
    view = store
        .add(view.revision, target(2, "Widgets remote", Some(1)), &mut tx)
        .unwrap();
    assert_eq!(store.sites().unwrap().sites_of(&id(1)), vec![id(1), id(2)]);
    // Adding a site of a project that does not exist, or of a site (a chain).
    assert_eq!(
        site_error(store.add(view.revision.clone(), target(4, "Lost", Some(99)), &mut tx)),
        SiteError::ProjectUnknown
    );
    assert_eq!(
        site_error(store.add(view.revision.clone(), target(4, "Chain", Some(2)), &mut tx)),
        SiteError::Chain
    );
    // A project with sites cannot itself become a site.
    assert_eq!(
        site_error(store.rename(
            &id(1),
            RenameTarget {
                site_of: Some(id(3)),
                ..rename(view.revision.clone(), "Widgets")
            },
            &mut tx
        )),
        SiteError::Chain
    );
    // A target is never a site of itself.
    assert!(matches!(
        store.rename(
            &id(3),
            RenameTarget {
                site_of: Some(id(3)),
                ..rename(view.revision.clone(), "Gadgets")
            },
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    // Setting and clearing at once is refused without a write.
    assert!(matches!(
        store.rename(
            &id(2),
            RenameTarget {
                site_of: Some(id(1)),
                clear_site_of: true,
                ..rename(view.revision.clone(), "Widgets remote")
            },
            &mut tx
        ),
        Err(OrchestrationError::Invalid(_))
    ));
    // Renaming a site keeps it a site.
    view = store
        .rename(
            &id(2),
            rename(view.revision, "Widgets on the build host"),
            &mut tx,
        )
        .unwrap();
    assert_eq!(view.targets[2].site_of, Some(id(1)));
    // Repository identities of one project's sites must agree.
    view = store
        .rename(
            &id(1),
            RenameTarget {
                expected_repository_identity: Some("https://repo.example.test/widgets".into()),
                ..rename(view.revision, "Widgets")
            },
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        site_error(store.rename(
            &id(2),
            RenameTarget {
                expected_repository_identity: Some("https://repo.example.test/other".into()),
                ..rename(view.revision.clone(), "Widgets on the build host")
            },
            &mut tx
        )),
        SiteError::RepositoryMismatch
    );
    view = store
        .rename(
            &id(2),
            RenameTarget {
                expected_repository_identity: Some("https://repo.example.test/widgets".into()),
                ..rename(view.revision, "Widgets on the build host")
            },
            &mut tx,
        )
        .unwrap();
    // Regroup the site under another project (Gadgets declares no repository).
    view = store
        .rename(
            &id(2),
            RenameTarget {
                site_of: Some(id(3)),
                ..rename(view.revision, "Widgets on the build host")
            },
            &mut tx,
        )
        .unwrap();
    let sites = store.sites().unwrap();
    assert_eq!(sites.sites_of(&id(1)), vec![id(1)]);
    assert_eq!(sites.sites_of(&id(3)), vec![id(3), id(2)]);
    // A project cannot be removed while a site of it is active.
    assert_eq!(
        site_error(store.remove(view.revision.clone().unwrap(), &id(3), &mut tx)),
        SiteError::ProjectHasSites
    );
    // Remove the site, then the project.
    view = store
        .remove(view.revision.unwrap(), &id(2), &mut tx)
        .unwrap();
    view = store
        .remove(view.revision.unwrap(), &id(3), &mut tx)
        .unwrap();
    // The tombstone keeps the project it belonged to, so its history still counts there.
    let removed = view.targets.iter().find(|t| t.target_id == id(2)).unwrap();
    assert_eq!(removed.state, DeclarationState::Removed);
    assert_eq!(removed.site_of, Some(id(3)));
    // No new site of a removed project.
    assert_eq!(
        site_error(store.add(view.revision.clone(), target(5, "Late", Some(3)), &mut tx)),
        SiteError::ProjectUnknown
    );
    // The last site gone, the manifest is version 1 again only once no entry names a
    // project; a tombstone still does, so it stays version 2.
    assert_eq!(version(root.path()), 2);
}

/// Observes whatever store a candidate checkout reaches; no transport.
struct Stores {
    root: PathBuf,
    projections: Mutex<Vec<CheckoutProjection>>,
}
impl TargetObserver for Stores {
    type Proof = ();
    fn observe(
        &self,
        _: &TargetRef,
        destination: &TargetDestination,
    ) -> Result<ObservedTarget<()>, OrchestrationError> {
        let found = self
            .projections
            .lock()
            .unwrap()
            .iter()
            .find(|projection| projection.checkout_id == destination.checkout_id())
            .cloned()
            .ok_or(TargetFailure::SourceMissing);
        Ok(ObservedTarget::new((), found))
    }
    fn with_source_guard<T>(
        &self,
        _: &str,
        root: &Path,
        _: Option<&()>,
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        assert_eq!(root, self.root);
        commit()
    }
    fn with_remote_guard<T>(
        &self,
        _: &(),
        commit: impl FnOnce() -> Result<T, OrchestrationError>,
    ) -> Result<T, OrchestrationError> {
        commit()
    }
}
/// A checkout of the ticket store `store` (one letter of its synthetic identity).
fn checkout(name: &str, store: char) -> (CheckoutProjection, TargetDestination) {
    checkout_on(name, store, "tickets")
}
/// HS2-RDBMVM: the same, with the store under the connection id `connection`, as
/// each machine names one store by its own local path.
fn checkout_on(
    name: &str,
    store: char,
    connection: &str,
) -> (CheckoutProjection, TargetDestination) {
    let sources = vec![TargetSource {
        connection_id: connection.into(),
        provider: "git".into(),
        identity: Some(store.to_string().repeat(64)),
        state: SourceState::Available,
        capabilities: Some(crate::ProviderCapabilities::git()),
    }];
    let projection = CheckoutProjection {
        schema_version: 1,
        server_id: id(77),
        checkout_id: name.into(),
        checkout_authority: checkout_authority(&sources).unwrap(),
        sources,
    };
    let destination = TargetDestination::Local {
        server_id: projection.server_id.clone(),
        checkout_id: projection.checkout_id.clone(),
        checkout_authority: projection.checkout_authority.clone(),
    };
    (projection, destination)
}
fn bind(
    flow: &Orchestration,
    stores: &Stores,
    target: u128,
    destination: &TargetDestination,
    tx: &mut Transaction<'_>,
) -> Result<TargetResolution, OrchestrationError> {
    let reference = TargetRef {
        owner_checkout_id: OWNER.into(),
        target_id: id(target),
    };
    let binding = flow.binding(&reference)?;
    flow.bind(
        BindTarget {
            target: reference,
            expected_manifest_revision: flow.declarations()?.revision.unwrap(),
            expected_binding_revision: binding.revision,
            previous_destination: binding.destination,
            destination: destination.clone(),
        },
        stores,
        tx,
    )
}

/// Every site of a project reaches one ticket store: binding or rebinding a site to
/// another store is refused, and so is grouping targets bound to different stores.
#[test]
fn hs2_shh7ww_a_site_bound_to_another_ticket_store_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (local, at_local) = checkout("widgets-local", 'a');
    let (remote, at_remote) = checkout("widgets-remote", 'a');
    let (other, at_other) = checkout("widgets-fork", 'b');
    let stores = Stores {
        root: root.path().into(),
        projections: Mutex::new(vec![local, remote, other]),
    };
    let flow = Orchestration::new(
        OWNER.into(),
        ManifestStore::new(root.path()),
        BindingRegistry::new(home.path()),
    )
    .unwrap();
    flow.add(
        AddTarget {
            expected_revision: None,
            target_id: Some(id(1)),
            label: "Widgets".into(),
            expected_repository_identity: None,
            site_of: None,
        },
        &stores,
        &mut tx,
    )
    .unwrap();
    let view = flow
        .add(
            AddTarget {
                expected_revision: flow.declarations().unwrap().revision,
                target_id: Some(id(2)),
                label: "Widgets remote".into(),
                expected_repository_identity: None,
                site_of: Some(id(1)),
            },
            &stores,
            &mut tx,
        )
        .unwrap();
    assert_eq!(view.targets[1].site_of, Some(id(1)));
    bind(&flow, &stores, 1, &at_local, &mut tx).unwrap();
    // Another store for the second site: refused before any write.
    let before = flow
        .binding(&TargetRef {
            owner_checkout_id: OWNER.into(),
            target_id: id(2),
        })
        .unwrap();
    let refused = bind(&flow, &stores, 2, &at_other, &mut tx).unwrap_err();
    assert!(matches!(
        refused,
        OrchestrationError::Site(SiteError::StoreMismatch)
    ));
    assert_eq!(refused.code(), "site_store_mismatch");
    assert_eq!(
        flow.binding(&TargetRef {
            owner_checkout_id: OWNER.into(),
            target_id: id(2),
        })
        .unwrap(),
        before
    );
    // The same store through another checkout is the point of a site.
    bind(&flow, &stores, 2, &at_remote, &mut tx).unwrap();
    // Rebinding the project's own target to another store is refused too.
    assert!(matches!(
        bind(&flow, &stores, 1, &at_other, &mut tx),
        Err(OrchestrationError::Site(SiteError::StoreMismatch))
    ));
    // Separate the site; now each may go its own way.
    let revision = flow.declarations().unwrap().revision;
    flow.rename(
        &TargetRef {
            owner_checkout_id: OWNER.into(),
            target_id: id(2),
        },
        RenameTarget {
            clear_site_of: true,
            ..rename(revision, "Widgets remote")
        },
        &stores,
        &mut tx,
    )
    .unwrap();
    bind(&flow, &stores, 2, &at_other, &mut tx).unwrap();
    // Grouping two targets bound to different stores is refused.
    let revision = flow.declarations().unwrap().revision;
    let grouping = flow.rename(
        &TargetRef {
            owner_checkout_id: OWNER.into(),
            target_id: id(2),
        },
        RenameTarget {
            site_of: Some(id(1)),
            ..rename(revision.clone(), "Widgets remote")
        },
        &stores,
        &mut tx,
    );
    assert!(matches!(
        grouping,
        Err(OrchestrationError::Site(SiteError::StoreMismatch))
    ));
    assert_eq!(flow.declarations().unwrap().revision, revision);

    // The sites view lists each project with its sites and this machine's bindings.
    let sites = flow.sites().unwrap();
    assert_eq!(sites.revision, revision);
    let labels: Vec<_> = sites.projects.iter().map(|p| p.label.as_str()).collect();
    assert_eq!(labels, ["Widgets", "Widgets remote"]);
    assert_eq!(sites.projects[0].sites.len(), 1);
    assert!(sites.projects[0].sites[0].project);
    assert_eq!(sites.projects[0].sites[0].destination, Some(at_local));
}

fn new_run(operation: &str, target: u128, native: &str, worker: &str) -> NewRun {
    NewRun {
        operation_id: operation.into(),
        ticket: RunTicket {
            connection_id: "owner-source".into(),
            native_id: id(500),
        },
        target: ticket(target, native),
        mode: DispatchMode::Direct,
        worker_id: Some(worker.into()),
    }
}

/// One ticket of a project has one active run whichever site dispatched it, and a
/// worker one direct run per project; ungrouped targets keep their own runs.
#[test]
fn hs2_shh7ww_one_active_run_per_ticket_of_a_project() {
    let home = tempfile::tempdir().unwrap();
    let runs = RunRegistry::new(home.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let sites = ProjectSites::of(&[target(1, "Widgets", None), target(2, "Remote", Some(1))]);
    let (first, created) = runs
        .begin_in(
            OWNER,
            new_run("op-1", 1, "T-1", "worker-a"),
            &sites,
            None,
            &mut tx,
        )
        .unwrap();
    assert!(created);
    // The same ticket through the other site.
    assert!(matches!(
        runs.begin_in(
            OWNER,
            new_run("op-2", 2, "T-1", "worker-b"),
            &sites,
            None,
            &mut tx
        ),
        Err(OrchestrationError::Run(RunError::TargetActive))
    ));
    // The same worker on another ticket of the project, through the other site.
    assert!(matches!(
        runs.begin_in(
            OWNER,
            new_run("op-3", 2, "T-2", "worker-a"),
            &sites,
            None,
            &mut tx
        ),
        Err(OrchestrationError::Run(RunError::WorkerBusy))
    ));
    // Another worker on another ticket is fine, through either site.
    runs.begin_in(
        OWNER,
        new_run("op-4", 2, "T-2", "worker-b"),
        &sites,
        None,
        &mut tx,
    )
    .unwrap();
    // A repeat of the first operation returns the same run.
    let (again, created) = runs
        .begin_in(
            OWNER,
            new_run("op-1", 1, "T-1", "worker-a"),
            &sites,
            None,
            &mut tx,
        )
        .unwrap();
    assert!(!created);
    assert_eq!(again.run_id, first.run_id);
    // Once the first run is finished, the ticket may be dispatched through the other
    // site; the run keeps the concrete site it names.
    runs.update(
        OWNER,
        &first.run_id,
        RunUpdate {
            to: Some(RunState::Cancelled),
            reason: Some("operator_cancelled".into()),
            ..RunUpdate::default()
        },
        &mut tx,
    )
    .unwrap();
    let (moved, _) = runs
        .begin_in(
            OWNER,
            new_run("op-5", 2, "T-1", "worker-c"),
            &sites,
            None,
            &mut tx,
        )
        .unwrap();
    assert_eq!(moved.target.target_id, id(2));
    // Without a grouping the two targets are separate projects, as before.
    runs.begin_in(
        OWNER,
        new_run("op-6", 1, "T-1", "worker-d"),
        &ProjectSites::default(),
        None,
        &mut tx,
    )
    .unwrap();
}

/// Adversarial re-keying: records written before a grouping are read through it and
/// never rewritten, and a grouping that would make them ambiguous is refused.
#[test]
fn hs2_shh7ww_grouping_is_refused_while_two_active_runs_stand_on_one_ticket() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let store = ManifestStore::new(root.path()).on_machine(home.path(), OWNER);
    let runs = RunRegistry::new(home.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let mut view = store
        .add(None, target(1, "Widgets", None), &mut tx)
        .unwrap();
    view = store
        .add(view.revision, target(2, "Widgets remote", None), &mut tx)
        .unwrap();
    // Before the grouping these were two projects: one run each on "the same" ticket.
    let ungrouped = store.sites().unwrap();
    let (a, _) = runs
        .begin_in(
            OWNER,
            new_run("op-a", 1, "T-1", "worker-a"),
            &ungrouped,
            None,
            &mut tx,
        )
        .unwrap();
    let (b, _) = runs
        .begin_in(
            OWNER,
            new_run("op-b", 2, "T-1", "worker-b"),
            &ungrouped,
            None,
            &mut tx,
        )
        .unwrap();
    let group = |revision: Option<String>, tx: &mut Transaction<'_>| {
        store.rename(
            &id(2),
            RenameTarget {
                site_of: Some(id(1)),
                ..rename(revision, "Widgets remote")
            },
            tx,
        )
    };
    assert_eq!(
        site_error(group(view.revision.clone(), &mut tx)),
        SiteError::RunConflict
    );
    assert_eq!(version(root.path()), 1);
    // Repeating the refused change refuses again; nothing was half-applied.
    assert_eq!(
        site_error(group(view.revision.clone(), &mut tx)),
        SiteError::RunConflict
    );
    // A finished run no longer stands on the ticket.
    runs.update(
        OWNER,
        &b.run_id,
        RunUpdate {
            to: Some(RunState::Completed),
            reason: Some("target_completed".into()),
            ..RunUpdate::default()
        },
        &mut tx,
    )
    .unwrap();
    view = group(view.revision, &mut tx).unwrap();
    assert_eq!(view.targets[1].site_of, Some(id(1)));
    // The stored runs still name the sites they were dispatched through.
    let stored = runs.list(OWNER, false).unwrap();
    assert_eq!(stored[0].target, a.target);
    assert_eq!(stored[1].target, b.target);
    // And the grouping now keeps a second active run off the ticket.
    let sites = store.sites().unwrap();
    assert!(matches!(
        runs.begin_in(
            OWNER,
            new_run("op-c", 2, "T-1", "worker-c"),
            &sites,
            None,
            &mut tx
        ),
        Err(OrchestrationError::Run(RunError::TargetActive))
    ));
    // Separating and grouping again (empty, then refill) holds the same rule.
    view = store
        .rename(
            &id(2),
            RenameTarget {
                clear_site_of: true,
                ..rename(view.revision, "Widgets remote")
            },
            &mut tx,
        )
        .unwrap();
    runs.begin_in(
        OWNER,
        new_run("op-d", 2, "T-1", "worker-d"),
        &store.sites().unwrap(),
        None,
        &mut tx,
    )
    .unwrap();
    assert_eq!(
        site_error(group(view.revision, &mut tx)),
        SiteError::RunConflict
    );
}

struct Plan {
    root: tempfile::TempDir,
    home: tempfile::TempDir,
    registry: PlanRegistry,
}
fn plan() -> Plan {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let anchor = root.path().to_path_buf();
    Plan {
        registry: PlanRegistry::new(home.path(), move |owner| {
            assert_eq!(owner, OWNER);
            Ok(anchor.clone())
        }),
        root,
        home,
    }
}
fn define(
    registry: &PlanRegistry,
    target: RunTarget,
    prerequisites: Vec<Prerequisite>,
    tx: &mut Transaction<'_>,
) -> Result<(PlanStep, bool), OrchestrationError> {
    registry.define(
        OWNER,
        NewPlanStep {
            ticket: RunTicket {
                connection_id: "owner-source".into(),
                native_id: id(500),
            },
            target,
            prerequisites,
            resources: Vec::new(),
            tool: None,
            model: None,
            requirements: Default::default(),
        },
        Some("planner".into()),
        tx,
    )
}

/// One plan step per ticket of a project: planning it through another site moves the
/// step there, finding it through either site finds it, and grouping targets whose
/// checked-in plan already has two steps on one ticket is refused.
#[test]
fn hs2_shh7ww_one_plan_step_per_ticket_of_a_project() {
    let Plan {
        root,
        home,
        registry,
    } = plan();
    let store = ManifestStore::new(root.path()).on_machine(home.path(), OWNER);
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let mut view = store
        .add(None, target(1, "Widgets", None), &mut tx)
        .unwrap();
    view = store
        .add(view.revision, target(2, "Widgets remote", None), &mut tx)
        .unwrap();
    let (on_a, _) = define(&registry, ticket(1, "T-1"), Vec::new(), &mut tx).unwrap();
    let (on_b, _) = define(&registry, ticket(2, "T-1"), Vec::new(), &mut tx).unwrap();
    assert_ne!(on_a.step_id, on_b.step_id);
    let group = |revision: Option<String>, tx: &mut Transaction<'_>| {
        store.rename(
            &id(2),
            RenameTarget {
                site_of: Some(id(1)),
                ..rename(revision, "Widgets remote")
            },
            tx,
        )
    };
    assert_eq!(
        site_error(group(view.revision.clone(), &mut tx)),
        SiteError::PlanConflict
    );
    registry.remove(OWNER, &on_b.step_id, &mut tx).unwrap();
    view = group(view.revision, &mut tx).unwrap();
    assert_eq!(view.targets[1].site_of, Some(id(1)));
    // Through the other site, the ticket's step is found and changed, not doubled.
    let found = registry
        .find_target(OWNER, &ticket(2, "T-1"))
        .unwrap()
        .unwrap();
    assert_eq!(found.step_id, on_a.step_id);
    let (moved, changed) = define(&registry, ticket(2, "T-1"), Vec::new(), &mut tx).unwrap();
    assert!(changed);
    assert_eq!(moved.step_id, on_a.step_id);
    assert_eq!(moved.target, ticket(2, "T-1"));
    assert_eq!(moved.revision, on_a.revision + 1);
    // Repeating it changes nothing.
    let (same, changed) = define(&registry, ticket(2, "T-1"), Vec::new(), &mut tx).unwrap();
    assert!(!changed);
    assert_eq!(same, moved);
    assert_eq!(registry.records(OWNER).unwrap().len(), 1);
    // Another ticket of the project is its own step.
    let (other, _) = define(&registry, ticket(1, "T-2"), Vec::new(), &mut tx).unwrap();
    assert_ne!(other.step_id, on_a.step_id);
    assert_eq!(registry.records(OWNER).unwrap().len(), 2);
}

/// Readiness joins a ticket by project: a prerequisite named through one site is an
/// edge to the step planned through another, so cycles across sites are found too.
#[test]
fn hs2_shh7ww_readiness_edges_join_tickets_across_sites() {
    let sites = ProjectSites::of(&[target(1, "Widgets", None), target(2, "Remote", Some(1))]);
    let step = |number: u128, target: RunTarget, prerequisites: Vec<Prerequisite>| PlanStep {
        step_id: id(number),
        owner_checkout_id: OWNER.into(),
        ticket: RunTicket {
            connection_id: "owner-source".into(),
            native_id: id(500 + number),
        },
        target,
        prerequisites,
        resources: Vec::new(),
        tool: None,
        model: None,
        requirements: Default::default(),
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        updated_by: None,
    };
    let needs = |target: u128, native: &str| Prerequisite::Ticket {
        target_id: id(target),
        connection_id: "target-source".into(),
        native_id: native.into(),
        gates: GateRequirement::None,
        commit: None,
    };
    let steps = vec![
        step(10, ticket(1, "T-1"), Vec::new()),
        step(11, ticket(1, "T-2"), vec![needs(2, "T-1")]),
    ];
    let grouped = evaluate_plan(
        &steps,
        &PlanFacts {
            sites: sites.clone(),
            ..PlanFacts::default()
        },
    );
    assert_eq!(grouped[0].unblocks, vec![id(11)]);
    let ungrouped = evaluate_plan(&steps, &PlanFacts::default());
    assert!(ungrouped[0].unblocks.is_empty());
    // A cycle that closes only through the other site.
    let cyclic = vec![
        step(10, ticket(1, "T-1"), vec![needs(2, "T-2")]),
        step(11, ticket(2, "T-2"), vec![needs(1, "T-1")]),
    ];
    let answers = evaluate_plan(
        &cyclic,
        &PlanFacts {
            sites,
            ..PlanFacts::default()
        },
    );
    assert!(answers.iter().all(|a| a.level == ReadinessLevel::Cyclic));
}

fn on(target: u128, connection: &str, native: &str) -> RunTarget {
    RunTarget {
        target_id: id(target),
        connection_id: connection.into(),
        native_id: native.into(),
    }
}

/// HS2-3JGPDR: one store has another connection id on each machine. With the sources
/// this machine's bindings confirmed, a site's ids join the home's by source identity;
/// without them, or for a source the home lacks, ids compare as they are.
#[test]
fn hs2_3jgpdr_site_connection_ids_join_the_homes_by_source_identity() {
    let targets = [
        target(1, "Home", None),
        target(2, "Site", Some(1)),
        target(3, "Other project", None),
    ];
    let (home, site, other) = (id(1), id(2), id(3));
    let plain = ProjectSites::of(&targets);
    let sites = ProjectSites::of(&targets).with_sources([
        (
            home.as_str(),
            vec![("home-conn", "store-x"), ("home-extra", "store-y")],
        ),
        (
            site.as_str(),
            vec![("site-conn", "store-x"), ("site-only", "store-z")],
        ),
        (other.as_str(), vec![("other-conn", "store-x")]),
    ]);
    // Without sources nothing is joined.
    assert!(!plain.same_ticket(&on(1, "home-conn", "T"), &on(2, "site-conn", "T")));
    // The site's id for the shared store is the home's.
    assert!(sites.same_ticket(&on(1, "home-conn", "T"), &on(2, "site-conn", "T")));
    assert!(!sites.same_ticket(&on(1, "home-conn", "T"), &on(2, "site-conn", "U")));
    assert_eq!(
        sites.ticket_key(&on(2, "site-conn", "T")),
        sites.ticket_key(&on(1, "home-conn", "T"))
    );
    assert_eq!(
        sites.canonical(&(site.clone(), "site-conn".into(), "T".into())),
        (home.clone(), "home-conn".into(), "T".into())
    );
    assert_eq!(sites.home_connection(&site, "site-conn"), "home-conn");
    // A source the home does not have keeps its own id, and so does another project.
    assert_eq!(sites.home_connection(&site, "site-only"), "site-only");
    assert_eq!(sites.home_connection(&other, "other-conn"), "other-conn");
    // Translating to a site: from the home, from the site, and to the home.
    assert_eq!(sites.connection_on(&home, "home-conn", &site), "site-conn");
    assert_eq!(sites.connection_on(&site, "site-conn", &home), "home-conn");
    assert_eq!(sites.connection_on(&site, "site-conn", &site), "site-conn");
    // A source the target site has no id for is named as the home names it.
    assert_eq!(
        sites.connection_on(&home, "home-extra", &site),
        "home-extra"
    );
    // A grouping change leaves no stale mapping behind: a new grouping starts empty.
    assert_eq!(ProjectSites::of(&targets), plain);
}

/// HS2-EPRWB6: the checked-in plan names a ticket by the home's connection id and each
/// machine reads it back with its own id for the site a step names. Translating from
/// the home, from the site, and back is stable; without joined sources nothing moves.
#[test]
fn hs2_eprwb6_plan_ids_translate_between_the_home_and_a_site() {
    let targets = [target(1, "Home", None), target(2, "Site", Some(1))];
    let (home, site) = (id(1), id(2));
    let plain = ProjectSites::of(&targets);
    let sites = ProjectSites::of(&targets).with_sources([
        (home.as_str(), vec![("home-conn", "store-x")]),
        (
            site.as_str(),
            vec![("site-conn", "store-x"), ("site-only", "store-z")],
        ),
    ]);
    // Written: the home's id, whichever site names the ticket.
    assert_eq!(sites.shared_connection(&site, "site-conn"), "home-conn");
    assert_eq!(sites.shared_connection(&home, "home-conn"), "home-conn");
    // Read: the named site's own id; a home-named ticket stays the home's.
    assert_eq!(sites.local_connection(&site, "home-conn"), "site-conn");
    assert_eq!(sites.local_connection(&home, "home-conn"), "home-conn");
    // A plan written before the ids were shared reads as it was.
    assert_eq!(sites.local_connection(&site, "site-conn"), "site-conn");
    // Round trips are stable, repeated or not.
    for _ in 0..2 {
        let written = sites.shared_connection(&site, "site-conn");
        assert_eq!(sites.local_connection(&site, &written), "site-conn");
        assert_eq!(sites.shared_connection(&site, &written), written);
    }
    // A source the home lacks, and a machine without joined sources, keep the id.
    assert_eq!(sites.shared_connection(&site, "site-only"), "site-only");
    assert_eq!(sites.local_connection(&site, "site-only"), "site-only");
    assert_eq!(plain.shared_connection(&site, "site-conn"), "site-conn");
    assert_eq!(plain.local_connection(&site, "home-conn"), "home-conn");
}

/// A plan registry on one machine over the shared checkout `root`, joining the home's
/// and the site's sources by identity when `bound`.
fn machine(root: &Path, home: &Path, bound: bool) -> PlanRegistry {
    let anchor = root.to_path_buf();
    let registry = PlanRegistry::new(home, move |owner| {
        assert_eq!(owner, OWNER);
        Ok(anchor.clone())
    });
    if !bound {
        return registry;
    }
    registry.with_sites(|owner, sites| {
        assert_eq!(owner, OWNER);
        let (home, site) = (id(1), id(2));
        sites.with_sources([
            (home.as_str(), vec![("home-conn", "store-x")]),
            (site.as_str(), vec![("site-conn", "store-x")]),
        ])
    })
}

fn after(target: u128, connection: &str, native: &str) -> Prerequisite {
    Prerequisite::Ticket {
        target_id: id(target),
        connection_id: connection.into(),
        native_id: native.into(),
        gates: GateRequirement::None,
        commit: None,
    }
}

/// HS2-EPRWB6: one ticket of a project is one checked-in step whichever site and
/// machine defined it. Each machine reads the step with its own ids, a prerequisite
/// named through either site is an edge, and the machine's reads of it are kept.
#[test]
fn hs2_eprwb6_one_checked_in_step_whichever_machine_defines_it() {
    let root = tempfile::tempdir().unwrap();
    let (home_a, home_b, home_c) = (
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
        tempfile::tempdir().unwrap(),
    );
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let view = store.add(None, target(1, "Home", None), &mut tx).unwrap();
    store
        .add(view.revision, target(2, "Site", Some(1)), &mut tx)
        .unwrap();
    let a = machine(root.path(), home_a.path(), true);
    let b = machine(root.path(), home_b.path(), true);
    let checked_in = || SharedPlanStore::new(root.path()).read().unwrap().unwrap();

    // Machine A plans the ticket through the site, after a ticket named through it.
    let (first, changed) = define(
        &a,
        on(2, "site-conn", "T-1"),
        vec![after(2, "site-conn", "T-0")],
        &mut tx,
    )
    .unwrap();
    assert!(changed);
    assert_eq!(first.target, on(2, "site-conn", "T-1"));
    let document = checked_in();
    assert_eq!(document.steps.len(), 1);
    assert_eq!(document.steps[0].target, on(2, "home-conn", "T-1"));
    assert_eq!(
        document.steps[0].prerequisites,
        vec![after(2, "home-conn", "T-0")]
    );

    // Machine B plans the same ticket through the home: the same step, moved.
    let (moved, changed) = define(
        &b,
        on(1, "home-conn", "T-1"),
        vec![after(1, "home-conn", "T-0")],
        &mut tx,
    )
    .unwrap();
    assert!(changed);
    assert_eq!(moved.step_id, first.step_id);
    assert_eq!(moved.revision, first.revision + 1);
    // And back through the site: still one step; repeating it changes nothing.
    let (back, changed) = define(
        &b,
        on(2, "site-conn", "T-1"),
        vec![after(2, "site-conn", "T-0")],
        &mut tx,
    )
    .unwrap();
    assert!(changed);
    assert_eq!(back.step_id, first.step_id);
    let revision = checked_in().revision;
    for registry in [&a, &b] {
        let (same, changed) = define(
            registry,
            on(2, "site-conn", "T-1"),
            vec![after(2, "site-conn", "T-0")],
            &mut tx,
        )
        .unwrap();
        assert!(!changed);
        assert_eq!(same, back);
    }
    assert_eq!(checked_in().revision, revision);
    assert_eq!(checked_in().steps.len(), 1);

    // Either machine finds it through either site, named with its own ids.
    for registry in [&a, &b] {
        for target in [on(1, "home-conn", "T-1"), on(2, "site-conn", "T-1")] {
            let found = registry.find_target(OWNER, &target).unwrap().unwrap();
            assert_eq!(found.step_id, first.step_id);
            assert_eq!(found.target, on(2, "site-conn", "T-1"));
        }
        assert_eq!(registry.records(OWNER).unwrap().len(), 1);
    }

    // The prerequisite planned through the home is an edge on both machines.
    let (before, _) = define(&a, on(1, "home-conn", "T-0"), Vec::new(), &mut tx).unwrap();
    for registry in [&a, &b] {
        let steps: Vec<PlanStep> = registry
            .records(OWNER)
            .unwrap()
            .into_iter()
            .map(|record| record.step)
            .collect();
        let facts = PlanFacts {
            sites: registry.sites(OWNER).unwrap(),
            ..PlanFacts::default()
        };
        let answers = evaluate_plan(&steps, &facts);
        let prerequisite = answers
            .iter()
            .find(|answer| answer.step.step_id == before.step_id)
            .unwrap();
        assert_eq!(prerequisite.unblocks, vec![first.step_id.clone()]);
    }

    // A read through the site with this machine's id is kept for the step.
    let seen = TicketUpdate {
        observed: Some(TicketSeen {
            at_ms: 100,
            exists: true,
            status: "not_started".into(),
            priority: None,
            up_next: None,
            created_at_ms: None,
            opted_out: None,
            slug: None,
        }),
        unreachable: Some(None),
    };
    assert!(a.observe(OWNER, &on(2, "site-conn", "T-1"), seen).unwrap());
    assert!(
        a.tickets(OWNER)
            .unwrap()
            .iter()
            .any(|record| record.target == on(2, "site-conn", "T-1"))
    );

    // A machine that has not joined the sources reads the shared ids as they are.
    let c = machine(root.path(), home_c.path(), false);
    let found = c
        .find_target(OWNER, &on(2, "home-conn", "T-1"))
        .unwrap()
        .unwrap();
    assert_eq!(found.step_id, first.step_id);
}

/// HS2-EPRWB6: a plan checked in before the ids were shared keeps reading as it was.
/// Defining the same ticket again finds that step and does not rewrite it; a change
/// writes it with the home's ids.
#[test]
fn hs2_eprwb6_a_plan_written_with_site_ids_keeps_reading() {
    let root = tempfile::tempdir().unwrap();
    let (home_old, home_new) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let store = ManifestStore::new(root.path());
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let view = store.add(None, target(1, "Home", None), &mut tx).unwrap();
    store
        .add(view.revision, target(2, "Site", Some(1)), &mut tx)
        .unwrap();
    // Written by a machine that compared ids as they were.
    let old = machine(root.path(), home_old.path(), false);
    let (legacy, _) = define(&old, on(2, "site-conn", "T-1"), Vec::new(), &mut tx).unwrap();
    let document = SharedPlanStore::new(root.path()).read().unwrap().unwrap();
    assert_eq!(document.steps[0].target, on(2, "site-conn", "T-1"));

    let new = machine(root.path(), home_new.path(), true);
    assert_eq!(
        new.records(OWNER).unwrap()[0].step.target,
        on(2, "site-conn", "T-1")
    );
    // The same definition through the site, or the ticket found through the home.
    let (same, changed) = define(&new, on(2, "site-conn", "T-1"), Vec::new(), &mut tx).unwrap();
    assert!(!changed);
    assert_eq!(same.step_id, legacy.step_id);
    assert_eq!(
        SharedPlanStore::new(root.path()).read().unwrap().unwrap(),
        document
    );
    assert_eq!(
        new.find_target(OWNER, &on(1, "home-conn", "T-1"))
            .unwrap()
            .unwrap()
            .step_id,
        legacy.step_id
    );
    // A change through the home moves it there, with the home's id.
    let (moved, changed) = define(&new, on(1, "home-conn", "T-1"), Vec::new(), &mut tx).unwrap();
    assert!(changed);
    assert_eq!(moved.step_id, legacy.step_id);
    let document = SharedPlanStore::new(root.path()).read().unwrap().unwrap();
    assert_eq!(document.steps.len(), 1);
    assert_eq!(document.steps[0].target, on(1, "home-conn", "T-1"));
    assert_eq!(document.schema_version, ORCHESTRATION_VERSION);
}

/// HS2-RDBMVM: a grouping change is checked through the sources this machine's
/// bindings confirmed, as the plan registry reads the plan (HS2-EPRWB6). Steps and
/// runs written before the ids were shared, naming one ticket through the site's own
/// connection id and through the home's, are refused; once one is gone the grouping
/// is accepted and the records keep the ids they were written with.
#[test]
fn hs2_rdbmvm_grouping_joins_site_connection_ids_by_bound_sources() {
    let root = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let clock = Clock(100);
    let mut rng = Rng(1);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    // One ticket store, reached under another connection id by each checkout.
    let (local, at_local) = checkout_on("widgets-local", 'a', "home-conn");
    let (remote, at_remote) = checkout_on("widgets-remote", 'a', "site-conn");
    let stores = Stores {
        root: root.path().into(),
        projections: Mutex::new(vec![local, remote]),
    };
    let flow = Orchestration::new(
        OWNER.into(),
        ManifestStore::new(root.path()),
        BindingRegistry::new(home.path()),
    )
    .unwrap();
    for (number, label) in [(1, "Widgets"), (2, "Widgets remote")] {
        flow.add(
            AddTarget {
                expected_revision: flow.declarations().unwrap().revision,
                target_id: Some(id(number)),
                label: label.into(),
                expected_repository_identity: None,
                site_of: None,
            },
            &stores,
            &mut tx,
        )
        .unwrap();
    }
    bind(&flow, &stores, 1, &at_local, &mut tx).unwrap();
    bind(&flow, &stores, 2, &at_remote, &mut tx).unwrap();
    let group = |tx: &mut Transaction<'_>| {
        flow.rename(
            &TargetRef {
                owner_checkout_id: OWNER.into(),
                target_id: id(2),
            },
            RenameTarget {
                site_of: Some(id(1)),
                ..rename(flow.declarations().unwrap().revision, "Widgets remote")
            },
            &stores,
            tx,
        )
    };

    // Two separate projects, so a plan written then has a step through each.
    let registry = machine(root.path(), home.path(), false);
    let (on_home, _) = define(&registry, on(1, "home-conn", "T-1"), Vec::new(), &mut tx).unwrap();
    let (on_site, _) = define(&registry, on(2, "site-conn", "T-1"), Vec::new(), &mut tx).unwrap();
    assert_ne!(on_home.step_id, on_site.step_id);
    let revision = flow.declarations().unwrap().revision;
    // Joined by the store both bindings confirmed, that is two steps on one ticket.
    for _ in 0..2 {
        assert_eq!(site_error(group(&mut tx)), SiteError::PlanConflict);
        assert_eq!(flow.declarations().unwrap().revision, revision);
        assert_eq!(version(root.path()), 1);
    }

    // With one step left, two active runs on one ticket are refused the same way.
    registry.remove(OWNER, &on_site.step_id, &mut tx).unwrap();
    let runs = RunRegistry::new(home.path());
    let ungrouped = ProjectSites::default();
    let run = |operation: &str, target: RunTarget, worker: &str| NewRun {
        target,
        ..new_run(operation, 1, "T-2", worker)
    };
    runs.begin_in(
        OWNER,
        run("op-a", on(1, "home-conn", "T-2"), "worker-a"),
        &ungrouped,
        None,
        &mut tx,
    )
    .unwrap();
    let (on_remote, _) = runs
        .begin_in(
            OWNER,
            run("op-b", on(2, "site-conn", "T-2"), "worker-b"),
            &ungrouped,
            None,
            &mut tx,
        )
        .unwrap();
    assert_eq!(site_error(group(&mut tx)), SiteError::RunConflict);
    runs.update(
        OWNER,
        &on_remote.run_id,
        RunUpdate {
            to: Some(RunState::Completed),
            reason: Some("target_completed".into()),
            ..RunUpdate::default()
        },
        &mut tx,
    )
    .unwrap();

    // Now the grouping holds, and the records keep the ids they were written with.
    let view = group(&mut tx).unwrap();
    assert_eq!(view.targets[1].site_of, Some(id(1)));
    let document = SharedPlanStore::new(root.path()).read().unwrap().unwrap();
    assert_eq!(document.steps.len(), 1);
    assert_eq!(document.steps[0].target, on(1, "home-conn", "T-1"));
    assert_eq!(
        runs.list(OWNER, false).unwrap()[1].target,
        on(2, "site-conn", "T-2")
    );
}
