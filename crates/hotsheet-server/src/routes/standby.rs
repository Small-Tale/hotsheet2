//! `GET`/`PUT /standby-settings` (HS2-EQVZTZ): the machine-wide standby settings every client
//! reads to decide when to enter its night display (`docs/34-glance-surfaces.md` §34.5). The CLI
//! writes the same keys with `hotsheet-cli settings set standby.<name> <value>`.

use crate::AppState;
use crate::api_error::ApiError;
use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use hotsheet_ticketing::standby::{
    ConfiguredStandbySettings, ENABLED_SETTING, IDLE_MINUTES_SETTING, StandbySettings,
    default_settings, effective_settings, stored_settings, validate_settings,
};
use hotsheet_ticketing::{Scope, Settings};
use serde::Serialize;

/// The machine-wide standby settings: defaults, explicit values (`null` = default), and effective.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
pub(crate) struct StandbySettingsView {
    pub(crate) defaults: StandbySettings,
    pub(crate) configured: ConfiguredStandbySettings,
    pub(crate) effective: StandbySettings,
}

fn machine_settings(state: &AppState) -> Settings {
    Settings::with_global_home(state.store.root(), state.machine_home())
}

fn settings_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
}

fn join_error(error: impl std::fmt::Display) -> ApiError {
    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
}

/// Read the stored settings; a mistyped or out-of-range stored value takes its default.
fn read_settings(state: &AppState) -> Result<StandbySettingsView, ApiError> {
    let settings = machine_settings(state);
    let enabled = settings
        .get(ENABLED_SETTING, Scope::Global)
        .map_err(settings_error)?;
    let idle = settings
        .get(IDLE_MINUTES_SETTING, Scope::Global)
        .map_err(settings_error)?;
    let configured = stored_settings(enabled.as_ref(), idle.as_ref());
    Ok(StandbySettingsView {
        defaults: default_settings(),
        configured,
        effective: effective_settings(configured),
    })
}

pub(crate) async fn get_standby_settings(
    State(state): State<AppState>,
) -> Result<Json<StandbySettingsView>, ApiError> {
    let view = tokio::task::spawn_blocking(move || read_settings(&state))
        .await
        .map_err(join_error)??;
    Ok(Json(view))
}

/// Replace both settings: a `null` or missing field clears it back to its default.
pub(crate) async fn put_standby_settings(
    State(state): State<AppState>,
    Json(value): Json<ConfiguredStandbySettings>,
) -> Result<Json<StandbySettingsView>, ApiError> {
    validate_settings(&value)
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
    let view = tokio::task::spawn_blocking(move || {
        let settings = machine_settings(&state);
        let entries = [
            (ENABLED_SETTING, value.enabled.map(serde_json::Value::from)),
            (
                IDLE_MINUTES_SETTING,
                value.idle_minutes.map(serde_json::Value::from),
            ),
        ];
        for (key, stored) in entries {
            match stored {
                Some(stored) => settings
                    .set(key, stored, Scope::Global)
                    .map_err(settings_error)?,
                None => {
                    settings.unset(key, Scope::Global).map_err(settings_error)?;
                }
            }
        }
        read_settings(&state)
    })
    .await
    .map_err(join_error)??;
    Ok(Json(view))
}

#[cfg(test)]
mod tests {
    use crate::{AppState, app};
    use axum::body::Body;
    use axum::http::{Request, StatusCode, header};
    use hotsheet_ticketing::{FsStore, Scope, Settings, StoreMetadata};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    const SECRET: &str = "standby-secret";

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

    #[tokio::test]
    async fn standby_settings_round_trip_and_reject_bad_values() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store, SECRET.into())
            .unwrap()
            .with_machine_home(dir.path().join("machine-home"));
        let router = app(state.clone());
        let (status, initial) = json(&router, "GET", "/standby-settings", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(initial["effective"]["enabled"], true);
        assert_eq!(initial["effective"]["idle_minutes"], 15);
        assert!(initial["configured"]["idle_minutes"].is_null());

        let (status, saved) = json(
            &router,
            "PUT",
            "/standby-settings",
            Some(r#"{"enabled":false,"idle_minutes":30}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(saved["effective"]["enabled"], false);
        assert_eq!(saved["configured"]["idle_minutes"], 30);
        let (_, reread) = json(&router, "GET", "/standby-settings", None).await;
        assert_eq!(reread, saved);
        // Stored machine-wide, where `hotsheet-cli settings set standby.idle_minutes` writes.
        let settings =
            Settings::with_global_home(state.store.root(), dir.path().join("machine-home"));
        assert_eq!(
            settings.get("standby.idle_minutes", Scope::Global).unwrap(),
            Some(serde_json::json!(30))
        );

        let (status, _) = json(
            &router,
            "PUT",
            "/standby-settings",
            Some(r#"{"idle_minutes":0}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        let (status, _) = json(&router, "PUT", "/standby-settings", Some(r#"{"idle":5}"#)).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);

        let (status, cleared) = json(&router, "PUT", "/standby-settings", Some("{}")).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(cleared["effective"]["enabled"], true);
        assert!(cleared["configured"]["enabled"].is_null());
        assert_eq!(
            settings.get("standby.idle_minutes", Scope::Global).unwrap(),
            None
        );
    }
}
