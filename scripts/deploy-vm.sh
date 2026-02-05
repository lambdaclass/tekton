#!/usr/bin/env bash
# Deploy a microvm to the NixOS server.
# Usage: ./scripts/deploy-vm.sh <vm-name> <server-ip> [--clone <repo-url>]
set -euo pipefail

usage() {
  echo "Usage: $0 <vm-name> <server-ip> [--clone <repo-url>]"
  echo ""
  echo "Deploys a microvm configuration to the server and starts it."
  echo ""
  echo "Arguments:"
  echo "  vm-name      Name of the microvm (must match a microvm-<name>.nix file)"
  echo "  server-ip    IP address of the NixOS server"
  echo "  --clone URL  (Optional) Clone a git repo into the VM's workspace"
  exit 1
}

if [ $# -lt 2 ]; then
  usage
fi

VM_NAME="$1"
SERVER_IP="$2"
CLONE_URL=""

shift 2
while [ $# -gt 0 ]; do
  case "$1" in
    --clone)
      CLONE_URL="$2"
      shift 2
      ;;
    *)
      echo "Unknown argument: $1"
      usage
      ;;
  esac
done

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VM_CONFIG="$REPO_ROOT/server-config/microvm-${VM_NAME}.nix"

if [ ! -f "$VM_CONFIG" ]; then
  echo "Error: VM config file not found: $VM_CONFIG"
  echo "Create the config first (or use the create-microvm skill)."
  exit 1
fi

echo "==> Deploying microvm '$VM_NAME' to $SERVER_IP"

# 1. Copy server config to the server
echo "==> Copying server-config to /etc/nixos/ on $SERVER_IP..."
scp -r "$REPO_ROOT/server-config/." "root@${SERVER_IP}:/etc/nixos/"

# 2. Create workspace and SSH host key directories
echo "==> Creating workspace and SSH host key directories..."
ssh "root@${SERVER_IP}" "mkdir -p /var/lib/microvms/${VM_NAME}/workspace /var/lib/microvms/${VM_NAME}/ssh-host-keys"

# 3. Rebuild NixOS
echo "==> Running nixos-rebuild switch..."
ssh "root@${SERVER_IP}" "cd /etc/nixos && nixos-rebuild switch"

# 4. Start the VM
echo "==> Starting microvm@${VM_NAME}..."
ssh "root@${SERVER_IP}" "systemctl start microvm@${VM_NAME}"

# 5. Optionally clone a repo
if [ -n "$CLONE_URL" ]; then
  echo "==> Cloning $CLONE_URL into workspace..."
  ssh "root@${SERVER_IP}" "cd /var/lib/microvms/${VM_NAME}/workspace && git clone ${CLONE_URL}"
fi

# 6. Extract the VM's IP from the config file
VM_IP=$(grep -oP 'vmIP\s*=\s*"\K[^"]+' "$VM_CONFIG" || echo "<unknown>")

echo ""
echo "==> Deployment complete!"
echo ""
echo "SSH into the VM:"
echo "  ssh -J root@${SERVER_IP} agent@${VM_IP}"
echo ""
echo "Run Claude inside the VM:"
echo "  claude --dangerously-skip-permissions"
