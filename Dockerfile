FROM node:24.19.0-alpine3.23 AS web-build
WORKDIR /src/web
COPY web/package.json web/package-lock.json ./
RUN npm ci
COPY web/ ./
RUN npm run build

FROM rust:1.98.1-bookworm AS api-build
WORKDIR /src
COPY Cargo.toml Cargo.lock ./
COPY api/Cargo.toml api/Cargo.toml
COPY api/migrations api/migrations
COPY api/src api/src
RUN cargo build --locked --release --package lumeno-api

FROM debian:bookworm-slim AS runtime
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd --system --uid 10001 --create-home --home-dir /app lumeno
WORKDIR /app
COPY --from=api-build /src/target/release/lumeno-api /usr/local/bin/lumeno-api
COPY --from=web-build /src/web/dist /app/web/dist
COPY LICENSE /app/LICENSE
USER lumeno
ENV LUMENO_BIND=0.0.0.0:3000 \
    LUMENO_STATIC_DIR=/app/web/dist \
    RUST_LOG=lumeno_api=info,tower_http=info
EXPOSE 3000
HEALTHCHECK --interval=20s --timeout=4s --start-period=15s --retries=5 CMD ["curl", "--fail", "--silent", "http://127.0.0.1:3000/health/ready"]
ENTRYPOINT ["lumeno-api"]
