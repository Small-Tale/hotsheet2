//! Checkout ticket attachment routes: upload, read, thumbnails, host actions, metadata, and markup (HS2-YMG48C).

use crate::api_error::{ActorReq, ApiError, parse_actor};
use crate::events::ChangeEvent;
use crate::multistore::StoreEntry;
use crate::routes::checkout_tickets::checkout_ticket_owner;
use crate::routes::checkouts::repository_browser_api_error;
use crate::routes::project_tools::checkout_settings;
use crate::routes::providers::{provider_for, provider_transfer_error};
use crate::routes::store_scoped::ResolvedTicket;
use crate::routes::ticket_writes::{
    attachment_filename, attachment_metadata, validate_attachment_metadata,
};
use crate::routes::tickets::{api_ticket, api_ticket_with_settings, contextualize_api_ticket};
use crate::{AppState, image_crop_cache, media, multistore, now, repository_browser};
use axum::Json;
use axum::body::Bytes;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::Response;
use hotsheet_model::{Ticket, Ulid};
use hotsheet_ticketing::ApiTicket;
use hotsheet_ticketing::wire::ApiAttachment;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub(crate) async fn add_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<ResolvedTicket>), ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        // An external provider stores the file natively when its capability allows it
        // (GitHub through its assets repository, HS2-HSA64D); otherwise it refuses explicitly.
        let filename = attachment_filename(&headers)?;
        let metadata = attachment_metadata(&headers)?;
        let ticket = provider_for(&state, &source.connection_id)?
            .add_attachment(
                &native_id,
                ApiAttachment {
                    id: Ulid::new().to_string(),
                    filename,
                    created_at: now().as_str().to_string(),
                    batch_id: metadata.batch_id,
                    batch_label: metadata.batch_label,
                    actor: metadata.actor,
                    purpose: metadata.purpose,
                    annotations: vec![],
                    crop: None,
                    revision: None,
                },
                body.to_vec(),
            )
            .map_err(provider_transfer_error)?;
        return Ok((
            StatusCode::CREATED,
            Json(ResolvedTicket {
                store: source.connection_id,
                ticket: contextualize_api_ticket(ticket, &settings)?,
            }),
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let filename = attachment_filename(&headers)?;
    let metadata = attachment_metadata(&headers)?;
    let (updated, _) = entry.store.write_attachment_with_metadata(
        &ticket.id,
        Ulid::new(),
        now(),
        &filename,
        &body,
        metadata,
    )?;
    state.changed_in(&entry, "attachment_added", &updated);
    Ok((
        StatusCode::CREATED,
        Json(ResolvedTicket {
            store: multistore::store_url_id(&entry.store),
            ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
        }),
    ))
}

/// Resolve a ticket reference — a qualified `{connection}:{native}` id, a bare ULID, or a slug —
/// to the hosted git store entry that owns it plus the ticket itself. Every checkout-scoped
/// handler that needs the store goes through here, so a qualified id the client routes by
/// (HS2-HX0VM9) resolves exactly like a bare one (HS2-QS9EQD).
pub(crate) fn checkout_git_ticket(
    state: &AppState,
    reference: &str,
    id: &str,
) -> Result<(StoreEntry, Ticket), ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider != "git" {
        // Store-only operations (attachment edits, thumbnails, local file actions, note
        // deletion) name the provider instead of reporting a missing git store (HS2-HSA64D).
        return Err(ApiError::new(
            StatusCode::CONFLICT,
            format!(
                "provider connection '{}' ({}) does not support this operation",
                source.connection_id, source.provider
            ),
        ));
    }
    let entry = state.hosted_source(&source).ok_or_else(|| {
        ApiError::new(
            StatusCode::CONFLICT,
            "checkout links an unhosted git source",
        )
    })?;
    let ticket = entry
        .resolve(&native_id)?
        .ok_or_else(|| ApiError::not_found(id))?;
    Ok((entry, ticket))
}

/// Serve an external provider's attachment through the provider (HS2-HSA64D), so the
/// browser never needs the provider's credentials. Returns `None` for a git-owned ticket.
fn provider_attachment_response(
    state: &AppState,
    reference: &str,
    id: &str,
    matches: impl Fn(&ApiAttachment) -> bool,
    range: Option<&str>,
    original: bool,
) -> Result<Option<Response>, ApiError> {
    let (source, native_id) = checkout_ticket_owner(state, reference, id)?;
    if source.provider == "git" {
        return Ok(None);
    }
    let provider = provider_for(state, &source.connection_id)?;
    let ticket = provider.get(&native_id).map_err(provider_transfer_error)?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| matches(attachment))
        .ok_or_else(|| ApiError::not_found(id))?;
    let bytes = if original {
        provider.attachment_original_bytes(&native_id, &attachment.id)
    } else {
        provider.attachment_bytes(&native_id, &attachment.id)
    }
    .map_err(provider_transfer_error)?;
    Ok(Some(media::attachment_response(
        &attachment.filename,
        bytes,
        range,
    )))
}

pub(crate) async fn get_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.id == attachment_id,
        headers.get("range").and_then(|value| value.to_str().ok()),
        false,
    )? {
        return Ok(response);
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    git_attachment_response(
        state.cache_dir(),
        &attachment.filename,
        &path,
        attachment.crop,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
}

pub(crate) async fn get_checkout_ticket_attachment_original(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.id == attachment_id,
        headers.get("range").and_then(|value| value.to_str().ok()),
        true,
    )? {
        return Ok(response);
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    git_attachment_response(
        state.cache_dir(),
        &attachment.filename,
        &path,
        None,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
}

async fn git_attachment_response(
    cache_root: &std::path::Path,
    filename: &str,
    path: &std::path::Path,
    crop: Option<hotsheet_model::ImageCrop>,
    range: Option<&str>,
) -> Result<Response, ApiError> {
    if let Some(crop) = crop {
        let bytes = tokio::fs::read(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ApiError::not_found(filename)
            } else {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            }
        })?;
        let filename_owned = filename.to_string();
        let cache_root = cache_root.to_path_buf();
        let rendition = tokio::task::spawn_blocking(move || {
            use sha2::{Digest, Sha256};
            let mut digest = Sha256::new();
            digest.update(&bytes);
            digest.update(filename_owned.as_bytes());
            digest.update(crop.x.to_be_bytes());
            digest.update(crop.y.to_be_bytes());
            digest.update(crop.width.to_be_bytes());
            digest.update(crop.height.to_be_bytes());
            let key = format!("{:x}", digest.finalize());
            if let Some(cached) = image_crop_cache::read(&cache_root, &key) {
                return Ok(cached);
            }
            let result =
                hotsheet_ticketing::image_crop::cropped_rendition(&filename_owned, &bytes, crop)?;
            image_crop_cache::store(&cache_root, &key, &result);
            Ok::<_, hotsheet_ticketing::image_crop::ImageCropError>(result)
        })
        .await
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?
        .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?;
        return Ok(media::attachment_response(filename, rendition, range));
    }
    media::attachment_file_response(filename, path, range)
        .await
        .map_err(|error| {
            if matches!(&error, media::MediaError::Io(error) if error.kind() == std::io::ErrorKind::NotFound) {
                ApiError::not_found(filename)
            } else {
                ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
            }
        })
}

pub(crate) async fn get_checkout_ticket_attachment_by_name(
    State(state): State<AppState>,
    Path((reference, id, filename)): Path<(String, String, String)>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    if let Some(response) = provider_attachment_response(
        &state,
        &reference,
        &id,
        |attachment| attachment.filename == filename,
        headers.get("range").and_then(|value| value.to_str().ok()),
        false,
    )? {
        return Ok(response);
    }
    let (entry, ticket, attachment_id) =
        checkout_attachment_by_name(&state, &reference, &id, &filename)?;
    let attachment = ticket
        .attachments
        .iter()
        .find(|attachment| attachment.id == attachment_id)
        .ok_or_else(|| ApiError::not_found(&attachment_id.to_string()))?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    git_attachment_response(
        state.cache_dir(),
        &attachment.filename,
        &path,
        attachment.crop,
        headers.get("range").and_then(|value| value.to_str().ok()),
    )
    .await
}

pub(crate) async fn get_checkout_ticket_attachment_thumbnail(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Response, ApiError> {
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let filename = attachment.filename;
    let thumbnail = {
        let cache_dir = Arc::clone(&state.cache_dir);
        tokio::task::spawn_blocking(move || {
            media::optional_video_poster(&cache_dir, &filename, &bytes)
        })
    }
    .await
    .map_err(|error| {
        ApiError::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("video thumbnail task failed: {error}"),
        )
    })?
    .map_err(|error| ApiError::new(StatusCode::UNPROCESSABLE_ENTITY, error.to_string()))?
    .ok_or_else(|| ApiError::new(StatusCode::NOT_FOUND, "video poster has not been generated"))?;
    let mut response = media::attachment_response("thumbnail.jpg", thumbnail, None);
    response.headers_mut().insert(
        "cache-control",
        axum::http::HeaderValue::from_static("public, max-age=31536000, immutable"),
    );
    Ok(response)
}

pub(crate) async fn put_checkout_ticket_attachment_thumbnail(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, ApiError> {
    if headers
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .map(|value| value.split(';').next().unwrap_or_default().trim())
        != Some("image/jpeg")
    {
        return Err(ApiError::new(
            StatusCode::UNSUPPORTED_MEDIA_TYPE,
            "video posters must be JPEG images",
        ));
    }
    if body.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "video poster cannot be empty",
        ));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, bytes) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    if !media::is_video(&attachment.filename) {
        return Err(ApiError::new(
            StatusCode::UNPROCESSABLE_ENTITY,
            "attachment is not a supported video",
        ));
    }
    let cache_dir = Arc::clone(&state.cache_dir);
    tokio::task::spawn_blocking(move || media::cache_video_poster(&cache_dir, &bytes, &body))
        .await
        .map_err(|error| {
            ApiError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("video poster task failed: {error}"),
            )
        })?
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct AttachmentHostActionRequest {
    action: AttachmentHostAction,
}

#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum AttachmentHostAction {
    Open,
    Reveal,
    Path,
}

#[derive(Debug, Serialize)]
pub(crate) struct AttachmentHostActionResponse {
    path: String,
}

fn checkout_attachment_by_name(
    state: &AppState,
    reference: &str,
    id: &str,
    filename: &str,
) -> Result<(StoreEntry, Ticket, Ulid), ApiError> {
    let (entry, ticket) = checkout_git_ticket(state, reference, id)?;
    let attachment_id = ticket
        .attachments
        .iter()
        .rev()
        .filter(|attachment| filename.starts_with(&attachment.filename))
        .fold(
            None::<&hotsheet_model::Attachment>,
            |best, attachment| match best {
                Some(current) if current.filename.len() >= attachment.filename.len() => {
                    Some(current)
                }
                _ => Some(attachment),
            },
        )
        .map(|attachment| attachment.id)
        .ok_or_else(|| ApiError::not_found(filename))?;
    Ok((entry, ticket, attachment_id))
}

fn attachment_disk_path(
    entry: &StoreEntry,
    ticket_id: &Ulid,
    attachment_id: &Ulid,
    filename: &str,
) -> std::path::PathBuf {
    let nested = entry
        .store
        .attachment_dir(ticket_id)
        .join(attachment_id.to_string())
        .join(filename);
    if nested.is_file() {
        nested
    } else {
        entry.store.attachment_dir(ticket_id).join(filename)
    }
}

pub(crate) async fn act_on_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(request): Json<AttachmentHostActionRequest>,
) -> Result<Json<AttachmentHostActionResponse>, ApiError> {
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let (attachment, _) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    if request.action != AttachmentHostAction::Path {
        repository_browser::act_on_host_path(
            &path,
            match request.action {
                AttachmentHostAction::Open => repository_browser::RepositoryFileAction::Open,
                AttachmentHostAction::Reveal => repository_browser::RepositoryFileAction::Reveal,
                AttachmentHostAction::Path => unreachable!(),
            },
        )
        .map_err(repository_browser_api_error)?;
    }
    Ok(Json(AttachmentHostActionResponse {
        path: path.display().to_string(),
    }))
}

pub(crate) async fn act_on_checkout_ticket_attachment_by_name(
    State(state): State<AppState>,
    Path((reference, id, filename)): Path<(String, String, String)>,
    Json(request): Json<AttachmentHostActionRequest>,
) -> Result<Json<AttachmentHostActionResponse>, ApiError> {
    let (entry, ticket, attachment_id) =
        checkout_attachment_by_name(&state, &reference, &id, &filename)?;
    let (attachment, _) = entry.store.read_attachment(&ticket.id, &attachment_id)?;
    let path = attachment_disk_path(&entry, &ticket.id, &attachment_id, &attachment.filename);
    if request.action != AttachmentHostAction::Path {
        repository_browser::act_on_host_path(
            &path,
            match request.action {
                AttachmentHostAction::Open => repository_browser::RepositoryFileAction::Open,
                AttachmentHostAction::Reveal => repository_browser::RepositoryFileAction::Reveal,
                AttachmentHostAction::Path => unreachable!(),
            },
        )
        .map_err(repository_browser_api_error)?;
    }
    Ok(Json(AttachmentHostActionResponse {
        path: path.display().to_string(),
    }))
}

pub(crate) async fn delete_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let updated = entry
        .store
        .remove_attachment(&ticket.id, &attachment_id, now())
        .map_err(|error| {
            if error.is_io_kind(std::io::ErrorKind::NotFound) {
                ApiError::not_found(&attachment_id.to_string())
            } else {
                error.into()
            }
        })?;
    state.changed_in(&entry, "attachment_removed", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAttachmentMetadataBody {
    attachment_ids: Vec<String>,
    #[serde(flatten)]
    metadata: hotsheet_model::AttachmentMetadata,
}

pub(crate) async fn update_checkout_ticket_attachment_metadata(
    State(state): State<AppState>,
    Path((reference, id)): Path<(String, String)>,
    Json(body): Json<UpdateAttachmentMetadataBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    if body.attachment_ids.is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "attachment_ids must not be empty",
        ));
    }
    validate_attachment_metadata(&body.metadata)?;
    let mut seen = std::collections::HashSet::new();
    let attachment_ids = body
        .attachment_ids
        .iter()
        .map(|id| {
            let parsed = Ulid::from_string(id)
                .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
            if !seen.insert(parsed) {
                return Err(ApiError::new(
                    StatusCode::BAD_REQUEST,
                    "attachment_ids must be unique",
                ));
            }
            Ok(parsed)
        })
        .collect::<Result<Vec<_>, ApiError>>()?;
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let updated =
        entry
            .store
            .set_attachment_metadata(&ticket.id, &attachment_ids, body.metadata, now())?;
    if updated.attachments != ticket.attachments {
        state.changed_in(&entry, "attachment_metadata_updated", &updated);
    }
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct RenameAttachmentBody {
    filename: String,
}

pub(crate) async fn rename_checkout_ticket_attachment(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<RenameAttachmentBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    if body.filename.trim().is_empty() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "filename is required",
        ));
    }
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id = Ulid::from_string(&attachment_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
    let updated =
        entry
            .store
            .rename_attachment(&ticket.id, &attachment_id, now(), &body.filename)?;
    if updated.attachments != ticket.attachments {
        state.changed_in(&entry, "attachment_renamed", &updated);
    }
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAttachmentAnnotationsBody {
    annotations: Vec<hotsheet_model::MediaAnnotation>,
    #[serde(default)]
    actor: Option<ActorReq>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct UpdateAttachmentMarkupBody {
    annotations: Vec<hotsheet_model::MediaAnnotation>,
    crop: Option<hotsheet_model::ImageCrop>,
    #[serde(default)]
    expected_revision: Option<String>,
    #[serde(default)]
    actor: Option<ActorReq>,
}

pub(crate) async fn update_checkout_ticket_attachment_markup(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateAttachmentMarkupBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (source, native_id) = checkout_ticket_owner(&state, &reference, &id)?;
    if source.provider != "git" {
        let provider = provider_for(&state, &source.connection_id)?;
        let updated = provider
            .set_attachment_markup(
                &native_id,
                &attachment_id,
                body.expected_revision.as_deref(),
                hotsheet_ticketing::store::AttachmentMarkup {
                    annotations: body.annotations,
                    crop: body.crop,
                },
            )
            .map_err(provider_transfer_error)?;
        state.emit(ChangeEvent {
            cursor: None,
            store: source.connection_id.clone(),
            kind: "attachment_markup_updated".into(),
            id: native_id.clone(),
            slug: updated.slug.clone(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        });
        return Ok(Json(ResolvedTicket {
            store: source.connection_id,
            ticket: contextualize_api_ticket(updated, &settings)?,
        }));
    }
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id = Ulid::from_string(&attachment_id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, "invalid attachment ULID"))?;
    let actor = parse_actor(body.actor.as_ref())?;
    let updated = entry.store.set_attachment_markup_with_activity(
        &ticket.id,
        &attachment_id,
        hotsheet_ticketing::store::AttachmentMarkup {
            annotations: body.annotations,
            crop: body.crop,
        },
        hotsheet_ticketing::actor::note_actor(actor.as_ref()),
        Ulid::new(),
        now(),
    )?;
    state.changed_in(&entry, "attachment_markup_updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}

pub(crate) async fn update_ticket_attachment_annotations(
    State(state): State<AppState>,
    Path((id, attachment_id)): Path<(String, String)>,
    Json(body): Json<UpdateAttachmentAnnotationsBody>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = state
        .default_entry()
        .resolve(&id)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let actor = parse_actor(body.actor.as_ref())?;
    let updated = state.store.set_attachment_annotations_with_activity(
        &ticket.id,
        &attachment_id,
        body.annotations,
        hotsheet_ticketing::actor::note_actor(actor.as_ref()),
        Ulid::new(),
        now(),
    )?;
    state.changed_in(
        &state.default_entry(),
        "attachment_annotations_updated",
        &updated,
    );
    Ok(Json(api_ticket(&state.default_entry(), &updated)?))
}

pub(crate) async fn update_checkout_ticket_attachment_annotations(
    State(state): State<AppState>,
    Path((reference, id, attachment_id)): Path<(String, String, String)>,
    Json(body): Json<UpdateAttachmentAnnotationsBody>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let (entry, ticket) = checkout_git_ticket(&state, &reference, &id)?;
    let attachment_id =
        Ulid::from_string(&attachment_id).map_err(|_| ApiError::not_found(&attachment_id))?;
    let actor = parse_actor(body.actor.as_ref())?;
    let updated = entry
        .store
        .set_attachment_annotations_with_activity(
            &ticket.id,
            &attachment_id,
            body.annotations,
            hotsheet_ticketing::actor::note_actor(actor.as_ref()),
            Ulid::new(),
            now(),
        )
        .map_err(ApiError::from)?;
    state.changed_in(&entry, "attachment_annotations_updated", &updated);
    Ok(Json(ResolvedTicket {
        store: multistore::store_url_id(&entry.store),
        ticket: api_ticket_with_settings(&entry, &updated, &settings)?,
    }))
}
