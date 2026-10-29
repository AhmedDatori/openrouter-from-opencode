FROM rust:1.88-slim-bookworm AS builder
WORKDIR /build
RUN apt-get update && apt-get install -y --no-install-recommends gcc libc6-dev && rm -rf /var/lib/apt/lists/*
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY static ./static
RUN cargo build --release --locked

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* \
    && groupadd --gid 10001 gateway && useradd --uid 10001 --gid gateway --no-create-home gateway
WORKDIR /app
COPY --from=builder /build/target/release/go-router /app/go-router
COPY config/models.json /app/config/models.json
RUN mkdir -p /app/data && chown -R gateway:gateway /app
USER 10001:10001
ENV BIND=0.0.0.0:8080
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 CMD ["/app/go-router", "--healthcheck"]
ENTRYPOINT ["/app/go-router"]
