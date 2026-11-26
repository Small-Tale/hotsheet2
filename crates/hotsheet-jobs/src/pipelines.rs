//! HS2-YA5S82: the repository each CI pipeline builds, as this machine's Job monitor
//! read it.
//!
//! A Buildkite pipeline (`organization/pipeline`) and a CircleCI project addressed by
//! ids (`circleci/<org-id>/<project-id>`) name no repository, so a machine that hosts a
//! checkout cannot tell from the slug alone whether a watch it is asked for reads that
//! checkout's own repository. The provider says which repository a pipeline builds,
//! but a hosted request never calls a provider. Instead the request records that it
//! wants the pipeline's repository, and the Job monitor, which already holds this
//! machine's provider credentials, reads it ([`WatchSource::pipeline_repository`]) and
//! records it here. Until then the hosted request answers a retryable error.
//!
//! One small machine-local file under the Job directory, written under its own lock
//! and replaced atomically. Entries are bounded; the oldest settled ones are dropped
//! first. Remote URLs are recorded as the provider gave them (no credentials: URLs
//! carrying user information are not recorded).
//!
//! [`WatchSource::pipeline_repository`]: crate::WatchSource::pipeline_repository

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::lock::{DirLock, open_lock_file};

/// Under the Job directory (`<home>/jobs`), beside the checkout directories.
pub const PIPELINE_REPOSITORIES_FILE: &str = "pipeline-repositories.json";
const LOCK_FILE: &str = "pipeline-repositories.lock";
/// At most this many pipelines are remembered.
pub const MAX_PIPELINE_ENTRIES: usize = 256;
/// A known repository older than this is read again the next time it is wanted.
pub const PIPELINE_REPOSITORY_REFRESH_MS: u64 = 60 * 60 * 1000;
const MAX_FILE_BYTES: u64 = 256 * 1024;
const MAX_SLUG_BYTES: usize = 512;
const MAX_URL_BYTES: usize = 2048;
const VERSION: u32 = 1;

/// What is known about one pipeline's repository.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum PipelineRepository {
    /// Nothing yet: wanted, and the monitor has not read it.
    Unknown,
    /// The remote URL of the repository the pipeline builds.
    Known { repository: String },
    /// The provider answered, but named no repository this machine may compare: the
    /// pipeline does not exist, the credentials may not read it, or it names none.
    Unreadable,
}

// No `deny_unknown_fields`: serde does not combine it with the flattened state.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
struct Entry {
    provider: String,
    slug: String,
    #[serde(flatten)]
    repository: PipelineRepository,
    /// When it was wanted (Unknown) or read (the others).
    at_ms: u64,
    /// A known entry wanted again because it is old: read it again.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    refresh: bool,
}

impl Entry {
    fn wanted(&self) -> bool {
        self.refresh || self.repository == PipelineRepository::Unknown
    }
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema_version: u32,
    entries: Vec<Entry>,
}

/// The machine's record of pipeline repositories.
#[derive(Clone, Debug)]
pub struct PipelineRepositories {
    dir: PathBuf,
}

fn valid(provider: &str, slug: &str) -> bool {
    !provider.is_empty()
        && provider.len() <= 64
        && provider
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !slug.is_empty()
        && slug.len() <= MAX_SLUG_BYTES
        && !slug.chars().any(char::is_control)
}

/// A remote URL that may be recorded: bounded, printable and without user
/// information (`https://user:token@host/…` is never written to disk). The scp-like
/// `git@host:owner/name` form names a login, not a secret, and is kept.
fn recordable(url: &str) -> bool {
    if url.is_empty() || url.len() > MAX_URL_BYTES || url.chars().any(char::is_control) {
        return false;
    }
    match url.split_once("://") {
        Some((_, rest)) => !rest.split('/').next().unwrap_or("").contains('@'),
        None => true,
    }
}

impl PipelineRepositories {
    pub fn at(machine_home: impl AsRef<Path>) -> Self {
        Self {
            dir: machine_home.as_ref().join("jobs"),
        }
    }

    fn read(&self) -> File {
        let path = self.dir.join(PIPELINE_REPOSITORIES_FILE);
        let Ok(file) = fs::File::open(path) else {
            return File::default();
        };
        let mut bytes = Vec::new();
        if io::Read::read_to_end(&mut io::Read::take(file, MAX_FILE_BYTES + 1), &mut bytes).is_err()
            || bytes.len() as u64 > MAX_FILE_BYTES
        {
            return File::default();
        }
        // A damaged or newer file is read as empty: nothing is known, so a hosted check
        // answers "not yet known" and the next write starts the file over.
        serde_json::from_slice::<File>(&bytes)
            .ok()
            .filter(|file| file.schema_version == VERSION)
            .unwrap_or_default()
    }

    fn change<T>(&self, apply: impl FnOnce(&mut Vec<Entry>) -> (T, bool)) -> io::Result<T> {
        fs::create_dir_all(&self.dir)?;
        let _lock = DirLock::acquire(open_lock_file(&self.dir.join(LOCK_FILE))?)?;
        let mut file = self.read();
        let (answer, changed) = apply(&mut file.entries);
        if changed {
            while file.entries.len() > MAX_PIPELINE_ENTRIES {
                // The oldest settled entry goes first; a wanted one only when every
                // entry is wanted.
                let index = file
                    .entries
                    .iter()
                    .enumerate()
                    .filter(|(_, entry)| !entry.wanted())
                    .min_by_key(|(_, entry)| entry.at_ms)
                    .or_else(|| {
                        file.entries
                            .iter()
                            .enumerate()
                            .min_by_key(|(_, entry)| entry.at_ms)
                    })
                    .map(|(index, _)| index)
                    .unwrap_or(0);
                file.entries.remove(index);
            }
            file.schema_version = VERSION;
            let bytes = serde_json::to_vec(&file).map_err(io::Error::other)?;
            let mut temp = tempfile::NamedTempFile::new_in(&self.dir)?;
            io::Write::write_all(&mut temp, &bytes)?;
            temp.as_file().sync_all()?;
            hotsheet_sync::files::persist(temp, self.dir.join(PIPELINE_REPOSITORIES_FILE))?;
        }
        Ok(answer)
    }

    /// What is known about one pipeline. Writes nothing.
    pub fn lookup(&self, provider: &str, slug: &str) -> PipelineRepository {
        self.read()
            .entries
            .into_iter()
            .find(|entry| entry.provider == provider && entry.slug == slug)
            .map_or(PipelineRepository::Unknown, |entry| entry.repository)
    }

    /// The hosted check's question: what is known, after asking the monitor to read
    /// the pipeline when nothing is known yet or what is known is older than
    /// [`PIPELINE_REPOSITORY_REFRESH_MS`] (the old answer still counts meanwhile).
    /// Repeating it while the read is outstanding changes nothing.
    pub fn want(&self, provider: &str, slug: &str, now_ms: u64) -> io::Result<PipelineRepository> {
        if !valid(provider, slug) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "pipeline"));
        }
        self.change(|entries| {
            match entries
                .iter_mut()
                .find(|entry| entry.provider == provider && entry.slug == slug)
            {
                Some(entry) if entry.wanted() => (entry.repository.clone(), false),
                Some(entry) => {
                    let stale =
                        now_ms.saturating_sub(entry.at_ms) >= PIPELINE_REPOSITORY_REFRESH_MS;
                    entry.refresh = stale;
                    (entry.repository.clone(), stale)
                }
                None => {
                    entries.push(Entry {
                        provider: provider.into(),
                        slug: slug.into(),
                        repository: PipelineRepository::Unknown,
                        at_ms: now_ms,
                        refresh: false,
                    });
                    (PipelineRepository::Unknown, true)
                }
            }
        })
    }

    /// The pipelines the monitor should read, oldest want first.
    pub fn wanted(&self) -> Vec<(String, String)> {
        let mut wanted: Vec<Entry> = self
            .read()
            .entries
            .into_iter()
            .filter(Entry::wanted)
            .collect();
        wanted.sort_by_key(|entry| entry.at_ms);
        wanted
            .into_iter()
            .map(|entry| (entry.provider, entry.slug))
            .collect()
    }

    /// Record what the monitor read. A repository URL that may not be recorded (one
    /// carrying user information) is recorded as unreadable.
    pub fn record(
        &self,
        provider: &str,
        slug: &str,
        repository: Option<&str>,
        now_ms: u64,
    ) -> io::Result<()> {
        if !valid(provider, slug) {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "pipeline"));
        }
        let read = match repository {
            Some(url) if recordable(url) => PipelineRepository::Known {
                repository: url.to_owned(),
            },
            _ => PipelineRepository::Unreadable,
        };
        self.change(|entries| {
            let entry = Entry {
                provider: provider.into(),
                slug: slug.into(),
                repository: read,
                at_ms: now_ms,
                refresh: false,
            };
            match entries
                .iter_mut()
                .find(|entry| entry.provider == provider && entry.slug == slug)
            {
                Some(existing) if *existing == entry => ((), false),
                Some(existing) => {
                    *existing = entry;
                    ((), true)
                }
                None => {
                    entries.push(entry);
                    ((), true)
                }
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BK: &str = "buildkite";

    #[test]
    fn hs2_ya5s82_a_want_is_read_once_recorded_and_refreshed_when_old() {
        let home = tempfile::tempdir().unwrap();
        let cache = PipelineRepositories::at(home.path());
        // Nothing yet: unknown, and reading writes nothing.
        assert_eq!(
            cache.lookup(BK, "acme/widgets"),
            PipelineRepository::Unknown
        );
        assert!(!home.path().join("jobs").exists());
        assert!(cache.wanted().is_empty());
        // Wanted, repeatedly: one entry, still unknown.
        for at in [10, 20] {
            assert_eq!(
                cache.want(BK, "acme/widgets", at).unwrap(),
                PipelineRepository::Unknown
            );
        }
        cache.want("circleci", "circleci/org/project", 30).unwrap();
        assert_eq!(
            cache.wanted(),
            vec![
                (BK.to_owned(), "acme/widgets".to_owned()),
                ("circleci".to_owned(), "circleci/org/project".to_owned())
            ]
        );
        // The monitor reads one: known, no longer wanted; the other stays wanted.
        cache
            .record(
                BK,
                "acme/widgets",
                Some("git@github.com:Acme/Widgets.git"),
                40,
            )
            .unwrap();
        let known = PipelineRepository::Known {
            repository: "git@github.com:Acme/Widgets.git".into(),
        };
        assert_eq!(cache.lookup(BK, "acme/widgets"), known);
        assert_eq!(cache.wanted().len(), 1);
        // Wanting a fresh one again answers it and writes nothing.
        let path = home.path().join("jobs").join(PIPELINE_REPOSITORIES_FILE);
        let before = fs::read(&path).unwrap();
        assert_eq!(cache.want(BK, "acme/widgets", 50).unwrap(), known);
        assert_eq!(fs::read(&path).unwrap(), before);
        // An old one still answers, and is read again.
        let later = 40 + PIPELINE_REPOSITORY_REFRESH_MS;
        assert_eq!(cache.want(BK, "acme/widgets", later).unwrap(), known);
        assert_eq!(cache.wanted().len(), 2);
        // The pipeline moved: the new repository replaces the old one.
        cache
            .record(
                BK,
                "acme/widgets",
                Some("https://github.com/acme/other"),
                later,
            )
            .unwrap();
        assert_eq!(
            cache.lookup(BK, "acme/widgets"),
            PipelineRepository::Known {
                repository: "https://github.com/acme/other".into()
            }
        );
        // Unreadable, and a URL with credentials is never written.
        cache
            .record("circleci", "circleci/org/project", None, 60)
            .unwrap();
        assert_eq!(
            cache.lookup("circleci", "circleci/org/project"),
            PipelineRepository::Unreadable
        );
        cache
            .record(
                BK,
                "acme/secret",
                Some("https://user:token@github.com/acme/x"),
                70,
            )
            .unwrap();
        assert_eq!(
            cache.lookup(BK, "acme/secret"),
            PipelineRepository::Unreadable
        );
        assert!(
            !String::from_utf8(fs::read(&path).unwrap())
                .unwrap()
                .contains("token")
        );
        assert!(cache.wanted().is_empty());
    }

    #[test]
    fn hs2_ya5s82_the_record_is_bounded_and_a_damaged_file_knows_nothing() {
        let home = tempfile::tempdir().unwrap();
        let cache = PipelineRepositories::at(home.path());
        assert!(cache.want(BK, "", 1).is_err());
        assert!(cache.want("Build Kite", "a/b", 1).is_err());
        for index in 0..=MAX_PIPELINE_ENTRIES as u64 {
            cache
                .record(
                    BK,
                    &format!("org/p{index}"),
                    Some("https://github.com/a/b"),
                    index,
                )
                .unwrap();
        }
        // The oldest settled entry went first.
        assert_eq!(cache.lookup(BK, "org/p0"), PipelineRepository::Unknown);
        assert!(matches!(
            cache.lookup(BK, "org/p1"),
            PipelineRepository::Known { .. }
        ));
        // A want past the bound evicts a settled entry, not itself.
        cache.want(BK, "org/new", 10_000).unwrap();
        assert_eq!(cache.wanted(), vec![(BK.to_owned(), "org/new".to_owned())]);
        fs::write(
            home.path().join("jobs").join(PIPELINE_REPOSITORIES_FILE),
            b"not json",
        )
        .unwrap();
        assert_eq!(cache.lookup(BK, "org/p2"), PipelineRepository::Unknown);
        assert!(cache.wanted().is_empty());
        // A write starts the file over.
        cache.want(BK, "org/again", 1).unwrap();
        assert_eq!(cache.wanted().len(), 1);
    }
}
