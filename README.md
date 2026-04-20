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

A simple web UI that evaluates UPLC locally in the browser, then proves and verifies execution via the backend.

With OpenVM 2.0 beta, STARK proof verification now runs through the backend's native verifier instead of a browser-local WASM verifier.

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
