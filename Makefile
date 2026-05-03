ROOT_DIR := $(shell pwd)
SELF := $(firstword $(MAKEFILE_LIST))

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

PHONY_TARGETS := \
	help \
	all \
	build \
	run \
	setup-linux \
	backend-build \
	backend-linux \
	backend-package \
	backend-deploy \
	backend-rekey \
	backend-teardown \
	gh-secrets \
	backend-logs \
	caddy-logs \
	ssh-add-key \
	uplc-build \
	aiken-build \
	openvm-verifier-build \
	npm-install \
	esbuild \
	web-serve \
	web-with-backend \
	web \
	mcu-check \
	mcu-nostd-check \
	mcu-test \
	mcu-qemu-smoke \
	mcu-tool \
	mcu-tui \
	mcu-stark-artifacts \
	mcu-stark-pack \
	mcu-stark-e2e \
	mcu-evm-artifacts \
	mcu-evm-pack \
	mcu-evm-verify \
	mcu-evm-e2e \
	mcu-rp2350-build \
	mcu-rp2350-uf2 \
	mcu-rp2350-flash \
	mcu-rp2350-ble-build \
	mcu-rp2350-ble-flash \
	mcu-rp2350-ble-release-build \
	mcu-rp2350-ble-release-flash \
	mcu-radium-xtensa-patch \
	mcu-esp32s3-build \
	mcu-esp32s3-flash \
	mcu-esp32s3-flash-monitor \
	mcu-esp32s3-espidf-build \
	mcu-esp32s3-espidf-flash \
	mcu-esp32s3-flash-halo2-std \
	mcu-esp32s3-flash-evm-status \
	mcu-evm-esp32s3-e2e \
	mcu-esp32s3-monitor \
	clean

.PHONY: $(PHONY_TARGETS)

define PRINT_HELP_SECTION
	@echo "\033[1;4m$(1):\033[00m"
	@grep -E '^[a-zA-Z0-9_.-]+:.*## &$(2) ' $(SELF) | while read -r line; do \
		target=$$(printf "%s" "$$line" | cut -f 1 -d':'); \
		description=$$(printf "%s" "$$line" | cut -f 3- -d'#' | sed 's/^ \&$(2)//'); \
		printf "  \033[1;32m%s\033[00m:%s\n" "$$target" "$$description"; \
	done
	@echo ""
endef

help: ## &start Show this help
	$(call PRINT_HELP_SECTION,Getting Started,start)
	$(call PRINT_HELP_SECTION,Web & Backend,web)
	$(call PRINT_HELP_SECTION,Deployment & Ops,ops)
	$(call PRINT_HELP_SECTION,MCU,mcu)
	$(call PRINT_HELP_SECTION,Maintenance,misc)
	@echo "\033[1;4mConfiguration:\033[00m"
	@grep -E '^[a-zA-Z0-9_]+ \?= ' $(SELF) | sort | while read -r line; do \
		printf "  \033[36m%s\033[00m=%s\n" "$$(echo $$line | cut -f 1 -d'=')" "$$(echo $$line | cut -f 2- -d'=')"; \
	done

all: build run ## &start Run the full end-to-end demo: build -> run
	@echo ""
	@echo "============================================"
	@echo " E2E demo completed successfully"
	@echo "============================================"

build: ## &start Build the guest program for OpenVM
	@OPENVM_GUEST_DIR=$(GUEST_DIR) $(CARDANO_ZKVMS) setup

run: ## &start Run the guest in OpenVM (execution only, no proof)
	@echo "Use 'make web-with-backend' to run the full server with proof generation"

# ---------------------------------------------------------------------------
# Web: Build browser WASM modules and serve the web verifier with esbuild
# ---------------------------------------------------------------------------
#
# The web/ directory structure:
#   • crates/
#     - uplc-wasm/    — Rust→WASM crate that evaluates UPLC in browser
#     - aiken-wasm/   — Rust→WASM crate that compiles Aiken to UPLC
#     - backend/      — Actix-web backend for proof generation and native verification
#   • assets/
#     - style.css     — Extracted CSS styles
#     - index.js      — Extracted JavaScript application logic
#   • dist/           — built outputs (WASM + bundled assets)
#     - uplc/         — UPLC evaluator WASM
#     - aiken/        — Aiken compiler WASM
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
MCU_DIR := $(GUEST_DIR)/mcu
MCU_BACKEND_URL ?= http://127.0.0.1:8080
MCU_PROGRAM_HEX ?= 010000481501
MCU_AGG_VK ?= /tmp/agg_stark.vk
MCU_PROVE_RESPONSE ?= /tmp/openvm-mcu-prove.json
MCU_STARK_KEY ?= /tmp/openvm-mcu-stark.key
MCU_STARK_PROOF ?= /tmp/openvm-mcu-stark.proof
MCU_EVM_VERIFIER ?= /tmp/openvm-evm-verifier.bin
MCU_EVM_VERIFIER_DIR ?= /tmp/openvm-evm-verifier
MCU_EVM_PROOF_JSON ?= /tmp/openvm-evm-proof.json
MCU_EVM_PROGRAM_BIN ?= /tmp/openvm-evm-program.flat
MCU_EVM_VMEXE ?= $(ROOT_DIR)/target/openvm/release/openvm-guest.vmexe
MCU_EVM_CONFIG ?= $(GUEST_DIR)/openvm.toml
MCU_EVM_APP_PK ?= $(ROOT_DIR)/target/openvm/app.pk
MCU_EVM_AGG_PK ?= $(ROOT_DIR)/target/openvm/agg_stark.pk
MCU_EVM_ROOT_PK ?=
MCU_EVM_HALO2_PK ?=
MCU_EVM_WRITE_ROOT_PK ?=
MCU_EVM_WRITE_HALO2_PK ?=
MCU_EVM_KEY ?= /tmp/openvm-mcu-evm.key
MCU_EVM_PROOF ?= /tmp/openvm-mcu-evm.proof
MCU_TUI_ARGS ?= --key $(MCU_EVM_KEY) --proof $(MCU_EVM_PROOF)
MCU_ESP_EXPORT ?= $(HOME)/export-esp.sh
MCU_ESP32S3_PORT ?= $(shell ls -1 /dev/cu.usbmodem* 2>/dev/null | head -n1)
MCU_ESP32S3_TARGET ?= xtensa-esp32s3-none-elf
MCU_ESP32S3_ESPIDF_TARGET ?= xtensa-esp32s3-espidf
MCU_ESP32S3_ELF ?= $(MCU_DIR)/target/$(MCU_ESP32S3_TARGET)/debug/openvm-mcu-esp32s3
MCU_ESP32S3_ESPIDF_ELF ?= $(MCU_DIR)/target/$(MCU_ESP32S3_ESPIDF_TARGET)/debug/openvm-mcu-esp32s3
MCU_ESP32S3_HOST_EVM_DETAIL ?= Halo2/KZG checked by native host verifier
MCU_ESP32S3_ESPIDF_SDKCONFIG_DEFAULTS ?= $(MCU_DIR)/boards/esp32s3/sdkconfig.defaults
MCU_RP2350_TARGET ?= thumbv8m.main-none-eabihf
MCU_RP2350_ELF ?= $(MCU_DIR)/target/$(MCU_RP2350_TARGET)/debug/openvm-mcu-rp2350
MCU_RP2350_BLE_DIR ?= $(MCU_DIR)/boards/rp2350/ble
MCU_RP2350_BLE_PROFILE ?= release
MCU_RP2350_BLE_CARGO_PROFILE ?= $(if $(filter release,$(MCU_RP2350_BLE_PROFILE)),--release,)
MCU_RP2350_BLE_ELF ?= $(MCU_RP2350_BLE_DIR)/target/$(MCU_RP2350_TARGET)/$(MCU_RP2350_BLE_PROFILE)/openvm-mcu-rp2350-ble
MCU_RP2350_UF2 ?= /tmp/openvm-mcu-rp2350.uf2
MCU_RP2350_FAMILY ?= 0xe48bff59
PICOTOOL ?= picotool
MCU_RADIUM_SRC ?= $(HOME)/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/radium-0.7.0
MCU_RADIUM_PATCH ?= /tmp/radium-0.7.0-xtensa

BACKEND_BIN := $(WEB_DIR)/crates/backend/target/release/cardano-zkvms
BACKEND_LINUX_BIN := $(WEB_DIR)/crates/backend/target/x86_64-unknown-linux-gnu/release/cardano-zkvms

# Load configuration from conf/.env (optional - for backend deployment)
# In GitHub Actions CI, this file won't exist and will be silently skipped
-include conf/.env

# SSH destination: uses SSH_USER@SSH_HOST if SSH_USER is set, otherwise just SSH_HOST
# This lets SSH config handle auth when using aliases (SSH_USER empty)
SSH_DEST = $(if $(SSH_USER),$(SSH_USER)@)$(SSH_HOST)

# SSH options: include explicit key if SSH_KEY_PATH is defined
# ControlMaster multiplexes all ssh/scp over one connection → one passphrase prompt
SSH_OPTS = $(if $(SSH_KEY_PATH),-i $(SSH_KEY_PATH),) -o ControlMaster=auto -o ControlPath=/tmp/ssh-deploy-%r@%h:%p -o ControlPersist=120

setup-linux: ## &start Install cross-compilation tools for Linux targets
	@echo "Installing x86_64-unknown-linux-gnu target..."
	rustup target add x86_64-unknown-linux-gnu
	@echo "✓ Rust Linux target installed"
	@echo ""
	@echo "⚠  Note: To cross-compile from macOS to Linux, you need a Linux C toolchain."
	@echo "   This is complex to set up. Instead, it's recommended to:"
	@echo "   1. Build on a Linux server directly"
	@echo "   2. Use Docker: docker run --rm -v \$$(pwd):/work rust:latest"
	@echo "   3. Use GitHub Actions CI for automated builds"

backend-build: ## &web Build backend binary for the current platform (macOS)
	@echo "──────────────────────────────────────────────"
	@echo " Building backend for macOS"
	@echo "──────────────────────────────────────────────"
	@cd $(WEB_DIR)/crates/backend && cargo build --release
	@echo "✓ Binary ready at: $(BACKEND_BIN)"

backend-linux: ## &web Explain Linux backend build options
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

backend-package: backend-build ## &ops Package the backend binary for deployment
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

backend-deploy: ## &ops Deploy the backend to the remote server
	@bash scripts/deploy.sh

backend-rekey: ## &ops Deploy with forced key regeneration after an OpenVM change
	@FORCE_KEYGEN=1 bash scripts/deploy.sh

backend-teardown: ## &ops Remove everything installed by backend-deploy on the remote host
	@bash scripts/teardown.sh


gh-secrets: ## &ops Set GitHub secret BACKEND_URL_PROD from conf/.env
	@if [ -z "$(BACKEND_URL)" ]; then \
		echo "❌ BACKEND_URL not set in conf/.env"; \
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

backend-logs: ## &ops Tail backend logs on the remote server
	@ssh $(SSH_OPTS) $(SSH_DEST) "sudo journalctl -u cardano-zkvms -f"

caddy-logs: ## &ops Tail Caddy reverse-proxy logs on the remote server
	@ssh $(SSH_OPTS) $(SSH_DEST) "sudo journalctl -u caddy -f"

ssh-add-key: ## &ops Add the SSH key to the agent for password-free deployment
	@if [ -z "$(SSH_KEY_PATH)" ]; then \
		echo "ℹ SSH_KEY_PATH not set, skipping ssh-add"; \
	else \
		echo "Ensuring SSH key is loaded: $(SSH_KEY_PATH)"; \
		ssh-add $(SSH_KEY_PATH) 2>/dev/null || ssh-add -K $(SSH_KEY_PATH) 2>/dev/null || true; \
		echo "✓ SSH key ready (you may be prompted for passphrase once)."; \
	fi

uplc-build: ## &web Build the UPLC WASM module for the web verifier
	@echo "──────────────────────────────────────────────"
	@echo " Building UPLC WASM module"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/uplc-wasm && wasm-pack build --target web --out-dir ../../dist/uplc
	@echo ""

aiken-build: ## &web Build the Aiken compiler WASM module
	@echo "──────────────────────────────────────────────"
	@echo " Building Aiken WASM module"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/aiken-wasm && bash fetch-deps.sh && \
		$(if $(shell command -v brew 2>/dev/null),CC="$$(brew --prefix llvm)/bin/clang" AR="$$(brew --prefix llvm)/bin/llvm-ar",CC=clang AR=llvm-ar) \
		wasm-pack build --target web --out-dir ../../dist/aiken
	@echo ""

openvm-verifier-build: ## &web Build the OpenVM verifier WASM module for browser-local proof verification
	@echo "──────────────────────────────────────────────"
	@echo " Building OpenVM STARK verifier WASM module"
	@echo "──────────────────────────────────────────────"
	rm -rf $(WEB_DIR)/dist/openvm-verifier
	cd crates/zkvms/openvm/verify && wasm-pack build --target web --out-dir ../../../../web/dist/openvm-verifier
	@echo ""

npm-install: ## &web Install npm dependencies for the web app
	@echo "──────────────────────────────────────────────"
	@echo " Installing npm dependencies"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR) && npm install
	@echo ""

esbuild: npm-install ## &web Bundle assets with esbuild
	@echo "──────────────────────────────────────────────"
	@echo " Bundling with esbuild"
	@if [ -n "$(BACKEND_URL)" ]; then \
		echo " Backend URL: $(BACKEND_URL)"; \
	fi
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR) && BACKEND_URL=$(BACKEND_URL) npm run build:prod
	@echo ""


web-serve: ## &web Serve the static web verifier on http://localhost:8080
	@echo "──────────────────────────────────────────────"
	@echo " Serving web verifier at http://localhost:8080"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/dist && python3 -m http.server 8080

web-with-backend: build uplc-build aiken-build openvm-verifier-build ## &web Run the UI and local proof backend together
	@echo "──────────────────────────────────────────────"
	@echo " Building with local backend (http://localhost:8080)"
	@echo "──────────────────────────────────────────────"
	BACKEND_URL=http://localhost:8080 make esbuild
	@echo "──────────────────────────────────────────────"
	@echo " Building Rust backend with proof generation"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/crates/backend && cargo build --release
	@echo "──────────────────────────────────────────────"
	@echo " Backend API:  http://localhost:8080"
	@echo " Web UI:       http://localhost:3000"
	@echo " Proof generation:   /api/prove"
	@echo " Proof verification: browser WASM"
	@echo "──────────────────────────────────────────────"
	@cd $(WEB_DIR)/crates/backend && OPENVM_GUEST_DIR="$(GUEST_DIR)" \
		cargo run --release & \
	BACKEND_PID=$$!; \
	trap "kill $$BACKEND_PID 2>/dev/null; exit" INT TERM; \
	cd $(WEB_DIR)/dist && python3 -m http.server 3000; \
	kill $$BACKEND_PID 2>/dev/null

web: uplc-build aiken-build openvm-verifier-build ## &web Build and serve the browser app without the backend
	@echo "──────────────────────────────────────────────"
	@echo " Building with no backend (local only)"
	@echo "──────────────────────────────────────────────"
	BACKEND_URL=/ make esbuild
	@echo "──────────────────────────────────────────────"
	@echo " Serving web verifier at http://localhost:8080"
	@echo "──────────────────────────────────────────────"
	make web-serve

# ---------------------------------------------------------------------------
# MCU verifier workspace
# ---------------------------------------------------------------------------

mcu-check: ## &mcu Check the OpenVM MCU verifier workspace
	cd $(MCU_DIR) && cargo check --workspace

mcu-nostd-check: ## &mcu Check shared MCU crates without std
	cd $(MCU_DIR) && cargo check -p openvm-mcu-protocol --no-default-features && \
		cargo check -p openvm-mcu-verifier-core --no-default-features && \
		cargo check -p openvm-mcu-device-app --no-default-features

mcu-test: ## &mcu Run MCU protocol, verifier, device, and host tests
	cd $(MCU_DIR) && cargo test --workspace

mcu-tool: ## &mcu Build the OpenVM MCU host artifact packer
	cd $(MCU_DIR) && cargo build -p openvm-mcu-host-tools

mcu-tui: ## &mcu Run the Ratatui dashboard for MCU proof artifacts
	cd $(MCU_DIR) && cargo run -p openvm-mcu-host-tools --bin openvm-mcu-tui -- $(MCU_TUI_ARGS)

mcu-stark-artifacts: ## &mcu Fetch a real web-style STARK proof and aggregation VK from a backend
	curl -fsS $(MCU_BACKEND_URL)/data/agg_stark.vk -o $(MCU_AGG_VK)
	curl -fsS -X POST $(MCU_BACKEND_URL)/api/prove \
		-H 'Content-Type: application/json' \
		-d '{"program_hex":"$(MCU_PROGRAM_HEX)"}' \
		-o $(MCU_PROVE_RESPONSE)

mcu-stark-pack: ## &mcu Pack fetched STARK artifacts into MCU protocol envelopes
	cd $(MCU_DIR) && cargo run -p openvm-mcu-host-tools -- pack-stark-key \
		--agg-vk-file $(MCU_AGG_VK) \
		--out $(MCU_STARK_KEY)
	cd $(MCU_DIR) && cargo run -p openvm-mcu-host-tools -- pack-stark-proof \
		--prove-response-json $(MCU_PROVE_RESPONSE) \
		--key-file $(MCU_AGG_VK) \
		--out $(MCU_STARK_PROOF)

mcu-evm-artifacts: build ## &mcu Generate Halo2/KZG proof JSON and native verifier metadata
	@mkdir -p $$(dirname $(MCU_EVM_PROGRAM_BIN)) $(MCU_EVM_VERIFIER_DIR)
	@printf '$(MCU_PROGRAM_HEX)' | xxd -r -p > $(MCU_EVM_PROGRAM_BIN)
	@EXTRA_ARGS="--config $(MCU_EVM_CONFIG)"; \
	if [ -f "$(MCU_EVM_APP_PK)" ]; then EXTRA_ARGS="--app-pk $(MCU_EVM_APP_PK)"; fi; \
	if [ -f "$(MCU_EVM_AGG_PK)" ]; then EXTRA_ARGS="$$EXTRA_ARGS --agg-pk $(MCU_EVM_AGG_PK)"; fi; \
	if [ -n "$(MCU_EVM_ROOT_PK)" ]; then EXTRA_ARGS="$$EXTRA_ARGS --root-pk $(MCU_EVM_ROOT_PK)"; fi; \
	if [ -n "$(MCU_EVM_HALO2_PK)" ]; then EXTRA_ARGS="$$EXTRA_ARGS --halo2-pk $(MCU_EVM_HALO2_PK)"; fi; \
	if [ -n "$(MCU_EVM_WRITE_ROOT_PK)" ]; then EXTRA_ARGS="$$EXTRA_ARGS --write-root-pk $(MCU_EVM_WRITE_ROOT_PK)"; fi; \
	if [ -n "$(MCU_EVM_WRITE_HALO2_PK)" ]; then EXTRA_ARGS="$$EXTRA_ARGS --write-halo2-pk $(MCU_EVM_WRITE_HALO2_PK)"; fi; \
	cargo run --release --manifest-path $(GUEST_DIR)/Cargo.toml \
		-p openvm-prover --features evm-prove --bin openvm-evm-artifacts -- \
		$$EXTRA_ARGS \
		--vmexe $(MCU_EVM_VMEXE) \
		--program $(MCU_EVM_PROGRAM_BIN) \
		--proof-json $(MCU_EVM_PROOF_JSON) \
		--verifier-dir $(MCU_EVM_VERIFIER_DIR)
	@cp $(MCU_EVM_VERIFIER_DIR)/native-verifier.bin $(MCU_EVM_VERIFIER)

mcu-evm-pack: ## &mcu Pack Halo2/KZG proof JSON and verifier metadata into MCU envelopes
	cd $(MCU_DIR) && cargo run -p openvm-mcu-host-tools -- pack-key \
		--key-file $(MCU_EVM_VERIFIER) \
		--out $(MCU_EVM_KEY)
	cd $(MCU_DIR) && cargo run -p openvm-mcu-host-tools -- pack-evm-proof \
		--proof-json $(MCU_EVM_PROOF_JSON) \
		--key-file $(MCU_EVM_VERIFIER) \
		--out $(MCU_EVM_PROOF)

mcu-evm-verify: mcu-evm-artifacts ## &mcu Verify a real OpenVM Halo2/KZG proof with the native verifier
	@echo "native Halo2/KZG verification completed by mcu-evm-artifacts"

mcu-evm-e2e: mcu-evm-artifacts mcu-evm-pack ## &mcu Generate and verify a real OpenVM Halo2/KZG proof end to end

mcu-rp2350-build: ## &mcu Build Tufty/RP2350 no_std firmware
	cd $(MCU_DIR) && cargo build -p openvm-mcu-rp2350 --target $(MCU_RP2350_TARGET)

mcu-rp2350-uf2: mcu-rp2350-build ## &mcu Convert Tufty/RP2350 firmware to an RP2350-family UF2
	$(PICOTOOL) uf2 convert $(MCU_RP2350_ELF) -t elf $(MCU_RP2350_UF2) -t uf2 --family $(MCU_RP2350_FAMILY)
	@echo "✓ UF2 ready at $(MCU_RP2350_UF2)"

mcu-rp2350-flash: mcu-rp2350-build ## &mcu Flash Tufty/RP2350 firmware over USB BOOTSEL with picotool
	$(PICOTOOL) load --ignore-partitions -v -x $(MCU_RP2350_ELF) -t elf

mcu-rp2350-ble-build: ## &mcu Build Tufty/RP2350 BLE firmware
	cd $(MCU_RP2350_BLE_DIR) && cargo build --target $(MCU_RP2350_TARGET) $(MCU_RP2350_BLE_CARGO_PROFILE)

mcu-rp2350-ble-flash: ## &mcu Flash the current Tufty/RP2350 BLE firmware over USB BOOTSEL
	@if [ ! -f "$(MCU_RP2350_BLE_ELF)" ]; then \
		echo "Error: BLE ELF not found at $(MCU_RP2350_BLE_ELF)"; \
		echo "Run 'make mcu-rp2350-ble-build MCU_RP2350_BLE_PROFILE=$(MCU_RP2350_BLE_PROFILE)' first."; \
		exit 1; \
	fi
	$(PICOTOOL) load --ignore-partitions -v -x $(MCU_RP2350_BLE_ELF) -t elf

mcu-rp2350-ble-release-build: MCU_RP2350_BLE_PROFILE = release
mcu-rp2350-ble-release-build: mcu-rp2350-ble-build ## &mcu Build Tufty/RP2350 BLE firmware in release mode

mcu-rp2350-ble-release-flash: MCU_RP2350_BLE_PROFILE = release
mcu-rp2350-ble-release-flash: mcu-rp2350-ble-flash ## &mcu Flash the current Tufty/RP2350 BLE release firmware over USB BOOTSEL

mcu-radium-xtensa-patch: ## &mcu Prepare a temporary radium patch for ESP-IDF Xtensa 64-bit atomic detection
	@rm -rf $(MCU_RADIUM_PATCH)
	@cp -R $(MCU_RADIUM_SRC) $(MCU_RADIUM_PATCH)
	@perl -0pi -e 's/"avr" => atomics = Atomics::NONE,/"avr" => atomics = Atomics::NONE,\n        "xtensa" => atomics.has_64 = false,/' $(MCU_RADIUM_PATCH)/build.rs

mcu-esp32s3-build: ## &mcu Build Waveshare ESP32-S3-Touch-LCD-2 firmware
	. $(MCU_ESP_EXPORT) && cd $(MCU_DIR) && cargo +esp build \
		-p openvm-mcu-esp32s3 \
		-Zbuild-std=core,alloc \
		--target $(MCU_ESP32S3_TARGET)

mcu-esp32s3-flash: mcu-esp32s3-build ## &mcu Flash Waveshare ESP32-S3-Touch-LCD-2 firmware over USB/JTAG serial
	. $(MCU_ESP_EXPORT) && espflash flash \
		--before usb-reset \
		--after hard-reset \
		--chip esp32s3 \
		--port $(MCU_ESP32S3_PORT) \
		$(MCU_ESP32S3_ELF)

mcu-esp32s3-flash-monitor: mcu-esp32s3-build ## &mcu Flash and monitor Waveshare ESP32-S3-Touch-LCD-2 firmware
	. $(MCU_ESP_EXPORT) && espflash flash \
		--before usb-reset \
		--after hard-reset \
		--monitor \
		--chip esp32s3 \
		--port $(MCU_ESP32S3_PORT) \
		$(MCU_ESP32S3_ELF)

mcu-esp32s3-espidf-build: mcu-radium-xtensa-patch ## &mcu Build ESP-IDF/std ESP32-S3 firmware with native Halo2/KZG verification
	. $(MCU_ESP_EXPORT) && cd $(MCU_DIR) && \
		CARGO_INCREMENTAL=0 \
		CARGO_PROFILE_DEV_DEBUG=0 \
		CARGO_PROFILE_DEV_OPT_LEVEL=1 \
		ESP_IDF_SDKCONFIG_DEFAULTS="$(MCU_ESP32S3_ESPIDF_SDKCONFIG_DEFAULTS)" \
		RUST_MIN_STACK=134217728 \
		cargo +esp build \
		-p openvm-mcu-esp32s3 \
		-Zbuild-std=std,panic_abort \
		--target $(MCU_ESP32S3_ESPIDF_TARGET) \
		--no-default-features \
		--features display-st7789,ble-transfer \
		--config 'patch.crates-io.radium.path="$(MCU_RADIUM_PATCH)"'

mcu-esp32s3-espidf-flash: mcu-esp32s3-espidf-build ## &mcu Flash ESP-IDF/std ESP32-S3 firmware over USB/JTAG serial
	. $(MCU_ESP_EXPORT) && espflash flash \
		--before usb-reset \
		--after watchdog-reset \
		--chip esp32s3 \
		--port $(MCU_ESP32S3_PORT) \
		$(MCU_ESP32S3_ESPIDF_ELF)

mcu-esp32s3-flash-halo2-std: mcu-evm-e2e ## &mcu Flash ESP32-S3 firmware that verifies Halo2/KZG on board
	@PV_LEN=$$(node -e 'const fs=require("fs"); const proof=JSON.parse(fs.readFileSync(process.argv[1], "utf8")); const hex=(proof.user_public_values || "").replace(/^0x/, ""); console.log((hex.length / 2) + " bytes");' $(MCU_EVM_PROOF_JSON)); \
	PROOF_SHA=$$(shasum -a 256 $(MCU_EVM_PROOF_JSON) | awk '{print substr($$1, 1, 16)}'); \
	ESP32S3_HOST_EVM_STATUS="native host verified" \
	ESP32S3_HOST_EVM_DETAIL="$(MCU_ESP32S3_HOST_EVM_DETAIL)" \
	ESP32S3_HOST_EVM_PROOF_SHA="$$PROOF_SHA" \
	ESP32S3_HOST_EVM_PUBLIC_VALUES="$$PV_LEN" \
	ESP32S3_VERIFIER_KEY="$(MCU_EVM_KEY)" \
	ESP32S3_PROOF_ENVELOPE="$(MCU_EVM_PROOF)" \
	$(MAKE) mcu-esp32s3-espidf-flash

mcu-esp32s3-flash-evm-status: mcu-esp32s3-flash-halo2-std ## &mcu Legacy alias for ESP32-S3 Halo2/KZG verification

mcu-evm-esp32s3-e2e: mcu-esp32s3-flash-halo2-std ## &mcu Generate and verify Halo2/KZG, then verify it on ESP32-S3

mcu-esp32s3-monitor: ## &mcu Monitor Waveshare ESP32-S3-Touch-LCD-2 USB/JTAG serial output
	. $(MCU_ESP_EXPORT) && espflash monitor \
		--chip esp32s3 \
		--port $(MCU_ESP32S3_PORT)

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------
clean: ## &misc Remove generated files
	rm -f $(LOG_FILE) $(COMMITMENT_FILE)
	rm -rf $(WEB_DIR)/dist
	rm -rf $(WEB_DIR)/node_modules
	rm -f $(WEB_DIR)/package-lock.json
	cd $(WEB_DIR)/crates/backend && cargo clean 2>/dev/null || true
