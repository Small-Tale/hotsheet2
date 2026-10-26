//! Desktop shell for the existing production web client (HS2-5R706Q).
//! The local bridge belongs to this window; the Rust service it starts is independent.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tauri::{Manager, RunEvent, WebviewUrl, WebviewWindowBuilder, WindowEvent};

mod close_lifecycle;
use close_lifecycle::{CloseLifecycle, caller_allowed};

struct BridgeChild(Mutex<Child>);
struct DesktopClose {
    origin: String,
    lifecycle: Mutex<CloseLifecycle>,
}

#[tauri::command]
fn desktop_flush_complete(
    window: tauri::WebviewWindow,
    app: tauri::AppHandle,
    state: tauri::State<'_, DesktopClose>,
    request: String,
    success: bool,
) -> Result<(), String> {
    // The dynamic ACL also admits only this launch's exact origin and main webview.
    // Recheck the actual caller before mutating the close generation (HS2-E0H7S0).
    let webview: &tauri::Webview = window.as_ref();
    let actual_origin = window
        .url()
        .map_err(|_| "Could not validate desktop caller.")?
        .origin()
        .ascii_serialization();
    if !caller_allowed(
        webview.window().label(),
        webview.label(),
        &actual_origin,
        &state.origin,
    ) {
        return Err("Invalid desktop close caller.".into());
    }
    let exit = state
        .lifecycle
        .lock()
        .map_err(|_| "Could not validate desktop close request.")?
        .acknowledge(&request, success)?;
    if exit {
        app.exit(0);
    }
    Ok(())
}

fn request_desktop_close(app: &tauri::AppHandle) {
    let Some(state) = app.try_state::<DesktopClose>() else {
        return;
    };
    // HS2-E0H7S0: boot/missing-window requests must not occupy a generation forever.
    let window = app.get_webview_window("main");
    let Ok(mut lifecycle) = state.lifecycle.lock() else {
        return;
    };
    let Some(request) = lifecycle.begin(window.is_some()) else {
        return;
    };
    drop(lifecycle);
    let Some(window) = window else {
        return;
    };
    let token = serde_json::to_string(&request).expect("close token is a string");
    // Listener-not-ready fails closed under the same bounded deadline as a stalled PUT.
    let _ = window.eval(format!("window.__HOTSHEET_DESKTOP_CLOSE__?.({token});"));
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(30));
        let Some(state) = app.try_state::<DesktopClose>() else {
            return;
        };
        let Ok(mut lifecycle) = state.lifecycle.lock() else {
            return;
        };
        if !lifecycle.cancel(&request) {
            return;
        }
        drop(lifecycle);
        if let Some(window) = app.get_webview_window("main") {
            let _ = window.eval(format!(
                "if (window.__HOTSHEET_DESKTOP_CLOSE_CANCEL__) {{ window.__HOTSHEET_DESKTOP_CLOSE_CANCEL__({token}); }} else {{ alert('Local changes could not be saved. Hot Sheet will stay open. Try closing again.'); }}"
            ));
        }
    });
}

fn required(root: &Path, relative: &str) -> Result<PathBuf, Box<dyn Error>> {
    let path = root.join(relative);
    if !path.is_file() {
        return Err(format!("Hot Sheet desktop resource is missing: {relative}").into());
    }
    Ok(path)
}

fn launch_bridge(
    root: &Path,
    binaries: &Path,
    storage: &Path,
) -> Result<(Child, tauri::Url), Box<dyn Error>> {
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let ready = std::env::temp_dir().join(format!(
        "hotsheet-desktop-{}-{nonce}.json",
        std::process::id()
    ));
    let node = required(binaries, "node")?;
    let script = required(root, "dist-host/local-host.mjs")?;
    let mut child = Command::new(node)
        .arg(script)
        .args(["--host", "127.0.0.1", "--port", "0"])
        .env("HOTSHEET_REPO_ROOT", root)
        .env("HOTSHEET_DESKTOP_STORAGE_DIRECTORY", storage)
        .env(
            "HOTSHEET_SERVER_BIN",
            required(binaries, "hotsheet-server")?,
        )
        .env("HOTSHEET_CLI_BIN", required(binaries, "hotsheet-cli")?)
        .env(
            "HOTSHEET_MIGRATE_BIN",
            required(binaries, "hotsheet-migrate")?,
        )
        .env("HOTSHEET_MCP_BIN", required(binaries, "hotsheet-mcp")?)
        .env(
            "HOTSHEET_MIGRATOR",
            required(root, "migrator/src/export.mjs")?,
        )
        .env("HOTSHEET_CLIENT_READY_FILE", &ready)
        .env("HOTSHEET_HOST_EXIT_ON_STDIN_EOF", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()?;

    let deadline = Instant::now() + Duration::from_secs(30);
    let result = loop {
        if let Ok(contents) = fs::read_to_string(&ready) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&contents) {
                if let Some(url) = value.get("url").and_then(serde_json::Value::as_str) {
                    break tauri::Url::parse(url).map_err(|error| error.into());
                }
            }
        }
        if let Some(status) = child.try_wait()? {
            break Err(format!("Hot Sheet local bridge exited during startup: {status}").into());
        }
        if Instant::now() >= deadline {
            break Err("Hot Sheet local bridge did not start within 30 seconds".into());
        }
        thread::sleep(Duration::from_millis(50));
    };
    let _ = fs::remove_file(&ready);
    match result {
        Ok(url) => Ok((child, url)),
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            Err(error)
        }
    }
}

fn stop_bridge(child: &Mutex<Child>) {
    let Ok(mut child) = child.lock() else { return };
    // Closing the pipe lets the Node bridge close its sockets and remove its
    // temporary snapshot. The independently launched Rust server stays alive.
    child.stdin.take();
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            return;
        }
        thread::sleep(Duration::from_millis(50));
    }
    let _ = child.kill();
    let _ = child.wait();
}

pub fn run() {
    let app = tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![desktop_flush_complete])
        .setup(|app| {
            let url = if cfg!(debug_assertions) {
                tauri::Url::parse("http://127.0.0.1:4175/")?
            } else {
                let root = app.path().resource_dir()?;
                let executable = std::env::current_exe()?;
                let binaries = executable
                    .parent()
                    .ok_or("Hot Sheet executable has no parent directory")?;
                let storage = match std::env::var_os("HOTSHEET_DESKTOP_STORAGE_DIRECTORY") {
                    Some(directory) => PathBuf::from(directory),
                    None => app.path().app_data_dir()?.join("browser-preferences-v1"),
                };
                let (child, url) = launch_bridge(&root, binaries, &storage)?;
                app.manage(BridgeChild(Mutex::new(child)));
                let origin = url.origin().ascii_serialization();
                app.add_capability(
                    serde_json::json!({
                        "identifier": "desktop-local-close",
                        "windows": ["main"],
                        "webviews": ["main"],
                        "local": false,
                        "remote": { "urls": [format!("{origin}/*")] },
                        "permissions": ["allow-desktop-flush-complete"]
                    })
                    .to_string(),
                )?;
                app.manage(DesktopClose {
                    origin,
                    lifecycle: Mutex::new(CloseLifecycle::default()),
                });
                url
            };
            let mut window = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url));
            if !cfg!(debug_assertions) {
                window =
                    window.initialization_script("window.__HOTSHEET_DESKTOP_STORAGE__ = true;");
            }
            window
                .title("Hot Sheet 2")
                .inner_size(1280.0, 840.0)
                .min_inner_size(360.0, 560.0)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("failed to build Hot Sheet desktop app");
    app.run(|app, event| {
        match &event {
            RunEvent::WindowEvent {
                label,
                event: WindowEvent::CloseRequested { api, .. },
                ..
            } if label == "main" && app.try_state::<DesktopClose>().is_some() => {
                api.prevent_close();
                request_desktop_close(app);
            }
            RunEvent::ExitRequested { api, .. } => {
                if let Some(state) = app.try_state::<DesktopClose>() {
                    let allowed = state
                        .lifecycle
                        .lock()
                        .map(|gate| gate.allowed())
                        .unwrap_or(false);
                    if !allowed {
                        api.prevent_exit();
                        request_desktop_close(app);
                    }
                }
            }
            _ => {}
        }
        if let RunEvent::Exit = event {
            if let Some(child) = app.try_state::<BridgeChild>() {
                stop_bridge(&child.0);
            }
        }
    });
}
