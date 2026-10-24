# Security Policy

## Reporting a vulnerability

Email labattman2k@gmail.com  with a description, the affected
version or commit, and reproduction steps if you have them.

> **This address is a placeholder.** It must be replaced with a monitored
> mailbox before any public release. Until then, treat this document as a
> statement of intent rather than a working disclosure channel.

Please do not open a public GitHub issue for a vulnerability.

## What to expect

We aim to acknowledge within 5 working days, and to publish a fix or a
mitigation plan within 30 days for anything rated high or critical.

This is a pre-release project maintained by one person. Those are targets,
not commitments, and we would rather give you an honest slack deadline than
a missed firm one.

## PGP

No key has been generated yet. The block below is a placeholder and **must
not** be used to encrypt anything.

```
-----BEGIN PGP PUBLIC KEY BLOCK-----
PLACEHOLDER - no key generated. Do not encrypt to this.
-----END PGP PUBLIC KEY BLOCK-----
```

`docs/STANDARDS.md` section 5 requires a published PGP key. That requirement
is currently unmet; this is a known gap, not an oversight.

## Scope

In scope:

- the core service (`crates/core`), including its local HTTP surface
- the CLI (`crates/cli`)
- the desktop shell (`crates/tauri`)
- the browser extension and the Atlas UI

Out of scope:

- attacks that require the attacker to already have code execution on the
  user's machine
- denial of service against a user's own local instance
- the contents of a user's own database

## Threat model

See [`docs/THREAT_MODEL.md`](docs/THREAT_MODEL.md) for the current data flows,
trust boundaries, controls, and known risks. The disclosure address and PGP
key above remain placeholders and must be made operational before public beta.

## Bug bounty

There is no bug bounty at this time. Reporters who want credit will get it.

## Privacy posture

Kinexfluence is local-first. `docs/STANDARDS.md` section 5 is the contract:

- no network calls outside 127.0.0.1 unless sync is explicitly enabled
- the server binds loopback only, with an origin check
- no remote telemetry of any kind
- no PII in logs at INFO or above
