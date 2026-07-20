# Roadmap

This document describes what has been built, what is planned next, and — in
detail — the milestones that address the hardest remaining engineering
challenge: repacking delta chunks into Sia's fixed-size sector layout.

---

## Milestone 0 — Local differential sync engine ✅ Complete

**Goal**: Prove that content-defined chunking + manifest diffing eliminates
redundant upload bytes, independent of any live network service.

**Deliverables**:

- `src/chunker.rs` — FastCDC chunker with SHA-256 per-chunk hashes
- `src/manifest.rs` — `FileManifest` model, serde round-trip, validation
- `src/sync_engine.rs` — diff remote vs local manifest, classify each chunk as
  reuse or upload
- `src/payload.rs` — assemble delta: read only the byte ranges that need
  uploading, skipping all reused chunks
- `src/pipeline.rs` — orchestrate: chunk → diff → delta → upload → manifest
- `src/indexd.rs` / `src/sia.rs` — trait boundaries with in-memory mocks so
  the entire engine runs without any network access
- `tests/sync_integration.rs` — end-to-end: append and middle-insert scenarios
  prove >50% bandwidth savings on typical edits; CI verifies this on every push

**Evidence**: `cargo test` — 33/33 tests pass, including append-edit and
middle-insert reuse scenarios.

---

## Milestone 1 — Official Sia SDK integration ✅ Complete

**Goal**: Replace HTTP shims with the canonical `sia_storage` / indexd SDK path
so that real uploads use Sia's encryption, erasure coding, and host selection.

**Deliverables**:

- `src/sia_sdk.rs` — `SdkSyncAdapter` implementing both `ManifestStore` and
  `StorageBackend` via `sia_storage 0.9`
- `examples/register_app_key.rs` — one-time app-key registration against a
  live indexd instance
- `examples/sia_live_demo.rs` — end-to-end demo that uploads a file, then
  appends bytes and proves only the delta is re-uploaded
- `docs/LIVE_DEMO.md` — runbook: stack setup, known blockers (GeoIP, signature
  host mismatch), bring-up steps, expected output

**Key resolved blocker**: indexd verifies request signatures against the IP it
binds to (`127.0.0.1`). Using `localhost` in `SIA_INDEXER_URL` produces a hash
mismatch and an `invalid signature` error. The demo and all docs now enforce
`http://127.0.0.1:9982`.

---

## Milestone 2 — Repacking optimizer

**Goal**: Bridge the mismatch between FastCDC's variable-size chunks and Sia's
fixed-size sectors (4 MiB) so that differential edits do not strand partial
sectors or force host round-trips to reconstruct sector boundaries.

### The problem in detail

FastCDC produces chunks with boundaries derived from content fingerprints. A
single-byte edit near the middle of a file shifts chunk boundaries in its
neighbourhood, producing several new small chunks. Those small chunks must be
uploaded, but:

1. **Partially-filled sectors**: if the new chunks are smaller than the sector
   size, they either leave a sector partially filled (wasting space and
   increasing per-byte cost) or must be merged with surrounding data — which
   requires reading back already-stored bytes from remote hosts.

2. **Write amplification**: naively writing each delta chunk as a new sector
   object defeats the purpose of differential sync. A 500-byte edit should not
   result in a 4 MiB sector write.

3. **Read amplification on the next diff**: if chunk boundaries shift on every
   edit and sectors are not aligned to any stable boundary, the next sync must
   read back increasingly fragmented sector maps from indexd to reconstruct
   which remote bytes each chunk hash corresponds to.

### Proposed solution

Introduce a packing layer (`src/packer.rs`) between `payload.rs` and the
storage backend:

1. **Bin-pack delta chunks into sector-aligned groups.** Collect the delta
   chunks from `payload::assemble_delta` and group them greedily into 4 MiB
   bins. Chunks that do not fill a full sector on their own are coalesced with
   neighbouring delta or in-memory staging bytes.

2. **Stable sector anchors.** Assign each coalesced sector a deterministic
   content-addressed key derived from the hashes of its constituent chunks.
   This means the sector can be skipped on subsequent syncs if none of its
   constituent chunks changed — even if other chunks in the file did.

3. **Sector manifest layer.** Extend `FileManifest` with an optional
   `SectorManifest` that records, for each logical chunk, which sector object
   contains it and at which byte offset. The indexd manifest entry stores both
   the chunk-level and sector-level maps. On the next diff, the engine can
   determine not just which chunks changed but whether those changes cross a
   sector boundary, avoiding unnecessary sector reads.

4. **Partial-sector staging.** When a delta is smaller than 4 MiB, write the
   new data into a staging sector alongside any remaining space from the
   previous tail sector. Track the staging sector's fill level in the indexd
   manifest. Only once a staging sector fills to capacity (or a flush is
   explicitly requested) does it get committed as a permanent sector object.

### Deliverables

| Deliverable | Module | Description |
|-------------|--------|-------------|
| Sector packing algorithm | `src/packer.rs` | Greedy bin-pack of delta chunks into 4 MiB sectors |
| Sector manifest extension | `src/manifest.rs` | `SectorManifest` alongside `FileManifest` |
| Pipeline integration | `src/pipeline.rs` | Pack step inserted between `assemble_delta` and `upload_delta` |
| Staging sector tracker | `src/packer.rs` | Track fill level; flush only on capacity or explicit call |
| Tests | `tests/packing_integration.rs` | Small-delta → single sector; large-delta → multiple sectors; partial-sector append reuses tail |
| Benchmark | `benches/repack_bench.rs` | Measure write amplification before and after for a 1 GiB file with 10 KB edits |

### Acceptance criteria

- A 1 KB edit to a 100 MB file produces at most **one** sector write (the
  staging sector), not one sector per delta chunk.
- A 10 MB append to a 100 MB file produces at most **3** sector writes
  (⌈10 MB / 4 MiB⌉), not one per chunk.
- Two successive edits to different parts of the same file do not result in any
  shared-sector reads.
- All existing 33 tests continue to pass unchanged (the packing layer is
  opt-in via a `SyncOptions` flag).

---

## Milestone 3 — Tooling

**Goal**: Make differential sync usable from the command line and a watch mode.

**Deliverables**:

- `core-sync sync <path> --key <object-key>` CLI subcommand
- `core-sync watch <dir>` — inotify/FSEvents-backed continuous sync
- Config file (`~/.config/core-sync/config.toml`) for indexer URL, app key
  path, and packing options

---

## Milestone 4 — Production hardening

- Streaming I/O so files larger than available RAM can be chunked without
  loading into memory
- Multi-file / directory manifests with a single indexd round-trip per sync
  cycle
- crates.io release under `core-sync`

---

## Not in scope

- Custom network protocols
- Host picking, contract formation, wallet management — that is the
  `sia_storage` SDK's job, not this crate's
