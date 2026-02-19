ROOT_DIR := $(shell pwd)

# Path to the OpenVM guest crate
GUEST_DIR := $(ROOT_DIR)/crates/zkvms/openvm

# A flat-encoded UPLC program that evaluates to the integer 42.
PROGRAM_HEX     := 010000481501

# Temp files used between steps
LOG_FILE        := /tmp/openvm_run.log
COMMITMENT_FILE := /tmp/openvm_commitment.txt

# Path to the backend binary (also handles setup)
CARDANO_ZKVMS   = cargo run --release --manifest-path $(WEB_DIR)/crates/backend/Cargo.toml --bin cardano-zkvms --

# ---------------------------------------------------------------------------
# Targets
# ---------------------------------------------------------------------------

.PHONY: all build run clean help demo

## Run the full end-to-end demo: build → run
all: build run
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
	@OPENVM_GUEST_DIR=$(GUEST_DIR) $(CARDANO_ZKVMS) setup

run: ## Run the guest in OpenVM (execution only, no proof)
	@echo "Use 'make web-with-backend' to run the full server with proof generation"

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
#     - openvm-verifier/ — OpenVM STARK verifier WASM (from local crate)
#     - assets/       — CSS (copied by esbuild)
#     - assets/index.js — Bundled JavaScript (by esbuild)
#   • index.html      — minimal HTML template
#   • esbuild.config.js — esbuild bundler configuration
#   • package.json    — npm dependencies and build scripts
#
# `make uplc-build` compiles UPLC WASM. `make aiken-build` compiles Aiken WASM.
# `make esbuild` bundles assets with esbuild.
# `make web-serve` starts a local HTTP server on port 8080.
#

WEB_DIR := $(ROOT_DIR)/web

.PHONY: uplc-build aiken-build openvm-verifier-build npm-install esbuild web-serve web web-with-backend backend-build backend-linux backend-package backend-deploy gh-secrets ssh-add-key setup-linux

BACKEND_BIN := $(WEB_DIR)/crates/backend/target/release/cardano-zkvms
BACKEND_LINUX_BIN := $(WEB_DIR)/crates/backend/target/x86_64-unknown-linux-gnu/release/cardano-zkvms

# Load configuration from web/conf/.env (optional - for backend deployment)
# In GitHub Actions CI, this file won't exist and will be silently skipped
-include web/conf/.env

# SSH destination: uses SSH_USER@SSH_HOST if SSH_USER is set, otherwise just SSH_HOST
# This lets SSH config handle auth when using aliases (SSH_USER empty)
SSH_DEST = $(if $(SSH_USER),$(SSH_USER)@)$(SSH_HOST)

# SSH options: include explicit key if SSH_KEY_PATH is defined
# ControlMaster multiplexes all ssh/scp over one connection → one passphrase prompt
SSH_OPTS = $(if $(SSH_KEY_PATH),-i $(SSH_KEY_PATH),) -o ControlMaster=auto -o ControlPath=/tmp/ssh-deploy-%r@%h:%p -o ControlPersist=120

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
	@cd $(WEB_DIR)/crates/backend && cargo build --release
	@echo "✓ Binary ready at: $(BACKEND_BIN)"

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
	@echo "export PORT=8080" >> /tmp/openvm-backend/DEPLOY.md
	@echo "./openvm-web-backend-macos" >> /tmp/openvm-backend/DEPLOY.md
	@echo "\`\`\`" >> /tmp/openvm-backend/DEPLOY.md
	@echo "" >> /tmp/openvm-backend/DEPLOY.md
	@echo "## Environment Variables" >> /tmp/openvm-backend/DEPLOY.md
	@echo "- \`OPENVM_GUEST_DIR\`: Path to OpenVM guest crate (required)" >> /tmp/openvm-backend/DEPLOY.md
	@echo "- \`PORT\`: HTTP port to bind (default: 8080)" >> /tmp/openvm-backend/DEPLOY.md
	@tar -czf /tmp/openvm-backend.tar.gz -C /tmp openvm-backend
	@echo "✓ Package ready: /tmp/openvm-backend.tar.gz"
	@echo ""
	@echo "Deployment:"
	@echo "  tar -xzf /tmp/openvm-backend.tar.gz"
	@echo "  scp openvm-backend/* user@remote:/path/to/deployment/"

backend-deploy: ## Deploy backend to remote server (builds on server via setup.sh)
	@bash web/scripts/deploy.sh

backend-rekey: ## Deploy with forced key regeneration (after OpenVM version change)
	@FORCE_KEYGEN=1 bash web/scripts/deploy.sh

backend-teardown: ## Remove everything installed on the remote host by backend-deploy
	@bash web/scripts/teardown.sh


gh-secrets: ## Set GitHub secret BACKEND_URL_PROD from .env file
	@if [ -z "$(BACKEND_URL)" ]; then \
		echo "❌ BACKEND_URL not set in web/conf/.env"; \
		exit 1; \
	fi
	@command -v gh > /dev/null || { echo "❌ gh CLI not installed. Install from https://github.com/cli/cli"; exit 1; }
	@echo "Setting GitHub secret BACKEND_URL_PROD = $(BACKEND_URL)"
	@echo "$(BACKEND_URL)" | gh secret set BACKEND_URL_PROD --body --
	@echo "✓ GitHub secret BACKEND_URL_PROD updated successfully"
	@echo ""
	@echo "GitHub Actions will now use:"
	@echo "  BACKEND_URL_PROD = $(BACKEND_URL)"
	@echo ""
	@echo "Next step: Push to main branch to trigger CD workflow"
	@echo "  git push origin main"

backend-logs: ## Tail backend (cardano-zkvms) logs on remote server
	@ssh $(SSH_OPTS) $(SSH_DEST) "sudo journalctl -u cardano-zkvms -f"

caddy-logs: ## Tail Caddy reverse-proxy logs on remote server
	@ssh $(SSH_OPTS) $(SSH_DEST) "sudo journalctl -u caddy -f"

ssh-add-key: ## Add SSH key to agent (automatic, called by backend-deploy for password-free deployment)
	@if [ -z "$(SSH_KEY_PATH)" ]; then \
		echo "ℹ SSH_KEY_PATH not set, skipping ssh-add"; \
	else \
		echo "Ensuring SSH key is loaded: $(SSH_KEY_PATH)"; \
		ssh-add $(SSH_KEY_PATH) 2>/dev/null || ssh-add -K $(SSH_KEY_PATH) 2>/dev/null || true; \
		echo "✓ SSH key ready (you may be prompted for passphrase once)."; \
	fi

uplc-build: ## Build the UPLC WASM module for the browser verifier
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

openvm-verifier-build: ## Build OpenVM STARK verifier WASM from local crate
	@echo "──────────────────────────────────────────────"
	@echo " Building OpenVM STARK verifier WASM"
	@echo "──────────────────────────────────────────────"
	cd $(ROOT_DIR)/crates/zkvms/openvm/verify && \
		$(if $(shell command -v brew 2>/dev/null),CC="$$(brew --prefix llvm)/bin/clang" AR="$$(brew --prefix llvm)/bin/llvm-ar",CC=clang AR=llvm-ar) \
		wasm-pack build --target web --out-dir $(WEB_DIR)/dist/openvm-verifier
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
	@if [ -n "$(BACKEND_URL)" ]; then \
		echo " Backend URL: $(BACKEND_URL)"; \
	fi
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR) && BACKEND_URL=$(BACKEND_URL) npm run build:prod
	@echo ""


web-serve: ## Serve the web verifier from dist/ on http://localhost:8080 (static only, no proof generation)
	@echo "──────────────────────────────────────────────"
	@echo " Serving web verifier at http://localhost:8080"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/dist && python3 -m http.server 8080

web-with-backend: uplc-build aiken-build openvm-verifier-build ## Build all WASM + esbuild bundle + backend, then serve with proof generation
	@echo "──────────────────────────────────────────────"
	@echo " Building with local backend (http://localhost:8080)"
	@echo "──────────────────────────────────────────────"
	BACKEND_URL=http://localhost:8080 make esbuild
	@echo "──────────────────────────────────────────────"
	@echo " Building Rust backend with integrated check-vk"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/backend && cargo build --release
	@echo "──────────────────────────────────────────────"
	@echo " Backend API:  http://localhost:8080"
	@echo " Web UI:       http://localhost:3000"
	@echo " Proof generation enabled via /api/prove"
	@echo "──────────────────────────────────────────────"
	@cd $(WEB_DIR)/crates/backend && OPENVM_GUEST_DIR="$(GUEST_DIR)" \
		cargo run --release & \
	BACKEND_PID=$$!; \
	trap "kill $$BACKEND_PID 2>/dev/null; exit" INT TERM; \
	cd $(WEB_DIR)/dist && python3 -m http.server 3000; \
	kill $$BACKEND_PID 2>/dev/null

web: uplc-build aiken-build openvm-verifier-build ## Build WASM + esbuild bundle and serve (no backend)
	@echo "──────────────────────────────────────────────"
	@echo " Building with no backend (local only)"
	@echo "──────────────────────────────────────────────"
	BACKEND_URL=/ make esbuild
	@echo "──────────────────────────────────────────────"
	@echo " Serving web verifier at http://localhost:8080"
	@echo "──────────────────────────────────────────────"
	make web-serve

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------
clean: ## Remove generated files
	rm -f $(LOG_FILE) $(COMMITMENT_FILE)
	rm -rf $(WEB_DIR)/dist
	rm -rf $(WEB_DIR)/node_modules
	rm -f $(WEB_DIR)/package-lock.json
	cd $(WEB_DIR)/crates/backend && cargo clean 2>/dev/null || true
