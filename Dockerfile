# syntax=docker/dockerfile:1
FROM rust:1.96.0-trixie AS builder

WORKDIR /app

RUN curl -o /tmp/sccache.tgz -L https://github.com/mozilla/sccache/releases/download/0.2.13/sccache-0.2.13-x86_64-unknown-linux-musl.tar.gz && \
  tar xf /tmp/sccache.tgz -C /tmp && \
  mv /tmp/sccache*/sccache /usr/local/bin && \
  rm -rf /tmp/sccache*

ENV CARGO_HOME=/var/cache/cargo
ENV RUSTC_WRAPPER=/usr/local/bin/sccache
ENV SCCACHE_DIR=/var/cache/sccache

COPY . .

RUN \
  --mount=type=cache,target=/var/cache/cargo \
  --mount=type=cache,target=/var/cache/sccache \
  cargo fetch --locked

RUN \
  # --mount=type=cache,target=./target \
  --mount=type=cache,target=/var/cache/cargo \
  --mount=type=cache,target=/var/cache/sccache \
  cargo build --offline --release

FROM debian:trixie-slim

WORKDIR /app

# RUN \
#   --mount=type=cache,target=/var/lib/apt,sharing=locked \
#   --mount=type=cache,target=/var/cache/apt,sharing=locked \
#   apt-get update && apt-get install -y \
#   ca-certificates openssl \
#   && apt-get clean \
#   && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/server /app/server
COPY --from=builder /app/dist /app/dist

EXPOSE 8080
CMD ["/app/server"]
