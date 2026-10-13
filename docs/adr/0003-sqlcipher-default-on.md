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
