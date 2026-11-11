//! `GET`/`PUT /standby-settings` (HS2-EQVZTZ): the machine-wide standby settings every client
//! reads to decide when to enter its night display (`docs/34-glance-surfaces.md` §34.5). The CLI
//! writes the same keys with `hotsheet-cli settings set standby.<name> <value>`.
//!
//! Every change is pushed as a cursored `standby_settings` change event (HS2-A5BTTK): a `PUT`
//! emits it directly, and the CLI announces its own writes through
//! `POST /standby-settings/changed`. Running clients re-read on that event and on stream resync,
//! so a change reaches them without a reload and without polling.

use crate::AppState;
use crate::api_error::ApiError;
use crate::events::ChangeEvent;
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

/// The change event kind running clients re-read the settings on.
pub(crate) const STANDBY_SETTINGS_EVENT: &str = "standby_settings";

/// Announce that the machine-wide standby settings may have changed. It carries no values:
/// clients re-read `GET /standby-settings`, which is the one source of truth.
fn emit_changed(state: &AppState) {
    state.emit(ChangeEvent {
        cursor: None,
        store: String::new(),
        kind: STANDBY_SETTINGS_EVENT.into(),
        id: String::new(),
        slug: String::new(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
        live: None,
    });
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
    let running = state.clone();
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
    emit_changed(&running);
    Ok(Json(view))
}

/// A headless `hotsheet-cli settings set standby.*` wrote the shared machine settings file:
/// push the change to this server's clients and answer the settings now in effect.
pub(crate) async fn standby_settings_changed(
    State(state): State<AppState>,
) -> Result<Json<StandbySettingsView>, ApiError> {
    let reader = state.clone();
    let view = tokio::task::spawn_blocking(move || read_settings(&reader))
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

    /// The `standby_settings` events emitted after `since`, oldest first, with their cursors.
    fn standby_events(state: &AppState, since: u64) -> Vec<u64> {
        state.event_log.with_lock(|log| {
            log.since(since)
                .0
                .into_iter()
                .filter(|event| event.kind == super::STANDBY_SETTINGS_EVENT)
                .map(|event| {
                    assert_eq!(event.store, "", "machine-wide, not a store's change");
                    event.cursor.unwrap()
                })
                .collect()
        })
    }

    #[tokio::test]
    async fn standby_settings_changes_push_a_cursored_change_event() {
        // HS2-A5BTTK: a PUT and a CLI announcement each push one cursored event; reads and a
        // rejected PUT push none.
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("store"), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store, SECRET.into())
            .unwrap()
            .with_machine_home(dir.path().join("machine-home"));
        let router = app(state.clone());
        let start = state.event_cursor();
        let (status, _) = json(&router, "GET", "/standby-settings", None).await;
        assert_eq!(status, StatusCode::OK);
        assert!(standby_events(&state, start).is_empty(), "a read is silent");

        let (status, _) = json(
            &router,
            "PUT",
            "/standby-settings",
            Some(r#"{"idle_minutes":0}"#),
        )
        .await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
        assert!(
            standby_events(&state, start).is_empty(),
            "a rejected PUT is silent"
        );

        let (status, _) = json(
            &router,
            "PUT",
            "/standby-settings",
            Some(r#"{"idle_minutes":20}"#),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let after_put = standby_events(&state, start);
        assert_eq!(after_put.len(), 1);

        // The CLI wrote the machine settings file itself, then announced it.
        Settings::with_global_home(state.store.root(), dir.path().join("machine-home"))
            .set("standby.enabled", serde_json::json!(false), Scope::Global)
            .unwrap();
        let (status, announced) = json(&router, "POST", "/standby-settings/changed", None).await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(announced["effective"]["enabled"], false);
        assert_eq!(announced["effective"]["idle_minutes"], 20);
        let all = standby_events(&state, start);
        assert_eq!(all.len(), 2);
        assert!(all[1] > after_put[0], "cursors keep increasing for replay");
    }
}
