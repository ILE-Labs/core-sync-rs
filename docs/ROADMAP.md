# Roadmap

Rough plan for what comes after the current local engine.

## Done

- FastCDC chunker, SHA-256 manifests
- Diff engine, delta assembly
- Pipeline with in-memory indexd + Sia mocks
- Test suite, CI

## Up next

**Sia SDK hookup**

- `StorageBackend` adapter around `sia_storage::Sdk::upload`
- Example that uploads to a test indexer
- `PackedUpload` for batching small chunks

**indexd hookup**

- `ManifestStore` adapter reading/writing `coresync:manifest` on objects
- Test against a live indexer

**Tooling**

- CLI (`core-sync sync`, watch mode)
- Config for indexer URL + app key path

**Later**

- Streaming I/O for large files
- Multi-file / directory sync
- crates.io publish

## Not in scope here

- Custom network protocols
- Host picking, contracts, wallets — that's the SDK's job
