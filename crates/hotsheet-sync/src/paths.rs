//! The one path spelling Hot Sheet keeps for a directory (HS2-ECJ188, HS2-CTV942).
//!
//! On Windows, [`std::fs::canonicalize`] returns a *verbatim* path (`\\?\C:\Users\...`, or
//! `\\?\UNC\server\share\...`). Git for Windows rejects that form: `git init` and
//! `git worktree add` fail, `git clone` reads `\\?\C:` as a UNC host name, and Git's own
//! output (`C:/Users/...`) never matches it. A person cannot paste it into most tools
//! either, and the readable checkout id hashed from it differs from the one hashed from
//! the ordinary spelling of the same directory.
//!
//! [`canonicalize`] is therefore the only canonicalization Hot Sheet uses: the standard
//! library's, followed by [`simplified`], which drops the verbatim prefix whenever the
//! ordinary spelling names exactly the same file. Canonicalization already expands 8.3
//! short names (`RUNNER~1`) to long ones, so the result is one stable spelling per
//! directory. A path that cannot be simplified losslessly (longer than `MAX_PATH`, a
//! reserved device name such as `NUL`, a trailing dot or space, a character the Win32
//! namespace forbids) stays verbatim. Off Windows both functions are the standard
//! behavior: a Unix file name may legitimately begin with `\\?\`.
//!
//! [`simplify_verbatim`] holds the rule as a pure string function so every platform's
//! tests cover it with Windows-shaped inputs. A source-scan test forbids any other
//! `canonicalize` call under `crates/`.

use std::borrow::Cow;
use std::io;
use std::path::{Path, PathBuf};

/// `MAX_PATH`: the longest ordinary Win32 path, in UTF-16 units, including its NUL.
const MAX_PATH: usize = 260;

/// Device names that the Win32 namespace maps to a device in any directory and with
/// any extension; a component named like this is reachable only verbatim.
const RESERVED_NAMES: &[&str] = &[
    "CON",
    "PRN",
    "AUX",
    "NUL",
    "COM0",
    "COM1",
    "COM2",
    "COM3",
    "COM4",
    "COM5",
    "COM6",
    "COM7",
    "COM8",
    "COM9",
    "COM\u{b9}",
    "COM\u{b2}",
    "COM\u{b3}",
    "LPT0",
    "LPT1",
    "LPT2",
    "LPT3",
    "LPT4",
    "LPT5",
    "LPT6",
    "LPT7",
    "LPT8",
    "LPT9",
    "LPT\u{b9}",
    "LPT\u{b2}",
    "LPT\u{b3}",
    "CONIN$",
    "CONOUT$",
];

/// `path` made absolute with every symbolic link resolved, in its ordinary spelling
/// when that is lossless (see the module documentation).
///
/// # Errors
///
/// As [`std::fs::canonicalize`]: the path does not exist, or a component cannot be read.
pub fn canonicalize<P: AsRef<Path>>(path: P) -> io::Result<PathBuf> {
    // The one sanctioned canonicalization; every other site calls this function.
    let canonical = std::fs::canonicalize(path.as_ref())?;
    Ok(match simplified(&canonical) {
        Cow::Borrowed(_) => canonical,
        Cow::Owned(ordinary) => ordinary,
    })
}

/// `path` without a Windows verbatim prefix when the ordinary spelling names the same
/// file; `path` itself otherwise, and always off Windows.
#[must_use]
pub fn simplified(path: &Path) -> Cow<'_, Path> {
    if cfg!(windows)
        && let Some(ordinary) = path.to_str().and_then(simplify_verbatim)
    {
        return Cow::Owned(PathBuf::from(ordinary));
    }
    Cow::Borrowed(path)
}

/// A path Git printed (`rev-parse --show-toplevel`, `--absolute-git-dir`), in this
/// platform's own spelling. Git for Windows prints `C:/Users/...`; Hot Sheet shows and
/// stores `C:\Users\...`, the spelling [`canonicalize`] returns for the same directory.
#[must_use]
pub fn from_git(output: &str) -> PathBuf {
    if cfg!(windows)
        && let Some(native) = windows_separators(output)
    {
        return PathBuf::from(native);
    }
    PathBuf::from(output)
}

/// `spelling` with `/` separators turned into `\` when it is an absolute Windows drive
/// (`C:/dir`) or UNC (`//server/share`) path; `None` for any other spelling. A pure
/// string rule, like [`simplify_verbatim`], so every platform's tests cover it.
#[must_use]
pub fn windows_separators(spelling: &str) -> Option<String> {
    let bytes = spelling.as_bytes();
    let drive = bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && matches!(bytes[2], b'/' | b'\\');
    let unc = spelling.starts_with("//") && !spelling.starts_with("//?/") && spelling.len() > 2;
    (spelling.contains('/') && (drive || unc)).then(|| spelling.replace('/', "\\"))
}

/// The ordinary Win32 spelling of the verbatim path `spelling`, or `None` when it is not
/// verbatim or when dropping the prefix would change which file it names.
///
/// `\\?\C:\dir` becomes `C:\dir` and `\\?\UNC\server\share\dir` becomes
/// `\\server\share\dir`. This is a pure string rule, so it behaves the same on every
/// platform; [`simplified`] applies it only on Windows.
#[must_use]
pub fn simplify_verbatim(spelling: &str) -> Option<String> {
    let rest = spelling.strip_prefix(r"\\?\")?;
    let ordinary = if let Some(unc) = strip_prefix_ignore_ascii_case(rest, r"UNC\") {
        let mut parts = unc.splitn(3, '\\');
        let server = parts.next().filter(|server| !server.is_empty())?;
        let share = parts.next().filter(|share| !share.is_empty())?;
        if !plain_component(server) || !plain_component(share) {
            return None;
        }
        let tail = parts.next();
        if let Some(tail) = tail {
            plain_components(tail)?;
        }
        format!(
            r"\\{server}\{share}{}",
            tail.map_or(String::new(), |tail| format!(r"\{tail}"))
        )
    } else {
        let mut chars = rest.chars();
        let drive = chars.next().filter(char::is_ascii_alphabetic)?;
        if chars.next() != Some(':') {
            return None;
        }
        let after = chars.as_str();
        // `\\?\C:` is the drive's root; plain `C:` would mean its working directory.
        let tail = if after.is_empty() {
            ""
        } else {
            after.strip_prefix('\\')?
        };
        plain_components(tail)?;
        format!(r"{drive}:\{tail}")
    };
    (ordinary.encode_utf16().count() < MAX_PATH).then_some(ordinary)
}

fn strip_prefix_ignore_ascii_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// `Some(())` when every `\`-separated component of `tail` is spelled the same with and
/// without the verbatim prefix. One trailing separator is allowed.
fn plain_components(tail: &str) -> Option<()> {
    let tail = tail.strip_suffix('\\').unwrap_or(tail);
    if tail.is_empty() {
        return Some(());
    }
    tail.split('\\').all(plain_component).then_some(())
}

/// Whether the Win32 namespace leaves `component` as it is: not empty, `.` or `..`,
/// without a forbidden character, not ending in a dot or space, and not a device name.
fn plain_component(component: &str) -> bool {
    if component.is_empty() || component == "." || component == ".." {
        return false;
    }
    if component.ends_with(['.', ' ']) {
        return false;
    }
    if component
        .chars()
        .any(|c| c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '/' | '|' | '?' | '*'))
    {
        return false;
    }
    let stem = component.split('.').next().unwrap_or(component).trim_end();
    let stem = stem.to_uppercase();
    !RESERVED_NAMES.contains(&stem.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple(spelling: &str) -> Option<String> {
        simplify_verbatim(spelling)
    }

    /// HS2-ECJ188: a verbatim drive path loses its prefix; the drive root keeps its
    /// separator so it never becomes drive-relative.
    #[test]
    fn hs2_ecj188_a_verbatim_drive_path_becomes_its_ordinary_spelling() {
        assert_eq!(
            simple(r"\\?\C:\Users\runneradmin\AppData\Local\Temp\.tmpttgbo9\target-worktree")
                .as_deref(),
            Some(r"C:\Users\runneradmin\AppData\Local\Temp\.tmpttgbo9\target-worktree")
        );
        assert_eq!(simple(r"\\?\C:\").as_deref(), Some(r"C:\"));
        assert_eq!(simple(r"\\?\d:").as_deref(), Some(r"d:\"));
        assert_eq!(simple(r"\\?\C:\dir\").as_deref(), Some(r"C:\dir\"));
        assert_eq!(
            simple(r"\\?\C:\Users\Ünïcode dir\file.txt").as_deref(),
            Some(r"C:\Users\Ünïcode dir\file.txt")
        );
    }

    /// HS2-ECJ188: a verbatim UNC path becomes the `\\server\share` spelling.
    #[test]
    fn hs2_ecj188_a_verbatim_unc_path_becomes_a_plain_unc_path() {
        assert_eq!(
            simple(r"\\?\UNC\server\share\repo").as_deref(),
            Some(r"\\server\share\repo")
        );
        assert_eq!(
            simple(r"\\?\unc\server\share").as_deref(),
            Some(r"\\server\share")
        );
        assert_eq!(simple(r"\\?\UNC\server"), None);
        assert_eq!(simple(r"\\?\UNC\\share"), None);
    }

    /// HS2-ECJ188: ordinary and non-path spellings are not verbatim and stay as they are.
    #[test]
    fn hs2_ecj188_a_path_that_is_not_verbatim_is_left_alone() {
        for spelling in [
            r"C:\Users\RUNNER~1\AppData\Local\Temp",
            "C:/Users/runneradmin/repo/.git",
            r"\\server\share\repo",
            "/home/user/repo",
            r"\\.\pipe\hotsheet",
            r"\\?\Volume{12345678-0000-0000-0000-000000000000}\dir",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1",
            r"\\?\1:\dir",
            r"\\?\CC:\dir",
            "",
        ] {
            assert_eq!(simple(spelling), None, "{spelling}");
        }
    }

    /// HS2-ECJ188: the prefix stays when the ordinary spelling would name another file.
    #[test]
    fn hs2_ecj188_a_lossy_simplification_keeps_the_verbatim_prefix() {
        for spelling in [
            r"\\?\C:\dir\NUL",
            r"\\?\C:\dir\con.txt",
            r"\\?\C:\dir\Com1",
            "\\\\?\\C:\\dir\\lpt\u{b9}.log",
            r"\\?\C:\dir\aux .txt",
            r"\\?\C:\dir\trailing.",
            r"\\?\C:\dir\trailing ",
            r"\\?\C:\dir\.\here",
            r"\\?\C:\dir\..\up",
            r"\\?\C:\dir\\double",
            r"\\?\C:\dir\a/b",
            r"\\?\C:\dir\what?",
            r"\\?\C:\dir\pipe|name",
            "\\\\?\\C:\\dir\\control\u{1}",
            r"\\?\UNC\server\share\NUL",
            r"\\?\C:dir",
        ] {
            assert_eq!(simple(spelling), None, "{spelling}");
        }
        // A name that only begins like a device name is ordinary.
        assert_eq!(
            simple(r"\\?\C:\dir\console\nullable.txt").as_deref(),
            Some(r"C:\dir\console\nullable.txt")
        );
    }

    /// HS2-ECJ188: a path at or beyond `MAX_PATH` is reachable only verbatim.
    #[test]
    fn hs2_ecj188_a_path_beyond_max_path_keeps_the_verbatim_prefix() {
        let fits = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH - 4));
        assert_eq!(simple(&fits).map(|s| s.len()), Some(MAX_PATH - 1));
        let too_long = format!(r"\\?\C:\{}", "a".repeat(MAX_PATH - 3));
        assert_eq!(simple(&too_long), None);
    }

    /// HS2-ECJ188: Git's forward-slash spelling of a Windows path reads back with native
    /// separators; anything else (a Unix path, a relative path) is left alone.
    #[test]
    fn hs2_ecj188_git_output_reads_back_in_native_form() {
        assert_eq!(
            windows_separators("C:/Users/runneradmin/repo/.git").as_deref(),
            Some(r"C:\Users\runneradmin\repo\.git")
        );
        assert_eq!(
            windows_separators("//server/share/repo").as_deref(),
            Some(r"\\server\share\repo")
        );
        assert_eq!(
            windows_separators(r"C:\mixed/sep").as_deref(),
            Some(r"C:\mixed\sep")
        );
        for spelling in [
            r"C:\already\native",
            "/home/user/repo",
            "relative/dir",
            "C:relative/dir",
            "//?/C:/verbatim",
            "//",
            "",
        ] {
            assert_eq!(windows_separators(spelling), None, "{spelling}");
        }
        let read = from_git("C:/Users/runneradmin/repo");
        if cfg!(windows) {
            assert_eq!(read.to_str(), Some(r"C:\Users\runneradmin\repo"));
        } else {
            assert_eq!(read, Path::new("C:/Users/runneradmin/repo"));
        }
        assert_eq!(from_git("/home/user/repo"), Path::new("/home/user/repo"));
    }

    /// HS2-ECJ188: [`simplified`] applies the rule only on Windows; a Unix file name may
    /// begin with `\\?\`.
    #[test]
    fn hs2_ecj188_simplified_changes_only_windows_paths() {
        let verbatim = Path::new(r"\\?\C:\Users\runneradmin\repo");
        let result = simplified(verbatim);
        if cfg!(windows) {
            assert_eq!(result, Path::new(r"C:\Users\runneradmin\repo"));
        } else {
            assert!(matches!(result, Cow::Borrowed(_)));
            assert_eq!(result, verbatim);
        }
        let ordinary = Path::new("plain/relative");
        assert!(matches!(simplified(ordinary), Cow::Borrowed(_)));
    }

    /// HS2-ECJ188, HS2-CTV942: canonicalization never returns a verbatim spelling for an
    /// ordinary directory, and spells one directory the same however it was reached.
    #[test]
    fn hs2_ecj188_canonicalize_returns_one_ordinary_spelling() {
        let root = std::env::temp_dir().join(format!("hs2-ecj188-{}", std::process::id()));
        let dir = root.join("repo");
        std::fs::create_dir_all(&dir).unwrap();
        let direct = canonicalize(&dir).unwrap();
        assert!(direct.is_absolute());
        assert!(!direct.to_string_lossy().starts_with(r"\\?\"), "{direct:?}");
        let roundabout = canonicalize(dir.join("..").join("repo")).unwrap();
        assert_eq!(roundabout, direct);
        assert_eq!(canonicalize(direct.as_path()).unwrap(), direct);
        assert!(canonicalize(root.join("missing")).is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}
