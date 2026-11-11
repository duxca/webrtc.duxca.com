# Project Analysis: webrtc.duxca.com

This document provides a summary of the `webrtc.duxca.com` project.

## High-Level Summary

This project is a **Rust-based web server** that provides backend services for a WebRTC application. It includes a robust authentication system using **GitHub OAuth2** and a simple API for client-side interaction. The server is built with the `axum` web framework and is designed to be containerized using Docker.

## Technical Stack

- **Language**: Rust (2021 Edition)
- **Web Framework**: `axum`
- **Asynchronous Runtime**: `tokio`
- **Authentication**: `axum-login` for session-based authentication, with `oauth2` for the GitHub OAuth2 flow.
- **Configuration**: Environment variables (`HOST_ADDR`, `GITHUB_CLIENT_ID`, `GITHUB_CLIENT_SECRET`, `REDIRECT_URL`, optional `ALLOWED_GITHUB_IDS`), loaded with `dotenvy` and parsed with `envy`.
- **Deployment**: `Dockerfile` is present, indicating container-based deployment. `Makefile` and shell scripts (`deploy.bash`, `run_local.bash`) are available for automation.
- **CI/CD**: `.github/workflows/check.yml` runs on pull requests; `.github/workflows/deploy.yml` deploys `main` to Cloud Run.

## Architecture

- `main.rs`: reads configuration from the environment and starts the server.
- `app.rs`: builds the `axum` router, session layer (signed `__Host-` cookie, bounded in-memory store) and shared state. `Settings` holds every limit.
- `auth.rs`: GitHub OAuth2 with PKCE (`/login`, `/oauth/callback`, `/logout`). The pending login (state + PKCE verifier) is kept in a signed cookie, not in the session store. Optional `ALLOWED_GITHUB_IDS` allowlist. Signing out rotates the user's session secret, which ends all of that user's sessions.
- `web.rs`: `/api`, a JSON-RPC-style endpoint (`GetMe`, `SetValue`, `GetValue`, `DeleteValue`). Authentication and per-user rate limiting run before the body is read.
- `signaling.rs`: the signaling store. Keys are `(user id, slot)` with `slot` in `sdp-req` / `sdp-res`, so a client can only reach its own data. Values are size-limited and expire.
- `security.rs`: same-origin check for state-changing requests and security response headers (CSP etc.). There is no CORS layer.
- `session_store.rs`, `rate_limit.rs`, `error.rs`: bounded session store, token-bucket limiter, opaque 500 responses.
- `tests.rs`: end-to-end tests against a fake GitHub OAuth server.

## How It Works

1.  A user accesses the frontend application, which is served from the `dist/` directory.
2.  To perform any action, the user must log in. They are redirected to GitHub to authorize the application.
3.  Upon successful authorization, the server creates a session for the user.
4.  The authenticated frontend calls `/api` to exchange SDP offers/answers between two devices of the same user through the `sdp-req` / `sdp-res` slots.
