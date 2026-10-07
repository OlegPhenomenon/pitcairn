# syntax=docker/dockerfile:1

# ---- Stage 1: frontend build ----
FROM node:22-bookworm-slim AS web
WORKDIR /web
COPY frontend/package.json frontend/package-lock.json ./
RUN npm ci
COPY frontend/ ./
RUN npm run build

# ---- Stage 2: backend build ----
FROM rust:1.98.0-slim-bookworm AS build
RUN apt-get update && apt-get install -y --no-install-recommends \
    build-essential pkg-config ca-certificates \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /app
COPY backend/Cargo.toml backend/Cargo.lock backend/rust-toolchain.toml ./
COPY backend/migrations ./migrations
COPY backend/src ./src
RUN cargo build --locked --release \
    && strip target/release/pitcairn

# ---- Stage 3: runtime ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd --system --gid 10001 pitcairn \
    && useradd --system --uid 10001 --gid pitcairn \
       --home-dir /var/lib/pitcairn --create-home --shell /usr/sbin/nologin pitcairn
COPY --from=build /app/target/release/pitcairn /usr/local/bin/pitcairn
COPY --from=web /web/dist /app/static
RUN chown -R pitcairn:pitcairn /var/lib/pitcairn /app/static
USER pitcairn
ENV PITCAIRN_BIND=0.0.0.0:8080 \
    PITCAIRN_DATA_DIR=/var/lib/pitcairn \
    PITCAIRN_STATIC_DIR=/app/static
VOLUME ["/var/lib/pitcairn"]
EXPOSE 8080
ENTRYPOINT ["/usr/local/bin/pitcairn"]
CMD ["serve"]
