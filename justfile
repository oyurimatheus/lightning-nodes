# Development task runner. Tool versions themselves are managed by mise.
# Run `mise exec -- just --list` after `mise install`.

default:
    @just --list

# Prepare development tools and environment
setup:
    mise trust
    mise install
    cp -n .env.example .env || true
    @echo "Setup complete. Use 'mise exec -- just <recipe>'."

# Run the application locally (expects PostgreSQL from `just db-up`)
dev:
    cargo run

# Build the application (debug)
build:
    cargo build

# Build optimized release binary
release:
    cargo build --release

# Format code
fmt:
    cargo fmt

# Validate formatting
fmt-check:
    cargo fmt --check

# Run Clippy with warnings as errors
lint:
    cargo clippy --all-targets -- -D warnings

# Run cargo check
check:
    cargo check

# Run unit tests (no network, no Docker required)
test:
    cargo test --lib

# Run integration tests (requires Docker for Testcontainers)
test-integration:
    cargo test --test '*' -- --test-threads=1

# Run all tests
test-all: test test-integration

# Start development PostgreSQL
db-up:
    docker compose -f docker/compose.yaml up -d postgres
    @echo "Waiting for PostgreSQL to be healthy..."
    until docker compose -f docker/compose.yaml exec -T postgres pg_isready -U lightning >/dev/null 2>&1; do sleep 1; done

# Stop development PostgreSQL without deleting data
db-down:
    docker compose -f docker/compose.yaml stop postgres

# Apply migrations to the development database
db-migrate:
    mise exec -- sqlx migrate run

# Reset the development database (DESTRUCTIVE: drops the persistent volume)
db-reset:
    @echo "WARNING: this deletes the development database volume and all data."
    @read -p "Type 'yes' to continue: " answer && [ "$$answer" = "yes" ]
    docker compose -f docker/compose.yaml down -v
    just db-up

# Start the complete Docker Compose stack (migrations run at app startup)
up:
    docker compose -f docker/compose.yaml up --build -d

# Stop the stack without removing volumes
down:
    docker compose -f docker/compose.yaml down

# Display application logs
logs:
    docker compose -f docker/compose.yaml logs -f app

# Execute benchmark tooling (requires a running stack: `just up`)
benchmark:
    ./scripts/benchmark.sh

# Execute all CI quality checks
ci: fmt-check lint check
    cargo test --lib
    cargo build --release
