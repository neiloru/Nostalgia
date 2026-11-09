# syntax=docker/dockerfile:1
# Nostalgia server image. Build from the repository root:
#   docker build -f server/Dockerfile -t nostalgia-server .
# The admin UI (server/admin) is embedded into the binary.

# Compiled on the build machine for every target architecture (no emulation, which is very slow
# for Rust): amd64 and arm64 with Debian's cross compilers.
FROM --platform=$BUILDPLATFORM rust:1-slim-bookworm AS build
ARG TARGETARCH BUILDARCH
RUN set -eux; \
    case "$TARGETARCH" in \
      amd64) triple=x86_64-unknown-linux-gnu; gcc=x86_64-linux-gnu; pkg=x86-64-linux-gnu ;; \
      arm64) triple=aarch64-unknown-linux-gnu; gcc=aarch64-linux-gnu; pkg=aarch64-linux-gnu ;; \
      *) echo "unsupported architecture $TARGETARCH" >&2; exit 1 ;; \
    esac; \
    if [ "$TARGETARCH" != "$BUILDARCH" ]; then \
      apt-get update; \
      apt-get install -y --no-install-recommends gcc-$pkg libc6-dev-$TARGETARCH-cross; \
      rm -rf /var/lib/apt/lists/*; \
      rustup target add $triple; \
      echo $gcc-gcc > /cc; \
    else \
      echo gcc > /cc; \
    fi; \
    echo $triple > /triple
WORKDIR /src
# Cargo needs every workspace member's manifest and targets; only the server is compiled.
COPY Cargo.toml Cargo.lock ./
COPY shared shared
COPY server server
COPY client/Cargo.toml client/Cargo.toml
COPY client/src client/src
COPY client/assets client/assets
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/target,id=nostalgia-target-$TARGETARCH \
    set -eux; \
    triple=$(cat /triple); cc=$(cat /cc); \
    export "CARGO_TARGET_$(echo $triple | tr 'a-z-' 'A-Z_')_LINKER=$cc" "CC_$(echo $triple | tr '-' '_')=$cc"; \
    cargo build --release --locked --target $triple -p nostalgia-server; \
    cp target/$triple/release/nostalgia-server /nostalgia-server

FROM debian:bookworm-slim
LABEL org.opencontainers.image.title="Nostalgia server" \
      org.opencontainers.image.description="Self-hosted retro game library: games, emulators, users and versioned cloud saves" \
      org.opencontainers.image.source="https://github.com/neiloru/Nostalgia" \
      org.opencontainers.image.licenses="MIT"
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl && rm -rf /var/lib/apt/lists/*
WORKDIR /app
ENV NOSTALGIA_SETTINGS=/config/settings.yaml \
    NOSTALGIA_LIBRARY=/library \
    NOSTALGIA_DATABASE=/database \
    NOSTALGIA_SAVES=/saves
COPY --from=build /nostalgia-server /app/nostalgia-server
COPY server/settings.example.yaml LICENSE ./
# settings.yaml, the game library, the database and the cloud saves are separate volumes.
VOLUME ["/config", "/library", "/database", "/saves"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD curl -fs "http://127.0.0.1:${NOSTALGIA_PORT:-8080}/api/health" || exit 1
CMD ["/app/nostalgia-server"]
