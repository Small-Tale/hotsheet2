//! Direct external ticket providers. Remote systems remain authoritative: adapters
//! translate their native records at the [`hotsheet_ticketing::TicketProvider`]
//! boundary and never mirror them into the default git store.

pub mod github;
pub mod github_app_config;
pub mod github_attachments;
pub mod github_credential;
pub mod github_device;
pub mod gitlab;
pub mod jira;
pub mod note_trailer;

pub use github::{
    GitHubConfig, GitHubProvider, GitHubTransport, GitHubWebhook, HttpResponse,
    UreqGitHubTransport, parse_webhook,
};
pub use github_attachments::GitHubAttachmentRepository;
pub use github_credential::{
    GitHubCredentialError, connection_access_token, credentials_with_sites,
    store_device_authorization,
};
pub use github_device::{
    AppInstallation, DeviceAuthorization, DevicePoll, GitHubDeviceClient, GitHubDeviceError,
    GitHubTokenBundle, RepositoryAccess,
};
pub use gitlab::{GitLabConfig, GitLabProvider};
pub use jira::{JiraConfig, JiraProvider};

use std::sync::Arc;

use hotsheet_ticketing::{ProviderConnection, ProviderDescriptor, ProviderError, TicketProvider};

pub fn credential_reference(connection: &ProviderConnection) -> Result<&str, ProviderError> {
    connection
        .settings
        .get("credential")
        .and_then(|value| value.get("secret"))
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| ProviderError::Authentication {
            connection_id: connection.id.clone(),
            message: "settings.credential.secret is required".into(),
        })
}

pub fn live_provider(
    connection: &ProviderConnection,
    token: String,
) -> Result<Arc<dyn TicketProvider>, ProviderError> {
    match connection.provider.as_str() {
        "github" => Ok(Arc::new(GitHubProvider::live(
            GitHubConfig::from_connection(connection, token)?,
        ))),
        "gitlab" => Ok(Arc::new(GitLabProvider::live(
            GitLabConfig::from_connection(connection, token)?,
        ))),
        "jira" => Ok(Arc::new(JiraProvider::live(JiraConfig::from_connection(
            connection, token,
        )?))),
        provider => Err(ProviderError::Conflict {
            ticket: connection.id.clone(),
            message: format!("external provider '{provider}' is not implemented"),
        }),
    }
}

pub fn descriptor(connection: &ProviderConnection) -> Result<ProviderDescriptor, ProviderError> {
    match connection.provider.as_str() {
        "github" => Ok(GitHubProvider::live(GitHubConfig::from_connection(
            connection,
            String::new(),
        )?)
        .descriptor()),
        "jira" => Ok(
            JiraProvider::live(JiraConfig::from_connection(connection, String::new())?)
                .descriptor(),
        ),
        "gitlab" => Ok(GitLabProvider::live(GitLabConfig::from_connection(
            connection,
            String::new(),
        )?)
        .descriptor()),
        provider => Err(ProviderError::Conflict {
            ticket: connection.id.clone(),
            message: format!("external provider '{provider}' is not implemented"),
        }),
    }
}

/// Prepare a non-secret connection registry update for either the HTTP server or a headless
/// host. The caller persists the returned list and owns any checkout link changes.
pub fn updated_connections(
    mut connections: Vec<ProviderConnection>,
    connection: ProviderConnection,
    replacing: Option<&str>,
) -> Result<Vec<ProviderConnection>, ProviderError> {
    if connection.provider == "git" {
        return Err(ProviderError::Conflict {
            ticket: connection.id,
            message: "git connections are managed through the store registry".into(),
        });
    }
    descriptor(&connection)?;
    if connection.default {
        for existing in &mut connections {
            existing.default = false;
        }
    }
    match replacing {
        Some(id) => {
            let slot = connections
                .iter_mut()
                .find(|candidate| candidate.id == id)
                .ok_or_else(|| ProviderError::UnknownConnection(id.into()))?;
            *slot = connection;
        }
        None => {
            if connections
                .iter()
                .any(|candidate| candidate.id == connection.id)
            {
                return Err(ProviderError::Conflict {
                    ticket: connection.id,
                    message: "provider connection id already exists".into(),
                });
            }
            connections.push(connection);
        }
    }
    connections.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(connections)
}

#[cfg(test)]
mod connection_tests {
    use super::*;

    fn connection(id: &str, locator: &str, default: bool) -> ProviderConnection {
        ProviderConnection {
            id: id.into(),
            provider: "github".into(),
            locator: locator.into(),
            name: Some("GitHub Issues".into()),
            default,
            settings: serde_json::json!({"credential":{"secret":"fixture"}}),
            disabled: false,
        }
    }

    #[test]
    fn connection_updates_preserve_one_default_and_reject_duplicate_ids() {
        let first = connection("github-one", "acme/one", true);
        let second = connection("github-two", "acme/two", true);
        let updated = updated_connections(vec![first], second.clone(), None).unwrap();
        assert!(!updated[0].default);
        assert!(updated[1].default);
        assert!(updated_connections(updated.clone(), second.clone(), None).is_err());
        assert_eq!(
            updated_connections(updated.clone(), second, Some("github-two")).unwrap(),
            updated
        );
        assert!(matches!(
            updated_connections(
                updated,
                connection("github-three", "acme/three", false),
                Some("missing")
            ),
            Err(ProviderError::UnknownConnection(_))
        ));
    }
}
