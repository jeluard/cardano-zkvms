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
- `crates/zkvms/openvm/Cargo.toml` - for OpenVM guest
- Any crate depending on `uplc`

Example:
```toml
# Switch from uplc-turbo-riscv to uplc-turbo
uplc = { path = "../../uplc", features = ["uplc-turbo"] }
```

# Web

Leverages [openvm-wasm-stark-verifier](https://github.com/ethproofs/openvm-wasm-stark-verifier)