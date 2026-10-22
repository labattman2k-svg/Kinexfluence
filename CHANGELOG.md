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
- ONNX embedder (`crates/core/src/embeddings.rs`): `all-MiniLM-L6-v2` loaded
  through ONNX Runtime, with the model's own tokenizer. The graph emits one
  vector per token; the mean pooling over the non-padding tokens and the
  L2 normalisation are applied in Rust, which is what the
  sentence-transformers wrapper does
- `scripts/download-model.sh`, which fetches the model and its tokenizer into
  `models/` at a pinned revision and verifies both against a SHA-256. The
  model is not committed: it is 90 MB, and the core builds a different ONNX
  Runtime for every platform it targets
- The model and its tokenizer are checksum-verified again when the embedder
  loads them, as ADR 0002 promises. A model with the wrong bytes still loads
  and still returns 384 numbers, so without this check a substituted or
  corrupted model would quietly move every semantic score in the product
- S1, the semantic signal, in the scorer: the cosine similarity between a
  snippet's embedding and its project's, rescaled into [0, 1]. It carries the
  heaviest weight of the seven (0.35), and a missing embedding leaves it
  neutral at 0.5 so a machine without the model scores the same as one where
  the model simply had nothing to say
- The server holds the embedder and `GET /v1/projects/:id/snippets/scored`
  uses it, embedding the project brief and each snippet on the fly. A model
  that is not installed leaves the server working, with S1 neutral
- CI fetches the embedding model before the tests and the end-to-end run, so
  the ONNX tests execute rather than skipping themselves. The browser test
  therefore sees the same scores a developer does
- The probe engine (`crates/core/src/probe.rs`): a probe is a question put to
  the user about one item, together with the answer that came back. A settled
  probe puts a relevance floor of 0.90 under the item and a provisional one
  0.70; an unanswered probe floors nowhere, because a question the user has
  not answered is not evidence. Submitting a probe creates the item it is
  about, marks it `user_forced` in `item_project`, and writes an `event_log`
  row for both the probe and the forced item
- The probe floor is applied to the finished score, not to any of the seven
  signals. The breakdown keeps saying what the heuristic said and reports
  the floor beside it, so a reader can see that the ranking was overruled by
  the user and not by the scorer. The floor lifts a score and never lowers
  one, both in the score the API computes and in the relevance stored on
  `item_project`
- Probe HTTP API: `POST /v1/probes` creates a probe and returns it with the
  floor it bought, `GET /v1/probes/pending` lists the probes that are not
  yet settled, and `PATCH /v1/probes/:id` records the answer, which is what
  actually moves the item. A status the engine does not know is refused
  rather than stored, because a typo in `probe.status` would otherwise make
  the floor silently never apply
- The probe modal (`ui/src/components/ProbeModal.tsx`): the four questions a
  probe asks -- intent, impact, rationale, and confidence -- put to the user
  when the scorer says a snippet does not matter and the user says it does.
  The answers map onto the backend's `intent`, `impact`, `rationale`, and
  `confidence` columns, so the mechanism needed no new schema
- API client methods: `getPendingProbes`, `submitProbe`, and `answerProbe`
- Ticking a snippet in the low band opens the probe modal instead of
  selecting it: a snippet the scorer thinks is worth nothing is the one the
  user is most likely to be right about
- The probe queue: the probes in the project still waiting for an answer.
  Answering one settles it and floors that snippet
- The score breakdown shows the probe floor beside the signals
- Probe E2E test (`ui/tests/e2e/probe-flow.spec.ts`): probes a low-scoring
  snippet and checks the floor reaches the score the server computes, then
  answers a probe from the queue and checks the same
- The context pack (`crates/core/src/exporter.rs`): a project's committed
  context, rendered as Markdown, so it can be read in a terminal, pasted into
  a prompt, or committed to a repository. Only committed items are in a pack
  -- an item in the cart is a candidate, not context. Prose is grouped into
  decisions, constraints, open questions and notes, and code is fenced with
  its language, in a fence long enough to hold the backticks inside it
- `GET /v1/projects/:id_or_slug/export` returns the pack as `text/markdown`,
  and `kinexfluence project export <id_or_slug> --out <file>` writes it to a
  file, or to standard output when no file is named
- The commit flow: `commit_cart` (`crates/core/src/cart.rs`) moves every item
  in a project's cart to `status = 'committed'` with a `committed_at`, and
  writes an `event_log` row for each one. `POST /v1/projects/:id/commit`
  answers with `{"committed": N}`, and `kinexfluence cart commit --project
  <project>` does the same from the command line. The count is in the answer
  because a commit of an empty cart is a legitimate zero, and it must not
  look like a failure
- A "Commit to Project" button in the cart sidebar, which empties the cart
  and says how many items moved
- Commit E2E test (`ui/tests/e2e/commit-flow.spec.ts`): fills the cart,
  commits it, and checks the export holds what was in the cart
- A README (`README.md`): what the project is, what it takes to install it,
  the five-step loop with the commands for each step, the caveats, the
  security posture, and the licence and legal disclaimers. Every command
  in it was run against a scratch database before it was written down
- ADR 0008 (`docs/adr/0008-proprietary-licence.md`): the decision to
  license the project proprietarily rather than under the AGPL its
  manifests used to declare, and what that decision does and does not
  change about the versions already published

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
- `GET /v1/probes/pending` takes an optional `?project_id=`, so a client showing
  one project's queue does not have to fetch every project's and throw most of
  it away. The filter lives in the engine, not the client, so the rule is in
  one place
- The score bands are calibrated from the snippets the scorer scored, leaving
  out the ones a probe has floored. A floor is the user's override of the
  scorer rather than another opinion from it, and letting a floored 0.90 into
  the calibration would push the green edge up to its nominal 0.70 and paint
  every other snippet in the list grey
- The licence is proprietary, not AGPL-3.0-or-later. `LICENSE` replaces the
  copyleft terms with a non-commercial evaluation licence: you may run the
  software and use its output, and nothing else. The workspace's `license`
  field is now `LicenseRef-Proprietary`, every crate sets `publish = false`
  so the source can never reach a public registry, and `deny.toml` no longer
  tolerates an AGPL dependency. The versions already published under AGPL
  keep their AGPL grant; see section 6 of the `LICENSE` file.

### Fixed
- `add_to_cart` and `remove_from_cart` now write to `event_log`, which
  STANDARDS section 2 requires of every mutation to `item_project`. Each
  membership change is recorded as `created` or `deleted`, alongside the
  `committed` the commit flow already wrote, and the membership and its log
  row are written in the same transaction so the two cannot disagree
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
- The `snippets list` and `cart list` tables had their column headers
  swapped: the kind column was headed `TYPE` and the text column was headed
  `KIND`, so a reader saw the word `KIND` over the snippet's text. They now
  read `KIND` and `TEXT`, which is what the columns hold
- The `cart add|list|remove|commit` commands now take a project by id or by
  slug, as the changelog claimed they did and as `project show` and
  `project export` already did. They took only a UUID before, and a UUID is
  not a thing anyone types. The lookup lives in `project::resolve_id`, next
  to the `resolve` it is derived from, because the CLI cannot name a
  `SqlitePool`
