# syntax=docker/dockerfile:1@sha256:4edf897a3ffa55b89f906fc8cc78afdb3f1834cc9c7083565e611a8a7d5fe99e
# Base images are pinned by digest; Dependabot (docker) proposes updates.
FROM rust:1.96.0-trixie@sha256:58fe97504a0e4cbba5d85599619a589923d3e779472a6fb0840d58d1c4ba99d7 AS builder

WORKDIR /app

ENV CARGO_HOME=/var/cache/cargo

COPY . .

RUN \
  --mount=type=cache,target=/var/cache/cargo \
  cargo fetch --locked

RUN \
  --mount=type=cache,target=/var/cache/cargo \
  cargo build --offline --locked --release

FROM debian:trixie-slim@sha256:a72b4433de1cf7d6bae0c4cee74d0547f58601a643b3375f070c51600d63b6b2

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

USER 65532:65532
EXPOSE 8080
CMD ["/app/server"]
