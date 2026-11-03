//! HS2-4J6V46: metadata-only, provider-owned identity/capability projection.
//! Tokens, account names, locators and credential references are never returned.
use hotsheet_ticketing::{
    ProviderConnection,
    orchestration::{SourceState, TargetSource},
};
use sha2::{Digest, Sha256};

use crate::{GitHubConfig, GitLabConfig, JiraConfig};

/// Typed provider configuration owns source identity; no live provider, transport,
/// account lookup or SecretStore is constructed. Capabilities advertise implemented
/// operations only and confer no permission. Unknown providers remain explicit.
pub fn source_resolution(connection: &ProviderConnection) -> TargetSource {
    let mut source = TargetSource {
        connection_id: connection.id.clone(),
        provider: connection.provider.clone(),
        identity: None,
        state: SourceState::Unsupported,
        capabilities: None,
    };
    let fields = match connection.provider.as_str() {
        "github" => GitHubConfig::from_connection(connection, String::new()).map(|config| {
            (
                config.api_base.trim_end_matches('/').to_owned(),
                config.repository.clone(),
                config.descriptor().capabilities,
            )
        }),
        "gitlab" => GitLabConfig::from_connection(connection, String::new()).map(|config| {
            (
                config.api_base.trim_end_matches('/').to_owned(),
                config.project.clone(),
                config.descriptor().capabilities,
            )
        }),
        "jira" => JiraConfig::from_connection(connection, String::new()).map(|config| {
            (
                config.base_url.clone(),
                config.project_key.clone(),
                config.descriptor().capabilities,
            )
        }),
        _ => return source,
    };
    let Ok((endpoint, repository, capabilities)) = fields else {
        source.state = SourceState::ReviewRequired;
        return source;
    };
    if hotsheet_ticketing::orchestration::validate_source_identity_endpoint(&endpoint).is_err()
        || endpoint.chars().any(char::is_whitespace)
        || !valid_identity_locator(&connection.provider, &repository)
    {
        source.state = SourceState::ReviewRequired;
        return source;
    }
    // Length-delimited JSON fields avoid concatenation ambiguity. Use only these
    // explicit nonsecret fields; names/defaults/credentials/webhooks are excluded.
    let mut hash = Sha256::new();
    hash.update(b"hs2-external-source-identity-v1\0");
    hash.update(
        serde_json::to_vec(&[connection.provider.as_str(), &endpoint, &repository])
            .expect("source identity strings are serializable"),
    );
    source.identity = Some(format!("{:x}", hash.finalize()));
    source.state = if connection.disabled {
        SourceState::Disabled
    } else {
        SourceState::Available
    };
    source.capabilities = (source.state == SourceState::Available).then_some(capabilities);
    source
}

/// Projection-only grammar: legacy CRUD/typed constructors retain their contract.
/// Reject URLs, userinfo and query/fragment-bearing values before they can become
/// a secret-derived identity. Reuse owner parsers without applying their trimming.
fn valid_identity_locator(provider: &str, locator: &str) -> bool {
    if locator.is_empty()
        || locator.len() > 2048
        || locator
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return false;
    }
    match provider {
        "github" | "gitlab" => {
            let parts = locator.split('/').collect::<Vec<_>>();
            (if provider == "github" {
                parts.len() == 2
            } else {
                parts.len() >= 2
            }) && parts
                .iter()
                .all(|part| crate::github_attachments::valid_name(part))
        }
        "jira" => crate::jira::validate_key(locator).is_ok(),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connection(provider: &str) -> ProviderConnection {
        ProviderConnection {
            id: "synthetic".into(),
            provider: provider.into(),
            locator: if provider == "jira" {
                "SYNTH"
            } else {
                "synthetic/project"
            }
            .into(),
            name: None,
            default: false,
            disabled: false,
            settings: serde_json::json!({"credential":{"secret":"synthetic-reference"},"email":"synthetic@example.test","base_url":"https://jira.example.test"}),
        }
    }

    #[test]
    fn hs2_4j6v46_provider_identity_excludes_account_default_label_and_credential_metadata() {
        for provider in ["github", "gitlab", "jira"] {
            let mut config = connection(provider);
            let before = source_resolution(&config);
            assert_eq!(before.state, SourceState::Available);
            config.id = "renamed-source".into();
            config.name = Some("Changed label".into());
            config.default = true;
            config.settings["credential"]["secret"] = "other-reference".into();
            config.settings["email"] = "other@example.test".into();
            assert_eq!(source_resolution(&config).identity, before.identity);
            let wire = serde_json::to_string(&before).unwrap();
            for hidden in [
                "synthetic/project",
                "example.test",
                "synthetic-reference",
                "locator",
                "email",
                "credential",
            ] {
                assert!(!wire.contains(hidden));
            }
            config.locator = if provider == "jira" {
                "OTHER"
            } else {
                "different/project"
            }
            .into();
            assert_ne!(source_resolution(&config).identity, before.identity);
            config.disabled = true;
            let disabled = source_resolution(&config);
            assert_eq!(disabled.state, SourceState::Disabled);
            assert!(disabled.capabilities.is_none());
        }
    }

    #[test]
    fn hs2_4j6v46_provider_endpoint_identity_keeps_explicit_port_and_rejects_credentials() {
        let mut config = connection("github");
        config.settings["api_base"] = "https://api.example.test".into();
        let plain = source_resolution(&config);
        config.settings["api_base"] = "https://api.example.test:443".into();
        assert_ne!(source_resolution(&config).identity, plain.identity);
        config.settings["api_base"] = "https://user:secret@api.example.test".into();
        let rejected = source_resolution(&config);
        assert_eq!(rejected.state, SourceState::ReviewRequired);
        assert!(rejected.identity.is_none());
        assert!(rejected.capabilities.is_none());
        assert_eq!(
            source_resolution(&connection("unimplemented")).state,
            SourceState::Unsupported
        );
    }

    #[test]
    fn hs2_4j6v46_projection_rejects_persisted_secret_shaped_and_malformed_locators() {
        let home = tempfile::tempdir().unwrap();
        let registry =
            hotsheet_ticketing::ProviderConfigRegistry::new(home.path().join("providers.json"));
        let malformed = [
            "https://user:synthetic-secret@host.example.test/repo",
            "https://host.example.test/repo",
            "user:synthetic-secret@host/repo",
            "owner/repo?access_token=synthetic-secret",
            "owner/repo#synthetic-secret",
            "owner/repo%3Faccess_token=synthetic-secret",
            "owner/rep o",
            " owner/repo",
            "owner/repo ",
            "owner/rep\to",
            "owner/rep\no",
            "owner/rep\u{0000}o",
            "owner/rep\u{00a0}o",
            "owner/rep\u{2003}o",
            "owner//repo",
            "/owner/repo",
            "owner/repo/",
            "owner/../repo",
            "owner/./repo",
            "owner/repo\\path",
        ];
        for provider in ["github", "gitlab", "jira"] {
            for locator in malformed {
                let mut config = connection(provider);
                config.locator = locator.into();
                // HS2-6H8ZY6 config validation may now refuse some historical
                // shapes; any shape it still persists must fail closed after a real
                // save/load, and the in-memory record must fail closed either way.
                let stored = match registry.save(std::slice::from_ref(&config)) {
                    Ok(()) => registry.load().unwrap(),
                    Err(_) => Vec::new(),
                };
                for candidate in std::iter::once(&config).chain(stored.first()) {
                    let result = source_resolution(candidate);
                    assert_eq!(result.state, SourceState::ReviewRequired, "{provider}");
                    assert!(result.identity.is_none());
                    assert!(result.capabilities.is_none());
                }
            }
        }
    }

    #[test]
    fn hs2_4j6v46_projection_accepts_provider_owned_plain_identity_forms() {
        for (provider, locators) in [
            ("github", vec!["owner/repo", "owner-name/repo.name_v2"]),
            (
                "gitlab",
                vec!["group/project", "group/sub-group/nested/repo.name_v2"],
            ),
            ("jira", vec!["APP", "APP_2", "Mixed42"]),
        ] {
            for locator in locators {
                let mut config = connection(provider);
                config.locator = locator.into();
                let result = source_resolution(&config);
                assert_eq!(result.state, SourceState::Available);
                assert!(result.identity.is_some());
                assert!(result.capabilities.is_some());
            }
        }
        let mut github = connection("github");
        github.locator = "owner/subgroup/repo".into();
        assert_eq!(
            source_resolution(&github).state,
            SourceState::ReviewRequired
        );
    }
}
