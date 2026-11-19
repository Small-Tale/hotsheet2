//! HS2-MZM988: the headless `hotsheet-cli screensaver` workflow end to end. Every run points the saver's
//! home at a temporary directory (`HOTSHEET_SCREENSAVER_HOME`), so nothing reaches the real
//! `~/Library/Screen Savers` or the screen saver engine's container.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use assert_cmd::Command;
use predicates::prelude::*;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};

fn cli(home: &Path) -> Command {
    let mut command = Command::new(workspace_bin::path("hotsheet-cli"));
    command.env("HOTSHEET_SCREENSAVER_HOME", home);
    command
}

fn json(home: &Path, args: &[&str]) -> Value {
    let output = cli(home).args(args).arg("--json").output().unwrap();
    assert!(
        output.status.success(),
        "{args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

/// A bundle with the files the engine needs.
fn bundle(root: &Path, marker: &str) -> PathBuf {
    let bundle = root.join("build").join("HotSheet.saver");
    fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
    fs::create_dir_all(bundle.join("Contents/Resources/standby")).unwrap();
    fs::write(bundle.join("Contents/Info.plist"), "<plist/>").unwrap();
    fs::write(bundle.join("Contents/MacOS/HotSheetSaver"), marker).unwrap();
    fs::write(
        bundle.join("Contents/Resources/standby/index.html"),
        "<html/>",
    )
    .unwrap();
    bundle
}

#[cfg(not(target_os = "macos"))]
#[test]
fn hs2_mzm988_other_platforms_say_there_is_no_saver_and_change_nothing() {
    let home = tempfile::tempdir().unwrap();
    assert_eq!(
        json(home.path(), &["screensaver", "status"]),
        serde_json::json!({"supported": false})
    );
    cli(home.path())
        .args(["screensaver", "install"])
        .assert()
        .success()
        .stdout(predicate::str::contains("macOS only"));
    assert!(fs::read_dir(home.path()).unwrap().next().is_none());
}

#[cfg(target_os = "macos")]
mod macos {
    use super::*;

    #[test]
    fn hs2_mzm988_status_of_a_fresh_machine_names_the_install_step_and_the_password_setting() {
        let home = tempfile::tempdir().unwrap();
        let status = json(home.path(), &["screensaver", "status"]);
        assert_eq!(status["supported"], true);
        assert_eq!(status["installed"], false);
        assert_eq!(status["snapshot_present"], false);
        assert_eq!(status["snapshot_dirs"], serde_json::json!([]));
        assert!(
            ["immediately", "after_seconds", "off", "unknown"]
                .contains(&status["password"]["state"].as_str().unwrap()),
            "{status}"
        );
        assert_eq!(
            status["next_steps"][0],
            "Install it: hotsheet-cli screensaver install"
        );
        // Reading status creates nothing.
        assert!(fs::read_dir(home.path()).unwrap().next().is_none());
        cli(home.path())
            .args(["screensaver", "status"])
            .assert()
            .success()
            .stdout(predicate::str::contains("not installed"))
            .stdout(
                predicate::str::contains("Lock Screen")
                    .or(predicate::str::contains("Screen Saver")),
            );
    }

    #[test]
    fn hs2_mzm988_install_status_reinstall_and_uninstall_round_trip() {
        let home = tempfile::tempdir().unwrap();
        let source = bundle(home.path(), "one");
        let engine = home
            .path()
            .join("Library/Containers/com.apple.ScreenSaver.Engine.legacyScreenSaver/Data/Library");
        fs::create_dir_all(&engine).unwrap();

        let first = json(
            home.path(),
            &[
                "screensaver",
                "install",
                "--bundle",
                source.to_str().unwrap(),
            ],
        );
        assert_eq!(first["replaced"], false);
        let installed = home.path().join("Library/Screen Savers/HotSheet.saver");
        assert_eq!(first["bundle"], installed.to_str().unwrap());
        assert_eq!(
            fs::read_to_string(installed.join("Contents/MacOS/HotSheetSaver")).unwrap(),
            "one"
        );
        // The engine's container exists, so the server is opted in with its snapshot folder.
        assert_eq!(first["snapshot_dirs"].as_array().unwrap().len(), 1);

        let status = json(home.path(), &["screensaver", "status"]);
        assert_eq!(status["installed"], true);
        assert_eq!(status["snapshot_present"], false);

        // A snapshot the server leaves is reported.
        let folder = PathBuf::from(first["snapshot_dirs"][0].as_str().unwrap());
        fs::write(folder.join("standby-snapshot.json"), "{}").unwrap();
        assert_eq!(
            json(home.path(), &["screensaver", "status"])["snapshot_present"],
            true
        );

        // Installing again is safe and replaces the bundle whole.
        let newer = bundle(home.path(), "two");
        let second = json(
            home.path(),
            &[
                "screensaver",
                "install",
                "--bundle",
                newer.to_str().unwrap(),
            ],
        );
        assert_eq!(second["replaced"], true);
        assert_eq!(
            fs::read_to_string(installed.join("Contents/MacOS/HotSheetSaver")).unwrap(),
            "two"
        );

        let removed = json(home.path(), &["screensaver", "uninstall"]);
        assert_eq!(removed["removed"], true);
        assert!(!installed.exists());
        assert!(!folder.exists(), "the opt-in folder goes with the saver");
        assert_eq!(
            json(home.path(), &["screensaver", "status"])["installed"],
            false
        );
        assert_eq!(
            json(home.path(), &["screensaver", "uninstall"])["removed"],
            false
        );
        cli(home.path())
            .args(["screensaver", "uninstall"])
            .assert()
            .success()
            .stdout(predicate::str::contains("was not installed"));
    }

    #[test]
    fn hs2_mzm988_install_refuses_a_folder_that_is_not_a_saver_and_leaves_the_machine_alone() {
        let home = tempfile::tempdir().unwrap();
        let not_a_bundle = home.path().join("elsewhere");
        fs::create_dir_all(&not_a_bundle).unwrap();
        cli(home.path())
            .args([
                "screensaver",
                "install",
                "--bundle",
                not_a_bundle.to_str().unwrap(),
            ])
            .assert()
            .failure()
            .stderr(predicate::str::contains(
                "not a Hot Sheet screen saver bundle",
            ));
        assert!(!home.path().join("Library").exists());
    }

    #[test]
    fn hs2_mzm988_human_status_lists_what_is_in_place_in_plain_words() {
        let home = tempfile::tempdir().unwrap();
        let source = bundle(home.path(), "one");
        cli(home.path())
            .args([
                "screensaver",
                "install",
                "--bundle",
                source.to_str().unwrap(),
            ])
            .assert()
            .success()
            .stdout(predicate::str::contains(
                "Installed the Hot Sheet screen saver",
            ))
            .stdout(predicate::str::contains("System Settings > Screen Saver"));
        cli(home.path())
            .args(["screensaver", "status"])
            .assert()
            .success()
            .stdout(predicate::str::contains("installed"))
            .stdout(predicate::str::contains("password:"));
    }
}
