# Contract Tests

Contract tests validate protocol messages against the language-agnostic specification in `protocol/schema/`.

## Purpose

- **NDJSON Codec**: Verify framing (newline-delimited JSON serialization and deserialization)
- **Version Comparison**: Test `MAJOR.MINOR` comparison logic (D7 from research.md)
- **Message Serialization**: Round-trip testing of each message type against `protocol/schema/v0.1/*.schema.json`
- **Handshake, Widget, Action, Error Models**: Ensure all forms serialize/deserialize correctly per spec

## Where the tests actually live

This directory is the conceptual home of the protocol's contract tests, but Cargo only ever
compiles/runs integration tests found inside `<crate>/tests/*.rs` — a loose `tests/` directory at
the workspace root (outside any crate) is never picked up by `cargo test`, regardless of what's
inside it. So the tests described above are split across two places:

- **NDJSON codec** and **version comparison (D7)**: unit tests (`#[cfg(test)]`) inside
  `crates/farol-protocol/src/framing.rs` and `crates/farol-protocol/src/version.rs` — these only
  need the Rust types to agree with themselves (round-trip), so they live next to the code they
  test.
- **JSON Schema contract validation** (message (de)serialization checked against the normative
  `protocol/schema/v0.1/*.schema.json`, not just against the Rust types' own round-trip): the
  integration test at
  [`crates/farol-protocol/tests/contract_schema_validation.rs`](../../crates/farol-protocol/tests/contract_schema_validation.rs).
  It loads all 4 schemas together into a `jsonschema::Registry` (they cross-reference each other
  via `$id`/`$ref`, so they must be resolved as a set), then validates a serialized instance of
  each message form — handshake, widget, action, and the shared error object — against its
  corresponding schema, with both a positive case and a deliberately-invalid negative case per
  schema.

## Running

```bash
# Codec + version unit tests, and the JSON Schema contract test — all under one package:
cargo test --package farol-protocol

# Just the JSON Schema contract test:
cargo test --package farol-protocol --test contract_schema_validation
```

## Reference

- `protocol/SPEC.md` — Protocol specification (framing, handshake, versioning, errors)
- `protocol/schema/v0.1/` — JSON Schema definitions
- `specs/001-walking-skeleton-git-plugin/contracts/` — Detailed contract examples
