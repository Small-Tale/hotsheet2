//! HS2-HWC0FS: private stored-row format; public metadata stays generation-free.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use ulid::Ulid;

use super::{KeyMetadata, SecretError};

#[derive(Clone, Default, Serialize)]
#[serde(transparent)]
pub(super) struct Generation(Option<String>);

impl Generation {
    pub(super) fn fresh() -> Self {
        Self(Some(Ulid::new().to_string()))
    }
    pub(super) fn is_missing(&self) -> bool {
        self.0.is_none()
    }
    fn parse(value: &str) -> Self {
        if value.len() == 26 && Ulid::from_string(value).is_ok_and(|id| id.to_string() == value) {
            Self(Some(value.into()))
        } else {
            Self::default()
        }
    }
}

// Invalid optional generations do not invalidate otherwise legitimate old rows.
// Consume/discard invalid shapes rather than retaining arbitrary private JSON.
impl<'de> Deserialize<'de> for Generation {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = Generation;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an optional opaque credential generation")
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Generation, E> {
                Ok(Generation::parse(value))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Generation, E> {
                Ok(Generation::parse(&value))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Generation, E> {
                Ok(Generation::default())
            }
            fn visit_none<E: serde::de::Error>(self) -> Result<Generation, E> {
                Ok(Generation::default())
            }
            fn visit_bool<E: serde::de::Error>(self, _: bool) -> Result<Generation, E> {
                Ok(Generation::default())
            }
            fn visit_i64<E: serde::de::Error>(self, _: i64) -> Result<Generation, E> {
                Ok(Generation::default())
            }
            fn visit_u64<E: serde::de::Error>(self, _: u64) -> Result<Generation, E> {
                Ok(Generation::default())
            }
            fn visit_f64<E: serde::de::Error>(self, _: f64) -> Result<Generation, E> {
                Ok(Generation::default())
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut values: A,
            ) -> Result<Generation, A::Error> {
                while values.next_element::<serde::de::IgnoredAny>()?.is_some() {}
                Ok(Generation::default())
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut values: A,
            ) -> Result<Generation, A::Error> {
                while values
                    .next_entry::<serde::de::IgnoredAny, serde::de::IgnoredAny>()?
                    .is_some()
                {}
                Ok(Generation::default())
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}

#[derive(Clone, Serialize, Deserialize)]
// A duplicate known credential_generation property is malformed, not a
// generation-less legacy row. Single invalid optional values remain compatible.
pub(super) struct StoredKeyMetadata {
    #[serde(flatten)]
    pub(super) metadata: KeyMetadata,
    #[serde(default, skip_serializing_if = "Generation::is_missing")]
    pub(super) credential_generation: Generation,
}

pub(super) type StoredMetadata = BTreeMap<String, StoredKeyMetadata>;

pub(super) fn decode(bytes: &[u8]) -> Result<StoredMetadata, serde_json::Error> {
    serde_json::from_slice(bytes)
}

/// Opaque, non-serialized cache observation. It is not source/native authority.
#[derive(Clone)]
pub struct KeyMetadataCacheObservation {
    document: Option<[u8; 32]>,
    cacheable: bool,
}

impl KeyMetadataCacheObservation {
    pub fn is_cacheable(&self) -> bool {
        self.cacheable
    }
    /// Compare only the observed document, not source identity or native material.
    pub fn same_document(&self, other: &Self) -> bool {
        self.document == other.document
    }
}

/// Read/hash/decode one byte image. Missing/invalid generations never enable a cache.
/// Public get semantics remain independent of malformed metadata: it disables this
/// optimization, while real file I/O errors still propagate to the caller.
pub fn key_metadata_cache_observation(
    home: &Path,
    account: &str,
) -> Result<KeyMetadataCacheObservation, SecretError> {
    match std::fs::read(home.join("keys.json")) {
        Ok(bytes) => {
            let cacheable = decode(&bytes)
                .ok()
                .and_then(|map| {
                    map.get(account).map(|entry| {
                        entry.metadata.provider == account
                            && !entry.credential_generation.is_missing()
                    })
                })
                .unwrap_or(false);
            Ok(KeyMetadataCacheObservation {
                document: Some(Sha256::digest(&bytes).into()),
                cacheable,
            })
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            Ok(KeyMetadataCacheObservation {
                document: None,
                cacheable: false,
            })
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;
