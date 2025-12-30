# syntax=docker/dockerfile:1.6
FROM rust:1.76-slim AS builder

WORKDIR /app
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY rust-toolchain.toml Cargo.toml Cargo.lock* ./
COPY src ./src

RUN cargo build --release --locked

FROM debian:bookworm-slim AS runtime

RUN adduser --disabled-password --gecos "" appuser

WORKDIR /app
COPY --from=builder /app/target/release/telugu-dating-backend /app/telugu-dating-backend

ENV BIND_ADDR=0.0.0.0:8080
ENV RUST_LOG=info

EXPOSE 8080

USER appuser
CMD ["/app/telugu-dating-backend"]
