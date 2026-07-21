# syntax=docker/dockerfile:1

# ---- build stage: produce a fully static musl binary -------------------------
FROM rust:1-alpine AS builder
# build-base (gcc + musl-dev + make) is needed to compile the `ring` C
# sources used by the TLS stack.
RUN apk add --no-cache build-base
WORKDIR /app
COPY . .
# rust:alpine targets x86_64-unknown-linux-musl by default → static binary.
RUN cargo build --release --locked --bin modelx \
    && cp target/release/modelx /modelx

# ---- runtime stage: tiny Alpine image running `modelx api` -------------------
FROM alpine:3.20
# CA certificates for HTTPS to models.dev and the Hugging Face datasets-server.
RUN apk add --no-cache ca-certificates \
    && adduser -D -u 10001 modelx \
    && mkdir -p /data && chown modelx:modelx /data
COPY --from=builder /modelx /usr/local/bin/modelx
USER modelx
# Cache lives in a mountable volume so refreshed data can persist.
ENV XDG_CACHE_HOME=/data
VOLUME ["/data"]
EXPOSE 8080
ENTRYPOINT ["modelx"]
# Default: serve the JSON API on all interfaces, auto-refreshing hourly.
CMD ["api", "--listen-addr", "0.0.0.0", "--listen-port", "8080", "--refresh-interval", "1h"]
