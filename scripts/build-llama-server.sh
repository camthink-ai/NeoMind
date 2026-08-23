#!/usr/bin/env bash
# =============================================================================
# Build the bundled `neomind-llama-server` (llama.cpp llama-server) for Tauri.
#
# The desktop app spawns this sibling binary (externalBin) for the builtin
# LFM2.5 model. Same pattern as `neomind-extension-runner`.
#
# Platform handling (see builtin-llm design §9 / accel variant selection):
#   - darwin (arm64/x86_64)  → GGML_METAL=ON
#   - linux x86_64           → GGML_AMX_INT8=OFF  (llama.cpp #19184: AMX
#                              backend breaks LFM's shortconv graph on CPU)
#   - linux aarch64 / windows→ default (NEON / CPU)
#
# Optional GPU (Jetson / NVIDIA Linux servers) — build ON the device:
#   CUDA=1      scripts/build-llama-server.sh   # GGML_CUDA=ON, arch=native
#   CUDA_ARCH=87 ...                            # override arch (Orin=87)
#
# Usage:
#   TARGET=<rust-triple> scripts/build-llama-server.sh     # explicit (CI)
#   scripts/build-llama-server.sh                          # detect host
#
# Output: web/src-tauri/binaries/neomind-llama-server[-<target>][.exe]
# =============================================================================
set -euo pipefail

LLAMA_CPP_TAG="${LLAMA_CPP_TAG:-b10545}"
REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WORK="$(mktemp -d)"

# --- target detection ------------------------------------------------------
detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os:$arch" in
    Darwin:arm64)  echo "aarch64-apple-darwin" ;;
    Darwin:x86_64) echo "x86_64-apple-darwin" ;;
    Linux:x86_64)  echo "x86_64-unknown-linux-gnu" ;;
    Linux:aarch64) echo "aarch64-unknown-linux-gnu" ;;
    MINGW*:x86_64|MSYS*:x86_64) echo "x86_64-pc-windows-msvc" ;;
    *) echo "unsupported: $os:$arch" >&2; exit 1 ;;
  esac
}
TARGET="${TARGET:-$(detect_target)}"
echo "🔧 target: $TARGET"

# --- optional CUDA branch (default OFF — CI/desktop paths unchanged) ---------
if [ "${CUDA:-0}" = "1" ]; then
  command -v nvcc >/dev/null 2>&1 || {
    echo "❌ CUDA=1 but nvcc not found — install the CUDA toolkit first (Jetson: via JetPack)" >&2
    exit 1
  }
  CUDA_ARCH_RESOLVED="${CUDA_ARCH:-native}"
  echo "🔌 CUDA build: GGML_CUDA=ON, arch=${CUDA_ARCH_RESOLVED} (build ON the target device for native)"
fi

# --- clone + build llama.cpp -------------------------------------------------
cleanup() { rm -rf "$WORK"; }
trap cleanup EXIT

echo "📥 cloning llama.cpp @ $LLAMA_CPP_TAG ..."
git clone --depth 1 --branch "$LLAMA_CPP_TAG" \
  https://github.com/ggml-org/llama.cpp.git "$WORK/llama.cpp" -q

CMAKE_FLAGS=(-DLLAMA_CURL=OFF -DGGML_LLAMAFILE=OFF)
case "$TARGET" in
  aarch64-apple-darwin|x86_64-apple-darwin)  CMAKE_FLAGS+=(-DGGML_METAL=ON) ;;
  x86_64-unknown-linux-gnu)                  CMAKE_FLAGS+=(-DGGML_AMX_INT8=OFF) ;;
  *) ;; # arm64 linux (NEON default) / windows (CPU)
esac
# Composes with the per-target flags above (e.g. linux-x64 keeps AMX off).
if [ "${CUDA:-0}" = "1" ]; then
  CMAKE_FLAGS+=(-DGGML_CUDA=ON -DCMAKE_CUDA_ARCHITECTURES="${CUDA_ARCH_RESOLVED}")
  # First GPU-enabled build wins at runtime (bundled binary > auto-download).
  echo "🔌 CUDA flags: ${CMAKE_FLAGS[-2]} ${CMAKE_FLAGS[-1]}"
fi

echo "⚙️  configuring cmake ..."
cmake -S "$WORK/llama.cpp" -B "$WORK/build" \
  -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF \
  "${CMAKE_FLAGS[@]}"

echo "🏗️  building llama-server ..."
cmake --build "$WORK/build" --target llama-server -j"$(sysctl -n hw.ncpu 2>/dev/null || nproc 2>/dev/null || echo 4)"

# --- stage for Tauri ----------------------------------------------------------
OUT_DIR="$REPO_ROOT/web/src-tauri/binaries"
mkdir -p "$OUT_DIR"

case "$TARGET" in
  *windows*)
    SRC="$WORK/build/bin/llama-server.exe"
    OUT="$OUT_DIR/neomind-llama-server-$TARGET.exe" ;;
  *)
    SRC="$WORK/build/bin/llama-server"
    OUT="$OUT_DIR/neomind-llama-server-$TARGET" ;;
esac

cp "$SRC" "$OUT"
chmod +x "$OUT"
echo "✅ bundled llama-server → $OUT"
ls -lh "$OUT"
