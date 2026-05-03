A set of experimentation around ZKVMs and Cardano. The goal is to generate a proof of execution of a UPLC contract that can be verified on-chain.

## Quick Start

### Building

```bash
# Clone and build
git clone https://github.com/jeluard/cardano-zkvms.git
cd cardano-zkvms

# Default build (uses uplc-turbo-riscv)
make all

# Build with specific backend
cd crates/uplc && cargo build --no-default-features --features uplc-turbo
```

### Using the Abstraction Layer

The `uplc` crate provides a unified interface for UPLC evaluation:

```rust
use uplc::UplcEvaluator;

let evaluator = uplc::get_evaluator();
let program_hex = "c00000"; // flat-encoded UPLC program
let result = evaluator.evaluate_program(&program_hex)?;
println!("Result: {}", result);
println!("Implementation: {}", evaluator.version());
```

## Development

### Switching UPLC Implementations

Edit the feature flags in:

- `crates/zkvms/openvm/guest/Cargo.toml` - for OpenVM guest
- Any crate depending on `uplc`

Example:

```toml
# Switch from uplc-turbo-riscv to uplc-turbo
uplc = { path = "../../../uplc", features = ["uplc-turbo"] }
```

# Web

A simple web UI that evaluates UPLC locally in the browser, sends the program to the backend for proof generation, then performs the final STARK verification locally in the browser via WASM.

The browser verifier needs `agg_stark.vk`. It first tries the deployed static asset, then falls back to the configured backend at `/data/agg_stark.vk`, which keeps the GitHub Pages deployment working even when the key is not bundled into the static site.

# MCU Verification

`crates/zkvms/openvm/mcu/` contains the embedded-facing OpenVM verifier workspace:

- `protocol`: `no_std` proof/key envelopes and framed wire format,
- `verifier-core`: shared verifier boundary, std OpenVM STARK and EVM/Halo2 verification adapters, and fail-closed no-std crypto behavior,
- `device-app`: board-independent proof handling state machine,
- `host-tools`: desktop packer for OpenVM STARK/EVM proof and verifier key payloads,
- `boards/rp2350`, `boards/esp32s3`: target entry crates.

Useful commands:

```bash
make mcu-check
make mcu-nostd-check
make mcu-test
make mcu-tool
```

For an operator-style terminal view of the current MCU artifacts, run:

```bash
make mcu-tui
```

By default this opens a Ratatui dashboard for `MCU_EVM_KEY` and `MCU_EVM_PROOF`, runs the native host verifier, and shows proof metadata plus BLE identifiers when available. To inspect a fresh web backend MCU response directly:

```bash
make mcu-tui MCU_TUI_ARGS="--backend-response /tmp/openvm-web-mcu-halo2.json"
```

Inside the dashboard, press `r` to reload artifacts and `q` to quit.

To generate and verify a real OpenVM Halo2/KZG proof on the desktop host path, run:

```bash
make mcu-evm-e2e
```

That target builds the OpenVM guest, converts `MCU_PROGRAM_HEX` into flat bytes, runs the real OpenVM Halo2/KZG prover, and checks the generated wrapper proof with the native Halo2/KZG verifier. It does not compile Solidity, execute bytecode, or use `revm`.

To verify that proof on the Waveshare ESP32-S3 itself and render proof details on the onboard display, plug the board in and run:

```bash
make mcu-esp32s3-flash-halo2-std
```

That target generates and host-verifies the OpenVM Halo2/KZG proof, packs the verifier key and proof envelope, builds the ESP-IDF/std firmware with the real proof embedded, flashes the first detected `/dev/cu.usbmodem*` port, and the board verifies the proof on silicon. The expected serial markers are:

```text
openvm-mcu-esp32s3-espidf: proof_status=verified
openvm-mcu-esp32s3-espidf: display updated
```

The legacy convenience target still points at the same real on-device verifier path:

```bash
make mcu-evm-esp32s3-e2e
```

The screen reports the native host result as metadata and keeps the MCU crypto line tied to the on-device verifier result. It does not draw a green proof state unless the ESP32-S3 firmware verifier accepts the embedded proof.

The web UI also has a BLE path for MCU verification. Start the backend and web UI from a secure browser context (`localhost`, Chrome, or Edge), compile Aiken or provide UPLC hex, then use the `MCU BLE` step. The backend endpoint `/api/prove/mcu-halo2` generates an OpenVM Halo2/KZG proof, natively checks it on the host, packs the `VerifierKey` and `ProofEnvelope`, and the browser sends those envelopes to the ESP32-S3 over BLE. The firmware verifies the received proof with the same native Halo2/KZG verifier on the MCU and returns the status to the web UI. The onboard ST7789 LCD uses Ratatui rendered through mousefood's embedded-graphics backend for BLE receive progress, verifier state, and final proof metadata; the separate `mcu-tui` target remains a host terminal dashboard.

The ESP32-S3 verifier task uses a PSRAM-backed FreeRTOS stack. Keep `CONFIG_SPIRAM_ALLOW_STACK_EXTERNAL_MEMORY` enabled and create the verifier task with `xTaskCreatePinnedToCoreWithCaps(..., MALLOC_CAP_SPIRAM)`; large Rust `std::thread` stacks can fail to spawn from internal RAM before verification starts.

BLE identifiers:

```text
service  7b7c0001-78f1-4f9a-8b29-6f1f1d95a100
control  7b7c0002-78f1-4f9a-8b29-6f1f1d95a100
data     7b7c0003-78f1-4f9a-8b29-6f1f1d95a100
status   7b7c0004-78f1-4f9a-8b29-6f1f1d95a100
name     OpenVM MCU
```

If an experimental image wedges before `app_main()` and `espflash` cannot reconnect, force ROM download mode manually using the Waveshare recovery sequence: hold `BOOT`, plug in USB or tap `RESET`, then release `BOOT` after the USB port appears. If `espflash monitor --before no-reset-no-sync` reports `Secure Download Mode is enabled on this chip`, the board is not in the normal writable download mode over the USB/JTAG serial path; reset normally or repeat the recovery sequence before flashing again.

For pre-generated artifacts, provide the OpenVM proof JSON and generated native verifier metadata, then run:

```bash
make mcu-evm-verify \
	MCU_EVM_VERIFIER=/path/to/native-verifier.json \
	MCU_EVM_PROOF_JSON=/path/to/evm-proof.json
```

The Halo2/KZG prover is expensive. To avoid regenerating the heavier layers every run, pass existing keys with `MCU_EVM_APP_PK`, `MCU_EVM_AGG_PK`, `MCU_EVM_ROOT_PK`, and `MCU_EVM_HALO2_PK`, or persist newly generated root/Halo2 keys with `MCU_EVM_WRITE_ROOT_PK` and `MCU_EVM_WRITE_HALO2_PK`.

The no-std board targets still need a no-std verifier backend before they can accept a proof directly. The ESP32-S3 ESP-IDF/std target is the validated real-silicon path for native Halo2/KZG verification today.

The ESP32-S3 target is wired for the Waveshare ESP32-S3-Touch-LCD-2. It reports the current proof status over USB/JTAG serial and on the onboard ST7789 SPI display. The onboard CST816 touch controller is I2C-wired on GPIO47/GPIO48 with interrupt on GPIO46; touch input is reserved for status paging once the proof verifier has more states to inspect.

## Building & Running

```bash
cd web
npm install
npm run dev       # Start development server with watch mode
npm run build     # Debug build
npm run build:prod  # Production build with minification
```

## Deployment

The web UI is automatically deployed to GitHub Pages via GitHub Actions on every push to `main`.

The configured backend must also expose `/api/health`, `/api/prove`, and `/data/agg_stark.vk`.

Backend deployment helpers now live at the repository root so `web/` only contains the frontend and backend application code:

- `scripts/deploy.sh`
- `scripts/setup.sh`
- `scripts/teardown.sh`
- `scripts/build-backend-remote.sh`
- `conf/.env.example`
- `conf/Caddyfile.template`
- `conf/cardano-zkvms.service.template`

Start from `conf/.env.example` and write your local values to `conf/.env` before using the deployment targets:

```bash
SSH_HOST=your-server
REMOTE_PATH=/opt/cardano-zkvms
OPENVM_GUEST_DIR=crates/zkvms/openvm
CADDY_DOMAIN=zk.example.com
CADDY_PORT=443
BACKEND_PORT=8080

# Optional
SSH_USER=ubuntu
SSH_KEY_PATH=~/.ssh/id_ed25519
BACKEND_URL=https://zk.example.com
```

Useful commands:

```bash
make backend-build
make backend-deploy
make backend-rekey
make backend-teardown
make gh-secrets
make backend-logs
make caddy-logs
```

Run the deploy scripts from the repository root; the Makefile targets above already do that.

If you already have a local web/conf/.env, move it to conf/.env.
