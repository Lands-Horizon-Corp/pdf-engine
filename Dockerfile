# ==========================================
# Stage 1: Build the Rust application
# ==========================================
FROM rust:bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY benches ./benches
COPY vendor ./vendor

RUN cargo build --release

# Stage 2: collect the curated font set (fonts/install.sh) from the Noto packages.
FROM debian:bookworm-slim AS fonts
RUN apt-get update && apt-get install -y --no-install-recommends \
    fonts-noto-core \
    fonts-noto-extra \
    fonts-noto-mono \
    fonts-noto-cjk \
    fonts-noto-color-emoji
COPY fonts /etc/pdf-engine
RUN /etc/pdf-engine/install.sh

FROM debian:bookworm-slim

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

COPY --from=builder /app/target/release/pdf-engine /usr/local/bin/pdf-engine

CMD ["pdf-engine"]