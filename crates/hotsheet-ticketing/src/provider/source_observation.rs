//! HS2-4J6V46: private metadata snapshot and held provider-owner guard.
use super::*;

/// Contains local configuration only, never a SecretStore value. Intentionally no
/// Debug/serde implementation: credential references and locators stay internal.
#[derive(Clone)]
pub struct ProviderConfigurationProof {
    registry: PathBuf,
    connections: Vec<ProviderConnection>,
}

impl ProviderConfigRegistry {
    pub fn observe_configuration(
        &self,
    ) -> Result<(Vec<ProviderConnection>, ProviderConfigurationProof), ProviderError> {
        self.with_configuration_observation(None, |connections, proof| {
            (connections.to_vec(), proof.clone())
        })
    }

    /// Nest after the checkout guard and before manifest/binding/remote guards.
    /// The existing stable update lock stays held until the callback returns;
    /// configuration is not changed. No transport, secrets or recursive update.
    pub fn with_configuration_observation<T>(
        &self,
        proof: Option<&ProviderConfigurationProof>,
        commit: impl FnOnce(&[ProviderConnection], &ProviderConfigurationProof) -> T,
    ) -> Result<T, ProviderError> {
        self.update(|connections| {
            if proof.is_some_and(|proof| {
                proof.registry != self.path || proof.connections != *connections
            }) {
                return Err(ProviderError::Conflict {
                    ticket: String::new(),
                    message: "provider observation changed; observe again".into(),
                });
            }
            let current = ProviderConfigurationProof {
                registry: self.path.clone(),
                connections: connections.clone(),
            };
            Ok(commit(connections, &current))
        })
    }

    /// Coalesce an owner's and a local destination's shared config lock; acquire
    /// distinct stable lock paths in deterministic order. Call only inside the
    /// checkout batch guard and before any manifest/binding/remote guard.
    pub fn with_configuration_observations<T>(
        observations: &[(&ProviderConfigRegistry, &ProviderConfigurationProof)],
        commit: impl FnOnce() -> T,
    ) -> Result<T, ProviderError> {
        if observations.len() > 2 {
            return Err(ProviderError::Conflict {
                ticket: String::new(),
                message: "provider proof limit exceeded".into(),
            });
        }
        let mut registries = std::collections::BTreeMap::new();
        for (registry, proof) in observations {
            if proof.registry != registry.path {
                return Err(ProviderError::Conflict {
                    ticket: String::new(),
                    message: "provider observation changed; observe again".into(),
                });
            }
            // Each captured proof already acquired/created its stable lock file.
            // Missing/replaced aliases fail rather than invent a new proof identity.
            let path = registry
                .path
                .with_extension("json.lock")
                .canonicalize()
                .map_err(StoreError::Io)?;
            registries.entry(path).or_insert(*registry);
        }
        let mut guards = Vec::with_capacity(registries.len());
        for registry in registries.values() {
            guards.push(registry.lock_configuration()?);
        }
        for (registry, proof) in observations {
            if registry.load()? != proof.connections {
                return Err(ProviderError::Conflict {
                    ticket: String::new(),
                    message: "provider observation changed; observe again".into(),
                });
            }
        }
        let result = commit();
        drop(guards);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;

    #[test]
    fn hs2_4j6v46_same_provider_file_coalesces_lock_but_checks_every_proof() {
        let temp = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        let (_, old) = registry.observe_configuration().unwrap();
        assert!(!registry.path.exists()); // Read/guard never creates provider data.
        ProviderConfigRegistry::with_configuration_observations(
            &[(&registry, &old), (&registry, &old)],
            || {
                let fd = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(registry.path.with_extension("json.lock"))
                    .unwrap();
                assert!(crate::FileLock::try_acquire(fd).unwrap().is_none());
            },
        )
        .unwrap();
        registry
            .update(|connections| {
                connections.push(ProviderConnection {
                    id: "synthetic".into(),
                    provider: "synthetic".into(),
                    locator: "synthetic".into(),
                    name: None,
                    default: false,
                    settings: serde_json::Value::Null,
                    disabled: false,
                });
                Ok(())
            })
            .unwrap();
        let (_, current) = registry.observe_configuration().unwrap();
        let before = std::fs::read(&registry.path).unwrap();
        assert!(
            ProviderConfigRegistry::with_configuration_observations(
                &[(&registry, &current), (&registry, &old)],
                || panic!("stale coalesced proof committed")
            )
            .is_err()
        );
        assert_eq!(std::fs::read(&registry.path).unwrap(), before);
    }

    #[test]
    fn hs2_4j6v46_distinct_provider_guards_are_held_independent_of_input_order() {
        let temp = tempfile::tempdir().unwrap();
        let a = ProviderConfigRegistry::new(temp.path().join("a.json"));
        let b = ProviderConfigRegistry::new(temp.path().join("b.json"));
        let (_, pa) = a.observe_configuration().unwrap();
        let (_, pb) = b.observe_configuration().unwrap();
        for reversed in [false, true] {
            let proofs = if reversed {
                vec![(&b, &pb), (&a, &pa)]
            } else {
                vec![(&a, &pa), (&b, &pb)]
            };
            ProviderConfigRegistry::with_configuration_observations(&proofs, || {
                for registry in [&a, &b] {
                    let fd = OpenOptions::new()
                        .read(true)
                        .write(true)
                        .open(registry.path.with_extension("json.lock"))
                        .unwrap();
                    assert!(crate::FileLock::try_acquire(fd).unwrap().is_none());
                }
            })
            .unwrap();
        }
    }
}
