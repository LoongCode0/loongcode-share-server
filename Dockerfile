# ---- web 构建 ----
FROM node:22-alpine AS web
WORKDIR /app/web
RUN corepack enable
COPY web/package.json web/pnpm-lock.yaml web/pnpm-workspace.yaml ./
RUN pnpm install --frozen-lockfile
COPY web/ ./
RUN pnpm build

# ---- rust 构建 ----
FROM rust:1.85-slim AS build
WORKDIR /app
COPY Cargo.toml ./
COPY Cargo.lock ./
COPY src ./src
RUN cargo build --release

# ---- 运行 ----
FROM debian:bookworm-slim
WORKDIR /srv
COPY --from=build /app/target/release/share-server /usr/local/bin/share-server
COPY --from=web /app/web/dist /srv/web/dist
ENV SHARE_DB_PATH=/data/shares.db \
    SHARE_WEB_DIR=/srv/web/dist \
    SHARE_LISTEN=0.0.0.0:8787
VOLUME /data
EXPOSE 8787
CMD ["share-server"]
