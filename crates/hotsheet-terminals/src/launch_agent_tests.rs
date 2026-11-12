//! HS2-HYF56Y: the relay LaunchAgent's definition and idempotent install state machine, driven
//! through a fake `launchctl` and a temporary agents directory. Nothing here reaches launchd.

use super::*;
use crate::launch_relay::{BrokerLauncher, LaunchRequest};
use std::os::unix::fs::PermissionsExt;
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct FakeLaunchctl {
    calls: Mutex<Vec<Vec<String>>>,
    loaded: Mutex<bool>,
    manager: String,
    fail_bootstrap: bool,
}

impl FakeLaunchctl {
    fn aqua() -> Self {
        Self {
            manager: "Aqua".into(),
            ..Self::default()
        }
    }

    fn verbs(&self) -> Vec<String> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .map(|call| call[0].clone())
            .collect()
    }

    fn reset_calls(&self) {
        self.calls.lock().unwrap().clear();
    }
}

impl Launchctl for FakeLaunchctl {
    fn run(&self, args: &[String]) -> io::Result<LaunchctlOutput> {
        self.calls.lock().unwrap().push(args.to_vec());
        let ok = |stdout: &str| LaunchctlOutput {
            success: true,
            code: Some(0),
            stdout: stdout.into(),
            stderr: String::new(),
        };
        let failed = |code: i32, stderr: &str| LaunchctlOutput {
            success: false,
            code: Some(code),
            stdout: String::new(),
            stderr: stderr.into(),
        };
        let mut loaded = self.loaded.lock().unwrap();
        Ok(match args[0].as_str() {
            "managername" => ok(&format!("{}\n", self.manager)),
            "print" if *loaded => ok("state = running"),
            "print" => failed(113, "Could not find service"),
            "bootstrap" if self.fail_bootstrap => {
                failed(5, "Bootstrap failed: 5: Input/output error")
            }
            "bootstrap" => {
                *loaded = true;
                ok("")
            }
            "bootout" => {
                *loaded = false;
                ok("")
            }
            "kickstart" => ok(""),
            other => failed(64, &format!("unexpected verb {other}")),
        })
    }
}

struct NoLaunch;

impl BrokerLauncher for NoLaunch {
    fn broker(&self) -> (PathBuf, bool) {
        (PathBuf::from("/opt/hotsheet-server"), true)
    }

    fn launch(&self, _request: &LaunchRequest) -> io::Result<u32> {
        Err(io::Error::other("not in this test"))
    }
}

struct Fixture {
    _directory: tempfile::TempDir,
    agent: RelayAgent,
}

fn fixture() -> Fixture {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let home = directory.path().join("home");
    let agent = RelayAgent {
        label: "test.hotsheet2.terminal-relay".into(),
        plist: directory
            .path()
            .join("agents")
            .join("test.hotsheet2.terminal-relay.plist"),
        program: PathBuf::from("/opt/Hot Sheet 2.app/Contents/MacOS/hotsheet-server"),
        socket: directory.path().join("relay.sock"),
        log: home.join("logs").join("terminal-relay.log"),
        home,
    };
    Fixture {
        _directory: directory,
        agent,
    }
}

/// Stand in for the relay launchd would start: a real relay server on the agent's socket.
fn serve_relay_at(socket: &Path) -> std::thread::JoinHandle<()> {
    let (ready_tx, ready_rx) = std::sync::mpsc::channel();
    let socket = socket.to_path_buf();
    let handle = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(async move {
            let (listener, _ownership) = crate::broker_socket::bind(&socket).unwrap();
            ready_tx.send(()).unwrap();
            crate::launch_relay::serve_relay(listener, Arc::new(NoLaunch)).await;
        });
    });
    ready_rx.recv().unwrap();
    handle
}

const QUICK: Duration = Duration::from_millis(200);

#[test]
fn hs2_hyf56y_plist_runs_the_server_relay_mode_in_the_aqua_session() {
    let mut fixture = fixture();
    fixture.agent.home = PathBuf::from("/opt/a&b/.hotsheet2");
    let plist = fixture.agent.render_plist();
    assert!(plist.contains("<string>test.hotsheet2.terminal-relay</string>"));
    assert!(plist.contains("<string>/opt/Hot Sheet 2.app/Contents/MacOS/hotsheet-server</string>"));
    assert!(plist.contains("<string>--terminal-launch-relay</string>"));
    assert!(plist.contains(&format!(
        "<string>{}</string>",
        fixture.agent.socket.display()
    )));
    assert!(plist.contains("<key>LimitLoadToSessionType</key>\n\t<string>Aqua</string>"));
    assert!(plist.contains("<key>AbandonProcessGroup</key>\n\t<true/>"));
    assert!(plist.contains("<key>KeepAlive</key>\n\t<true/>"));
    assert!(plist.contains("<string>/opt/a&amp;b/.hotsheet2</string>"));
    assert!(!plist.contains("a&b"));
    // `plutil -lint` of an installed definition runs in the CLI's terminal_relay E2E.
}

#[test]
fn hs2_hyf56y_only_the_default_home_gets_the_plain_label() {
    let directory = tempfile::tempdir_in("/tmp").unwrap();
    let home = directory.path().join(".hotsheet2");
    let other = directory.path().join("dev-home");
    fs::create_dir_all(&home).unwrap();
    fs::create_dir_all(&other).unwrap();
    let agents = directory.path().join("agents");
    let program = Path::new("/opt/hotsheet-server");
    let default = RelayAgent::for_home(&home, program, &agents, Some(&home)).unwrap();
    assert_eq!(default.label, BASE_LABEL);
    assert_eq!(default.plist, agents.join(format!("{BASE_LABEL}.plist")));
    let development = RelayAgent::for_home(&other, program, &agents, Some(&home)).unwrap();
    assert!(development.label.starts_with(&format!("{BASE_LABEL}.")));
    assert_ne!(development.label, default.label);
    assert_ne!(development.socket, default.socket);
    assert_eq!(
        development,
        RelayAgent::for_home(&other, program, &agents, Some(&home)).unwrap()
    );
}

#[test]
fn hs2_hyf56y_install_is_idempotent_and_reloads_only_a_changed_definition() {
    let fixture = fixture();
    let _relay = serve_relay_at(&fixture.agent.socket);
    let launchctl = FakeLaunchctl::aqua();

    let report = install(&fixture.agent, &launchctl, 501, QUICK).unwrap();
    assert_eq!(report.outcome, InstallOutcome::Installed);
    assert_eq!(report.relay.pid, std::process::id());
    assert_eq!(
        fs::read_to_string(&fixture.agent.plist).unwrap(),
        fixture.agent.render_plist()
    );
    assert_eq!(launchctl.verbs(), ["managername", "print", "bootstrap"]);
    assert_eq!(
        launchctl.calls.lock().unwrap()[2],
        [
            "bootstrap".to_owned(),
            "gui/501".to_owned(),
            fixture.agent.plist.to_string_lossy().into_owned()
        ]
    );
    assert!(fixture.agent.log.parent().unwrap().is_dir());

    launchctl.reset_calls();
    let again = install(&fixture.agent, &launchctl, 501, QUICK).unwrap();
    assert_eq!(again.outcome, InstallOutcome::AlreadyCurrent);
    assert_eq!(launchctl.verbs(), ["managername", "print"]);

    launchctl.reset_calls();
    let mut moved = fixture.agent.clone();
    moved.program = PathBuf::from("/Applications/Hot Sheet 2.app/Contents/MacOS/hotsheet-server");
    let updated = install(&moved, &launchctl, 501, QUICK).unwrap();
    assert_eq!(updated.outcome, InstallOutcome::Updated);
    assert_eq!(
        launchctl.verbs(),
        ["managername", "print", "bootout", "bootstrap"]
    );
    assert_eq!(
        launchctl.calls.lock().unwrap()[2],
        ["bootout", "gui/501/test.hotsheet2.terminal-relay"]
    );
    assert_eq!(
        fs::read_to_string(&moved.plist).unwrap(),
        moved.render_plist()
    );
}

#[test]
fn hs2_hyf56y_install_without_a_desktop_session_changes_nothing_and_names_next_steps() {
    let fixture = fixture();
    let launchctl = FakeLaunchctl {
        manager: "Background".into(),
        ..FakeLaunchctl::default()
    };
    let error = install(&fixture.agent, &launchctl, 501, QUICK).unwrap_err();
    assert!(
        matches!(error, InstallError::NoGuiSession { .. }),
        "{error}"
    );
    let message = error.to_string();
    assert!(message.contains("Background"), "{message}");
    assert!(
        message.contains("hotsheet-cli terminal-relay install"),
        "{message}"
    );
    assert_eq!(launchctl.verbs(), ["managername"]);
    assert!(!fixture.agent.plist.exists());
}

#[test]
fn hs2_hyf56y_a_missing_launchctl_is_no_desktop_session() {
    struct Missing;
    impl Launchctl for Missing {
        fn run(&self, _args: &[String]) -> io::Result<LaunchctlOutput> {
            Err(io::ErrorKind::NotFound.into())
        }
    }
    let fixture = fixture();
    let missing = Missing;
    let error = install(&fixture.agent, &missing, 501, QUICK).unwrap_err();
    assert!(
        matches!(&error, InstallError::NoGuiSession { reason } if reason.contains("unavailable")),
        "{error}"
    );
    assert!(!fixture.agent.plist.exists());
}

#[test]
fn hs2_hyf56y_a_failed_bootstrap_is_reported_with_launchctl_output() {
    let fixture = fixture();
    let launchctl = FakeLaunchctl {
        fail_bootstrap: true,
        ..FakeLaunchctl::aqua()
    };
    let error = install(&fixture.agent, &launchctl, 501, QUICK).unwrap_err();
    let message = error.to_string();
    assert!(
        matches!(
            error,
            InstallError::Launchctl {
                step: "bootstrap",
                ..
            }
        ),
        "{message}"
    );
    assert!(message.contains("Input/output error"), "{message}");
}

#[test]
fn hs2_hyf56y_a_loaded_but_silent_relay_is_restarted_then_reported() {
    let fixture = fixture();
    let launchctl = FakeLaunchctl::aqua();
    // Loaded with the current definition, but nothing answers on the socket.
    fs::create_dir_all(fixture.agent.plist.parent().unwrap()).unwrap();
    fs::write(&fixture.agent.plist, fixture.agent.render_plist()).unwrap();
    *launchctl.loaded.lock().unwrap() = true;
    let error = install(&fixture.agent, &launchctl, 501, QUICK).unwrap_err();
    assert!(matches!(error, InstallError::NotReady { .. }), "{error}");
    assert!(error.to_string().contains("terminal-relay.log"), "{error}");
    assert_eq!(launchctl.verbs(), ["managername", "print", "kickstart"]);
    assert_eq!(
        launchctl.calls.lock().unwrap()[2],
        ["kickstart", "-k", "gui/501/test.hotsheet2.terminal-relay"]
    );
}

#[test]
fn hs2_hyf56y_uninstall_boots_out_and_removes_then_is_a_no_op() {
    let fixture = fixture();
    let _relay = serve_relay_at(&fixture.agent.socket);
    let launchctl = FakeLaunchctl::aqua();
    install(&fixture.agent, &launchctl, 501, QUICK).unwrap();
    let before = status(&fixture.agent, &launchctl, 501);
    assert!(before.plist_present && before.plist_current && before.loaded);
    assert!(before.relay.is_some(), "{:?}", before.relay_error);

    launchctl.reset_calls();
    assert_eq!(
        uninstall(&fixture.agent, &launchctl, 501).unwrap(),
        UninstallOutcome::Removed
    );
    assert_eq!(launchctl.verbs(), ["print", "bootout"]);
    assert!(!fixture.agent.plist.exists());
    assert_eq!(
        uninstall(&fixture.agent, &launchctl, 501).unwrap(),
        UninstallOutcome::NotInstalled
    );
    let after = status(&fixture.agent, &launchctl, 501);
    assert!(!after.plist_present && !after.loaded && !after.healthy());
}

#[test]
fn hs2_hyf56y_status_flags_a_stale_definition() {
    let fixture = fixture();
    let launchctl = FakeLaunchctl::aqua();
    fs::create_dir_all(fixture.agent.plist.parent().unwrap()).unwrap();
    fs::write(&fixture.agent.plist, "<plist/>").unwrap();
    let report = status(&fixture.agent, &launchctl, 501);
    assert!(report.plist_present);
    assert!(!report.plist_current);
    assert!(!report.healthy());
    assert!(report.relay_error.is_some());
}

/// The test environment must never reach the real launchd or `~/Library/LaunchAgents`
/// (`.cargo/config.toml` `[env]`).
#[test]
fn hs2_hyf56y_tests_never_target_the_real_launch_agents() {
    let agents = default_agents_dir().unwrap();
    assert!(
        !agents.ends_with("Library/LaunchAgents"),
        "{}",
        agents.display()
    );
    let program = launchctl_program();
    assert_ne!(program, PathBuf::from("/bin/launchctl"));
    assert!(!program.exists(), "{}", program.display());
}
