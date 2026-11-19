//! macOS screen saver support (HS2-MZM988, `docs/34-glance-surfaces.md` §34.12): where the saver
//! bundle and its snapshot live, the credential-free snapshot the server leaves for it, and the
//! install, uninstall, and status workflow the CLI runs.
//!
//! The saver is a system screen saver hosted by the sandboxed `legacyScreenSaver` engine, which is
//! why macOS asks for the account password when it ends. The engine gives the saver no server
//! secret and no network, so the server writes one JSON snapshot into the engine's container and the
//! saver watches it. Writing is opt-in: the server writes only into a snapshot directory that
//! exists, and `install` (or the saver's first run) is what creates it. These are plain file
//! operations kept in `ticketing` so the CLI and the server share them.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::Serialize;

/// The saver bundle's file name inside `~/Library/Screen Savers`.
pub const BUNDLE_NAME: &str = "HotSheet.saver";
/// The snapshot's file name inside each snapshot directory.
pub const SNAPSHOT_FILE: &str = "standby-snapshot.json";
/// The snapshot format version the saver's page accepts.
pub const SNAPSHOT_VERSION: u32 = 1;
/// How long the server vouches for a snapshot. The server rewrites it every minute while it runs,
/// so once this passes the saver says the connection was lost instead of showing stale numbers.
pub const SNAPSHOT_VALID_MS: i64 = 3 * 60_000;

/// The legacy engine's container ids: the native one and its Rosetta twin.
const ENGINE_CONTAINERS: [&str; 2] = [
    "com.apple.ScreenSaver.Engine.legacyScreenSaver",
    "com.apple.ScreenSaver.Engine.legacyScreenSaver.x86-64",
];
/// The saver reads `<container>/Data/Library/Application Support/HotSheet`.
const SNAPSHOT_DIRECTORY: &str = "HotSheet";

/// Overrides the home directory the saver's locations hang off. Production leaves it unset, so the
/// saver lives in the real `$HOME`; `.cargo/config.toml` points every test process at a repository-local
/// folder so none can touch the developer's real container or `~/Library/Screen Savers`.
pub const HOME_ENV: &str = "HOTSHEET_SCREENSAVER_HOME";

/// The home the saver's locations hang off: the override when it is set and not blank, else `home`.
pub fn resolve_home(
    override_home: Option<std::ffi::OsString>,
    home: Option<std::ffi::OsString>,
) -> Option<PathBuf> {
    [override_home, home]
        .into_iter()
        .flatten()
        .find(|value| !value.to_string_lossy().trim().is_empty())
        .map(PathBuf::from)
}

/// The saver's locations for this process, or `None` when no home directory is known.
pub fn locations_from_env() -> Option<Locations> {
    resolve_home(std::env::var_os(HOME_ENV), std::env::var_os("HOME")).map(Locations::for_home)
}

/// Where the saver and its snapshots live for one user's home directory.
#[derive(Debug, Clone)]
pub struct Locations {
    home: PathBuf,
}

impl Locations {
    pub fn for_home(home: impl Into<PathBuf>) -> Self {
        Self { home: home.into() }
    }

    /// `~/Library/Screen Savers`, where the system looks for a user's own savers.
    pub fn savers_dir(&self) -> PathBuf {
        self.home.join("Library").join("Screen Savers")
    }

    /// The installed bundle.
    pub fn bundle(&self) -> PathBuf {
        self.savers_dir().join(BUNDLE_NAME)
    }

    fn container_data(&self, container: &str) -> PathBuf {
        self.home
            .join("Library")
            .join("Containers")
            .join(container)
            .join("Data")
    }

    /// Every snapshot directory the saver could read, whether or not it exists yet.
    pub fn snapshot_dirs(&self) -> Vec<PathBuf> {
        ENGINE_CONTAINERS
            .iter()
            .map(|container| {
                self.container_data(container)
                    .join("Library")
                    .join("Application Support")
                    .join(SNAPSHOT_DIRECTORY)
            })
            .collect()
    }

    /// The snapshot directories that exist: the ones the server writes into.
    pub fn active_snapshot_dirs(&self) -> Vec<PathBuf> {
        self.snapshot_dirs()
            .into_iter()
            .filter(|dir| dir.is_dir())
            .collect()
    }
}

/// The file the server leaves for the saver; the saver's page parses exactly this shape.
#[derive(Serialize)]
struct Snapshot<'a, T: Serialize> {
    version: u32,
    written_at_ms: i64,
    valid_until_ms: i64,
    live: &'a T,
}

/// The snapshot text for `live` (the same list `GET /live-state` returns) written at `now_ms`.
pub fn snapshot_json<T: Serialize>(live: &T, now_ms: i64) -> serde_json::Result<String> {
    serde_json::to_string(&Snapshot {
        version: SNAPSHOT_VERSION,
        written_at_ms: now_ms,
        valid_until_ms: now_ms.saturating_add(SNAPSHOT_VALID_MS),
        live,
    })
}

/// What a snapshot write did.
#[derive(Debug, Default)]
pub struct WriteReport {
    /// Directories that now hold the new snapshot.
    pub written: usize,
    /// Directories that could not be written, with why. A failure never blocks the others.
    pub failed: Vec<(PathBuf, io::Error)>,
}

/// Replace `<dir>/standby-snapshot.json` atomically, so the watching saver sees the old file or the
/// new one and never half of it. The file is readable by the user only.
fn write_atomically(dir: &Path, text: &str) -> io::Result<()> {
    let mut staged = tempfile::Builder::new()
        .prefix(".standby-snapshot-")
        .suffix(".tmp")
        .tempfile_in(dir)?;
    staged.write_all(text.as_bytes())?;
    staged.as_file().sync_all()?;
    staged
        .persist(dir.join(SNAPSHOT_FILE))
        .map_err(|e| e.error)?;
    Ok(())
}

/// Leave `text` in every existing snapshot directory. Nothing is created: a machine that never
/// installed the saver gets no file and no work.
pub fn write_snapshots(locations: &Locations, text: &str) -> WriteReport {
    let mut report = WriteReport::default();
    for dir in locations.active_snapshot_dirs() {
        match write_atomically(&dir, text) {
            Ok(()) => report.written += 1,
            Err(error) => report.failed.push((dir, error)),
        }
    }
    report
}

/// Why installing the saver failed.
#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error("{0} is not a Hot Sheet screen saver bundle (missing {1})")]
    NotABundle(PathBuf, &'static str),
    #[error("could not install the screen saver: {0}")]
    Io(#[from] io::Error),
}

/// The files a bundle must carry to be one the engine can load.
const REQUIRED: [&str; 2] = ["Contents/Info.plist", "Contents/MacOS/HotSheetSaver"];

fn copy_tree(from: &Path, to: &Path) -> io::Result<()> {
    fs::create_dir_all(to)?;
    for entry in fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_symlink() {
            #[cfg(unix)]
            std::os::unix::fs::symlink(fs::read_link(entry.path())?, &target)?;
        } else {
            fs::copy(entry.path(), &target)?;
        }
    }
    Ok(())
}

/// What `install` did.
#[derive(Debug)]
pub struct Installed {
    pub bundle: PathBuf,
    /// Whether a bundle was already there and was replaced.
    pub replaced: bool,
    /// The snapshot directories that exist now, which the server starts writing into.
    pub snapshot_dirs: Vec<PathBuf>,
}

/// Copy the bundle at `source` into `~/Library/Screen Savers`, replacing a previous install without
/// ever leaving a half-copied bundle in place. Re-running it is safe.
///
/// A snapshot directory is created only inside a container the engine has already made: creating a
/// container by hand would leave it without the metadata the system writes. When none exists yet the
/// saver creates it the first time macOS runs it, and the server starts writing then.
pub fn install(locations: &Locations, source: &Path) -> Result<Installed, InstallError> {
    for required in REQUIRED {
        if !source.join(required).is_file() {
            return Err(InstallError::NotABundle(source.to_path_buf(), required));
        }
    }
    let savers = locations.savers_dir();
    fs::create_dir_all(&savers)?;
    let staging = tempfile::Builder::new()
        .prefix(".HotSheet-install-")
        .tempdir_in(&savers)?;
    let staged = staging.path().join(BUNDLE_NAME);
    copy_tree(source, &staged)?;
    let destination = locations.bundle();
    let replaced = destination.exists();
    if replaced {
        fs::remove_dir_all(&destination)?;
    }
    fs::rename(&staged, &destination)?;
    for container in ENGINE_CONTAINERS {
        if locations.container_data(container).is_dir() {
            let dir = locations
                .container_data(container)
                .join("Library")
                .join("Application Support")
                .join(SNAPSHOT_DIRECTORY);
            fs::create_dir_all(&dir)?;
        }
    }
    Ok(Installed {
        bundle: destination,
        replaced,
        snapshot_dirs: locations.active_snapshot_dirs(),
    })
}

/// Remove the bundle and every snapshot, and the snapshot directories that opt the server in.
/// Returns whether anything was there to remove.
pub fn uninstall(locations: &Locations) -> io::Result<bool> {
    let mut removed = false;
    let bundle = locations.bundle();
    if bundle.exists() {
        fs::remove_dir_all(&bundle)?;
        removed = true;
    }
    for dir in locations.active_snapshot_dirs() {
        fs::remove_dir_all(&dir)?;
        removed = true;
    }
    Ok(removed)
}

/// Whether macOS asks for the account password when the saver ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "state", content = "seconds", rename_all = "snake_case")]
pub enum PasswordSetting {
    /// The password is asked for the moment the saver starts.
    Immediately,
    /// The password is asked for once the saver has run this many seconds.
    AfterSeconds(u64),
    /// No password is asked for.
    Off,
    /// The setting could not be read, or this is not macOS.
    Unknown,
}

impl PasswordSetting {
    /// Whether the saver ends in a password prompt.
    pub fn requires_password(self) -> Option<bool> {
        match self {
            Self::Immediately | Self::AfterSeconds(_) => Some(true),
            Self::Off => Some(false),
            Self::Unknown => None,
        }
    }
}

/// Read `sysadminctl -screenLock status` output, e.g. `screenLock delay is 300 seconds`,
/// `screenLock delay is immediate`, or `screenLock is off`. Anything else is `Unknown`, never a guess.
pub fn parse_screen_lock(output: &str) -> PasswordSetting {
    for line in output.lines() {
        let Some((_, rest)) = line.split_once("screenLock") else {
            continue;
        };
        let rest = rest.trim();
        if rest == "is off" || rest.starts_with("is off") {
            return PasswordSetting::Off;
        }
        let Some(delay) = rest.strip_prefix("delay is") else {
            continue;
        };
        let delay = delay.trim();
        if delay == "immediate" {
            return PasswordSetting::Immediately;
        }
        let seconds = delay.strip_suffix("seconds").map(str::trim);
        if let Some(seconds) = seconds.and_then(|text| text.parse::<u64>().ok()) {
            return if seconds == 0 {
                PasswordSetting::Immediately
            } else {
                PasswordSetting::AfterSeconds(seconds)
            };
        }
    }
    PasswordSetting::Unknown
}

/// The machine's current setting. `sysadminctl` prints it on standard error.
#[cfg(target_os = "macos")]
pub fn read_password_setting() -> PasswordSetting {
    match std::process::Command::new("/usr/sbin/sysadminctl")
        .args(["-screenLock", "status"])
        .output()
    {
        Ok(output) => {
            let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
            text.push('\n');
            text.push_str(&String::from_utf8_lossy(&output.stderr));
            parse_screen_lock(&text)
        }
        Err(_) => PasswordSetting::Unknown,
    }
}

/// Not macOS: there is no screen saver password to read.
#[cfg(not(target_os = "macos"))]
pub fn read_password_setting() -> PasswordSetting {
    PasswordSetting::Unknown
}

/// What `hotsheet-cli screensaver status` reports.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    /// Whether this is macOS, the only platform with the saver.
    pub supported: bool,
    pub installed: bool,
    pub bundle: PathBuf,
    /// Whether the server has somewhere to write, i.e. the saver is installed and has a container.
    pub snapshot_dirs: Vec<PathBuf>,
    /// Whether a snapshot is waiting for the saver.
    pub snapshot_present: bool,
    pub password: PasswordSetting,
}

pub fn status(locations: &Locations, supported: bool, password: PasswordSetting) -> Status {
    let snapshot_dirs = locations.active_snapshot_dirs();
    Status {
        supported,
        installed: locations.bundle().join("Contents/Info.plist").is_file(),
        bundle: locations.bundle(),
        snapshot_present: snapshot_dirs
            .iter()
            .any(|dir| dir.join(SNAPSHOT_FILE).is_file()),
        snapshot_dirs,
        password,
    }
}

impl Status {
    /// The next steps, in plain words, for whatever is not yet in place. The password prompt is a
    /// system setting an app cannot change, so the exact place to turn it on is part of the answer.
    pub fn next_steps(&self) -> Vec<String> {
        let mut steps = Vec::new();
        if !self.supported {
            steps.push("The Hot Sheet screen saver is for macOS only.".to_string());
            return steps;
        }
        if !self.installed {
            steps.push("Install it: hotsheet-cli screensaver install".to_string());
        } else {
            steps.push(
                "Choose Hot Sheet in System Settings > Screen Saver (macOS keeps that choice per user)."
                    .to_string(),
            );
        }
        match self.password.requires_password() {
            Some(true) => {}
            Some(false) => steps.push(
                "Turn on the password: System Settings > Lock Screen > \"Require password after screen saver begins or display is turned off\"."
                    .to_string(),
            ),
            None => steps.push(
                "Check that the password is required: System Settings > Lock Screen > \"Require password after screen saver begins or display is turned off\"."
                    .to_string(),
            ),
        }
        steps
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn home() -> tempfile::TempDir {
        tempfile::tempdir().expect("temporary home")
    }

    /// A bundle with the files the engine needs, plus a nested resource.
    fn bundle(root: &Path, marker: &str) -> PathBuf {
        let bundle = root.join("build").join(BUNDLE_NAME);
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

    /// The engine's container, as the system leaves it before any saver has written into it.
    fn engine_container(locations: &Locations, container: &str) {
        fs::create_dir_all(locations.container_data(container).join("Library")).unwrap();
    }

    #[test]
    fn locations_name_the_user_saver_folder_and_both_engine_containers() {
        let locations = Locations::for_home("/h");
        assert_eq!(
            locations.savers_dir(),
            PathBuf::from("/h/Library/Screen Savers")
        );
        assert_eq!(
            locations.bundle(),
            PathBuf::from("/h/Library/Screen Savers/HotSheet.saver")
        );
        assert_eq!(
            locations.snapshot_dirs(),
            vec![
                PathBuf::from(
                    "/h/Library/Containers/com.apple.ScreenSaver.Engine.legacyScreenSaver/Data/Library/Application Support/HotSheet"
                ),
                PathBuf::from(
                    "/h/Library/Containers/com.apple.ScreenSaver.Engine.legacyScreenSaver.x86-64/Data/Library/Application Support/HotSheet"
                ),
            ]
        );
    }

    #[test]
    fn the_override_wins_over_home_and_a_blank_one_is_ignored() {
        use std::ffi::OsString;
        let os = |text: &str| Some(OsString::from(text));
        assert_eq!(
            resolve_home(os("/override"), os("/home")),
            Some(PathBuf::from("/override"))
        );
        assert_eq!(
            resolve_home(None, os("/home")),
            Some(PathBuf::from("/home"))
        );
        assert_eq!(
            resolve_home(os(""), os("/home")),
            Some(PathBuf::from("/home"))
        );
        assert_eq!(
            resolve_home(os("   "), os("/home")),
            Some(PathBuf::from("/home"))
        );
        assert_eq!(resolve_home(None, os("")), None);
        assert_eq!(resolve_home(None, None), None);
    }

    #[test]
    fn the_snapshot_is_the_exact_shape_the_page_parses() {
        let live = json!({"epoch_ms": 7, "stores": []});
        let text = snapshot_json(&live, 1_000).unwrap();
        let value: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(
            value,
            json!({
                "version": 1,
                "written_at_ms": 1_000,
                "valid_until_ms": 1_000 + SNAPSHOT_VALID_MS,
                "live": {"epoch_ms": 7, "stores": []},
            })
        );
        // A clock at the end of time never wraps into the past.
        let late: Value = serde_json::from_str(&snapshot_json(&live, i64::MAX).unwrap()).unwrap();
        assert_eq!(late["valid_until_ms"], json!(i64::MAX));
    }

    #[test]
    fn writing_is_opt_in_and_creates_nothing() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let report = write_snapshots(&locations, "{}");
        assert_eq!(report.written, 0);
        assert!(report.failed.is_empty());
        assert!(
            !home.path().join("Library").exists(),
            "a machine without the saver gets no files"
        );
    }

    #[test]
    fn writing_replaces_the_snapshot_atomically_in_every_existing_directory() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let dirs = locations.snapshot_dirs();
        for dir in &dirs {
            fs::create_dir_all(dir).unwrap();
        }
        assert_eq!(write_snapshots(&locations, "first").written, 2);
        assert_eq!(write_snapshots(&locations, "second").written, 2);
        for dir in &dirs {
            assert_eq!(
                fs::read_to_string(dir.join(SNAPSHOT_FILE)).unwrap(),
                "second"
            );
            let entries: Vec<_> = fs::read_dir(dir)
                .unwrap()
                .map(|e| e.unwrap().file_name())
                .collect();
            assert_eq!(
                entries,
                vec![std::ffi::OsString::from(SNAPSHOT_FILE)],
                "no staging file is left behind"
            );
        }
    }

    #[test]
    fn a_directory_that_cannot_be_written_does_not_stop_the_others() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let dirs = locations.snapshot_dirs();
        for dir in &dirs {
            fs::create_dir_all(dir).unwrap();
        }
        // A directory standing where the snapshot file goes cannot be replaced by a file.
        fs::create_dir(dirs[0].join(SNAPSHOT_FILE)).unwrap();
        let report = write_snapshots(&locations, "ok");
        assert_eq!(report.written, 1);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, dirs[0]);
        assert_eq!(
            fs::read_to_string(dirs[1].join(SNAPSHOT_FILE)).unwrap(),
            "ok"
        );
    }

    #[cfg(unix)]
    #[test]
    fn the_snapshot_is_readable_by_the_user_only() {
        use std::os::unix::fs::PermissionsExt;
        let home = home();
        let locations = Locations::for_home(home.path());
        let dir = &locations.snapshot_dirs()[0];
        fs::create_dir_all(dir).unwrap();
        write_snapshots(&locations, "{}");
        let mode = fs::metadata(dir.join(SNAPSHOT_FILE))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o077,
            0,
            "group and others get no access, mode {mode:o}"
        );
    }

    #[test]
    fn install_copies_the_whole_bundle_and_is_safe_to_repeat() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let source = bundle(home.path(), "one");
        let first = install(&locations, &source).unwrap();
        assert!(!first.replaced);
        assert_eq!(first.bundle, locations.bundle());
        assert_eq!(
            fs::read_to_string(locations.bundle().join("Contents/MacOS/HotSheetSaver")).unwrap(),
            "one"
        );
        assert!(
            locations
                .bundle()
                .join("Contents/Resources/standby/index.html")
                .is_file()
        );

        // A new build replaces the old one completely, with nothing of the old build left.
        fs::write(
            locations.bundle().join("Contents/Resources/stale.txt"),
            "old",
        )
        .unwrap();
        let source = bundle(home.path(), "two");
        let second = install(&locations, &source).unwrap();
        assert!(second.replaced);
        assert_eq!(
            fs::read_to_string(locations.bundle().join("Contents/MacOS/HotSheetSaver")).unwrap(),
            "two"
        );
        assert!(
            !locations
                .bundle()
                .join("Contents/Resources/stale.txt")
                .exists()
        );
        let leftovers: Vec<_> = fs::read_dir(locations.savers_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            leftovers,
            vec![std::ffi::OsString::from(BUNDLE_NAME)],
            "no staging folder remains"
        );
    }

    #[test]
    fn install_refuses_a_folder_that_is_not_a_bundle_and_changes_nothing() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let source = bundle(home.path(), "one");
        install(&locations, &source).unwrap();
        let empty = home.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        let error = install(&locations, &empty).unwrap_err();
        assert!(matches!(
            error,
            InstallError::NotABundle(_, "Contents/Info.plist")
        ));
        fs::remove_file(source.join("Contents/MacOS/HotSheetSaver")).unwrap();
        let error = install(&locations, &source).unwrap_err();
        assert!(matches!(
            error,
            InstallError::NotABundle(_, "Contents/MacOS/HotSheetSaver")
        ));
        assert_eq!(
            fs::read_to_string(locations.bundle().join("Contents/MacOS/HotSheetSaver")).unwrap(),
            "one",
            "a refused install leaves the working one in place"
        );
    }

    #[test]
    fn install_opts_the_server_in_only_where_the_engine_already_has_a_container() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let source = bundle(home.path(), "one");
        // No container yet: nothing is created by hand, so the saver makes it on its first run.
        let none = install(&locations, &source).unwrap();
        assert!(none.snapshot_dirs.is_empty());
        assert!(!home.path().join("Library/Containers").exists());
        engine_container(&locations, ENGINE_CONTAINERS[0]);
        let some = install(&locations, &source).unwrap();
        assert_eq!(
            some.snapshot_dirs,
            vec![locations.snapshot_dirs()[0].clone()]
        );
        assert_eq!(write_snapshots(&locations, "{}").written, 1);
    }

    #[test]
    fn uninstall_removes_the_bundle_and_the_opt_in_and_reports_whether_it_did_anything() {
        let home = home();
        let locations = Locations::for_home(home.path());
        assert!(!uninstall(&locations).unwrap(), "nothing to remove");
        engine_container(&locations, ENGINE_CONTAINERS[0]);
        install(&locations, &bundle(home.path(), "one")).unwrap();
        write_snapshots(&locations, "{}");
        assert!(uninstall(&locations).unwrap());
        assert!(!locations.bundle().exists());
        assert!(locations.active_snapshot_dirs().is_empty());
        assert_eq!(
            write_snapshots(&locations, "{}").written,
            0,
            "the server stops writing"
        );
        assert!(
            !uninstall(&locations).unwrap(),
            "a second uninstall is a no-op"
        );
    }

    #[test]
    fn the_password_setting_reads_every_form_sysadminctl_prints_and_never_guesses() {
        assert_eq!(
            parse_screen_lock(
                "2026-11-18 22:29:17.429 sysadminctl[1:2] screenLock delay is 300 seconds"
            ),
            PasswordSetting::AfterSeconds(300)
        );
        assert_eq!(
            parse_screen_lock("screenLock delay is immediate"),
            PasswordSetting::Immediately
        );
        assert_eq!(
            parse_screen_lock("screenLock delay is 0 seconds"),
            PasswordSetting::Immediately
        );
        assert_eq!(
            parse_screen_lock("x sysadminctl[1:2] screenLock is off"),
            PasswordSetting::Off
        );
        for unknown in [
            "",
            "no such thing",
            "screenLock delay is soon",
            "screenLock delay is -5 seconds",
        ] {
            assert_eq!(
                parse_screen_lock(unknown),
                PasswordSetting::Unknown,
                "{unknown:?}"
            );
        }
        // The line may follow unrelated output.
        assert_eq!(
            parse_screen_lock("noise\n\nscreenLock delay is 5 seconds\n"),
            PasswordSetting::AfterSeconds(5)
        );
        assert_eq!(PasswordSetting::Immediately.requires_password(), Some(true));
        assert_eq!(
            PasswordSetting::AfterSeconds(5).requires_password(),
            Some(true)
        );
        assert_eq!(PasswordSetting::Off.requires_password(), Some(false));
        assert_eq!(PasswordSetting::Unknown.requires_password(), None);
    }

    #[test]
    fn status_reports_what_is_in_place_and_the_next_step_for_what_is_not() {
        let home = home();
        let locations = Locations::for_home(home.path());
        let bare = status(&locations, true, PasswordSetting::Off);
        assert!(!bare.installed && !bare.snapshot_present && bare.snapshot_dirs.is_empty());
        let steps = bare.next_steps();
        assert_eq!(steps[0], "Install it: hotsheet-cli screensaver install");
        assert!(steps[1].contains("Lock Screen"), "{steps:?}");

        engine_container(&locations, ENGINE_CONTAINERS[0]);
        install(&locations, &bundle(home.path(), "one")).unwrap();
        write_snapshots(&locations, "{}");
        let ready = status(&locations, true, PasswordSetting::AfterSeconds(60));
        assert!(ready.installed && ready.snapshot_present);
        let steps = ready.next_steps();
        assert_eq!(
            steps.len(),
            1,
            "a required password needs no further step: {steps:?}"
        );
        assert!(steps[0].contains("System Settings > Screen Saver"));
        let unknown = status(&locations, true, PasswordSetting::Unknown).next_steps();
        assert!(
            unknown
                .last()
                .unwrap()
                .starts_with("Check that the password is required")
        );
    }

    #[test]
    fn status_says_plainly_when_the_platform_has_no_saver() {
        let home = home();
        let steps = status(
            &Locations::for_home(home.path()),
            false,
            PasswordSetting::Unknown,
        )
        .next_steps();
        assert_eq!(
            steps,
            vec!["The Hot Sheet screen saver is for macOS only.".to_string()]
        );
    }
}
