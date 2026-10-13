# Kinexfluence

**Where scattered thinking becomes project intelligence.**

Kinexfluence collects relevant fragments from every AI chat you use and
weaves them into one project memory you control. Local-first, provider-agnostic,
human-in-the-loop.

## Layout

- `crates/core` — storage, normalizer, scorer, probe engine, exporter
- `crates/adapters` — provider adapters (ChatGPT, Claude, Gemini, ...)
- `crates/cli` — `kinexfluence` command-line tool
- `crates/tauri` — desktop shell (Tauri v2)
- `extension` — browser extension (Manifest V3)
- `ui` — React UI for the Atlas
- `schema/migrations` — SQL migrations (source of truth)
- `fixtures` — provider DOM snapshots and export files
- `docs/adr` — architecture decision records

## Quickstart

```bash
# install JS deps
(cd ui && npm install)
(cd extension && npm install)

# apply DB migrations
./scripts/migrate.sh

# run desktop app + UI
./scripts/dev.sh
```

## Status

Pre-alpha. Building MVP per the 8-week plan.
