//! Direct external ticket providers. Remote systems remain authoritative: adapters
//! translate their native records at the [`hotsheet_ticketing::TicketProvider`]
//! boundary and never mirror them into the default git store.

pub mod github;

pub use github::{
    GitHubConfig, GitHubProvider, GitHubTransport, GitHubWebhook, HttpResponse,
    UreqGitHubTransport, parse_webhook,
};

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
        provider => Err(ProviderError::Conflict {
            ticket: connection.id.clone(),
            message: format!("external provider '{provider}' is not implemented"),
        }),
    }
}
