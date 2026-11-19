//! `hotsheet-cli resource backend|namespace|source`: claim backend configurations and
//! their bindings (HS2-JJWESD, `docs/36-resource-claims.md` §36.13). Every command is
//! idempotent, prints JSON, and needs no running server. The registry lives in
//! `hotsheet_ticketing::claim_backends`; keys live in the OS credential store.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use hotsheet_resource_claims::external::{
    SettingType, approval_state, approve, approve_if_new, check_settings, load_all, load_plugin,
    plugin_root, secret_account,
};
use hotsheet_resource_claims::upstash::{TOKEN_SETTING, check_settings as check_upstash_settings};
use hotsheet_resource_claims::{
    OfflinePolicy, configuration_json, export_key, generate_key, import_key, key_next_step,
    test_configuration,
};
use hotsheet_ticketing::claim_backends::{
    BackendConfiguration, BackendKind, Change, ClaimRegistry, Registries, RegistryError, Scope,
    git_store_sources, validate_id, validate_source,
};
use hotsheet_ticketing::secrets::SecretStore;
use hotsheet_ticketing::{FsStore, OsKeychain};
use serde_json::{Value, json};

#[derive(Args, Clone, Default)]
pub(crate) struct ScopeArg {
    /// Keep this in the project's checked-in registry
    /// (`<dir>/.hotsheet2/resource-claims.json`) instead of the machine's. A ticket store
    /// directory is a project too, so its binding reaches every clone.
    #[arg(long, value_name = "DIR")]
    project: Option<PathBuf>,
}

#[derive(Subcommand)]
pub(crate) enum BackendCmd {
    /// Add or update a configuration (idempotent).
    Add {
        id: String,
        /// `local` or `git-ref`.
        #[arg(long)]
        kind: String,
        /// A non-secret setting `key=value` (repeatable). git-ref takes `remote` and
        /// `repository`; local takes none.
        #[arg(long = "setting", value_name = "KEY=VALUE")]
        settings: Vec<String>,
        /// Keep a key for metadata codecs in the OS credential store under
        /// `resource-claims.<id>` (see `resource backend key`).
        #[arg(long)]
        credential: bool,
        /// With `--credential`, do not generate the key when the configuration is created:
        /// this machine will import the key another machine exported (HS2-EWGPTZ).
        #[arg(long, requires = "credential")]
        no_key: bool,
        /// While the backend is unreachable: `fail-closed` (default) or `provisional`.
        #[arg(long, default_value = "fail-closed")]
        offline: String,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// List the configurations and bindings of the machine (and the project).
    List {
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Show one configuration and what is bound to it.
    Show {
        id: String,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Make one cheap real call to the backend and check its key is present.
    Test {
        id: String,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Remove a configuration. Refused while anything is bound to it.
    Remove {
        id: String,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Store a secret setting of an external plugin in the OS credential store; the value is
    /// read from standard input and never printed (HS2-322ZSZ).
    Secret {
        id: String,
        /// The plugin setting, declared `type = "secret"` in its manifest.
        key: String,
        /// Remove the stored secret instead.
        #[arg(long)]
        delete: bool,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Put the configuration's key in the OS credential store, or copy it out.
    Key {
        id: String,
        /// Create a new key unless one exists.
        #[arg(long, conflicts_with_all = ["import", "export"])]
        generate: bool,
        /// Store a key exported from another machine, read from standard input.
        #[arg(long, conflicts_with = "export")]
        import: bool,
        /// Print the stored key so another machine can import it.
        #[arg(long)]
        export: bool,
        #[command(flatten)]
        scope: ScopeArg,
    },
}

/// `resource plugin`: the external claim-backend plugins on this machine (HS2-322ZSZ).
#[derive(Subcommand)]
pub(crate) enum PluginCmd {
    /// List plugins under `<HOTSHEET_HOME>/claim-backends/`, their approval, and any that
    /// failed to load.
    List,
    /// Approve a plugin as it is now. Review its directory first: approval covers exactly
    /// the files that exist, and any change needs approving again.
    Trust { id: String },
}

#[derive(Subcommand)]
pub(crate) enum NamespaceCmd {
    /// Serve a resource namespace through a configuration.
    Bind {
        namespace: String,
        #[arg(long)]
        backend: String,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Return a namespace to the machine's local backend.
    Unbind {
        namespace: String,
        #[command(flatten)]
        scope: ScopeArg,
    },
}

#[derive(Subcommand)]
pub(crate) enum SourceCmd {
    /// Route the ticket claims of a data source through a configuration.
    Bind {
        /// A connection id, or `store:<instanceId>`. Omit it with `--store`.
        source: Option<String>,
        /// A Git ticket store; binds its shared identity `store:<instanceId>`, which
        /// every clone of the store answers to.
        #[arg(long, value_name = "PATH", conflicts_with = "source")]
        store: Option<PathBuf>,
        /// The configuration to use.
        #[arg(long, required_unless_present = "like", conflicts_with = "like")]
        backend: Option<String>,
        /// Use the configuration another source is already bound to.
        #[arg(long, value_name = "SOURCE")]
        like: Option<String>,
        #[command(flatten)]
        scope: ScopeArg,
    },
    /// Stop routing a data source's ticket claims through a configuration.
    Unbind {
        source: Option<String>,
        #[arg(long, value_name = "PATH", conflicts_with = "source")]
        store: Option<PathBuf>,
        #[command(flatten)]
        scope: ScopeArg,
    },
}

impl ScopeArg {
    pub(crate) fn project(&self) -> Option<PathBuf> {
        self.project.clone()
    }
}

fn home() -> PathBuf {
    hotsheet_plugins::hotsheet_home()
}

fn scope_of(arg: &ScopeArg) -> Scope {
    match &arg.project {
        Some(root) => Scope::Project { root: root.clone() },
        None => Scope::Machine { home: home() },
    }
}

fn registry_error(error: RegistryError) -> anyhow::Error {
    anyhow!("{}: {error}", error.code())
}

fn print(value: &Value) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}

/// Commit a project registry that sits in a Hot Sheet store, so clones receive it.
fn publish(scope: &Scope, change: Change) -> Result<()> {
    let Scope::Project { root } = scope else {
        return Ok(());
    };
    if matches!(change, Change::Unchanged | Change::Absent) {
        return Ok(());
    }
    if let Ok(store) = FsStore::open(root) {
        store
            .autocommit_paths("Update resource-claim configuration", &[scope.path()])
            .context("committing the store's claim configuration")?;
    }
    Ok(())
}

fn mutate(
    scope: &Scope,
    change: impl FnOnce(&mut ClaimRegistry) -> Result<Change, RegistryError>,
) -> Result<Change> {
    let done = scope.update(change).map_err(registry_error)?;
    publish(scope, done)?;
    Ok(done)
}

/// A configuration visible from `scope`: the project's first, then the machine's.
fn find(scope: &ScopeArg, id: &str) -> Result<(&'static str, BackendConfiguration, Vec<String>)> {
    let registries = Registries::load(&home(), scope.project.as_deref()).map_err(registry_error)?;
    let (found_in, configuration) = registries
        .configuration(id)
        .ok_or_else(|| anyhow!("not_found: no configuration '{id}'"))?;
    let mut bindings = registries.machine.bindings_of(id);
    if let Some((_, project)) = &registries.project {
        bindings.extend(
            project
                .bindings_of(id)
                .into_iter()
                .map(|b| format!("project {b}")),
        );
    }
    Ok((found_in, configuration.clone(), bindings))
}

pub(crate) fn backend(cmd: BackendCmd) -> Result<()> {
    match cmd {
        BackendCmd::Add {
            id,
            kind,
            settings,
            credential,
            no_key,
            offline,
            scope,
        } => {
            let mut configuration =
                BackendConfiguration::new(&id, BackendKind::parse(&kind).map_err(registry_error)?)
                    .map_err(registry_error)?;
            for entry in &settings {
                let (key, value) = entry
                    .split_once('=')
                    .ok_or_else(|| anyhow!("--setting takes key=value"))?;
                if configuration
                    .settings
                    .insert(key.into(), value.into())
                    .is_some()
                {
                    bail!("--setting names '{key}' more than once");
                }
            }
            if credential {
                configuration.credential = Some(BackendConfiguration::default_credential(&id));
            }
            configuration.offline = parse_offline(&offline)?;
            if configuration.kind == BackendKind::UpstashRedis {
                check_upstash_settings(&configuration).map_err(claim_error)?;
            }
            let plugin = if configuration.kind == BackendKind::External {
                let manifest = load_plugin(&home(), &configuration).map_err(claim_error)?;
                check_settings(&manifest, &configuration).map_err(claim_error)?;
                Some(manifest)
            } else {
                None
            };
            let target = scope_of(&scope);
            let change = mutate(&target, |r| r.upsert_configuration(configuration.clone()))?;
            // Adding a configuration approves its plugin as it is now; adding again never
            // re-trusts code that changed since (HS2-208RB1's rule, for claim backends).
            let approval = match &plugin {
                Some(manifest) => {
                    approve_if_new(&home(), &manifest.id, &manifest.dir).map_err(claim_error)?;
                    Some(
                        approval_state(&home(), &manifest.id, &manifest.dir)
                            .map_err(claim_error)?,
                    )
                }
                None => None,
            };
            let (key, mut next_steps) = if credential {
                created_key(&OsKeychain::at(home()), &configuration, change, no_key)
            } else {
                (json!(null), Vec::new())
            };
            if configuration.kind == BackendKind::UpstashRedis {
                next_steps.push(format!(
                    "store the database's REST token: `hotsheet-cli resource backend secret {id} {TOKEN_SETTING}` with the token on standard input, then `hotsheet-cli resource backend test {id}`"
                ));
            }
            print(&json!({
                "change": change,
                "configuration": configuration_json(target.name(), &configuration, &[]),
                "key": key,
                "plugin": approval,
                "next_steps": next_steps,
            }))
        }
        BackendCmd::List { scope } => {
            let registries =
                Registries::load(&home(), scope.project.as_deref()).map_err(registry_error)?;
            let view = |name: &str, registry: &ClaimRegistry| {
                json!({
                    "scope": name,
                    "configurations": registry.configurations.iter().map(|c| {
                        configuration_json(name, c, &registry.bindings_of(&c.id))
                    }).collect::<Vec<_>>(),
                    "namespaces": registry.namespaces,
                    "sources": registry.sources,
                })
            };
            let mut scopes = vec![view("machine", &registries.machine)];
            if let Some((_, project)) = &registries.project {
                scopes.push(view("project", project));
            }
            print(&json!({ "scopes": scopes }))
        }
        BackendCmd::Show { id, scope } => {
            let (found_in, configuration, bindings) = find(&scope, &id)?;
            print(&configuration_json(found_in, &configuration, &bindings))
        }
        BackendCmd::Test { id, scope } => {
            let (_, configuration, _) = find(&scope, &id)?;
            let home = home();
            let report = test_configuration(
                &home,
                scope.project.as_deref(),
                &configuration,
                &OsKeychain::at(&home),
            );
            print(&serde_json::to_value(&report)?)?;
            if !report.ok {
                bail!(
                    "{}",
                    report
                        .error
                        .clone()
                        .unwrap_or_else(|| format!("configuration '{id}' is not ready"))
                );
            }
            Ok(())
        }
        BackendCmd::Remove { id, scope } => {
            validate_id(&id, "configuration id").map_err(registry_error)?;
            let change = mutate(&scope_of(&scope), |r| r.remove_configuration(&id))?;
            print(&json!({ "change": change, "id": id }))
        }
        BackendCmd::Secret {
            id,
            key,
            delete,
            scope,
        } => {
            let (_, configuration, _) = find(&scope, &id)?;
            match configuration.kind {
                BackendKind::External => {
                    let manifest = load_plugin(&home(), &configuration).map_err(claim_error)?;
                    let declared = manifest
                        .setting(&key)
                        .is_some_and(|spec| spec.kind == SettingType::Secret);
                    if !declared {
                        bail!(
                            "invalid: plugin '{}' declares no secret setting '{key}'",
                            manifest.id
                        );
                    }
                }
                BackendKind::UpstashRedis => {
                    if key != TOKEN_SETTING {
                        bail!(
                            "invalid: an upstash-redis backend keeps one secret, '{TOKEN_SETTING}'"
                        );
                    }
                }
                _ => bail!(
                    "invalid: a {} backend has no secret settings",
                    configuration.kind.as_str()
                ),
            }
            let secrets = OsKeychain::at(home());
            let account = secret_account(&id, &key);
            let change = if delete {
                if secrets.delete(&account).map_err(secret_error)? {
                    Change::Removed
                } else {
                    Change::Absent
                }
            } else {
                let mut value = String::new();
                std::io::stdin()
                    .take(8192)
                    .read_to_string(&mut value)
                    .context("reading the secret from standard input")?;
                let value = value.trim_end_matches(['\r', '\n']);
                if value.is_empty() {
                    bail!("invalid: the secret on standard input is empty");
                }
                let previous = secrets.get(&account).map_err(secret_error)?;
                if previous.as_deref() == Some(value) {
                    Change::Unchanged
                } else {
                    secrets.set(&account, value).map_err(secret_error)?;
                    if previous.is_some() {
                        Change::Updated
                    } else {
                        Change::Created
                    }
                }
            };
            print(&json!({ "change": change, "configuration": id, "secret": key }))
        }
        BackendCmd::Key {
            id,
            generate,
            import,
            export,
            scope,
        } => {
            let (_, configuration, _) = find(&scope, &id)?;
            let secrets = OsKeychain::at(home());
            let cli = |e: hotsheet_resource_claims::ClaimError| anyhow!("{}: {e}", e.code());
            if export {
                println!("{}", export_key(&secrets, &configuration).map_err(cli)?);
                return Ok(());
            }
            let change = if import {
                let mut key = String::new();
                std::io::stdin()
                    .take(1024)
                    .read_to_string(&mut key)
                    .context("reading the key from standard input")?;
                import_key(&secrets, &configuration, &key).map_err(cli)?
            } else if generate {
                generate_key(&secrets, &configuration).map_err(cli)?
            } else {
                bail!("pass --generate, --import, or --export");
            };
            print(&json!({ "change": change, "configuration": id }))
        }
    }
}

/// HS2-EWGPTZ: a configuration created with `--credential` gets its key at once, so the
/// first machine can claim without a second step. A configuration that already existed
/// keeps its key (it may have been imported), and an unavailable credential store does
/// not fail the add: the answer names the commands that finish it.
pub(crate) fn created_key<S: hotsheet_ticketing::secrets::SecretStore>(
    secrets: &S,
    configuration: &BackendConfiguration,
    change: hotsheet_ticketing::claim_backends::Change,
    skipped: bool,
) -> (Value, Vec<String>) {
    use hotsheet_ticketing::claim_backends::Change;
    let id = &configuration.id;
    if skipped {
        return (
            json!({ "status": "skipped" }),
            vec![format!("import the key: {}", key_next_step(id))],
        );
    }
    if change != Change::Created {
        return (
            json!({ "status": "kept" }),
            vec![format!(
                "unless this machine already has the key: {}",
                key_next_step(id)
            )],
        );
    }
    match generate_key(secrets, configuration) {
        Ok(change) => (
            json!({ "status": "ready", "change": change }),
            vec![format!(
                "copy the key to every other machine that shares this configuration: `hotsheet-cli resource backend key {id} --export` here, `hotsheet-cli resource backend key {id} --import` there"
            )],
        ),
        Err(error) => (
            json!({ "status": "unavailable", "error": format!("{}: {error}", error.code()) }),
            vec![key_next_step(id)],
        ),
    }
}

fn parse_offline(text: &str) -> Result<OfflinePolicy> {
    match text.trim().to_ascii_lowercase().replace('_', "-").as_str() {
        "fail-closed" => Ok(OfflinePolicy::FailClosed),
        "provisional" => Ok(OfflinePolicy::Provisional),
        other => bail!("invalid: --offline is fail-closed or provisional, not '{other}'"),
    }
}

/// A binding's configuration must be visible from the scope it is written in.
fn require_configuration(scope: &ScopeArg, id: &str) -> Result<()> {
    find(scope, id).map(|_| ())
}

pub(crate) fn namespace(cmd: NamespaceCmd) -> Result<()> {
    match cmd {
        NamespaceCmd::Bind {
            namespace,
            backend,
            scope,
        } => {
            let namespace = hotsheet_resource_claims::Namespace::parse(&namespace)
                .map_err(|e| anyhow!("{}: {e}", e.code()))?;
            require_configuration(&scope, &backend)?;
            let change = mutate(&scope_of(&scope), |r| {
                Ok(r.bind_namespace(namespace.as_str(), &backend))
            })?;
            print(&json!({ "change": change, "namespace": namespace, "configuration": backend }))
        }
        NamespaceCmd::Unbind { namespace, scope } => {
            let change = mutate(&scope_of(&scope), |r| Ok(r.unbind_namespace(&namespace)))?;
            print(&json!({ "change": change, "namespace": namespace }))
        }
    }
}

fn source_key(source: Option<String>, store: Option<&Path>) -> Result<String> {
    if let Some(store) = store {
        let store = FsStore::open(store)
            .with_context(|| format!("opening the ticket store at {}", store.display()))?;
        store
            .ensure_instance_id()
            .context("reading the store identity")?;
        return git_store_sources(&store)
            .into_iter()
            .next()
            .ok_or_else(|| anyhow!("the store has no identity"));
    }
    let source = source.ok_or_else(|| anyhow!("name a source or pass --store <path>"))?;
    validate_source(&source).map_err(registry_error)
}

fn claim_error(e: hotsheet_resource_claims::ClaimError) -> anyhow::Error {
    anyhow!("{}: {e}", e.code())
}

fn secret_error(e: hotsheet_ticketing::secrets::SecretError) -> anyhow::Error {
    anyhow!("backend_unreachable: OS credential store: {e}")
}

pub(crate) fn plugin(cmd: PluginCmd) -> Result<()> {
    let root = plugin_root(&home());
    match cmd {
        PluginCmd::List => {
            let (plugins, load_errors) = load_all(&root);
            let mut listed = Vec::new();
            for manifest in &plugins {
                let approval =
                    approval_state(&home(), &manifest.id, &manifest.dir).map_err(claim_error)?;
                listed.push(json!({
                    "id": manifest.id,
                    "display_name": manifest.display_name,
                    "directory": manifest.dir,
                    "capabilities": manifest.capabilities(),
                    "settings": manifest.settings,
                    "approval": approval,
                }));
            }
            print(&json!({
                "directory": root,
                "plugins": listed,
                "load_errors": load_errors,
            }))
        }
        PluginCmd::Trust { id } => {
            let (plugins, load_errors) = load_all(&root);
            let Some(manifest) = plugins.iter().find(|m| m.id == id) else {
                let why = load_errors
                    .iter()
                    .find(|e| e.contains(&id))
                    .cloned()
                    .unwrap_or_else(|| format!("expected {}", root.join(&id).display()));
                bail!("not_found: no loadable claim-backend plugin '{id}' ({why})");
            };
            let change = approve(&home(), &manifest.id, &manifest.dir).map_err(claim_error)?;
            let state =
                approval_state(&home(), &manifest.id, &manifest.dir).map_err(claim_error)?;
            print(&json!({ "change": change, "id": id, "approval": state }))
        }
    }
}

pub(crate) fn source(cmd: SourceCmd) -> Result<()> {
    match cmd {
        SourceCmd::Bind {
            source,
            store,
            backend,
            like,
            scope,
        } => {
            let key = source_key(source, store.as_deref())?;
            let configuration = match (backend, like) {
                (Some(backend), _) => backend,
                (None, Some(like)) => {
                    let registries = Registries::load(&home(), scope.project.as_deref())
                        .map_err(registry_error)?;
                    registries
                        .source(std::slice::from_ref(&like))
                        .map_err(registry_error)?
                        .ok_or_else(|| anyhow!("not_found: source '{like}' is not bound"))?
                        .configuration
                        .id
                }
                (None, None) => bail!("pass --backend <id> or --like <source>"),
            };
            let (_, bound, _) = find(&scope, &configuration)?;
            if matches!(
                bound.kind,
                BackendKind::External | BackendKind::UpstashRedis
            ) {
                bail!(
                    "unsupported: ticket claims cannot go through a {} backend yet (HS2-3SJ8JX); '{configuration}' serves resource namespaces only",
                    bound.kind.as_str()
                );
            }
            let change = mutate(&scope_of(&scope), |r| {
                Ok(r.bind_source(&key, &configuration))
            })?;
            print(&json!({ "change": change, "source": key, "configuration": configuration }))
        }
        SourceCmd::Unbind {
            source,
            store,
            scope,
        } => {
            let key = source_key(source, store.as_deref())?;
            let change = mutate(&scope_of(&scope), |r| Ok(r.unbind_source(&key)))?;
            print(&json!({ "change": change, "source": key }))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    use hotsheet_ticketing::claim_backends::Change;
    use hotsheet_ticketing::secrets::{SecretError, SecretStore};

    use super::*;

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

    struct Unavailable;

    impl SecretStore for Unavailable {
        fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
            Err(SecretError::Unavailable("no store".into()))
        }
        fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
            Err(SecretError::Unavailable("no store".into()))
        }
        fn delete(&self, _: &str) -> Result<bool, SecretError> {
            Err(SecretError::Unavailable("no store".into()))
        }
    }

    fn keyed(id: &str) -> BackendConfiguration {
        let mut configuration = BackendConfiguration::new(id, BackendKind::GitRef).unwrap();
        configuration.credential = Some(BackendConfiguration::default_credential(id));
        configuration
    }

    #[test]
    fn hs2_ewgptz_a_created_configuration_gets_its_key_once() {
        let store = Memory::default();
        let configuration = keyed("shared");
        let (key, steps) = created_key(&store, &configuration, Change::Created, false);
        assert_eq!(key["status"], "ready");
        assert!(steps[0].contains("--export"));
        let first = store.get("resource-claims.shared").unwrap().unwrap();
        // The same add again keeps the key rather than replacing it.
        let (key, _) = created_key(&store, &configuration, Change::Unchanged, false);
        assert_eq!(key["status"], "kept");
        assert_eq!(store.get("resource-claims.shared").unwrap().unwrap(), first);
    }

    #[test]
    fn hs2_ewgptz_no_key_leaves_the_store_empty_and_names_the_import() {
        let store = Memory::default();
        let (key, steps) = created_key(&store, &keyed("shared"), Change::Created, true);
        assert_eq!(key["status"], "skipped");
        assert!(steps[0].contains("--import"));
        assert!(store.get("resource-claims.shared").unwrap().is_none());
    }

    #[test]
    fn hs2_ewgptz_an_unavailable_credential_store_does_not_fail_the_add() {
        let (key, steps) = created_key(&Unavailable, &keyed("shared"), Change::Created, false);
        assert_eq!(key["status"], "unavailable");
        assert!(key["error"].as_str().unwrap().contains("no store"));
        assert!(steps[0].contains("--generate"));
    }
}
