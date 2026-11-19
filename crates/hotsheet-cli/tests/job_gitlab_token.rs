//! HS2-RTEQ7R: the built `hotsheet-cli` finds a GitLab token in `GITLAB_TOKEN`, else from
//! a signed-in `glab` for the API root's host, and says which in `job watch status`,
//! never the token itself. A synthetic `glab` script stands in for the real CLI.
//!
//! HS2-652GYE: Unix only. The stand-in is a `/bin/sh` script, and Windows resolves a bare
//! `glab` to an executable image, not a script.
#![cfg(unix)]

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::os::unix::fs::PermissionsExt;
use std::process::Command;

use serde_json::Value;

const TOKEN: &str = "synthetic-glab-token";

fn status(home: &std::path::Path, path: &str, environment: Option<&str>) -> (Value, String) {
    let mut command = Command::new(workspace_bin::path("hotsheet-cli"));
    command
        .args(["job", "watch", "status"])
        .env("HOTSHEET_HOME", home)
        .env("PATH", path)
        .env(
            "HOTSHEET_GITLAB_API_URL",
            "https://gitlab.example.test/api/v4",
        )
        .env_remove("GITLAB_TOKEN");
    if let Some(token) = environment {
        command.env("GITLAB_TOKEN", token);
    }
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    (serde_json::from_str(&stdout).unwrap(), stdout)
}

#[test]
fn hs2_rteq7r_a_glab_sign_in_supplies_the_gitlab_token() {
    let home = tempfile::tempdir().unwrap();
    let bin = tempfile::tempdir().unwrap();
    let asked = bin.path().join("asked");
    // `glab config get token --host <host>` answers for the API root's host only.
    let glab = bin.path().join("glab");
    std::fs::write(
        &glab,
        format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\n\
             if [ \"$*\" = 'config get token --host gitlab.example.test' ]; then echo {TOKEN}; else exit 1; fi\n",
            asked.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&glab, std::fs::Permissions::from_mode(0o755)).unwrap();
    let with_glab = format!("{}:/usr/bin:/bin", bin.path().display());

    let (from_glab, text) = status(home.path(), &with_glab, None);
    assert_eq!(from_glab["gitlab_token"], "glab");
    assert!(!text.contains(TOKEN), "the token is never printed");
    assert_eq!(
        std::fs::read_to_string(&asked).unwrap().trim(),
        "config get token --host gitlab.example.test"
    );
    // The variable wins, and glab is not asked.
    let (from_environment, _) = status(home.path(), &with_glab, Some("synthetic-env-token"));
    assert_eq!(from_environment["gitlab_token"], "GITLAB_TOKEN");
    assert_eq!(std::fs::read_to_string(&asked).unwrap().lines().count(), 1);
    // Neither: no source, said plainly.
    let empty = tempfile::tempdir().unwrap();
    let (none, _) = status(
        home.path(),
        &format!("{}:/usr/bin:/bin", empty.path().display()),
        None,
    );
    assert!(none["gitlab_token"].is_null());
    // Nothing the machine keeps holds the token.
    for entry in walkdir(home.path()) {
        let bytes = std::fs::read(&entry).unwrap_or_default();
        assert!(
            !String::from_utf8_lossy(&bytes).contains(TOKEN),
            "{}",
            entry.display()
        );
    }
}

fn walkdir(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            found.extend(walkdir(&path));
        } else {
            found.push(path);
        }
    }
    found
}
