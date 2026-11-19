//! HS2-WJF0D8: the Git-ref claim backend against a real bare remote and two clones.
//! Homes are temporary directories; no configuration names a credential, so nothing
//! reaches a credential store.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use hotsheet_ticketing::claim_backends::{BackendConfiguration, BackendKind, Scope};

use super::*;
use crate::ClaimService;

struct Fixture {
    dir: tempfile::TempDir,
    a: PathBuf,
    b: PathBuf,
}

fn git(dir: &Path, args: &[&str]) {
    let ok = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(dir)
        .args(args)
        .status()
        .unwrap()
        .success();
    assert!(ok, "git {args:?}");
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let bare = dir.path().join("remote.git");
    git(
        dir.path(),
        &["init", "--quiet", "--bare", bare.to_str().unwrap()],
    );
    git(&bare, &["config", "maintenance.auto", "false"]);
    let mut clones = Vec::new();
    for name in ["a", "b"] {
        let path = dir.path().join(name);
        git(dir.path(), &["init", "--quiet", path.to_str().unwrap()]);
        git(&path, &["remote", "add", "origin", bare.to_str().unwrap()]);
        git(&path, &["config", "maintenance.auto", "false"]);
        clones.push(path);
    }
    let b = clones.pop().unwrap();
    let a = clones.pop().unwrap();
    Fixture { dir, a, b }
}

fn backend(f: &Fixture, repo: &Path, offline: OfflinePolicy) -> GitRefBackend {
    GitRefBackend::new(
        &f.dir.path().join("home"),
        "shared",
        repo,
        "origin",
        offline,
        None,
    )
}

fn target() -> ResourceRef {
    ResourceRef::parse("vm/runner").unwrap()
}

fn request() -> AcquireRequest {
    AcquireRequest::new(Duration::from_secs(60))
}

fn offline_clone(f: &Fixture, repo: &Path) {
    let gone = f.dir.path().join("gone.git");
    git(
        repo,
        &["remote", "set-url", "origin", gone.to_str().unwrap()],
    );
}

#[test]
fn hs2_wjf0d8_one_grant_across_two_clones_with_token_scoped_renew_and_release() {
    let f = fixture();
    let (a, b) = (
        backend(&f, &f.a, OfflinePolicy::FailClosed),
        backend(&f, &f.b, OfflinePolicy::FailClosed),
    );
    let grant = a.acquire(&target(), &request()).unwrap();
    assert_eq!(grant.claim.guarantee, Guarantee::ExclusiveWithFencing);
    match b.acquire(&target(), &request()) {
        Err(ClaimError::HeldBy { holder }) => assert_eq!(holder.claim_id, grant.claim.claim_id),
        other => panic!("{other:?}"),
    }
    assert_eq!(
        b.get(&target()).unwrap().unwrap().claim_id,
        grant.claim.claim_id
    );
    assert_eq!(b.list(&target().namespace().clone()).unwrap().len(), 1);
    assert_eq!(
        b.namespaces().unwrap(),
        vec![Namespace::parse("vm").unwrap()]
    );
    b.verify_fencing(&target(), grant.claim.fencing_token)
        .unwrap();

    // Another machine renews with the token alone; a tampered token is refused.
    let renewed = b
        .renew(&grant.renewal_token, Duration::from_secs(600))
        .unwrap();
    assert!(renewed.expires_at_ms > grant.claim.expires_at_ms);
    let mut tampered = grant.renewal_token.clone();
    tampered.pop();
    tampered.push(if grant.renewal_token.ends_with('A') {
        'B'
    } else {
        'A'
    });
    assert!(matches!(
        b.renew(&tampered, Duration::from_secs(60)),
        Err(ClaimError::TokenInvalid)
    ));

    a.release(&grant.renewal_token).unwrap();
    assert!(b.get(&target()).unwrap().is_none());
    assert!(matches!(
        a.release(&grant.renewal_token),
        Err(ClaimError::TokenInvalid)
    ));
    let next = b.acquire(&target(), &request()).unwrap();
    assert!(next.claim.fencing_token > grant.claim.fencing_token);
    assert!(matches!(
        a.verify_fencing(&target(), grant.claim.fencing_token),
        Err(ClaimError::NotHolder)
    ));
    assert!(matches!(
        a.renew(&grant.renewal_token, Duration::from_secs(60)),
        Err(ClaimError::NotHolder)
    ));
}

#[test]
fn hs2_wjf0d8_offline_fail_closed_and_provisional_modes() {
    let f = fixture();
    offline_clone(&f, &f.a);
    let closed = backend(&f, &f.a, OfflinePolicy::FailClosed);
    assert!(matches!(
        closed.acquire(&target(), &request()),
        Err(ClaimError::BackendUnreachable(_))
    ));
    assert!(matches!(
        closed.get(&target()),
        Err(ClaimError::BackendUnreachable(_))
    ));

    let open = backend(&f, &f.a, OfflinePolicy::Provisional);
    let grant = open.acquire(&target(), &request()).unwrap();
    assert_eq!(
        grant.claim.guarantee,
        Guarantee::AdvisoryOnly,
        "never exclusive"
    );
    assert_eq!(
        open.get(&target()).unwrap().unwrap().guarantee,
        Guarantee::AdvisoryOnly
    );
    let renewed = open
        .renew(&grant.renewal_token, Duration::from_secs(120))
        .unwrap();
    assert_eq!(renewed.guarantee, Guarantee::AdvisoryOnly);
    open.release(&grant.renewal_token).unwrap();
}

#[test]
fn hs2_wjf0d8_metadata_without_a_key_is_plain_only_and_waits_are_refused() {
    let f = fixture();
    let a = backend(&f, &f.a, OfflinePolicy::FailClosed);
    assert!(matches!(a.codecs(), Err(ClaimError::Unsupported(_))));
    let mut metadata = BTreeMap::new();
    metadata.insert("label".to_owned(), "plain:runner".to_owned());
    let grant = a
        .acquire(&target(), &request().with_metadata(metadata.clone()))
        .unwrap();
    assert_eq!(grant.claim.metadata, metadata);
    assert_eq!(a.capabilities().wait, WaitMechanism::None);
    assert!(matches!(
        a.wait_free(&target(), Duration::from_secs(1)),
        Err(ClaimError::Unsupported(_))
    ));
    assert!(matches!(
        a.acquire_wait(&target(), &request(), Duration::ZERO),
        Err(ClaimError::WaitTimedOut { .. })
    ));
}

#[test]
fn hs2_wjf0d8_a_project_binding_serves_its_namespace_through_the_remote() {
    let f = fixture();
    let home = f.dir.path().join("home");
    for project in [&f.a, &f.b] {
        Scope::Project {
            root: project.clone(),
        }
        .update(|r| {
            r.upsert_configuration(BackendConfiguration::new("shared", BackendKind::GitRef)?)?;
            r.bind_namespace("vm", "shared");
            Ok(())
        })
        .unwrap();
    }
    let take = |project: &Path| {
        ClaimService::new(&home).with_project(project).acquire(
            &target(),
            Duration::from_secs(60),
            None,
            &BTreeMap::new(),
        )
    };
    take(&f.a).unwrap();
    assert_eq!(take(&f.b).unwrap_err().code(), "held_by");
    let shown = ClaimService::new(&home)
        .with_project(&f.b)
        .request(
            "GET",
            "/resource-claims/vm/runner",
            &serde_json::Value::Null,
        )
        .unwrap();
    assert_eq!(shown["capabilities"]["backend"], "git_ref");
    assert_eq!(shown["configuration"], "shared");
    assert_eq!(shown["held"], true);
    // The machine scope never sees the project's namespace.
    assert!(ClaimService::new(&home).show(&target()).unwrap().is_none());
}

#[test]
fn hs2_322zsz_the_git_ref_backend_passes_the_conformance_suite() {
    let f = fixture();
    let backend = backend(&f, &f.a, OfflinePolicy::FailClosed);
    crate::conformance::run(&crate::conformance::Fixture {
        backend: &backend,
        namespace: Namespace::parse("conformance").unwrap(),
        advance: &std::thread::sleep,
        short_lease: Duration::from_millis(900),
    });
}

#[test]
fn hs2_cr4sta_a_wait_on_the_git_ref_backend_is_refused_with_the_way_out_named() {
    let f = fixture();
    let backend = backend(&f, &f.a, OfflinePolicy::FailClosed);
    let held = backend.acquire(&target(), &request()).unwrap();
    let refused = backend
        .acquire_wait(&target(), &request(), Duration::from_secs(5))
        .unwrap_err();
    assert_eq!(refused.code(), "unsupported");
    assert!(refused.to_string().contains("upstash-redis"), "{refused}");
    let check = backend
        .wait_free(&target(), Duration::from_secs(5))
        .unwrap_err();
    assert_eq!(check.code(), "unsupported");
    assert!(check.to_string().contains("bind this namespace"), "{check}");
    // A zero-timeout wait is a try-now that still names the holder.
    match backend.acquire_wait(&target(), &request(), Duration::ZERO) {
        Err(ClaimError::WaitTimedOut { holder }) => {
            assert_eq!(holder.claim_id, held.claim.claim_id)
        }
        other => panic!("{other:?}"),
    }
}
