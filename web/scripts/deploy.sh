#!/bin/bash
set -e

# Color codes for emoji-based output
RED='\033[0;31m'
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Script must be run from repository root
if [ ! -f "Makefile" ]; then
    echo "❌ Error: This script must be run from the repository root"
    exit 1
fi

# Load environment variables
if [ ! -f "web/conf/.env" ]; then
    echo "❌ .env file not found at web/conf/.env"
    exit 1
fi
source web/conf/.env

# Check if required variables are set
for var in SSH_HOST REMOTE_PATH OPENVM_GUEST_DIR OPENVM_STATIC_DIR CADDY_DOMAIN CADDY_PORT BACKEND_PORT; do
    if [ -z "${!var}" ]; then
        echo "❌ Error: $var is not set in web/conf/.env"
        exit 1
    fi
done

# Construct SSH destination and options
SSH_DEST="${SSH_USER:+$SSH_USER@}$SSH_HOST"
if [ -n "$SSH_KEY_PATH" ]; then
    SSH_KEY_PATH="${SSH_KEY_PATH/#\~/$HOME}"  # Expand ~ to home directory
    SSH_OPTS="-i $SSH_KEY_PATH"
else
    SSH_OPTS=""
fi
# Add SSH multiplexing options
SSH_OPTS="$SSH_OPTS -o ControlMaster=auto -o ControlPath=/tmp/ssh-deploy-%r@%h:%p -o ControlPersist=120"

echo ""
echo "🚀 Deploying Cardano ZKVMs Backend & Caddy"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Verify required files exist
BACKEND_BIN="web/crates/backend/target/release/cardano-zkvms"
if [ ! -f "$BACKEND_BIN" ]; then
    echo "❌ Backend binary not found at $BACKEND_BIN"
    echo "   Run 'make backend-build' first"
    exit 1
fi

if [ ! -f "web/conf/cardano-zkvms.service.template" ]; then
    echo "❌ Service template not found"
    exit 1
fi

if [ ! -f "web/scripts/setup.sh" ]; then
    echo "❌ Setup script not found"
    exit 1
fi

if [ ! -f "web/conf/Caddyfile.template" ]; then
    echo "❌ Caddyfile template not found"
    exit 1
fi

if [ -n "$SSH_KEY_PATH" ]; then
    echo "🔐 SSH multiplexing enabled (single passphrase)"
    echo ""
fi

# Step 1: Create deployment directory
echo "📦 1. Creating deployment directory..."
ssh $SSH_OPTS $SSH_DEST "mkdir -p $REMOTE_PATH"

# Step 2: Send artifacts
echo "📤 2. Sending artifacts..."
ssh $SSH_OPTS $SSH_DEST "mkdir -p $REMOTE_PATH/crates/zkvms/openvm/target/riscv32im-risc0-zkvm-elf $REMOTE_PATH/web/data $REMOTE_PATH/web/crates/backend"
# Send guest artifacts if they exist locally
if [ -d "crates/zkvms/openvm/target/riscv32im-risc0-zkvm-elf/release" ] && [ -n "$(ls -A crates/zkvms/openvm/target/riscv32im-risc0-zkvm-elf/release 2>/dev/null)" ]; then
    scp $SSH_OPTS -r crates/zkvms/openvm/target/riscv32im-risc0-zkvm-elf/release $SSH_DEST:$REMOTE_PATH/crates/zkvms/openvm/target/riscv32im-risc0-zkvm-elf/ 2>/dev/null
fi
scp $SSH_OPTS crates/zkvms/openvm/Cargo.toml $SSH_DEST:$REMOTE_PATH/crates/zkvms/openvm/ 2>/dev/null
scp $SSH_OPTS web/scripts/setup.sh $SSH_DEST:$REMOTE_PATH/setup.sh 2>/dev/null
# Send backend source code for remote builds
scp $SSH_OPTS -r web/crates/backend/src $SSH_DEST:$REMOTE_PATH/web/crates/backend/ 2>/dev/null
scp $SSH_OPTS web/crates/backend/Cargo.toml $SSH_DEST:$REMOTE_PATH/web/crates/backend/ 2>/dev/null
scp $SSH_OPTS web/crates/backend/Cargo.lock $SSH_DEST:$REMOTE_PATH/web/crates/backend/ 2>/dev/null
if [ -f "web/data/agg_stark.vk" ]; then
    scp $SSH_OPTS web/data/agg_stark.vk $SSH_DEST:$REMOTE_PATH/web/data/ 2>/dev/null
fi

# Step 3: Run setup
echo "⚙️  3. Running setup..."
ssh $SSH_OPTS $SSH_DEST "bash $REMOTE_PATH/setup.sh $REMOTE_PATH $OPENVM_GUEST_DIR $OPENVM_STATIC_DIR"

# Step 4: Install Caddy
echo "🔧 4. Installing Caddy..."
ssh $SSH_OPTS $SSH_DEST "command -v caddy >/dev/null 2>&1 || (sudo apt-get update -qq && sudo apt-get install -y -qq caddy) || echo '⚠️  Caddy installation skipped'"

# Step 5: Deploy backend binary
echo "💾 5. Deploying backend binary..."
scp $SSH_OPTS "$BACKEND_BIN" $SSH_DEST:$REMOTE_PATH/cardano-zkvms 2>/dev/null
ssh $SSH_OPTS $SSH_DEST "chmod +x $REMOTE_PATH/cardano-zkvms"

# Step 6: Install systemd service
echo "🎯 6. Installing systemd service..."
sed "s|\${REMOTE_PATH}|$REMOTE_PATH|g" web/conf/cardano-zkvms.service.template | \
    sed "s|\${OPENVM_GUEST_DIR}|$OPENVM_GUEST_DIR|g" | \
    sed "s|\${OPENVM_STATIC_DIR}|$OPENVM_STATIC_DIR|g" | \
    sed "s|\${BACKEND_PORT}|$BACKEND_PORT|g" | \
    ssh $SSH_OPTS $SSH_DEST "sudo tee /etc/systemd/system/cardano-zkvms.service >/dev/null"
ssh $SSH_OPTS $SSH_DEST "sudo systemctl daemon-reload"

# Step 7: Setup reverse proxy
echo "🔀 7. Setting up reverse proxy..."
sed "s|\${CADDY_DOMAIN}|$CADDY_DOMAIN|g" web/conf/Caddyfile.template | \
    sed "s|\${CADDY_PORT}|$CADDY_PORT|g" | \
    sed "s|\${BACKEND_PORT}|$BACKEND_PORT|g" | \
    ssh $SSH_OPTS $SSH_DEST "sudo tee /etc/caddy/Caddyfile >/dev/null"

# Step 8: Start services
echo "🎬 8. Starting services..."
ssh $SSH_OPTS $SSH_DEST "sudo systemctl enable cardano-zkvms caddy && sudo systemctl restart caddy cardano-zkvms"

# Summary
echo ""
echo "✅ Deployment Complete!"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "📊 Status:"
ssh $SSH_OPTS $SSH_DEST "sudo systemctl status cardano-zkvms --no-pager | head -3"
ssh $SSH_OPTS $SSH_DEST "sudo systemctl status caddy --no-pager | head -3"

echo ""
echo "📋 View logs:"
echo "  Backend logs:  ssh $SSH_DEST sudo journalctl -u cardano-zkvms -f"
echo "  Caddy logs:    ssh $SSH_DEST sudo journalctl -u caddy -f"
echo ""
echo "🌐 Access: https://$CADDY_DOMAIN:$CADDY_PORT"
echo ""
