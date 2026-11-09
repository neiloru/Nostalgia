# syntax=docker/dockerfile:1
# Rust server image. Build from the repository root (the admin UI in server/admin is embedded):
#   docker build -f rust/Dockerfile -t nostalgia-server-rs .
# Same volumes, environment variables and port as the Node.js image (server/Dockerfile).

FROM rust:1-slim AS build
WORKDIR /src
COPY server/admin server/admin
COPY rust rust
WORKDIR /src/rust
RUN cargo build --release -p nostalgia-server

FROM debian:stable-slim
LABEL org.opencontainers.image.title="Nostalgia server (Rust)" \
      org.opencontainers.image.source="https://github.com/neiloru/Nostalgia" \
      org.opencontainers.image.licenses="MIT"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
ENV NOSTALGIA_SETTINGS=/config/settings.yaml \
    NOSTALGIA_LIBRARY=/library \
    NOSTALGIA_DATABASE=/database \
    NOSTALGIA_SAVES=/saves
COPY --from=build /src/rust/target/release/nostalgia-server /app/nostalgia-server
COPY server/settings.example.yaml LICENSE ./
VOLUME ["/config", "/library", "/database", "/saves"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -fs "http://127.0.0.1:${NOSTALGIA_PORT:-8080}/api/health" || exit 1
CMD ["/app/nostalgia-server"]
