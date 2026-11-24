//! Which directory a store's tickets watcher holds open (HS2-VN96J5).
//!
//! A native recursive watch on `<store>/tickets` keeps a directory handle open beneath the
//! store root. NTFS refuses to rename a directory while a handle is open anywhere beneath
//! it, so on Windows a hosted store could not be moved (Explorer reported the folder in
//! use; `std::fs::rename` failed with os error 5). A handle on the store root itself does
//! not block the rename, as measured on Windows 11 ARM64. On Windows the watcher therefore
//! watches the store root and admits only events under `tickets/`; on macOS and Linux it
//! keeps watching `tickets/`. Linux also guards against old-path events when the store moves.
//!
//! A Windows directory handle and a Linux inotify watch follow the directory when it moves,
//! and notify keeps spelling the events with the path it was given. While the root is missing from
//! that path (moved away), its events describe files somewhere else, so they are dropped:
//! applying them would delete every touched ticket from the index. The next event admitted
//! once the store is back carries a rescan flag, so the watch loop re-reads every ticket
//! file and picks up what changed while the store was away.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// The directory a store's tickets watcher holds open and the events it admits.
#[derive(Debug)]
pub(crate) struct TicketsWatchScope {
    root: PathBuf,
    tickets: PathBuf,
    /// `tickets` in every spelling an event may use (macOS reports `/private/var`).
    ticket_spellings: Vec<PathBuf>,
    watch_root: bool,
    guard_missing_root: bool,
    /// Events were dropped while the store root was missing.
    missed: AtomicBool,
    /// How many events the guarded scope delivered (the idle-rate tests read it).
    delivered: AtomicUsize,
}

impl TicketsWatchScope {
    /// The platform's scope: the store root on Windows, `tickets/` elsewhere.
    pub(crate) fn for_store(root: &Path) -> Self {
        Self::new(root, cfg!(windows))
    }

    /// A scope that watches the store root (`watch_root`) or its `tickets/` directory.
    /// Tests use it to exercise the Windows scope on every platform.
    pub(crate) fn new(root: &Path, watch_root: bool) -> Self {
        Self::new_with_guard(root, watch_root, watch_root || cfg!(target_os = "linux"))
    }

    /// Tests force Linux's tickets-scope guard on every platform.
    fn new_with_guard(root: &Path, watch_root: bool, guard_missing_root: bool) -> Self {
        let tickets = root.join("tickets");
        let mut ticket_spellings = vec![tickets.clone()];
        if let Ok(canonical) = hotsheet_sync::paths::canonicalize(root) {
            let canonical = canonical.join("tickets");
            if canonical != tickets {
                ticket_spellings.push(canonical);
            }
        }
        Self {
            root: root.to_path_buf(),
            tickets,
            ticket_spellings,
            watch_root,
            guard_missing_root,
            missed: AtomicBool::new(false),
            delivered: AtomicUsize::new(0),
        }
    }

    /// The directory the native watcher watches recursively.
    pub(crate) fn watch_dir(&self) -> &Path {
        if self.watch_root {
            &self.root
        } else {
            &self.tickets
        }
    }

    fn is_ticket_path(&self, path: &Path) -> bool {
        self.ticket_spellings
            .iter()
            .any(|tickets| path.starts_with(tickets))
    }

    /// Narrow one native event's `paths` to the store's ticket paths. `None` drops the
    /// event; `Some(rescan)` delivers it, asking for a full rescan when events were dropped
    /// while the store was away. Linux's tickets scope keeps every path when present,
    /// but applies the same missing-root guard.
    pub(crate) fn admit_paths(&self, paths: &mut Vec<PathBuf>) -> Option<bool> {
        if !self.guard_missing_root {
            return Some(false);
        }
        if self.watch_root {
            paths.retain(|path| self.is_ticket_path(path));
            if paths.is_empty() {
                return None;
            }
        }
        if !self.root.is_dir() {
            // The store moved away: these paths name files that are no longer here.
            self.missed.store(true, Ordering::SeqCst);
            return None;
        }
        self.delivered.fetch_add(1, Ordering::SeqCst);
        Some(self.missed.swap(false, Ordering::SeqCst))
    }

    /// How many events the guarded scope has delivered so far.
    #[cfg(test)]
    pub(crate) fn delivered(&self) -> usize {
        self.delivered.load(Ordering::SeqCst)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn missed(&self) -> bool {
        self.missed.load(Ordering::SeqCst)
    }

    /// [`Self::admit_paths`] for the store watcher's events. Watcher errors and rescan
    /// requests always pass.
    pub(crate) fn admit(
        &self,
        result: notify::Result<notify::Event>,
    ) -> Option<notify::Result<notify::Event>> {
        let mut event = match result {
            Ok(event) if !event.need_rescan() => event,
            other => return Some(other),
        };
        let rescan = self.admit_paths(&mut event.paths)?;
        if rescan {
            event = event.set_flag(notify::event::Flag::Rescan);
        }
        Some(Ok(event))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use notify::{Event, EventKind};

    fn event(paths: &[&Path]) -> notify::Result<Event> {
        let mut event = Event::new(EventKind::Any);
        for path in paths {
            event = event.add_path(path.to_path_buf());
        }
        Ok(event)
    }

    fn paths(result: Option<notify::Result<Event>>) -> Vec<PathBuf> {
        result.unwrap().unwrap().paths
    }

    #[test]
    fn hs2_vn96j5_the_platform_scope_watches_the_root_only_on_windows() {
        let dir = tempfile::tempdir().unwrap();
        let scope = TicketsWatchScope::for_store(dir.path());
        let expected = if cfg!(windows) {
            dir.path().to_path_buf()
        } else {
            dir.path().join("tickets")
        };
        assert_eq!(scope.watch_dir(), expected);
    }

    #[test]
    fn hs2_vn96j5_the_unguarded_tickets_scope_admits_every_event_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let scope = TicketsWatchScope::new_with_guard(dir.path(), false, false);
        let outside = dir.path().join(".git/index");
        assert_eq!(paths(scope.admit(event(&[&outside]))), [outside]);
        // macOS retains this behavior; Linux applies the missing-root guard (HS2-C3REQ1).
        let gone = TicketsWatchScope::new_with_guard(&dir.path().join("gone"), false, false);
        let ticket = dir.path().join("gone/tickets/AB/x.md");
        assert_eq!(paths(gone.admit(event(&[&ticket]))), [ticket]);
    }

    #[test]
    fn hs2_c3req1_linux_tickets_scope_drops_old_path_events_and_rescans_on_return() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let away = dir.path().join("away");
        std::fs::create_dir_all(root.join("tickets/AB")).unwrap();
        let scope = TicketsWatchScope::new_with_guard(&root, false, true);
        assert_eq!(scope.watch_dir(), root.join("tickets"));
        let ticket = root.join("tickets/AB/01ABC.md");
        let initial = scope.admit(event(&[&ticket])).unwrap().unwrap();
        assert!(!initial.need_rescan());
        for _round in 0..2 {
            std::fs::rename(&root, &away).unwrap();
            assert!(scope.admit(event(&[&ticket])).is_none());
            assert!(scope.admit(event(&[&ticket])).is_none());
            std::fs::rename(&away, &root).unwrap();
            let first = scope.admit(event(&[&ticket])).unwrap().unwrap();
            assert!(
                first.need_rescan(),
                "the first event back rescans the store"
            );
            assert_eq!(first.paths, std::slice::from_ref(&ticket));
            let next = scope.admit(event(&[&ticket])).unwrap().unwrap();
            assert!(!next.need_rescan(), "the rescan is requested only once");
        }
    }

    #[test]
    fn hs2_vn96j5_the_root_scope_admits_only_ticket_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        std::fs::create_dir_all(root.join("tickets")).unwrap();
        let scope = TicketsWatchScope::new(&root, true);
        assert_eq!(scope.watch_dir(), root);
        let ticket = root.join("tickets/AB/01ABC.md");
        let shard = root.join("tickets/AB");
        let git = root.join(".git/objects/ab/cdef");
        let metadata = root.join("hotsheet-store.json");
        // Git churn and store metadata never reach the watch loop.
        assert!(scope.admit(event(&[&git])).is_none());
        assert!(scope.admit(event(&[&metadata])).is_none());
        assert!(scope.admit(event(&[&root])).is_none());
        // A mixed event keeps only its ticket paths; the tickets dir itself counts.
        assert_eq!(
            paths(scope.admit(event(&[&git, &ticket, &metadata]))),
            std::slice::from_ref(&ticket)
        );
        assert_eq!(paths(scope.admit(event(&[&shard]))), [shard]);
        let tickets = root.join("tickets");
        assert_eq!(paths(scope.admit(event(&[&tickets]))), [tickets]);
        // A sibling whose name merely starts with "tickets" is not under it.
        assert!(
            scope
                .admit(event(&[&root.join("tickets-old/AB/x.md")]))
                .is_none()
        );
        // The canonical spelling (macOS /private/var) is admitted too.
        let canonical = hotsheet_sync::paths::canonicalize(&root)
            .unwrap()
            .join("tickets/AB/01ABC.md");
        assert_eq!(paths(scope.admit(event(&[&canonical]))), [canonical]);
    }

    #[test]
    fn hs2_vn96j5_the_root_scope_passes_errors_and_rescans() {
        let dir = tempfile::tempdir().unwrap();
        let scope = TicketsWatchScope::new(dir.path(), true);
        assert!(matches!(
            scope.admit(Err(notify::Error::generic("lost"))),
            Some(Err(_))
        ));
        let rescan = Event::new(EventKind::Other).set_flag(notify::event::Flag::Rescan);
        assert!(scope.admit(Ok(rescan)).unwrap().unwrap().need_rescan());
    }

    /// The store moved away, events kept arriving under the old path, and it came back:
    /// nothing is applied while it is gone, and the first event after it returns asks for
    /// a rescan once. Repeated moves re-arm the rescan each time.
    #[test]
    fn hs2_vn96j5_events_while_the_store_is_away_are_dropped_then_rescanned() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("store");
        let away = dir.path().join("away");
        std::fs::create_dir_all(root.join("tickets/AB")).unwrap();
        let scope = TicketsWatchScope::new(&root, true);
        let ticket = root.join("tickets/AB/01ABC.md");
        let admitted = scope.admit(event(&[&ticket])).unwrap().unwrap();
        assert!(!admitted.need_rescan(), "a present store needs no rescan");
        for _round in 0..2 {
            std::fs::rename(&root, &away).unwrap();
            assert!(scope.admit(event(&[&ticket])).is_none());
            assert!(scope.admit(event(&[&ticket])).is_none());
            // Non-ticket churn while away does not arm anything on its own.
            assert!(scope.admit(event(&[&root.join(".git/x")])).is_none());
            std::fs::rename(&away, &root).unwrap();
            let first = scope.admit(event(&[&ticket])).unwrap().unwrap();
            assert!(
                first.need_rescan(),
                "the first event back asks for a rescan"
            );
            assert_eq!(first.paths, std::slice::from_ref(&ticket));
            let next = scope.admit(event(&[&ticket])).unwrap().unwrap();
            assert!(!next.need_rescan(), "the rescan is asked for once");
        }
    }
}
