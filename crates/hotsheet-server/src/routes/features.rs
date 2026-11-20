//! `GET`/`PUT /feature-settings` (HS2-3GJZGG): the machine-wide feature-area choices every client
//! of this host reads to decide which non-core areas to show (`docs/06-clients.md` §6.12). The CLI
//! writes the same keys with `hotsheet-cli settings set features.<id> on|off|default`.
//!
//! Every change is pushed as a cursored `feature_settings` change event: a `PUT` emits it directly,
//! and the CLI announces its own writes through `POST /feature-settings/changed`. Running clients
//! re-read on that event and on stream resync, so a change reaches them without a reload and
//! without polling.

use crate::AppState;
use crate::api_error::ApiError;
use crate::events::ChangeEvent;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use hotsheet_ticketing::features::{
    ConfiguredFeatures, FeatureSettingsView, setting_key, validate_configured, view,
};
use hotsheet_ticketing::{Scope, Settings};

fn machine_settings(state: &AppState) -> Settings {
    Settings::with_global_home(state.store.root(), state.machine_home())
}

fn settings_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
}

fn join_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

/// The change event kind running clients re-read the feature settings on.
pub(crate) const FEATURE_SETTINGS_EVENT: &str = "feature_settings";

/// Announce that the feature settings may have changed. It carries no values: clients re-read
/// `GET /feature-settings`, which is the one source of truth.
fn emit_changed(state: &AppState) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: FEATURE_SETTINGS_EVENT.into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
        live: None,
    });
}

/// Read every registered feature's stored choice; a mistyped stored value takes its default.
pub(crate) fn read_settings(settings: &Settings) -> Result<FeatureSettingsView, ApiError> {
    let mut failure = None;
    let view = view(|key| match settings.get(key, Scope::Global) {
        Ok(value) => value,
        Err(error) => {
            failure.get_or_insert(error);
            None
        }
    });
    match failure {
        Some(error) => Err(settings_error(error)),
        None => Ok(view),
    }
}

pub(crate) async fn get_feature_settings(
    State(state): State<AppState>,
) -> Result<Json<FeatureSettingsView>, ApiError> {
    let view = tokio::task::spawn_blocking(move || read_settings(&machine_settings(&state)))
        .await
        .map_err(join_error)??;
    Ok(Json(view))
}

/// Merge choices: each named feature is set (`true`/`false`) or cleared to its default (`null`);
/// features the body leaves out keep their choice. An unknown feature rejects the whole body.
pub(crate) async fn put_feature_settings(
    State(state): State<AppState>,
    Json(value): Json<ConfiguredFeatures>,
) -> Result<Json<FeatureSettingsView>, ApiError> {
    validate_configured(&value)
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    let running = state.clone();
    let view = tokio::task::spawn_blocking(move || {
        let settings = machine_settings(&state);
        for (id, choice) in &value {
            let key = setting_key(id);
            match choice {
                Some(on) => settings
                    .set(&key, serde_json::Value::Bool(*on), Scope::Global)
                    .map_err(settings_error)?,
                None => {
                    settings
                        .unset(&key, Scope::Global)
                        .map_err(settings_error)?;
                }
            }
        }
        read_settings(&settings)
    })
    .await
    .map_err(join_error)??;
    emit_changed(&running);
    Ok(Json(view))
}

/// A headless `hotsheet-cli settings set features.*` wrote the shared machine settings file: push
/// the change to this server's clients and answer the choices now in effect.
pub(crate) async fn feature_settings_changed(
    State(state): State<AppState>,
) -> Result<Json<FeatureSettingsView>, ApiError> {
    let reader = state.clone();
    let view = tokio::task::spawn_blocking(move || read_settings(&machine_settings(&reader)))
        .await
        .map_err(join_error)??;
    emit_changed(&state);
    Ok(Json(view))
}

#[cfg(test)]
mod tests {
    use crate::{AppState, app};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use hotsheet_sync::LockExt;
    use hotsheet_ticketing::{FsStore, Scope, Settings, StoreMetadata};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const SECRET: &str = "feature-secret";

    async fn json(
        router: &axum::Router,
        method: &str,
        uri: &str,
        body: Option<&str>,
    ) -> (StatusCode, serde_json::Value) {
        let request = Request::builder()
            .method(method)
            .uri(uri)
            .header("x-hotsheet-secret", SECRET)
            .header(header::CONTENT_TYPE, "application/json")
            .body(body.map_or(Body::empty(), |body| Body::from(body.to_string())))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        let status = response.status();
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
        )
    }

    fn feature_events(state: &AppState, since: u64) -> Vec<u64> {
        state.event_log.with_lock(|log| {
            log.since(since)
                .0
                .into_iter()
                .filter(|event| event.kind == super::FEATURE_SETTINGS_EVENT)
                .map(|event| {
                    assert_eq!(event.store, "", "machine-wide, not a store's change");
                    event.cursor.unwrap()
                })
                .collect()
        })
    }

    #[tokio::test]
    async fn feature_settings_merge_clear_reject_and_push_events() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store, SECRET.into())
            .unwrap()
            .with_machine_home(dir.path().join("machine-home"));
        let router = app(state.clone());
        let start = state.event_cursor();

        let (status, initial) = json(&router, "GET", "/feature-settings", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(initial["effective"]["drive"], false);
        assert_eq!(initial["effective"]["standby"], true);
        assert!(initial["configured"]["drive"].is_null());
        assert!(feature_events(&state, start).is_empty(), "a read is silent");

        let (status, saved) = json(
            &router,
            "PUT",
            "/feature-settings",
            Some(r#"{"drive":true,"standby":false}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["effective"]["drive"], true);
        assert_eq!(saved["effective"]["standby"], false);
        // A second PUT merges: the features it leaves out keep their choice.
        let (_, merged) = json(
            &router,
            "PUT",
            "/feature-settings",
            Some(r#"{"ai-chat":true}"#),
        )
        .await;
        assert_eq!(merged["effective"]["drive"], true);
        assert_eq!(merged["effective"]["ai-chat"], true);
        // Stored machine-wide, where `hotsheet-cli settings set features.drive` writes.
        let settings =
            Settings::with_global_home(state.store.root(), dir.path().join("machine-home"));
        assert_eq!(
            settings.get("features.drive", Scope::Global).unwrap(),
            Some(serde_json::json!(true))
        );

        let (status, _) = json(
            &router,
            "PUT",
            "/feature-settings",
            Some(r#"{"teleport":true}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            feature_events(&state, start).len(),
            2,
            "a rejected PUT is silent"
        );

        // Restore Defaults clears every choice.
        let (status, cleared) = json(
            &router,
            "PUT",
            "/feature-settings",
            Some(r#"{"drive":null,"standby":null,"ai-chat":null}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cleared["effective"]["drive"], false);
        assert!(cleared["configured"]["standby"].is_null());
        assert_eq!(settings.get("features.drive", Scope::Global).unwrap(), None);

        // The CLI wrote the machine settings file itself, then announced it.
        settings
            .set(
                "features.workers-view",
                serde_json::json!(false),
                Scope::Global,
            )
            .unwrap();
        let (status, announced) = json(&router, "POST", "/feature-settings/changed", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(announced["effective"]["workers-view"], false);
        let all = feature_events(&state, start);
        assert_eq!(all.len(), 4);
        assert!(all.windows(2).all(|pair| pair[1] > pair[0]));
    }
}
