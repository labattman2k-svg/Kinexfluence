# Changelog

All notable changes to Kinexfluence are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Engineering standards framework (`docs/STANDARDS.md`)
- Quality metrics and targets (`docs/QUALITY.md`)
- Pull request template
- Test suite across core, CLI, and adapters (92 tests)
- CI gates for fmt, clippy, test, and cargo-deny
- `deny.toml` with advisory allowlist
- ChatGPT export parser (`crates/adapters`), fixture-tested. CLI wiring is
  PR #3b; live DOM capture is deferred by ADR 0007
- `scripts/sanitize-fixture.sh`, a best-effort fixture sanitizer
- `SECURITY.md` disclosure policy stub, including an explicit note that the
  address and PGP key are placeholders
- Schema migrations for attachment status and conversation content hash
  (`0002_attachment_status.sql`, `0003_conversation_content_hash.sql`)
- Migration patterns guide (`docs/migrations.md`)
- Snippet extractor for canonical messages (`crates/core/src/normalizer`):
  text blocks split on blank lines, code kept whole, SHA-256 over
  whitespace-collapsed lowercase text
- Capture normalizer (`crates/core/src/normalizer`): `normalize` turns parsed
  `RawCapture`s into rows for conversation, message, snippet, and attachment,
  resolving provider parent ids to generated message ids, converting epoch
  timestamps to RFC 3339, and hashing conversation content for re-import
  detection. Pure transformation; the writes are `writer::write_batch`
- `write_batch` and `ImportSummary` (`crates/core/src/writer.rs`): writes a
  normalized batch into conversation, message, snippet, and attachment in one
  transaction, creating the `source` row a first import needs. Re-importing
  the same export inserts nothing, because a conversation whose content hash
  is already present is skipped whole
- `kinexfluence import chatgpt <path>` wired end to end: read, parse,
  normalize, write, and print a summary. Exit codes distinguish an adapter
  failure (2) from a normalizer failure (3) and a database failure (4)
- Project entity (`crates/core/src/project.rs`): `create`, `list`,
  `list_by_status`, `get`, `find_by_slug`, `resolve`, and `update_status`, with
  generated and deduplicated slugs, uuid v7 ids, and an `event_log` row for
  every mutation
- CLI `project` subcommands: `create`, `list`, `show`, and `status`. `create`
  prints only the new id on stdout, so it can be captured in a script
- Project HTTP API (`crates/core/src/api.rs`): `GET /v1/projects`,
  `POST /v1/projects`, `GET /v1/projects/:id_or_slug`, and
  `PATCH /v1/projects/:id_or_slug`, with a structured JSON error body and
  status codes derived from `ProjectError`
- Atlas UI (`ui/`): project list, create, and detail views wired to the HTTP
  API. Plain CSS for now; the first end-to-end UI test is a follow-up
- `kinexfluence serve --port <port> --db <path>`: runs the HTTP API in the
  foreground, so the server can be started without the desktop shell
- Playwright end-to-end suite (`ui/tests/e2e`), plus an `e2e` CI job that runs
  it against a real core and a real database
- Snippet browsing service (`crates/core/src/snippet.rs`): `list` with limit and
  offset for paging through imported fragments
- Cart management service (`crates/core/src/cart.rs`): `add_to_cart`,
  `list_cart`, and `remove_from_cart`. Materialises an `item` from a `snippet`
  on first use and reuses it afterwards, so adding the same snippet twice is
  idempotent
- Cart HTTP API: `GET /v1/snippets`, `POST` and `GET /v1/projects/:id/cart`, and
  `DELETE /v1/projects/:id/cart/:item_id`
- CLI `snippets list [--limit] [--offset]` and the `cart add|list|remove` group,
  which accept a project id or slug
- API client methods: listSnippets, getCart, addToCart, removeFromCart
- SnippetList UI component: displays imported snippets with checkboxes
- CartSidebar UI component: displays cart items with remove buttons
- Split-view layout: snippet browser on left, cart sidebar on right
- Cart flow E2E test: create project, add to cart, remove from cart
- Heuristic scoring engine and entity extraction (`crates/core/src/scorer.rs`):
  evaluates snippets with signals S2-S7 (code, decision markers, project
  entities, novelty stub, position within conversation turns, and source
  provenance), composed through a sigmoid normalised by the reachable weight
  total so a strong snippet can clear the 0.70 auto-select threshold
- Scored snippets HTTP API: `GET /v1/projects/:id/snippets/scored`, returning
  each candidate with its final score and per-signal breakdown, sorted by
  descending score
- Score UI: each snippet shows a score badge rounded to two decimals, with a
  breakdown panel (Code, Markers, Entities, Position) behind an info button.
  Snippets in the high band start checked, and an "Add N to Cart" button
  commits the selection, so a default check is a proposal rather than a
  decision
- Adaptive score bands: the green and amber thresholds scale with the best
  score in the list, capped by the nominal 0.70 and 0.40 edges. A fixed
  threshold would paint every real snippet grey, because the reachable
  scores depend on the brief and the conversation rather than on the
  quality of the snippet alone
- `ScoredSnippet` and `ScoreDetails` types and the `listScoredSnippets` API
  client method
- The cart sidebar shows the score of each item, matched back to its snippet
  through `snippet_id`, using the same bands as the snippet list
- A shared `ScoreBadge` component, so the snippet list and the cart sidebar
  render the same badge: the number, the band colour, and a glyph that carries
  the band for anyone who cannot see the colour
- Scoring E2E test (`ui/tests/e2e/scoring-flow.spec.ts`): reads a score, opens
  the breakdown, checks the auto-selection, and bulk-adds to the cart. The
  project brief is derived from the fixture's own snippets, so the entity
  match holds whatever the fixture contains

### Changed
- `sqlx` 0.7.4 -> 0.8.6 (RUSTSEC-2024-0363)
- TypeScript configs now set `noEmit` to prevent compiled output in `src/`
- `CreateProject` now accepts an optional explicit slug
- `ProjectError` now includes `InvalidStatus` for schema-value validation
- The core server answers CORS for the Atlas UI's origins only
  (`http://localhost:5173` in development, and Tauri's `tauri://localhost` /
  `http://tauri.localhost`). Other origins are refused, so a page the user
  happens to visit cannot read their projects
- The Atlas UI discovers the core's port (probing 47821-47830 and caching the
  answer) instead of assuming 47821, which the core only uses when it is free

### Fixed
- Corrected docs: `-- no-transaction` is inert on SQLite in sqlx 0.8.x
- `crates/tauri` propagates the Tauri run error instead of panicking on it.
  This was the last `expect` outside tests in the tree
- CI `js` job: `actions/setup-node` with `cache: npm` resolves the lockfile
  from the repository root, so it failed even after the lockfiles were
  committed. Adds `cache-dependency-path`.
- CI `rust` job: `cargo clippy --all-targets` builds every workspace member,
  including `kfx-tauri`, which needs the webkit2gtk/GTK system libraries on
  Linux. Installs them on the ubuntu leg.
- `crates/tauri/icons/icon.png` was missing, so `tauri::generate_context!()`
  panicked on macOS and Linux. Windows resolved the default icon to the
  existing `icon.ico` and masked it.
- `cargo deny` config migrated to cargo-deny 0.20; `[licenses] version = 2`
  is required or the `allow` list is silently inert
- Duplicate ADR numbering: the SQLCipher deferral ADR is now 0006
- `docs/STANDARDS.md` section 10 stated the coverage gate as PR #4+; it is
  PR #6, matching section 1 and `docs/QUALITY.md`
