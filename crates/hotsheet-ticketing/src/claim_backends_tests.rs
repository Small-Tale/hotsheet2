//! HS2-JJWESD: claim backend configurations and bindings. Synthetic names only; every
//! home and project is a temporary directory.

use super::*;

fn git_ref(id: &str) -> BackendConfiguration {
    let mut c = BackendConfiguration::new(id, BackendKind::GitRef).unwrap();
    c.settings.insert("remote".into(), "origin".into());
    c
}

fn machine(dir: &tempfile::TempDir) -> Scope {
    Scope::Machine {
        home: dir.path().join("home"),
    }
}

fn project(dir: &tempfile::TempDir) -> Scope {
    Scope::Project {
        root: dir.path().join("project"),
    }
}

#[test]
fn hs2_jjwesd_mutations_are_idempotent_and_report_what_they_did() {
    let dir = tempfile::tempdir().unwrap();
    let scope = machine(&dir);
    let add = |c: BackendConfiguration| scope.update(|r| r.upsert_configuration(c)).unwrap();
    assert_eq!(add(git_ref("shared")), Change::Created);
    assert_eq!(add(git_ref("shared")), Change::Unchanged);
    let mut provisional = git_ref("shared");
    provisional.offline = OfflinePolicy::Provisional;
    assert_eq!(add(provisional.clone()), Change::Updated);
    assert_eq!(add(provisional), Change::Unchanged);

    let bind = |s: &str| scope.update(|r| Ok(r.bind_source(s, "shared"))).unwrap();
    assert_eq!(bind("conn-a"), Change::Created);
    assert_eq!(bind("conn-a"), Change::Unchanged);
    let unbind = || scope.update(|r| Ok(r.unbind_source("conn-a"))).unwrap();
    assert_eq!(unbind(), Change::Removed);
    assert_eq!(unbind(), Change::Absent);

    let remove = || scope.update(|r| r.remove_configuration("shared")).unwrap();
    assert_eq!(remove(), Change::Removed);
    assert_eq!(remove(), Change::Absent);
    assert_eq!(scope.load().unwrap(), ClaimRegistry::default());
}

#[test]
fn hs2_jjwesd_two_sources_share_one_configuration_and_unbinding_one_keeps_the_other() {
    let dir = tempfile::tempdir().unwrap();
    let scope = machine(&dir);
    scope
        .update(|r| r.upsert_configuration(git_ref("shared")))
        .unwrap();
    scope
        .update(|r| {
            r.bind_source("conn-a", "shared");
            r.bind_source("conn-b", "shared");
            Ok(())
        })
        .unwrap();
    let registries = Registries::load(&dir.path().join("home"), None).unwrap();
    for source in ["conn-a", "conn-b"] {
        let found = registries.source(&[source.into()]).unwrap().unwrap();
        assert_eq!(found.configuration.id, "shared");
        assert_eq!(found.binding_scope, "machine");
    }

    // Removing a bound configuration is refused and names every binding.
    let refused = scope
        .update(|r| r.remove_configuration("shared"))
        .unwrap_err();
    assert_eq!(refused.code(), "in_use");
    let text = refused.to_string();
    assert!(
        text.contains("source conn-a") && text.contains("source conn-b"),
        "{text}"
    );

    scope.update(|r| Ok(r.unbind_source("conn-a"))).unwrap();
    let registries = Registries::load(&dir.path().join("home"), None).unwrap();
    assert!(registries.source(&["conn-a".into()]).unwrap().is_none());
    assert_eq!(
        registries
            .source(&["conn-b".into()])
            .unwrap()
            .unwrap()
            .configuration
            .id,
        "shared"
    );
}

#[test]
fn hs2_jjwesd_project_binding_wins_and_scopes_stay_isolated() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let (machine, project) = (machine(&dir), project(&dir));
    machine
        .update(|r| {
            r.upsert_configuration(BackendConfiguration::new("box", BackendKind::Local)?)?;
            r.bind_namespace("vm", "box");
            Ok(())
        })
        .unwrap();
    project
        .update(|r| {
            r.upsert_configuration(git_ref("team"))?;
            r.bind_namespace("vm", "team");
            // A project binding may name a machine configuration.
            r.bind_namespace("builds", "box");
            Ok(())
        })
        .unwrap();

    let without_project = Registries::load(&home, None).unwrap();
    assert_eq!(
        without_project
            .namespace("vm")
            .unwrap()
            .unwrap()
            .configuration
            .id,
        "box"
    );
    assert!(without_project.namespace("builds").unwrap().is_none());

    let with_project = Registries::load(&home, Some(&dir.path().join("project"))).unwrap();
    let vm = with_project.namespace("vm").unwrap().unwrap();
    assert_eq!(
        (vm.configuration.id.as_str(), vm.binding_scope),
        ("team", "project")
    );
    let builds = with_project.namespace("builds").unwrap().unwrap();
    assert_eq!(
        (builds.configuration.id.as_str(), builds.binding_scope),
        ("box", "project")
    );
    assert!(with_project.namespace("other").unwrap().is_none());

    // A binding to a configuration defined nowhere is an error, not "unbound".
    project
        .update(|r| Ok(r.bind_namespace("ghost", "gone")))
        .unwrap();
    let with_project = Registries::load(&home, Some(&dir.path().join("project"))).unwrap();
    assert_eq!(
        with_project.namespace("ghost").unwrap_err().code(),
        "not_found"
    );
}

#[test]
fn hs2_jjwesd_settings_refuse_secrets_and_unknown_keys() {
    let mut local = BackendConfiguration::new("box", BackendKind::Local).unwrap();
    local.settings.insert("remote".into(), "origin".into());
    assert_eq!(local.validate().unwrap_err().code(), "invalid");

    let mut token = git_ref("shared");
    token
        .settings
        .insert("token".into(), "ghp_synthetic".into());
    let error = token.validate().unwrap_err().to_string();
    assert!(error.contains("Secrets never go in settings"), "{error}");

    let mut credential = git_ref("shared");
    credential.credential = Some("ghp_synthetic".into());
    assert_eq!(credential.validate().unwrap_err().code(), "invalid");
    credential.credential = Some(BackendConfiguration::default_credential("shared"));
    credential.validate().unwrap();

    let mut provisional_local = BackendConfiguration::new("box", BackendKind::Local).unwrap();
    provisional_local.offline = OfflinePolicy::Provisional;
    assert_eq!(provisional_local.validate().unwrap_err().code(), "invalid");

    assert!(BackendConfiguration::new("Bad Id", BackendKind::Local).is_err());
    assert!(validate_source("store:01ARZ3NDEKTSV4RRFFQ69G5FAV").is_ok());
    assert!(validate_source("store:").is_err());
    assert!(validate_source("a/b").is_err());
    assert_eq!(BackendKind::parse("git-ref").unwrap(), BackendKind::GitRef);
    assert!(BackendKind::parse("redis").is_err());
}

#[test]
fn hs2_jjwesd_offline_policy_defaults_to_fail_closed_and_round_trips() {
    let dir = tempfile::tempdir().unwrap();
    let scope = machine(&dir);
    scope
        .update(|r| r.upsert_configuration(git_ref("closed")))
        .unwrap();
    let mut open = git_ref("open");
    open.offline = OfflinePolicy::Provisional;
    scope.update(|r| r.upsert_configuration(open)).unwrap();
    let text = fs::read_to_string(scope.path()).unwrap();
    assert!(text.contains("\"offline\": \"provisional\""), "{text}");
    assert!(text.contains("\"$hotsheetSchema\": 1"), "{text}");
    let registry = scope.load().unwrap();
    assert_eq!(
        registry.configuration("closed").unwrap().offline,
        OfflinePolicy::FailClosed
    );
    assert_eq!(
        registry.configuration("open").unwrap().offline,
        OfflinePolicy::Provisional
    );
}

#[test]
fn hs2_jjwesd_a_newer_or_damaged_registry_is_refused_and_kept() {
    let dir = tempfile::tempdir().unwrap();
    let scope = machine(&dir);
    fs::create_dir_all(scope.path().parent().unwrap()).unwrap();
    for text in ["{\"$hotsheetSchema\": 99}", "not json"] {
        fs::write(scope.path(), text).unwrap();
        assert_eq!(scope.load().unwrap_err().code(), "corrupt");
        let error = scope
            .update(|r| r.upsert_configuration(git_ref("x")))
            .unwrap_err();
        assert_eq!(error.code(), "corrupt");
        assert_eq!(fs::read_to_string(scope.path()).unwrap(), text);
    }
}

#[cfg(unix)]
#[test]
fn hs2_jjwesd_the_machine_registry_is_private() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let scope = machine(&dir);
    scope
        .update(|r| r.upsert_configuration(git_ref("shared")))
        .unwrap();
    let mode = fs::metadata(scope.path()).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0, "mode {mode:o}");
}

#[test]
fn hs2_jjwesd_a_git_store_answers_to_its_shared_identity_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = crate::FsStore::init(dir.path(), &crate::StoreMetadata::new("T")).unwrap();
    let instance = store.ensure_instance_id().unwrap();
    let sources = git_store_sources(&store);
    assert_eq!(sources[0], format!("store:{instance}"));
    assert_eq!(sources[1], crate::provider::git_connection_id(&store));
}
