//! HS2-86ES0B: follow a remembered checkout root to where an explicit relocate moved it.
//!
//! A client remembers open projects by root path. After `hotsheet-cli checkout relocate`
//! (or any other [`CheckoutRegistry::relocate`] caller) moves a checkout, the remembered
//! path no longer exists. The registry keeps every former root it was told about, and a
//! checkout id is derived from the canonical root it was first registered at, so either
//! proves which checkout a vanished root belonged to. Nothing here searches the disk: a
//! folder moved without telling the registry stays unavailable.
use super::*;

/// Remember that `relocated` left `former_root`. Its new root is no longer a former root
/// (a checkout moved back), so that entry goes.
pub(super) fn record(
    relocated_roots: &mut BTreeMap<String, String>,
    former_root: &str,
    relocated: &Checkout,
) {
    relocated_roots.remove(&relocated.root);
    if former_root != relocated.root {
        relocated_roots.insert(former_root.to_owned(), relocated.id.clone());
    }
}

/// The one checkout that a vanished root (any of its `spellings`) moved away from. `None`
/// when a checkout is still registered at that root, when nothing proves a move, or when
/// the evidence names more than one checkout.
pub(super) fn relocated_checkout<'a>(
    checkouts: &'a [Checkout],
    relocated_roots: &BTreeMap<String, String>,
    spellings: &[String],
) -> Option<&'a Checkout> {
    if checkouts
        .iter()
        .any(|checkout| spellings.contains(&checkout.root))
    {
        return None;
    }
    let mut ids = BTreeSet::new();
    for spelling in spellings {
        if let Some(id) = relocated_roots.get(spelling) {
            ids.insert(id.clone());
        }
        // A checkout relocated before its former root was recorded still carries the id
        // its first canonical root produced.
        ids.insert(checkout_id_for_canonical(Path::new(spelling)));
    }
    let mut matches = checkouts
        .iter()
        .filter(|checkout| ids.contains(&checkout.id));
    let one = matches.next()?;
    matches.next().is_none().then_some(one)
}

impl CheckoutRegistry {
    /// HS2-86ES0B: the checkout an explicit relocate moved away from the absolute `root`,
    /// when that root no longer exists and the checkout's current root is a directory.
    /// Read-only. `None` for an existing or relative path, so an open of a real folder is
    /// never redirected.
    pub fn follow_relocation(&self, root: &Path) -> Result<Option<Checkout>, CheckoutError> {
        if !root.is_absolute() || hotsheet_sync::paths::canonicalize(root).is_ok() {
            return Ok(None);
        }
        let file = self.read()?;
        let spellings = removal_path_candidates(root);
        Ok(
            relocated_checkout(&file.checkouts, &file.relocated_roots, &spellings)
                .filter(|checkout| Path::new(&checkout.root).is_dir())
                .cloned(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn checkout(id: &str, root: &str) -> Checkout {
        serde_json::from_value(serde_json::json!({ "id": id, "root": root, "alias": id })).unwrap()
    }

    fn roots(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(root, id)| ((*root).to_owned(), (*id).to_owned()))
            .collect()
    }

    #[test]
    fn hs2_86es0b_record_keeps_every_former_root_and_forgets_a_root_moved_back_to() {
        let mut recorded = BTreeMap::new();
        record(&mut recorded, "/a", &checkout("app", "/b"));
        record(&mut recorded, "/b", &checkout("app", "/c"));
        assert_eq!(recorded, roots(&[("/a", "app"), ("/b", "app")]));
        record(&mut recorded, "/c", &checkout("app", "/a"));
        assert_eq!(recorded, roots(&[("/b", "app"), ("/c", "app")]));
        record(&mut recorded, "/a", &checkout("app", "/a"));
        assert_eq!(
            recorded,
            roots(&[("/b", "app"), ("/c", "app")]),
            "a same-root relocate records nothing"
        );
    }

    #[test]
    fn hs2_86es0b_follows_a_recorded_former_root_or_the_id_its_first_root_produced() {
        let moved = checkout("app", "/new/app");
        let entries = [moved.clone(), checkout("other", "/other")];
        let recorded = roots(&[("/old/app", "app")]);
        assert_eq!(
            relocated_checkout(&entries, &recorded, &["/old/app".into()]),
            Some(&moved)
        );
        let legacy = checkout(
            &checkout_id_for_canonical(Path::new("/first/app")),
            "/new/app",
        );
        assert_eq!(
            relocated_checkout(
                std::slice::from_ref(&legacy),
                &BTreeMap::new(),
                &["/first/app".into()]
            ),
            Some(&legacy),
            "relocated before former roots were recorded"
        );
        assert_eq!(
            relocated_checkout(&entries, &recorded, &["/unknown".into()]),
            None
        );
    }

    #[test]
    fn hs2_86es0b_never_follows_a_registered_root_an_unknown_id_or_ambiguous_evidence() {
        let entries = [checkout("app", "/new/app"), checkout("again", "/old/app")];
        let recorded = roots(&[("/old/app", "app")]);
        assert_eq!(
            relocated_checkout(&entries, &recorded, &["/old/app".into()]),
            None,
            "a checkout registered at the old root again owns it"
        );
        assert_eq!(
            relocated_checkout(
                &[checkout("app", "/new/app")],
                &roots(&[("/old/app", "removed")]),
                &["/old/app".into()]
            ),
            None
        );
        let legacy_id = checkout_id_for_canonical(Path::new("/old/app"));
        assert_eq!(
            relocated_checkout(
                &[
                    checkout("app", "/new/app"),
                    checkout(&legacy_id, "/elsewhere")
                ],
                &recorded,
                &["/old/app".into()]
            ),
            None,
            "two checkouts claim the old root"
        );
    }

    #[test]
    fn hs2_86es0b_registry_follows_a_relocated_checkout_only_while_its_old_root_is_gone() {
        let temp = tempfile::tempdir().unwrap();
        let base = hotsheet_sync::paths::canonicalize(temp.path()).unwrap();
        let (original, relocated) = (base.join("original"), base.join("relocated"));
        std::fs::create_dir(&original).unwrap();
        let registry = CheckoutRegistry::new(base.join("checkouts.json"));
        let before = registry.register(&original, None, None, vec![]).unwrap();
        assert_eq!(registry.follow_relocation(&original).unwrap(), None);

        std::fs::rename(&original, &relocated).unwrap();
        assert_eq!(
            registry.follow_relocation(&original).unwrap(),
            None,
            "a folder moved without telling the registry stays unavailable"
        );
        let after = registry.relocate(&before.id, &relocated, None).unwrap();
        assert_eq!(
            registry.follow_relocation(&original).unwrap(),
            Some(after.clone())
        );
        assert!(
            std::fs::read_to_string(base.join("checkouts.json"))
                .unwrap()
                .contains("relocatedRoots")
        );
        assert_eq!(
            registry.follow_relocation(Path::new("original")).unwrap(),
            None,
            "relative paths are never followed"
        );

        std::fs::create_dir(&original).unwrap();
        assert_eq!(
            registry.follow_relocation(&original).unwrap(),
            None,
            "an existing folder at the old root is opened as itself"
        );
        std::fs::remove_dir(&original).unwrap();

        std::fs::rename(&relocated, base.join("gone")).unwrap();
        assert_eq!(
            registry.follow_relocation(&original).unwrap(),
            None,
            "the current root must exist too"
        );
    }

    #[test]
    fn hs2_86es0b_registry_follows_a_checkout_relocated_before_former_roots_were_recorded() {
        let temp = tempfile::tempdir().unwrap();
        let base = hotsheet_sync::paths::canonicalize(temp.path()).unwrap();
        let (original, relocated) = (base.join("hotsheet2"), base.join("moved"));
        std::fs::create_dir(&relocated).unwrap();
        let id = checkout_id_for_canonical(&original);
        let path = base.join("checkouts.json");
        std::fs::write(
            &path,
            serde_json::json!({
                "schemaVersion": CHECKOUT_REGISTRY_SCHEMA_VERSION,
                "checkouts": [{ "id": id, "root": relocated.to_string_lossy(), "alias": "hotsheet2" }],
            })
            .to_string(),
        )
        .unwrap();
        let followed = CheckoutRegistry::new(path)
            .follow_relocation(&original)
            .unwrap()
            .unwrap();
        assert_eq!(followed.id, id);
        assert_eq!(followed.root, relocated.to_string_lossy());
    }
}
