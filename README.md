# Kinexfluence

**Where scattered thinking becomes project intelligence.**

Kinexfluence collects the fragments worth keeping out of your AI chats,
scores them against a project you are actually working on, and weaves the
ones that matter into a single context pack you can paste into your next
prompt.

It is local-first. The database, the embeddings, the scoring, and the
export all run on your machine. Nothing you import leaves it.

> **Status: pre-alpha.** The core loop works end to end and the test suite is
> green, but this is a working prototype, not a product. Read
> [Caveats](#caveats-and-known-limitations) before you rely on it, and read
> the [licence](#licence) before you use it for anything.

## Contents

- [What it is](#what-it-is)
- [Requirements](#requirements)
- [Installation](#installation)
- [Usage](#usage)
- [Caveats and known limitations](#caveats-and-known-limitations)
- [Security and privacy](#security-and-privacy)
- [Development](#development)
- [Repository layout](#repository-layout)
- [Licence](#licence)
- [Legal disclaimer](#legal-disclaimer)

## What it is

You have a dozen conversations with an assistant, across a dozen projects,
and the useful parts are scattered across all of them. A decision you made
in March, a constraint somebody gave you in May, a piece of code that took
an afternoon to get right -- all of it is in a chat transcript, and none of
it is in your repository.

Kinexfluence turns that into one thing per project. It works in five steps:

| Step | What happens |
| --- | --- |
| **Import** | Read a provider export, split it into normalised messages and **snippets** (the small pieces worth keeping: a paragraph, a code block, a single answer). |
| **Score** | Score every snippet against a project's **brief** using seven signals, and sort them. |
| **Probe** | Where the scorer says "this does not matter" and you say "it does", ask four questions and take your answer as the floor. |
| **Commit** | Move the snippets you kept from the cart into the project's context. This is the deliberate step. |
| **Export** | Render the committed context as one Markdown document: decisions, constraints, open questions, notes, and code. |

Everything is stored in one SQLite database on your machine. The whole
application is a loopback-only HTTP service plus a web interface on top of
it.

## Requirements

| | |
| --- | --- |
| **Rust** | stable, 1.78 or newer (the workspace's declared minimum) |
| **Node.js** | 20 or newer, for the Atlas UI and the browser extension |
| **A C toolchain** | `cc` and `pkg-config`; on Linux, also the Tauri webkit stack |
| **`curl`**, and **`sha256sum`** or **`shasum`** | to fetch the embedding model |
| **Disk** | a few GB for the Rust build, plus 90 MB for the model |
| **Network** | on first build only, to fetch the ONNX Runtime and the model |

**Windows** -- Visual Studio Build Tools 2022 with the "Desktop development
with C++" workload *and* the Windows SDK component. Without the SDK the
build fails at link time with an unhelpful error.

**macOS** -- the Xcode command line tools (`xcode-select --install`).

**Linux** -- `build-essential` and `pkg-config`. The desktop shell
additionally needs `libwebkit2gtk-4.1-dev`, `libgtk-3-dev`,
`libayatana-appindicator3-dev`, and `librsvg2-dev`. The CLI and the core
need none of those.

## Installation

### 1. Get the source

Clone or unpack the source into a directory of your choice:

```bash
git clone <the-repository-url> kinexfluence
cd kinexfluence
```

### 2. Fetch the embedding model

The semantic signal (S1) needs a sentence-embedding model, `all-MiniLM-L6-v2`.
It is 90 MB and is deliberately not committed to the repository, so fetch it
separately:

```bash
bash scripts/download-model.sh
```

The script pins a specific upstream revision, verifies both files against
their SHA-256, and refuses to install a file whose bytes do not match. It
writes to `models/` in the repository root.

On Windows, run it from Git Bash (which has `curl` and `sha256sum` already)
or from WSL.

The core finds the model in `models/` relative to the working directory, and
anywhere else if you point `KINEXFLUENCE_MODEL_DIR` at it:

```bash
export KINEXFLUENCE_MODEL_DIR=/path/to/models
```

**If you skip this step, everything still works.** A missing model is not an
error: the semantic signal simply falls back to a neutral 0.5, and every
other signal is unaffected. The scores you get will be worse, and the
difference is invisible in the output, so fetch the model unless you are
deliberately testing without it.

### 3. Build

```bash
cargo build --workspace
```

The first build downloads the ONNX Runtime binaries for your platform from
`cdn.pyke.io` and caches them outside the repository (on Windows under
`%LOCALAPPDATA%\ort.pyke.io`, on Linux under `~/.cache/ort.pyke.io`, on
macOS under `~/Library/Caches/ort.pyke.io`). This is why the first build
needs network access. The download is checksum-verified by the `ort` crate.
If you need to build offline, point `ORT_LIB_LOCATION` at a local copy of
the ONNX Runtime libraries.

Building the whole workspace is not required. The core, the adapters, and
the CLI are enough for everything except the desktop app:

```bash
cargo build -p kfx-core -p kfx-adapters -p kfx-cli
```

### 4. Create the database

```bash
cargo run -p kfx-cli -- migrate
```

The database defaults to `~/.kinexfluence/kinexfluence.db`; the directory is
created for you. Pass `--db <path>` to put it somewhere else. The migration
step is idempotent, so run it again whenever you pull new code.

### 5. Install the JavaScript packages

Only needed for the UI, the extension, and the end-to-end tests:

```bash
npm --prefix ui install
npm --prefix extension install
```

## Usage

### The five steps, from the command line

Every command below is a real command, and the output is what the tool
actually prints. The paths are the ones in this repository.

#### 1. Import a chat export

```bash
cargo run -p kfx-cli -- import chatgpt fixtures/chatgpt/sample-export.json
```

```text
Imported from fixtures/chatgpt/sample-export.json:
  Conversations: 2 inserted, 0 skipped (already present)
  Messages:      7
  Snippets:      6
  Attachments:   0 present, 1 missing
```

The importer also logs a warning about content it did not recognise:

```text
WARN kfx.adapters.chatgpt: unrecognised content type; preserved raw and
skipped content_type="tether_browsing_display"
```

The text is kept, so nothing is lost, but the block is not turned into a
snippet of its own.

The only provider implemented today is ChatGPT. To export your own
conversations from ChatGPT, use *Settings → Data controls → Export data* and
wait for the e-mail; the export arrives as a `conversations.json` inside a
zip. Point the importer at that file:

```bash
cargo run -p kfx-cli -- import chatgpt ~/Downloads/chatgpt-export/conversations.json
```

Re-importing the same export is safe. A conversation whose content hash is
already in the database is skipped whole, so importing the same file twice
inserts nothing the second time.

#### 2. Create a project

A project is a project you are working on. It has a **brief**, and the
brief is what the scores are measured against, so write it well:

```bash
cargo run -p kfx-cli -- project create "Apollo migration" \
  --brief "Move the Apollo service off the legacy scheduler without downtime."
```

```text
01a16552-13ca-7107-b6a3-9f66023e6156
created project "Apollo migration" (slug: apollo-migration, status: recon)
```

The id goes to standard output and the human-readable line to standard
error, so the id can be captured directly:

```bash
PID=$(cargo run -q -p kfx-cli -- project create "Apollo migration" --brief "...")
```

Other project commands: `project list`, `project show <id-or-slug>`,
`project status <id-or-slug> <status>`, and `project export <id-or-slug>`.
The status is one of `recon`, `planning`, `active`, or `archived`; a new
project starts as `recon`.

#### 3. Review the snippets and their scores

```bash
cargo run -p kfx-cli -- snippets list
```

```text
ID                                     KIND     TEXT
01a16552-0a17-714c-b614-d992a6b6f93d   text     You are a helpful assistant.
01a16552-0a18-708e-a76b-35d50d278f94   text     Write a Python function that adds two numbers.
01a16552-0a18-708e-a76b-35d645f12b58   code     def add(a, b):
01a16552-0a18-708e-a76b-35d715dbee75   text     Here is a shorter version.
01a16552-0a2f-757c-93e1-420a4a7e806b   text     What is a haiku?
01a16552-0a2f-757c-93e1-420bb3d6be41   text     Five syllables first,
```

`snippets list` takes `--limit` and `--offset` for paging.

The CLI lists the snippets; the **scoring** is what the UI and the
`GET /v1/projects/:id/snippets/scored` endpoint do, because a score needs
the project's brief and the embedding model to be meaningful. See
[the desktop app](#the-desktop-app-and-the-ui).

#### 4. Put the ones you want in the cart, then commit

```bash
cargo run -p kfx-cli -- cart add 01a16552-0a18-708e-a76b-35d50d278f94 --project apollo-migration
cargo run -p kfx-cli -- cart list --project apollo-migration
```

```text
01a16552-9d0e-7722-ba16-f01888ef7b16
added snippet 01a16552-0a18-708e-a76b-35d50d278f94 to cart
```

```text
ITEM ID                                KIND     TEXT
01a16552-9d0e-7722-ba16-f01888ef7b16   text     Write a Python function that adds two numbers.
```

The project is named by id or by slug, whichever you have. A cart entry is
a *candidate*. It is not yet part of the project's context. The commit is
the step where you say so:

```bash
cargo run -p kfx-cli -- cart commit --project apollo-migration
```

```text
1
committed 1 item(s) to project
```

`cart remove <item-id> --project <slug>` takes one back out.

#### 5. Export the context pack

```bash
cargo run -p kfx-cli -- project export apollo-migration
```

```markdown
# Apollo migration

Move the Apollo service off the legacy scheduler without downtime.

## Notes

- Write a Python function that adds two numbers.
```

Add `--out <file>` to write it to a file instead of standard output. The
pack holds **only committed items**, and it groups prose into `Decisions`,
`Constraints`, `Open Questions`, and `Notes`, with code in its own fenced
section. The grouping is keyword-based, so a fragment that reads like a
decision lands under `Decisions` and one that reads like a note lands under
`Notes`.

That Markdown is the point of the whole tool. It is what you paste into the
next prompt.

### The desktop app and the UI

The Atlas UI is a React application. You can run it two ways.

**With the Tauri desktop shell** -- the window, the core, everything:

```bash
cargo install tauri-cli --version "^2"   # once
npm --prefix ui install
./scripts/dev.sh                          # Linux and macOS
powershell scripts/dev.ps1                # Windows
```

The shell starts the core server itself in the background, so you do not
have to start it separately. The UI is at `http://localhost:5173` in
development.

**In a browser, against a core you start yourself:**

```bash
# terminal 1
cargo run -p kfx-cli -- serve

# terminal 2
npm --prefix ui run dev
```

The UI finds the core by probing `47899` and then `47821` through `47830`
for a `/v1/health` response, and caches the port it found. The core takes
the first free port from `47821` upwards, so the two agree without
configuration. If the UI cannot reach the core it says so and names the
ports it tried.

In the UI:

1. **Create a project** on the list screen, with a name and a brief.
2. **Open it**, and you get the split view: scored snippets on the left, the
   cart on the right.
3. **Read the scores.** Each snippet carries a score badge. Snippets in the
   high band start checked -- a default check is a proposal, not a
   decision. The info button opens the per-signal breakdown, so you can see
   *why* something scored the way it did.
4. **Tick something the scorer dismissed.** Ticking a low-scoring snippet
   opens the probe modal instead of selecting it: *intent*, *impact*,
   *rationale*, *confidence*. Answering it puts a relevance floor of 0.90
   under that snippet, and the breakdown says so beside the signals, so you
   can tell that the ranking was overruled by you and not by the scorer.
   Unanswered probes appear in the probe queue on the same screen.
5. **Add to the cart**, and **Commit to Project** when you are satisfied.
   The commit tells you how many items moved.
6. **Export** the pack. The UI has no export control yet, so use the CLI
   (`kinexfluence project export <slug>`) or the API
   (`GET /v1/projects/:id_or_slug/export`).

#### A note on scores

The scores are **relative to the list, not absolute**. A fixed threshold
would paint every real snippet grey, because the reachable score depends on
the brief and on the conversation, not on the quality of the snippet alone.
The bands are therefore calibrated from the best score in the list, and the
shipped fixture's whole distribution sits between 0.06 and 0.25. Even a
snippet that matches the brief perfectly and hits none of the code, marker,
or entity signals tops out around 0.30, so a fixed 0.70 threshold is
unreachable rather than merely strict.

Read a score as *how well this fits this project, compared to the other
snippets in front of you* -- not as a grade.

### The HTTP API

The core serves JSON on loopback. The routes are:

| Method | Path | |
| --- | --- | --- |
| `GET` | `/v1/health` | liveness and schema version |
| `GET` | `/v1/projects` | list projects |
| `POST` | `/v1/projects` | create a project |
| `GET` | `/v1/projects/:id_or_slug` | one project |
| `PATCH` | `/v1/projects/:id_or_slug` | change a project |
| `GET` | `/v1/projects/:id_or_slug/export` | the context pack, as `text/markdown` |
| `GET` | `/v1/projects/:id/snippets/scored` | scored candidates with their breakdowns |
| `GET` | `/v1/snippets` | imported snippets |
| `GET` | `/v1/projects/:id/cart` | the cart |
| `POST` | `/v1/projects/:id/cart` | add a snippet to the cart |
| `DELETE` | `/v1/projects/:id/cart/:item_id` | remove a cart entry |
| `POST` | `/v1/projects/:id/commit` | commit the cart; answers `{"committed": N}` |
| `POST` | `/v1/probes` | open a probe |
| `GET` | `/v1/probes/pending` | the unanswered probes |
| `PATCH` | `/v1/probes/:id` | answer a probe |

The server binds to `127.0.0.1` only, and answers CORS for the Atlas UI's
origins only (`http://localhost:5173`, `tauri://localhost`,
`http://tauri.localhost`). Any other origin is refused. See
[Security and privacy](#security-and-privacy) for what that does and does
not protect.

### Exit codes

The CLI uses a fixed taxonomy, so scripts can react to the number:

| Code | Meaning |
| --- | --- |
| `0` | success |
| `1` | the command line asked for something the program does not do |
| `2` | the export could not be read or parsed, or a project argument was unusable |
| `3` | the captures parsed but could not be normalized, or a lookup found nothing |
| `4` | the database could not be opened, migrated, or written |

## Caveats and known limitations

Read this section before you rely on any of it. Every item here is a real
gap in the current tree, not a roadmap item.

**Import**

- **Only ChatGPT is implemented.** The adapter layer supports more, and the
  manifest names Claude, Gemini, and DeepSeek, but there is no parser for
  them. `import claude` exits 1 with
  `unsupported provider: claude (try "chatgpt")`.
- **Only the active path of a branched conversation is captured.** A ChatGPT
  conversation with edited branches keeps the branch you were last on. This
  is a deliberate decision, recorded in ADR 0007.
- **Live capture does not exist.** The browser extension is a scaffold: its
  service worker and content script load on the five supported chat sites
  and write a line to the browser console, and that is all they do. There
  is no capture bar, no sidebar, and nothing saved. The import path is a
  file you export by hand.
- **Attachments are recorded but not kept.** An attachment row is written
  with its status, and the file is not copied into the database.

**Scoring**

- **The seven signals are heuristics, not a model.** S1 compares embeddings
  and is a real semantic signal; S2 (code), S3 (decision markers), S4
  (entity match) and S6 (position in the conversation) are keyword, pattern
  and position rules.
- **Two of the seven are stubs.** S5 (novelty) and S7 (source provenance)
  are not implemented and always return 0.5, so they contribute a constant
  and nothing else.
- **S1 is the heaviest signal and the noisiest.** It carries 0.35 of the
  weight, more than any other. A brief that shares vocabulary with
  everything will make everything look relevant.
- **The probe floor is a floor, not a score.** It lifts a score and never
  lowers one. A settled probe puts 0.90 under the item; a provisional one
  puts 0.70.
- **Scores are relative to the list**, as described above. There is no
  absolute quality measure.

**Interface**

- **The UI has no export control.** The whole loop works in the browser up
  to and including the commit, and the last step -- writing the context
  pack -- is a command line away (`kinexfluence project export <slug>`).
- **The UI is plain.** No styling framework, no component library, and no
  accessibility audit. It is a working interface, not a designed one.
- **The extension has no interface.** Its manifest declares a popup, and
  the popup is a static HTML file that says the capture bar is active.

**Storage and security**

- **The database is not encrypted.** `StorageConfig` has a `use_encryption`
  flag, and the core's `open()` warns that SQLCipher is not wired yet
  (ADR 0006) when it is set -- but the CLI hardcodes the flag to `false`
  and offers no way to change it, so the warning never appears. Your
  chats sit in a plaintext SQLite file. Anyone with access to your
  account can read it. Treat the database as sensitive.
- **The server is unauthenticated.** It binds to loopback and checks the
  origin, which stops a web page you visit from reading your projects. It
  does not stop another program running as you, and there is no token.
  The design calls for one at `~/.kinexfluence/token`; it does not exist.
- **The `SECURITY.md` contact address and PGP key are placeholders.** There
  is no monitored mailbox to report a vulnerability to.
- **There is no threat model.** `docs/THREAT_MODEL.md` is referenced and
  does not exist.

**Packaging and distribution**

- **There is no installer.** Tauri bundling is switched off, so there is no
  `.dmg`, no `.msi`, no `.deb`, and no AppImage. The desktop app runs from
  source with `cargo tauri dev`.
- **`cargo tauri` is a separate installation.** The `tauri-cli` is not a
  project dependency; install it yourself.
- **The repository is public.** The source is visible to anyone who looks,
  and the history is public. The licence governs what you may do with it,
  but it cannot make the code private. See [Licence](#licence).

**Quality**

- **Pre-alpha.** The interfaces will change. There is no migration
  guarantee for the database schema beyond what the migrations provide, and
  the export format is not frozen.
- **The end-to-end tests only run on Linux.** CI runs the Rust suite on
  Linux, macOS, and Windows, but the browser suite only runs on Linux.

## Security and privacy

**What leaves your machine: nothing.** There is no telemetry, no analytics,
no crash reporting, and no network call other than to `127.0.0.1`. The
embedding model runs locally through ONNX Runtime.

**What is on your machine:** the SQLite database (plaintext), the embedding
model (90 MB, fetched once), the ONNX Runtime binaries in the OS cache
directory, and the build tree.

**The origin check.** The core is loopback-only and allows three origins.
That is a real protection against the common case -- a page you happen to
be visiting reading your projects over CORS -- and it is not
authentication. Any process running as you can talk to the core directly.

**Reporting a problem.** `SECURITY.md` describes the policy, but its contact
address and PGP key are placeholders that have to be replaced before the
policy is worth anything.

## Development

```bash
cargo test --workspace          # the whole suite
cargo fmt --all -- --check      # formatting is a separate gate
cargo clippy --all-targets -- -D warnings
cargo deny check                # licence and advisory policy
```

The suite needs the embedding model to be present, or the embedding tests
skip themselves and leave a hole in the coverage:

```bash
bash scripts/download-model.sh
cargo test --workspace
```

The end-to-end suite drives a real browser against a real core and a real
database:

```bash
npm --prefix ui install
cd ui && npx playwright install --with-deps chromium && cd ..
npm --prefix ui run test:e2e
```

On a small machine, `cargo test --workspace -j 1` with
`RUST_TEST_THREADS=2` avoids the OOM killer.

The project has its own engineering contract in
[`docs/STANDARDS.md`](docs/STANDARDS.md), the current metric targets in
[`docs/QUALITY.md`](docs/QUALITY.md), and the architectural decisions in
[`docs/adr/`](docs/adr). Read the standards before changing anything: they
say which checks are hard gates, and they are stricter than the tooling
suggests.

## Repository layout

```text
crates/core        storage, normalizer, scorer, probe engine, exporter, HTTP API
crates/adapters    the ProviderAdapter trait, and one module per provider
crates/cli         the `kinexfluence` command-line tool
crates/tauri       the Tauri desktop shell
ui                 the Atlas UI (React, Vite)
extension          the browser extension (Manifest V3)
schema/migrations  the SQL migrations -- the source of truth for the schema
fixtures           provider export files, and the expected parse of them
docs/adr           architecture decision records
scripts            dev helpers, and the model download
```

## Licence

**Kinexfluence is proprietary software. It is not open source.**

The full terms are in [`LICENSE`](LICENSE). In summary:

- You may **run** the software and **use its output**.
- You may **not** redistribute it, sublicense it, sell it, or use it
  commercially. **Nothing is licensed to anyone else.**
- There is **no warranty** of any kind, and no liability beyond what the law
  does not allow to be excluded.
- Third-party components keep their own licences. The
  `all-MiniLM-L6-v2` embedding model is Apache-2.0; the Rust and
  JavaScript dependencies are MIT, Apache-2.0, BSD, and similar.

The workspace's `license` field is `LicenseRef-Proprietary`, and every
crate sets `publish = false` so the source can never be published to a
public registry by accident.

**One honest caveat.** Versions of this software released before 22 October
2026 were published to a public repository whose package manifests declared
the GNU Affero General Public License, version 3 or later. That licence is
irrevocable for anyone who holds a copy of those versions, and it is not
revoked by the switch to a proprietary licence. The corresponding source for
them remains in that repository's public history. The proprietary licence
governs this version and every version after it. Section 6 of the `LICENSE`
file says the same thing in the formal terms.

## Legal disclaimer

**No warranty.** The software is provided "as is", without warranty of any
kind, express or implied, including the warranties of merchantability,
fitness for a particular purpose, title, and non-infringement. It is
pre-alpha software and is not fit for any purpose you would not use
unfinished software for.

**Limitation of liability.** To the maximum extent permitted by law, the
licensor is not liable for any claim, damages, or other liability arising
from the software or its use, including lost data, lost profits, and
business interruption. Nothing excludes liability that cannot be excluded
by law.

**Your data is your data.** Kinexfluence reads chat exports and writes a
database on your machine. You are responsible for what you put into it, for
holding the export files securely, and for the content of the context packs
you produce. The context pack is assembled by heuristics from text you
supplied; it is a draft, not a record of truth.

**Third-party content.** The software ingests content from third-party
services, and the licensor makes no representation about the rights in that
content or about its accuracy.

**Not professional advice.** The context packs and scores are mechanical
output. They are not engineering, legal, medical, or financial advice, and
they should not be relied on as though a person had reviewed them.

**Trade marks.** The name "Kinexfluence" and the project's icon are used
by the licensor as marks. The licence grants no right to use them as a
trade mark, and none to use them in a way that suggests the licensor
endorses your product or service.

**This is not legal advice.** The summary above is a reading aid. The
`LICENSE` file is the operative document, and the governing-law clause in
section 12 defers to a jurisdiction and a contact address that have to be
fixed before the software is distributed to anyone outside the licensor's
own machine. If you need to rely on these terms, have a lawyer read the
`LICENSE` file.
