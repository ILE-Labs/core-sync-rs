//! SDK-backed Sia integration via `sia_storage::Sdk`.
//!
//! Implements [`StorageBackend`] and [`ManifestStore`] using the official Sia SDK:
//! delta bytes go through `Sdk::upload` + `pin_object`, manifests live in object
//! metadata via `Sdk::update_object_metadata`.

use crate::error::{CoreSyncError, Result};
use crate::indexd::{ManifestRecord, ManifestStore, MANIFEST_METADATA_KEY};
use crate::manifest::FileManifest;
use crate::payload::DeltaPayload;
use crate::sia::{pack_delta_stream, StorageBackend, UploadReceipt};
use sia_storage::{app_id, AppKey, AppMetadata, Builder, Object, ObjectsCursor, Sdk, UploadOptions};
use std::env;
use std::io::Cursor;
use std::sync::{PoisonError, RwLock};
use tokio::runtime::Runtime;

/// Maximum metadata size indexd accepts on a pinned object (see indexd OpenAPI).
const MAX_OBJECT_METADATA_BYTES: usize = 1024;
const OBJECT_PAGE_SIZE: usize = 100;

/// Environment variable for the indexd URL used by the SDK.
pub const SIA_INDEXER_URL_ENV: &str = "SIA_INDEXER_URL";

/// Environment variable for an exported `AppKey` (64-char hex, 32 bytes).
pub const SIA_APP_KEY_ENV: &str = "SIA_APP_KEY";

/// Stable application identity for `Builder::connected`. Must never change in production.
pub const APP_META: AppMetadata = AppMetadata {
    id: app_id!("c0e5790c5e796e63000000000000000000000000000000000000000000000001"),
    name: "core-sync-rs",
    description: "Local differential sync for Sia",
    service_url: "https://github.com/ile-labs/core-sync-rs",
    logo_url: None,
    callback_url: None,
};

/// Configuration for the SDK-backed adapter.
#[derive(Clone)]
pub struct SdkSyncConfig {
    /// Base URL of the indexd service (e.g. `http://localhost:9982`).
    pub indexer_url: String,
    /// Exported application key for authentication.
    pub app_key: AppKey,
}

impl SdkSyncConfig {
    /// Loads SDK configuration from environment variables.
    pub fn from_env() -> Result<Self> {
        let indexer_url = env::var(SIA_INDEXER_URL_ENV).map_err(|_| {
            CoreSyncError::Storage(format!(
                "missing environment variable {SIA_INDEXER_URL_ENV}"
            ))
        })?;
        let app_key_hex = env::var(SIA_APP_KEY_ENV).map_err(|_| {
            CoreSyncError::Storage(format!("missing environment variable {SIA_APP_KEY_ENV}"))
        })?;
        let app_key = parse_app_key_hex(&app_key_hex)?;

        Ok(Self {
            indexer_url,
            app_key,
        })
    }
}

/// SDK-backed adapter implementing both storage upload and manifest persistence.
pub struct SdkSyncAdapter {
    sdk: Sdk,
    rt: Runtime,
    objects: RwLock<std::collections::HashMap<String, Object>>,
}

impl SdkSyncAdapter {
    /// Connects to indexd and returns an adapter ready for `sync_file`.
    pub fn connect(config: SdkSyncConfig) -> Result<Self> {
        let rt = Runtime::new().map_err(|e| CoreSyncError::Storage(e.to_string()))?;
        let sdk = rt
            .block_on(async {
                let builder = Builder::new(&config.indexer_url, APP_META)
                    .map_err(|e| CoreSyncError::Storage(e.to_string()))?;
                builder
                    .connected(&config.app_key)
                    .await
                    .map_err(|e| CoreSyncError::Storage(e.to_string()))?
                    .ok_or_else(|| {
                        CoreSyncError::Storage(
                            "app key not registered with indexer - run the SDK approval flow first"
                                .into(),
                        )
                    })
            })?;

        Ok(Self {
            sdk,
            rt,
            objects: RwLock::new(std::collections::HashMap::new()),
        })
    }

    /// Connects using environment variables.
    pub fn from_env() -> Result<Self> {
        Self::connect(SdkSyncConfig::from_env()?)
    }

    fn block_on_storage<F, T, E>(&self, fut: F) -> Result<T>
    where
        F: std::future::Future<Output = std::result::Result<T, E>>,
        E: std::fmt::Display,
    {
        self.rt
            .block_on(fut)
            .map_err(|e| CoreSyncError::Storage(e.to_string()))
    }

    fn cached_object(&self, object_key: &str) -> Result<Option<Object>> {
        let cache = self
            .objects
            .read()
            .map_err(|_: PoisonError<_>| CoreSyncError::StorageLockPoisoned)?;
        Ok(cache.get(object_key).cloned())
    }

    fn store_object(&self, object_key: &str, object: Object) -> Result<()> {
        self.objects
            .write()
            .map_err(|_: PoisonError<_>| CoreSyncError::StorageLockPoisoned)?
            .insert(object_key.to_string(), object);
        Ok(())
    }

    fn fetch_object(&self, object_key: &str) -> Result<Option<Object>> {
        if let Some(object) = self.cached_object(object_key)? {
            return Ok(Some(object));
        }

        let mut found: Option<Object> = None;
        let mut cursor = None;
        loop {
            let page = self.block_on_storage(self.sdk.object_events(cursor, Some(OBJECT_PAGE_SIZE)))?;
            if page.is_empty() {
                if let Some(object) = &found {
                    self.store_object(object_key, object.clone())?;
                }
                return Ok(found);
            }

            for event in &page {
                if let Some(object) = &event.object {
                    if let Some(stored_key) = stored_object_key(object)? {
                        if stored_key == object_key {
                            let replace = found
                                .as_ref()
                                .map(|current| object.updated_at() > current.updated_at())
                                .unwrap_or(true);
                            if replace {
                                found = Some(object.clone());
                            }
                        }
                    }
                }
            }

            if page.len() < OBJECT_PAGE_SIZE {
                if let Some(object) = found {
                    self.store_object(object_key, object.clone())?;
                    return Ok(Some(object));
                }
                return Ok(None);
            }

            let last = page.last().expect("non-empty page");
            cursor = Some(ObjectsCursor {
                after: last.updated_at,
                id: last.id,
            });
        }
    }

    fn manifest_from_object(object: &Object) -> Result<Option<FileManifest>> {
        if object.metadata.is_empty() {
            return Ok(None);
        }

        let metadata = std::str::from_utf8(&object.metadata)
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;

        if let Ok(envelope) = serde_json::from_str::<SdkManifestEnvelope>(metadata) {
            envelope.record.manifest.validate()?;
            return Ok(Some(envelope.record.manifest));
        }

        if let Ok(record) = ManifestRecord::from_json(metadata) {
            record.manifest.validate()?;
            return Ok(Some(record.manifest));
        }

        let manifest = serde_json::from_str::<FileManifest>(metadata)
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;
        manifest.validate()?;
        Ok(Some(manifest))
    }
}

impl StorageBackend for SdkSyncAdapter {
    fn upload_delta(&self, object_key: &str, delta: &DeltaPayload) -> Result<UploadReceipt> {
        if delta.is_empty() {
            return Ok(UploadReceipt {
                object_key: object_key.to_string(),
                bytes_uploaded: 0,
                chunks_uploaded: 0,
            });
        }

        let existing = self.fetch_object(object_key)?;
        let base_object = existing.unwrap_or_default();
        let packed = pack_delta_stream(delta);
        let bytes_uploaded = packed.len();
        let chunks_uploaded = delta.len();

        let updated = self.block_on_storage(self.sdk.upload(
            base_object,
            Cursor::new(packed),
            UploadOptions::default(),
        ))?;
        self.block_on_storage(self.sdk.pin_object(&updated))?;
        self.store_object(object_key, updated)?;

        Ok(UploadReceipt {
            object_key: object_key.to_string(),
            bytes_uploaded,
            chunks_uploaded,
        })
    }
}

impl ManifestStore for SdkSyncAdapter {
    fn get_manifest(&self, object_key: &str) -> Result<Option<FileManifest>> {
        let Some(object) = self.fetch_object(object_key)? else {
            return Ok(None);
        };
        Self::manifest_from_object(&object)
    }

    fn put_manifest(&self, object_key: &str, manifest: &FileManifest) -> Result<()> {
        manifest.validate()?;
        let envelope = SdkManifestEnvelope::new(object_key, manifest.clone());
        let json = serde_json::to_string(&envelope)
            .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;
        if json.len() > MAX_OBJECT_METADATA_BYTES {
            return Err(CoreSyncError::Indexd(format!(
                "manifest JSON is {} bytes; indexd metadata limit is {MAX_OBJECT_METADATA_BYTES} bytes ({MANIFEST_METADATA_KEY})",
                json.len()
            )));
        }

        let mut object = self
            .fetch_object(object_key)?
            .ok_or_else(|| CoreSyncError::Indexd(format!("no pinned object for key `{object_key}`")))?;

        object.metadata = json.into_bytes();
        self.block_on_storage(self.sdk.update_object_metadata(&object))?;
        self.store_object(object_key, object)?;
        Ok(())
    }
}

fn parse_app_key_hex(input: &str) -> Result<AppKey> {
    let bytes = hex::decode(input.trim()).map_err(|e| CoreSyncError::Storage(e.to_string()))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CoreSyncError::Storage(format!("{SIA_APP_KEY_ENV} must be 64 hex characters (32 bytes)")))?;
    Ok(AppKey::import(seed))
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
struct SdkManifestEnvelope {
    object_key: String,
    record: ManifestRecord,
}

impl SdkManifestEnvelope {
    fn new(object_key: &str, manifest: FileManifest) -> Self {
        Self {
            object_key: object_key.to_string(),
            record: ManifestRecord::new(manifest),
        }
    }
}

fn stored_object_key(object: &Object) -> Result<Option<String>> {
    if object.metadata.is_empty() {
        return Ok(None);
    }
    let metadata = std::str::from_utf8(&object.metadata)
        .map_err(|e| CoreSyncError::Indexd(e.to_string()))?;
    match serde_json::from_str::<SdkManifestEnvelope>(metadata) {
        Ok(envelope) => Ok(Some(envelope.object_key)),
        Err(_) => Ok(None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_round_trip() {
        let manifest = {
            let mut m = FileManifest::new("dataset.bin");
            m.add_chunk(crate::manifest::ChunkMeta {
                hash: "abc123".into(),
                offset: 0,
                length: 1024,
            });
            m
        };
        let envelope = SdkManifestEnvelope::new("backups/dataset.bin", manifest.clone());
        let json = serde_json::to_string(&envelope).unwrap();
        let decoded: SdkManifestEnvelope = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.object_key, "backups/dataset.bin");
        assert_eq!(decoded.record.manifest, manifest);
    }

    #[test]
    fn rejects_invalid_app_key_length() {
        assert!(matches!(
            parse_app_key_hex("abcd"),
            Err(CoreSyncError::Storage(_))
        ));
    }
}
