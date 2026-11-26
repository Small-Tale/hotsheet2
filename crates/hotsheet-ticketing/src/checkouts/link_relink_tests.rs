//! HS2-FVWKJC: a changed `.hotsheet2/store` link replaces only the source the link
//! previously named. Setup refresh (`Discovered`) and the server's selected-store open
//! (`SelectedGitStore`) share this one rule, so neither can defeat the other.

use super::*;

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    original: PathBuf,
    replacement: PathBuf,
    extra: PathBuf,
    registry: CheckoutRegistry,
}

fn fixture() -> Fixture {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("app");
    std::fs::create_dir(&root).unwrap();
    let store = |name: &str| {
        let path = temp.path().join(name);
        FsStore::init_git(&path, &crate::store::StoreMetadata::new("HS")).unwrap();
        hotsheet_sync::paths::canonicalize(&path).unwrap()
    };
    let original = store("custom.hs2");
    let replacement = store("replacement.hs2");
    let extra = store("extra.hs2");
    let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
    Fixture {
        root: hotsheet_sync::paths::canonicalize(&root).unwrap(),
        _temp: temp,
        original,
        replacement,
        extra,
        registry,
    }
}

impl Fixture {
    fn link(&self, store: &Path) {
        std::fs::create_dir_all(self.root.join(".hotsheet2")).unwrap();
        std::fs::write(
            self.root.join(".hotsheet2/store"),
            format!("{}\n", store.display()),
        )
        .unwrap();
    }

    fn open(&self, stores: &[&Path], mode: OpenSourceMode) -> Checkout {
        self.registry
            .open_sources(
                &self.root,
                None,
                None,
                stores.iter().map(TicketSource::git).collect(),
                None,
                mode,
            )
            .unwrap()
    }

    /// What `hotsheet-cli setup --refresh` does: link the store it was given, conservatively.
    fn setup_refresh(&self, store: &Path) -> Checkout {
        self.open(&[store], OpenSourceMode::Discovered)
    }

    /// What the client's open does after setup: select the linked store.
    fn server_open(&self, store: &Path) -> Checkout {
        self.open(&[store], OpenSourceMode::SelectedGitStore)
    }

    fn registry_bytes(&self) -> Vec<u8> {
        std::fs::read(self.registry.path()).unwrap()
    }
}

fn stores(checkout: &Checkout) -> Vec<PathBuf> {
    let mut stores = checkout
        .sources
        .iter()
        .filter(|source| source.provider == "git")
        .map(|source| PathBuf::from(&source.locator))
        .collect::<Vec<_>>();
    stores.sort();
    stores
}

fn sorted(paths: &[&Path]) -> Vec<PathBuf> {
    let mut paths = paths
        .iter()
        .map(|path| path.to_path_buf())
        .collect::<Vec<_>>();
    paths.sort();
    paths
}

fn default_store(checkout: &Checkout) -> Option<PathBuf> {
    let id = checkout.default_source.as_deref()?;
    checkout
        .source(id)
        .map(|source| PathBuf::from(&source.locator))
}

#[test]
fn hs2_fvwkjc_relink_then_reopen_replaces_the_link_derived_store() {
    let f = fixture();
    f.link(&f.original);
    f.setup_refresh(&f.original);
    assert_eq!(stores(&f.server_open(&f.original)), sorted(&[&f.original]));

    f.link(&f.replacement);
    // Setup refresh runs first and must not leave the old link-derived store behind.
    let refreshed = f.setup_refresh(&f.replacement);
    assert_eq!(stores(&refreshed), sorted(&[&f.replacement]));
    assert_eq!(default_store(&refreshed), Some(f.replacement.clone()));
    let opened = f.server_open(&f.replacement);
    assert_eq!(stores(&opened), sorted(&[&f.replacement]));
    assert_eq!(default_store(&opened), Some(f.replacement.clone()));
}

#[test]
fn hs2_fvwkjc_relink_back_to_the_original_store_restores_it() {
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.link(&f.replacement);
    f.setup_refresh(&f.replacement);
    f.server_open(&f.replacement);

    f.link(&f.original);
    let back = f.setup_refresh(&f.original);
    assert_eq!(stores(&back), sorted(&[&f.original]));
    assert_eq!(default_store(&back), Some(f.original.clone()));
    assert_eq!(stores(&f.server_open(&f.original)), sorted(&[&f.original]));
}

#[test]
fn hs2_fvwkjc_explicit_extra_store_survives_a_relink() {
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.registry
        .add_source(f.root.to_str().unwrap(), TicketSource::git(&f.extra), false)
        .unwrap();

    f.link(&f.replacement);
    let refreshed = f.setup_refresh(&f.replacement);
    assert_eq!(stores(&refreshed), sorted(&[&f.replacement, &f.extra]));
    assert_eq!(default_store(&refreshed), Some(f.replacement.clone()));
    assert_eq!(
        stores(&f.server_open(&f.replacement)),
        sorted(&[&f.replacement, &f.extra])
    );
    // Even a discovery-only open with no requested store applies the same rule.
    f.link(&f.original);
    assert_eq!(
        stores(&f.open(&[], OpenSourceMode::Discovered)),
        sorted(&[&f.original, &f.extra])
    );
}

#[test]
fn hs2_fvwkjc_missing_empty_or_dangling_links_never_change_sources() {
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.registry
        .add_source(f.root.to_str().unwrap(), TicketSource::git(&f.extra), false)
        .unwrap();
    let expected = sorted(&[&f.original, &f.extra]);

    std::fs::remove_file(f.root.join(".hotsheet2/store")).unwrap();
    assert_eq!(stores(&f.open(&[], OpenSourceMode::Discovered)), expected);
    std::fs::write(f.root.join(".hotsheet2/store"), "\n").unwrap();
    assert_eq!(stores(&f.open(&[], OpenSourceMode::Discovered)), expected);
    f.link(&f.root.join("no-such-store.hs2"));
    assert_eq!(stores(&f.open(&[], OpenSourceMode::Discovered)), expected);

    // The recorded link survives the gap: relinking afterwards still replaces only it.
    f.link(&f.replacement);
    assert_eq!(
        stores(&f.open(&[], OpenSourceMode::Discovered)),
        sorted(&[&f.replacement, &f.extra])
    );
}

#[test]
fn hs2_fvwkjc_repeated_refreshes_are_idempotent() {
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.link(&f.replacement);
    f.setup_refresh(&f.replacement);
    f.server_open(&f.replacement);
    let settled = f.registry_bytes();
    for _ in 0..3 {
        f.setup_refresh(&f.replacement);
        f.server_open(&f.replacement);
        f.open(&[], OpenSourceMode::Discovered);
        assert_eq!(f.registry_bytes(), settled);
    }
}

#[test]
fn hs2_fvwkjc_legacy_registry_relinks_only_a_sole_git_source() {
    // Sole Git source, no recorded link: it is the link-derived store.
    let f = fixture();
    f.server_open(&f.original);
    f.link(&f.replacement);
    assert_eq!(
        stores(&f.setup_refresh(&f.replacement)),
        sorted(&[&f.replacement])
    );

    // Several Git sources and no record: nothing is guessed or removed.
    let g = fixture();
    g.server_open(&g.original);
    g.registry
        .add_source(g.root.to_str().unwrap(), TicketSource::git(&g.extra), false)
        .unwrap();
    g.link(&g.replacement);
    assert_eq!(
        stores(&g.setup_refresh(&g.replacement)),
        sorted(&[&g.original, &g.extra, &g.replacement])
    );
    // That open recorded the link, so a later relink replaces exactly that store.
    g.link(&g.original);
    assert_eq!(
        stores(&g.setup_refresh(&g.original)),
        sorted(&[&g.original, &g.extra])
    );
}

#[test]
fn hs2_fvwkjc_explicit_source_sets_are_not_rewritten_by_the_link() {
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.link(&f.replacement);
    let explicit = f.open(&[&f.original, &f.extra], OpenSourceMode::Explicit);
    assert_eq!(stores(&explicit), sorted(&[&f.original, &f.extra]));
}

#[test]
fn hs2_fvwkjc_concurrent_opens_converge_on_the_relinked_store() {
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.registry
        .add_source(f.root.to_str().unwrap(), TicketSource::git(&f.extra), false)
        .unwrap();
    f.link(&f.replacement);
    let expected = sorted(&[&f.replacement, &f.extra]);
    std::thread::scope(|scope| {
        let handles = (0..6)
            .map(|index| {
                let f = &f;
                scope.spawn(move || {
                    if index % 2 == 0 {
                        f.setup_refresh(&f.replacement)
                    } else {
                        f.server_open(&f.replacement)
                    }
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            assert_eq!(stores(&handle.join().unwrap()), expected);
        }
    });
    let checkout = f.registry.resolve(f.root.to_str().unwrap()).unwrap();
    assert_eq!(stores(&checkout), expected);
    assert_eq!(default_store(&checkout), Some(f.replacement.clone()));
}

#[test]
fn hs2_fvwkjc_a_selected_store_open_first_after_relink_keeps_the_extra_store() {
    // Without setup refresh first, the selected-store rule must not mistake the remaining
    // explicit extra store for the sole previous Git store and replace it.
    let f = fixture();
    f.link(&f.original);
    f.server_open(&f.original);
    f.registry
        .add_source(f.root.to_str().unwrap(), TicketSource::git(&f.extra), false)
        .unwrap();
    f.link(&f.replacement);
    let opened = f.server_open(&f.replacement);
    assert_eq!(stores(&opened), sorted(&[&f.replacement, &f.extra]));
    assert_eq!(default_store(&opened), Some(f.replacement.clone()));
}
