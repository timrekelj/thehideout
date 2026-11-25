# syntax=docker/dockerfile:1.7
#
# thehideout server image. The build context is the repository root (the server
# build runs `buf build` over ../proto, see server/build.rs):
#
#   docker build -t thehideout-server .
#
# Stages: chef (planner) -> builder (cached dependency layer, then the app);
# web-planner -> web-builder (the web client bundle, precompressed) -> runtime.

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

# The web client (client/web/build.sh): a wasm32 build of the desktop app, served by the server at /app/.
FROM chef AS web-planner
COPY client ./client
# client/desktop and client/api have the server as a dev-dependency (path ../../server): cargo
# must be able to read its manifest, though nothing of it is built here.
COPY server/Cargo.toml ./server/Cargo.toml
RUN mkdir -p server/src && touch server/src/lib.rs server/src/main.rs
WORKDIR /src/client
RUN cargo chef prepare --recipe-path /web-recipe.json

FROM chef AS web-builder
# Must match wasm-bindgen in client/Cargo.lock (build.sh checks and says so if it doesn't).
ARG WASM_BINDGEN_VERSION=0.2.130
ARG BINARYEN_VERSION=133
# python3: build.sh finds rune's font files with it; brotli: precompression. wasm-opt comes from a
# binaryen release (Debian's is too old for the wasm Rust emits today).
RUN apt-get update \
    && apt-get install -y --no-install-recommends python3 brotli \
    && rm -rf /var/lib/apt/lists/* \
    && arch="$(uname -m)" \
    && curl -fsSL "https://github.com/WebAssembly/binaryen/releases/download/version_${BINARYEN_VERSION}/binaryen-version_${BINARYEN_VERSION}-${arch}-linux.tar.gz" \
        | tar -xz -C /opt \
    && ln -s /opt/binaryen-version_${BINARYEN_VERSION}/bin/wasm-opt /usr/local/bin/wasm-opt
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    cargo install wasm-bindgen-cli --locked --version ${WASM_BINDGEN_VERSION}
COPY buf.yaml ./buf.yaml
COPY proto ./proto
COPY server/Cargo.toml ./server/Cargo.toml
RUN mkdir -p server/src && touch server/src/lib.rs server/src/main.rs
WORKDIR /src/client
# The pinned toolchain (client/rust-toolchain.toml) gets the wasm target.
COPY client/rust-toolchain.toml ./rust-toolchain.toml
RUN rustup target add wasm32-unknown-unknown
COPY client/web/build.sh ./web/build.sh
COPY --from=web-planner /web-recipe.json /web-recipe.json
# Dependencies first, as in the server build: rebuilt only when a Cargo.toml / Cargo.lock changes.
COPY client/api/build.rs ./api/build.rs
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    web/build.sh --locked --cook /web-recipe.json
COPY client ./
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    web/build.sh --locked --out /web \
    # The server (ServeDir) serves `x.br` / `x.gz` beside `x` when the browser accepts them.
    && find /web -type f \( -name '*.js' -o -name '*.wasm' -o -name '*.html' -o -name '*.ttf' \) \
        -exec gzip -9 -k {} \; -exec brotli -q 11 -k {} \;

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl tini \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --no-create-home --shell /usr/sbin/nologin hideout
COPY --from=builder /src/server/target/release/thehideout-server /usr/local/bin/thehideout-server
COPY --from=web-builder /web /usr/share/thehideout/web
USER hideout
WORKDIR /home/hideout
ENV THEHIDEOUT_LISTEN_ADDR=0.0.0.0:8080 \
    THEHIDEOUT_WEB_DIR=/usr/share/thehideout/web \
    RUST_LOG=info
EXPOSE 8080
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=15s --timeout=5s --start-period=30s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8080/health || exit 1
# tini reaps zombies and forwards SIGTERM to the server (graceful drain).
ENTRYPOINT ["/usr/bin/tini", "--", "thehideout-server"]
