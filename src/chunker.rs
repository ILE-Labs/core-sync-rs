//! Content-defined chunking via FastCDC.

use crate::error::{CoreSyncError, Result};
use crate::manifest::{ChunkMeta, FileManifest};
use fastcdc::v2020::FastCDC;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::Read;
use std::path::Path;

/// Minimum chunk size for FastCDC (2 KiB).
pub const CHUNK_MIN: u32 = 2_048;
/// Target average chunk size for FastCDC (8 KiB).
pub const CHUNK_AVG: u32 = 8_192;
/// Maximum chunk size for FastCDC (32 KiB).
pub const CHUNK_MAX: u32 = 32_768;

/// Reads a file from disk and produces a chunk manifest.
pub fn process_file<P: AsRef<Path>>(path: P) -> Result<FileManifest> {
    let path_ref = path.as_ref();
    let path_str = path_ref.to_string_lossy().to_string();

    let mut file = File::open(path_ref).map_err(|source| CoreSyncError::Io {
        path: path_str.clone(),
        source,
    })?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|source| CoreSyncError::Io {
            path: path_str,
            source,
        })?;

    chunk_bytes(&buffer, path_ref.to_string_lossy().into_owned())
}

/// Chunks an in-memory buffer and produces a manifest.
///
/// Useful for tests and for callers that already have file bytes loaded.
pub fn chunk_bytes(data: &[u8], file_path: impl Into<String>) -> Result<FileManifest> {
    let mut manifest = FileManifest::new(file_path);

    if data.is_empty() {
        return Ok(manifest);
    }

    let chunker = FastCDC::new(data, CHUNK_MIN, CHUNK_AVG, CHUNK_MAX);

    for chunk in chunker {
        let chunk_data = &data[chunk.offset..chunk.offset + chunk.length];
        let hash_hex = hash_chunk(chunk_data);

        manifest.add_chunk(ChunkMeta {
            hash: hash_hex,
            offset: chunk_offset_as_u64(chunk.offset)?,
            length: chunk.length,
        });
    }

    Ok(manifest)
}

fn chunk_offset_as_u64(offset: usize) -> Result<u64> {
    u64::try_from(offset).map_err(|_| {
        CoreSyncError::InvalidManifest(format!(
            "chunk offset {offset} does not fit in u64 on this target"
        ))
    })
}

/// Generates pseudo-random test data with enough entropy for FastCDC boundaries.
///
/// Prefer this over zero-filled buffers in tests and demos — uniform data
/// collapses into very few chunks, which does not exercise CDC reuse.
#[must_use]
pub fn varied_data(size: usize) -> Vec<u8> {
    (0..size)
        .map(|i| ((i.wrapping_mul(2_654_435_761)) >> 16) as u8)
        .collect()
}

/// Computes the SHA-256 hex digest of a chunk payload.
#[must_use]
pub fn hash_chunk(data: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(data);
    hex::encode(hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_produces_empty_manifest() {
        let manifest = chunk_bytes(&[], "empty.bin").unwrap();
        assert!(manifest.chunks.is_empty());
        assert_eq!(manifest.file_size, 0);
    }

    #[test]
    fn identical_content_produces_identical_hashes() {
        let data = varied_data(50_000);
        let m1 = chunk_bytes(&data, "a.bin").unwrap();
        let m2 = chunk_bytes(&data, "b.bin").unwrap();
        let hashes1: Vec<_> = m1.chunks.iter().map(|c| c.hash.as_str()).collect();
        let hashes2: Vec<_> = m2.chunks.iter().map(|c| c.hash.as_str()).collect();
        assert_eq!(hashes1, hashes2);
    }

    #[test]
    fn append_preserves_prefix_chunks() {
        let v1 = varied_data(256 * 1024);
        let mut v2 = v1.clone();
        v2.extend(varied_data(32 * 1024));

        let m1 = chunk_bytes(&v1, "v1.bin").unwrap();
        let m2 = chunk_bytes(&v2, "v2.bin").unwrap();

        assert!(
            m1.chunks.len() > 4,
            "test data should produce multiple chunks"
        );

        let reused = m1
            .chunks
            .iter()
            .filter(|c| m2.chunks.iter().any(|c2| c2.hash == c.hash))
            .count();

        assert!(
            reused >= m1.chunks.len() / 2,
            "append should reuse at least half of v1 chunks (reused={reused}/{})",
            m1.chunks.len()
        );
    }

    #[test]
    fn middle_insert_preserves_boundary_chunks() {
        let original_data = varied_data(256 * 1024);
        let mut data = original_data.clone();
        let insert_at = 128 * 1024;
        data.splice(insert_at..insert_at, varied_data(4 * 1024));

        let original = chunk_bytes(&original_data, "original.bin").unwrap();
        let modified = chunk_bytes(&data, "modified.bin").unwrap();

        let shared = modified
            .chunks
            .iter()
            .filter(|c| original.chunks.iter().any(|o| o.hash == c.hash))
            .count();

        assert!(
            shared >= 1,
            "middle insert should reuse unaffected chunks (shared={shared})"
        );
        assert!(
            shared < modified.chunks.len(),
            "edit should require some new chunks"
        );
    }
}
