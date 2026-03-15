# Auth Tests Plan

**Date:** 2026-03-15  
**Status:** Done  
**Priority:** High

## Problem

The auth subsystem has zero test coverage. The `AuthStore` is the one place
where a subtle bug — wrong permissions, failed atomic rename, TOML schema
drift — could silently corrupt or expose credentials without any visible
failure at the call site.

## Scope

Unit tests for `AuthStore` credential persistence only.

**Explicitly out of scope:**
- `extract_base_url` / `extract_account_id` helper tests — functions are
  trivially correct and failures surface immediately at runtime
- `build_provider` refactor — production complexity added for test convenience
  only, no user-facing benefit
- HTTP-dependent tests (`login`, `refresh`) — tracked separately in roadmap

## Affected files

| File | Change |
|------|--------|
| `Cargo.toml` | add `tempfile = "3"` dev-dependency |
| `src/auth/store.rs` | add `#[cfg(test)] mod tests` |
| `docs/ROADMAP.md` | add HTTP-dependent / 401→refresh→retry test item |

## Test cases

### AuthStore

- `load_missing_path_returns_default` — non-existent path → empty `AuthFile`
- `round_trip_copilot` — set + save + reload + get → fields match
- `round_trip_codex` — set + save + reload + get → fields match
- `set_copilot_preserves_codex` — both providers survive a full save/reload cycle
- `atomic_save_creates_file` — file is present on disk after `save()`
- `atomic_save_perms_0o600` (unix only) — file mode is `0o600`

## Success criteria

- `cargo clippy --all-targets -- -D warnings` passes
- `cargo test` passes with all new tests green
- No existing tests regress

## Implementation order

1. Update this plan doc (done)
2. Add `tempfile` dev-dependency
3. `AuthStore` tests
4. Roadmap update
