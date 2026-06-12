# Architecture

core-sync-rs is a local preparation layer for file sync on Sia. It figures out which bytes need uploading before anything hits the network.

## The problem

Sia's SDKs handle upload and storage well, but they'll upload whatever you give them. Change one byte in a large file and you're still sending the whole file unless something on the client side does the diff first.

indexd can hold metadata about what's already stored. CoreSync's job is to chunk locally, compare manifests, and hand the SDK a minimal delta.

## How it works

1. Chunk the local file with FastCDC
2. SHA-256 hash each chunk, build a manifest
3. Pull the remote manifest from indexd
4. Diff — anything the remote already has by hash gets skipped
5. Read the new chunk bytes from disk, pack the delta
6. Pass delta to `sia_storage` for upload
7. Write the updated manifest back to indexd

```
Local file → chunker → manifest → diff → payload → sia_storage → network
                                      ↑
                              indexd (remote manifest)
```

## Where it sits

CoreSync doesn't touch SDK internals or indexd's object API directly. Two traits define the boundaries:

| Trait | Role | Default | Live (`sia-live`) |
|-------|------|---------|-------------------|
| `ManifestStore` | read/write manifests | `InMemoryManifestStore` | `IndexdManifestStore` (HTTP) |
| `StorageBackend` | upload delta bytes | `InMemoryStorageBackend` | `SiaStorageBackend` (HTTP) |

Manifests go into indexd object metadata under `coresync:manifest` as versioned JSON. Format and wiring notes are in [docs/INTEGRATION.md](docs/INTEGRATION.md).

## CDC and edits

FastCDC picks chunk boundaries from content, not fixed offsets. That means:

**Append** — prefix chunks stay identical. Only new tail chunks upload.

**Middle insert** — chunks far from the edit usually survive. Only the neighborhood around the change gets re-chunked.

The demo and tests exercise both patterns.

## Current state

The local path is implemented: chunking, manifests, diffing, delta assembly, pipeline orchestration. Tests cover reuse on append and insert, manifest validation, and the full mocked pipeline.

Live adapters ship behind the `sia-live` feature flag:

- `src/sia_real.rs` — chunk-level PUT/HEAD against a Sia Storage HTTP endpoint
- `src/indexd_real.rs` — manifest GET/PUT against an indexd HTTP endpoint
- `examples/sia_live_demo.rs` — end-to-end demo against real credentials

Mocks remain the default so `cargo test` and the default binary run without network access.

### Integration maturity

The live adapters are **HTTP shims** around trait implementations, not verified bindings to `sia_storage::Sdk` or indexd's canonical object-metadata API. They prove the pipeline and differential sync against persistent remote state, but they do not exercise encryption, erasure coding, host selection, or official indexer object lifecycle.

See [docs/INTEGRATION.md — Integration maturity](docs/INTEGRATION.md#integration-maturity) for what is proven today, what the live shim demonstrates, remaining work, and the planned SDK-native path.

## What's next

- Native `sia_storage` SDK binding (async upload instead of HTTP shim)
- Streaming reads instead of loading whole files
- CLI for `core-sync sync <path> --key <object-key>`
- Directory-level manifests

See [docs/ROADMAP.md](docs/ROADMAP.md) for a rough ordering.
