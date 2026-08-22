# syntax=docker/dockerfile:1

# Build context is the repo root, so `.sqlx/` and the workspace both land here.
# (`docker build -f backend-rs/Dockerfile .`)

FROM rust:1.96.1-bookworm AS chef
WORKDIR /workspace
RUN cargo install cargo-chef --locked --version ^0.1

# Dependencies get their own layer, the way the Gradle image resolved them
# separately: a source edit must not re-download and rebuild ~200 crates.
FROM chef AS planner
COPY backend-rs/ backend-rs/
WORKDIR /workspace/backend-rs
RUN cargo chef prepare --recipe-path /recipe.json

FROM chef AS build
WORKDIR /workspace/backend-rs
COPY --from=planner /recipe.json /recipe.json
RUN cargo chef cook --release --recipe-path /recipe.json
COPY backend-rs/ /workspace/backend-rs/
# The committed query metadata in `.sqlx/`. Without this the `query_as!` macros
# would try to reach a live database at compile time, which no build host has.
# `cargo sqlx prepare --check` in CI is what keeps the metadata honest.
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin umfl-server

# Debian rather than Alpine, deliberately: the ES256 verification the Supabase
# JWT needs pulls in an EC backend that musl makes a fight for no benefit, and
# this image is ~30 MB against the JRE's ~180 MB either way.
FROM debian:bookworm-slim AS runtime
WORKDIR /app

# ca-certificates: the JRE bundled its own trust store, and this process makes
#   TLS connections to Supabase (Postgres) and to the JWKS endpoint.
# wget: the VPS's hand-managed compose file healthchecks with it. Keeping it
#   installed is what makes this a zero-edit deploy on that box.
RUN apt-get update \
 && apt-get install --no-install-recommends -y ca-certificates wget \
 && rm -rf /var/lib/apt/lists/*

RUN groupadd -r umfl && useradd -r -g umfl umfl
COPY --from=build /workspace/backend-rs/target/release/umfl-server /app/umfl-server
USER umfl

EXPOSE 8080
HEALTHCHECK --interval=10s --timeout=5s --start-period=30s --retries=5 \
  CMD wget -qO- http://localhost:8080/actuator/health | grep -q '"status":"UP"' || exit 1

ENTRYPOINT ["/app/umfl-server"]
