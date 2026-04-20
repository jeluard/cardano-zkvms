#!/bin/bash

set -eo pipefail
trap 'echo ""; echo "❌ Interrupted."; exit 130' INT

# Script must be run from repository root
if [ ! -f "Makefile" ]; then
    echo "❌ Error: This script must be run from the repository root"
    exit 1
fi

# Load environment variables
if [ ! -f "conf/.env" ]; then
    echo "❌ .env file not found at conf/.env"
    exit 1
fi
source conf/.env

# Check if required variables are set
for var in SSH_HOST REMOTE_PATH OPENVM_GUEST_DIR CADDY_DOMAIN CADDY_PORT BACKEND_PORT; do
    if [ -z "${!var}" ]; then
        echo "❌ Error: $var is not set in conf/.env"
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

if [ ! -f "conf/cardano-zkvms.service.template" ]; then
    echo "❌ Service template not found"
    exit 1
fi

if [ ! -f "scripts/setup.sh" ]; then
    echo "❌ Setup script not found"
    exit 1
fi

if [ ! -f "conf/Caddyfile.template" ]; then
    echo "❌ Caddyfile template not found"
    exit 1
fi

if [ -n "$SSH_KEY_PATH" ]; then
    echo "🔐 SSH multiplexing enabled (single passphrase)"
    echo ""
fi

# Step 1: Sync source code to remote
echo "📤 1. [local → remote] Syncing source code..."
RSYNC_SSH="ssh $SSH_OPTS"
rsync -az --delete -e "$RSYNC_SSH" \
    --include='Cargo.toml' \
    --include='crates/' \
    --include='crates/uplc/***' \
    --include='crates/zkvms/' \
    --include='crates/zkvms/openvm/' \
    --include='crates/zkvms/openvm/Cargo.toml' \
    --include='crates/zkvms/openvm/openvm.toml' \
    --include='crates/zkvms/openvm/guest/' \
    --include='crates/zkvms/openvm/guest/Cargo.toml' \
    --include='crates/zkvms/openvm/guest/src/***' \
    --include='crates/zkvms/openvm/core/' \
    --include='crates/zkvms/openvm/core/Cargo.toml' \
    --include='crates/zkvms/openvm/core/src/***' \
    --include='scripts/' \
    --include='scripts/setup.sh' \
    --include='web/' \
    --include='web/crates/' \
    --include='web/crates/backend/' \
    --include='web/crates/backend/Cargo.toml' \
    --include='web/crates/backend/Cargo.lock' \
    --include='web/crates/backend/src/***' \
    --exclude='*' \
    ./ "$SSH_DEST:$REMOTE_PATH/"
# setup.sh is expected at the root by the deploy flow
rsync -az -e "$RSYNC_SSH" scripts/setup.sh "$SSH_DEST:$REMOTE_PATH/setup.sh"


# Step 2: Run setup
echo "⚙️  2. [remote] Running setup (this may take a while for first build)..."
if [ "${FORCE_KEYGEN:-0}" = "1" ]; then
    echo "   🔑 Force key regeneration enabled (FORCE_KEYGEN=1)"
fi
if ! ssh $SSH_OPTS $SSH_DEST "bash $REMOTE_PATH/setup.sh $REMOTE_PATH $OPENVM_GUEST_DIR ${FORCE_KEYGEN:-0}"; then
    echo "⚠️  Setup script reported issues. Building services anyway..."
fi

# Step 3: Install Caddy
echo "🔧 3. [remote] Installing Caddy..."
ssh $SSH_OPTS $SSH_DEST "command -v caddy >/dev/null 2>&1 || (sudo apt-get update -qq && sudo apt-get install -y -qq caddy) || echo '⚠️  Caddy installation skipped'"

# Step 4: Install systemd service
echo "🎯 4. [local → remote] Installing systemd service..."
sed "s|\${REMOTE_PATH}|$REMOTE_PATH|g" conf/cardano-zkvms.service.template | \
    sed "s|\${OPENVM_GUEST_DIR}|$OPENVM_GUEST_DIR|g" | \
    sed "s|\${BACKEND_PORT}|$BACKEND_PORT|g" | \
    ssh $SSH_OPTS $SSH_DEST "sudo tee /etc/systemd/system/cardano-zkvms.service >/dev/null"
ssh $SSH_OPTS $SSH_DEST "sudo systemctl daemon-reload"

# Step 5: Setup reverse proxy
echo "🔀 5. [local → remote] Setting up reverse proxy..."
sed "s|\${CADDY_DOMAIN}|$CADDY_DOMAIN|g" conf/Caddyfile.template | \
    sed "s|\${CADDY_PORT}|$CADDY_PORT|g" | \
    sed "s|\${BACKEND_PORT}|$BACKEND_PORT|g" | \
    ssh $SSH_OPTS $SSH_DEST "sudo tee /etc/caddy/Caddyfile >/dev/null"

# Step 6: Start services
echo "🎬 6. [remote] Starting services..."
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
echo "  Backend logs:  make backend-logs"
echo "  Caddy logs:    make caddy-logs"
echo ""
echo "🌐 Access: https://$CADDY_DOMAIN:$CADDY_PORT"
echo ""