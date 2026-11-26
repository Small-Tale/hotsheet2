//! The headless workflow for connections and bindings (HS2-8X2S08, `docs/37` §37.6): every
//! operation is idempotent, returns a [`Change`], and prints no secret. The CLI calls these
//! functions; the server and the client settings call the same ones.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};

use hotsheet_ticketing::claim_backends::Change;
use hotsheet_ticketing::secrets::SecretStore;

use super::model::{AdapterDescriptor, ChangeAdapter, ConnectionContext, RepoIdentity};
use super::registry::{Binding, Connection, DEFAULT_REF, Registry, RegistryError};

#[derive(Debug, thiserror::Error)]
pub enum WorkflowError {
    #[error(transparent)]
    Registry(#[from] RegistryError),
    #[error(
        "unsupported_plugin: no remote-change plugin '{plugin}' in this build; available: {available}"
    )]
    UnsupportedPlugin { plugin: String, available: String },
    #[error("invalid: {0}")]
    Invalid(String),
    #[error("backend_unreachable: OS credential store: {0}")]
    Secret(String),
}

impl WorkflowError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Registry(e) => e.code(),
            Self::UnsupportedPlugin { .. } => "unsupported_plugin",
            Self::Invalid(_) => "invalid",
            Self::Secret(_) => "backend_unreachable",
        }
    }
}

pub struct Workflow<'a> {
    pub home: &'a Path,
    pub adapters: &'a [Arc<dyn ChangeAdapter>],
    pub secrets: &'a dyn SecretStore,
}

/// The answer of a connection test. It never carries the credential.
#[derive(Debug, Clone, Serialize)]
pub struct TestReport {
    pub connection: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub code: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub next_steps: Vec<String>,
}

impl Workflow<'_> {
    fn adapter(&self, plugin: &str) -> Result<&Arc<dyn ChangeAdapter>, WorkflowError> {
        self.adapters
            .iter()
            .find(|a| a.descriptor().id == plugin)
            .ok_or_else(|| WorkflowError::UnsupportedPlugin {
                plugin: plugin.to_owned(),
                available: self
                    .adapters
                    .iter()
                    .map(|a| a.descriptor().id)
                    .collect::<Vec<_>>()
                    .join(", "),
            })
    }

    pub fn plugins(&self) -> Vec<AdapterDescriptor> {
        let mut all: Vec<AdapterDescriptor> =
            self.adapters.iter().map(|a| a.descriptor()).collect();
        all.sort_by_key(|d| d.id);
        all
    }

    /// Add or change a connection. Settings are checked against the plugin's descriptor, so
    /// a token pasted as a setting is refused before anything is written.
    pub fn add_connection(
        &self,
        id: &str,
        plugin: &str,
        settings: BTreeMap<String, String>,
    ) -> Result<(Change, Vec<String>), WorkflowError> {
        let descriptor = self.adapter(plugin)?.descriptor();
        for key in settings.keys() {
            if !descriptor.settings.iter().any(|s| s.key == key) {
                let known: Vec<&str> = descriptor.settings.iter().map(|s| s.key).collect();
                return Err(WorkflowError::Invalid(format!(
                    "plugin '{plugin}' has no setting '{key}'; it takes {}. A credential is never a setting: use `connection secret`",
                    if known.is_empty() {
                        "none".to_owned()
                    } else {
                        known.join(", ")
                    }
                )));
            }
        }
        for spec in &descriptor.settings {
            if spec.required && !settings.contains_key(spec.key) {
                return Err(WorkflowError::Invalid(format!(
                    "plugin '{plugin}' needs setting '{}' ({}); add --setting {}=<value>",
                    spec.key, spec.label, spec.key
                )));
            }
        }
        let mut connection = Connection::new(id, plugin)?;
        connection.settings = settings;
        if descriptor.credential {
            connection.credential = Some(Connection::default_credential(id));
        }
        let change = Registry::update(self.home, |r| r.upsert_connection(connection.clone()))?;
        let mut next = Vec::new();
        if descriptor.credential && !self.has_credential(&connection) {
            next.push(format!(
                "store the credential ({}): `hotsheet-cli remote-change connection secret {id}` with it on standard input, then `hotsheet-cli remote-change connection test {id}`",
                descriptor.credential_help
            ));
        }
        Ok((change, next))
    }

    fn has_credential(&self, connection: &Connection) -> bool {
        connection.credential.as_ref().is_some_and(
            |account| matches!(self.secrets.get(account), Ok(Some(v)) if !v.is_empty()),
        )
    }

    /// Store, replace or delete a connection's credential. The value is never returned.
    pub fn set_secret(&self, id: &str, value: Option<&str>) -> Result<Change, WorkflowError> {
        let registry = Registry::load(self.home)?;
        let connection = registry
            .connection(id)
            .ok_or_else(|| RegistryError::NotFound(format!("no connection '{id}'")))?;
        let account = connection.credential.clone().ok_or_else(|| {
            WorkflowError::Invalid(format!(
                "plugin '{}' takes no credential",
                connection.plugin
            ))
        })?;
        let secret_err =
            |e: hotsheet_ticketing::secrets::SecretError| WorkflowError::Secret(e.to_string());
        match value {
            None => Ok(if self.secrets.delete(&account).map_err(secret_err)? {
                Change::Removed
            } else {
                Change::Absent
            }),
            Some(value) => {
                let value = value.trim_end_matches(['\r', '\n']);
                if value.is_empty() || value.len() > 4096 || value.chars().any(char::is_control) {
                    return Err(WorkflowError::Invalid(
                        "the credential on standard input must be 1-4096 printable characters"
                            .into(),
                    ));
                }
                let previous = self.secrets.get(&account).map_err(secret_err)?;
                if previous.as_deref() == Some(value) {
                    return Ok(Change::Unchanged);
                }
                self.secrets.set(&account, value).map_err(secret_err)?;
                Ok(if previous.is_some() {
                    Change::Updated
                } else {
                    Change::Created
                })
            }
        }
    }

    pub fn remove_connection(&self, id: &str) -> Result<Change, WorkflowError> {
        let change = Registry::update(self.home, |r| r.remove_connection(id))?;
        // The credential goes with the connection it belonged to.
        if change == Change::Removed {
            let _ = self.secrets.delete(&Connection::default_credential(id));
        }
        Ok(change)
    }

    /// Bind a source to a connection's channel. `refs` empty means the default branches.
    pub fn bind(
        &self,
        source: &str,
        connection: &str,
        channel: &str,
        repository: RepoIdentity,
        refs: Vec<String>,
    ) -> Result<Change, WorkflowError> {
        let refs = if refs.is_empty() {
            vec![DEFAULT_REF.to_owned()]
        } else {
            refs
        };
        let binding = Binding {
            source: source.to_owned(),
            connection: connection.to_owned(),
            channel: channel.to_owned(),
            repository,
            refs,
        };
        Ok(Registry::update(self.home, |r| r.bind(binding.clone()))?)
    }

    pub fn unbind(
        &self,
        source: &str,
        connection: &str,
        channel: &str,
    ) -> Result<Change, WorkflowError> {
        Ok(Registry::update(self.home, |r| {
            Ok(r.unbind(source, connection, channel))
        })?)
    }

    fn connection_view(&self, registry: &Registry, connection: &Connection) -> Value {
        let bindings: Vec<&Binding> = registry.bindings_of(&connection.id);
        json!({
            "id": connection.id,
            "plugin": connection.plugin,
            "settings": connection.settings,
            "credential": connection.credential.as_ref().map(|_| if self.has_credential(connection) { "stored" } else { "missing" }),
            "bindings": bindings,
        })
    }

    /// Every connection with its bindings. A credential reads `stored` or `missing`, never a value.
    pub fn list(&self) -> Result<Value, WorkflowError> {
        let registry = Registry::load(self.home)?;
        let connections: Vec<Value> = registry
            .connections
            .iter()
            .map(|c| self.connection_view(&registry, c))
            .collect();
        Ok(json!({
            "plugins": self.plugins(),
            "connections": connections,
            "bindings": registry.bindings,
        }))
    }

    pub fn show(&self, id: &str) -> Result<Value, WorkflowError> {
        let registry = Registry::load(self.home)?;
        let connection = registry
            .connection(id)
            .ok_or_else(|| RegistryError::NotFound(format!("no connection '{id}'")))?;
        Ok(self.connection_view(&registry, connection))
    }

    /// One cheap real check through the plugin. A missing credential is a failed test that
    /// names the command that fixes it, not an outage.
    pub fn test(&self, id: &str) -> Result<TestReport, WorkflowError> {
        let registry = Registry::load(self.home)?;
        let connection = registry
            .connection(id)
            .ok_or_else(|| RegistryError::NotFound(format!("no connection '{id}'")))?
            .clone();
        let adapter = self.adapter(&connection.plugin)?;
        let mut report = TestReport {
            connection: id.to_owned(),
            ok: false,
            code: None,
            error: None,
            next_steps: Vec::new(),
        };
        let credential = if adapter.descriptor().credential {
            let account = connection
                .credential
                .clone()
                .unwrap_or_else(|| Connection::default_credential(id));
            match self.secrets.get(&account) {
                Ok(Some(value)) if !value.is_empty() => Some(value),
                Ok(_) => {
                    report.code = Some("missing_credential");
                    report.error = Some(format!("no credential is stored for connection '{id}'"));
                    report.next_steps.push(format!(
                        "run `hotsheet-cli remote-change connection secret {id}` with the credential on standard input"
                    ));
                    return Ok(report);
                }
                Err(e) => {
                    report.code = Some("backend_unreachable");
                    report.error = Some(format!("OS credential store: {e}"));
                    return Ok(report);
                }
            }
        } else {
            None
        };
        let context = ConnectionContext {
            connection: id.to_owned(),
            settings: connection.settings.clone(),
            credential: credential.map(hotsheet_sync::secret::SecretString::from),
        };
        match adapter.test(&context) {
            Ok(()) => report.ok = true,
            Err(error) => {
                report.code = Some(error.code());
                report.error = Some(error.to_string());
            }
        }
        Ok(report)
    }
}
