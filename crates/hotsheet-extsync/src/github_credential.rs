//! Shared persistence and refresh for GitHub App device credentials. Both the HTTP server and
//! headless CLI keep the bundle in the OS keychain and pass only its access token to providers.

use hotsheet_ticketing::connection_removal::MANAGED_CREDENTIAL_PREFIX;
use hotsheet_ticketing::{
    KeyMetadata, KeyRegistry, ProviderConnection, ProviderError, SecretError, SecretStore,
};
use serde_json::{Value, json};

use crate::{GitHubDeviceClient, GitHubDeviceError, GitHubTokenBundle, credential_reference};

#[derive(Debug, thiserror::Error)]
pub enum GitHubCredentialError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Secret(#[from] SecretError),
    #[error("stored GitHub authorization is invalid: {0}")]
    Invalid(String),
    #[error("GitHub authorization expired; sign in with GitHub again")]
    Expired,
    #[error("GitHub authorization could not be refreshed; sign in again: {0}")]
    Refresh(#[from] GitHubDeviceError),
}

pub fn store_device_authorization<S: SecretStore>(
    keys: &KeyRegistry<S>,
    reference: &str,
    client_id: &str,
    web_base: &str,
    token: &GitHubTokenBundle,
    now: i64,
) -> Result<(), SecretError> {
    let stored = json!({"kind":"github_app","client_id":client_id,"web_base":web_base,"obtained_at":now,"token":token});
    keys.set(reference, &stored.to_string())?;
    // The web origin is not secret: keep it beside the name so an account no source uses yet
    // still reports its host (HS2-16MYXN).
    keys.record_site(reference, web_base)
}

/// The registry's credentials, after recording the web origin of every Hot Sheet GitHub
/// sign-in stored before `keys.json` kept one (HS2-16MYXN). Each such bundle is read from
/// the keychain once; its `web_base` is then kept as non-secret metadata, so later listings
/// need no keychain read. A bundle that cannot be read stays without a site rather than
/// failing the listing. Shared by `GET /accounts` and `hotsheet account list`.
pub fn credentials_with_sites<S: SecretStore>(
    keys: &KeyRegistry<S>,
) -> Result<Vec<KeyMetadata>, SecretError> {
    let mut listed = keys.list()?;
    for key in &mut listed {
        if key.site.is_some() || !key.provider.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            continue;
        }
        let Some(site) = keys
            .get(&key.provider)
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .filter(|stored| stored.get("kind").and_then(Value::as_str) == Some("github_app"))
            .and_then(|stored| {
                stored
                    .get("web_base")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            })
        else {
            continue;
        };
        if keys.record_site(&key.provider, &site).is_ok() {
            key.site = Some(site.trim_end_matches('/').to_owned());
        }
    }
    Ok(listed)
}

pub fn connection_access_token<S: SecretStore>(
    connection: &ProviderConnection,
    keys: &KeyRegistry<S>,
    now: i64,
) -> Result<String, GitHubCredentialError> {
    let reference = credential_reference(connection)?;
    let raw = keys.get(reference)?;
    if connection.provider != "github" {
        return Ok(raw);
    }
    resolve_token(
        &raw,
        now,
        |client_id, web_base, refresh| {
            GitHubDeviceClient::live(client_id, web_base).refresh(refresh)
        },
        |updated| {
            keys.set(reference, updated)
                .map_err(GitHubCredentialError::from)
        },
    )
}

fn resolve_token(
    raw: &str,
    now: i64,
    refresh: impl FnOnce(&str, &str, &str) -> Result<GitHubTokenBundle, GitHubDeviceError>,
    save: impl FnOnce(&str) -> Result<(), GitHubCredentialError>,
) -> Result<String, GitHubCredentialError> {
    let Ok(mut stored) = serde_json::from_str::<Value>(raw) else {
        return Ok(raw.to_owned());
    };
    if stored.get("kind").and_then(Value::as_str) != Some("github_app") {
        return Ok(raw.to_owned());
    }
    let mut token: GitHubTokenBundle =
        serde_json::from_value(stored.get("token").cloned().unwrap_or_default())
            .map_err(|error| GitHubCredentialError::Invalid(error.to_string()))?;
    let obtained = stored
        .get("obtained_at")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let expires = token.expires_in.unwrap_or(u64::MAX);
    let due = now
        >= obtained
            .saturating_add(i64::try_from(expires).unwrap_or(i64::MAX))
            .saturating_sub(60);
    if due {
        let refresh_token = token
            .refresh_token
            .as_deref()
            .ok_or(GitHubCredentialError::Expired)?;
        let client_id = stored
            .get("client_id")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let web_base = stored
            .get("web_base")
            .and_then(Value::as_str)
            .unwrap_or("https://github.com");
        let previous_refresh = token.refresh_token.clone();
        token = refresh(client_id, web_base, refresh_token)?;
        if token.refresh_token.is_none() {
            token.refresh_token = previous_refresh;
        }
        stored["obtained_at"] = now.into();
        stored["token"] = serde_json::to_value(&token)
            .map_err(|error| GitHubCredentialError::Invalid(error.to_string()))?;
        save(&stored.to_string())?;
    }
    Ok(token.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct MemorySecrets(Arc<Mutex<std::collections::HashMap<String, String>>>);

    impl SecretStore for MemorySecrets {
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

    fn bundle(access: &str, refresh: Option<&str>) -> GitHubTokenBundle {
        GitHubTokenBundle {
            access_token: access.into(),
            refresh_token: refresh.map(str::to_owned),
            expires_in: Some(120),
            refresh_token_expires_in: None,
            token_type: None,
            scope: None,
        }
    }

    #[test]
    fn raw_and_live_bundles_follow_the_same_refresh_boundary() {
        assert_eq!(
            resolve_token(
                "personal-token",
                100,
                |_, _, _| unreachable!(),
                |_| unreachable!()
            )
            .unwrap(),
            "personal-token"
        );
        let raw = json!({"kind":"github_app","client_id":"IvTest12345678","web_base":"https://github.test","obtained_at":100,"token":bundle("old",Some("refresh"))}).to_string();
        assert_eq!(
            resolve_token(&raw, 159, |_, _, _| unreachable!(), |_| unreachable!()).unwrap(),
            "old"
        );
        let mut saved = String::new();
        let access = resolve_token(
            &raw,
            160,
            |id, base, refresh| {
                assert_eq!(
                    (id, base, refresh),
                    ("IvTest12345678", "https://github.test", "refresh")
                );
                Ok(bundle("new", None))
            },
            |value| {
                saved = value.into();
                Ok(())
            },
        )
        .unwrap();
        assert_eq!(access, "new");
        assert_eq!(
            serde_json::from_str::<Value>(&saved).unwrap()["token"]["refresh_token"],
            "refresh"
        );
        assert_eq!(
            serde_json::from_str::<Value>(&saved).unwrap()["obtained_at"],
            160
        );
        assert_eq!(
            resolve_token(&saved, 161, |_, _, _| unreachable!(), |_| unreachable!()).unwrap(),
            "new"
        );
    }

    #[test]
    fn malformed_and_expired_bundles_never_pass_json_to_a_provider() {
        let invalid = json!({"kind":"github_app","token":{}}).to_string();
        assert!(matches!(
            resolve_token(&invalid, 0, |_, _, _| unreachable!(), |_| unreachable!()),
            Err(GitHubCredentialError::Invalid(_))
        ));
        let expired =
            json!({"kind":"github_app","obtained_at":0,"token":bundle("old",None)}).to_string();
        assert!(matches!(
            resolve_token(&expired, 61, |_, _, _| unreachable!(), |_| unreachable!()),
            Err(GitHubCredentialError::Expired)
        ));
    }

    #[test]
    fn device_authorization_is_saved_as_a_reference_and_resolved_for_provider_reads() {
        let home = tempfile::tempdir().unwrap();
        let keys = KeyRegistry::new(home.path(), MemorySecrets::default());
        store_device_authorization(
            &keys,
            "github-app-test",
            "IvTest12345678",
            "https://github.com",
            &bundle("access", Some("refresh")),
            100,
        )
        .unwrap();
        let connection = ProviderConnection {
            id: "github-main".into(),
            provider: "github".into(),
            locator: "acme/repo".into(),
            name: None,
            default: false,
            settings: json!({"credential":{"secret":"github-app-test"}}),
            disabled: false,
        };
        assert_eq!(
            connection_access_token(&connection, &keys, 159).unwrap(),
            "access"
        );
        let stored: Value = serde_json::from_str(&keys.get("github-app-test").unwrap()).unwrap();
        assert_eq!(stored["client_id"], "IvTest12345678");
        assert_eq!(stored["obtained_at"], 100);
        // The web origin is recorded as non-secret metadata (HS2-16MYXN).
        assert_eq!(
            keys.list().unwrap()[0].site.as_deref(),
            Some("https://github.com")
        );
    }

    #[test]
    fn sign_ins_stored_before_sites_existed_are_backfilled_once() {
        let home = tempfile::tempdir().unwrap();
        let secrets = MemorySecrets::default();
        let keys = KeyRegistry::new(home.path(), secrets.clone());
        // A pre-HS2-16MYXN Enterprise sign-in: bundle in the keychain, no site in keys.json.
        keys.set(
            "github-app-old",
            &json!({"kind":"github_app","client_id":"IvTest12345678","web_base":"https://ghe.corp.test/","obtained_at":1,"token":bundle("a",None)}).to_string(),
        )
        .unwrap();
        // A plain key and a managed-looking name whose bundle is not a sign-in stay site-less.
        keys.set("anthropic", "sk-test").unwrap();
        keys.set("github-app-broken", "not json").unwrap();
        let listed = credentials_with_sites(&keys).unwrap();
        let sites = listed
            .iter()
            .map(|key| (key.provider.as_str(), key.site.as_deref()))
            .collect::<Vec<_>>();
        assert_eq!(
            sites,
            [
                ("anthropic", None),
                ("github-app-broken", None),
                ("github-app-old", Some("https://ghe.corp.test")),
            ]
        );
        // Recorded on disk, so the next listing reads no keychain entry for it.
        secrets.0.lock().unwrap().remove("github-app-old");
        let again = credentials_with_sites(&keys).unwrap();
        assert_eq!(again[2].site.as_deref(), Some("https://ghe.corp.test"));
    }
}
