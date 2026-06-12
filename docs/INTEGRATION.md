# Integration with Sia tooling

How core-sync-rs is meant to connect to `sia_storage` and indexd. The repo now ships in-memory mocks by default and feature-gated live adapters under `sia-live`.

## Integration maturity

Adopters should understand exactly what is proven today, what the live demo actually demonstrates, and what remains before this is a verified, production-grade binding to official Sia tooling. The library is actively in development — the local sync engine is solid; SDK-native wiring is the next milestone.

### What is complete and independently verifiable

The **local differential sync engine** is finished, tested, and not blocked on any external service:

| Layer | Module | Evidence |
|-------|--------|----------|
| Content-defined chunking | `chunker.rs` | Unit tests + CDC boundary cases |
| Manifest model | `manifest.rs` | Serde round-trip, validation rules |
| Remote vs local diff | `sync_engine.rs` | Reuse on append, insert, identical files |
| Delta assembly | `payload.rs` | Reads only changed byte ranges from disk |
| Pipeline orchestration | `pipeline.rs` | Full mocked end-to-end sync |
| Integration tests | `tests/sync_integration.rs` | Initial upload + delta sync scenarios |

This is what CoreSync is built to do: **minimize bytes before they reach Sia**. That logic runs entirely on the client, produces a `DeltaPayload` with only new chunk bytes, and reports reuse ratio and bandwidth saved. None of that depends on HTTP shims or live credentials.

The **trait boundaries** (`StorageBackend`, `ManifestStore`) are the correct long-term architecture. CoreSync does not reimplement encryption, erasure coding, host selection, contracts, or wallet logic — those stay in `sia_storage` and the indexer.

### What the `sia-live` HTTP adapters actually are

The live path (`src/sia_real.rs`, `src/indexd_real.rs`) is an **integration shim**, not a certified binding to the official `sia_storage` Rust crate or indexd's canonical object-metadata API.

```
Today (sia-live)                    Target (production)
─────────────────                   ───────────────────
reqwest PUT/HEAD                    sia_storage::Sdk::upload
  → /chunks/{hash}                    → erasure-code, encrypt, distribute
reqwest GET/PUT                     Sdk::object + update_object_metadata
  → /manifests/{key}                  → coresync:manifest on pinned Object
```

The shim proves three things that matter if you are evaluating adoption:

1. **The pipeline wires correctly** — `sync_file()` can drive real I/O through trait implementations, not just mocks.
2. **Differential sync works against persistent remote state** — a manifest stored after run 1 is read on run 2, chunks already present are skipped, and upload bytes drop sharply.
3. **The integration contract is stable** — env vars, object keys, manifest JSON envelope, and delta handoff are defined and demoed.

The shim does **not** prove:

| Gap | Why it matters |
|-----|----------------|
| **No `sia_storage::Sdk` calls** | Uploads skip client-side encryption, erasure coding, slab packing, host RPC, and contract accounting that the official SDK performs. Bytes may reach a custom HTTP endpoint, but not necessarily the Sia network through the supported path. |
| **Custom HTTP routes** | `PUT /chunks/{hash}` and `GET /manifests/{key}` are CoreSync-defined routes. They are not renterd's `/api/worker/objects/…`, not the S3 worker layer, and not indexd's standard object-metadata fields. Running only renterd + official indexd against these URLs will not work without a compatible proxy or adapter. |
| **No upstream conformance suite** | There is no automated test against `sia_storage` releases or indexd API versions yet. API drift in upstream tooling would not be caught by CI. |
| **Blocking HTTP client** | Production SDK usage is async (`Sdk::upload` is `async`). The shim uses blocking `reqwest`, which is acceptable for a demo but not how downstream apps will embed CoreSync beside an async SDK runtime. |
| **Chunk-level vs object-level storage** | Sia stores erasure-coded **slabs** attached to **objects**. The shim uploads raw chunks by content hash. Mapping chunk hashes to slabs, objects, and pin lifecycle is integration work still to do. |
| **Auth models differ** | Shim storage uses HTTP basic (`sia` / password); renterd uses empty-username basic; `sia_storage` uses `AppKey` derived from user approval. Shim indexd uses Bearer token, which aligns better with `AppKey`, but manifest storage path still differs from `update_object_metadata`. |

**Bottom line:** a successful `sia_live_demo` run is strong evidence that **CoreSync's diff engine saves bandwidth when remote state is available**. It is not, by itself, evidence that **CoreSync is a drop-in, officially verified extension of `sia_storage` and indexd** — that path is still under active development.

### Architectural risk if left as-is

If the HTTP shim is mistaken for full Sia integration:

- **Adopters** may assume integration is complete when SDK-native upload, pin, and metadata wiring is still open.
- **Teams trying renterd or official indexd directly** may point those services at the shim URLs and fail — unless they also deploy a compatible proxy or wait for the SDK-backed adapters.
- **Security posture** is unclear: chunk bytes on a custom HTTP endpoint may not be encrypted the way `sia_storage` encrypts before upload.
- **Durability guarantees** differ: Sia's redundancy and repair paths are not exercised through a raw PUT.

The risk is **integration completeness**, not **sync algorithm correctness**. The algorithm is tested; the last mile to official tooling is not.

### What production-grade integration looks like

Production `StorageBackend` implementation:

```rust
// Target shape — not yet in repo
let packed = pack_delta_stream(&delta);
let object = sdk.upload(Object::default(), Cursor::new(packed), UploadOptions::default()).await?;
sdk.pin_object(&object).await?;
```

Production `ManifestStore` implementation:

```rust
// Target shape — manifest lives on the pinned Object
let object = sdk.object(&object_id).await?;
let manifest = parse_manifest_record(&object.metadata["coresync:manifest"])?;
// after sync:
object.metadata.insert(MANIFEST_METADATA_KEY, record.to_json()?);
sdk.update_object_metadata(&object).await?;
```

Checklist for SDK-native integration (planned, not yet shipped):

1. `StorageBackend` calls `sia_storage::Sdk::upload` + `pin_object` with assembled delta bytes (or slab-aware upload if the official SDK recommends a chunk-native path).
2. `ManifestStore` reads/writes `coresync:manifest` via `Sdk::object` and `update_object_metadata` on a pinned object keyed by `object_key`.
3. CI or a manual conformance checklist runs against a documented indexer + SDK version.
4. `sia_live_demo` (or successor) runs through SDK path, not custom HTTP routes.
5. Second-sync bandwidth savings reproduced through SDK path — same proof as today, different transport.

Estimated scope: **one focused integration milestone** (SDK-backed trait impls + demo update), not a rewrite of chunker/manifest/diff/payload.

### What you can rely on today

| Question | Answer |
|----------|--------|
| Does CoreSync reduce upload bytes locally? | **Yes** — tested, reproducible without network. |
| Does the pipeline orchestrate chunk → diff → upload → manifest? | **Yes** — mocked and live-shim paths. |
| Does differential sync work when remote manifest persists? | **Yes** — live demo second run, if shim endpoints are available. |
| Is it a verified `sia_storage` / indexd binding? | **Not yet** — HTTP shim today; SDK path documented above. |
| Ready for production deployment as a full Sia client? | **Not yet** — until SDK-native adapters ship. |
| Right architecture to reach production? | **Yes** — traits isolate sync logic from transport. |

### Try it yourself

1. `cargo test` — validates the sync engine without credentials (32 tests).
2. `cargo run --example diff_two_files -- old new` — shows the diff plan on arbitrary local files.
3. If you have shim-compatible endpoints: `cargo run --example sia_live_demo --features sia-live -- ./file` — demonstrates persistent-state delta sync.
4. Read this section — know the shim vs SDK boundary before depending on live integration.

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

The `ManifestStore` trait in `src/indexd.rs` abstracts get/put. Production apps can also read manifests from `Sdk::object()` metadata and write via `update_object_metadata`.

The live HTTP adapter in `src/indexd_real.rs` (`--features sia-live`) persists versioned `ManifestRecord` JSON:

| Operation | HTTP | Path |
|-----------|------|------|
| Get manifest | `GET` | `{INDEXD_ENDPOINT}/manifests/{object_key}` |
| Put manifest | `PUT` | `{INDEXD_ENDPOINT}/manifests/{object_key}` |

Auth: `Authorization: Bearer {INDEXD_API_KEY}`. Set `INDEXD_ENDPOINT` and `INDEXD_API_KEY` in `.env`.

## sia_storage

After CoreSync builds a `DeltaPayload`, chunk bytes get packed in order and passed to storage:

```rust
let packed = pack_delta_stream(&delta);
sdk.upload(object, Cursor::new(packed), UploadOptions::default()).await?;
sdk.pin_object(&object).await?;
```

The `StorageBackend` trait in `src/sia.rs` is the hook point. `InMemoryStorageBackend` is what tests and the default demo use today.

The live HTTP adapter in `src/sia_real.rs` (`--features sia-live`) uploads individual chunks by hash:

| Operation | HTTP | Path |
|-----------|------|------|
| Upload chunk | `PUT` | `{SIA_API_ENDPOINT}/chunks/{hash}` |
| Check chunk | `HEAD` | `{SIA_API_ENDPOINT}/chunks/{hash}` |

Auth: HTTP basic (`sia` / `SIA_API_PASSWORD`). Set `SIA_API_ENDPOINT` and `SIA_API_PASSWORD` in `.env` (see `.env.example`).

## Live demo

1. Copy `.env.example` to `.env` and fill in credentials.
2. Run:

```bash
cargo run --example sia_live_demo --features sia-live -- ./testfile.txt
```

The demo syncs a file, appends a few bytes, and syncs again so the second run uploads only the delta.

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
