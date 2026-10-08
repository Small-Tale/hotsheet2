//! Real-process regression: an IDE launcher may also occupy `agy` on PATH.

#[cfg(unix)]
#[test]
fn native_cli_is_probed_and_ide_launcher_is_never_executed() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::process::Command;

    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    let bin = home.join(".local/bin");
    fs::create_dir_all(&bin).unwrap();
    let marker = temp.path().join("launched");
    let agy = bin.join("agy");
    let ide = format!("#!/bin/sh\nprintf ide >> '{}'\n", marker.display());
    fs::write(&agy, &ide).unwrap();
    fs::set_permissions(&agy, fs::Permissions::from_mode(0o755)).unwrap();

    // Compile a small native fixture before altering PATH. A script cannot masquerade
    // as the documented native CLI merely by setting its executable permission.
    let source = temp.path().join("agent.rs");
    fs::write(&source, r#"
fn main() {
    use std::io::Write;
    let arg = std::env::args().nth(1).unwrap_or_default();
    let marker = std::env::var("AGY_FIXTURE_MARKER").unwrap();
    let mut log = std::fs::OpenOptions::new().create(true).append(true).open(marker).unwrap();
    writeln!(log, "{arg}").unwrap();
    match arg.as_str() {
        "--version" => println!("agy {}", std::env::var("AGY_FIXTURE_VERSION").unwrap_or_else(|_| "1".into())),
        "models" => match std::env::var("AGY_FIXTURE_MODE").unwrap_or_default().as_str() {
            "unsupported" => { eprintln!("models unavailable"); std::process::exit(2); }
            "new" => println!("gemini-new-high    Gemini New (High)"),
            _ => println!("gemini-live-high     Gemini Live (High)\ngemini-live-medium   Gemini Live (Medium)"),
        },
        _ => std::process::exit(2),
    }
}
"#).unwrap();
    let native = temp.path().join("native-agy");
    assert!(
        Command::new("rustc")
            .arg("--edition=2024")
            .arg(&source)
            .arg("-o")
            .arg(&native)
            .status()
            .unwrap()
            .success()
    );

    // This is the only test in its integration binary, so HOME/PATH mutations
    // cannot interfere with parallel unit tests.
    let previous_path = std::env::var_os("PATH");
    let previous_home = std::env::var_os("HOME");
    let previous_trusted_dir = std::env::var_os("HOTSHEET_ANTIGRAVITY_CLI_DIR");
    unsafe {
        std::env::set_var("PATH", &bin);
        std::env::set_var("HOME", &home);
        std::env::remove_var("HOTSHEET_ANTIGRAVITY_CLI_DIR");
        std::env::set_var("AGY_FIXTURE_MARKER", &marker);
    }
    let mut cache = hotsheet_aitools::ModelCatalogCache::default();
    let discover = |cache: &mut hotsheet_aitools::ModelCatalogCache, refresh| {
        hotsheet_aitools::discover_ai_tool_descriptors(&[], temp.path(), cache, refresh)
            .into_iter()
            .find(|tool| tool.id == "antigravity")
            .unwrap()
    };
    for refresh in [false, false, true] {
        let tool = discover(&mut cache, refresh);
        assert_eq!(
            tool.default_model.as_deref(),
            Some("gemini-3.8-flash-medium")
        );
        assert_eq!(tool.models.len(), 7);
        assert!(!marker.exists(), "discovery launched the IDE script");
    }

    fs::remove_file(&agy).unwrap();
    fs::copy(&native, &agy).unwrap();
    fs::set_permissions(&agy, fs::Permissions::from_mode(0o755)).unwrap();
    unsafe { std::env::set_var("AGY_FIXTURE_MODE", "unsupported") };
    assert_eq!(discover(&mut cache, false).models.len(), 7);
    unsafe { std::env::set_var("AGY_FIXTURE_MODE", "live") };
    assert_eq!(
        discover(&mut cache, false).models.len(),
        7,
        "failed catalog is cached"
    );
    assert_eq!(
        fs::read_to_string(&marker)
            .unwrap()
            .matches("models")
            .count(),
        1
    );
    let live = discover(&mut cache, true);
    assert_eq!(
        live.models
            .iter()
            .map(|model| model.id.as_str())
            .collect::<Vec<_>>(),
        ["gemini-live-high", "gemini-live-medium"]
    );
    assert_eq!(
        fs::read_to_string(&marker)
            .unwrap()
            .matches("models")
            .count(),
        2
    );
    assert_eq!(discover(&mut cache, false).models, live.models);
    assert_eq!(
        fs::read_to_string(&marker)
            .unwrap()
            .matches("models")
            .count(),
        2
    );

    unsafe {
        std::env::set_var("AGY_FIXTURE_VERSION", "2");
        std::env::set_var("AGY_FIXTURE_MODE", "new");
    }
    assert_eq!(discover(&mut cache, false).models[0].id, "gemini-new-high");
    fs::remove_file(&agy).unwrap();
    fs::write(&agy, &ide).unwrap();
    fs::set_permissions(&agy, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        discover(&mut cache, false).models.len(),
        7,
        "IDE replacement drops the live catalog"
    );
    assert!(!fs::read_to_string(&marker).unwrap().contains("ide"));

    // The user may explicitly trust a custom installer directory. Discovery uses
    // that exact native binary even when an IDE launcher wins the PATH lookup.
    let custom = temp.path().join("custom/bin");
    fs::create_dir_all(&custom).unwrap();
    fs::copy(&native, custom.join("agy")).unwrap();
    fs::set_permissions(custom.join("agy"), fs::Permissions::from_mode(0o755)).unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_ANTIGRAVITY_CLI_DIR", &custom);
        std::env::set_var("AGY_FIXTURE_VERSION", "3");
    }
    assert_eq!(discover(&mut cache, false).models[0].id, "gemini-new-high");
    assert!(!fs::read_to_string(&marker).unwrap().contains("ide"));

    let other_custom = temp.path().join("other/bin");
    fs::create_dir_all(&other_custom).unwrap();
    fs::copy(&native, other_custom.join("agy")).unwrap();
    fs::set_permissions(other_custom.join("agy"), fs::Permissions::from_mode(0o755)).unwrap();
    unsafe {
        std::env::set_var("HOTSHEET_ANTIGRAVITY_CLI_DIR", &other_custom);
        std::env::set_var("AGY_FIXTURE_MODE", "live");
    }
    assert_eq!(
        discover(&mut cache, false).models[0].id,
        "gemini-live-high",
        "changing the trusted path invalidates a same-version catalog"
    );
    unsafe { std::env::set_var("HOTSHEET_ANTIGRAVITY_CLI_DIR", "relative/bin") };
    assert_eq!(discover(&mut cache, false).models.len(), 7);
    fs::write(other_custom.join("agy"), &ide).unwrap();
    unsafe { std::env::set_var("HOTSHEET_ANTIGRAVITY_CLI_DIR", &other_custom) };
    assert_eq!(discover(&mut cache, false).models.len(), 7);
    assert!(!fs::read_to_string(&marker).unwrap().contains("ide"));
    unsafe { std::env::remove_var("HOTSHEET_ANTIGRAVITY_CLI_DIR") };
    assert_eq!(discover(&mut cache, false).models.len(), 7);

    if let Some(path) = previous_path {
        unsafe { std::env::set_var("PATH", path) };
    } else {
        unsafe { std::env::remove_var("PATH") };
    }
    if let Some(home) = previous_home {
        unsafe { std::env::set_var("HOME", home) };
    } else {
        unsafe { std::env::remove_var("HOME") };
    }
    if let Some(dir) = previous_trusted_dir {
        unsafe { std::env::set_var("HOTSHEET_ANTIGRAVITY_CLI_DIR", dir) };
    }
    for name in [
        "AGY_FIXTURE_MARKER",
        "AGY_FIXTURE_MODE",
        "AGY_FIXTURE_VERSION",
    ] {
        unsafe { std::env::remove_var(name) };
    }
}
