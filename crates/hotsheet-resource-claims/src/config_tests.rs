//! HS2-JJWESD: configurations applied to claims. Every home and project is a temporary
//! directory and every secret store is in memory, so nothing touches the keychain.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;

use hotsheet_ticketing::claim_backends::Scope;
use hotsheet_ticketing::secrets::SecretError;

use super::*;
use crate::{ClaimService, ResourceRef};

#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, String>>);

impl SecretStore for Memory {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self.0.lock().unwrap().remove(account).is_some())
    }
}

/// A store that refuses every call, like `HOTSHEET_NATIVE_SECRETS=deny`.
struct Denied(RefCell<usize>);

impl SecretStore for Denied {
    fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
        *self.0.borrow_mut() += 1;
        Err(SecretError::Unavailable("denied".into()))
    }
    fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
        *self.0.borrow_mut() += 1;
        Err(SecretError::Unavailable("denied".into()))
    }
    fn delete(&self, _: &str) -> Result<bool, SecretError> {
        Err(SecretError::Unavailable("denied".into()))
    }
}

fn local(id: &str) -> BackendConfiguration {
    BackendConfiguration::new(id, BackendKind::Local).unwrap()
}

fn keyed(id: &str) -> BackendConfiguration {
    let mut c = BackendConfiguration::new(id, BackendKind::GitRef).unwrap();
    c.credential = Some(BackendConfiguration::default_credential(id));
    c
}

fn ns(name: &str) -> Namespace {
    Namespace::parse(name).unwrap()
}

#[test]
fn hs2_jjwesd_an_unbound_namespace_keeps_the_machine_backend() {
    let dir = tempfile::tempdir().unwrap();
    let resolved = resolve(dir.path(), None, &ns("vm")).unwrap();
    assert_eq!(resolved.configuration, None);
    assert_eq!(resolved.offline, OfflinePolicy::FailClosed);
    assert_eq!(resolved.backend.capabilities().backend, "local");
}

#[test]
fn hs2_jjwesd_project_and_machine_namespaces_of_one_name_never_share_state() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let (project_a, project_b) = (dir.path().join("a"), dir.path().join("b"));
    for root in [&project_a, &project_b] {
        Scope::Project { root: root.clone() }
            .update(|r| {
                r.upsert_configuration(local("box"))?;
                r.bind_namespace("vm", "box");
                Ok(())
            })
            .unwrap();
    }
    let target = ResourceRef::parse("vm/runner").unwrap();
    let take = |service: ClaimService| {
        service.acquire(&target, Duration::from_secs(60), None, &BTreeMap::new())
    };
    take(ClaimService::new(&home)).unwrap();
    take(ClaimService::new(&home).with_project(&project_a)).unwrap();
    take(ClaimService::new(&home).with_project(&project_b)).unwrap();
    // Each scope now holds its own claim; a second acquire in project A is refused.
    let again = take(ClaimService::new(&home).with_project(&project_a)).unwrap_err();
    assert_eq!(again.code(), "held_by");
    assert_ne!(
        project_state_root(&home, &project_a),
        project_state_root(&home, &project_b)
    );
    let shown = ClaimService::new(&home)
        .with_project(&project_a)
        .request("GET", "/resource-claims/vm/runner", &Value::Null)
        .unwrap();
    assert_eq!(shown["configuration"], "box");
    assert_eq!(shown["offline"], "fail_closed");
}

#[test]
fn hs2_jjwesd_offline_policy_follows_the_bound_configuration() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let mut open = keyed("shared");
    open.offline = OfflinePolicy::Provisional;
    Scope::Machine { home: home.clone() }
        .update(|r| {
            r.upsert_configuration(open)?;
            r.upsert_configuration(local("box"))?;
            r.bind_namespace("remote-pool", "shared");
            r.bind_namespace("vm", "box");
            Ok(())
        })
        .unwrap();
    assert_eq!(
        offline_policy(&home, None, &ns("remote-pool")).unwrap(),
        OfflinePolicy::Provisional
    );
    assert_eq!(
        offline_policy(&home, None, &ns("vm")).unwrap(),
        OfflinePolicy::FailClosed
    );
    assert_eq!(
        offline_policy(&home, None, &ns("unbound")).unwrap(),
        OfflinePolicy::FailClosed
    );
}

#[test]
fn hs2_jjwesd_keys_live_in_the_injected_credential_store_only() {
    let dir = tempfile::tempdir().unwrap();
    let config = keyed("shared");
    let store = Memory::default();
    let provider = CredentialKeyProvider::new(Memory::default(), &config).unwrap();
    let missing = provider.key().unwrap_err().to_string();
    assert!(
        missing.contains("resource backend key shared --generate"),
        "{missing}"
    );

    assert_eq!(generate_key(&store, &config).unwrap(), Change::Created);
    assert_eq!(generate_key(&store, &config).unwrap(), Change::Unchanged);
    let exported = export_key(&store, &config).unwrap();
    let other = Memory::default();
    assert_eq!(
        import_key(&other, &config, &exported).unwrap(),
        Change::Created
    );
    assert_eq!(
        import_key(&other, &config, &exported).unwrap(),
        Change::Unchanged
    );
    assert!(import_key(&other, &config, "short").is_err());

    // Two machines holding the imported key derive the same codecs.
    let a = CredentialKeyProvider::new(store, &config)
        .unwrap()
        .key()
        .unwrap();
    let b = CredentialKeyProvider::new(other, &config)
        .unwrap()
        .key()
        .unwrap();
    let codecs_a = crate::MetadataCodecs::new(&a).unwrap();
    let digest = codecs_a.encode_input("hmac-sha256:worker-1").unwrap();
    assert_eq!(
        crate::MetadataCodecs::new(&b)
            .unwrap()
            .encode_input("hmac-sha256:worker-1")
            .unwrap(),
        digest
    );

    // No configuration file under the home holds the key.
    Scope::Machine {
        home: dir.path().into(),
    }
    .update(|r| r.upsert_configuration(config.clone()))
    .unwrap();
    let text =
        std::fs::read_to_string(dir.path().join("resource-claims/configurations.json")).unwrap();
    assert!(!text.contains(&exported));
}

#[test]
fn hs2_jjwesd_test_reports_reachability_and_the_missing_key_steps() {
    let dir = tempfile::tempdir().unwrap();
    let report = test_configuration(dir.path(), None, &local("box"), &Memory::default());
    assert!(report.ok && report.reachable, "{report:?}");

    // A git-ref configuration against a bare remote that exists.
    let bare = dir.path().join("remote.git");
    let repo = dir.path().join("clone");
    let git = |args: &[&str]| {
        assert!(
            hotsheet_ticketing::git::command()
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    };
    git(&["init", "--quiet", "--bare", bare.to_str().unwrap()]);
    git(&["init", "--quiet", repo.to_str().unwrap()]);
    git(&[
        "-C",
        repo.to_str().unwrap(),
        "remote",
        "add",
        "origin",
        bare.to_str().unwrap(),
    ]);
    let config = keyed("shared");
    let report = test_configuration(dir.path(), Some(&repo), &config, &Memory::default());
    assert!(report.reachable, "{report:?}");
    assert_eq!(report.key_present, Some(false));
    assert!(!report.ok);
    assert!(report.next_steps[0].contains("--generate"));

    // An unreachable remote is reported, not an error.
    git(&[
        "-C",
        repo.to_str().unwrap(),
        "remote",
        "set-url",
        "origin",
        dir.path().join("gone.git").to_str().unwrap(),
    ]);
    let store = Memory::default();
    generate_key(&store, &config).unwrap();
    let report = test_configuration(dir.path(), Some(&repo), &config, &store);
    assert!(!report.reachable && !report.ok, "{report:?}");
    assert!(report.error.unwrap().starts_with("backend_unreachable"));

    // A refused credential store is a failed test, never a prompt.
    let denied = Denied(RefCell::new(0));
    let report = test_configuration(dir.path(), Some(&repo), &config, &denied);
    assert_eq!(report.key_present, Some(false));
    assert_eq!(*denied.0.borrow(), 1);
}
