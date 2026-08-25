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
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default)]
    checkouts: Vec<Checkout>,
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
    #[error(transparent)]
    Io(#[from] std::io::Error),
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
        let id = checkout_id(&root)?;
        let alias = alias.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("checkout")
                .to_owned()
        });
        let mut store_strings = Vec::new();
        for store in stores {
            let canonical = store.canonicalize().unwrap_or(store);
            let value = canonical.to_string_lossy().into_owned();
            if !store_strings.contains(&value) {
                store_strings.push(value);
            }
        }
        store_strings.sort();
        let entry = Checkout {
            id: id.clone(),
            root: root.to_string_lossy().into_owned(),
            alias,
            repository,
            stores: store_strings,
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

    fn read(&self) -> Result<RegistryFile, CheckoutError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(v) => v,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(RegistryFile::default());
            }
            Err(e) => return Err(e.into()),
        };
        serde_json::from_str(&text).map_err(|e| CheckoutError::Invalid(e.to_string()))
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
        assert_eq!(registry.resolve("frontend").unwrap(), saved);
        assert_eq!(registry.resolve(&saved.id[..8]).unwrap(), saved);
        assert_eq!(registry.resolve(checkout.to_str().unwrap()).unwrap(), saved);
    }
}
