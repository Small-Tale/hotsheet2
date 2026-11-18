//! HS2-2R32SD: which file a validation step's program names on Windows.
//!
//! Pure, so the Windows rules are tested on every platform. Unix keeps
//! [`hotsheet_ticketing::git::program_path`] (an `execvp`-like search).
//!
//! - An absolute program (`C:\tools\x.exe`, `\\server\share\x.cmd`, `\x.exe`) is used
//!   as is. A relative program with a directory part (`scripts\check.cmd`,
//!   `scripts/check`, `.\check`) is the checkout's own and is joined to its root.
//! - A bare name is looked up only in the step's `PATH` entries, in order; a relative
//!   entry counts from the checkout root. The current directory is never searched
//!   implicitly, unlike `CreateProcess`.
//! - A name whose extension is listed in `PATHEXT` must exist as named. Any other name
//!   is tried with each `PATHEXT` extension appended, in order, and never as named
//!   (Windows would not start it). `PATHEXT` defaults to `.COM;.EXE;.BAT;.CMD`;
//!   extensions compare without case.
//! - A drive-relative name (`C:tool.exe`) depends on a per-drive current directory and
//!   is refused.
//!
//! `.cmd` and `.bat` files are started by the standard library, which runs them under
//! `cmd.exe` with its own argument escaping; an argument it cannot pass safely fails
//! the launch.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The extensions Windows tries when `PATHEXT` is unset.
pub(crate) const DEFAULT_PATHEXT: &str = ".COM;.EXE;.BAT;.CMD";

fn separator(byte: u8) -> bool {
    byte == b'\\' || byte == b'/'
}

/// `C:` followed by something other than a separator: relative to that drive's own
/// current directory.
fn drive_relative(program: &str) -> bool {
    let bytes = program.as_bytes();
    bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && !bytes.get(2).copied().is_some_and(separator)
}

fn absolute(program: &str) -> bool {
    let bytes = program.as_bytes();
    bytes.first().copied().is_some_and(separator)
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && separator(bytes[2]))
}

/// The extension of the last path component, with its dot, if any.
fn extension(program: &str) -> Option<&str> {
    let name = program.rsplit(['\\', '/']).next().unwrap_or(program);
    name.rfind('.')
        .filter(|dot| *dot > 0 && *dot + 1 < name.len())
        .map(|dot| &name[dot..])
}

/// The extensions `pathext` lists (`.EXT` entries only), or the default when it lists
/// none.
fn extensions(pathext: Option<&str>) -> Vec<&str> {
    let listed = pathext.map(split).unwrap_or_default();
    if listed.is_empty() {
        split(DEFAULT_PATHEXT)
    } else {
        listed
    }
}

fn split(value: &str) -> Vec<&str> {
    value
        .split(';')
        .map(str::trim)
        .filter(|ext| ext.len() > 1 && ext.starts_with('.'))
        .collect()
}

/// The file `base` names, by the `PATHEXT` rule above.
fn candidate(
    base: PathBuf,
    named: &str,
    pathext: &[&str],
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let listed =
        extension(named).is_some_and(|ext| pathext.iter().any(|e| e.eq_ignore_ascii_case(ext)));
    if listed {
        return is_file(&base).then_some(base);
    }
    pathext.iter().find_map(|ext| {
        let mut with: OsString = base.clone().into_os_string();
        with.push(ext);
        let with = PathBuf::from(with);
        is_file(&with).then_some(with)
    })
}

/// The file a step's `program` names on Windows, for a step that runs in `root` with
/// `path` (`;`-separated) and `pathext` from its own environment. `None` when nothing
/// matches: the step ends `program_not_found`.
pub(crate) fn windows_step_program(
    program: &str,
    root: &Path,
    path: Option<&str>,
    pathext: Option<&str>,
    is_file: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    if program.is_empty() || drive_relative(program) {
        return None;
    }
    let pathext = extensions(pathext);
    if absolute(program) {
        return candidate(PathBuf::from(program), program, &pathext, is_file);
    }
    if program.bytes().any(separator) {
        return candidate(root.join(program), program, &pathext, is_file);
    }
    path?
        .split(';')
        .map(|entry| entry.trim().trim_matches('"'))
        .filter(|entry| !entry.is_empty() && !drive_relative(entry))
        .find_map(|entry| {
            let base = if absolute(entry) {
                PathBuf::from(entry)
            } else {
                root.join(entry)
            };
            candidate(base.join(program), program, &pathext, is_file)
        })
}

/// The step environment's value of `name`, compared without case as Windows does.
pub(crate) fn variable<'a>(environment: &'a [(OsString, OsString)], name: &str) -> Option<&'a str> {
    environment
        .iter()
        .rev()
        .find(|(key, _)| {
            key.to_str()
                .is_some_and(|key| key.eq_ignore_ascii_case(name))
        })
        .and_then(|(_, value)| value.to_str())
}
