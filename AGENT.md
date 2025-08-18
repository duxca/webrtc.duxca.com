# Project Analysis: webrtc.duxca.com

This document provides a summary of the `webrtc.duxca.com` project.

## High-Level Summary

This project is a **Rust-based web server** that provides backend services for a WebRTC application. It includes a robust authentication system using **GitHub OAuth2** and a simple API for client-side interaction. The server is built with the `axum` web framework and is designed to be containerized using Docker.

## Technical Stack

- **Language**: Rust (2021 Edition)
- **Web Framework**: `axum`
- **Asynchronous Runtime**: `tokio`
- **Authentication**: `axum-login` for session-based authentication, with `oauth2` for the GitHub OAuth2 flow.
- **Configuration**: Environment variables, loaded with `dotenvy` and parsed with `envy`.
- **Deployment**: `Dockerfile` is present, indicating container-based deployment. `Makefile` and shell scripts (`deploy.bash`, `run_local.bash`) are available for automation.
- **CI/CD**: A GitHub Actions workflow (`.github/workflows/rust.yml`) is set up for continuous integration.

## Architecture

The application is structured into three main Rust modules: `main.rs`, `auth.rs`, and `web.rs`.

### `main.rs` - Application Entrypoint

- Initializes logging (`tracing`), configuration, and the database.
- Sets up the `axum` router, defining all the application's routes.
- Applies middleware for sessions, Cross-Origin Resource Sharing (CORS), tracing, and compression.
- Serves a static frontend from the `dist/` directory.
- Binds to a TCP listener and starts the server.

### `auth.rs` - Authentication Module

- Implements the full GitHub OAuth2 login flow (`/login`, `/oauth/callback`).
- Manages user sessions and CSRF protection.
- Defines the `User` model and the `AuthnBackend` trait implementation for `axum-login`.
- Fetches user information from the GitHub API upon successful authentication.
- Uses an in-memory `HashMap` as a stand-in for a user database.

### `web.rs` - Web API Module

- Defines the core API logic at the `/api` endpoint.
- Uses a JSON-RPC-style approach where a single endpoint handles multiple actions based on a `type` field in the request body.
- Supported API actions include:
    - `GetMe`: Fetches the current authenticated user's ID.
    - `SetValue`, `GetValue`, `DeleteValue`: Implements a simple, in-memory key-value store.
- Requires authentication for all API actions.

## How It Works

1.  A user accesses the frontend application, which is served from the `dist/` directory.
2.  To perform any action, the user must log in. They are redirected to GitHub to authorize the application.
3.  Upon successful authorization, the server creates a session for the user.
4.  The authenticated frontend can then make requests to the `/api` endpoint to interact with the backend services (e.g., get user info, store data in the key-value store).
5.  The project name suggests that the key-value store is likely used for signaling in a WebRTC context (e.g., exchanging SDP offers/answers and ICE candidates).
