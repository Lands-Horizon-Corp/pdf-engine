# ==========================================
# Stage 1: Build the Rust application
# ==========================================
FROM rust:bookworm AS builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN cargo build --release

FROM debian:bookworm-slim

WORKDIR /app

RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    libfontconfig1 \
    libxml2 \
    libpixman-1-0 \
    libgif7 \
    libjpeg62-turbo \
    libtiff6 \
    fonts-noto-core \
    fonts-noto-cjk \
    fonts-noto-ui-extra \
    fonts-noto-color-emoji \
    fonts-noto-extra \
    && curl -LO https://www.princexml.com/download/prince_16.2-1_debian12_amd64.deb \
    && apt-get install -y ./prince_16.2-1_debian12_amd64.deb \
    && rm prince_16.2-1_debian12_amd64.deb \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/pdf-engine /usr/local/bin/pdf-engine

CMD ["pdf-engine"]