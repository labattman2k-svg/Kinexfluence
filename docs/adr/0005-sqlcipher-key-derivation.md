# ADR 0005 — SQLCipher key derivation strategy

Status: accepted
Date: 2026-10-13

## Context
SQLCipher requires an encryption key to secure the database. We need a strategy
that:
- Works offline without user interaction
- Allows the user to access their data if they move machines
- Is reasonably secure against casual threats
- Can be enhanced in the future

## Decision
For the MVP, derive the key from machine-specific identifiers (hostname + OS)
combined with an application salt. Store the derived key in
`~/.kinexfluence/key` with 0600 permissions. This allows:
- Automatic unlocking on the same machine
- Portability by copying the key file
- Future enhancement to support user-provided passphrases

The key derivation uses SHA-256(hostname + OS + "kinexfluence-v1-salt").

## Consequences
- Data is tied to the machine unless the user copies the key file
- The key file itself must be protected by OS-level permissions
- Users can opt out of encryption entirely
- Future versions can add user-provided passphrases via PBKDF2/Argon2

## Implementation Notes
Key derivation is deferred until SQLCipher is properly integrated. The current
implementation:
- Has the `use_encryption` config flag in place
- Logs a debug message when encryption is requested
- TODO: Implement key derivation once SQLx is built against SQLCipher
