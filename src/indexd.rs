//! indexd integration — manifest lookup and registration.
//!
//! CoreSync stores [`FileManifest`] records as object metadata on the Sia indexer
//! (`indexd`). Before syncing, the remote manifest is fetched; after a successful
//! upload the updated local manifest is published back.
//!
//! Production apps wire this trait to [`sia_storage::Sdk::object`] and
//! [`sia_storage::Sdk::update_object_metadata`] (see `docs/INTEGRATION.md`).

use crate::error::{CoreSyncError, Result};
use crate::manifest::FileManifest;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

/// Metadata key used when attaching CoreSync manifests to Sia `Object` metadata.
pub const MANIFEST_METADATA_KEY: &str = "coresync:manifest";

/// Schema version for serialized manifests stored on indexd.
pub const MANIFEST_SCHEMA_VERSION: u32 = 1;

/// Envelope stored in indexd object metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestRecord {
    /// Schema version for forward-compatible deserialization.
    pub version: u32,
    /// The chunk manifest for this object.
    pub manifest: FileManifest,
}

impl ManifestRecord {
    /// Wraps a manifest in a versioned record for indexd storage.
    #[must_use]
    pub fn new(manifest: FileManifest) -> Self {
        Self {
            version: MANIFEST_SCHEMA_VERSION,
            manifest,
        }
    }

    /// Serializes the record to a JSON string for object metadata.
    pub fn to_json(&self) -> Result<String> {
        serde_json::to_string(self).map_err(|e| CoreSyncError::Indexd(e.to_string()))
    }

    /// Deserializes a record from indexd object metadata JSON.
    pub fn from_json(json: &str) -> Result<Self> {
        let record: Self = serde_json::from_str(json)
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;
        if record.version != MANIFEST_SCHEMA_VERSION {
            return Err(CoreSyncError::Indexd(format!(
                "unsupported manifest schema version: {}",
                record.version
            )));
        }
        Ok(record)
    }
}

/// Abstraction over indexd manifest read/write operations.
///
/// Implement this trait to connect CoreSync to your indexer backend.
/// The in-memory implementation is used for tests and local demos.
pub trait ManifestStore: Send + Sync {
    /// Returns the remote manifest for `object_key`, if one has been registered.
    fn get_manifest(&self, object_key: &str) -> Result<Option<FileManifest>>;

    /// Publishes an updated manifest to indexd after a successful sync.
    fn put_manifest(&self, object_key: &str, manifest: &FileManifest) -> Result<()>;
}

/// In-memory manifest store for tests, demos, and offline development.
#[derive(Debug, Default)]
pub struct InMemoryManifestStore {
    records: RwLock<HashMap<String, ManifestRecord>>,
}

impl InMemoryManifestStore {
    /// Creates an empty in-memory store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Seeds the store with an initial manifest (simulates a prior upload).
    pub fn seed(&self, object_key: impl Into<String>, manifest: FileManifest) -> Result<()> {
        let key = object_key.into();
        manifest.validate()?;
        let record = ManifestRecord::new(manifest);
        self.records
            .write()
            .map_err(|_: PoisonError<_>| CoreSyncError::IndexdLockPoisoned)?
            .insert(key, record);
        Ok(())
    }
}

impl ManifestStore for InMemoryManifestStore {
    fn get_manifest(&self, object_key: &str) -> Result<Option<FileManifest>> {
        let records = self
            .records
            .read()
            .map_err(|_: PoisonError<_>| CoreSyncError::IndexdLockPoisoned)?;
        Ok(records.get(object_key).map(|r| r.manifest.clone()))
    }

    fn put_manifest(&self, object_key: &str, manifest: &FileManifest) -> Result<()> {
        manifest.validate()?;
        let record = ManifestRecord::new(manifest.clone());
        self.records
            .write()
            .map_err(|_: PoisonError<_>| CoreSyncError::IndexdLockPoisoned)?
            .insert(object_key.to_string(), record);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ChunkMeta;
    use std::sync::Arc;
    use std::thread;

    fn sample_manifest() -> FileManifest {
        let mut m = FileManifest::new("dataset.bin");
        m.add_chunk(ChunkMeta {
            hash: "abc123".into(),
            offset: 0,
            length: 1024,
        });
        m
    }

    #[test]
    fn manifest_record_round_trip() {
        let record = ManifestRecord::new(sample_manifest());
        let json = record.to_json().unwrap();
        let restored = ManifestRecord::from_json(&json).unwrap();
        assert_eq!(record, restored);
    }

    #[test]
    fn manifest_record_rejects_unsupported_schema_version() {
        let json = r#"{"version":99,"manifest":{"file_path":"x","file_size":0,"chunks":[]}}"#;
        let err = ManifestRecord::from_json(json).unwrap_err();
        assert!(matches!(err, CoreSyncError::Indexd(msg) if msg.contains("unsupported")));
    }

    #[test]
    fn in_memory_store_get_put() {
        let store = InMemoryManifestStore::new();
        let manifest = sample_manifest();

        assert!(store.get_manifest("file-a").unwrap().is_none());
        store.put_manifest("file-a", &manifest).unwrap();
        let fetched = store.get_manifest("file-a").unwrap().unwrap();
        assert_eq!(fetched, manifest);
    }

    #[test]
    fn seed_validates_and_inserts() {
        let store = InMemoryManifestStore::new();
        let manifest = sample_manifest();
        store.seed("seeded", manifest.clone()).unwrap();
        assert_eq!(
            store.get_manifest("seeded").unwrap().unwrap(),
            manifest
        );

        let mut invalid = FileManifest::new("bad");
        invalid.add_chunk(ChunkMeta {
            hash: "x".into(),
            offset: 0,
            length: 100,
        });
        invalid.add_chunk(ChunkMeta {
            hash: "y".into(),
            offset: 200,
            length: 100,
        });
        assert!(store.seed("bad", invalid).is_err());
    }

    #[test]
    fn concurrent_manifest_access() {
        let store = Arc::new(InMemoryManifestStore::new());
        let manifest = sample_manifest();
        store.put_manifest("shared", &manifest).unwrap();

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let store = Arc::clone(&store);
                let manifest = manifest.clone();
                thread::spawn(move || {
                    for _ in 0..50 {
                        let _ = store.get_manifest("shared");
                        if i % 2 == 0 {
                            let _ = store.put_manifest("shared", &manifest);
                        }
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }
}
