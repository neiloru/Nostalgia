# syntax=docker/dockerfile:1
# Build from the repository root:  docker build -f server/Dockerfile -t nostalgia-server .
# Prebuilt images: ghcr.io/neiloru/nostalgia-server (see .github/workflows/docker.yml)

# The bundle is plain JavaScript, so it is built once on the build machine's platform
# and copied into every target architecture.
FROM --platform=$BUILDPLATFORM node:24-slim AS build
WORKDIR /src
COPY package.json package-lock.json tsconfig.base.json ./
COPY shared/package.json shared/
COPY server/package.json server/
COPY client/package.json client/
RUN npm ci --workspace server
COPY shared shared
COPY server server
RUN npm run build -w server

FROM node:24-slim
LABEL org.opencontainers.image.title="Nostalgia server" \
      org.opencontainers.image.description="Self-hosted retro game library: games, emulators, users and versioned cloud saves" \
      org.opencontainers.image.source="https://github.com/neiloru/Nostalgia" \
      org.opencontainers.image.licenses="MIT"
WORKDIR /app
ENV NODE_ENV=production \
    NOSTALGIA_SETTINGS=/config/settings.yaml \
    NOSTALGIA_LIBRARY=/library \
    NOSTALGIA_DATA=/data
# server.js is fully bundled: no node_modules needed.
COPY --from=build /src/server/dist/ ./
COPY server/settings.example.yaml LICENSE ./
# /config/settings.yaml, the game library and the data folder (database + cloud saves) are volumes.
VOLUME ["/config", "/library", "/data"]
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=30s --retries=3 \
  CMD ["node", "-e", "fetch('http://127.0.0.1:' + (process.env.NOSTALGIA_PORT || 8080) + '/api/health').then((r) => process.exit(r.ok ? 0 : 1), () => process.exit(1))"]
CMD ["node", "server.js"]
