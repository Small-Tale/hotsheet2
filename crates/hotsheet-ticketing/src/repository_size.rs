//! Repository size report for a checkout's code repository and its linked ticket stores
//! (HS2-QF44T7).
//!
//! GitHub warns about repositories over 5 GB and refuses pushes past 10 GB. Ticket stores
//! grow mostly through committed attachments, so the report measures, per repository:
//!
//! - the git object database (`git count-objects -v`: loose plus packed bytes), which is
//!   what a push carries and what hosting limits apply to; and
//! - for a git ticket store, the on-disk `attachments/` payload in the working tree.
//!
//! Thresholds are defined once here ([`WARNING_BYTES`], [`CRITICAL_BYTES`]) and every
//! entry carries its computed [`SizeLevel`], so clients display the level and never
//! recompute it. Measurement is bounded: `count-objects` reads pack metadata only, and the
//! attachment walk stops after [`MAX_ATTACHMENT_ENTRIES`] directory entries and reports
//! itself as truncated. Every function here is blocking; servers call it off the async
//! worker threads.

use crate::checkouts::Checkout;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// GitHub's soft repository-size limit: at or above this a repository is a `warning`.
pub const WARNING_BYTES: u64 = 5_000_000_000;
/// GitHub's hard repository-size limit: at or above this a repository is `critical`.
pub const CRITICAL_BYTES: u64 = 10_000_000_000;
/// Upper bound on directory entries visited while summing one store's attachments.
pub const MAX_ATTACHMENT_ENTRIES: usize = 200_000;

/// How close a repository is to the hosting limits.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum SizeLevel {
    #[default]
    Ok,
    Warning,
    Critical,
}

impl SizeLevel {
    /// The level for a repository whose git objects take `bytes`.
    #[must_use]
    pub fn for_bytes(bytes: u64) -> Self {
        if bytes >= CRITICAL_BYTES {
            Self::Critical
        } else if bytes >= WARNING_BYTES {
            Self::Warning
        } else {
            Self::Ok
        }
    }
}

/// The thresholds a report was computed with, echoed so clients can name them.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct SizeThresholds {
    pub warning_bytes: u64,
    pub critical_bytes: u64,
}

impl Default for SizeThresholds {
    fn default() -> Self {
        Self {
            warning_bytes: WARNING_BYTES,
            critical_bytes: CRITICAL_BYTES,
        }
    }
}

/// Which repository an entry describes.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RepositoryKind {
    Code,
    TicketStore,
}

/// Whether an entry could be measured.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MeasurementStatus {
    /// Git objects (and attachments, for a store) were measured.
    Measured,
    /// The path exists but is not inside a git repository.
    NotRepository,
    /// The source's provider has no local repository to measure (GitHub Issues, Jira, ...).
    Unsupported,
    /// The path is missing or git failed; `detail` says why.
    Unavailable,
}

/// `git count-objects -v`, in bytes.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitObjectSize {
    pub loose_objects: u64,
    pub loose_bytes: u64,
    pub packed_objects: u64,
    pub packs: u64,
    pub packed_bytes: u64,
    /// Loose plus packed: the object database a push carries.
    pub total_bytes: u64,
}

/// The on-disk size of a ticket store's `attachments/` directory.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttachmentSize {
    pub bytes: u64,
    pub files: u64,
    /// The walk stopped at [`MAX_ATTACHMENT_ENTRIES`]; `bytes` is a lower bound.
    pub truncated: bool,
}

/// One measured repository.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositorySizeEntry {
    pub kind: RepositoryKind,
    /// The ticket source's connection id; absent for the code repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub connection_id: Option<String>,
    /// The ticket source's provider (`git`, `github`, ...); absent for the code repository.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    /// A short display name: the checkout alias or the store directory name.
    pub label: String,
    /// The measured local path, or the provider locator for an unsupported source.
    pub locator: String,
    pub status: MeasurementStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<GitObjectSize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<AttachmentSize>,
    /// A ticket store kept inside the code repository shares its object database, so its
    /// git size is the code repository's, not an additional repository.
    #[serde(default)]
    pub shared_with_code: bool,
    pub level: SizeLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `GET /checkouts/{reference}/repository/size` and `hotsheet-cli checkout repo-size`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RepositorySizeReport {
    pub checkout_id: String,
    pub thresholds: SizeThresholds,
    /// The highest level among the entries.
    pub level: SizeLevel,
    pub entries: Vec<RepositorySizeEntry>,
}

/// Parse `git count-objects -v` output (sizes are KiB) into bytes.
#[must_use]
pub fn parse_count_objects(output: &str) -> GitObjectSize {
    let mut size = GitObjectSize::default();
    for line in output.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let Ok(value) = value.trim().parse::<u64>() else {
            continue;
        };
        match key.trim() {
            "count" => size.loose_objects = value,
            "size" => size.loose_bytes = value.saturating_mul(1024),
            "in-pack" => size.packed_objects = value,
            "packs" => size.packs = value,
            "size-pack" => size.packed_bytes = value.saturating_mul(1024),
            _ => {}
        }
    }
    size.total_bytes = size.loose_bytes.saturating_add(size.packed_bytes);
    size
}

enum GitProbe {
    Measured {
        toplevel: PathBuf,
        size: GitObjectSize,
    },
    NotRepository,
    Unavailable(String),
}

fn probe_git(path: &Path) -> GitProbe {
    match std::fs::metadata(path) {
        Ok(metadata) if metadata.is_dir() => {}
        Ok(_) => return GitProbe::Unavailable("The path is not a directory.".into()),
        Err(error) => return GitProbe::Unavailable(format!("The path is not available: {error}")),
    }
    let toplevel = match crate::git::command_in(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
    {
        Ok(output) if output.status.success() => {
            let text = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            let path = PathBuf::from(text);
            path.canonicalize().unwrap_or(path)
        }
        Ok(_) => return GitProbe::NotRepository,
        Err(error) => return GitProbe::Unavailable(format!("Could not run Git: {error}")),
    };
    match crate::git::command_in(path)
        .args(["count-objects", "-v"])
        .output()
    {
        Ok(output) if output.status.success() => GitProbe::Measured {
            toplevel,
            size: parse_count_objects(&String::from_utf8_lossy(&output.stdout)),
        },
        Ok(output) => GitProbe::Unavailable(format!(
            "Git could not count objects: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )),
        Err(error) => GitProbe::Unavailable(format!("Could not run Git: {error}")),
    }
}

/// Sum the regular files under `dir` without following symlinks, visiting at most
/// `limit` directory entries. A missing directory is an empty payload.
#[must_use]
pub fn directory_size(dir: &Path, limit: usize) -> AttachmentSize {
    let mut size = AttachmentSize::default();
    let mut visited = 0usize;
    let mut pending = vec![dir.to_path_buf()];
    while let Some(next) = pending.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            if visited >= limit {
                size.truncated = true;
                return size;
            }
            visited += 1;
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file()
                && let Ok(metadata) = entry.metadata()
            {
                size.bytes = size.bytes.saturating_add(metadata.len());
                size.files += 1;
            }
        }
    }
    size
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

fn measure(
    kind: RepositoryKind,
    label: String,
    path: &Path,
    code_toplevel: Option<&Path>,
) -> (RepositorySizeEntry, Option<PathBuf>) {
    let mut entry = RepositorySizeEntry {
        kind,
        connection_id: None,
        provider: None,
        label,
        locator: path.display().to_string(),
        status: MeasurementStatus::Measured,
        git: None,
        attachments: None,
        shared_with_code: false,
        level: SizeLevel::Ok,
        detail: None,
    };
    let mut toplevel_out = None;
    match probe_git(path) {
        GitProbe::Measured { toplevel, size } => {
            entry.level = SizeLevel::for_bytes(size.total_bytes);
            entry.git = Some(size);
            entry.shared_with_code = kind == RepositoryKind::TicketStore
                && code_toplevel.is_some_and(|code| code == toplevel);
            toplevel_out = Some(toplevel);
        }
        GitProbe::NotRepository => {
            entry.status = MeasurementStatus::NotRepository;
            entry.detail = Some("The directory is not inside a Git repository.".into());
        }
        GitProbe::Unavailable(detail) => {
            entry.status = MeasurementStatus::Unavailable;
            entry.detail = Some(detail);
        }
    }
    if kind == RepositoryKind::TicketStore && entry.status != MeasurementStatus::Unavailable {
        entry.attachments = Some(directory_size(
            &path.join("attachments"),
            MAX_ATTACHMENT_ENTRIES,
        ));
    }
    (entry, toplevel_out)
}

/// Measure a checkout's code repository and every linked ticket source. Blocking.
#[must_use]
pub fn report(checkout: &Checkout) -> RepositorySizeReport {
    let root = PathBuf::from(&checkout.root);
    let label = if checkout.alias.trim().is_empty() {
        display_name(&root)
    } else {
        checkout.alias.clone()
    };
    let (code, code_toplevel) = measure(RepositoryKind::Code, label, &root, None);
    let mut entries = vec![code];
    for source in &checkout.sources {
        let mut entry = if source.provider == "git" {
            let path = PathBuf::from(&source.locator);
            measure(
                RepositoryKind::TicketStore,
                display_name(&path),
                &path,
                code_toplevel.as_deref(),
            )
            .0
        } else {
            RepositorySizeEntry {
                kind: RepositoryKind::TicketStore,
                connection_id: None,
                provider: None,
                label: source.connection_id.clone(),
                locator: source.locator.clone(),
                status: MeasurementStatus::Unsupported,
                git: None,
                attachments: None,
                shared_with_code: false,
                level: SizeLevel::Ok,
                detail: Some(format!(
                    "This {} source is hosted by its provider, so there is no local repository to measure.",
                    source.provider
                )),
            }
        };
        entry.connection_id = Some(source.connection_id.clone());
        entry.provider = Some(source.provider.clone());
        entries.push(entry);
    }
    RepositorySizeReport {
        checkout_id: checkout.id.clone(),
        thresholds: SizeThresholds::default(),
        level: entries
            .iter()
            .map(|entry| entry.level)
            .max()
            .unwrap_or_default(),
        entries,
    }
}

/// The git object size of the repository holding `path`, or `None` when it cannot be
/// measured (not a repository, Git unavailable). Cheap: one `git count-objects -v`, no
/// attachment walk, so `sync` can compare the level before and after a push (HS2-P1GYWH).
#[must_use]
pub fn git_object_size(path: &Path) -> Option<GitObjectSize> {
    match probe_git(path) {
        GitProbe::Measured { size, .. } => Some(size),
        GitProbe::NotRepository | GitProbe::Unavailable(_) => None,
    }
}

/// One line naming an entry over a GitHub limit, or `None` for an entry within limits.
#[must_use]
pub fn limit_message(label: &str, bytes: u64, level: SizeLevel) -> Option<String> {
    match level {
        SizeLevel::Ok => None,
        SizeLevel::Warning => Some(format!(
            "{label} is {} of git objects, over GitHub's {} soft limit; large attachments are the usual cause",
            format_bytes(bytes),
            format_bytes(WARNING_BYTES)
        )),
        SizeLevel::Critical => Some(format!(
            "{label} is {} of git objects, over GitHub's {} hard limit; GitHub rejects pushes this large",
            format_bytes(bytes),
            format_bytes(CRITICAL_BYTES)
        )),
    }
}

/// The attention lines for every entry of `report` over a GitHub limit (for `doctor`).
#[must_use]
pub fn limit_messages(report: &RepositorySizeReport) -> Vec<(SizeLevel, String)> {
    report
        .entries
        .iter()
        .filter_map(|entry| {
            let kind = match entry.kind {
                RepositoryKind::Code => "code repository",
                RepositoryKind::TicketStore => "ticket store",
            };
            limit_message(
                &format!("{kind} {}", entry.label),
                entry.git.map_or(0, |git| git.total_bytes),
                entry.level,
            )
            .map(|line| (entry.level, line))
        })
        .collect()
}

/// The warning to print after a sync that moved a repository from `before` to a higher
/// level (HS2-P1GYWH). Staying at, or dropping below, a level already reported says nothing,
/// so a repository over a limit is not re-announced on every sync.
#[must_use]
pub fn crossing_message(label: &str, before: SizeLevel, after: GitObjectSize) -> Option<String> {
    let level = SizeLevel::for_bytes(after.total_bytes);
    (level > before)
        .then(|| limit_message(label, after.total_bytes, level))
        .flatten()
}

/// A byte count in decimal units (GitHub states its limits in GB), e.g. `4.2 GB`.
#[must_use]
pub fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "kB", "MB", "GB", "TB"];
    if bytes < 1000 {
        return format!("{bytes} B");
    }
    #[allow(clippy::cast_precision_loss)]
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1000.0 && unit < UNITS.len() - 1 {
        value /= 1000.0;
        unit += 1;
    }
    let number = if value >= 100.0 {
        format!("{value:.0}")
    } else {
        format!("{value:.1}")
    };
    format!("{} {}", number.trim_end_matches(".0"), UNITS[unit])
}

/// A plain-text table of the report for the CLI.
#[must_use]
pub fn render_text(report: &RepositorySizeReport) -> String {
    use std::fmt::Write as _;
    let mut out = String::new();
    for entry in &report.entries {
        let kind = match entry.kind {
            RepositoryKind::Code => "code",
            RepositoryKind::TicketStore => "tickets",
        };
        let _ = write!(out, "{kind:<8} {:<24} ", entry.label);
        match (entry.status, entry.git) {
            (MeasurementStatus::Measured, Some(git)) => {
                let _ = write!(out, "git {:>9}", format_bytes(git.total_bytes));
            }
            (MeasurementStatus::NotRepository, _) => out.push_str("not a git repository"),
            (MeasurementStatus::Unsupported, _) => out.push_str("unsupported"),
            _ => out.push_str("unavailable"),
        }
        if let Some(attachments) = entry.attachments {
            let _ = write!(
                out,
                "  attachments {}{} in {} files",
                format_bytes(attachments.bytes),
                if attachments.truncated { "+" } else { "" },
                attachments.files
            );
        }
        if entry.shared_with_code {
            out.push_str("  (shares the code repository)");
        }
        match entry.level {
            SizeLevel::Ok => {}
            SizeLevel::Warning => out.push_str("  WARNING: over the 5 GB GitHub soft limit"),
            SizeLevel::Critical => out.push_str("  CRITICAL: over the 10 GB GitHub hard limit"),
        }
        if let Some(detail) = &entry.detail
            && entry.status != MeasurementStatus::Measured
        {
            let _ = write!(out, "  ({detail})");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checkouts::TicketSource;

    fn git(dir: &Path, args: &[&str]) {
        let status = crate::git::command_in(dir)
            .args(args)
            .env("GIT_AUTHOR_NAME", "t")
            .env("GIT_AUTHOR_EMAIL", "t@example.com")
            .env("GIT_COMMITTER_NAME", "t")
            .env("GIT_COMMITTER_EMAIL", "t@example.com")
            .output()
            .expect("git runs");
        assert!(status.status.success(), "git {args:?}: {status:?}");
    }

    fn checkout(root: &Path, sources: Vec<TicketSource>) -> Checkout {
        Checkout {
            id: "demo-1234".into(),
            root: root.display().to_string(),
            alias: "demo".into(),
            repository: None,
            stores: Vec::new(),
            sources,
            store_instance_ids: Default::default(),
            unverified_store_sources: Default::default(),
            source_colors: Default::default(),
            default_source: None,
            default_source_cleared: false,
        }
    }

    fn objects(total_bytes: u64) -> GitObjectSize {
        GitObjectSize {
            total_bytes,
            ..GitObjectSize::default()
        }
    }

    #[test]
    fn crossing_messages_fire_only_when_a_sync_raises_the_level() {
        let cases = [
            (SizeLevel::Ok, 1_000, None),
            (SizeLevel::Ok, WARNING_BYTES, Some("soft limit")),
            (SizeLevel::Ok, CRITICAL_BYTES, Some("hard limit")),
            (SizeLevel::Warning, WARNING_BYTES + 1, None),
            (SizeLevel::Warning, CRITICAL_BYTES, Some("hard limit")),
            (SizeLevel::Critical, CRITICAL_BYTES * 2, None),
            (SizeLevel::Critical, 1_000, None),
            (SizeLevel::Warning, 1_000, None),
        ];
        for (before, after, expected) in cases {
            let message = crossing_message("ticket store tickets", before, objects(after));
            match expected {
                None => assert_eq!(message, None, "{before:?} -> {after}"),
                Some(limit) => {
                    let message = message.unwrap();
                    assert!(message.contains(limit), "{message}");
                    assert!(message.starts_with("ticket store tickets is "), "{message}");
                }
            }
        }
    }

    #[test]
    fn limit_messages_name_each_entry_over_a_limit_and_measure_a_store_alone() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(git_object_size(dir.path()), None);
        git(dir.path(), &["init", "-q"]);
        std::fs::write(dir.path().join("a.txt"), "a\n").unwrap();
        git(dir.path(), &["add", "a.txt"]);
        git(dir.path(), &["commit", "-qm", "a"]);
        let measured = git_object_size(dir.path()).unwrap();
        assert!(measured.total_bytes > 0 || measured.loose_objects > 0);
        let mut report = report(&checkout(dir.path(), Vec::new()));
        assert!(limit_messages(&report).is_empty());
        report.entries[0].level = SizeLevel::Critical;
        report.entries[0].git = Some(objects(CRITICAL_BYTES));
        let lines = limit_messages(&report);
        assert_eq!(lines.len(), 1);
        let (level, line) = &lines[0];
        assert_eq!(*level, SizeLevel::Critical);
        assert!(line.starts_with("code repository demo is 10 GB"), "{line}");
        assert!(line.contains("rejects pushes"), "{line}");
    }

    #[test]
    fn parses_count_objects_kib_into_bytes() {
        let size = parse_count_objects(
            "count: 3\nsize: 12\nin-pack: 48384\npacks: 5\nsize-pack: 62048\nprune-packable: 0\ngarbage: 1\nsize-garbage: 0\n",
        );
        assert_eq!(size.loose_objects, 3);
        assert_eq!(size.loose_bytes, 12 * 1024);
        assert_eq!(size.packed_objects, 48384);
        assert_eq!(size.packs, 5);
        assert_eq!(size.packed_bytes, 62048 * 1024);
        assert_eq!(size.total_bytes, (12 + 62048) * 1024);
        assert_eq!(
            parse_count_objects("junk\nsize: x\n"),
            GitObjectSize::default()
        );
    }

    #[test]
    fn levels_cross_both_thresholds_inclusively() {
        assert_eq!(SizeLevel::for_bytes(0), SizeLevel::Ok);
        assert_eq!(SizeLevel::for_bytes(WARNING_BYTES - 1), SizeLevel::Ok);
        assert_eq!(SizeLevel::for_bytes(WARNING_BYTES), SizeLevel::Warning);
        assert_eq!(SizeLevel::for_bytes(CRITICAL_BYTES - 1), SizeLevel::Warning);
        assert_eq!(SizeLevel::for_bytes(CRITICAL_BYTES), SizeLevel::Critical);
        assert!(SizeLevel::Critical > SizeLevel::Warning && SizeLevel::Warning > SizeLevel::Ok);
    }

    #[test]
    fn formats_decimal_units() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(999), "999 B");
        assert_eq!(format_bytes(1_500), "1.5 kB");
        assert_eq!(format_bytes(250_000_000), "250 MB");
        assert_eq!(format_bytes(WARNING_BYTES), "5 GB");
        assert_eq!(format_bytes(60_600_000), "60.6 MB");
        assert_eq!(format_bytes(12_340_000_000_000_000), "12340 TB");
    }

    #[test]
    fn directory_size_sums_nested_files_and_stops_at_its_bound() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            directory_size(&dir.path().join("missing"), 10),
            AttachmentSize::default()
        );
        std::fs::create_dir_all(dir.path().join("a/b")).unwrap();
        std::fs::write(dir.path().join("a/one.bin"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.path().join("a/b/two.bin"), vec![0u8; 32]).unwrap();
        let size = directory_size(dir.path(), 100);
        assert_eq!((size.bytes, size.files, size.truncated), (42, 2, false));
        let bounded = directory_size(dir.path(), 1);
        assert!(bounded.truncated);
        assert_eq!(bounded.files, 0);
    }

    #[test]
    fn reports_code_store_shared_and_unsupported_sources() {
        let code = tempfile::tempdir().unwrap();
        git(code.path(), &["init", "-q"]);
        std::fs::write(code.path().join("readme.md"), "hello").unwrap();
        git(code.path(), &["add", "."]);
        git(code.path(), &["commit", "-q", "-m", "init"]);
        // An embedded store inside the code repository.
        let embedded = code.path().join(".hotsheet");
        std::fs::create_dir_all(embedded.join("attachments/01ABC")).unwrap();
        std::fs::write(embedded.join("attachments/01ABC/shot.png"), vec![1u8; 2048]).unwrap();
        // A standalone store in its own repository.
        let standalone = tempfile::tempdir().unwrap();
        git(standalone.path(), &["init", "-q"]);
        // A plain directory and a missing one.
        let plain = tempfile::tempdir().unwrap();
        let missing = plain.path().join("gone");
        let report = report(&checkout(
            code.path(),
            vec![
                TicketSource::git(&embedded),
                TicketSource::git(standalone.path()),
                TicketSource::git(plain.path()),
                TicketSource {
                    connection_id: "missing".into(),
                    provider: "git".into(),
                    locator: missing.display().to_string(),
                },
                TicketSource {
                    connection_id: "gh".into(),
                    provider: "github".into(),
                    locator: "Small-Tale/hotsheet2".into(),
                },
            ],
        ));
        assert_eq!(report.checkout_id, "demo-1234");
        assert_eq!(report.level, SizeLevel::Ok);
        assert_eq!(report.thresholds, SizeThresholds::default());
        let [code_entry, embedded, standalone, plain, missing, github] = report.entries.as_slice()
        else {
            panic!("six entries: {:?}", report.entries);
        };
        assert_eq!(code_entry.kind, RepositoryKind::Code);
        assert_eq!(code_entry.label, "demo");
        assert_eq!(code_entry.status, MeasurementStatus::Measured);
        assert!(code_entry.git.unwrap().total_bytes > 0);
        assert!(code_entry.attachments.is_none());
        assert!(embedded.shared_with_code);
        assert_eq!(embedded.attachments.unwrap().bytes, 2048);
        assert_eq!(embedded.provider.as_deref(), Some("git"));
        assert!(!standalone.shared_with_code);
        assert_eq!(standalone.status, MeasurementStatus::Measured);
        assert_eq!(standalone.attachments.unwrap(), AttachmentSize::default());
        assert_eq!(plain.status, MeasurementStatus::NotRepository);
        assert!(plain.git.is_none());
        assert_eq!(missing.status, MeasurementStatus::Unavailable);
        assert!(missing.attachments.is_none());
        assert_eq!(github.status, MeasurementStatus::Unsupported);
        assert_eq!(github.connection_id.as_deref(), Some("gh"));
        assert!(github.detail.as_deref().unwrap().contains("github"));
        let text = render_text(&report);
        assert!(text.contains("code     demo"), "{text}");
        assert!(text.contains("shares the code repository"), "{text}");
        assert!(text.contains("not a git repository"), "{text}");
        assert!(text.contains("unsupported"), "{text}");
        assert!(text.contains("unavailable"), "{text}");
    }

    #[test]
    fn text_names_the_github_limit_for_warning_and_critical_entries() {
        let mut report = RepositorySizeReport {
            checkout_id: "x".into(),
            thresholds: SizeThresholds::default(),
            level: SizeLevel::Critical,
            entries: Vec::new(),
        };
        for (bytes, level) in [
            (WARNING_BYTES, SizeLevel::Warning),
            (CRITICAL_BYTES, SizeLevel::Critical),
        ] {
            report.entries.push(RepositorySizeEntry {
                kind: RepositoryKind::TicketStore,
                connection_id: Some("c".into()),
                provider: Some("git".into()),
                label: "tickets".into(),
                locator: "/x".into(),
                status: MeasurementStatus::Measured,
                git: Some(GitObjectSize {
                    total_bytes: bytes,
                    ..GitObjectSize::default()
                }),
                attachments: Some(AttachmentSize {
                    bytes: 3,
                    files: 1,
                    truncated: true,
                }),
                shared_with_code: false,
                level,
                detail: None,
            });
        }
        let text = render_text(&report);
        assert!(
            text.contains("WARNING: over the 5 GB GitHub soft limit"),
            "{text}"
        );
        assert!(
            text.contains("CRITICAL: over the 10 GB GitHub hard limit"),
            "{text}"
        );
        assert!(text.contains("attachments 3 B+ in 1 files"), "{text}");
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["level"], "critical");
        assert_eq!(json["entries"][0]["level"], "warning");
        assert_eq!(json["entries"][0]["kind"], "ticket_store");
        assert_eq!(json["thresholds"]["warning_bytes"], WARNING_BYTES);
    }
}
