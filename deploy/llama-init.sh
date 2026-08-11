#!/usr/bin/env bash
#
# llama-init — one-shot GGUF downloader for the NeoMind llama.cpp companion.
# Runs as a separate init container (see docker-compose.override.yml) and
# writes the text model + vision mmproj into a shared named volume the llama
# service mounts. Idempotent across restarts: skips files already present.
#
# Env: LLAMA_MODEL_DIR / LLAMA_MODEL_URL / LLAMA_MODEL_FILE /
#      LLAMA_MMPROJ_URL / LLAMA_MMPROJ_FILE
set -euo pipefail

MODEL_DIR="${LLAMA_MODEL_DIR:-/models}"
MODEL_URL="${LLAMA_MODEL_URL:-https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/main/gemma-4-E2B_q4_0-it.gguf}"
MODEL_FILE="${LLAMA_MODEL_FILE:-gemma-4-E2B_q4_0-it.gguf}"
MMPROJ_URL="${LLAMA_MMPROJ_URL:-https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/main/gemma-4-E2B-it-mmproj.gguf}"
MMPROJ_FILE="${LLAMA_MMPROJ_FILE:-gemma-4-E2B-it-mmproj.gguf}"

mkdir -p "${MODEL_DIR}"

# Download one file: resume partials, retry transient failures, atomic rename.
# curl -C - resumes; a completed-but-renamed file means we never reuse a partial.
download() {
  local url="$1" path="$2" name="$3"

  if [[ -f "${path}" && -s "${path}" ]]; then
    echo "Model already present: ${name} ($(du -h "${path}" | cut -f1))"
    return 0
  fi

  echo "Downloading ${name} (${url}) -> ${path}"
  if ! curl -fL --retry 5 --retry-delay 5 -C - -o "${path}.part" "${url}"; then
    rc=$?
    # HTTP 416 = range not satisfiable → the .part is already complete.
    if [[ ${rc} -eq 33 ]]; then
      echo "Partial file already complete (HTTP 416); continuing with ${path}.part"
    else
      echo "ERROR: ${name} download failed (curl rc=${rc})" >&2
      exit "${rc}"
    fi
  fi

  if [[ ! -s "${path}.part" ]]; then
    echo "ERROR: ${name} downloaded file is empty" >&2
    exit 1
  fi

  mv "${path}.part" "${path}"
  echo "Model ready: ${name} ($(du -h "${path}" | cut -f1))"
}

download "${MODEL_URL}"   "${MODEL_DIR}/${MODEL_FILE}"   "text model"
download "${MMPROJ_URL}"  "${MODEL_DIR}/${MMPROJ_FILE}"  "vision mmproj"

echo "All models ready in ${MODEL_DIR}:"
ls -lh "${MODEL_DIR}"
