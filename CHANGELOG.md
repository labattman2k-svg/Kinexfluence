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

### Changed
- `sqlx` 0.7.4 -> 0.8.6 (RUSTSEC-2024-0363)
- TypeScript configs now set `noEmit` to prevent compiled output in `src/`
- `CreateProject` now accepts an optional explicit slug
- `ProjectError` now includes `InvalidStatus` for schema-value validation

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
