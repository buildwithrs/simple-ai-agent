.PHONY: help build run check test test-unit test-doc clippy clippy-fix fmt fmt-check fix clean audit deny doc deny outdated update

CARGO         := cargo
CARGO_NEXTEST ?= cargo-nextest
RUSTFLAGS_DENY := -D warnings

help: ## Show this help message
	@awk 'BEGIN {FS = ":.*##"; printf "Usage: make \033[36m<target>\033[0m\n\nTargets:\n"} /^[a-zA-Z_-]+:.*?##/ { printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2 }' $(MAKEFILE_LIST)

build: ## Build the project in debug mode
	$(CARGO) build

release: ## Build the project in release mode
	$(CARGO) build --release

run: ## Run the project
	$(CARGO) run

check: ## Run cargo check (fast type/lint check)
	$(CARGO) check --all-targets

test: ## Run all tests
	$(CARGO) test --all-targets

test-unit: ## Run library unit tests only
	$(CARGO) test --lib

test-doc: ## Run doctests
	$(CARGO) test --doc

nextest: ## Run tests with cargo-nextest if installed
	@if command -v $(CARGO_NEXTEST) >/dev/null 2>&1; then \
		$(CARGO_NEXTEST) run --all-targets; \
	else \
		echo "cargo-nextest not installed; falling back to cargo test"; \
		$(CARGO) test --all-targets; \
	fi

clippy: ## Run clippy with warnings as errors
	$(CARGO) clippy --all-targets --all-features -- $(RUSTFLAGS_DENY)

clippy-fix: ## Apply clippy auto-fixes
	$(CARGO) clippy --all-targets --all-features --fix --allow-dirty --allow-staged -- $(RUSTFLAGS_DENY)

fmt: ## Format the codebase
	$(CARGO) fmt --all

fmt-check: ## Check formatting without modifying files
	$(CARGO) fmt --all -- --check

fix: ## Alias for clippy-fix
	$(CARGO) clippy --all-targets --all-features --fix --allow-dirty --allow-staged -- $(RUSTFLAGS_DENY)

doc: ## Build documentation
	$(CARGO) doc --no-deps --all-features

audit: ## Run cargo-audit to check for security advisories
	@if command -v cargo-audit >/dev/null 2>&1; then \
		cargo-audit; \
	else \
		echo "cargo-audit not installed (install with: cargo install cargo-audit)"; \
	fi

deny: ## Run cargo-deny to check licenses/advisories
	@if command -v cargo-deny >/dev/null 2>&1; then \
		cargo-deny check; \
	else \
		echo "cargo-deny not installed (install with: cargo install cargo-deny)"; \
	fi

outdated: ## Show outdated dependencies
	@if command -v cargo-outdated >/dev/null 2>&1; then \
		cargo-outdated; \
	else \
		echo "cargo-outdated not installed (install with: cargo install cargo-outdated)"; \
	fi

update: ## Update dependencies
	$(CARGO) update

clean: ## Remove build artifacts
	$(CARGO) clean

.PHONY: ci
ci: fmt-check clippy test ## Run the full CI pipeline locally
