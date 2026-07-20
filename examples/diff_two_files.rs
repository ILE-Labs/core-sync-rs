//! Compare two files on disk and print a sync plan.
//!
//! ```bash
//! cargo run --example diff_two_files -- remote.bin local.bin
//! ```

use core_sync_rs::{chunker, payload, sync_engine};
use std::env;
use std::process;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: {} <remote_file> <local_file>", args[0]);
        process::exit(1);
    }

    let remote_path = &args[1];
    let local_path = &args[2];

    let remote = match chunker::process_file(remote_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error chunking remote file: {e}");
            process::exit(1);
        }
    };

    let local = match chunker::process_file(local_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error chunking local file: {e}");
            process::exit(1);
        }
    };

    if let Err(e) = local.validate() {
        eprintln!("invalid local manifest: {e}");
        process::exit(1);
    }

    let plan = sync_engine::compute_diff(&remote, &local);

    match payload::assemble_delta(local_path, &plan) {
        Ok(delta) => {
            println!("Sync plan for `{}` → `{}`", remote_path, local_path);
            println!("  local size:       {} bytes", local.file_size);
            println!("  chunks (remote):  {}", remote.chunk_count());
            println!("  chunks (local):   {}", local.chunk_count());
            println!("  reused:           {}", plan.reused_count());
            println!(
                "  to upload:        {} chunks ({} bytes)",
                plan.upload_count(),
                plan.payload_size()
            );
            println!("  reuse ratio:      {:.1}%", plan.reuse_ratio() * 100.0);
            println!(
                "  delta verified:   {} bytes in {} chunks",
                delta.total_bytes(),
                delta.len()
            );
        }
        Err(e) => {
            eprintln!("error assembling delta: {e}");
            process::exit(1);
        }
    }
}
