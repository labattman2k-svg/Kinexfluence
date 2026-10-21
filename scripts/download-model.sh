#!/usr/bin/env bash
#
# Fetch the embedding model and its tokenizer into `models/`.
#
# The model is `all-MiniLM-L6-v2`, a 384-dimensional sentence encoder used for
# the S1 semantic signal. It is not bundled into the binary -- it is 90 MB, and
# the core builds a different ONNX Runtime for every platform it targets --
# so it is fetched separately and ignored by git.
#
# The graph's inputs and outputs are fixed by the upstream export:
#
#   inputs:  input_ids, attention_mask, token_type_ids  (all int64)
#   outputs: last_hidden_state                           (float32, [1, seq, 384])
#
# The pooling and the normalisation are applied in Rust, not in the graph.
#
# The revision is pinned. A model that changes under a moving tag would make
# the scores incomparable between a developer's machine and CI, and the
# sentence-transformers wrapper has changed the graph's output names before.
#
# Usage: scripts/download-model.sh [target-dir]   (default: models/)

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET_DIR="${1:-${REPO_ROOT}/models}"

BASE_URL="https://huggingface.co/sentence-transformers/all-MiniLM-L6-v2/resolve/1110a243fdf4706b3f48f1d95db1a4f5529b4d41"

# `sha256sum` on Linux, `shasum -a 256` on macOS.
if command -v sha256sum >/dev/null 2>&1; then
  sha256_of() { sha256sum "$1" | cut -d' ' -f1; }
elif command -v shasum >/dev/null 2>&1; then
  sha256_of() { shasum -a 256 "$1" | cut -d' ' -f1; }
else
  echo "error: neither sha256sum nor shasum is available; cannot verify the download" >&2
  exit 1
fi

# Fetch to a temporary name and move it into place, so an interrupted
# download never leaves a truncated file that looks complete.
fetch() {
  local path="$1" expected="$2" destination="${TARGET_DIR}/$3"

  if [ -f "${destination}" ] && [ "$(sha256_of "${destination}")" = "${expected}" ]; then
    echo "already present: ${destination}"
    return
  fi

  echo "fetching ${path}..."
  curl --fail --location --silent --show-error \
    --output "${destination}.tmp" \
    "${BASE_URL}/${path}"

  local actual
  actual="$(sha256_of "${destination}.tmp")"
  if [ "${actual}" != "${expected}" ]; then
    rm -f "${destination}.tmp"
    echo "error: ${path} has checksum ${actual}, expected ${expected}" >&2
    exit 1
  fi

  mv "${destination}.tmp" "${destination}"
  echo "wrote ${destination}"
}

mkdir -p "${TARGET_DIR}"
fetch "onnx/model.onnx" \
  "6fd5d72fe4589f189f8ebc006442dbb529bb7ce38f8082112682524616046452" \
  "model.onnx"
fetch "tokenizer.json" \
  "be50c3628f2bf5bb5e3a7f17b1f74611b2561a3a27eeab05e5aa30f411572037" \
  "tokenizer.json"

echo
echo "The model is in ${TARGET_DIR}. The core finds it there when it runs"
echo "from the repository root, and elsewhere when KINEXFLUENCE_MODEL_DIR"
echo "points at it."
