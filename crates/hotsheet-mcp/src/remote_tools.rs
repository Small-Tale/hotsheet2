//! HS2-1Y45MZ: machine-local remote enrollment; tool arguments contain file names, not secrets.

use serde_json::{Value, json};

use crate::{Backend, BackendError};

pub(crate) fn definition() -> Value {
    json!({
        "name": "hotsheet_remotes",
        "description": "Manage this machine's explicitly paired remote Hot Sheet servers. list performs no requests; checkouts performs fresh authenticated discovery. Registration/review imports a local credential JSON file without returning its contents. Only local backends support this tool; credentials are never copied to a remote backend. Revoke disables local access and does not revoke the issued remote device certificate.",
        "inputSchema": { "type": "object", "required": ["action"], "properties": {
            "action": { "type": "string", "enum": ["identity", "list", "register", "review", "revoke", "checkouts"] },
            "id": { "type": "string", "description": "immutable registration ULID, required for mutation/discovery" },
            "label": { "type": "string" }, "origin": { "type": "string" },
            "expected_server_id": { "type": "string", "description": "server ULID explicitly supplied by pairing" },
            "credentials_file": { "type": "string", "description": "local file containing bearer_secret and HTTPS ca_pem/client_certificate_pem/client_key_pem; never put credential values in tool arguments" },
            "expected_revision": { "type": "integer", "minimum": 1 },
            "previous_identity": { "type": "object", "required": ["server_id", "ca_fingerprint"], "properties": {
                "server_id": { "type": "string" }, "ca_fingerprint": { "type": ["string", "null"] }
            } }
        } }
    })
}

pub(crate) fn is_registry_path(path: &str) -> bool {
    path == "/remotes" || path.starts_with("/remotes/") || path == "/remote-identity"
}

pub(crate) fn backend_error(
    error: hotsheet_ticketing::remote_servers::RemoteError,
) -> BackendError {
    use hotsheet_ticketing::remote_servers::RemoteError;
    let status = match error {
        RemoteError::Invalid(_) => 400,
        RemoteError::Conflict | RemoteError::Revoked => 409,
        RemoteError::NotFound => 404,
        RemoteError::Verification(_) => 502,
        RemoteError::Credentials => 503,
        RemoteError::UnsupportedVersion => 426,
        RemoteError::Storage(_) | RemoteError::Corrupt => 500,
    };
    BackendError {
        status: Some(status),
        message: error.to_string(),
    }
}

pub(crate) fn dispatch(args: &Value, backend: &dyn Backend) -> Result<Value, String> {
    if !backend.machine_local() {
        return Err("remote registry administration requires a machine-local backend; credentials were not read or copied".into());
    }
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .ok_or("action is required")?;
    if action == "list" {
        return backend.get("/remotes", &[]).map_err(crate::be_msg);
    }
    if action == "identity" {
        return backend.get("/remote-identity", &[]).map_err(crate::be_msg);
    }
    let id = args
        .get("id")
        .and_then(Value::as_str)
        .ok_or("id is required")?;
    id.parse::<hotsheet_model::Ulid>()
        .map_err(|_| "id must be a ULID".to_string())?;
    if action == "checkouts" {
        return backend
            .get(&format!("/remotes/{id}/checkouts"), &[])
            .map_err(crate::be_msg);
    }
    let mut body = args.clone();
    let object = body.as_object_mut().ok_or("arguments must be an object")?;
    object.remove("action");
    object.remove("id");
    if matches!(action, "register" | "review") {
        if object.contains_key("credentials") {
            return Err(
                "use a local credential file, not credential values in tool arguments".into(),
            );
        }
        let file = object
            .remove("credentials_file")
            .and_then(|value| value.as_str().map(str::to_owned))
            .ok_or("credentials_file is required")?;
        if file == "-" {
            return Err(
                "MCP credential input requires a local file; stdin belongs to the MCP protocol"
                    .into(),
            );
        }
        let input = std::fs::File::open(file)
            .map_err(|_| "could not read local credential file".to_string())?;
        let credentials = hotsheet_cli::remote_servers::read_remote_credentials(input)
            .map_err(|error| error.to_string())?;
        object.insert(
            "credentials".into(),
            serde_json::to_value(credentials)
                .map_err(|_| "invalid credential input".to_string())?,
        );
    }
    if action == "register" {
        object.insert("id".into(), Value::String(id.into()));
        backend
            .send("POST", "/remotes", &body)
            .map_err(crate::be_msg)
    } else if action == "review" {
        backend
            .send("PUT", &format!("/remotes/{id}"), &body)
            .map_err(crate::be_msg)
    } else if action == "revoke" {
        backend
            .send("DELETE", &format!("/remotes/{id}"), &body)
            .map_err(crate::be_msg)
    } else {
        Err("unsupported remote action".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_sync::LockExt;
    use std::sync::Mutex;

    struct Local {
        calls: Mutex<Vec<(String, Value)>>,
    }
    impl Backend for Local {
        fn machine_local(&self) -> bool {
            true
        }
        fn get(&self, path: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls
                .lock_or_recover()
                .push((path.into(), Value::Null));
            Ok(json!([]))
        }
        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            self.calls
                .lock_or_recover()
                .push((format!("{method} {path}"), body.clone()));
            Ok(json!({"registered":true}))
        }
    }

    #[test]
    fn hs2_1y45mz_nonlocal_backend_refuses_before_opening_a_credential_file() {
        let backend = crate::HttpBackend::new("https://remote.example.test", "synthetic-secret");
        let error = dispatch(
            &json!({"action":"register", "credentials_file":"must-not-be-opened"}),
            &backend,
        )
        .unwrap_err();
        assert!(error.contains("machine-local"));
        // A project wrapper may relocate after preflight; the actual HTTP route
        // must independently refuse both read and credential-bearing mutations.
        assert_eq!(backend.get("/remotes", &[]).unwrap_err().status, Some(403));
        assert_eq!(
            backend
                .send("POST", "/remotes", &json!({}))
                .unwrap_err()
                .status,
            Some(403)
        );
        assert!(!crate::HttpBackend::new("http://localhost:1234", "synthetic").machine_local());
        assert!(crate::HttpBackend::new("http://127.0.0.1:1234", "synthetic").machine_local());
    }

    #[test]
    fn hs2_1y45mz_local_file_import_is_not_returned_and_all_actions_route_explicitly() {
        let root = tempfile::tempdir().unwrap();
        let file = root.path().join("synthetic-credentials.json");
        std::fs::write(&file, r#"{"bearer_secret":"synthetic-mcp-bearer"}"#).unwrap();
        let backend = Local {
            calls: Mutex::new(Vec::new()),
        };
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let request = json!({"action":"register", "id":id, "label":"Synthetic", "origin":"http://127.0.0.1:1234", "expected_server_id":id, "credentials_file":file});
        let result = crate::handle_message_for_checkout(
            &json!({"jsonrpc":"2.0", "id":1, "method":"tools/call", "params":{
                "name":"hotsheet_remotes", "arguments":request
            }}),
            &backend,
            Some("synthetic-project"),
        )
        .unwrap();
        assert_eq!(result["result"]["isError"], false);
        assert!(!result.to_string().contains("synthetic-mcp-bearer"));
        let calls = backend.calls.lock_or_recover();
        assert_eq!(calls[0].0, "POST /remotes");
        assert_eq!(
            calls[0].1["credentials"]["bearer_secret"],
            "synthetic-mcp-bearer"
        );
        assert!(calls[0].1.get("credentials_file").is_none());
        assert!(calls[0].1.get("checkout").is_none());
        drop(calls);
        for action in ["list", "identity", "checkouts", "revoke"] {
            dispatch(
                &json!({"action":action, "id":id, "expected_revision":1}),
                &backend,
            )
            .unwrap();
        }
        assert_eq!(backend.calls.lock_or_recover().len(), 5);
    }
    #[test]
    fn hs2_1y45mz_registry_requests_refuse_redirects_before_forwarding_credentials() {
        use std::io::{Read, Write};
        use std::net::TcpListener;
        for method in ["GET", "POST"] {
            let destination = TcpListener::bind("127.0.0.1:0").unwrap();
            destination.set_nonblocking(true).unwrap();
            let selected = TcpListener::bind("127.0.0.1:0").unwrap();
            let origin = format!("http://{}", selected.local_addr().unwrap());
            let location = format!("http://{}/untrusted", destination.local_addr().unwrap());
            let responder = std::thread::spawn(move || {
                let (mut stream, _) = selected.accept().unwrap();
                stream
                    .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                    .unwrap();
                let mut request = Vec::new();
                let mut byte = [0; 1];
                while !request.ends_with(b"\r\n\r\n") {
                    assert_eq!(stream.read(&mut byte).unwrap(), 1);
                    request.push(byte[0]);
                    assert!(request.len() < 16_384);
                }
                let headers = String::from_utf8(request).unwrap();
                assert!(headers.contains("synthetic-controller-bearer"));
                let length = headers
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        key.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                let mut body = vec![0; length];
                stream.read_exact(&mut body).unwrap();
                write!(stream, "HTTP/1.1 303 See Other\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
            });
            let backend = crate::HttpBackend::new(&origin, "synthetic-controller-bearer");
            let result = if method == "GET" {
                backend.get("/remotes", &[])
            } else {
                backend.send(
                    "POST",
                    "/remotes",
                    &json!({"credentials":{"bearer_secret":"synthetic-peer-bearer"}}),
                )
            };
            responder.join().unwrap();
            assert!(result.unwrap_err().message.contains("redirect"));
            assert_eq!(
                destination.accept().unwrap_err().kind(),
                std::io::ErrorKind::WouldBlock
            );
        }
    }
}
