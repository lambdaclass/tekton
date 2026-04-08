#!/usr/bin/env bash
# =============================================================================
# Update server NixOS configuration
#
# Grabs host-specific values from the server's current configuration.nix,
# substitutes them into the repo template, uploads the result, and runs
# nixos-rebuild switch.
#
# Usage:
#   ./update-server-config.sh <server-ip>
# =============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${CYAN}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

SERVER="${1:-}"
if [[ -z "$SERVER" ]]; then
    echo "Usage: ./update-server-config.sh <server-ip>"
    exit 1
fi

SSH="ssh root@${SERVER}"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
TEMPLATE_DIR="$SCRIPT_DIR/server-config"
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# =============================================================================
# Step 1: Download current configs from the server
# =============================================================================
info "Downloading current configuration from server..."
$SSH "cat /etc/nixos/configuration.nix" > "$TMPDIR/old-configuration.nix"
success "Downloaded current config."

# =============================================================================
# Step 2: Extract host-specific values from the old configuration.nix
# =============================================================================
info "Extracting host-specific values..."

old_cfg="$TMPDIR/old-configuration.nix"

# Disk devices — e.g. devices = [ "/dev/sda" "/dev/sdb" ];
DISK_DEVICES=$(sed -n 's/.*devices = \[ *\(.*[^ ]\) *\];/\1/p' "$old_cfg")

# Initrd kernel modules — e.g. availableKernelModules = [ "ahci" "sd_mod" "r8169" ];
INITRD_MODULES=$(sed -n 's/.*availableKernelModules = \[ *\(.*[^ ]\) *\];/\1/p' "$old_cfg")

# Server IP — e.g. address = "1.2.3.4";
SERVER_IP=$(grep -A1 'ipv4.addresses' "$old_cfg" | sed -n 's/.*address = "\([^"]*\)".*/\1/p')

# Prefix length — e.g. prefixLength = 26;
PREFIX_LENGTH=$(sed -n 's/.*prefixLength = \([0-9]*\);/\1/p' "$old_cfg")

# Gateway — e.g. address = "1.2.3.1";
GATEWAY=$(grep -A2 'defaultGateway' "$old_cfg" | sed -n 's/.*address = "\([^"]*\)".*/\1/p')

# Network interface — e.g. interfaces.enp3s0
INTERFACE=$(sed -n 's/.*interfaces\.\([a-zA-Z0-9]*\) .*/\1/p' "$old_cfg" | head -1)

# SSH keys from configuration.nix (root authorized keys)
SSH_KEY=$(grep 'ssh-' "$old_cfg" | head -1 | sed 's/.*"\(ssh-[^"]*\)".*/\1/')

# Domain — e.g. dashboard.bono.dev { -> bono.dev
DOMAIN=$(sed -n 's/.*dashboard\.\([a-zA-Z0-9._-]*\) {/\1/p' "$old_cfg" | head -1)

# Nameservers — e.g. nameservers = [ "185.12.64.1" "185.12.64.2" ];
NAMESERVERS=$(sed -n 's/.*nameservers = \[ *\(.*[^ ]\) *\];/\1/p' "$old_cfg")

# Print summary
echo ""
echo -e "  ${BOLD}Disk devices:${NC}     $DISK_DEVICES"
echo -e "  ${BOLD}Initrd modules:${NC}   $INITRD_MODULES"
echo -e "  ${BOLD}Server IP:${NC}        $SERVER_IP"
echo -e "  ${BOLD}Prefix length:${NC}    /$PREFIX_LENGTH"
echo -e "  ${BOLD}Gateway:${NC}          $GATEWAY"
echo -e "  ${BOLD}Nameservers:${NC}      $NAMESERVERS"
echo -e "  ${BOLD}Interface:${NC}        $INTERFACE"
echo -e "  ${BOLD}SSH key:${NC}          ${SSH_KEY:0:40}..."
echo -e "  ${BOLD}Domain:${NC}           $DOMAIN"
echo ""

# Verify all values were extracted
for var in DISK_DEVICES INITRD_MODULES SERVER_IP PREFIX_LENGTH GATEWAY NAMESERVERS INTERFACE SSH_KEY DOMAIN; do
    if [[ -z "${!var}" ]]; then
        error "Failed to extract $var from old config. Check the server's configuration.nix."
    fi
done
success "All values extracted."

# =============================================================================
# Step 3: Substitute into repo templates
# =============================================================================
info "Preparing new configuration files..."

# --- configuration.nix ---
cp "$TEMPLATE_DIR/configuration.nix" "$TMPDIR/new-configuration.nix"
cfg="$TMPDIR/new-configuration.nix"

V="$DISK_DEVICES"        perl -pi -e 's|"DISK_DEVICE_0" "DISK_DEVICE_1"|$ENV{V}|g' "$cfg"
V="$INITRD_MODULES"      perl -pi -e 's|INITRD_KERNEL_MODULES|$ENV{V}|g' "$cfg"
V="$SERVER_IP"           perl -pi -e 's|YOUR\.SERVER\.IP\.HERE|$ENV{V}|g' "$cfg"
V="$PREFIX_LENGTH"       perl -pi -e 's|prefixLength = [0-9]*;|prefixLength = $ENV{V};|g' "$cfg"
V="$GATEWAY"             perl -pi -e 's|YOUR\.GATEWAY\.IP\.HERE|$ENV{V}|g' "$cfg"
V="$NAMESERVERS"         perl -pi -e 's|"1\.1\.1\.1" "1\.0\.0\.1"|$ENV{V}|g' "$cfg"
V="$INTERFACE"           perl -pi -e 's|interfaces\.enp3s0|interfaces.$ENV{V}|g' "$cfg"
V="$INTERFACE"           perl -pi -e 's|interface = "enp3s0"|interface = "$ENV{V}"|g' "$cfg"
V="$INTERFACE"           perl -pi -e 's|externalInterface = "enp3s0"|externalInterface = "$ENV{V}"|g' "$cfg"
V="$SSH_KEY"             perl -pi -e 's|ssh-ed25519 AAAA\.\.\. your-key-here|$ENV{V}|g' "$cfg"
V="$DOMAIN"              perl -pi -e 's|dashboard\.YOUR_DOMAIN|dashboard.$ENV{V}|g' "$cfg"
success "configuration.nix prepared."

# =============================================================================
# Step 4: Show diff and confirm
# =============================================================================
echo ""
info "Diff for configuration.nix:"
diff --color=always "$TMPDIR/old-configuration.nix" "$TMPDIR/new-configuration.nix" || true
echo ""

echo -en "${YELLOW}Upload and run nixos-rebuild switch? [y/N]${NC} "
read -r answer
if [[ ! "$answer" =~ ^[Yy]$ ]]; then
    echo "Aborted."
    exit 0
fi

# =============================================================================
# Step 5: Upload and rebuild
# =============================================================================
info "Uploading new configs to server..."
scp "$TMPDIR/new-configuration.nix" "root@${SERVER}:/etc/nixos/configuration.nix"
success "Config uploaded."

info "Running nixos-rebuild switch..."
$SSH "cd /etc/nixos && nixos-rebuild switch"
success "nixos-rebuild switch complete."

echo ""
success "Server configuration updated. Services should restart automatically."
echo -e "  Verify with: ssh root@${SERVER} systemctl status dashboard"
