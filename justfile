set dotenv-load

# Default: list available recipes
default:
    @just --list

# Check all crates compile
check:
    cargo check --workspace

# Build all crates in debug mode
build:
    cargo build --workspace

# Build all crates in release mode
build-release:
    cargo build --workspace --release

# Run all unit tests
test:
    cargo test --workspace --lib

# Run integration tests (requires running postgres)
test-integration:
    cargo test -p gitdoc-server --test integration

# Run all tests (unit + integration)
test-all:
    cargo test --workspace

# Start postgres via docker compose
db-up:
    docker compose up -d postgres

# Stop postgres
db-down:
    docker compose down

# Start the gitdoc-server with docker-compose postgres
run-server:
    GITDOC_DATABASE_URL="postgres://gitdoc:gitdoc@localhost:5433/gitdoc" cargo run -p gitdoc-server

# Start the gitdoc-server in release mode
run-server-release:
    GITDOC_DATABASE_URL="postgres://gitdoc:gitdoc@localhost:5433/gitdoc" cargo run -p gitdoc-server --release

# Build the MCP binary in release mode for local use
build-mcp:
    cargo build -p gitdoc-mcp --message-format=json | jq -r 'select(.executable != null) | .executable'

# Start the MCP server via stdio
run-mcp:
    cargo run -p gitdoc-mcp

# Start postgres + server together
start: db-up run-server

# Clean build artifacts
clean:
    cargo clean
