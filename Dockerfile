# syntax=docker.io/docker/dockerfile:1.7-labs

ARG TARGETOS=linux
ARG TARGETARCH=x86_64
ARG FEATURES=""
ARG BUILD_PROFILE=release

LABEL org.opencontainers.image.source=https://github.com/SorellaLabs/angstrom

FROM lukemathwalker/cargo-chef:latest-rust-1 AS chef
WORKDIR /app

RUN apt-get update && apt-get -y upgrade && apt-get install -y libclang-dev pkg-config cmake libclang-dev curl

# Stage 2: Foundry Image Setup
FROM ghcr.io/foundry-rs/foundry:stable AS foundry
WORKDIR /app
COPY --exclude=.git --exclude=dist . .

# Stage 3: Prepare Recipe with Cargo Chef
FROM chef AS planner
WORKDIR /app
COPY --exclude=.git --exclude=dist . .
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

RUN cargo chef cook --profile $BUILD_PROFILE --features "$FEATURES" --recipe-path recipe.json
COPY --exclude=.git --exclude=dist . .
RUN cargo build --profile $BUILD_PROFILE --features "$FEATURES" --locked --bin angstrom --manifest-path /app/bin/angstrom/Cargo.toml

FROM ubuntu:22.04 AS runtime
ARG FEATURES=""
ARG BUILD_PROFILE=release

ENV FEATURES=$FEATURES
ENV BUILD_PROFILE=$BUILD_PROFILE
WORKDIR /app
COPY --from=builder /app/target/$BUILD_PROFILE/angstrom /app/angstrom
RUN chmod +x /app/angstrom

EXPOSE 30303 30303/udp 9001 8545 8546 
ENTRYPOINT ["/app/angstrom"]
