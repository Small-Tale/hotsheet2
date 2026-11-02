//! Shared idempotent GitHub webhook configuration for CLI and HTTP (HS2-SGQ183).

use hotsheet_ticketing::github_endpoint::EndpointRegistry;
use hotsheet_ticketing::{
    KeyRegistry, ProviderConfigRegistry, ProviderConnection, ProviderError, SecretStore,
};

#[derive(Debug, thiserror::Error)]
pub enum WebhookSetupError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("webhook key reference unavailable; run `hotsheet-cli key set <reference>` first")]
    KeyUnavailable,
}

pub struct WebhookConfiguration {
    pub connection: ProviderConnection,
    pub previous_ref: Option<String>,
}

/// HS2-6H8ZY6: own the actual key lookup outside locks between exact ordinary
/// snapshot checks. Never read the previous key or recapture after an effect.
pub fn configure_webhook<S: SecretStore>(
    registry: &ProviderConfigRegistry,
    keys: &KeyRegistry<S>,
    connection_id: &str,
    secret_ref: &str,
) -> Result<WebhookConfiguration, WebhookSetupError> {
    if secret_ref.is_empty() || secret_ref.trim() != secret_ref {
        return Err(ProviderError::Conflict {
            ticket: connection_id.into(),
            message: "invalid webhook key reference".into(),
        }
        .into());
    }
    let owner = EndpointRegistry::new(registry);
    let snapshot = owner.capture_ordinary(connection_id)?;
    let previous_ref = snapshot
        .connection()
        .settings
        .pointer("/webhook/secret")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    if snapshot
        .connection()
        .settings
        .pointer("/credential/secret")
        .and_then(serde_json::Value::as_str)
        == Some(secret_ref)
    {
        return Err(ProviderError::Conflict {
            ticket: connection_id.into(),
            message: "webhook key must differ from provider credential".into(),
        }
        .into());
    }
    owner.check_ordinary(&snapshot)?;
    let key = keys.get(secret_ref);
    owner.check_ordinary(&snapshot)?;
    key.map_err(|_| WebhookSetupError::KeyUnavailable)?;
    let connection = owner.update_ordinary(&snapshot, |connection| {
        if connection
            .settings
            .pointer("/credential/secret")
            .and_then(serde_json::Value::as_str)
            == Some(secret_ref)
        {
            return Err(ProviderError::Conflict {
                ticket: connection_id.into(),
                message: "webhook key must differ from provider credential".into(),
            });
        }
        if !connection.settings.is_object() {
            connection.settings = serde_json::json!({});
        }
        connection.settings["webhook"] = serde_json::json!({"secret": secret_ref});
        Ok(())
    })?;
    Ok(WebhookConfiguration {
        connection,
        previous_ref,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::{
        SecretError,
        github_endpoint::{EndpointError, State},
    };
    use std::collections::BTreeMap;
    use std::sync::{Arc, Mutex};

    type GetHook = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;
    #[derive(Clone, Default)]
    struct Secrets {
        values: Arc<Mutex<BTreeMap<String, String>>>,
        gets: Arc<Mutex<Vec<String>>>,
        during_get: GetHook,
    }
    impl SecretStore for Secrets {
        fn get(&self, reference: &str) -> Result<Option<String>, SecretError> {
            self.gets.lock().unwrap().push(reference.into());
            let value = self.values.lock().unwrap().get(reference).cloned();
            let hook = self.during_get.lock().unwrap().take();
            // The actual get cannot return until this owned interleaving finishes.
            // No worker/barrier can strand another thread on assertion failure.
            if let Some(hook) = hook {
                hook();
            }
            Ok(value)
        }
        fn set(&self, reference: &str, value: &str) -> Result<(), SecretError> {
            self.values
                .lock()
                .unwrap()
                .insert(reference.into(), value.into());
            Ok(())
        }
        fn delete(&self, _: &str) -> Result<bool, SecretError> {
            panic!("setup must not delete a key")
        }
    }
    fn keys(home: &std::path::Path) -> (KeyRegistry<Secrets>, Secrets) {
        let secrets = Secrets::default();
        let keys = KeyRegistry::new(home, secrets.clone());
        assert!(std::env::var_os(hotsheet_ticketing::secrets::env_name("missing-hook")).is_none());
        for reference in ["synthetic-hook-key", "hook-key", "replacement-hook"] {
            assert!(std::env::var_os(hotsheet_ticketing::secrets::env_name(reference)).is_none());
            keys.set(reference, "synthetic hook bytes").unwrap();
        }
        (keys, secrets)
    }

    #[test]
    fn hs2_sgq183_setup_is_idempotent_and_keeps_credentials_separate() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        registry
            .save(&[ProviderConnection {
                id: "github-synthetic".into(),
                provider: "github".into(),
                locator: "synthetic/issues".into(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({"credential":{"secret":"synthetic-api-key"}}),
            }])
            .unwrap();
        let (keys, secrets) = keys(dir.path());
        let first =
            configure_webhook(&registry, &keys, "github-synthetic", "synthetic-hook-key").unwrap();
        let second =
            configure_webhook(&registry, &keys, "github-synthetic", "synthetic-hook-key").unwrap();
        assert_eq!(first.connection, second.connection);
        assert_eq!(first.previous_ref, None);
        assert_eq!(second.previous_ref.as_deref(), Some("synthetic-hook-key"));
        assert_eq!(registry.load().unwrap(), vec![first.connection]);
        assert!(
            configure_webhook(&registry, &keys, "github-synthetic", "synthetic-api-key").is_err()
        );
        assert!(configure_webhook(&registry, &keys, "other", "synthetic-hook-key").is_err());
        assert_eq!(
            *secrets.gets.lock().unwrap(),
            ["synthetic-hook-key", "synthetic-hook-key"]
        );
    }

    #[test]
    fn hs2_6h8zy6_actual_get_source_edit_refuses_old_snapshot_and_fresh_retry_preserves_edit() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        registry
            .save(&[ProviderConnection {
                id: "github-synthetic".into(),
                provider: "github".into(),
                locator: "synthetic/issues".into(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({"credential":{"secret":"api-key"}}),
            }])
            .unwrap();
        let (keys, secrets) = keys(dir.path());
        let edit_registry = registry.clone();
        *secrets.during_get.lock().unwrap() = Some(Box::new(move || {
            edit_registry
                .update(|connections| {
                    connections[0].name = Some("Concurrent edit".into());
                    Ok(())
                })
                .unwrap();
        }));
        assert!(matches!(
            configure_webhook(&registry, &keys, "github-synthetic", "hook-key"),
            Err(WebhookSetupError::Provider(ProviderError::Endpoint(
                EndpointError::Conflict
            )))
        ));
        let after_refusal = registry.load().unwrap().remove(0);
        assert_eq!(after_refusal.name.as_deref(), Some("Concurrent edit"));
        assert!(after_refusal.settings.get("webhook").is_none());
        let retried = configure_webhook(&registry, &keys, "github-synthetic", "hook-key").unwrap();
        assert_eq!(retried.connection, registry.load().unwrap()[0]);
        assert_eq!(*secrets.gets.lock().unwrap(), ["hook-key", "hook-key"]);
        let [final_connection] = registry.load().unwrap().try_into().unwrap();
        assert_eq!(final_connection.name.as_deref(), Some("Concurrent edit"));
        assert_eq!(
            final_connection
                .settings
                .pointer("/webhook/secret")
                .and_then(serde_json::Value::as_str),
            Some("hook-key")
        );
    }

    #[test]
    fn hs2_6h8zy6_configuration_zero_get_refusals_and_disabled_metadata_maintenance() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        let source = ProviderConnection {
            id: "source".into(),
            provider: "github".into(),
            locator: "synthetic/project".into(),
            name: None,
            default: false,
            disabled: true,
            settings: serde_json::json!({"credential":{"secret":"provider-key"},"webhook":{"secret":"obsolete-hook"}}),
        };
        registry.save(&[source]).unwrap();
        let (keys, secrets) = keys(dir.path());
        for reference in ["", " hook-key", "provider-key"] {
            assert!(configure_webhook(&registry, &keys, "source", reference).is_err());
        }
        assert!(secrets.gets.lock().unwrap().is_empty());
        let saved = configure_webhook(&registry, &keys, "source", "replacement-hook").unwrap();
        assert!(saved.connection.disabled);
        assert_eq!(saved.previous_ref.as_deref(), Some("obsolete-hook"));
        assert_eq!(
            saved.connection.settings["webhook"]["secret"],
            "replacement-hook"
        );
        assert_eq!(*secrets.gets.lock().unwrap(), ["replacement-hook"]);
        assert!(
            State::load(&saved.connection).unwrap().is_none(),
            "metadata-only legacy edit must not invent authority"
        );
        let before = std::fs::read(registry.path()).unwrap();
        assert!(matches!(
            configure_webhook(&registry, &keys, "source", "missing-hook"),
            Err(WebhookSetupError::KeyUnavailable)
        ));
        assert_eq!(std::fs::read(registry.path()).unwrap(), before);
        assert_eq!(
            *secrets.gets.lock().unwrap(),
            ["replacement-hook", "missing-hook"]
        );
    }

    #[test]
    fn hs2_6h8zy6_configuration_malformed_pending_and_during_get_refuse_without_publication() {
        use hotsheet_ticketing::github_endpoint::{
            Endpoint, Intent, Link, LinkObserver, Observation, Request,
        };
        struct NoLinks;
        impl LinkObserver for NoLinks {
            fn is_linked(&self, _: &str, _: &Link) -> Result<bool, EndpointError> {
                Ok(false)
            }
        }
        for condition in ["malformed", "pending", "during-get"] {
            let dir = tempfile::tempdir().unwrap();
            let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
            let raw = ProviderConnection {
                id: "source".into(),
                provider: "github".into(),
                locator: "synthetic/project".into(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({"credential":{"secret":"provider-key"}}),
            };
            let saved = EndpointRegistry::new(&registry)
                .apply(
                    &Request {
                        operation_id: hotsheet_model::Ulid::new(),
                        expected: None,
                        connection: raw,
                        intent: Intent::Explicit {
                            credential_ref: "provider-key".into(),
                            endpoint: Endpoint::Public,
                        },
                        link: (condition == "pending").then(|| Link {
                            checkout_id: "synthetic-checkout".into(),
                            make_default: false,
                        }),
                    },
                    &Observation::Unavailable,
                    &NoLinks,
                )
                .unwrap()
                .connection;
            let (keys, secrets) = keys(dir.path());
            if condition == "malformed" {
                let mut broken = saved.clone();
                broken.settings["github_endpoint"] = serde_json::Value::Null;
                std::fs::write(
                    registry.path(),
                    serde_json::to_vec(&serde_json::json!({"connections":[broken]})).unwrap(),
                )
                .unwrap();
            }
            if condition == "during-get" {
                let changed = registry.clone();
                *secrets.during_get.lock().unwrap() = Some(Box::new(move || {
                    changed
                        .update(|rows| {
                            rows.clear();
                            Ok(())
                        })
                        .unwrap();
                }));
            }
            let before = std::fs::read(registry.path()).unwrap();
            assert!(matches!(
                configure_webhook(&registry, &keys, "source", "hook-key"),
                Err(WebhookSetupError::Provider(ProviderError::Endpoint(_)))
                    | Err(WebhookSetupError::Provider(
                        ProviderError::UnknownConnection(_)
                    ))
            ));
            if condition == "during-get" {
                assert!(
                    registry.load().unwrap().is_empty(),
                    "failed setup must not resurrect removed record"
                );
            } else {
                assert_eq!(std::fs::read(registry.path()).unwrap(), before);
            }
            assert_eq!(
                secrets.gets.lock().unwrap().len(),
                usize::from(condition == "during-get")
            );
        }
    }
}
