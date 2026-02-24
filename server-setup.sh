#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Server-Side Setup Script
# =============================================================================
# Run this ON the server after NixOS is installed and the repo is cloned.
#
# Prerequisites:
#   - NixOS installed (via setup.sh)
#   - Repo cloned at /opt/src/ (done by setup.sh)
#   - Secret files uploaded to /var/secrets/ (done by setup.sh):
#     - cloudflare-origin.pem, cloudflare-origin-key.pem
#     - github-app.pem (if using GitHub App auth)
#
# Usage:
#   cd /opt/src && ./server-setup.sh
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER_CONFIG="$SCRIPT_DIR/server-config"

# -- Colors -------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# -- Helpers ------------------------------------------------------------------
info()    { echo -e "${BLUE}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }
fatal()   { error "$@"; exit 1; }
header()  { echo -e "\n${BOLD}${CYAN}=== $* ===${NC}\n"; }

confirm() {
    local prompt="${1:-Continue?}"
    echo -en "${YELLOW}${prompt} [y/N]${NC} "
    read -r answer
    [[ "$answer" =~ ^[Yy]$ ]]
}

prompt() {
    local var_name="$1"
    local prompt_text="$2"
    local default="${3:-}"
    local value

    if [[ -n "$default" ]]; then
        echo -ne "${BOLD}${prompt_text}${NC} [${default}]: "
        read -r value
        value="${value:-$default}"
    else
        echo -ne "${BOLD}${prompt_text}${NC}: "
        read -r value
        while [[ -z "$value" ]]; do
            echo -ne "${RED}Required.${NC} ${BOLD}${prompt_text}${NC}: "
            read -r value
        done
    fi
    eval "$var_name='$value'"
}

prompt_secret() {
    local var_name="$1"
    local prompt_text="$2"
    local value

    echo -ne "${BOLD}${prompt_text}${NC}: "
    read -rs value
    echo
    while [[ -z "$value" ]]; do
        echo -ne "${RED}Required.${NC} ${BOLD}${prompt_text}${NC}: "
        read -rs value
        echo
    done
    eval "$var_name='$value'"
}

# =============================================================================
echo ""
echo -e "${BOLD}${CYAN}"
echo "  ╔══════════════════════════════════════════════════════════╗"
echo "  ║   Server Setup for Claude Code Agents                   ║"
echo "  ║   Run this on the NixOS server                          ║"
echo "  ╚══════════════════════════════════════════════════════════╝"
echo -e "${NC}"

# Verify we're running on the server
if [[ ! -d /etc/nixos ]]; then
    fatal "This script must be run on the NixOS server (no /etc/nixos found)."
fi

if [[ ! -d "$SERVER_CONFIG" ]]; then
    fatal "Cannot find server-config/ at $SERVER_CONFIG. Are you running from the repo root?"
fi

# =============================================================================
# Step 1: Collect configuration
# =============================================================================
header "Step 1: Configuration"

echo -e "${BOLD}--- Domain ---${NC}"
prompt DOMAIN "Preview domain (e.g., yourdomain.com)"
prompt ALLOWED_DOMAIN "Allowed email domain for login (e.g., yourdomain.com)" "$DOMAIN"
echo ""

echo -e "${BOLD}--- Google OAuth ---${NC}"
echo "Create at: https://console.cloud.google.com/ > APIs & Services > Credentials"
echo "Redirect URI must be: https://dashboard.${DOMAIN}/api/auth/callback"
echo ""
prompt GOOGLE_CLIENT_ID "Google OAuth Client ID"
prompt_secret GOOGLE_CLIENT_SECRET "Google OAuth Client Secret"
echo ""

echo -e "${BOLD}--- GitHub App ---${NC}"
echo "Create at: GitHub > Settings > Developer settings > GitHub Apps"
echo ""
prompt GITHUB_APP_ID "GitHub App ID"
prompt GITHUB_INSTALLATION_ID "GitHub App Installation ID"
echo ""

# Check for GitHub App PEM
if [[ -f /var/secrets/github-app.pem ]]; then
    success "GitHub App private key found at /var/secrets/github-app.pem"
else
    warn "No GitHub App private key found at /var/secrets/github-app.pem"
    warn "If using GitHub App auth, upload it before starting services."
fi
echo ""

echo -e "${BOLD}--- Webhook Secret ---${NC}"
echo -en "${BOLD}GitHub webhook secret (leave blank to auto-generate):${NC} "
read -r GITHUB_WEBHOOK_SECRET
if [[ -z "$GITHUB_WEBHOOK_SECRET" ]]; then
    GITHUB_WEBHOOK_SECRET=$(openssl rand -hex 16)
    success "Generated webhook secret: $GITHUB_WEBHOOK_SECRET"
    echo -e "  ${YELLOW}Save this secret — you'll need it when configuring the GitHub webhook.${NC}"
else
    success "Webhook secret set."
fi
echo ""

echo -e "${BOLD}--- SSH Key ---${NC}"
echo "Your SSH public key for accessing agent/preview containers."
echo ""
prompt SSH_PUBKEY "Your SSH public key (ssh-ed25519 ...)"
echo ""

echo -e "${BOLD}--- Repository Configuration ---${NC}"
echo "Restrict which repos can be deployed as previews."
echo ""
echo -en "${BOLD}Allowed repos (comma-separated owner/repo, leave blank for all):${NC} "
read -r ALLOWED_REPOS
ALLOWED_REPOS="${ALLOWED_REPOS:-}"
echo -en "${BOLD}Vertex/Elixir repos (comma-separated owner/repo, leave blank for none):${NC} "
read -r VERTEX_REPOS
VERTEX_REPOS="${VERTEX_REPOS:-}"
if [[ -n "$ALLOWED_REPOS" ]]; then
    success "Allowed repos: $ALLOWED_REPOS"
else
    success "All repos allowed."
fi
if [[ -n "$VERTEX_REPOS" ]]; then
    success "Vertex repos: $VERTEX_REPOS"
fi
echo ""

echo -e "${BOLD}--- Git Commit Signing ---${NC}"
echo "Agent containers sign commits with an SSH key."
echo "The commit email must match the GitHub account where the signing key is added."
echo ""
prompt GIT_EMAIL "Git commit email (must match your GitHub account)"
echo ""

# =============================================================================
# Step 2: Auto-detect hardware from live system
# =============================================================================
header "Step 2: Hardware Detection"

info "Detecting hardware from running system..."

# Server IP
SERVER_IP=$(ip -4 addr show scope global | grep -oP 'inet \K[0-9.]+' | head -1) || true
if [[ -z "$SERVER_IP" ]]; then
    fatal "Could not detect server IP address."
fi
success "Server IP: $SERVER_IP"

# Gateway
GATEWAY=$(ip route | grep default | awk '{print $3}' | head -1) || true
if [[ -z "$GATEWAY" ]]; then
    fatal "Could not detect gateway."
fi
success "Gateway: $GATEWAY"

# Interface
INTERFACE=$(ip route | grep default | awk '{print $5}' | head -1) || true
if [[ -z "$INTERFACE" ]]; then
    fatal "Could not detect network interface."
fi
success "Interface: $INTERFACE"

# Prefix length
PREFIX_LENGTH=$(ip -4 addr show dev "$INTERFACE" | grep 'inet ' | head -1 | awk '{print $2}' | cut -d/ -f2) || true
if [[ -z "$PREFIX_LENGTH" ]]; then
    warn "Could not detect prefix length, defaulting to 26"
    PREFIX_LENGTH="26"
fi
success "Prefix length: /$PREFIX_LENGTH"

# Disk devices — detect from mdstat (RAID members)
info "Detecting disk devices..."
DISK_DEVICE_0=""
DISK_DEVICE_1=""

if [[ -f /proc/mdstat ]]; then
    # Extract disk devices from RAID arrays
    local_disks=$(grep -oP '[a-z]+(?=\[\d+\])' /proc/mdstat | sort -u) || true
    if [[ -n "$local_disks" ]]; then
        disk_array=()
        while IFS= read -r disk; do
            # Strip partition number to get base device
            base=$(echo "$disk" | sed 's/[0-9]*$//')
            disk_array+=("/dev/$base")
        done <<< "$local_disks"
        # Get unique base devices
        readarray -t unique_disks < <(printf '%s\n' "${disk_array[@]}" | sort -u)
        if [[ ${#unique_disks[@]} -ge 2 ]]; then
            DISK_DEVICE_0="${unique_disks[0]}"
            DISK_DEVICE_1="${unique_disks[1]}"
        fi
    fi
fi

if [[ -z "$DISK_DEVICE_0" ]]; then
    # Fallback: detect from lsblk
    readarray -t all_disks < <(lsblk -d -n -o NAME,TYPE | awk '$2=="disk" && $1!~/^loop/ {print "/dev/" $1}')
    if [[ ${#all_disks[@]} -ge 2 ]]; then
        DISK_DEVICE_0="${all_disks[0]}"
        DISK_DEVICE_1="${all_disks[1]}"
    else
        fatal "Could not detect at least 2 disk devices."
    fi
fi
success "Disk 0: $DISK_DEVICE_0"
success "Disk 1: $DISK_DEVICE_1"

# Initrd modules — detect NVMe vs SATA
if [[ "$DISK_DEVICE_0" == /dev/nvme* ]]; then
    INITRD_MODULES='"nvme"'
    success "NVMe disks detected."
else
    INITRD_MODULES='"ahci" "sd_mod"'
    success "SATA disks detected."
fi

# Network driver
net_driver=""
if [[ -L "/sys/class/net/$INTERFACE/device/driver" ]]; then
    net_driver=$(basename "$(readlink -f "/sys/class/net/$INTERFACE/device/driver")") || true
fi
if [[ -n "$net_driver" ]]; then
    INITRD_MODULES="$INITRD_MODULES \"$net_driver\""
    success "Network driver: $net_driver"
else
    INITRD_MODULES="$INITRD_MODULES \"r8169\""
    warn "Could not detect network driver, defaulting to r8169"
fi

# =============================================================================
# Confirm
# =============================================================================
header "Summary"
echo "  Server IP:      $SERVER_IP"
echo "  Gateway:        $GATEWAY"
echo "  Interface:      $INTERFACE"
echo "  Prefix:         /$PREFIX_LENGTH"
echo "  Disk 0:         $DISK_DEVICE_0"
echo "  Disk 1:         $DISK_DEVICE_1"
echo "  Domain:         $DOMAIN"
echo "  Login domain:   $ALLOWED_DOMAIN"
echo "  Google Client:  ${GOOGLE_CLIENT_ID:0:20}..."
echo "  GitHub App ID:  $GITHUB_APP_ID"
echo "  Allowed repos:  ${ALLOWED_REPOS:-<all>}"
echo "  Vertex repos:   ${VERTEX_REPOS:-<none>}"
echo "  Git email:      $GIT_EMAIL"
echo ""

if ! confirm "Proceed with setup?"; then
    echo "Aborted."
    exit 0
fi

# =============================================================================
# Step 3: Generate secrets
# =============================================================================
header "Step 3: Generate Secrets"

info "Generating JWT secret..."
JWT_SECRET=$(openssl rand -hex 32)
success "JWT secret generated."

info "Generating SSH signing key for git commits..."
mkdir -p /var/secrets/claude-signing /var/secrets/claude
if [[ ! -f /var/secrets/claude-signing/signing_key ]]; then
    ssh-keygen -t ed25519 -f /var/secrets/claude-signing/signing_key -N '' -C 'claude-dashboard-signing' 2>/dev/null
    success "SSH signing key generated."
else
    success "SSH signing key already exists."
fi
cp /var/secrets/claude-signing/signing_key /var/secrets/claude/signing_key
cp /var/secrets/claude-signing/signing_key.pub /var/secrets/claude/signing_key.pub

SIGNING_PUBKEY=$(cat /var/secrets/claude-signing/signing_key.pub)
echo ""
echo -e "${YELLOW}=== ACTION REQUIRED ===${NC}"
echo -e "Add this SSH key to GitHub as a ${BOLD}Signing Key${NC}:"
echo "  1. Go to https://github.com/settings/keys"
echo "  2. Click 'New SSH key'"
echo "  3. Key type: Signing Key"
echo "  4. Paste:"
echo ""
echo "  $SIGNING_PUBKEY"
echo ""
echo -ne "Press Enter when done..."
read -r

info "Ensuring root has an SSH key..."
if [[ ! -f /root/.ssh/id_ed25519 ]]; then
    ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519 -N '' -C 'root@nixos-server'
    success "Root SSH key generated."
else
    success "Root SSH key already exists."
fi
ROOT_PUBKEY=$(cat /root/.ssh/id_ed25519.pub)

# =============================================================================
# Step 4: Substitute placeholders and install configs
# =============================================================================
header "Step 4: Configure NixOS"

info "Preparing configuration files..."

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# --- configuration.nix ---
cp "$SERVER_CONFIG/configuration.nix" "$TMPDIR/configuration.nix"
cfg="$TMPDIR/configuration.nix"

sed -i "s|YOUR.SERVER.IP.HERE|$SERVER_IP|g" "$cfg"
sed -i "s|YOUR.GATEWAY.IP.HERE|$GATEWAY|g" "$cfg"
sed -i "s|ssh-ed25519 AAAA\.\.\. your-key-here|$SSH_PUBKEY|g" "$cfg"
sed -i "s|prefixLength = [0-9]*;|prefixLength = $PREFIX_LENGTH;|g" "$cfg"
sed -i "s|interfaces\.enp3s0|interfaces.$INTERFACE|g" "$cfg"
sed -i "s|interface = \"enp3s0\"|interface = \"$INTERFACE\"|g" "$cfg"
sed -i "s|externalInterface = \"enp3s0\"|externalInterface = \"$INTERFACE\"|g" "$cfg"
sed -i "s|DISK_DEVICE_0|$DISK_DEVICE_0|g" "$cfg"
sed -i "s|DISK_DEVICE_1|$DISK_DEVICE_1|g" "$cfg"
sed -i "s|INITRD_KERNEL_MODULES|$INITRD_MODULES|g" "$cfg"
sed -i "s|dashboard\.YOUR_DOMAIN|dashboard.$DOMAIN|g" "$cfg"
success "configuration.nix prepared."

# --- agent-config.nix ---
cp "$SERVER_CONFIG/agent-config.nix" "$TMPDIR/agent-config.nix"
agent_cfg="$TMPDIR/agent-config.nix"

sed -i "s|ssh-ed25519 AAAA\.\.\. your-key-here|$SSH_PUBKEY|g" "$agent_cfg"
sed -i "s|ssh-ed25519 AAAA\.\.\. root-key-here|$ROOT_PUBKEY|g" "$agent_cfg"
sed -i "s|email = YOUR_GIT_EMAIL|email = $GIT_EMAIL|g" "$agent_cfg"
success "agent-config.nix prepared."

# --- preview-config.nix ---
cp "$SERVER_CONFIG/preview-config.nix" "$TMPDIR/preview-config.nix"
preview_cfg="$TMPDIR/preview-config.nix"
sed -i "s|ssh-ed25519 AAAA\.\.\. your-key-here|$SSH_PUBKEY|g" "$preview_cfg"
success "preview-config.nix prepared."

# --- vertex-preview-config.nix ---
cp "$SERVER_CONFIG/vertex-preview-config.nix" "$TMPDIR/vertex-preview-config.nix"
vertex_cfg="$TMPDIR/vertex-preview-config.nix"
sed -i "s|ssh-ed25519 AAAA\.\.\. your-key-here|$SSH_PUBKEY|g" "$vertex_cfg"
success "vertex-preview-config.nix prepared."

# Install all configs to /etc/nixos/
info "Installing configuration to /etc/nixos/..."
cp "$TMPDIR/configuration.nix" /etc/nixos/configuration.nix
cp "$TMPDIR/agent-config.nix" /etc/nixos/agent-config.nix
cp "$TMPDIR/preview-config.nix" /etc/nixos/preview-config.nix
cp "$TMPDIR/vertex-preview-config.nix" /etc/nixos/vertex-preview-config.nix

# Copy safe files directly (no placeholders)
cp "$SERVER_CONFIG/preview.sh" /etc/nixos/preview.sh
cp "$SERVER_CONFIG/agent.sh" /etc/nixos/agent.sh
cp "$SERVER_CONFIG/flake.nix" /etc/nixos/flake.nix
success "All NixOS configs installed."

# =============================================================================
# Step 5: Write environment files
# =============================================================================
header "Step 5: Write Environment Files"

info "Creating directories..."
mkdir -p /var/secrets/claude /var/secrets/claude-signing \
    /opt/dashboard/static /opt/preview-webhook \
    /etc/caddy/previews /var/lib/dashboard /var/lib/preview-deploys \
    /var/lib/claude-agents
chmod 755 /var/secrets /var/secrets/claude

info "Writing dashboard.env..."
cat > /var/secrets/dashboard.env << ENVEOF
LISTEN_ADDR=0.0.0.0:3200
DATABASE_URL=sqlite:///var/lib/dashboard/dashboard.db
JWT_SECRET=${JWT_SECRET}
GOOGLE_CLIENT_ID=${GOOGLE_CLIENT_ID}
GOOGLE_CLIENT_SECRET=${GOOGLE_CLIENT_SECRET}
GOOGLE_REDIRECT_URI=https://dashboard.${DOMAIN}/api/auth/callback
ALLOWED_DOMAIN=${ALLOWED_DOMAIN}
PREVIEW_DOMAIN=${DOMAIN}
ALLOWED_REPOS=${ALLOWED_REPOS}
VERTEX_REPOS=${VERTEX_REPOS}
PREVIEW_BIN=/run/current-system/sw/bin/preview
AGENT_BIN=/run/current-system/sw/bin/agent
STATIC_DIR=/opt/dashboard/static
ENVEOF
chmod 600 /var/secrets/dashboard.env
success "dashboard.env written."

info "Writing preview.env..."
cat > /var/secrets/preview.env << ENVEOF
GITHUB_APP_ID=${GITHUB_APP_ID}
GITHUB_APP_INSTALLATION_ID=${GITHUB_INSTALLATION_ID}
GITHUB_APP_PRIVATE_KEY_PATH=/var/secrets/github-app.pem
GITHUB_WEBHOOK_SECRET=${GITHUB_WEBHOOK_SECRET}
PREVIEW_DOMAIN=${DOMAIN}
WEBHOOK_PORT=3100
ALLOWED_REPOS=${ALLOWED_REPOS}
VERTEX_REPOS=${VERTEX_REPOS}
ENVEOF
chmod 600 /var/secrets/preview.env
success "preview.env written."

info "Writing webhook Caddy route..."
cat > /etc/caddy/previews/webhook.caddy << CADDYEOF
webhook.${DOMAIN} {
    import cloudflare_tls
    reverse_proxy localhost:3100
}
CADDYEOF
success "Caddy webhook route written."

# =============================================================================
# Step 6: nixos-rebuild switch
# =============================================================================
header "Step 6: NixOS Rebuild"

info "Running nixos-rebuild switch (this may take a while on first run)..."

gen_before=$(readlink /nix/var/nix/profiles/system)

if ! rebuild_output=$(cd /etc/nixos && nixos-rebuild switch 2>&1); then
    # Check if it failed due to the empty Caddy plugin hash — extract the real hash and retry
    caddy_hash=$(echo "$rebuild_output" | sed -n 's/.*got: *\(sha256-[A-Za-z0-9+/=]*\).*/\1/p' | head -1)
    if [[ -n "$caddy_hash" ]]; then
        warn "Caddy plugin hash was empty. Got: $caddy_hash — patching and retrying..."
        sed -i "s|hash = \".*\";.*# Leave empty on first build.*|hash = \"$caddy_hash\";|" /etc/nixos/configuration.nix
        if ! (cd /etc/nixos && nixos-rebuild switch); then
            warn "nixos-rebuild switch exited with errors on retry, verifying..."
            gen_after=$(readlink /nix/var/nix/profiles/system)
            if [[ "$gen_before" == "$gen_after" ]]; then
                fatal "nixos-rebuild switch failed and no new generation was created."
            fi
            warn "New generation active ($gen_after). Non-critical service errors — safe to continue."
        fi
    else
        warn "nixos-rebuild switch exited with errors, verifying..."
        gen_after=$(readlink /nix/var/nix/profiles/system)
        if [[ "$gen_before" == "$gen_after" ]]; then
            echo "$rebuild_output"
            fatal "nixos-rebuild switch failed and no new generation was created."
        fi
        warn "New generation active ($gen_after). Non-critical service errors — safe to continue."
    fi
fi

success "NixOS rebuild complete."

# Fix Origin CA cert permissions (caddy group only exists after nixos-rebuild)
if [[ -f /var/secrets/cloudflare-origin.pem ]]; then
    info "Setting Origin CA certificate permissions..."
    chown root:caddy /var/secrets/cloudflare-origin.pem /var/secrets/cloudflare-origin-key.pem 2>/dev/null || true
    chmod 640 /var/secrets/cloudflare-origin.pem /var/secrets/cloudflare-origin-key.pem
    success "Certificate permissions set."
fi

# =============================================================================
# Step 7: Build dashboard
# =============================================================================
header "Step 7: Build Dashboard"

info "Building dashboard frontend..."
cd "$SCRIPT_DIR/dashboard/frontend"
nix-shell -p nodejs_22 --run 'npm ci && npm run build'
rm -rf /opt/dashboard/static
cp -r "$SCRIPT_DIR/dashboard/frontend/dist" /opt/dashboard/static
success "Frontend built and deployed."

info "Building dashboard backend (this takes ~2 minutes on first build)..."
cd "$SCRIPT_DIR/dashboard/backend"
nix-shell -p rustc cargo gcc pkg-config --run 'cargo build --release'
cp "$SCRIPT_DIR/dashboard/backend/target/release/dashboard" /opt/dashboard/dashboard
success "Backend built and deployed."

# =============================================================================
# Step 8: Build webhook
# =============================================================================
header "Step 8: Build Webhook"

info "Building preview webhook..."
cp -r "$SERVER_CONFIG/preview-webhook/src" /opt/preview-webhook/src
cp "$SERVER_CONFIG/preview-webhook/package.json" /opt/preview-webhook/
cp "$SERVER_CONFIG/preview-webhook/package-lock.json" /opt/preview-webhook/
cp "$SERVER_CONFIG/preview-webhook/tsconfig.json" /opt/preview-webhook/
cd /opt/preview-webhook
nix-shell -p nodejs_22 --run 'npm ci && npm run build'
success "Preview webhook built."

# =============================================================================
# Step 9: Build container closures
# =============================================================================
header "Step 9: Build Container Closures"

info "Building agent container closure..."
agent build || {
    warn "Agent pre-build failed. The first 'agent create' will build it automatically."
}

info "Building preview container closure..."
preview build || {
    warn "Preview pre-build failed. The first 'preview create' will build it automatically."
}

info "Building vertex preview container closure..."
preview build --type vertex || {
    warn "Vertex pre-build failed. The first 'preview create --type vertex' will build it automatically."
}

# =============================================================================
# Step 10: Start and verify services
# =============================================================================
header "Step 10: Start Services"

info "Restarting services..."
systemctl restart dashboard preview-webhook
sleep 2

WEBHOOK_STATUS=$(systemctl is-active preview-webhook || true)
DASHBOARD_STATUS=$(systemctl is-active dashboard || true)

if [[ "$WEBHOOK_STATUS" == "active" ]]; then
    success "Preview webhook: running"
else
    error "Preview webhook: $WEBHOOK_STATUS"
    error "Check: journalctl -u preview-webhook -n 20"
fi

if [[ "$DASHBOARD_STATUS" == "active" ]]; then
    success "Dashboard: running"
else
    error "Dashboard: $DASHBOARD_STATUS"
    error "Check: journalctl -u dashboard -n 20"
fi

# =============================================================================
# Step 11: Claude login
# =============================================================================
header "Step 11: Claude Authentication"

echo -e "${BOLD}Set up Claude Code credentials.${NC}"
echo -e "You can either:"
echo -e "  1. Run 'claude login' interactively (OAuth flow)"
echo -e "  2. Use a setup token: run 'claude setup-token' on your local machine"
echo -e "     and paste the token into /var/secrets/claude/oauth_token"
echo ""

if confirm "Run 'claude login' now?"; then
    CLAUDE_CONFIG_DIR=/var/secrets/claude claude login || {
        warn "Claude login may have exited with an error (this is OK if you dismissed the trust prompt)."
    }

    if [[ -f /var/secrets/claude/.credentials.json ]]; then
        info "Fixing credential permissions..."
        chmod -R a+rX /var/secrets/claude
        success "Credentials found and permissions set."
    else
        warn "No credentials found. You can log in later with:"
        echo "  CLAUDE_CONFIG_DIR=/var/secrets/claude claude login"
        echo "  chmod -R a+rX /var/secrets/claude"
    fi
else
    warn "Skipping Claude login. Set up credentials later:"
    echo "  Option 1: CLAUDE_CONFIG_DIR=/var/secrets/claude claude login"
    echo "  Option 2: claude setup-token (on local machine), then:"
    echo "    echo 'sk-ant-oat01-...' > /var/secrets/claude/oauth_token"
    echo "    chmod 600 /var/secrets/claude/oauth_token"
fi

# =============================================================================
# Step 12: Summary
# =============================================================================
header "Setup Complete!"

echo -e "${GREEN}Your NixOS server is fully configured.${NC}"
echo ""
echo -e "  ${BOLD}Dashboard:${NC}  https://dashboard.${DOMAIN}"
echo -e "  ${BOLD}Webhook:${NC}    https://webhook.${DOMAIN}/webhook/github"
echo ""
echo -e "${BOLD}DNS Records (Cloudflare, all proxied):${NC}"
echo "  A   dashboard   →  $SERVER_IP"
echo "  A   *           →  $SERVER_IP"
echo "  A   @           →  $SERVER_IP"
echo ""
echo -e "${BOLD}GitHub Webhook Setup:${NC}"
echo "  URL:     https://webhook.${DOMAIN}/webhook/github"
echo "  Secret:  $GITHUB_WEBHOOK_SECRET"
echo "  Events:  Pull requests"
echo ""
echo -e "${BOLD}Quick Start:${NC}"
echo ""
echo -e "  ${CYAN}# Create an agent container${NC}"
echo "  agent create myagent"
echo ""
echo -e "  ${CYAN}# SSH into the agent${NC}"
echo "  ssh agent@<container-ip>"
echo ""
echo -e "  ${CYAN}# Run Claude${NC}"
echo "  claude"
echo ""
echo -e "  ${CYAN}# List and destroy agents${NC}"
echo "  agent list"
echo "  agent destroy myagent"
echo ""
echo -e "${BOLD}Ongoing deploys:${NC}"
echo "  From your local machine: ./deploy.sh $SERVER_IP"
echo ""
echo -e "See ${BOLD}README.md${NC} for more details."
