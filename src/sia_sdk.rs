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
use sha2::{Digest, Sha256};
use sia_storage::{
    app_id, AppKey, AppMetadata, Builder, Hash256, Object, Sdk, UploadOptions,
};
use std::collections::HashMap;
use std::env;
use std::io::Cursor;
use std::sync::{PoisonError, RwLock};
use tokio::runtime::Runtime;

/// Maximum metadata size indexd accepts on a pinned object (see indexd OpenAPI).
const MAX_OBJECT_METADATA_BYTES: usize = 1024;

/// Environment variable for the indexd URL used by the SDK.
pub const SIA_INDEXER_URL_ENV: &str = "SIA_INDEXER_URL";

/// Environment variable for an exported `AppKey` (64-char hex, 32 bytes).
pub const SIA_APP_KEY_ENV: &str = "SIA_APP_KEY";

/// Stable application identity for `Builder::connected`. Must never change in production.
const APP_META: AppMetadata = AppMetadata {
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
    objects: RwLock<HashMap<String, Object>>,
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
                            "app key not registered with indexer — run the SDK approval flow first"
                                .into(),
                        )
                    })
            })?;

        Ok(Self {
            sdk,
            rt,
            objects: RwLock::new(HashMap::new()),
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

        let id = object_key_hash(object_key);
        match self.block_on_storage(self.sdk.object(&id)) {
            Ok(object) => {
                self.store_object(object_key, object.clone())?;
                Ok(Some(object))
            }
            Err(err) if object_not_found(&err) => Ok(None),
            Err(err) => Err(err),
        }
    }

    fn manifest_from_object(object: &Object) -> Result<Option<FileManifest>> {
        if object.metadata.is_empty() {
            return Ok(None);
        }

        let record = ManifestRecord::from_json(
            std::str::from_utf8(&object.metadata).map_err(|e| CoreSyncError::Indexd(e.to_string()))?,
        )?;
        record.manifest.validate()?;
        Ok(Some(record.manifest))
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
        let record = ManifestRecord::new(manifest.clone());
        let json = record.to_json()?;
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

/// Maps a logical CoreSync object key to the `Hash256` id used by indexd.
#[must_use]
pub fn object_key_hash(object_key: &str) -> Hash256 {
    let digest = Sha256::digest(object_key.as_bytes());
    Hash256::from(<[u8; 32]>::try_from(digest.as_slice()).expect("sha256 is 32 bytes"))
}

fn parse_app_key_hex(input: &str) -> Result<AppKey> {
    let bytes = hex::decode(input.trim()).map_err(|e| CoreSyncError::Storage(e.to_string()))?;
    let seed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| CoreSyncError::Storage(format!("{SIA_APP_KEY_ENV} must be 64 hex characters (32 bytes)")))?;
    Ok(AppKey::import(seed))
}

fn object_not_found(err: &CoreSyncError) -> bool {
    let CoreSyncError::Storage(msg) = err else {
        return false;
    };
    let lower = msg.to_ascii_lowercase();
    lower.contains("not found") || lower.contains("404")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn object_key_hash_is_stable() {
        let a = object_key_hash("backups/dataset.bin");
        let b = object_key_hash("backups/dataset.bin");
        assert_eq!(a, b);
        assert_ne!(a, object_key_hash("other-key"));
    }

    #[test]
    fn rejects_invalid_app_key_length() {
        assert!(matches!(
            parse_app_key_hex("abcd"),
            Err(CoreSyncError::Storage(_))
        ));
    }
}
