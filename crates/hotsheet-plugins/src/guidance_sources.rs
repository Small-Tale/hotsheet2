//! Pinned, project-owned rule and skill sources. Registration is the only network-capable
//! operation; ordinary setup reads the authenticated manifest snapshot kept with the project.

use base64::Engine;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const CONFIG_PATH: &str = ".hotsheet2/guidance-sources.json";
pub const DEFAULT_REPOSITORY: &str = "https://github.com/Small-Tale/ai-playbook.git";

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

fn git(repository: &Path, args: &[&str]) -> Result<Vec<u8>, GuidanceSourceError> {
    let output = hotsheet_ticketing::git::command_in(repository)
        .args(args)
        .output()?;
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
    let output = hotsheet_ticketing::git::command()
        .args(["clone", "--quiet", "--no-checkout", "--"])
        .arg(repository)
        .arg(&checkout)
        .output()?;
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

fn split_rule_body(file: &str) -> Result<(&str, String), GuidanceSourceError> {
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
        let marker = if entry.name == "hotsheet-workflow" {
            format!("<!-- hotsheet-instructions-version: {} -->", entry.version)
        } else {
            format!(
                "<!-- hotsheet:begin section={} v={} -->",
                entry.name.strip_prefix("hotsheet-").unwrap_or(&entry.name),
                entry.version
            )
        };
        if !body.starts_with(&marker) {
            return Err(GuidanceSourceError::Invalid(format!(
                "rule version mismatch: {}",
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
        repository: repository.to_owned(),
        requested_ref: requested_ref.to_owned(),
        commit,
        rules,
        skills,
        selected_rules: Vec::new(),
        selected_skills: Vec::new(),
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
            if !slug(&rule.name)
                || !rule_names.insert(&rule.name)
                || digest(rule.body.as_bytes()) != rule.sha256
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
    let mut config = load(project)?;
    let mut source = inspect_source(id, repository, requested_ref)?;
    if let Some(old) = config.sources.iter().find(|old| old.id == id) {
        source.selected_rules = old.selected_rules.clone();
        source.selected_skills = old.selected_skills.clone();
        for rule in &mut source.rules {
            if let Some(previous) = old.rules.iter().find(|previous| previous.name == rule.name) {
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
    config.sources.retain(|old| old.id != id);
    config.sources.push(source);
    save(project, &config)?;
    Ok(config)
}

pub fn select(
    project: &Path,
    id: &str,
    rules: &[String],
    skills: &[String],
) -> Result<GuidanceConfig, GuidanceSourceError> {
    let mut config = load(project)?;
    let source = config
        .sources
        .iter_mut()
        .find(|source| source.id == id)
        .ok_or_else(|| GuidanceSourceError::Invalid("unknown source".into()))?;
    source.selected_rules = rules.to_vec();
    source.selected_skills = skills.to_vec();
    config.explicit_selection = true;
    save(project, &config)?;
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
    let curated = project.join(".rulesync/rules/.curated");
    fs::create_dir_all(&curated)?;
    let mut offered = std::collections::BTreeMap::<&str, Vec<(&SourceRule, bool)>>::new();
    for source in &config.sources {
        for rule in &source.rules {
            offered
                .entry(&rule.name)
                .or_default()
                .push((rule, source.selected_rules.contains(&rule.name)));
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
            entries.iter().any(|(rule, _)| {
                body == rule.body || rule.known_bodies.iter().any(|old| body == old)
            })
        });
        if let Some((rule, _)) = entries.iter().find(|(_, selected)| *selected) {
            let content = format!(
                "---\nroot: true\ntargets: [\"*\"]\ndescription: {}\n---\n\n{}",
                serde_json::to_string(&rule.description)
                    .map_err(|error| GuidanceSourceError::Invalid(error.to_string()))?,
                rule.body
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
    let output = Command::new("rulesync")
        .current_dir(project)
        .arg("generate")
        .arg("--delete")
        .arg("--config")
        .arg(&generated)
        .output()
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
