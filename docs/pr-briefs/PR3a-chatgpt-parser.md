# PR #3a — ChatGPT export parser (library only)

Status: ready to hand to an agent
Base: `main` @ `0374857` (green on all four CI jobs)
Supersedes: the PR #3a brief drafted before PRs #1 and #2 merged

This brief is the contract for PR #3a. PR #3b (normalizer, snippet
extraction, DB writes, CLI wiring, two migrations) is specced separately,
because its design depends on what this parser actually produces.

---

## Corrections applied since the first draft

Four things in the original brief were stale or unstated. Each is fixed
below, and the agent prompt reflects the corrected version.

1. **Test count.** The draft said "8 new tests on top of the 11 from #2a".
   The suite is now **15** (four `shellexpand` tests landed in PR #1), so
   PR #3a takes it to **23**.
2. **ADR number.** The draft had PR #3a create
   `docs/adr/0007-import-scope-export-only.md`. That ADR already exists in
   `main` — it landed in PR #1. PR #3a must not create it. PR #3a's own ADR
   is **0008**.
3. **`crates/cli` was not listed as forbidden.** `crates/cli/src/main.rs:44`
   carries a TODO awaiting an issue number (AGENTS.md rule 6). Adding
   `crates/cli` to the must-not-touch list keeps that change and the parser
   work from colliding.
4. **The canonical capture shape was undefined.** The draft said "map
   `content_type` to canonical content blocks" without saying what
   canonical is, and `RawCapture` currently carries untyped
   `serde_json::Value`. PR #3b cannot be written against an undefined
   shape, so it is pinned in §4.

---

## 1. Scope

**In:** a ChatGPT export-file parser as a pure library in `crates/adapters`.
No database writes. No CLI wiring. No normalizer. No network.

**Deliverable:** `cargo test -p kfx-adapters` passes, driven by a sanitized
fixture, and the parser returns the pinned `RawCapture` shape for it.

**Out of scope, do not attempt:**
- Live DOM capture (deferred by ADR 0007)
- Snippet extraction, scoring, embedding
- Any `schema/migrations` change
- Writing to SQLite
- Fetching attachments

---

## 2. Starting state (verified, not assumed)

- `ProviderAdapter` in `crates/adapters/src/provider.rs` has **no
  implementors**. Changing its signature is therefore safe and breaks
  nothing.
- `kfx-adapters` already depends on `thiserror`, `serde`, `serde_json`,
  `chrono`, `uuid`, and `anyhow`. **No new dependency is required** for
  `AdapterError`.
- `fixtures/` does not exist. PR #3a creates it.
- `scripts/sanitize-fixture.sh` is named by `docs/STANDARDS.md` §1 and does
  **not exist**. §1 claims it does. PR #3a creates it, because PR #3a is
  the first PR to introduce fixtures and §1 makes sanitization a
  requirement for them.
- There is no pre-commit hook and no fixture gate in CI, though §10 lists
  fixture sanitization as both. Wiring those is **not** in PR #3a; note it
  as a follow-up.

---

## 3. File boundaries

**May touch:**

```
crates/adapters/src/lib.rs
crates/adapters/src/provider.rs
crates/adapters/src/chatgpt/mod.rs              (new)
crates/adapters/src/chatgpt/parser.rs           (new)
crates/adapters/src/chatgpt/types.rs            (new)
crates/adapters/tests/chatgpt_parser.rs         (new)
crates/adapters/Cargo.toml                      (only if a dep is truly needed)
fixtures/chatgpt/sample-export.json             (new, sanitized)
fixtures/chatgpt/sample-export.expected.json    (new, golden)
scripts/sanitize-fixture.sh                     (new)
docs/adr/0008-canonical-capture-shape.md        (new)
CHANGELOG.md                                    (Unreleased entry only)
```

**Must not touch:**

```
crates/core/**          (read it for types if useful, do not edit)
crates/cli/**           TODO-issue change is pending there
crates/tauri/**
ui/**
extension/**
schema/migrations/**    PR #3b owns migrations
docs/STANDARDS.md
docs/QUALITY.md
docs/adr/0001-0007      accepted and immutable per STANDARDS §2
AGENTS.md
.github/workflows/**
```

If a change outside the allowlist seems necessary, **stop and report**
rather than making it.

---

## 4. The canonical capture shape (pinned)

This is the contract PR #3b is written against. `RawCapture` keeps its
current field names so `crates/adapters/src/lib.rs` needs no re-export
change, but the inner shape is now specified rather than free-form.

```jsonc
{
  "source": "chatgpt",
  "conversation": {
    "external_id": "<conversation id>",
    "title": "…" | null,
    "create_time": 1699999999.0 | null,   // unix epoch seconds, verbatim
    "update_time": 1699999999.0 | null,
    "model": "gpt-4" | null,
    "url": null
  },
  "messages": [
    {
      "external_id": "<mapping node id>",
      "parent_external_id": "<node id>" | null,
      "role": "system" | "user" | "assistant" | "tool",
      "turn_index": 0,
      "create_time": 1699999999.0 | null,  // verbatim, never coerced
      "blocks": [
        { "type": "text", "text": "…" },
        { "type": "code", "language": "python" | null, "text": "…" },
        { "type": "execution_output", "text": "…" }
      ],
      "metadata_json": { "unknown_blocks": [ /* raw blocks, verbatim */ ] } | null
    }
  ],
  "attachments": [
    {
      "external_id": "…",
      "filename": "…" | null,
      "mime": "…" | null,
      "status": "missing"
    }
  ]
}
```

Rules the tests will hold you to:

- **Timestamps stay as unix epoch numbers, exactly as they appear in the
  export.** Do not convert to `DateTime`, do not round, do not drop a
  `null`. Typed conversion belongs to PR #3b, which has to satisfy
  `message.created_at DATETIME`.
- **`turn_index` is the index in the reconstructed chronological order**,
  starting at 0, counting only messages on the active path.
- **`blocks` is never empty for an imported message** unless the message
  genuinely had no extractable content; if a block type is unknown it goes
  to `metadata_json.unknown_blocks`, not to `blocks`.
- **`attachments[].status` is always `"missing"`** in this PR. Attachments
  are references only; nothing is fetched (ADR 0007).
- Unknown keys must not be invented. If a case isn't covered here, it is an
  open question — report it, do not improvise.

---

## 5. Trait changes

```rust
#[derive(Debug, thiserror::Error)]
pub enum AdapterError {
    #[error("input is not a recognised export format")]
    InvalidFormat,
    #[error("unsupported export version: {0}")]
    UnsupportedVersion(String),
    #[error("malformed message at index {index}: {reason}")]
    MalformedMessage { index: usize, reason: String },
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn matches_url(&self, url: &str) -> bool;
    fn parse_export(&self, bytes: &[u8]) -> Result<RawCapture, AdapterError>;
    fn supported_export_versions(&self) -> &[&'static str];
}
```

`parse_export` changes from `anyhow::Result` to `Result<_, AdapterError>`.
This is safe because there are no implementors today. `anyhow` stays in
`Cargo.toml` for now even if unused by this crate, to keep the diff small.

---

## 6. Parser requirements

1. Detect the ChatGPT export shape: a JSON **array** of conversation
   objects, each with `title`, `create_time`, `mapping` (a map of node id
   to node), and `current_node`.
2. Reconstruct the linear sequence by walking **from `current_node` back
   through `parent`** to the root, then reversing. Siblings off the active
   path are ignored in PR #3a — record this as a known limitation in
   ADR 0008.
3. Per node extract `author.role`, `content.content_type`,
   `content.parts`, `create_time`.
4. Content type mapping:

   | `content_type` | produces |
   |---|---|
   | `text` | one `text` block |
   | `code` | one `code` block; `language` from the block if present |
   | `execution_output` | one `execution_output` block |
   | `multimodal_text` | join string parts into one `text` block |
   | anything else | raw block into `metadata_json.unknown_blocks`, `tracing::warn!`, **do not fail** |

5. `parts` may contain non-string elements (arrays, objects, null). Preserve
   those raw in `unknown_blocks`; extract what is stringifiable. Never
   panic.
6. A node whose `message` is `null` (some exports have structural
   placeholders) is skipped, not an error.
7. `matches_url` returns true for `chatgpt.com` and `chat.openai.com`.
8. `supported_export_versions` returns the versions this parser actually
   accepts. It is fine to return a single provisional entry, but it must be
   a real claim you can defend.

---

## 7. Fixture requirements

**Decision: PR #3a uses a synthetic fixture.** No real export is available,
and the parser and its tests should not wait on one. The fixture is
hand-written to match the documented ChatGPT export shape and **must be
labelled synthetic** — in a comment at the top of the fixture file and in
`crates/adapters/tests/chatgpt_parser.rs`.

This is a real limitation, not a formality. A synthetic fixture tests the
parser against **our assumptions** about the export format rather than the
format as it actually is, and the things most likely to be wrong are
exactly the ones this parser exists to handle:

- the precise `content_type` strings in current exports
- the shape of `parts` for structured content
- whether a `mapping` node really can carry a null `message`

**Follow-up, required before PR #3b:** run the parser against a real
sanitized export and regenerate the golden file. Any content type the
synthetic fixture did not anticipate should surface there. PR #3b is where
a wrong assumption becomes expensive, because that is where the parsed
shape reaches the database.

The agent must not present a synthetic fixture as a real capture.

`fixtures/chatgpt/sample-export.json` must contain:

- a conversation with a code block
- a message with a system role
- a branch point (a node with two children) with `current_node` on one side
- an unknown content type
- a message with `create_time: null`
- a node whose `message` is `null`

Keep it under 50 KB. Redact via `scripts/sanitize-fixture.sh`:
emails → `user@example.com`, names → `User`/`Assistant`, URLs →
`https://example.com/...`, any API-key-shaped string → `REDACTED`.

`fixtures/chatgpt/sample-export.expected.json` is the golden output. It is
generated from the parser and then **hand-verified** against §4. A golden
file that merely records whatever the parser emitted is not evidence.

---

## 8. Test list — `crates/adapters/tests/chatgpt_parser.rs`

1. `parses_minimal_conversation` — one user, one assistant; order and
   `turn_index` correct.
2. `parses_code_block_as_content_block` — a code part yields a `code` block
   with `language` and `text`.
3. `walks_branch_to_current_node` — only the active path is returned, and
   the ignored sibling is absent.
4. `unknown_content_type_preserved_in_metadata` — unknown block lands in
   `metadata_json.unknown_blocks` and the import still succeeds.
5. `null_create_time_handled` — `create_time` stays `null`, no panic.
6. `null_message_node_skipped` — a structural placeholder node is skipped
   without error.
7. `malformed_json_returns_invalid_format` — bad input yields
   `AdapterError::InvalidFormat`.
8. `missing_mapping_returns_malformed_message` — a conversation with no
   `mapping` errors cleanly with the offending index.
9. `idempotent_parse` — parsing the same bytes twice yields identical
   output.
10. `golden_fixture_matches` — `parse_export(sample-export.json)` deep-equals
    `sample-export.expected.json`.

Suite goes from 15 tests to 25.

---

## 9. Acceptance criteria

- `cargo fmt --all -- --check` passes
- `cargo clippy --all-targets -- -D warnings` passes
- `cargo test --workspace` passes — 25 tests
- `cargo deny check` passes
- no file touched outside §3
- diff under 800 lines excluding fixtures and lockfiles
- no `unwrap`/`expect` outside tests (STANDARDS §7)
- public items have doc comments

---

## 10. Commit sequence

Five commits. ADR and standards-adjacent files get their own docs-only
commits, per AGENTS.md rule 5 — never bundled with code.

1. `feat(adapters): add typed AdapterError and extend ProviderAdapter`
2. `feat(adapters): add ChatGPT export parser`
3. `test(adapters): add fixture-driven ChatGPT parser tests`
4. `docs(adr): pin the canonical capture shape` (ADR 0008 + CHANGELOG, docs only)
5. `chore(scripts): add fixture sanitizer`

---

## 11. Forward note for PR #3b — the migration trap

Not PR #3a's work, but it must not be lost, because it is the single
likeliest way PR #3b burns a round.

**Q5 — `attachment.status` cannot be done with `ALTER TABLE`.** SQLite
supports `ALTER TABLE` only for `RENAME`, `ADD COLUMN`, `DROP COLUMN`, and
`RENAME COLUMN`. Relaxing `hash TEXT NOT NULL` and `blob_path TEXT NOT NULL`
to nullable, and adding the `CHECK` constraint from the Q5 proposal,
requires the full table-rebuild procedure: create the replacement, copy
rows, drop the original, rename, then **recreate `idx_attachment_owner` and
`idx_attachment_hash`**.

**The compounding trap:** `crates/core/src/storage.rs` sets
`PRAGMA foreign_keys=ON`, and SQLite treats `PRAGMA foreign_keys` as a
**no-op inside a transaction**. sqlx wraps each migration in a transaction
by default. So a rebuild that needs FK checks off will silently keep them
on unless the migration opts out — sqlx's mechanism for that is a
`-- no-transaction` directive at the top of the migration file, which must
then be verified against sqlx 0.8.6 rather than assumed.

Also unresolved: the `attachment` table keys on generic
`owner_type`/`owner_id`, with no `message_id`. PR #3b must decide how an
attachment binds to a message before writing that migration.

**Q6 — `conversation.content_hash` is the easy one.** Plain
`ALTER TABLE conversation ADD COLUMN content_hash TEXT;` plus
`CREATE INDEX idx_conversation_hash ON conversation(content_hash);`. No
rebuild. Hash over normalized content, not raw bytes (ADR 0007 archives the
raw file separately, so re-exports differing only in metadata still
collide).

---

## 12. Open items the agent must not silently resolve

- Confirm the fixture is labelled synthetic (§7). It is not a real capture,
  and the golden file records our assumptions, not verified reality.
- Any `content_type` encountered but not in the §6 table — list them.
- Any test skipped, and why.
- If the diff exceeds 800 lines excluding fixtures and lockfiles: **stop
  and report**, do not push.

---

## Agent prompt — paste as the session opener

```
## Session task: PR #3a — ChatGPT export parser

Repo: C:\Users\Admin\Downloads\Kip\kinexfluence

Before starting:
1. Read AGENTS.md, docs/STANDARDS.md, docs/QUALITY.md, and
   docs/pr-briefs/PR3a-chatgpt-parser.md (this brief).
2. Read docs/adr/0007-import-scope-export-only.md.
3. git log --oneline -15
4. Confirm a clean tree: git status
5. git switch main && git pull
6. git switch -c feat/chatgpt-parser

Scope: a ChatGPT export-file parser as a pure library in crates/adapters.
No database writes. No CLI wiring. No normalizer. No migrations. No
network. The deliverable is `cargo test -p kfx-adapters` passing with the
ten tests in the brief's section 8, driven by the fixtures.

The canonical output shape is PINNED in the brief's section 4. Follow it
exactly; do not invent fields or rename keys.

Files you may touch (nothing else):
  crates/adapters/src/lib.rs
  crates/adapters/src/provider.rs
  crates/adapters/src/chatgpt/mod.rs
  crates/adapters/src/chatgpt/parser.rs
  crates/adapters/src/chatgpt/types.rs
  crates/adapters/tests/chatgpt_parser.rs
  crates/adapters/Cargo.toml              (only if a dep is truly needed)
  fixtures/chatgpt/sample-export.json
  fixtures/chatgpt/sample-export.expected.json
  scripts/sanitize-fixture.sh
  docs/adr/0008-canonical-capture-shape.md
  CHANGELOG.md                            (Unreleased entry only)

Files you must NOT touch:
  crates/core/**, crates/cli/**, crates/tauri/**, ui/**, extension/**,
  schema/migrations/**, docs/STANDARDS.md, docs/QUALITY.md,
  docs/adr/0001-0007 (accepted, immutable), AGENTS.md,
  .github/workflows/**

  docs/adr/0007-import-scope-export-only.md already exists. Do not create
  it. Your ADR is 0008.

House rules (AGENTS.md):
  - Never merge. Propose.
  - Never disable a check to make a build pass.
  - Do not use git reset or git checkout to rewrite committed history.
    Fix forward, or ask.
  - Protected files change only in dedicated docs-only commits.
  - Every TODO comment must link an issue: // TODO(#3): ...

Implementation steps:

1. provider.rs: add AdapterError (thiserror, already a dependency) with
   InvalidFormat, UnsupportedVersion(String), MalformedMessage { index,
   reason }, Io(std::io::Error). Change parse_export to return
   Result<RawCapture, AdapterError> and add supported_export_versions().
   There are no existing implementors, so this breaks nothing.

2. chatgpt/types.rs: serde structs mirroring the export shape. Note the
   export nests content as content.content_type and content.parts, and
   mapping is a map keyed by node id.

3. chatgpt/parser.rs: walk from current_node back through parent to the
   root, reverse for chronological order, assign turn_index from 0. Map
   content types per the brief's table. Unknown types go to
   metadata_json.unknown_blocks with a tracing::warn!, never an error.
   Skip nodes whose message is null. Keep timestamps as unix epoch
   numbers, verbatim, including null.

4. Fixture: hand-write a SYNTHETIC fixture matching the documented ChatGPT
   export shape. Label it as synthetic in a comment at the top of the
   fixture file and in crates/adapters/tests/chatgpt_parser.rs. No real
   export is available; do not claim otherwise. It must cover: a code
   block, a system role, a branch point, an unknown content type, a null
   create_time, and a null message node.
   Generate the golden file from the parser, then HAND-VERIFY it against
   the brief's section 4 before committing it.

5. Ten tests, named exactly as in the brief's section 8.

6. ADR 0008: record the canonical capture shape, the active-path-only
   limitation, and that attachments are references only.

7. CHANGELOG.md under [Unreleased] -> Added.

Commit sequence (five commits):
  1. feat(adapters): add typed AdapterError and extend ProviderAdapter
  2. feat(adapters): add ChatGPT export parser
  3. test(adapters): add fixture-driven ChatGPT parser tests
  4. docs(adr): pin the canonical capture shape
  5. chore(scripts): add fixture sanitizer

Verify before each commit:
  cargo fmt --all
  cargo clippy --all-targets -- -D warnings
  cargo test -p kfx-adapters

Verify before reporting:
  cargo test --workspace          # must be 25 tests
  cargo deny check
  git diff main..HEAD --stat      # under 800 lines excluding fixtures and
                                  # lockfiles

Do NOT push. Do NOT merge. Report:
  - commit hashes and one-line messages
  - output summary of `cargo test --workspace`
  - total diff line count excluding fixtures and lockfiles
  - confirmation that the synthetic fixture is labelled as such
  - any content_type you met but did not handle
  - any test you skipped and why
  - anything in this brief that turned out to be wrong about the code

If the diff exceeds 800 lines excluding fixtures and lockfiles, stop and
report. Do not push.
```
