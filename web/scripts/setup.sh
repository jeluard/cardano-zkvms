#!/bin/bash
set -e

# Cardano ZKVMs - Remote Setup Script
# Idempotent setup for OpenVM backend deployment
# Usage: ./setup.sh <REPO_PATH> <OPENVM_GUEST_DIR> [FORCE_KEYGEN]

REPO_PATH="${1:-.}"
OPENVM_GUEST_DIR="${2:-crates/zkvms/openvm}"
FORCE_KEYGEN="${3:-0}"

# Construct full paths relative to REMOTE_PATH
GUEST_DIR="$REPO_PATH/$OPENVM_GUEST_DIR"
WEB_DATA_DIR="$REPO_PATH/web/data"
OPENVM_HOME="${HOME}/.openvm"
BACKEND_DIR="$REPO_PATH/web/crates/backend"
BACKEND_BIN="$BACKEND_DIR/target/release/cardano-zkvms"
CARDANO_ZKVMS="$REPO_PATH/cardano-zkvms"

echo ""
echo "⚙️  Cardano ZKVMs — Remote Setup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Check guest crate exists
if [ ! -d "$GUEST_DIR" ]; then
    echo "❌ Guest crate not found: $GUEST_DIR"
    exit 1
fi

# =========================================================================
# 1. Check/install Rust and build tools
# =========================================================================
echo "🔧 1. [remote] Checking Rust & build tools..."

if ! command -v cargo &> /dev/null; then
    echo "   Installing Rust..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --quiet >/dev/null 2>&1
    source $HOME/.cargo/env
    echo "   ✓ Rust installed"
else
    echo "   ✓ Rust ready"
fi

if ! command -v gcc &> /dev/null; then
    echo "   Installing build tools..."
    sudo apt-get update -qq >/dev/null 2>&1
    sudo apt-get install -y -qq build-essential pkg-config >/dev/null 2>&1
    echo "   ✓ Build tools installed"
else
    echo "   ✓ Build tools ready"
fi

# =========================================================================
# 2. Build backend binary from source (the single binary for serve + setup)
# =========================================================================
echo "🔨 2. [remote] Building backend binary..."

source $HOME/.cargo/env 2>/dev/null || true
cd "$REPO_PATH"
if cargo build --release --manifest-path "$BACKEND_DIR/Cargo.toml" 2>&1 | tail -5; then
    if [ -f "$BACKEND_BIN" ]; then
        cp "$BACKEND_BIN" "$CARDANO_ZKVMS"
        chmod +x "$CARDANO_ZKVMS"
        echo "   ✓ Built and installed"
    else
        echo "   ❌ Build completed but binary not found at $BACKEND_BIN"
        exit 1
    fi
else
    echo "   ❌ Failed to build backend"
    exit 1
fi

# =========================================================================
# 3. Run one-time setup: build guest, keygen, agg keygen (idempotent)
# =========================================================================
echo "🔑 3. [remote] OpenVM setup (build guest + keygen + agg keygen)..."

source $HOME/.cargo/env 2>/dev/null || true
OPENVM_GUEST_DIR_ABS="$REPO_PATH/$OPENVM_GUEST_DIR"
if OPENVM_GUEST_DIR="$OPENVM_GUEST_DIR_ABS" "$CARDANO_ZKVMS" setup 2>&1 | sed 's/^/   /'; then
    echo "   ✓ Setup complete"
else
    echo "   ⚠ Setup reported issues (non-fatal, check output above)"
fi

if [ "$FORCE_KEYGEN" = "1" ]; then
    echo "   Force keygen requested — removing existing keys..."
    rm -f "$REPO_PATH/target/openvm/app.pk"
    rm -f "$HOME/.openvm/agg_stark.pk"
    rm -f "$HOME/.openvm/agg_stark.vk"
    if OPENVM_GUEST_DIR="$OPENVM_GUEST_DIR_ABS" "$CARDANO_ZKVMS" setup 2>&1 | sed 's/^/   /'; then
        echo "   ✓ Re-keygen complete"
    else
        echo "   ⚠ Re-keygen reported issues"
    fi
fi

# =========================================================================
# 4. Verify verifying key
# =========================================================================
echo "🔍 4. [remote] Checking verifying key..."

VK_FILE="$HOME/.openvm/agg_stark.vk"
if [ -f "$VK_FILE" ]; then
    echo "   ✓ Found"
else
    echo "   ⚠ Not found (backend may not verify proofs)"
fi

# =========================================================================
# 5. Prepare deployment directories
# =========================================================================
echo "📁 5. [remote] Preparing deployment directories..."

mkdir -p "$WEB_DATA_DIR" 2>/dev/null || true
echo "   ✓ Ready"

echo ""
echo "✅ Setup complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
