//! Error types for the CoreSync library.

use thiserror::Error;

/// Errors that can occur during chunking, manifest handling, or payload assembly.
#[derive(Debug, Error)]
pub enum CoreSyncError {
    /// An I/O operation failed while reading a file.
    #[error("failed to read file `{path}`: {source}")]
    Io {
        /// Path to the file that could not be read.
        path: String,
        /// Underlying OS I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The local file does not cover the byte ranges referenced by the sync plan.
    #[error("chunk at offset {offset} with length {length} extends past file size {file_size}")]
    ChunkOutOfBounds {
        /// Byte offset of the out-of-bounds chunk.
        offset: u64,
        /// Declared chunk length.
        length: usize,
        /// Actual file size on disk.
        file_size: u64,
    },

    /// Manifest validation failed.
    #[error("invalid manifest: {0}")]
    InvalidManifest(String),

    /// An indexd manifest store operation failed.
    #[error("indexd error: {0}")]
    Indexd(String),

    /// The indexd manifest store lock was poisoned after a panicking thread.
    #[error("indexd store lock poisoned")]
    IndexdLockPoisoned,

    /// A Sia storage upload operation failed.
    #[error("sia storage error: {0}")]
    Storage(String),

    /// The storage backend lock was poisoned after a panicking thread.
    #[error("storage backend lock poisoned")]
    StorageLockPoisoned,
}

/// CoreSync result type alias.
pub type Result<T> = std::result::Result<T, CoreSyncError>;
