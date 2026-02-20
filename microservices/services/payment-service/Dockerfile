FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY services/payment-service ./services/payment-service

RUN cargo build --release --package payment-service

FROM alpine:3.19

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /app/target/release/payment-service ./payment-service

EXPOSE 8005

CMD ["./payment-service"]
