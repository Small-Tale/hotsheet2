//! The PTY-owned OpenCode v2 server/TUI pair (HS2-W42KWS).
//!
//! `opencode --standalone` hides its random server address from Hot Sheet. This wrapper is
//! the terminal's foreground process: it starts an authenticated loopback `serve` child on
//! Hot Sheet's allocated port, creates the selected-model session, then attaches the real
//! TUI. The server child lives and dies with the TUI, including across a Hot Sheet server
//! restart while the detached terminal broker keeps this wrapper alive.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use hotsheet_aitools::session_push::{
    HttpPort, HttpRequest, OpenCodeV2Push, PushMessage, SessionPush, UreqHttp, basic_auth_header,
};
use serde_json::{Value, json};

const STARTUP_DEADLINE: Duration = Duration::from_secs(20);

struct ServerChild(Child);

impl Drop for ServerChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn value_arg(value: Option<OsString>, name: &str) -> Result<String> {
    value
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty())
        .with_context(|| format!("missing or invalid {name} for OpenCode v2 launch"))
}

pub(crate) fn run(mut args: impl Iterator<Item = OsString>) -> Result<()> {
    let program = PathBuf::from(value_arg(args.next(), "program")?);
    let port: u16 = value_arg(args.next(), "port")?
        .parse()
        .context("invalid OpenCode v2 port")?;
    let session_id = value_arg(args.next(), "session id")?;
    let model = args
        .next()
        .and_then(|value| value.into_string().ok())
        .filter(|value| !value.is_empty());
    let initial_prompt = args.next().and_then(|value| value.into_string().ok());
    if args.next().is_some() || !session_id.starts_with("ses_") {
        bail!("invalid OpenCode v2 launch arguments");
    }
    let password = std::env::var("OPENCODE_SERVER_PASSWORD")
        .context("OpenCode v2 launch has no session server password")?;
    let directory = std::env::current_dir().context("OpenCode v2 launch directory unavailable")?;
    let base_url = format!("http://127.0.0.1:{port}");
    let authorization = basic_auth_header("opencode", &password);
    let http = UreqHttp::new(Duration::from_millis(250));
    let server = Command::new(&program)
        .args(["serve", "--hostname", "127.0.0.1", "--port"])
        .arg(port.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit())
        .spawn()
        .with_context(|| format!("starting OpenCode v2 server at {}", program.display()))?;
    let mut server = ServerChild(server);
    wait_ready(&http, &base_url, &authorization, &mut server.0)?;
    create_session(
        &http,
        &base_url,
        &authorization,
        &session_id,
        &directory.to_string_lossy(),
        model.as_deref(),
    )?;
    if let Some(prompt) = initial_prompt {
        let push = OpenCodeV2Push::new(
            port,
            &password,
            session_id.clone(),
            std::sync::Arc::new(UreqHttp::new(Duration::from_secs(60))),
        );
        push.message(&PushMessage::Prompt {
            text: prompt,
            banner: None,
        })
        .map_err(|error| anyhow::anyhow!("sending the initial OpenCode v2 prompt: {error}"))?;
    }

    let status = Command::new(&program)
        .args(["--server", &base_url, "--session", &session_id])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("starting OpenCode v2 TUI")?;
    if !status.success() {
        bail!("OpenCode v2 TUI exited with {status}");
    }
    Ok(())
}

fn wait_ready(
    http: &dyn HttpPort,
    base_url: &str,
    authorization: &str,
    child: &mut Child,
) -> Result<()> {
    let deadline = Instant::now() + STARTUP_DEADLINE;
    loop {
        if let Some(status) = child.try_wait().context("checking OpenCode v2 server")? {
            bail!("OpenCode v2 server exited during startup with {status}");
        }
        if let Ok(response) = http.send(&HttpRequest {
            method: "GET",
            url: format!("{base_url}/api/info"),
            authorization: Some(authorization.into()),
            body: None,
        }) && response.is_success()
            && serde_json::from_str::<Value>(&response.body)
                .ok()
                .and_then(|info| info["pid"].as_u64())
                == Some(u64::from(child.id()))
        {
            return Ok(());
        }
        if Instant::now() >= deadline {
            bail!("OpenCode v2 server did not become ready on {base_url}");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn create_session(
    http: &dyn HttpPort,
    base_url: &str,
    authorization: &str,
    session_id: &str,
    directory: &str,
    model: Option<&str>,
) -> Result<()> {
    let model = model
        .map(|model| {
            let (provider, id) = model
                .split_once('/')
                .filter(|(provider, id)| !provider.is_empty() && !id.is_empty())
                .context("OpenCode model must be provider/model")?;
            Ok::<Value, anyhow::Error>(json!({ "providerID": provider, "id": id }))
        })
        .transpose()?;
    let mut body = json!({
        "id": session_id,
        "title": "Hot Sheet",
        "location": { "directory": directory },
    });
    if let Some(model) = model {
        body["model"] = model;
    }
    let response = http
        .send(&HttpRequest {
            method: "POST",
            url: format!("{base_url}/api/session"),
            authorization: Some(authorization.into()),
            body: Some(body),
        })
        .map_err(|error| anyhow::anyhow!("creating OpenCode v2 session: {error}"))?;
    if !response.is_success() {
        bail!(
            "OpenCode v2 rejected the session (HTTP {}): {}",
            response.status,
            response.body
        );
    }
    let created: Value = serde_json::from_str(&response.body)
        .context("OpenCode v2 returned an unreadable session")?;
    if created["data"]["id"].as_str() != Some(session_id) {
        bail!("OpenCode v2 returned a different session id");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_aitools::session_push::HttpResponse;
    use std::sync::Mutex;

    #[derive(Default)]
    struct RecordingHttp(Mutex<Vec<HttpRequest>>);

    impl HttpPort for RecordingHttp {
        fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
            self.0.lock().unwrap().push(request.clone());
            Ok(HttpResponse {
                status: 200,
                body: r#"{"data":{"id":"ses_synthetic"}}"#.into(),
            })
        }
    }

    #[test]
    fn hs2_w42kws_v2_session_starts_with_the_chosen_model_and_directory() {
        let http = RecordingHttp::default();
        create_session(
            &http,
            "http://127.0.0.1:4096",
            "Basic synthetic",
            "ses_synthetic",
            "/synthetic/project",
            Some("opencode/beam-free"),
        )
        .unwrap();
        let requests = http.0.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].url, "http://127.0.0.1:4096/api/session");
        assert_eq!(
            requests[0].body,
            Some(json!({
                "id": "ses_synthetic",
                "title": "Hot Sheet",
                "model": { "providerID": "opencode", "id": "beam-free" },
                "location": { "directory": "/synthetic/project" },
            }))
        );
    }

    #[test]
    fn hs2_w42kws_rejects_unqualified_model_before_creating_a_session() {
        let http = RecordingHttp::default();
        assert!(
            create_session(
                &http,
                "http://127.0.0.1:4096",
                "Basic synthetic",
                "ses_synthetic",
                "/synthetic/project",
                Some("beam-free"),
            )
            .is_err()
        );
        assert!(http.0.lock().unwrap().is_empty());
    }

    #[test]
    fn hs2_w42kws_omits_model_when_the_launch_uses_the_cli_default() {
        let http = RecordingHttp::default();
        create_session(
            &http,
            "http://127.0.0.1:4096",
            "Basic synthetic",
            "ses_synthetic",
            "/synthetic/project",
            None,
        )
        .unwrap();
        assert_eq!(
            http.0.lock().unwrap()[0]
                .body
                .as_ref()
                .unwrap()
                .get("model"),
            None
        );
    }
}
