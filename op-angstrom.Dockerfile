# syntax=docker.io/docker/dockerfile:1.7-labs

ARG TARGETOS=linux
ARG TARGETARCH=x86_64
ARG FEATURES=""
ARG BUILD_PROFILE=release


FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef

LABEL org.opencontainers.image.source=https://github.com/SorellaLabs/angstrom

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    libclang-dev \
    pkg-config \
    cmake \
    && rm -rf /var/lib/apt/lists/*

# Stage 2: Foundry Image Setup
FROM ghcr.io/foundry-rs/foundry:stable AS foundry
WORKDIR /app
# Only copy what's needed for foundry
COPY contracts/ ./contracts/

# Stage 3: Prepare Recipe with Cargo Chef
FROM chef AS planner
WORKDIR /app
# Copy everything for dependency analysis - cargo chef only reads Cargo.toml files anyway
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Stage 4: Build Stage
FROM chef AS builder
WORKDIR /app

COPY --from=foundry /usr/local/bin/forge /usr/local/bin/forge
COPY --from=foundry /usr/local/bin/cast /usr/local/bin/cast
COPY --from=foundry /usr/local/bin/anvil /usr/local/bin/anvil
COPY --from=planner /app/recipe.json recipe.json

ARG FEATURES=""
ARG BUILD_PROFILE=release

ENV FEATURES=$FEATURES
ENV BUILD_PROFILE=$BUILD_PROFILE
ENV PATH="/usr/local/bin:${PATH}"

RUN cargo chef cook --profile $BUILD_PROFILE --features "$FEATURES" --recipe-path recipe.json
# Copy source code only after dependencies are built
COPY bin/ ./bin/
COPY crates/ ./crates/
COPY testing-tools/ ./testing-tools/
COPY contracts/ ./contracts/
RUN cargo build --profile $BUILD_PROFILE --features "$FEATURES" --locked --bin op-angstrom --manifest-path /app/bin/op-angstrom/Cargo.toml

FROM ubuntu:22.04 AS runtime

ARG BUILD_PROFILE=release

WORKDIR /app
COPY --from=builder /app/target/$BUILD_PROFILE/op-angstrom /app/op-angstrom
COPY etc/op-angstrom-entrypoint.sh /app/op-angstrom-entrypoint.sh
RUN chmod +x /app/op-angstrom /app/op-angstrom-entrypoint.sh

EXPOSE 30303 30303/udp 9001 8545 8546 