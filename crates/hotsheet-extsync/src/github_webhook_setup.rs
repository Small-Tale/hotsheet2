//! Shared idempotent GitHub webhook configuration for CLI and HTTP (HS2-SGQ183).

use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection, ProviderError};

/// Record only a machine-secret reference. Callers verify the reference resolves in
/// their own machine key registry before invoking this workflow.
pub fn configure_webhook(
    registry: &ProviderConfigRegistry,
    connection_id: &str,
    secret_ref: &str,
) -> Result<ProviderConnection, ProviderError> {
    if secret_ref.is_empty() || secret_ref.trim() != secret_ref {
        return Err(ProviderError::Conflict {
            ticket: connection_id.into(),
            message: "invalid webhook key reference".into(),
        });
    }
    registry.update(|connections| {
        let connection = connections
            .iter_mut()
            .find(|item| item.id == connection_id && item.provider == "github")
            .ok_or_else(|| ProviderError::UnknownConnection(connection_id.into()))?;
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
        Ok(connection.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};

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
        let first = configure_webhook(&registry, "github-synthetic", "synthetic-hook-key").unwrap();
        let second =
            configure_webhook(&registry, "github-synthetic", "synthetic-hook-key").unwrap();
        assert_eq!(first, second);
        assert_eq!(registry.load().unwrap(), vec![first]);
        assert!(configure_webhook(&registry, "github-synthetic", "synthetic-api-key").is_err());
        assert!(configure_webhook(&registry, "other", "synthetic-hook-key").is_err());
    }

    #[test]
    fn hs2_sgq183_setup_and_provider_edit_keep_both_concurrent_changes() {
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
        let barrier = Arc::new(Barrier::new(3));
        std::thread::scope(|scope| {
            let setup_barrier = barrier.clone();
            let setup_registry = registry.clone();
            scope.spawn(move || {
                setup_barrier.wait();
                configure_webhook(&setup_registry, "github-synthetic", "hook-key").unwrap();
            });
            let edit_barrier = barrier.clone();
            let edit_registry = registry.clone();
            scope.spawn(move || {
                edit_barrier.wait();
                edit_registry
                    .update(|connections| {
                        std::thread::sleep(std::time::Duration::from_millis(10));
                        connections[0].name = Some("Concurrent edit".into());
                        Ok(())
                    })
                    .unwrap();
            });
            barrier.wait();
        });
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
}
