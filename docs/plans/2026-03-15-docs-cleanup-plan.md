# Documentation cleanup and reconciliation plan

**Date:** 2026-03-15
**Status:** Completed
**Scope:** `docs/`

## Goal

Aggressively prune stale historical docs and reconcile the remaining canonical documentation with the current `tau` implementation.

## Scope

### Keep (canonical + active)
- `docs/ARCHITECTURE.md`
- `docs/ROADMAP.md`
- `docs/USER-INTERFACE-SPEC.md`
- `docs/plans/2026-03-15-provider-auth-redesign-design.md`
- `docs/plans/2026-03-14-context-management.md`
- `docs/plans/2026-03-14-tests.md`
- `docs/plans/2026-03-15-docs-cleanup-plan.md` (this file)

### Remove (stale, implemented, superseded, or low-value historical)
- Prune historical exploratory and superseded implementation notes under `docs/plans/`.
- Keep only active, decision-relevant plan/design artifacts.
- Record exact removals in git history rather than in long-lived docs.

## Reconciliation work

1. Update `docs/ROADMAP.md` to remove contradictions with current code:
   - Auth redesign is no longer pending (tau-owned auth + in-app `/login` already exists).
   - Reframe remaining auth work as hardening (keyring storage, polish, CI).
2. Update `docs/ARCHITECTURE.md` links/references if any deleted plan links remain.
3. Add a compact `docs/plans/README.md` explaining that `docs/plans` now contains only active plans/design artifacts.

## Verification

- Ensure no broken references to removed plan files:
  - `rg -n "docs/plans/|plans/" docs`
- Ensure docs set is pruned as intended:
  - `find docs -maxdepth 3 -type f | sort`
- Run quality checks required by repo policy:
  - `cargo test`
  - `cargo clippy --all-targets --all-features -- -D warnings`

## Success criteria

- `docs/` contains a small, current set of canonical docs.
- `ROADMAP.md` and `ARCHITECTURE.md` are consistent with the current implementation.
- No broken doc links to deleted files.
- Project checks pass after documentation edits.
