# Changelog

All notable changes to Kinexfluence are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Engineering standards framework (`docs/STANDARDS.md`)
- Quality metrics and targets (`docs/QUALITY.md`)
- Pull request template
- Test suite for core and CLI (11 tests)
- CI gates for fmt, clippy, test, and cargo-deny
- `deny.toml` with advisory allowlist

### Changed
- `sqlx` 0.7.4 -> 0.8.6 (RUSTSEC-2024-0363)
- TypeScript configs now set `noEmit` to prevent compiled output in `src/`

### Fixed
- `cargo deny` config migrated to cargo-deny 0.20; `[licenses] version = 2`
  is required or the `allow` list is silently inert
- Duplicate ADR numbering: the SQLCipher deferral ADR is now 0006
- `docs/STANDARDS.md` section 10 stated the coverage gate as PR #4+; it is
  PR #6, matching section 1 and `docs/QUALITY.md`
