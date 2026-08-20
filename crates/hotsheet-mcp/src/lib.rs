//! MCP shim: a stdio JSON-RPC 2.0 server exposing the `hotsheet_*` tools, each
//! proxying a running `hotsheet-server` over HTTP (`docs/05-ai-tool-plugins.md`
//! §5.8). Per-project — an AI tool spawns it from its own config pointed at the
//! project's server URL + secret (the HS1 `channel.ts` model). Kept out of the server
//! so the tool config owns which project it reaches.

use serde_json::{Value, json};

/// The MCP protocol revision we speak.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// The HTTP backend a tool call proxies to (real in the binary; faked in tests).
pub trait Backend {
    fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError>;
    fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError>;
}

/// A failed backend call: an HTTP status (when the server answered) + a message.
#[derive(Debug)]
pub struct BackendError {
    pub status: Option<u16>,
    pub message: String,
}

/// Handle one JSON-RPC message. Returns the response value, or `None` for a
/// notification (no `id`). Transport-level infallible.
pub fn handle_message(msg: &Value, backend: &dyn Backend) -> Option<Value> {
    let id = msg.get("id").cloned()?; // notifications (no id) get no reply
    let method = msg.get("method").and_then(Value::as_str).unwrap_or("");

    let result = match method {
        "initialize" => Ok(initialize_result()),
        "tools/list" => Ok(tools_list()),
        "tools/call" => call_tool(msg.get("params"), backend),
        "ping" => Ok(json!({})),
        other => Err(RpcError::method_not_found(other)),
    };
    Some(match result {
        Ok(value) => json!({ "jsonrpc": "2.0", "id": id, "result": value }),
        Err(e) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": e.code, "message": e.message } })
        }
    })
}

struct RpcError {
    code: i64,
    message: String,
}

impl RpcError {
    fn method_not_found(m: &str) -> Self {
        Self {
            code: -32601,
            message: format!("method not found: {m}"),
        }
    }
    fn invalid_params(m: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: m.into(),
        }
    }
}

fn initialize_result() -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": {} },
        "serverInfo": { "name": "hotsheet-mcp", "version": env!("CARGO_PKG_VERSION") }
    })
}

fn tools_list() -> Value {
    let str_prop = |desc: &str| json!({ "type": "string", "description": desc });
    json!({ "tools": [
        {
            "name": "hotsheet_query",
            "description": "List/filter tickets (status, priority, category, tags, text, up_next, open, sort).",
            "inputSchema": { "type": "object", "properties": {
                "status": str_prop("filter by status"),
                "priority": str_prop("filter by priority"),
                "category": str_prop("filter by category"),
                "tags": str_prop("comma-separated tags the ticket must all carry"),
                "text": str_prop("substring across title/details/notes"),
                "up_next": { "type": "boolean" },
                "open": { "type": "boolean" },
                "sort": str_prop("id|created|updated|priority|status|title")
            } }
        },
        {
            "name": "hotsheet_get",
            "description": "Fetch one ticket by slug or ULID.",
            "inputSchema": { "type": "object", "properties": { "id": str_prop("slug or ULID") }, "required": ["id"] }
        },
        {
            "name": "hotsheet_create",
            "description": "Create a ticket.",
            "inputSchema": { "type": "object", "properties": {
                "title": str_prop("required"),
                "category": str_prop(""), "priority": str_prop(""),
                "details": str_prop(""), "tags": { "type": "array", "items": { "type": "string" } },
                "up_next": { "type": "boolean" }
            }, "required": ["title"] }
        },
        {
            "name": "hotsheet_update",
            "description": "Update a ticket's fields (title/details/category/priority/status/tags/up_next).",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("slug or ULID"),
                "title": str_prop(""), "details": str_prop(""), "category": str_prop(""),
                "priority": str_prop(""), "status": str_prop(""),
                "tags": { "type": "array", "items": { "type": "string" } }, "up_next": { "type": "boolean" }
            }, "required": ["id"] }
        },
        {
            "name": "hotsheet_close",
            "description": "Record a close outcome (completed|not_planned|duplicate|obsolete).",
            "inputSchema": { "type": "object", "properties": {
                "id": str_prop("slug or ULID"),
                "reason": str_prop("completed|not_planned|duplicate|obsolete"),
                "duplicate_of": str_prop("required when reason=duplicate")
            }, "required": ["id", "reason"] }
        }
    ] })
}

fn call_tool(params: Option<&Value>, backend: &dyn Backend) -> Result<Value, RpcError> {
    let params = params.ok_or_else(|| RpcError::invalid_params("missing params"))?;
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| RpcError::invalid_params("missing tool name"))?;
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    // Tool-level failures are reported as an `isError` result, not an RPC error.
    Ok(match dispatch(name, &args, backend) {
        Ok(value) => tool_text(
            &serde_json::to_string_pretty(&value).unwrap_or_default(),
            false,
        ),
        Err(message) => tool_text(&message, true),
    })
}

fn dispatch(name: &str, args: &Value, backend: &dyn Backend) -> Result<Value, String> {
    match name {
        "hotsheet_query" => backend.get("/tickets", &query_pairs(args)).map_err(be_msg),
        "hotsheet_get" => backend
            .get(&format!("/tickets/{}", arg_str(args, "id")?), &[])
            .map_err(be_msg),
        "hotsheet_create" => backend.send("POST", "/tickets", args).map_err(be_msg),
        "hotsheet_update" => {
            let id = arg_str(args, "id")?;
            backend
                .send("PATCH", &format!("/tickets/{id}"), &without(args, "id"))
                .map_err(be_msg)
        }
        "hotsheet_close" => {
            let id = arg_str(args, "id")?;
            let body = json!({
                "reason": arg_str(args, "reason")?,
                "duplicate_of": args.get("duplicate_of").cloned().unwrap_or(Value::Null),
            });
            backend
                .send("POST", &format!("/tickets/{id}/close"), &body)
                .map_err(be_msg)
        }
        other => Err(format!("unknown tool: {other}")),
    }
}

fn query_pairs(args: &Value) -> Vec<(String, String)> {
    let mut pairs = Vec::new();
    for key in [
        "status", "priority", "category", "tags", "text", "up_next", "open", "sort",
    ] {
        if let Some(v) = args.get(key) {
            let s = match v {
                Value::String(s) => s.clone(),
                Value::Bool(b) => b.to_string(),
                Value::Null => continue,
                other => other.to_string(),
            };
            if !s.is_empty() {
                pairs.push((key.to_string(), s));
            }
        }
    }
    pairs
}

fn arg_str(args: &Value, key: &str) -> Result<String, String> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .ok_or_else(|| format!("missing required argument '{key}'"))
}

fn without(args: &Value, key: &str) -> Value {
    let mut v = args.clone();
    if let Some(obj) = v.as_object_mut() {
        obj.remove(key);
    }
    v
}

fn be_msg(e: BackendError) -> String {
    match e.status {
        Some(code) => format!("server error {code}: {}", e.message),
        None => format!("could not reach server: {}", e.message),
    }
}

fn tool_text(text: &str, is_error: bool) -> Value {
    json!({ "content": [{ "type": "text", "text": text }], "isError": is_error })
}

// ---- HTTP backend ----------------------------------------------------------------

/// Proxies to a running server over HTTP with the shared secret.
pub struct HttpBackend {
    base: String,
    secret: String,
    agent: ureq::Agent,
}

impl HttpBackend {
    pub fn new(base: impl Into<String>, secret: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            secret: secret.into(),
            agent: ureq::agent(),
        }
    }
}

impl Backend for HttpBackend {
    fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
        let mut req = self
            .agent
            .get(&format!("{}{path}", self.base))
            .set("X-Hotsheet-Secret", &self.secret);
        for (k, v) in query {
            req = req.query(k, v);
        }
        exec(req.call())
    }

    fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
        let req = self
            .agent
            .request(method, &format!("{}{path}", self.base))
            .set("X-Hotsheet-Secret", &self.secret)
            .set("Content-Type", "application/json");
        let payload = serde_json::to_string(body).unwrap_or_else(|_| "{}".to_string());
        exec(req.send_string(&payload))
    }
}

fn exec(res: Result<ureq::Response, ureq::Error>) -> Result<Value, BackendError> {
    match res {
        Ok(resp) => {
            let text = resp.into_string().unwrap_or_default();
            Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
        }
        Err(ureq::Error::Status(code, resp)) => {
            let text = resp.into_string().unwrap_or_default();
            let message = serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|v| v.get("error").and_then(Value::as_str).map(String::from))
                .unwrap_or(text);
            Err(BackendError {
                status: Some(code),
                message,
            })
        }
        Err(e) => Err(BackendError {
            status: None,
            message: e.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct FakeBackend {
        calls: RefCell<Vec<String>>,
    }
    impl Backend for FakeBackend {
        fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls
                .borrow_mut()
                .push(format!("GET {path} {query:?}"));
            Ok(json!([{ "slug": "HS-AAA111", "title": "t" }]))
        }
        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            self.calls
                .borrow_mut()
                .push(format!("{method} {path} {body}"));
            Ok(json!({ "slug": "HS-NEW999", "title": body.get("title") }))
        }
    }

    fn req(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    #[test]
    fn initialize_advertises_tools() {
        let r = handle_message(&req("initialize", json!({})), &FakeBackend::default()).unwrap();
        assert_eq!(r["result"]["protocolVersion"], PROTOCOL_VERSION);
        assert_eq!(r["result"]["serverInfo"]["name"], "hotsheet-mcp");
        assert!(r["result"]["capabilities"]["tools"].is_object());
    }

    #[test]
    fn tools_list_has_the_hotsheet_tools() {
        let r = handle_message(&req("tools/list", json!({})), &FakeBackend::default()).unwrap();
        let names: Vec<&str> = r["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for want in [
            "hotsheet_query",
            "hotsheet_get",
            "hotsheet_create",
            "hotsheet_update",
            "hotsheet_close",
        ] {
            assert!(names.contains(&want), "missing {want}");
        }
    }

    #[test]
    fn a_notification_gets_no_reply() {
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_message(&note, &FakeBackend::default()).is_none());
    }

    #[test]
    fn unknown_method_is_an_rpc_error() {
        let r = handle_message(&req("nope", json!({})), &FakeBackend::default()).unwrap();
        assert_eq!(r["error"]["code"], -32601);
    }

    #[test]
    fn create_tool_proxies_a_post() {
        let backend = FakeBackend::default();
        let r = handle_message(
            &req(
                "tools/call",
                json!({ "name": "hotsheet_create", "arguments": { "title": "Fix it" } }),
            ),
            &backend,
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], false);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("HS-NEW999")
        );
        assert!(backend.calls.borrow()[0].starts_with("POST /tickets"));
        assert!(backend.calls.borrow()[0].contains("Fix it"));
    }

    #[test]
    fn query_tool_forwards_filters() {
        let backend = FakeBackend::default();
        handle_message(
            &req(
                "tools/call",
                json!({ "name": "hotsheet_query", "arguments": { "status": "started", "text": "x" } }),
            ),
            &backend,
        );
        let call = backend.calls.borrow()[0].clone();
        assert!(call.starts_with("GET /tickets"));
        assert!(call.contains("status"));
        assert!(call.contains("started"));
    }

    #[test]
    fn missing_required_argument_is_a_tool_error() {
        let r = handle_message(
            &req(
                "tools/call",
                json!({ "name": "hotsheet_get", "arguments": {} }),
            ),
            &FakeBackend::default(),
        )
        .unwrap();
        assert_eq!(r["result"]["isError"], true);
        assert!(
            r["result"]["content"][0]["text"]
                .as_str()
                .unwrap()
                .contains("missing required argument 'id'")
        );
    }
}
