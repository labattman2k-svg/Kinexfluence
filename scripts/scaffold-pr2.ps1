# scripts/scaffold-pr2.ps1
# Creates the PR #2 artifacts. Idempotent — will not overwrite existing files
# unless -Force is passed.

param([switch]$Force)

$ErrorActionPreference = "Stop"

function Write-Utf8NoBom {
    param([string]$Path, [string]$Content, [switch]$Force)
    if ((Test-Path $Path) -and -not $Force) {
        Write-Host "Skip (exists): $Path" -ForegroundColor Yellow
        return
    }
    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Path $dir -Force | Out-Null }
    $utf8 = New-Object System.Text.UTF8Encoding($false)
    [System.IO.File]::WriteAllText((Resolve-Path -LiteralPath $dir).Path + "\" + (Split-Path -Leaf $Path), $Content, $utf8)
    Write-Host "Wrote: $Path" -ForegroundColor Green
}

# ---------- docs/STANDARDS.md ----------
Write-Utf8NoBom "docs/STANDARDS.md" @'
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
| Format | rustfmt, prettier | rustfmt --check | — |
| Lint | clippy, eslint | clippy -D warnings, eslint | — |
| Unit tests | cargo test | Yes | Yes |
| Integration | — | Yes | Yes |
| Coverage | — | Report + gate (PR #4+) | Reported |
| Benchmarks | — | Budgets | Regression report |
| Accessibility | — | axe-core (UI PRs) | Manual pass |
| Dependencies | — | cargo deny, npm audit | SBOM |
| Fixture sanitization | pre-commit hook | Yes | — |
| Signed commits | Yes | Verified | Tag signed |

## For coding agents

1. Never merge. Propose; the human disposes.
2. Never disable a check to make a build pass. Fix the check or the code, in
   a separate commit with justification.
3. Cite the standard. When trading off, name the standard honored and the
   one deferred, and open an issue for the deferral.
'@

# ---------- docs/QUALITY.md ----------
Write-Utf8NoBom "docs/QUALITY.md" @'
# Kinexfluence Quality Metrics

Targets are reviewed quarterly. Metrics without owners are decoration.

## Hard gates (fail CI)

| Metric | Target | Owner | Since |
|---|---|---|---|
| rustfmt clean | 100% | build | PR #2 |
| clippy -D warnings | 0 warnings | build | PR #2 |
| Unit + integration tests | All pass on Linux/macOS/Windows | build | PR #2 |
| cargo deny | No high/critical | security | PR #2 |
| npm audit | No high/critical | security | PR #2 |
| axe-core (UI PRs) | 0 violations | ui | PR #3 |

## Aspirational gates (reported, not enforced)

| Metric | Target | Owner | Since |
|---|---|---|---|
| Coverage per crate | >= 70% | build | PR #4 |
| Capture-to-cart P50 | < 300 ms | core | PR #4 |
| Score 100 P50 | < 400 ms | core | PR #4 |
| Cold start | < 800 ms | core | PR #4 |
| Average PR size | <= 400 lines | maintainer | PR #2 |
| Review turnaround | < 24 h | maintainer | PR #2 |
| TODO age | < 30 days | maintainer | PR #2 |

## Monthly review

Tracked and reviewed on the first Monday of each month:

- Test pass rate and flake rate.
- P50/P95 for the four perf budgets.
- Crash-free session rate (local only, user-reported).
- Dependencies with known CVEs and time-to-patch.
- ADRs landed vs. architectural changes made.
'@

# ---------- .github/PULL_REQUEST_TEMPLATE.md ----------
Write-Utf8NoBom ".github/PULL_REQUEST_TEMPLATE.md" @'
## Summary

<!-- One sentence: what does this PR do? -->

## Standards checklist

- [ ] Tests added or updated for every changed public function
- [ ] `cargo fmt --all -- --check` passes
- [ ] `cargo clippy --all-targets -- -D warnings` passes
- [ ] `cargo test --workspace` passes
- [ ] UI: `npm --prefix ui run build` passes
- [ ] Extension: `npm --prefix extension run build` passes
- [ ] `CHANGELOG.md` updated under `[Unreleased]`
- [ ] No `unwrap`/`expect` outside tests
- [ ] No `any` in TypeScript without an inline justification comment
- [ ] No PII in log macros
- [ ] Public functions have doc comments

## Auditability

- [ ] Every new mutation to project/item/probe/pack writes to `event_log`
- [ ] If the schema changed, a new migration was added (existing migrations untouched)
- [ ] If the change is architectural, an ADR was added under `docs/adr/`

## Privacy

- [ ] No new network calls outside 127.0.0.1
- [ ] No new telemetry
- [ ] No user content logged above DEBUG

## Standards trade-offs

<!-- Name the standard honored and any standard deferred, with an issue link. -->

## Test evidence

<!-- Paste commands run and their output, or a link to a CI run. -->
'@

# ---------- AGENTS.md patch (append) ----------
$agentsPath = "AGENTS.md"
if (Test-Path $agentsPath) {
    $content = Get-Content $agentsPath -Raw
    $append = @'

---

## Standards

Before every session, read `docs/STANDARDS.md`. It is the contract.

Three rules for the agent:

1. Never merge. Propose; the human disposes.
2. Never disable a check to make a build pass. Fix the check or the code
   in a separate commit, with justification in the message.
3. When making a trade-off, cite the standard honored and the standard
   deferred. Open an issue for the deferral.

Read `docs/QUALITY.md` for current metric targets. Hard gates must pass.
Aspirational gates should not regress.

## Known Windows/Tauri gotchas

- Tauri v2 requires `use tauri::Manager;` for `app.manage()`,
  `use tauri::Emitter;` for `app.emit()`, and `use tauri::Listener;`
  for `app.listen()`. These are not in the prelude.
- Tauri Windows builds require a valid multi-layer `icon.ico` (v3.00 format).
  Regenerate with `cargo tauri icon <path-to-1024px-png>` if RC2175 appears.
- Do all Rust commands from PowerShell, not Git Bash. Git Bash puts GNU
  tools ahead of MSVC tools on PATH and reintroduces the linker conflict.
'@
    Set-Content -Path $agentsPath -Value ($content + $append) -NoNewline
    Write-Host "Appended standards section to AGENTS.md" -ForegroundColor Green
} else {
    Write-Host "AGENTS.md not found; skipping" -ForegroundColor Yellow
}

# ---------- CI workflow (replace) ----------
Write-Utf8NoBom ".github/workflows/ci.yml" @'
name: ci
on:
  push: { branches: [main] }
  pull_request:

jobs:
  rust:
    strategy:
      fail-fast: false
      matrix:
        os: [ubuntu-latest, macos-latest, windows-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
        with:
          components: rustfmt, clippy
      - uses: Swatinem/rust-cache@v2
      - name: fmt
        run: cargo fmt --all -- --check
      - name: clippy
        run: cargo clippy --all-targets -- -D warnings
      - name: test
        run: cargo test --workspace
      - name: cargo deny
        if: matrix.os == 'ubuntu-latest'
        uses: EmbarkStudios/cargo-deny-action@v2

  js:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: actions/setup-node@v4
        with: { node-version: 20, cache: npm }
      - run: npm --prefix ui ci || npm --prefix ui install
      - run: npm --prefix ui run build
      - run: npm --prefix extension ci || npm --prefix extension install
      - run: npm --prefix extension run build
'@ -Force

Write-Host ""
Write-Host "PR #2 artifacts scaffolded." -ForegroundColor Green
Write-Host "Next: edit files as needed, then commit in the sequence listed in the plan." -ForegroundColor Cyan
