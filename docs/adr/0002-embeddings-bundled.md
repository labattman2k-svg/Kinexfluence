# ADR 0002 — Embeddings model is bundled

Status: accepted
Date: 2026-10-13

## Context
Offline-first is a brand promise. Downloading a model on first run would
make the first experience network-dependent.

## Decision
Bundle `all-MiniLM-L6-v2` (~80 MB ONNX) in the installer.

## Consequences
- Larger installer; no network needed on first run.
- Model version is pinned; upgrades require an app update.
- Checksum verified at load time.
