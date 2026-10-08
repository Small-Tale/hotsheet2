//! Private, review-gated AI feedback synthesis. No source text is sent to a model
//! and this module never publishes repository guidance.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use hotsheet_model::AiFeedbackRating;
use hotsheet_ticketing::AiFeedbackRecord;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, Default, Serialize, Deserialize)]
struct ReviewState {
    #[serde(default)]
    reviewed: BTreeMap<String, String>,
    #[serde(default)]
    cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
struct PendingReview {
    cursor: String,
    sources: BTreeMap<String, String>,
    providers: Vec<String>,
    errors: Vec<String>,
}

pub fn state_dir(store_root: &Path) -> Result<PathBuf> {
    let canonical = store_root.canonicalize()?;
    let hash = Sha256::digest(canonical.to_string_lossy().as_bytes());
    Ok(hotsheet_plugins::hotsheet_home()
        .join("feedback-synthesis")
        .join(hex(&hash[..12])))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn source_key(row: &AiFeedbackRecord) -> String {
    format!("{}\0{}\0{}", row.connection_id, row.ticket_id, row.note_id)
}

fn fingerprint(row: &AiFeedbackRecord) -> Result<String> {
    let bytes = serde_json::to_vec(&(
        &row.target,
        row.rating,
        &row.explanation,
        &row.rater,
        &row.edited_at,
        row.legacy,
    ))?;
    Ok(hex(&Sha256::digest(bytes)))
}

fn cursor(sources: &BTreeMap<String, String>) -> Result<String> {
    Ok(format!(
        "sha256:{}",
        hex(&Sha256::digest(serde_json::to_vec(sources)?))
    ))
}

fn private_dir(path: &Path) -> Result<()> {
    fs::create_dir_all(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    Ok(())
}

fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    use std::io::Write;
    let temp = path.with_extension("tmp");
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
    }
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temp, path)?;
    Ok(())
}

fn read_state(path: &Path) -> Result<ReviewState> {
    match fs::read(path) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).context("invalid feedback review state")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(ReviewState::default()),
        Err(error) => Err(error.into()),
    }
}

/// Remove obvious credentials, email addresses, personal machine paths, and URLs
/// before a phrase is placed in a local draft. Human review remains mandatory.
fn redact(input: &str) -> String {
    let mut words = Vec::new();
    let mut hide_next = false;
    for word in input.split_whitespace().take(80) {
        let trimmed = word.trim_matches(|c: char| ",.;:()[]{}<>\"'".contains(c));
        let lower = trimmed.to_ascii_lowercase();
        let sensitive = hide_next
            || trimmed.contains('@')
            || trimmed.starts_with('/')
            || trimmed.starts_with("~/")
            || lower.contains("://")
            || lower.starts_with("ghp_")
            || lower.starts_with("github_pat_")
            || lower.starts_with("sk-")
            || lower.starts_with("token=")
            || lower.starts_with("access_token=")
            || lower.starts_with("api_key=")
            || lower.starts_with("apikey=")
            || lower.starts_with("password=")
            || (trimmed.len() > 2 && trimmed.as_bytes()[1] == b':' && trimmed.contains('\\'));
        words.push(if sensitive { "[redacted]" } else { word });
        hide_next = matches!(lower.as_str(), "token" | "password" | "secret" | "bearer");
    }
    words.join(" ")
}

fn reference(row: &AiFeedbackRecord) -> String {
    format!("{}/{}#{}", row.connection_id, row.slug, row.note_id)
}

fn render_draft(
    rows: &[AiFeedbackRecord],
    providers: &[String],
    errors: &[(String, String)],
    removed: &[String],
    proposed_cursor: &str,
) -> String {
    let mut out = String::from("# AI feedback synthesis — review draft\n\n");
    out.push_str("Private local draft. Edit or reject every candidate before adding accepted guidance to repository docs. No guidance is published by this command.\n\n");
    out.push_str(&format!(
        "Review window: {} through {} (source creation and last edit).\n\n",
        rows.iter()
            .map(|r| r.created_at.as_str())
            .min()
            .unwrap_or("none"),
        rows.iter()
            .map(|r| r.edited_at.as_str())
            .max()
            .unwrap_or("none")
    ));
    out.push_str(&format!(
        "Selected providers: {}.\n\n",
        providers.join(", ")
    ));
    if !errors.is_empty() {
        out.push_str("## Coverage gaps\n\n");
        for (provider, error) in errors {
            out.push_str(&format!("- `{provider}`: {}\n", redact(error)));
        }
        out.push('\n');
    }

    let mut groups: BTreeMap<String, Vec<&AiFeedbackRecord>> = BTreeMap::new();
    let mut uncertain = Vec::new();
    for row in rows {
        let Some(rating) = row.rating else {
            uncertain.push(format!(
                "- {}: rating withdrawn; check prior guidance.",
                reference(row)
            ));
            continue;
        };
        let Some(explanation) = row.explanation.as_deref() else {
            uncertain.push(format!(
                "- {}: {rating:?} without an explanation.",
                reference(row)
            ));
            continue;
        };
        let phrase = redact(explanation);
        if row.legacy || phrase.is_empty() || phrase == "[redacted]" {
            uncertain.push(format!(
                "- {}: legacy or redacted evidence needs triage.",
                reference(row)
            ));
            continue;
        }
        groups
            .entry(phrase.to_ascii_lowercase())
            .or_default()
            .push(row);
    }

    let mut recurring = Vec::new();
    for (phrase, evidence) in groups {
        let independent = evidence
            .iter()
            .filter_map(|row| row.rater.as_ref()?.id.as_deref())
            .collect::<BTreeSet<_>>();
        if independent.len() >= 2 {
            recurring.push((phrase, evidence));
        } else {
            uncertain.push(format!(
                    "- Candidate `{phrase}` with {} source(s), below the independent-example threshold: {}.",
                    evidence.len(),
                evidence
                    .iter()
                    .map(|row| reference(row))
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
    }
    recurring.sort_by(|a, b| b.1.len().cmp(&a.1.len()).then(a.0.cmp(&b.0)));
    out.push_str("## Candidate themes\n\n");
    if recurring.is_empty() {
        out.push_str("No recurring theme meets the two-independent-rater threshold.\n\n");
    }
    for (phrase, evidence) in recurring.iter().take(5) {
        let positive = evidence
            .iter()
            .filter(|r| r.rating == Some(AiFeedbackRating::Helpful))
            .count();
        let negative = evidence.len() - positive;
        let scopes = evidence
            .iter()
            .map(|row| row.target.split(':').next().unwrap_or("unknown"))
            .collect::<BTreeSet<_>>();
        out.push_str(&format!("### {}\n\n", phrase));
        out.push_str(&format!(
            "Proposed action: consider `{phrase}`. Scope: {} feedback in the selected providers. Human review and editing required.\n\n",
            scopes.into_iter().collect::<Vec<_>>().join(", ")
        ));
        out.push_str(&format!(
            "Evidence: {} distinct notes ({} helpful, {} not helpful).\n\n",
            evidence.len(),
            positive,
            negative
        ));
        if positive > 0 && negative > 0 {
            out.push_str("Conflicting ratings: identify the context where each applies before accepting guidance.\n\n");
        }
        out.push_str("Sources: ");
        out.push_str(
            &evidence
                .iter()
                .map(|row| format!("`{}`", reference(row)))
                .collect::<Vec<_>>()
                .join(", "),
        );
        out.push_str(".\n\n");
    }
    for (_, evidence) in recurring.iter().skip(5) {
        uncertain.push(format!(
            "- Additional recurring candidate for review: {}.",
            evidence
                .iter()
                .map(|row| reference(row))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    out.push_str("## Conflicting or uncertain evidence\n\n");
    for item in uncertain {
        out.push_str(&item);
        out.push('\n');
    }
    for key in removed {
        let mut parts = key.split('\0');
        let connection = parts.next().unwrap_or("unknown");
        let ticket = parts.next().unwrap_or("unknown");
        let note = parts.next().unwrap_or("unknown");
        out.push_str(&format!(
            "- Removed source `{connection}/{ticket}#{note}`: check whether prior guidance needs revision.\n"
        ));
    }
    if rows.is_empty() && removed.is_empty() {
        out.push_str("No readable new source records.\n");
    }
    out.push_str("\n## Proposed tickets\n\nReview these source tickets for concrete repairs; open a new ticket only after human triage:\n\n");
    let tickets = rows
        .iter()
        .map(|row| format!("{}/{}", row.connection_id, row.slug))
        .collect::<BTreeSet<_>>();
    for ticket in tickets.iter().take(10) {
        out.push_str(&format!(
            "- [ ] `{ticket}` — proposed action: [human review required]\n"
        ));
    }
    if tickets.len() > 10 {
        out.push_str(&format!(
            "- {} additional source tickets omitted from this concise draft.\n",
            tickets.len() - 10
        ));
    }
    out.push('\n');
    out.push_str(&format!(
        "Proposed last-reviewed cursor: `{proposed_cursor}`.\n"
    ));
    out
}

/// Prepare or reuse a private draft. The review state advances only through
/// `accept_review`, so a repeated prepare is idempotent and preserves human edits.
pub fn prepare(
    dir: &Path,
    providers: &[String],
    rows: Vec<AiFeedbackRecord>,
    errors: &[(String, String)],
) -> Result<Option<PathBuf>> {
    if providers.is_empty() {
        bail!("select at least one provider connection");
    }
    private_dir(dir)?;
    let reviewed = read_state(&dir.join("reviewed.json"))?;
    let mut current = BTreeMap::new();
    let mut unique = BTreeMap::new();
    for row in rows {
        let key = source_key(&row);
        current.insert(key.clone(), fingerprint(&row)?);
        unique.insert(key, row);
    }
    let new_rows = unique
        .into_iter()
        .filter_map(|(key, row)| (reviewed.reviewed.get(&key) != current.get(&key)).then_some(row))
        .collect::<Vec<_>>();
    let failed = errors
        .iter()
        .map(|(provider, _)| provider.as_str())
        .collect::<BTreeSet<_>>();
    let removed = reviewed
        .reviewed
        .keys()
        .filter(|key| {
            let provider = key.split('\0').next().unwrap_or_default();
            providers.iter().any(|selected| selected == provider)
                && !failed.contains(provider)
                && !current.contains_key(*key)
        })
        .cloned()
        .collect::<Vec<_>>();
    let mut proposed = reviewed.reviewed.clone();
    proposed.extend(current);
    for key in &removed {
        proposed.remove(key);
    }
    let proposed_cursor = cursor(&proposed)?;
    let draft_path = dir.join("draft.md");
    let pending_path = dir.join("pending.json");
    let error_keys = errors
        .iter()
        .map(|(provider, error)| format!("{provider}:{}", hex(&Sha256::digest(error.as_bytes()))))
        .collect::<Vec<_>>();
    if let Ok(bytes) = fs::read(&pending_path) {
        let pending: PendingReview = serde_json::from_slice(&bytes)?;
        if pending.cursor == proposed_cursor
            && pending.providers == providers
            && pending.errors == error_keys
            && draft_path.is_file()
        {
            return Ok(Some(draft_path));
        }
        bail!(
            "a review draft is pending; accept or archive it before preparing a changed source set"
        );
    }
    if new_rows.is_empty() && removed.is_empty() {
        return Ok(None);
    }
    let pending = PendingReview {
        cursor: proposed_cursor.clone(),
        sources: proposed,
        providers: providers.to_vec(),
        errors: error_keys,
    };
    private_write(
        &draft_path,
        render_draft(&new_rows, providers, errors, &removed, &proposed_cursor).as_bytes(),
    )?;
    private_write(&pending_path, &serde_json::to_vec_pretty(&pending)?)?;
    Ok(Some(draft_path))
}

/// Explicit human acknowledgement only advances the local cursor. A maintainer must
/// separately edit and commit accepted repository guidance.
pub fn accept_review(dir: &Path) -> Result<String> {
    let pending_path = dir.join("pending.json");
    let pending: PendingReview =
        serde_json::from_slice(&fs::read(&pending_path).context("no prepared review draft")?)?;
    if !dir.join("draft.md").is_file() {
        bail!("the review draft is missing");
    }
    let reviewed = ReviewState {
        reviewed: pending.sources,
        cursor: Some(pending.cursor.clone()),
    };
    private_write(
        &dir.join("reviewed.json"),
        &serde_json::to_vec_pretty(&reviewed)?,
    )?;
    fs::remove_file(pending_path)?;
    Ok(pending.cursor)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{AttachmentActorRole, NoteActor};

    fn row(
        note: &str,
        rater: &str,
        rating: Option<AiFeedbackRating>,
        explanation: Option<&str>,
    ) -> AiFeedbackRecord {
        AiFeedbackRecord {
            connection_id: "local".into(),
            ticket_id: "ticket-1".into(),
            slug: "HS-ONE".into(),
            note_id: note.into(),
            target: "activity:run".into(),
            rating,
            explanation: explanation.map(str::to_owned),
            rater: Some(NoteActor {
                role: AttachmentActorRole::Human,
                id: Some(rater.into()),
            }),
            created_at: "2026-10-01T00:00:00Z".into(),
            edited_at: "2026-10-02T00:00:00Z".into(),
            legacy: false,
        }
    }

    #[test]
    fn drafts_deduplicate_correct_withdraw_and_wait_for_human_acceptance() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let providers = vec!["local".to_owned()];
        let first = row(
            "a",
            "rater-1",
            Some(AiFeedbackRating::NotHelpful),
            Some("Give shorter answers"),
        );
        let second = row(
            "b",
            "rater-2",
            Some(AiFeedbackRating::Helpful),
            Some("Give shorter answers"),
        );
        let draft = prepare(
            dir,
            &providers,
            vec![first.clone(), second.clone(), first.clone()],
            &[],
        )
        .unwrap()
        .unwrap();
        let text = fs::read_to_string(&draft).unwrap();
        assert!(text.contains("give shorter answers"));
        assert!(text.contains("Proposed action: consider `give shorter answers`"));
        assert!(text.contains("Scope: activity feedback"));
        assert!(text.contains("Conflicting ratings"));
        assert!(text.contains("2 distinct notes (1 helpful, 1 not helpful)"));
        assert!(text.contains("local/HS-ONE#a"));
        fs::write(&draft, "human edited draft").unwrap();
        assert_eq!(
            prepare(dir, &providers, vec![first.clone(), second.clone()], &[]).unwrap(),
            Some(draft.clone())
        );
        assert_eq!(fs::read_to_string(&draft).unwrap(), "human edited draft");
        let accepted = accept_review(dir).unwrap();
        assert!(accepted.starts_with("sha256:"));
        assert!(
            prepare(dir, &providers, vec![first.clone(), second.clone()], &[])
                .unwrap()
                .is_none()
        );

        let mut corrected = first;
        corrected.rating = Some(AiFeedbackRating::Helpful);
        corrected.edited_at = "2026-10-03T00:00:00Z".into();
        let draft = prepare(
            dir,
            &providers,
            vec![corrected.clone(), second.clone()],
            &[],
        )
        .unwrap()
        .unwrap();
        let changed = fs::read_to_string(&draft).unwrap();
        assert!(changed.contains("below the independent-example threshold"));
        assert!(changed.contains("local/HS-ONE#a"));
        assert!(!changed.contains("local/HS-ONE#b"));
        assert!(
            prepare(dir, &providers, vec![second.clone()], &[]).is_err(),
            "pending edits must not be overwritten"
        );
        accept_review(dir).unwrap();

        corrected.rating = None;
        corrected.explanation = None;
        corrected.edited_at = "2026-10-04T00:00:00Z".into();
        let withdrawn = prepare(dir, &providers, vec![corrected, second.clone()], &[])
            .unwrap()
            .unwrap();
        assert!(
            fs::read_to_string(withdrawn)
                .unwrap()
                .contains("rating withdrawn")
        );
        accept_review(dir).unwrap();
        let removed = prepare(dir, &providers, vec![second], &[])
            .unwrap()
            .unwrap();
        assert!(
            fs::read_to_string(removed)
                .unwrap()
                .contains("Removed source")
        );
    }

    #[test]
    fn redacts_sensitive_text_and_marks_legacy_or_unreadable_sources_uncertain() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let providers = vec!["local".to_owned(), "github".to_owned()];
        let mut legacy = row(
            "old",
            "legacy",
            Some(AiFeedbackRating::Helpful),
            Some("contact me@example.com at /Users/alice/private with ghp_secret"),
        );
        legacy.legacy = true;
        let draft = prepare(
            dir,
            &providers,
            vec![legacy],
            &[(
                "github".into(),
                "ai_feedback unsupported at https://api.example.com?token=private".into(),
            )],
        )
        .unwrap()
        .unwrap();
        let text = fs::read_to_string(draft).unwrap();
        assert!(text.contains("Coverage gaps"));
        assert!(text.contains("legacy or redacted evidence needs triage"));
        assert!(!text.contains("me@example.com"));
        assert!(!text.contains("/Users/alice"));
        assert!(!text.contains("ghp_secret"));
        assert!(!text.contains("api.example.com"));
        assert!(
            !fs::read_to_string(dir.join("pending.json"))
                .unwrap()
                .contains("api.example.com")
        );
        assert_eq!(
            redact("email me@example.com token ghp_secret path /Users/alice"),
            "email [redacted] token [redacted] path [redacted]"
        );
    }

    #[test]
    fn a_provider_error_does_not_retract_reviewed_sources() {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path();
        let providers = vec!["local".to_owned()];
        let source = row(
            "a",
            "rater-1",
            Some(AiFeedbackRating::Helpful),
            Some("Keep the context"),
        );
        prepare(dir, &providers, vec![source.clone()], &[]).unwrap();
        accept_review(dir).unwrap();
        assert!(
            prepare(
                dir,
                &providers,
                vec![],
                &[("local".into(), "offline".into())]
            )
            .unwrap()
            .is_none()
        );
        assert!(
            prepare(dir, &providers, vec![source], &[])
                .unwrap()
                .is_none()
        );
        let removed = prepare(dir, &providers, vec![], &[]).unwrap().unwrap();
        assert!(
            fs::read_to_string(removed)
                .unwrap()
                .contains("Removed source")
        );
    }
}
