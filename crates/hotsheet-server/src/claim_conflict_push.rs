//! Telling a session at once that its ticket claim was lost to another machine (HS2-1639Q0,
//! `docs/36-resource-claims.md` §36.14).
//!
//! `hotsheet_ticketing::claim_gate::reconcile` runs after every sync pass that pulls and records
//! each claim this host held that another machine's claim now overrides. Nothing read that record
//! until the losing session's next renew was refused. After each sync pass the server now reads
//! the pending conflicts and pushes each one into the live terminal whose claim worker id is the
//! loser, through the same session-push path as every other server-initiated prompt. A conflict
//! whose session is not reachable stays pending and is tried again after the next pass; one that
//! was delivered is acknowledged, so a session is told once.

use std::path::Path;

use hotsheet_aitools::session_push::PushMessage;
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::claim_gate::{ClaimConflict, acknowledge_conflicts, pending_conflicts};

use crate::AppState;
use crate::routes::{feedback_session, session_messages, terminals};

/// How conflicts reach sessions. The server's reach is [`ServerReach`]; tests supply a fake.
pub trait SessionReach {
    /// Live push-capable terminals of this project as `(terminal id, claim worker id)`.
    fn sessions(&self) -> Vec<(String, String)>;
    /// Push `prompt` into one terminal.
    fn push(&self, terminal: &str, prompt: &str) -> Result<(), String>;
}

/// The words a losing session is told. The holder is the readable label (or the unreadable
/// marker), never a worker id or a key.
pub fn message(conflict: &ClaimConflict) -> String {
    format!(
        "Hot Sheet: your claim on {slug} was lost. Another machine holds it now ({holder}); this \
         was detected after a sync. Stop work on {slug} now, set down anything you started for \
         it, and take other work. `hotsheet-cli show {slug}` names the current holder.",
        slug = conflict.slug,
        holder = conflict.holder,
    )
}

/// Push each pending conflict of `store` to its losing session and acknowledge the ones
/// delivered. Returns how many sessions were told.
pub fn deliver(store: &FsStore, reach: &dyn SessionReach) -> usize {
    let pending = pending_conflicts(store);
    if pending.is_empty() {
        return 0;
    }
    let sessions = reach.sessions();
    let mut told = Vec::new();
    for conflict in &pending {
        let Some((terminal, _)) = sessions
            .iter()
            .find(|(_, worker)| *worker == conflict.worker)
        else {
            continue;
        };
        if reach.push(terminal, &message(conflict)).is_ok() {
            told.push(conflict.ticket);
        }
    }
    acknowledge_conflicts(store, &told);
    told.len()
}

/// The server's reach: live terminals that report a push-capable AI session.
pub struct ServerReach<'a> {
    pub state: &'a AppState,
    pub project: &'a Path,
    pub workers: std::collections::HashMap<String, String>,
}

impl SessionReach for ServerReach<'_> {
    fn sessions(&self) -> Vec<(String, String)> {
        feedback_session::push_capable_terminals(self.state, None)
            .into_iter()
            .filter(|t| crate::watcher::same_path(Path::new(&t.project), self.project))
            .filter_map(|t| {
                let worker = self.workers.get(&t.id)?.clone();
                Some((t.id, worker))
            })
            .collect()
    }

    fn push(&self, terminal: &str, prompt: &str) -> Result<(), String> {
        let adapter = session_messages::adapter(self.state, terminal)
            .ok_or_else(|| "the AI session is no longer reachable".to_string())?;
        adapter
            .message(&PushMessage::Prompt {
                text: prompt.to_string(),
                banner: None,
            })
            .map(|_| ())
            .map_err(|error| error.to_string())
    }
}

/// After a sync pass: tell the sessions of `store_root`'s project about claims they lost.
pub async fn after_pass(state: &AppState, project: &Path, store_root: &Path) -> usize {
    let Ok(store) = FsStore::open(store_root) else {
        return 0;
    };
    if pending_conflicts(&store).is_empty() {
        return 0;
    }
    let Some(workers) = terminals::terminal_workers(state).await else {
        return 0;
    };
    let reach = ServerReach {
        state,
        project,
        workers,
    };
    deliver(&store, &reach)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use hotsheet_model::{Timestamp, Ulid};
    use hotsheet_ticketing::StoreMetadata;

    use super::*;

    struct Fake {
        sessions: Vec<(String, String)>,
        pushed: RefCell<Vec<(String, String)>>,
        fail: bool,
    }

    impl SessionReach for Fake {
        fn sessions(&self) -> Vec<(String, String)> {
            self.sessions.clone()
        }
        fn push(&self, terminal: &str, prompt: &str) -> Result<(), String> {
            if self.fail {
                return Err("gone".into());
            }
            self.pushed
                .borrow_mut()
                .push((terminal.into(), prompt.into()));
            Ok(())
        }
    }

    /// A Git store with `conflicts` recorded the way `reconcile` records them.
    fn store_with(conflicts: &[ClaimConflict]) -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init_git(dir.path().join("s.hs2"), &StoreMetadata::new("HS")).unwrap();
        let output = hotsheet_ticketing::git::command_in(store.root())
            .args(["rev-parse", "--git-path", "hotsheet-claim-conflicts.json"])
            .output()
            .unwrap();
        let path = std::path::PathBuf::from(String::from_utf8_lossy(&output.stdout).trim());
        let path = if path.is_absolute() {
            path
        } else {
            store.root().join(path)
        };
        std::fs::write(&path, serde_json::to_string(conflicts).unwrap()).unwrap();
        (dir, store)
    }

    fn conflict(slug: &str, worker: &str, holder: &str) -> ClaimConflict {
        ClaimConflict {
            ticket: Ulid::new(),
            slug: slug.into(),
            worker: worker.into(),
            holder: holder.into(),
            detected_at: Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        }
    }

    #[test]
    fn hs2_1639q0_the_losing_session_alone_is_told_once_and_the_others_stay_pending() {
        let lost = conflict("HS-1", "claude-a", "Build agent");
        let unreachable = conflict("HS-2", "claude-gone", "Build agent");
        let (_dir, store) = store_with(&[lost.clone(), unreachable.clone()]);
        let reach = Fake {
            sessions: vec![
                ("t1".into(), "claude-b".into()),
                ("t2".into(), "claude-a".into()),
            ],
            pushed: RefCell::default(),
            fail: false,
        };
        assert_eq!(deliver(&store, &reach), 1);
        let pushed = reach.pushed.borrow().clone();
        assert_eq!(pushed.len(), 1);
        assert_eq!(
            pushed[0].0, "t2",
            "only the terminal whose worker lost the claim"
        );
        assert!(
            pushed[0].1.contains("HS-1") && pushed[0].1.contains("Build agent"),
            "{}",
            pushed[0].1
        );
        assert!(
            !pushed[0].1.contains("claude-a"),
            "the losing worker id is not echoed"
        );
        // Told once: the delivered conflict is acknowledged, the unreachable one waits.
        let left = pending_conflicts(&store);
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].slug, "HS-2");
        assert_eq!(
            deliver(&store, &reach),
            0,
            "nothing new reaches a session twice"
        );
        // When its session appears, the waiting conflict is delivered and cleared.
        let later = Fake {
            sessions: vec![("t9".into(), "claude-gone".into())],
            pushed: RefCell::default(),
            fail: false,
        };
        assert_eq!(deliver(&store, &later), 1);
        assert!(pending_conflicts(&store).is_empty());
    }

    #[test]
    fn hs2_1639q0_a_failed_push_keeps_the_conflict_pending() {
        let (_dir, store) = store_with(&[conflict("HS-1", "claude-a", "Build agent")]);
        let reach = Fake {
            sessions: vec![("t2".into(), "claude-a".into())],
            pushed: RefCell::default(),
            fail: true,
        };
        assert_eq!(deliver(&store, &reach), 0);
        assert_eq!(
            pending_conflicts(&store).len(),
            1,
            "retried after the next pass"
        );
    }

    #[test]
    fn hs2_1639q0_no_pending_conflict_asks_for_no_session_list() {
        let (_dir, store) = store_with(&[]);
        struct Panics;
        impl SessionReach for Panics {
            fn sessions(&self) -> Vec<(String, String)> {
                panic!("a healthy store never lists sessions");
            }
            fn push(&self, _: &str, _: &str) -> Result<(), String> {
                unreachable!()
            }
        }
        assert_eq!(deliver(&store, &Panics), 0);
    }
}
