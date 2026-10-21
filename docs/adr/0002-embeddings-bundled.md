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

## Addendum — 2026-10-21

**The checksum consequence is honoured; the bundle step is deferred.**
PR #10 implements the embedder but fetches the model with
`scripts/download-model.sh` rather than embedding it in the installer,
because no installer exists yet -- Tauri's bundling is still disabled --
and the model is 90 MB of platform-specific binary that would have to be
repacked for every target.

The model is therefore fetched once, at build time, and verified twice: the
script checks the digest of what it downloads, and the embedder checks the
digest again when it loads the file. A model whose bytes do not match the
pinned revision is refused rather than loaded, because a model with the
wrong bytes still returns 384 numbers and would silently move every
semantic score in the product.

The offline promise is preserved in the meantime: a machine without the
model is not a broken machine. The server starts, the scorer works, and S1
holds its neutral value of 0.5. Bundling the model into the installer
remains the decision -- the fetch script is a stand-in for it, not a
replacement -- and lands with the first release that ships an installer.
