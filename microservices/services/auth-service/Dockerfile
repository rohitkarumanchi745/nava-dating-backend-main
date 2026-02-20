FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY services/auth-service ./services/auth-service

RUN cargo build --release --package auth-service

FROM alpine:3.19

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /app/target/release/auth-service ./auth-service

EXPOSE 8001

CMD ["./auth-service"]
