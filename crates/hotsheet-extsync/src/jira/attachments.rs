//! Jira issue attachments through the first-class attachment API (HS2-KE18XX).
//!
//! Jira stores files natively: `POST /rest/api/3/issue/{key}/attachments` (multipart `file`,
//! `X-Atlassian-Token: no-check`) adds them, the issue's `attachment` field lists them, and
//! `GET /rest/api/3/attachment/content/{id}?redirect=false` reads them. Each Jira attachment
//! projects as one Hot Sheet attachment whose id is the Jira attachment id, so files people
//! add in Jira appear too.
//!
//! Hot Sheet metadata that Jira has no field for (the Hot Sheet attachment id used for
//! idempotent retries, batch, actor, purpose, crop, annotations, and the crop rendition) is
//! kept in one issue property per attachment, `hotsheet.attachment.<jira id>`. One property
//! per attachment keeps each record far below Jira's 32 KB property limit and keeps writes
//! to different attachments from overwriting each other. A crop uploads a derived
//! rendition named `hotsheet-<jira id>-crop-<hash>.<ext>`; renditions never project as
//! attachments of their own, and a replaced rendition is deleted after the property points
//! away from it. The original upload is never changed.

use std::collections::{BTreeMap, HashSet};

use hotsheet_model::{AttachmentActor, AttachmentPurpose, ImageCrop, MediaAnnotation};
use hotsheet_ticketing::store::AttachmentMarkup;
use hotsheet_ticketing::wire::ApiAttachment;
use hotsheet_ticketing::{ApiTicket, ProviderError, TicketProvider};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{HttpResponse, JiraProvider, base64, validate_key};
use crate::attachment_crop;
use crate::github::{RawBody, multipart_file_body};
use crate::github_attachments::valid_attachment_id;

/// Issue property key prefix for per-attachment Hot Sheet metadata.
pub const PROPERTY_PREFIX: &str = "hotsheet.attachment.";

/// One entry of an issue's `attachment` field.
#[derive(Debug, Clone, Deserialize)]
pub(super) struct JiraAttachment {
    pub id: String,
    pub filename: String,
    pub created: String,
}

/// Hot Sheet metadata for one Jira attachment, stored as an issue property value.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct JiraAttachmentRecord {
    /// The Hot Sheet attachment id that created this file; makes a retried add idempotent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hotsheet_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<AttachmentActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<AttachmentPurpose>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub crop: Option<ImageCrop>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub annotations: Vec<MediaAnnotation>,
    /// The Jira attachment id of the current crop rendition.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rendition_id: Option<String>,
}

#[derive(Debug, Deserialize)]
struct IssueAttachmentsAndProperties {
    fields: AttachmentFields,
    #[serde(default)]
    properties: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct AttachmentFields {
    #[serde(default)]
    attachment: Vec<JiraAttachment>,
}

/// A rendition this provider uploaded: `hotsheet-<digits>-crop-<hash>.<ext>`.
pub(super) fn is_rendition_name(filename: &str) -> bool {
    filename
        .strip_prefix("hotsheet-")
        .and_then(|rest| rest.split_once("-crop-"))
        .is_some_and(|(id, _)| !id.is_empty() && id.chars().all(|c| c.is_ascii_digit()))
}

fn valid_jira_attachment_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 32 && id.chars().all(|c| c.is_ascii_digit())
}

/// The optimistic-concurrency revision of one attachment's metadata.
fn record_revision(id: &str, record: Option<&JiraAttachmentRecord>) -> String {
    let body = match record {
        Some(record) => serde_json::to_string(record).expect("record serializes"),
        None => "none".into(),
    };
    attachment_crop::revision(&format!("{id}\n{body}"))
}

/// Jira timestamps use `+0000`; RFC 3339 readers expect `+00:00`.
fn rfc3339(created: &str) -> String {
    let bytes = created.as_bytes();
    let len = bytes.len();
    if len > 5
        && matches!(bytes[len - 5], b'+' | b'-')
        && bytes[len - 4..].iter().all(u8::is_ascii_digit)
    {
        format!("{}:{}", &created[..len - 2], &created[len - 2..])
    } else {
        created.to_owned()
    }
}

/// An issue's current attachments and parsed metadata records.
struct Snapshot {
    files: Vec<JiraAttachment>,
    records: BTreeMap<String, JiraAttachmentRecord>,
}

impl Snapshot {
    fn from_parts(files: Vec<JiraAttachment>, properties: BTreeMap<String, Value>) -> Self {
        let records = properties
            .into_iter()
            .filter_map(|(key, value)| {
                let id = key.strip_prefix(PROPERTY_PREFIX)?;
                let record = serde_json::from_value::<JiraAttachmentRecord>(value).ok()?;
                valid_jira_attachment_id(id).then(|| (id.to_owned(), record))
            })
            .collect();
        Self { files, records }
    }

    /// Ids of every rendition, by name or by a record pointing at it.
    fn renditions(&self) -> HashSet<&str> {
        let mut ids: HashSet<&str> = self
            .files
            .iter()
            .filter(|file| is_rendition_name(&file.filename))
            .map(|file| file.id.as_str())
            .collect();
        ids.extend(
            self.records
                .values()
                .filter_map(|record| record.rendition_id.as_deref()),
        );
        ids
    }

    /// The attachments users see: every file except crop renditions.
    fn visible(&self) -> Vec<&JiraAttachment> {
        let renditions = self.renditions();
        self.files
            .iter()
            .filter(|file| !renditions.contains(file.id.as_str()))
            .collect()
    }

    fn project(&self) -> Vec<ApiAttachment> {
        self.visible()
            .into_iter()
            .map(|file| {
                let record = self.records.get(&file.id);
                let owned = record.cloned().unwrap_or_default();
                ApiAttachment {
                    id: file.id.clone(),
                    filename: file.filename.clone(),
                    created_at: rfc3339(&file.created),
                    batch_id: owned.batch_id,
                    batch_label: owned.batch_label,
                    actor: owned.actor,
                    purpose: owned.purpose,
                    annotations: owned.annotations,
                    crop: owned.crop,
                    revision: Some(record_revision(&file.id, record)),
                }
            })
            .collect()
    }
}

impl JiraProvider {
    fn raw(
        &self,
        method: &str,
        url: &str,
        body: Option<RawBody<'_>>,
    ) -> Result<Vec<u8>, ProviderError> {
        let credentials = base64(&format!("{}:{}", self.config.email, self.config.token));
        let headers = [
            ("Authorization", format!("Basic {credentials}")),
            ("Accept", "application/json".into()),
            ("User-Agent", "hotsheet2".into()),
            // Jira rejects multipart writes without this XSRF opt-out.
            ("X-Atlassian-Token", "no-check".into()),
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

    /// The issue's attachments and every Hot Sheet attachment property, in one request.
    fn snapshot(&self, key: &str) -> Result<Snapshot, ProviderError> {
        validate_key(key)?;
        let response = self.request(
            "GET",
            &self.endpoint(&format!("issue/{key}?fields=attachment&properties=*all")),
            None,
        )?;
        let issue: IssueAttachmentsAndProperties = self.json(response)?;
        Ok(Snapshot::from_parts(
            issue.fields.attachment,
            issue.properties,
        ))
    }

    /// Project a detail read's attachment field, reading metadata properties only when the
    /// issue has attachments (an attachment-less ticket costs no extra request).
    pub(super) fn project_attachments(
        &self,
        key: &str,
        files: &[JiraAttachment],
    ) -> Result<Vec<ApiAttachment>, ProviderError> {
        if files.is_empty() {
            return Ok(Vec::new());
        }
        Ok(self.snapshot(key)?.project())
    }

    fn write_record(
        &self,
        key: &str,
        id: &str,
        record: &JiraAttachmentRecord,
    ) -> Result<(), ProviderError> {
        self.request(
            "PUT",
            &self.endpoint(&format!("issue/{key}/properties/{PROPERTY_PREFIX}{id}")),
            Some(&serde_json::to_value(record).expect("record serializes")),
        )?;
        Ok(())
    }

    fn upload(
        &self,
        key: &str,
        filename: &str,
        boundary_seed: &str,
        bytes: &[u8],
    ) -> Result<JiraAttachment, ProviderError> {
        let (content_type, body) = multipart_file_body("file", filename, boundary_seed, bytes);
        let response = self.raw(
            "POST",
            &self.endpoint(&format!("issue/{key}/attachments")),
            Some(RawBody {
                content_type: &content_type,
                bytes: &body,
            }),
        )?;
        let mut created: Vec<JiraAttachment> =
            serde_json::from_slice(&response).map_err(|error| ProviderError::Conflict {
                ticket: self.config.connection_id.clone(),
                message: format!("invalid Jira attachment response: {error}"),
            })?;
        if created.len() != 1 || !valid_jira_attachment_id(&created[0].id) {
            return Err(ProviderError::Conflict {
                ticket: format!("{}:{key}", self.config.connection_id),
                message: "Jira did not return exactly one created attachment".into(),
            });
        }
        Ok(created.remove(0))
    }

    fn download(&self, id: &str) -> Result<Vec<u8>, ProviderError> {
        self.raw(
            "GET",
            &self.endpoint(&format!("attachment/content/{id}?redirect=false")),
            None,
        )
    }

    pub(super) fn add_native_attachment(
        &self,
        key: &str,
        attachment: ApiAttachment,
        bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        validate_key(key)?;
        if !valid_attachment_id(&attachment.id) {
            return Err(ProviderError::InvalidNativeId {
                provider: "jira",
                id: attachment.id,
            });
        }
        let snapshot = self.snapshot(key)?;
        let existing = snapshot
            .files
            .iter()
            .map(|file| file.id.as_str())
            .collect::<HashSet<_>>();
        if snapshot.records.iter().any(|(id, record)| {
            existing.contains(id.as_str()) && record.hotsheet_id.as_deref() == Some(&attachment.id)
        }) {
            return self.get(key);
        }
        let created = self.upload(key, &attachment.filename, &attachment.id, &bytes)?;
        let record = JiraAttachmentRecord {
            hotsheet_id: Some(attachment.id),
            batch_id: attachment.batch_id,
            batch_label: attachment.batch_label,
            actor: attachment.actor,
            purpose: attachment.purpose,
            crop: None,
            annotations: vec![],
            rendition_id: None,
        };
        self.write_record(key, &created.id, &record)?;
        self.get(key)
    }

    /// The visible attachment, or `NotFound` for an unknown id or a crop rendition.
    fn visible_file(
        &self,
        snapshot: &Snapshot,
        attachment_id: &str,
    ) -> Result<JiraAttachment, ProviderError> {
        snapshot
            .visible()
            .into_iter()
            .find(|file| file.id == attachment_id)
            .cloned()
            .ok_or_else(|| ProviderError::NotFound {
                connection_id: self.config.connection_id.clone(),
                native_id: attachment_id.into(),
            })
    }

    pub(super) fn native_attachment_bytes(
        &self,
        key: &str,
        attachment_id: &str,
        visible: bool,
    ) -> Result<Vec<u8>, ProviderError> {
        let snapshot = self.snapshot(key)?;
        let file = self.visible_file(&snapshot, attachment_id)?;
        let rendition = visible
            .then(|| snapshot.records.get(&file.id))
            .flatten()
            .and_then(|record| record.rendition_id.as_deref())
            .filter(|id| snapshot.files.iter().any(|file| file.id == *id));
        self.download(rendition.unwrap_or(&file.id))
    }

    /// Crop and annotations rewrite only this attachment's property, after a revision
    /// check. Jira properties have no compare-and-swap, so the property is re-read right
    /// before the write and a change made during a rendition upload is a conflict.
    pub(super) fn update_native_markup(
        &self,
        key: &str,
        attachment_id: &str,
        expected_revision: Option<&str>,
        markup: AttachmentMarkup,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = format!("{}:{key}", self.config.connection_id);
        hotsheet_model::validate_media_annotations(&markup.annotations).map_err(|error| {
            ProviderError::Conflict {
                ticket: ticket.clone(),
                message: error.to_string(),
            }
        })?;
        let snapshot = self.snapshot(key)?;
        let file = self.visible_file(&snapshot, attachment_id)?;
        let current = snapshot.records.get(&file.id).cloned();
        let current_revision = record_revision(&file.id, current.as_ref());
        let mut record = current.clone().unwrap_or_default();
        let unchanged = record.crop == markup.crop && record.annotations == markup.annotations;
        if expected_revision != Some(current_revision.as_str()) {
            if unchanged {
                return self.get(key);
            }
            return Err(ProviderError::Conflict {
                ticket,
                message: "attachment changed since this gallery session opened".into(),
            });
        }
        if unchanged {
            return self.get(key);
        }
        let previous_rendition = record.rendition_id.clone();
        if record.crop != markup.crop || (markup.crop.is_some() && record.rendition_id.is_none()) {
            record.rendition_id = None;
            record.crop = None;
            if let Some(crop) = markup.crop {
                let original = self.download(&file.id)?;
                let rendered = attachment_crop::render(&ticket, &file.filename, &original, crop)?;
                record.crop = rendered.crop;
                if let Some((name, bytes)) = rendered.rendition {
                    let filename = format!("hotsheet-{}-{name}", file.id);
                    // A retry after an interrupted property write reuses its rendition.
                    let reused = snapshot
                        .files
                        .iter()
                        .find(|candidate| candidate.filename == filename)
                        .map(|candidate| candidate.id.clone());
                    record.rendition_id = Some(match reused {
                        Some(id) => id,
                        None => self.upload(key, &filename, &filename, &bytes)?.id,
                    });
                }
            }
        }
        record.annotations = markup.annotations;
        let latest = self.snapshot(key)?;
        if record_revision(&file.id, latest.records.get(&file.id)) != current_revision {
            return Err(ProviderError::Conflict {
                ticket,
                message: "attachment changed during crop upload".into(),
            });
        }
        self.write_record(key, &file.id, &record)?;
        if let Some(old) = previous_rendition
            && record.rendition_id.as_deref() != Some(old.as_str())
        {
            // Best effort: an undeleted old rendition stays hidden by its name.
            let _ = self.raw("DELETE", &self.endpoint(&format!("attachment/{old}")), None);
        }
        self.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::github::{BinaryResponse, GitHubTransport};
    use crate::jira::JiraConfig;
    use hotsheet_sync::LockExt;
    use serde_json::json;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    const HOTSHEET_ID: &str = "01K6JIRA000000000000000000";

    /// A stateful fake of Jira Cloud's issue, comment, property, and attachment endpoints
    /// with the real response shapes and status codes (attachment POST returns a `200`
    /// array, property PUT `200`/`201`, attachment DELETE `204`, unknown ids `404`).
    #[derive(Default)]
    struct FakeJira {
        state: Mutex<FakeState>,
    }

    #[derive(Default)]
    struct FakeState {
        files: Vec<(String, String, Vec<u8>)>,
        properties: BTreeMap<String, Value>,
        next_id: u64,
        fail_property_put_once: bool,
        /// Change this attachment's property while the next upload is in flight.
        edit_property_during_upload: Option<String>,
        uploads: u64,
        deletes: Vec<String>,
        xsrf_headers: Vec<String>,
        json_requests: Vec<String>,
    }

    fn text(status: u16, body: Value) -> HttpResponse {
        HttpResponse {
            status,
            headers: HashMap::new(),
            body: body.to_string(),
        }
    }

    fn issue_json(state: &FakeState) -> Value {
        json!({"key":"ENG-7","fields":{"summary":"Evidence","description":null,"status":{"statusCategory":{"key":"new"}},"priority":{"name":"Medium"},"issuetype":{"name":"Bug"},"labels":[],"assignee":null,"created":"2026-10-01T00:00:00.000+0000","updated":"2026-10-01T00:00:00.000+0000","resolutiondate":null,"attachment":attachments_json(state)}})
    }

    fn attachments_json(state: &FakeState) -> Value {
        state
            .files
            .iter()
            .map(|(id, filename, bytes)| {
                json!({"self":format!("https://jira.test/rest/api/3/attachment/{id}"),"id":id,"filename":filename,"author":{"accountId":"acct-1","displayName":"Dev"},"created":"2026-10-01T00:00:05.000+0000","size":bytes.len(),"mimeType":"application/octet-stream","content":format!("https://jira.test/rest/api/3/attachment/content/{id}")})
            })
            .collect()
    }

    fn parse_multipart(content_type: &str, body: &[u8]) -> (String, Vec<u8>) {
        let boundary = content_type.split("boundary=").nth(1).unwrap();
        let text = String::from_utf8_lossy(body);
        assert!(text.contains("name=\"file\""));
        let filename = text
            .split("filename=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap()
            .to_owned();
        let start = body.windows(4).position(|w| w == b"\r\n\r\n").unwrap() + 4;
        let end = body.len() - format!("\r\n--{boundary}--\r\n").len();
        (filename, body[start..end].to_vec())
    }

    impl GitHubTransport for FakeJira {
        fn request(
            &self,
            method: &str,
            url: &str,
            _: &[(&str, String)],
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            let mut state = self.state.lock_or_recover();
            let path = url
                .strip_prefix("https://jira.test/rest/api/3/")
                .ok_or("unexpected host")?;
            state.json_requests.push(format!("{method} {path}"));
            if method == "GET" && path == "issue/ENG-7" {
                return Ok(text(200, issue_json(&state)));
            }
            if method == "GET" && path == "issue/ENG-7?fields=attachment&properties=*all" {
                return Ok(text(
                    200,
                    json!({"key":"ENG-7","fields":{"attachment":attachments_json(&state)},"properties":state.properties}),
                ));
            }
            if method == "GET" && path.starts_with("issue/ENG-7/comment?") {
                return Ok(text(200, json!({"comments":[],"total":0})));
            }
            if method == "PUT"
                && let Some(property) = path.strip_prefix("issue/ENG-7/properties/")
            {
                if state.fail_property_put_once {
                    state.fail_property_put_once = false;
                    return Ok(text(
                        500,
                        json!({"errorMessages":["Internal server error"]}),
                    ));
                }
                let created = !state.properties.contains_key(property);
                state
                    .properties
                    .insert(property.into(), body.unwrap().clone());
                return Ok(text(if created { 201 } else { 200 }, json!({})));
            }
            Err(format!("unexpected request {method} {url}"))
        }

        fn request_raw(
            &self,
            method: &str,
            url: &str,
            headers: &[(&str, String)],
            body: Option<RawBody<'_>>,
        ) -> Result<BinaryResponse, String> {
            let mut state = self.state.lock_or_recover();
            let path = url
                .strip_prefix("https://jira.test/rest/api/3/")
                .ok_or("unexpected host")?;
            let respond = |status: u16, body: Vec<u8>| BinaryResponse {
                status,
                headers: HashMap::new(),
                body,
            };
            if method == "POST" && path == "issue/ENG-7/attachments" {
                state.xsrf_headers.extend(
                    headers
                        .iter()
                        .filter(|(name, _)| *name == "X-Atlassian-Token")
                        .map(|(_, value)| value.clone()),
                );
                let body = body.ok_or("upload without body")?;
                let (filename, bytes) = parse_multipart(body.content_type, body.bytes);
                state.next_id += 1;
                state.uploads += 1;
                let id = (10_000 + state.next_id).to_string();
                state.files.push((id.clone(), filename.clone(), bytes));
                if let Some(target) = state.edit_property_during_upload.take() {
                    state.properties.insert(
                        format!("{PROPERTY_PREFIX}{target}"),
                        json!({"batch_label":"edited in another session"}),
                    );
                }
                let created = attachments_json(&state)
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter(|file| file["id"] == id)
                    .cloned()
                    .collect::<Vec<_>>();
                return Ok(respond(200, serde_json::to_vec(&created).unwrap()));
            }
            if method == "GET"
                && let Some(rest) = path.strip_prefix("attachment/content/")
            {
                let id = rest
                    .strip_suffix("?redirect=false")
                    .ok_or("redirect not disabled")?;
                return Ok(match state.files.iter().find(|file| file.0 == id) {
                    Some(file) => respond(200, file.2.clone()),
                    None => respond(404, br#"{"errorMessages":["Not found"]}"#.to_vec()),
                });
            }
            if method == "DELETE"
                && let Some(id) = path.strip_prefix("attachment/")
            {
                state.deletes.push(id.into());
                let before = state.files.len();
                state.files.retain(|file| file.0 != id);
                return Ok(respond(
                    if state.files.len() < before { 204 } else { 404 },
                    vec![],
                ));
            }
            Err(format!("unexpected raw request {method} {url}"))
        }
    }

    fn provider(fake: Arc<FakeJira>) -> JiraProvider {
        JiraProvider::new(
            JiraConfig {
                connection_id: "jira-eng".into(),
                project_key: "ENG".into(),
                base_url: "https://jira.test".into(),
                email: "dev@example.com".into(),
                token: "token".into(),
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

    fn png() -> Vec<u8> {
        use image::{DynamicImage, ImageFormat, Rgb, RgbImage};
        let image = RgbImage::from_fn(24, 20, |x, y| Rgb([x as u8, y as u8, 42]));
        let mut bytes = std::io::Cursor::new(Vec::new());
        DynamicImage::ImageRgb8(image)
            .write_to(&mut bytes, ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    }

    fn wire(attachments: &[ApiAttachment]) -> Value {
        serde_json::to_value(attachments).unwrap()
    }

    #[test]
    fn jira_reports_native_attachments_without_edit() {
        let capabilities = provider(Arc::default()).descriptor().capabilities;
        assert!(capabilities.attachments);
        assert!(!capabilities.attachment_edit);
        assert!(capabilities.attachment_crop);
    }

    #[test]
    fn upload_projects_native_attachments_with_metadata_and_reads_bytes_back() {
        let fake = Arc::new(FakeJira::default());
        let provider = provider(fake.clone());
        // A file someone attached directly in Jira projects without Hot Sheet metadata.
        fake.state.lock_or_recover().files.push((
            "9999".into(),
            "native.txt".into(),
            b"native".to_vec(),
        ));
        let ticket = provider
            .add_attachment(
                "ENG-7",
                evidence(HOTSHEET_ID, "trace log.txt"),
                b"trace".to_vec(),
            )
            .unwrap();
        assert_eq!(ticket.attachments.len(), 2);
        let native = &ticket.attachments[0];
        assert_eq!(native.id, "9999");
        assert_eq!(native.created_at, "2026-10-01T00:00:05.000+00:00");
        assert!(native.purpose.is_none());
        let added = &ticket.attachments[1];
        assert_eq!(added.id, "10001");
        assert_eq!(added.filename, "trace log.txt");
        assert_eq!(added.batch_id.as_deref(), Some("batch-1"));
        assert_eq!(added.batch_label.as_deref(), Some("Before"));
        assert_eq!(added.purpose, Some(AttachmentPurpose::ProblemEvidence));
        assert!(added.revision.is_some());
        {
            let state = fake.state.lock_or_recover();
            assert_eq!(state.xsrf_headers, ["no-check"]);
            assert_eq!(
                state.properties[&format!("{PROPERTY_PREFIX}10001")]["hotsheet_id"],
                HOTSHEET_ID
            );
        }
        assert_eq!(
            provider.attachment_bytes("ENG-7", "10001").unwrap(),
            b"trace"
        );
        assert_eq!(
            provider.attachment_original_bytes("ENG-7", "9999").unwrap(),
            b"native"
        );
        assert!(matches!(
            provider.attachment_bytes("ENG-7", "123"),
            Err(ProviderError::NotFound { .. })
        ));
    }

    #[test]
    fn retries_are_idempotent_by_hotsheet_id() {
        let fake = Arc::new(FakeJira::default());
        let provider = provider(fake.clone());
        let first = provider
            .add_attachment("ENG-7", evidence(HOTSHEET_ID, "a.png"), png())
            .unwrap();
        let again = provider
            .add_attachment("ENG-7", evidence(HOTSHEET_ID, "a.png"), png())
            .unwrap();
        assert_eq!(wire(&first.attachments), wire(&again.attachments));
        assert_eq!(fake.state.lock_or_recover().uploads, 1);
        // A failed property write leaves the file without metadata; the retry uploads again
        // (Jira has no client id), and the earlier file stays a plain native attachment.
        fake.state.lock_or_recover().fail_property_put_once = true;
        let other = "01K6JIRA00000000000000000B";
        assert!(
            provider
                .add_attachment("ENG-7", evidence(other, "b.png"), png())
                .is_err()
        );
        let retried = provider
            .add_attachment("ENG-7", evidence(other, "b.png"), png())
            .unwrap();
        assert_eq!(fake.state.lock_or_recover().uploads, 3);
        assert_eq!(retried.attachments.len(), 3);
        assert!(retried.attachments[1].batch_id.is_none());
        assert_eq!(retried.attachments[2].batch_id.as_deref(), Some("batch-1"));
    }

    #[test]
    fn invalid_ids_and_malformed_properties_are_handled() {
        let fake = Arc::new(FakeJira::default());
        let provider = provider(fake.clone());
        assert!(matches!(
            provider.add_attachment("ENG 7", evidence(HOTSHEET_ID, "a.txt"), vec![]),
            Err(ProviderError::InvalidNativeId { .. })
        ));
        assert!(matches!(
            provider.add_attachment("ENG-7", evidence("bad id", "a.txt"), vec![]),
            Err(ProviderError::InvalidNativeId { .. })
        ));
        {
            let mut state = fake.state.lock_or_recover();
            state
                .files
                .push(("10050".into(), "x.txt".into(), b"x".to_vec()));
            state.properties.insert(
                format!("{PROPERTY_PREFIX}10050"),
                json!({"crop":"not a crop"}),
            );
            state
                .properties
                .insert("other.app.key".into(), json!({"anything":true}));
        }
        let ticket = provider.get("ENG-7").unwrap();
        assert_eq!(ticket.attachments.len(), 1);
        assert!(ticket.attachments[0].crop.is_none());
        assert!(is_rendition_name("hotsheet-10001-crop-0123.png"));
        assert!(!is_rendition_name("hotsheet-abc-crop-0123.png"));
        assert!(!is_rendition_name("screen.png"));
        assert_eq!(
            rfc3339("2026-10-01T00:00:05.000-0700"),
            "2026-10-01T00:00:05.000-07:00"
        );
        assert_eq!(rfc3339("2026-10-01T00:00:05Z"), "2026-10-01T00:00:05Z");
    }

    #[test]
    fn attachment_less_detail_reads_skip_the_property_request() {
        let fake = Arc::new(FakeJira::default());
        provider(fake.clone()).get("ENG-7").unwrap();
        let requests = fake.state.lock_or_recover().json_requests.clone();
        assert!(
            requests
                .iter()
                .all(|request| !request.contains("properties")),
            "{requests:?}"
        );
    }

    #[test]
    fn crop_uploads_a_hidden_rendition_keeps_the_original_and_restores() {
        let original = png();
        let fake = Arc::new(FakeJira::default());
        let provider = provider(fake.clone());
        let first = provider
            .add_attachment("ENG-7", evidence(HOTSHEET_ID, "shot.png"), original.clone())
            .unwrap();
        let revision = first.attachments[0].revision.clone();
        let crop = ImageCrop {
            x: 3,
            y: 2,
            width: 12,
            height: 10,
        };
        let markup =
            |crop, annotations: Vec<MediaAnnotation>| AttachmentMarkup { annotations, crop };
        let cropped = provider
            .set_attachment_markup(
                "ENG-7",
                "10001",
                revision.as_deref(),
                markup(Some(crop), vec![]),
            )
            .unwrap();
        assert_eq!(cropped.attachments.len(), 1, "the rendition is hidden");
        assert_eq!(cropped.attachments[0].crop, Some(crop));
        assert_ne!(cropped.attachments[0].revision, revision);
        assert_eq!(cropped.attachments[0].batch_id.as_deref(), Some("batch-1"));
        let rendition = provider.attachment_bytes("ENG-7", "10001").unwrap();
        assert_eq!(image::load_from_memory(&rendition).unwrap().width(), 12);
        assert_eq!(
            provider
                .attachment_original_bytes("ENG-7", "10001")
                .unwrap(),
            original
        );
        assert!(matches!(
            provider.attachment_bytes("ENG-7", "10002"),
            Err(ProviderError::NotFound { .. })
        ));
        {
            let state = fake.state.lock_or_recover();
            assert!(state.files[1].1.starts_with("hotsheet-10001-crop-"));
        }
        // Annotation-only change: no upload, rendition kept.
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
                "ENG-7",
                "10001",
                cropped.attachments[0].revision.as_deref(),
                markup(Some(crop), vec![annotation.clone()]),
            )
            .unwrap();
        assert_eq!(fake.state.lock_or_recover().uploads, 2);
        assert_eq!(
            annotated.attachments[0].annotations,
            vec![annotation.clone()]
        );
        // Stale with a different markup conflicts; stale with the same markup is a no-op.
        assert!(matches!(
            provider.set_attachment_markup(
                "ENG-7",
                "10001",
                revision.as_deref(),
                markup(None, vec![])
            ),
            Err(ProviderError::Conflict { .. })
        ));
        let same = provider
            .set_attachment_markup(
                "ENG-7",
                "10001",
                revision.as_deref(),
                markup(Some(crop), vec![annotation.clone()]),
            )
            .unwrap();
        assert_eq!(wire(&same.attachments), wire(&annotated.attachments));
        // A different crop replaces the rendition and deletes the old one.
        let wider = ImageCrop {
            x: 0,
            y: 0,
            width: 16,
            height: 16,
        };
        let recropped = provider
            .set_attachment_markup(
                "ENG-7",
                "10001",
                annotated.attachments[0].revision.as_deref(),
                markup(Some(wider), vec![annotation.clone()]),
            )
            .unwrap();
        assert_eq!(recropped.attachments[0].crop, Some(wider));
        assert_eq!(fake.state.lock_or_recover().deletes, ["10002"]);
        // Restore shows the original again, keeps annotations, and deletes the rendition.
        let restored = provider
            .set_attachment_markup(
                "ENG-7",
                "10001",
                recropped.attachments[0].revision.as_deref(),
                markup(None, vec![annotation.clone()]),
            )
            .unwrap();
        assert!(restored.attachments[0].crop.is_none());
        assert_eq!(restored.attachments[0].annotations, vec![annotation]);
        assert_eq!(
            provider.attachment_bytes("ENG-7", "10001").unwrap(),
            original
        );
        let state = fake.state.lock_or_recover();
        assert_eq!(state.deletes, ["10002", "10003"]);
        assert_eq!(state.files.len(), 1);
    }

    #[test]
    fn crop_of_a_native_attachment_and_retry_after_interrupted_property_write() {
        let fake = Arc::new(FakeJira::default());
        let provider = provider(fake.clone());
        fake.state
            .lock_or_recover()
            .files
            .push(("9999".into(), "native.png".into(), png()));
        let first = provider.get("ENG-7").unwrap();
        let crop = ImageCrop {
            x: 1,
            y: 1,
            width: 8,
            height: 8,
        };
        fake.state.lock_or_recover().fail_property_put_once = true;
        let request = || AttachmentMarkup {
            annotations: vec![],
            crop: Some(crop),
        };
        assert!(
            provider
                .set_attachment_markup(
                    "ENG-7",
                    "9999",
                    first.attachments[0].revision.as_deref(),
                    request()
                )
                .is_err()
        );
        // The orphan rendition stays hidden by name and is reused by the retry.
        let pending = provider.get("ENG-7").unwrap();
        assert_eq!(pending.attachments.len(), 1);
        assert!(pending.attachments[0].crop.is_none());
        let cropped = provider
            .set_attachment_markup(
                "ENG-7",
                "9999",
                pending.attachments[0].revision.as_deref(),
                request(),
            )
            .unwrap();
        assert_eq!(cropped.attachments[0].crop, Some(crop));
        assert_eq!(fake.state.lock_or_recover().uploads, 1);
        assert_eq!(
            image::load_from_memory(&provider.attachment_bytes("ENG-7", "9999").unwrap())
                .unwrap()
                .width(),
            8
        );
    }

    #[test]
    fn a_property_changed_during_the_rendition_upload_conflicts() {
        let fake = Arc::new(FakeJira::default());
        let provider = provider(fake.clone());
        let first = provider
            .add_attachment("ENG-7", evidence(HOTSHEET_ID, "shot.png"), png())
            .unwrap();
        fake.state.lock_or_recover().edit_property_during_upload = Some("10001".into());
        let result = provider.set_attachment_markup(
            "ENG-7",
            "10001",
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
        let current = provider.get("ENG-7").unwrap();
        assert_eq!(current.attachments.len(), 1);
        assert_eq!(
            current.attachments[0].batch_label.as_deref(),
            Some("edited in another session")
        );
    }

    /// HS2-KE18XX: upload, idempotent retry, read-back, crop, and restore against a real
    /// Jira issue (the operator's HS2T test site). Opt-in only; writes need
    /// `HOTSHEET_JIRA_LIVE_WRITE=1` and `HOTSHEET_JIRA_LIVE_ATTACHMENT_ISSUE`.
    #[test]
    #[ignore = "writes attachments to a real Jira issue; set HOTSHEET_JIRA_LIVE_BASE_URL/PROJECT/EMAIL/TOKEN, HOTSHEET_JIRA_LIVE_WRITE=1 and HOTSHEET_JIRA_LIVE_ATTACHMENT_ISSUE"]
    fn jira_live_attachment_upload_crop_and_read() {
        assert_eq!(
            std::env::var("HOTSHEET_JIRA_LIVE_WRITE").as_deref(),
            Ok("1"),
            "write access to the live project must be opted into"
        );
        let env = |name: &str| std::env::var(name).unwrap_or_else(|_| panic!("{name}"));
        let provider = JiraProvider::live(JiraConfig {
            connection_id: "jira-live".into(),
            project_key: env("HOTSHEET_JIRA_LIVE_PROJECT"),
            base_url: env("HOTSHEET_JIRA_LIVE_BASE_URL"),
            email: env("HOTSHEET_JIRA_LIVE_EMAIL"),
            token: env("HOTSHEET_JIRA_LIVE_TOKEN"),
            default: false,
        });
        let issue = env("HOTSHEET_JIRA_LIVE_ATTACHMENT_ISSUE");
        let hotsheet_id = hotsheet_model::Ulid::new().to_string();
        let bytes = png();
        let ticket = provider
            .add_attachment(
                &issue,
                evidence(&hotsheet_id, "hotsheet-live.png"),
                bytes.clone(),
            )
            .unwrap();
        let attachment = ticket
            .attachments
            .iter()
            .find(|item| item.filename == "hotsheet-live.png" && item.batch_id.is_some())
            .expect("uploaded attachment projects back")
            .clone();
        assert_eq!(
            provider.attachment_bytes(&issue, &attachment.id).unwrap(),
            bytes
        );
        let again = provider
            .add_attachment(
                &issue,
                evidence(&hotsheet_id, "hotsheet-live.png"),
                bytes.clone(),
            )
            .unwrap();
        assert_eq!(again.attachments.len(), ticket.attachments.len());
        let crop = ImageCrop {
            x: 2,
            y: 2,
            width: 10,
            height: 8,
        };
        let cropped = provider
            .set_attachment_markup(
                &issue,
                &attachment.id,
                attachment.revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![],
                    crop: Some(crop),
                },
            )
            .unwrap();
        let rendition = provider.attachment_bytes(&issue, &attachment.id).unwrap();
        assert_eq!(image::load_from_memory(&rendition).unwrap().width(), 10);
        let current = cropped
            .attachments
            .iter()
            .find(|item| item.id == attachment.id)
            .unwrap();
        provider
            .set_attachment_markup(
                &issue,
                &attachment.id,
                current.revision.as_deref(),
                AttachmentMarkup {
                    annotations: vec![],
                    crop: None,
                },
            )
            .unwrap();
        assert_eq!(
            provider.attachment_bytes(&issue, &attachment.id).unwrap(),
            bytes
        );
    }
}
