# =============================================================================
# NeoMind Dockerfile — Multi-stage build (Ubuntu 22.04 / glibc 2.35)
# =============================================================================
# Usage:
#   docker build -t neomind:latest .
#   docker compose up -d
#
# Platforms: linux/amd64, linux/arm64
#
# Why glibc (not Alpine/musl): extensions ship native binaries (`extension.so`)
# built against glibc on Ubuntu — a musl container cannot dlopen a glibc-linked
# shared library (different libc + dynamic linker), so the extension marketplace
# is unusable in an Alpine image. Ubuntu 22.04 (glibc 2.35) matches the
# bare-metal release baseline (see release-build-glibc22.04 memory), so Docker
# and bare-metal load the exact same extension binaries. Both the build and
# runtime stages use ubuntu:22.04 so the produced binary + loaded extensions
# share one glibc version (2.35).
# ============================================================================

# ---------------------------------------------------------------------------
# Stage 1: Build frontend (static output — libc-irrelevant, alpine is fine)
# ---------------------------------------------------------------------------
FROM --platform=$BUILDPLATFORM node:20-alpine AS frontend

WORKDIR /build/web

# Install dependencies first (layer cache)
COPY web/package.json web/package-lock.json ./
RUN npm ci --ignore-scripts

# Copy source and build
COPY web/ ./
RUN npm run build

# ---------------------------------------------------------------------------
# Stage 2: Build backend (ubuntu:22.04 = glibc 2.35, matches bare-metal)
# ---------------------------------------------------------------------------
FROM --platform=$TARGETPLATFORM ubuntu:22.04 AS backend

ARG TARGETARCH
ENV DEBIAN_FRONTEND=noninteractive

# build-essential = gcc + g++ + make (make is required by tikv-jemalloc-sys's
# C build). curl+ca-certificates for rustup. pkg-config for build scripts.
# No libssl-dev: reqwest/lettre are rustls-only; the only "openssl" in the tree
# is openssl-probe (pure-Rust cert-path lookup, no link).
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        curl \
        ca-certificates \
        pkg-config \
    && rm -rf /var/lib/apt/lists/*

# Install Rust (pin to match rust-toolchain.toml). ubuntu:22.04 has no official
# rust:*-jammy image, so rustup is the path to a glibc-2.35 toolchain.
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --default-toolchain 1.92.0 --profile minimal
ENV PATH="/root/.cargo/bin:${PATH}"

WORKDIR /build

# Cache dependencies by creating a dummy build first
COPY Cargo.toml Cargo.lock ./
COPY crates/neomind-core/Cargo.toml crates/neomind-core/Cargo.toml
COPY crates/neomind-api/Cargo.toml crates/neomind-api/Cargo.toml
COPY crates/neomind-agent/Cargo.toml crates/neomind-agent/Cargo.toml
COPY crates/neomind-cli/Cargo.toml crates/neomind-cli/Cargo.toml
COPY crates/neomind-cli-ops/Cargo.toml crates/neomind-cli-ops/Cargo.toml
COPY crates/neomind-storage/Cargo.toml crates/neomind-storage/Cargo.toml
COPY crates/neomind-devices/Cargo.toml crates/neomind-devices/Cargo.toml
COPY crates/neomind-rules/Cargo.toml crates/neomind-rules/Cargo.toml
COPY crates/neomind-messages/Cargo.toml crates/neomind-messages/Cargo.toml
COPY crates/neomind-extension-sdk/Cargo.toml crates/neomind-extension-sdk/Cargo.toml
COPY crates/neomind-extension-runner/Cargo.toml crates/neomind-extension-runner/Cargo.toml
COPY crates/neomind-data-push/Cargo.toml crates/neomind-data-push/Cargo.toml

# Create dummy source files for dependency caching
RUN mkdir -p crates/neomind-core/src && echo "" > crates/neomind-core/src/lib.rs && \
    mkdir -p crates/neomind-api/src && echo "fn main(){}" > crates/neomind-api/src/lib.rs && \
    mkdir -p crates/neomind-agent/src && echo "" > crates/neomind-agent/src/lib.rs && \
    mkdir -p crates/neomind-cli/src && echo "fn main(){}" > crates/neomind-cli/src/main.rs && \
    mkdir -p crates/neomind-cli-ops/src && echo "" > crates/neomind-cli-ops/src/lib.rs && \
    mkdir -p crates/neomind-storage/src && echo "" > crates/neomind-storage/src/lib.rs && \
    mkdir -p crates/neomind-devices/src && echo "" > crates/neomind-devices/src/lib.rs && \
    mkdir -p crates/neomind-rules/src && echo "" > crates/neomind-rules/src/lib.rs && \
    mkdir -p crates/neomind-messages/src && echo "" > crates/neomind-messages/src/lib.rs && \
    mkdir -p crates/neomind-extension-sdk/src && echo "" > crates/neomind-extension-sdk/src/lib.rs && \
    mkdir -p crates/neomind-extension-runner/src && echo "" > crates/neomind-extension-runner/src/lib.rs && \
    mkdir -p crates/neomind-data-push/src && echo "" > crates/neomind-data-push/src/lib.rs

# jemalloc (neomind-cli global allocator) must assume 64KB pages on ARM, else it
# crashes on 64KB-page hosts like Raspberry Pi 5 / Jetson (the arm64 container
# runs on the host kernel, so a 64KB-page Pi5 host still sees 64KB pages inside
# the container). No-op on amd64 (4KB pages). See release-build-glibc22.04.
RUN if [ "$TARGETARCH" = "arm64" ] || [ "$TARGETARCH" = "aarch64" ]; then export JEMALLOC_SYS_WITH_LG_PAGE=16; fi && \
    cargo build --release -p neomind-cli -p neomind-extension-runner 2>/dev/null || true

# Copy real source code and build
COPY crates/ crates/
RUN if [ "$TARGETARCH" = "arm64" ] || [ "$TARGETARCH" = "aarch64" ]; then export JEMALLOC_SYS_WITH_LG_PAGE=16; fi && \
    cargo build --release -p neomind-cli -p neomind-extension-runner

# ---------------------------------------------------------------------------
# Stage 3: Runtime (ubuntu:22.04 = glibc 2.35, same as build)
# ---------------------------------------------------------------------------
# ---------------------------------------------------------------------------
# Stage 3: Bundled llama-server (llama.cpp) for the builtin LFM model.
#   amd64 → GGML_AMX_INT8=OFF (llama.cpp #19184: AMX breaks LFM shortconv)
#   arm64 → default (NEON)
# ---------------------------------------------------------------------------
FROM --platform=$TARGETPLATFORM ubuntu:22.04 AS llamaserver
ARG TARGETARCH
RUN apt-get update && apt-get install -y --no-install-recommends curl ca-certificates cmake build-essential \
    && rm -rf /var/lib/apt/lists/*
# curl+tarball (not git clone) — Docker build networks often block git; the
# tarball is a plain HTTPS GET and is uniformly reliable.
RUN curl -fsSL -o /tmp/llama.cpp.tar.gz \
      https://github.com/ggml-org/llama.cpp/archive/refs/tags/b10524.tar.gz \
    && mkdir -p /build/llama.cpp \
    && tar -xzf /tmp/llama.cpp.tar.gz -C /build/llama.cpp --strip-components=1 \
    && rm /tmp/llama.cpp.tar.gz
WORKDIR /build/llama.cpp
RUN if [ "$TARGETARCH" = "amd64" ]; then \
      cmake -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF -DGGML_AMX_INT8=OFF; \
    else \
      cmake -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_SHARED_LIBS=OFF; \
    fi \
    && cmake --build build --target llama-server -j"$(nproc)"

FROM ubuntu:22.04 AS runtime

ENV DEBIAN_FRONTEND=noninteractive

# apt-get upgrade patches base-image packages between refreshes (the main
# source of "high" findings in image scans). Then add runtime deps.
#
# python3 + python3-pip: agents and Python-sidecar extensions (voice / TTS /
# ASR / OCR-VL…) invoke `python3` for data processing and sidecar services.
# Without it, `python3` in the container exits 127 and those workloads fail.
# python-is-python3: scripts/extensions that hardcode `python` get it too.
#
# ffmpeg intentionally NOT included: +~326MB for media/stream pipelines
# (stream-player, video extensions). If you need it, add `ffmpeg` to the apt
# line below — the image then grows to ~450MB.
RUN apt-get update && apt-get upgrade -y && \
    apt-get install -y --no-install-recommends ca-certificates curl tzdata \
        python3 python3-pip python-is-python3 \
        libgomp1 && \
    rm -rf /var/lib/apt/lists/* && \
    groupadd --system neomind && useradd --system --gid neomind --home-dir /app neomind

WORKDIR /app

# Copy backend binaries (neomind finds extension-runner in same directory or PATH)
COPY --from=backend /build/target/release/neomind /usr/local/bin/neomind
COPY --from=backend /build/target/release/neomind-extension-runner /usr/local/bin/neomind-extension-runner

# Copy the bundled llama-server (spawned by the builtin LFM bootstrap).
# NOTE: links against libgomp (OpenMP) — the runtime stage's `libgomp1` is
# what makes this binary exec; removing it silently breaks the builtin LLM.
COPY --from=llamaserver /build/llama.cpp/build/bin/llama-server /usr/local/bin/neomind-llama-server

# Copy frontend build output
COPY --from=frontend /build/web/dist /var/www/neomind

# Create data directory + pre-bundle the builtin LFM model (QAD Q4_0) for
# out-of-box agent use. The bootstrap reads the manifest, so both the GGUF
# and manifest.json must land under /app/data/models/<id>/. The VOLUME at
# /app/data initializes from this content on first run (named/anon volumes);
# a bind-mounted /app/data skips the seed (use the download API there).
RUN mkdir -p /app/data/models/lfm25-2.6b \
    && curl -fsSL -o /app/data/models/lfm25-2.6b/lfm25-2.6b-qad_q4_0.gguf \
       https://huggingface.co/LiquidAI/LFM2.5-2.6B-GGUF/resolve/main/LFM2.5-2.6B-QAD-Q4_0.gguf \
    && printf '{"id":"lfm25-2.6b","version":"1.0","file_name":"lfm25-2.6b-qad_q4_0.gguf","sha256":"a247afd6414918eac8e520a9e6137dc271235461ecbe1180462221d5b8d40b03","quant":"qad_q4_0"}' \
       > /app/data/models/lfm25-2.6b/manifest.json \
    && chown -R neomind:neomind /app/data

# Environment defaults
ENV NEOMIND_WEB_DIR=/var/www/neomind
ENV RUST_LOG=neomind=info
ENV RUST_BACKTRACE=1

EXPOSE 9375 1883 8081

# Health check
HEALTHCHECK --interval=30s --timeout=5s --start-period=10s --retries=3 \
    CMD curl -f http://localhost:9375/api/health || exit 1

USER neomind

VOLUME ["/app/data"]

ENTRYPOINT ["neomind"]
CMD ["serve"]
