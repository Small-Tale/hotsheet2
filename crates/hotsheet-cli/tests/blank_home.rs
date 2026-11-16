//! HS2-1D4DQF: a blank `HOTSHEET_HOME` (a failed `$(mktemp -d)` substitution, say) must
//! behave as unset. It once resolved to the working directory, so the CLI wrote its SQLite
//! `index/` and other machine-home files into whatever checkout it ran from.
//!
//! The default home is `$HOME/.hotsheet2`, so every run points `HOME` at a temp directory;
//! no run can reach the developer's real home. `HOTSHEET_NATIVE_SECRETS=deny` comes from
//! `.cargo/config.toml` and is repeated here so a direct run keeps it too.

#[path = "../../test-support/workspace_bin.rs"]
mod workspace_bin;

use std::path::Path;
use std::process::Command;

fn cli(cwd: &Path, user_home: &Path, hotsheet_home: &str, store: &Path) -> Command {
    let mut cmd = Command::new(workspace_bin::path("hotsheet-cli"));
    cmd.current_dir(cwd)
        .env("HOME", user_home)
        .env("HOTSHEET_HOME", hotsheet_home)
        .env("HOTSHEET_NATIVE_SECRETS", "deny")
        .env_remove("HOTSHEET_STORE")
        .env_remove("HOTSHEET_ACTOR_ROLE")
        .env_remove("HOTSHEET_ACTOR_ID")
        .arg("-C")
        .arg(store);
    cmd
}

fn run(cmd: &mut Command) {
    let out = cmd.output().unwrap();
    assert!(
        out.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

fn entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn index_files(home: &Path) -> Vec<String> {
    entries(&home.join("index"))
        .into_iter()
        .filter(|name| name.ends_with(".sqlite"))
        .collect()
}

#[test]
fn hs2_1d4dqf_blank_hotsheet_home_writes_the_default_home_not_the_cwd() {
    for blank in ["", "   "] {
        let root = tempfile::tempdir().unwrap();
        let cwd = root.path().join("checkout");
        let user_home = root.path().join("user");
        let store = root.path().join("tickets");
        std::fs::create_dir_all(&cwd).unwrap();
        std::fs::create_dir_all(&user_home).unwrap();
        hotsheet_ticketing::FsStore::init(&store, &hotsheet_ticketing::StoreMetadata::new("HS"))
            .unwrap();

        run(cli(&cwd, &user_home, blank, &store).args(["new", "--title", "blank home"]));
        run(cli(&cwd, &user_home, blank, &store).arg("ls"));

        assert_eq!(
            entries(&cwd),
            Vec::<String>::new(),
            "HOTSHEET_HOME={blank:?} wrote into the working directory"
        );
        assert!(
            !index_files(&user_home.join(".hotsheet2")).is_empty(),
            "HOTSHEET_HOME={blank:?} did not fall back to $HOME/.hotsheet2"
        );
    }
}

#[test]
fn hs2_1d4dqf_absolute_and_relative_hotsheet_homes_are_honored() {
    let root = tempfile::tempdir().unwrap();
    let cwd = root.path().join("checkout");
    let user_home = root.path().join("user");
    let explicit = root.path().join("explicit-home");
    let store = root.path().join("tickets");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&user_home).unwrap();
    hotsheet_ticketing::FsStore::init(&store, &hotsheet_ticketing::StoreMetadata::new("HS"))
        .unwrap();

    // Absolute: the index lives there and the default home stays untouched.
    run(cli(&cwd, &user_home, explicit.to_str().unwrap(), &store).arg("ls"));
    assert!(!index_files(&explicit).is_empty());
    assert!(!user_home.join(".hotsheet2").exists());
    assert_eq!(entries(&cwd), Vec::<String>::new());

    // Relative: an explicit choice, resolved against the working directory as given.
    run(cli(&cwd, &user_home, "relative-home", &store).arg("ls"));
    assert!(!index_files(&cwd.join("relative-home")).is_empty());
    assert!(!user_home.join(".hotsheet2").exists());
}
