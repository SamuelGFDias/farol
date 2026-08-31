# Contract Tests

Contract tests validate protocol messages against the language-agnostic specification in `protocol/schema/`.

## Purpose

- **NDJSON Codec**: Verify framing (newline-delimited JSON serialization and deserialization)
- **Version Comparison**: Test `MAJOR.MINOR` comparison logic (D7 from research.md)
- **Message Serialization**: Round-trip testing of each message type against `protocol/schema/v0.1/*.schema.json`
- **Handshake, Widget, Action, Error Models**: Ensure all forms serialize/deserialize correctly per spec

## Running

```bash
cargo test --test '*' --package farol-protocol
```

## Reference

- `protocol/SPEC.md` — Protocol specification (framing, handshake, versioning, errors)
- `protocol/schema/v0.1/` — JSON Schema definitions
- `specs/001-walking-skeleton-git-plugin/contracts/` — Detailed contract examples
