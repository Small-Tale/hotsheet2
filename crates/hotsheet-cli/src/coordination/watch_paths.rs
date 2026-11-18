//! Register a wait's watched paths in one batch (HS2-P0EHD4).
//!
//! On macOS each `Watcher::watch` call stops the FSEvents stream and starts a new one, and
//! every start is a blocking registration RPC to `fseventsd`. That RPC waits behind
//! `syspolicyd` when the machine is busy (for example during a test gate that launches many
//! fresh executables), so a wait that watched four paths one by one could spend tens of
//! seconds before it was armed. `paths_mut` adds every path and starts the stream once.

use std::path::Path;

use notify::{RecursiveMode, Watcher};

/// Watch every `(path, mode)` with one stream start.
pub(crate) fn watch_all(
    watcher: &mut dyn Watcher,
    paths: &[(&Path, RecursiveMode)],
) -> notify::Result<()> {
    let mut batch = watcher.paths_mut();
    for (path, mode) in paths {
        batch.add(path, *mode)?;
    }
    batch.commit()
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;

    /// HS2-P0EHD4: every batched path is watched, with its own recursion mode.
    #[test]
    fn hs2_p0ehd4_batched_paths_are_all_watched() {
        let root = tempfile::tempdir().unwrap();
        let (flat, deep) = (root.path().join("flat"), root.path().join("deep"));
        std::fs::create_dir_all(deep.join("nested")).unwrap();
        std::fs::create_dir_all(&flat).unwrap();
        let (sender, receiver) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                let _ = sender.send(event);
            })
            .unwrap();
        watch_all(
            &mut watcher,
            &[
                (&flat, RecursiveMode::NonRecursive),
                (&deep, RecursiveMode::Recursive),
            ],
        )
        .unwrap();
        let seen = |name: &str| {
            let deadline = std::time::Instant::now() + Duration::from_secs(20);
            while std::time::Instant::now() < deadline {
                if let Ok(Ok(event)) = receiver.recv_timeout(Duration::from_millis(100)) {
                    if event.paths.iter().any(|path| path.ends_with(name)) {
                        return true;
                    }
                }
            }
            false
        };
        std::fs::write(flat.join("one"), "x").unwrap();
        assert!(seen("one"), "a non-recursive path is watched");
        std::fs::write(deep.join("nested").join("two"), "x").unwrap();
        assert!(seen("two"), "a recursive path's subtree is watched");
    }
}
