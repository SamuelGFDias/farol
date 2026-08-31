# Integration Tests

Integration tests validate end-to-end scenarios combining `farol-core` (Rust GUI) with real plugins (`git-local` Python reference implementation).

## Purpose

- **End-to-End Workflows**: Real core process + real plugin process, communicating over stdin/stdout
- **Quickstart Scenarios**: Scenarios from `specs/001-walking-skeleton-git-plugin/quickstart.md`
  - SC-001: Open Farol, widget displays repository status
  - SC-002: Auto-refresh cycles work correctly
  - SC-003: Fetch action executes and updates display
  - SC-004: Plugin crash doesn't freeze UI
  - SC-005: Version incompatibility handled gracefully
  - SC-006: Unavailable plugin path handled gracefully
  - SC-007: Missing `git` binary reported as error without crashing

## Running

Integration tests require both binaries to be built:

```bash
cargo build
# Then run harness (script-based, not cargo test)
./tests/integration/harness.sh
```

## Reference

- `specs/001-walking-skeleton-git-plugin/quickstart.md` — 7 scenarios to validate manually or via harness
- `specs/001-walking-skeleton-git-plugin/data-model.md` § State Transitions — PluginState machine
