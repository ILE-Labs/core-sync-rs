//! Sia SDK integration — delta upload handoff.
//!
//! After computing a [`DeltaPayload`], this module
//! hands the minimized byte stream to a storage backend for upload.
//!
//! See `docs/INTEGRATION.md` for wiring to `sia_storage::Sdk::upload`.

use crate::error::{CoreSyncError, Result};
use crate::payload::{ChunkPayload, DeltaPayload};
use std::collections::HashMap;
use std::sync::{PoisonError, RwLock};

/// Receipt returned after a successful delta upload.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadReceipt {
    /// Object key the delta was uploaded under.
    pub object_key: String,
    /// Total bytes handed off to the storage layer.
    pub bytes_uploaded: usize,
    /// Number of chunk payloads uploaded.
    pub chunks_uploaded: usize,
}

/// Abstraction over Sia storage upload operations.
///
/// Production apps implement this by calling `sia_storage::Sdk::upload` with
/// the assembled delta bytes. The in-memory backend is for tests and demos.
pub trait StorageBackend: Send + Sync {
    /// Uploads the delta payload for `object_key` to decentralized storage.
    fn upload_delta(&self, object_key: &str, delta: &DeltaPayload) -> Result<UploadReceipt>;
}

/// In-memory storage backend for tests, demos, and offline development.
#[derive(Debug, Default)]
pub struct InMemoryStorageBackend {
    uploads: RwLock<HashMap<String, Vec<ChunkPayload>>>,
}

impl InMemoryStorageBackend {
    /// Creates an empty in-memory backend.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns uploaded chunks for an object key (test helper).
    pub fn uploaded_chunks(&self, object_key: &str) -> Result<Vec<ChunkPayload>> {
        let uploads = self
            .uploads
            .read()
            .map_err(|_: PoisonError<_>| CoreSyncError::StorageLockPoisoned)?;
        Ok(uploads.get(object_key).cloned().unwrap_or_default())
    }
}

impl StorageBackend for InMemoryStorageBackend {
    fn upload_delta(&self, object_key: &str, delta: &DeltaPayload) -> Result<UploadReceipt> {
        if delta.is_empty() {
            return Ok(UploadReceipt {
                object_key: object_key.to_string(),
                bytes_uploaded: 0,
                chunks_uploaded: 0,
            });
        }

        let mut uploads = self
            .uploads
            .write()
            .map_err(|_: PoisonError<_>| CoreSyncError::StorageLockPoisoned)?;

        uploads.insert(object_key.to_string(), delta.chunks.clone());

        Ok(UploadReceipt {
            object_key: object_key.to_string(),
            bytes_uploaded: delta.total_bytes(),
            chunks_uploaded: delta.len(),
        })
    }
}

/// Packs delta chunk payloads into a single contiguous byte buffer.
///
/// Chunks are concatenated in upload-plan order. This is not a full file
/// reconstruction — non-contiguous source offsets stay non-contiguous in the
/// packed stream.
#[must_use]
pub fn pack_delta_stream(delta: &DeltaPayload) -> Vec<u8> {
    let mut packed = Vec::with_capacity(delta.total_bytes());
    for chunk in &delta.chunks {
        packed.extend_from_slice(&chunk.data);
    }
    packed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunker;
    use crate::payload::assemble_delta_from_bytes;
    use crate::sync_engine::compute_diff;
    use std::sync::Arc;
    use std::thread;

    #[test]
    fn pack_delta_concatenates_chunks_in_plan_order() {
        let delta = DeltaPayload {
            chunks: vec![
                ChunkPayload {
                    hash: "a".into(),
                    offset: 0,
                    data: vec![1, 2, 3],
                },
                ChunkPayload {
                    hash: "b".into(),
                    offset: 100,
                    data: vec![4, 5],
                },
            ],
        };
        assert_eq!(pack_delta_stream(&delta), vec![1, 2, 3, 4, 5]);
    }

    #[test]
    fn pack_delta_matches_source_slices_in_order() {
        let data = chunker::varied_data(50_000);
        let remote = chunker::chunk_bytes(&data[..40_000], "remote.bin").unwrap();
        let local = chunker::chunk_bytes(&data, "local.bin").unwrap();
        let plan = compute_diff(&remote, &local);
        let delta = assemble_delta_from_bytes(&data, &plan.uploads).unwrap();
        let packed = pack_delta_stream(&delta);

        let mut packed_offset = 0;
        for chunk in &delta.chunks {
            let slice = &packed[packed_offset..packed_offset + chunk.len()];
            let source = &data[chunk.offset as usize..][..chunk.len()];
            assert_eq!(slice, source);
            packed_offset += chunk.len();
        }
        assert_eq!(packed_offset, packed.len());
    }

    #[test]
    fn in_memory_backend_records_uploads() {
        let backend = InMemoryStorageBackend::new();
        let delta = DeltaPayload {
            chunks: vec![ChunkPayload {
                hash: "x".into(),
                offset: 0,
                data: vec![0xAB; 100],
            }],
        };

        let receipt = backend.upload_delta("obj-1", &delta).unwrap();
        assert_eq!(receipt.bytes_uploaded, 100);
        assert_eq!(backend.uploaded_chunks("obj-1").unwrap().len(), 1);
    }

    #[test]
    fn concurrent_upload_access() {
        let backend = Arc::new(InMemoryStorageBackend::new());
        let delta = DeltaPayload {
            chunks: vec![ChunkPayload {
                hash: "h".into(),
                offset: 0,
                data: vec![1, 2, 3],
            }],
        };

        let handles: Vec<_> = (0..8)
            .map(|i| {
                let backend = Arc::clone(&backend);
                let delta = delta.clone();
                thread::spawn(move || {
                    for j in 0..50 {
                        let key = format!("obj-{i}-{j}");
                        let _ = backend.upload_delta(&key, &delta);
                        let _ = backend.uploaded_chunks(&key);
                    }
                })
            })
            .collect();

        for handle in handles {
            handle.join().unwrap();
        }
    }
}
