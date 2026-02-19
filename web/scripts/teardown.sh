#!/bin/bash
set -e

# Script must be run from repository root
if [ ! -f "Makefile" ]; then
    echo "❌ Error: This script must be run from the repository root"
    exit 1
fi

if [ ! -f "web/conf/.env" ]; then
    echo "❌ .env file not found at web/conf/.env"
    exit 1
fi
source web/conf/.env

SSH_DEST="${SSH_USER:+$SSH_USER@}$SSH_HOST"
if [ -n "$SSH_KEY_PATH" ]; then
    SSH_KEY_PATH="${SSH_KEY_PATH/#\~/$HOME}"
    SSH_OPTS="-i $SSH_KEY_PATH"
else
    SSH_OPTS=""
fi
SSH_OPTS="$SSH_OPTS -o ControlMaster=auto -o ControlPath=/tmp/ssh-deploy-%r@%h:%p -o ControlPersist=120"

echo ""
echo "🧹 Tearing down Cardano ZKVMs on $SSH_DEST"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "This will:"
echo "  • Stop and remove cardano-zkvms systemd service"
echo "  • Remove Caddyfile configuration"
echo "  • Delete deployment directory ($REMOTE_PATH)"
echo "  • Delete OpenVM keys (~/.openvm)"
echo ""
read -p "Continue? [y/N] " confirm
if [ "$confirm" != "y" ] && [ "$confirm" != "Y" ]; then
    echo "Aborted."
    exit 0
fi

echo "Stopping services..."
ssh $SSH_OPTS $SSH_DEST "sudo systemctl stop cardano-zkvms 2>/dev/null; sudo systemctl disable cardano-zkvms 2>/dev/null; sudo rm -f /etc/systemd/system/cardano-zkvms.service; sudo systemctl daemon-reload"

echo "Removing Caddyfile..."
ssh $SSH_OPTS $SSH_DEST "sudo rm -f /etc/caddy/Caddyfile; sudo systemctl reload caddy 2>/dev/null || true"

echo "Removing deployment directory..."
ssh $SSH_OPTS $SSH_DEST "rm -rf $REMOTE_PATH"

echo "Removing OpenVM keys..."
ssh $SSH_OPTS $SSH_DEST "rm -rf ~/.openvm"

echo ""
echo "✅ Teardown complete."
