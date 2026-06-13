# Integration with Sia tooling

This document explains how `core-sync-rs` connects to Sia through two feature-gated paths:

- `sia-sdk` for the official `sia_storage` / indexd SDK wiring
- `sia-live` for HTTP compatibility adapters used as an integration shim

## Integration maturity

The local diff engine is complete and tested. The SDK-backed adapter is implemented, and the HTTP shim remains available for compatibility testing or environments that still expect the CoreSync-specific routes.

### What is complete and independently verifiable

These modules do not depend on any live service:

| Layer | Module | Evidence |
|-------|--------|----------|
| Content-defined chunking | `chunker.rs` | Unit tests and CDC boundary cases |
| Manifest model | `manifest.rs` | Serde round-trip and validation |
| Remote vs local diff | `sync_engine.rs` | Reuse on append, insert, identical files |
| Delta assembly | `payload.rs` | Reads only changed byte ranges from disk |
| Pipeline orchestration | `pipeline.rs` | Full mocked end-to-end sync |
| Integration tests | `tests/sync_integration.rs` | Initial upload and delta sync scenarios |

### SDK-backed path

The `sia-sdk` feature uses the official Rust SDK from `SiaFoundation/sia-sdk-rs`.

It does three things:

1. Connects to indexd with `Builder::connected`
2. Uploads data with `Sdk::upload` and `Sdk::pin_object`
3. Persists manifests with `Sdk::update_object_metadata`

The SDK adapter in `src/sia_sdk.rs` stores a durable manifest envelope in object metadata so the logical CoreSync object key can be rediscovered across process restarts.

Required environment variables:

- `SIA_INDEXER_URL`
- `SIA_APP_KEY`

Example:

```bash
cargo run --example sia_live_demo --features sia-sdk -- ./testfile.txt
```

## HTTP compatibility path

The `sia-live` feature keeps the previous HTTP integration layer available:

- `PUT /chunks/{hash}`
- `HEAD /chunks/{hash}`
- `GET /manifests/{key}`
- `PUT /manifests/{key}`

This path is useful when you want to prove the sync pipeline against persistent remote state using a compatible proxy or adapter. It is not the same thing as the official SDK path.

Required environment variables:

- `SIA_API_ENDPOINT`
- `SIA_API_PASSWORD`
- `INDEXD_ENDPOINT`
- `INDEXD_API_KEY`

Example:

```bash
cargo run --example sia_live_demo --features sia-live -- ./testfile.txt
```

## Full flow

`pipeline::sync_file` runs the sequence:

1. Chunk local file
2. `ManifestStore::get_manifest`
3. Diff and assemble delta
4. `StorageBackend::upload_delta`
5. `ManifestStore::put_manifest`

## What to expect

| Question | Answer |
|----------|--------|
| Does CoreSync reduce upload bytes locally? | Yes |
| Does the SDK-backed path use official Sia tooling? | Yes |
| Does the shim path prove persistence and delta reuse? | Yes, if you point it at compatible endpoints |
| Are both paths feature-gated? | Yes |

## Live demo

The live demo reads `.env` if present. Copy `.env.example` to `.env`, fill in the credentials for the path you want, then run the matching command above.
