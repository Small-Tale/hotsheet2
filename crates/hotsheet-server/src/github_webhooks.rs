//! Authenticated GitHub issue invalidations (HS2-SGQ183).
//!
//! The signed body supplies only an identity. Ticket data always comes from GitHub.

use crate::routes::providers::provider_for;
use crate::{ApiError, AppState, ChangeEvent};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::ProviderError;
use hotsheet_ticketing::{ProviderConfigRegistry, ProviderConnection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

pub(crate) const MAX_WEBHOOK_BYTES: usize = 256 * 1024;
const MAX_DELIVERIES: usize = 1024;
const DELIVERY_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn secret_proof(secret: &[u8]) -> [u8; 32] {
    Sha256::digest(secret).into()
}

#[derive(Clone, Default)]
pub(crate) struct DeliveryState(Arc<Mutex<RecentDeliveries>>);

#[derive(Default)]
struct RecentDeliveries {
    seen: HashMap<String, Instant>,
    order: VecDeque<(String, Instant)>,
    /// Latest authoritative revision emitted for one qualified provider issue.
    revisions: HashMap<String, String>,
    /// Process-local proof tied to the actual HMAC key bytes, not only its name.
    verified_sources: HashMap<String, [u8; 32]>,
}

impl DeliveryState {
    fn reserve(&self, key: &str) -> bool {
        let mut recent = self.0.lock_or_recover();
        let now = Instant::now();
        while recent.order.front().is_some_and(|(oldest, recorded)| {
            recent
                .seen
                .get(oldest)
                .is_none_or(|at| at != recorded || now.duration_since(*at) >= DELIVERY_TTL)
                || recent.order.len() >= MAX_DELIVERIES
        }) {
            if let Some((oldest, recorded)) = recent.order.pop_front() {
                if recent.seen.get(&oldest) == Some(&recorded) {
                    recent.seen.remove(&oldest);
                }
            }
        }
        if recent.seen.contains_key(key) {
            return false;
        }
        recent.order.push_back((key.to_owned(), now));
        recent.seen.insert(key.to_owned(), now);
        true
    }

    fn retry(&self, key: &str) {
        self.0.lock_or_recover().seen.remove(key);
    }

    fn newer_revision(&self, issue: &str, revision: &str) -> bool {
        let mut recent = self.0.lock_or_recover();
        if recent
            .revisions
            .get(issue)
            .is_some_and(|old| old.as_str() >= revision)
        {
            return false;
        }
        // This map cannot outgrow the delivery ring under a flood of distinct issues.
        if recent.revisions.len() >= MAX_DELIVERIES {
            recent.revisions.clear();
        }
        recent
            .revisions
            .insert(issue.to_owned(), revision.to_owned());
        true
    }

    fn mark_verified(&self, source_key: &str, secret: &[u8]) {
        let mut recent = self.0.lock_or_recover();
        if recent.verified_sources.len() >= MAX_DELIVERIES {
            recent.verified_sources.clear();
        }
        recent
            .verified_sources
            .insert(source_key.to_owned(), secret_proof(secret));
    }

    fn verified(&self, source_key: &str, secret: &[u8]) -> bool {
        self.0
            .lock_or_recover()
            .verified_sources
            .get(source_key)
            .is_some_and(|proof| proof == &secret_proof(secret))
    }

    fn unverify_connection(&self, connection_id: &str) {
        let prefix = format!("{connection_id}:");
        self.0
            .lock_or_recover()
            .verified_sources
            .retain(|key, _| !key.starts_with(&prefix));
    }
}

/// A disconnected HTTP caller must not strand a reserved delivery while its
/// provider read continues on the blocking pool.
struct DeliveryReservation {
    deliveries: DeliveryState,
    key: String,
    committed: bool,
}

impl DeliveryReservation {
    fn new(deliveries: &DeliveryState, key: String) -> Self {
        Self {
            deliveries: deliveries.clone(),
            key,
            committed: false,
        }
    }

    fn commit(&mut self) {
        self.committed = true;
    }
}

impl Drop for DeliveryReservation {
    fn drop(&mut self) {
        if !self.committed {
            self.deliveries.retry(&self.key);
        }
    }
}

pub(crate) fn forget_connection(state: &AppState, connection_id: &str) {
    state.github_deliveries.unverify_connection(connection_id);
}

#[derive(Serialize)]
pub(crate) struct WebhookReceipt {
    accepted: bool,
    duplicate: bool,
    cursor: u64,
}

#[derive(Deserialize)]
pub(crate) struct WebhookSetupBody {
    /// Name of an existing machine secret, set with `hotsheet-cli key set`.
    secret_ref: String,
}

#[derive(Serialize)]
pub(crate) struct WebhookSetup {
    path: String,
    secret_ref: Option<String>,
    events: &'static [&'static str],
    content_type: &'static str,
    configured: bool,
    /// Successful authenticated delivery was observed during this server process.
    active: bool,
}

fn setup(state: &AppState, connection_id: &str) -> Result<WebhookSetup, ApiError> {
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(|error| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?
        .into_iter()
        .find(|item| item.id == connection_id && item.provider == "github")
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "GitHub source unavailable"))?;
    let secret_ref = connection
        .settings
        .pointer("/webhook/secret")
        .and_then(serde_json::Value::as_str)
        .map(str::to_owned);
    let resolved_secret = secret_ref
        .as_deref()
        .and_then(|name| state.key_registry().get(name).ok());
    let configured =
        !connection.disabled && linked_checkout(state, &connection)? && resolved_secret.is_some();
    let active = configured
        && resolved_secret.as_deref().is_some_and(|secret| {
            state.github_deliveries.verified(
                &format!(
                    "{connection_id}:{}:{}",
                    connection.locator,
                    secret_ref.as_deref().unwrap_or_default()
                ),
                secret.as_bytes(),
            )
        });
    Ok(WebhookSetup {
        path: format!("/webhooks/github/{connection_id}"),
        secret_ref,
        events: &["issues", "issue_comment"],
        content_type: "application/json",
        configured,
        active,
    })
}

fn linked_checkout(state: &AppState, connection: &ProviderConnection) -> Result<bool, ApiError> {
    state
        .checkout_registry
        .list()
        .map(|checkouts| {
            checkouts.iter().any(|checkout| {
                checkout.source(&connection.id).is_some_and(|source| {
                    source.provider == connection.provider && source.locator == connection.locator
                })
            })
        })
        .map_err(|error| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))
}

/// A capability becomes truthful only after a signed delivery from this exact
/// configured source succeeded during the current server process.
pub(crate) fn watch_ready(state: &AppState, connection: &ProviderConnection) -> bool {
    if connection.provider != "github" || connection.disabled {
        return false;
    }
    let Some(secret_ref) = connection
        .settings
        .pointer("/webhook/secret")
        .and_then(serde_json::Value::as_str)
    else {
        return false;
    };
    let source_key = format!("{}:{}:{secret_ref}", connection.id, connection.locator);
    let Ok(secret) = state.key_registry().get(secret_ref) else {
        return false;
    };
    state
        .github_deliveries
        .verified(&source_key, secret.as_bytes())
        && linked_checkout(state, connection).unwrap_or(false)
}

/// Protected, idempotent setup endpoint. It never accepts or returns the secret value.
pub(crate) async fn configure(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    Json(body): Json<WebhookSetupBody>,
) -> Result<Json<WebhookSetup>, ApiError> {
    if body.secret_ref.trim() != body.secret_ref || body.secret_ref.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid webhook key reference",
        ));
    }
    state
        .key_registry()
        .get(&body.secret_ref)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "webhook key reference unavailable"))?;
    let previous_ref = setup(&state, &connection_id)?.secret_ref;
    let registry = ProviderConfigRegistry::new(state.store.root().join("providers.json"));
    hotsheet_extsync::configure_webhook(&registry, &connection_id, &body.secret_ref).map_err(
        |error| match error {
            ProviderError::UnknownConnection(_) => {
                ApiError::new(StatusCode::NOT_FOUND, "GitHub source unavailable")
            }
            ProviderError::Conflict { message, .. } => {
                ApiError::new(StatusCode::BAD_REQUEST, message)
            }
            other => ApiError::new(StatusCode::SERVICE_UNAVAILABLE, other.to_string()),
        },
    )?;
    if previous_ref.as_deref() != Some(body.secret_ref.as_str()) {
        forget_connection(&state, &connection_id);
    }
    setup(&state, &connection_id).map(Json)
}

pub(crate) async fn setup_instructions(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
) -> Result<Json<WebhookSetup>, ApiError> {
    setup(&state, &connection_id).map(Json)
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, ApiError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, format!("missing {name}")))
}

/// `POST /webhooks/github/{connection_id}`. A public route with its own source-specific
/// authentication; it does not accept the local Hot Sheet bearer as a substitute.
pub(crate) async fn receive(
    State(state): State<AppState>,
    Path(connection_id): Path<String>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<WebhookReceipt>, ApiError> {
    let connection = ProviderConfigRegistry::new(state.store.root().join("providers.json"))
        .load()
        .map_err(|error| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, error.to_string()))?
        .into_iter()
        .find(|item| item.id == connection_id && item.provider == "github" && !item.disabled)
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "GitHub source unavailable"))?;
    let secret_ref = connection
        .settings
        .pointer("/webhook/secret")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "GitHub webhook not configured"))?;
    let verification_key = format!("{connection_id}:{}:{secret_ref}", connection.locator);
    let secret = state
        .key_registry()
        .get(secret_ref)
        .map_err(|_| ApiError::new(StatusCode::SERVICE_UNAVAILABLE, "webhook key unavailable"))?;
    let signature = header(&headers, "x-hub-signature-256")?;
    if !hotsheet_extsync::github::verify_webhook_signature(secret.as_bytes(), &body, signature) {
        return Err(ApiError::new(
            StatusCode::UNAUTHORIZED,
            "invalid webhook signature",
        ));
    }
    let event = header(&headers, "x-github-event")?;
    let Some(invalidation) = hotsheet_extsync::github::parse_webhook(event, &body)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid GitHub webhook body"))?
    else {
        return Ok(Json(WebhookReceipt {
            accepted: false,
            duplicate: false,
            cursor: state.event_cursor(),
        }));
    };
    if invalidation.repository != connection.locator {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "webhook repository does not match source",
        ));
    }
    if !linked_checkout(&state, &connection)? {
        return Err(ApiError::new(
            StatusCode::FORBIDDEN,
            "GitHub source is not linked to a checkout",
        ));
    }
    let delivery = header(&headers, "x-github-delivery")?;
    if delivery.len() > 128
        || !delivery
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid delivery id",
        ));
    }
    let delivery_key = format!("{connection_id}:{delivery}");
    if !state.github_deliveries.reserve(&delivery_key) {
        return Ok(Json(WebhookReceipt {
            accepted: false,
            duplicate: true,
            cursor: state.event_cursor(),
        }));
    }
    let mut reservation = DeliveryReservation::new(&state.github_deliveries, delivery_key.clone());
    let issue_key = format!("{connection_id}:{}", invalidation.native_id);
    // The signed issue update time lets a delayed delivery be rejected after a newer one.
    let signed_updated_at = serde_json::from_slice::<serde_json::Value>(&body)
        .ok()
        .and_then(|value| {
            value
                .pointer("/issue/updated_at")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .ok_or_else(|| ApiError::new(StatusCode::BAD_REQUEST, "missing issue update time"))?;
    let signed_at = OffsetDateTime::parse(&signed_updated_at, &Rfc3339)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid issue update time"))?;
    if signed_at > OffsetDateTime::now_utc() + time::Duration::minutes(5) {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "future issue update time",
        ));
    }
    let read_state = state.clone();
    let read_connection_id = connection_id.clone();
    let read_native_id = invalidation.native_id.clone();
    // The provider uses a synchronous HTTP transport. Keep a slow GitHub read
    // off Tokio's request workers so other event and setup routes stay live.
    let read = tokio::task::spawn_blocking(move || {
        let provider = provider_for(&read_state, &read_connection_id)?;
        Ok::<_, ApiError>(provider.get(&read_native_id))
    })
    .await;
    let authoritative = match read {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            state.github_deliveries.retry(&delivery_key);
            return Err(error);
        }
        Err(error) => {
            state.github_deliveries.retry(&delivery_key);
            return Err(ApiError::new(
                StatusCode::SERVICE_UNAVAILABLE,
                format!("GitHub re-read task failed: {error}"),
            ));
        }
    };
    let ticket = match authoritative {
        Ok(ticket) => ticket,
        Err(ProviderError::NotFound { .. })
            if event == "issues" && invalidation.action == "deleted" =>
        {
            // The authoritative 404 confirms removal. There is no issue revision to
            // compare after deletion; the signed delivery id prevents a duplicate.
            state
                .github_deliveries
                .mark_verified(&verification_key, secret.as_bytes());
            state.emit(ChangeEvent {
                cursor: None,
                store: connection_id,
                kind: "deleted".into(),
                id: invalidation.native_id,
                slug: issue_key,
                message: None,
                activity: None,
                assignment: None,
                turn: None,
            });
            reservation.commit();
            return Ok(Json(WebhookReceipt {
                accepted: true,
                duplicate: false,
                cursor: state.event_cursor(),
            }));
        }
        Err(error) => {
            state.github_deliveries.retry(&delivery_key);
            return Err(ApiError::new(
                StatusCode::BAD_GATEWAY,
                format!("GitHub re-read failed: {error}"),
            ));
        }
    };
    let authoritative_at = OffsetDateTime::parse(&ticket.updated_at, &Rfc3339).map_err(|_| {
        state.github_deliveries.retry(&delivery_key);
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "GitHub returned an invalid revision",
        )
    })?;
    if signed_at > authoritative_at {
        state.github_deliveries.retry(&delivery_key);
        return Err(ApiError::new(
            StatusCode::BAD_GATEWAY,
            "GitHub returned an older revision",
        ));
    }
    if signed_at < authoritative_at {
        state
            .github_deliveries
            .mark_verified(&verification_key, secret.as_bytes());
        reservation.commit();
        return Ok(Json(WebhookReceipt {
            accepted: false,
            duplicate: false,
            cursor: state.event_cursor(),
        }));
    }
    let fresh = state
        .github_deliveries
        .newer_revision(&issue_key, &ticket.updated_at);
    state
        .github_deliveries
        .mark_verified(&verification_key, secret.as_bytes());
    if fresh {
        state.emit(ChangeEvent {
            cursor: None,
            store: connection_id,
            kind: "changed".into(),
            id: invalidation.native_id,
            slug: ticket.slug,
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
    }
    reservation.commit();
    Ok(Json(WebhookReceipt {
        accepted: fresh,
        duplicate: !fresh,
        cursor: state.event_cursor(),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hs2_sgq183_delivery_ids_and_revisions_are_bounded_and_source_scoped() {
        let deliveries = DeliveryState::default();
        assert!(deliveries.reserve("source-a:delivery-1"));
        assert!(!deliveries.reserve("source-a:delivery-1"));
        assert!(deliveries.reserve("source-b:delivery-1"));
        assert!(deliveries.newer_revision("source-a:7", "2026-10-30T00:00:00Z"));
        assert!(!deliveries.newer_revision("source-a:7", "2026-10-29T00:00:00Z"));
        assert!(deliveries.newer_revision("source-b:7", "2026-10-29T00:00:00Z"));
        deliveries.retry("source-a:delivery-1");
        assert!(deliveries.reserve("source-a:delivery-1"));
        for index in 0..(MAX_DELIVERIES * 2) {
            assert!(deliveries.reserve(&format!("source-a:{index}")));
        }
        let recent = deliveries.0.lock_or_recover();
        assert!(recent.seen.len() <= MAX_DELIVERIES);
        assert!(recent.order.len() <= MAX_DELIVERIES);
        assert!(recent.revisions.len() <= MAX_DELIVERIES);
    }

    #[test]
    fn hs2_sgq183_rotating_key_value_revokes_watch_proof() {
        let deliveries = DeliveryState::default();
        let source = "github-synthetic:owner/repo:hook-ref";
        deliveries.mark_verified(source, b"first synthetic key");
        assert!(deliveries.verified(source, b"first synthetic key"));
        assert!(!deliveries.verified(source, b"rotated synthetic key"));
        deliveries.mark_verified(source, b"rotated synthetic key");
        assert!(deliveries.verified(source, b"rotated synthetic key"));
        assert!(!deliveries.verified(source, b"first synthetic key"));
    }

    #[test]
    fn hs2_sgq183_cancelled_read_releases_delivery_reservation() {
        let deliveries = DeliveryState::default();
        assert!(deliveries.reserve("source:delivery"));
        {
            let _pending = DeliveryReservation::new(&deliveries, "source:delivery".into());
        }
        assert!(deliveries.reserve("source:delivery"));
        let mut accepted = DeliveryReservation::new(&deliveries, "source:delivery".into());
        accepted.commit();
        drop(accepted);
        assert!(!deliveries.reserve("source:delivery"));
    }
}
