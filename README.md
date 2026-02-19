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

A simple web UI that allows to prove some UPLC program execution then verify it in your browser.

Leverages [OpenVM verify-stark](https://github.com/openvm-org/openvm/tree/feat/v1-verify-stark/crates/verify-stark) compiled to WASM for in-browser STARK proof verification.

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
