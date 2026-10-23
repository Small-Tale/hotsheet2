//! Project-scoped, authenticated guidance source and selection routes (HS2-2RD1M1).

use super::providers::provider_for;
use crate::{ApiError, AppState};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use hotsheet_plugins::guidance_interview::{self, InterviewAnswer, InterviewReview};
use hotsheet_plugins::guidance_recommendations::{self, RecommendationReport};
use hotsheet_plugins::guidance_sources as sources;
use hotsheet_plugins::guidance_updates::{self, SourceReview, SourceUpdate};
use hotsheet_plugins::rulesync_updates::{self, RulesyncUpdateReport};
use hotsheet_ticketing::FsStore;
use hotsheet_ticketing::checkouts::{Checkout, TicketSource};
use serde::{Deserialize, Serialize};
use std::path::{Path as FsPath, PathBuf};

#[derive(Deserialize)]
pub(crate) struct GuidanceQuery {
    query: Option<String>,
    owner: Option<String>,
    tag: Option<String>,
}

#[derive(Serialize)]
pub(crate) struct GuidanceOverview {
    owner: &'static str,
    pending_consolidation_review: bool,
    explicit_selection: bool,
    default_repository: &'static str,
    local_rules: Vec<String>,
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

fn selected_ticket_source(checkout: &Checkout) -> Option<TicketSource> {
    checkout
        .default_source
        .as_deref()
        .and_then(|id| checkout.source(id))
        .or_else(|| (checkout.sources.len() == 1).then(|| &checkout.sources[0]))
        .cloned()
}

fn queue_consolidation_review(
    state: &AppState,
    project: &FsPath,
    source: Option<TicketSource>,
) -> Result<(), ApiError> {
    let Some(source) = source else {
        return Ok(());
    };
    if source.provider == "git" {
        let store = FsStore::open(&source.locator)
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
        hotsheet_plugins::rulesync_consolidation::ensure_ticket(&store, project)
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    } else {
        let provider = provider_for(state, &source.connection_id)?;
        hotsheet_plugins::rulesync_consolidation::ensure_provider_ticket(
            provider.as_ref(),
            project,
        )
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    }
    hotsheet_plugins::rulesync_consolidation::mark_reviewed(project, state.machine_home())
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
}

pub(crate) fn retry_pending_review(state: &AppState, checkout: &Checkout) -> Result<(), ApiError> {
    let project = FsPath::new(&checkout.root);
    if hotsheet_plugins::setup::guidance_owner_in(project)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
        == "rulesync"
        && hotsheet_plugins::rulesync_consolidation::needs_review(project, state.machine_home())
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
    {
        queue_consolidation_review(state, project, selected_ticket_source(checkout))?;
    }
    Ok(())
}

fn overview(
    project: &std::path::Path,
    query: &GuidanceQuery,
    home: &FsPath,
) -> Result<GuidanceOverview, ApiError> {
    let config = sources::load(project)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let owner = hotsheet_plugins::setup::guidance_owner_in(project)
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
    let term = query.query.as_deref().unwrap_or("").to_lowercase();
    Ok(GuidanceOverview {
        owner,
        pending_consolidation_review: owner == "rulesync"
            && hotsheet_plugins::rulesync_consolidation::needs_review(project, home)
                .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?,
        explicit_selection: config.explicit_selection,
        default_repository: sources::DEFAULT_REPOSITORY,
        local_rules: if owner == "rulesync" {
            hotsheet_plugins::setup::local_rules_in(project)
                .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
        } else {
            Vec::new()
        },
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
    tokio::task::spawn_blocking(move || overview(&project, &query, state.machine_home()))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map(Json)
}

/// Preview one pinned rule against the local installed instruction section without writing.
pub(crate) async fn get_checkout_guidance_rule_preview(
    State(state): State<AppState>,
    Path((reference, source, rule)): Path<(String, String, String)>,
) -> Result<Json<hotsheet_plugins::setup::GuidanceRulePreview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        hotsheet_plugins::setup::preview_guidance_rule_in(&project, &source, &rule)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
    .map(Json)
}

pub(crate) async fn get_checkout_guidance_recommendations(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<RecommendationReport>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || guidance_recommendations::scan(&project))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
        .map(Json)
}

pub(crate) async fn get_checkout_guidance_interview_preview(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<InterviewReview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || guidance_interview::review(&project))
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
        .map(Json)
}

#[derive(Deserialize)]
pub(crate) struct InterviewOpenRequest {
    fingerprint: String,
    consent: bool,
}

pub(crate) async fn open_checkout_guidance_interview(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(request): Json<InterviewOpenRequest>,
) -> Result<Json<InterviewReview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        guidance_interview::open(&project, &request.fingerprint, request.consent)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
    .map(Json)
}

#[derive(Deserialize)]
pub(crate) struct InterviewSubmitRequest {
    fingerprint: String,
    answer: InterviewAnswer,
}

pub(crate) async fn submit_checkout_guidance_interview(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(request): Json<InterviewSubmitRequest>,
) -> Result<Json<InterviewReview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        guidance_interview::submit(&project, &request.fingerprint, request.answer)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
    .map(Json)
}

#[derive(Deserialize)]
pub(crate) struct RecommendationDecision {
    source: String,
    kind: String,
    name: String,
    fingerprint: String,
    decision: String,
}

pub(crate) async fn decide_checkout_guidance_recommendation(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(request): Json<RecommendationDecision>,
) -> Result<Json<RecommendationReport>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        guidance_recommendations::decide(
            &project,
            &request.source,
            &request.kind,
            &request.name,
            &request.fingerprint,
            &request.decision,
        )
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))
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

/// Resolve and compare one selected source candidate without changing the project.
pub(crate) async fn get_checkout_guidance_review(
    State(state): State<AppState>,
    Path((reference, source)): Path<(String, String)>,
) -> Result<Json<SourceReview>, ApiError> {
    let project = project_root(&state, &reference)?;
    tokio::task::spawn_blocking(move || {
        guidance_updates::review_project_source(&project, &source)
            .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))
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
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let project = PathBuf::from(&checkout.root);
    let source = selected_ticket_source(&checkout);
    tokio::task::spawn_blocking(move || {
        if hotsheet_plugins::setup::guidance_owner_in(&project)
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
            == "rulesync"
            && hotsheet_plugins::rulesync_consolidation::needs_review(
                &project,
                state.machine_home(),
            )
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
        {
            queue_consolidation_review(&state, &project, source)?;
        }
        Ok::<_, ApiError>(rulesync_updates::check_project(&project))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
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
        expected_commit: Option<String>,
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
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|error| ApiError::new(StatusCode::NOT_FOUND, error.to_string()))?;
    let project = PathBuf::from(&checkout.root);
    let ticket_source = selected_ticket_source(&checkout);
    tokio::task::spawn_blocking(move || {
        let mut review_needed = false;
        match action {
            GuidanceAction::Register {
                id,
                repository,
                requested_ref,
                expected_commit,
            } => {
                sources::register_checked(
                    &project,
                    &id,
                    &repository,
                    &requested_ref,
                    expected_commit.as_deref(),
                )
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
                let before = hotsheet_plugins::setup::guidance_owner_in(&project)
                    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
                hotsheet_plugins::setup::set_guidance_owner_in(&project, &owner)
                    .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, error.to_string()))?;
                review_needed = before != "rulesync" && owner == "rulesync";
            }
            GuidanceAction::Apply => {
                if hotsheet_plugins::setup::guidance_owner_in(&project)
                    .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?
                    == "rulesync"
                {
                    let before = std::fs::read(project.join("AGENTS.md")).ok();
                    sources::apply_rulesync(&project)
                        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
                    review_needed = std::fs::read(project.join("AGENTS.md")).ok() != before;
                } else {
                    hotsheet_plugins::setup::apply_selected_guidance_in(&project)
                        .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
                }
            }
        }
        if review_needed {
            queue_consolidation_review(&state, &project, ticket_source)?;
        }
        overview(
            &project,
            &GuidanceQuery {
                query: None,
                owner: None,
                tag: None,
            },
            state.machine_home(),
        )
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
    .map(Json)
}
