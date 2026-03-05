# ==========================================
# Stage 1: Build the Rust application
# ==========================================
FROM rust:bookworm AS builder

# Set the working directory inside the container
WORKDIR /app

# Copy your source code and manifests into the container
COPY Cargo.toml Cargo.lock ./
COPY src ./src

# Compile the application in release mode for maximum performance
RUN cargo build --release

# ==========================================
# Stage 2: Create the lightweight runtime image
# ==========================================
FROM debian:bookworm-slim

WORKDIR /app

# Install standard fonts, curl, and PrinceXML 16.2
RUN apt-get update && apt-get install -y \
    curl \
    ca-certificates \
    fontconfig \
    fonts-liberation \
    && curl -LO https://www.princexml.com/download/prince_16.2-1_debian12_amd64.deb \
    && apt-get install -y ./prince_16.2-1_debian12_amd64.deb \
    && rm prince_16.2-1_debian12_amd64.deb \
    && apt-get clean \
    && rm -rf /var/lib/apt/lists/*

# Copy the compiled Rust binary from the builder stage
COPY --from=builder /app/target/release/pdf-engine /usr/local/bin/pdf-engine

# Start the application
CMD ["pdf-engine"]