# syntax=docker/dockerfile:1.6
FROM rust:1.76-slim AS builder

WORKDIR /app

# Install build dependencies
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates \
       pkg-config \
       libssl-dev \
       curl \
    && rm -rf /var/lib/apt/lists/*

# Copy manifest files first for better caching
COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./

# Create dummy src to cache dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release 2>/dev/null || true
RUN rm -rf src

# Copy actual source
COPY src ./src
COPY migrations ./migrations
COPY .sqlx ./.sqlx

# Build release binary with optimizations
# Touch a file to force rebuild with actual source
ENV SQLX_OFFLINE=true
RUN touch src/main.rs && cargo build --release && \
    strip target/release/telugu-dating-backend

FROM debian:bookworm-slim AS runtime

# Install runtime dependencies including curl for healthchecks and ffmpeg for HLS transcoding
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
       ca-certificates \
       libssl3 \
       curl \
       ffmpeg \
    && rm -rf /var/lib/apt/lists/* \
    && adduser --disabled-password --gecos "" --uid 1000 appuser

WORKDIR /app

# Copy binary from builder
COPY --from=builder /app/target/release/telugu-dating-backend /app/telugu-dating-backend

# Copy migrations for runtime access
COPY --from=builder /app/migrations /app/migrations

# Create directories for uploads and models
RUN mkdir -p /app/uploads /app/models && chown -R appuser:appuser /app

# Environment defaults (production-ready for 10k+ users)
ENV BIND_ADDR=0.0.0.0:8080 \
    RUST_LOG=info,telugu_dating_backend=info,sqlx=warn \
    UPLOAD_DIR=/app/uploads \
    VISION_MODEL_DIR=/app/models \
    ENVIRONMENT=production \
    DB_MAX_CONNECTIONS=25 \
    DB_MIN_CONNECTIONS=5 \
    DB_ACQUIRE_TIMEOUT_SECS=10 \
    PGBOUNCER_MODE=true \
    RATE_LIMIT_RPM=120 \
    RATE_LIMIT_BURST=30 \
    WS_CHAT_BUFFER_SIZE=500 \
    WS_CALL_BUFFER_SIZE=200 \
    REQUEST_TIMEOUT_SECS=30 \
    SHUTDOWN_TIMEOUT_SECS=30

# Expose port
EXPOSE 8080

# Health check (K8s handles probes, but useful for local testing)
# In K8s, disable with: docker run --health-cmd=none
HEALTHCHECK --interval=30s --timeout=10s --start-period=10s --retries=3 \
    CMD curl -sf http://localhost:8080/ready || exit 1

# Run as non-root user
USER appuser

# Use exec form for proper signal handling
CMD ["/app/telugu-dating-backend"]
