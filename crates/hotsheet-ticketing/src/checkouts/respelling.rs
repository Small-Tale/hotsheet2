//! HS2-CTV942: rewrite checkout spellings stored before HS2-ECJ188, keeping every id.
//!
//! Before the shared canonicalization, a Windows registry stored checkout roots and Git
//! store locators in the verbatim long form (`\\?\C:\Users\...`) or the 8.3 short form
//! (`C:\Users\RUNNER~1\...`). Every new registration and lookup now uses the one
//! ordinary spelling, so an old entry would no longer match its own directory and a
//! re-registration would add a duplicate.
//!
//! The maintainer's decision on HS2-CTV942 is **"Rewrite paths, keep ids"**: a stored
//! pre-fix spelling of a directory that still exists is replaced by its canonical
//! spelling, and the checkout id, the Git source connection id, and everything keyed
//! by them (default source, source colors, store identities, permission vault) stay as
//! they are. A spelling whose path no longer exists is left untouched, and a rewrite
//! that would collide with another entry's root (or another source's locator in the
//! same checkout) is skipped, so the rewrite never creates a duplicate.
//!
//! The rule is a pure function of the stored strings and the file system, so running
//! it again changes nothing. Locked registry reads persist it (see
//! `CheckoutRegistry::persist_respelling_locked`); unlocked reads apply it in memory.

use std::collections::BTreeSet;
use std::path::Path;

use super::Checkout;

/// A spelling Hot Sheet wrote before HS2-ECJ188: Windows verbatim (`\\?\`) or one with an
/// 8.3 short-name component (`NAME~1`). Only these are ever rewritten, so a registry
/// written after the fix is never touched and reading it costs no file-system call.
pub(super) fn is_pre_fix_spelling(spelling: &str) -> bool {
    spelling.starts_with(r"\\?\") || spelling.split(['\\', '/']).any(short_name_component)
}

/// `NAME~N` with optional extension: the shape Windows gives an 8.3 alias.
fn short_name_component(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component);
    stem.rsplit_once('~').is_some_and(|(name, digits)| {
        !name.is_empty() && !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
    })
}

/// The canonical spelling of a stored pre-fix spelling, or `None` when the spelling is
/// already canonical, is not a pre-fix spelling, or no longer names an existing path.
pub(super) fn respelled(spelling: &str) -> Option<String> {
    if !is_pre_fix_spelling(spelling) {
        return None;
    }
    let canonical = hotsheet_sync::paths::canonicalize(Path::new(spelling)).ok()?;
    let canonical = canonical.to_string_lossy().into_owned();
    (canonical != spelling).then_some(canonical)
}

/// Rewrite every pre-fix root, store locator, and Git source locator in `checkouts` to
/// its canonical spelling, keeping all ids. Returns whether anything changed.
pub(super) fn respell_pre_fix_spellings(checkouts: &mut [Checkout]) -> bool {
    let mut changed = false;
    let mut roots: BTreeSet<String> = checkouts.iter().map(|row| row.root.clone()).collect();
    for checkout in checkouts.iter_mut() {
        if let Some(root) = respelled(&checkout.root)
            && !roots.contains(&root)
        {
            roots.remove(&checkout.root);
            roots.insert(root.clone());
            checkout.root = root;
            changed = true;
        }
        changed |= respell_locators(checkout);
    }
    changed
}

fn respell_locators(checkout: &mut Checkout) -> bool {
    let mut changed = false;
    let mut locators: BTreeSet<String> = checkout
        .sources
        .iter()
        .filter(|source| source.provider == "git")
        .map(|source| source.locator.clone())
        .collect();
    for source in checkout
        .sources
        .iter_mut()
        .filter(|source| source.provider == "git")
    {
        if let Some(locator) = respelled(&source.locator)
            && !locators.contains(&locator)
        {
            locators.remove(&source.locator);
            locators.insert(locator.clone());
            source.locator = locator;
            changed = true;
        }
    }
    let mut stores: BTreeSet<String> = checkout.stores.iter().cloned().collect();
    for store in &mut checkout.stores {
        if let Some(locator) = respelled(store)
            && !stores.contains(&locator)
        {
            stores.remove(store.as_str());
            stores.insert(locator.clone());
            *store = locator;
            changed = true;
        }
    }
    if changed {
        checkout.stores.sort();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_ctv942_pre_fix_spellings_are_verbatim_or_short_names() {
        for spelling in [
            r"\\?\C:\Users\runneradmin\app",
            r"\\?\UNC\server\share\app",
            r"C:\Users\RUNNER~1\AppData\Local\Temp\app",
            r"C:\PROGRA~2\app",
            r"C:\Users\LONGNA~12.TXT\app",
            "/tmp/RUNNER~1/app",
        ] {
            assert!(is_pre_fix_spelling(spelling), "{spelling}");
        }
        for spelling in [
            r"C:\Users\runneradmin\app",
            "/home/me/app~",
            "/home/me/~/app",
            "/home/me/~1/app",
            "/home/me/app~beta",
            "",
        ] {
            assert!(!is_pre_fix_spelling(spelling), "{spelling}");
        }
    }

    #[test]
    fn hs2_ctv942_a_canonical_or_missing_spelling_is_not_respelled() {
        let temp = tempfile::tempdir().unwrap();
        let canonical = hotsheet_sync::paths::canonicalize(temp.path()).unwrap();
        assert_eq!(respelled(&canonical.to_string_lossy()), None);
        let missing = canonical.join("GONE~1").join("app");
        assert_eq!(respelled(&missing.to_string_lossy()), None);
    }
}
