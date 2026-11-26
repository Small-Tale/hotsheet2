//! HS2-BNDRYP: AI-tool discovery never runs an installed tool in a ticket store.
//!
//! `agy models` starts the MCP servers registered at its user scope in its working
//! directory, and HS1's channel server then wrote `.hotsheet/mcp.log` and `channel-port*`
//! into a newly hosted store, failing the HS1 backup readiness check. The fixture tool
//! here does the same: it records its working directory and writes a synthetic
//! `.hotsheet/mcp.log` there.

#[cfg(unix)]
#[test]
fn hs2_bndryp_discovery_probes_run_in_a_private_workspace_never_the_store() {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    let temp = tempfile::tempdir().unwrap();
    let bin = temp.path().join("bin");
    let store = temp.path().join("store");
    fs::create_dir(&bin).unwrap();
    fs::create_dir(&store).unwrap();
    let hits = temp.path().join("hits");
    let tool = bin.join("opencode");
    fs::write(
        &tool,
        format!(
            "#!/bin/sh\n\
             echo \"$1|$(pwd -P)\" >> '{hits}'\n\
             mkdir -p .hotsheet && echo synthetic >> .hotsheet/mcp.log\n\
             case \"$1\" in\n\
             --version) echo 'opencode 1.0' ;;\n\
             models) echo 'synthetic/model-a' ;;\n\
             *) exit 2 ;;\n\
             esac\n",
            hits = hits.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&tool, fs::Permissions::from_mode(0o755)).unwrap();

    // This is the only test in its integration binary, so the PATH, probe switch and
    // working-directory changes cannot interfere with parallel tests. The CLI used to
    // probe in its working directory and the server in the hosted store's root.
    let previous_dir = std::env::current_dir().unwrap();
    unsafe {
        std::env::set_var("PATH", &bin);
        std::env::remove_var(hotsheet_aitools::AI_TOOL_PROBES_ENV);
    }
    std::env::set_current_dir(&store).unwrap();
    let mut cache = hotsheet_aitools::ModelCatalogCache::default();
    let tools = hotsheet_aitools::discover_ai_tool_descriptors(&[], &mut cache, true);
    std::env::set_current_dir(previous_dir).unwrap();
    unsafe { std::env::set_var(hotsheet_aitools::AI_TOOL_PROBES_ENV, "off") };

    let opencode = tools.iter().find(|tool| tool.id == "opencode").unwrap();
    assert!(
        opencode
            .models
            .iter()
            .any(|model| model.id == "synthetic/model-a"),
        "the probe ran and its catalog was read"
    );
    let probes = fs::read_to_string(&hits).unwrap();
    let mut args = Vec::new();
    let store_real = hotsheet_sync::paths::canonicalize(&store).unwrap();
    for line in probes.lines() {
        let (arg, cwd) = line.split_once('|').unwrap();
        args.push(arg);
        let cwd = Path::new(cwd);
        assert_ne!(cwd, store_real, "a probe ran in the store: {probes}");
        assert!(
            cwd.file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("hotsheet-ai-probe-"),
            "{probes}"
        );
        assert!(
            !cwd.exists(),
            "the probe workspace and what the tool wrote there are removed"
        );
    }
    assert_eq!(args, ["--version", "models"]);
    assert_eq!(
        fs::read_dir(&store).unwrap().count(),
        0,
        "nothing a probe started wrote into the store"
    );
}
