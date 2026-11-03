//! HS2-4J6V46: project target parity over the shared request workflow.
use crate::{Backend, BackendError};
use serde_json::{Value, json};

pub(crate) fn definition() -> Value {
    json!({"name":"hotsheet_targets", "description":"List/add/rename/remove shared project targets; inspect fresh candidates then explicitly bind/unbind/resolve this machine's reviewed target destinations. Lists make no requests. Inspect/bind/unbind/resolve and list with target_id require a machine-local backend. A fresh projection/capability is never a grant. Retain target_id and expected revisions for repeatable setup; stale revisions conflict, including otherwise identical retries.",
    "inputSchema":{"type":"object","required":["action"],"properties":{
        "action":{"type":"string","enum":["list","add","rename","remove","bind","unbind","resolve","inspect"]},
        "destination_checkout":{"type":"string","description":"registered local/remote checkout id to inspect"},
            "registration_id":{"type":"string","description":"reviewed remote registration for inspect; omit for local"},
            "checkout":{"type":"string","description":"owning registered checkout id; inherits selected project"},
        "target_id":{"type":"string","description":"immutable canonical target ULID; retain generated id for retries"},
        "expected_revision":{"type":["string","null"],"description":"shared opaque manifest revision; null only for an absent manifest"},
        "label":{"type":"string"},"clear_expected_repository_identity":{"type":"boolean"},"expected_repository_identity":{"type":["string","null"]},
        "expected_manifest_revision":{"type":"string"},"expected_binding_revision":{"type":"integer","minimum":0},
        "destination":{"type":"object","description":"explicit reviewed local/remote server, CA, checkout and source authority identity; never origin/credentials"},
        "previous_destination":{"type":["object","null"],"description":"exact previous reviewed identity, null for initial/unbound"}
    }}})
}
pub(crate) fn backend_error(
    error: hotsheet_ticketing::orchestration::OrchestrationError,
) -> BackendError {
    BackendError {
        status: Some(hotsheet_cli::orchestration::error_status(&error)),
        message: format!("{}: {}", error.code(), error),
    }
}
fn required<'a>(args: &'a Value, name: &str) -> Result<&'a str, String> {
    args.get(name)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("{name} is required"))
}
fn optional_id<'a>(args: &'a Value, name: &str) -> Result<Option<&'a str>, String> {
    match args.get(name) {
        None => Ok(None),
        Some(Value::String(value)) => Ok(Some(value)),
        Some(_) => Err(format!("{name} must be a string when present")),
    }
}
pub(crate) fn dispatch(args: &Value, backend: &dyn Backend) -> Result<Value, String> {
    let action = required(args, "action")?;
    // HS2-4J6V46: tools/call does not enforce inputSchema; only omission selects defaults.
    let id = optional_id(args, "target_id")?;
    let registration = optional_id(args, "registration_id")?;
    let machine = matches!(action, "bind" | "unbind" | "resolve" | "inspect")
        || (action == "list" && id.is_some());
    // Preflight before any backend request; HttpBackend repeats it for actual origin.
    if machine && !backend.machine_local() {
        return Err("target binding administration requires a machine-local backend".into());
    }
    let owner = required(args, "checkout")?;
    if owner.is_empty()
        || owner.len() > 160
        || !owner
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
    {
        return Err("registered checkout id required".into());
    }
    if let Some(id) = id {
        if id
            .parse::<hotsheet_model::Ulid>()
            .ok()
            .map(|id| id.to_string())
            .as_deref()
            != Some(id)
        {
            return Err("canonical target ULID required".into());
        }
    }
    let prefix = format!("/checkouts/{owner}/orchestration/targets");
    if action == "list" {
        return backend
            .get(
                &id.map(|id| format!("{prefix}/{id}/binding"))
                    .unwrap_or(prefix),
                &[],
            )
            .map_err(crate::be_msg);
    }
    if action == "inspect" {
        let checkout_id = required(args, "destination_checkout")?;
        let destination = match registration {
            Some(registration_id) => {
                json!({"kind":"remote","registration_id":registration_id,"checkout_id":checkout_id})
            }
            None => json!({"kind":"local","checkout_id":checkout_id}),
        };
        return backend
            .send(
                "POST",
                &format!("/checkouts/{owner}/orchestration/inspect"),
                &json!({"destination":destination}),
            )
            .map_err(crate::be_msg);
    }
    let target = || json!({"owner_checkout_id":owner,"target_id":id});
    let revision = args
        .get("expected_revision")
        .cloned()
        .unwrap_or(Value::Null);
    match action {
        "add" => backend.send("POST", &prefix, &json!({"expected_revision":revision,"target_id":id,
            "label":required(args,"label")?,"expected_repository_identity":args.get("expected_repository_identity").cloned().unwrap_or(Value::Null)})),
        "rename" => backend.send("PATCH", &format!("{prefix}/{}",id.ok_or("target_id is required")?), &json!({"expected_revision":required(args,"expected_revision")?,"label":required(args,"label")?,"expected_repository_identity":args.get("expected_repository_identity").cloned().unwrap_or(Value::Null),"clear_expected_repository_identity":args.get("clear_expected_repository_identity").cloned().unwrap_or(json!(false))})),
        "remove" => backend.send("DELETE", &format!("{prefix}/{}",id.ok_or("target_id is required")?), &json!({"expected_revision":required(args,"expected_revision")?})),
        "bind" | "unbind" => {
            let id = id.ok_or("target_id is required")?;
            let expected = args.get("expected_binding_revision").and_then(Value::as_u64).ok_or("expected_binding_revision is required")?;
            let mut body = json!({"target":target(),"expected_binding_revision":expected,
                "previous_destination":args.get("previous_destination").cloned().unwrap_or(Value::Null)});
            if action == "bind" {
                body["expected_manifest_revision"] = required(args,"expected_manifest_revision")?.into();
                body["destination"] = args.get("destination").cloned().ok_or("destination is required")?;
            }
            backend.send(if action == "bind" {"PUT"} else {"DELETE"}, &format!("{prefix}/{id}/binding"), &body)
        }
        "resolve" => backend.get(&format!("{prefix}/{}/resolve", id.ok_or("target_id is required")?), &[]),
        _ => return Err("unsupported target action".into()),
    }.map_err(crate::be_msg)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_sync::LockExt;
    use hotsheet_ticketing::orchestration::{CheckoutProjection, TargetFailure};
    use hotsheet_ticketing::remote_servers::{
        RemoteCheckoutTransport, RemoteCredentials, RemoteFailure, RemoteSnapshot, RemoteTransport,
    };
    use hotsheet_ticketing::{
        FsStore, ProviderConfigRegistry, ProviderConnection, SecretError, SecretStore,
        StoreMetadata,
    };
    use std::sync::{Arc, Mutex};
    struct OffBox {
        calls: Mutex<Vec<String>>,
    }
    impl Backend for OffBox {
        fn get(&self, path: &str, _: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls.lock_or_recover().push(path.into());
            Ok(json!({"targets":[]}))
        }
        fn send(&self, _: &str, path: &str, _: &Value) -> Result<Value, BackendError> {
            self.calls.lock_or_recover().push(path.into());
            Ok(Value::Null)
        }
    }
    #[test]
    fn hs2_4j6v46_offbox_admin_preflight_precedes_backend_and_actual_origin_is_rechecked() {
        let backend = OffBox {
            calls: Mutex::new(Vec::new()),
        };
        for action in ["bind", "unbind", "resolve", "inspect"] {
            assert!(
                dispatch(&json!({"action":action}), &backend)
                    .unwrap_err()
                    .contains("machine-local")
            );
        }
        assert!(
            dispatch(
                &json!({"action":"list","target_id":"01ARZ3NDEKTSV4RRFFQ69G5FAV"}),
                &backend
            )
            .is_err()
        );
        assert!(backend.calls.lock_or_recover().is_empty());
        dispatch(
            &json!({"action":"list","checkout":"owner-0123456789ab"}),
            &backend,
        )
        .unwrap();
        assert_eq!(backend.calls.lock_or_recover().len(), 1);
        let http = crate::HttpBackend::new("https://offbox.example.test", "synthetic");
        for path in [
            "/checkouts/owner/orchestration/targets/id/binding",
            "/checkouts/owner/orchestration/targets/id/resolve",
            "/checkouts/owner/orchestration/inspect",
        ] {
            assert_eq!(http.get(path, &[]).unwrap_err().status, Some(403));
            assert_eq!(
                http.send("POST", path, &Value::Null).unwrap_err().status,
                Some(403)
            );
        }
    }
    struct Never;
    impl SecretStore for Never {
        fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
            panic!("local flow read native credentials")
        }
        fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
            panic!("local flow wrote credentials")
        }
        fn delete(&self, _: &str) -> Result<bool, SecretError> {
            panic!("local flow deleted credentials")
        }
    }
    impl RemoteTransport for Never {
        fn probe(&self, _: &str, _: &RemoteCredentials) -> Result<RemoteSnapshot, RemoteFailure> {
            panic!("local flow performed remote discovery")
        }
    }
    impl RemoteCheckoutTransport for Never {
        fn checkout_projection(
            &self,
            _: &str,
            _: &RemoteCredentials,
            _: &str,
        ) -> Result<CheckoutProjection, TargetFailure> {
            panic!("local flow performed remote metadata request")
        }
    }
    fn call(backend: &dyn Backend, owner: &str, args: Value) -> Value {
        let reply=crate::handle_message_for_checkout(&json!({"jsonrpc":"2.0","id":1,"method":"tools/call", "params":{"name":"hotsheet_targets","arguments":args}}),backend,Some(owner)).unwrap();
        assert_eq!(reply["result"]["isError"], false, "{reply}");
        serde_json::from_str(reply["result"]["content"][0]["text"].as_str().unwrap()).unwrap()
    }
    fn rejected(backend: &dyn Backend, owner: &str, args: Value) {
        let reply = crate::handle_message_for_checkout(&json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hotsheet_targets","arguments":args}}), backend, Some(owner)).unwrap();
        assert_eq!(reply["result"]["isError"], true, "{reply}");
    }
    struct Counted<'a> {
        inner: &'a dyn Backend,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl Backend for Counted<'_> {
        fn machine_local(&self) -> bool {
            self.inner.machine_local()
        }
        fn get(&self, path: &str, query: &[(String, String)]) -> Result<Value, BackendError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.get(path, query)
        }
        fn send(&self, method: &str, path: &str, body: &Value) -> Result<Value, BackendError> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            self.inner.send(method, path, body)
        }
    }
    #[test]
    fn hs2_4j6v46_tools_call_malformed_optional_ids_never_reach_backend() {
        let backend = OffBox {
            calls: Mutex::new(Vec::new()),
        };
        for bad in [
            json!(123),
            Value::Null,
            json!(true),
            json!({"id":"synthetic"}),
        ] {
            for _ in 0..2 {
                rejected(
                    &backend,
                    "owner",
                    json!({"action":"add","target_id":bad,"label":"Synthetic","expected_revision":null}),
                );
                rejected(
                    &backend,
                    "owner",
                    json!({"action":"inspect","registration_id":bad,"destination_checkout":"owner"}),
                );
            }
        }
        assert!(backend.calls.lock_or_recover().is_empty());
    }
    #[test]
    fn hs2_4j6v46_real_core_tool_parent_inherits_project_and_orphan_cleanup_avoids_live_constructor()
     {
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir(&home).unwrap();
        let store = FsStore::init(temp.path().join("host"), &StoreMetadata::new("SYN")).unwrap();
        let providers = ProviderConfigRegistry::new(store.root().join("providers.json"));
        providers
            .save(&[ProviderConnection {
                id: "synthetic".into(),
                provider: "github".into(),
                locator: "synthetic/repo".into(),
                name: None,
                default: false,
                disabled: false,
                settings: Value::Null,
            }])
            .unwrap();
        let registry =
            hotsheet_ticketing::checkouts::CheckoutRegistry::new(home.join("checkouts.json"));
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let owner = registry
            .register_registered_sources(
                &providers,
                &project,
                None,
                None,
                vec![hotsheet_ticketing::checkouts::TicketSource {
                    connection_id: "synthetic".into(),
                    provider: "github".into(),
                    locator: "synthetic/repo".into(),
                }],
                None,
            )
            .unwrap();
        let runtime = hotsheet_cli::orchestration::OrchestrationRuntime::new(
            home.clone(),
            registry.clone(),
            providers.clone(),
            Arc::new(Never),
            Arc::new(Never),
            hotsheet_model::Ulid::from(10u128).to_string(),
        )
        .unwrap();
        let backend = crate::CoreBackend::new(store.clone())
            .with_checkout_registry(registry.clone())
            .with_orchestration_runtime(runtime);
        let inspect = call(
            &backend,
            &owner.id,
            json!({"action":"inspect","destination_checkout":owner.id}),
        );
        assert_eq!(inspect["usable"], false);
        assert!(!home.join("orchestration-bindings.json").exists());
        let manifest = project.join(".hotsheet2/orchestration.json");
        let counted = Counted {
            inner: &backend,
            calls: std::sync::atomic::AtomicUsize::new(0),
        };
        for bad in [
            json!(123),
            Value::Null,
            json!(true),
            json!({"id":"synthetic"}),
        ] {
            for _ in 0..2 {
                rejected(
                    &counted,
                    &owner.id,
                    json!({"action":"add","target_id":bad,"label":"Synthetic","expected_revision":null}),
                );
                rejected(
                    &counted,
                    &owner.id,
                    json!({"action":"inspect","registration_id":bad,"destination_checkout":owner.id}),
                );
                assert!(!manifest.exists());
                assert!(!home.join("orchestration-bindings.json").exists());
            }
        }
        assert_eq!(counted.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let added = call(
            &backend,
            &owner.id,
            json!({"action":"add","target_id":id,"label":"Synthetic","expected_revision":null}),
        );
        let renamed = call(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":added["revision"],"expected_repository_identity":"https://repo.example.test/group/project"}),
        );
        assert_eq!(renamed["targets"][0]["target_id"], id);
        assert_eq!(renamed["targets"][0]["label"], "Renamed");
        assert_eq!(
            renamed["targets"][0]["expected_repository_identity"],
            "https://repo.example.test/group/project"
        );
        assert_ne!(renamed["revision"], added["revision"]);
        assert_eq!(call(&backend, &owner.id, json!({"action":"list"})), renamed);
        let bytes = std::fs::read(&manifest).unwrap();
        for bad in [json!(123), Value::Null, json!(false), json!({})] {
            for _ in 0..2 {
                rejected(
                    &counted,
                    &owner.id,
                    json!({"action":"add","target_id":bad,"label":"Synthetic","expected_revision":renamed["revision"]}),
                );
                assert_eq!(std::fs::read(&manifest).unwrap(), bytes);
            }
        }
        assert_eq!(counted.calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        rejected(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Wrong","expected_revision":renamed["revision"],"expected_repository_identity":"https://repo.example.test/other","clear_expected_repository_identity":true}),
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), bytes);
        let cleared = call(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":renamed["revision"],"clear_expected_repository_identity":true}),
        );
        assert_eq!(cleared["targets"][0]["target_id"], id);
        assert_eq!(cleared["targets"][0]["label"], "Renamed");
        assert!(
            cleared["targets"][0]
                .get("expected_repository_identity")
                .is_none()
        );
        assert_ne!(cleared["revision"], renamed["revision"]);
        let before = std::fs::read(&manifest).unwrap();
        rejected(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"Stale","expected_revision":renamed["revision"]}),
        );
        assert_eq!(std::fs::read(&manifest).unwrap(), before);
        assert_eq!(
            call(
                &backend,
                &owner.id,
                json!({"action":"rename","target_id":id,"label":"Renamed","expected_revision":cleared["revision"],"clear_expected_repository_identity":true})
            ),
            cleared
        );
        let after_clear = call(
            &backend,
            &owner.id,
            json!({"action":"rename","target_id":id,"label":"After clear","expected_revision":cleared["revision"]}),
        );
        assert_eq!(after_clear["targets"][0]["target_id"], id);
        assert_eq!(after_clear["targets"][0]["label"], "After clear");
        assert!(
            after_clear["targets"][0]
                .get("expected_repository_identity")
                .is_none()
        );
        assert_ne!(after_clear["revision"], cleared["revision"]);
        let cleared = after_clear;
        assert!(!home.join("orchestration-bindings.json").exists());
        let bound = call(
            &backend,
            &owner.id,
            json!({"action":"bind","target_id":id,"expected_manifest_revision":cleared["revision"],"expected_binding_revision":0,"previous_destination":null,"destination":inspect["destination"]}),
        );
        assert_eq!(bound["usable"], true);
        assert_eq!(
            call(
                &backend,
                &owner.id,
                json!({"action":"resolve","target_id":id})
            )["usable"],
            true
        );
        assert_eq!(
            call(&backend, &owner.id, json!({"action":"list"}))["targets"][0]["target_id"],
            id
        );
        call(
            &backend,
            &owner.id,
            json!({"action":"remove","target_id":id,"expected_revision":cleared["revision"]}),
        );
        let current = call(&backend, &owner.id, json!({"action":"list"}));
        let generated = call(
            &backend,
            &owner.id,
            json!({"action":"add","label":"Generated","expected_revision":current["revision"]}),
        );
        let generated_id = generated["targets"]
            .as_array()
            .unwrap()
            .iter()
            .find(|target| target["label"] == "Generated")
            .unwrap()["target_id"]
            .as_str()
            .unwrap();
        assert_ne!(generated_id, id);
        assert_eq!(
            generated_id
                .parse::<hotsheet_model::Ulid>()
                .unwrap()
                .to_string(),
            generated_id
        );
        std::fs::write(registry.path(), b"corrupt").unwrap();
        std::fs::write(providers.path(), b"corrupt").unwrap();
        std::fs::write(home.join("server-identity.json"), b"corrupt").unwrap();
        let cleanup = crate::CoreBackend::new(store).with_checkout_registry(registry);
        assert_eq!(
            call(&cleanup, &owner.id, json!({"action":"list","target_id":id}))["revision"],
            1
        );
        assert_eq!(
            call(
                &cleanup,
                &owner.id,
                json!({"action":"unbind","target_id":id,"expected_binding_revision":1,"previous_destination":inspect["destination"]})
            )["revision"],
            2
        );
        assert_eq!(
            std::fs::read(home.join("server-identity.json")).unwrap(),
            b"corrupt"
        );
    }
}
