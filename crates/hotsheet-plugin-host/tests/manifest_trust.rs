//! HS2-XY0VTN: code-bearing manifest validation and the approval store (docs/38 §38.3,
//! §38.7).

use std::path::Path;

use hotsheet_plugin_host::manifest::{SettingKind, is_code_bearing, platform_key};
use hotsheet_plugin_host::{CodePluginManifest, TrustError, TrustStore};

const VALID: &str = r#"
id = "github"
display_name = "GitHub Issues"
version = "1.0.0"
protocol = { min = 1, max = 2 }
capabilities = ["ticket_source"]

[executable]
program = "bin/hs-plugin-github"
args = ["--quiet"]

[executable.targets]
"windows-x86_64" = "bin/hs-plugin-github.exe"

[[settings]]
key = "token"
label = "Access token"
type = "secret"
required = true

[[settings]]
key = "flavor"
label = "Flavor"
type = "choice"
choices = ["cloud", "server"]
"#;

#[test]
fn hs2_xy0vtn_a_valid_manifest_parses() {
    let manifest = CodePluginManifest::parse(VALID, "github").expect("valid");
    assert_eq!(manifest.id, "github");
    assert_eq!(manifest.protocol, (1, 2));
    assert_eq!(manifest.capabilities, vec!["ticket_source"]);
    assert_eq!(manifest.executable.args, vec!["--quiet"]);
    assert_eq!(manifest.settings[0].kind, SettingKind::Secret);
    assert!(is_code_bearing(VALID));
    assert!(!is_code_bearing(
        "id = \"claude\"\n[instructions]\nfile = \"CLAUDE.md\"\n"
    ));
    assert!(!is_code_bearing("not toml ["));
}

#[test]
fn hs2_xy0vtn_invalid_manifests_are_rejected_with_a_reason() {
    let cases: Vec<(String, &str, &str)> = vec![
        (VALID.to_string(), "other", "directory name"),
        (
            VALID.replace("id = \"github\"", "id = \"Git Hub\""),
            "Git Hub",
            "invalid plugin id",
        ),
        (
            VALID.replace("min = 1, max = 2", "min = 2, max = 3"),
            "github",
            "speaks protocol",
        ),
        (
            VALID.replace("min = 1, max = 2", "min = 2, max = 1"),
            "github",
            "is empty",
        ),
        (
            VALID.replace("[\"ticket_source\"]", "[]"),
            "github",
            "at least one capability",
        ),
        (
            VALID.replace("[\"ticket_source\"]", "[\"telepathy\"]"),
            "github",
            "unknown capability",
        ),
        (
            VALID.replace("bin/hs-plugin-github\"\nargs", "/usr/bin/evil\"\nargs"),
            "github",
            "relative path",
        ),
        (
            VALID.replace("bin/hs-plugin-github.exe", "../escape.exe"),
            "github",
            "relative path",
        ),
        (
            VALID.replace("choices = [\"cloud\", \"server\"]", ""),
            "github",
            "choices are required",
        ),
        (
            VALID.replace("key = \"flavor\"", "key = \"token\""),
            "github",
            "repeated",
        ),
        (
            VALID.replace("version = \"1.0.0\"", "version = \"\""),
            "github",
            "required",
        ),
        (
            format!("{VALID}\nsurprise = true\n"),
            "github",
            "invalid manifest",
        ),
    ];
    for (text, dir, expected) in cases {
        let error = CodePluginManifest::parse(&text, dir).expect_err(expected);
        assert!(
            error.contains(expected),
            "expected {expected:?} in {error:?}"
        );
    }
}

fn plugin_dir(root: &Path, program: &str, executable: bool) -> std::path::PathBuf {
    let dir = root.join("github");
    std::fs::create_dir_all(dir.join("bin")).expect("dir");
    std::fs::write(dir.join("manifest.toml"), VALID).expect("manifest");
    let path = dir.join(program);
    std::fs::write(&path, "#!/bin/sh\n").expect("program");
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).expect("mode");
    }
    #[cfg(not(unix))]
    let _ = executable;
    dir
}

#[test]
fn hs2_xy0vtn_the_program_resolves_inside_the_plugin_directory() {
    let root = tempfile::tempdir().expect("temp");
    let dir = plugin_dir(root.path(), "bin/hs-plugin-github", true);
    let manifest = CodePluginManifest::load(&dir).expect("load");
    if platform_key() != "windows-x86_64" {
        assert_eq!(
            manifest.resolve_program(&dir).expect("resolves"),
            dir.join("bin/hs-plugin-github")
        );
    }
    std::fs::remove_file(dir.join("bin/hs-plugin-github")).expect("remove");
    if platform_key() != "windows-x86_64" {
        assert!(manifest.resolve_program(&dir).is_err(), "missing program");
    }
}

#[cfg(unix)]
#[test]
fn hs2_xy0vtn_a_program_without_the_exec_bit_is_refused() {
    let root = tempfile::tempdir().expect("temp");
    let dir = plugin_dir(root.path(), "bin/hs-plugin-github", false);
    let manifest = CodePluginManifest::load(&dir).expect("load");
    let error = manifest.resolve_program(&dir).expect_err("not executable");
    assert!(error.contains("not executable"), "{error}");
}

#[test]
fn hs2_xy0vtn_approvals_cover_exact_contents_and_can_be_revoked() {
    let root = tempfile::tempdir().expect("temp");
    let home = root.path().join("home");
    let dir = plugin_dir(root.path(), "bin/hs-plugin-github", true);
    let store = TrustStore::new(&home);
    assert!(matches!(
        store.check("github", &dir),
        Err(TrustError::NotApproved { .. })
    ));
    let digest = store.approve("github", &dir).expect("approve");
    assert_eq!(
        store
            .approval("github")
            .expect("read")
            .expect("present")
            .digest,
        digest
    );
    store.check("github", &dir).expect("matches");
    // Another plugin's approval is independent.
    assert!(matches!(
        store.check("jira", &dir),
        Err(TrustError::NotApproved { .. })
    ));
    std::fs::write(
        dir.join("bin/hs-plugin-github"),
        "#!/bin/sh\necho changed\n",
    )
    .expect("edit");
    match store.check("github", &dir) {
        Err(TrustError::Changed {
            approved, current, ..
        }) => {
            assert_eq!(approved, digest);
            assert_ne!(current, digest);
        }
        other => panic!("expected Changed, got {other:?}"),
    }
    assert!(store.revoke("github").expect("revoke"));
    assert!(!store.revoke("github").expect("revoke again"));
    assert!(matches!(
        store.check("github", &dir),
        Err(TrustError::NotApproved { .. })
    ));
}

#[test]
fn hs2_xy0vtn_an_unknown_approvals_schema_is_refused() {
    let root = tempfile::tempdir().expect("temp");
    let home = root.path().join("home");
    let store = TrustStore::new(&home);
    std::fs::create_dir_all(store.path().parent().expect("parent")).expect("dir");
    std::fs::write(store.path(), r#"{"schema": 99, "approvals": {}}"#).expect("write");
    assert!(matches!(
        store.approval("github"),
        Err(TrustError::Unreadable(_))
    ));
    std::fs::write(store.path(), "not json").expect("write");
    assert!(matches!(
        store.approval("github"),
        Err(TrustError::Unreadable(_))
    ));
}
