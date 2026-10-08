//! GitHub Issues attachments through a configured assets repository (HS2-HSA64D).
//!
//! GitHub has no issue-attachment API usable by app or personal tokens (its
//! `user-attachments` upload is browser-only), so, like the original Hot Sheet GitHub
//! plugin, an attachment is a file committed to a configured assets repository through the
//! Contents API plus one issue comment linking it. The comment is human-readable on GitHub
//! (`![name](url)` for images, `[name](url)` otherwise) and ends with a hidden marker that
//! lets Hot Sheet project the comment back as an attachment and make retries idempotent:
//!
//! ```text
//! ![screen.png](https://raw.githubusercontent.com/acme/assets/main/hotsheet-attachments/01J…-screen.png)
//!
//! <!-- hotsheet-attachment:01J… {"filename":"screen.png","path":"…","repository":"acme/assets","branch":"main","sha":"…"} -->
//! ```
//!
//! Connection settings keep the original plugin's keys: `attachment_repo` (`owner/repo`),
//! `attachment_folder` (default `hotsheet-attachments`), and `attachment_branch` (default
//! `main`). Attachments are reported as supported only when `attachment_repo` is set.

use hotsheet_model::{AttachmentActor, AttachmentPurpose};
use hotsheet_ticketing::wire::ApiAttachment;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Setting key naming the `owner/repo` assets repository.
pub const ATTACHMENT_REPO_SETTING: &str = "attachment_repo";
/// Setting key naming the folder inside the assets repository.
pub const ATTACHMENT_FOLDER_SETTING: &str = "attachment_folder";
/// Setting key naming the branch uploads are committed to.
pub const ATTACHMENT_BRANCH_SETTING: &str = "attachment_branch";
/// The original Hot Sheet plugin's default folder.
pub const DEFAULT_ATTACHMENT_FOLDER: &str = "hotsheet-attachments";
/// The original Hot Sheet plugin's default branch.
pub const DEFAULT_ATTACHMENT_BRANCH: &str = "main";

const MARKER_PREFIX: &str = "<!-- hotsheet-attachment:";

/// Where a GitHub connection stores attachment files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitHubAttachmentRepository {
    pub repository: String,
    pub folder: String,
    pub branch: String,
}

impl GitHubAttachmentRepository {
    /// Validate an assets repository, applying the HS1 defaults for an omitted folder or
    /// branch. The folder is normalized without leading/trailing slashes and may be empty
    /// (the repository root).
    pub fn new(
        repository: &str,
        folder: Option<&str>,
        branch: Option<&str>,
    ) -> Result<Self, String> {
        let repository = repository.trim();
        let mut parts = repository.split('/');
        let valid_repository = matches!(
            (parts.next(), parts.next(), parts.next()),
            (Some(owner), Some(name), None) if valid_name(owner) && valid_name(name)
        );
        if !valid_repository {
            return Err(format!(
                "attachment repository must be 'owner/repository', got '{repository}'"
            ));
        }
        let folder = folder
            .unwrap_or(DEFAULT_ATTACHMENT_FOLDER)
            .trim()
            .trim_matches('/');
        if folder.split('/').any(|segment| {
            matches!(segment, "." | "..") || (segment.is_empty() && !folder.is_empty())
        }) {
            return Err(format!(
                "attachment folder '{folder}' is not a plain relative path"
            ));
        }
        let branch = branch.unwrap_or(DEFAULT_ATTACHMENT_BRANCH).trim();
        if branch.is_empty() || branch.contains("..") || branch.chars().any(char::is_whitespace) {
            return Err(format!(
                "attachment branch '{branch}' is not a valid branch name"
            ));
        }
        Ok(Self {
            repository: repository.into(),
            folder: folder.into(),
            branch: branch.into(),
        })
    }

    /// Read the configured repository from connection settings; `None` when unset.
    pub fn from_settings(settings: &Value) -> Result<Option<Self>, String> {
        let text = |key| settings.get(key).and_then(Value::as_str);
        match text(ATTACHMENT_REPO_SETTING).map(str::trim) {
            None | Some("") => Ok(None),
            Some(repository) => Self::new(
                repository,
                text(ATTACHMENT_FOLDER_SETTING),
                text(ATTACHMENT_BRANCH_SETTING),
            )
            .map(Some),
        }
    }

    /// Write this repository into connection settings (always with explicit folder/branch).
    pub fn write_settings(&self, settings: &mut Value) {
        if !settings.is_object() {
            *settings = Value::Object(Default::default());
        }
        settings[ATTACHMENT_REPO_SETTING] = self.repository.clone().into();
        settings[ATTACHMENT_FOLDER_SETTING] = self.folder.clone().into();
        settings[ATTACHMENT_BRANCH_SETTING] = self.branch.clone().into();
    }

    /// Remove every attachment-repository setting.
    pub fn clear_settings(settings: &mut Value) {
        if let Some(object) = settings.as_object_mut() {
            for key in [
                ATTACHMENT_REPO_SETTING,
                ATTACHMENT_FOLDER_SETTING,
                ATTACHMENT_BRANCH_SETTING,
            ] {
                object.remove(key);
            }
        }
    }

    /// The repository path of one attachment: `{folder}/{attachment id}-{safe filename}`.
    /// The id keeps the path unique and makes a retried upload address the same file.
    pub fn file_path(&self, attachment_id: &str, filename: &str) -> String {
        let name = format!("{attachment_id}-{}", safe_filename(filename));
        if self.folder.is_empty() {
            name
        } else {
            format!("{}/{name}", self.folder)
        }
    }

    /// The permanent URL linked from the issue comment. On github.com this is the original
    /// plugin's `raw.githubusercontent.com` form (not the short-lived `download_url`); on
    /// GitHub Enterprise it is the file page's `/raw/` form.
    pub fn link_url(&self, api_base: &str, path: &str, html_url: Option<&str>) -> String {
        let encoded = encode_path(path);
        if api_base.trim_end_matches('/') == "https://api.github.com" {
            return format!(
                "https://raw.githubusercontent.com/{}/{}/{encoded}",
                self.repository,
                encode_path(&self.branch)
            );
        }
        match html_url.filter(|url| url.contains("/blob/")) {
            Some(url) => url.replacen("/blob/", "/raw/", 1),
            None => {
                let web = api_base
                    .trim_end_matches('/')
                    .trim_end_matches("/api/v3")
                    .to_string();
                format!(
                    "{web}/{}/raw/{}/{encoded}",
                    self.repository,
                    encode_path(&self.branch)
                )
            }
        }
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// The original plugin's filename sanitizer: anything outside `[A-Za-z0-9._-]` becomes `_`.
pub fn safe_filename(filename: &str) -> String {
    let safe: String = filename
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if safe.is_empty() {
        "attachment".into()
    } else {
        safe
    }
}

/// Percent-encode each path segment for a URL, keeping `/` separators.
pub fn encode_path(path: &str) -> String {
    path.split('/')
        .map(|segment| {
            segment
                .bytes()
                .map(|byte| {
                    if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
                        (byte as char).to_string()
                    } else {
                        format!("%{byte:02X}")
                    }
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Attachment details carried by the hidden marker of a link comment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AttachmentMarker {
    pub filename: String,
    pub path: String,
    pub repository: String,
    pub branch: String,
    /// Git blob sha of the uploaded file, used to read it back through the API.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub batch_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<AttachmentActor>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub purpose: Option<AttachmentPurpose>,
}

/// The marker text identifying an attachment id inside a comment.
pub fn marker_key(attachment_id: &str) -> String {
    format!("{MARKER_PREFIX}{attachment_id} ")
}

/// Whether an attachment id is safe to embed in a marker (a ULID or similar token).
pub fn valid_attachment_id(attachment_id: &str) -> bool {
    !attachment_id.is_empty()
        && attachment_id.len() <= 64
        && attachment_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
}

/// The full comment body for an uploaded attachment.
pub fn compose_comment(attachment_id: &str, url: &str, marker: &AttachmentMarker) -> String {
    let label = marker
        .filename
        .replace('\\', "\\\\")
        .replace('[', "\\[")
        .replace(']', "\\]");
    let link = if is_image(&marker.filename) {
        format!("![{label}]({url})")
    } else {
        format!("[{label}]({url})")
    };
    // `>` only occurs inside JSON strings, so escaping it keeps `-->` out of the comment.
    let json = serde_json::to_string(marker)
        .expect("attachment marker serializes")
        .replace('>', "\\u003e");
    format!("{link}\n\n{}{json} -->", marker_key(attachment_id))
}

/// Parse a comment written by [`compose_comment`]; any other comment returns `None`.
pub fn parse_comment(body: &str) -> Option<(String, AttachmentMarker)> {
    let start = body.rfind(MARKER_PREFIX)?;
    let rest = body[start + MARKER_PREFIX.len()..].trim_end();
    let rest = rest.strip_suffix("-->")?.trim_end();
    let (id, json) = rest.split_once(' ')?;
    if !valid_attachment_id(id) {
        return None;
    }
    let marker = serde_json::from_str::<AttachmentMarker>(json.trim()).ok()?;
    Some((id.to_owned(), marker))
}

/// Project a parsed marker comment as a normalized attachment.
pub fn api_attachment(id: String, marker: AttachmentMarker, created_at: String) -> ApiAttachment {
    ApiAttachment {
        id,
        filename: marker.filename,
        created_at,
        batch_id: marker.batch_id,
        batch_label: marker.batch_label,
        actor: marker.actor,
        purpose: marker.purpose,
        annotations: vec![],
        crop: None,
    }
}

fn is_image(filename: &str) -> bool {
    let extension = filename
        .rsplit_once('.')
        .map(|(_, extension)| extension.to_ascii_lowercase());
    matches!(
        extension.as_deref(),
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg" | "bmp" | "avif")
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn settings_use_hs1_keys_and_defaults_and_reject_malformed_values() {
        assert_eq!(
            GitHubAttachmentRepository::from_settings(&json!({})),
            Ok(None)
        );
        assert_eq!(
            GitHubAttachmentRepository::from_settings(&json!({"attachment_repo": "  "})),
            Ok(None)
        );
        let repo =
            GitHubAttachmentRepository::from_settings(&json!({"attachment_repo": "acme/assets"}))
                .unwrap()
                .unwrap();
        assert_eq!(repo.folder, "hotsheet-attachments");
        assert_eq!(repo.branch, "main");
        let custom =
            GitHubAttachmentRepository::new("acme/assets", Some("/evidence/hs/"), Some("media"))
                .unwrap();
        assert_eq!(custom.folder, "evidence/hs");
        let mut settings = json!({"credential": {"secret": "k"}});
        custom.write_settings(&mut settings);
        assert_eq!(settings["attachment_folder"], "evidence/hs");
        assert_eq!(
            GitHubAttachmentRepository::from_settings(&settings).unwrap(),
            Some(custom)
        );
        GitHubAttachmentRepository::clear_settings(&mut settings);
        assert_eq!(settings, json!({"credential": {"secret": "k"}}));
        for bad in [
            "acme",
            "acme/assets/extra",
            "/assets",
            "ac me/assets",
            "acme/..",
        ] {
            assert!(
                GitHubAttachmentRepository::new(bad, None, None).is_err(),
                "{bad}"
            );
        }
        assert!(GitHubAttachmentRepository::new("acme/assets", Some("a/../b"), None).is_err());
        assert!(GitHubAttachmentRepository::new("acme/assets", Some("a//b"), None).is_err());
        assert!(GitHubAttachmentRepository::new("acme/assets", None, Some("a b")).is_err());
        assert!(GitHubAttachmentRepository::new("acme/assets", None, Some(" ")).is_err());
        assert_eq!(
            GitHubAttachmentRepository::new("acme/assets", Some(""), None)
                .unwrap()
                .file_path("ID", "a b.png"),
            "ID-a_b.png"
        );
    }

    #[test]
    fn links_use_the_permanent_raw_url_or_the_enterprise_raw_page() {
        let repo = GitHubAttachmentRepository::new("acme/assets", None, Some("main")).unwrap();
        let path = repo.file_path("01J", "shot 1.png");
        assert_eq!(path, "hotsheet-attachments/01J-shot_1.png");
        assert_eq!(
            repo.link_url("https://api.github.com/", &path, None),
            "https://raw.githubusercontent.com/acme/assets/main/hotsheet-attachments/01J-shot_1.png"
        );
        assert_eq!(
            repo.link_url(
                "https://ghe.example/api/v3",
                &path,
                Some(
                    "https://ghe.example/acme/assets/blob/main/hotsheet-attachments/01J-shot_1.png"
                )
            ),
            "https://ghe.example/acme/assets/raw/main/hotsheet-attachments/01J-shot_1.png"
        );
        assert_eq!(
            repo.link_url("https://ghe.example/api/v3", &path, None),
            "https://ghe.example/acme/assets/raw/main/hotsheet-attachments/01J-shot_1.png"
        );
        assert_eq!(encode_path("a b/ü"), "a%20b/%C3%BC");
        assert_eq!(safe_filename("日本"), "__");
        assert_eq!(safe_filename(""), "attachment");
    }

    #[test]
    fn comments_round_trip_and_ordinary_comments_are_not_attachments() {
        let marker = AttachmentMarker {
            filename: "log [final].txt".into(),
            path: "f/ID-log__final_.txt".into(),
            repository: "acme/assets".into(),
            branch: "main".into(),
            sha: Some("abc".into()),
            batch_id: Some("b1".into()),
            batch_label: Some("before --> after".into()),
            actor: None,
            purpose: Some(AttachmentPurpose::ProblemEvidence),
        };
        let body = compose_comment("01JX", "https://x/y", &marker);
        assert!(body.starts_with("[log \\[final\\].txt](https://x/y)\n\n"));
        assert_eq!(body.matches("-->").count(), 1, "{body}");
        assert_eq!(parse_comment(&body), Some(("01JX".into(), marker.clone())));
        let image = compose_comment(
            "01JY",
            "https://x/i.png",
            &AttachmentMarker {
                filename: "i.PNG".into(),
                ..marker
            },
        );
        assert!(image.starts_with("![i.PNG](https://x/i.png)"));
        for ordinary in [
            "just a comment",
            "<!-- hotsheet-attachment:bad id {} -->",
            "<!-- hotsheet-attachment:01J not json -->",
            "<!-- hotsheet-attachment:01J {\"filename\":\"a\"} -->",
            "<!-- hotsheet-note-id:01J -->",
        ] {
            assert_eq!(parse_comment(ordinary), None, "{ordinary}");
        }
        assert!(!valid_attachment_id(""));
        assert!(!valid_attachment_id(&"x".repeat(65)));
    }
}
