//! Project-scoped, authenticated guidance source and selection routes (HS2-2RD1M1).

use crate::{ApiError, AppState};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_plugins::guidance_sources as sources;
use hotsheet_plugins::guidance_updates::{self, SourceUpdate};
use hotsheet_plugins::rulesync_updates::{self, RulesyncUpdateReport};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Deserialize)]
pub(crate) struct GuidanceQuery {
    query: Option<String>,
    owner: Option<String>,
    tag: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct GuidanceOverview {
    owner: &'static str,
    explicit_selection: bool,
    default_repository: &'static str,
    sources: Vec<GuidanceSourceView>,
}

#[derive(Serialize)]
struct GuidanceSourceView {
    id: String,
    repository: String,
    requested_ref: String,
    commit: String,
    rules: Vec<GuidanceRuleView>,
    skills: Vec<GuidanceSkillView>,
}

#[derive(Serialize)]
struct GuidanceRuleView {
    name: String,
    version: u64,
    description: String,
    owner: Option<String>,
    tags: Vec<String>,
    selected: bool,
    installed_version: Option<u64>,
}

#[derive(Serialize)]
struct GuidanceSkillView {
    name: String,
    selected: bool,
}

fn project_root(state: &AppState, reference: &str) -> Result<PathBuf, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    Ok(checkout.root.into())
}

fn overview(
    project: &std::path::Path,
    query: &GuidanceQuery,
) -> Result<GuidanceOverview, ApiError> {
    let config = sources::load(project)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let owner = hotsheet_plugins::setup::guidance_owner_in(project)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let term = query.query.as_deref().unwrap_or("").to_lowercase();
    Ok(GuidanceOverview {
        owner,
        explicit_selection: config.explicit_selection,
        default_repository: sources::DEFAULT_REPOSITORY,
        sources: config
            .sources
            .into_iter()
            .map(|source| GuidanceSourceView {
                id: source.id,
                repository: source.repository,
                requested_ref: source.requested_ref,
                commit: source.commit,
                rules: source
                    .rules
                    .into_iter()
                    .filter(|rule| {
                        (term.is_empty()
                            || rule.name.to_lowercase().contains(&term)
                            || rule.description.to_lowercase().contains(&term))
                            && query
                                .owner
                                .as_deref()
                                .is_none_or(|wanted| rule.owner.as_deref() == Some(wanted))
                            && query
                                .tag
                                .as_deref()
                                .is_none_or(|wanted| rule.tags.iter().any(|tag| tag == wanted))
                    })
                    .map(|rule| GuidanceRuleView {
                        selected: source.selected_rules.contains(&rule.name),
                        installed_version: sources::installed_version(project, &rule.name),
                        name: rule.name,
                        version: rule.version,
                        description: rule.description,
                        owner: rule.owner,
                        tags: rule.tags,
                    })
                    .collect(),
                skills: source
                    .skills
                    .into_iter()
                    .map(|skill| GuidanceSkillView {
                        selected: source.selected_skills.contains(&skill.name),
                        name: skill.name,
                    })
                    .collect(),
            })
            .collect(),
    })
}

pub(crate) async fn get_checkout_guidance(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(query): Query<GuidanceQuery>,
) -> Result<Json<GuidanceOverview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || overview(&project, &query))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
}

/// Read-only, bounded remote ref checks; unlike registration this never replaces a pin.
pub(crate) async fn get_checkout_guidance_updates(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<SourceUpdate>>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        guidance_updates::check_project(&project)
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
}

/// Standalone Rulesync lock status; separate from Hot Sheet's pinned sources.
pub(crate) async fn get_checkout_rulesync_updates(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Option<RulesyncUpdateReport>>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || rulesync_updates::check_project(&project))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
        .map(Json)
}

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub(crate) enum GuidanceAction {
    Register {
        id: String,
        repository: String,
        #[serde(rename = "ref")]
        requested_ref: String,
    },
    Select {
        source: String,
        rules: Vec<String>,
        skills: Vec<String>,
    },
    Owner {
        owner: String,
    },
    Apply,
}

pub(crate) async fn update_checkout_guidance(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(action): Json<GuidanceAction>,
) -> Result<Json<GuidanceOverview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        match action {
            GuidanceAction::Register {
                id,
                repository,
                requested_ref,
            } => {
                sources::register(&project, &id, &repository, &requested_ref)
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
            }
            GuidanceAction::Select {
                source,
                rules,
                skills,
            } => {
                sources::select(&project, &source, &rules, &skills)
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
            }
            GuidanceAction::Owner { owner } => {
                hotsheet_plugins::setup::set_guidance_owner_in(&project, &owner)
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
            }
            GuidanceAction::Apply => {
                if hotsheet_plugins::setup::guidance_owner_in(&project)
                    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
                    == "rulesync"
                {
                    sources::apply_rulesync(&project)
                        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
                } else {
                    hotsheet_plugins::setup::apply_selected_guidance_in(&project)
                        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
                }
            }
        }
        overview(
            &project,
            &GuidanceQuery {
                query: None,
                owner: None,
                tag: None,
            },
        )
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
}
