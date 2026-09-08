//! Global secret registry backed by the operating-system credential store.
//!
//! Secret values never enter Hot Sheet settings or the ticket store. The global registry
//! persists only provider metadata under `${HOTSHEET_HOME}/keys.json`; values live in the
//! macOS Keychain, Linux Secret Service, or Windows Credential Manager. A provider-specific
//! environment variable is the only fallback and is read-only/explicit — there is no
//! plaintext-on-disk fallback.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
#[cfg(target_os = "linux")]
use std::{
    io::Write,
    process::{Command, Stdio},
};

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
        platform_set(account, secret)
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        platform_get(account)
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        platform_delete(account)
    }
}

#[cfg(target_os = "macos")]
fn platform_set(account: &str, secret: &str) -> Result<(), SecretError> {
    security_framework::passwords::set_generic_password(SERVICE, account, secret.as_bytes())
        .map_err(|error| SecretError::Backend(error.to_string()))
}

#[cfg(target_os = "macos")]
fn platform_get(account: &str) -> Result<Option<String>, SecretError> {
    match security_framework::passwords::get_generic_password(SERVICE, account) {
        Ok(secret) => String::from_utf8(secret)
            .map(Some)
            .map_err(|error| SecretError::Backend(error.to_string())),
        Err(error) if error.code() == -25300 => Ok(None),
        Err(error) => Err(SecretError::Backend(error.to_string())),
    }
}

#[cfg(target_os = "macos")]
fn platform_delete(account: &str) -> Result<bool, SecretError> {
    match security_framework::passwords::delete_generic_password(SERVICE, account) {
        Ok(()) => Ok(true),
        Err(error) if error.code() == -25300 => Ok(false),
        Err(error) => Err(SecretError::Backend(error.to_string())),
    }
}

#[cfg(target_os = "linux")]
fn platform_set(account: &str, secret: &str) -> Result<(), SecretError> {
    let mut child = Command::new("secret-tool")
        .args([
            "store",
            "--label",
            "Hot Sheet 2",
            "service",
            SERVICE,
            "account",
            account,
        ])
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
    status(child.wait_with_output()?)
}

#[cfg(target_os = "linux")]
fn platform_get(account: &str) -> Result<Option<String>, SecretError> {
    let output = Command::new("secret-tool")
        .args(["lookup", "service", SERVICE, "account", account])
        .output()
        .map_err(|e| SecretError::Unavailable(e.to_string()))?;
    Ok(output.status.success().then(|| {
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string()
    }))
}

#[cfg(target_os = "linux")]
fn platform_delete(account: &str) -> Result<bool, SecretError> {
    Command::new("secret-tool")
        .args(["clear", "service", SERVICE, "account", account])
        .output()
        .map(|output| output.status.success())
        .map_err(|e| SecretError::Unavailable(e.to_string()))
}

#[cfg(target_os = "windows")]
fn platform_set(account: &str, secret: &str) -> Result<(), SecretError> {
    use windows_sys::Win32::Security::Credentials::{
        CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC, CREDENTIALW, CredWriteW,
    };

    let mut target = windows_target(account);
    let mut username = windows_wide(account);
    let blob_size = u32::try_from(secret.len())
        .map_err(|_| SecretError::Backend("credential is too large for Windows".into()))?;
    let credential = CREDENTIALW {
        Type: CRED_TYPE_GENERIC,
        TargetName: target.as_mut_ptr(),
        CredentialBlobSize: blob_size,
        CredentialBlob: secret.as_ptr().cast_mut(),
        Persist: CRED_PERSIST_LOCAL_MACHINE,
        UserName: username.as_mut_ptr(),
        ..Default::default()
    };

    if unsafe { CredWriteW(&credential, 0) } == 0 {
        Err(windows_backend_error("writing credential"))
    } else {
        Ok(())
    }
}

#[cfg(target_os = "windows")]
fn platform_get(account: &str) -> Result<Option<String>, SecretError> {
    use std::{ptr, slice};
    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::Security::Credentials::{
        CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW,
    };

    let target = windows_target(account);
    let mut raw: *mut CREDENTIALW = ptr::null_mut();
    if unsafe { CredReadW(target.as_ptr(), CRED_TYPE_GENERIC, 0, &mut raw) } == 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
            Ok(None)
        } else {
            Err(SecretError::Backend(format!("reading credential: {error}")))
        };
    }

    // CredReadW owns the returned allocation. Copy the blob before releasing it so every
    // conversion path frees the native credential exactly once.
    let bytes = unsafe {
        let credential = &*raw;
        let bytes = slice::from_raw_parts(
            credential.CredentialBlob.cast_const(),
            credential.CredentialBlobSize as usize,
        )
        .to_vec();
        CredFree(raw.cast());
        bytes
    };
    String::from_utf8(bytes)
        .map(Some)
        .map_err(|error| SecretError::Backend(format!("credential is not valid UTF-8: {error}")))
}

#[cfg(target_os = "windows")]
fn platform_delete(account: &str) -> Result<bool, SecretError> {
    use windows_sys::Win32::Foundation::ERROR_NOT_FOUND;
    use windows_sys::Win32::Security::Credentials::{CRED_TYPE_GENERIC, CredDeleteW};

    let target = windows_target(account);
    if unsafe { CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) } != 0 {
        return Ok(true);
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(ERROR_NOT_FOUND as i32) {
        Ok(false)
    } else {
        Err(SecretError::Backend(format!(
            "deleting credential: {error}"
        )))
    }
}

#[cfg(target_os = "windows")]
fn windows_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(target_os = "windows")]
fn windows_target(account: &str) -> Vec<u16> {
    windows_wide(&format!("{SERVICE}/{account}"))
}

#[cfg(target_os = "windows")]
fn windows_backend_error(operation: &str) -> SecretError {
    SecretError::Backend(format!("{operation}: {}", std::io::Error::last_os_error()))
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_set(_: &str, _: &str) -> Result<(), SecretError> {
    Err(unsupported_platform())
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_get(_: &str) -> Result<Option<String>, SecretError> {
    Err(unsupported_platform())
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn platform_delete(_: &str) -> Result<bool, SecretError> {
    Err(unsupported_platform())
}
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
fn unsupported_platform() -> SecretError {
    SecretError::Unavailable("this platform has no implemented credential-store adapter".into())
}

#[cfg(target_os = "linux")]
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
        let env = env_name(provider);
        if let Ok(secret) = std::env::var(&env) {
            return Ok(secret);
        }
        let unavailable = match self.store.get(provider) {
            Ok(Some(secret)) => return Ok(secret),
            Ok(None) => None,
            Err(error @ SecretError::Unavailable(_)) => Some(error),
            Err(error) => return Err(error),
        };
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

    #[cfg(target_os = "windows")]
    #[test]
    fn windows_credential_manager_round_trip() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let account = format!("hotsheet-test-{}-{nonce}", std::process::id());
        let secret = "native-windows-credential-secret";

        let _ = platform_delete(&account);
        platform_set(&account, secret).unwrap();
        assert_eq!(platform_get(&account).unwrap().as_deref(), Some(secret));
        assert!(platform_delete(&account).unwrap());
        assert_eq!(platform_get(&account).unwrap(), None);
        assert!(!platform_delete(&account).unwrap());
    }
}
