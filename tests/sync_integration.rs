//! End-to-end integration tests across the full sync pipeline.

use core_sync_rs::{
    chunker,
    indexd::{InMemoryManifestStore, ManifestStore},
    payload,
    pipeline::{sync_file, SyncOptions},
    sia::InMemoryStorageBackend,
    sync_engine,
};
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

fn write_temp_file(dir: &TempDir, name: &str, data: &[u8]) -> std::path::PathBuf {
    let path = dir.path().join(name);
    let mut file = File::create(&path).unwrap();
    file.write_all(data).unwrap();
    path
}

#[test]
fn append_edit_minimizes_upload() {
    let dir = TempDir::new().unwrap();
    let v1_data = chunker::varied_data(50_000);
    let mut v2_data = v1_data.clone();
    v2_data.extend(chunker::varied_data(10_000));

    let v1_path = write_temp_file(&dir, "v1.bin", &v1_data);
    let v2_path = write_temp_file(&dir, "v2.bin", &v2_data);

    let remote = chunker::process_file(&v1_path).unwrap();
    let local = chunker::process_file(&v2_path).unwrap();
    local.validate().unwrap();

    let plan = sync_engine::compute_diff(&remote, &local);
    let delta = payload::assemble_delta(&v2_path, &plan).unwrap();

    assert!(plan.reused_count() > 0, "append should reuse prefix chunks");
    assert!(plan.upload_count() < local.chunk_count());
    assert!(plan.payload_size() < local.file_size as usize);
    assert_eq!(delta.total_bytes(), plan.payload_size());
    assert!(plan.reuse_ratio() > 0.5);
}

#[test]
fn middle_insert_preserves_distant_chunks() {
    let dir = TempDir::new().unwrap();
    let v1_data = chunker::varied_data(256 * 1024);
    let mut v2_data = v1_data.clone();
    v2_data.splice(128 * 1024..128 * 1024, chunker::varied_data(4 * 1024));

    let v1_path = write_temp_file(&dir, "v1.bin", &v1_data);
    let v2_path = write_temp_file(&dir, "v2.bin", &v2_data);

    let remote = chunker::process_file(&v1_path).unwrap();
    let local = chunker::process_file(&v2_path).unwrap();

    let plan = sync_engine::compute_diff(&remote, &local);

    assert!(
        plan.reused_count() >= 1,
        "middle insert should reuse unaffected chunks"
    );
    assert!(
        plan.upload_count() > 0,
        "edit region should require new chunks"
    );
    assert!(plan.payload_size() < v2_data.len());
}

#[test]
fn identical_files_produce_empty_delta() {
    let dir = TempDir::new().unwrap();
    let data = vec![0x11; 32_000];
    let path = write_temp_file(&dir, "same.bin", &data);

    let manifest = chunker::process_file(&path).unwrap();
    let plan = sync_engine::compute_diff(&manifest, &manifest);
    let delta = payload::assemble_delta(&path, &plan).unwrap();

    assert_eq!(plan.upload_count(), 0);
    assert!(delta.is_empty());
}

#[test]
fn manifest_json_is_stable() {
    let data = vec![0u8; 16_384];
    let manifest = chunker::chunk_bytes(&data, "/data/test.bin").unwrap();
    manifest.validate().unwrap();

    let json = serde_json::to_string_pretty(&manifest).unwrap();
    let restored: core_sync_rs::manifest::FileManifest = serde_json::from_str(&json).unwrap();
    assert_eq!(manifest, restored);
    assert!(json.contains("\"file_path\""));
    assert!(json.contains("\"chunks\""));
}

#[test]
fn full_pipeline_initial_then_delta_sync() {
    let dir = TempDir::new().unwrap();
    let v1_path = write_temp_file(&dir, "v1.bin", &chunker::varied_data(40_000));

    let store = InMemoryManifestStore::new();
    let storage = InMemoryStorageBackend::new();

    let initial = sync_file(
        &v1_path,
        &SyncOptions {
            object_key: "sync/test.bin".into(),
        },
        &store,
        &storage,
    )
    .unwrap();

    assert!(initial.initial_upload);
    assert!(store.get_manifest("sync/test.bin").unwrap().is_some());

    let mut v2_data = chunker::varied_data(40_000);
    v2_data.extend(chunker::varied_data(8_000));
    let v2_path = write_temp_file(&dir, "v2.bin", &v2_data);

    let delta = sync_file(
        &v2_path,
        &SyncOptions {
            object_key: "sync/test.bin".into(),
        },
        &store,
        &storage,
    )
    .unwrap();

    assert!(!delta.initial_upload);
    assert!(delta.plan.reused_count() > 0);
    assert!(delta.upload.bytes_uploaded < v2_data.len());
}
