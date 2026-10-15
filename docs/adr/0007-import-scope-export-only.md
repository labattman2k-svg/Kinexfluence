# ADR 0007 — Import scope: export-file import only

Status: accepted
Date: 2026-10-15
Related: ADR 0003, ADR 0006

## Context

Users accumulate project-relevant fragments in ChatGPT. There are two ways to
capture them: read the live DOM with a browser extension, or import the
provider's own export file.

Live DOM capture requires the extension to exist before any import can be
tested, and brings mutation observers, session auth, CSP, cross-origin and
anti-automation concerns. Export-file import needs none of that, is
deterministic, and is testable from fixtures.

## Decision

Import supports **provider export files only**. Live DOM capture is deferred.

- The raw export file is archived under
  `~/.kinexfluence/imports/<source>/<timestamp>/` and its path is recorded in
  `conversation.raw_path`.
- Per-message raw JSON is retained in `message.raw_json`, so a message can be
  audited or reconstructed without re-parsing the whole archive.
- Import is **project-blind**. Conversations land in a global pool;
  `conversation` has no `project_id`. Assignment to a project happens later,
  through the cart.
- Unknown content blocks are preserved in `metadata_json` and logged at WARN.
  An unknown block never fails an import.
- Attachments are recorded as references only and are **never** fetched from
  the provider CDN.

## Consequences

- Import cannot observe conversations the user never exported.
- Nothing in this phase depends on the browser extension existing.
- Only the active branch of a conversation tree is imported; sibling branches
  off the active path are ignored for now. This is a known limitation.
- Attachment rows cannot be fully populated until the `attachment.status`
  migration lands, which is scheduled for the same PR as the importer.
- Drag-and-drop import in the Atlas UI is deferred to a later PR.

## Addendum — 2026-10-15

**Canonical capture shape.** PR #3a pins the shape a parsed conversation
takes, so PR #3b can be written against it. It is recorded as an addendum
rather than a new ADR because it is a contract internal to one PR, per
`docs/STANDARDS.md` §2 as clarified on 2026-10-15. If it turns out to be
load-bearing beyond PR #3b, it should be promoted to its own ADR.

Each captured message carries `external_id` (the mapping node id),
`parent_external_id`, `role`, `turn_index`, `create_time`, `blocks`, and
`metadata_json`.

`turn_index` is the position in reconstructed chronological order over
emitted messages, starting at 0. A node whose `message` is `null` is a
structural placeholder and is skipped, so the path's placeholders do not
consume an index.

`blocks` is a list of typed blocks: `text`, `code` (with an optional
`language`), and `execution_output`. A content type the parser does not
recognise produces no block; the raw block is preserved verbatim in
`metadata_json.unknown_blocks` and a WARN is logged. An unrecognised block
never fails an import.

Timestamps remain unix epoch numbers exactly as exported, including null.
Converting them to a SQLite `DATETIME` is PR #3b's responsibility, because
that is where the database's expectations apply.

`attachments[].status` is always `"missing"`: attachment references are
recorded, never fetched.

**Limitations accepted in PR #3a, both open.** Only the active path is
captured; siblings off `current_node` are ignored, which section "Consequences"
already records. Separately, only the first conversation in an export is
captured, because `RawCapture` carries exactly one conversation while a real
export holds many. That second limitation has no issue yet and must be
resolved before PR #3b writes imports to the database, since silently
importing one conversation out of hundreds is data loss, not a deferral.

**Fixture status.** The parser's tests run against a synthetic fixture, not a
real capture, so they hold the parser to our assumptions about the format
rather than to the format itself. Validating against a real sanitized export
is issue #6 and is required before PR #3b.

**Raw message JSON is not yet retained.** Section "Decision" says
per-message raw JSON is kept in `message.raw_json`. The canonical shape above
has no field carrying it, so PR #3b cannot populate that column from the
parser's output as things stand. Either the shape gains a raw field or the
decision is amended; that choice belongs to PR #3b and is flagged here rather
than quietly dropped.
