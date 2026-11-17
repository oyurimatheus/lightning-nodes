# --- Build stage: compile the release binary with the pinned toolchain ---
FROM rust:1.94-slim AS builder
WORKDIR /build

# Cache dependencies separately from sources for faster rebuilds.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs && cargo build --release && rm -rf src

COPY src ./src
COPY migrations ./migrations
RUN touch src/main.rs && cargo build --release

# --- Runtime stage: minimal image, non-root user, CA certificates ---
FROM debian:12-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --no-create-home --uid 10001 nodes
COPY --from=builder /build/target/release/nodes-svc /usr/local/bin/nodes-svc
USER nodes
EXPOSE 3000
ENV RUST_LOG=info \
    SERVER_HOST=0.0.0.0 \
    SERVER_PORT=3000

# Handles SIGTERM (graceful shutdown) via the application's signal handling.
ENTRYPOINT ["/usr/local/bin/nodes-svc"]
