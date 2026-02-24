#!/usr/bin/env bash
set -euo pipefail

# =============================================================================
# NixOS Setup Script for Claude Code Agents on Hetzner
# =============================================================================
# Single entry point for full setup: NixOS installation via nixos-anywhere,
# repo clone on server, secret upload, and interactive server configuration.
#
# Usage:
#   ./setup.sh                 # Full setup (NixOS install + configure)
#   ./setup.sh --skip-install  # Skip NixOS install (server already running)
#
# Flow:
#   Phase 1: Gather info (server IP, SSH key, rescue-mode hardware detection)
#   Phase 2: Install NixOS via nixos-anywhere
#   Phase 3: Clone repo on server, upload secrets, run server-setup.sh
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

    if [[ "$SKIP_INSTALL" != "y" ]]; then
        command -v nix &>/dev/null || missing+=("nix (with flakes enabled)")
    fi
    command -v ssh &>/dev/null || missing+=("ssh")
    command -v scp &>/dev/null || missing+=("scp")
    command -v sed &>/dev/null || missing+=("sed")
    command -v git &>/dev/null || missing+=("git")

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

    # Detect 1Password SSH agent -- its socket path in ~/.ssh/config is the
    # telltale sign.  When present and we have a key file, add IdentitiesOnly
    # so SSH never queries the 1Password agent (which pops a GUI prompt that
    # hangs non-interactive loops).
    local onepassword_agent=false
    if [[ -f "$HOME/.ssh/config" ]] && grep -q '2BUA8C4S2C.com.1password' "$HOME/.ssh/config" 2>/dev/null; then
        onepassword_agent=true
    fi

    # Build SSH identity flag (used in ssh_opts throughout the script)
    if [[ -n "$SSH_IDENTITY" ]]; then
        if $onepassword_agent; then
            SSH_IDENTITY_OPT="-i $SSH_IDENTITY -o IdentitiesOnly=yes"
            success "SSH key selected (identity: $SSH_IDENTITY)."
            info "1Password SSH agent detected -- using IdentitiesOnly to prevent GUI prompts."
        else
            SSH_IDENTITY_OPT="-i $SSH_IDENTITY"
            success "SSH key selected (identity: $SSH_IDENTITY)."
        fi
    else
        SSH_IDENTITY_OPT=""
        if $onepassword_agent; then
            warn "1Password SSH agent detected but no key file specified."
            warn "SSH may stall waiting for 1Password approval popups during automated steps."
            warn "Consider specifying a key file (e.g. ~/.ssh/id_ed25519) to avoid hangs."
        fi
        success "SSH key selected (will use SSH agent/defaults for auth)."
    fi

    # --- SSH connection test + hardware detection ---
    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o LogLevel=ERROR"

    if [[ "$SKIP_INSTALL" != "y" ]]; then
        # Rescue-mode detection (only needed for NixOS install)
        info "Connecting to rescue mode to detect network configuration..."
        info "(Make sure the server is in rescue mode and you can SSH in.)"
        echo ""

        if ! ssh $ssh_opts root@"$SERVER_IP" true; then
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
                # Parse domain:bus:device.function -> enp{bus}s{device}f{function}
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

        # --- Detect disk devices ---
        info "Detecting disk devices..."
        local disk_list
        disk_list=$(ssh $ssh_opts root@"$SERVER_IP" "lsblk -d -n -o NAME,TYPE | awk '\$2==\"disk\" && \$1!~/^loop/ {print \"/dev/\" \$1}'" 2>/dev/null) || true

        if [[ -z "$disk_list" ]]; then
            fatal "Could not detect any disk devices on the server."
        fi

        local disks=()
        while IFS= read -r line; do
            disks+=("$line")
        done <<< "$disk_list"

        if [[ ${#disks[@]} -lt 2 ]]; then
            fatal "Expected at least 2 disks for RAID 1, found ${#disks[@]}: ${disks[*]}"
        fi

        info "Found ${#disks[@]} disk(s):"
        for i in "${!disks[@]}"; do
            echo -e "  ${BOLD}$((i+1)))${NC} ${disks[$i]}"
        done

        if [[ ${#disks[@]} -eq 2 ]]; then
            DISK_DEVICE_0="${disks[0]}"
            DISK_DEVICE_1="${disks[1]}"
            success "Using ${DISK_DEVICE_0} and ${DISK_DEVICE_1} for RAID 1."
        else
            echo -en "${BOLD}Pick disk 1 (1-${#disks[@]}):${NC} "
            read -r d1
            echo -en "${BOLD}Pick disk 2 (1-${#disks[@]}):${NC} "
            read -r d2
            if [[ "$d1" == "$d2" ]]; then
                fatal "Cannot use the same disk twice."
            fi
            DISK_DEVICE_0="${disks[$((d1-1))]}"
            DISK_DEVICE_1="${disks[$((d2-1))]}"
            success "Using ${DISK_DEVICE_0} and ${DISK_DEVICE_1} for RAID 1."
        fi

        # --- Detect kernel modules ---
        info "Detecting hardware for kernel modules..."

        # Disk modules: NVMe vs SATA
        if [[ "$DISK_DEVICE_0" == /dev/nvme* ]]; then
            INITRD_MODULES='"nvme"'
            success "NVMe disks detected — using nvme initrd module."
        else
            INITRD_MODULES='"ahci" "sd_mod"'
            success "SATA disks detected — using ahci/sd_mod initrd modules."
        fi

        # Network driver
        local net_driver
        net_driver=$(ssh $ssh_opts root@"$SERVER_IP" "basename \$(readlink -f /sys/class/net/$rescue_iface/device/driver) 2>/dev/null" 2>/dev/null) || true
        if [[ -n "$net_driver" ]]; then
            INITRD_MODULES="$INITRD_MODULES \"$net_driver\""
            success "Detected network driver: $net_driver"
        else
            INITRD_MODULES="$INITRD_MODULES \"r8169\""
            warn "Could not detect network driver, defaulting to r8169"
        fi

        # CPU vendor -> KVM module
        local cpu_vendor
        cpu_vendor=$(ssh $ssh_opts root@"$SERVER_IP" "grep -m1 vendor_id /proc/cpuinfo | awk '{print \$3}'" 2>/dev/null) || true
        if [[ "$cpu_vendor" == "GenuineIntel" ]]; then
            KVM_MODULE='"kvm-intel"'
            success "Detected Intel CPU."
        elif [[ "$cpu_vendor" == "AuthenticAMD" ]]; then
            KVM_MODULE='"kvm-amd"'
            success "Detected AMD CPU."
        else
            KVM_MODULE='"kvm-intel"'
            warn "Could not detect CPU vendor, defaulting to kvm-intel."
        fi
    else
        # --skip-install: just verify SSH works (server already running NixOS)
        info "Testing SSH connection to server..."
        if ! ssh $ssh_opts root@"$SERVER_IP" true; then
            fatal "Cannot SSH into root@$SERVER_IP."
        fi
        success "SSH connection successful."
    fi

    # --- Secret files (optional) ---
    header "Secret Files (optional)"
    echo -e "Upload secret files for preview deployments."
    echo -e "You can also upload them later manually to /var/secrets/ on the server."
    echo ""

    ORIGIN_CERT_PATH=""
    ORIGIN_KEY_PATH=""
    PEM_PATH=""

    if confirm "Upload Cloudflare Origin CA certificate + key?"; then
        echo -en "${BOLD}Path to Cloudflare Origin CA certificate (.pem):${NC} "
        read -r ORIGIN_CERT_PATH
        if [[ -z "$ORIGIN_CERT_PATH" ]] || [[ ! -f "$ORIGIN_CERT_PATH" ]]; then
            fatal "Certificate not found: ${ORIGIN_CERT_PATH:-<empty>}"
        fi
        echo -en "${BOLD}Path to Cloudflare Origin CA private key (.pem):${NC} "
        read -r ORIGIN_KEY_PATH
        if [[ -z "$ORIGIN_KEY_PATH" ]] || [[ ! -f "$ORIGIN_KEY_PATH" ]]; then
            fatal "Private key not found: ${ORIGIN_KEY_PATH:-<empty>}"
        fi
        success "Cloudflare Origin CA cert + key found."
    fi

    if confirm "Upload GitHub App private key?"; then
        echo -en "${BOLD}Path to GitHub App private key (.pem):${NC} "
        read -r PEM_PATH
        if [[ -z "$PEM_PATH" ]] || [[ ! -f "$PEM_PATH" ]]; then
            fatal "PEM file not found: ${PEM_PATH:-<empty>}"
        fi
        success "GitHub App private key found."
    fi

    # --- Summary ---
    header "Configuration Summary"
    echo -e "  ${BOLD}Server IP:${NC}     $SERVER_IP"
    if [[ "$SKIP_INSTALL" != "y" ]]; then
        echo -e "  ${BOLD}Gateway:${NC}       $GATEWAY"
        echo -e "  ${BOLD}Interface:${NC}     $INTERFACE"
        echo -e "  ${BOLD}Prefix length:${NC} /$PREFIX_LENGTH"
        echo -e "  ${BOLD}Disk 1:${NC}        $DISK_DEVICE_0"
        echo -e "  ${BOLD}Disk 2:${NC}        $DISK_DEVICE_1"
    fi
    echo -e "  ${BOLD}SSH Key:${NC}       ${SSH_KEY:0:50}..."
    local secrets_summary=""
    [[ -n "$ORIGIN_CERT_PATH" ]] && secrets_summary="Cloudflare cert"
    if [[ -n "$PEM_PATH" ]]; then
        [[ -n "$secrets_summary" ]] && secrets_summary="$secrets_summary, "
        secrets_summary="${secrets_summary}GitHub App PEM"
    fi
    echo -e "  ${BOLD}Secrets:${NC}       ${secrets_summary:-none}"
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

    # Substitute kernel modules in configuration.nix
    sed -i.tmp "s|INITRD_KERNEL_MODULES|$INITRD_MODULES|g" "$config_file"
    sed -i.tmp "s|KVM_KERNEL_MODULE|$KVM_MODULE|g" "$config_file"

    rm -f "$config_file.tmp"

    # Substitute disk devices in disk-config.nix
    local disk_config="$work_dir/disk-config.nix"
    sed -i.tmp "s|DISK_DEVICE_0|$DISK_DEVICE_0|g" "$disk_config"
    sed -i.tmp "s|DISK_DEVICE_1|$DISK_DEVICE_1|g" "$disk_config"
    rm -f "$disk_config.tmp"

    success "Configuration updated."

    # Ensure rescue system has working DNS (Hetzner rescue may lack resolv.conf
    # and nixos-anywhere's build env expects systemd-resolved which doesn't exist)
    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=10 -o LogLevel=ERROR"
    info "Ensuring DNS is configured in rescue system..."
    ssh $ssh_opts root@"$SERVER_IP" 'grep -q nameserver /etc/resolv.conf 2>/dev/null || echo "nameserver 1.1.1.1" > /etc/resolv.conf'

    info "Running nixos-anywhere (this will take 5-10 minutes)..."
    echo ""

    if ! (cd "$work_dir" && nix run github:nix-community/nixos-anywhere/1.13.0 -- \
        --build-on-remote \
        --flake '.#hetzner-dedicated' \
        root@"$SERVER_IP"); then
        rm -rf "$work_dir"
        fatal "nixos-anywhere failed."
    fi

    rm -rf "$work_dir"

    success "nixos-anywhere completed."
    info "Waiting for server to come back online..."

    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o ConnectTimeout=5 -o BatchMode=yes"
    local attempts=0
    local max_attempts=60

    while (( attempts < max_attempts )); do
        attempts=$((attempts + 1))
        echo -ne "\r  attempt ${attempts}/${max_attempts}... "

        # Run SSH with a hard 15-second timeout via a background watchdog.
        # ConnectTimeout only covers TCP; if auth stalls (e.g. an SSH agent
        # GUI prompt), SSH hangs forever.  macOS lacks `timeout`.
        #
        # The watchdog pattern (background jobs + wait + kill) does not work
        # under set -e, so we run it in a subshell with set +e.  The subshell
        # exits 0 only when SSH succeeds.
        if (
            set +e
            ssh $ssh_opts root@"$SERVER_IP" true &>/dev/null &
            pid=$!
            ( sleep 15 && kill $pid 2>/dev/null ) &
            wpid=$!
            wait $pid 2>/dev/null
            rc=$?
            kill $wpid 2>/dev/null
            wait $wpid 2>/dev/null
            exit $rc
        ); then
            echo ""
            success "Server is back online!"
            return
        fi

        sleep 5
    done

    echo ""
    fatal "Server did not come back online after 5 minutes. Check the Hetzner console."
}

# -- Phase 3: Server setup ---------------------------------------------------
setup_server() {
    header "Phase 3: Server Setup"

    local ssh_opts="$SSH_IDENTITY_OPT -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR"

    # Clone repo on server
    info "Setting up repository on server at /opt/src/..."
    local repo_url
    repo_url=$(git remote get-url origin 2>/dev/null) || true
    if [[ -z "$repo_url" ]]; then
        echo -en "${BOLD}Git remote URL for cloning on server:${NC} "
        read -r repo_url
        if [[ -z "$repo_url" ]]; then
            fatal "Repository URL is required."
        fi
    fi

    ssh $ssh_opts root@"$SERVER_IP" "
        if [ -d /opt/src/.git ]; then
            echo 'Repository already exists at /opt/src/, pulling latest...'
            cd /opt/src && git fetch origin && git pull origin main || true
        else
            git clone '$repo_url' /opt/src
        fi
    "
    success "Repository ready at /opt/src/"

    # Upload secret files
    ssh $ssh_opts root@"$SERVER_IP" "mkdir -p /var/secrets"

    if [[ -n "$ORIGIN_CERT_PATH" ]]; then
        info "Uploading Cloudflare Origin CA certificate + key..."
        scp $ssh_opts "$ORIGIN_CERT_PATH" root@"$SERVER_IP":/var/secrets/cloudflare-origin.pem
        scp $ssh_opts "$ORIGIN_KEY_PATH" root@"$SERVER_IP":/var/secrets/cloudflare-origin-key.pem
        success "Cloudflare Origin CA cert + key uploaded."
    fi

    if [[ -n "$PEM_PATH" ]]; then
        info "Uploading GitHub App private key..."
        scp $ssh_opts "$PEM_PATH" root@"$SERVER_IP":/var/secrets/github-app.pem
        ssh $ssh_opts root@"$SERVER_IP" "chmod 600 /var/secrets/github-app.pem"
        success "GitHub App private key uploaded."
    fi

    # Run server-setup.sh interactively
    info "Starting interactive server setup..."
    echo ""
    echo -e "${YELLOW}Handing off to server-setup.sh on the server.${NC}"
    echo -e "${YELLOW}Follow the prompts below to complete configuration.${NC}"
    echo ""

    ssh -t $ssh_opts root@"$SERVER_IP" 'cd /opt/src && ./server-setup.sh'
}

# -- Main ---------------------------------------------------------------------
SKIP_INSTALL="n"

main() {
    # Parse flags
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --skip-install) SKIP_INSTALL="y"; shift ;;
            *) fatal "Unknown argument: $1" ;;
        esac
    done

    echo -e "${BOLD}${CYAN}"
    echo "  ╔══════════════════════════════════════════════════════════╗"
    echo "  ║   NixOS Setup for Claude Code Agents                   ║"
    echo "  ║   Hetzner Dedicated Server                             ║"
    echo "  ╚══════════════════════════════════════════════════════════╝"
    echo -e "${NC}"

    check_prerequisites
    gather_info
    if [[ "$SKIP_INSTALL" == "y" ]]; then
        info "Skipping NixOS installation (--skip-install)."
    else
        install_nixos
    fi
    setup_server
}

main "$@"
