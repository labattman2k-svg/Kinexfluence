# ADR 0008 — The licence is proprietary

Status: accepted
Date: 2026-10-22
Related: ADR 0007

## Context

The workspace has declared `AGPL-3.0-or-later` in its package manifests
since the first commit, on 2026-10-13, and the repository has been public
since that commit. Every copy of the code that existed before this ADR
therefore carries a copyleft grant that cannot be withdrawn from the people
who hold it.

The maintainer has decided that the project should not be distributed under
a copyleft licence, and that it should carry a proprietary licence instead.

Two mechanical consequences follow from the existing configuration. The
`deny.toml` allow list carried `AGPL-3.0-or-later` only because the
workspace's own crates are path dependencies of each other, so cargo-deny
checked them like any other crate; no third-party dependency in the graph
was AGPL. And the `license` field had to change, because a proprietary
licence has no SPDX identifier, so it needs the `LicenseRef-` form.

## Decision

The project is licensed under a proprietary licence. `LICENSE` grants a
non-commercial evaluation licence: the Licensee may install and run the
Software and use its Output, and may not redistribute, sublicense, sell, or
use it commercially. Nothing in the licence is a grant to anyone else.

Mechanically:

- `license = "LicenseRef-Proprietary"` in `[workspace.package]`, inherited
  by all four crates. The identifier is not on the SPDX list, which is what
  a proprietary licence requires, and which means crates.io would refuse
  the crates anyway.
- `publish = false` in every crate manifest, so the source can never reach
  a public registry. The setting is repeated per crate rather than
  inherited from `[workspace.package]`, because `publish` only became an
  inheritable workspace key in a Cargo release newer than the workspace's
  declared `rust-version`, and an inherited key that old a toolchain cannot
  parse is a broken build rather than a safe default.
- `AGPL-3.0-or-later` removed from the `deny.toml` allow list, and
  `LicenseRef-Proprietary` added in its place. The allow list is tighter
  after this change, not looser.

## Consequences

- The project is not open source, and says so in a machine-readable way.
- The `publish = false` guard is the last line of defence against
  publishing proprietary source by accident.
- **The relicensing is not retroactive.** Section 6 of `LICENSE` says so
  in the operative terms, and that section is the reason it exists. Anyone
  who obtained a copy of a pre-2026-10-22 version keeps their rights under
  the AGPL the manifests declared, including the right to redistribute it,
  and the corresponding source remains in the public history of the
  repository. The proprietary licence governs this version and every
  version after it.
- **A proprietary licence is not a private one.** The repository is public
  and the history is public. The licence governs what a reader may do with
  the code; it does not make the code invisible. If the source should not
  be public, it has to move to a private repository, and that is a separate
  decision from this one.
- The `SECURITY.md` disclosure address and PGP key remain placeholders,
  and `docs/THREAT_MODEL.md` still does not exist. A licence that says
  "report problems here" is only worth what the reporting address is worth.

## Addendum

Section 12 of `LICENSE` refers to "the Licensor's place of establishment"
and asks for a contact address for legal notices. Neither has been fixed.
They have to be supplied before the software is distributed to anyone
outside the maintainer's own machine. This is a known gap in the document,
not a deliberate choice, and it is recorded here so that it is not
mistaken for one.

2026-10-24: Added `docs/THREAT_MODEL.md` to document the current security
boundaries and unresolved risks; the legal jurisdiction, notice address,
monitored disclosure contact, and usable PGP key remain outstanding.
