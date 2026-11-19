//! `hotsheet-cli resource backend|namespace|source`: claim backend configurations and
//! their bindings (HS2-JJWESD, `docs/36-resource-claims.md` §36.13). Every command is
//! idempotent, prints JSON, and needs no running server. The registry lives in
//! `hotsheet_ticketing::claim_backends`; keys live in the OS credential store.

use std::io::Read;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use clap::{Args, Subcommand};
use hotsheet_resource_claims::{
    OfflinePolicy, configuration_json, export_key, generate_key, import_key, key_next_step,
    test_configuration,
};
use hotsheet_ticketing::claim_backends::{
    BackendConfiguration, BackendKind, Change, ClaimRegistry, Registries, RegistryError, Scope,
    git_store_sources, validate_id, validate_source,
};
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
            let target = scope_of(&scope);
            let change = mutate(&target, |r| r.upsert_configuration(configuration.clone()))?;
            let next_steps: Vec<String> = if credential {
                vec![format!(
                    "unless this machine already has the key: {}",
                    key_next_step(&id)
                )]
            } else {
                Vec::new()
            };
            print(&json!({
                "change": change,
                "configuration": configuration_json(target.name(), &configuration, &[]),
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
            require_configuration(&scope, &configuration)?;
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
