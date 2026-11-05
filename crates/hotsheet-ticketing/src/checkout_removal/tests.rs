//! HS2-X4RZ73: transition coverage for removing a registered checkout.

use std::path::{Path, PathBuf};

use super::*;
use crate::checkouts::{CheckoutError, TicketSource};
use crate::provider::ProviderConnection;
use crate::store::{FsStore, StoreMetadata};

fn connection(id: &str, credential: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: format!("acme/{id}"),
        name: None,
        default: false,
        settings: serde_json::json!({"credential": {"secret": credential}}),
        disabled: false,
    }
}

fn github(id: &str) -> TicketSource {
    TicketSource {
        connection_id: id.into(),
        provider: "github".into(),
        locator: format!("acme/{id}"),
    }
}

struct Fixture {
    dir: tempfile::TempDir,
    providers: ProviderConfigRegistry,
    checkouts: CheckoutRegistry,
    one: Checkout,
    two: Checkout,
    store: PathBuf,
}

impl Fixture {
    fn registry_json(&self) -> serde_json::Value {
        serde_json::from_str(&std::fs::read_to_string(self.checkouts.path()).unwrap()).unwrap()
    }

    fn provider_ids(&self) -> Vec<String> {
        self.providers
            .load()
            .unwrap()
            .into_iter()
            .map(|connection| connection.id)
            .collect()
    }
}

/// `one` links a Git store, `github-main` (only it), and `github-docs` (shared with `two`).
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let providers = ProviderConfigRegistry::new(dir.path().join("providers.json"));
    providers
        .save(&[
            connection("github-main", "github-app-01ABC"),
            connection("github-docs", "shared-pat"),
        ])
        .unwrap();
    let checkouts = CheckoutRegistry::new(dir.path().join("checkouts.json"));
    let store = dir.path().join("one.hs2");
    FsStore::init_git(&store, &StoreMetadata::new("ONE")).unwrap();
    let register = |name: &str, sources: Vec<TicketSource>| {
        let root = dir.path().join(name);
        std::fs::create_dir(&root).unwrap();
        checkouts
            .register_registered_sources(&providers, &root, Some(name), None, sources, None)
            .unwrap()
    };
    let one = register(
        "one",
        vec![
            TicketSource::git(&store),
            github("github-main"),
            github("github-docs"),
        ],
    );
    let two = register("two", vec![github("github-docs")]);
    Fixture {
        dir,
        providers,
        checkouts,
        one,
        two,
        store: store.canonicalize().unwrap(),
    }
}

#[test]
fn removing_a_checkout_keeps_shared_sources_stores_and_credentials() {
    let f = fixture();
    let report = remove_checkout(&f.providers, &f.checkouts, &f.one.id, false).unwrap();

    assert!(report.registered && report.removed && !report.dry_run);
    assert_eq!(report.checkout_id.as_deref(), Some(f.one.id.as_str()));
    assert_eq!(report.alias.as_deref(), Some("one"));
    assert_eq!(report.removed_connections, vec!["github-main".to_owned()]);
    assert_eq!(
        report.shared_connections.get("github-docs"),
        Some(&vec![f.two.id.clone()])
    );
    assert_eq!(report.kept_credentials, vec!["github-app-01ABC".to_owned()]);
    assert_eq!(
        report.kept_git_stores,
        vec![f.store.to_string_lossy().into_owned()]
    );

    let remaining = f.checkouts.list().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0], f.two, "the other checkout is untouched");
    assert_eq!(f.provider_ids(), vec!["github-docs".to_owned()]);
    // Nothing on disk but the registries changed.
    assert!(f.dir.path().join("one").is_dir(), "checkout directory kept");
    assert!(FsStore::open(&f.store).is_ok(), "ticket store kept");
}

#[test]
fn an_unregistered_reference_is_a_no_op_and_removal_repeats_cleanly() {
    let f = fixture();
    let before = std::fs::read(f.checkouts.path()).unwrap();
    let absent = remove_checkout(&f.providers, &f.checkouts, "no-such-project", false).unwrap();
    assert!(!absent.registered && !absent.removed);
    assert_eq!(absent.checkout_id, None);
    assert_eq!(std::fs::read(f.checkouts.path()).unwrap(), before);

    remove_checkout(&f.providers, &f.checkouts, &f.one.id, false).unwrap();
    let again = remove_checkout(&f.providers, &f.checkouts, &f.one.id, false).unwrap();
    assert!(!again.registered && !again.removed);
    assert_eq!(f.provider_ids(), vec!["github-docs".to_owned()]);

    // Empty then refill: the same root registers again under the same readable id.
    let back = f
        .checkouts
        .register(&f.dir.path().join("one"), None, None, Vec::new())
        .unwrap();
    assert_eq!(back.id, f.one.id);
}

#[test]
fn a_dry_run_reports_the_plan_without_writing() {
    let f = fixture();
    let registry = std::fs::read(f.checkouts.path()).unwrap();
    let providers = std::fs::read(f.dir.path().join("providers.json")).unwrap();
    let report = remove_checkout(&f.providers, &f.checkouts, "one", true).unwrap();
    assert!(report.registered && report.dry_run && !report.removed);
    assert_eq!(report.removed_connections, vec!["github-main".to_owned()]);
    assert!(report.shared_connections.contains_key("github-docs"));
    assert_eq!(std::fs::read(f.checkouts.path()).unwrap(), registry);
    assert_eq!(
        std::fs::read(f.dir.path().join("providers.json")).unwrap(),
        providers
    );
}

#[test]
fn removal_accepts_id_alias_and_path_but_never_an_id_prefix() {
    let f = fixture();
    let prefix = &f.one.id[..f.one.id.len() - 2];
    let report = remove_checkout(&f.providers, &f.checkouts, prefix, false).unwrap();
    assert!(!report.registered, "an id prefix must not remove anything");
    assert_eq!(f.checkouts.list().unwrap().len(), 2);

    let by_path = remove_checkout(
        &f.providers,
        &f.checkouts,
        f.dir.path().join("two").to_str().unwrap(),
        false,
    )
    .unwrap();
    assert_eq!(by_path.checkout_id.as_deref(), Some(f.two.id.as_str()));
    let by_alias = remove_checkout(&f.providers, &f.checkouts, "one", false).unwrap();
    assert_eq!(by_alias.checkout_id.as_deref(), Some(f.one.id.as_str()));
    assert!(f.checkouts.list().unwrap().is_empty());
    // With no project left, every external connection was collected.
    assert!(f.provider_ids().is_empty());
}

#[test]
fn a_checkout_whose_directory_is_gone_is_removed_by_its_old_path() {
    let f = fixture();
    let root = f.dir.path().join("two");
    std::fs::remove_dir(&root).unwrap();
    let report =
        remove_checkout(&f.providers, &f.checkouts, root.to_str().unwrap(), false).unwrap();
    assert_eq!(report.checkout_id.as_deref(), Some(f.two.id.as_str()));
    assert!(report.removed);
    assert!(
        report.shared_connections.contains_key("github-docs"),
        "one still links github-docs"
    );
    assert_eq!(f.provider_ids().len(), 2);
}

#[test]
fn a_shared_alias_is_refused_as_ambiguous() {
    let f = fixture();
    let third = f.dir.path().join("nested");
    std::fs::create_dir_all(third.join("one")).unwrap();
    f.checkouts
        .register(&third.join("one"), None, None, Vec::new())
        .unwrap();
    let error = remove_checkout(&f.providers, &f.checkouts, "one", false).unwrap_err();
    assert!(
        matches!(
            error,
            ConnectionRemovalError::Checkout(CheckoutError::Ambiguous(ref message))
                if message.contains(&f.one.id)
        ),
        "{error}"
    );
    assert_eq!(f.checkouts.list().unwrap().len(), 3);
}

#[test]
fn retained_source_aliases_leave_with_their_checkout() {
    let f = fixture();
    let mut raw = f.registry_json();
    raw["sourceAliases"] = serde_json::json!({
        f.one.id.clone(): {"github-old": "github-main"},
        f.two.id.clone(): {"github-older": "github-docs"},
    });
    std::fs::write(f.checkouts.path(), raw.to_string()).unwrap();
    remove_checkout(&f.providers, &f.checkouts, &f.one.id, false).unwrap();
    let aliases = &f.registry_json()["sourceAliases"];
    assert!(aliases.get(&f.one.id).is_none(), "{aliases}");
    assert_eq!(aliases[&f.two.id]["github-older"], "github-docs");
}

#[test]
fn a_git_only_checkout_never_touches_provider_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let checkouts = CheckoutRegistry::new(dir.path().join("checkouts.json"));
    let root = dir.path().join("app");
    std::fs::create_dir(&root).unwrap();
    let store = dir.path().join("app.hs2");
    FsStore::init_git(&store, &StoreMetadata::new("AP")).unwrap();
    let entry = checkouts
        .register(&root, None, None, vec![store.clone()])
        .unwrap();
    // A provider registry in a directory that does not exist would fail any lock or write.
    let providers =
        ProviderConfigRegistry::new(Path::new(dir.path()).join("missing/providers.json"));
    let report = remove_checkout(&providers, &checkouts, &entry.id, false).unwrap();
    assert!(report.removed);
    assert!(report.removed_connections.is_empty());
    assert!(!dir.path().join("missing").exists());
    assert!(store.join("hotsheet-store.json").is_file());
}

#[test]
fn an_empty_reference_is_rejected() {
    let f = fixture();
    assert!(matches!(
        remove_checkout(&f.providers, &f.checkouts, "  ", false),
        Err(ConnectionRemovalError::Checkout(CheckoutError::Invalid(_)))
    ));
}
