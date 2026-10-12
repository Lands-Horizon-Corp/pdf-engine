# syntax=docker/dockerfile:1
#
# Multi-arch image (linux/amd64, linux/arm64). The Rust build runs natively on the build
# machine and cross-compiles for the target, which is far faster than compiling under QEMU.
#
#   docker buildx build --platform linux/amd64,linux/arm64 -t zalven88/pdf-engine:1.0.0 .
#   (or `make release`)

# ==========================================
# Stage 1: Build the Rust application
# ==========================================
FROM --platform=$BUILDPLATFORM rust:bookworm AS builder
ARG TARGETARCH

# Cross toolchain for the target architecture (skipped when building natively).
RUN case "$TARGETARCH" in \
        amd64) gnu=x86-64; cross=amd64 ;; \
        arm64) gnu=aarch64; cross=arm64 ;; \
        *) echo "unsupported architecture: $TARGETARCH" >&2; exit 1 ;; \
    esac \
    && if [ "$(dpkg --print-architecture)" != "$TARGETARCH" ]; then \
        apt-get update \
        && apt-get install -y --no-install-recommends gcc-$gnu-linux-gnu g++-$gnu-linux-gnu libc6-dev-$cross-cross \
        && rm -rf /var/lib/apt/lists/*; \
    fi

ENV CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_LINKER=x86_64-linux-gnu-gcc \
    CC_x86_64_unknown_linux_gnu=x86_64-linux-gnu-gcc \
    CXX_x86_64_unknown_linux_gnu=x86_64-linux-gnu-g++ \
    AR_x86_64_unknown_linux_gnu=x86_64-linux-gnu-ar \
    CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc \
    CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc \
    CXX_aarch64_unknown_linux_gnu=aarch64-linux-gnu-g++ \
    AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY vendor ./vendor

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target,id=pdf-engine-target-$TARGETARCH \
    case "$TARGETARCH" in \
        amd64) target=x86_64-unknown-linux-gnu ;; \
        arm64) target=aarch64-unknown-linux-gnu ;; \
    esac \
    && rustup target add "$target" \
    && cargo build --release --locked --target "$target" \
    && cp "target/$target/release/pdf-engine" /pdf-engine

# ==========================================
# Stage 2: Collect the curated font set (fonts/install.sh) from the Noto packages
# ==========================================
FROM --platform=$BUILDPLATFORM debian:bookworm-slim AS fonts
RUN apt-get update && apt-get install -y --no-install-recommends \
    fonts-noto-core \
    fonts-noto-extra \
    fonts-noto-mono \
    fonts-noto-cjk \
    fonts-noto-color-emoji
COPY fonts /etc/pdf-engine
RUN /etc/pdf-engine/install.sh

# ==========================================
# Stage 3: Runtime
# ==========================================
FROM debian:bookworm-slim

ARG VERSION=dev
LABEL org.opencontainers.image.title="pdf-engine" \
      org.opencontainers.image.description="HTML/CSS + Minijinja templates to PDF, rendered in-process with Fulgur" \
      org.opencontainers.image.source="https://github.com/Lands-Horizon-Corp/pdf-engine" \
      org.opencontainers.image.version="$VERSION"

WORKDIR /app

# Fulgur renders in-process; the image only needs fontconfig (loaded at runtime via dlopen, so
# `ldd` won't show it) and the curated fonts that fonts/fonts.conf exposes.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    fontconfig \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

COPY fonts /etc/pdf-engine
COPY --from=fonts /usr/share/fonts/pdf-engine /usr/share/fonts/pdf-engine
RUN FONTCONFIG_FILE=/etc/pdf-engine/fonts.conf fc-cache -f
ENV FONTCONFIG_FILE=/etc/pdf-engine/fonts.conf

COPY --from=builder /pdf-engine /usr/local/bin/pdf-engine

EXPOSE 6767
CMD ["pdf-engine"]
