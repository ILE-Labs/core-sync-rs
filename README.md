# core-sync-rs

Local differential sync for the [Sia](https://sia.tech) network.

The local chunking and diff engine is complete and tested. Live wiring is available behind two feature flags:
- `sia-live` for HTTP compatibility adapters
- `sia-sdk` for the official `sia_storage` / indexd SDK wiring

## What it does

When a file changes, most sync tools re-upload the whole thing. CoreSync chunks the file locally with FastCDC, hashes each piece, diffs against what the remote already has, and uploads only the bytes that are actually new.

```text
Standard sync:     60 KB file, small edit  ->  upload 60 KB
CoreSync:          60 KB file, append 10 KB ->  upload ~11 KB (rest reused)
```

The project keeps the sync logic local and leaves transport to the backend:

```text
core-sync-rs  ->  chunk, diff, pack delta locally
sia_storage   ->  upload, encrypt, erasure-code
indexd        ->  store chunk manifests on objects
```

## Live Sia Integration

To run against real Sia Storage and indexd with the official SDK path:

1. Copy `.env.example` to `.env`.
2. Fill in `SIA_INDEXER_URL` and `SIA_APP_KEY`.
3. Run the live demo:

```bash
cargo run --example sia_live_demo --features sia-sdk -- ./testfile.txt
```

The demo uploads a file, then syncs a modified version, printing bandwidth savings on each run.
It checks the required environment variables up front and fails fast with a clear message if they are missing.

If you need the HTTP compatibility path instead, use:

```bash
cargo run --example sia_live_demo --features sia-live -- ./testfile.txt
```

That path expects shim-compatible endpoints for `PUT/HEAD /chunks/{hash}` and `GET/PUT /manifests/{key}`.

## Getting Started

### Prerequisites

You need Rust stable installed on your system.
- Install Rust: [rustup](https://rustup.rs/)
- Platform support: this project compiles natively on Windows, macOS, and Linux

### Build and Run

1. Clone the repository:

```bash
git clone https://github.com/ILE-Labs/core-sync-rs
cd core-sync-rs
```

2. Run the default demo:

```bash
cargo run
```

3. Run the test suite:

```bash
cargo test
```

4. Diff two local files:

```bash
cargo run --example diff_two_files -- <path-to-old-file> <path-to-new-file>
```

## Scope

**Working now**

- FastCDC chunking and SHA-256 manifests
- Manifest diff and delta payload assembly
- Pipeline orchestration via `pipeline::sync_file`
- Trait boundaries for indexd and Sia upload, with in-memory mocks and feature-gated live adapters
- SDK-backed live wiring under `sia-sdk`
- HTTP compatibility adapters under `sia-live`
- Tests and CI

**Not yet**

- CLI, watch mode, directory sync
- GUI
- Streaming reads for large files
- crates.io release

## Layout

```text
src/
|-- chunker.rs          FastCDC + hashing
|-- manifest.rs         ChunkMeta, FileManifest
|-- sync_engine.rs      diff remote vs local
|-- payload.rs          assemble upload bytes
|-- indexd.rs           manifest store trait + mock
|-- sia.rs              storage backend trait + mock
|-- indexd_real.rs      HTTP shim manifest adapter (`sia-live`)
|-- sia_real.rs         HTTP shim storage adapter (`sia-live`)
|-- sia_sdk.rs          SDK-backed storage + manifest adapter (`sia-sdk`)
|-- pipeline.rs         orchestration
`-- bin/core-sync-rs.rs demo

tests/sync_integration.rs
examples/
|-- diff_two_files.rs
|-- sync_pipeline.rs
`-- sia_live_demo.rs
```

More detail in [docs/INTEGRATION.md](docs/INTEGRATION.md).

## Dependencies

| Crate | Use |
|-------|-----|
| fastcdc | content-defined chunking |
| sha2 / hex | chunk hashes |
| serde / serde_json | manifest serialization |
| thiserror | errors |
| dotenvy | load `.env` for live demo |
| reqwest (feature-gated) | HTTP compatibility adapters |
| sia_storage (feature-gated) | official Sia SDK path |

## License

[MIT](LICENSE)
