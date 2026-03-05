# --- Stage 1: Build the Rust binary ---
FROM rust:1.80-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY . .

# Build for release
RUN cargo build --release

# --- Stage 2: Final Runtime Image ---
FROM debian:bookworm-slim

# 1. Install PrinceXML dependencies and runtime libs
RUN apt-get update && apt-get install -y --no-install-recommends \
    wget \
    ca-certificates \
    libfontconfig1 \
    libpixman-1-0 \
    libxml2 \
    libcurl4 \
    libgif7 \
    libjpeg62-turbo \
    libpng16-16 \
    libtiff6 \
    fonts-liberation \
    openssl \
    && rm -rf /var/lib/apt/lists/*

# 2. Install PrinceXML
RUN wget https://www.princexml.com/download/prince-16.2-linux-generic-x86_64.tar.gz \
    && tar -xzf prince-16.2-linux-generic-x86_64.tar.gz \
    && cd prince-16.2-linux-generic-x86_64 \
    && ./install.sh /usr \
    && cd .. && rm -rf prince-16.2-linux-generic-x86_64*

# 3. Copy the compiled Rust binary from the builder stage
# Replace 'pdf-engine' with the actual binary name from your Cargo.toml
COPY --from=builder /app/target/release/pdf-engine /usr/local/bin/pdf-engine

WORKDIR /data

EXPOSE 6767

# 4. Start the Rust server
ENTRYPOINT ["pdf-engine"]