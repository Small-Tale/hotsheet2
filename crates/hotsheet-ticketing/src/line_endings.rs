//! The store's line-ending policy (HS2-WNPX98, `docs/02` §2.7).
//!
//! Hot Sheet writes every store file with LF line endings, and Git must carry those bytes
//! unchanged between sites. A user's Git configuration can say otherwise: Git for Windows
//! installs `core.autocrlf=true` system-wide, so a rebase, merge, abort, or recovery
//! checkout on Windows rewrote ticket files as CRLF. A Windows site and a macOS or Linux
//! site holding the same store then read different bytes for one ticket.
//!
//! The store owns the policy instead of inheriting it. Each checkout carries
//! [`ATTRIBUTE`] for every path under the store in `<git-dir>/info/attributes`, the
//! attribute file Git always reads and that outranks any committed `.gitattributes`, the
//! user's `core.attributesFile`, `core.autocrlf`, and `core.eol`. `-text` turns off every
//! end-of-line conversion, so Git checks out and commits store files byte for byte. It
//! leaves merges and diffs alone: those detect binary content themselves and take the
//! merge driver from the `merge` attribute. Like the merge driver's registration, it is
//! never committed, so every machine pins it for itself, and [`ensure_pinned`] runs where
//! [`crate::merge_driver::ensure_registered`] does: before every sync integration and when
//! a store is created, linked, opened by the server, or checked by `doctor`. New stores
//! also commit the attribute in their `.gitattributes`, so a plain `git clone` is right
//! before Hot Sheet first opens it.
//!
//! A checkout made before the policy, or by plain Git on Windows, can already hold CRLF
//! copies of files whose committed bytes are LF. Pinning `-text` would make those copies
//! differ from the index, and the next sync would commit the CRLF bytes. So the first
//! pin also repairs the working tree: every tracked file whose index blob has only LF
//! endings and whose working copy has CRLF endings (all or some) gets its CRLF pairs
//! turned back into LF. That is exactly the conversion the old `autocrlf` policy applied
//! when it committed such a file, so pending local edits keep their content and only
//! lose the line endings Git added. Files committed with CRLF, binary files, and
//! untracked files are left as they are.

use std::path::{Path, PathBuf};

/// The attribute that turns off Git's end-of-line conversion for store files.
pub const ATTRIBUTE: &str = "-text";

/// The comment written above the attribute line.
const COMMENT: &str = "# Hot Sheet store line endings: byte-exact, never converted (HS2-WNPX98)";

/// The attribute line committed in a new store's own `.gitattributes`, which applies to
/// the directory holding it.
pub const COMMITTED_LINE: &str = "* -text";

/// Whether a checkout's line endings are pinned, as found or after [`ensure_pinned`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineEndingStatus {
    /// The attribute was already present.
    Pinned,
    /// This call added the attribute and converted `repaired` CRLF working copies back
    /// to the committed LF bytes.
    Repaired { repaired: usize },
    /// The store is not inside a Git repository, or its attribute file could not be
    /// written; Git's own configuration still decides line endings.
    Missing,
}

impl LineEndingStatus {
    /// Whether Git carries the store's bytes unchanged in this checkout.
    #[must_use]
    pub fn applies(self) -> bool {
        self != Self::Missing
    }
}

/// The `info/attributes` path and the line that covers every path under the store.
fn layout(store: &Path) -> Option<(PathBuf, String)> {
    let (attributes, prefix) = crate::merge_driver::info_attributes(store)?;
    let line = if prefix.is_empty() {
        format!("* {ATTRIBUTE}")
    } else {
        // A designated root inside a code repository pins only its own directory.
        format!("{prefix}** {ATTRIBUTE}")
    };
    Some((attributes, line))
}

fn has_line(attributes: &Path, line: &str) -> bool {
    std::fs::read_to_string(attributes).is_ok_and(|text| text.lines().any(|l| l.trim() == line))
}

/// The pin's status without changing anything.
#[must_use]
pub fn status(store: &Path) -> LineEndingStatus {
    match layout(store) {
        Some((attributes, line)) if has_line(&attributes, &line) => LineEndingStatus::Pinned,
        _ => LineEndingStatus::Missing,
    }
}

/// Pin the store's line endings in this checkout, repairing CRLF working copies the
/// first time (see the module documentation). Idempotent; never commits anything.
pub fn ensure_pinned(store: &Path) -> LineEndingStatus {
    let Some((attributes, line)) = layout(store) else {
        return LineEndingStatus::Missing;
    };
    if has_line(&attributes, &line) {
        return LineEndingStatus::Pinned;
    }
    if !append_line(&attributes, &line) {
        return LineEndingStatus::Missing;
    }
    LineEndingStatus::Repaired {
        repaired: repair_working_tree(store),
    }
}

fn append_line(attributes: &Path, line: &str) -> bool {
    if let Some(dir) = attributes.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return false;
    }
    let mut text = std::fs::read_to_string(attributes).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push_str(COMMENT);
    text.push('\n');
    text.push_str(line);
    text.push('\n');
    crate::store::write_file_atomically(attributes, text.as_bytes()).is_ok()
}

/// Tracked files under `store` whose index blob has only LF endings and whose working
/// copy has some CRLF endings, relative to `store`.
fn converted_copies(store: &Path) -> Vec<PathBuf> {
    let Ok(out) = crate::git::command()
        .arg("-C")
        .arg(store)
        .args(["ls-files", "--eol", "-z"])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    out.stdout
        .split(|byte| *byte == 0)
        .filter_map(|entry| {
            let entry = std::str::from_utf8(entry).ok()?;
            let (info, path) = entry.split_once('\t')?;
            let mut fields = info.split_whitespace();
            let index = fields.next()?;
            let worktree = fields.next()?;
            (index == "i/lf" && matches!(worktree, "w/crlf" | "w/mixed"))
                .then(|| PathBuf::from(path))
        })
        .collect()
}

/// Turn every CRLF pair back into LF in each [`converted_copies`] file; returns how many
/// files changed. A file that cannot be read or written keeps its CRLF copy, which Git
/// then reports as a modification.
///
/// The index still records each repaired file's old (CRLF) size, and Git reports a size
/// change as a modification without reading the file. So each repaired file whose
/// content now equals its index blob is re-added, which only refreshes its stat data;
/// a file with a pending edit is left unstaged, as it was. Best-effort: an add that
/// loses `index.lock` to a concurrent commit leaves only stat-dirty entries, which the
/// next `git add` clears without changing any blob.
fn repair_working_tree(store: &Path) -> usize {
    let repaired = normalize_copies(store);
    if repaired.is_empty() {
        return 0;
    }
    // `git diff` compares content, so it lists only the files that really differ.
    let edited: Vec<PathBuf> = crate::git::command()
        .arg("-C")
        .arg(store)
        .args(["diff", "--name-only", "--relative", "-z", "--", "."])
        .output()
        .ok()
        .filter(|out| out.status.success())
        .map(|out| {
            out.stdout
                .split(|byte| *byte == 0)
                .filter(|path| !path.is_empty())
                .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
                .collect()
        })
        .unwrap_or_default();
    let mut pathspecs = Vec::new();
    for path in repaired.iter().filter(|path| !edited.contains(path)) {
        pathspecs.extend_from_slice(path.to_string_lossy().as_bytes());
        pathspecs.push(0);
    }
    if !pathspecs.is_empty() {
        let _ = add_pathspecs(store, &pathspecs);
    }
    repaired.len()
}

fn add_pathspecs(store: &Path, pathspecs: &[u8]) -> std::io::Result<()> {
    use std::io::Write;
    let mut child = crate::git::command()
        .arg("-C")
        .arg(store)
        .args([
            "--literal-pathspecs",
            "add",
            "--pathspec-from-file=-",
            "--pathspec-file-nul",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(pathspecs)?;
    }
    child.wait()?;
    Ok(())
}

/// The [`converted_copies`] files whose CRLF pairs were turned into LF, relative to
/// `store`.
fn normalize_copies(store: &Path) -> Vec<PathBuf> {
    converted_copies(store)
        .into_iter()
        .filter(|relative| {
            let path = store.join(relative);
            let Ok(bytes) = std::fs::read(&path) else {
                return false;
            };
            let normalized = crlf_to_lf(&bytes);
            normalized != bytes && crate::store::write_file_atomically(&path, &normalized).is_ok()
        })
        .collect()
}

/// `bytes` with each CRLF pair replaced by LF; a lone CR is kept, as Git's own
/// conversion keeps it.
#[must_use]
pub fn crlf_to_lf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().peekable();
    while let Some(&byte) = iter.next() {
        if byte == b'\r' && iter.peek() == Some(&&b'\n') {
            continue;
        }
        out.push(byte);
    }
    out
}

#[cfg(test)]
#[path = "line_endings_tests.rs"]
mod tests;
