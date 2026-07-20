# core-sync-rs

Local differential sync for the [Sia](https://sia.tech) network.

## What it does

Most sync tools re-upload a whole file whenever it changes. CoreSync chunks the
file with content-defined chunking (FastCDC), hashes each piece, diffs against
what the remote already has, and uploads only the bytes that actually changed.

```text
Standard sync:  60 KB file, small edit   →  upload 60 KB
CoreSync:       60 KB file, append 10 KB →  upload ~11 KB (rest reused)
```

The sync logic lives entirely in this repo. Transport and storage are handled by
the backend:

```text
core-sync-rs  →  chunk, diff, pack delta locally
sia_storage   →  upload, encrypt, erasure-code
indexd        →  store chunk manifests as Sia objects
```

## Feature flags

| Flag | Purpose |
|------|---------|
| `sia-sdk` | Official `sia_storage` crate + indexd SDK path (recommended) |
| `sia-live` | HTTP shim adapters for custom PUT/HEAD/GET endpoints |

## Getting started

### Prerequisites

- [Rust stable](https://rustup.rs/)
- For the live integration: a running [indexd](https://sia.tech) instance

### Build and run

```bash
git clone https://github.com/ILE-Labs/core-sync-rs
cd core-sync-rs

# Offline demo (no Sia required)
cargo run

# Run the test suite
cargo test

# Diff two local files
cargo run --example diff_two_files -- <old-file> <new-file>
```

## Live Sia integration (SDK path)

### First time: register your app key

```bash
# Use 127.0.0.1, not localhost — indexd verifies signatures against the IP it
# binds to; "localhost" produces a mismatched hash and an "invalid signature" error.
export SIA_INDEXER_URL=http://127.0.0.1:9982
cargo run --example register_app_key --features sia-sdk
```

This opens an approval URL in the console. Open it in your browser, approve the
connection, and the tool prints your `SIA_APP_KEY`. Save it to `.env`:

```bash
cp .env.example .env
# paste the printed SIA_APP_KEY into .env
```

### Run the live demo

```bash
cargo run --example sia_live_demo --features sia-sdk -- ./your-file.txt
```

The demo uploads the file, then syncs a modified version and prints the
bandwidth saved on each run. It checks required environment variables up front
and exits with a clear message if they are missing.

### HTTP compatibility path

If you are targeting a custom shim instead of the official SDK:

```bash
cargo run --example sia_live_demo --features sia-live -- ./your-file.txt
```

The shim expects `PUT/HEAD /chunks/{hash}` and `GET/PUT /manifests/{key}`.

## Scope

### Implemented and tested

- FastCDC chunking and SHA-256 manifests
- Manifest diff and delta payload assembly
- Pipeline orchestration via `pipeline::sync_file`
- Trait boundaries for indexd and Sia upload, with in-memory mocks
- Feature-gated SDK adapter (`sia-sdk`) using the official `sia_storage` crate
- Feature-gated HTTP shim adapters (`sia-live`)
- Integration tests and CI

### Not yet

- CLI, watch mode, directory sync
- GUI
- Streaming reads for very large files
- crates.io release

## Project layout

```text
src/
├── chunker.rs          FastCDC + SHA-256 hashing
├── manifest.rs         ChunkMeta, FileManifest
├── sync_engine.rs      diff remote vs local manifest
├── payload.rs          assemble upload bytes
├── indexd.rs           manifest store trait + in-memory mock
├── sia.rs              storage backend trait + in-memory mock
├── indexd_real.rs      HTTP shim manifest adapter   (sia-live)
├── sia_real.rs         HTTP shim storage adapter    (sia-live)
├── sia_sdk.rs          SDK-backed adapter            (sia-sdk)
├── pipeline.rs         orchestration
└── bin/core-sync-rs.rs offline demo binary

tests/sync_integration.rs
examples/
├── diff_two_files.rs
├── sync_pipeline.rs
├── sia_live_demo.rs     live end-to-end demo
└── register_app_key.rs  one-time SDK key registration helper
```

See [ARCHITECTURE.md](ARCHITECTURE.md) for module design rationale.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `fastcdc` | content-defined chunking |
| `sha2` / `hex` | chunk hashing |
| `serde` / `serde_json` | manifest serialization |
| `thiserror` | error types |
| `dotenvy` | load `.env` for live demo |
| `reqwest` *(feature-gated)* | HTTP shim adapters |
| `tokio` *(feature-gated)* | async runtime for live paths |
| `sia_storage` *(feature-gated)* | official Sia SDK |

## License

[MIT](LICENSE)
