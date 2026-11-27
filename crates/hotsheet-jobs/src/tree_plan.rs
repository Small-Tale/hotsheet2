//! HS2-236XY3: which directories a per-directory tree watcher watches, and how it follows
//! the tree as directories appear, vanish and move.
//!
//! Linux inotify has no recursive watch: notify's recursive mode adds one watch per
//! directory, including every directory under ignored build trees (`target/`,
//! `node_modules/`, agent worktrees), which costs kernel watches against the per-user limit
//! and delivers their churn. The Linux tree watcher (`inotify_tree.rs`) instead watches each
//! directory on its own and leaves excluded subtrees out. Everything it decides lives here,
//! free of platform calls, so it is tested on every host:
//! - [`TreePlan::start`] walks the roots, watching each directory before listing it (so a
//!   subdirectory created meanwhile is either listed or reported), pruning excluded
//!   subtrees and never following a symlink;
//! - [`notices`] reads what an event means for the set of watched directories;
//! - [`TreePlan::appeared`] watches a directory that appeared (created, or renamed in) with
//!   everything under it, and [`TreePlan::gone`] forgets a directory that went away
//!   (removed, or renamed out) with every watched directory under it;
//! - after lost events, [`TreePlan::reset`] then [`TreePlan::start`] rebuild the whole set.
//!
//! A watch the platform refuses for a directory that vanished or cannot be read skips that
//! subtree ([`Watched::Skipped`]); exhausting the watch limit stops the walk
//! ([`Watched::Stop`]), and the owner reports it.

use std::collections::BTreeSet;
use std::ops::Bound;
use std::path::{Path, PathBuf};

use crate::watch::event::{CreateKind, ModifyKind, RenameMode};
use crate::watch::{Delivery, Error, ErrorKind, Event, EventKind};

/// What a directory entry is, never following a symlink.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntryKind {
    Directory,
    /// A file, a symlink (even to a directory), or anything else that is not watched.
    Other,
}

/// The file system as the plan sees it.
pub(crate) trait Tree {
    /// What is at `path`, without following a final symlink; `None` when nothing is.
    fn kind(&self, path: &Path) -> Option<EntryKind>;
    /// Whether `root` is a directory, following symlinks: a root is watched as given.
    fn root_is_dir(&self, root: &Path) -> bool;
    /// The entries directly inside `dir`, none followed.
    fn entries(&self, dir: &Path) -> std::io::Result<Vec<(PathBuf, EntryKind)>>;
}

/// The real file system.
pub(crate) struct DiskTree;

impl Tree for DiskTree {
    fn kind(&self, path: &Path) -> Option<EntryKind> {
        std::fs::symlink_metadata(path).ok().map(|metadata| {
            if metadata.is_dir() {
                EntryKind::Directory
            } else {
                EntryKind::Other
            }
        })
    }

    fn root_is_dir(&self, root: &Path) -> bool {
        root.is_dir()
    }

    fn entries(&self, dir: &Path) -> std::io::Result<Vec<(PathBuf, EntryKind)>> {
        let mut entries = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let Ok(entry) = entry else { continue };
            // `file_type` describes the entry itself: a symlink stays a symlink.
            let kind = match entry.file_type() {
                Ok(kind) if kind.is_dir() => EntryKind::Directory,
                _ => EntryKind::Other,
            };
            entries.push((entry.path(), kind));
        }
        Ok(entries)
    }
}

/// How the platform answered a request to watch one directory.
#[derive(Debug)]
pub(crate) enum Watched<E> {
    Yes,
    /// Not watched (it vanished, or cannot be read): skip it and everything under it.
    Skipped,
    /// Stop walking (the watch limit is exhausted, or the watcher is stopping).
    Stop(E),
}

/// How a refused watch request continues: only an exhausted watch limit stops a walk.
pub(crate) fn refusal(error: Error) -> Watched<Error> {
    match error.kind {
        ErrorKind::MaxFilesWatch => Watched::Stop(error),
        _ => Watched::Skipped,
    }
}

/// What a walk added.
#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct Grown {
    /// The directories newly watched, in the order they were watched.
    pub(crate) watched: Vec<PathBuf>,
    /// Every entry inside a newly watched directory. A directory that appears may already
    /// hold entries created before its watch; the owner reports them.
    pub(crate) found: Vec<PathBuf>,
}

/// What an event means for the set of watched directories.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Notice {
    /// A directory may have appeared here (created, or renamed in).
    Appeared(PathBuf),
    /// Whatever was here went away (removed, or renamed out).
    Gone(PathBuf),
    /// Events were lost: rebuild the set from the tree.
    Rescan,
}

/// The notices for one delivery, in the order they apply.
pub(crate) fn notices(delivery: &Delivery) -> Vec<Notice> {
    let Ok(event) = delivery else {
        return Vec::new();
    };
    if event.need_rescan() {
        return vec![Notice::Rescan];
    }
    let each = |notice: fn(PathBuf) -> Notice| -> Vec<Notice> {
        event.paths.iter().cloned().map(notice).collect()
    };
    match event.kind {
        // A file (or a symlink, which is never followed) holds no watch.
        EventKind::Create(CreateKind::File) => Vec::new(),
        EventKind::Create(_) => each(Notice::Appeared),
        EventKind::Remove(_) => each(Notice::Gone),
        EventKind::Modify(ModifyKind::Name(RenameMode::From)) => each(Notice::Gone),
        EventKind::Modify(ModifyKind::Name(RenameMode::To)) => each(Notice::Appeared),
        // A paired rename repeats the `From` and `To` events reported before it.
        EventKind::Modify(ModifyKind::Name(RenameMode::Both)) => Vec::new(),
        // A rename of unknown direction: forget the path, then watch it if a directory is
        // there now.
        EventKind::Modify(ModifyKind::Name(_)) => event
            .paths
            .iter()
            .flat_map(|path| [Notice::Gone(path.clone()), Notice::Appeared(path.clone())])
            .collect(),
        _ => Vec::new(),
    }
}

/// The event that reports entries a walk found inside newly watched directories.
pub(crate) fn found_event(found: Vec<PathBuf>) -> Event {
    let mut event = Event::new(EventKind::Create(CreateKind::Any));
    event.paths = found;
    event
}

/// The directories one tree watcher watches.
#[derive(Debug)]
pub(crate) struct TreePlan {
    roots: Vec<PathBuf>,
    excluded: Vec<PathBuf>,
    watched: BTreeSet<PathBuf>,
}

impl TreePlan {
    /// A plan over `roots` (directories watched with everything under them) leaving out
    /// `excluded`. An exclusion spelled through a root's canonical path (macOS reports
    /// `/private/var` for `/var`; a checkout may sit behind a symlink) is also applied in
    /// the root's own spelling, which is the one events and walks use.
    pub(crate) fn new(
        roots: Vec<PathBuf>,
        excluded: &[PathBuf],
        canonical: impl Fn(&Path) -> Option<PathBuf>,
    ) -> Self {
        let mut spelled: Vec<PathBuf> = Vec::new();
        for exclusion in excluded {
            spelled.push(exclusion.clone());
            for root in &roots {
                if let Some(real) = canonical(root)
                    && real != *root
                    && let Ok(rest) = exclusion.strip_prefix(&real)
                {
                    spelled.push(root.join(rest));
                }
            }
        }
        spelled.sort();
        spelled.dedup();
        Self {
            roots,
            excluded: spelled,
            watched: BTreeSet::new(),
        }
    }

    /// Whether `path` is an excluded directory or lies inside one.
    pub(crate) fn excludes(&self, path: &Path) -> bool {
        self.excluded
            .iter()
            .any(|exclusion| path.starts_with(exclusion))
    }

    /// The directories watched now.
    #[cfg(test)]
    pub(crate) fn watched(&self) -> &BTreeSet<PathBuf> {
        &self.watched
    }

    /// Watch every root and every directory under it that is not watched yet.
    pub(crate) fn start<E>(
        &mut self,
        tree: &impl Tree,
        mut watch: impl FnMut(&Path) -> Watched<E>,
    ) -> Result<Grown, E> {
        let mut grown = Grown::default();
        for root in self.roots.clone() {
            if self.excludes(&root) || !tree.root_is_dir(&root) {
                continue;
            }
            let more = self.grow(&root, tree, &mut watch)?;
            grown.watched.extend(more.watched);
            grown.found.extend(more.found);
        }
        Ok(grown)
    }

    /// A directory may have appeared at `path`: watch it and everything under it, unless it
    /// is excluded, outside the roots, not a directory (a symlink is not followed), already
    /// watched, or inside a directory that is not watched (excluded, or refused).
    pub(crate) fn appeared<E>(
        &mut self,
        path: &Path,
        tree: &impl Tree,
        mut watch: impl FnMut(&Path) -> Watched<E>,
    ) -> Result<Grown, E> {
        if self.watched.contains(path) || self.excludes(path) {
            return Ok(Grown::default());
        }
        let is_dir = if self.roots.iter().any(|root| root == path) {
            tree.root_is_dir(path)
        } else {
            let under_watched = path
                .parent()
                .is_some_and(|parent| self.watched.contains(parent));
            under_watched && tree.kind(path) == Some(EntryKind::Directory)
        };
        if !is_dir {
            return Ok(Grown::default());
        }
        self.grow(path, tree, &mut watch)
    }

    /// Whatever was at `path` went away: forget it and every watched directory under it.
    /// Returns the directories forgotten, for the owner to unwatch.
    pub(crate) fn gone(&mut self, path: &Path) -> Vec<PathBuf> {
        // Paths order by component, so `path` and everything under it are contiguous.
        let doomed: Vec<PathBuf> = self
            .watched
            .range::<Path, _>((Bound::Included(path), Bound::Unbounded))
            .take_while(|dir| dir.starts_with(path))
            .cloned()
            .collect();
        for dir in &doomed {
            self.watched.remove(dir);
        }
        doomed
    }

    /// Forget every watched directory (events were lost); returns them for the owner to
    /// unwatch before it starts again.
    pub(crate) fn reset(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.watched).into_iter().collect()
    }

    /// Watch one more root with everything under it.
    pub(crate) fn add_root<E>(
        &mut self,
        root: &Path,
        tree: &impl Tree,
        watch: impl FnMut(&Path) -> Watched<E>,
    ) -> Result<Grown, E> {
        if !self.roots.iter().any(|known| known == root) {
            self.roots.push(root.to_path_buf());
        }
        self.appeared(root, tree, watch)
    }

    /// Stop watching `root`; returns the directories to unwatch. A directory that another
    /// root still covers stays watched.
    pub(crate) fn remove_root(&mut self, root: &Path) -> Vec<PathBuf> {
        let before = self.roots.len();
        self.roots.retain(|known| known != root);
        if self.roots.len() == before {
            return Vec::new();
        }
        let doomed = self.gone(root);
        let (kept, dropped): (Vec<PathBuf>, Vec<PathBuf>) = doomed
            .into_iter()
            .partition(|dir| self.roots.iter().any(|other| dir.starts_with(other)));
        self.watched.extend(kept);
        dropped
    }

    /// Watch `start` and every directory under it, depth first. A directory is watched
    /// before it is listed; a directory already watched is not walked again.
    fn grow<E>(
        &mut self,
        start: &Path,
        tree: &impl Tree,
        watch: &mut impl FnMut(&Path) -> Watched<E>,
    ) -> Result<Grown, E> {
        let mut grown = Grown::default();
        let mut stack = vec![start.to_path_buf()];
        while let Some(dir) = stack.pop() {
            if self.watched.contains(&dir) {
                continue;
            }
            match watch(&dir) {
                Watched::Yes => {}
                Watched::Skipped => continue,
                Watched::Stop(error) => return Err(error),
            }
            self.watched.insert(dir.clone());
            grown.watched.push(dir.clone());
            let Ok(mut entries) = tree.entries(&dir) else {
                continue;
            };
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            // Reversed onto the stack, so subdirectories are walked in name order.
            for (entry, kind) in entries.into_iter().rev() {
                if kind == EntryKind::Directory && !self.excludes(&entry) {
                    stack.push(entry.clone());
                }
                grown.found.push(entry);
            }
        }
        grown.found.sort();
        Ok(grown)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::watch::event::{AccessKind, DataChange, RemoveKind};

    /// An in-memory tree: every path with its kind. A symlink is `Other`; `links` names the
    /// roots that are symlinks to directories.
    #[derive(Default)]
    struct FakeTree {
        entries: BTreeMap<PathBuf, EntryKind>,
        links: Vec<PathBuf>,
        unreadable: Vec<PathBuf>,
    }

    impl FakeTree {
        fn with(dirs: &[&str], others: &[&str]) -> Self {
            let mut tree = Self::default();
            for dir in dirs {
                tree.mkdir(dir);
            }
            for other in others {
                tree.entries.insert(PathBuf::from(other), EntryKind::Other);
            }
            tree
        }

        /// `mkdir -p`.
        fn mkdir(&mut self, dir: &str) {
            let mut path = PathBuf::new();
            for part in Path::new(dir).iter() {
                path.push(part);
                self.entries.insert(path.clone(), EntryKind::Directory);
            }
        }

        /// `rm -r`.
        fn remove(&mut self, path: &str) {
            self.entries.retain(|entry, _| !entry.starts_with(path));
        }

        /// `mv from to`.
        fn rename(&mut self, from: &str, to: &str) {
            let moved: Vec<(PathBuf, EntryKind)> = self
                .entries
                .iter()
                .filter(|(entry, _)| entry.starts_with(from))
                .map(|(entry, kind)| (entry.clone(), *kind))
                .collect();
            self.remove(from);
            for (entry, kind) in moved {
                let rest = entry.strip_prefix(from).unwrap();
                self.entries.insert(Path::new(to).join(rest), kind);
            }
        }
    }

    impl Tree for FakeTree {
        fn kind(&self, path: &Path) -> Option<EntryKind> {
            self.entries.get(path).copied()
        }

        fn root_is_dir(&self, root: &Path) -> bool {
            self.kind(root) == Some(EntryKind::Directory) || self.links.iter().any(|l| l == root)
        }

        fn entries(&self, dir: &Path) -> std::io::Result<Vec<(PathBuf, EntryKind)>> {
            if self.unreadable.iter().any(|path| path == dir) {
                return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
            }
            Ok(self
                .entries
                .iter()
                .filter(|(entry, _)| entry.parent() == Some(dir))
                .map(|(entry, kind)| (entry.clone(), *kind))
                .collect())
        }
    }

    fn paths(items: &[&str]) -> Vec<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    fn plan(roots: &[&str], excluded: &[&str]) -> TreePlan {
        TreePlan::new(paths(roots), &paths(excluded), |_| None)
    }

    /// A platform that watches everything and records each request.
    fn accept(log: &mut Vec<PathBuf>) -> impl FnMut(&Path) -> Watched<&'static str> + '_ {
        |dir| {
            log.push(dir.to_path_buf());
            Watched::Yes
        }
    }

    fn watched(plan: &TreePlan) -> Vec<PathBuf> {
        plan.watched().iter().cloned().collect()
    }

    /// A repo-shaped checkout: tracked directories, three ignored build trees (one deeply
    /// nested), and a symlink to a directory.
    fn checkout() -> FakeTree {
        let mut tree = FakeTree::with(
            &[
                "/r/src/a/b/c/d/e/f/g",
                "/r/target/debug/deps",
                "/r/clients/web/node_modules/pkg/lib",
                "/r/clients/web/src",
                "/r/.claude/worktrees/agent/target/debug",
                "/r/docs",
            ],
            &[
                "/r/README.md",
                "/r/src/lib.rs",
                "/r/src/a/b/c/d/e/f/g/deep.rs",
            ],
        );
        // A symlink to a tracked directory and one to an excluded one: neither is followed.
        tree.entries
            .insert(PathBuf::from("/r/link-to-src"), EntryKind::Other);
        tree.entries
            .insert(PathBuf::from("/r/docs/link-to-target"), EntryKind::Other);
        tree
    }

    const EXCLUDED: [&str; 3] = [
        "/r/target",
        "/r/clients/web/node_modules",
        "/r/.claude/worktrees",
    ];

    #[test]
    fn hs2_236xy3_the_start_watches_tracked_directories_only() {
        let tree = checkout();
        let mut plan = plan(&["/r"], &EXCLUDED);
        let mut log = Vec::new();
        let grown = plan.start(&tree, accept(&mut log)).unwrap();
        let expected = paths(&[
            "/r",
            "/r/.claude",
            "/r/clients",
            "/r/clients/web",
            "/r/clients/web/src",
            "/r/docs",
            "/r/src",
            "/r/src/a",
            "/r/src/a/b",
            "/r/src/a/b/c",
            "/r/src/a/b/c/d",
            "/r/src/a/b/c/d/e",
            "/r/src/a/b/c/d/e/f",
            "/r/src/a/b/c/d/e/f/g",
        ]);
        assert_eq!(watched(&plan), expected, "deep nesting, no excluded tree");
        assert_eq!(log.len(), expected.len(), "each directory watched once");
        assert_eq!(
            log[0],
            PathBuf::from("/r"),
            "a directory before its children"
        );
        assert_eq!(grown.watched, log);
        // The excluded directories' own entries and the symlinks are found, not walked.
        for entry in ["/r/target", "/r/clients/web/node_modules", "/r/link-to-src"] {
            assert!(grown.found.contains(&PathBuf::from(entry)), "{entry}");
        }
        assert!(!grown.found.iter().any(|f| f.starts_with("/r/target/debug")));
        // Starting again watches nothing more.
        let mut again = Vec::new();
        assert_eq!(
            plan.start(&tree, accept(&mut again)).unwrap(),
            Grown::default()
        );
        assert!(again.is_empty());
    }

    #[test]
    fn hs2_236xy3_excluded_paths_match_whole_components() {
        let plan = plan(&["/r"], &["/r/target"]);
        assert!(plan.excludes(Path::new("/r/target")));
        assert!(plan.excludes(Path::new("/r/target/debug")));
        assert!(!plan.excludes(Path::new("/r/target-notes")));
        assert!(!plan.excludes(Path::new("/r/targ")));
        assert!(!plan.excludes(Path::new("/r")));
    }

    /// An ignored tree created after the start: its own directory is watched until the
    /// owner's exclusions name it, and nothing appears inside an excluded tree.
    #[test]
    fn hs2_236xy3_directories_created_after_the_start() {
        let mut tree = checkout();
        let mut plan = plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();
        let before = watched(&plan);

        // Inside an excluded tree, at any depth, and the excluded directory recreated.
        tree.mkdir("/r/target/release/build/x");
        for path in [
            "/r/target",
            "/r/target/release",
            "/r/target/release/build/x",
        ] {
            let mut log = Vec::new();
            assert_eq!(
                plan.appeared(Path::new(path), &tree, accept(&mut log))
                    .unwrap(),
                Grown::default(),
                "{path}"
            );
            assert!(log.is_empty());
        }
        assert_eq!(watched(&plan), before);

        // A tracked directory created with nested content (`mkdir -p`, then files).
        tree.mkdir("/r/src/new/inner/deeper");
        tree.entries
            .insert(PathBuf::from("/r/src/new/inner/early.rs"), EntryKind::Other);
        let mut log = Vec::new();
        let grown = plan
            .appeared(Path::new("/r/src/new"), &tree, accept(&mut log))
            .unwrap();
        assert_eq!(
            log,
            paths(&["/r/src/new", "/r/src/new/inner", "/r/src/new/inner/deeper"])
        );
        assert_eq!(
            grown.found,
            paths(&[
                "/r/src/new/inner",
                "/r/src/new/inner/deeper",
                "/r/src/new/inner/early.rs"
            ]),
            "entries created before their directory's watch are reported"
        );
        // The same appearance reported again (create, then rename-to) and a nested one
        // already walked add nothing.
        for path in ["/r/src/new", "/r/src/new/inner/deeper"] {
            let mut log = Vec::new();
            assert_eq!(
                plan.appeared(Path::new(path), &tree, accept(&mut log))
                    .unwrap(),
                Grown::default()
            );
            assert!(log.is_empty());
        }

        // A new ignored tree no exclusion names yet (a first build of another tool).
        tree.mkdir("/r/dist/assets");
        plan.appeared(Path::new("/r/dist"), &tree, accept(&mut Vec::new()))
            .unwrap();
        assert!(plan.watched().contains(Path::new("/r/dist/assets")));
    }

    #[test]
    fn hs2_236xy3_what_never_appears_as_a_watched_directory() {
        let mut tree = checkout();
        tree.entries
            .insert(PathBuf::from("/r/src/file.rs"), EntryKind::Other);
        tree.mkdir("/elsewhere/dir");
        let mut plan = plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();
        let before = watched(&plan);
        for path in [
            "/r/src/file.rs",       // a file
            "/r/link-to-src",       // a symlink to a directory is not followed
            "/r/src/vanished",      // gone before the event was handled
            "/elsewhere/dir",       // outside the roots
            "/r/target/debug/deps", // inside an excluded tree
        ] {
            let mut log = Vec::new();
            plan.appeared(Path::new(path), &tree, accept(&mut log))
                .unwrap();
            assert!(log.is_empty(), "{path}");
        }
        // A directory whose parent is not watched (its watch was refused) waits for the
        // parent: the parent's walk watches it.
        tree.mkdir("/r/docs/refused/child");
        let mut refusing = |dir: &Path| {
            if dir == Path::new("/r/docs/refused") {
                Watched::<&str>::Skipped
            } else {
                Watched::Yes
            }
        };
        plan.appeared(Path::new("/r/docs/refused"), &tree, &mut refusing)
            .unwrap();
        let mut log = Vec::new();
        plan.appeared(Path::new("/r/docs/refused/child"), &tree, accept(&mut log))
            .unwrap();
        assert!(log.is_empty());
        assert_eq!(watched(&plan), before);
    }

    #[test]
    fn hs2_236xy3_a_removed_directory_is_forgotten_with_everything_under_it() {
        let mut tree = checkout();
        tree.mkdir("/r/src/ab");
        tree.mkdir("/r/src/a b");
        let mut plan = plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();

        tree.remove("/r/src/a");
        let forgotten = plan.gone(Path::new("/r/src/a"));
        assert_eq!(forgotten.len(), 7, "a and its six nested directories");
        assert!(forgotten.iter().all(|dir| dir.starts_with("/r/src/a")));
        for sibling in ["/r/src", "/r/src/ab", "/r/src/a b"] {
            assert!(plan.watched().contains(Path::new(sibling)), "{sibling}");
        }
        // Removed twice (its own delete and its parent's), or a file removed: nothing more.
        assert!(plan.gone(Path::new("/r/src/a")).is_empty());
        assert!(plan.gone(Path::new("/r/README.md")).is_empty());
        // Recreated: watched again, empty and then refilled.
        tree.mkdir("/r/src/a");
        let mut log = Vec::new();
        plan.appeared(Path::new("/r/src/a"), &tree, accept(&mut log))
            .unwrap();
        assert_eq!(log, paths(&["/r/src/a"]));
        tree.mkdir("/r/src/a/again");
        plan.appeared(Path::new("/r/src/a/again"), &tree, accept(&mut Vec::new()))
            .unwrap();
        assert!(plan.watched().contains(Path::new("/r/src/a/again")));
    }

    /// Renames within the tree, out of it, into it, out of an excluded tree and into one.
    #[test]
    fn hs2_236xy3_renamed_directories_follow_their_new_paths() {
        let mut tree = checkout();
        tree.mkdir("/outside/incoming/sub");
        let mut plan = plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();

        // Within the tree: the old paths are forgotten, the new ones watched.
        tree.rename("/r/src/a/b", "/r/docs/b");
        let from = plan.gone(Path::new("/r/src/a/b"));
        assert_eq!(from.len(), 6);
        let mut log = Vec::new();
        plan.appeared(Path::new("/r/docs/b"), &tree, accept(&mut log))
            .unwrap();
        assert_eq!(log.len(), 6);
        assert!(log.iter().all(|dir| dir.starts_with("/r/docs/b")));
        assert!(!plan.watched().iter().any(|d| d.starts_with("/r/src/a/b")));

        // Out of the tree: only the rename-from arrives.
        tree.remove("/r/docs/b");
        assert_eq!(plan.gone(Path::new("/r/docs/b")).len(), 6);

        // Into the tree: only the rename-to arrives.
        tree.rename("/outside/incoming", "/r/incoming");
        let mut log = Vec::new();
        plan.appeared(Path::new("/r/incoming"), &tree, accept(&mut log))
            .unwrap();
        assert_eq!(log, paths(&["/r/incoming", "/r/incoming/sub"]));

        // Out of an excluded tree into a tracked directory: watched from now on.
        tree.rename("/r/target/debug", "/r/src/debug");
        let mut log = Vec::new();
        plan.appeared(Path::new("/r/src/debug"), &tree, accept(&mut log))
            .unwrap();
        assert_eq!(log, paths(&["/r/src/debug", "/r/src/debug/deps"]));

        // From a tracked directory into an excluded tree: forgotten, never watched there.
        tree.rename("/r/src/debug", "/r/target/debug");
        assert_eq!(plan.gone(Path::new("/r/src/debug")).len(), 2);
        let mut log = Vec::new();
        plan.appeared(Path::new("/r/target/debug"), &tree, accept(&mut log))
            .unwrap();
        assert!(log.is_empty());
    }

    /// The owner stops excluding a directory Git no longer ignores by starting a plan with
    /// the new exclusions (or changing roots): the directory and its subtree are watched.
    #[test]
    fn hs2_236xy3_an_excluded_directory_becoming_tracked_is_watched() {
        let tree = checkout();
        let mut narrower = plan(
            &["/r"],
            &["/r/clients/web/node_modules", "/r/.claude/worktrees"],
        );
        let mut log = Vec::new();
        narrower.start(&tree, accept(&mut log)).unwrap();
        assert!(log.contains(&PathBuf::from("/r/target/debug/deps")));
        assert!(
            !log.iter()
                .any(|dir| dir.starts_with("/r/.claude/worktrees"))
        );
        // No exclusions: everything, as notify's recursive watch did (symlinks aside).
        let mut everything = plan(&["/r"], &[]);
        everything.start(&tree, accept(&mut Vec::new())).unwrap();
        let dirs = tree
            .entries
            .iter()
            .filter(|(dir, kind)| **kind == EntryKind::Directory && dir.starts_with("/r"))
            .count();
        assert_eq!(everything.watched().len(), dirs);
    }

    #[test]
    fn hs2_236xy3_roots_added_removed_excluded_and_gone() {
        let mut tree = checkout();
        tree.mkdir("/git/common/refs");
        let mut plan = plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();
        let checkout_dirs = plan.watched().len();

        // A second root (a linked worktree's Git directory) and back.
        let mut log = Vec::new();
        plan.add_root(Path::new("/git/common"), &tree, accept(&mut log))
            .unwrap();
        assert_eq!(log, paths(&["/git/common", "/git/common/refs"]));
        assert_eq!(plan.remove_root(Path::new("/git/common")).len(), 2);
        assert_eq!(plan.watched().len(), checkout_dirs);
        assert!(plan.remove_root(Path::new("/never")).is_empty());
        // A nested root keeps its directories when the outer one goes.
        plan.add_root(Path::new("/r/src"), &tree, accept(&mut Vec::new()))
            .unwrap();
        let dropped = plan.remove_root(Path::new("/r"));
        assert!(dropped.iter().all(|dir| !dir.starts_with("/r/src")));
        assert!(plan.watched().iter().all(|dir| dir.starts_with("/r/src")));

        // The root removed: everything forgotten. Recreated, nothing watches it (as with
        // notify's inotify watch) until the owner starts again.
        let mut plan = self::plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();
        tree.remove("/r");
        assert_eq!(plan.gone(Path::new("/r")).len(), checkout_dirs);
        tree = checkout();
        assert_eq!(
            plan.start(&tree, accept(&mut Vec::new()))
                .unwrap()
                .watched
                .len(),
            checkout_dirs
        );

        // An excluded root, a missing root and a symlinked root.
        let mut excluded = self::plan(&["/r/target"], &["/r/target"]);
        let mut log = Vec::new();
        excluded.start(&tree, accept(&mut log)).unwrap();
        assert!(log.is_empty());
        let mut missing = self::plan(&["/missing"], &[]);
        missing.start(&tree, accept(&mut log)).unwrap();
        assert!(log.is_empty());
        tree.links.push(PathBuf::from("/link-root"));
        let mut linked = self::plan(&["/link-root"], &[]);
        linked.start(&tree, accept(&mut log)).unwrap();
        assert_eq!(log, paths(&["/link-root"]), "a root is followed");
    }

    /// Exclusions in the canonical spelling apply to a root watched through a symlink.
    #[test]
    fn hs2_236xy3_exclusions_follow_the_roots_spelling() {
        let canonical =
            |root: &Path| (root == Path::new("/var/r")).then(|| PathBuf::from("/private/var/r"));
        let plan = TreePlan::new(
            paths(&["/var/r", "/other"]),
            &paths(&["/private/var/r/target", "/other/node_modules"]),
            canonical,
        );
        assert!(plan.excludes(Path::new("/var/r/target/debug")));
        assert!(plan.excludes(Path::new("/private/var/r/target")));
        assert!(plan.excludes(Path::new("/other/node_modules")));
        assert!(!plan.excludes(Path::new("/var/r/src")));
        // The canonical root itself, when excluded, excludes the given spelling too.
        let all = TreePlan::new(paths(&["/var/r"]), &paths(&["/private/var/r"]), canonical);
        assert!(all.excludes(Path::new("/var/r")));
    }

    /// Exhausting the watch limit stops the walk, at the start or later; a refused directory
    /// is skipped with its subtree while its siblings are still watched; watches freed by a
    /// removal let a later directory in.
    #[test]
    fn hs2_236xy3_watch_limit_exhaustion_and_refused_directories() {
        let mut tree = checkout();
        tree.unreadable.push(PathBuf::from("/r/docs"));
        let limited = |limit: usize| {
            let mut used = 0;
            move |dir: &Path| {
                if dir == Path::new("/r/clients/web") {
                    return Watched::Skipped;
                }
                if used == limit {
                    return Watched::Stop("limit");
                }
                used += 1;
                Watched::Yes
            }
        };
        let mut plan = plan(&["/r"], &EXCLUDED);
        assert_eq!(plan.start(&tree, limited(3)), Err("limit"));
        assert_eq!(plan.watched().len(), 3, "the watches taken are kept");

        let mut plan = self::plan(&["/r"], &EXCLUDED);
        let grown = plan.start(&tree, limited(100)).unwrap();
        assert!(
            !grown
                .watched
                .iter()
                .any(|dir| dir.starts_with("/r/clients/web")),
            "a refused directory is skipped with its subtree"
        );
        assert!(plan.watched().contains(Path::new("/r/clients")));
        assert!(
            plan.watched().contains(Path::new("/r/docs")),
            "watched, though it cannot be listed"
        );
        assert!(plan.watched().contains(Path::new("/r/src/a/b/c/d/e/f/g")));

        // At the limit later: the new directory is refused; a removal frees a watch.
        tree.mkdir("/r/src/late");
        let mut full = |_: &Path| Watched::Stop("limit");
        assert_eq!(
            plan.appeared(Path::new("/r/src/late"), &tree, &mut full),
            Err("limit")
        );
        assert!(!plan.watched().contains(Path::new("/r/src/late")));
        plan.gone(Path::new("/r/src/a"));
        plan.appeared(Path::new("/r/src/late"), &tree, accept(&mut Vec::new()))
            .unwrap();
        assert!(plan.watched().contains(Path::new("/r/src/late")));
    }

    /// Lost events: every watch is dropped and the tree walked again, picking up directories
    /// created and dropping ones removed meanwhile.
    #[test]
    fn hs2_236xy3_a_rescan_rebuilds_the_watched_set() {
        let mut tree = checkout();
        let mut plan = plan(&["/r"], &EXCLUDED);
        plan.start(&tree, accept(&mut Vec::new())).unwrap();
        let before = plan.watched().len();
        tree.remove("/r/docs");
        tree.mkdir("/r/src/unseen/child");
        tree.mkdir("/r/target/unseen");
        let dropped = plan.reset();
        assert_eq!(dropped.len(), before);
        assert!(plan.watched().is_empty());
        plan.start(&tree, accept(&mut Vec::new())).unwrap();
        assert!(plan.watched().contains(Path::new("/r/src/unseen/child")));
        assert!(!plan.watched().contains(Path::new("/r/docs")));
        assert!(!plan.watched().iter().any(|d| d.starts_with("/r/target")));
        assert_eq!(plan.watched().len(), before - 1 + 2);
    }

    #[test]
    fn hs2_236xy3_notices_for_every_event_kind() {
        let at = |kind: EventKind, items: &[&str]| -> Delivery {
            let mut event = Event::new(kind);
            event.paths = paths(items);
            Ok(event)
        };
        let appeared = |path: &str| Notice::Appeared(PathBuf::from(path));
        let gone = |path: &str| Notice::Gone(PathBuf::from(path));
        let cases: Vec<(Delivery, Vec<Notice>)> = vec![
            (
                at(EventKind::Create(CreateKind::Folder), &["/r/d"]),
                vec![appeared("/r/d")],
            ),
            (
                at(EventKind::Create(CreateKind::Any), &["/r/d"]),
                vec![appeared("/r/d")],
            ),
            (at(EventKind::Create(CreateKind::File), &["/r/f"]), vec![]),
            (
                at(EventKind::Remove(RemoveKind::Folder), &["/r/d"]),
                vec![gone("/r/d")],
            ),
            // A watched directory's own deletion arrives without the directory flag.
            (
                at(EventKind::Remove(RemoveKind::File), &["/r/d"]),
                vec![gone("/r/d")],
            ),
            (
                at(
                    EventKind::Modify(ModifyKind::Name(RenameMode::From)),
                    &["/r/old"],
                ),
                vec![gone("/r/old")],
            ),
            (
                at(
                    EventKind::Modify(ModifyKind::Name(RenameMode::To)),
                    &["/r/new"],
                ),
                vec![appeared("/r/new")],
            ),
            (
                at(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                    &["/r/old", "/r/new"],
                ),
                vec![],
            ),
            (
                at(
                    EventKind::Modify(ModifyKind::Name(RenameMode::Any)),
                    &["/r/x"],
                ),
                vec![gone("/r/x"), appeared("/r/x")],
            ),
            (
                at(
                    EventKind::Modify(ModifyKind::Data(DataChange::Any)),
                    &["/r/f"],
                ),
                vec![],
            ),
            (at(EventKind::Access(AccessKind::Any), &["/r/f"]), vec![]),
            (at(EventKind::Any, &[]), vec![]),
            (Ok(crate::watch::rescan_event()), vec![Notice::Rescan]),
            (Err(Error::generic("lost")), vec![]),
        ];
        for (delivery, expected) in cases {
            assert_eq!(notices(&delivery), expected, "{delivery:?}");
        }
        let found = found_event(paths(&["/r/a", "/r/b"]));
        assert_eq!(found.kind, EventKind::Create(CreateKind::Any));
        assert_eq!(found.paths, paths(&["/r/a", "/r/b"]));
    }

    #[test]
    fn hs2_236xy3_only_an_exhausted_limit_stops_a_walk() {
        assert!(matches!(
            refusal(Error::new(ErrorKind::MaxFilesWatch)),
            Watched::Stop(_)
        ));
        let missing = std::io::Error::from(std::io::ErrorKind::NotFound);
        assert!(matches!(refusal(Error::io(missing)), Watched::Skipped));
        assert!(matches!(refusal(Error::generic("x")), Watched::Skipped));
        assert!(matches!(refusal(Error::path_not_found()), Watched::Skipped));
    }

    /// The real file system: directories, files and missing paths, in name order.
    #[test]
    fn hs2_236xy3_the_disk_tree_lists_entries_by_kind() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("r");
        std::fs::create_dir_all(root.join("src/inner")).unwrap();
        std::fs::write(root.join("file"), b"x").unwrap();
        let tree = DiskTree;
        assert_eq!(tree.kind(&root.join("src")), Some(EntryKind::Directory));
        assert_eq!(tree.kind(&root.join("file")), Some(EntryKind::Other));
        assert_eq!(tree.kind(&root.join("missing")), None);
        assert!(tree.root_is_dir(&root));
        assert!(!tree.root_is_dir(&root.join("file")));
        assert!(tree.entries(&root.join("missing")).is_err());
        let mut plan = TreePlan::new(vec![root.clone()], &[root.join("src/inner")], |_| None);
        let mut log = Vec::new();
        let grown = plan.start(&tree, accept(&mut log)).unwrap();
        assert_eq!(log, vec![root.clone(), root.join("src")]);
        assert_eq!(
            grown.found,
            vec![root.join("file"), root.join("src"), root.join("src/inner")]
        );
    }

    /// The real file system: symlinks are entries, never followed; a symlinked root is.
    #[cfg(unix)]
    #[test]
    fn hs2_236xy3_the_disk_tree_never_follows_a_symlink_below_a_root() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("r");
        std::fs::create_dir_all(root.join("src/inner")).unwrap();
        std::fs::create_dir_all(temp.path().join("outside/deep")).unwrap();
        std::os::unix::fs::symlink(temp.path().join("outside"), root.join("link")).unwrap();
        std::os::unix::fs::symlink(&root, temp.path().join("root-link")).unwrap();
        let tree = DiskTree;
        assert_eq!(tree.kind(&root.join("link")), Some(EntryKind::Other));
        assert!(tree.root_is_dir(&temp.path().join("root-link")));

        let linked = temp.path().join("root-link");
        let mut plan = TreePlan::new(vec![linked.clone()], &[], |_| None);
        let mut log = Vec::new();
        let grown = plan.start(&tree, accept(&mut log)).unwrap();
        assert_eq!(
            log,
            vec![linked.clone(), linked.join("src"), linked.join("src/inner")]
        );
        assert!(grown.found.contains(&linked.join("link")));
        assert!(!log.iter().any(|dir| dir.ends_with("deep")));
    }
}
