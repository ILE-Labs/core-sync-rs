# Integration with Sia tooling

How core-sync-rs is meant to connect to `sia_storage` and indexd. The repo now ships in-memory mocks by default and feature-gated live adapters under `sia-live`. The live path is an HTTP integration layer, not a verified upstream SDK binding.

## Stack

| Piece | Responsibility |
|-------|----------------|
| core-sync-rs | chunk, diff, assemble delta locally |
| sia_storage | upload, encrypt, erasure-code, distribute |
| indexd | object metadata, manifest lookup |

```
Local file -> core-sync-rs -> sia_storage -> hosts
                ^
              indexd
```

## indexd

Manifests live in object metadata:

```json
{
  "version": 1,
  "manifest": {
    "file_path": "backups/dataset.bin",
    "file_size": 60000,
    "chunks": [
      { "hash": "a3f2b1...", "offset": 0, "length": 8192 }
    ]
  }
}
```

Key: `coresync:manifest`

The `ManifestStore` trait in `src/indexd.rs` abstracts get/put. Production code would read this from `Sdk::object()` and write via the application API. The live HTTP adapter lives in `src/indexd_real.rs` and is enabled with `--features sia-live`.

## sia_storage

After CoreSync builds a `DeltaPayload`, chunk bytes get packed in order and passed to the SDK:

```rust
// sketch - not in repo yet
let packed = pack_delta_stream(&delta);
sdk.upload(object, Cursor::new(packed), UploadOptions::default()).await?;
sdk.pin_object(&object).await?;
```

The `StorageBackend` trait in `src/sia.rs` is the hook point. `InMemoryStorageBackend` is what tests and the default demo use today. The live adapter lives in `src/sia_real.rs` and is enabled with `--features sia-live`.

## Live demo

Create a local `.env` file before running the live example and set these variables:

- `SIA_API_ENDPOINT`
- `SIA_API_PASSWORD`
- `INDEXD_ENDPOINT`
- `INDEXD_API_KEY`

Then run:

```bash
cargo run --example sia_live_demo --features sia-live
```

## Full flow

`pipeline::sync_file` runs the sequence:

1. Chunk local file
2. `ManifestStore::get_manifest`
3. Diff + assemble delta
4. `StorageBackend::upload_delta`
5. `ManifestStore::put_manifest`

## SDK references

- Rust: [sia_storage](https://docs.rs/sia_storage) - `Builder`, `Sdk`, `upload`
- TypeScript: [@siafoundation/sia-storage](https://www.npmjs.com/package/@siafoundation/sia-storage)

Same idea either way: CoreSync produces the bytes, the SDK ships them.
