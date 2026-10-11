//! Hosted-store listing, GitHub device sign-in, and account routes.

use crate::*;

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
    pub(crate) status: tokio::sync::watch::Sender<GitHubAuthStatus>,
    pub(crate) client_id: String,
    pub(crate) web_base: String,
    pub(crate) credential_reference: String,
}

#[derive(Debug, Deserialize)]
pub(crate) struct StartGitHubAuthBody {
    #[serde(default = "default_github_web_base")]
    pub(crate) web_base: String,
}

pub(crate) fn default_github_web_base() -> String {
    "https://github.com".into()
}

#[derive(Debug, Serialize)]
pub(crate) struct StartGitHubAuthResponse {
    pub(crate) session_id: String,
    pub(crate) user_code: String,
    pub(crate) verification_uri: String,
    pub(crate) expires_in: u64,
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
    pub(crate) repositories: Vec<String>,
    /// What each app installation grants and where to change it (HS2-27T5WT).
    pub(crate) installations: Vec<hotsheet_extsync::AppInstallation>,
    pub(crate) install_url: Option<String>,
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

pub(crate) async fn github_repository_access(
    client_id: String,
    web_base: String,
    access_token: String,
) -> Result<Json<GitHubRepositoriesResponse>, ApiError> {
    let client = hotsheet_extsync::GitHubDeviceClient::live(client_id, web_base);
    let access = tokio::task::spawn_blocking(move || client.repository_access(&access_token))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::BAD_GATEWAY, error.to_string()))?;
    Ok(Json(GitHubRepositoriesResponse {
        repositories: access.repositories,
        installations: access.installations,
        install_url: access.install_url,
    }))
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
        let mut connections = providers.load().map_err(provider_transfer_error)?;
        let checkouts = state
            .checkout_registry
            .list()
            .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
        hotsheet_ticketing::accounts::take_unused_source(
            &mut connections,
            &checkouts,
            &account,
            &connection_id,
        )
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
        providers
            .save(&connections)
            .map_err(provider_transfer_error)?;
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
