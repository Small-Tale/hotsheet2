//! Global secret registry backed by the operating-system credential store.
//!
//! Secret values never enter Hot Sheet settings or the ticket store. The global registry
//! persists only provider metadata under `${HOTSHEET_HOME}/keys.json`; values live in the
//! macOS Keychain or Linux Secret Service. A provider-specific environment variable is the
//! only fallback and is read-only/explicit — there is no plaintext-on-disk fallback.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};

use crate::settings::{Settings, SettingsError};

const SERVICE: &str = "com.smalltale.hotsheet2";

#[derive(Debug, thiserror::Error)]
pub enum SecretError {
    #[error("invalid provider id '{0}' (use letters, digits, '.', '_' or '-')")]
    InvalidProvider(String),
    #[error("OS credential store unavailable: {0}")]
    Unavailable(String),
    #[error("credential store operation failed: {0}")]
    Backend(String),
    #[error("no API key registered for '{0}'")]
    NotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("parsing {path}: {source}")]
    Parse {
        path: String,
        source: serde_json::Error,
    },
    #[error(transparent)]
    Settings(#[from] SettingsError),
    #[error("setting '{0}' is not a secret reference (expected {{\"secret\":\"provider\"}})")]
    InvalidReference(String),
}

/// Injectable port so core logic and CLI behavior can be tested without a real keychain.
pub trait SecretStore {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError>;
    fn get(&self, account: &str) -> Result<Option<String>, SecretError>;
    fn delete(&self, account: &str) -> Result<bool, SecretError>;
}

/// Native process adapter. Arguments never contain secret material; writes use stdin.
pub struct OsKeychain;

impl SecretStore for OsKeychain {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        #[cfg(target_os = "macos")]
        let mut command = {
            let mut c = Command::new("security");
            c.args([
                "add-generic-password",
                "-U",
                "-a",
                account,
                "-s",
                SERVICE,
                "-w",
            ]);
            c
        };
        #[cfg(target_os = "linux")]
        let mut command = {
            let mut c = Command::new("secret-tool");
            c.args([
                "store",
                "--label",
                "Hot Sheet 2",
                "service",
                SERVICE,
                "account",
                account,
            ]);
            c
        };
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        return Err(SecretError::Unavailable(
            "this platform has no implemented credential-store adapter".into(),
        ));

        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| SecretError::Unavailable(e.to_string()))?;
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(secret.as_bytes())?;
        let output = child.wait_with_output()?;
        status(output)
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        #[cfg(target_os = "macos")]
        let output = Command::new("security")
            .args(["find-generic-password", "-a", account, "-s", SERVICE, "-w"])
            .output();
        #[cfg(target_os = "linux")]
        let output = Command::new("secret-tool")
            .args(["lookup", "service", SERVICE, "account", account])
            .output();
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        return Err(SecretError::Unavailable(
            "this platform has no implemented credential-store adapter".into(),
        ));

        let output = output.map_err(|e| SecretError::Unavailable(e.to_string()))?;
        if output.status.success() {
            return Ok(Some(
                String::from_utf8_lossy(&output.stdout)
                    .trim_end()
                    .to_string(),
            ));
        }
        Ok(None)
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        #[cfg(target_os = "macos")]
        let output = Command::new("security")
            .args(["delete-generic-password", "-a", account, "-s", SERVICE])
            .output();
        #[cfg(target_os = "linux")]
        let output = Command::new("secret-tool")
            .args(["clear", "service", SERVICE, "account", account])
            .output();
        #[cfg(not(any(target_os = "macos", target_os = "linux")))]
        return Err(SecretError::Unavailable(
            "this platform has no implemented credential-store adapter".into(),
        ));
        let output = output.map_err(|e| SecretError::Unavailable(e.to_string()))?;
        Ok(output.status.success())
    }
}

fn status(output: std::process::Output) -> Result<(), SecretError> {
    if output.status.success() {
        Ok(())
    } else {
        Err(SecretError::Backend(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyMetadata {
    pub provider: String,
    pub env: String,
}

/// Global provider registry. The registry file contains names only, never values.
pub struct KeyRegistry<S> {
    root: PathBuf,
    store: S,
}

impl<S: SecretStore> KeyRegistry<S> {
    pub fn new(root: impl Into<PathBuf>, store: S) -> Self {
        Self {
            root: root.into(),
            store,
        }
    }

    pub fn set(&self, provider: &str, secret: &str) -> Result<(), SecretError> {
        validate(provider)?;
        self.store.set(provider, secret)?;
        let mut map = self.metadata()?;
        map.insert(
            provider.into(),
            KeyMetadata {
                provider: provider.into(),
                env: env_name(provider),
            },
        );
        if let Err(error) = self.write_metadata(&map) {
            let _ = self.store.delete(provider);
            return Err(error);
        }
        Ok(())
    }

    pub fn get(&self, provider: &str) -> Result<String, SecretError> {
        validate(provider)?;
        let unavailable = match self.store.get(provider) {
            Ok(Some(secret)) => return Ok(secret),
            Ok(None) => None,
            Err(error @ SecretError::Unavailable(_)) => Some(error),
            Err(error) => return Err(error),
        };
        let env = env_name(provider);
        if let Ok(secret) = std::env::var(&env) {
            return Ok(secret);
        }
        match unavailable {
            Some(error) => Err(error),
            None => Err(SecretError::NotFound(provider.into())),
        }
    }

    pub fn delete(&self, provider: &str) -> Result<bool, SecretError> {
        validate(provider)?;
        let deleted = self.store.delete(provider)?;
        let mut map = self.metadata()?;
        let recorded = map.remove(provider).is_some();
        if recorded {
            self.write_metadata(&map)?;
        }
        Ok(deleted || recorded)
    }

    pub fn list(&self) -> Result<Vec<KeyMetadata>, SecretError> {
        Ok(self.metadata()?.into_values().collect())
    }

    fn path(&self) -> PathBuf {
        self.root.join("keys.json")
    }

    fn metadata(&self) -> Result<BTreeMap<String, KeyMetadata>, SecretError> {
        let path = self.path();
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).map_err(|source| SecretError::Parse {
                path: path.display().to_string(),
                source,
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
            Err(e) => Err(e.into()),
        }
    }

    fn write_metadata(&self, map: &BTreeMap<String, KeyMetadata>) -> Result<(), SecretError> {
        std::fs::create_dir_all(&self.root)?;
        let path = self.path();
        std::fs::write(
            &path,
            serde_json::to_string_pretty(map).expect("serializable") + "\n",
        )?;
        restrict_file(&path)?;
        Ok(())
    }
}

pub fn env_name(provider: &str) -> String {
    format!(
        "HOTSHEET_API_KEY_{}",
        provider
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() {
                c.to_ascii_uppercase()
            } else {
                '_'
            })
            .collect::<String>()
    )
}

/// Resolve a settings key through the registry without ever placing its value in settings.
/// Settings carry only `{ "secret": "provider-id" }`; missing settings return `None`.
pub fn resolve_setting_secret<S: SecretStore>(
    settings: &Settings,
    key: &str,
    registry: &KeyRegistry<S>,
) -> Result<Option<String>, SecretError> {
    let Some(value) = settings.get_effective(key)? else {
        return Ok(None);
    };
    let Some(provider) = value.get("secret").and_then(serde_json::Value::as_str) else {
        return Err(SecretError::InvalidReference(key.into()));
    };
    registry.get(provider).map(Some)
}

fn validate(provider: &str) -> Result<(), SecretError> {
    if provider.is_empty()
        || !provider
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
    {
        Err(SecretError::InvalidProvider(provider.into()))
    } else {
        Ok(())
    }
}

#[cfg(unix)]
fn restrict_file(path: &Path) -> Result<(), std::io::Error> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> Result<(), std::io::Error> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    #[derive(Default)]
    struct Memory(RefCell<BTreeMap<String, String>>);
    impl SecretStore for Memory {
        fn set(&self, a: &str, s: &str) -> Result<(), SecretError> {
            self.0.borrow_mut().insert(a.into(), s.into());
            Ok(())
        }
        fn get(&self, a: &str) -> Result<Option<String>, SecretError> {
            Ok(self.0.borrow().get(a).cloned())
        }
        fn delete(&self, a: &str) -> Result<bool, SecretError> {
            Ok(self.0.borrow_mut().remove(a).is_some())
        }
    }

    #[test]
    fn registry_round_trip_stores_only_metadata_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let registry = KeyRegistry::new(dir.path(), Memory::default());
        registry.set("openai", "super-secret").unwrap();
        assert_eq!(registry.get("openai").unwrap(), "super-secret");
        let disk = std::fs::read_to_string(dir.path().join("keys.json")).unwrap();
        assert!(disk.contains("openai") && disk.contains("HOTSHEET_API_KEY_OPENAI"));
        assert!(!disk.contains("super-secret"));
        assert!(registry.delete("openai").unwrap());
        assert!(matches!(
            registry.get("openai"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn validates_provider_and_normalizes_environment_name() {
        assert_eq!(env_name("open-ai.v2"), "HOTSHEET_API_KEY_OPEN_AI_V2");
        assert!(validate("bad/provider").is_err());
    }

    #[test]
    fn settings_hold_only_a_reference_to_the_registry() {
        let dir = tempfile::tempdir().unwrap();
        let registry = KeyRegistry::new(dir.path().join("home"), Memory::default());
        registry.set("openai", "secret-value").unwrap();
        let settings = Settings::new(dir.path());
        settings
            .set(
                "tts.api_key",
                serde_json::json!({"secret":"openai"}),
                crate::settings::Scope::Shared,
            )
            .unwrap();
        assert_eq!(
            resolve_setting_secret(&settings, "tts.api_key", &registry).unwrap(),
            Some("secret-value".into())
        );
        let disk = std::fs::read_to_string(dir.path().join("hotsheet-settings.json")).unwrap();
        assert!(!disk.contains("secret-value"));
    }

    #[test]
    fn explicit_environment_fallback_works_when_keychain_is_unavailable() {
        struct Unavailable;
        impl SecretStore for Unavailable {
            fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
                Err(SecretError::Unavailable("offline".into()))
            }
            fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
                Err(SecretError::Unavailable("offline".into()))
            }
            fn delete(&self, _: &str) -> Result<bool, SecretError> {
                Err(SecretError::Unavailable("offline".into()))
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let registry = KeyRegistry::new(dir.path(), Unavailable);
        unsafe { std::env::set_var("HOTSHEET_API_KEY_TEST_ENV", "from-env") };
        assert_eq!(registry.get("test-env").unwrap(), "from-env");
        unsafe { std::env::remove_var("HOTSHEET_API_KEY_TEST_ENV") };
    }
}
