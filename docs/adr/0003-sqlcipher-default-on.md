# ADR 0003 — SQLCipher is on by default

Status: accepted
Date: 2026-10-13

## Context
The database contains project intelligence: code, decisions, constraints.
A stolen laptop should not expose it.

## Decision
Enable SQLCipher by default. Users can disable it in settings.

## Consequences
- Slightly slower large queries; acceptable for target corpus sizes.
- Must verify FTS5 and sqlite-vec compatibility early in week 1.

## Implementation Notes
SQLCipher integration requires building SQLx against SQLCipher libraries, which is more complex than a simple feature flag. The current implementation:
- Has the `use_encryption` config flag in place (default: true)
- Logs a debug message when encryption is requested but not yet implemented
- TODO: Build SQLx against SQLCipher libraries and implement key derivation (ADR 0005)
- For now, uses standard SQLite with the encryption flag as a placeholder

## Addendum — 2026-10-15

Implementation deferred to PR #2b per ADR 0006. A startup warning fires
when `use_encryption = true` is set before that PR lands.
