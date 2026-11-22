# syntax=docker/dockerfile:1.7
#
# thehideout server image. The build context is the repository root (the server
# build runs `buf build` over ../proto, see server/build.rs):
#
#   docker build -t thehideout-server .
#
# Stages: chef (planner) -> builder (cached dependency layer, then the app) -> runtime.

ARG RUST_VERSION=1.99
ARG BUF_VERSION=1.50.0

FROM bufbuild/buf:${BUF_VERSION} AS buf

FROM rust:${RUST_VERSION}-bookworm AS chef
# aws-lc-sys (rustls) needs cmake and a C toolchain.
RUN apt-get update \
    && apt-get install -y --no-install-recommends cmake \
    && rm -rf /var/lib/apt/lists/* \
    && cargo install cargo-chef --locked --version ^0.1
COPY --from=buf /usr/local/bin/buf /usr/local/bin/buf
WORKDIR /src

FROM chef AS planner
COPY server ./server
WORKDIR /src/server
RUN cargo chef prepare --recipe-path /recipe.json

FROM chef AS builder
# Dependencies first: this layer is rebuilt only when Cargo.toml / Cargo.lock change.
COPY --from=planner /recipe.json /recipe.json
COPY buf.yaml ./buf.yaml
COPY proto ./proto
COPY server/build.rs ./server/build.rs
WORKDIR /src/server
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo chef cook --release --locked --recipe-path /recipe.json
COPY server ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo build --release --locked --bin thehideout-server \
    && strip target/release/thehideout-server

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin hideout
COPY --from=builder /src/server/target/release/thehideout-server /usr/local/bin/thehideout-server
USER hideout
WORKDIR /home/hideout
ENV THEHIDEOUT_LISTEN_ADDR=0.0.0.0:8080 \
    RUST_LOG=info
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1
# tini reaps zombies and forwards SIGTERM to the server (graceful drain).
ENTRYPOINT ["/usr/bin/tini", "--", "thehideout-server"]
