//! Manifest types for describing chunked file state.

use crate::error::{CoreSyncError, Result};
use serde::{Deserialize, Serialize};

/// Metadata for a single content-defined chunk within a file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkMeta {
    /// SHA-256 hash of the chunk payload, hex-encoded.
    pub hash: String,
    /// Byte offset of this chunk within the source file.
    pub offset: u64,
    /// Length of the chunk in bytes.
    pub length: usize,
}

/// A complete manifest describing how a file is partitioned into chunks.
///
/// Manifests are designed to be serialized (JSON) and ingested by indexers
/// such as `indexd`, enabling remote-side deduplication lookups.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileManifest {
    /// Path or logical identifier for the source file.
    pub file_path: String,
    /// Total size of the source file in bytes.
    pub file_size: u64,
    /// Ordered list of chunks covering the file from start to end.
    pub chunks: Vec<ChunkMeta>,
}

impl FileManifest {
    /// Creates an empty manifest for the given file path.
    #[must_use]
    pub fn new(file_path: impl Into<String>) -> Self {
        Self {
            file_path: file_path.into(),
            file_size: 0,
            chunks: Vec::new(),
        }
    }

    /// Appends a chunk and updates the recorded file size.
    pub fn add_chunk(&mut self, chunk: ChunkMeta) {
        let end = chunk.offset + chunk.length as u64;
        if end > self.file_size {
            self.file_size = end;
        }
        self.chunks.push(chunk);
    }

    /// Returns the number of chunks in this manifest.
    #[must_use]
    pub fn chunk_count(&self) -> usize {
        self.chunks.len()
    }

    /// Returns the sum of all chunk payload lengths.
    #[must_use]
    pub fn covered_bytes(&self) -> u64 {
        self.chunks.iter().map(|c| c.length as u64).sum()
    }

    /// Validates that chunks are contiguous and cover the full file.
    pub fn validate(&self) -> Result<()> {
        if self.chunks.is_empty() {
            if self.file_size == 0 {
                return Ok(());
            }
            return Err(CoreSyncError::InvalidManifest(
                "empty manifest for non-empty file".into(),
            ));
        }

        let mut expected_offset = 0u64;
        for chunk in &self.chunks {
            if chunk.offset != expected_offset {
                return Err(CoreSyncError::InvalidManifest(format!(
                    "gap or overlap at offset {expected_offset}, found chunk at {}",
                    chunk.offset
                )));
            }
            expected_offset += chunk.length as u64;
        }

        if expected_offset != self.file_size {
            return Err(CoreSyncError::InvalidManifest(format!(
                "chunks cover {expected_offset} bytes but file_size is {}",
                self.file_size
            )));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_chunk(offset: u64, length: usize, hash: &str) -> ChunkMeta {
        ChunkMeta {
            hash: hash.into(),
            offset,
            length,
        }
    }

    #[test]
    fn validate_contiguous_chunks() {
        let mut manifest = FileManifest::new("test.bin");
        manifest.add_chunk(sample_chunk(0, 100, "aaa"));
        manifest.add_chunk(sample_chunk(100, 200, "bbb"));
        manifest.validate().unwrap();
        assert_eq!(manifest.file_size, 300);
    }

    #[test]
    fn validate_rejects_gap() {
        let mut manifest = FileManifest::new("test.bin");
        manifest.add_chunk(sample_chunk(0, 100, "aaa"));
        manifest.add_chunk(sample_chunk(200, 100, "bbb"));
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn serde_round_trip() {
        let mut manifest = FileManifest::new("/data/file.bin");
        manifest.add_chunk(sample_chunk(0, 8192, "deadbeef"));
        let json = serde_json::to_string(&manifest).unwrap();
        let restored: FileManifest = serde_json::from_str(&json).unwrap();
        assert_eq!(manifest, restored);
    }
}
