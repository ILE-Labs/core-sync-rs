# Local differential sync for the [Sia](https://sia.tech) network.

Early-stage library — the local chunking and diff engine is complete and tested. Live wiring is available behind the `sia-live` feature flag (HTTP adapters today; native `sia_storage` / indexd SDK binding is planned). See [Scope](#scope) and [Integration maturity](docs/INTEGRATION.md#integration-maturity).

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

To run against real Sia Storage and indexd:

1. Copy `.env.example` to `.env` and fill in your credentials.
2. Run the live demo:

```bash
cargo run --example sia_live_demo --features sia-live -- ./testfile.txt
```

3. The demo uploads a file, then syncs a modified version, printing bandwidth savings on each run.

Requirements: **shim-compatible HTTP endpoints** that implement CoreSync's custom routes (`PUT/HEAD /chunks/{hash}` and `GET/PUT /manifests/{key}`). Vanilla renterd + official indexd do **not** expose those paths — pointing them at the URLs in `.env.example` will fail unless you deploy a compatible proxy or adapter.

The live path is feature-gated (`sia-live`) so mocks remain the default for CI. Verified integration with official Sia tooling uses the `sia-sdk` feature (`sia_storage::Sdk`); see [Integration maturity](docs/INTEGRATION.md#integration-maturity).

**Note for adopters:** the live adapters are HTTP shims that prove differential sync against persistent remote state. They are not yet a verified `sia_storage` / indexd SDK binding. The sync engine is complete and tested; SDK-native upload and manifest persistence are the next integration milestone. See [Integration maturity](docs/INTEGRATION.md#integration-maturity).

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

- SDK adapters are scaffolded under `sia-sdk`; live verification against a running indexd is still open (HTTP shims ship under `sia-live`)
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

More detail in [ARCHITECTURE.md](ARCHITECTURE.md) and [docs/INTEGRATION.md](docs/INTEGRATION.md).

## Dependencies

| Crate | Use |
|-------|-----|
| fastcdc | content-defined chunking |
| sha2 / hex | chunk hashes |
| serde / serde_json | manifest serialization |
| thiserror | errors |
| dotenvy | load `.env` for live demo |
| reqwest (feature-gated) | live HTTP adapters |

## License

[MIT](LICENSE)
