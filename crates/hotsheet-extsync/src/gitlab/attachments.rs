//! GitLab issue attachments through native project uploads (HS2-ENHE8K).
//!
//! `POST /projects/:id/uploads` (multipart `file`) stores the file in the project and returns
//! a project-relative Markdown link (`/uploads/<secret>/<name>`). Hot Sheet then posts one
//! issue note that GitLab renders as that link or image and that ends with the same hidden
//! marker shape as GitHub's assets-repository comments:
//!
//! ```text
//! ![screen.png](/uploads/66dbcd21ec5d24ed6ea225176098d52b/01J…-screen.png)
//!
//! <!-- hotsheet-attachment:01J… {"filename":"screen.png","secret":"66db…","upload_name":"01J…-screen.png"} -->
//! ```
//!
//! Marked notes project as attachments (not notes); the marker makes a retried add return
//! the existing attachment. Bytes are read through
//! `GET /projects/:id/uploads/:secret/:filename` (GitLab 17.4+, any project member), so a
//! private project renders without handing the browser a token. A crop uploads a derived
//! rendition and rewrites only the marker note; the original upload is never replaced.

use hotsheet_model::{AttachmentActor, AttachmentPurpose, ImageCrop, MediaAnnotation};
use hotsheet_ticketing::store::AttachmentMarkup;
use hotsheet_ticketing::wire::ApiAttachment;
use hotsheet_ticketing::{ApiTicket, ProviderError, TicketProvider};
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::{GitLabNote, GitLabProvider, HttpResponse, validate_iid};
use crate::attachment_crop;
use crate::github::{RawBody, multipart_file_body};
use crate::github_attachments::{
    compose_marked_comment, encode_path, marker_key, parse_marked_comment, safe_filename,
    valid_attachment_id,
};

/// Attachment details carried by the hidden marker of an upload note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GitLabAttachmentMarker {
    pub filename: String,
    /// The upload's 32-character secret path segment.
    pub secret: String,
    /// The upload's stored file name (the last URL segment GitLab returned).
    pub upload_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upload_id: Option<u64>,
    /// A derived crop rendition; `secret`/`upload_name` always name the original.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendition_secret: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendition_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<ImageCrop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<MediaAnnotation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<AttachmentActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<AttachmentPurpose>,
}

impl GitLabAttachmentMarker {
    /// The project-relative link GitLab renders (`/uploads/<secret>/<name>`).
    fn link(&self) -> String {
        let (secret, name) = self.visible();
        format!("/uploads/{secret}/{}", encode_path(name))
    }

    fn visible(&self) -> (&str, &str) {
        match (&self.rendition_secret, &self.rendition_name) {
            (Some(secret), Some(name)) => (secret, name),
            _ => (&self.secret, &self.upload_name),
        }
    }

    fn valid(&self) -> bool {
        valid_upload(&self.secret, &self.upload_name)
            && match (&self.rendition_secret, &self.rendition_name) {
                (Some(secret), Some(name)) => valid_upload(secret, name),
                (None, None) => true,
                _ => false,
            }
    }
}

fn valid_upload(secret: &str, name: &str) -> bool {
    !secret.is_empty()
        && secret.len() <= 64
        && secret.chars().all(|c| c.is_ascii_alphanumeric())
        && !name.is_empty()
        && !name.contains('/')
        && name != "."
        && name != ".."
}

#[derive(Debug, Deserialize)]
struct GitLabUpload {
    #[serde(default)]
    id: Option<u64>,
    url: String,
}

/// Split GitLab's `/uploads/<secret>/<name>` link into its secret and name.
fn upload_parts(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("/uploads/")?;
    let (secret, name) = rest.split_once('/')?;
    let name = percent_decode(name);
    valid_upload(secret, &name).then(|| (secret.to_owned(), name))
}

fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'%'
            && index + 2 < bytes.len()
            && let Some(byte) = std::str::from_utf8(&bytes[index + 1..index + 3])
                .ok()
                .and_then(|hex| u8::from_str_radix(hex, 16).ok())
        {
            out.push(byte);
            index += 3;
        } else {
            out.push(bytes[index]);
            index += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Project a marked upload note as an attachment; any other note returns `None`.
pub(super) fn project(note: &GitLabNote) -> Option<ApiAttachment> {
    let (id, marker) = parse_marked_comment::<GitLabAttachmentMarker>(&note.body)?;
    if !marker.valid() {
        return None;
    }
    Some(ApiAttachment {
        id,
        filename: marker.filename,
        created_at: note.created_at.clone(),
        batch_id: marker.batch_id,
        batch_label: marker.batch_label,
        actor: marker.actor,
        purpose: marker.purpose,
        annotations: marker.annotations,
        crop: marker.crop,
        revision: Some(attachment_crop::revision(&note.body)),
    })
}

impl GitLabProvider {
    fn raw(
        &self,
        method: &str,
        url: &str,
        body: Option<RawBody<'_>>,
    ) -> Result<Vec<u8>, ProviderError> {
        let headers = [
            ("PRIVATE-TOKEN", self.config.token.clone()),
            ("User-Agent", "hotsheet2".into()),
        ];
        let response = self
            .transport
            .request_raw(method, url, &headers, body)
            .map_err(|message| ProviderError::Conflict {
                ticket: self.config.connection_id.clone(),
                message,
            })?;
        if (200..=299).contains(&response.status) {
            return Ok(response.body);
        }
        let text = HttpResponse {
            status: response.status,
            headers: response.headers,
            body: String::from_utf8_lossy(&response.body).into_owned(),
        };
        self.checked(url, text).map(|_| Vec::new())
    }

    fn marker_note(
        &self,
        native_id: &str,
        attachment_id: &str,
    ) -> Result<(GitLabNote, GitLabAttachmentMarker), ProviderError> {
        self.notes(native_id)?
            .into_iter()
            .filter(|note| !note.system)
            .find_map(|note| {
                parse_marked_comment::<GitLabAttachmentMarker>(&note.body)
                    .filter(|(id, marker)| id == attachment_id && marker.valid())
                    .map(|(_, marker)| (note, marker))
            })
            .ok_or_else(|| ProviderError::NotFound {
                connection_id: self.config.connection_id.clone(),
                native_id: attachment_id.into(),
            })
    }

    /// Upload one file to the project's Markdown uploads.
    fn upload(
        &self,
        filename: &str,
        boundary_seed: &str,
        bytes: &[u8],
    ) -> Result<(Option<u64>, String, String), ProviderError> {
        let (content_type, body) = multipart_file_body("file", filename, boundary_seed, bytes);
        let response = self.raw(
            "POST",
            &self.endpoint("uploads"),
            Some(RawBody {
                content_type: &content_type,
                bytes: &body,
            }),
        )?;
        let upload: GitLabUpload =
            serde_json::from_slice(&response).map_err(|error| ProviderError::Conflict {
                ticket: self.config.connection_id.clone(),
                message: format!("invalid GitLab upload response: {error}"),
            })?;
        let (secret, name) = upload_parts(&upload.url).ok_or_else(|| ProviderError::Conflict {
            ticket: self.config.connection_id.clone(),
            message: format!("unexpected GitLab upload URL '{}'", upload.url),
        })?;
        Ok((upload.id, secret, name))
    }

    fn download(&self, secret: &str, name: &str) -> Result<Vec<u8>, ProviderError> {
        self.raw(
            "GET",
            &self.endpoint(&format!("uploads/{secret}/{}", encode_path(name))),
            None,
        )
        .map_err(|error| match error {
            ProviderError::NotFound {
                connection_id,
                native_id,
            } => ProviderError::NotFound {
                connection_id,
                native_id: format!(
                    "{native_id} (reading uploads through the API needs GitLab 17.4 or later)"
                ),
            },
            other => other,
        })
    }

    pub(super) fn add_upload_attachment(
        &self,
        native_id: &str,
        attachment: ApiAttachment,
        bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        validate_iid(native_id)?;
        if !valid_attachment_id(&attachment.id) {
            return Err(ProviderError::InvalidNativeId {
                provider: "gitlab",
                id: attachment.id,
            });
        }
        let key = marker_key(&attachment.id);
        if self
            .notes(native_id)?
            .iter()
            .any(|note| note.body.contains(&key))
        {
            return self.get(native_id);
        }
        // The id prefix keeps the stored name unique per attachment, like GitHub's paths.
        let stored = format!("{}-{}", attachment.id, safe_filename(&attachment.filename));
        let (upload_id, secret, upload_name) = self.upload(&stored, &attachment.id, &bytes)?;
        let marker = GitLabAttachmentMarker {
            filename: attachment.filename,
            secret,
            upload_name,
            upload_id,
            rendition_secret: None,
            rendition_name: None,
            crop: None,
            annotations: vec![],
            batch_id: attachment.batch_id,
            batch_label: attachment.batch_label,
            actor: attachment.actor,
            purpose: attachment.purpose,
        };
        let body =
            compose_marked_comment(&attachment.id, &marker.filename, &marker.link(), &marker);
        self.request(
            "POST",
            &self.endpoint(&format!("issues/{native_id}/notes")),
            Some(&json!({ "body": body })),
        )?;
        self.get(native_id)
    }

    /// The visible (rendition when cropped) or original bytes of one attachment.
    pub(super) fn upload_bytes(
        &self,
        native_id: &str,
        attachment_id: &str,
        visible: bool,
    ) -> Result<Vec<u8>, ProviderError> {
        validate_iid(native_id)?;
        let (_, marker) = self.marker_note(native_id, attachment_id)?;
        let (secret, name) = if visible {
            marker.visible()
        } else {
            (marker.secret.as_str(), marker.upload_name.as_str())
        };
        self.download(secret, name)
    }

    /// Crop and annotations rewrite the marker note after a revision check. GitLab notes
    /// have no compare-and-swap, so the note is re-read right before the write and a change
    /// made during a rendition upload is reported as a conflict.
    pub(super) fn update_upload_markup(
        &self,
        native_id: &str,
        attachment_id: &str,
        expected_revision: Option<&str>,
        markup: AttachmentMarkup,
    ) -> Result<ApiTicket, ProviderError> {
        validate_iid(native_id)?;
        let ticket = format!("{}:{native_id}", self.config.connection_id);
        hotsheet_model::validate_media_annotations(&markup.annotations).map_err(|error| {
            ProviderError::Conflict {
                ticket: ticket.clone(),
                message: error.to_string(),
            }
        })?;
        let (note, mut marker) = self.marker_note(native_id, attachment_id)?;
        let current_revision = attachment_crop::revision(&note.body);
        let unchanged = marker.crop == markup.crop && marker.annotations == markup.annotations;
        if expected_revision != Some(current_revision.as_str()) {
            if unchanged {
                return self.get(native_id);
            }
            return Err(ProviderError::Conflict {
                ticket,
                message: "attachment changed since this gallery session opened".into(),
            });
        }
        if unchanged {
            return self.get(native_id);
        }
        if marker.crop != markup.crop
            || (markup.crop.is_some() && marker.rendition_secret.is_none())
        {
            match markup.crop {
                Some(crop) => {
                    let original = self.download(&marker.secret, &marker.upload_name)?;
                    let rendered =
                        attachment_crop::render(&ticket, &marker.filename, &original, crop)?;
                    marker.crop = rendered.crop;
                    match rendered.rendition {
                        Some((name, bytes)) => {
                            let stored = format!("{attachment_id}-{name}");
                            let seed = format!("{attachment_id}-{}", &name[5..25]);
                            let (_, secret, upload_name) = self.upload(&stored, &seed, &bytes)?;
                            marker.rendition_secret = Some(secret);
                            marker.rendition_name = Some(upload_name);
                        }
                        None => {
                            marker.rendition_secret = None;
                            marker.rendition_name = None;
                        }
                    }
                }
                None => {
                    marker.crop = None;
                    marker.rendition_secret = None;
                    marker.rendition_name = None;
                }
            }
        }
        marker.annotations = markup.annotations;
        let (latest, _) = self.marker_note(native_id, attachment_id)?;
        if latest.id != note.id || attachment_crop::revision(&latest.body) != current_revision {
            return Err(ProviderError::Conflict {
                ticket,
                message: "attachment changed during crop upload".into(),
            });
        }
        let body = compose_marked_comment(attachment_id, &marker.filename, &marker.link(), &marker);
        self.request(
            "PUT",
            &self.endpoint(&format!("issues/{native_id}/notes/{}", note.id)),
            Some(&json!({ "body": body })),
        )?;
        self.get(native_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{BinaryResponse, GitHubTransport};
    use crate::gitlab::GitLabConfig;
    use hotsheet_sync::LockExt;
    use serde_json::Value;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    const ATTACHMENT_ID: &str = "01K6GLAB00000000000000000A";

    /// A stateful fake of the GitLab issue, notes, and uploads endpoints with the real
    /// response shapes and status codes (201 on create, 404 for unknown uploads).
    #[derive(Default)]
    struct FakeGitLab {
        state: Mutex<FakeState>,
    }

    #[derive(Default)]
    struct FakeState {
        notes: Vec<Value>,
        uploads: HashMap<(String, String), Vec<u8>>,
        upload_count: u64,
        fail_note_post_once: bool,
        /// Edit the marker note while the next upload is in flight (a concurrent writer).
        edit_note_during_upload: bool,
        multipart_types: Vec<String>,
        tokens: Vec<String>,
    }

    fn issue() -> Value {
        json!({"iid":7,"title":"Evidence","description":"body","state":"opened","web_url":"https://gitlab.test/acme/repo/-/issues/7","created_at":"2026-10-01T00:00:00Z","updated_at":"2026-10-01T00:01:00Z","closed_at":null,"labels":[],"assignees":[]})
    }

    fn text(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    fn parse_multipart(content_type: &str, body: &[u8]) -> (String, Vec<u8>) {
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8_lossy(body);
        let filename = text
            .split("filename=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned();
        let start = body.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let end_marker = format!("\r\n--{boundary}--\r\n");
        let end = body.len() - end_marker.len();
        assert_eq!(&body[end..], end_marker.as_bytes());
        assert!(text.contains("name=\"file\""));
        (filename, body[start..end].to_vec())
    }

    impl GitHubTransport for FakeGitLab {
        fn request(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, String)],
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            let mut state = self.state.lock_or_recover();
            state.tokens.extend(
                headers
                    .iter()
                    .filter(|(name, _)| *name == "PRIVATE-TOKEN")
                    .map(|(_, value)| value.clone()),
            );
            let base = "https://gitlab.test/api/v4/projects/acme%2Frepo/";
            let path = url.strip_prefix(base).ok_or("unexpected host")?;
            if method == "GET" && path == "issues/7" {
                return Ok(text(200, issue()));
            }
            if method == "GET" && path.starts_with("issues/7/notes?") {
                return Ok(text(200, Value::Array(state.notes.clone())));
            }
            if method == "POST" && path == "issues/7/notes" {
                if state.fail_note_post_once {
                    state.fail_note_post_once = false;
                    return Ok(text(500, json!({"message":"500 Internal Server Error"})));
                }
                let id = 100 + state.notes.len() as u64;
                let note = json!({"id":id,"body":body.unwrap()["body"],"created_at":format!("2026-10-01T00:0{}:00Z", state.notes.len() % 10),"system":false});
                state.notes.push(note.clone());
                return Ok(text(201, note));
            }
            if method == "PUT"
                && let Some(id) = path.strip_prefix("issues/7/notes/")
            {
                let id: u64 = id.parse().map_err(|_| "bad note id")?;
                let Some(note) = state.notes.iter_mut().find(|note| note["id"] == id) else {
                    return Ok(text(404, json!({"message":"404 Not found"})));
                };
                note["body"] = body.unwrap()["body"].clone();
                return Ok(text(200, note.clone()));
            }
            Err(format!("unexpected request {method} {url}"))
        }

        fn request_raw(
            &self,
            method: &str,
            url: &str,
            _: &[(&str, String)],
            body: Option<RawBody<'_>>,
        ) -> Result<BinaryResponse, String> {
            let mut state = self.state.lock_or_recover();
            let base = "https://gitlab.test/api/v4/projects/acme%2Frepo/";
            let path = url.strip_prefix(base).ok_or("unexpected host")?;
            let binary = |status: u16, body: Vec<u8>| BinaryResponse {
                status,
                headers: HashMap::new(),
                body,
            };
            if method == "POST" && path == "uploads" {
                let body = body.ok_or("upload without body")?;
                state.multipart_types.push(body.content_type.into());
                let (filename, bytes) = parse_multipart(body.content_type, body.bytes);
                state.upload_count += 1;
                let secret = format!("{:032x}", state.upload_count);
                state
                    .uploads
                    .insert((secret.clone(), filename.clone()), bytes);
                if state.edit_note_during_upload {
                    state.edit_note_during_upload = false;
                    if let Some(note) = state.notes.last_mut() {
                        let edited = format!("{} ", note["body"].as_str().unwrap());
                        note["body"] = edited.into();
                    }
                }
                let url = format!("/uploads/{secret}/{filename}");
                return Ok(binary(
                    201,
                    json!({"id":state.upload_count,"alt":filename,"url":url,"full_path":format!("/-/project/1234{url}"),"markdown":format!("![{filename}]({url})")})
                        .to_string()
                        .into_bytes(),
                ));
            }
            if method == "GET"
                && let Some(rest) = path.strip_prefix("uploads/")
            {
                let (secret, name) = rest.split_once('/').ok_or("bad upload path")?;
                return Ok(
                    match state.uploads.get(&(secret.into(), percent_decode(name))) {
                        Some(bytes) => binary(200, bytes.clone()),
                        None => binary(404, br#"{"message":"404 Not found"}"#.to_vec()),
                    },
                );
            }
            Err(format!("unexpected raw request {method} {url}"))
        }
    }

    fn provider(fake: Arc<FakeGitLab>) -> GitLabProvider {
        GitLabProvider::new(
            GitLabConfig {
                connection_id: "gitlab-main".into(),
                project: "acme/repo".into(),
                api_base: "https://gitlab.test/api/v4".into(),
                token: "glpat-token".into(),
                default: false,
            },
            fake,
        )
    }

    fn evidence(id: &str, filename: &str) -> ApiAttachment {
        ApiAttachment {
            id: id.into(),
            filename: filename.into(),
            created_at: "2026-10-01T00:00:05Z".into(),
            batch_id: Some("batch-1".into()),
            batch_label: Some("Before".into()),
            actor: None,
            purpose: Some(AttachmentPurpose::ProblemEvidence),
            annotations: vec![],
            crop: None,
            revision: None,
        }
    }

    fn wire(attachments: &[ApiAttachment]) -> Value {
        serde_json::to_value(attachments).unwrap()
    }

    fn png() -> Vec<u8> {
        use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
        let image = RgbImage::from_fn(24, 20, |x, y| Rgb([x as u8, y as u8, 42]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    #[test]
    fn gitlab_reports_native_attachments_without_edit() {
        let capabilities = provider(Arc::default()).descriptor().capabilities;
        assert!(capabilities.attachments);
        assert!(!capabilities.attachment_edit);
        assert!(capabilities.attachment_crop);
    }

    #[test]
    fn upload_links_a_marked_note_projects_it_and_reads_bytes_back() {
        let fake = Arc::new(FakeGitLab::default());
        let provider = provider(fake.clone());
        fake.state.lock_or_recover().notes.push(
            json!({"id":1,"body":"An ordinary note","created_at":"2026-10-01T00:00:01Z","system":false}),
        );
        let ticket = provider
            .add_attachment(
                "7",
                evidence(ATTACHMENT_ID, "trace log.txt"),
                b"trace".to_vec(),
            )
            .unwrap();
        assert_eq!(ticket.notes.len(), 1, "the upload note is not a note");
        assert_eq!(ticket.attachments.len(), 1);
        let attachment = &ticket.attachments[0];
        assert_eq!(attachment.id, ATTACHMENT_ID);
        assert_eq!(attachment.filename, "trace log.txt");
        assert_eq!(attachment.batch_id.as_deref(), Some("batch-1"));
        assert_eq!(attachment.batch_label.as_deref(), Some("Before"));
        assert_eq!(attachment.purpose, Some(AttachmentPurpose::ProblemEvidence));
        assert!(attachment.revision.is_some());
        let state = fake.state.lock_or_recover();
        assert!(state.multipart_types[0].starts_with(&format!(
            "multipart/form-data; boundary=----hotsheet-{ATTACHMENT_ID}"
        )));
        assert!(state.tokens.iter().all(|token| token == "glpat-token"));
        let body = state.notes[1]["body"].as_str().unwrap().to_owned();
        let secret = format!("{:032x}", 1);
        assert!(
            body.starts_with(&format!(
                "[trace log.txt](/uploads/{secret}/{ATTACHMENT_ID}-trace_log.txt)\n\n<!-- hotsheet-attachment:{ATTACHMENT_ID} "
            )),
            "{body}"
        );
        drop(state);
        assert_eq!(
            provider.attachment_bytes("7", ATTACHMENT_ID).unwrap(),
            b"trace"
        );
        assert_eq!(
            provider
                .attachment_original_bytes("7", ATTACHMENT_ID)
                .unwrap(),
            b"trace"
        );
        assert!(matches!(
            provider.attachment_bytes("7", "01K6MISSING"),
            Err(ProviderError::NotFound { .. })
        ));
    }

    #[test]
    fn retries_are_idempotent_and_a_failed_note_post_retries_cleanly() {
        let fake = Arc::new(FakeGitLab::default());
        let provider = provider(fake.clone());
        fake.state.lock_or_recover().fail_note_post_once = true;
        assert!(
            provider
                .add_attachment("7", evidence(ATTACHMENT_ID, "a.png"), png())
                .is_err()
        );
        let first = provider
            .add_attachment("7", evidence(ATTACHMENT_ID, "a.png"), png())
            .unwrap();
        let again = provider
            .add_attachment("7", evidence(ATTACHMENT_ID, "a.png"), png())
            .unwrap();
        assert_eq!(wire(&first.attachments), wire(&again.attachments));
        let state = fake.state.lock_or_recover();
        // The interrupted attempt leaves one orphan upload; the marker stops a third.
        assert_eq!(state.upload_count, 2);
        assert_eq!(state.notes.len(), 1);
        assert!(
            state.notes[0]["body"]
                .as_str()
                .unwrap()
                .starts_with("![a.png](/uploads/")
        );
    }

    #[test]
    fn invalid_ids_and_forged_markers_are_rejected() {
        let fake = Arc::new(FakeGitLab::default());
        let provider = provider(fake.clone());
        assert!(matches!(
            provider.add_attachment("x", evidence(ATTACHMENT_ID, "a.txt"), vec![]),
            Err(ProviderError::InvalidNativeId { .. })
        ));
        assert!(matches!(
            provider.add_attachment("7", evidence("bad id", "a.txt"), vec![]),
            Err(ProviderError::InvalidNativeId { .. })
        ));
        // A marker naming a path outside uploads stays an ordinary note.
        fake.state.lock_or_recover().notes.push(json!({"id":1,"body":"x\n\n<!-- hotsheet-attachment:01K6X {\"filename\":\"a\",\"secret\":\"../x\",\"upload_name\":\"a\"} -->","created_at":"2026-10-01T00:00:01Z"}));
        let ticket = provider.get("7").unwrap();
        assert!(ticket.attachments.is_empty());
        assert_eq!(ticket.notes.len(), 1);
        assert_eq!(
            upload_parts("/uploads/abc/a%20b.png"),
            Some(("abc".into(), "a b.png".into()))
        );
        assert_eq!(upload_parts("/uploads/abc/x/y"), None);
        let (content_type, body) = multipart_file_body("file", "a\"b\r\n.txt", "seed", b"x");
        assert_eq!(
            content_type,
            "multipart/form-data; boundary=----hotsheet-seed"
        );
        assert!(String::from_utf8_lossy(&body).contains("filename=\"a_b__.txt\""));
        assert_eq!(upload_parts("/files/abc/x"), None);
    }

    #[test]
    fn unsupported_transports_fail_explicitly() {
        struct JsonOnly;
        impl GitHubTransport for JsonOnly {
            fn request(
                &self,
                _: &str,
                url: &str,
                _: &[(&str, String)],
                _: Option<&Value>,
            ) -> Result<HttpResponse, String> {
                Ok(text(
                    200,
                    if url.contains("/notes") {
                        json!([])
                    } else {
                        issue()
                    },
                ))
            }
        }
        let provider = GitLabProvider::new(
            GitLabConfig {
                connection_id: "gitlab-main".into(),
                project: "acme/repo".into(),
                api_base: "https://gitlab.test/api/v4".into(),
                token: "t".into(),
                default: false,
            },
            Arc::new(JsonOnly),
        );
        let error = provider
            .add_attachment("7", evidence(ATTACHMENT_ID, "a.txt"), vec![1])
            .unwrap_err();
        assert!(error.to_string().contains("binary"), "{error}");
    }

    #[test]
    fn crop_uploads_a_rendition_keeps_the_original_and_restores() {
        let original = png();
        let fake = Arc::new(FakeGitLab::default());
        let provider = provider(fake.clone());
        let first = provider
            .add_attachment("7", evidence(ATTACHMENT_ID, "shot.png"), original.clone())
            .unwrap();
        let revision = first.attachments[0].revision.clone();
        let crop = ImageCrop {
            x: 3,
            y: 2,
            width: 12,
            height: 10,
        };
        let cropped = provider
            .set_attachment_markup(
                "7",
                ATTACHMENT_ID,
                revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![],
                    crop: Some(crop),
                },
            )
            .unwrap();
        assert_eq!(cropped.attachments[0].crop, Some(crop));
        assert_ne!(cropped.attachments[0].revision, revision);
        assert_eq!(fake.state.lock_or_recover().upload_count, 2);
        let rendition = provider.attachment_bytes("7", ATTACHMENT_ID).unwrap();
        assert_eq!(image::load_from_memory(&rendition).unwrap().width(), 12);
        assert_eq!(
            provider
                .attachment_original_bytes("7", ATTACHMENT_ID)
                .unwrap(),
            original
        );
        // The note now shows the rendition; the marker still names the original upload.
        let body = fake.state.lock_or_recover().notes[0]["body"]
            .as_str()
            .unwrap()
            .to_owned();
        let (_, marker) = parse_marked_comment::<GitLabAttachmentMarker>(&body).unwrap();
        assert_eq!(marker.secret, format!("{:032x}", 1));
        assert!(body.starts_with(&format!(
            "![shot.png](/uploads/{:032x}/{ATTACHMENT_ID}-crop-",
            2
        )));
        // Annotation-only change: no new upload, rendition kept.
        let annotation = MediaAnnotation {
            id: "review".into(),
            x: 1_000,
            y: 2_000,
            width: 3_000,
            height: 2_000,
            start_ms: None,
            end_ms: None,
            text: "Look here".into(),
            shape: None,
            intents: vec![],
        };
        let annotated = provider
            .set_attachment_markup(
                "7",
                ATTACHMENT_ID,
                cropped.attachments[0].revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![annotation.clone()],
                    crop: Some(crop),
                },
            )
            .unwrap();
        assert_eq!(fake.state.lock_or_recover().upload_count, 2);
        assert_eq!(
            annotated.attachments[0].annotations,
            vec![annotation.clone()]
        );
        assert_eq!(
            provider.attachment_bytes("7", ATTACHMENT_ID).unwrap(),
            rendition
        );
        // A stale revision with a different markup conflicts; an identical one is a no-op.
        let stale = provider.set_attachment_markup(
            "7",
            ATTACHMENT_ID,
            revision.as_deref(),
            AttachmentMarkup {
                annotations: vec![],
                crop: None,
            },
        );
        assert!(matches!(stale, Err(ProviderError::Conflict { .. })));
        let same = provider
            .set_attachment_markup(
                "7",
                ATTACHMENT_ID,
                revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![annotation.clone()],
                    crop: Some(crop),
                },
            )
            .unwrap();
        assert_eq!(wire(&same.attachments), wire(&annotated.attachments));
        // Restore: the original is shown again and annotations survive.
        let restored = provider
            .set_attachment_markup(
                "7",
                ATTACHMENT_ID,
                annotated.attachments[0].revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![annotation.clone()],
                    crop: None,
                },
            )
            .unwrap();
        assert!(restored.attachments[0].crop.is_none());
        assert_eq!(restored.attachments[0].annotations, vec![annotation]);
        assert_eq!(
            provider.attachment_bytes("7", ATTACHMENT_ID).unwrap(),
            original
        );
        // Re-crop after restore uploads a fresh rendition (empty-then-refill).
        let recropped = provider
            .set_attachment_markup(
                "7",
                ATTACHMENT_ID,
                restored.attachments[0].revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![],
                    crop: Some(crop),
                },
            )
            .unwrap();
        assert_eq!(recropped.attachments[0].crop, Some(crop));
        assert_eq!(fake.state.lock_or_recover().upload_count, 3);
    }

    #[test]
    fn a_note_edited_during_the_rendition_upload_conflicts_without_overwriting() {
        let fake = Arc::new(FakeGitLab::default());
        let provider = provider(fake.clone());
        let first = provider
            .add_attachment("7", evidence(ATTACHMENT_ID, "shot.png"), png())
            .unwrap();
        fake.state.lock_or_recover().edit_note_during_upload = true;
        let result = provider.set_attachment_markup(
            "7",
            ATTACHMENT_ID,
            first.attachments[0].revision.as_deref(),
            AttachmentMarkup {
                annotations: vec![],
                crop: Some(ImageCrop {
                    x: 0,
                    y: 0,
                    width: 8,
                    height: 8,
                }),
            },
        );
        match result {
            Err(ProviderError::Conflict { message, .. }) => {
                assert!(message.contains("during crop upload"), "{message}")
            }
            other => panic!("expected conflict, got {other:?}"),
        }
        let current = provider.get("7").unwrap();
        assert!(current.attachments[0].crop.is_none());
    }

    #[test]
    fn invalid_annotations_and_unknown_uploads_fail_typed() {
        let fake = Arc::new(FakeGitLab::default());
        let provider = provider(fake.clone());
        let first = provider
            .add_attachment("7", evidence(ATTACHMENT_ID, "shot.png"), png())
            .unwrap();
        let bad = MediaAnnotation {
            id: String::new(),
            x: 0,
            y: 0,
            width: 20_000,
            height: 1,
            start_ms: None,
            end_ms: None,
            text: String::new(),
            shape: None,
            intents: vec![],
        };
        assert!(matches!(
            provider.set_attachment_markup(
                "7",
                ATTACHMENT_ID,
                first.attachments[0].revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![bad],
                    crop: None,
                },
            ),
            Err(ProviderError::Conflict { .. })
        ));
        fake.state.lock_or_recover().uploads.clear();
        match provider.attachment_bytes("7", ATTACHMENT_ID) {
            Err(ProviderError::NotFound { native_id, .. }) => {
                assert!(native_id.contains("17.4"), "{native_id}")
            }
            other => panic!("expected not found, got {other:?}"),
        }
    }

    /// HS2-ENHE8K: upload, idempotent retry, read-back, crop, and restore against a real
    /// GitLab project (creates and closes one issue). Opt-in only.
    #[test]
    #[ignore = "creates and closes a real GitLab issue; set HOTSHEET_GITLAB_LIVE_PROJECT, HOTSHEET_GITLAB_LIVE_TOKEN and HOTSHEET_GITLAB_LIVE_WRITE=1"]
    fn gitlab_live_attachment_upload_crop_and_read() {
        use hotsheet_model::{Priority, Status, Timestamp};
        use hotsheet_ticketing::{MutationContext, ProviderDraft};
        assert_eq!(
            std::env::var("HOTSHEET_GITLAB_LIVE_WRITE").as_deref(),
            Ok("1"),
            "write access to the live project must be opted into"
        );
        let provider = GitLabProvider::live(GitLabConfig {
            connection_id: "gitlab-live".into(),
            project: std::env::var("HOTSHEET_GITLAB_LIVE_PROJECT").expect("live project"),
            api_base: std::env::var("HOTSHEET_GITLAB_LIVE_API_BASE")
                .unwrap_or_else(|_| "https://gitlab.com/api/v4".into()),
            token: std::env::var("HOTSHEET_GITLAB_LIVE_TOKEN").expect("live token"),
            default: false,
        });
        let now = || Timestamp::new("2026-10-16T00:00:00Z");
        let created = provider
            .create(
                MutationContext {
                    now: now(),
                    generated_id: hotsheet_model::Ulid::new(),
                },
                ProviderDraft {
                    title: format!(
                        "Hot Sheet attachment live test {}",
                        hotsheet_model::Ulid::new()
                    ),
                    category: "test".into(),
                    priority: Priority::Default,
                    status: Status::NotStarted,
                    details:
                        "Created by an opt-in Hot Sheet provider validation; closed automatically."
                            .into(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let issue = created.native_id.clone();
        let id = hotsheet_model::Ulid::new().to_string();
        let bytes = png();
        let ticket = provider
            .add_attachment(&issue, evidence(&id, "hotsheet-live.png"), bytes.clone())
            .unwrap();
        let attachment = ticket
            .attachments
            .iter()
            .find(|attachment| attachment.id == id)
            .expect("uploaded attachment projects back")
            .clone();
        assert_eq!(provider.attachment_bytes(&issue, &id).unwrap(), bytes);
        let again = provider
            .add_attachment(&issue, evidence(&id, "hotsheet-live.png"), bytes.clone())
            .unwrap();
        assert_eq!(
            again
                .attachments
                .iter()
                .filter(|item| item.id == id)
                .count(),
            1
        );
        let crop = ImageCrop {
            x: 2,
            y: 2,
            width: 10,
            height: 8,
        };
        let cropped = provider
            .set_attachment_markup(
                &issue,
                &id,
                attachment.revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![],
                    crop: Some(crop),
                },
            )
            .unwrap();
        let rendition = provider.attachment_bytes(&issue, &id).unwrap();
        assert_eq!(image::load_from_memory(&rendition).unwrap().width(), 10);
        assert_eq!(
            provider.attachment_original_bytes(&issue, &id).unwrap(),
            bytes
        );
        let restored = provider
            .set_attachment_markup(
                &issue,
                &id,
                cropped.attachments[0].revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![],
                    crop: None,
                },
            )
            .unwrap();
        assert!(restored.attachments[0].crop.is_none());
        assert_eq!(provider.attachment_bytes(&issue, &id).unwrap(), bytes);
        provider
            .close(&issue, now(), hotsheet_model::CloseReason::Completed, None)
            .unwrap();
    }
}
