# syntax=docker/dockerfile:1

# =============================================================================
# Stage 1: Build React web client
# =============================================================================
FROM node:22-alpine AS webbuilder

WORKDIR /build

COPY webclientsrc/package*.json ./
RUN npm ci

COPY webclientsrc/ ./
RUN npm run build
RUN npm run test

# =============================================================================
# Stage 2: Build Rust daemon (web client must be present at compile time
# because include_dir! embeds it statically)
# =============================================================================
FROM rust:1.95 AS rustbuilder

WORKDIR /build

# Git is required at build time (build.rs probes ../webclientsrc/dist) and is also
# the runtime dependency of the daemon.
RUN apt-get update \
    && apt-get install -y git \
    && rm -rf /var/lib/apt/lists/* \
    && git config --global user.name "GitDiverge" \
    && git config --global user.email "gitdiverge@localhost"

# Copy workspace manifest and member crates
COPY Cargo.toml Cargo.lock ./
COPY gitdiverge.toml.example ./
COPY gitdiverge-lib/ ./gitdiverge-lib/
COPY gitdiverge/ ./gitdiverge/

# Place the compiled web client where the Rust crate expects it:
# $CARGO_MANIFEST_DIR for gitdiverge is /build/gitdiverge,
# so include_dir!("$CARGO_MANIFEST_DIR/../webclientsrc/dist") resolves to /build/webclientsrc/dist.
COPY --from=webbuilder /build/dist/ ./webclientsrc/dist/

RUN cargo test --workspace
RUN cargo build --profile extreme
#RUN cargo build --profile release

# =============================================================================
# Stage 3: Runtime image
# =============================================================================
FROM ubuntu:latest

# Install git (mandatory runtime dependency) and ca-certificates for TLS.
RUN apt-get update \
    && apt-get install -y git ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && git --version

COPY --from=rustbuilder /build/target/extreme/gitdiverge /usr/local/bin/gitdiverge

WORKDIR /app
RUN mkdir -p /app/repos

EXPOSE 8080

ENTRYPOINT ["gitdiverge"]
CMD ["daemon", "--bind", "0.0.0.0", "--port", "8080", "--clone-dir", "/app/repos", "-vv"]
