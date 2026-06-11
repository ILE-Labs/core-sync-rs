//! Local differential sync — CDC chunking, manifest diffing, delta assembly.
//!
//! Chunks files with FastCDC, diffs manifests, assembles upload deltas locally.
//! Storage and indexer backends sit behind traits; in-memory mocks ship with the
//! crate. See `docs/INTEGRATION.md` for production wiring.
//!
//! ## Example
//!
//! ```no_run
//! use core_sync_rs::{
//!     indexd::InMemoryManifestStore,
//!     pipeline::{sync_file, SyncOptions},
//!     sia::InMemoryStorageBackend,
//! };
//! use std::path::Path;
//!
//! let manifests = InMemoryManifestStore::new();
//! let storage = InMemoryStorageBackend::new();
//!
//! let report = sync_file(
//!     Path::new("dataset.bin"),
//!     &SyncOptions { object_key: "backups/dataset.bin".into() },
//!     &manifests,
//!     &storage,
//! )?;
//!
//! println!("uploaded {} bytes, reuse {:.1}%",
//!     report.upload.bytes_uploaded,
//!     report.reuse_ratio() * 100.0);
//! # Ok::<(), core_sync_rs::CoreSyncError>(())
//! ```

#![deny(missing_docs)]

pub mod chunker;
pub mod error;
pub mod indexd;
pub mod manifest;
pub mod payload;
pub mod pipeline;
pub mod sia;
pub mod sync_engine;

#[cfg(feature = "sia-live")]
pub mod indexd_live;

#[cfg(feature = "sia-live")]
pub mod sia_live;

pub use error::{CoreSyncError, Result};
