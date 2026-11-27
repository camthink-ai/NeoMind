#!/usr/bin/env bash
#
# llama-init — one-shot GGUF downloader for the NeoMind llama.cpp companion.
# Runs as a separate init container (see docker-compose.override.yml) and
# writes the text model + vision mmproj into a shared named volume the llama
# service mounts. Idempotent across restarts: skips files already present.
#
# Env: LLAMA_MODEL_DIR / LLAMA_MODEL_URL / LLAMA_MODEL_FILE / LLAMA_MODEL_SHA256 /
#      LLAMA_MMPROJ_URL / LLAMA_MMPROJ_FILE / LLAMA_MMPROJ_SHA256
set -euo pipefail

MODEL_DIR="${LLAMA_MODEL_DIR:-/models}"
MODEL_URL="${LLAMA_MODEL_URL:-https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/main/gemma-4-E2B_q4_0-it.gguf}"
MODEL_FILE="${LLAMA_MODEL_FILE:-gemma-4-E2B_q4_0-it.gguf}"
MODEL_SHA256="${LLAMA_MODEL_SHA256:-}"
MMPROJ_URL="${LLAMA_MMPROJ_URL:-https://huggingface.co/google/gemma-4-E2B-it-qat-q4_0-gguf/resolve/main/gemma-4-E2B-it-mmproj.gguf}"
MMPROJ_FILE="${LLAMA_MMPROJ_FILE:-gemma-4-E2B-it-mmproj.gguf}"
MMPROJ_SHA256="${LLAMA_MMPROJ_SHA256:-}"

mkdir -p "${MODEL_DIR}"

verify_sha256() {
  local file="$1" expected="$2" name="$3"
  if [[ -z "${expected}" ]]; then
    return 0
  fi
  if ! command -v sha256sum >/dev/null 2>&1; then
    echo "ERROR: sha256sum not found; cannot verify ${name}" >&2
    return 1
  fi
  local actual expected_lc
  actual="$(sha256sum "${file}" | awk '{print $1}' | tr '[:upper:]' '[:lower:]')"
  expected_lc="$(printf '%s' "${expected}" | tr '[:upper:]' '[:lower:]')"
  if [[ "${actual}" != "${expected_lc}" ]]; then
    echo "ERROR: SHA-256 mismatch for ${name}: expected ${expected}, got ${actual}" >&2
    return 1
  fi
  echo "SHA-256 verified: ${name}"
}

# Download one file: resume partials, retry transient failures, verify optional
# SHA-256, atomic rename. curl -C - resumes; a completed-but-renamed file means
# we never reuse a partial.
download() {
  local url="$1" path="$2" name="$3" expected_sha="${4:-}"

  if [[ -f "${path}" && -s "${path}" ]]; then
    if verify_sha256 "${path}" "${expected_sha}" "${name}"; then
      echo "Model already present: ${name} ($(du -h "${path}" | cut -f1))"
      return 0
    fi
    echo "WARN: existing ${name} failed checksum verification; re-downloading..." >&2
    rm -f "${path}"
  fi

  echo "Downloading ${name} (${url}) -> ${path}"
  local rc=0
  curl -fL --retry 5 --retry-delay 5 -C - -o "${path}.part" "${url}" || rc=$?
  if [[ ${rc} -ne 0 ]]; then
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
    rm -f "${path}.part"
    exit 1
  fi

  if ! verify_sha256 "${path}.part" "${expected_sha}" "${name}"; then
    rm -f "${path}.part"
    exit 1
  fi

  mv "${path}.part" "${path}"
  echo "Model ready: ${name} ($(du -h "${path}" | cut -f1))"
}

download "${MODEL_URL}"  "${MODEL_DIR}/${MODEL_FILE}"  "text model"    "${MODEL_SHA256}"
download "${MMPROJ_URL}" "${MODEL_DIR}/${MMPROJ_FILE}" "vision mmproj" "${MMPROJ_SHA256}"

echo "All models ready in ${MODEL_DIR}:"
ls -lh "${MODEL_DIR}"
