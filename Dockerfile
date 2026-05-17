# sacredvote-civic-news — multi-stage Docker build.
#
# Stage 1: build the release binary on rust:alpine (musl-libc, small).
# Stage 2: copy the static binary into a minimal scratch-adjacent image
# and run as a non-root user. Final image is ~2 MB (binary + tini).
#
# Build:
#   docker build -t sacredvote-civic-news:0.6.0 .
#
# Run (loopback-only — DO NOT expose 3005 publicly):
#   docker run --rm -p 127.0.0.1:3005:3005 \
#     -e CIVIC_NEWS_SOURCES="https://example.com/rss" \
#     sacredvote-civic-news:0.6.0
#
# Run with a ratings TOML bind-mounted:
#   docker run --rm -p 127.0.0.1:3005:3005 \
#     -e CIVIC_NEWS_SOURCES="https://a.example/rss" \
#     -e CIVIC_NEWS_RATINGS_TOML=/etc/ratings.toml \
#     -v $(pwd)/deploy/ratings.toml.example:/etc/ratings.toml:ro \
#     sacredvote-civic-news:0.6.0

# ─── Stage 1: build ──────────────────────────────────────────────────
FROM rust:1.83-alpine AS builder

# musl deps for rustls-tls + feed-rs compile. NO openssl needed since
# reqwest is configured for rustls-tls (see Cargo.toml).
RUN apk add --no-cache musl-dev pkgconfig

WORKDIR /build

# Cache deps separately from src: copy manifests first, build a stub,
# then copy real source. Saves a few minutes on incremental rebuilds.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src && \
    echo 'fn main() {}' > src/main.rs && \
    cargo build --release && \
    rm -f target/release/sacredvote-civic-news \
          target/release/deps/sacredvote_civic_news*

# Now copy real source and build for real
COPY src ./src
RUN cargo build --release && \
    strip target/release/sacredvote-civic-news

# ─── Stage 2: runtime ────────────────────────────────────────────────
FROM alpine:3.20 AS runtime

# tini = small init that reaps zombies + forwards signals to the
# Rust process. Otherwise Ctrl-C / docker stop won't shut the sidecar
# down cleanly.
RUN apk add --no-cache tini ca-certificates && \
    addgroup -S civicnews && \
    adduser -S -G civicnews -s /sbin/nologin -H civicnews

COPY --from=builder /build/target/release/sacredvote-civic-news /usr/local/bin/sacredvote-civic-news
RUN chown root:root /usr/local/bin/sacredvote-civic-news && \
    chmod 0755 /usr/local/bin/sacredvote-civic-news

USER civicnews

# Default: bind 0.0.0.0:3005 INSIDE the container. Caller MUST publish
# only to 127.0.0.1 (-p 127.0.0.1:3005:3005) to keep the sidecar
# loopback-only at the host level.
ENV CIVIC_NEWS_BIND=0.0.0.0:3005 \
    RUST_LOG=info

EXPOSE 3005

# Healthcheck — runs every 30s, fails if /health returns non-200.
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 \
  CMD wget -q -O- http://127.0.0.1:3005/health > /dev/null || exit 1

ENTRYPOINT ["/sbin/tini", "--", "/usr/local/bin/sacredvote-civic-news"]
