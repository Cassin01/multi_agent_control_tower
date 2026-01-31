.PHONY: build test clean fmt lint check install run

# Build the project
build:
	cargo build --release

# Run tests
test:
	cargo test

# Clean build artifacts
clean:
	cargo clean
	rm -rf queue/tasks/*.yaml queue/reports/*.yaml

# Format code
fmt:
	cargo fmt

# Run clippy lints
lint:
	cargo clippy -- -D warnings

# Check compilation without building
check:
	cargo check

# Install the binary
install: build
	cargo install --path .

# Run the application (for development)
run:
	cargo run -- $(ARGS)

# Run with specific command
start:
	cargo run -- start $(ARGS)

tower:
	cargo run -- tower $(ARGS)

status:
	cargo run -- status $(ARGS)

sessions:
	cargo run -- sessions

down:
	cargo run -- down $(ARGS)

# Create queue directories
init-dirs:
	mkdir -p queue/tasks queue/reports queue/sessions
	mkdir -p instructions

# Development helpers
dev-setup: init-dirs
	cp -n instructions/core.md.example instructions/core.md 2>/dev/null || true
	cp -n instructions/architect.md.example instructions/architect.md 2>/dev/null || true

# Watch for changes and rebuild
watch:
	cargo watch -x check -x test
