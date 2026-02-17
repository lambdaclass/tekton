#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# NixOS Setup Script for Claude Code Agents on Hetzner
# =============================================================================
# Automates the full setup: network detection, NixOS installation, server
# configuration with nspawn containers, and Claude credential setup.
# =============================================================================

# -- Colors -------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m' # No Color

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

# -- Validation ---------------------------------------------------------------
validate_ip() {
    local ip="$1"
    if [[ "$ip" =~ ^[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}$ ]]; then
        local IFS='.'
        read -ra octets <<< "$ip"
        for octet in "${octets[@]}"; do
            if (( octet > 255 )); then
                return 1
            fi
        done
        return 0
    fi
    return 1
}

# -- Prerequisite checks -----------------------------------------------------
check_prerequisites() {
    header "Checking Prerequisites"

    local missing=()

    if ! command -v nix &>/dev/null; then
        missing+=("nix (with flakes enabled)")
    fi
    if ! command -v ssh &>/dev/null; then
        missing+=("ssh")
    fi
    if ! command -v scp &>/dev/null; then
        missing+=("scp")
    fi
    if ! command -v sed &>/dev/null; then
        missing+=("sed")
    fi

    if [[ ${#missing[@]} -gt 0 ]]; then
        error "Missing required tools:"
        for tool in "${missing[@]}"; do
            echo "  - $tool"
        done
        fatal "Please install the missing tools and try again."
    fi

    success "All prerequisites found."
}

# -- Phase 1: Gather info ----------------------------------------------------
gather_info() {
    header "Phase 1: Gather Information"

    # --- Server IP ---
    echo -en "${BOLD}Enter your Hetzner server IP address:${NC} "
    read -r SERVER_IP
    if ! validate_ip "$SERVER_IP"; then
        fatal "Invalid IP address: $SERVER_IP"
    fi
    success "Server IP: $SERVER_IP"

    # --- SSH Key ---
    info "Looking for SSH public keys..."
    local pub_keys=()
    for f in "$HOME"/.ssh/*.pub; do
        [[ -f "$f" ]] && pub_keys+=("$f")
    done

    SSH_IDENTITY=""  # path to private key (if known)

    if [[ ${#pub_keys[@]} -eq 0 ]]; then
        warn "No SSH public keys found in ~/.ssh/"
        echo -en "${BOLD}Paste your SSH public key:${NC} "
        read -r SSH_KEY
    elif [[ ${#pub_keys[@]} -eq 1 ]]; then
        SSH_KEY=$(cat "${pub_keys[0]}")
        SSH_IDENTITY="${pub_keys[0]%.pub}"
        success "Using SSH key: ${pub_keys[0]}"
        echo -e "  ${CYAN}${SSH_KEY:0:60}...${NC}"
    else
        info "Found multiple SSH public keys:"
        for i in "${!pub_keys[@]}"; do
            local key_content
            key_content=$(cat "${pub_keys[$i]}")
            echo -e "  ${BOLD}$((i+1)))${NC} ${pub_keys[$i]}"
            echo -e "     ${CYAN}${key_content:0:60}...${NC}"
        done
        echo -en "${BOLD}Pick a key (1-${#pub_keys[@]}) or 0 to paste a different one:${NC} "
        read -r key_choice
        if [[ "$key_choice" == "0" ]]; then
            echo -en "${BOLD}Paste your SSH public key:${NC} "
            read -r SSH_KEY
        elif [[ "$key_choice" =~ ^[0-9]+$ ]] && (( key_choice >= 1 && key_choice <= ${#pub_keys[@]} )); then
            SSH_KEY=$(cat "${pub_keys[$((key_choice-1))]}")
            SSH_IDENTITY="${pub_keys[$((key_choice-1))]%.pub}"
        else
            fatal "Invalid selection."
        fi
    fi

    if [[ -z "${SSH_KEY:-}" ]]; then
        fatal "No SSH key provided."
    fi

    # Verify the private key exists when we have a path
    if [[ -n "$SSH_IDENTITY" ]] && [[ ! -f "$SSH_IDENTITY" ]]; then
        warn "Private key not found at $SSH_IDENTITY, SSH will use agent/defaults."
        SSH_IDENTITY=""
    fi

    # Build SSH identity flag (used in ssh_opts throughout the script)
    if [[ -n "$SSH_IDENTITY" ]]; then
        SSH_IDENTITY_OPT="-i $SSH_IDENTITY"
        success "SSH key selected (identity: $SSH_IDENTITY)."
    else
        SSH_IDENTITY_OPT=""
        success "SSH key selected (will use SSH agent/defaults for auth)."
    fi

    # --- Auto-detect network info from rescue mode ---
    info "Connecting to rescue mode to detect network configuration..."
    info "(Make sure the server is in rescue mode and you can SSH in.)"
    echo ""

    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o LogLevel=ERROR"

    if ! ssh $ssh_opts root@"$SERVER_IP" true 2>/dev/null; then
        fatal "Cannot SSH into rescue mode at root@$SERVER_IP. Is rescue mode active?"
    fi
    success "SSH connection to rescue mode successful."

    # Detect gateway
    GATEWAY=$(ssh $ssh_opts root@"$SERVER_IP" "ip route | grep default | awk '{print \$3}'" 2>/dev/null) || true
    if [[ -z "$GATEWAY" ]] || ! validate_ip "$GATEWAY"; then
        fatal "Could not detect gateway. Got: '${GATEWAY:-}'"
    fi
    success "Detected gateway: $GATEWAY"

    # Detect interface — rescue mode uses legacy names (eth0) but NixOS uses
    # predictable names (enp3s0). Derive the predictable name from the PCI slot.
    local rescue_iface
    rescue_iface=$(ssh $ssh_opts root@"$SERVER_IP" "ip route | grep default | awk '{print \$5}'" 2>/dev/null) || true
    if [[ -z "$rescue_iface" ]]; then
        fatal "Could not detect network interface."
    fi

    INTERFACE=$(ssh $ssh_opts root@"$SERVER_IP" "
        iface='$rescue_iface'
        # Read the PCI slot from sysfs (e.g. 0000:03:00.0)
        slot=\$(basename \$(readlink -f /sys/class/net/\$iface/device) 2>/dev/null) || true
        if [ -n \"\$slot\" ]; then
            # Parse domain:bus:device.function → enp{bus}s{device}f{function}
            # Strip leading domain (0000:), then parse bus:dev.fn
            bdf=\${slot##*:}  # get last part after last colon: e.g. 00.0
            bus_hex=\$(echo \$slot | rev | cut -d: -f2 | rev)  # second-to-last field
            dev_hex=\${bdf%%.*}
            fn=\${bdf##*.}
            bus=\$((16#\$bus_hex))
            dev=\$((16#\$dev_hex))
            if [ \"\$fn\" = \"0\" ]; then
                echo \"enp\${bus}s\${dev}\"
            else
                echo \"enp\${bus}s\${dev}f\${fn}\"
            fi
        else
            echo \"\$iface\"
        fi
    " 2>/dev/null) || true

    if [[ -z "$INTERFACE" ]]; then
        INTERFACE="$rescue_iface"
        warn "Could not derive predictable interface name, using rescue name: $INTERFACE"
    fi
    success "Detected interface: $INTERFACE (rescue: $rescue_iface)"

    # Detect prefix length (use rescue_iface since we're still in rescue mode)
    PREFIX_LENGTH=$(ssh $ssh_opts root@"$SERVER_IP" \
        "ip -4 addr show dev $rescue_iface | grep 'inet ' | head -1 | awk '{print \$2}' | cut -d/ -f2" 2>/dev/null) || true
    if [[ -z "$PREFIX_LENGTH" ]]; then
        warn "Could not detect prefix length, defaulting to 26"
        PREFIX_LENGTH="26"
    fi
    success "Detected prefix length: /$PREFIX_LENGTH"

    # --- Preview deployment settings ---
    header "Preview Deployment Settings (optional)"
    echo -e "Configure PR preview deployments? This sets up Caddy, PostgreSQL, and a GitHub webhook."
    echo ""

    SETUP_PREVIEWS="n"
    if confirm "Enable PR preview deployments?"; then
        SETUP_PREVIEWS="y"

        echo -en "${BOLD}Enter your preview domain (e.g. preview.example.com):${NC} "
        read -r PREVIEW_DOMAIN
        if [[ -z "$PREVIEW_DOMAIN" ]]; then
            fatal "Preview domain is required."
        fi
        success "Preview domain: $PREVIEW_DOMAIN"

        echo -en "${BOLD}GitHub token (for cloning repos into preview containers):${NC} "
        read -r PREVIEW_GITHUB_TOKEN
        if [[ -z "$PREVIEW_GITHUB_TOKEN" ]]; then
            fatal "GitHub token is required."
        fi
        success "GitHub token set."

        echo -en "${BOLD}GitHub webhook secret (leave blank to auto-generate):${NC} "
        read -r GITHUB_WEBHOOK_SECRET
        if [[ -z "$GITHUB_WEBHOOK_SECRET" ]]; then
            GITHUB_WEBHOOK_SECRET=$(head -c 32 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 32)
            success "Generated webhook secret: $GITHUB_WEBHOOK_SECRET"
            echo -e "  ${YELLOW}Save this secret — you'll need it when configuring the GitHub webhook.${NC}"
        else
            success "Webhook secret set."
        fi

        echo -en "${BOLD}Allowed repos (comma-separated owner/repo, leave blank for all):${NC} "
        read -r ALLOWED_REPOS
    fi

    # --- Summary ---
    header "Configuration Summary"
    echo -e "  ${BOLD}Server IP:${NC}     $SERVER_IP"
    echo -e "  ${BOLD}Gateway:${NC}       $GATEWAY"
    echo -e "  ${BOLD}Interface:${NC}     $INTERFACE"
    echo -e "  ${BOLD}Prefix length:${NC} /$PREFIX_LENGTH"
    echo -e "  ${BOLD}SSH Key:${NC}       ${SSH_KEY:0:50}..."
    if [[ "$SETUP_PREVIEWS" == "y" ]]; then
        echo -e "  ${BOLD}Previews:${NC}      enabled"
        echo -e "  ${BOLD}Preview domain:${NC} *.${PREVIEW_DOMAIN}"
    fi
    echo ""

    if ! confirm "Proceed with these settings?"; then
        fatal "Aborted by user."
    fi
}

# -- Phase 2: Install NixOS --------------------------------------------------
install_nixos() {
    header "Phase 2: Install NixOS via nixos-anywhere"

    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local source_dir="$script_dir/initial-install"

    # Copy to a temp directory OUTSIDE the git repo so Nix evaluates the
    # filesystem directly instead of using git-based source resolution
    # (which may not pick up in-place sed modifications reliably).
    local work_dir
    work_dir=$(mktemp -d)
    cp -a "$source_dir"/. "$work_dir/"

    local config_file="$work_dir/configuration.nix"

    info "Substituting values in initial-install/configuration.nix..."

    # Perform substitutions on the temp copy
    sed -i.tmp "s|YOUR.SERVER.IP.HERE|$SERVER_IP|g" "$config_file"
    sed -i.tmp "s|YOUR.GATEWAY.IP.HERE|$GATEWAY|g" "$config_file"
    sed -i.tmp "s|ssh-ed25519 AAAA... your-key-here|$SSH_KEY|g" "$config_file"
    sed -i.tmp "s|prefixLength = [0-9]*;|prefixLength = $PREFIX_LENGTH;|g" "$config_file"

    # Handle interface name substitution (default placeholder is enp3s0)
    sed -i.tmp "s|interfaces\.enp3s0|interfaces.$INTERFACE|g" "$config_file"
    sed -i.tmp "s|interface = \"enp3s0\"|interface = \"$INTERFACE\"|g" "$config_file"

    rm -f "$config_file.tmp"

    success "Configuration updated."

    # Ensure rescue system has working DNS (Hetzner rescue may lack resolv.conf
    # and nixos-anywhere's build env expects systemd-resolved which doesn't exist)
    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o LogLevel=ERROR"
    info "Ensuring DNS is configured in rescue system..."
    ssh $ssh_opts root@"$SERVER_IP" 'grep -q nameserver /etc/resolv.conf 2>/dev/null || echo "nameserver 1.1.1.1" > /etc/resolv.conf'

    info "Running nixos-anywhere (this will take 5-10 minutes)..."
    echo ""

    if ! (cd "$work_dir" && nix run github:nix-community/nixos-anywhere/1.13.0 -- \
        --flake '.#hetzner-dedicated' \
        root@"$SERVER_IP"); then
        rm -rf "$work_dir"
        fatal "nixos-anywhere failed."
    fi

    rm -rf "$work_dir"

    success "nixos-anywhere completed."
    info "Waiting for server to come back online..."

    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o LogLevel=ERROR"
    local attempts=0
    local max_attempts=60

    while (( attempts < max_attempts )); do
        if ssh $ssh_opts root@"$SERVER_IP" true 2>/dev/null; then
            success "Server is back online!"
            return
        fi
        attempts=$((attempts + 1))
        sleep 5
    done

    fatal "Server did not come back online after 5 minutes. Check the Hetzner console."
}

# -- Phase 3: Configure server -----------------------------------------------
configure_server() {
    header "Phase 3: Configure Server with Agent Container Support"

    local script_dir
    script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
    local server_dir="$script_dir/server-config"
    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"

    # Create temp working copies (exclude node_modules and dist from webhook)
    local tmp_dir
    tmp_dir=$(mktemp -d)
    rsync -a --exclude='node_modules' --exclude='dist' "$server_dir"/ "$tmp_dir/"

    info "Substituting values in server-config files..."

    # --- configuration.nix ---
    local cfg="$tmp_dir/configuration.nix"
    sed -i.tmp "s|YOUR.SERVER.IP.HERE|$SERVER_IP|g" "$cfg"
    sed -i.tmp "s|YOUR.GATEWAY.IP.HERE|$GATEWAY|g" "$cfg"
    sed -i.tmp "s|ssh-ed25519 AAAA... your-key-here|$SSH_KEY|g" "$cfg"
    sed -i.tmp "s|prefixLength = [0-9]*;|prefixLength = $PREFIX_LENGTH;|g" "$cfg"
    sed -i.tmp "s|interfaces\.enp3s0|interfaces.$INTERFACE|g" "$cfg"
    sed -i.tmp "s|interface = \"enp3s0\"|interface = \"$INTERFACE\"|g" "$cfg"
    sed -i.tmp "s|externalInterface = \"enp3s0\"|externalInterface = \"$INTERFACE\"|g" "$cfg"
    rm -f "$cfg.tmp"

    # --- agent-config.nix ---
    local agent_cfg="$tmp_dir/agent-config.nix"
    sed -i.tmp "s|ssh-ed25519 AAAA... your-key-here|$SSH_KEY|g" "$agent_cfg"
    rm -f "$agent_cfg.tmp"

    # --- preview-config.nix ---
    local preview_cfg="$tmp_dir/preview-config.nix"
    sed -i.tmp "s|ssh-ed25519 AAAA... your-key-here|$SSH_KEY|g" "$preview_cfg"
    rm -f "$preview_cfg.tmp"

    # --- vertex-preview-config.nix ---
    local vertex_cfg="$tmp_dir/vertex-preview-config.nix"
    if [[ -f "$vertex_cfg" ]]; then
        sed -i.tmp "s|ssh-ed25519 AAAA... your-key-here|$SSH_KEY|g" "$vertex_cfg"
        rm -f "$vertex_cfg.tmp"
    fi

    # --- configuration.nix: substitute preview domain ---
    if [[ "$SETUP_PREVIEWS" == "y" ]]; then
        sed -i.tmp "s|preview.DOMAIN|${PREVIEW_DOMAIN}|g" "$cfg"
        rm -f "$cfg.tmp"
    fi

    success "Config files prepared."

    info "Copying server configuration to /etc/nixos/..."
    scp $ssh_opts -r "$tmp_dir"/. root@"$SERVER_IP":/etc/nixos/

    success "Files copied."

    info "Creating directories..."
    ssh $ssh_opts root@"$SERVER_IP" "mkdir -p /var/secrets/claude /var/lib/claude-agents && chmod 755 /var/secrets /var/secrets/claude"

    # Create preview directories and secrets
    if [[ "$SETUP_PREVIEWS" == "y" ]]; then
        info "Setting up preview deployment infrastructure..."

        ssh $ssh_opts root@"$SERVER_IP" "mkdir -p /var/lib/preview-deploys /etc/caddy/previews /opt/preview-webhook"

        # Create webhook Caddy route for HTTPS
        ssh $ssh_opts root@"$SERVER_IP" "echo 'webhook.${PREVIEW_DOMAIN} {
    reverse_proxy localhost:3100
}' > /etc/caddy/previews/webhook.caddy"

        # Write /var/secrets/preview.env
        ssh $ssh_opts root@"$SERVER_IP" "cat > /var/secrets/preview.env <<ENVEOF
GITHUB_TOKEN=${PREVIEW_GITHUB_TOKEN}
GITHUB_WEBHOOK_SECRET=${GITHUB_WEBHOOK_SECRET}
PREVIEW_DOMAIN=${PREVIEW_DOMAIN}
WEBHOOK_PORT=3100
ALLOWED_REPOS=${ALLOWED_REPOS:-}
ENVEOF
chmod 600 /var/secrets/preview.env"

        success "Preview secrets written."

        # Copy webhook source (build happens after nixos-rebuild makes npm available)
        info "Copying preview webhook source..."
        scp $ssh_opts -r "$tmp_dir/preview-webhook"/. root@"$SERVER_IP":/opt/preview-webhook/
        success "Preview webhook source copied."
    fi

    success "Directories created."

    info "Running nixos-rebuild switch (this may take a while on first run)..."
    local gen_before gen_after rebuild_output
    gen_before=$(ssh $ssh_opts root@"$SERVER_IP" "readlink /nix/var/nix/profiles/system")

    if ! rebuild_output=$(ssh $ssh_opts root@"$SERVER_IP" "cd /etc/nixos && nixos-rebuild switch 2>&1"); then
        # Check if it failed due to the empty Caddy plugin hash — extract the real hash and retry
        local caddy_hash
        caddy_hash=$(echo "$rebuild_output" | sed -n 's/.*got: *\(sha256-[A-Za-z0-9+/=]*\).*/\1/p' | head -1)
        if [[ -n "$caddy_hash" ]]; then
            warn "Caddy plugin hash was empty. Got: $caddy_hash — patching and retrying..."
            ssh $ssh_opts root@"$SERVER_IP" "sed -i 's|hash = \".*\";.*# Leave empty on first build.*|hash = \"$caddy_hash\";|' /etc/nixos/configuration.nix"
            if ! ssh $ssh_opts root@"$SERVER_IP" "cd /etc/nixos && nixos-rebuild switch"; then
                warn "nixos-rebuild switch exited with errors on retry, verifying..."
                gen_after=$(ssh $ssh_opts root@"$SERVER_IP" "readlink /nix/var/nix/profiles/system")
                if [[ "$gen_before" == "$gen_after" ]]; then
                    fatal "nixos-rebuild switch failed and no new generation was created. SSH into the server to investigate."
                fi
                warn "New generation active ($gen_after). Non-critical service errors occurred during activation — safe to continue."
            fi
        else
            warn "nixos-rebuild switch exited with errors, verifying the switch applied..."
            gen_after=$(ssh $ssh_opts root@"$SERVER_IP" "readlink /nix/var/nix/profiles/system")
            if [[ "$gen_before" == "$gen_after" ]]; then
                echo "$rebuild_output"
                fatal "nixos-rebuild switch failed and no new generation was created. SSH into the server to investigate."
            fi
            warn "New generation active ($gen_after). Non-critical service errors occurred during activation — safe to continue."
        fi
    fi

    success "Server configured with agent container support."

    if [[ "$SETUP_PREVIEWS" == "y" ]]; then
        info "Building preview webhook service..."
        ssh $ssh_opts root@"$SERVER_IP" "cd /opt/preview-webhook && npm ci && npm run build"
        success "Preview webhook service deployed."
    fi

    info "Pre-building agent container closure (so first 'agent create' is instant)..."
    ssh $ssh_opts root@"$SERVER_IP" "agent build" || {
        warn "Agent pre-build failed. The first 'agent create' will build it automatically."
    }

    if [[ "$SETUP_PREVIEWS" == "y" ]]; then
        info "Pre-building preview container closure..."
        ssh $ssh_opts root@"$SERVER_IP" "preview build" || {
            warn "Preview pre-build failed. The first 'preview create' will build it automatically."
        }
    fi

    if [[ "$SETUP_VERTEX" == "y" ]]; then
        info "Pre-building vertex preview container closure (this may take a while)..."
        ssh $ssh_opts root@"$SERVER_IP" "preview build --type vertex" || {
            warn "Vertex pre-build failed. The first 'preview create --type vertex' will build it automatically."
        }
    fi

    # Clean up
    rm -rf "$tmp_dir"
}

# -- Phase 4: Claude login ---------------------------------------------------
setup_claude() {
    header "Phase 4: Claude Code Login"

    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"

    echo -e "${BOLD}This will run 'claude login' on the server.${NC}"
    echo -e "You'll see an OAuth URL - ${YELLOW}open it in your browser${NC} to authenticate."
    echo -e "The login process is interactive and runs on the remote server."
    echo ""

    if ! confirm "Ready to log in to Claude?"; then
        warn "Skipping Claude login. You can do this later by running:"
        echo "  ssh -t root@$SERVER_IP 'CLAUDE_CONFIG_DIR=/var/secrets/claude claude login'"
        return
    fi

    ssh -t $ssh_opts root@"$SERVER_IP" 'CLAUDE_CONFIG_DIR=/var/secrets/claude claude login' || {
        warn "Claude login may have exited with an error (this can happen if you dismiss the trust prompt — that's OK)."
    }

    # Always fix permissions if credentials exist — the login may have succeeded
    # even if claude exited non-zero (e.g. trust prompt dismissed).
    # Files must be world-readable so they can be copied into containers.
    if ssh $ssh_opts root@"$SERVER_IP" "test -f /var/secrets/claude/.credentials.json"; then
        info "Fixing credential permissions..."
        ssh $ssh_opts root@"$SERVER_IP" "chmod -R a+rX /var/secrets/claude"
        success "Credentials found and permissions set."
    else
        warn "No credentials found at /var/secrets/claude/.credentials.json"
        warn "You can log in later with:"
        echo "  ssh -t root@$SERVER_IP 'CLAUDE_CONFIG_DIR=/var/secrets/claude claude login'"
        echo "  ssh root@$SERVER_IP 'chmod -R a+rX /var/secrets/claude'"
    fi
}

# -- Phase 5: Done -----------------------------------------------------------
print_summary() {
    header "Setup Complete!"

    echo -e "${GREEN}Your NixOS server with agent container support is ready.${NC}"
    echo ""
    echo -e "${BOLD}Quick Start:${NC}"
    echo ""
    echo -e "  ${CYAN}# Create a new agent container (instant after first build)${NC}"
    echo "  ssh root@$SERVER_IP 'agent create myagent'"
    echo ""
    echo -e "  ${CYAN}# SSH into the agent${NC}"
    echo "  ssh -J root@$SERVER_IP agent@<container-ip>"
    echo ""
    echo -e "  ${CYAN}# Run Claude in the container${NC}"
    echo "  claude"
    echo ""
    echo -e "  ${CYAN}# Run Claude headlessly (skip all permission prompts)${NC}"
    echo "  claude --dangerously-skip-permissions"
    echo ""
    echo -e "  ${CYAN}# List agents${NC}"
    echo "  ssh root@$SERVER_IP 'agent list'"
    echo ""
    echo -e "  ${CYAN}# Destroy when done${NC}"
    echo "  ssh root@$SERVER_IP 'agent destroy myagent'"
    echo ""
    if [[ "$SETUP_PREVIEWS" == "y" ]]; then
        echo ""
        echo -e "${BOLD}Preview Deployments:${NC}"
        echo ""
        echo -e "  ${CYAN}# Deploy a branch as a preview${NC}"
        echo "  ssh root@$SERVER_IP 'preview create owner/repo branch-name'"
        echo ""
        echo -e "  ${CYAN}# List active previews${NC}"
        echo "  ssh root@$SERVER_IP 'preview list'"
        echo ""
        echo -e "  ${CYAN}# View build logs${NC}"
        echo "  ssh root@$SERVER_IP 'preview logs <slug> --follow'"
        echo ""
        echo -e "  ${CYAN}# Destroy a preview${NC}"
        echo "  ssh root@$SERVER_IP 'preview destroy <slug>'"
        echo ""
        echo -e "  ${BOLD}Webhook URL:${NC}  https://$SERVER_IP:3100/webhook/github"
        echo -e "  ${BOLD}DNS required:${NC} *.${PREVIEW_DOMAIN} A → $SERVER_IP"
        echo ""
    fi

    echo -e "See ${BOLD}README.md${NC} for more details on managing agents."
}

# -- Main ---------------------------------------------------------------------
SETUP_VERTEX="n"

main() {
    # Parse flags
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --vertex) SETUP_VERTEX="y"; shift ;;
            *) fatal "Unknown argument: $1" ;;
        esac
    done

    echo -e "${BOLD}${CYAN}"
    echo "  ╔══════════════════════════════════════════════════════════╗"
    echo "  ║   NixOS Setup for Claude Code Agents                   ║"
    echo "  ║   Hetzner Dedicated Server                             ║"
    echo "  ╚══════════════════════════════════════════════════════════╝"
    echo -e "${NC}"

    if [[ "$SETUP_VERTEX" == "y" ]]; then
        info "Vertex preview support enabled."
    fi

    check_prerequisites
    gather_info
    install_nixos
    configure_server
    setup_claude
    print_summary
}

main "$@"
