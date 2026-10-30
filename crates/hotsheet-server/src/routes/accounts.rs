//! Hosted-store listing, GitHub device sign-in, and account routes.

use crate::api_error::ApiError;
use crate::multistore::StoreInfo;
use crate::routes::providers::{connection_token, provider_transfer_error};
use crate::{AppState, github_app_config};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::Duration;
use time::OffsetDateTime;

// ---- multi-store (HS2-87) --------------------------------------------------------

/// `GET /stores` — the stores this machine server hosts, with ticket counts. Counting
/// parses every ticket in every store, so it runs on a blocking thread and never stalls
/// `/health` or other requests (HS2-4XXRJP).
pub(crate) async fn list_stores(
    State(state): State<AppState>,
) -> Result<Json<Vec<StoreInfo>>, ApiError> {
    let host = state.host.clone();
    tokio::task::spawn_blocking(move || host.list())
        .await
        .map(Json)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum GitHubAuthStatus {
    Pending,
    Authorized { credential_reference: String },
    Denied,
    Expired,
    Cancelled,
    Error { message: String },
}

pub(crate) struct GitHubAuthSession {
    status: tokio::sync::watch::Sender<GitHubAuthStatus>,
    client_id: String,
    web_base: String,
    credential_reference: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StartGitHubAuthBody {
    #[serde(default = "default_github_web_base")]
    web_base: String,
}

fn default_github_web_base() -> String {
    "https://github.com".into()
}

#[derive(Debug, Serialize)]
pub(crate) struct StartGitHubAuthResponse {
    session_id: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
}

pub(crate) async fn start_github_device_auth(
    State(state): State<AppState>,
    Json(body): Json<StartGitHubAuthBody>,
) -> Result<(StatusCode, Json<StartGitHubAuthResponse>), ApiError> {
    let web_base = body.web_base.trim_end_matches('/').to_owned();
    let client_id = github_app_config::client_id_for_web_base(&web_base)
        .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error))?;
    let client = hotsheet_extsync::GitHubDeviceClient::live(client_id.clone(), web_base.clone());
    let authorization = tokio::task::spawn_blocking(move || client.start())
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    let session_id = ulid::Ulid::new().to_string();
    let credential_reference = format!("github-app-{}", session_id.to_ascii_lowercase());
    let (status, _) = tokio::sync::watch::channel(GitHubAuthStatus::Pending);
    let session = Arc::new(GitHubAuthSession {
        status,
        client_id: client_id.clone(),
        web_base: web_base.clone(),
        credential_reference: credential_reference.clone(),
    });
    state
        .github_auth_sessions
        .lock_or_recover()
        .insert(session_id.clone(), session.clone());
    let device_code = authorization.device_code.clone();
    let mut interval = authorization.interval.max(1);
    let keys = state.key_registry();
    std::thread::spawn(move || {
        let client =
            hotsheet_extsync::GitHubDeviceClient::live(client_id.clone(), web_base.clone());
        loop {
            std::thread::sleep(Duration::from_secs(interval));
            if !matches!(*session.status.borrow(), GitHubAuthStatus::Pending) {
                break;
            }
            match client.poll(&device_code) {
                Ok(hotsheet_extsync::DevicePoll::Pending) => {}
                Ok(hotsheet_extsync::DevicePoll::SlowDown) => interval = interval.saturating_add(5),
                Ok(hotsheet_extsync::DevicePoll::Expired) => {
                    let _ = session.status.send(GitHubAuthStatus::Expired);
                    break;
                }
                Ok(hotsheet_extsync::DevicePoll::Denied) => {
                    let _ = session.status.send(GitHubAuthStatus::Denied);
                    break;
                }
                Ok(hotsheet_extsync::DevicePoll::Authorized(bundle)) => {
                    let login = client.current_login(&bundle.access_token).ok();
                    let result = hotsheet_extsync::store_device_authorization(
                        &keys,
                        &credential_reference,
                        &client_id,
                        &web_base,
                        &bundle,
                        OffsetDateTime::now_utc().unix_timestamp(),
                    );
                    if result.is_ok() {
                        if let Some(login) = login {
                            // The credential itself is stored; only the display login is lost.
                            if let Err(error) = keys.record_identity(&credential_reference, &login)
                            {
                                tracing::warn!(%error, "recording the GitHub login failed");
                            }
                        }
                    }
                    let next = match result {
                        Ok(()) => GitHubAuthStatus::Authorized {
                            credential_reference: credential_reference.clone(),
                        },
                        Err(error) => GitHubAuthStatus::Error {
                            message: error.to_string(),
                        },
                    };
                    let _ = session.status.send(next);
                    break;
                }
                Err(error) => {
                    let _ = session.status.send(GitHubAuthStatus::Error {
                        message: error.to_string(),
                    });
                    break;
                }
            }
        }
    });
    Ok((
        StatusCode::ACCEPTED,
        Json(StartGitHubAuthResponse {
            session_id,
            user_code: authorization.user_code,
            verification_uri: authorization.verification_uri,
            expires_in: authorization.expires_in,
        }),
    ))
}

pub(crate) async fn wait_github_device_auth(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<GitHubAuthStatus>, ApiError> {
    let session = state
        .github_auth_sessions
        .lock_or_recover()
        .get(&session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(&session_id))?;
    let mut status = session.status.subscribe();
    let pending = matches!(*status.borrow(), GitHubAuthStatus::Pending);
    if pending {
        let _ = status.changed().await;
    }
    let result = status.borrow().clone();
    Ok(Json(result))
}

pub(crate) async fn cancel_github_device_auth(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<StatusCode, ApiError> {
    let session = state
        .github_auth_sessions
        .lock_or_recover()
        .get(&session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(&session_id))?;
    let _ = session.status.send(GitHubAuthStatus::Cancelled);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Serialize)]
pub(crate) struct GitHubRepositoriesResponse {
    repositories: Vec<String>,
    /// What each app installation grants and where to change it (HS2-27T5WT).
    installations: Vec<hotsheet_extsync::AppInstallation>,
    install_url: Option<String>,
}

pub(crate) async fn list_github_auth_repositories(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<GitHubRepositoriesResponse>, ApiError> {
    let session = state
        .github_auth_sessions
        .lock_or_recover()
        .get(&session_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(&session_id))?;
    if !matches!(
        *session.status.borrow(),
        GitHubAuthStatus::Authorized { .. }
    ) {
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            "GitHub sign-in is not complete",
        ));
    }
    let raw = state
        .key_registry()
        .get(&session.credential_reference)
        .map_err(provider_transfer_error)?;
    let stored: serde_json::Value = serde_json::from_str(&raw).map_err(|error| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            format!("stored GitHub authorization is invalid: {error}"),
        )
    })?;
    let token: hotsheet_extsync::GitHubTokenBundle = serde_json::from_value(
        stored.get("token").cloned().unwrap_or_default(),
    )
    .map_err(|error| {
        ApiError::new(
            StatusCode::UNAUTHORIZED,
            format!("stored GitHub authorization is invalid: {error}"),
        )
    })?;
    github_repository_access(
        session.client_id.clone(),
        session.web_base.clone(),
        token.access_token,
    )
    .await
}

async fn github_repository_access(
    client_id: String,
    web_base: String,
    access_token: String,
) -> Result<Json<GitHubRepositoriesResponse>, ApiError> {
    let client = hotsheet_extsync::GitHubDeviceClient::live(client_id, &web_base);
    let access = tokio::task::spawn_blocking(move || client.repository_access(&access_token))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    repository_access_response(access, || {
        github_app_config::install_url_for_web_base(&web_base)
    })
    .map(Json)
    .map_err(|error| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, error))
}

// Shared by newly authorized sessions and reused accounts. Do not require an
// Enterprise fallback when GitHub already supplied its registered App URL.
// HS2-FDTJ98: optional link metadata must not hide existing grants or repositories.
fn repository_access_response(
    access: hotsheet_extsync::RepositoryAccess,
    configured_install_url: impl FnOnce() -> Result<String, String>,
) -> Result<GitHubRepositoriesResponse, String> {
    let install_url = match access.install_url {
        Some(url) => Some(url),
        None => match configured_install_url() {
            Ok(url) => Some(url),
            Err(_) if !access.installations.is_empty() || !access.repositories.is_empty() => None,
            Err(error) => return Err(error),
        },
    };
    Ok(GitHubRepositoriesResponse {
        repositories: access.repositories,
        installations: access.installations,
        install_url,
    })
}

pub(crate) fn account_listing_inputs(
    state: &AppState,
) -> Result<
    (
        Vec<ProviderConnection>,
        Vec<hotsheet_ticketing::checkouts::Checkout>,
    ),
    ApiError,
> {
    let connections = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(provider_transfer_error)?;
    let checkouts = state
        .checkout_registry
        .list()
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok((connections, checkouts))
}

/// `GET /accounts` (HS2-SM9PM8): machine-wide provider sign-ins, each with the ticket
/// sources signed in through it and the projects that own those sources.
pub(crate) async fn list_accounts_route(
    State(state): State<AppState>,
) -> Result<Json<Vec<hotsheet_ticketing::accounts::Account>>, ApiError> {
    tokio::task::spawn_blocking(move || {
        let (connections, checkouts) = account_listing_inputs(&state)?;
        // Records the site of sign-ins stored before keys.json kept one (HS2-16MYXN).
        let credentials = hotsheet_extsync::credentials_with_sites(&state.key_registry())
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        Ok(Json(hotsheet_ticketing::accounts::list_accounts(
            &connections,
            &checkouts,
            &credentials,
        )))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// Resolve an older managed GitHub sign-in's username only when the user requests it.
/// Account listing itself never contacts GitHub or reads an already indexed keychain item.
pub(crate) async fn identify_github_account(
    State(state): State<AppState>,
    Path(account): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    tokio::task::spawn_blocking(move || {
        if !account.starts_with(hotsheet_ticketing::connection_removal::MANAGED_CREDENTIAL_PREFIX) {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "Account is not a managed GitHub sign-in",
            ));
        }
        let keys = state.key_registry();
        if !keys
            .list()
            .map_err(provider_transfer_error)?
            .iter()
            .any(|key| key.provider == account)
        {
            return Err(ApiError::not_found(&account));
        }
        let raw = keys.get(&account).map_err(|error| match error {
            hotsheet_ticketing::SecretError::NotFound(_) => ApiError::not_found(&account),
            other => provider_transfer_error(other),
        })?;
        let stored: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
        if stored.get("kind").and_then(serde_json::Value::as_str) != Some("github_app") {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                "Account is not a managed GitHub sign-in",
            ));
        }
        let client_id = stored
            .get("client_id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        let web_base = stored
            .get("web_base")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("https://github.com");
        let token = hotsheet_extsync::access_token_from_raw(
            &raw,
            &keys,
            &account,
            OffsetDateTime::now_utc().unix_timestamp(),
        )
        .map_err(|error| ApiError::new(StatusCode::UNAUTHORIZED, error.to_string()))?;
        let login = hotsheet_extsync::GitHubDeviceClient::live(client_id, web_base)
            .current_login(&token)
            .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
        keys.record_identity(&account, &login)
            .map_err(provider_transfer_error)?;
        Ok(Json(serde_json::json!({"identity": login})))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// `DELETE /accounts/{account}`: sign out. Refused (409) while a ticket source uses it.
pub(crate) async fn sign_out_account(
    State(state): State<AppState>,
    Path(account): Path<String>,
) -> Result<StatusCode, ApiError> {
    tokio::task::spawn_blocking(move || {
        let (connections, checkouts) = account_listing_inputs(&state)?;
        hotsheet_ticketing::accounts::sign_out(
            &connections,
            &checkouts,
            &state.key_registry(),
            &account,
        )
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(|error| {
            let status = match error {
                hotsheet_ticketing::accounts::AccountError::InUse { .. } => StatusCode::CONFLICT,
                hotsheet_ticketing::accounts::AccountError::NotFound(_) => StatusCode::NOT_FOUND,
                hotsheet_ticketing::accounts::AccountError::Secret(
                    hotsheet_ticketing::SecretError::InvalidProvider(_),
                ) => StatusCode::BAD_REQUEST,
                _ => StatusCode::INTERNAL_SERVER_ERROR,
            };
            ApiError::new(status, error.to_string())
        })
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// Remove only a source no checkout links and whose credential belongs to this account.
/// The account remains signed in for reuse or a separate sign-out (HS2-G0E8ZS).
pub(crate) async fn remove_unused_account_source(
    State(state): State<AppState>,
    Path((account, connection_id)): Path<(String, String)>,
) -> Result<StatusCode, ApiError> {
    tokio::task::spawn_blocking(move || {
        let providers = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
        let checkouts = state
            .checkout_registry
            .list()
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        providers
            .update(|connections| {
                let mut candidate = connections.clone();
                let outcome = hotsheet_ticketing::accounts::take_unused_source(
                    &mut candidate,
                    &checkouts,
                    &account,
                    &connection_id,
                );
                if outcome.is_ok() {
                    *connections = candidate;
                }
                Ok(outcome)
            })
            .map_err(provider_transfer_error)?
            .map_err(|error| {
                let status = match error {
                    hotsheet_ticketing::accounts::AccountError::SourceNotFound { .. } => {
                        StatusCode::NOT_FOUND
                    }
                    hotsheet_ticketing::accounts::AccountError::SourceInUse { .. } => {
                        StatusCode::CONFLICT
                    }
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                ApiError::new(status, error.to_string())
            })?;
        Ok(StatusCode::NO_CONTENT)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
}

/// `GET /accounts/{account}/github-repositories`: the repositories a signed-in GitHub
/// account can reach, so another project can add its own repository without signing in
/// again (HS2-SM9PM8). The token stays server-side and is refreshed when due.
pub(crate) async fn list_account_github_repositories(
    State(state): State<AppState>,
    Path(account): Path<String>,
) -> Result<Json<GitHubRepositoriesResponse>, ApiError> {
    let keys = state.key_registry();
    let raw = keys.get(&account).map_err(|error| match error {
        hotsheet_ticketing::SecretError::NotFound(_) => ApiError::not_found(&account),
        other => provider_transfer_error(other),
    })?;
    let stored: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
    if stored.get("kind").and_then(serde_json::Value::as_str) != Some("github_app") {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("'{account}' is not a Hot Sheet GitHub sign-in"),
        ));
    }
    let field = |key: &str, fallback: &str| {
        stored
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or(fallback)
            .to_owned()
    };
    let (client_id, web_base) = (
        field("client_id", ""),
        field("web_base", "https://github.com"),
    );
    let probe = ProviderConnection {
        id: "account-probe".into(),
        provider: "github".into(),
        locator: String::new(),
        name: None,
        default: false,
        settings: serde_json::json!({"credential": {"secret": account}}),
        disabled: false,
    };
    let token = tokio::task::spawn_blocking(move || connection_token(&state, &probe))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    github_repository_access(client_id, web_base, token).await
}

#[cfg(test)]
mod installation_url_tests {
    use super::*;

    // Restore overrides even after assertion failures; never read or write the keychain.
    struct SyntheticEnvironment(Vec<(&'static str, Option<std::ffi::OsString>)>);

    impl SyntheticEnvironment {
        fn set(&mut self, key: &'static str, value: String) {
            self.0.push((key, std::env::var_os(key)));
            unsafe { std::env::set_var(key, value) };
        }
    }

    impl Drop for SyntheticEnvironment {
        fn drop(&mut self) {
            for (key, value) in self.0.iter().rev() {
                unsafe {
                    match value {
                        Some(value) => std::env::set_var(key, value),
                        None => std::env::remove_var(key),
                    }
                }
            }
        }
    }

    #[tokio::test]
    async fn hs2_fdtj98_http_fresh_and_saved_accounts_offer_first_installation() {
        use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

        let calls = Arc::new(AtomicUsize::new(0));
        let upstream_calls = calls.clone();
        let existing_installation = Arc::new(AtomicBool::new(false));
        let upstream_existing = existing_installation.clone();
        let repository_calls = calls.clone();
        let upstream = axum::Router::new()
            .route(
                "/api/v3/user/installations",
                axum::routing::get(move |headers: axum::http::HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer synthetic-fdtj98-token");
                    upstream_calls.fetch_add(1, Ordering::SeqCst);
                    let installations = if upstream_existing.load(Ordering::SeqCst) {
                        serde_json::json!([{
                            "id": 7, "account": {"login": "synthetic"},
                            "repository_selection": "selected"
                        }])
                    } else {
                        serde_json::json!([])
                    };
                    Json(serde_json::json!({"installations": installations}))
                }),
            )
            .route(
                "/api/v3/user/installations/7/repositories",
                axum::routing::get(move |headers: axum::http::HeaderMap| async move {
                    assert_eq!(headers["authorization"], "Bearer synthetic-fdtj98-token");
                    repository_calls.fetch_add(1, Ordering::SeqCst);
                    Json(serde_json::json!({"repositories": [{"full_name": "synthetic/repository"}]}))
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let upstream_task = tokio::spawn(async move {
            axum::serve(listener, upstream).await.unwrap();
        });
        let mut environment = SyntheticEnvironment(Vec::new());
        environment.set(
            "HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS",
            serde_json::json!({&origin: "synthetic-app"}).to_string(),
        );
        let credential = "github-app-hs2-fdtj98-http";
        environment.set(
            "HOTSHEET_API_KEY_GITHUB_APP_HS2_FDTJ98_HTTP",
            serde_json::json!({
                "kind": "github_app", "client_id": "synthetic-client", "web_base": origin,
                "token": {"access_token": "synthetic-fdtj98-token"}
            })
            .to_string(),
        );
        let dir = tempfile::tempdir().unwrap();
        let store = hotsheet_ticketing::FsStore::init(
            dir.path().join("store"),
            &hotsheet_ticketing::StoreMetadata::new("HS"),
        )
        .unwrap();
        let state = AppState::new(store, "synthetic-server-token".into())
            .unwrap()
            .with_default_codex_home(dir.path().join("no-codex-home"))
            .with_machine_home(dir.path().join("machine-home"));
        // Start at the post-authorization boundary: OAuth approval itself is external.
        let (status, _) = tokio::sync::watch::channel(GitHubAuthStatus::Authorized {
            credential_reference: credential.into(),
        });
        state.github_auth_sessions.lock_or_recover().insert(
            "synthetic-session".into(),
            Arc::new(GitHubAuthSession {
                status,
                client_id: "synthetic-client".into(),
                web_base: origin.clone(),
                credential_reference: credential.into(),
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_origin = format!("http://{}", listener.local_addr().unwrap());
        let server_task = tokio::spawn(async move {
            axum::serve(listener, crate::app(state)).await.unwrap();
        });
        let paths = [
            "/github-auth/device/synthetic-session/repositories".to_owned(),
            format!("/accounts/{credential}/github-repositories"),
        ];
        for (configured, installed) in [(true, false), (false, false), (false, true)] {
            existing_installation.store(installed, Ordering::SeqCst);
            if !configured {
                environment.set("HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS", "{}".into());
            }
            for path in &paths {
                let url = format!("{server_origin}{path}");
                let (status, body) = tokio::task::spawn_blocking(move || {
                    let agent = ureq::AgentBuilder::new()
                        .timeout(Duration::from_secs(5))
                        .build();
                    let response = match agent
                        .get(&url)
                        .set("x-hotsheet-secret", "synthetic-server-token")
                        .call()
                    {
                        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
                        Err(error) => panic!("synthetic HTTP request failed: {error}"),
                    };
                    (
                        response.status(),
                        serde_json::from_reader::<_, serde_json::Value>(response.into_reader())
                            .unwrap(),
                    )
                })
                .await
                .unwrap();
                if installed {
                    assert_eq!(status, 200, "{path}: {body}");
                    assert_eq!(
                        body["repositories"],
                        serde_json::json!(["synthetic/repository"])
                    );
                    assert_eq!(body["installations"].as_array().unwrap().len(), 1);
                    assert!(body["install_url"].is_null());
                } else if configured {
                    assert_eq!(status, 200, "{path}: {body}");
                    assert_eq!(body["repositories"], serde_json::json!([]));
                    assert_eq!(body["installations"], serde_json::json!([]));
                    assert_eq!(
                        body["install_url"],
                        format!("{origin}/apps/synthetic-app/installations/new")
                    );
                } else {
                    assert_eq!(status, 503, "{path}: {body}");
                    assert!(
                        body.to_string()
                            .contains("HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS")
                    );
                    assert!(body.to_string().contains(&origin));
                }
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), 8);
        server_task.abort();
        upstream_task.abort();
    }

    #[test]
    fn hs2_fdtj98_existing_access_survives_missing_installation_metadata() {
        for access in [
            hotsheet_extsync::RepositoryAccess {
                repositories: vec!["synthetic/repository".into()],
                ..Default::default()
            },
            hotsheet_extsync::RepositoryAccess {
                installations: vec![hotsheet_extsync::AppInstallation {
                    account: "synthetic".into(),
                    selection: "selected".into(),
                    settings_url: None,
                }],
                ..Default::default()
            },
        ] {
            let expected = access.clone();
            let response = repository_access_response(access, || {
                Err("Configure HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS".into())
            })
            .unwrap();
            assert_eq!(response.repositories, expected.repositories);
            assert_eq!(response.installations, expected.installations);
            assert!(response.install_url.is_none());
        }
    }

    #[test]
    fn hs2_fdtj98_zero_installations_use_fallback_and_existing_urls_win() {
        let response = repository_access_response(Default::default(), || {
            Ok("https://github.com/apps/hot-sheet-2/installations/new".into())
        })
        .unwrap();
        assert!(response.repositories.is_empty() && response.installations.is_empty());
        assert_eq!(
            response.install_url.as_deref(),
            Some("https://github.com/apps/hot-sheet-2/installations/new")
        );
        let existing = "https://enterprise.test/apps/registered-app/installations/new";
        let response = repository_access_response(
            hotsheet_extsync::RepositoryAccess {
                install_url: Some(existing.into()),
                ..Default::default()
            },
            || panic!("existing Enterprise installation must not require slug configuration"),
        )
        .unwrap();
        assert_eq!(response.install_url.as_deref(), Some(existing));
        assert!(
            repository_access_response(Default::default(), || Err(
                "Configure HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS".into()
            ))
            .unwrap_err()
            .contains("HOTSHEET_GITHUB_ENTERPRISE_APP_SLUGS")
        );
    }
}
