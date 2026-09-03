//! Machine-local checkout discovery. A checkout is a working directory, not a git or
//! ticket-store identity: several checkouts may share one repository and each checkout
//! may use several ticket stores (and vice versa).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkout {
    pub id: String,
    pub root: String,
    pub alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default)]
    pub stores: Vec<String>,
    #[serde(default)]
    pub sources: Vec<TicketSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketSource {
    pub connection_id: String,
    pub provider: String,
    pub locator: String,
}

impl TicketSource {
    pub fn git(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let canonical = path.canonicalize().unwrap_or(path);
        let locator = canonical.to_string_lossy().into_owned();
        let hash = format!("{:x}", Sha256::digest(locator.as_bytes()));
        Self {
            connection_id: hash[..16].to_string(),
            provider: "git".into(),
            locator,
        }
    }
}

impl Checkout {
    pub fn source(&self, connection_id: &str) -> Option<&TicketSource> {
        self.sources
            .iter()
            .find(|source| source.connection_id == connection_id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default = "registry_schema_version", rename = "schemaVersion")]
    schema_version: u64,
    #[serde(default)]
    checkouts: Vec<Checkout>,
}

const CHECKOUT_REGISTRY_SCHEMA_VERSION: u64 = 2;
const fn registry_schema_version() -> u64 {
    CHECKOUT_REGISTRY_SCHEMA_VERSION
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            schema_version: CHECKOUT_REGISTRY_SCHEMA_VERSION,
            checkouts: Vec::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum CheckoutError {
    #[error("checkout path does not exist: {0}")]
    Missing(String),
    #[error("no checkout matches {0}")]
    NotFound(String),
    #[error("checkout reference is ambiguous: {0}")]
    Ambiguous(String),
    #[error("invalid checkout registry: {0}")]
    Invalid(String),
    #[error(
        "This project registry was created by a newer version of Hot Sheet 2 and cannot be opened by this version. Update Hot Sheet 2 to open it (found schema {found}, supported through {supported})."
    )]
    UpgradeRequired { found: u64, supported: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// Discover conventional git-backed ticket stores for a checkout. The first convention
/// is a sibling whose path is the checkout path plus `.hs2` (for example `app` and
/// `app.hs2`). Discovery is deliberately conservative: it never creates a store and never
/// guesses that one checkout maps to only one provider.
pub fn discover_ticket_stores(root: &Path) -> Result<Vec<PathBuf>, CheckoutError> {
    let canonical = root
        .canonicalize()
        .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
    let sibling = PathBuf::from(format!("{}.hs2", canonical.display()));
    if sibling.join("hotsheet-store.json").is_file() {
        Ok(vec![sibling.canonicalize().unwrap_or(sibling)])
    } else {
        Ok(Vec::new())
    }
}

/// Stable, discoverable checkout id: basename plus twelve hex chars from the canonical
/// absolute path. It deliberately is not a secret and changes when the checkout moves.
pub fn checkout_id(root: &Path) -> Result<String, CheckoutError> {
    let canonical = root
        .canonicalize()
        .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
    let name = canonical
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("checkout");
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let hash = format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    );
    Ok(format!("{}-{}", slug.trim_matches('-'), &hash[..12]))
}

#[derive(Debug, Clone)]
pub struct CheckoutRegistry {
    path: PathBuf,
}

impl CheckoutRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn list(&self) -> Result<Vec<Checkout>, CheckoutError> {
        let mut entries = self.read()?.checkouts;
        entries.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        Ok(entries)
    }

    pub fn register(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        stores: Vec<PathBuf>,
    ) -> Result<Checkout, CheckoutError> {
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let alias = alias.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("checkout")
                .to_owned()
        });
        let sources = stores
            .into_iter()
            .map(TicketSource::git)
            .collect::<Vec<_>>();
        let default_source = legacy_default_source(&root, &sources)
            .or_else(|| (sources.len() == 1).then(|| sources[0].connection_id.clone()));
        self.register_sources(
            root.as_path(),
            Some(&alias),
            repository,
            sources,
            default_source,
        )
    }

    pub fn register_sources(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        mut sources: Vec<TicketSource>,
        mut default_source: Option<String>,
    ) -> Result<Checkout, CheckoutError> {
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let id = checkout_id(&root)?;
        let alias = alias.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("checkout")
                .to_owned()
        });
        for source in &mut sources {
            if source.provider == "git" {
                let normalized = TicketSource::git(&source.locator);
                if default_source.as_deref() == Some(&source.connection_id) {
                    default_source = Some(normalized.connection_id.clone());
                }
                *source = normalized;
            }
        }
        sources.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        sources.dedup_by(|a, b| a.connection_id == b.connection_id);
        if let Some(default) = &default_source
            && !sources
                .iter()
                .any(|source| &source.connection_id == default)
        {
            return Err(CheckoutError::Invalid(format!(
                "default source '{default}' is not associated with checkout {id}"
            )));
        }
        let mut store_strings = sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.locator.clone())
            .collect::<Vec<_>>();
        store_strings.sort();
        let entry = Checkout {
            id: id.clone(),
            root: root.to_string_lossy().into_owned(),
            alias,
            repository,
            stores: store_strings,
            sources,
            default_source,
        };
        let mut file = self.read()?;
        if let Some(existing) = file.checkouts.iter_mut().find(|c| c.id == id) {
            *existing = entry.clone();
        } else {
            file.checkouts.push(entry.clone());
        }
        self.write(&file)?;
        Ok(entry)
    }

    pub fn resolve(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        let canonical = Path::new(reference).canonicalize().ok();
        let entries = self.list()?;
        let matches: Vec<_> = entries
            .into_iter()
            .filter(|c| {
                c.id == reference
                    || c.id.starts_with(reference)
                    || c.alias == reference
                    || canonical
                        .as_ref()
                        .is_some_and(|p| c.root == p.to_string_lossy())
            })
            .collect();
        match matches.as_slice() {
            [one] => Ok(one.clone()),
            [] => Err(CheckoutError::NotFound(reference.to_owned())),
            _ => Err(CheckoutError::Ambiguous(reference.to_owned())),
        }
    }

    pub fn add_source(
        &self,
        reference: &str,
        source: TicketSource,
        make_default: bool,
    ) -> Result<Checkout, CheckoutError> {
        let mut checkout = self.resolve(reference)?;
        checkout
            .sources
            .retain(|item| item.connection_id != source.connection_id);
        let source_id = source.connection_id.clone();
        checkout.sources.push(source);
        if make_default {
            checkout.default_source = Some(source_id);
        }
        self.register_sources(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            checkout.default_source,
        )
    }

    pub fn remove_source(
        &self,
        reference: &str,
        connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        let mut checkout = self.resolve(reference)?;
        let before = checkout.sources.len();
        checkout
            .sources
            .retain(|source| source.connection_id != connection_id);
        if checkout.sources.len() == before {
            return Err(CheckoutError::NotFound(connection_id.into()));
        }
        if checkout.default_source.as_deref() == Some(connection_id) {
            checkout.default_source = None;
        }
        self.register_sources(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            checkout.default_source,
        )
    }

    pub fn set_default_source(
        &self,
        reference: &str,
        connection_id: Option<&str>,
    ) -> Result<Checkout, CheckoutError> {
        let checkout = self.resolve(reference)?;
        self.register_sources(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            connection_id.map(str::to_owned),
        )
    }

    fn read(&self) -> Result<RegistryFile, CheckoutError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RegistryFile::default());
            }
            Err(e) => return Err(e.into()),
        };
        let file: RegistryFile =
            serde_json::from_str(&text).map_err(|e| CheckoutError::Invalid(e.to_string()))?;
        if file.schema_version > CHECKOUT_REGISTRY_SCHEMA_VERSION {
            return Err(CheckoutError::UpgradeRequired {
                found: file.schema_version,
                supported: CHECKOUT_REGISTRY_SCHEMA_VERSION,
            });
        }
        let mut file = file;
        file.schema_version = CHECKOUT_REGISTRY_SCHEMA_VERSION;
        for checkout in &mut file.checkouts {
            migrate_checkout(checkout);
        }
        Ok(file)
    }

    fn write(&self, file: &RegistryFile) -> Result<(), CheckoutError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self.path.with_extension("json.tmp");
        std::fs::write(
            &tmp,
            serde_json::to_vec_pretty(file).expect("serializable registry"),
        )?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }
}

fn legacy_default_source(root: &Path, sources: &[TicketSource]) -> Option<String> {
    let linked = legacy_link_source(root)?;
    sources
        .iter()
        .find(|source| source.connection_id == linked.connection_id)
        .map(|source| source.connection_id.clone())
}

fn legacy_link_source(root: &Path) -> Option<TicketSource> {
    let link = std::fs::read_to_string(root.join(".hotsheet/store")).ok()?;
    let path = PathBuf::from(link.trim());
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    Some(TicketSource::git(path))
}

fn migrate_checkout(checkout: &mut Checkout) {
    if checkout.sources.is_empty() {
        checkout.sources = checkout.stores.iter().map(TicketSource::git).collect();
        if checkout.sources.is_empty()
            && let Some(source) = legacy_link_source(Path::new(&checkout.root))
        {
            checkout.sources.push(source);
        }
    }
    if checkout.stores.is_empty() {
        checkout.stores = checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.locator.clone())
            .collect();
    }
    if checkout
        .default_source
        .as_ref()
        .is_some_and(|default| checkout.source(default).is_none())
    {
        checkout.default_source = None;
    }
    if checkout.default_source.is_none() {
        checkout.default_source =
            legacy_default_source(Path::new(&checkout.root), &checkout.sources).or_else(|| {
                (checkout.sources.len() == 1).then(|| checkout.sources[0].connection_id.clone())
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_readable_stable_and_distinguish_checkouts() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("project");
        let b = temp.path().join("other").join("project");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert!(checkout_id(&a).unwrap().starts_with("project-"));
        assert_eq!(checkout_id(&a).unwrap(), checkout_id(&a).unwrap());
        assert_ne!(checkout_id(&a).unwrap(), checkout_id(&b).unwrap());
    }

    #[test]
    fn registry_supports_many_to_many_and_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let store_a = temp.path().join("tickets-a");
        let store_b = temp.path().join("tickets-b");
        for path in [&checkout, &store_a, &store_b] {
            std::fs::create_dir(path).unwrap();
        }
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let saved = registry
            .register(
                &checkout,
                Some("frontend"),
                Some("github.com/acme/app".into()),
                vec![store_a, store_b],
            )
            .unwrap();
        assert_eq!(saved.stores.len(), 2);
        assert_eq!(saved.sources.len(), 2);
        assert!(saved.default_source.is_none());
        assert_eq!(registry.resolve("frontend").unwrap(), saved);
        assert_eq!(registry.resolve(&saved.id[..8]).unwrap(), saved);
        assert_eq!(registry.resolve(checkout.to_str().unwrap()).unwrap(), saved);
    }

    #[test]
    fn unversioned_registry_remains_readable_and_future_registry_requires_upgrade() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("checkouts.json");
        std::fs::write(&path, r#"{"checkouts":[]}"#).unwrap();
        let registry = CheckoutRegistry::new(&path);
        assert!(registry.list().unwrap().is_empty());
        std::fs::write(&path, r#"{"schemaVersion":99,"checkouts":[]}"#).unwrap();
        let error = registry.list().unwrap_err().to_string();
        assert!(error.contains("newer version of Hot Sheet 2"));
        assert!(error.contains("Update Hot Sheet 2"));
    }

    #[test]
    fn migrates_legacy_stores_and_link_into_explicit_sources_and_default() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let store_a = temp.path().join("a.hs2");
        let store_b = temp.path().join("b.hs2");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::create_dir(&store_a).unwrap();
        std::fs::create_dir(&store_b).unwrap();
        std::fs::create_dir(checkout.join(".hotsheet")).unwrap();
        std::fs::write(
            checkout.join(".hotsheet/store"),
            store_b.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let path = temp.path().join("checkouts.json");
        std::fs::write(
            &path,
            serde_json::json!({"schemaVersion":1,"checkouts":[{
                "id":checkout_id(&checkout).unwrap(),"root":checkout,"alias":"app",
                "stores":[store_a,store_b]
            }]})
            .to_string(),
        )
        .unwrap();
        let migrated = CheckoutRegistry::new(&path).list().unwrap().remove(0);
        assert_eq!(migrated.sources.len(), 2);
        assert_eq!(
            migrated
                .source(migrated.default_source.as_deref().unwrap())
                .unwrap()
                .locator,
            store_b.canonicalize().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn external_sources_defaults_and_removal_are_explicit_and_many_to_many() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let external = TicketSource {
            connection_id: "github-acme".into(),
            provider: "github".into(),
            locator: "acme/issues".into(),
        };
        for root in [&first, &second] {
            registry
                .register_sources(
                    root,
                    None,
                    None,
                    vec![external.clone()],
                    Some("github-acme".into()),
                )
                .unwrap();
        }
        assert_eq!(registry.list().unwrap().len(), 2);
        let alternate = TicketSource {
            connection_id: "jira-eng".into(),
            provider: "jira".into(),
            locator: "ENG".into(),
        };
        registry
            .add_source(first.to_str().unwrap(), alternate, false)
            .unwrap();
        let changed = registry
            .set_default_source(first.to_str().unwrap(), Some("jira-eng"))
            .unwrap();
        assert_eq!(changed.default_source.as_deref(), Some("jira-eng"));
        let removed_default = registry
            .remove_source(first.to_str().unwrap(), "jira-eng")
            .unwrap();
        assert!(removed_default.default_source.is_none());
        let changed = registry
            .set_default_source(first.to_str().unwrap(), None)
            .unwrap();
        assert!(changed.default_source.is_none());
        let removed = registry
            .remove_source(first.to_str().unwrap(), "github-acme")
            .unwrap();
        assert!(removed.sources.is_empty());
        assert!(
            registry
                .set_default_source(second.to_str().unwrap(), Some("missing"))
                .unwrap_err()
                .to_string()
                .contains("not associated")
        );
    }

    #[test]
    fn discovers_only_a_valid_parallel_hs2_store() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let sibling = temp.path().join("app.hs2");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        assert!(discover_ticket_stores(&checkout).unwrap().is_empty());
        std::fs::write(sibling.join("hotsheet-store.json"), "{}").unwrap();
        assert_eq!(
            discover_ticket_stores(&checkout).unwrap(),
            vec![sibling.canonicalize().unwrap()]
        );
    }
}
