//! Real-process regression: an `agy` on PATH may be an IDE launcher.

#[cfg(unix)]
#[test]
fn discovery_never_executes_an_unidentified_agy_even_on_refresh() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    fs::create_dir(&bin).unwrap();
    let marker = temp.path().join("ide-launched");
    let agy = bin.join("agy");
    fs::write(
        &agy,
        format!("#!/bin/sh\nprintf launched > '{}'\n", marker.display()),
    )
    .unwrap();
    fs::set_permissions(&agy, fs::Permissions::from_mode(0o755)).unwrap();
    let opencode = bin.join("opencode");
    fs::write(
        &opencode,
        "#!/bin/sh\ncase \"$1\" in\n  --version) printf 'opencode 1.0\\n' ;;\n  models) printf 'safe-model\\tSafe Model\\n' ;;\nesac\n",
    )
    .unwrap();
    fs::set_permissions(&opencode, fs::Permissions::from_mode(0o755)).unwrap();

    // This integration test is its own test process, so replacing PATH cannot affect
    // another test. Detection only checks for a file; catalog resolution must not run it.
    let original_path = std::env::var_os("PATH");
    unsafe { std::env::set_var("PATH", &bin) };
    let mut cache = hotsheet_aitools::ModelCatalogCache::default();
    for refresh in [false, false, true, true] {
        let tools =
            hotsheet_aitools::discover_ai_tool_descriptors(&[], temp.path(), &mut cache, refresh);
        let antigravity = tools.iter().find(|tool| tool.id == "antigravity").unwrap();
        assert_eq!(
            antigravity.default_model.as_deref(),
            Some("gemini-3.8-flash-medium")
        );
        assert_eq!(antigravity.models.len(), 7);
        assert!(!marker.exists(), "discovery launched agy");
        let safe_provider = tools.iter().find(|tool| tool.id == "opencode").unwrap();
        assert_eq!(safe_provider.models[0].id, "safe-model");
    }

    fs::remove_file(&agy).unwrap();
    assert!(
        hotsheet_aitools::discover_ai_tool_descriptors(&[], temp.path(), &mut cache, true)
            .iter()
            .all(|tool| tool.id != "antigravity")
    );
    if let Some(path) = original_path {
        unsafe { std::env::set_var("PATH", path) };
    } else {
        unsafe { std::env::remove_var("PATH") };
    }
}
