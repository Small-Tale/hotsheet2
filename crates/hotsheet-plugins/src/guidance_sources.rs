//! Pinned, project-owned rule and skill sources. Registration is the only network-capable
//! operation; ordinary setup reads the authenticated manifest snapshot kept with the project.

use base64::Engine;
use hotsheet_ticketing::FileLock;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CONFIG_PATH: &str = ".hotsheet2/guidance-sources.json";
pub const DEFAULT_REPOSITORY: &str = "https://github.com/Small-Tale/ai-playbook.git";

fn lock_config(project: &Path) -> Result<FileLock, GuidanceSourceError> {
    let directory = project.join(".hotsheet2");
    fs::create_dir_all(&directory)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(directory.join("guidance-sources.lock"))?;
    Ok(FileLock::acquire(file)?)
}

#[derive(Debug, thiserror::Error)]
pub enum GuidanceSourceError {
    #[error("guidance source I/O: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid guidance source: {0}")]
    Invalid(String),
    #[error("git failed: {0}")]
    Git(String),
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuidanceConfig {
    pub schema_version: u8,
    #[serde(default)]
    pub explicit_selection: bool,
    #[serde(default)]
    pub sources: Vec<GuidanceSource>,
}

impl Default for GuidanceConfig {
    fn default() -> Self {
        Self {
            schema_version: 1,
            explicit_selection: false,
            sources: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct GuidanceSource {
    pub id: String,
    /// A batch source's stable user-facing identity; absent on legacy one-ref sources.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub repository: String,
    pub requested_ref: String,
    pub commit: String,
    pub rules: Vec<SourceRule>,
    #[serde(default)]
    pub skills: Vec<SourceSkill>,
    #[serde(default)]
    pub selected_rules: Vec<String>,
    #[serde(default)]
    pub selected_skills: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchSourceRequest {
    pub id: String,
    pub repository: String,
    pub refs: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReviewedPin {
    pub id: String,
    pub group_id: String,
    pub repository: String,
    pub requested_ref: String,
    pub commit: String,
    pub rules: usize,
    pub skills: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct BatchReview {
    pub requests: Vec<BatchSourceRequest>,
    pub pins: Vec<ReviewedPin>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRule {
    pub name: String,
    pub version: u64,
    pub sha256: String,
    pub description: String,
    #[serde(default)]
    pub owner: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    pub body: String,
    #[serde(default)]
    pub known_bodies: Vec<String>,
    /// Optional lower bound declared by the source manifest (HS2-63D6F9).
    #[serde(default)]
    pub minimum_hotsheet_version: Option<String>,
    /// Verified historic offerings retained independently of the source's current pin.
    #[serde(default)]
    pub historical_versions: Vec<SourceRuleRevision>,
    /// None means the current pinned offering. A digest chooses one exact historic body.
    #[serde(default)]
    pub selected_sha256: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceRuleRevision {
    pub version: u64,
    pub sha256: String,
    pub description: String,
    pub body: String,
    pub commit: String,
    #[serde(default)]
    pub minimum_hotsheet_version: Option<String>,
}

pub struct ChosenRule<'a> {
    pub version: u64,
    pub sha256: &'a str,
    pub description: &'a str,
    pub body: &'a str,
    pub commit: &'a str,
    pub minimum_hotsheet_version: Option<&'a str>,
}

pub fn chosen_rule<'a>(source: &'a GuidanceSource, rule: &'a SourceRule) -> ChosenRule<'a> {
    if let Some(history) = rule
        .historical_versions
        .iter()
        .find(|history| Some(history.sha256.as_str()) == rule.selected_sha256.as_deref())
    {
        ChosenRule {
            version: history.version,
            sha256: &history.sha256,
            description: &history.description,
            body: &history.body,
            commit: &history.commit,
            minimum_hotsheet_version: history.minimum_hotsheet_version.as_deref(),
        }
    } else {
        ChosenRule {
            version: rule.version,
            sha256: &rule.sha256,
            description: &rule.description,
            body: &rule.body,
            commit: &source.commit,
            minimum_hotsheet_version: rule.minimum_hotsheet_version.as_deref(),
        }
    }
}

pub fn require_compatible(choice: &ChosenRule<'_>) -> Result<(), GuidanceSourceError> {
    let Some(minimum) = choice.minimum_hotsheet_version else {
        return Ok(());
    };
    let required = semver::Version::parse(minimum).map_err(|_| {
        GuidanceSourceError::Invalid("invalid Hot Sheet compatibility version".into())
    })?;
    let current = semver::Version::parse(env!("CARGO_PKG_VERSION"))
        .map_err(|_| GuidanceSourceError::Invalid("invalid running Hot Sheet version".into()))?;
    if current < required {
        return Err(GuidanceSourceError::Invalid(format!(
            "rule requires Hot Sheet {required} or later (running {current})"
        )));
    }
    Ok(())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSkill {
    pub name: String,
    pub files: Vec<SourceSkillFile>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceSkillFile {
    /// Relative to `skills/<name>/` in the source and the installed skill directory.
    pub path: String,
    pub sha256: String,
    pub base64: String,
    #[serde(default)]
    pub known_base64: Vec<String>,
}

pub fn installed_version(project: &Path, name: &str) -> Option<u64> {
    let text = fs::read_to_string(project.join("AGENTS.md")).ok()?;
    let section = name.strip_prefix("hotsheet-").unwrap_or(name);
    let marker = if name == "hotsheet-workflow" {
        "<!-- hotsheet-instructions-version: ".to_owned()
    } else {
        format!("<!-- hotsheet:begin section={section} v=")
    };
    text.lines().find_map(|line| {
        line.trim()
            .strip_prefix(&marker)
            .and_then(|rest| rest.strip_suffix(" -->"))
            .and_then(|value| value.parse().ok())
    })
}

#[derive(Deserialize)]
struct Manifest {
    schema_version: u8,
    rules: Vec<ManifestRule>,
}

#[derive(Deserialize)]
struct ManifestRule {
    name: String,
    path: String,
    version: u64,
    sha256: String,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    minimum_hotsheet_version: Option<String>,
}

fn slug(value: &str) -> bool {
    let mut parts = value.split('-');
    parts.next().is_some_and(|part| {
        !part.is_empty()
            && part.as_bytes()[0].is_ascii_lowercase()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    }) && parts.all(|part| {
        !part.is_empty()
            && part
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
    })
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn rule_marker(name: &str, version: u64) -> String {
    if name == "hotsheet-workflow" {
        format!("<!-- hotsheet-instructions-version: {version} -->")
    } else {
        format!(
            "<!-- hotsheet:begin section={} v={version} -->",
            name.strip_prefix("hotsheet-").unwrap_or(name)
        )
    }
}

fn valid_minimum_version(minimum: Option<&str>) -> bool {
    minimum.is_none_or(|value| semver::Version::parse(value).is_ok())
}

fn guidance_process_context(
    purpose: hotsheet_process_journal::Purpose,
) -> hotsheet_process_journal::LaunchContext {
    hotsheet_process_journal::LaunchContext {
        journal: hotsheet_process_journal::ProcessJournal::at(crate::hotsheet_home()),
        supervisor: "hotsheet",
        purpose,
        project_id: None,
        session_id: None,
        binary_revision: option_env!("HOT_SHEET_BUILD_REVISION").map(str::to_owned),
    }
}

fn git(repository: &Path, args: &[&str]) -> Result<Vec<u8>, GuidanceSourceError> {
    let mut command = hotsheet_ticketing::git::command_in(repository);
    command.args(args);
    let output = guidance_process_context(hotsheet_process_journal::Purpose::GuidanceGit)
        .output(&mut command)?;
    if !output.status.success() {
        return Err(GuidanceSourceError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok(output.stdout)
}

fn source_checkout(
    repository: &str,
) -> Result<(PathBuf, Option<tempfile::TempDir>), GuidanceSourceError> {
    let local = Path::new(repository);
    if local.is_dir() {
        return Ok((local.canonicalize()?, None));
    }
    if !(repository.starts_with("https://")
        || repository.starts_with("ssh://")
        || repository.starts_with("git@"))
    {
        return Err(GuidanceSourceError::Invalid(
            "repository must be a local Git directory or a Git URL".into(),
        ));
    }
    let temporary = tempfile::tempdir()?;
    let checkout = temporary.path().join("source");
    let mut command = hotsheet_ticketing::git::command();
    command
        .args(["clone", "--quiet", "--no-checkout", "--"])
        .arg(repository)
        .arg(&checkout);
    let output = guidance_process_context(hotsheet_process_journal::Purpose::GuidanceGit)
        .output(&mut command)?;
    if !output.status.success() {
        return Err(GuidanceSourceError::Git(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    Ok((checkout, Some(temporary)))
}

fn git_file(repository: &Path, commit: &str, path: &str) -> Result<Vec<u8>, GuidanceSourceError> {
    git(repository, &["show", &format!("{commit}:{path}")])
}

pub(crate) fn split_rule_body(file: &str) -> Result<(&str, String), GuidanceSourceError> {
    let rest = file
        .strip_prefix("---\n")
        .and_then(|text| text.split_once("\n---\n\n"))
        .ok_or_else(|| GuidanceSourceError::Invalid("rule frontmatter missing".into()))?;
    let description = rest
        .0
        .lines()
        .find_map(|line| line.strip_prefix("description: \""))
        .and_then(|text| text.strip_suffix('"'))
        .ok_or_else(|| GuidanceSourceError::Invalid("rule description missing".into()))?;
    Ok((rest.1, description.to_owned()))
}

/// Resolve one tag or commit now, verify every offered file, and keep its exact bytes in
/// project configuration so later setup/refresh does not need a source checkout or network.
pub fn inspect_source(
    id: &str,
    repository: &str,
    requested_ref: &str,
) -> Result<GuidanceSource, GuidanceSourceError> {
    if !slug(id) || requested_ref.is_empty() || requested_ref.starts_with('-') {
        return Err(GuidanceSourceError::Invalid(
            "invalid source id or ref".into(),
        ));
    }
    let (checkout, _temporary) = source_checkout(repository)?;
    let commit = String::from_utf8(git(
        &checkout,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{requested_ref}^{{commit}}"),
        ],
    )?)
    .map_err(|_| GuidanceSourceError::Invalid("non-UTF-8 commit".into()))?
    .trim()
    .to_owned();
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(GuidanceSourceError::Invalid(
            "source ref did not resolve to a full commit".into(),
        ));
    }
    let manifest: Manifest =
        serde_json::from_slice(&git_file(&checkout, &commit, "rules-manifest.json")?).map_err(
            |error| GuidanceSourceError::Invalid(format!("invalid rules manifest: {error}")),
        )?;
    if manifest.schema_version != 1 {
        return Err(GuidanceSourceError::Invalid(
            "unsupported rules manifest version".into(),
        ));
    }
    let mut names = HashSet::new();
    let mut rules = Vec::new();
    for entry in manifest.rules {
        if !slug(&entry.name)
            || entry.path != format!("rules/{}.md", entry.name)
            || !names.insert(entry.name.clone())
            || entry
                .owner
                .as_deref()
                .is_some_and(|owner| !slug(owner) || !entry.name.starts_with(&format!("{owner}-")))
            || entry.tags.iter().any(|tag| !slug(tag))
        {
            return Err(GuidanceSourceError::Invalid(
                "unsafe or duplicate rule manifest entry".into(),
            ));
        }
        let file = String::from_utf8(git_file(&checkout, &commit, &entry.path)?)
            .map_err(|_| GuidanceSourceError::Invalid("non-UTF-8 rule".into()))?;
        let (body, description) = split_rule_body(&file)?;
        if digest(body.as_bytes()) != entry.sha256 {
            return Err(GuidanceSourceError::Invalid(format!(
                "rule hash mismatch: {}",
                entry.name
            )));
        }
        if !body.starts_with(&rule_marker(&entry.name, entry.version))
            || !valid_minimum_version(entry.minimum_hotsheet_version.as_deref())
        {
            return Err(GuidanceSourceError::Invalid(format!(
                "rule version or compatibility mismatch: {}",
                entry.name
            )));
        }
        rules.push(SourceRule {
            name: entry.name,
            version: entry.version,
            sha256: entry.sha256,
            description,
            owner: entry.owner,
            tags: entry.tags,
            body: body.to_owned(),
            known_bodies: Vec::new(),
            minimum_hotsheet_version: entry.minimum_hotsheet_version,
            historical_versions: Vec::new(),
            selected_sha256: None,
        });
    }
    let tree = String::from_utf8(git(
        &checkout,
        &["ls-tree", "-r", "--name-only", &commit, "skills/"],
    )?)
    .map_err(|_| GuidanceSourceError::Invalid("non-UTF-8 skill path".into()))?;
    let mut skills: Vec<SourceSkill> = Vec::new();
    for path in tree.lines() {
        let Some(rest) = path.strip_prefix("skills/") else {
            continue;
        };
        let Some((name, relative)) = rest.split_once('/') else {
            continue;
        };
        if !slug(name)
            || relative.is_empty()
            || relative
                .split('/')
                .any(|part| part == "." || part == ".." || part.is_empty())
        {
            return Err(GuidanceSourceError::Invalid("unsafe skill path".into()));
        }
        let data = git_file(&checkout, &commit, path)?;
        let skill = if let Some(skill) = skills.iter_mut().find(|skill| skill.name == name) {
            skill
        } else {
            skills.push(SourceSkill {
                name: name.to_owned(),
                files: Vec::new(),
            });
            skills.last_mut().unwrap()
        };
        skill.files.push(SourceSkillFile {
            path: relative.to_owned(),
            sha256: digest(&data),
            base64: base64::engine::general_purpose::STANDARD.encode(data),
            known_base64: Vec::new(),
        });
    }
    skills.retain(|skill| skill.files.iter().any(|file| file.path == "SKILL.md"));
    Ok(GuidanceSource {
        id: id.to_owned(),
        group_id: None,
        repository: repository.to_owned(),
        requested_ref: requested_ref.to_owned(),
        commit,
        rules,
        skills,
        selected_rules: Vec::new(),
        selected_skills: Vec::new(),
    })
}

/// Resolve a bounded set of source refs for review. Wildcards expand tags only, so
/// moving branches and unconstrained commit ranges cannot silently grow the set.
pub fn review_batch(requests: &[BatchSourceRequest]) -> Result<BatchReview, GuidanceSourceError> {
    if requests.is_empty() || requests.len() > 8 {
        return Err(GuidanceSourceError::Invalid(
            "review requires between one and eight sources".into(),
        ));
    }
    let mut groups = HashSet::new();
    let mut pins = Vec::new();
    let mut ids = HashSet::new();
    for request in requests {
        if !slug(&request.id)
            || !groups.insert(request.id.as_str())
            || request.refs.is_empty()
            || request.refs.len() > 8
        {
            return Err(GuidanceSourceError::Invalid(
                "invalid or duplicate batch source".into(),
            ));
        }
        let (checkout, _temporary) = source_checkout(&request.repository)?;
        let mut refs = Vec::new();
        let mut seen_refs = HashSet::new();
        for pattern in &request.refs {
            if let Some(prefix) = pattern.strip_suffix('*') {
                if prefix.is_empty()
                    || !prefix.bytes().all(|byte| {
                        byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'/')
                    })
                {
                    return Err(GuidanceSourceError::Invalid(
                        "only a trailing tag wildcard is supported".into(),
                    ));
                }
                let expanded = String::from_utf8(git(&checkout, &["tag", "--list", pattern])?)
                    .map_err(|_| GuidanceSourceError::Invalid("non-UTF-8 tag".into()))?;
                let tags: Vec<_> = expanded.lines().collect();
                if tags.is_empty() || tags.len() > 32 {
                    return Err(GuidanceSourceError::Invalid(
                        "tag wildcard matched zero or too many revisions".into(),
                    ));
                }
                for tag in tags {
                    if !seen_refs.insert(tag.to_owned()) {
                        return Err(GuidanceSourceError::Invalid(
                            "duplicate ref in batch review".into(),
                        ));
                    }
                    refs.push(tag.to_owned());
                }
            } else {
                if pattern.is_empty()
                    || pattern.starts_with('-')
                    || pattern.contains(['*', '?', '[', ']', '\\'])
                    || !seen_refs.insert(pattern.clone())
                {
                    return Err(GuidanceSourceError::Invalid(
                        "invalid or duplicate source ref".into(),
                    ));
                }
                refs.push(pattern.clone());
            }
        }
        refs.sort();
        for reference in refs {
            if pins.len() >= 32 {
                return Err(GuidanceSourceError::Invalid(
                    "batch review exceeds 32 pinned revisions".into(),
                ));
            }
            let id = if request.refs.len() == 1 && !request.refs[0].ends_with('*') {
                request.id.clone()
            } else {
                format!("{}-{}", request.id, &digest(reference.as_bytes())[..10])
            };
            if !ids.insert(id.clone()) {
                return Err(GuidanceSourceError::Invalid(
                    "batch source identities collide".into(),
                ));
            }
            let source = inspect_source(&id, &request.repository, &reference)?;
            pins.push(ReviewedPin {
                id,
                group_id: request.id.clone(),
                repository: request.repository.clone(),
                requested_ref: reference,
                commit: source.commit,
                rules: source.rules.len(),
                skills: source.skills.len(),
            });
        }
    }
    Ok(BatchReview {
        requests: requests.to_vec(),
        pins,
    })
}

pub fn load(project: &Path) -> Result<GuidanceConfig, GuidanceSourceError> {
    let path = project.join(CONFIG_PATH);
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(GuidanceConfig::default());
        }
        Err(error) => return Err(error.into()),
    };
    let config: GuidanceConfig = serde_json::from_slice(&bytes).map_err(|error| {
        GuidanceSourceError::Invalid(format!("invalid guidance sources: {error}"))
    })?;
    validate(&config)?;
    Ok(config)
}

pub fn validate(config: &GuidanceConfig) -> Result<(), GuidanceSourceError> {
    if config.schema_version != 1 {
        return Err(GuidanceSourceError::Invalid(
            "unsupported guidance config version".into(),
        ));
    }
    let mut ids = HashSet::new();
    let mut sections = HashSet::new();
    let mut selected_skills = HashSet::new();
    for source in &config.sources {
        if !slug(&source.id)
            || source.group_id.as_deref().is_some_and(|id| !slug(id))
            || !ids.insert(&source.id)
            || source.commit.len() != 40
            || !source.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(GuidanceSourceError::Invalid(
                "invalid or duplicate guidance source".into(),
            ));
        }
        let mut rule_names = HashSet::new();
        for rule in &source.rules {
            let mut versions = HashSet::from([rule.version]);
            let mut digests = HashSet::from([rule.sha256.as_str()]);
            if !slug(&rule.name)
                || !rule_names.insert(&rule.name)
                || digest(rule.body.as_bytes()) != rule.sha256
                || !valid_minimum_version(rule.minimum_hotsheet_version.as_deref())
                || rule.historical_versions.iter().any(|history| {
                    history.commit.len() != 40
                        || !history.commit.bytes().all(|byte| byte.is_ascii_hexdigit())
                        || !versions.insert(history.version)
                        || !digests.insert(history.sha256.as_str())
                        || digest(history.body.as_bytes()) != history.sha256
                        || !history
                            .body
                            .starts_with(&rule_marker(&rule.name, history.version))
                        || !valid_minimum_version(history.minimum_hotsheet_version.as_deref())
                })
                || rule.selected_sha256.as_ref().is_some_and(|selected| {
                    !rule
                        .historical_versions
                        .iter()
                        .any(|history| &history.sha256 == selected)
                })
                || rule
                    .known_bodies
                    .iter()
                    .any(|body| !body.starts_with("<!-- hotsheet:"))
            {
                return Err(GuidanceSourceError::Invalid("invalid cached rule".into()));
            }
            if source.selected_rules.contains(&rule.name)
                && !sections.insert(
                    rule.name
                        .strip_prefix("hotsheet-")
                        .unwrap_or(&rule.name)
                        .to_owned(),
                )
            {
                return Err(GuidanceSourceError::Invalid(
                    "selected rule name collides across sources".into(),
                ));
            }
        }
        if source.selected_rules.len() != source.selected_rules.iter().collect::<HashSet<_>>().len()
            || source.selected_skills.len()
                != source.selected_skills.iter().collect::<HashSet<_>>().len()
        {
            return Err(GuidanceSourceError::Invalid("duplicate selection".into()));
        }
        let mut skill_names = HashSet::new();
        if source
            .selected_rules
            .iter()
            .any(|name| !source.rules.iter().any(|rule| &rule.name == name))
            || source
                .selected_skills
                .iter()
                .any(|name| !source.skills.iter().any(|skill| &skill.name == name))
            || source
                .selected_skills
                .iter()
                .any(|name| name == "hotsheet" || !selected_skills.insert(name))
        {
            return Err(GuidanceSourceError::Invalid(
                "selection names a missing rule or skill".into(),
            ));
        }
        for skill in &source.skills {
            if !slug(&skill.name)
                || !skill_names.insert(&skill.name)
                || skill
                    .files
                    .iter()
                    .map(|file| &file.path)
                    .collect::<HashSet<_>>()
                    .len()
                    != skill.files.len()
                || skill.files.iter().any(|file| {
                    file.path.is_empty()
                        || file.path.split('/').any(|part| {
                            part.is_empty() || part == "." || part == ".." || part.contains('\\')
                        })
                        || base64::engine::general_purpose::STANDARD
                            .decode(&file.base64)
                            .ok()
                            .is_none_or(|data| digest(&data) != file.sha256)
                        || file.known_base64.iter().any(|encoded| {
                            base64::engine::general_purpose::STANDARD
                                .decode(encoded)
                                .is_err()
                        })
                })
            {
                return Err(GuidanceSourceError::Invalid("invalid cached skill".into()));
            }
        }
    }
    Ok(())
}

pub fn save(project: &Path, config: &GuidanceConfig) -> Result<(), GuidanceSourceError> {
    validate(config)?;
    let path = project.join(CONFIG_PATH);
    fs::create_dir_all(path.parent().unwrap())?;
    let bytes = serde_json::to_vec_pretty(config)
        .map_err(|error| GuidanceSourceError::Invalid(error.to_string()))?;
    let mut staged = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    std::io::Write::write_all(&mut staged, &bytes)?;
    std::io::Write::write_all(&mut staged, b"\n")?;
    staged
        .persist(&path)
        .map_err(|error| GuidanceSourceError::Io(error.error))?;
    Ok(())
}

pub fn register(
    project: &Path,
    id: &str,
    repository: &str,
    requested_ref: &str,
) -> Result<GuidanceConfig, GuidanceSourceError> {
    register_checked(project, id, repository, requested_ref, None)
}

/// Register only the exact revision the user reviewed, while retaining the mutable
/// requested ref so future checks can still observe movement.
pub fn register_checked(
    project: &Path,
    id: &str,
    repository: &str,
    requested_ref: &str,
    expected_commit: Option<&str>,
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let mut source = inspect_source(id, repository, requested_ref)?;
    if expected_commit.is_some_and(|expected| expected != source.commit) {
        return Err(GuidanceSourceError::Invalid(
            "source changed since review; check and review it again".into(),
        ));
    }
    let _lock = lock_config(project)?;
    let mut config = load(project)?;
    if let Some(old) = config.sources.iter().find(|old| old.id == id) {
        preserve_previous(&mut source, old);
    }
    config.sources.retain(|old| old.id != id);
    config.sources.push(source);
    save(project, &config)?;
    Ok(config)
}

fn preserve_previous(source: &mut GuidanceSource, old: &GuidanceSource) {
    source.group_id = old.group_id.clone().or(source.group_id.clone());
    source.selected_rules = old.selected_rules.clone();
    source.selected_skills = old.selected_skills.clone();
    for rule in &mut source.rules {
        if let Some(previous) = old.rules.iter().find(|previous| previous.name == rule.name) {
            let mut history = previous.historical_versions.clone();
            if previous.sha256 != rule.sha256 {
                history.push(SourceRuleRevision {
                    version: previous.version,
                    sha256: previous.sha256.clone(),
                    description: previous.description.clone(),
                    body: previous.body.clone(),
                    commit: old.commit.clone(),
                    minimum_hotsheet_version: previous.minimum_hotsheet_version.clone(),
                });
            }
            history.retain(|item| item.sha256 != rule.sha256);
            history.sort_by_key(|item| std::cmp::Reverse(item.version));
            rule.known_bodies
                .extend(history.iter().map(|item| item.body.clone()));
            rule.historical_versions = history;
            if let Some(chosen) = previous.selected_sha256.as_deref()
                && chosen != rule.sha256
            {
                rule.selected_sha256 = Some(chosen.to_owned());
            }
            if previous.body != rule.body {
                rule.known_bodies.push(previous.body.clone());
            }
            rule.known_bodies.extend(previous.known_bodies.clone());
            rule.known_bodies.sort();
            rule.known_bodies.dedup();
        }
    }
    for skill in &mut source.skills {
        if let Some(previous) = old
            .skills
            .iter()
            .find(|previous| previous.name == skill.name)
        {
            for file in &mut skill.files {
                if let Some(before) = previous
                    .files
                    .iter()
                    .find(|before| before.path == file.path)
                {
                    if before.base64 != file.base64 {
                        file.known_base64.push(before.base64.clone());
                    }
                    file.known_base64.extend(before.known_base64.clone());
                    file.known_base64.sort();
                    file.known_base64.dedup();
                }
            }
        }
    }
}

/// Apply an exact reviewed pin set in one config write. Re-resolving every ref detects
/// movement between review and registration before any source is replaced.
pub fn register_batch_checked(
    project: &Path,
    reviewed: &BatchReview,
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let observed = review_batch(&reviewed.requests)?;
    if observed != *reviewed {
        return Err(GuidanceSourceError::Invalid(
            "source set changed since review; review it again".into(),
        ));
    }
    let mut incoming = Vec::with_capacity(reviewed.pins.len());
    for pin in &reviewed.pins {
        let mut source = inspect_source(&pin.id, &pin.repository, &pin.commit)?;
        source.requested_ref = pin.requested_ref.clone();
        source.group_id = (pin.id != pin.group_id).then(|| pin.group_id.clone());
        incoming.push(source);
    }
    let _lock = lock_config(project)?;
    let mut config = load(project)?;
    for source in &mut incoming {
        let group = source.group_id.as_deref().unwrap_or(&source.id);
        let old = config.sources.iter().find(|old| {
            old.repository == source.repository
                && old.group_id.as_deref().unwrap_or(&old.id) == group
                && (old.id == source.id || old.requested_ref == source.requested_ref)
        });
        if let Some(old) = old {
            preserve_previous(source, old);
        }
    }
    let groups: HashSet<_> = reviewed
        .requests
        .iter()
        .map(|request| request.id.as_str())
        .collect();
    config
        .sources
        .retain(|source| !groups.contains(source.group_id.as_deref().unwrap_or(&source.id)));
    config.sources.extend(incoming);
    save(project, &config)?;
    Ok(config)
}

/// Cache one previously reviewed rule revision without changing the source pin.
/// The full commit is required to prevent a moving tag from changing after review.
pub fn cache_older_rule(
    project: &Path,
    id: &str,
    name: &str,
    requested_ref: &str,
    expected_commit: &str,
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let before = load(project)?;
    let source = before
        .sources
        .iter()
        .find(|source| source.id == id)
        .ok_or_else(|| GuidanceSourceError::Invalid("unknown source".into()))?;
    let repository = source.repository.clone();
    let pinned_commit = source.commit.clone();
    let candidate = inspect_source(id, &repository, requested_ref)?;
    if candidate.commit != expected_commit {
        return Err(GuidanceSourceError::Invalid(
            "historic source changed since review; inspect it again".into(),
        ));
    }
    let offered = candidate
        .rules
        .iter()
        .find(|rule| rule.name == name)
        .ok_or_else(|| GuidanceSourceError::Invalid("rule missing at historic ref".into()))?;
    let _lock = lock_config(project)?;
    let mut config = load(project)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| {
            source.id == id && source.repository == repository && source.commit == pinned_commit
        })
        .ok_or_else(|| GuidanceSourceError::Invalid("source changed since review".into()))?;
    let rule = source
        .rules
        .iter_mut()
        .find(|rule| rule.name == name)
        .ok_or_else(|| GuidanceSourceError::Invalid("rule missing from pinned source".into()))?;
    if offered.version >= rule.version {
        return Err(GuidanceSourceError::Invalid(
            "historic ref does not offer an older rule version".into(),
        ));
    }
    if !rule
        .historical_versions
        .iter()
        .any(|history| history.sha256 == offered.sha256)
    {
        rule.historical_versions.push(SourceRuleRevision {
            version: offered.version,
            sha256: offered.sha256.clone(),
            description: offered.description.clone(),
            body: offered.body.clone(),
            commit: candidate.commit,
            minimum_hotsheet_version: offered.minimum_hotsheet_version.clone(),
        });
        rule.historical_versions
            .sort_by_key(|item| std::cmp::Reverse(item.version));
        rule.known_bodies.push(offered.body.clone());
        rule.known_bodies.sort();
        rule.known_bodies.dedup();
        save(project, &config)?;
    }
    Ok(config)
}

/// Choose one exact cached rule body; an empty digest restores the source's pin.
pub fn select_rule_version(
    project: &Path,
    id: &str,
    name: &str,
    sha256: Option<&str>,
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let _lock = lock_config(project)?;
    let mut config = load(project)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == id)
        .ok_or_else(|| GuidanceSourceError::Invalid("unknown source".into()))?;
    let rule = source
        .rules
        .iter_mut()
        .find(|rule| rule.name == name)
        .ok_or_else(|| GuidanceSourceError::Invalid("unknown rule".into()))?;
    rule.selected_sha256 = sha256
        .filter(|digest| *digest != rule.sha256)
        .map(str::to_owned);
    validate(&config)?;
    let source = config
        .sources
        .iter()
        .find(|source| source.id == id)
        .unwrap();
    let rule = source.rules.iter().find(|rule| rule.name == name).unwrap();
    require_compatible(&chosen_rule(source, rule))?;
    save(project, &config)?;
    Ok(config)
}

pub fn select(
    project: &Path,
    id: &str,
    rules: &[String],
    skills: &[String],
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let _lock = lock_config(project)?;
    let mut config = load(project)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == id)
        .ok_or_else(|| GuidanceSourceError::Invalid("unknown source".into()))?;
    source.selected_rules = rules.to_vec();
    source.selected_skills = skills.to_vec();
    for name in rules {
        let rule = source
            .rules
            .iter()
            .find(|rule| &rule.name == name)
            .ok_or_else(|| GuidanceSourceError::Invalid("unknown selected rule".into()))?;
        require_compatible(&chosen_rule(source, rule))?;
    }
    config.explicit_selection = true;
    save(project, &config)?;
    Ok(config)
}

pub fn skill_digest(skill: &SourceSkill) -> String {
    let mut files = skill
        .files
        .iter()
        .map(|file| (file.path.as_str(), file.sha256.as_str()))
        .collect::<Vec<_>>();
    files.sort_unstable();
    digest(&serde_json::to_vec(&(skill.name.as_str(), files)).expect("skill fields serialize"))
}

/// Add one reviewed item without replacing the project's other deliberate selections.
pub fn select_item(
    project: &Path,
    source_id: &str,
    kind: &str,
    name: &str,
    expected_version: u64,
    expected_digest: &str,
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let _lock = lock_config(project)?;
    let mut config = load(project)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == source_id)
        .ok_or_else(|| GuidanceSourceError::Invalid("unknown source".into()))?;
    if kind == "rule" {
        let rule = source
            .rules
            .iter()
            .find(|rule| rule.name == name)
            .ok_or_else(|| GuidanceSourceError::Invalid("unknown rule".into()))?;
        require_compatible(&chosen_rule(source, rule))?;
    }
    let selected = match kind {
        "rule"
            if source.rules.iter().any(|rule| {
                rule.name == name
                    && rule.version == expected_version
                    && rule.sha256 == expected_digest
            }) =>
        {
            &mut source.selected_rules
        }
        "skill"
            if source
                .skills
                .iter()
                .any(|skill| skill.name == name && skill_digest(skill) == expected_digest) =>
        {
            &mut source.selected_skills
        }
        _ => {
            return Err(GuidanceSourceError::Invalid(
                "guidance recommendation changed since review".into(),
            ));
        }
    };
    if !selected.iter().any(|item| item == name) || !config.explicit_selection {
        if !selected.iter().any(|item| item == name) {
            selected.push(name.to_owned());
        }
        config.explicit_selection = true;
        save(project, &config)?;
    }
    Ok(config)
}

/// Materialize the selected, already-pinned source files as Rulesync inputs, then let
/// Rulesync own `AGENTS.md`. No ref is fetched by this operation.
pub fn apply_rulesync(project: &Path) -> Result<(), GuidanceSourceError> {
    let config = load(project)?;
    if !config.explicit_selection {
        return Err(GuidanceSourceError::Invalid(
            "select rules before applying Rulesync".into(),
        ));
    }
    for source in &config.sources {
        for rule in source
            .rules
            .iter()
            .filter(|rule| source.selected_rules.contains(&rule.name))
        {
            require_compatible(&chosen_rule(source, rule))?;
        }
    }
    let curated = project.join(".rulesync/rules/.curated");
    fs::create_dir_all(&curated)?;
    let mut offered =
        std::collections::BTreeMap::<&str, Vec<(&GuidanceSource, &SourceRule, bool)>>::new();
    for source in &config.sources {
        for rule in &source.rules {
            offered.entry(&rule.name).or_default().push((
                source,
                rule,
                source.selected_rules.contains(&rule.name),
            ));
        }
    }
    for (name, entries) in offered {
        let path = curated.join(format!("{name}.md"));
        let existing = fs::read_to_string(&path).ok();
        let existing_body = existing
            .as_deref()
            .and_then(|text| split_rule_body(text).ok())
            .map(|(body, _)| body);
        let known = existing_body.is_some_and(|body| {
            entries.iter().any(|(_, rule, _)| {
                body == rule.body || rule.known_bodies.iter().any(|old| body == old)
            })
        });
        if let Some((source, rule, _)) = entries.iter().find(|(_, _, selected)| *selected) {
            let chosen = chosen_rule(source, rule);
            let content = format!(
                "---\nroot: true\ntargets: [\"*\"]\ndescription: {}\n---\n\n{}",
                serde_json::to_string(chosen.description)
                    .map_err(|error| GuidanceSourceError::Invalid(error.to_string()))?,
                chosen.body
            );
            if (existing.is_none() || known) && existing.as_deref() != Some(content.as_str()) {
                fs::write(&path, content.as_bytes())?;
            }
        } else if known {
            fs::remove_file(&path)?;
        }
    }
    let generated = project.join(".hotsheet2/rulesync.generated.jsonc");
    fs::create_dir_all(generated.parent().unwrap())?;
    let settings = serde_json::json!({
        "targets": ["agentsmd"],
        "features": ["rules"],
    });
    let bytes = serde_json::to_vec_pretty(&settings)
        .map_err(|error| GuidanceSourceError::Invalid(error.to_string()))?;
    if fs::read(&generated).ok().as_deref() != Some(bytes.as_slice()) {
        fs::write(&generated, bytes)?;
    }
    let mut command = Command::new("rulesync");
    command
        .current_dir(project)
        .arg("generate")
        .arg("--delete")
        .arg("--config")
        .arg(&generated);
    let output = guidance_process_context(hotsheet_process_journal::Purpose::Rulesync)
        .output(&mut command)
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GuidanceSourceError::Invalid(
                    "Rulesync is unavailable; install rulesync@38.0.0 and run `rulesync generate --delete --config .hotsheet2/rulesync.generated.jsonc` from the project root".into(),
                )
            } else {
                GuidanceSourceError::Io(error)
            }
        })?;
    if !output.status.success() {
        return Err(GuidanceSourceError::Invalid(format!(
            "Rulesync generation failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    crate::setup::apply_selected_skills_in(project)
        .map_err(|error| GuidanceSourceError::Invalid(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_rule(repository: &Path, version: u64, text: &str) {
        let body = format!(
            "<!-- hotsheet:begin section=rust-lint-and-format v={version} -->\n\n{text}\n\n<!-- hotsheet:end section=rust-lint-and-format -->\n"
        );
        fs::create_dir_all(repository.join("rules")).unwrap();
        fs::write(
            repository.join("rules/rust-lint-and-format.md"),
            format!(
                "---\nroot: true\ntargets: [\"*\"]\ndescription: \"Rust checks\"\n---\n\n{body}"
            ),
        )
        .unwrap();
        fs::write(repository.join("rules-manifest.json"), serde_json::to_vec(&serde_json::json!({
            "schema_version": 1,
            "rules": [{"name": "rust-lint-and-format", "path": "rules/rust-lint-and-format.md",
                "version": version, "sha256": digest(body.as_bytes()), "owner": "rust", "tags": ["rust"]}]
        })).unwrap()).unwrap();
    }

    fn commit(repository: &Path) -> String {
        git(repository, &["add", "."]).unwrap();
        let output = hotsheet_ticketing::git::command_in(repository)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "-qm",
                "snapshot",
            ])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(git(repository, &["rev-parse", "HEAD"]).unwrap())
            .unwrap()
            .trim()
            .to_owned()
    }

    #[test]
    fn pinned_source_selection_and_refresh_preserve_local_edits() {
        let source = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        git(source.path(), &["init", "-q"]).unwrap();
        write_rule(source.path(), 1, "First version.");
        fs::create_dir_all(source.path().join("skills/example")).unwrap();
        fs::write(source.path().join("skills/example/SKILL.md"), "# Example\n").unwrap();
        let first = commit(source.path());
        let source_path = source.path().to_str().unwrap();
        let registered = register(project.path(), "test", source_path, &first).unwrap();
        assert_eq!(registered.sources[0].commit, first);
        assert_eq!(registered.sources[0].skills[0].name, "example");
        select(
            project.path(),
            "test",
            &["rust-lint-and-format".into()],
            &["example".into()],
        )
        .unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert_eq!(
            installed_version(project.path(), "rust-lint-and-format"),
            Some(1)
        );

        write_rule(source.path(), 2, "Second version.");
        fs::write(
            source.path().join("skills/example/SKILL.md"),
            "# Updated example\n",
        )
        .unwrap();
        let second = commit(source.path());
        let refreshed = register(project.path(), "test", source_path, &second).unwrap();
        assert_eq!(
            refreshed.sources[0].skills[0].files[0].known_base64.len(),
            1
        );
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        let installed = fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        assert!(installed.contains("Second version."));
        assert!(!installed.contains("First version."));

        fs::write(
            project.path().join("AGENTS.md"),
            installed.replace("Second version.", "Local edit."),
        )
        .unwrap();
        select(project.path(), "test", &[], &[]).unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert!(
            fs::read_to_string(project.path().join("AGENTS.md"))
                .unwrap()
                .contains("Local edit.")
        );

        let mut alternate = load(project.path()).unwrap();
        let mut second_source = alternate.sources[0].clone();
        second_source.id = "other".into();
        second_source.rules[0].name = "hotsheet-rust-lint-and-format".into();
        second_source.selected_rules.clear();
        second_source.selected_skills.clear();
        alternate.sources.push(second_source);
        alternate.sources[0].selected_rules = vec!["rust-lint-and-format".into()];
        save(project.path(), &alternate).unwrap();
        fs::write(project.path().join("AGENTS.md"), "").unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert_eq!(
            installed_version(project.path(), "rust-lint-and-format"),
            Some(2)
        );
    }

    #[test]
    fn batch_review_pins_tag_glob_atomically_and_preserves_selected_ref() {
        let repository = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        git(repository.path(), &["init", "-q"]).unwrap();
        write_rule(repository.path(), 1, "First tagged rule.");
        let first = commit(repository.path());
        git(repository.path(), &["tag", "v1.0"]).unwrap();
        write_rule(repository.path(), 2, "Second tagged rule.");
        let second = commit(repository.path());
        git(repository.path(), &["tag", "v1.1"]).unwrap();
        let request = BatchSourceRequest {
            id: "test".into(),
            repository: repository.path().to_str().unwrap().into(),
            refs: vec!["v1.*".into()],
        };
        let reviewed = review_batch(&[request]).unwrap();
        assert_eq!(reviewed.pins.len(), 2);
        assert_eq!(reviewed.pins[0].commit, first);
        assert_eq!(reviewed.pins[1].commit, second);
        assert_ne!(reviewed.pins[0].id, reviewed.pins[1].id);
        let config = register_batch_checked(project.path(), &reviewed).unwrap();
        assert_eq!(config.sources.len(), 2);
        let selected = &reviewed.pins[0].id;
        select(
            project.path(),
            selected,
            &["rust-lint-and-format".into()],
            &[],
        )
        .unwrap();
        let newer = &reviewed.pins[1].id;
        let cached = cache_older_rule(
            project.path(),
            newer,
            "rust-lint-and-format",
            &first,
            &first,
        )
        .unwrap();
        let historic_sha = cached
            .sources
            .iter()
            .find(|source| &source.id == newer)
            .unwrap()
            .rules[0]
            .historical_versions[0]
            .sha256
            .clone();
        select_rule_version(
            project.path(),
            newer,
            "rust-lint-and-format",
            Some(&historic_sha),
        )
        .unwrap();
        let repeated = register_batch_checked(project.path(), &reviewed).unwrap();
        assert_eq!(repeated.sources.len(), 2);
        assert!(
            repeated
                .sources
                .iter()
                .find(|source| &source.id == selected)
                .unwrap()
                .selected_rules
                .contains(&"rust-lint-and-format".to_owned())
        );
        let retained_rule = &repeated
            .sources
            .iter()
            .find(|source| &source.id == newer)
            .unwrap()
            .rules[0];
        assert_eq!(
            retained_rule.selected_sha256.as_deref(),
            Some(historic_sha.as_str())
        );
        assert_eq!(retained_rule.historical_versions[0].commit, first);

        git(repository.path(), &["tag", "-f", "v1.0", &second]).unwrap();
        let before = fs::read(project.path().join(CONFIG_PATH)).unwrap();
        assert!(register_batch_checked(project.path(), &reviewed).is_err());
        assert_eq!(fs::read(project.path().join(CONFIG_PATH)).unwrap(), before);
    }

    #[test]
    fn older_revision_is_selected_by_digest_and_can_replace_a_newer_install() {
        let source = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        git(source.path(), &["init", "-q"]).unwrap();
        write_rule(source.path(), 1, "Older instructions.");
        let first = commit(source.path());
        write_rule(source.path(), 2, "Newer instructions.");
        let second = commit(source.path());
        let repository = source.path().to_str().unwrap();
        register_checked(project.path(), "test", repository, &second, Some(&second)).unwrap();
        select(
            project.path(),
            "test",
            &["rust-lint-and-format".into()],
            &[],
        )
        .unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert_eq!(
            installed_version(project.path(), "rust-lint-and-format"),
            Some(2)
        );

        assert!(
            cache_older_rule(
                project.path(),
                "test",
                "rust-lint-and-format",
                &first,
                &second
            )
            .is_err()
        );
        let cached = cache_older_rule(
            project.path(),
            "test",
            "rust-lint-and-format",
            &first,
            &first,
        )
        .unwrap();
        assert_eq!(cached.sources[0].commit, second);
        let revision = &cached.sources[0].rules[0].historical_versions[0];
        assert_eq!(revision.version, 1);
        let digest = revision.sha256.clone();
        cache_older_rule(
            project.path(),
            "test",
            "rust-lint-and-format",
            &first,
            &first,
        )
        .unwrap();
        assert_eq!(
            load(project.path()).unwrap().sources[0].rules[0]
                .historical_versions
                .len(),
            1
        );
        select_rule_version(
            project.path(),
            "test",
            "rust-lint-and-format",
            Some(&digest),
        )
        .unwrap();
        let preview =
            crate::setup::preview_guidance_rule_in(project.path(), "test", "rust-lint-and-format")
                .unwrap();
        assert_eq!(preview.outcome, "replace");
        assert!(preview.pinned.contains("Older instructions."));
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert_eq!(
            installed_version(project.path(), "rust-lint-and-format"),
            Some(1)
        );
        assert!(
            fs::read_to_string(project.path().join("AGENTS.md"))
                .unwrap()
                .contains("Older instructions.")
        );
        select_rule_version(project.path(), "test", "rust-lint-and-format", None).unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert_eq!(
            installed_version(project.path(), "rust-lint-and-format"),
            Some(2)
        );
    }

    #[test]
    fn incompatible_revision_cannot_be_selected_or_applied() {
        let source = tempfile::tempdir().unwrap();
        let project = tempfile::tempdir().unwrap();
        git(source.path(), &["init", "-q"]).unwrap();
        write_rule(source.path(), 1, "Needs a newer app.");
        let manifest_path = source.path().join("rules-manifest.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).unwrap()).unwrap();
        manifest["rules"][0]["minimum_hotsheet_version"] = serde_json::json!("999.0.0");
        fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        let first = commit(source.path());
        write_rule(source.path(), 2, "Compatible.");
        let second = commit(source.path());
        let repository = source.path().to_str().unwrap();
        register(project.path(), "test", repository, &second).unwrap();
        let cached = cache_older_rule(
            project.path(),
            "test",
            "rust-lint-and-format",
            &first,
            &first,
        )
        .unwrap();
        let digest = &cached.sources[0].rules[0].historical_versions[0].sha256;
        assert!(
            select_rule_version(project.path(), "test", "rust-lint-and-format", Some(digest))
                .is_err()
        );
        assert!(
            load(project.path()).unwrap().sources[0].rules[0]
                .selected_sha256
                .is_none()
        );
        let mut tampered = load(project.path()).unwrap();
        tampered.sources[0].rules[0].historical_versions[0]
            .body
            .push_str("tampered");
        assert!(save(project.path(), &tampered).is_err());

        let incompatible_project = tempfile::tempdir().unwrap();
        register(incompatible_project.path(), "test", repository, &first).unwrap();
        assert!(
            select(
                incompatible_project.path(),
                "test",
                &["rust-lint-and-format".into()],
                &[],
            )
            .is_err()
        );
        let mut incompatible = load(incompatible_project.path()).unwrap();
        incompatible.sources[0]
            .selected_rules
            .push("rust-lint-and-format".into());
        save(incompatible_project.path(), &incompatible).unwrap();
        assert!(
            crate::setup::preview_guidance_rule_in(
                incompatible_project.path(),
                "test",
                "rust-lint-and-format"
            )
            .is_err()
        );
        assert!(crate::setup::apply_selected_guidance_in(incompatible_project.path()).is_err());
        assert!(!incompatible_project.path().join("AGENTS.md").exists());
    }

    #[test]
    fn rejects_duplicate_and_escaping_cached_entries() {
        let mut config = GuidanceConfig::default();
        let source = tempfile::tempdir().unwrap();
        git(source.path(), &["init", "-q"]).unwrap();
        write_rule(source.path(), 1, "A.");
        let first = commit(source.path());
        config
            .sources
            .push(inspect_source("test", source.path().to_str().unwrap(), &first).unwrap());
        let duplicate = config.sources[0].rules[0].clone();
        config.sources[0].rules.push(duplicate);
        assert!(validate(&config).is_err());
        config.sources[0].rules.pop();
        config.sources[0].skills.push(SourceSkill {
            name: "escape".into(),
            files: vec![SourceSkillFile {
                path: "..\\secret".into(),
                sha256: digest(b"x"),
                base64: base64::engine::general_purpose::STANDARD.encode(b"x"),
                known_base64: vec![],
            }],
        });
        assert!(validate(&config).is_err());
    }

    #[test]
    fn selected_workflow_installs_without_a_detected_tool() {
        let project = tempfile::tempdir().unwrap();
        let body = "<!-- hotsheet-instructions-version: 64 -->\n\n## Hot Sheet workflow\n";
        let config = GuidanceConfig {
            schema_version: 1,
            explicit_selection: true,
            sources: vec![GuidanceSource {
                id: "fixture".into(),
                group_id: None,
                repository: "fixture".into(),
                requested_ref: "v1".into(),
                commit: "a".repeat(40),
                selected_rules: vec!["hotsheet-workflow".into()],
                selected_skills: vec![],
                skills: vec![],
                rules: vec![SourceRule {
                    name: "hotsheet-workflow".into(),
                    version: 64,
                    sha256: digest(body.as_bytes()),
                    description: "Workflow".into(),
                    owner: Some("hotsheet".into()),
                    tags: vec![],
                    body: body.into(),
                    known_bodies: vec![],
                    minimum_hotsheet_version: None,
                    historical_versions: Vec::new(),
                    selected_sha256: None,
                }],
            }],
        };
        save(project.path(), &config).unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        let first = fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        assert!(first.contains("## Hot Sheet workflow"));
        assert_eq!(
            installed_version(project.path(), "hotsheet-workflow"),
            Some(64)
        );
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        assert_eq!(
            fs::read_to_string(project.path().join("AGENTS.md")).unwrap(),
            first
        );
        fs::write(
            project.path().join("AGENTS.md"),
            format!(
                "<!-- BEGIN hotsheet:codex -->\n{}\n<!-- END hotsheet:codex -->\n",
                include_str!("../../../plugins/codex/instructions.md").trim_end(),
            ),
        )
        .unwrap();
        crate::setup::apply_selected_guidance_in(project.path()).unwrap();
        let per_tool = fs::read_to_string(project.path().join("AGENTS.md")).unwrap();
        assert!(per_tool.contains("## Hot Sheet workflow"));
        assert!(!per_tool.contains("<!-- BEGIN hotsheet:agents-md -->"));
    }
}
