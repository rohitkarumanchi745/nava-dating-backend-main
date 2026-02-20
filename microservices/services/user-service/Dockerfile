FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY services/user-service ./services/user-service

RUN cargo build --release --package user-service

FROM alpine:3.19

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /app/target/release/user-service ./user-service

EXPOSE 8002

CMD ["./user-service"]
