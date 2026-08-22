//! **Plugin conformance suite** (HS2-64, docs/12 §12.7.7) — the hard CI gate every AI-tool
//! plugin must pass. It is **parameterized over the whole registry** (`builtin_plugins()`),
//! so a NEW tool inherits the gate simply by existing: add a plugin dir and it is validated
//! here — identity, detection, instructions, skills (absence-as-feature), MCP config
//! (writes a valid, re-parseable config), drive declaration (resolves to a real host
//! Drive), write-target safety, and a full headless `setup` against a temp fixture project
//! (idempotent). If a plugin is malformed or its setup produces junk, this test fails and
//! the change can't merge.
//!
//! This is the "north-star" acceptance for HS2-64: the maintainer can add another tool
//! (OpenCode/Cursor/…) and know it works without hand-testing, because the suite covers it.
//!
//! Not covered here (tracked separately): the `hs-fake-agent` PTY/permission-protocol E2E +
//! busy-state transition matrix — those need terminals (HS2-10) + the permission bridge
//! (HS2-113), so they land with those (HS2-1GJY50). The drive protocol itself is already
//! fake-tested per transport in
//! `hotsheet-aitools` (scripted daemons).

use hotsheet_plugins::{KNOWN_MCP_FORMATS, Plugin, builtin_plugins, is_safe_rel_path};
use hotsheet_ticketing::{FsStore, StoreMetadata};

/// Transports a plugin may declare — the host must know how to map each to a `Drive`
/// (built or explicitly planned). A declaration outside this set is a conformance failure.
const KNOWN_TRANSPORTS: &[&str] = &["spawn", "claude-channel", "app-server", "acp"];

/// The gate: run every conformance check on every registered plugin. One `#[test]` so a
/// failure names the offending plugin + aspect.
#[test]
fn every_registered_plugin_passes_conformance() {
    let plugins = builtin_plugins();
    assert!(
        !plugins.is_empty(),
        "the registry must have at least one plugin"
    );

    for p in &plugins {
        let id = p.id().to_string();
        check_identity(p, &id);
        check_detection(p, &id);
        check_instructions(p, &id);
        check_skills(p, &id);
        check_mcp(p, &id);
        check_drive(p, &id);
        check_target_safety(p, &id);
        check_headless_setup(p, &id);
    }
}

fn check_identity(p: &Plugin, id: &str) {
    let m = &p.manifest;
    assert!(!m.id.trim().is_empty(), "[{id}] id must be non-empty");
    assert!(
        !m.display_name.trim().is_empty(),
        "[{id}] display_name required"
    );
    assert!(
        !m.product_name.trim().is_empty(),
        "[{id}] product_name required"
    );
    assert!(!m.tier.trim().is_empty(), "[{id}] tier required");
}

fn check_detection(p: &Plugin, id: &str) {
    assert!(
        !p.manifest.detection.binaries.is_empty(),
        "[{id}] must declare at least one detection binary (how the host knows it's installed)"
    );
    for b in &p.manifest.detection.binaries {
        assert!(
            !b.trim().is_empty(),
            "[{id}] detection binary must be non-empty"
        );
    }
}

fn check_instructions(p: &Plugin, id: &str) {
    assert!(
        is_safe_rel_path(&p.manifest.instructions.target),
        "[{id}] instructions target must be a project-relative path: {}",
        p.manifest.instructions.target
    );
    // The loader already checks the section file exists; require it to carry real content.
    assert!(
        !p.instructions_body().trim().is_empty(),
        "[{id}] instructions section is empty"
    );
}

fn check_skills(p: &Plugin, id: &str) {
    // Absence-as-feature: `skill()` is Some iff the manifest declares `[skills]`.
    match (&p.manifest.skills, p.skill()) {
        (Some(_), Some((target, body))) => {
            assert!(
                is_safe_rel_path(target),
                "[{id}] skill target must be project-relative: {target}"
            );
            assert!(
                !body.trim().is_empty(),
                "[{id}] declared skill body is empty"
            );
        }
        (None, None) => {} // a tool with no skills concept (e.g. Codex) — the correct signal
        _ => panic!("[{id}] skills manifest/accessor disagree (absence-as-feature broken)"),
    }
}

fn check_mcp(p: &Plugin, id: &str) {
    let mcp = &p.manifest.mcp;
    assert!(
        KNOWN_MCP_FORMATS.contains(&mcp.format.as_str()),
        "[{id}] unknown MCP format '{}' (known: {KNOWN_MCP_FORMATS:?})",
        mcp.format
    );
    assert!(
        is_safe_rel_path(&mcp.target),
        "[{id}] MCP target must be project-relative: {}",
        mcp.target
    );
    assert!(
        !mcp.server_name.trim().is_empty(),
        "[{id}] MCP server_name required"
    );
    assert!(
        !mcp.command.trim().is_empty(),
        "[{id}] MCP command required"
    );
    // `{store}` substitution must actually take (the shim needs the store path).
    let args = p.mcp_args("/tmp/store");
    assert!(
        args.iter().any(|a| a.contains("/tmp/store")),
        "[{id}] MCP args don't reference the store path after {{store}} substitution: {args:?}"
    );
}

fn check_drive(p: &Plugin, id: &str) {
    let Some(drive) = &p.manifest.drive else {
        // Absent = not drivable (an editor tool, or a transport not built) — allowed.
        assert!(
            hotsheet_aitools::drive_for(p).is_none(),
            "[{id}] declares no drive but the host resolved one"
        );
        return;
    };
    assert!(
        KNOWN_TRANSPORTS.contains(&drive.transport.as_str()),
        "[{id}] unknown drive transport '{}' (known: {KNOWN_TRANSPORTS:?})",
        drive.transport
    );
    assert!(
        !drive.program.trim().is_empty(),
        "[{id}] drive program required"
    );
    // The declaration must resolve to a real host Drive of the matching transport.
    let resolved = hotsheet_aitools::drive_for(p).unwrap_or_else(|| {
        panic!(
            "[{id}] declares drive '{}' but the host can't resolve it",
            drive.transport
        )
    });
    let got = format!("{:?}", resolved.info().transport).to_lowercase();
    let want = drive.transport.replace('-', "");
    assert!(
        got.contains(&want),
        "[{id}] host Drive transport {got:?} doesn't match the declared '{}'",
        drive.transport
    );
}

fn check_target_safety(p: &Plugin, id: &str) {
    let bad = p.unsafe_targets();
    assert!(
        bad.is_empty(),
        "[{id}] declares unsafe write target(s) that escape the project: {bad:?}"
    );
}

/// The load-bearing check: a full headless `setup` for this plugin against a fresh temp
/// project writes exactly its declared artifacts, they're valid, and it's idempotent.
fn check_headless_setup(p: &Plugin, id: &str) {
    let dir = tempfile::tempdir().unwrap();
    FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();

    // Run setup twice — must be idempotent (managed blocks refreshed, not duplicated).
    for _ in 0..2 {
        hotsheet_cli::run_setup(dir.path(), dir.path(), Some(id), false)
            .unwrap_or_else(|e| panic!("[{id}] headless setup failed: {e}"));
    }

    // Instructions: written, with exactly one managed block for this plugin.
    let instr_path = dir.path().join(&p.manifest.instructions.target);
    let instr = std::fs::read_to_string(&instr_path).unwrap_or_else(|_| {
        panic!(
            "[{id}] setup didn't write {}",
            p.manifest.instructions.target
        )
    });
    let marker = format!("<!-- BEGIN hotsheet:{id} -->");
    assert_eq!(
        instr.matches(&marker).count(),
        1,
        "[{id}] expected exactly one managed instruction block after re-running setup"
    );

    // Skill: written iff declared.
    if let Some((target, _)) = p.skill() {
        assert!(
            dir.path().join(target).is_file(),
            "[{id}] declared skill not written: {target}"
        );
    }

    // MCP config: written + parses as its declared format, with the hotsheet server present.
    let mcp_path = dir.path().join(&p.manifest.mcp.target);
    let mcp_text = std::fs::read_to_string(&mcp_path).unwrap_or_else(|_| {
        panic!(
            "[{id}] setup didn't write MCP config {}",
            p.manifest.mcp.target
        )
    });
    assert_valid_mcp(
        id,
        &p.manifest.mcp.format,
        &p.manifest.mcp.server_name,
        &mcp_text,
    );
}

fn assert_valid_mcp(id: &str, format: &str, server_name: &str, text: &str) {
    match format {
        "claude-json" => {
            let v: serde_json::Value = serde_json::from_str(text)
                .unwrap_or_else(|e| panic!("[{id}] MCP JSON invalid: {e}"));
            assert!(
                v["mcpServers"][server_name].is_object(),
                "[{id}] MCP JSON missing mcpServers.{server_name}"
            );
        }
        "codex-toml" => {
            let v: toml::Value =
                toml::from_str(text).unwrap_or_else(|e| panic!("[{id}] MCP TOML invalid: {e}"));
            assert!(
                v.get("mcp_servers")
                    .and_then(|s| s.get(server_name))
                    .is_some(),
                "[{id}] MCP TOML missing mcp_servers.{server_name}"
            );
        }
        other => panic!("[{id}] conformance can't validate MCP format '{other}'"),
    }
}

/// A brand-new plugin dir, loaded from disk (not bundled), must pass the same battery — the
/// gate is inherited by *existing*, exactly as a third-party tool would experience it. We
/// install it on an isolated machine search path so `run_setup` discovers it by id and runs
/// the full headless setup E2E, just like a built-in.
#[test]
fn a_new_on_disk_plugin_inherits_the_gate() {
    let home = tempfile::tempdir().unwrap();
    // SAFETY: nextest runs each test in its own process; setting HOTSHEET_HOME is local.
    unsafe { std::env::set_var("HOTSHEET_HOME", home.path()) };
    let pdir = home.path().join("plugins").join("mytool");
    std::fs::create_dir_all(&pdir).unwrap();
    std::fs::write(
        pdir.join("manifest.toml"),
        r#"id = "mytool"
display_name = "My Tool"
product_name = "My Tool CLI"
tier = "cli-agent"
[detection]
binaries = ["mytool"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mytool/mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
    )
    .unwrap();
    std::fs::write(
        pdir.join("instructions.md"),
        "## Hot Sheet\nWork the queue.\n",
    )
    .unwrap();

    // Loads + resolves on the machine search path, exactly like a bundled plugin.
    let p = hotsheet_plugins::find("mytool").expect("on-disk plugin discovered by id");
    assert!(!p.is_builtin(), "the on-disk plugin is not a built-in");
    let id = p.id().to_string();
    // The same battery a built-in gets — including the full headless setup E2E.
    check_identity(&p, &id);
    check_detection(&p, &id);
    check_instructions(&p, &id);
    check_skills(&p, &id);
    check_mcp(&p, &id);
    check_drive(&p, &id);
    check_target_safety(&p, &id);
    check_headless_setup(&p, &id);
}
