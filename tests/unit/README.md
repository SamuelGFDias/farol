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

## `uptime-kuma` plugin tests

Unit tests for the `uptime-kuma` plugin (moved here from `plugins/uptime-kuma/` in issue #8, to
follow the same `tests/unit/` layout as `git-local`):

- **Metrics parsing** (`test_uptime_kuma_metrics_parser.py`): Prometheus text-format parsing of
  `monitor_status`/`monitor_response_time` lines, including the `-1` sentinel → `None` translation
  for "not applicable" response times and malformed/missing-line handling.
- **Status mapping**: the four known `monitor_status` values (`STATUS_MAP`) mapping to the plugin's
  status domain, and rejection of values outside `{0, 1, 2, 3}`.
- **Poller cache/error behavior** (`test_uptime_kuma_poller.py`): `MetricsCache` initial state
  (placeholder error before any read), success/error recording, and `_poll_once` behavior on
  successful fetch, unreachable instance, and invalid response body — HTTP calls are mocked, never
  real network or thread scheduling.
- **Config/secrets** (`test_uptime_kuma_config.py`, `test_uptime_kuma_secrets.py`): environment
  variable loading (`load_base_url`, `load_api_key`), with absence and empty string both collapsing
  to `None`.

Each test file inserts `plugins/uptime-kuma/` into `sys.path` at import time (same pattern as
`test_git_local_scan.py` for `git-local`) so it can import the plugin's modules directly.

### Running

```bash
# All uptime-kuma unit tests
pytest tests/unit/test_uptime_kuma_*.py -v

# Equivalent via stdlib unittest
python3 -m unittest discover -s tests/unit -p "test_uptime_kuma_*.py" -v
```

### Reference

- `specs/002-uptime-kuma-plugin/tasks.md` — T046 (test authoring)
- `specs/002-uptime-kuma-plugin/data-model.md` § 2.3 — `MetricsCache` state
