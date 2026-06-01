//! Demo binary — runs append and middle-insert scenarios with mocked indexd + storage.

use core_sync_rs::{
    chunker,
    indexd::InMemoryManifestStore,
    pipeline::{sync_file, SyncOptions},
    sia::InMemoryStorageBackend,
    Result,
};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

fn main() {
    if let Err(err) = run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    println!("core-sync-rs demo");
    println!("=================\n");

    let dir = std::env::temp_dir().join("core-sync-rs-demo");
    std::fs::create_dir_all(&dir).map_err(|e| core_sync_rs::CoreSyncError::Io {
        path: dir.to_string_lossy().into_owned(),
        source: e,
    })?;

    let result = run_scenarios(&dir);

    // Clean up temp directory
    let _ = std::fs::remove_dir_all(&dir);

    result
}

fn run_scenarios(dir: &Path) -> Result<()> {
    let store = InMemoryManifestStore::new();
    let storage = InMemoryStorageBackend::new();

    let v1_path = build_v1_only(dir)?;
    run_pipeline_on_path(
        "Initial upload",
        &v1_path,
        "data/dataset.bin",
        &store,
        &storage,
        "First sync — full file chunked and registered",
    )?;

    println!();

    let v2_append = build_append_edit(dir)?;
    run_pipeline_on_path(
        "Append edit",
        &v2_append,
        "data/dataset.bin",
        &store,
        &storage,
        "Append 10 KiB — only delta uploaded",
    )?;

    println!();

    let v1_insert = build_v1_insert(dir)?;
    sync_file(
        &v1_insert,
        &SyncOptions {
            object_key: "data/insert-demo.bin".into(),
        },
        &store,
        &storage,
    )?;

    let v2_insert = build_middle_insert(dir)?;
    run_pipeline_on_path(
        "Middle insert",
        &v2_insert,
        "data/insert-demo.bin",
        &store,
        &storage,
        "Insert 1 KiB at midpoint — prefix/suffix chunks reused",
    )?;

    println!("\nDemo complete.");
    Ok(())
}

fn run_pipeline_on_path(
    name: &str,
    local_path: &Path,
    object_key: &str,
    store: &InMemoryManifestStore,
    storage: &InMemoryStorageBackend,
    description: &str,
) -> Result<()> {
    println!("Scenario: {name}");
    println!("  {description}");
    println!("  object key: {object_key}");
    println!("  {}", "-".repeat(55));

    let report = sync_file(
        local_path,
        &SyncOptions {
            object_key: object_key.into(),
        },
        store,
        storage,
    )?;

    let saved_pct = if report.local_manifest.file_size > 0 {
        (report.bandwidth_saved_bytes() as f64 / report.local_manifest.file_size as f64) * 100.0
    } else {
        0.0
    };

    println!(
        "  {} bytes, {} chunks",
        report.local_manifest.file_size,
        report.local_manifest.chunk_count()
    );
    if report.initial_upload {
        println!("  first upload — no remote manifest");
    }
    println!(
        "  reused {} chunks, uploading {} ({:.1}% reuse)",
        report.plan.reused_count(),
        report.plan.upload_count(),
        report.reuse_ratio() * 100.0
    );
    println!(
        "  delta: {} bytes in {} chunks (mock upload)",
        report.upload.bytes_uploaded, report.upload.chunks_uploaded
    );
    if !report.initial_upload {
        println!(
            "  saved {} bytes vs full file ({saved_pct:.1}%)",
            report.bandwidth_saved_bytes()
        );
    }

    Ok(())
}

fn build_v1_only(dir: &Path) -> Result<PathBuf> {
    let path = dir.join("append_v1.bin");
    write_file(&path, &chunker::varied_data(50_000))?;
    Ok(path)
}

fn build_append_edit(dir: &Path) -> Result<PathBuf> {
    let path = dir.join("append_v2.bin");
    let mut data = chunker::varied_data(50_000);
    data.extend(chunker::varied_data(10_000));
    write_file(&path, &data)?;
    Ok(path)
}

fn build_v1_insert(dir: &Path) -> Result<PathBuf> {
    let path = dir.join("insert_v1.bin");
    write_file(&path, &chunker::varied_data(50_000))?;
    Ok(path)
}

fn build_middle_insert(dir: &Path) -> Result<PathBuf> {
    let path = dir.join("insert_v2.bin");
    let mut data = chunker::varied_data(50_000);
    data.splice(25_000..25_000, chunker::varied_data(1_024));
    write_file(&path, &data)?;
    Ok(path)
}

fn write_file(path: &Path, data: &[u8]) -> Result<()> {
    let mut file = File::create(path).map_err(|source| core_sync_rs::CoreSyncError::Io {
        path: path.display().to_string(),
        source,
    })?;
    file.write_all(data)
        .map_err(|source| core_sync_rs::CoreSyncError::Io {
            path: path.display().to_string(),
            source,
        })?;
    Ok(())
}
