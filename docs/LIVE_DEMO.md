# Live Demo Runbook

End-to-end verification of `examples/sia_live_demo.rs` against a local WSL
stack: PostgreSQL → indexd → `sia_storage` SDK.

## Stack status (current)

| Service | Port | Status |
|---------|------|--------|
| PostgreSQL | 5432 | Running |
| indexd (app API) | 9982 | Running |
| indexd (admin UI) | 9983 | Running |
| indexd (p2p) | 9985 | Running |

### Resolved blockers

- **GeoIP database** — `indexd` requires `GeoLite2-City.mmdb` at startup.
  Resolved by copying the MaxMind test database to `~/.indexd/GeoLite2-City.mmdb`.
- **Binary URL patch** — `indexd` had a hardcoded MaxMind download URL.
  Resolved with a binary string patch to point it at a local HTTP server.
- **`invalid signature` on `POST /auth/connect`** — Root cause: the SDK
  (`sia_storage 0.9.1`) builds the request-hash by hashing the hostname
  verbatim from the URL string (`localhost`).  indexd (Go) binds to
  `127.0.0.1:9982` and verifies the signature using *that* literal string.
  The two hashes disagree, producing `"invalid signature for \"POST\" host
  \"127.0.0.1:9982/auth/connect\""`.
  **Fix**: always use `http://127.0.0.1:9982` (never `http://localhost:9982`)
  in `SIA_INDEXER_URL`.

## How repacking works

Sia stores data in fixed-size sectors (4 MiB). CoreSync's content-defined chunks
(FastCDC) are variable-size, so there is a boundary mismatch when chunks are
written to sectors and then edited locally.

Rather than re-uploading the whole file, CoreSync:

1. Chunks the new local version with FastCDC.
2. Diffs the new manifest against the remote manifest stored in indexd.
3. **Repacks only the changed offsets** into a minimal delta payload
   (`src/payload.rs`).
4. Uploads the delta bytes; unchanged chunks are never read from disk again.
5. Writes the updated manifest back to indexd so the next sync starts from the
   new baseline.

This is why the second-run output shows `reused N chunks` — those chunk hashes
matched the remote manifest and were skipped entirely, including any sectors they
occupy on Sia hosts.

See [ARCHITECTURE.md — The Repacking Challenge](../ARCHITECTURE.md#the-repacking-challenge)
for the full design rationale.

## Bring-up checklist

These steps are already done. Listed here so the state can be reproduced from
scratch if WSL is reset.

### 1. PostgreSQL

```bash
sudo service postgresql start
sudo -u postgres psql -c "CREATE ROLE indexd LOGIN PASSWORD 'indexd';"
sudo -u postgres psql -c "CREATE DATABASE indexd OWNER indexd;"
```

### 2. indexd config (`~/.indexd/indexd.yml`)

```yaml
recoveryPhrase: "<12-word BIP-39 phrase>"
adminAPI:
  password: "<admin-password>"
  address: "localhost:9983"
syncer:
  address: ":9985"
database:
  host: "localhost"
  port: 5432
  user: "indexd"
  password: "indexd"
  database: "indexd"
  sslmode: "disable"
```

### 3. GeoIP workaround

```bash
# Copy the MaxMind test database (bundled with indexd test suite or downloaded)
cp /path/to/GeoIP2-City-Test.mmdb ~/.indexd/GeoLite2-City.mmdb
```

### 4. Start services

```bash
~/.indexd/start_indexd.sh &   # starts indexd
```

## Register an app key (one-time)

Run this once to obtain a `SIA_APP_KEY` for the demo:

```bash
export SIA_INDEXER_URL=http://127.0.0.1:9982   # must be IP, not 'localhost' — see resolved blockers
cargo run --example register_app_key --features sia-sdk
```

The tool prints an approval URL. Open it in the indexd UI and approve. It then
prints the 64-hex `SIA_APP_KEY` — paste it into `.env`.

## Run the live demo

```bash
cp .env.example .env
# Fill SIA_INDEXER_URL and SIA_APP_KEY in .env

cargo run --example sia_live_demo --features sia-sdk -- ./testfile.txt
```

Expected output (first run — full upload):

```text
Scenario: Initial upload
  First sync — full file chunked and registered
  object key: data/dataset.bin
  -------------------------------------------------------
  50000 bytes, 5 chunks
  first upload — no remote manifest
  reused 0 chunks, uploading 5 (0.0% reuse)
  delta: 50000 bytes in 5 chunks (mock upload)
```

Expected output (second run — reuse after append):

```text
Scenario: Append edit
  Append 10 KiB — only delta uploaded
  object key: data/dataset.bin
  -------------------------------------------------------
  60000 bytes, 5 chunks
  reused 4 chunks, uploading 1 (80.0% reuse)
  delta: 10998 bytes in 1 chunks (mock upload)
  saved 49002 bytes vs full file (81.7%)
```
