//! Full sync pipeline using in-memory indexd and Sia mocks.
//!
//! ```bash
//! cargo run --example sync_pipeline
//! ```

use core_sync_rs::{
    chunker,
    indexd::InMemoryManifestStore,
    pipeline::{sync_file, SyncOptions},
    sia::InMemoryStorageBackend,
};
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

fn main() {
    let dir = TempDir::new().expect("temp dir");
    let path = dir.path().join("document.bin");

    let mut file = File::create(&path).expect("create file");
    file.write_all(&chunker::varied_data(32_768))
        .expect("write");

    let manifests = InMemoryManifestStore::new();
    let storage = InMemoryStorageBackend::new();

    // First upload
    let initial = sync_file(
        &path,
        &SyncOptions {
            object_key: "docs/document.bin".into(),
        },
        &manifests,
        &storage,
    )
    .expect("initial sync");

    println!("Initial upload:");
    println!("  chunks: {}", initial.local_manifest.chunk_count());
    println!("  uploaded: {} bytes", initial.upload.bytes_uploaded);

    // Simulate edit
    let mut file = File::create(&path).expect("rewrite file");
    let mut data = chunker::varied_data(32_768);
    data.extend(chunker::varied_data(4_096));
    file.write_all(&data).expect("write edit");

    let updated = sync_file(
        &path,
        &SyncOptions {
            object_key: "docs/document.bin".into(),
        },
        &manifests,
        &storage,
    )
    .expect("delta sync");

    println!("\nAfter edit:");
    println!("  reuse ratio: {:.1}%", updated.reuse_ratio() * 100.0);
    println!("  delta uploaded: {} bytes", updated.upload.bytes_uploaded);
    println!(
        "  bandwidth saved: {} bytes",
        updated.bandwidth_saved_bytes()
    );
}
