# Kinexfluence Engineering Standards

Status: active
Owner: project maintainer
Last revised: 2026-10-15

This document is the canonical engineering standard for Kinexfluence.
It applies equally to human contributors and coding agents. Where this
document and an ADR disagree, this document wins unless the ADR is
newer and explicitly supersedes.

## 1. Testing

No feature ships without a test that would fail if the feature regressed.
Coverage is a floor, not a goal.

- Every public function has at least one test.
- Every HTTP endpoint has an integration test.
- Scoring invariants (bounds, monotonicity, determinism) have property tests.
- Adapter fixtures are snapshot-tested. A fixture change requires reviewer sign-off.
- All fixtures are sanitized. `scripts/sanitize-fixture.sh` exists for this.
- Coverage gate: 70% per crate from PR #4 onward. Reported as a PR comment.

## 2. Auditability

Every state change is reconstructable. Every decision is traceable.

- All mutations to `project`, `item`, `item_project`, `probe`, `context_pack`
  emit a row to `event_log`.
- Migrations are forward-only, numbered, and never edited after merge.
- Non-trivial decisions become ADRs under `docs/adr`. ADRs are immutable
  once accepted; superseding requires a new ADR that references the old.
- Commits use Conventional Commits and are signed.
- Releases publish checksums, an SBOM, and a signed tag.

## 3. Scalability

Every component has a documented capacity and a plan for what happens past it.

| Metric | Budget |
|---|---|
| Capture to cart rendered (50 msgs, cached) | < 300 ms |
| Score 100 snippets | < 400 ms |
| Embedding throughput (M1 CPU) | >= 300/sec |
| Core cold start | < 800 ms |
| Cart query (10k items) | < 50 ms |
| WebSocket event latency | < 30 ms local |

- Every query has an EXPLAIN QUERY PLAN test in CI.
- Every background worker has a bounded queue and documented backpressure.
- Every cache has an eviction policy and a maximum size.
- New features declare worst-case complexity in the PR description.

## 4. Reliability

The user's data survives crashes, upgrades, and user mistakes.

- SQLite in WAL mode with `synchronous=NORMAL`.
- Daily `VACUUM INTO` backup, rotated 7 daily / 4 weekly / 12 monthly.
- `kinexfluence recover` CLI command rebuilds from backup + event log.
- Destructive operations have a 10-second undo or a confirmation dialog, never both.
- Schema downgrade is not supported; upgrades are tested against the prior two versions.

## 5. Privacy and Security

Privacy claims are architectural, not aspirational.

- No network calls outside 127.0.0.1 unless sync is explicitly enabled.
- Server binds loopback only; origin check enforced.
- Auth token at `~/.kinexfluence/token`, mode 0600. Server refuses to start otherwise.
- No PII in logs at INFO or above. DEBUG logging is opt-in and warns.
- `SECURITY.md` documents disclosure process and PGP key.
- Threat model in `docs/THREAT_MODEL.md` before public beta.

## 6. Accessibility

- Every interactive element keyboard-reachable, focus visible.
- Score indicators use icon + color, never color alone.
- WCAG 2.2 AA target.
- `axe-core` runs in Playwright tests; failures block merge.

## 7. Code Quality

- Rust: rustfmt, clippy -D warnings, no unwrap/expect outside tests.
- TypeScript: strict mode, no `any` without a documented reason, ESLint clean.
- PRs under 400 lines of diff, excluding migrations and generated files.
- Every PR updates `CHANGELOG.md` under `[Unreleased]`.
- Public functions have doc comments.
- No commented-out code in main. TODOs link to an issue.

## 8. Versioning and Compatibility

- Core exposes `/v1/version` and `/v1/compat`.
- Extension refuses to talk to an incompatible core; shows upgrade prompt.
- Schema version bumped on every migration. App refuses newer-than-understood schemas.
- Export formats are versioned. Context packs declare their format version.
- Semver applies to CLI, HTTP API, and extension.

## 9. Observability

Logs and traces serve debugging, not surveillance.

- `tracing` structured logs with stable field names.
- Levels: ERROR (unexpected), WARN (recoverable anomaly), INFO (lifecycle),
  DEBUG (dev), TRACE (hot paths).
- No PII in logs. CI greps log macros for known PII patterns.
- Optional local `/v1/metrics` endpoint for users running their own instance.
- No remote telemetry.

## 10. Enforcement

| Gate | Local | CI (per PR) | Release |
|---|---|---|---|
| Format | rustfmt, prettier | rustfmt --check | â€” |
| Lint | clippy, eslint | clippy -D warnings, eslint | â€” |
| Unit tests | cargo test | Yes | Yes |
| Integration | â€” | Yes | Yes |
| Coverage | â€” | Report + gate (PR #4+) | Reported |
| Benchmarks | â€” | Budgets | Regression report |
| Accessibility | â€” | axe-core (UI PRs) | Manual pass |
| Dependencies | â€” | cargo deny, npm audit | SBOM |
| Fixture sanitization | pre-commit hook | Yes | â€” |
| Signed commits | Yes | Verified | Tag signed |

## For coding agents

1. Never merge. Propose; the human disposes.
2. Never disable a check to make a build pass. Fix the check or the code, in
   a separate commit with justification.
3. Cite the standard. When trading off, name the standard honored and the
   one deferred, and open an issue for the deferral.