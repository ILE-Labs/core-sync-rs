//! End-to-end sync: chunk → fetch remote manifest → diff → upload → publish manifest.

use crate::chunker;
use crate::error::Result;
use crate::indexd::ManifestStore;
use crate::manifest::FileManifest;
use crate::payload::{self, DeltaPayload};
use crate::sia::{StorageBackend, UploadReceipt};
use crate::sync_engine::{self, SyncPlan};
use std::path::Path;

/// Configuration for a single-file sync operation.
#[derive(Debug, Clone)]
pub struct SyncOptions {
    /// Logical object key on the Sia indexer (e.g. `backups/dataset.bin`).
    pub object_key: String,
}

/// Outcome of a completed sync pipeline run.
#[derive(Debug, Clone)]
pub struct SyncReport {
    /// Object key that was synced.
    pub object_key: String,
    /// Local manifest after chunking the current file.
    pub local_manifest: FileManifest,
    /// Remote manifest used for diffing (`None` on first upload).
    pub remote_manifest: Option<FileManifest>,
    /// Computed upload plan.
    pub plan: SyncPlan,
    /// Assembled delta handed to storage.
    pub delta: DeltaPayload,
    /// Receipt from the storage upload step.
    pub upload: UploadReceipt,
    /// Whether this was an initial upload (no prior remote manifest).
    pub initial_upload: bool,
}

impl SyncReport {
    /// Fraction of local chunks reused from the remote (0.0 – 1.0).
    #[must_use]
    pub fn reuse_ratio(&self) -> f64 {
        self.plan.reuse_ratio()
    }

    /// Bytes saved compared to uploading the full local file.
    #[must_use]
    pub fn bandwidth_saved_bytes(&self) -> u64 {
        self.plan
            .bandwidth_saved_bytes(self.local_manifest.file_size)
    }
}

/// Runs the full CoreSync → Sia SDK → indexd pipeline for a local file.
///
/// # Errors
///
/// Returns an error if chunking, manifest validation, delta assembly, upload,
/// or manifest registration fails.
pub fn sync_file(
    local_path: &Path,
    options: &SyncOptions,
    manifests: &dyn ManifestStore,
    storage: &dyn StorageBackend,
) -> Result<SyncReport> {
    let local_manifest = chunker::process_file(local_path)?;
    local_manifest.validate()?;

    let remote_manifest = manifests.get_manifest(&options.object_key)?;
    if let Some(ref remote) = remote_manifest {
        remote.validate()?;
    }
    let initial_upload = remote_manifest.is_none();

    let empty_remote = FileManifest::new(&options.object_key);
    let remote = remote_manifest.as_ref().unwrap_or(&empty_remote);
    let plan = sync_engine::compute_diff(remote, &local_manifest);
    let delta = payload::assemble_delta(local_path, &plan)?;

    let upload = storage.upload_delta(&options.object_key, &delta)?;
    manifests.put_manifest(&options.object_key, &local_manifest)?;

    Ok(SyncReport {
        object_key: options.object_key.clone(),
        local_manifest,
        remote_manifest,
        plan,
        delta,
        upload,
        initial_upload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::indexd::InMemoryManifestStore;
    use crate::sia::InMemoryStorageBackend;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn write_file(dir: &TempDir, name: &str, data: &[u8]) -> std::path::PathBuf {
        let path = dir.path().join(name);
        let mut file = File::create(&path).unwrap();
        file.write_all(data).unwrap();
        path
    }

    #[test]
    fn initial_upload_chunks_and_registers_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "v1.bin", &chunker::varied_data(20_000));

        let store = InMemoryManifestStore::new();
        let storage = InMemoryStorageBackend::new();

        let report = sync_file(
            &path,
            &SyncOptions {
                object_key: "data/v1.bin".into(),
            },
            &store,
            &storage,
        )
        .unwrap();

        assert!(report.initial_upload);
        assert!(report.upload.bytes_uploaded > 0);
        assert!(store.get_manifest("data/v1.bin").unwrap().is_some());
    }

    #[test]
    fn subsequent_sync_uploads_only_delta() {
        let dir = TempDir::new().unwrap();
        let v1_path = write_file(&dir, "v1.bin", &chunker::varied_data(50_000));

        let store = InMemoryManifestStore::new();
        let storage = InMemoryStorageBackend::new();

        sync_file(
            &v1_path,
            &SyncOptions {
                object_key: "data/dataset.bin".into(),
            },
            &store,
            &storage,
        )
        .unwrap();

        let mut v2_data = chunker::varied_data(50_000);
        v2_data.extend(chunker::varied_data(10_000));
        let v2_path = write_file(&dir, "v2.bin", &v2_data);

        let report = sync_file(
            &v2_path,
            &SyncOptions {
                object_key: "data/dataset.bin".into(),
            },
            &store,
            &storage,
        )
        .unwrap();

        assert!(!report.initial_upload);
        assert!(report.plan.reused_count() > 0);
        assert!(report.upload.bytes_uploaded < v2_data.len());
        assert!(report.reuse_ratio() > 0.5);
    }

    struct InvalidRemoteStore;

    impl ManifestStore for InvalidRemoteStore {
        fn get_manifest(&self, _object_key: &str) -> Result<Option<FileManifest>> {
            let mut manifest = FileManifest::new("corrupt.bin");
            manifest.add_chunk(crate::manifest::ChunkMeta {
                hash: "aaa".into(),
                offset: 0,
                length: 100,
            });
            manifest.add_chunk(crate::manifest::ChunkMeta {
                hash: "bbb".into(),
                offset: 200,
                length: 100,
            });
            Ok(Some(manifest))
        }

        fn put_manifest(&self, _object_key: &str, _manifest: &FileManifest) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn identical_file_sync_uploads_nothing_but_publishes_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "same.bin", &chunker::varied_data(20_000));

        let store = InMemoryManifestStore::new();
        let storage = InMemoryStorageBackend::new();
        let key = "data/same.bin";

        sync_file(
            &path,
            &SyncOptions {
                object_key: key.into(),
            },
            &store,
            &storage,
        )
        .unwrap();

        let report = sync_file(
            &path,
            &SyncOptions {
                object_key: key.into(),
            },
            &store,
            &storage,
        )
        .unwrap();

        assert!(!report.initial_upload);
        assert_eq!(report.plan.upload_count(), 0);
        assert_eq!(report.upload.bytes_uploaded, 0);
        assert!(report.delta.is_empty());
        assert!(store.get_manifest(key).unwrap().is_some());
    }

    #[test]
    fn rejects_malformed_remote_manifest() {
        let dir = TempDir::new().unwrap();
        let path = write_file(&dir, "v1.bin", &chunker::varied_data(10_000));
        let storage = InMemoryStorageBackend::new();

        let err = sync_file(
            &path,
            &SyncOptions {
                object_key: "data/corrupt.bin".into(),
            },
            &InvalidRemoteStore,
            &storage,
        )
        .unwrap_err();

        assert!(matches!(
            err,
            crate::error::CoreSyncError::InvalidManifest(_)
        ));
    }
}
