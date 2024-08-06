# Stage 1: Builder
FROM rust:alpine as builder

WORKDIR /
COPY . .

# Install build dependencies
RUN apk add --no-cache musl-dev

RUN cargo build --release

FROM scratch

COPY --from=builder /target/release/resolve-aws-secrets /resolve-aws-secrets
