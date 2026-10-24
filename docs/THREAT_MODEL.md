# Threat Model

Status: current implementation, pre-alpha
Last reviewed: 2026-10-24

This document describes the security boundaries of the software as it exists
today. It is not a security certification. The plaintext database, lack of API
authentication, and unfinished disclosure channel are material limitations.

## Scope and assets

The system includes the Rust CLI and core, a local Axum HTTP API, the Tauri
desktop shell, the Atlas UI, and a browser extension scaffold. The main assets
are imported chat exports, snippets and project briefs, committed context,
SQLite database files, generated exports, and the locally installed embedding
model.

Imported conversations can contain personal, confidential, or proprietary
information. Treat the source exports, database, SQLite WAL/SHM files, backups,
and context packs as sensitive. The application does not encrypt the database:
SQLCipher is not wired in, even when the storage configuration requests
encryption.

## Trust boundaries and data flows

- The user selects a provider export and imports it from a local file. The CLI
  parses the export and writes normalized content to the local SQLite database.
- The CLI and Tauri shell start the core on `127.0.0.1` by default. The Atlas
  UI sends project and snippet requests to that loopback HTTP API. The API can
  read and mutate the local database, including committing cart items.
- The server's CORS layer permits the Atlas development origin and the two
  configured Tauri origins. CORS is enforced by browsers, not by the server as
  an authorization check. It can prevent an untrusted web page from reading a
  response, and browsers preflight many cross-origin requests, but a simple
  request may still reach a handler. In particular, the bodyless commit route
  can be invoked as a simple request. The current server does not reject an
  unapproved `Origin` header before dispatching the request.
- The API has no authentication token. Loopback binding limits remote network
  access in the default CLI/Tauri configurations; it does not distinguish the
  application from another process running as the same user. The core server
  APIs also accept a host from configuration, so callers other than the CLI
  and Tauri must keep it loopback-only themselves.
- The extension currently loads on supported provider pages and logs the page
  URL to the browser console. It does not capture conversations or call the
  local API. Its declared host permissions cover the provider sites, not the
  local core.
- During installation and development, package managers and build tools fetch
  dependencies. The ONNX Runtime may be downloaded during a build, and
  `scripts/download-model.sh` fetches a model and tokenizer from Hugging Face
  at a pinned revision and verifies their SHA-256 hashes. These setup-time
  connections are distinct from application runtime behavior.

## Threat actors

This model considers:

- A malicious or compromised website the user visits in the same browser,
  attempting to read or change local project data through the browser.
- Another untrusted process running as the same operating-system user,
  attempting to connect to the local API or read the database.
- A compromised dependency, build tool, model source, or provider export that
  supplies malicious code or misleading content.
- A person or process with access to the user's account, filesystem, backups,
  or unlocked device.

It does not treat an attacker with operating-system administrator access as
contained by the application. Such an attacker can generally inspect files,
processes, and decrypted data available to the user.

## Current controls

- The standard CLI and Tauri startup configurations bind the API to
  `127.0.0.1`; they do not expose it on all network interfaces.
- CORS response headers are limited to the UI and Tauri origins. This is a
  browser read boundary, not authentication and not complete server-side
  request rejection.
- The embedding download script pins the upstream model revision and checks
  the downloaded model and tokenizer against fixed SHA-256 values. The
  embedder checks these files again when loading them.
- The application makes no telemetry or sync requests. Its API traffic is
  local; setup and dependency downloads are external as described above.

## Known risks and user guidance

- **Database confidentiality:** SQLite is plaintext. Protect the operating
  system account and filesystem, restrict access to the database and its
  adjacent WAL/SHM files, and secure exports and backups. Do not place them in
  a shared or public directory.
- **Local API access:** there is no authentication. Any process running as
  the user can make API requests. Do not treat loopback binding or CORS as a
  security boundary against local software. The access token required by
  `docs/STANDARDS.md` is not implemented.
- **Browser-origin writes:** CORS does not stop every request from being
  transmitted. Until the server rejects unapproved origins before dispatch,
  avoid relying on CORS to protect state-changing routes from pages open in
  the same browser. This risk is distinct from the authentication gap.
- **Host configuration:** only the standard CLI and Tauri startup paths
  select loopback explicitly. Any other caller configuring the core server
  must not expose it on a network interface.
- **Supply chain and imports:** dependency and model downloads, as well as
  imported exports, cross trust boundaries. Use trusted sources, retain the
  model checksum verification, and do not assume imported text is accurate
  or safe to publish.
- **Disclosure:** `SECURITY.md` still identifies its contact address and PGP
  key as placeholders. The address has not been confirmed as monitored, and
  the PGP block is not a usable key. Do not send sensitive vulnerability
  details there or encrypt to that placeholder. A working private disclosure
  channel remains a release blocker.
- **Legal distribution details:** the governing jurisdiction and legal-notice
  address in `LICENSE` are unresolved. The license says these details must be
  supplied before distribution beyond the maintainer's own machine.

## Review triggers

Review this document when authentication, storage encryption, browser capture,
network behavior, or distribution changes. Reassess before public beta and
before making new privacy or security claims.
