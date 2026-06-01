//! Delta payload assembly from sync plans.

use crate::chunker;
use crate::error::{CoreSyncError, Result};
use crate::sync_engine::{ChunkUploadRef, SyncPlan};
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// A chunk payload ready to hand off to a storage or network layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkPayload {
    /// SHA-256 hash of the data.
    pub hash: String,
    /// Byte offset within the source file.
    pub offset: u64,
    /// Raw chunk bytes.
    pub data: Vec<u8>,
}

impl ChunkPayload {
    /// Length of the chunk payload in bytes.
    #[must_use]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns `true` if the payload contains no bytes.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }
}

/// An assembled delta consisting of all chunks that must be uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeltaPayload {
    /// Ordered list of chunk payloads to upload.
    pub chunks: Vec<ChunkPayload>,
}

impl DeltaPayload {
    /// Number of chunks in this delta.
    #[must_use]
    pub fn len(&self) -> usize {
        self.chunks.len()
    }

    /// Returns `true` if there is nothing to upload.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.chunks.is_empty()
    }

    /// Total byte size across all chunk payloads.
    #[must_use]
    pub fn total_bytes(&self) -> usize {
        self.chunks.iter().map(ChunkPayload::len).sum()
    }
}

/// Reads chunk byte ranges from a local file and assembles the upload delta.
///
/// Reads only the byte ranges listed in the sync plan — the full file is never
/// loaded into memory.
pub fn assemble_delta<P: AsRef<Path>>(local_path: P, plan: &SyncPlan) -> Result<DeltaPayload> {
    let path = local_path.as_ref();
    let path_str = path.to_string_lossy().to_string();

    let mut file = File::open(path).map_err(|source| CoreSyncError::Io {
        path: path_str.clone(),
        source,
    })?;

    let file_size = file
        .metadata()
        .map_err(|source| CoreSyncError::Io {
            path: path_str.clone(),
            source,
        })?
        .len();

    read_upload_chunks(&mut file, &path_str, file_size, &plan.uploads)
}

/// Assembles a delta from an in-memory file buffer.
///
/// Prefer [`assemble_delta`] for on-disk files so only upload ranges are held in memory.
pub fn assemble_delta_from_bytes(
    file_data: &[u8],
    uploads: &[ChunkUploadRef],
) -> Result<DeltaPayload> {
    let file_size = file_data.len() as u64;
    let mut chunks = Vec::with_capacity(uploads.len());

    for upload in uploads {
        validate_chunk_bounds(upload, file_size)?;

        let start =
            usize::try_from(upload.offset).map_err(|_| CoreSyncError::ChunkOutOfBounds {
                offset: upload.offset,
                length: upload.length,
                file_size,
            })?;
        let end = start + upload.length;
        let data = file_data[start..end].to_vec();
        chunks.push(verify_chunk_payload(upload, data)?);
    }

    Ok(DeltaPayload { chunks })
}

fn read_upload_chunks(
    file: &mut File,
    path: &str,
    file_size: u64,
    uploads: &[ChunkUploadRef],
) -> Result<DeltaPayload> {
    let mut chunks = Vec::with_capacity(uploads.len());

    for upload in uploads {
        validate_chunk_bounds(upload, file_size)?;

        let mut buf = vec![0u8; upload.length];
        file.seek(SeekFrom::Start(upload.offset))
            .map_err(|source| CoreSyncError::Io {
                path: path.to_string(),
                source,
            })?;
        file.read_exact(&mut buf)
            .map_err(|source| CoreSyncError::Io {
                path: path.to_string(),
                source,
            })?;

        chunks.push(verify_chunk_payload(upload, buf)?);
    }

    Ok(DeltaPayload { chunks })
}

fn verify_chunk_payload(upload: &ChunkUploadRef, data: Vec<u8>) -> Result<ChunkPayload> {
    let computed = chunker::hash_chunk(&data);
    if computed != upload.hash {
        return Err(CoreSyncError::InvalidManifest(format!(
            "hash mismatch at offset {}: expected {}, computed {}",
            upload.offset, upload.hash, computed
        )));
    }

    Ok(ChunkPayload {
        hash: computed,
        offset: upload.offset,
        data,
    })
}

fn validate_chunk_bounds(upload: &ChunkUploadRef, file_size: u64) -> Result<()> {
    let end =
        upload
            .offset
            .checked_add(upload.length as u64)
            .ok_or(CoreSyncError::ChunkOutOfBounds {
                offset: upload.offset,
                length: upload.length,
                file_size,
            })?;

    if end > file_size {
        return Err(CoreSyncError::ChunkOutOfBounds {
            offset: upload.offset,
            length: upload.length,
            file_size,
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chunker;
    use crate::sync_engine::compute_diff;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn assembled_delta_matches_upload_plan() {
        let v1 = chunker::varied_data(50_000);
        let mut v2 = v1.clone();
        v2.extend(chunker::varied_data(10_000));

        let remote = chunker::chunk_bytes(&v1, "v1.bin").unwrap();
        let local = chunker::chunk_bytes(&v2, "v2.bin").unwrap();
        let plan = compute_diff(&remote, &local);

        let delta = assemble_delta_from_bytes(&v2, &plan.uploads).unwrap();
        assert_eq!(delta.len(), plan.upload_count());
        assert_eq!(delta.total_bytes(), plan.payload_size());

        for (payload, upload_ref) in delta.chunks.iter().zip(plan.uploads.iter()) {
            assert_eq!(payload.hash, upload_ref.hash);
            assert_eq!(
                payload.data,
                v2[upload_ref.offset as usize..][..upload_ref.length]
            );
        }
    }

    #[test]
    fn assemble_delta_reads_only_upload_ranges_from_disk() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("data.bin");
        let data = chunker::varied_data(50_000);
        let mut file = File::create(&path).unwrap();
        file.write_all(&data).unwrap();

        let remote = chunker::chunk_bytes(&data[..40_000], "remote.bin").unwrap();
        let local = chunker::chunk_bytes(&data, "local.bin").unwrap();
        let plan = compute_diff(&remote, &local);

        let delta = assemble_delta(&path, &plan).unwrap();
        assert_eq!(delta.total_bytes(), plan.payload_size());
        assert!(!delta.is_empty());
    }

    #[test]
    fn rejects_out_of_bounds_chunk() {
        let uploads = vec![ChunkUploadRef {
            hash: "abc".into(),
            offset: 100,
            length: 50,
        }];
        let data = vec![0u8; 100];
        assert!(assemble_delta_from_bytes(&data, &uploads).is_err());
    }

    #[test]
    fn rejects_hash_mismatch_at_integrity_checkpoint() {
        let data = chunker::varied_data(1_024);
        let uploads = vec![ChunkUploadRef {
            hash: "deadbeef".into(),
            offset: 0,
            length: data.len(),
        }];
        let err = assemble_delta_from_bytes(&data, &uploads).unwrap_err();
        assert!(matches!(
            err,
            CoreSyncError::InvalidManifest(msg) if msg.contains("hash mismatch")
        ));
    }
}
