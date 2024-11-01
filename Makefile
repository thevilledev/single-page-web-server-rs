# Project variables
BINARY_NAME = single-page-web-server-rs

# Cargo commands
CARGO = cargo
CARGO_FLAGS = 
RELEASE_FLAGS = --release

.PHONY: all build run test clean dockerlint fmt check release-dry-run release

# Default target
all: build

# Development build
build: ## Build the development binary
	$(CARGO) build $(CARGO_FLAGS)

# Release build
release: ## Build the release binary
	$(CARGO) build $(RELEASE_FLAGS)

# Run the development server
run: ## Run the development server
	$(CARGO) run $(CARGO_FLAGS)

# Run with specific parameters
run-with-params: ## Run with custom parameters (use PORT=xxxx ADDR=x.x.x.x INDEX=path)
	$(CARGO) run -- \
		$(if $(PORT),--port $(PORT)) \
		$(if $(ADDR),--addr $(ADDR)) \
		$(if $(INDEX),--index-path $(INDEX))

# Run tests
test: ## Run all tests
	$(CARGO) test

# Clean build artifacts
clean: ## Clean build artifacts
	$(CARGO) clean

# Format code
fmt: ## Format code using rustfmt
	$(CARGO) fmt

# Lint code
lint: ## Lint code using clippy
	$(CARGO) clippy -- -D warnings

# Check code without building
check: ## Check code without building
	$(CARGO) check

# Docker commands
docker: ## Build Docker image
	docker build -t $(DOCKER_IMAGE):latest -f Dockerfile.static .

# Install development dependencies
dev-deps: ## Install development dependencies
	$(CARGO) install cargo-watch cargo-edit cargo-bloat

# Watch for changes and rebuild
watch: ## Watch for changes and rebuild
	cargo watch -x run

# Analyze binary size
bloat: ## Analyze binary size
	cargo bloat --release

# Production release build with optimizations
build-prod: ## Create optimized production build
	RUSTFLAGS="-C target-cpu=native -C opt-level=3" cargo build --release
	strip target/release/$(BINARY_NAME)

release-dry-run: ## Test the release process
	cargo install cargo-dist
	cargo dist plan

release: ## Create and publish a new release
	@if [ -z "$(VERSION)" ]; then \
		echo "Please specify VERSION=x.x.x"; \
		exit 1; \
	fi
	@if [ -n "`git status --porcelain`" ]; then \
		echo "Working directory is not clean"; \
		exit 1; \
	fi
	@echo "Creating release $(VERSION)"
	@git tag -a v$(VERSION) -m "Release v$(VERSION)"
	@git push origin v$(VERSION)