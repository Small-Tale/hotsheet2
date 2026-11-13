//! The project group registry (HS2-9KZSD4): one committed file per group under the code
//! checkout, `<checkout>/.hotsheet2/groups/<id>.json` holding `{ "name": … }`.
//!
//! One file per group means two clones that each create a group merge without a Git
//! conflict; two clones that create the same *name* leave two ids, which the listing flags
//! as duplicates and the merge operation folds together. Entries are never deleted
//! automatically: an entry with no member tickets is simply not a visible group.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use hotsheet_model::{GroupNameError, group_key, is_group_id, new_group_id, normalize_group_name};

use crate::file_lock::FileLock;

/// Directory of the registry inside a code checkout, relative to its root.
pub const GROUP_REGISTRY_DIR: &str = ".hotsheet2/groups";
/// Machine-local lock beside the registry; `.hotsheet2/*` stays ignored, so it is never
/// committed.
const GROUP_REGISTRY_LOCK: &str = ".hotsheet2/groups.lock";

/// Why the registry could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum GroupRegistryError {
    #[error("group registry {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error(transparent)]
    InvalidName(#[from] GroupNameError),
    #[error("a group needs a name")]
    EmptyName,
    #[error("'{0}' is not a group id")]
    InvalidId(String),
}

fn io_error(path: &Path) -> impl FnOnce(io::Error) -> GroupRegistryError + '_ {
    move |source| GroupRegistryError::Io {
        path: path.to_path_buf(),
        source,
    }
}

/// The id-to-name table of one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupRegistry {
    root: PathBuf,
}

/// A registry snapshot: every readable entry, id to display name.
pub type GroupNames = BTreeMap<String, String>;

impl GroupRegistry {
    /// The registry of the code checkout rooted at `checkout_root`. Nothing is created
    /// until the first entry is written.
    pub fn for_checkout(checkout_root: &Path) -> Self {
        Self {
            root: checkout_root.to_path_buf(),
        }
    }

    /// How the registry's Git publication last went (HS2-9EGWC4), when it was attempted.
    pub fn sync_status(&self) -> Option<super::git_sync::RegistrySyncStatus> {
        super::git_sync::read_status(&self.root)
    }

    /// The directory holding the entry files.
    pub fn dir(&self) -> PathBuf {
        self.root.join(GROUP_REGISTRY_DIR)
    }

    fn entry_path(&self, id: &str) -> PathBuf {
        self.dir().join(format!("{id}.json"))
    }

    /// Every entry. A missing directory is an empty registry. A file whose name is not an
    /// id, or whose content has no usable name, is skipped rather than failing the
    /// project: a hand edit or a half-merged file never hides the other groups.
    pub fn names(&self) -> Result<GroupNames, GroupRegistryError> {
        let dir = self.dir();
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(super::git_sync::overlay_names(&self.root));
            }
            Err(error) => return Err(io_error(&dir)(error)),
        };
        let mut names = GroupNames::new();
        for entry in entries {
            let path = entry.map_err(io_error(&dir))?.path();
            let Some(id) = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.strip_suffix(".json"))
                .filter(|id| is_group_id(id))
            else {
                continue;
            };
            if let Some(name) = read_entry_name(&path) {
                names.insert(id.to_string(), name);
            }
        }
        // HS2-9EGWC4: entries the upstream branch added or renamed and this checkout has not
        // touched, until the user's next pull brings the files themselves.
        names.extend(super::git_sync::overlay_names(&self.root));
        Ok(names)
    }

    /// The display name of `id`, when this project names it.
    pub fn name(&self, id: &str) -> Result<Option<String>, GroupRegistryError> {
        if !is_group_id(id) {
            return Ok(None);
        }
        if let Some(name) = super::git_sync::overlay_names(&self.root).remove(id) {
            return Ok(Some(name));
        }
        Ok(read_entry_name(&self.entry_path(id)))
    }

    /// The ids whose names share `name`'s key, in id order.
    pub fn ids_named(&self, name: &str) -> Result<Vec<String>, GroupRegistryError> {
        let key = group_key(name);
        Ok(self
            .names()?
            .into_iter()
            .filter(|(_, entry)| group_key(entry) == key)
            .map(|(id, _)| id)
            .collect())
    }

    /// Run `f` while holding the registry's process-safe lock, so a find-or-create by name
    /// is atomic on this machine.
    pub fn locked<T>(
        &self,
        f: impl FnOnce() -> Result<T, GroupRegistryError>,
    ) -> Result<T, GroupRegistryError> {
        let lock_path = self.root.join(GROUP_REGISTRY_LOCK);
        if let Some(parent) = lock_path.parent() {
            fs::create_dir_all(parent).map_err(io_error(parent))?;
        }
        let file = fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&lock_path)
            .map_err(io_error(&lock_path))?;
        let _lock = FileLock::acquire(file).map_err(io_error(&lock_path))?;
        f()
    }

    /// The id of the group named `name`, creating a new entry when none has its key.
    /// Several entries with one key (a merge of two clones) resolve to the lowest id.
    /// Returns the id, the stored display name, and whether an entry was created.
    pub fn find_or_create(&self, name: &str) -> Result<(String, String, bool), GroupRegistryError> {
        let name = normalize_group_name(name)?.ok_or(GroupRegistryError::EmptyName)?;
        let found = self.locked(|| {
            let key = group_key(&name);
            if let Some((id, existing)) = self
                .names()?
                .into_iter()
                .find(|(_, entry)| group_key(entry) == key)
            {
                return Ok((id, existing, false));
            }
            let id = self.create_entry(&name)?;
            Ok((id, name, true))
        })?;
        if found.2 {
            self.written(&format!("Create group {}", found.1));
        }
        Ok(found)
    }

    /// Commit (and publish) a registry write in the code checkout (HS2-9EGWC4). Never
    /// fails the write: Git problems become the registry's sync status.
    fn written(&self, message: &str) {
        super::git_sync::registry_written(&self.root, message);
    }

    /// Under one lock, give each `key → spelling` an id: the lowest existing id with that
    /// key, else a new entry with the spelling. Returns the assignments and how many
    /// entries were created and reused (the group migration, HS2-9KZSD4).
    pub fn assign_ids(
        &self,
        spellings: &BTreeMap<String, String>,
    ) -> Result<(BTreeMap<String, String>, usize, usize), GroupRegistryError> {
        self.locked(|| {
            let names = self.names()?;
            let (mut assignments, mut created, mut reused) = (BTreeMap::new(), 0, 0);
            for (key, spelling) in spellings {
                let existing = names
                    .iter()
                    .find(|(_, name)| group_key(name) == *key)
                    .map(|(id, _)| id.clone());
                let id = match existing {
                    Some(id) => {
                        reused += 1;
                        id
                    }
                    None => {
                        created += 1;
                        let name =
                            normalize_group_name(spelling)?.ok_or(GroupRegistryError::EmptyName)?;
                        self.create_entry(&name)?
                    }
                };
                assignments.insert(key.clone(), id);
            }
            Ok((assignments, created, reused))
        })
        .inspect(|(_, created, _)| {
            if *created > 0 {
                self.written(&format!(
                    "Migrate {created} group name{} to the group registry",
                    if *created == 1 { "" } else { "s" }
                ));
            }
        })
    }

    /// Write a new entry for `name` under a fresh id. Callers check uniqueness.
    fn create_entry(&self, name: &str) -> Result<String, GroupRegistryError> {
        let dir = self.dir();
        fs::create_dir_all(&dir).map_err(io_error(&dir))?;
        loop {
            let id = new_group_id();
            let path = self.entry_path(&id);
            match fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(_) => {
                    self.write_entry(&id, name)?;
                    return Ok(id);
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(io_error(&path)(error)),
            }
        }
    }

    /// Set the display name of `id`, creating its entry when this project has none (as
    /// when naming a placeholder). Other fields of an existing entry are kept.
    pub fn set_name(&self, id: &str, name: &str) -> Result<String, GroupRegistryError> {
        if !is_group_id(id) {
            return Err(GroupRegistryError::InvalidId(id.to_string()));
        }
        let name = normalize_group_name(name)?.ok_or(GroupRegistryError::EmptyName)?;
        let name = self.locked(|| {
            let dir = self.dir();
            fs::create_dir_all(&dir).map_err(io_error(&dir))?;
            self.write_entry(id, &name)?;
            Ok(name)
        })?;
        self.written(&format!("Rename group {id} to {name}"));
        Ok(name)
    }

    fn write_entry(&self, id: &str, name: &str) -> Result<(), GroupRegistryError> {
        let path = self.entry_path(id);
        let mut object = fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| match value {
                serde_json::Value::Object(object) => Some(object),
                _ => None,
            })
            .unwrap_or_default();
        object.insert("name".into(), serde_json::Value::String(name.to_string()));
        let mut text = serde_json::to_string_pretty(&serde_json::Value::Object(object))
            .map_err(|error| io_error(&path)(io::Error::other(error)))?;
        text.push('\n');
        crate::store::write_file_atomically(&path, text.as_bytes()).map_err(io_error(&path))
    }
}

/// The normalized name of one entry file, or `None` when it is unreadable or unnamed.
fn read_entry_name(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    hotsheet_model::normalize_group_name_lossy(value.get("name")?.as_str()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry() -> (tempfile::TempDir, GroupRegistry) {
        let dir = tempfile::tempdir().unwrap();
        let registry = GroupRegistry::for_checkout(dir.path());
        (dir, registry)
    }

    #[test]
    fn hs2_9kzsd4_empty_then_create_then_reuse_by_key() {
        let (dir, registry) = registry();
        assert!(registry.names().unwrap().is_empty(), "no directory yet");
        let (id, name, created) = registry.find_or_create("  Sprint   12 ").unwrap();
        assert!(created);
        assert!(is_group_id(&id));
        assert_eq!(name, "Sprint 12");
        let file = dir
            .path()
            .join(GROUP_REGISTRY_DIR)
            .join(format!("{id}.json"));
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            "{\n  \"name\": \"Sprint 12\"\n}\n"
        );
        // Another spelling of the same name is the same group.
        let (again, stored, created) = registry.find_or_create("SPRINT 12").unwrap();
        assert_eq!(
            (again.as_str(), stored.as_str(), created),
            (id.as_str(), "Sprint 12", false)
        );
        // A different name is a new entry, and the lock file is not inside the registry.
        let (other, _, created) = registry.find_or_create("Backlog").unwrap();
        assert!(created);
        assert_ne!(other, id);
        assert_eq!(registry.names().unwrap().len(), 2);
        assert!(!registry.dir().join("groups.lock").exists());
        assert!(dir.path().join(GROUP_REGISTRY_LOCK).exists());
    }

    #[test]
    fn hs2_9kzsd4_rename_is_one_file_edit_that_keeps_other_fields() {
        let (_dir, registry) = registry();
        let (id, _, _) = registry.find_or_create("Sprint").unwrap();
        let path = registry.entry_path(&id);
        fs::write(&path, "{\"name\":\"Sprint\",\"color\":\"teal\"}").unwrap();
        assert_eq!(
            registry.set_name(&id, " Release  train ").unwrap(),
            "Release train"
        );
        let value: serde_json::Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(value["name"], "Release train");
        assert_eq!(value["color"], "teal");
        assert_eq!(
            registry.name(&id).unwrap().as_deref(),
            Some("Release train")
        );
        // Naming an id this project never saw creates its entry (a placeholder rename).
        registry.set_name("7K3M9P2QXA", "Shared").unwrap();
        assert_eq!(
            registry.name("7K3M9P2QXA").unwrap().as_deref(),
            Some("Shared")
        );
        assert!(matches!(
            registry.set_name("not-an-id", "x"),
            Err(GroupRegistryError::InvalidId(_))
        ));
        assert!(matches!(
            registry.set_name(&id, "   "),
            Err(GroupRegistryError::EmptyName)
        ));
        assert!(matches!(
            registry.set_name(&id, "a\nb"),
            Err(GroupRegistryError::InvalidName(_))
        ));
    }

    #[test]
    fn hs2_9kzsd4_unreadable_and_foreign_files_are_skipped() {
        let (_dir, registry) = registry();
        let (id, _, _) = registry.find_or_create("Kept").unwrap();
        let dir = registry.dir();
        fs::write(dir.join("README.md"), "notes").unwrap();
        fs::write(dir.join("not-an-id.json"), "{\"name\":\"x\"}").unwrap();
        fs::write(dir.join("7K3M9P2QXA.json"), "{ not json").unwrap();
        fs::write(dir.join("7K3M9P2QXB.json"), "{\"name\":42}").unwrap();
        fs::write(dir.join("7K3M9P2QXC.json"), "{\"name\":\"  \"}").unwrap();
        let names = registry.names().unwrap();
        assert_eq!(names.keys().collect::<Vec<_>>(), vec![&id]);
        assert_eq!(registry.name("7K3M9P2QXA").unwrap(), None);
    }

    #[test]
    fn hs2_9kzsd4_duplicate_entries_resolve_to_the_lowest_id() {
        let (_dir, registry) = registry();
        let dir = registry.dir();
        fs::create_dir_all(&dir).unwrap();
        // Two clones each created "Sprint" and the merge kept both files.
        fs::write(dir.join("ZZZZZZZZZZ.json"), "{\"name\":\"Sprint\"}").unwrap();
        fs::write(dir.join("0000000001.json"), "{\"name\":\"sprint\"}").unwrap();
        assert_eq!(
            registry.ids_named("SPRINT").unwrap(),
            vec!["0000000001".to_string(), "ZZZZZZZZZZ".to_string()]
        );
        let (id, name, created) = registry.find_or_create("Sprint").unwrap();
        assert_eq!(
            (id.as_str(), name.as_str(), created),
            ("0000000001", "sprint", false)
        );
    }
}
