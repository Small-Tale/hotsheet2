//! HS2-2R32SD: the Windows program-resolution rules, on every platform.
use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::orchestration::step_program::{DEFAULT_PATHEXT, variable, windows_step_program};

fn root() -> PathBuf {
    PathBuf::from("C:\\work\\checkout")
}

/// A resolver over a fixed set of existing files, spelled as the resolver builds them.
fn resolve(
    program: &str,
    path: Option<&str>,
    pathext: Option<&str>,
    files: &[PathBuf],
) -> Option<PathBuf> {
    let files: HashSet<&PathBuf> = files.iter().collect();
    windows_step_program(program, &root(), path, pathext, &|candidate: &Path| {
        files.contains(&candidate.to_path_buf())
    })
}

fn with(base: PathBuf, ext: &str) -> PathBuf {
    let mut text = base.into_os_string();
    text.push(ext);
    PathBuf::from(text)
}

#[test]
fn hs2_2r32sd_a_bare_name_searches_path_entries_in_order_with_pathext() {
    let tools = PathBuf::from("C:\\tools");
    let bin = PathBuf::from("C:\\bin");
    let path = Some("C:\\tools;C:\\bin");
    // Only `C:\bin\npm.cmd` exists: `.COM` and `.EXE` are tried first, then the batch file.
    let npm = with(bin.join("npm"), ".CMD");
    assert_eq!(
        resolve("npm", path, None, std::slice::from_ref(&npm)),
        Some(npm.clone())
    );
    // An earlier entry wins, and within an entry PATHEXT's order wins.
    let tools_exe = with(tools.join("npm"), ".EXE");
    let tools_bat = with(tools.join("npm"), ".BAT");
    assert_eq!(
        resolve(
            "npm",
            path,
            None,
            &[npm.clone(), tools_bat.clone(), tools_exe.clone()]
        ),
        Some(tools_exe)
    );
    // A name with a listed extension must exist exactly; it is never extended.
    let exact = bin.join("npm.cmd");
    assert_eq!(
        resolve("npm.cmd", path, None, std::slice::from_ref(&exact)),
        Some(exact)
    );
    assert_eq!(
        resolve("npm.cmd", path, None, &[with(bin.join("npm.cmd"), ".EXE")]),
        None
    );
    // A name with another extension (or none) is never run as named.
    assert_eq!(
        resolve("build.py", path, None, &[bin.join("build.py")]),
        None
    );
    assert_eq!(resolve("npm", path, None, &[bin.join("npm")]), None);
    // Without PATH nothing is found; the checkout itself is never searched implicitly.
    assert_eq!(
        resolve("npm", None, None, &[with(root().join("npm"), ".EXE")]),
        None
    );
    assert_eq!(
        resolve("npm", Some(""), None, &[with(root().join("npm"), ".EXE")]),
        None
    );
    // A relative PATH entry counts from the checkout; quotes and blanks are ignored.
    let local = with(root().join("node_modules\\.bin").join("tsc"), ".CMD");
    assert_eq!(
        resolve(
            "tsc",
            Some(" ;\"node_modules\\.bin\";C:\\bin"),
            None,
            std::slice::from_ref(&local)
        ),
        Some(local)
    );
}

#[test]
fn hs2_2r32sd_pathext_is_the_steps_own_compared_without_case() {
    let bin = PathBuf::from("C:\\bin");
    let path = Some("C:\\bin");
    // A step's PATHEXT replaces the default, in its order.
    let ps1 = with(bin.join("check"), ".PS1");
    let exe = with(bin.join("check"), ".EXE");
    assert_eq!(
        resolve(
            "check",
            path,
            Some(".PS1;.EXE"),
            &[exe.clone(), ps1.clone()]
        ),
        Some(ps1)
    );
    assert_eq!(resolve("check", path, Some(".EXE"), &[ps1_only()]), None);
    // A blank or malformed PATHEXT falls back to the default.
    assert_eq!(
        resolve("check", path, Some(" "), std::slice::from_ref(&exe)),
        Some(exe.clone())
    );
    assert_eq!(
        resolve("check", path, Some("EXE;;"), std::slice::from_ref(&exe)),
        Some(exe.clone())
    );
    // A listed extension matches whatever its case.
    let lower = bin.join("check.exe");
    assert_eq!(
        resolve(
            "check.exe",
            path,
            Some(".EXE"),
            std::slice::from_ref(&lower)
        ),
        Some(lower)
    );
    assert_eq!(DEFAULT_PATHEXT, ".COM;.EXE;.BAT;.CMD");
}

fn ps1_only() -> PathBuf {
    with(PathBuf::from("C:\\bin").join("check"), ".PS1")
}

#[test]
fn hs2_2r32sd_a_program_with_a_directory_part_is_the_checkouts_own_or_absolute() {
    // A relative program with a directory part is joined to the checkout root, with
    // either separator, and resolved by PATHEXT like any other name.
    let script = with(root().join("scripts\\check"), ".CMD");
    assert_eq!(
        resolve(
            "scripts\\check",
            Some("C:\\bin"),
            None,
            std::slice::from_ref(&script)
        ),
        Some(script)
    );
    let forward = root().join("scripts/check.bat");
    assert_eq!(
        resolve(
            "scripts/check.bat",
            None,
            None,
            std::slice::from_ref(&forward)
        ),
        Some(forward)
    );
    let dot = with(root().join(".\\check"), ".EXE");
    assert_eq!(
        resolve(".\\check", None, None, std::slice::from_ref(&dot)),
        Some(dot)
    );
    // It is never looked up on PATH, even when PATH has one.
    assert_eq!(
        resolve(
            "scripts\\check",
            Some("C:\\bin"),
            None,
            &[with(PathBuf::from("C:\\bin\\scripts\\check"), ".CMD")]
        ),
        None
    );
    // Absolute programs: a drive path, a UNC path and a root-relative path.
    for absolute in [
        "C:\\tools\\gate.exe",
        "\\\\server\\share\\gate.cmd",
        "\\tools\\gate.exe",
        "D:/tools/gate.exe",
    ] {
        let file = PathBuf::from(absolute);
        assert_eq!(
            resolve(absolute, None, None, std::slice::from_ref(&file)),
            Some(file),
            "{absolute}"
        );
    }
    let extended = with(PathBuf::from("C:\\tools\\gate"), ".EXE");
    assert_eq!(
        resolve(
            "C:\\tools\\gate",
            None,
            None,
            std::slice::from_ref(&extended)
        ),
        Some(extended)
    );
    // A drive-relative name depends on a per-drive directory and is refused, as is an
    // empty program; a drive-relative PATH entry is skipped.
    assert_eq!(
        resolve("C:gate.exe", None, None, &[PathBuf::from("C:gate.exe")]),
        None
    );
    assert_eq!(resolve("", Some("C:\\bin"), None, &[]), None);
    let later = with(PathBuf::from("C:\\bin").join("gate"), ".EXE");
    assert_eq!(
        resolve(
            "gate",
            Some("D:tools;C:\\bin"),
            None,
            std::slice::from_ref(&later)
        ),
        Some(later)
    );
}

#[test]
fn hs2_2r32sd_the_steps_variables_are_read_without_case_last_one_wins() {
    let environment = vec![
        ("Path".into(), "C:\\first".into()),
        ("PATHEXT".into(), ".EXE".into()),
        ("PATH".into(), "C:\\second".into()),
    ];
    assert_eq!(variable(&environment, "PATH"), Some("C:\\second"));
    assert_eq!(variable(&environment, "pathext"), Some(".EXE"));
    assert_eq!(variable(&environment, "ComSpec"), None);
}
