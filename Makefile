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
# Web: Build WASM modules and serve the browser verifier
# ---------------------------------------------------------------------------
#
# The web/ directory (at project root) contains:
#   • uplc-wasm/  — a Rust→WASM crate that evaluates UPLC programs in the browser
#   • pkg/        — the compiled WASM output (built by wasm-pack)
#   • stark-pkg/  — pre-built OpenVM STARK verifier WASM (from npm)
#   • index.html  — the single-page verification UI
#
# `make web-build` compiles uplc-wasm to WASM.
# `make web-serve` starts a local HTTP server on port 8080.
#

WEB_DIR := $(ROOT_DIR)/web

.PHONY: web-build web-serve web web-backend web-backend-build

web-build: ## Build the UPLC WASM module for the browser verifier
	@echo "──────────────────────────────────────────────"
	@echo " Building UPLC WASM module"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/uplc-wasm && wasm-pack build --target web --out-dir ../pkg
	@echo ""

web-serve: ## Serve the web verifier on http://localhost:8080 (static only, no proof generation)
	@echo "──────────────────────────────────────────────"
	@echo " Serving web verifier at http://localhost:8080"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR) && python3 -m http.server 8080

web-backend-build: ## Build the Rust HTTP backend
	@echo "──────────────────────────────────────────────"
	@echo " Building web backend"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/backend && cargo build --release
	@echo ""

web-backend: web-build web-backend-build ## Build WASM + backend, then serve with proof generation
	@echo "──────────────────────────────────────────────"
	@echo " Serving web with backend at http://localhost:8080"
	@echo " Proof generation enabled via /api/prove"
	@echo "──────────────────────────────────────────────"
	cd $(WEB_DIR)/backend && OPENVM_GUEST_DIR="$$(cd ../.. && pwd)" \
		OPENVM_VK_PATH="$(ROOT_DIR)/target/openvm/app.vk" \
		OPENVM_STATIC_DIR="$(WEB_DIR)" \
		cargo run --release

web: web-build web-serve ## Build WASM and serve the web verifier (static only)

# ---------------------------------------------------------------------------
# Cleanup
# ---------------------------------------------------------------------------
clean: ## Remove generated files
	rm -f $(GUEST_DIR)/data/input.json $(LOG_FILE) $(COMMITMENT_FILE)
	cd $(GUEST_DIR)/verify && cargo clean
	rm -rf $(WEB_DIR)/pkg
	cd $(WEB_DIR)/backend && cargo clean 2>/dev/null || true
