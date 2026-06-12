//! Live Sia integration demo.
//!
//! Run with:
//!
//! ```bash
//! cargo run --example sia_live_demo --features sia-live -- <path-to-file>
//! ```
//!
//! If no path is provided, the example creates a small temporary file and
//! appends to it so the second sync can prove differential reuse.

#[cfg(feature = "sia-live")]
use core_sync_rs::{
    indexd_real::IndexdManifestStore,
    pipeline::{sync_file, SyncOptions},
    sia_real::SiaStorageBackend,
    Result,
};
#[cfg(feature = "sia-live")]
use sha2::{Digest, Sha256};
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
    // Load `.env` from the project root when present (ignored if missing).
    let _ = dotenvy::dotenv();

    println!("core-sync-rs live Sia demo");
    println!("==========================\n");

    let manifests = IndexdManifestStore::from_env()?;
    let storage = SiaStorageBackend::from_env()?;
    let args: Vec<String> = std::env::args().collect();
    let (dir, path, owns_file) = if let Some(input) = args.get(1) {
        let path = PathBuf::from(input);
        let dir = path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| std::env::temp_dir());
        (dir, path, false)
    } else {
        let dir = std::env::temp_dir().join("core-sync-rs-live-demo");
        std::fs::create_dir_all(&dir).map_err(|e| core_sync_rs::CoreSyncError::Io {
            path: dir.to_string_lossy().into_owned(),
            source: e,
        })?;
        let path = dir.join("sample.bin");
        write_file(&path, b"core-sync-rs live demo\nfirst revision\n")?;
        (dir, path, true)
    };

    let result = run_scenarios(&path, &manifests, &storage);

    if owns_file {
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_dir_all(&dir);
    }

    result
}

#[cfg(feature = "sia-live")]
fn run_scenarios(
    path: &Path,
    manifests: &IndexdManifestStore,
    storage: &SiaStorageBackend,
) -> Result<()> {
    if !path.exists() {
        write_file(path, b"core-sync-rs live demo\nfirst revision\n")?;
    }

    let object_key = object_key_for(path);
    let initial = run_pipeline_on_path("Initial upload", path, &object_key, manifests, storage)?;

    println!();

    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(path)
        .map_err(|source| core_sync_rs::CoreSyncError::Io {
            path: path.display().to_string(),
            source,
        })?;
    file.write_all(b"appended live delta\n")
        .map_err(|source| core_sync_rs::CoreSyncError::Io {
            path: path.display().to_string(),
            source,
        })?;

    let second = run_pipeline_on_path("After append", path, &object_key, manifests, storage)?;

    println!("\nLive demo complete.");
    println!(
        "First sync uploaded {} bytes across {} chunks.",
        initial.upload.bytes_uploaded, initial.upload.chunks_uploaded
    );
    println!(
        "Second sync uploaded {} bytes across {} chunks.",
        second.upload.bytes_uploaded, second.upload.chunks_uploaded
    );

    Ok(())
}

#[cfg(feature = "sia-live")]
fn run_pipeline_on_path(
    name: &str,
    local_path: &Path,
    object_key: &str,
    store: &IndexdManifestStore,
    storage: &SiaStorageBackend,
) -> Result<core_sync_rs::pipeline::SyncReport> {
    println!("Scenario: {name}");
    println!("  file: {}", local_path.display());
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
        println!("  first upload - no remote manifest was present");
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

    Ok(report)
}

#[cfg(feature = "sia-live")]
fn object_key_for(path: &Path) -> String {
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("dataset.bin");
    let canonical = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut hasher = Sha256::new();
    hasher.update(canonical.to_string_lossy().as_bytes());
    let short_hash = hex::encode(&hasher.finalize()[..8]);
    format!("live-demo/{file_name}-{short_hash}")
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
