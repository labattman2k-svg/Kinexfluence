# Standards Exceptions

This document records exceptions to hard gates defined in `docs/STANDARDS.md`.
An exception without an issue link is not an exception; it is a violation.

| Date | Standard | Reason | Issue | Expires |
|---|---|---|---|---|
| 2026-10-15 | §7 PR size (800-line cap) | Brief bundled six deliverables (parser, tests, sanitizer, SECURITY, tauri fix, ADR addendum) under a cap sized for one. Irreducible core is 1019 lines. | #8 | 2026-11-15 |
| 2026-10-21 | §7 PR size (800-line cap) | PR #10 (embeddings) is 1349 lines excluding the generated `Cargo.lock`. The embedder (449) and its tests (311) cannot be separated without shipping a module that cannot be tested, and the S1 weight change moves every score in the product, so the scorer tests are not optional. Irreducible core is the embedder plus its tests at 760 lines. | #34 | 2026-11-21 |
| 2026-10-21 | §7 PR size (800-line cap) | PR #11a (the probe engine) is 1574 lines excluding the generated `Cargo.lock`. The engine (576) and its tests (455) cannot be separated without shipping a probe that is recorded but moves nothing, which is the exact failure the mechanism exists to fix, and §1 requires the test that would catch it. Irreducible core is the engine plus its tests at 1031 lines. | #36 | 2026-11-21 |
