//! Backend configurations applied to resource claims (HS2-JJWESD, `docs/36` §36.13).
//!
//! The registry itself (configurations, namespace and source bindings, the machine and
//! project scopes) lives in `hotsheet_ticketing::claim_backends`, because ticket claims
//! read it too. This module turns a binding into a [`ClaimBackend`], reports a
//! namespace's offline policy, keeps configuration keys in the OS credential store, and
//! runs the cheap reachability test. The CLI calls these functions; the server and the
//! web settings call the same ones.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::{SecureRandom, SystemRandom};
use serde::Serialize;
use serde_json::{Value, json};

use hotsheet_ticketing::claim_backends::{
    BackendConfiguration, BackendKind, Change, Registries, RegistryError, Resolved,
};
use hotsheet_ticketing::secrets::SecretStore;

use crate::backend::ClaimBackend;
use crate::clock::SystemClock;
use crate::codec::{ClaimKey, FileKeyProvider, KeyProvider};
use crate::error::ClaimError;
use crate::local::{LOCAL_ROOT_DIR, LocalBackend};
use crate::model::Namespace;

pub use hotsheet_ticketing::orchestration::OfflinePolicy;

/// The directory under `<home>/resource-claims` that holds project-scoped local state.
pub const PROJECTS_DIR: &str = "projects";

/// A namespace's backend together with the configuration that chose it.
#[derive(Clone)]
pub struct ResolvedBackend {
    pub backend: Arc<dyn ClaimBackend>,
    /// The bound configuration's id; `None` for an unbound namespace, which the
    /// machine's local backend serves.
    pub configuration: Option<String>,
    /// `machine` or `project` for a bound namespace.
    pub binding_scope: Option<&'static str>,
    /// What the configuration says to do while the backend is unreachable. An unbound
    /// namespace and the local backend are never unreachable, so they report
    /// [`OfflinePolicy::FailClosed`].
    pub offline: OfflinePolicy,
}

impl std::fmt::Debug for ResolvedBackend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedBackend")
            .field("configuration", &self.configuration)
            .field("binding_scope", &self.binding_scope)
            .field("offline", &self.offline)
            .finish_non_exhaustive()
    }
}

pub(crate) fn registry_error(error: RegistryError) -> ClaimError {
    match error {
        RegistryError::Io(error) => ClaimError::Storage(error),
        RegistryError::Corrupt { .. } => ClaimError::Corrupt,
        other => ClaimError::Invalid(other.to_string()),
    }
}

/// Resolve the backend for a namespace: the project's binding when `project` is given
/// and binds it, else the machine's binding, else the machine's local backend.
pub fn resolve(
    home: &Path,
    project: Option<&Path>,
    namespace: &Namespace,
) -> Result<ResolvedBackend, ClaimError> {
    let registries = Registries::load(home, project).map_err(registry_error)?;
    match registries
        .namespace(namespace.as_str())
        .map_err(registry_error)?
    {
        None => Ok(ResolvedBackend {
            backend: Arc::new(LocalBackend::machine(home)),
            configuration: None,
            binding_scope: None,
            offline: OfflinePolicy::FailClosed,
        }),
        Some(resolved) => Ok(ResolvedBackend {
            backend: backend_for(home, &resolved)?,
            configuration: Some(resolved.configuration.id.clone()),
            binding_scope: Some(resolved.binding_scope),
            offline: resolved.configuration.offline,
        }),
    }
}

/// The offline policy of the configuration a namespace is bound to (HS2-KCEHVR reads
/// this). An unbound namespace is served locally and reports `FailClosed`.
pub fn offline_policy(
    home: &Path,
    project: Option<&Path>,
    namespace: &Namespace,
) -> Result<OfflinePolicy, ClaimError> {
    let registries = Registries::load(home, project).map_err(registry_error)?;
    Ok(registries
        .namespace(namespace.as_str())
        .map_err(registry_error)?
        .map_or(OfflinePolicy::FailClosed, |r| r.configuration.offline))
}

/// The local state root of a project-scoped namespace, so the same namespace name in
/// two projects, or in a project and the machine, never shares state.
pub fn project_state_root(home: &Path, project: &Path) -> PathBuf {
    use ring::digest;
    let canonical = project
        .canonicalize()
        .unwrap_or_else(|_| project.to_path_buf());
    let hash = digest::digest(&digest::SHA256, canonical.to_string_lossy().as_bytes());
    let key: String = hash.as_ref()[..8]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    home.join(LOCAL_ROOT_DIR).join(PROJECTS_DIR).join(key)
}

/// Build the backend a resolved binding selects.
pub fn backend_for(home: &Path, resolved: &Resolved) -> Result<Arc<dyn ClaimBackend>, ClaimError> {
    match resolved.configuration.kind {
        BackendKind::Local => Ok(match &resolved.project {
            None => Arc::new(LocalBackend::machine(home)),
            Some(project) => {
                let root = project_state_root(home, project);
                let keys = Arc::new(FileKeyProvider::new(root.join("key")));
                Arc::new(LocalBackend::new(root, Arc::new(SystemClock), keys))
            }
        }),
        BackendKind::External => crate::external::backend_for(home, &resolved.configuration),
        BackendKind::GitRef => {
            let configuration = &resolved.configuration;
            let repository = git_repository(resolved.project.as_deref(), configuration)
                .ok_or_else(|| {
                    ClaimError::Invalid(format!(
                        "configuration '{}' needs a repository setting when bound on the machine",
                        configuration.id
                    ))
                })?;
            let keys: Option<Arc<dyn KeyProvider>> = match configuration.credential {
                Some(_) => Some(Arc::new(CredentialKeyProvider::new(
                    hotsheet_ticketing::OsKeychain::at(home),
                    configuration,
                )?)),
                None => None,
            };
            Ok(Arc::new(crate::gitref::GitRefBackend::new(
                home,
                &configuration.id,
                &repository,
                git_remote(configuration),
                configuration.offline,
                keys,
            )))
        }
    }
}

/// A configuration key kept in the OS credential store, as base64url of 32 bytes.
/// Tests inject an in-memory [`SecretStore`]; nothing here touches the keychain unless
/// the caller passes the native one.
pub struct CredentialKeyProvider<S> {
    store: S,
    account: String,
    configuration: String,
}

impl<S: SecretStore> CredentialKeyProvider<S> {
    pub fn new(store: S, configuration: &BackendConfiguration) -> Result<Self, ClaimError> {
        let account = configuration.credential.clone().ok_or_else(|| {
            ClaimError::Unsupported(format!(
                "configuration '{}' has no credential; add one with `hotsheet-cli resource backend add {} --kind {} --credential`",
                configuration.id,
                configuration.id,
                configuration.kind.as_str()
            ))
        })?;
        Ok(Self {
            store,
            account,
            configuration: configuration.id.clone(),
        })
    }
}

impl<S: SecretStore + Send + Sync> KeyProvider for CredentialKeyProvider<S> {
    fn key(&self) -> Result<ClaimKey, ClaimError> {
        let stored = self
            .store
            .get(&self.account)
            .map_err(|error| ClaimError::BackendUnreachable(error.to_string()))?
            .ok_or_else(|| {
                ClaimError::Unsupported(format!(
                    "no key for configuration '{0}' in the OS credential store; {1}",
                    self.configuration,
                    key_next_step(&self.configuration)
                ))
            })?;
        decode_key(&stored)
    }
}

/// The exact commands that put a configuration's key in place.
pub fn key_next_step(configuration: &str) -> String {
    format!(
        "run `hotsheet-cli resource backend key {configuration} --generate` on the first machine, then `hotsheet-cli resource backend key {configuration} --export` there and `hotsheet-cli resource backend key {configuration} --import` (key on standard input) on every other machine"
    )
}

fn decode_key(stored: &str) -> Result<ClaimKey, ClaimError> {
    let bytes = URL_SAFE_NO_PAD
        .decode(stored.trim())
        .map_err(|_| ClaimError::Corrupt)?;
    let bytes: [u8; 32] = bytes.try_into().map_err(|_| ClaimError::Corrupt)?;
    Ok(ClaimKey::from_bytes(bytes))
}

fn credential_account(configuration: &BackendConfiguration) -> Result<&str, ClaimError> {
    configuration.credential.as_deref().ok_or_else(|| {
        ClaimError::Invalid(format!(
            "configuration '{}' names no credential account",
            configuration.id
        ))
    })
}

fn secret_error(error: hotsheet_ticketing::secrets::SecretError) -> ClaimError {
    ClaimError::BackendUnreachable(format!("OS credential store: {error}"))
}

/// Create the configuration's key in the credential store unless it already exists.
pub fn generate_key<S: SecretStore>(
    store: &S,
    configuration: &BackendConfiguration,
) -> Result<Change, ClaimError> {
    let account = credential_account(configuration)?;
    if store.get(account).map_err(secret_error)?.is_some() {
        return Ok(Change::Unchanged);
    }
    let mut bytes = [0u8; 32];
    SystemRandom::new()
        .fill(&mut bytes)
        .map_err(|_| ClaimError::Unsupported("no secure random source".into()))?;
    store
        .set(account, &URL_SAFE_NO_PAD.encode(bytes))
        .map_err(secret_error)?;
    Ok(Change::Created)
}

/// Store a key exported from another machine. The same key again is `Unchanged`.
pub fn import_key<S: SecretStore>(
    store: &S,
    configuration: &BackendConfiguration,
    encoded: &str,
) -> Result<Change, ClaimError> {
    let account = credential_account(configuration)?;
    let encoded = encoded.trim();
    decode_key(encoded)
        .map_err(|_| ClaimError::Invalid("a configuration key is 32 bytes of base64url".into()))?;
    match store.get(account).map_err(secret_error)? {
        Some(existing) if existing.trim() == encoded => Ok(Change::Unchanged),
        existing => {
            store.set(account, encoded).map_err(secret_error)?;
            Ok(if existing.is_some() {
                Change::Updated
            } else {
                Change::Created
            })
        }
    }
}

/// The stored key, for copying to another machine. Only an explicit request prints it.
pub fn export_key<S: SecretStore>(
    store: &S,
    configuration: &BackendConfiguration,
) -> Result<String, ClaimError> {
    let account = credential_account(configuration)?;
    store.get(account).map_err(secret_error)?.ok_or_else(|| {
        ClaimError::Unsupported(format!(
            "no key for configuration '{}'; {}",
            configuration.id,
            key_next_step(&configuration.id)
        ))
    })
}

/// The answer of a reachability test.
#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub configuration: String,
    pub kind: BackendKind,
    pub ok: bool,
    pub reachable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub key_present: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
}

/// One real, cheap call against a configuration's backend, plus a check that its key is
/// in the credential store when it names one. Never acquires anything.
pub fn test_configuration<S: SecretStore>(
    home: &Path,
    project: Option<&Path>,
    configuration: &BackendConfiguration,
    secrets: &S,
) -> TestReport {
    let mut report = TestReport {
        configuration: configuration.id.clone(),
        kind: configuration.kind,
        ok: false,
        reachable: false,
        key_present: None,
        error: None,
        next_steps: Vec::new(),
    };
    let reach = match configuration.kind {
        BackendKind::Local => LocalBackend::machine(home).namespaces().map(|_| ()),
        BackendKind::GitRef => git_reachable(project, configuration),
        BackendKind::External => external_reachable(home, configuration),
    };
    match reach {
        Ok(()) => report.reachable = true,
        Err(error) => report.error = Some(format!("{}: {error}", error.code())),
    }
    if let Some(account) = &configuration.credential {
        match secrets.get(account) {
            Ok(Some(stored)) if decode_key(&stored).is_ok() => report.key_present = Some(true),
            Ok(_) => {
                report.key_present = Some(false);
                report.next_steps.push(key_next_step(&configuration.id));
            }
            Err(error) => {
                report.key_present = Some(false);
                report
                    .error
                    .get_or_insert(format!("OS credential store: {error}"));
            }
        }
    }
    report.ok = report.reachable && report.key_present != Some(false);
    report
}

/// The Git repository a git-ref configuration serializes claims through.
pub fn git_repository(
    project: Option<&Path>,
    configuration: &BackendConfiguration,
) -> Option<PathBuf> {
    configuration
        .setting("repository")
        .map(PathBuf::from)
        .or_else(|| project.map(Path::to_path_buf))
}

/// The Git remote a git-ref configuration pushes claims to (default `origin`).
pub fn git_remote(configuration: &BackendConfiguration) -> &str {
    configuration.setting("remote").unwrap_or("origin")
}

/// An external plugin answers one cheap read-only call (`namespaces`) once it is approved.
fn external_reachable(home: &Path, configuration: &BackendConfiguration) -> Result<(), ClaimError> {
    crate::external::backend_for(home, configuration)?
        .namespaces()
        .map(|_| ())
}

fn git_reachable(
    project: Option<&Path>,
    configuration: &BackendConfiguration,
) -> Result<(), ClaimError> {
    let repository = git_repository(project, configuration).ok_or_else(|| {
        ClaimError::Invalid(format!(
            "configuration '{}' needs --setting repository=<path> or --project <dir>",
            configuration.id
        ))
    })?;
    let output = hotsheet_ticketing::git::command()
        .arg("-C")
        .arg(&repository)
        .args([
            "ls-remote",
            git_remote(configuration),
            "refs/hotsheet/claims/*",
        ])
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|error| ClaimError::BackendUnreachable(error.to_string()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(ClaimError::BackendUnreachable(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ))
    }
}

/// A configuration's JSON view: never a secret, only whether the key is present.
pub fn configuration_json(
    scope: &str,
    configuration: &BackendConfiguration,
    bindings: &[String],
) -> Value {
    json!({
        "scope": scope,
        "id": configuration.id,
        "kind": configuration.kind,
        "settings": configuration.settings,
        "credential": configuration.credential,
        "offline": configuration.offline,
        "bindings": bindings,
    })
}

#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;
