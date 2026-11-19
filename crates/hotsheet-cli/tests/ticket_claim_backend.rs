//! HS2-WJF0D8 headless E2E: two clones of one bare ticket-store remote, bound through the
//! CLI to a `git_ref` claim backend, claim the same ticket from two concurrent
//! `hotsheet-cli claim` processes. Exactly one wins. Synthetic names only; the native
//! credential store is denied.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn cli(home: &Path, store: &Path, args: &[&str]) -> Command {
    let mut command = Command::new(workspace_bin::path("hotsheet-cli"));
    command
        .arg("-C")
        .arg(store)
        .args(args)
        .env("HOTSHEET_HOME", home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .current_dir(store)
        .stdin(Stdio::null());
    command
}

fn ok(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn git(dir: &Path, args: &[&str]) {
    let status = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap();
    assert!(status.success(), "git {args:?}");
}

struct Fleet {
    _root: tempfile::TempDir,
    home_a: PathBuf,
    home_b: PathBuf,
    a: PathBuf,
    b: PathBuf,
    slug: String,
}

fn fleet() -> Fleet {
    let root = tempfile::tempdir().unwrap();
    let bare = root.path().join("store.git");
    let (a, b) = (root.path().join("a.hs2"), root.path().join("b.hs2"));
    let (home_a, home_b) = (root.path().join("home-a"), root.path().join("home-b"));
    git(
        root.path(),
        &[
            "init",
            "--quiet",
            "--bare",
            "-b",
            "main",
            bare.to_str().unwrap(),
        ],
    );
    git(&bare, &["config", "maintenance.auto", "false"]);
    std::fs::create_dir_all(&a).unwrap();
    ok(cli(&home_a, &a, &["init", "--prefix", "CL"])
        .output()
        .unwrap());
    git(&a, &["config", "maintenance.auto", "false"]);
    git(&a, &["remote", "add", "origin", bare.to_str().unwrap()]);
    let a_arg = a.to_str().unwrap();
    ok(cli(
        &home_a,
        &a,
        &[
            "resource",
            "backend",
            "add",
            "shared",
            "--kind",
            "git-ref",
            "--project",
            a_arg,
        ],
    )
    .output()
    .unwrap());
    ok(cli(
        &home_a,
        &a,
        &[
            "resource",
            "source",
            "bind",
            "--store",
            a_arg,
            "--backend",
            "shared",
            "--project",
            a_arg,
        ],
    )
    .output()
    .unwrap());
    let created = ok(
        cli(&home_a, &a, &["new", "--title", "synthetic shared ticket"])
            .output()
            .unwrap(),
    );
    let slug = created
        .split_whitespace()
        .find(|word| word.starts_with("CL-"))
        .expect("created slug")
        .to_owned();
    git(&a, &["add", "-A"]);
    git(
        &a,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@example.invalid",
            "commit",
            "-q",
            "--allow-empty",
            "-m",
            "seed",
        ],
    );
    git(&a, &["push", "-q", "-u", "origin", "HEAD:main"]);
    git(
        root.path(),
        &["clone", "-q", bare.to_str().unwrap(), b.to_str().unwrap()],
    );
    git(&b, &["config", "maintenance.auto", "false"]);
    Fleet {
        _root: root,
        home_a,
        home_b,
        a,
        b,
        slug,
    }
}

#[test]
fn hs2_wjf0d8_two_clones_claiming_one_ticket_at_once_yield_one_holder() {
    let f = fleet();
    let spawn = |home: &Path, store: &Path, worker: &str| {
        cli(home, store, &["claim", &f.slug, "--worker", worker])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap()
    };
    let first = spawn(&f.home_a, &f.a, "worker-a");
    let second = spawn(&f.home_b, &f.b, "worker-b");
    let outputs = [
        first.wait_with_output().unwrap(),
        second.wait_with_output().unwrap(),
    ];
    let wins = outputs.iter().filter(|o| o.status.success()).count();
    assert_eq!(
        wins,
        1,
        "{:?}",
        outputs
            .iter()
            .map(|o| String::from_utf8_lossy(&o.stderr).into_owned())
            .collect::<Vec<_>>()
    );
    let loser = outputs.iter().find(|o| !o.status.success()).unwrap();
    let message = String::from_utf8_lossy(&loser.stderr);
    assert!(message.contains("is claimed by 'worker-"), "{message}");
}

#[test]
fn hs2_wjf0d8_an_unreachable_backend_fails_the_claim_closed() {
    let f = fleet();
    let gone = f.a.with_file_name("gone.git");
    git(
        &f.b,
        &["remote", "set-url", "origin", gone.to_str().unwrap()],
    );
    let output = cli(&f.home_b, &f.b, &["claim", &f.slug, "--worker", "worker-b"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("claim_backend_unreachable"), "{message}");
}
