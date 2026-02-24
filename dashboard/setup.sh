#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# Dashboard Setup Script
# Deploys the full dashboard stack to a NixOS server from scratch.
# Run this from the repo root: ./dashboard/setup.sh
# =============================================================================

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
SERVER_CONFIG="$REPO_ROOT/server-config"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${CYAN}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
warn()    { echo -e "${YELLOW}[WARN]${NC} $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }
fatal()   { error "$@"; exit 1; }

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

prompt_file() {
    local var_name="$1"
    local prompt_text="$2"
    local value

    echo -ne "${BOLD}${prompt_text}${NC}: "
    read -r value
    while [[ ! -f "$value" ]]; do
        echo -ne "${RED}File not found.${NC} ${BOLD}${prompt_text}${NC}: "
        read -r value
    done
    eval "$var_name='$value'"
}

# =============================================================================
echo ""
echo -e "${BOLD}========================================${NC}"
echo -e "${BOLD}   Preview Dashboard Setup${NC}"
echo -e "${BOLD}========================================${NC}"
echo ""
echo "This script will set up the dashboard on a NixOS server."
echo "You'll need the following ready:"
echo "  - SSH root access to the server"
echo "  - Cloudflare Origin CA certificate + key"
echo "  - Google OAuth credentials"
echo "  - GitHub App credentials (App ID, Installation ID, Private Key)"
echo "  - Claude Code credentials (run 'claude' on server first)"
echo ""
echo "See DEPLOY.md for detailed setup instructions."
echo ""

# =============================================================================
# Step 1: Server details
# =============================================================================
echo -e "${BOLD}--- Server Configuration ---${NC}"
prompt SERVER_IP "Server IP address"
prompt SERVER_USER "SSH user" "root"
SERVER="${SERVER_USER}@${SERVER_IP}"

info "Testing SSH connection..."
if ! ssh -o ConnectTimeout=5 "$SERVER" "echo ok" &>/dev/null; then
    fatal "Cannot connect to $SERVER. Check your SSH config."
fi
success "SSH connection OK"
echo ""

# =============================================================================
# Step 2: Domain
# =============================================================================
echo -e "${BOLD}--- Domain Configuration ---${NC}"
prompt DOMAIN "Preview domain (e.g., hipermegared.link)"
prompt ALLOWED_DOMAIN "Allowed email domain for login (e.g., lambdaclass.com)"
echo ""

# =============================================================================
# Step 3: Cloudflare Origin CA
# =============================================================================
echo -e "${BOLD}--- Cloudflare Origin CA Certificate ---${NC}"
echo "Generate at: Cloudflare > SSL/TLS > Origin Server > Create Certificate"
echo "Must cover: *.${DOMAIN} and ${DOMAIN}"
echo ""
prompt_file CF_CERT_PATH "Path to origin certificate (.pem)"
prompt_file CF_KEY_PATH "Path to origin private key (.pem)"
echo ""

# =============================================================================
# Step 4: Google OAuth
# =============================================================================
echo -e "${BOLD}--- Google OAuth Credentials ---${NC}"
echo "Create at: https://console.cloud.google.com/ > APIs & Services > Credentials"
echo "Redirect URI must be: https://dashboard.${DOMAIN}/api/auth/callback"
echo ""
prompt GOOGLE_CLIENT_ID "Google OAuth Client ID"
prompt_secret GOOGLE_CLIENT_SECRET "Google OAuth Client Secret"
echo ""

# =============================================================================
# Step 5: GitHub App
# =============================================================================
echo -e "${BOLD}--- GitHub App Credentials ---${NC}"
echo "Create at: GitHub > Settings > Developer settings > GitHub Apps"
echo ""
prompt GITHUB_APP_ID "GitHub App ID"
prompt GITHUB_INSTALLATION_ID "GitHub App Installation ID"
prompt_file GITHUB_APP_KEY_PATH "Path to GitHub App private key (.pem)"
prompt_secret GITHUB_WEBHOOK_SECRET "GitHub Webhook Secret"
echo ""

# =============================================================================
# Step 6: Repos
# =============================================================================
echo -e "${BOLD}--- Repository Configuration ---${NC}"
prompt ALLOWED_REPOS "Allowed repos (comma-separated, e.g., org/repo1,org/repo2)"
prompt VERTEX_REPOS "Vertex (Elixir) repos (comma-separated, leave empty if none)" ""
echo ""

# =============================================================================
# Step 7: SSH keys
# =============================================================================
echo -e "${BOLD}--- SSH Keys ---${NC}"
echo "These SSH public keys will be authorized in agent/preview containers."
echo "Enter your personal SSH public key (for direct SSH access to containers)."
echo ""
prompt SSH_PUBKEY "Your SSH public key (ssh-ed25519 ...)"
echo ""

# =============================================================================
# Step 8: Git signing email
# =============================================================================
echo -e "${BOLD}--- Git Commit Signing ---${NC}"
echo "Agent containers sign commits with an SSH key."
echo "The commit email must match the GitHub account where the signing key is added."
echo ""
prompt GIT_EMAIL "Git commit email (must match your GitHub account)"
echo ""

# =============================================================================
# Confirm
# =============================================================================
echo -e "${BOLD}--- Summary ---${NC}"
echo "  Server:         $SERVER"
echo "  Domain:         $DOMAIN"
echo "  Login domain:   $ALLOWED_DOMAIN"
echo "  Google Client:  ${GOOGLE_CLIENT_ID:0:20}..."
echo "  GitHub App ID:  $GITHUB_APP_ID"
echo "  Allowed repos:  $ALLOWED_REPOS"
echo "  Vertex repos:   ${VERTEX_REPOS:-<none>}"
echo "  Git email:      $GIT_EMAIL"
echo ""
echo -ne "${BOLD}Proceed with deployment? [y/N]:${NC} "
read -r confirm
if [[ "$confirm" != "y" && "$confirm" != "Y" ]]; then
    echo "Aborted."
    exit 0
fi

# =============================================================================
# Generate secrets
# =============================================================================
info "Generating secrets..."
JWT_SECRET=$(openssl rand -hex 32)

# =============================================================================
# Create server directories
# =============================================================================
info "Creating server directories..."
ssh "$SERVER" "mkdir -p /var/secrets/claude /var/secrets/claude-signing /opt/dashboard/static /opt/dashboard-build /opt/preview-webhook /etc/caddy/previews /var/lib/dashboard /var/lib/preview-deploys /var/lib/claude-agents"

# =============================================================================
# Upload Cloudflare certs
# =============================================================================
info "Uploading Cloudflare Origin CA certificates..."
scp "$CF_CERT_PATH" "$SERVER:/var/secrets/cloudflare-origin.pem"
scp "$CF_KEY_PATH" "$SERVER:/var/secrets/cloudflare-origin-key.pem"
ssh "$SERVER" "chmod 600 /var/secrets/cloudflare-origin-key.pem"

# =============================================================================
# Upload GitHub App key
# =============================================================================
info "Uploading GitHub App private key..."
scp "$GITHUB_APP_KEY_PATH" "$SERVER:/var/secrets/github-app.pem"
ssh "$SERVER" "chmod 600 /var/secrets/github-app.pem"

# =============================================================================
# Generate SSH signing key
# =============================================================================
info "Generating SSH signing key for git commits..."
ssh "$SERVER" "ssh-keygen -t ed25519 -f /var/secrets/claude-signing/signing_key -N '' -C 'claude-dashboard-signing' 2>/dev/null || true"
ssh "$SERVER" "cp /var/secrets/claude-signing/signing_key /var/secrets/claude/signing_key"

SIGNING_PUBKEY=$(ssh "$SERVER" "cat /var/secrets/claude-signing/signing_key.pub")
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

# =============================================================================
# Generate root SSH key (for accessing agent containers)
# =============================================================================
info "Ensuring root has an SSH key on the server..."
ssh "$SERVER" "test -f /root/.ssh/id_ed25519 || ssh-keygen -t ed25519 -f /root/.ssh/id_ed25519 -N '' -C 'root@nixos-server'"
ROOT_PUBKEY=$(ssh "$SERVER" "cat /root/.ssh/id_ed25519.pub")

# =============================================================================
# Write environment files
# =============================================================================
info "Writing dashboard.env..."
ssh "$SERVER" "cat > /var/secrets/dashboard.env << 'ENVEOF'
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
ENVEOF"

info "Writing preview.env..."
ssh "$SERVER" "cat > /var/secrets/preview.env << 'ENVEOF'
GITHUB_APP_ID=${GITHUB_APP_ID}
GITHUB_APP_INSTALLATION_ID=${GITHUB_INSTALLATION_ID}
GITHUB_APP_PRIVATE_KEY_PATH=/var/secrets/github-app.pem
GITHUB_WEBHOOK_SECRET=${GITHUB_WEBHOOK_SECRET}
PREVIEW_DOMAIN=${DOMAIN}
WEBHOOK_PORT=3100
ALLOWED_REPOS=${ALLOWED_REPOS}
VERTEX_REPOS=${VERTEX_REPOS}
ENVEOF"

# =============================================================================
# Prepare NixOS configs with real values
# =============================================================================
info "Preparing NixOS configuration files..."

TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# agent-config.nix — substitute SSH keys and email
cp "$SERVER_CONFIG/agent-config.nix" "$TMPDIR/agent-config.nix"
# The local file already has the right structure, just needs key substitution
# We handle this by using sed to replace placeholder keys
sed -i.bak "s|ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIEWMu5wyCIJclVNVk3Judmu5zkWxkbtTJrcC0BpEcVfy jrchatruc@gmail.com|${SSH_PUBKEY}|g" "$TMPDIR/agent-config.nix"
sed -i.bak "s|ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBIrWQgyW2acM35arp+DVr8Jo5S7A4vqbP9gLk3pMRhw root@nixos-server|${ROOT_PUBKEY}|g" "$TMPDIR/agent-config.nix"
sed -i.bak "s|email = jrchatruc@gmail.com|email = ${GIT_EMAIL}|g" "$TMPDIR/agent-config.nix"

# Upload NixOS configs
scp "$TMPDIR/agent-config.nix" "$SERVER:/etc/nixos/agent-config.nix"
scp "$SERVER_CONFIG/preview-config.nix" "$SERVER:/etc/nixos/preview-config.nix"
scp "$SERVER_CONFIG/vertex-preview-config.nix" "$SERVER:/etc/nixos/vertex-preview-config.nix"
scp "$SERVER_CONFIG/preview.sh" "$SERVER:/etc/nixos/preview.sh"
scp "$SERVER_CONFIG/agent.sh" "$SERVER:/etc/nixos/agent.sh"
scp "$SERVER_CONFIG/flake.nix" "$SERVER:/etc/nixos/flake.nix"

# Note: configuration.nix is NOT uploaded — it has hardware-specific values
# that must be configured manually on first install.
warn "configuration.nix was NOT uploaded. If this is a fresh server, you must"
warn "manually configure it with your server's IP, gateway, disk devices, etc."
warn "See server-config/configuration.nix for the template."

# =============================================================================
# Deploy preview webhook (before nixos-rebuild so the service can start)
# =============================================================================
info "Deploying preview webhook..."
scp -r "$SERVER_CONFIG/preview-webhook/src" "$SERVER_CONFIG/preview-webhook/package.json" "$SERVER_CONFIG/preview-webhook/package-lock.json" "$SERVER_CONFIG/preview-webhook/tsconfig.json" "$SERVER:/opt/preview-webhook/"
ssh "$SERVER" "cd /opt/preview-webhook && npm ci && npm run build"
success "Preview webhook deployed"

# =============================================================================
# Build and deploy dashboard (before nixos-rebuild so the service can start)
# =============================================================================
info "Building dashboard frontend..."
cd "$SCRIPT_DIR/frontend"
npm ci
npm run build

info "Uploading frontend..."
scp "$SCRIPT_DIR/frontend/dist/index.html" "$SERVER:/opt/dashboard/static/"
scp "$SCRIPT_DIR/frontend/dist/vite.svg" "$SERVER:/opt/dashboard/static/" 2>/dev/null || true
scp -r "$SCRIPT_DIR/frontend/dist/assets" "$SERVER:/opt/dashboard/static/"

info "Uploading backend source for server-side build..."
ssh "$SERVER" "mkdir -p /opt/dashboard-build"
scp -r "$SCRIPT_DIR/backend/src" "$SERVER:/opt/dashboard-build/"
scp "$SCRIPT_DIR/backend/Cargo.toml" "$SCRIPT_DIR/backend/Cargo.lock" "$SERVER:/opt/dashboard-build/"

info "Building backend on server (this takes ~2 minutes on first build)..."
ssh "$SERVER" "cd /opt/dashboard-build && nix-shell -p rustc cargo gcc pkg-config --run 'cargo build --release' 2>&1"
ssh "$SERVER" "cp /opt/dashboard-build/target/release/dashboard /opt/dashboard/dashboard"
success "Dashboard binary built and deployed"

# =============================================================================
# NixOS rebuild
# =============================================================================
info "Running nixos-rebuild switch (this may take a while)..."
GEN_BEFORE=$(ssh "$SERVER" "readlink /nix/var/nix/profiles/system")
if ! ssh "$SERVER" "cd /etc/nixos && nixos-rebuild switch 2>&1"; then
    GEN_AFTER=$(ssh "$SERVER" "readlink /nix/var/nix/profiles/system")
    if [[ "$GEN_BEFORE" == "$GEN_AFTER" ]]; then
        error "nixos-rebuild failed and no new generation was created."
        error "Check configuration.nix is properly set up."
        exit 1
    fi
    warn "nixos-rebuild had non-critical service errors during activation — safe to continue."
fi
success "NixOS rebuild complete"

# =============================================================================
# Build container closures
# =============================================================================
info "Building agent container closure..."
ssh "$SERVER" "agent build"
success "Agent closure built"

info "Building preview container closure..."
ssh "$SERVER" "preview build"
success "Preview closure built"

info "Building vertex preview container closure..."
ssh "$SERVER" "preview build --type vertex"
success "Vertex preview closure built"

# =============================================================================
# Start services
# =============================================================================
info "Starting services..."
ssh "$SERVER" "systemctl restart preview-webhook && systemctl restart dashboard"
sleep 2

# Verify
WEBHOOK_STATUS=$(ssh "$SERVER" "systemctl is-active preview-webhook" || true)
DASHBOARD_STATUS=$(ssh "$SERVER" "systemctl is-active dashboard" || true)

if [[ "$WEBHOOK_STATUS" == "active" ]]; then
    success "Preview webhook: running"
else
    error "Preview webhook: $WEBHOOK_STATUS"
    error "Check: ssh $SERVER 'journalctl -u preview-webhook -n 20'"
fi

if [[ "$DASHBOARD_STATUS" == "active" ]]; then
    success "Dashboard: running"
else
    error "Dashboard: $DASHBOARD_STATUS"
    error "Check: ssh $SERVER 'journalctl -u dashboard -n 20'"
fi

# =============================================================================
# Claude credentials reminder
# =============================================================================
echo ""
echo -e "${YELLOW}=== IMPORTANT ===${NC}"
echo "If you haven't already, set up Claude Code credentials on the server:"
echo "  1. ssh $SERVER"
echo "  2. Run: claude"
echo "  3. Complete the login flow"
echo "  4. Run:"
echo "     mkdir -p /var/secrets/claude"
echo "     cp ~/.claude/.claude.json /var/secrets/claude/"
echo "     cp ~/.claude/.credentials.json /var/secrets/claude/"
echo "     cp -r ~/.claude/cache /var/secrets/claude/ 2>/dev/null || true"
echo ""

# =============================================================================
# Done
# =============================================================================
echo ""
echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}   Setup Complete!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Dashboard: https://dashboard.${DOMAIN}"
echo ""
echo "Next steps:"
echo "  1. Ensure Cloudflare DNS records exist (A records for dashboard, *, @)"
echo "  2. Ensure Claude credentials are set up (see above)"
echo "  3. Visit https://dashboard.${DOMAIN} and log in with Google"
echo ""
