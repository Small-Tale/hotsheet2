//! Expected workers from the coordination worker pool (HS2-BHPPAS, `docs/32-notifier-plugins.md`
//! §32.8.3). A dispatcher or integrator pool (`hotsheet-cli coordination`) keeps one `state.json`
//! per shipping checkout under `<git-common-dir>/hotsheet-coordination/`, keyed to one ticket source
//! by `source_id` (the SHA-256 of the store's canonical root). Its registered worker sessions are the
//! worker plan: a worker that registered but never claimed a ticket still belongs to the plan, so
//! the live state can read it as missing.
//!
//! The live state is store-scoped and the pool is checkout-scoped, so a store's plan is the union of
//! the unexpired worker sessions in the pools of every registered checkout linked to it whose source
//! is this store. The reader is tolerant: it takes only the fields it needs, and a missing or
//! unreadable pool is no plan. It spawns no process: the common dir is resolved from the checkout's
//! `.git` directory or worktree file.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use sha2::{Digest, Sha256};

/// The pool fields this reader needs; every other member is ignored.
#[derive(Deserialize)]
struct Pool {
    #[serde(default)]
    source_id: String,
    #[serde(default)]
    workers: BTreeMap<String, PoolSession>,
}

#[derive(Deserialize)]
struct PoolSession {
    /// Unix seconds; the session is live while this is in the future.
    expires_at: i64,
}

/// The coordination `source_id` of a store root, as `Coordinator` derives it.
fn source_id(store_root: &Path) -> Option<String> {
    let canonical = store_root.canonicalize().ok()?;
    Some(format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    ))
}

/// The Git common directory of a checkout: `.git` itself, or for a linked worktree the directory
/// its `.git` file names, followed through that directory's `commondir`.
fn common_dir(checkout_root: &Path) -> Option<PathBuf> {
    let dot_git = checkout_root.join(".git");
    if dot_git.is_dir() {
        return Some(dot_git);
    }
    let text = std::fs::read_to_string(&dot_git).ok()?;
    let gitdir = text.strip_prefix("gitdir:")?.trim();
    let gitdir = checkout_root.join(gitdir);
    match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(common) => Some(gitdir.join(common.trim())),
        Err(_) => Some(gitdir),
    }
}

/// The live worker sessions planned for `store_root` across `checkout_roots` at `now_s` (Unix
/// seconds), or `None` when no linked checkout's pool for this store lists one.
pub(crate) fn planned_workers(
    checkout_roots: &[PathBuf],
    store_root: &Path,
    now_s: i64,
) -> Option<BTreeSet<String>> {
    let source = source_id(store_root)?;
    let mut seen = BTreeSet::new();
    let mut workers = BTreeSet::new();
    for root in checkout_roots {
        let Some(common) = common_dir(root) else {
            continue;
        };
        let path = common.join("hotsheet-coordination").join("state.json");
        // Worktrees of one checkout share a pool; read it once.
        let key = path.canonicalize().unwrap_or_else(|_| path.clone());
        if !seen.insert(key) {
            continue;
        }
        let Some(pool) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Pool>(&bytes).ok())
        else {
            continue;
        };
        if pool.source_id != source {
            continue;
        }
        workers.extend(
            pool.workers
                .into_iter()
                .filter(|(_, session)| session.expires_at > now_s)
                .map(|(id, _)| id),
        );
    }
    (!workers.is_empty()).then_some(workers)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pool(common: &Path, source: &str, workers: &[(&str, i64)]) {
        let dir = common.join("hotsheet-coordination");
        std::fs::create_dir_all(&dir).unwrap();
        let workers: serde_json::Map<String, serde_json::Value> = workers
            .iter()
            .map(|(id, expires)| {
                (
                    id.to_string(),
                    serde_json::json!({
                        "id": id,
                        "generation": "g1",
                        "expires_at": expires,
                        "return_route": { "kind": "native_parent", "recipient": "parent" },
                    }),
                )
            })
            .collect();
        std::fs::write(
            dir.join("state.json"),
            serde_json::to_vec(&serde_json::json!({
                "version": 9,
                "project_id": "demo-1234",
                "source_id": source,
                "revision": 3,
                "integrator": null,
                "workers": workers,
                "assignments": {},
                "stopped": null,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    #[test]
    fn hs2_bhppas_plan_is_the_unexpired_worker_sessions_of_pools_for_this_store() {
        let dir = tempfile::tempdir().unwrap();
        let store = dir.path().join("demo.hs2");
        std::fs::create_dir_all(&store).unwrap();
        let other_store = dir.path().join("other.hs2");
        std::fs::create_dir_all(&other_store).unwrap();
        let source = source_id(&store).unwrap();
        let now = 1_000;

        // No checkout, or a checkout without a pool, is no plan.
        assert_eq!(planned_workers(&[], &store, now), None);
        let main = dir.path().join("main");
        std::fs::create_dir_all(main.join(".git")).unwrap();
        assert_eq!(
            planned_workers(std::slice::from_ref(&main), &store, now),
            None
        );

        // A pool for this store: live sessions count, an expired one does not.
        pool(
            &main.join(".git"),
            &source,
            &[("w-live", now + 60), ("w-idle", now + 1), ("w-gone", now)],
        );
        let plan = planned_workers(std::slice::from_ref(&main), &store, now).unwrap();
        assert_eq!(
            plan.into_iter().collect::<Vec<_>>(),
            ["w-idle", "w-live"],
            "registered sessions count even without a claim"
        );
        // The same pool for another store is not this store's plan.
        assert_eq!(
            planned_workers(std::slice::from_ref(&main), &other_store, now),
            None
        );
        // Once every session expired there is no plan.
        assert_eq!(
            planned_workers(std::slice::from_ref(&main), &store, now + 60),
            None
        );

        // A linked worktree's `.git` file resolves to the shared pool, read once; a second
        // checkout's pool for the same store adds its workers.
        let worktree = dir.path().join("worktree");
        std::fs::create_dir_all(&worktree).unwrap();
        let worktree_git = main.join(".git").join("worktrees").join("wt");
        std::fs::create_dir_all(&worktree_git).unwrap();
        std::fs::write(worktree_git.join("commondir"), "../..\n").unwrap();
        std::fs::write(
            worktree.join(".git"),
            format!("gitdir: {}\n", worktree_git.display()),
        )
        .unwrap();
        let second = dir.path().join("second");
        std::fs::create_dir_all(second.join(".git")).unwrap();
        pool(&second.join(".git"), &source, &[("w-second", now + 60)]);
        let plan = planned_workers(&[main.clone(), worktree, second], &store, now).unwrap();
        assert_eq!(
            plan.into_iter().collect::<Vec<_>>(),
            ["w-idle", "w-live", "w-second"]
        );

        // An unreadable pool is no plan rather than an error.
        std::fs::write(
            main.join(".git/hotsheet-coordination/state.json"),
            "{not json",
        )
        .unwrap();
        assert_eq!(planned_workers(&[main], &store, now), None);
    }
}
