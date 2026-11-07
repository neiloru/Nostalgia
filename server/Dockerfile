# Build from the repository root:  docker build -f server/Dockerfile -t nostalgia-server .
FROM node:24-slim AS build
WORKDIR /src
COPY package.json package-lock.json ./
COPY shared/package.json shared/
COPY server/package.json server/
COPY client/package.json client/
RUN npm ci --workspace server
COPY shared shared
COPY server server
RUN npm run build -w server

FROM node:24-slim
WORKDIR /app
ENV NODE_ENV=production
COPY --from=build /src/server/dist ./dist
RUN npm init -y >/dev/null && npm install --omit=dev express@5 yaml@2 && npm cache clean --force
# /config/settings.yaml, the game library and the data folder (database + cloud saves) are volumes.
VOLUME ["/config", "/library", "/data"]
ENV NOSTALGIA_SETTINGS=/config/settings.yaml NOSTALGIA_LIBRARY=/library NOSTALGIA_DATA=/data
EXPOSE 8080
CMD ["node", "dist/server.js"]
