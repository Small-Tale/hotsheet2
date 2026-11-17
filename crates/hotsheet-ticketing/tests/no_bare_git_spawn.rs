//! HS2-RRD417 lint: every `git` subprocess under `crates/` goes through
//! `hotsheet_ticketing::git::command()`, which strips the inherited repository-locating
//! `GIT_*` environment. A bare `Command::new` of git anywhere else could let a hook's
//! `GIT_DIR` redirect Hot Sheet into the wrong repository.

use std::path::{Path, PathBuf};

/// The only file allowed to spawn `git` directly.
const SANCTIONED: &str = "hotsheet-ticketing/src/git.rs";

#[test]
fn hs2_sfvwn5_release_and_default_artifacts_refuse_fixture_activation() {
    let available = hotsheet_ticketing::secrets::test_credentials_available();
    assert_eq!(
        available,
        cfg!(all(feature = "test-credentials", debug_assertions))
    );
    if !available {
        assert!(hotsheet_ticketing::secrets::enable_test_credentials().is_err());
    }
}

#[test]
fn hs2_sfvwn5_synthetic_credentials_require_compiled_debug_and_explicit_activation() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap();
    let source =
        std::fs::read_to_string(root.join("crates/hotsheet-ticketing/src/secrets.rs")).unwrap();
    assert!(source.contains(
        "#[cfg(all(feature = \"test-credentials\", debug_assertions))]\nmod test_credentials;"
    ));
    assert_eq!(source.matches("if test_credentials::enabled()").count(), 3);
    let fixture = std::fs::read_to_string(
        root.join("crates/hotsheet-ticketing/src/secrets/test_credentials.rs"),
    )
    .unwrap();
    assert!(fixture.contains("AtomicBool::new(false)"));
    assert!(!fixture.contains("platform_get("));
    assert!(!fixture.contains("platform_set("));
    assert!(!fixture.contains("platform_delete("));
    assert!(!fixture.contains("var_os(\"HOME\")"));
    assert!(source.contains("#[ignore = \"native: explicitly opt in on a disposable Windows credential store (HS2-SFVWN5)\"]"));
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if path.is_dir() {
            if name != "target" && !name.starts_with('.') {
                rust_sources(&path, out);
            }
        } else if name.ends_with(".rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_bare_git_command_outside_the_shared_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    assert!(files.len() > 50, "scanned too few sources: {}", files.len());

    // Built at runtime so this file does not match itself.
    let needle = format!("Command::new({:?})", "git");
    let mut offenders = Vec::new();
    for file in files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        if rel.replace('\\', "/") == SANCTIONED {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        // Ignore whitespace so padded and line-wrapped spellings count too.
        let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        if compact.contains(&needle) {
            offenders.push(rel);
        }
    }
    assert!(
        offenders.is_empty(),
        "spawn git via hotsheet_ticketing::git::command() (or command_in), not a bare \
         {needle}: {offenders:?}"
    );
}

/// Crates whose production code launches processes for users and agents (HS2-J79CZF).
const LAUNCHING_CRATES: &[&str] = &[
    "hotsheet-aitools/src",
    "hotsheet-server/src",
    "hotsheet-cli/src",
    "hotsheet-terminals/src",
];

/// Signal-only helpers that never run git or user code, so they may inherit the env.
const ALLOWED_BARE: &[&str] = &["\"kill\"", "\"taskkill\""];

/// `text` with every inline `#[cfg(test)]` / `#[cfg(all(test, ...))]` module body removed.
fn without_test_modules(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("#[cfg(") {
        let attribute_end = rest[at..].find(']').map_or(rest.len(), |end| at + end + 1);
        let attribute = &rest[at..attribute_end];
        let after = rest[attribute_end..].trim_start();
        if !(attribute.contains("test") && after.starts_with("mod ")) {
            out.push_str(&rest[..attribute_end]);
            rest = &rest[attribute_end..];
            continue;
        }
        out.push_str(&rest[..at]);
        let body = rest.len() - after.len();
        let Some(open) = rest[body..].find(['{', ';']).map(|i| body + i) else {
            return out;
        };
        if rest.as_bytes()[open] == b';' {
            // `mod name_tests;` lives in its own file, which the scan reads separately.
            rest = &rest[open + 1..];
            continue;
        }
        let mut depth = 0usize;
        let mut close = rest.len();
        for (offset, ch) in rest[open..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        close = open + offset + 1;
                        break;
                    }
                }
                _ => {}
            }
        }
        rest = &rest[close..];
    }
    out.push_str(rest);
    out
}

#[test]
fn test_module_stripping_keeps_production_code() {
    let source = "fn a() {}\n#[cfg(test)]\nmod tests { fn b() { x(); } }\nfn c() {}\n\
                  #[cfg(all(test, unix))]\nmod more { }\n#[cfg(unix)]\nfn d() {}\n";
    let kept = without_test_modules(source);
    assert!(kept.contains("fn a()") && kept.contains("fn c()") && kept.contains("fn d()"));
    assert!(!kept.contains("fn b()") && !kept.contains("mod more"));
}

#[test]
fn launching_crates_start_processes_through_the_scrubbed_launch_constructor() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    for dir in LAUNCHING_CRATES {
        rust_sources(&crates.join(dir), &mut files);
    }
    assert!(files.len() > 20, "scanned too few sources: {}", files.len());

    // Built at runtime so this file does not match itself.
    let needle = format!("{}::new(", "Command");
    let mut offenders = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        // Production code only: inline test modules may spawn fixtures directly.
        let compact: String = without_test_modules(&text)
            .chars()
            .filter(|c| !c.is_whitespace())
            .collect();
        for (at, _) in compact.match_indices(&needle) {
            // `CommandBuilder::new(` (the PTY) scrubs through `scrub_env` instead.
            if compact[..at].ends_with("Builder") {
                continue;
            }
            let rest = &compact[at + needle.len()..];
            if ALLOWED_BARE.iter().any(|allowed| rest.starts_with(allowed)) {
                continue;
            }
            offenders.push(format!(
                "{}: {}",
                file.strip_prefix(crates).unwrap().display(),
                &compact[at..(at + needle.len() + 24).min(compact.len())]
            ));
        }
    }
    assert!(
        offenders.is_empty(),
        "launch processes via hotsheet_ticketing::git::launch() so inherited GIT_DIR and \
         friends are dropped (HS2-J79CZF): {offenders:?}"
    );
}

/// HS2-E5B0K0: how a production spawn site that touches a fork trigger is settled.
/// `Resolved` sites start their program through `git::launch_resolved` or
/// `git::program_path`; `Safe` ones cannot take the fork path for the stated reason;
/// `Forks` ones need `fork` by design and name the ticket that tracks it.
enum Verdict {
    Resolved,
    Safe(&'static str),
    /// HS2-Y4VDBK: launched through the journal's detached launch, which on macOS is a
    /// `posix_spawn` with attributes (descriptor closing, session, process group), never
    /// a fork; off macOS the standard library path is unchanged.
    Spawned,
    Forks(&'static str),
}

/// HS2-E5B0K0: every production source file with a fork trigger, its exact trigger counts,
/// and its verdict. The standard library starts a child with `fork` + `exec` (not
/// `posix_spawn`) for a bare name under a changed `PATH` (`env_clear`, a `PATH` set or
/// removed, or any variable list that may carry one), a relative path under a changed
/// working directory, a `pre_exec` hook (the journal's detached launches use one), or an
/// identity change. On macOS a forked child of a multi-threaded process can abort before
/// `exec` (HS2-H1EK8R). A new trigger fails this test until its site is settled here.
const FORK_TRIGGER_INVENTORY: &[(&str, &str, Verdict)] = &[
    (
        "hotsheet-aitools/src/appserver.rs",
        "env_dynamic=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-aitools/src/launch_safety.rs",
        "cwd=1",
        Verdict::Safe("bare `git` without a PATH change"),
    ),
    (
        "hotsheet-aitools/src/model_catalog.rs",
        "cwd=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-aitools/src/probe.rs",
        "env_dynamic=1",
        Verdict::Safe("test-only helper: the absolute test executable, a constant variable"),
    ),
    (
        "hotsheet-aitools/src/procio.rs",
        "cwd=1 env_dynamic=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-aitools/src/system.rs",
        "cwd=1 env_dynamic=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-cli/src/coordination/development_strategy.rs",
        "cwd=1",
        Verdict::Safe("bare `git` without a PATH change"),
    ),
    (
        "hotsheet-cli/src/dispatch/claim_process.rs",
        "env_dynamic=1",
        Verdict::Safe("the absolute current executable; a constant non-PATH variable"),
    ),
    (
        "hotsheet-cli/src/dispatch/setup.rs",
        "cwd=1 env_dynamic=1",
        Verdict::Safe("launch_safety::resolve_program returns an absolute path"),
    ),
    (
        "hotsheet-cli/src/dispatch/store.rs",
        "cwd=1",
        Verdict::Safe("git::command(): bare `git` without a PATH change"),
    ),
    (
        "hotsheet-cli/src/lib.rs",
        "cwd=2",
        Verdict::Safe("git::command(): bare `git` without a PATH change"),
    ),
    (
        "hotsheet-cli/src/orchestration/job_monitor.rs",
        "detached=1 env_dynamic=1",
        Verdict::Spawned,
    ),
    (
        "hotsheet-cli/src/orchestration/validation.rs",
        "cwd=1 env_clear=1 env_dynamic=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-plugins/src/guidance_sources.rs",
        "cwd=1",
        Verdict::Safe("bare `rulesync` without a PATH change"),
    ),
    (
        "hotsheet-plugins/src/rulesync_updates.rs",
        "cwd=1",
        Verdict::Safe("a canonical absolute executable"),
    ),
    (
        "hotsheet-process-journal/src/lib.rs",
        "detached=3 env_clear=1 env_dynamic=1 pre_exec=2",
        Verdict::Safe(
            "the detached launch itself: posix_spawn attributes on macOS; off macOS the std path keeps its setsid and descriptor-closing pre_exec hooks and its env_clear, unchanged",
        ),
    ),
    (
        "hotsheet-server/src/code_review.rs",
        "detached=1",
        Verdict::Spawned,
    ),
    (
        "hotsheet-server/src/commands.rs",
        "cwd=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-server/src/repository_browser.rs",
        "detached=1",
        Verdict::Spawned,
    ),
    (
        "hotsheet-server/src/terminal_broker.rs",
        "detached=1",
        Verdict::Spawned,
    ),
    (
        "hotsheet-server/src/terminal_launch_relay.rs",
        "cwd=1 detached=1",
        Verdict::Spawned,
    ),
    (
        "hotsheet-terminals/src/bootstrap.rs",
        "pre_exec=1",
        Verdict::Safe("test-only hook (null bootstrap port), never in a shipped launch"),
    ),
    (
        "hotsheet-terminals/src/terminal.rs",
        "env_clear=1 env_dynamic=1 pty=1",
        Verdict::Forks(
            "a PTY child needs setsid and a controlling terminal, which only fork gives",
        ),
    ),
    (
        "hotsheet-ticketing/src/git.rs",
        "cwd=1 env_dynamic=1",
        Verdict::Resolved,
    ),
    (
        "hotsheet-ticketing/src/repository_compaction.rs",
        "cwd=1",
        Verdict::Safe("git::command_in(): bare `git` without a PATH change"),
    ),
];

/// Fork-trigger counts of compacted production `text`, as `name=count` sorted by name.
fn fork_triggers(compact: &str) -> String {
    let count = |needle: &str| compact.matches(needle).count();
    let followed = |needle: &str, not: char| {
        compact
            .match_indices(needle)
            .filter(|(at, _)| !compact[at + needle.len()..].starts_with(not))
            .count()
    };
    let counts = [
        ("cwd", count(".current_dir(")),
        (
            "detached",
            count(".detached(")
                + count(".spawn_detached(")
                + count(".detached_as(")
                + count(".spawn_detached_as("),
        ),
        ("env_clear", count(".env_clear()")),
        ("env_dynamic", followed(".env(", '"') + count(".envs(")),
        (
            "env_path",
            count(".env(\"PATH\"") + count(".env_remove(\"PATH\""),
        ),
        (
            "identity",
            followed(".uid(", ')') + followed(".gid(", ')') + followed(".groups(", ')'),
        ),
        ("pre_exec", count(".pre_exec(")),
        ("pty", count(".spawn_command(")),
    ];
    counts
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(name, n)| format!("{name}={n}"))
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn hs2_e5b0k0_fork_trigger_counting_sees_each_trigger() {
    let compact = ".current_dir(a).env(\"PATH\",p).env(k,v).envs(x).env(\"K\",v).pre_exec(f)\
                   .spawn_detached(c).detached_as(d,e).uid(1)metadata.uid().spawn_command(b).env_clear()";
    assert_eq!(
        fork_triggers(compact),
        "cwd=1 detached=2 env_clear=1 env_dynamic=2 env_path=1 identity=1 pre_exec=1 pty=1"
    );
    assert_eq!(fork_triggers("git::command().args(a).output()"), "");
}

#[test]
fn hs2_e5b0k0_every_fork_trigger_is_resolved_safe_or_a_tracked_fork() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut files = Vec::new();
    rust_sources(crates, &mut files);
    let mut found = std::collections::BTreeMap::new();
    for file in files {
        let rel = file
            .strip_prefix(crates)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let name = rel.rsplit('/').next().unwrap_or("");
        if !rel.contains("/src/")
            || rel.contains("/tests/")
            || name == "tests.rs"
            || name.ends_with("_tests.rs")
        {
            continue;
        }
        let text = std::fs::read_to_string(&file).unwrap();
        let code: String = without_test_modules(&text)
            .lines()
            .filter(|line| !line.trim_start().starts_with("//"))
            .collect::<Vec<_>>()
            .join("\n");
        let compact: String = code.chars().filter(|c| !c.is_whitespace()).collect();
        let triggers = fork_triggers(&compact);
        if !triggers.is_empty() {
            found.insert(rel, (triggers, compact));
        }
    }
    let mut problems = Vec::new();
    for (file, expected, verdict) in FORK_TRIGGER_INVENTORY {
        match found.remove(*file) {
            None => problems.push(format!("{file}: inventoried but has no fork trigger")),
            Some((actual, _)) if actual != *expected => {
                problems.push(format!(
                    "{file}: triggers `{actual}`, inventoried `{expected}`"
                ));
            }
            Some((_, compact)) => match verdict {
                Verdict::Resolved
                    if !compact.contains("launch_resolved(")
                        && !compact.contains("program_path(") =>
                {
                    problems.push(format!("{file}: resolved, yet calls neither git::launch_resolved nor git::program_path"));
                }
                Verdict::Spawned
                    if !(compact.contains(".detached(")
                        || compact.contains(".detached_as(")
                        || compact.contains(".spawn_detached(")
                        || compact.contains(".spawn_detached_as(")) =>
                {
                    problems.push(format!(
                        "{file}: spawned, yet not through the journal's detached launch"
                    ));
                }
                Verdict::Safe(reason) | Verdict::Forks(reason) if reason.is_empty() => {
                    problems.push(format!("{file}: needs a reason"));
                }
                _ => {}
            },
        }
    }
    for (file, (triggers, _)) in found {
        problems.push(format!(
            "{file}: new fork trigger `{triggers}`; start the program through \
             git::launch_resolved (or settle it in FORK_TRIGGER_INVENTORY)"
        ));
    }
    assert!(problems.is_empty(), "HS2-E5B0K0: {problems:#?}");
    // The inventory only shrinks: a fork by design is removed, never added silently.
    let forks = FORK_TRIGGER_INVENTORY
        .iter()
        .filter(|(_, _, verdict)| matches!(verdict, Verdict::Forks(_)))
        .count();
    assert_eq!(
        forks, 1,
        "fork-by-design sites: only the PTY, which needs a controlling terminal"
    );
    // HS2-Y4VDBK: the detached launch really is a posix_spawn with these attributes on macOS.
    let journal =
        std::fs::read_to_string(crates.join("hotsheet-process-journal/src/lib.rs")).unwrap();
    for needle in [
        "#[cfg(target_os = \"macos\")]\nmod spawn_attributes",
        "libc::posix_spawn(",
        "POSIX_SPAWN_CLOEXEC_DEFAULT",
        "POSIX_SPAWN_SETSID",
        "POSIX_SPAWN_SETPGROUP",
    ] {
        assert!(
            journal.contains(needle),
            "HS2-Y4VDBK: the macOS detached launch lost `{needle}`"
        );
    }
}
