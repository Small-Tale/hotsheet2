//! HS2-WNPX98: the store's line-ending pin and the one-time repair of CRLF copies.
//! Every clone sets `core.autocrlf=true` in its own config, which is what Git for Windows
//! installs system-wide, so these run the Windows conversion on every platform.
//! Synthetic file bodies only.

use std::path::Path;

use super::*;

fn git(dir: &Path, args: &[&str]) -> String {
    let out = crate::git::command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?} failed in {}: {}",
        dir.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn identity(dir: &Path) {
    git(dir, &["config", "user.name", "Line Endings Test"]);
    git(dir, &["config", "user.email", "eol@test.invalid"]);
}

/// A repository with LF files committed, cloned into `windows` with `core.autocrlf=true`
/// so its checkout holds CRLF copies, as a store created before the fix did.
fn windows_clone() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let origin = dir.path().join("origin");
    std::fs::create_dir(&origin).unwrap();
    git(&origin, &["init", "--quiet"]);
    identity(&origin);
    std::fs::create_dir(origin.join("tickets")).unwrap();
    std::fs::write(
        origin.join("tickets/one.md"),
        "---\nslug: HS-1\n---\nbody\n",
    )
    .unwrap();
    std::fs::write(origin.join("tickets/two.md"), "line a\nline b\n").unwrap();
    std::fs::write(origin.join("committed-crlf.txt"), "kept\r\nas is\r\n").unwrap();
    std::fs::write(origin.join("image.bin"), b"\x00\x01\r\n\x02").unwrap();
    std::fs::write(origin.join(".gitignore"), ".hotsheet-note-mutation.lock\n").unwrap();
    // Commit the CRLF file byte for byte, as a store committed it before any policy.
    std::fs::write(
        origin.join(".git/info/attributes"),
        "committed-crlf.txt -text\n",
    )
    .unwrap();
    git(&origin, &["add", "-A"]);
    git(&origin, &["commit", "--quiet", "-m", "seed"]);
    std::fs::remove_file(origin.join(".git/info/attributes")).unwrap();
    let windows = dir.path().join("windows");
    git(
        dir.path(),
        &[
            "-c",
            "core.autocrlf=true",
            "clone",
            "--quiet",
            "--config",
            "core.autocrlf=true",
            origin.to_str().unwrap(),
            windows.to_str().unwrap(),
        ],
    );
    identity(&windows);
    (dir, windows)
}

fn read(dir: &Path, file: &str) -> Vec<u8> {
    std::fs::read(dir.join(file)).unwrap()
}

#[test]
fn hs2_wnpx98_crlf_to_lf_keeps_a_lone_cr() {
    assert_eq!(crlf_to_lf(b"a\r\nb\rc\r\n\r\n"), b"a\nb\rc\n\n");
    assert_eq!(crlf_to_lf(b""), b"");
    assert_eq!(crlf_to_lf(b"\r"), b"\r");
    assert_eq!(crlf_to_lf(b"no endings"), b"no endings");
}

#[test]
fn hs2_wnpx98_pins_once_and_commits_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let store = dir.path();
    git(store, &["init", "--quiet"]);
    assert_eq!(status(store), LineEndingStatus::Missing);
    assert_eq!(
        ensure_pinned(store),
        LineEndingStatus::Repaired { repaired: 0 }
    );
    assert_eq!(status(store), LineEndingStatus::Pinned);
    assert_eq!(ensure_pinned(store), LineEndingStatus::Pinned);
    let attributes = std::fs::read_to_string(store.join(".git/info/attributes")).unwrap();
    assert_eq!(attributes.matches("* -text").count(), 1, "{attributes}");
    assert!(!store.join(".gitattributes").exists());
    assert!(git(store, &["status", "--porcelain"]).is_empty());
}

#[test]
fn hs2_wnpx98_a_directory_outside_git_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    // A ceiling keeps discovery from finding an enclosing repository of the temp dir.
    let store = dir.path().join("plain");
    std::fs::create_dir(&store).unwrap();
    if crate::merge_driver::info_attributes(&store).is_none() {
        assert_eq!(ensure_pinned(&store), LineEndingStatus::Missing);
        assert!(!status(&store).applies());
    }
}

/// HS2-WNPX98 (pre-fix store): the first pin turns Git's CRLF copies back into the
/// committed LF bytes, leaves CRLF that was committed, binary, and untracked files alone,
/// and the checkout is clean afterwards, so the next sync commits nothing.
#[test]
fn hs2_wnpx98_the_first_pin_repairs_crlf_copies_of_a_pre_fix_checkout() {
    let (_dir, store) = windows_clone();
    assert_eq!(
        read(&store, "tickets/one.md"),
        b"---\r\nslug: HS-1\r\n---\r\nbody\r\n"
    );
    std::fs::write(store.join("untracked.txt"), "new\r\nfile\r\n").unwrap();

    assert_eq!(
        ensure_pinned(&store),
        LineEndingStatus::Repaired { repaired: 3 }
    );
    assert_eq!(
        read(&store, "tickets/one.md"),
        b"---\nslug: HS-1\n---\nbody\n"
    );
    assert_eq!(read(&store, "tickets/two.md"), b"line a\nline b\n");
    assert_eq!(
        read(&store, ".gitignore"),
        b".hotsheet-note-mutation.lock\n"
    );
    assert_eq!(read(&store, "committed-crlf.txt"), b"kept\r\nas is\r\n");
    assert_eq!(read(&store, "image.bin"), b"\x00\x01\r\n\x02");
    assert_eq!(read(&store, "untracked.txt"), b"new\r\nfile\r\n");
    assert_eq!(
        git(&store, &["status", "--porcelain"]),
        "?? untracked.txt\n"
    );

    // Repeated: nothing more to do, nothing rewritten.
    assert_eq!(ensure_pinned(&store), LineEndingStatus::Pinned);
    assert_eq!(read(&store, "untracked.txt"), b"new\r\nfile\r\n");
}

/// A pending local edit made on top of a CRLF copy keeps its content and loses only the
/// CRLF pairs, as the old `autocrlf` policy would have committed it.
#[test]
fn hs2_wnpx98_the_repair_keeps_a_pending_edit_and_normalizes_mixed_endings() {
    let (_dir, store) = windows_clone();
    // A writer that read the CRLF copy and appended LF lines leaves mixed endings.
    std::fs::write(store.join("tickets/two.md"), "line a\r\nline b\r\nadded\n").unwrap();
    assert_eq!(
        ensure_pinned(&store),
        LineEndingStatus::Repaired { repaired: 3 }
    );
    assert_eq!(read(&store, "tickets/two.md"), b"line a\nline b\nadded\n");
    assert_eq!(
        git(&store, &["status", "--porcelain"]),
        " M tickets/two.md\n"
    );
    git(&store, &["commit", "--quiet", "-am", "edit"]);
    assert_eq!(
        git(&store, &["show", "HEAD:tickets/two.md"]),
        "line a\nline b\nadded\n"
    );
}

/// After the pin, Git's checkouts in this clone write LF even with `core.autocrlf=true`:
/// a branch switch and a hard reset reproduce the committed bytes.
#[test]
fn hs2_wnpx98_checkouts_after_the_pin_keep_lf() {
    let (_dir, store) = windows_clone();
    ensure_pinned(&store);
    git(&store, &["checkout", "--quiet", "-b", "side"]);
    std::fs::write(
        store.join("tickets/one.md"),
        "---\nslug: HS-1\n---\nchanged\n",
    )
    .unwrap();
    git(&store, &["commit", "--quiet", "-am", "side"]);
    git(&store, &["checkout", "--quiet", "-"]);
    assert_eq!(
        read(&store, "tickets/one.md"),
        b"---\nslug: HS-1\n---\nbody\n"
    );
    git(&store, &["reset", "--quiet", "--hard", "side"]);
    assert_eq!(
        read(&store, "tickets/one.md"),
        b"---\nslug: HS-1\n---\nchanged\n"
    );
    assert!(git(&store, &["status", "--porcelain"]).is_empty());
}

/// A designated root inside a code repository pins only its own directory; the user's
/// code keeps the repository's own line-ending rules.
#[test]
fn hs2_wnpx98_a_designated_root_pins_only_its_directory() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path();
    git(repo, &["init", "--quiet"]);
    git(repo, &["config", "core.autocrlf", "true"]);
    identity(repo);
    let store = repo.join("hotsheet");
    std::fs::create_dir(&store).unwrap();
    std::fs::write(store.join("ticket.md"), "a\nb\n").unwrap();
    std::fs::write(repo.join("code.txt"), "x\ny\n").unwrap();
    git(repo, &["add", "-A"]);
    git(repo, &["commit", "--quiet", "-m", "seed"]);
    std::fs::remove_file(store.join("ticket.md")).unwrap();
    std::fs::remove_file(repo.join("code.txt")).unwrap();
    git(repo, &["checkout", "--quiet", "--", "."]);
    assert_eq!(read(&store, "ticket.md"), b"a\r\nb\r\n");

    assert_eq!(
        ensure_pinned(&store),
        LineEndingStatus::Repaired { repaired: 1 }
    );
    let attributes = std::fs::read_to_string(repo.join(".git/info/attributes")).unwrap();
    assert!(attributes.contains("hotsheet/** -text"), "{attributes}");
    assert!(!attributes.lines().any(|l| l == "* -text"), "{attributes}");
    assert_eq!(read(&store, "ticket.md"), b"a\nb\n");
    // The code file outside the store is untouched and still follows autocrlf.
    assert_eq!(read(repo, "code.txt"), b"x\r\ny\r\n");
    std::fs::remove_file(repo.join("code.txt")).unwrap();
    std::fs::remove_file(store.join("ticket.md")).unwrap();
    git(repo, &["checkout", "--quiet", "--", "."]);
    assert_eq!(read(repo, "code.txt"), b"x\r\ny\r\n");
    assert_eq!(read(&store, "ticket.md"), b"a\nb\n");
}
