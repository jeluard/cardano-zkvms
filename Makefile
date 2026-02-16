ROOT_DIR := $(shell pwd)

# Path to the OpenVM guest crate
GUEST_DIR := $(ROOT_DIR)/crates/zkvms/openvm

# A flat-encoded UPLC program that evaluates to the integer 42.
PROGRAM_HEX     := 010000481501

# The string representation of the expected evaluation result.
# This must match exactly what the guest's `result.to_string()` produces.
EXPECTED_RESULT := Integer(42)

# A deliberately wrong result, used to demonstrate verification failure.
WRONG_RESULT    := Integer(99)

# Temp files used between steps
LOG_FILE        := /tmp/openvm_run.log
COMMITMENT_FILE := /tmp/openvm_commitment.txt

# Path to the verifier binary
VERIFY_BIN      := $(GUEST_DIR)/verify/target/release/openvm-verify

# ---------------------------------------------------------------------------
# Targets
# ---------------------------------------------------------------------------

.PHONY: all build build-verifier input run verify verify-wrong clean help demo

## Run the full end-to-end demo: build → input → run → verify
all: build build-verifier input run verify
	@echo ""
	@echo "============================================"
	@echo " E2E demo completed successfully"
	@echo "============================================"

help: ## Show this help
	@echo "Available targets:"
	@echo ""
	@grep -E '^[a-zA-Z_-]+:.*?## ' $(MAKEFILE_LIST) | \
		awk 'BEGIN {FS = ":.*?## "}; {printf "  %-20s %s\n", $$1, $$2}'
	@echo ""
	@echo "Quick start:  make all"

build: ## Build the guest program for OpenVM
	cargo openvm build --manifest-path $(GUEST_DIR)/Cargo.toml

build-verifier: $(VERIFY_BIN) ## Build the verification tool

$(VERIFY_BIN): $(GUEST_DIR)/verify/src/main.rs $(GUEST_DIR)/verify/Cargo.toml
	cargo build --release --manifest-path $(GUEST_DIR)/verify/Cargo.toml

input: $(GUEST_DIR)/data/input.json ## Generate the input JSON file

$(GUEST_DIR)/data/input.json:
	@echo '{"input":["0x01$(PROGRAM_HEX)"]}' > $(GUEST_DIR)/data/input.json
	@cat $(GUEST_DIR)/data/input.json

run: $(GUEST_DIR)/data/input.json ## Run the guest in OpenVM (execution only, no proof)
	cargo openvm run --manifest-path $(GUEST_DIR)/Cargo.toml --input $(GUEST_DIR)/data/input.json 2>&1 | tee $(LOG_FILE)
	@# Extract the decimal byte array, convert each number to a 2-digit hex byte
	@grep -o 'Execution output: \[.*\]' $(LOG_FILE) \
		| sed 's/[^0-9,]//g' \
		| tr ',' '\n' \
		| awk 'NF {printf "%02x", $$1}' \
		> $(COMMITMENT_FILE)
	@echo "Captured commitment (hex):"
	@echo "  $$(cat $(COMMITMENT_FILE))"
	@echo ""

verify: $(VERIFY_BIN) ## Verify the commitment matches the expected result
	@$(VERIFY_BIN) $(PROGRAM_HEX) "$(EXPECTED_RESULT)" $$(cat $(COMMITMENT_FILE))

verify-wrong: $(VERIFY_BIN) ## Verify with a WRONG expected result (should fail)
	-@$(VERIFY_BIN) $(PROGRAM_HEX) "$(WRONG_RESULT)" $$(cat $(COMMITMENT_FILE))

# ---------------------------------------------------------------------------
# Web: Build WASM modules and serve the browser verifier with esbuild
# ---------------------------------------------------------------------------
#
# The web/ directory structure:
#   • crates/
#     - uplc-wasm/    — Rust→WASM crate that evaluates UPLC in browser
#     - aiken-wasm/   — Rust→WASM crate that compiles Aiken to UPLC
#     - backend/      — Actix-web backend for proof generation
#   • assets/
#     - style.css     — Extracted CSS styles
#     - index.js      — Extracted JavaScript application logic
#   • dist/           — built outputs (WASM + bundled assets)
#     - uplc/         — UPLC evaluator WASM
#     - aiken/        — Aiken compiler WASM
#     - openvm-verifier/ — OpenVM STARK verifier WASM (from npm)
#     - assets/       — CSS (copied by esbuild)
#     - assets/index.js — Bundled JavaScript (by esbuild)
#   • index.html      — minimal HTML template
#   • esbuild.config.js — esbuild bundler configuration
#   • package.json    — npm dependencies and build scripts
#
# `make web-build` compiles UPLC WASM. `make aiken-build` compiles Aiken WASM.
# `make esbuild` bundles assets with esbuild.
# `make web-serve` starts a local HTTP server on port 8080.
#

WEB_DIR := $(ROOT_DIR)/web

.PHONY: web-build aiken-build openvm-verifier-build npm-install esbuild web-serve web web-backend backend-build backend-linux backend-package setup-linux

BACKEND_BIN := $(WEB_DIR)/crates/backend/target/release/openvm-web-backend
BACKEND_LINUX_BIN := $(WEB_DIR)/crates/backend/target/x86_64-unknown-linux-gnu/release/openvm-web-backend

setup-linux: ## Install cross-compilation tools for Linux targets
	@echo "Installing x86_64-unknown-linux-gnu target..."
	rustup target add x86_64-unknown-linux-gnu
	@echo "✓ Rust Linux target installed"
	@echo ""
	@echo "⚠  Note: To cross-compile from macOS to Linux, you need a Linux C toolchain."
	@echo "   This is complex to set up. Instead, it's recommended to:"
	@echo "   1. Build on a Linux server directly"
	@echo "   2. Use Docker: docker run --rm -v \$$(pwd):/work rust:latest"
	@echo "   3. Use GitHub Actions CI for automated builds"

backend-build: ## Build backend binary for current platform (macOS)
	@echo "──────────────────────────────────────────────"
	@echo " Building backend for macOS"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/backend && cargo build --release
	@echo "✓ Binary ready at: $(BACKEND_BIN)"
	@echo ""
	@echo "To deploy to remote server:"
	@echo "  scp $(BACKEND_BIN) user@remote:/path/to/deployment/"

backend-linux: ## Build backend for Linux (requires Linux build environment)
	@echo "──────────────────────────────────────────────"
	@echo " Building backend for Linux"
	@echo "──────────────────────────────────────────────"
	@echo "⚠  Cross-compilation from macOS to Linux requires a full Linux"
	@echo "   C toolchain. Recommended alternatives:"
	@echo ""
	@echo "   1. BUILD ON LINUX DIRECTLY:"
	@echo "      On your Linux server:"
	@echo "      git clone <repo> && cd cardano-zkvms/web/crates/backend"
	@echo "      cargo build --release"
	@echo ""
	@echo "   2. USE DOCKER (platform-independent):"
	@echo "      docker run --rm -v \$$(pwd):/work rust:latest"
	@echo "      cd /work/web/crates/backend && cargo build --release --target x86_64-unknown-linux-gnu"
	@echo ""
	@echo "   3. GITHUB ACTIONS CI:"
	@echo "      Push to GitHub - CI will build Linux binary automatically"
	@echo ""

backend-package: backend-build ## Package backend binary for deployment
	@echo "──────────────────────────────────────────────"
	@echo " Packaging backend for deployment"
	@echo "──────────────────────────────────────────────"
	@if [ ! -f "$(BACKEND_BIN)" ]; then \
		echo "Error: Backend binary not found. Run 'make backend-build' first"; \
		exit 1; \
	fi
	@mkdir -p /tmp/openvm-backend
	@cp $(BACKEND_BIN) /tmp/openvm-backend/openvm-web-backend-macos
	@cp $(BACKEND_BIN) /tmp/openvm-backend/openvm-web-backend
	@echo "# OpenVM Web Backend - Deployment Package" > /tmp/openvm-backend/DEPLOY.md
	@echo "" >> /tmp/openvm-backend/DEPLOY.md
	@echo "## Deployment Steps" >> /tmp/openvm-backend/DEPLOY.md
	@echo "" >> /tmp/openvm-backend/DEPLOY.md
	@echo "### 1. Copy binary to server:" >> /tmp/openvm-backend/DEPLOY.md
	@echo "\`\`\`bash" >> /tmp/openvm-backend/DEPLOY.md
	@echo "scp openvm-web-backend-macos user@remote:/path/to/deploy/" >> /tmp/openvm-backend/DEPLOY.md
	@echo "\`\`\`" >> /tmp/openvm-backend/DEPLOY.md
	@echo "" >> /tmp/openvm-backend/DEPLOY.md
	@echo "### 2. On remote server, configure and run:" >> /tmp/openvm-backend/DEPLOY.md
	@echo "\`\`\`bash" >> /tmp/openvm-backend/DEPLOY.md
	@echo "chmod +x ./openvm-web-backend-macos" >> /tmp/openvm-backend/DEPLOY.md
	@echo "export OPENVM_GUEST_DIR=/path/to/guest/crates/zkvms/openvm" >> /tmp/openvm-backend/DEPLOY.md
	@echo "export OPENVM_STATIC_DIR=/path/to/web/dist" >> /tmp/openvm-backend/DEPLOY.md
	@echo "export PORT=8080" >> /tmp/openvm-backend/DEPLOY.md
	@echo "./openvm-web-backend-macos" >> /tmp/openvm-backend/DEPLOY.md
	@echo "\`\`\`" >> /tmp/openvm-backend/DEPLOY.md
	@echo "" >> /tmp/openvm-backend/DEPLOY.md
	@echo "## Environment Variables" >> /tmp/openvm-backend/DEPLOY.md
	@echo "- \`OPENVM_GUEST_DIR\`: Path to OpenVM guest crate (required)" >> /tmp/openvm-backend/DEPLOY.md
	@echo "- \`OPENVM_STATIC_DIR\`: Path to web/dist (required)" >> /tmp/openvm-backend/DEPLOY.md
	@echo "- \`PORT\`: HTTP port to bind (default: 8080)" >> /tmp/openvm-backend/DEPLOY.md
	@tar -czf /tmp/openvm-backend.tar.gz -C /tmp openvm-backend
	@echo "✓ Package ready: /tmp/openvm-backend.tar.gz"
	@echo ""
	@echo "Deployment:"
	@echo "  tar -xzf /tmp/openvm-backend.tar.gz"
	@echo "  scp openvm-backend/* user@remote:/path/to/deployment/"

web-build: ## Build the UPLC WASM module for the browser verifier
	@echo "──────────────────────────────────────────────"
	@echo " Building UPLC WASM module"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/uplc-wasm && wasm-pack build --target web --out-dir ../../dist/uplc
	@echo ""

aiken-build: ## Build the Aiken compiler WASM module
	@echo "──────────────────────────────────────────────"
	@echo " Building Aiken WASM module"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/aiken-wasm && bash fetch-deps.sh && \
		$(if $(shell command -v brew 2>/dev/null),CC="$$(brew --prefix llvm)/bin/clang" AR="$$(brew --prefix llvm)/bin/llvm-ar",CC=clang AR=llvm-ar) \
		wasm-pack build --target web --out-dir ../../dist/aiken
	@echo ""

openvm-verifier-build: npm-install ## Install OpenVM STARK verifier WASM from npm
	@echo "──────────────────────────────────────────────"
	@echo " Copying OpenVM STARK verifier from npm to dist"
	@echo "──────────────────────────────────────────────"
	@rm -rf $(WEB_DIR)/dist/openvm-verifier
	@cp -r $(WEB_DIR)/node_modules/@ethproofs/openvm-wasm-stark-verifier/pkg $(WEB_DIR)/dist/openvm-verifier
	@echo ""

npm-install: ## Install npm dependencies for web
	@echo "──────────────────────────────────────────────"
	@echo " Installing npm dependencies"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR) && npm install
	@echo ""

esbuild: npm-install ## Bundle assets with esbuild (replaces patch script)
	@echo "──────────────────────────────────────────────"
	@echo " Bundling with esbuild"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR) && npm run build:prod
	@echo ""

web-serve: ## Serve the web verifier from dist/ on http://localhost:8080 (static only, no proof generation)
	@echo "──────────────────────────────────────────────"
	@echo " Serving web verifier at http://localhost:8080"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/dist && python3 -m http.server 8080

web-backend: web-build aiken-build openvm-verifier-build esbuild ## Build all WASM + esbuild bundle + backend, then serve with proof generation
	@echo "──────────────────────────────────────────────"
	@echo " Building Rust backend with integrated check-vk"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/backend && cargo build --release
	@echo "──────────────────────────────────────────────"
	@echo " Serving web with backend at http://localhost:8080"
	@echo " Proof generation enabled via /api/prove"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/backend && OPENVM_GUEST_DIR="$(GUEST_DIR)" \
		OPENVM_VK_PATH="$(ROOT_DIR)/target/openvm/app.vk" \
		OPENVM_STATIC_DIR="$(WEB_DIR)/dist" \
		cargo run --release

web: web-build aiken-build openvm-verifier-build esbuild web-serve ## Build WASM + esbuild bundle and serve

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------
clean: ## Remove generated files
	rm -f $(GUEST_DIR)/data/input.json $(LOG_FILE) $(COMMITMENT_FILE)
	cd $(GUEST_DIR)/verify && cargo clean
	rm -rf $(WEB_DIR)/dist
	rm -rf $(WEB_DIR)/node_modules
	rm -f $(WEB_DIR)/package-lock.json
	cd $(WEB_DIR)/crates/backend && cargo clean 2>/dev/null || true
