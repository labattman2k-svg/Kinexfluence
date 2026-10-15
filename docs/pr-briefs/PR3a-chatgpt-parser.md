# PR #3a — ChatGPT export parser (library only)

Status: ready to hand to an agent
Base: `main` @ `0374857` (all four CI jobs green)
Supersedes: all earlier drafts of this brief

PR #3b (normalizer, snippet extraction, DB writes, CLI wiring, two
migrations) is specced separately, because its design depends on what this
parser actually produces. See §11 for the migration note it must inherit.

---

## Corrections applied to this revision

| Item | Was | Now |
|---|---|---|
| Test baseline | "11 from #2a" | **15** (four `shellexpand` tests landed in PR #1) |
| ADR 0007 | "create it" | **Already exists.** PR #3a does not create it; addendum only if needed |
| New ADR | ADR 0008 proposed | **No new ADR.** Canonical shape recorded as an ADR 0007 addendum |
| `crates/cli` | not listed | **Forbidden** — the rule-6 TODO fix is a separate branch |
| `scripts/sanitize-fixture.sh` | not mentioned | **Create it** |
| `SECURITY.md` | not mentioned | **Create stub** |
| `crates/tauri/src/main.rs` | not in scope | **Add** — fix the `.expect()` finding from PR #1 |
| Fixture source | real export | **Synthetic** (decision, see §7) |

---

## 1. Scope

**In:** a ChatGPT export-file parser as a pure library in `crates/adapters`,
plus three small housekeeping deliverables in §3.

**Deliverable:** `cargo test -p kfx-adapters` passes, driven by a fixture,
and the parser returns the pinned `RawCapture` shape.

**Out of scope, do not attempt:**
- Live DOM capture (deferred by ADR 0007)
- Snippet extraction, scoring, embedding
- Any `schema/migrations` change — PR #3b owns migrations
- Writing to SQLite
- Fetching attachments
- Any CLI wiring

---

## 2. Starting state (verified, not assumed)

- `ProviderAdapter` in `crates/adapters/src/provider.rs` has **no
  implementors**. Changing its signature breaks nothing.
- `kfx-adapters` already depends on `thiserror`, `serde`, `serde_json`,
  `chrono`, `uuid`, `anyhow`. **No new dependency is needed.**
- `fixtures/` does not exist. PR #3a creates it.
- `scripts/sanitize-fixture.sh` is named by `docs/STANDARDS.md` §1 and does
  **not** exist; §1 claims it does. PR #3a creates it.
- `SECURITY.md` is named by §5 and does not exist.
- `crates/tauri/src/main.rs:25` calls `.expect(...)`, which §7 forbids
  outside tests.
- There is no pre-commit hook and no fixture gate in CI, though §10 lists
  both. Wiring those is **not** in PR #3a — note it as a follow-up.

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
crates/tauri/src/main.rs                        (the .expect() fix ONLY)
fixtures/chatgpt/sample-export.json             (new, synthetic)
fixtures/chatgpt/sample-export.expected.json    (new, golden)
scripts/sanitize-fixture.sh                     (new, executable)
SECURITY.md                                     (new, stub)
docs/adr/0007-import-scope-export-only.md       (Addendum section only)
CHANGELOG.md                                    (Unreleased entries only)
```

**Must not touch:**

```
crates/core/**
crates/cli/**          the TODO(#3) fix is a separate branch
ui/**
extension/**
schema/migrations/**   PR #3b owns migrations
docs/STANDARDS.md
docs/QUALITY.md
AGENTS.md
.github/workflows/**
docs/pr-briefs/**      this brief is input, not output
```

If a change outside the allowlist seems necessary, **stop and report**
rather than making it.

---

## 4. The canonical capture shape (pinned)

This is the contract PR #3b is written against. `RawCapture` keeps its
current field names so `crates/adapters/src/lib.rs` needs no re-export
change, but the inner shape is specified rather than free-form.

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
      "metadata_json": { "unknown_blocks": [ /* raw, verbatim */ ] } | null
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

Rules the tests hold you to:

- **Timestamps stay unix epoch numbers, exactly as in the export.** Do not
  convert to `DateTime`, do not round, do not drop a `null`. Typed
  conversion is PR #3b's job, which must satisfy `message.created_at
  DATETIME`.
- **`turn_index` is the index in reconstructed chronological order**,
  starting at 0, counting only messages on the active path.
- **Unknown block types go to `metadata_json.unknown_blocks`, never to
  `blocks`.**
- **`attachments[].status` is always `"missing"`** in this PR. References
  only; nothing is fetched (ADR 0007).
- Do not invent fields. If a case is not covered here, it is an open
  question — report it, do not improvise.

Record this shape in a `## Addendum — 2026-10-15` section at the bottom of
`docs/adr/0007-import-scope-export-only.md`, per `STANDARDS.md` §2, which
allows an accepted ADR to gain an appended addendum but nothing else.

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

`parse_export` moves from `anyhow::Result` to `Result<_, AdapterError>`.
Safe: no implementors exist today.

---

## 6. Parser requirements

1. Detect the ChatGPT export shape: a JSON **array** of conversations, each
   with `title`, `create_time`, `mapping` (map of node id → node), and
   `current_node`.
2. Reconstruct order by walking **from `current_node` back through
   `parent`** to the root, then reversing. Siblings off the active path are
   ignored — record this limitation in the ADR addendum.
3. Per node extract `author.role`, `content.content_type`,
   `content.parts`, `create_time`.
4. Content type mapping:

   | `content_type` | produces |
   |---|---|
   | `text` | one `text` block |
   | `code` | one `code` block; `language` from the block if present |
   | `execution_output` | one `execution_output` block |
   | `multimodal_text` | join string parts into one `text` block |
   | anything else | raw block to `metadata_json.unknown_blocks`, `tracing::warn!`, **do not fail** |

5. `parts` may contain non-string elements (arrays, objects, null).
   Preserve those raw; extract what is stringifiable. Never panic.
6. A node whose `message` is `null` is skipped, not an error.
7. `matches_url` returns true for `chatgpt.com` and `chat.openai.com`.
8. `supported_export_versions` returns versions this parser actually
   accepts. A single provisional entry is fine, but it must be a claim you
   can defend.

---

## 7. Fixture requirements — SYNTHETIC

**Decision: PR #3a uses a synthetic fixture.** No real export is available
and the parser should not wait on one. The fixture is hand-written to match
the documented ChatGPT export shape and **must be labelled synthetic** — in
a comment at the top of the fixture file and in
`crates/adapters/tests/chatgpt_parser.rs`.

This is a real limitation, not a formality. A synthetic fixture tests the
parser against **our assumptions** about the format, and the assumptions
most likely to be wrong are exactly the ones this parser exists to handle:

- the precise `content_type` strings in current exports
- the shape of `parts` for structured content
- whether a `mapping` node really can carry a null `message`

**Required follow-up before PR #3b:** run the parser against a real
sanitized export and regenerate the golden file. PR #3b is where a wrong
assumption becomes expensive, because that is where the parsed shape
reaches the database.

`scripts/sanitize-fixture.sh` is still built in this PR — it is the tool
that follow-up will use. The agent must not present the synthetic fixture
as a real capture.

`fixtures/chatgpt/sample-export.json` must contain:

- a conversation with a code block
- a message with a system role
- a branch point (a node with two children), `current_node` on one side
- an unknown content type
- a message with `create_time: null`
- a node whose `message` is `null`

Under 50 KB. Generate the golden file from the parser, then **hand-verify
it against §4** before committing. A golden file that merely records
whatever the parser emitted is not evidence.

---

## 8. Test list — `crates/adapters/tests/chatgpt_parser.rs`

1. `parses_minimal_conversation` — order and `turn_index` correct
2. `parses_code_block_as_content_block` — `code` block with `language` + `text`
3. `walks_branch_to_current_node` — only the active path; sibling absent
4. `unknown_content_type_preserved_in_metadata` — lands in
   `metadata_json.unknown_blocks`, import still succeeds
5. `null_create_time_handled` — stays `null`, no panic
6. `malformed_json_returns_invalid_format` — `AdapterError::InvalidFormat`
7. `missing_mapping_returns_malformed_message` — clean error with index
8. `idempotent_parse` — same bytes twice, identical output

Suite goes from 15 tests to **23**.

> **Reviewer note, unresolved:** `sample-export.expected.json` is a
> deliverable here, but no test in this list asserts against it. As
> written, the golden file is never compared to anything, so it can drift
> silently the moment the parser changes. Either add a ninth test
> (`golden_fixture_matches`, deep-equality against the golden file) —
> taking the suite to 24 — or drop the golden file as a deliverable. Flag
> this before the agent runs; do not leave a committed artifact that
> nothing verifies.

---

## 9. Acceptance criteria

- `cargo fmt --all -- --check` passes
- `cargo clippy --all-targets -- -D warnings` passes
- `cargo test --workspace` passes with **23 tests**
- `cargo deny check` passes
- `scripts/sanitize-fixture.sh` is executable and supports `--help`
- `SECURITY.md` exists with a disclosure address and a "no bug bounty" note
- no file touched outside §3
- no `unwrap`/`expect` outside tests (§7)
- public items have doc comments
- diff excluding fixtures and lockfiles under **800 lines**

---

## 10. Commit sequence (six commits)

Protected files change only in dedicated docs-only commits (AGENTS.md
rule 5), never bundled with code.

1. `feat(adapters): extend ProviderAdapter trait with export parsing`
2. `feat(adapters): add ChatGPT export parser`
3. `test(adapters): add fixture-driven ChatGPT parser tests`
4. `chore(scripts): add fixture sanitization helper`
5. `docs(security): add disclosure policy stub`
6. `fix(tauri): propagate run error instead of expect`

The ADR 0007 addendum lands as its own small `docs(adr):` commit, or folds
into commit 2. The `.expect()` fix is its own commit because it is
unrelated to the parser.

**The `.expect()` fix must preserve existing behaviour.** The current
`main()` spawns the core server on a tokio task and calls `app.manage()`
in setup. Only the final `.expect(...)` changes:

```rust
#[tokio::main]
async fn main() -> tauri::Result<()> {
    // ... config, tokio::spawn(server), Builder::default()
    //     .setup(|app| { app.manage(config); Ok(()) })
    //     .run(tauri::generate_context!())     // <- no .expect()
}
```

`tauri::Builder::run` already returns `tauri::Result<()>`, and
`tauri::Error` implements `Debug`, so it satisfies `Termination`. Do not
delete the server spawn or the setup closure.

---

## 11. Forward note for PR #3b — the migration pattern

Not PR #3a's work, but PR #3b's brief must lead with it.

`attachment.status` cannot be added with `ALTER TABLE`. SQLite supports
only `RENAME`, `ADD COLUMN`, `DROP COLUMN`, `RENAME COLUMN` — dropping
`NOT NULL` and adding a `CHECK` constraint both require the full table
rebuild. And `PRAGMA foreign_keys` is a **no-op inside a transaction**,
while sqlx wraps each migration in one by default, so the pragma must be
toggled outside it.

**Verified:** sqlx 0.8.6 supports the opt-out. In
`sqlx-core-0.8.6/src/migrate/source.rs:127`:

```rust
let no_tx = sql.starts_with("-- no-transaction");
```

Two consequences worth writing down:

1. The directive must be the **literal first bytes of the file**. A license
   header or any comment before it makes it silently unrecognized, the
   migration runs in a transaction, and the `PRAGMA foreign_keys` lines
   become silent no-ops.
2. With `-- no-transaction` there is no implicit transaction, so a failure
   partway through the rebuild leaves the database half-migrated — and if
   it fails after `DROP TABLE attachment`, **the table is gone**. The
   migration must manage its own explicit `BEGIN`/`COMMIT` to stay atomic.

```sql
-- no-transaction

PRAGMA foreign_keys = OFF;
BEGIN;

CREATE TABLE attachment_new (
  id TEXT PRIMARY KEY,
  owner_type TEXT NOT NULL,
  owner_id TEXT NOT NULL,
  filename TEXT,
  mime TEXT,
  size_bytes INTEGER,
  status TEXT NOT NULL DEFAULT 'present',
  hash TEXT,
  blob_path TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  CHECK (
    (status = 'present' AND hash IS NOT NULL AND blob_path IS NOT NULL)
    OR (status != 'present')
  )
);

INSERT INTO attachment_new
  (id, owner_type, owner_id, filename, mime, size_bytes, status, hash, blob_path, created_at)
SELECT
  id, owner_type, owner_id, filename, mime, size_bytes,
  CASE WHEN hash IS NOT NULL AND blob_path IS NOT NULL THEN 'present' ELSE 'missing' END,
  hash, blob_path, created_at
FROM attachment;

DROP TABLE attachment;
ALTER TABLE attachment_new RENAME TO attachment;

CREATE INDEX idx_attachment_owner ON attachment(owner_type, owner_id);
CREATE INDEX idx_attachment_hash ON attachment(hash);

PRAGMA foreign_key_check;
COMMIT;

PRAGMA foreign_keys = ON;
```

Notes for whoever writes it:
- `BEGIN`/`COMMIT` are the addition to the originally proposed SQL. Without
  them the rebuild is not atomic.
- `PRAGMA foreign_key_check` before `COMMIT` is part of SQLite's documented
  12-step procedure; it aborts the migration if the rebuilt data violates a
  constraint.
- The FK toggling is currently **precautionary**: nothing in
  `schema/migrations/0001_init.sql` references `attachment` by foreign key.
  It matters the moment anything does.
- `ALTER TABLE ... RENAME` in SQLite 3.25+ also rewrites references to the
  renamed table in other objects. Nothing references `attachment` today, so
  this is safe now — but re-check at PR #3b time.
- Open question PR #3b must answer first: `attachment` keys on generic
  `owner_type`/`owner_id` and has **no `message_id`**. How an attachment
  binds to a message is undecided.

**Q6 — `conversation.content_hash` is the easy one.** Plain
`ALTER TABLE conversation ADD COLUMN content_hash TEXT;` plus
`CREATE INDEX idx_conversation_hash ON conversation(content_hash);`. No
rebuild, no `-- no-transaction`. Hash over normalized content, not raw
bytes (ADR 0007 archives the raw file separately, so re-exports differing
only in metadata still collide).

---

## 12. Open items the agent must not silently resolve

- Confirm the fixture is labelled synthetic (§7). It is not a real capture.
- Any content type met but not in the §6 table — list them.
- Any test skipped, and why.
- Anything in this brief that turns out to be wrong about the code.
- If the diff exceeds 800 lines excluding fixtures and lockfiles: **stop
  and report**, do not push.

---

## Agent prompt — paste as the session opener

```
## Session task: PR #3a — ChatGPT export parser

Repo: C:\Users\Admin\downloads\kip\kinexfluence

Before starting:
1. Read AGENTS.md, docs/STANDARDS.md, docs/QUALITY.md, and
   docs/pr-briefs/PR3a-chatgpt-parser.md (this brief).
2. Read docs/adr/0007-import-scope-export-only.md.
3. git log --oneline -15
4. git status must be clean.
5. git switch main && git pull
6. git switch -c feat/chatgpt-parser

Current state:
- 15 tests pass on main. Your work adds 8, for 23 total.
- ADR 0007 already exists. Do NOT create it. Do NOT create a new ADR.
  Record the canonical shape as an "## Addendum — 2026-10-15" section at
  the bottom of ADR 0007 (STANDARDS section 2 permits addenda, nothing
  else).

Scope: a ChatGPT export-file parser as a pure library in crates/adapters.
No database writes. No normalizer. No CLI wiring. No migrations.

The canonical output shape is PINNED in the brief's section 4. Follow it
exactly. Do not invent fields or rename keys.

Files you may touch (nothing else):
  crates/adapters/src/lib.rs
  crates/adapters/src/provider.rs
  crates/adapters/src/chatgpt/mod.rs              (new)
  crates/adapters/src/chatgpt/parser.rs           (new)
  crates/adapters/src/chatgpt/types.rs            (new)
  crates/adapters/tests/chatgpt_parser.rs         (new)
  crates/adapters/Cargo.toml                      (only if truly needed)
  crates/tauri/src/main.rs                        (.expect() fix ONLY)
  fixtures/chatgpt/sample-export.json             (new, synthetic)
  fixtures/chatgpt/sample-export.expected.json    (new, golden)
  scripts/sanitize-fixture.sh                     (new, executable)
  SECURITY.md                                     (new, stub)
  docs/adr/0007-import-scope-export-only.md       (Addendum section only)
  CHANGELOG.md                                    (Unreleased only)

Files you must NOT touch:
  crates/core/**, crates/cli/**, ui/**, extension/**,
  schema/migrations/**, docs/STANDARDS.md, docs/QUALITY.md, AGENTS.md,
  .github/workflows/**, docs/pr-briefs/**

  crates/cli is forbidden because a separate branch is fixing the
  TODO(#3) comment there.

House rules (AGENTS.md):
  - Never merge. Propose.
  - Never disable a check to make a build pass.
  - Do not use git reset or git checkout to rewrite committed history.
    Fix forward, or ask.
  - Protected files change only in dedicated docs-only commits.
  - Every TODO comment must link an issue: // TODO(#3): ...

Implementation steps:

1. provider.rs: add AdapterError (thiserror is already a dependency) with
   InvalidFormat, UnsupportedVersion(String), MalformedMessage { index,
   reason }, Io(std::io::Error). Change parse_export to return
   Result<RawCapture, AdapterError>, and add supported_export_versions().
   There are no existing implementors, so this breaks nothing.

2. crates/adapters/src/chatgpt/:
   - types.rs: serde structs mirroring the export JSON. Content nests as
     content.content_type and content.parts; mapping is a map keyed by
     node id.
   - parser.rs: ChatGptAdapter. Walk from current_node back through
     parent to the root, reverse for chronological order, assign
     turn_index from 0. Map content types per the brief's table. Unknown
     types go to metadata_json.unknown_blocks with tracing::warn!, never
     an error. Skip nodes whose message is null. Keep timestamps as unix
     epoch numbers, verbatim, including null.
   - mod.rs: public exports.

3. Test list: the eight tests named in the brief's section 8, named
   exactly.

4. Fixture: hand-write a SYNTHETIC fixture matching the documented ChatGPT
   export shape. Label it synthetic in a comment at the top of the fixture
   file and in crates/adapters/tests/chatgpt_parser.rs. No real export is
   available; do not claim otherwise. It must cover: a code block, a
   system role, a branch point, an unknown content type, a null
   create_time, and a null message node.
   Generate the golden file from the parser, then HAND-VERIFY it against
   the brief's section 4 before committing.

5. scripts/sanitize-fixture.sh: accepts a file path; redacts emails,
   names, URLs, API-key-shaped strings; prints a diff summary; writes
   <path>.sanitized; supports --help; chmod +x. It is the tool the real-
   export follow-up will use.

6. SECURITY.md: disclosure address placeholder, PGP key placeholder, "no
   bug bounty at this time", reference to docs/THREAT_MODEL.md (which may
   itself be a stub).

7. crates/tauri/src/main.rs: change the signature to
   `async fn main() -> tauri::Result<()>` and drop the trailing
   `.expect("error while running Kinexfluence")`. tauri::Builder::run
   already returns tauri::Result<()>. PRESERVE the existing tokio::spawn
   of the core server and the .setup() closure that calls app.manage().
   Change nothing else in that file.

8. CHANGELOG.md under [Unreleased] -> Added: ChatGPT export parser
   (fixture-tested); scripts/sanitize-fixture.sh helper; SECURITY.md
   disclosure policy. Under Fixed: crates/tauri propagates run errors
   instead of panicking.

Commit sequence (six commits, plus the ADR addendum as its own docs
commit if you prefer):
  1. feat(adapters): extend ProviderAdapter trait with export parsing
  2. feat(adapters): add ChatGPT export parser
  3. test(adapters): add fixture-driven ChatGPT parser tests
  4. chore(scripts): add fixture sanitization helper
  5. docs(security): add disclosure policy stub
  6. fix(tauri): propagate run error instead of expect

Verify before each commit:
  cargo fmt --all
  cargo clippy --all-targets -- -D warnings
  cargo test -p kfx-adapters

Verify before reporting:
  cargo test --workspace          # must be 23 passing
  cargo deny check
  git diff main..HEAD --stat      # under 800 lines excluding fixtures and
                                  # lockfiles

Do NOT push. Do NOT merge. Report:
  - commit hashes and one-line messages
  - the cargo test summary line
  - diff line count excluding fixtures and lockfiles
  - confirmation that the synthetic fixture is labelled as such
  - any content_type met but not handled
  - any test skipped and why
  - anything in this brief that turned out to be wrong about the code

If the diff exceeds 800 lines excluding fixtures and lockfiles, stop and
report. Do not push.
```
