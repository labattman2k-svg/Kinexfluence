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
| Coverage per crate | >= 70% | build | PR #6 (hard gate) |
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

## Hard gate timeline

| Gate | Becomes hard at |
|---|---|
| rustfmt, clippy, tests | PR #2a |
| cargo deny (high/critical) | PR #2a |
| Coverage 70% per crate | PR #6 |
| Signed commits | PR #4 |
| axe-core 0 violations | PR #3 (first UI PR) |