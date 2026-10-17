//! Unit tests for session push: the manifest wiring and the OpenCode adapter's state
//! machine (session unknown → listed/created → cached → deleted → re-resolved) over a
//! scripted fake HTTP port.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use serde_json::json;

use super::*;

/// A fake port: answers each request from a per-route queue (`"POST /tui/show-toast"`) and
/// records every request it saw.
/// One route's scripted answers, in order.
type Script = VecDeque<Result<HttpResponse, String>>;

#[derive(Default)]
struct FakeHttp {
    routes: Mutex<Vec<(String, Script)>>,
    seen: Mutex<Vec<HttpRequest>>,
}

impl FakeHttp {
    fn on(&self, route: &str, status: u16, body: &str) -> &Self {
        self.push(
            route,
            Ok(HttpResponse {
                status,
                body: body.into(),
            }),
        )
    }

    fn fail(&self, route: &str, error: &str) -> &Self {
        self.push(route, Err(error.into()))
    }

    fn push(&self, route: &str, answer: Result<HttpResponse, String>) -> &Self {
        let mut routes = self.routes.lock().unwrap();
        match routes.iter_mut().find(|(key, _)| key == route) {
            Some((_, queue)) => queue.push_back(answer),
            None => routes.push((route.into(), VecDeque::from([answer]))),
        }
        self
    }

    fn seen(&self) -> Vec<HttpRequest> {
        self.seen.lock().unwrap().clone()
    }

    fn routes_seen(&self) -> Vec<String> {
        self.seen().iter().map(route_of).collect()
    }
}

fn route_of(request: &HttpRequest) -> String {
    let path = request
        .url
        .trim_start_matches("http://127.0.0.1:4100")
        .split('?')
        .next()
        .unwrap_or_default()
        .to_string();
    format!("{} {path}", request.method)
}

impl HttpPort for FakeHttp {
    fn send(&self, request: &HttpRequest) -> Result<HttpResponse, String> {
        self.seen.lock().unwrap().push(request.clone());
        let route = route_of(request);
        let mut routes = self.routes.lock().unwrap();
        let queue = routes
            .iter_mut()
            .find(|(key, _)| *key == route)
            .map(|(_, queue)| queue);
        match queue {
            // The last scripted answer repeats so a test scripts only what changes.
            Some(queue) if queue.len() > 1 => queue.pop_front().unwrap(),
            Some(queue) => queue.front().cloned().unwrap(),
            None => Ok(HttpResponse {
                status: 599,
                body: format!("unscripted {route}"),
            }),
        }
    }
}

const DIR: &str = "/work/my project";
const LAUNCHED: u64 = 1_000_000;

fn launch(model: Option<&str>) -> PushLaunch {
    PushLaunch {
        port: 4100,
        password: "pw-1".into(),
        directory: DIR.into(),
        model: model.map(str::to_string),
        launched_at_ms: LAUNCHED,
    }
}

fn adapter(http: &Arc<FakeHttp>, model: Option<&str>) -> OpenCodeHttpPush {
    OpenCodeHttpPush::new(launch(model), http.clone())
}

fn prompt(text: &str, banner: Option<&str>) -> PushMessage {
    PushMessage::Prompt {
        text: text.into(),
        banner: banner.map(str::to_string),
    }
}

fn session(id: &str, directory: &str, created: u64, updated: u64) -> serde_json::Value {
    json!({ "id": id, "directory": directory, "time": { "created": created, "updated": updated } })
}

fn spec(mechanism: &str) -> hotsheet_plugins::SessionPushSpec {
    hotsheet_plugins::SessionPushSpec {
        mechanism: mechanism.into(),
        port_args: vec!["--port".into(), "{port}".into()],
        password_env: Some("OPENCODE_SERVER_PASSWORD".into()),
    }
}

#[test]
fn the_bundled_opencode_manifest_declares_a_supported_mechanism_and_antigravity_none() {
    let plugins = hotsheet_plugins::builtin_plugins();
    let push = |id: &str| {
        plugins
            .iter()
            .find(|plugin| plugin.id() == id)
            .and_then(|plugin| plugin.manifest.session_push.clone())
    };
    let opencode = push("opencode").expect("opencode declares session push");
    assert!(is_supported(&opencode));
    assert_eq!(opencode.mechanism, OPENCODE_HTTP);
    assert!(
        push("antigravity").is_none(),
        "antigravity has no attach point"
    );
}

#[test]
fn launch_wiring_substitutes_the_port_and_exports_the_password() {
    let wiring = launch_wiring(&spec(OPENCODE_HTTP), &launch(None)).unwrap();
    assert_eq!(wiring.args, ["--port", "4100"]);
    assert_eq!(
        wiring.env,
        [("OPENCODE_SERVER_PASSWORD".to_string(), "pw-1".to_string())]
    );
}

#[test]
fn an_unknown_mechanism_has_no_wiring_and_no_adapter() {
    let unknown = spec("carrier-pigeon");
    assert!(!is_supported(&unknown));
    assert!(launch_wiring(&unknown, &launch(None)).is_none());
    assert!(adapter_for(&unknown, launch(None), Arc::new(FakeHttp::default())).is_none());
    let known = adapter_for(
        &spec(OPENCODE_HTTP),
        launch(None),
        Arc::new(FakeHttp::default()),
    );
    assert_eq!(known.unwrap().mechanism(), OPENCODE_HTTP);
}

#[test]
fn allocate_loopback_port_returns_a_bindable_port() {
    let port = allocate_loopback_port().unwrap();
    assert!(port > 0);
    std::net::TcpListener::bind(("127.0.0.1", port)).unwrap();
}

#[test]
fn basic_auth_and_query_escape_match_the_wire_format() {
    // `opencode:spikepw` is what the spike's curl sent.
    assert_eq!(
        basic_auth_header("opencode", "spikepw"),
        "Basic b3BlbmNvZGU6c3Bpa2Vwdw=="
    );
    assert_eq!(basic_auth_header("a", "b"), "Basic YTpi");
    assert_eq!(basic_auth_header("ab", "c"), "Basic YWI6Yw==");
    assert_eq!(
        query_escape("/work/my project&x=1"),
        "/work/my%20project%26x%3D1"
    );
}

#[test]
fn the_first_prompt_uses_the_users_session_from_this_launch_and_caches_it() {
    let http = Arc::new(FakeHttp::default());
    let sessions = json!([
        // Another directory, an earlier launch, and a child session never qualify.
        session("ses_other_dir", "/elsewhere", LAUNCHED + 10, LAUNCHED + 900),
        session("ses_before_launch", DIR, LAUNCHED - 60_000, LAUNCHED + 800),
        { "id": "ses_child", "parentID": "ses_users", "directory": DIR,
          "time": { "created": LAUNCHED + 30, "updated": LAUNCHED + 700 } },
        session("ses_users", DIR, LAUNCHED + 20, LAUNCHED + 500),
        session("ses_older_users", DIR, LAUNCHED + 5, LAUNCHED + 100),
    ]);
    http.on("GET /session", 200, &sessions.to_string()).on(
        "POST /session/ses_users/prompt_async",
        204,
        "",
    );
    let push = adapter(&http, None);
    assert_eq!(push.session_id(), None);

    let receipt = push.message(&prompt("New note on HS2-1", None)).unwrap();
    assert_eq!(
        receipt,
        PushReceipt {
            mechanism: OPENCODE_HTTP,
            session_id: Some("ses_users".into()),
            confirmed: true
        }
    );
    // Second message: no second lookup.
    push.message(&prompt("again", None)).unwrap();
    assert_eq!(
        http.routes_seen(),
        [
            "GET /session",
            "POST /session/ses_users/prompt_async",
            "POST /session/ses_users/prompt_async"
        ]
    );
    let first = &http.seen()[0];
    assert!(
        first
            .url
            .ends_with("roots=true&directory=/work/my%20project"),
        "{}",
        first.url
    );
    assert_eq!(
        first.authorization.as_deref(),
        Some(basic_auth_header("opencode", "pw-1").as_str())
    );
    assert_eq!(push.session_id().as_deref(), Some("ses_users"));
}

#[test]
fn with_no_session_yet_the_adapter_creates_one_and_shows_it_in_the_tui() {
    let http = Arc::new(FakeHttp::default());
    http.on("GET /session", 200, "[]")
        .on(
            "POST /session",
            200,
            r#"{"id":"ses_new","directory":"/work/my project"}"#,
        )
        .on("POST /tui/select-session", 200, "true")
        .on("POST /session/ses_new/prompt_async", 204, "");
    let receipt = adapter(&http, None)
        .message(&prompt("hello", None))
        .unwrap();
    assert_eq!(receipt.session_id.as_deref(), Some("ses_new"));
    assert_eq!(
        http.routes_seen(),
        [
            "GET /session",
            "POST /session",
            "POST /tui/select-session",
            "POST /session/ses_new/prompt_async"
        ]
    );
    let seen = http.seen();
    assert_eq!(seen[1].body, Some(json!({ "title": "Hot Sheet" })));
    assert_eq!(seen[2].body, Some(json!({ "sessionID": "ses_new" })));
}

#[test]
fn a_failed_tui_select_does_not_block_delivery() {
    let http = Arc::new(FakeHttp::default());
    http.on("GET /session", 200, "[]")
        .on("POST /session", 200, r#"{"id":"ses_new"}"#)
        .fail("POST /tui/select-session", "connection reset")
        .on("POST /session/ses_new/prompt_async", 204, "");
    assert!(adapter(&http, None).message(&prompt("hello", None)).is_ok());
}

#[test]
fn a_created_session_without_an_id_is_rejected() {
    let http = Arc::new(FakeHttp::default());
    http.on("GET /session", 200, "[]")
        .on("POST /session", 200, "{}");
    let error = adapter(&http, None)
        .message(&prompt("hello", None))
        .unwrap_err();
    assert!(
        matches!(error, PushError::Rejected { status: 200, .. }),
        "{error:?}"
    );
}

#[test]
fn an_unreadable_session_list_is_rejected() {
    let http = Arc::new(FakeHttp::default());
    http.on("GET /session", 200, "<html>");
    let error = adapter(&http, None)
        .message(&prompt("hello", None))
        .unwrap_err();
    assert!(
        error.to_string().contains("unreadable session list"),
        "{error}"
    );
}

#[test]
fn the_launch_model_rides_every_prompt_and_a_bare_model_is_ignored() {
    let http = Arc::new(FakeHttp::default());
    http.on(
        "GET /session",
        200,
        &json!([session("ses_a", DIR, LAUNCHED, LAUNCHED)]).to_string(),
    )
    .on("POST /session/ses_a/prompt_async", 204, "");
    adapter(&http, Some("opencode/big-pickle"))
        .message(&prompt("hi", None))
        .unwrap();
    assert_eq!(
        http.seen()[1].body,
        Some(json!({
            "parts": [{ "type": "text", "text": "hi" }],
            "model": { "providerID": "opencode", "modelID": "big-pickle" }
        }))
    );

    let bare = Arc::new(FakeHttp::default());
    bare.on(
        "GET /session",
        200,
        &json!([session("ses_a", DIR, LAUNCHED, LAUNCHED)]).to_string(),
    )
    .on("POST /session/ses_a/prompt_async", 204, "");
    adapter(&bare, Some("big-pickle"))
        .message(&prompt("hi", None))
        .unwrap();
    assert_eq!(
        bare.seen()[1].body,
        Some(json!({ "parts": [{ "type": "text", "text": "hi" }] }))
    );
}

#[test]
fn a_banner_shows_a_toast_before_the_prompt_and_its_failure_is_tolerated() {
    let http = Arc::new(FakeHttp::default());
    http.on(
        "GET /session",
        200,
        &json!([session("ses_a", DIR, LAUNCHED, LAUNCHED)]).to_string(),
    )
    .on("POST /tui/show-toast", 500, "boom")
    .on("POST /session/ses_a/prompt_async", 204, "");
    adapter(&http, None)
        .message(&prompt("details", Some("New note on HS2-1")))
        .unwrap();
    assert_eq!(
        http.routes_seen(),
        [
            "GET /session",
            "POST /tui/show-toast",
            "POST /session/ses_a/prompt_async"
        ]
    );
    assert_eq!(
        http.seen()[1].body,
        Some(json!({ "title": "Hot Sheet", "message": "New note on HS2-1", "variant": "info" }))
    );

    // A blank banner shows no toast.
    let quiet = Arc::new(FakeHttp::default());
    quiet
        .on(
            "GET /session",
            200,
            &json!([session("ses_a", DIR, LAUNCHED, LAUNCHED)]).to_string(),
        )
        .on("POST /session/ses_a/prompt_async", 204, "");
    adapter(&quiet, None)
        .message(&prompt("details", Some("  ")))
        .unwrap();
    assert!(
        !quiet
            .routes_seen()
            .contains(&"POST /tui/show-toast".to_string())
    );
}

#[test]
fn a_deleted_session_is_re_resolved_once_then_cached_again() {
    let http = Arc::new(FakeHttp::default());
    http.on(
        "GET /session",
        200,
        &json!([session("ses_old", DIR, LAUNCHED, LAUNCHED)]).to_string(),
    )
    .on(
        "GET /session",
        200,
        &json!([session("ses_fresh", DIR, LAUNCHED + 5, LAUNCHED + 9)]).to_string(),
    )
    .on("POST /session/ses_old/prompt_async", 204, "")
    .on(
        "POST /session/ses_old/prompt_async",
        404,
        r#"{"name":"NotFoundError"}"#,
    )
    .on("POST /session/ses_fresh/prompt_async", 204, "");
    let push = adapter(&http, None);
    push.message(&prompt("one", None)).unwrap();
    let receipt = push.message(&prompt("two", None)).unwrap();
    assert_eq!(receipt.session_id.as_deref(), Some("ses_fresh"));
    push.message(&prompt("three", None)).unwrap();
    assert_eq!(
        http.routes_seen(),
        [
            "GET /session",
            "POST /session/ses_old/prompt_async",
            "POST /session/ses_old/prompt_async",
            "GET /session",
            "POST /session/ses_fresh/prompt_async",
            "POST /session/ses_fresh/prompt_async",
        ]
    );
}

#[test]
fn a_second_404_after_re_resolving_is_reported() {
    let http = Arc::new(FakeHttp::default());
    http.on(
        "GET /session",
        200,
        &json!([session("ses_a", DIR, LAUNCHED, LAUNCHED)]).to_string(),
    )
    .on("POST /session/ses_a/prompt_async", 404, "gone");
    let error = adapter(&http, None)
        .message(&prompt("x", None))
        .unwrap_err();
    assert_eq!(
        error,
        PushError::Rejected {
            status: 404,
            body: "gone".into()
        }
    );
}

#[test]
fn auth_failures_and_unreachable_sessions_are_distinct_errors() {
    let denied = Arc::new(FakeHttp::default());
    denied.on("GET /session", 401, "");
    let error = adapter(&denied, None)
        .message(&prompt("x", None))
        .unwrap_err();
    assert_eq!(
        error,
        PushError::Rejected {
            status: 401,
            body: String::new()
        }
    );

    let gone = Arc::new(FakeHttp::default());
    gone.fail("GET /session", "Connection refused");
    let error = adapter(&gone, None)
        .message(&prompt("x", None))
        .unwrap_err();
    assert_eq!(error, PushError::Unreachable("Connection refused".into()));
    assert!(error.to_string().starts_with("session unreachable"));
}

#[test]
fn empty_messages_are_invalid_without_any_request() {
    let http = Arc::new(FakeHttp::default());
    let push = adapter(&http, None);
    assert!(matches!(
        push.message(&prompt("  ", None)),
        Err(PushError::Invalid(_))
    ));
    assert!(matches!(
        push.message(&PushMessage::Command { command: "".into() }),
        Err(PushError::Invalid(_))
    ));
    assert!(http.seen().is_empty());
}

#[test]
fn a_command_runs_in_the_tui_unconfirmed_and_needs_no_session() {
    let http = Arc::new(FakeHttp::default());
    http.on("POST /tui/execute-command", 200, "true");
    let push = adapter(&http, None);
    let receipt = push
        .message(&PushMessage::Command {
            command: " agent_cycle ".into(),
        })
        .unwrap();
    assert_eq!(
        receipt,
        PushReceipt {
            mechanism: OPENCODE_HTTP,
            session_id: None,
            confirmed: false
        }
    );
    assert_eq!(
        http.seen()[0].body,
        Some(json!({ "command": "agent_cycle" }))
    );

    let failing = Arc::new(FakeHttp::default());
    failing.on("POST /tui/execute-command", 400, "bad");
    assert!(matches!(
        adapter(&failing, None).message(&PushMessage::Command {
            command: "x".into()
        }),
        Err(PushError::Rejected { status: 400, .. })
    ));
}
