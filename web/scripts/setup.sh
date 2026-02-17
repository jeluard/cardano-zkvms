#!/bin/bash
set -e

# Cardano ZKVMs - Remote Setup Script
# Idempotent setup for OpenVM backend deployment
# Usage: ./setup.sh <REPO_PATH> <OPENVM_GUEST_DIR> <OPENVM_STATIC_DIR>

REPO_PATH="${1:-.}"
OPENVM_GUEST_DIR="${2:-crates/zkvms/openvm}"
OPENVM_STATIC_DIR="${3:-web/dist}"

# Construct full paths relative to REMOTE_PATH
GUEST_DIR="$REPO_PATH/$OPENVM_GUEST_DIR"
WEB_DATA_DIR="$REPO_PATH/web/data"
OPENVM_HOME="${HOME}/.openvm"

echo "=========================================="
echo " Cardano ZKVMs - Backend Setup"
echo "=========================================="
echo ""

# Check guest crate exists
if [ ! -d "$GUEST_DIR" ]; then
    echo "❌ Guest crate not found: $GUEST_DIR"
    exit 1
fi

# =========================================================================
# 1. Check/install Rust and build tools
# =========================================================================
echo "[1/5] Rust & build tools"

if ! command -v cargo &> /dev/null; then
    echo "  Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet >/dev/null 2>&1
    source $HOME/.cargo/env
    echo "  ✓ Rust installed"
else
    echo "  ✓ Rust ready"
fi

if ! command -v gcc &> /dev/null; then
    echo "  Installing build tools..."
    sudo apt-get update -qq >/dev/null 2>&1
    sudo apt-get install -y -qq build-essential pkg-config >/dev/null 2>&1
    echo "  ✓ Build tools installed"
else
    echo "  ✓ Build tools ready"
fi

# =========================================================================
# 2. Install cargo-openvm (idempotent)
# =========================================================================
echo "[2/5] cargo-openvm"

source $HOME/.cargo/env 2>/dev/null || true

if cargo openvm --version >/dev/null 2>&1 || [ -f "$HOME/.cargo/bin/cargo-openvm" ]; then
    echo "  ✓ Already installed"
else
    echo "  Installing from git..."
    if cargo +1.90 install --locked --git https://github.com/openvm-org/openvm.git --tag v1.5.0 cargo-openvm >/dev/null 2>&1; then
        source $HOME/.cargo/env 2>/dev/null || true
        echo "  ✓ Installed"
    elif cargo install --locked --git https://github.com/openvm-org/openvm.git --tag v1.5.0 cargo-openvm >/dev/null 2>&1; then
        source $HOME/.cargo/env 2>/dev/null || true
        echo "  ✓ Installed"
    else
        echo "  ❌ Installation failed"
        exit 1
    fi
fi

# =========================================================================
# 3. Build guest if needed (idempotent)
# =========================================================================
echo "[3/5] Guest artifacts"

GUEST_RELEASE_DIR="$GUEST_DIR/target/riscv32im-risc0-zkvm-elf/release"
if [ -d "$GUEST_RELEASE_DIR" ] && [ -n "$(ls -A $GUEST_RELEASE_DIR 2>/dev/null)" ]; then
    echo "  ✓ Already built"
else
    if [ ! -f "$GUEST_DIR/Cargo.toml" ]; then
        echo "  ❌ Cargo.toml not found: $GUEST_DIR/Cargo.toml"
        exit 1
    fi
    echo "  Building (this may take a while)..."
    source $HOME/.cargo/env 2>/dev/null || true
    cd "$GUEST_DIR"
    
    # Try multiple ways to build the guest
    BUILD_SUCCESS=0
    
    # Method 1: Try cargo openvm build
    if cargo openvm build >/dev/null 2>&1; then
        BUILD_SUCCESS=1
    fi
    
    # Method 2: Try direct binary if method 1 failed
    if [ $BUILD_SUCCESS -eq 0 ] && [ -x "$HOME/.cargo/bin/cargo-openvm" ]; then
        if $HOME/.cargo/bin/cargo-openvm build >/dev/null 2>&1; then
            BUILD_SUCCESS=1
        fi
    fi
    
    # Check if artifacts exist (success indicator)
    if [ $BUILD_SUCCESS -eq 1 ] || ([ -d "$GUEST_RELEASE_DIR" ] && [ -n "$(ls -A $GUEST_RELEASE_DIR 2>/dev/null)" ]); then
        echo "  ✓ Built"
    else
        echo "  ⚠ Could not build guest locally"
        echo "    Backend can still run, but guests won't be verified"
        echo "    To build the guest, ensure cargo-openvm is properly installed on this machine"
    fi
fi

# =========================================================================
# 4. Verify verifying key (optional)
# =========================================================================
echo "[4/5] Verifying key"

VK_FILE="$REPO_PATH/web/data/agg_stark.vk"
if [ -f "$VK_FILE" ]; then
    echo "  ✓ Found"
else
    echo "  ⚠ Not found (backend may not verify proofs)"
fi

# =========================================================================
# 5. Prepare deployment directories
# =========================================================================
echo "[5/5] Deployment directories"

mkdir -p "$WEB_DATA_DIR" 2>/dev/null || true
echo "  ✓ Ready"

echo ""
echo "=========================================="
echo " ✓ Setup Complete!"
echo "=========================================="
echo ""
