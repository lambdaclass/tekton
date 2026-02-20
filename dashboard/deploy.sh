#!/usr/bin/env bash
set -euo pipefail

SERVER="root@<server-ip>"
REMOTE_DIR="/opt/dashboard"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

echo "=== Building frontend ==="
cd "$SCRIPT_DIR/frontend"
npm run build

echo ""
echo "=== Building backend (cross-compile for x86_64-linux-musl) ==="
cd "$SCRIPT_DIR/backend"

# Check for cross-compilation target
if ! rustup target list --installed | grep -q x86_64-unknown-linux-musl; then
    echo "Adding musl target..."
    rustup target add x86_64-unknown-linux-musl
fi

cargo build --release --target x86_64-unknown-linux-musl

echo ""
echo "=== Deploying to server ==="

# Create remote directory structure
ssh "$SERVER" "mkdir -p $REMOTE_DIR/static"

# Upload binary
scp "$SCRIPT_DIR/backend/target/x86_64-unknown-linux-musl/release/dashboard" "$SERVER:$REMOTE_DIR/dashboard"

# Upload frontend build
scp -r "$SCRIPT_DIR/frontend/dist/"* "$SERVER:$REMOTE_DIR/static/"

echo ""
echo "=== Restarting service ==="
ssh "$SERVER" "systemctl restart dashboard"

echo ""
echo "=== Checking status ==="
ssh "$SERVER" "systemctl status dashboard --no-pager" || true

echo ""
echo "=== Done! Dashboard should be available at https://dashboard.hipermegared.link ==="
