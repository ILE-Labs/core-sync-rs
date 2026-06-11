# core-sync-rs

Local differential sync for the [Sia](https://sia.tech) network.

Early-stage library - the local chunking and diff engine works and is tested; live `sia_storage` and indexd wiring is available behind a feature flag. See [Scope](#scope).
The live wiring is now available behind the `sia-live` feature flag while the mock path remains the default.

## What it does

When a file changes, most sync tools re-upload the whole thing. CoreSync chunks the file locally with FastCDC, hashes each piece, diffs against what the remote already has, and uploads only the bytes that are actually new.

```
Standard sync:     60 KB file, small edit  ->  upload 60 KB
CoreSync:          60 KB file, append 10 KB ->  upload ~11 KB (rest reused)
```

It doesn't replace Sia's networking or storage. The split looks like this:

```
core-sync-rs  ->  chunk, diff, pack delta locally
sia_storage   ->  upload, encrypt, erasure-code
indexd        ->  store chunk manifests on objects
```

Right now the Sia SDK and indexd sides are mocked in memory so you can run everything without credentials.
When you want to talk to live services, enable `sia-live` and provide the live URLs and credentials described below.

## Live Sia Integration

The live path is feature-gated so the mock path remains the default. It is an HTTP integration layer over the live endpoints, not a full upstream SDK binding.

1. Create a local `.env` file in the project root.
2. Set the live storage and indexd endpoints and credentials in that file.
3. Run the demo with the live feature flag:

```bash
cargo run --example sia_live_demo --features sia-live
```

Required environment variables in `.env`:

- `SIA_API_ENDPOINT`
- `SIA_API_PASSWORD`
- `INDEXD_ENDPOINT`
- `INDEXD_API_KEY`

## Getting Started

### Prerequisites

You need Rust stable installed on your system.
- **Install Rust**: Download and install via [rustup](https://rustup.rs/).
- **Platform Support**: This project compiles natively on Windows (MSVC/GNU), macOS, and Linux. No WSL or external containers are required.

### Build and Run

1. **Clone the Repository**
   ```bash
   git clone https://github.com/ILE-Labs/core-sync-rs
   cd core-sync-rs
   ```

2. **Run the Demo**
   The default demo walks through three edit scenarios (an initial file upload, an append edit, and a middle insert) using the in-memory mock store and storage backend.
   ```bash
   cargo run
   ```
   You will see output detailing chunk count, reused chunks, uploaded delta sizes, and bandwidth savings.

3. **Run the Test Suite**
   Run all unit and integration tests (validating CDC boundary conditions, offset safe-casting, concurrency safety, lock poisoning, and schema versioning):
   ```bash
   cargo test
   ```

4. **Diff Two Local Files**
   To compute and verify a sync plan between two arbitrary files on disk:
   ```bash
   cargo run --example diff_two_files -- <path-to-old-file> <path-to-new-file>
   ```

## Scope

**Working now**

- FastCDC chunking, SHA-256 manifests
- Manifest diff and delta payload assembly
- Pipeline that ties the steps together (`pipeline::sync_file`)
- Trait boundaries for indexd (`ManifestStore`) and Sia upload (`StorageBackend`) with in-memory impls plus live adapters behind `sia-live`
- Tests and CI
- Live Sia adapters behind the `sia-live` feature flag

**Not yet**

- CLI, watch mode, directory sync
- Streaming reads for large files
- crates.io release

## Layout

```
src/
|-- chunker.rs          FastCDC + hashing
|-- manifest.rs         ChunkMeta, FileManifest
|-- sync_engine.rs      diff remote vs local
|-- payload.rs          assemble upload bytes
|-- indexd.rs           manifest store trait + mock
|-- sia.rs              storage backend trait + mock
|-- indexd_real.rs      live indexd adapter (feature-gated)
|-- sia_real.rs         live Sia storage adapter (feature-gated)
|-- pipeline.rs         orchestration
`-- bin/core-sync-rs.rs demo

tests/sync_integration.rs
examples/
|-- diff_two_files.rs
|-- sync_pipeline.rs
`-- sia_live_demo.rs
```

More detail in [ARCHITECTURE.md](ARCHITECTURE.md) and [docs/INTEGRATION.md](docs/INTEGRATION.md).

## Dependencies

| Crate | Use |
|-------|-----|
| fastcdc | content-defined chunking |
| sha2 / hex | chunk hashes |
| serde / serde_json | manifest serialization |
| thiserror | errors |
| reqwest (feature-gated) | live HTTP adapters |

## License

MIT
