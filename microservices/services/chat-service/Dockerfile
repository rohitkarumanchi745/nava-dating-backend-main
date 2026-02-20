FROM rust:1.75-alpine AS builder

RUN apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig

WORKDIR /app

COPY Cargo.toml Cargo.lock ./
COPY shared ./shared
COPY services/chat-service ./services/chat-service

RUN cargo build --release --package chat-service

FROM alpine:3.19

RUN apk add --no-cache ca-certificates

WORKDIR /app

COPY --from=builder /app/target/release/chat-service ./chat-service

EXPOSE 8004

CMD ["./chat-service"]
