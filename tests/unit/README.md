# Unit Tests

Unit tests validate pure logic in isolation: state machines, version comparison, parsing, and codec functions.

## Purpose

- **PluginState Machine**: Transitions and invariants from `data-model.md` § 3
- **Version Comparison**: `MAJOR.MINOR` comparison logic (D7 from research.md) with all edge cases
- **Config Parsing**: `git-local` plugin config file reading (TOML parsing, defaults)
- **NDJSON Codec**: Encoding and decoding of single JSON lines
- **Git Operations**: Scanning directory structures, detecting `.git`, parsing `git status`/`git rev-list` output

## Running

```bash
# Rust unit tests (farol-protocol, farol-core)
cargo test --workspace --lib

# Python unit tests (git-local plugin)
pytest tests/unit/test_git_local_scan.py -v
```

## Reference

- `specs/001-walking-skeleton-git-plugin/data-model.md` § 2, § 3 — State definitions and transitions
- `specs/001-walking-skeleton-git-plugin/contracts/git-local-plugin.md` — Plugin behavior spec
