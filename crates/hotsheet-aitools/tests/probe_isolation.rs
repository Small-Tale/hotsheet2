//! HS2-7W3D7V: tests never execute the developer's installed AI tools.

#[cfg(unix)]
#[test]
fn hs2_7w3d7v_test_discovery_detects_tools_without_executing_them() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    // `.cargo/config.toml` forces the switch for every process cargo or nextest starts;
    // removing that guard fails here before any tool could run.
    assert_eq!(
        std::env::var(hotsheet_aitools::AI_TOOL_PROBES_ENV).as_deref(),
        Ok("off")
    );
    assert!(!hotsheet_aitools::ai_tool_probes_enabled());

    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let hits = temp.path().join("hits");
    for tool in ["codex", "opencode"] {
        let script = bin.join(tool);
        fs::write(
            &script,
            format!(
                "#!/bin/sh\necho \"{tool} $*\" >> '{}'\nexit 1\n",
                hits.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    }
    // This is the only test in its integration binary, so the PATH change cannot
    // interfere with parallel tests.
    unsafe { std::env::set_var("PATH", &bin) };
    let discover = || {
        let mut cache = hotsheet_aitools::ModelCatalogCache::default();
        hotsheet_aitools::discover_ai_tool_descriptors(&[], temp.path(), &mut cache, true)
    };

    let offline = discover();
    assert!(offline.iter().any(|tool| tool.id == "opencode"));
    let codex = offline.iter().find(|tool| tool.id == "codex").unwrap();
    assert!(!codex.models.is_empty(), "Codex keeps its bundled catalog");
    assert!(!hits.exists(), "discovery executed an installed tool");

    unsafe { std::env::remove_var(hotsheet_aitools::AI_TOOL_PROBES_ENV) };
    assert!(hotsheet_aitools::ai_tool_probes_enabled());
    discover();
    let probed = fs::read_to_string(&hits).unwrap();
    assert!(probed.contains("codex --version"), "{probed}");
    assert!(probed.contains("opencode --version"), "{probed}");

    unsafe { std::env::set_var(hotsheet_aitools::AI_TOOL_PROBES_ENV, "on") };
    assert!(
        hotsheet_aitools::ai_tool_probes_enabled(),
        "only `off` disables probes"
    );
}
