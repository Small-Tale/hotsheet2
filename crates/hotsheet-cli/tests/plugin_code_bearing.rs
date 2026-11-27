//! HS2-XY0VTN: the headless lifecycle of a code-bearing plugin (docs/38 §38.3, §38.7):
//! `plugin install` discloses and trusts, `list`/`info` show trust, `verify` starts the
//! plugin over both transports, a changed file needs `plugin trust` again, and `remove`
//! drops the approval. The AI-tool loader leaves code-bearing directories alone.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn cli(home: &Path, args: &[&str]) -> Output {
    Command::new(workspace_bin::path("hotsheet-cli"))
        .env("HOTSHEET_HOME", home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .args(args)
        .output()
        .expect("run hotsheet-cli")
}

fn text(out: &Output) -> (String, String) {
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
    )
}

/// A source plugin directory named unlike its id, holding a real copy of the
/// diagnostics plugin binary.
fn source_plugin(root: &Path, capability: &str) -> PathBuf {
    let src = root.join("build-output");
    std::fs::create_dir_all(src.join("bin")).expect("dir");
    let exe = workspace_bin::path("hs-plugin-diagnostics");
    let name = exe
        .file_name()
        .expect("name")
        .to_string_lossy()
        .into_owned();
    std::fs::copy(&exe, src.join("bin").join(&name)).expect("copy binary");
    std::fs::write(
        src.join("manifest.toml"),
        format!(
            "id = \"diag\"\ndisplay_name = \"Diagnostics\"\nversion = \"1.0.0\"\nprotocol = {{ min = 1, max = 1 }}\ncapabilities = [\"{capability}\"]\n\n[executable]\nprogram = \"bin/{name}\"\n\n[[settings]]\nkey = \"token\"\nlabel = \"Token\"\ntype = \"secret\"\nrequired = true\n"
        ),
    )
    .expect("manifest");
    std::fs::write(src.join("README.md"), "docs travel with the plugin\n").expect("readme");
    src
}

#[test]
fn hs2_xy0vtn_install_list_verify_retrust_and_remove_a_code_bearing_plugin() {
    let root = tempfile::tempdir().expect("temp");
    let home = root.path().join("home");
    let src = source_plugin(root.path(), "diagnostics");

    let out = cli(
        &home,
        &["plugin", "install", src.to_str().expect("utf8"), "--yes"],
    );
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    for disclosed in [
        "code-bearing plugin",
        "capabilities: diagnostics",
        "token (secret, required)",
        "digest:",
        "not sandboxed",
    ] {
        assert!(
            stdout.contains(disclosed),
            "install disclosure lacks {disclosed:?}:\n{stdout}"
        );
    }
    assert!(
        stdout.contains("Installed and trusted plugin 'diag'"),
        "{stdout}"
    );
    let installed = home.join("plugins/diag");
    assert!(
        installed.join("README.md").is_file(),
        "subdirectories and files are copied"
    );
    assert!(home.join("plugins/approvals.json").is_file());

    // `list` shows it with its trust state; the AI-tool loader does not call it broken.
    let (stdout, stderr) = text(&cli(&home, &["plugin", "list"]));
    let row = stdout
        .lines()
        .find(|l| l.starts_with("diag "))
        .unwrap_or_else(|| panic!("no row:\n{stdout}"));
    assert!(row.contains("trusted"), "{row}");
    assert!(
        !stderr.contains("diag"),
        "no skipped-plugin warning: {stderr}"
    );

    let out = cli(&home, &["plugin", "verify", "diag"]);
    let (stdout, stderr) = text(&out);
    assert!(out.status.success(), "{stdout}\n{stderr}");
    assert!(
        stdout.contains("speaks json") && stdout.contains("speaks protobuf"),
        "{stdout}"
    );

    // A changed file revokes trust until the user trusts it again.
    std::fs::write(installed.join("README.md"), "edited after install\n").expect("edit");
    let (stdout, _) = text(&cli(&home, &["plugin", "list"]));
    assert!(
        stdout
            .lines()
            .any(|l| l.starts_with("diag ") && l.contains("changed")),
        "{stdout}"
    );
    let out = cli(&home, &["plugin", "verify", "diag"]);
    assert!(!out.status.success());
    assert!(
        text(&out).1.contains("changed since it was trusted"),
        "{}",
        text(&out).1
    );
    let (stdout, _) = text(&cli(&home, &["plugin", "info", "diag"]));
    assert!(
        stdout.contains("trust:        changed since trusted"),
        "{stdout}"
    );
    let out = cli(&home, &["plugin", "trust", "diag", "--yes"]);
    assert!(out.status.success(), "{:?}", text(&out));
    assert!(text(&out).0.contains("Trusted plugin 'diag' at"));
    assert!(cli(&home, &["plugin", "verify", "diag"]).status.success());

    // Remove deletes the files and the approval, so a reinstall is consent again.
    let out = cli(&home, &["plugin", "remove", "diag"]);
    assert!(out.status.success());
    assert!(!installed.exists());
    let approvals =
        std::fs::read_to_string(home.join("plugins/approvals.json")).expect("approvals");
    assert!(!approvals.contains("\"diag\""), "{approvals}");
}

#[test]
fn hs2_xy0vtn_verify_reports_a_plugin_that_cannot_answer() {
    // The binary answers the handshake with `diagnostics` while the manifest claims
    // `notifier`, so every start fails the capability check.
    let root = tempfile::tempdir().expect("temp");
    let home = root.path().join("home");
    let src = source_plugin(root.path(), "notifier");
    assert!(
        cli(
            &home,
            &["plugin", "install", src.to_str().expect("utf8"), "--yes"]
        )
        .status
        .success()
    );
    let out = cli(&home, &["plugin", "verify", "diag"]);
    let (_, stderr) = text(&out);
    assert!(!out.status.success());
    assert!(
        stderr.contains("over json (required)") && stderr.contains("capabilities"),
        "{stderr}"
    );
}

#[test]
fn hs2_xy0vtn_install_refuses_unsafe_or_invalid_plugin_directories() {
    let root = tempfile::tempdir().expect("temp");
    let home = root.path().join("home");

    // A manifest that does not validate.
    let src = source_plugin(root.path(), "diagnostics");
    let manifest = std::fs::read_to_string(src.join("manifest.toml")).expect("read");
    std::fs::write(
        src.join("manifest.toml"),
        manifest.replace("min = 1, max = 1", "min = 7, max = 9"),
    )
    .expect("write");
    let out = cli(
        &home,
        &["plugin", "install", src.to_str().expect("utf8"), "--yes"],
    );
    assert!(!out.status.success());
    assert!(text(&out).1.contains("speaks protocol"), "{}", text(&out).1);
    std::fs::write(src.join("manifest.toml"), manifest).expect("restore");

    // A symbolic link inside the directory.
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(root.path(), src.join("escape")).expect("link");
        let out = cli(
            &home,
            &["plugin", "install", src.to_str().expect("utf8"), "--yes"],
        );
        assert!(!out.status.success());
        assert!(text(&out).1.contains("symbolic link"), "{}", text(&out).1);
        std::fs::remove_file(src.join("escape")).expect("unlink");
    }

    // A missing executable.
    let exe = std::fs::read_dir(src.join("bin"))
        .expect("bin")
        .next()
        .expect("one")
        .expect("entry")
        .path();
    std::fs::remove_file(&exe).expect("remove binary");
    let out = cli(
        &home,
        &["plugin", "install", src.to_str().expect("utf8"), "--yes"],
    );
    assert!(!out.status.success());
    assert!(text(&out).1.contains("cannot run"), "{}", text(&out).1);
    assert!(
        !home.join("plugins/diag").exists(),
        "nothing half-installed"
    );
}
