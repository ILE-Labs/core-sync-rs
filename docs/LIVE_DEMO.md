# Live Demo Runbook

End-to-end verification of `examples/sia_live_demo.rs` against a local WSL
stack: PostgreSQL → indexd → renterd → `sia_storage` SDK.

## Stack status (current)

| Service | Port | Status |
|---------|------|--------|
| PostgreSQL | 5432 | Running |
| indexd (app API) | 9982 | Running |
| indexd (admin UI) | 9983 | Running |
| indexd (p2p) | 9985 | Running |
| renterd | 9980 / 9981 | Running |

### Resolved blockers

- **GeoIP database** — `indexd` requires `GeoLite2-City.mmdb` at startup.
  Resolved by copying the MaxMind test database to `~/.indexd/GeoLite2-City.mmdb`.
- **Binary URL patch** — `indexd` had a hardcoded MaxMind download URL.
  Resolved with a binary string patch to point it at a local HTTP server.

### Resolved blockers (continued)

- **`invalid signature` on `POST /auth/connect`** — Root cause: the SDK
  (`sia_storage 0.9.1`) builds the request-hash by hashing the hostname
  verbatim from the URL string (`localhost`).  indexd (Go) binds to
  `127.0.0.1:9982` and verifies the signature using *that* literal string.
  The two hashes disagree, producing `"invalid signature for \"POST\" host
  \"127.0.0.1:9982/auth/connect\""`.
  **Fix**: always use `http://127.0.0.1:9982` (never `http://localhost:9982`)
  in `SIA_INDEXER_URL`.

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
~/.renterd/start_renterd.sh & # starts renterd
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
[sync] uploading 3 chunks (N KB total)
[sync] done — uploaded N KB, saved 0 KB (0%)
```

Expected output (second run — reuse):

```text
[sync] uploading 1 chunk (N KB total) — 2 chunks reused
[sync] done — uploaded N KB, saved N KB (NN%)
```
