//! Manifest diffing and sync plan generation.

use crate::manifest::{ChunkMeta, FileManifest};
use std::collections::HashSet;

/// Reference to a chunk that must be uploaded to the remote.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkUploadRef {
    /// SHA-256 hash of the chunk payload.
    pub hash: String,
    /// Byte offset within the local source file.
    pub offset: u64,
    /// Length of the chunk in bytes.
    pub length: usize,
}

/// A computed plan describing which local chunks need to be uploaded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SyncPlan {
    /// Chunks present locally but absent from the remote manifest.
    pub uploads: Vec<ChunkUploadRef>,
    /// Chunks already known to the remote (no upload required).
    pub reused: Vec<ChunkMeta>,
}

impl SyncPlan {
    /// Number of chunks that require upload.
    #[must_use]
    pub fn upload_count(&self) -> usize {
        self.uploads.len()
    }

    /// Number of chunks reused from the remote.
    #[must_use]
    pub fn reused_count(&self) -> usize {
        self.reused.len()
    }

    /// Total byte size of the upload payload.
    #[must_use]
    pub fn payload_size(&self) -> usize {
        self.uploads.iter().map(|c| c.length).sum()
    }

    /// Fraction of local chunks that are reused (0.0 – 1.0).
    #[must_use]
    pub fn reuse_ratio(&self) -> f64 {
        let total = self.upload_count() + self.reused_count();
        if total == 0 {
            return 1.0;
        }
        self.reused_count() as f64 / total as f64
    }

    /// Bandwidth savings compared to uploading the full local file.
    #[must_use]
    pub fn bandwidth_saved_bytes(&self, local_file_size: u64) -> u64 {
        local_file_size.saturating_sub(self.payload_size() as u64)
    }
}

/// Compares a remote manifest (what the network already has) against a local manifest
/// (the current file state) and returns a plan of chunks to upload.
#[must_use]
pub fn compute_diff(remote: &FileManifest, local: &FileManifest) -> SyncPlan {
    let remote_hashes: HashSet<&str> = remote.chunks.iter().map(|c| c.hash.as_str()).collect();

    let mut uploads = Vec::new();
    let mut reused = Vec::new();

    for chunk in &local.chunks {
        if remote_hashes.contains(chunk.hash.as_str()) {
            reused.push(chunk.clone());
        } else {
            uploads.push(ChunkUploadRef {
                hash: chunk.hash.clone(),
                offset: chunk.offset,
                length: chunk.length,
            });
        }
    }

    SyncPlan { uploads, reused }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ChunkMeta;

    fn chunk(hash: &str, offset: u64, length: usize) -> ChunkMeta {
        ChunkMeta {
            hash: hash.into(),
            offset,
            length,
        }
    }

    fn manifest_with(chunks: Vec<ChunkMeta>) -> FileManifest {
        let mut m = FileManifest::new("test.bin");
        for c in chunks {
            m.add_chunk(c);
        }
        m
    }

    #[test]
    fn identical_manifests_upload_nothing() {
        let m = manifest_with(vec![chunk("aaa", 0, 100), chunk("bbb", 100, 200)]);
        let plan = compute_diff(&m, &m);
        assert_eq!(plan.upload_count(), 0);
        assert_eq!(plan.reused_count(), 2);
        assert_eq!(plan.payload_size(), 0);
    }

    #[test]
    fn disjoint_manifests_upload_everything() {
        let remote = manifest_with(vec![chunk("aaa", 0, 100)]);
        let local = manifest_with(vec![chunk("bbb", 0, 100)]);
        let plan = compute_diff(&remote, &local);
        assert_eq!(plan.upload_count(), 1);
        assert_eq!(plan.reused_count(), 0);
    }

    #[test]
    fn partial_overlap() {
        let remote = manifest_with(vec![chunk("aaa", 0, 100), chunk("bbb", 100, 200)]);
        let local = manifest_with(vec![
            chunk("aaa", 0, 100),
            chunk("ccc", 100, 150),
            chunk("ddd", 250, 100),
        ]);
        let plan = compute_diff(&remote, &local);
        assert_eq!(plan.upload_count(), 2);
        assert_eq!(plan.reused_count(), 1);
        assert_eq!(plan.payload_size(), 250);
    }
}
