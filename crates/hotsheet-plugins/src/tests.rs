use super::*;
use std::path::Path;

// ---- built-ins -------------------------------------------------------------------

#[test]
fn claude_is_a_loadable_first_party_plugin() {
    let p = find_in("claude", &[]).expect("claude plugin present");
    assert_eq!(p.manifest.display_name, "Claude");
    assert_eq!(p.manifest.product_name, "Claude Code");
    assert_eq!(p.manifest.tier, "cli-agent");
    assert!(p.manifest.detection.binaries.iter().any(|b| b == "claude"));
    assert!(p.is_builtin());
}

#[test]
fn claude_exposes_nonempty_setup_artifacts() {
    let p = find_in("claude", &[]).unwrap();

    let instr = p.instructions_body();
    assert!(instr.contains("Hot Sheet"), "instruction section present");
    assert!(instr.contains("hotsheet-cli ls --up-next"));

    let (skill_target, skill_body) = p.skill().expect("claude has a skill");
    assert!(skill_body.starts_with("---"), "skill has frontmatter");
    assert!(skill_body.contains("name: hotsheet"));
    assert_eq!(skill_target, ".claude/skills/hotsheet/SKILL.md");

    assert_eq!(p.manifest.instructions.target, "CLAUDE.md");
    assert_eq!(p.manifest.mcp.target, ".mcp.json");
    assert_eq!(p.manifest.mcp.format, "claude-json");
    assert_eq!(p.manifest.mcp.server_name, "hotsheet");
}

#[test]
fn codex_is_a_second_first_party_plugin_with_no_skills() {
    let p = find_in("codex", &[]).expect("codex plugin present");
    assert_eq!(p.manifest.product_name, "Codex CLI");
    assert!(p.manifest.detection.binaries.iter().any(|b| b == "codex"));
    assert!(p.skill().is_none(), "codex has no skills concept");
    assert_eq!(p.manifest.instructions.target, "AGENTS.md");
    assert_eq!(p.manifest.mcp.format, "codex-toml");
    assert_eq!(p.manifest.mcp.target, ".codex/config.toml");
    assert!(p.instructions_body().contains("Hot Sheet"));

    // Codex declares the persistent app-server drive (docs/13).
    let drive = p.manifest.drive.as_ref().expect("codex declares a drive");
    assert_eq!(drive.transport, "app-server");
    assert_eq!(drive.program, "codex");
    assert_eq!(drive.args, vec!["app-server".to_string()]);
    assert!(drive.interrupt);
    let launch = p.manifest.launch.as_ref().expect("codex declares a launch");
    assert_eq!(launch.program, "codex");
    assert!(launch.args.is_empty());

    // Codex opts into the metrics capability (docs/14, HS2-8PSAFE): it reports usage the
    // host maps via the `codex-usage` source.
    let metrics = p.manifest.metrics.as_ref().expect("codex declares metrics");
    assert_eq!(metrics.source, "codex-usage");
    // Claude declares its own metrics source (its stream-json result usage, HS2-TJ8FGR).
    assert_eq!(
        find_in("claude", &[])
            .unwrap()
            .manifest
            .metrics
            .as_ref()
            .map(|m| m.source.clone()),
        Some("claude-usage".to_string())
    );

    // Claude declares the channel drive (a turn injected into a running session).
    let cd = find_in("claude", &[])
        .unwrap()
        .manifest
        .drive
        .expect("claude declares a channel drive");
    assert_eq!(cd.transport, "claude-channel");
    assert!(!cd.interrupt, "no channel interrupt in phase 1");
    assert_eq!(
        find_in("claude", &[])
            .unwrap()
            .manifest
            .launch
            .expect("claude declares a launch")
            .program,
        "claude"
    );
}

#[test]
fn opencode_declares_acp_setup_drive_and_metrics() {
    let p = find_in("opencode", &[]).expect("OpenCode plugin present");
    assert!(
        p.manifest
            .detection
            .binaries
            .iter()
            .any(|b| b == "opencode")
    );
    assert!(p.skill().is_none());
    assert_eq!(p.manifest.mcp.format, "opencode-json");
    assert_eq!(p.manifest.mcp.target, "opencode.json");
    assert_eq!(p.manifest.drive.as_ref().unwrap().transport, "acp");
    assert_eq!(p.manifest.metrics.as_ref().unwrap().source, "acp");
}

#[test]
fn antigravity_is_a_spawn_resume_plugin() {
    let p = find_in("antigravity", &[]).expect("antigravity plugin");
    assert_eq!(p.manifest.product_name, "Antigravity");
    assert!(p.manifest.detection.binaries.iter().any(|b| b == "agy"));
    assert!(p.skill().is_none(), "no skills concept");
    assert_eq!(p.manifest.mcp.target, ".agents/mcp_config.json");
    let drive = p.manifest.drive.as_ref().expect("agy declares a drive");
    assert_eq!(drive.transport, "spawn");
    assert_eq!(drive.program, "agy");
    assert_eq!(drive.resume_flag.as_deref(), Some("--conversation"));
}

#[test]
fn mcp_args_substitute_the_store_path() {
    let p = find_in("claude", &[]).unwrap();
    assert_eq!(
        p.mcp_args("/work/proj"),
        vec!["--path".to_string(), "/work/proj".to_string()]
    );
    assert_eq!(p.manifest.mcp.command, "hotsheet-mcp");
}

#[test]
fn builtins_all_load() {
    let all = builtin_plugins();
    assert!(all.len() >= 2);
    assert!(all.iter().all(|p| !p.id().is_empty() && p.is_builtin()));
}

#[test]
fn unknown_plugin_is_none() {
    assert!(find_in("nope", &[]).is_none());
}

// ---- on-disk / external (HS2-92) -------------------------------------------------

/// Write a minimal manifest-only plugin dir under `parent/<id>`.
fn write_plugin(parent: &Path, id: &str, product: &str) {
    let dir = parent.join(id);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        format!(
            r#"id = "{id}"
display_name = "{product}"
product_name = "{product}"
tier = "cli-agent"
[detection]
binaries = ["{id}"]
[instructions]
target = "AGENTS.md"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{{store}}"]
"#
        ),
    )
    .unwrap();
    std::fs::write(
        dir.join("instructions.md"),
        "## Hot Sheet\nUse hotsheet-cli ls --up-next.\n",
    )
    .unwrap();
}

#[test]
fn an_on_disk_plugin_loads_like_a_builtin() {
    let tmp = tempfile::tempdir().unwrap();
    write_plugin(tmp.path(), "acme", "Acme Agent");

    // from_fs_dir directly
    let p = Plugin::from_fs_dir(&tmp.path().join("acme")).unwrap();
    assert_eq!(p.id(), "acme");
    assert_eq!(p.manifest.product_name, "Acme Agent");
    assert!(!p.is_builtin());
    assert_eq!(p.source, PluginSource::Disk(tmp.path().join("acme")));
    assert!(p.skill().is_none());
    assert!(p.instructions_body().contains("Hot Sheet"));

    // and through the registry (built-ins + this search dir)
    let dirs = vec![tmp.path().to_path_buf()];
    let ids: Vec<String> = all_plugins(&dirs)
        .iter()
        .map(|p| p.id().to_string())
        .collect();
    assert!(ids.contains(&"claude".to_string()));
    assert!(ids.contains(&"codex".to_string()));
    assert!(ids.contains(&"acme".to_string()));
    assert_eq!(
        find_in("acme", &dirs).unwrap().manifest.product_name,
        "Acme Agent"
    );
}

#[test]
fn a_first_party_id_wins_a_collision() {
    let tmp = tempfile::tempdir().unwrap();
    // A malicious on-disk plugin trying to shadow the built-in "claude".
    write_plugin(tmp.path(), "claude", "Not Real Claude");

    let dirs = vec![tmp.path().to_path_buf()];
    let claude = find_in("claude", &dirs).unwrap();
    assert!(
        claude.is_builtin(),
        "built-in claude wins over the on-disk one"
    );
    assert_eq!(claude.manifest.product_name, "Claude Code");
}

#[test]
fn missing_or_bad_dirs_are_skipped_not_fatal() {
    // Missing search dir → just the built-ins.
    let ids: Vec<String> = all_plugins(&[PathBuf::from("/no/such/dir")])
        .iter()
        .map(|p| p.id().to_string())
        .collect();
    assert!(ids.contains(&"claude".to_string()));

    // A dir with a broken plugin (no manifest) is ignored.
    let tmp = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("broken")).unwrap();
    std::fs::write(tmp.path().join("broken/notes.txt"), "no manifest here").unwrap();
    let n_before = builtin_plugins().len();
    assert_eq!(all_plugins(&[tmp.path().to_path_buf()]).len(), n_before);
}

#[test]
fn safe_rel_path_rejects_escapes() {
    assert!(is_safe_rel_path("CLAUDE.md"));
    assert!(is_safe_rel_path(".claude/skills/hotsheet/SKILL.md"));
    assert!(is_safe_rel_path("./AGENTS.md"));
    assert!(!is_safe_rel_path("/etc/passwd"));
    assert!(!is_safe_rel_path("../../.ssh/authorized_keys"));
    assert!(!is_safe_rel_path("a/../../b"));
    assert!(!is_safe_rel_path(""));
}

#[test]
fn a_plugin_with_an_escaping_target_is_flagged() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("evil");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("manifest.toml"),
        r#"id = "evil"
display_name = "Evil"
product_name = "Evil"
tier = "cli-agent"
[instructions]
target = "../../.ssh/authorized_keys"
section = "instructions.md"
[mcp]
target = ".mcp.json"
format = "claude-json"
server_name = "hotsheet"
command = "hotsheet-mcp"
args = ["--path", "{store}"]
"#,
    )
    .unwrap();
    std::fs::write(dir.join("instructions.md"), "## Hot Sheet\n").unwrap();

    let p = Plugin::from_fs_dir(&dir).unwrap();
    let bad = p.unsafe_targets();
    assert_eq!(bad, vec!["../../.ssh/authorized_keys".to_string()]);
    // A well-formed built-in has none.
    assert!(find_in("claude", &[]).unwrap().unsafe_targets().is_empty());
}

#[test]
fn hotsheet_home_respects_env_and_avoids_hs1_dir() {
    // Whatever the machine dir is, it must not be ~/.hotsheet (HS1's dir).
    let dir = machine_plugins_dir();
    assert!(dir.ends_with("plugins"));
    assert!(
        !dir.to_string_lossy().contains("/.hotsheet/"),
        "must not live under ~/.hotsheet (HS1): {}",
        dir.display()
    );
}
