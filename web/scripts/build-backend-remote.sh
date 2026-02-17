#!/bin/bash
set -e

echo "🔨 Building backend on remote server..."

# SSH to remote and build
ssh -i ~/.ssh/id_ovh -o ControlMaster=auto -o ControlPath=/tmp/ssh-deploy-%r@%h:%p -o ControlPersist=120 ovh bash << 'EOF'
set -e

# Set up environment
export PATH="$HOME/.cargo/bin:$PATH"

# Verify cargo is available
if ! command -v cargo &> /dev/null; then
    echo "❌ cargo not found. Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --no-modify-path
    source $HOME/.cargo/env
fi

echo "✓ Rust ready"
echo "  cargo: $(cargo --version)"

# Navigate to backend source and build
cd $HOME/cardano-zkvms/web/crates/backend || { echo "❌ Backend source not found at $HOME/cardano-zkvms/web/crates/backend"; exit 1; }
echo "🔨 Building backend (this may take a few minutes)..."
cargo build --release 2>&1 | grep -v "^    Downloading\|^       Compiling\|^Checking\|^Thinking\|Running\|^   Compiling" || true

# Check result
if [ -f target/release/cardano-zkvms ]; then
    SIZE=$(ls -lh target/release/cardano-zkvms | awk '{print $5}')
    echo "✅ Backend built successfully: $SIZE"
    echo "   Binary: $(file target/release/cardano-zkvms | cut -d: -f2-)"
    
    # Copy to deployment location
    cp target/release/cardano-zkvms $HOME/cardano-zkvms/cardano-zkvms
    chmod +x $HOME/cardano-zkvms/cardano-zkvms
    echo "✓ Copied to deployment location"
else
    echo "❌ Build failed - binary not found"
    exit 1
fi
EOF

