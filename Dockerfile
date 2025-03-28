# syntax=docker/dockerfile:1
FROM rust:alpine3.21 AS chef

WORKDIR /usr/src/project

RUN set -eux; \
    apk add --no-cache musl-dev openssl-dev openssl-libs-static pkgconfig; \
    cargo install cargo-chef; \
    rm -rf $CARGO_HOME/registry

FROM chef as planner

COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder

COPY --from=planner /usr/src/project/recipe.json .
RUN cargo chef cook --release --recipe-path recipe.json

COPY . .
RUN cargo build --release

FROM alpine:3.21

WORKDIR /usr/local/bin

# Install runtime dependencies
RUN apk add --no-cache openssl libgcc

COPY --from=builder /usr/src/project/target/release/urx .

CMD ["./urx"]