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
