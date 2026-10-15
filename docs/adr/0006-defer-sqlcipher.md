# ADR 0006 — Defer SQLCipher implementation to PR #2b

Status: accepted
Date: 2026-10-15
Related: ADR 0003, ADR 0004

## Context

ADR 0003 accepted SQLCipher-on-by-default. Implementing it requires
adding `libsqlite3-sys` with the `bundled-sqlcipher-vendored-openssl`
feature, threading the key through SQLx (which does not expose SQLCipher
cleanly), integrating OS keychain access for key derivation, and testing
open/write/close/reopen cycles. This is a substantial change that does
not belong in the standards-and-tests PR.

## Decision

SQLCipher implementation lands in PR #2b, its own PR, after PR #2a
(standards + tests + CI) merges. Until PR #2b lands, the storage layer
logs a warning at startup when `use_encryption = true`:

    WARN kfx.storage: use_encryption=true is set but SQLCipher is not
    yet wired; data is stored in plaintext. See ADR 0006.

## Consequences

- PR #2a stays reviewable.
- Users who read logs are told plainly that encryption is not active.
- The privacy whitepaper must state this until PR #2b lands.
- ADR 0003 is not superseded; it is deferred. It becomes active again
  when PR #2b merges.
