# ---- deps (caché de dependencias) ----
FROM rust:1.98-slim AS deps
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src

# ---- builder ----
FROM rust:1.98-slim AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends pkg-config libssl-dev \
    && rm -rf /var/lib/apt/lists/*
COPY --from=deps /app/target /app/target
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# ---- runtime mínimo, no-root, fs de solo lectura ----
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && groupadd -r --gid 10000 appuser && useradd -r --uid 10000 -g appuser appuser
WORKDIR /app
COPY --from=builder /app/target/release/occb-backend /app/occb-backend
RUN chown appuser:appuser /app/occb-backend && chmod 555 /app/occb-backend
USER 10000:10000
EXPOSE 8000
ENV RUST_LOG=info
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -fsS http://127.0.0.1:8000/api/v1/health > /dev/null || exit 1
ENTRYPOINT ["/app/occb-backend"]
