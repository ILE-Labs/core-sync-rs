# Roadmap

Rough plan for what comes after the current local engine.

## Done

- FastCDC chunker, SHA-256 manifests
- Diff engine, delta assembly
- Pipeline with in-memory indexd + Sia mocks
- Feature-gated live Sia storage adapter
- Feature-gated live indexd manifest adapter
- Test suite, CI

## Up next

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
