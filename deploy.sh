#!/usr/bin/env bash
# =============================================================================
# Deploy script — builds and deploys everything on the server
#
# Usage:
#   ./deploy.sh <server-ip>                    # Deploy everything
#   ./deploy.sh <server-ip> dashboard          # Dashboard only (backend + frontend)
#   ./deploy.sh <server-ip> webhook            # Preview webhook only
#   ./deploy.sh <server-ip> nix                # NixOS configs only (nixos-rebuild)
#   ./deploy.sh <server-ip> nix-agents         # NixOS configs + rebuild agent closure
#   ./deploy.sh <server-ip> nix-previews       # NixOS configs + rebuild preview closures
#
# The server keeps a git clone at /opt/src/. This script pushes the current
# branch, pulls on the server, builds with nix-shell, and restarts services.
# =============================================================================

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${CYAN}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; exit 1; }

SERVER="${1:-}"
COMPONENT="${2:-all}"

if [[ -z "$SERVER" ]]; then
    echo "Usage: ./deploy.sh <server-ip> [component]"
    echo ""
    echo "Components:"
    echo "  all           Deploy everything (default)"
    echo "  dashboard     Dashboard backend + frontend"
    echo "  webhook       Preview webhook"
    echo "  nix           NixOS configs (nixos-rebuild switch)"
    echo "  nix-agents    NixOS configs + rebuild agent closure"
    echo "  nix-previews  NixOS configs + rebuild all preview closures"
    exit 1
fi

SSH="ssh root@${SERVER}"
REMOTE_SRC="/opt/src"
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)

# ── Ensure the repo clone exists on the server ──────────────────────────────

setup_repo() {
    info "Ensuring repo clone exists at ${REMOTE_SRC}..."
    $SSH "
        if [ ! -d ${REMOTE_SRC}/.git ]; then
            echo 'Cloning repo for the first time...'
            mkdir -p ${REMOTE_SRC}
            git clone $(git remote get-url origin) ${REMOTE_SRC}
        fi
    "
}

# ── Push current branch and pull on server ──────────────────────────────────

sync_code() {
    info "Pushing branch '${CURRENT_BRANCH}' and pulling on server..."
    git push origin "${CURRENT_BRANCH}" 2>/dev/null || true
    $SSH "cd ${REMOTE_SRC} && git fetch origin && git checkout ${CURRENT_BRANCH} && git reset --hard origin/${CURRENT_BRANCH}"
}

# ── Dashboard (Rust backend + React frontend) ──────────────────────────────

deploy_dashboard() {
    info "Ensuring PostgreSQL dashboard database exists..."
    $SSH "
        sudo -u postgres psql -tc \"SELECT 1 FROM pg_roles WHERE rolname='dashboard'\" | grep -q 1 || \
            sudo -u postgres psql -c \"CREATE USER dashboard;\"
        sudo -u postgres psql -tc \"SELECT 1 FROM pg_database WHERE datname='dashboard'\" | grep -q 1 || \
            sudo -u postgres psql -c \"CREATE DATABASE dashboard OWNER dashboard;\"
    "

    info "Updating DATABASE_URL in dashboard.env if needed..."
    $SSH "
        if grep -q 'sqlite:' /var/secrets/dashboard.env 2>/dev/null; then
            sed -i 's|DATABASE_URL=sqlite:.*|DATABASE_URL=postgresql:///dashboard?host=/run/postgresql\&user=dashboard|' /var/secrets/dashboard.env
            echo 'Migrated DATABASE_URL from SQLite to PostgreSQL.'
        fi
    "

    info "Building dashboard frontend..."
    $SSH "
        cd ${REMOTE_SRC}/dashboard/frontend && \
        nix-shell -p nodejs_22 --run 'npm ci && npm run build'
    "

    info "Building dashboard backend..."
    $SSH "
        cd ${REMOTE_SRC}/dashboard/backend && \
        nix-shell -p rustc cargo gcc pkg-config --run 'cargo build --release'
    "

    info "Deploying dashboard..."
    $SSH "
        rm -rf /opt/dashboard/static.new && \
        cp -r ${REMOTE_SRC}/dashboard/frontend/dist /opt/dashboard/static.new && \
        systemctl stop dashboard && \
        rm -rf /opt/dashboard/static.old && \
        mv /opt/dashboard/static /opt/dashboard/static.old 2>/dev/null || true && \
        mv /opt/dashboard/static.new /opt/dashboard/static && \
        cp ${REMOTE_SRC}/dashboard/backend/target/release/dashboard /opt/dashboard/dashboard && \
        rm -rf /opt/dashboard/static.old && \
        systemctl start dashboard
    "
    success "Dashboard deployed."
}

# ── Preview webhook ─────────────────────────────────────────────────────────

deploy_webhook() {
    info "Building and deploying preview webhook..."
    $SSH "
        cp -r ${REMOTE_SRC}/server-config/preview-webhook/src /opt/preview-webhook/src && \
        cp ${REMOTE_SRC}/server-config/preview-webhook/package*.json /opt/preview-webhook/ && \
        cp ${REMOTE_SRC}/server-config/preview-webhook/tsconfig.json /opt/preview-webhook/ && \
        cd /opt/preview-webhook && \
        nix-shell -p nodejs_22 --run 'npm ci && npm run build' && \
        systemctl restart preview-webhook
    "
    success "Preview webhook deployed."
}

# ── NixOS configs ───────────────────────────────────────────────────────────

deploy_nix() {
    info "Deploying NixOS configs..."
    # Copy everything EXCEPT configuration.nix and agent-config.nix (have host-specific placeholders)
    # preview-config.nix and *-preview-config.nix are repo-side templates, not deployed to the server
    $SSH "
        cd ${REMOTE_SRC}/server-config && \
        for f in agent.sh preview.sh flake.nix; do
            if [ -f \"\$f\" ]; then
                cp \"\$f\" /etc/nixos/\"\$f\"
            fi
        done && \
        cd /etc/nixos && nixos-rebuild switch
    "
    success "NixOS configs deployed."
}

deploy_nix_agents() {
    deploy_nix
    info "Rebuilding agent closure..."
    $SSH "agent build"
    success "Agent closure rebuilt."
}

deploy_nix_previews() {
    deploy_nix
    info "Preview closures are built on demand per-repo — nothing to pre-build."
    info "Run 'preview build <owner/repo> <branch>' on the server to pre-warm a specific closure."
}

# ── Main ────────────────────────────────────────────────────────────────────

setup_repo
sync_code

case "$COMPONENT" in
    all)
        deploy_nix
        deploy_dashboard
        deploy_webhook
        ;;
    dashboard)
        deploy_dashboard
        ;;
    webhook)
        deploy_webhook
        ;;
    nix)
        deploy_nix
        ;;
    nix-agents)
        deploy_nix_agents
        ;;
    nix-previews)
        deploy_nix_previews
        ;;
    *)
        error "Unknown component: ${COMPONENT}"
        ;;
esac

echo ""
success "Deploy complete."
