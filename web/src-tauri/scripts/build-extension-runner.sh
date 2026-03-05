#!/bin/bash
# Build and copy neomind-extension-runner for Tauri packaging
#
# This script builds the extension-runner binary and copies it to the
# correct location for Tauri's externalBin feature.
#
# Usage:
#   ./build-extension-runner.sh [release|debug]
#
# The binary will be placed in:
#   src-tauri/binaries/neomind-extension-runner-{target-triple}

set -e

# Get script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# Project root is 3 levels up from script (scripts -> src-tauri -> web -> project root)
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
BINARIES_DIR="$SCRIPT_DIR/../binaries"

# Build mode (release or debug)
BUILD_MODE="${1:-release}"
BUILD_FLAG=""
if [ "$BUILD_MODE" = "release" ]; then
    BUILD_FLAG="--release"
fi

# Detect target triple
detect_target() {
    local OS="$(uname -s)"
    local ARCH="$(uname -m)"
    
    case "$OS" in
        Darwin)
            case "$ARCH" in
                arm64) echo "aarch64-apple-darwin" ;;
                x86_64) echo "x86_64-apple-darwin" ;;
                *) echo "unknown-apple-darwin" ;;
            esac
            ;;
        Linux)
            case "$ARCH" in
                x86_64) echo "x86_64-unknown-linux-gnu" ;;
                aarch64) echo "aarch64-unknown-linux-gnu" ;;
                *) echo "unknown-unknown-linux-gnu" ;;
            esac
            ;;
        MINGW*|MSYS*|CYGWIN*)
            echo "x86_64-pc-windows-msvc"
            ;;
        *)
            echo "unknown-unknown-unknown"
            ;;
    esac
}

TARGET=$(detect_target)
echo "Detected target: $TARGET"

# Create binaries directory
mkdir -p "$BINARIES_DIR"

# Build the extension-runner
echo "Building neomind-extension-runner ($BUILD_MODE)..."
cd "$PROJECT_ROOT"
cargo build $BUILD_FLAG -p neomind-extension-runner

# Determine binary path (relative to project root)
if [ "$BUILD_MODE" = "release" ]; then
    BINARY_PATH="$PROJECT_ROOT/target/release/neomind-extension-runner"
else
    BINARY_PATH="$PROJECT_ROOT/target/debug/neomind-extension-runner"
fi

# Add .exe extension for Windows
if [[ "$TARGET" == *"windows"* ]]; then
    BINARY_PATH="${BINARY_PATH}.exe"
fi

# Check if binary exists
if [ ! -f "$BINARY_PATH" ]; then
    echo "Error: Binary not found at $BINARY_PATH"
    exit 1
fi

# Copy to binaries directory with target triple suffix
OUTPUT_NAME="neomind-extension-runner-$TARGET"
if [[ "$TARGET" == *"windows"* ]]; then
    OUTPUT_NAME="${OUTPUT_NAME}.exe"
fi

cp "$BINARY_PATH" "$BINARIES_DIR/$OUTPUT_NAME"
echo "Copied to: $BINARIES_DIR/$OUTPUT_NAME"

# Make executable
chmod +x "$BINARIES_DIR/$OUTPUT_NAME"

echo "✅ Extension runner built successfully!"
echo "   Target: $TARGET"
echo "   Mode: $BUILD_MODE"
echo "   Output: $BINARIES_DIR/$OUTPUT_NAME"