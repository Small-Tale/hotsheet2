//! Real separate-process reviewed shared sections (HS2-YHYNJ6). Every pool is a disposable,
//! independently initialized project and ticket store; all content is synthetic.

#[path = "support/coordination.rs"]
mod support;

use std::fs;

use hotsheet_cli::coordination::Phase;
use hotsheet_ticketing::git_connection_id;
use support::{Fixture, git};

const COVERAGE: &str = "docs/TEST-COVERAGE.md";
const DOC: &str = "\
# Coverage

| Feature | Status |
| --- | --- |
| one | planned |
| two | planned |
| three | planned |

## Notes

### Shared notes

- existing

## Archive

### Other notes

- other

";

fn fixture() -> (Fixture, String) {
    let f = Fixture::new();
    fs::create_dir_all(f.project.join("docs")).unwrap();
    fs::create_dir_all(f.project.join("src")).unwrap();
    fs::write(f.project.join(COVERAGE), DOC).unwrap();
    for name in ["one", "two", "three", "other"] {
        fs::write(f.project.join(format!("src/{name}.rs")), "// synthetic\n").unwrap();
    }
    git(&f.project, &["add", "docs", "src"]);
    git(&f.project, &["commit", "-qm", "synthetic shared document"]);
    let base = git(&f.project, &["rev-parse", "HEAD"]);
    (f, base)
}

fn register(f: &Fixture, role: &str, id: &str) {
    f.run(&[
        "register",
        "--role",
        role,
        "--id",
        id,
        "--session",
        &format!("{id}-session"),
        "--route",
        "human",
        "--lease-seconds",
        "600",
    ]);
}

fn integrator(f: &Fixture, command: &str, extra: &[&str]) {
    let mut args = vec![command, "--id", "root", "--session", "root-session"];
    args.extend_from_slice(extra);
    f.run(&args);
}

fn output(f: &Fixture, args: &[&str]) -> std::process::Output {
    let args = f.coord(args);
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

fn dispatch(
    f: &Fixture,
    session: &str,
    operation: &str,
    worker: &str,
    ticket: &hotsheet_model::Ticket,
    base: &str,
    scope: &[&str],
) -> std::process::Output {
    let connection = git_connection_id(&f.store);
    let native = ticket.id.to_string();
    let worker_session = format!("{worker}-session");
    let mut args = vec![
        "assign",
        "--id",
        "dispatcher",
        "--session",
        session,
        "--worker",
        worker,
        "--worker-session",
        &worker_session,
        "--operation",
        operation,
        "--connection",
        &connection,
        "--native-id",
        &native,
        "--slug",
        &ticket.slug,
        "--base",
        base,
    ];
    for path in scope {
        args.extend(["--scope", path]);
    }
    output(f, &args)
}

fn succeeded(out: &std::process::Output) {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

fn state_bytes(f: &Fixture) -> Vec<u8> {
    fs::read(f.project.join(".git/hotsheet-coordination/state.json")).unwrap()
}

/// A refusal names its reason and leaves the durable pool byte-identical.
fn refused(f: &Fixture, out: impl FnOnce() -> std::process::Output, reason: &str) {
    let before = state_bytes(f);
    let out = out();
    assert!(!out.status.success(), "expected refusal: {reason}");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains(reason), "{stderr}");
    assert_eq!(state_bytes(f), before, "refusal changed state: {reason}");
}

fn accept(f: &Fixture, worker: &str, operation: &str, ticket: &hotsheet_model::Ticket) {
    f.cli(&["claim", &ticket.slug, "--worker", worker, "--eta", "10m"]);
    let session = format!("{worker}-session");
    f.run(&[
        "worker",
        "--id",
        worker,
        "--session",
        &session,
        "--assignment",
        operation,
        "--phase",
        "accepted",
    ]);
}

/// Commit `edits` on a worker branch from `base`, leaving `main` checked out.
fn commit(f: &Fixture, branch: &str, base: &str, edits: &[(&str, &str)]) -> String {
    git(&f.project, &["checkout", "-q", "-B", branch, base]);
    for (path, content) in edits {
        fs::write(f.project.join(path), content).unwrap();
        git(&f.project, &["add", path]);
    }
    git(
        &f.project,
        &["commit", "-qm", &format!("synthetic {branch}")],
    );
    let sha = git(&f.project, &["rev-parse", "HEAD"]);
    git(&f.project, &["checkout", "-q", "main"]);
    sha
}

fn handoff(
    f: &Fixture,
    worker: &str,
    operation: &str,
    ticket: &hotsheet_model::Ticket,
    sha: &str,
) -> std::process::Output {
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note",
        &format!("## Integration handoff {sha}"),
    ]);
    // Releasing an already released claim is harmless for a second attempt.
    let _ = f
        .command(&["release", &ticket.slug, "--worker", worker])
        .output();
    let session = format!("{worker}-session");
    output(
        f,
        &[
            "worker",
            "--id",
            worker,
            "--session",
            &session,
            "--assignment",
            operation,
            "--phase",
            "handoff",
            "--commit",
            sha,
        ],
    )
}

fn row(doc: &str, id: &str, status: &str) -> String {
    doc.replace(
        &format!("| {id} | planned |"),
        &format!("| {id} | {status} |"),
    )
}

#[test]
fn hs2_yhynj6_independent_workers_share_reviewed_sections_while_earlier_work_integrates() {
    let (f, base) = fixture();
    let tickets: Vec<_> = (0..4).map(|_| f.ticket()).collect();
    register(&f, "integrator", "root");
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    // Without the review, a section scope is unapproved sharing.
    integrator(&f, "publish", &["--base", &base]);
    register(&f, "dispatcher", "dispatcher");
    for worker in ["one", "two", "three", "four"] {
        register(&f, "worker", worker);
    }
    refused(
        &f,
        || {
            dispatch(
                &f,
                "dispatcher-session",
                "op-1",
                "one",
                &tickets[0],
                &base,
                &["src/one.rs", "docs/TEST-COVERAGE.md#row:one"],
            )
        },
        "not published as a reviewed shared document",
    );
    // The review must name a real file at the published base.
    refused(
        &f,
        || {
            output(
                &f,
                &[
                    "publish",
                    "--id",
                    "root",
                    "--session",
                    "root-session",
                    "--base",
                    &base,
                    "--shared",
                    "docs/MISSING.md",
                    "--all-participants-upgraded",
                ],
            )
        },
        "is not a file at the published base",
    );
    integrator(
        &f,
        "publish",
        &[
            "--base",
            &base,
            "--shared",
            COVERAGE,
            "--all-participants-upgraded",
        ],
    );
    assert_eq!(f.run(&["status"]).state.version, 5);

    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-1",
        "one",
        &tickets[0],
        &base,
        &["src/one.rs", "docs/TEST-COVERAGE.md#row:one"],
    ));
    accept(&f, "one", "op-1", &tickets[0]);
    let first = commit(
        &f,
        "one",
        &base,
        &[
            ("src/one.rs", "// one\n"),
            (COVERAGE, &row(DOC, "one", "covered")),
        ],
    );
    succeeded(&handoff(&f, "one", "op-1", &tickets[0], &first));
    integrator(
        &f,
        "mark",
        &["--assignment", "op-1", "--phase", "integrating"],
    );

    // Two more workers truthfully declare the same reviewed document during integration and CI.
    // Worker two is a real waiting process; the section-scoped reservation wakes it.
    let mut two = f.waiting(&[
        "next",
        "--role",
        "worker",
        "--id",
        "two",
        "--session",
        "two-session",
        "--route",
        "human",
        "--lease-seconds",
        "600",
    ]);
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-2",
        "two",
        &tickets[1],
        &base,
        &["src/two.rs", "docs/TEST-COVERAGE.md#row:two"],
    ));
    let woke = two.result();
    let offered = woke.state.active_assignment("two").unwrap();
    assert_eq!(offered.id, "op-2");
    assert_eq!(
        offered.scope,
        ["docs/TEST-COVERAGE.md#row:two", "src/two.rs"]
    );
    assert_eq!(woke.state.assignments["op-1"].phase, Phase::Integrating);
    integrator(&f, "mark", &["--assignment", "op-1", "--phase", "ci"]);
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-3",
        "three",
        &tickets[2],
        &base,
        &[
            "src/three.rs",
            "docs/TEST-COVERAGE.md#heading:three-notes-hs2-cccccc",
        ],
    ));
    let state = f.run(&["status"]).state;
    let pending: Vec<_> = state
        .assignments
        .values()
        .filter(|a| a.phase.pending())
        .map(|a| (a.id.as_str(), a.phase))
        .collect();
    assert_eq!(
        pending,
        [
            ("op-1", Phase::Ci),
            ("op-2", Phase::Offered),
            ("op-3", Phase::Offered)
        ]
    );

    // Same section, the whole shared file, its directory, held code, and a stale generation.
    for (scope, reason) in [
        ("docs/TEST-COVERAGE.md#row:two", "overlaps"),
        (COVERAGE, "overlaps"),
        ("docs", "overlaps"),
        ("src/three.rs", "overlaps"),
    ] {
        refused(
            &f,
            || {
                dispatch(
                    &f,
                    "dispatcher-session",
                    "op-4",
                    "four",
                    &tickets[3],
                    &base,
                    &["src/other.rs", scope],
                )
            },
            reason,
        );
    }
    refused(
        &f,
        || {
            dispatch(
                &f,
                "stale-session",
                "op-4",
                "four",
                &tickets[3],
                &base,
                &["src/other.rs"],
            )
        },
        "assignment authority is the dispatcher",
    );
    // A lost delivery replays exactly; a replay that changes scope is refused.
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-2",
        "two",
        &tickets[1],
        &base,
        &["docs/TEST-COVERAGE.md#row:two", "src/two.rs"],
    ));
    refused(
        &f,
        || {
            dispatch(
                &f,
                "dispatcher-session",
                "op-2",
                "two",
                &tickets[1],
                &base,
                &["src/two.rs", "docs/TEST-COVERAGE.md#row:one"],
            )
        },
        "different assignment",
    );
}

#[test]
fn hs2_yhynj6_code_handoffs_must_change_only_their_declared_scope() {
    let (f, base) = fixture();
    let ticket = f.ticket();
    register(&f, "integrator", "root");
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(
        &f,
        "publish",
        &[
            "--base",
            &base,
            "--shared",
            COVERAGE,
            "--all-participants-upgraded",
        ],
    );
    register(&f, "dispatcher", "dispatcher");
    register(&f, "worker", "two");
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-2",
        "two",
        &ticket,
        &base,
        &["src/two.rs", "docs/TEST-COVERAGE.md#row:two"],
    ));
    accept(&f, "two", "op-2", &ticket);

    let undeclared_section = commit(
        &f,
        "two-sections",
        &base,
        &[
            ("src/two.rs", "// two\n"),
            (COVERAGE, &row(&row(DOC, "two", "covered"), "one", "stolen")),
        ],
    );
    let undeclared_file = commit(
        &f,
        "two-files",
        &base,
        &[("src/two.rs", "// two\n"), ("src/other.rs", "// other\n")],
    );
    let preamble = commit(
        &f,
        "two-preamble",
        &base,
        &[(COVERAGE, &format!("Lead.\n{}", row(DOC, "two", "covered")))],
    );
    let unrelated_root = {
        let tree = git(&f.project, &["rev-parse", &format!("{base}^{{tree}}")]);
        git(
            &f.project,
            &["commit-tree", &tree, "-m", "synthetic unrelated root"],
        )
    };
    for (sha, reason) in [
        (&undeclared_section, "undeclared sections row:one"),
        (&undeclared_file, "src/other.rs outside the declared scope"),
        (&preamble, "edit outside keyed sections"),
        (&unrelated_root, "does not descend from the assignment base"),
    ] {
        refused(&f, || handoff(&f, "two", "op-2", &ticket, sha), reason);
        let state = f.run(&["status"]).state;
        assert_eq!(state.assignments["op-2"].phase, Phase::Accepted);
    }
    // The truthful commit hands off; a fresh process (restart) reads the same verdict.
    let truthful = commit(
        &f,
        "two",
        &base,
        &[
            ("src/two.rs", "// two\n"),
            (COVERAGE, &row(DOC, "two", "covered")),
        ],
    );
    succeeded(&handoff(&f, "two", "op-2", &ticket, &truthful));
    let state = f.run(&["status"]).state;
    assert_eq!(state.assignments["op-2"].phase, Phase::Handoff);
    assert_eq!(
        state.assignments["op-2"].commit.as_deref(),
        Some(truthful.as_str())
    );
    // Replaying the recorded handoff is idempotent.
    succeeded(&handoff(&f, "two", "op-2", &ticket, &truthful));
}

#[test]
fn hs2_yhynj6_a_too_narrow_scope_widens_only_through_reviewed_reassignment() {
    let (f, base) = fixture();
    let ticket = f.ticket();
    register(&f, "integrator", "root");
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(
        &f,
        "publish",
        &[
            "--base",
            &base,
            "--shared",
            COVERAGE,
            "--all-participants-upgraded",
        ],
    );
    register(&f, "dispatcher", "dispatcher");
    register(&f, "worker", "two");
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-narrow",
        "two",
        &ticket,
        &base,
        &["src/two.rs", "docs/TEST-COVERAGE.md#row:two"],
    ));
    accept(&f, "two", "op-narrow", &ticket);
    let wide = commit(
        &f,
        "two-wide",
        &base,
        &[
            ("src/two.rs", "// two\n"),
            ("src/other.rs", "// needed too\n"),
            (COVERAGE, &row(DOC, "two", "covered")),
        ],
    );
    refused(
        &f,
        || handoff(&f, "two", "op-narrow", &ticket, &wide),
        "src/other.rs outside the declared scope",
    );
    // The worker blocks with a durable note naming the scope it needs.
    let request = f.root.path().join("scope-request.md");
    fs::write(
        &request,
        "## Scope change request\n\nAlso needs `src/other.rs`.\n",
    )
    .unwrap();
    f.cli(&[
        "edit",
        &ticket.slug,
        "--note-file",
        request.to_str().unwrap(),
    ]);
    f.run(&[
        "worker",
        "--id",
        "two",
        "--session",
        "two-session",
        "--assignment",
        "op-narrow",
        "--phase",
        "blocked",
    ]);
    // Until the integrator reviews it, the blocked operation still holds its scope.
    refused(
        &f,
        || {
            dispatch(
                &f,
                "dispatcher-session",
                "op-wide",
                "two",
                &ticket,
                &base,
                &["src", "docs/TEST-COVERAGE.md#row:two"],
            )
        },
        "overlaps",
    );
    integrator(
        &f,
        "mark",
        &["--assignment", "op-narrow", "--phase", "done"],
    );
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-wide",
        "two",
        &ticket,
        &base,
        &["src", "docs/TEST-COVERAGE.md#row:two"],
    ));
    accept(&f, "two", "op-wide", &ticket);
    succeeded(&handoff(&f, "two", "op-wide", &ticket, &wide));
}

#[test]
fn hs2_yhynj6_legacy_integrator_handoffs_keep_their_unscoped_contract() {
    let (f, base) = fixture();
    let ticket = f.ticket();
    f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "integrator",
        "--session",
        "legacy-root",
        "--route",
        "human",
        "--lease-seconds",
        "600",
    ]);
    register(&f, "worker", "worker");
    f.assign("op-legacy", &ticket, &base, "legacy-root");
    accept(&f, "worker", "op-legacy", &ticket);
    // A schema-three assignment declares no scope, so its handoff is not scope-reviewed.
    let anything = commit(
        &f,
        "legacy",
        &base,
        &[
            ("src/other.rs", "// legacy\n"),
            (COVERAGE, &format!("Lead.\n{DOC}")),
        ],
    );
    succeeded(&handoff(&f, "worker", "op-legacy", &ticket, &anything));
    let state = f.run(&["status"]).state;
    assert_eq!(state.version, 3);
    assert_eq!(state.assignments["op-legacy"].phase, Phase::Handoff);
}

#[test]
fn hs2_yhynj6_handoffs_that_move_unchanged_headings_are_refused() {
    let (f, base) = fixture();
    let ticket = f.ticket();
    register(&f, "integrator", "root");
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(
        &f,
        "publish",
        &[
            "--base",
            &base,
            "--shared",
            COVERAGE,
            "--all-participants-upgraded",
        ],
    );
    register(&f, "dispatcher", "dispatcher");
    register(&f, "worker", "two");
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-2",
        "two",
        &ticket,
        &base,
        &["src/two.rs", "docs/TEST-COVERAGE.md#row:two"],
    ));
    accept(&f, "two", "op-2", &ticket);
    let declared = row(DOC, "two", "covered");
    // Unchanged blocks moved: top-level blocks swapped, and a child moved to another parent.
    let notes = "## Notes\n\n### Shared notes\n\n- existing\n\n";
    let archive = "## Archive\n\n### Other notes\n\n- other\n\n";
    let other = "### Other notes\n\n- other\n\n";
    let reordered = declared.replace(&format!("{notes}{archive}"), &format!("{archive}{notes}"));
    let reparented = declared
        .replace(other, "")
        .replace("- existing\n\n", &format!("- existing\n\n{other}"));
    assert_ne!(reordered, declared);
    assert_ne!(reparented, declared);
    for (branch, content) in [("two-reorder", reordered), ("two-reparent", reparented)] {
        let sha = commit(&f, branch, &base, &[(COVERAGE, &content)]);
        refused(
            &f,
            || handoff(&f, "two", "op-2", &ticket, &sha),
            "edit outside keyed sections",
        );
        assert_eq!(
            f.run(&["status"]).state.assignments["op-2"].phase,
            Phase::Accepted
        );
    }
    let truthful = commit(&f, "two", &base, &[(COVERAGE, &declared)]);
    succeeded(&handoff(&f, "two", "op-2", &ticket, &truthful));
}

/// A `git` first on `PATH` that blocks this process's first `cat-file` on a FIFO until the
/// test opens it for writing, so a publication's base validation is held at a known point.
#[cfg(unix)]
fn held_git(f: &Fixture) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let bin = f.root.path().join("held-git");
    fs::create_dir_all(&bin).unwrap();
    let fifo = f.root.path().join("git-hold");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let real = String::from_utf8(
        std::process::Command::new("sh")
            .args(["-c", "command -v git"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap();
    let wrapper = bin.join("git");
    fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\ncase \"$*\" in *cat-file*) if [ -p \"{fifo}\" ]; then cat \"{fifo}\" >/dev/null; rm -f \"{fifo}\"; fi ;; esac\nexec \"{real}\" \"$@\"\n",
            fifo = fifo.display(),
            real = real.trim()
        ),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    (bin, fifo)
}

#[cfg(unix)]
#[test]
fn hs2_yhynj6_an_integrator_expiring_during_base_validation_cannot_publish_shared_policy() {
    let (f, base) = fixture();
    register(&f, "integrator", "root");
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    f.run(&[
        "register",
        "--role",
        "integrator",
        "--id",
        "root",
        "--session",
        "root-session",
        "--route",
        "human",
        "--lease-seconds",
        "10",
    ]);
    let registered = std::time::Instant::now();
    let before = state_bytes(&f);
    let (bin, fifo) = held_git(&f);
    let publish = f.coord(&[
        "publish",
        "--id",
        "root",
        "--session",
        "root-session",
        "--base",
        &base,
        "--shared",
        COVERAGE,
        "--all-participants-upgraded",
    ]);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let blocked = f
        .command(&publish.iter().map(String::as_str).collect::<Vec<_>>())
        .env("PATH", path)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // Only the integrator's real availability lapses while its base validation is held.
    std::thread::sleep(std::time::Duration::from_secs(12).saturating_sub(registered.elapsed()));
    // Opening the FIFO for writing waits for the held reader, then releases it.
    drop(fs::OpenOptions::new().write(true).open(&fifo).unwrap());
    let output = blocked.wait_with_output().unwrap();
    assert!(!output.status.success(), "an expired integrator published");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("register or renew the exact integrator session"),
        "{stderr}"
    );
    assert_eq!(state_bytes(&f), before, "no schema five or shared policy");
    // The same generation renews, then publishes the review legitimately.
    register(&f, "integrator", "root");
    succeeded(&output_of(&f, &publish));
    let state = f.run(&["status"]).state;
    assert_eq!(state.version, 5);
    assert_eq!(state.integration.unwrap().shared, [COVERAGE]);
}

#[cfg(unix)]
fn output_of(f: &Fixture, args: &[String]) -> std::process::Output {
    f.command(&args.iter().map(String::as_str).collect::<Vec<_>>())
        .output()
        .unwrap()
}

#[test]
fn hs2_yhynj6_undeclared_trailing_whitespace_edits_are_refused() {
    let (f, base) = fixture();
    let ticket = f.ticket();
    register(&f, "integrator", "root");
    integrator(
        &f,
        "authority",
        &["--mode", "dispatcher", "--all-participants-upgraded"],
    );
    integrator(
        &f,
        "publish",
        &[
            "--base",
            &base,
            "--shared",
            COVERAGE,
            "--all-participants-upgraded",
        ],
    );
    register(&f, "dispatcher", "dispatcher");
    register(&f, "worker", "two");
    succeeded(&dispatch(
        &f,
        "dispatcher-session",
        "op-2",
        "two",
        &ticket,
        &base,
        &[
            "src/two.rs",
            "docs/TEST-COVERAGE.md#row:two",
            "docs/TEST-COVERAGE.md#heading:appended-notes",
        ],
    ));
    accept(&f, "two", "op-2", &ticket);
    let declared = row(DOC, "two", "covered");
    for (branch, suffix) in [
        ("two-nbsp", "\u{a0}"),
        ("two-tab", "\t"),
        ("two-space", " "),
    ] {
        let content = declared.replace("- existing\n", &format!("- existing{suffix}\n"));
        let sha = commit(&f, branch, &base, &[(COVERAGE, &content)]);
        refused(
            &f,
            || handoff(&f, "two", "op-2", &ticket, &sha),
            "undeclared sections heading:shared-notes",
        );
        assert_eq!(
            f.run(&["status"]).state.assignments["op-2"].phase,
            Phase::Accepted
        );
    }
    // A declared row plus a declared section appended after the final blank separator.
    let truthful = commit(
        &f,
        "two",
        &base,
        &[(
            COVERAGE,
            &format!("{declared}### Appended notes\n\n- appended\n"),
        )],
    );
    succeeded(&handoff(&f, "two", "op-2", &ticket, &truthful));
}
