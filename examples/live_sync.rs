//! Live Sia integration demo.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example live_sync --features sia-live
//! ```
//!
//! The demo expects live endpoints and credentials in the environment.

#[cfg(feature = "sia-live")]
use core_sync_rs::{
    chunker,
    indexd_live::IndexdManifestStore,
    pipeline::{sync_file, SyncOptions},
    sia_live::SiaStorageBackend,
    Result,
};
#[cfg(feature = "sia-live")]
use std::fs::File;
#[cfg(feature = "sia-live")]
use std::io::Write;
#[cfg(feature = "sia-live")]
use std::path::{Path, PathBuf};

fn main() {
    #[cfg(feature = "sia-live")]
    {
        if let Err(err) = run() {
            eprintln!("error: {err}");
            std::process::exit(1);
        }
        return;
    }

    #[cfg(not(feature = "sia-live"))]
    {
        eprintln!("build this example with `--features sia-live` to enable live Sia wiring");
    }
}

#[cfg(feature = "sia-live")]
fn run() -> Result<()> {
    println!("core-sync-rs live Sia demo");
    println!("==========================\n");

    let manifests = IndexdManifestStore::from_env()?;
    let storage = SiaStorageBackend::from_env()?;

    let dir = std::env::temp_dir().join("core-sync-rs-live-demo");
    std::fs::create_dir_all(&dir).map_err(|e| core_sync_rs::CoreSyncError::Io {
        path: dir.to_string_lossy().into_owned(),
        source: e,
    })?;

    let result = run_scenarios(&dir, &manifests, &storage);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

#[cfg(feature = "sia-live")]
fn run_scenarios(
    dir: &Path,
    manifests: &IndexdManifestStore,
    storage: &SiaStorageBackend,
) -> Result<()> {
    let object_key = std::env::var("CORE_SYNC_OBJECT_KEY")
        .unwrap_or_else(|_| "live-demo/dataset.bin".to_string());

    let v1_path = build_v1_only(dir)?;
    run_pipeline_on_path(
        "Initial upload",
        &v1_path,
        &object_key,
        manifests,
        storage,
        "First sync against live storage and indexd",
    )?;

    println!();

    let v2_path = build_append_edit(dir)?;
    run_pipeline_on_path(
        "Append edit",
        &v2_path,
        &object_key,
        manifests,
        storage,
        "Second sync reuses the existing manifest and uploads only the delta",
    )?;

    println!("\nLive demo complete.");
    Ok(())
}

#[cfg(feature = "sia-live")]
fn run_pipeline_on_path(
    name: &str,
    local_path: &Path,
    object_key: &str,
    store: &IndexdManifestStore,
    storage: &SiaStorageBackend,
    description: &str,
) -> Result<()> {
    println!("Scenario: {name}");
    println!("  {description}");
    println!("  object key: {object_key}");
    println!("  {}", "-".repeat(60));

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
        println!("  first upload — no remote manifest was present");
    }
    println!(
        "  reused {} chunks, uploading {} ({:.1}% reuse)",
        report.plan.reused_count(),
        report.plan.upload_count(),
        report.reuse_ratio() * 100.0
    );
    println!(
        "  uploaded {} bytes in {} chunks",
        report.upload.bytes_uploaded, report.upload.chunks_uploaded
    );
    if !report.initial_upload {
        println!(
            "  bandwidth saved vs full file: {} bytes ({saved_pct:.1}%)",
            report.bandwidth_saved_bytes()
        );
    }

    Ok(())
}

#[cfg(feature = "sia-live")]
fn build_v1_only(dir: &Path) -> Result<PathBuf> {
    let path = dir.join("live_v1.bin");
    write_file(&path, &chunker::varied_data(64_000))?;
    Ok(path)
}

#[cfg(feature = "sia-live")]
fn build_append_edit(dir: &Path) -> Result<PathBuf> {
    let path = dir.join("live_v2.bin");
    let mut data = chunker::varied_data(64_000);
    data.extend(chunker::varied_data(12_000));
    write_file(&path, &data)?;
    Ok(path)
}

#[cfg(feature = "sia-live")]
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
