# Changelog

All notable changes to Kinexfluence are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Engineering standards framework (`docs/STANDARDS.md`)
- Quality metrics and targets (`docs/QUALITY.md`)
- Pull request template
- Test suite across core, CLI, and adapters (39 tests)
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

### Changed
- `sqlx` 0.7.4 -> 0.8.6 (RUSTSEC-2024-0363)
- TypeScript configs now set `noEmit` to prevent compiled output in `src/`

### Fixed
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
