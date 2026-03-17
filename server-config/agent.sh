# =============================================================================
# Agent container lifecycle manager
# Uses imperative nixos-container with nspawn for instant agent creation
# =============================================================================

AGENT_DIR="/var/lib/claude-agents"
CREDS_DIR="/var/secrets/claude"
FLAKE_DIR="/etc/nixos"
SUBNET_PREFIX="10.100"
SYSTEM_PATH_CACHE="$AGENT_DIR/.system-path"

# -- Helpers ------------------------------------------------------------------
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

info()    { echo -e "${CYAN}[INFO]${NC} $*"; }
success() { echo -e "${GREEN}[OK]${NC} $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }
fatal()   { error "$@"; exit 1; }

ensure_root() {
    if [[ $EUID -ne 0 ]]; then
        fatal "This command must be run as root."
    fi
}

ensure_agent_dir() {
    mkdir -p "$AGENT_DIR"
}

# -- System path cache --------------------------------------------------------
# Pre-build the agent system closure once, then reuse the store path for
# instant container creation (skips nix evaluation on each create).

get_system_path() {
    if [[ -f "$SYSTEM_PATH_CACHE" ]]; then
        local cached
        cached=$(cat "$SYSTEM_PATH_CACHE")
        # Verify the path still exists in the store
        if [[ -d "$cached" ]]; then
            echo "$cached"
            return
        fi
    fi
    # Build and cache
    info "Building agent system closure (first time only)..." >&2
    local path
    path=$(nix build "${FLAKE_DIR}#nixosConfigurations.agent.config.system.build.toplevel" --no-link --print-out-paths)
    echo "$path" > "$SYSTEM_PATH_CACHE"
    echo "$path"
}

# -- IP allocation ------------------------------------------------------------
# Each container gets a pair: host-address .{N*2} / local-address .{N*2+1}
# Slots are recycled: we scan tracking files to find used slots, then pick
# the lowest available one.  Max slot is 127 (produces .254/.255).

MAX_SLOT=32512

used_slots() {
    # Collect slot numbers from agent tracking files ($AGENT_DIR/<name>)
    # and preview tracking files ($PREVIEW_DIR/<slug>).  Format: "slot ..."
    local dir file slot
    for dir in "$AGENT_DIR" "/var/lib/preview-deploys"; do
        [[ -d "$dir" ]] || continue
        for file in "$dir"/*; do
            [[ -f "$file" ]] || continue
            local name
            name=$(basename "$file")
            [[ "$name" == .* ]] && continue
            [[ "$name" == *.type ]] && continue
            [[ "$name" == *.meta ]] && continue
            [[ "$name" == *.sha ]] && continue
            slot=$(awk '{print $1}' "$file")
            [[ "$slot" =~ ^[0-9]+$ ]] && echo "$slot"
        done
    done
}

next_slot() {
    local -A in_use
    local s
    while read -r s; do
        in_use[$s]=1
    done < <(used_slots)

    local slot
    for (( slot=1; slot<=MAX_SLOT; slot++ )); do
        if [[ -z "${in_use[$slot]:-}" ]]; then
            echo "$slot"
            return
        fi
    done
    fatal "No free IP slots available (all $MAX_SLOT slots in use)."
}

slot_to_ips() {
    local slot="$1"
    local host_flat=$(( slot * 2 ))
    local local_flat=$(( slot * 2 + 1 ))
    local host_third=$(( host_flat / 256 ))
    local host_fourth=$(( host_flat % 256 ))
    local local_third=$(( local_flat / 256 ))
    local local_fourth=$(( local_flat % 256 ))
    echo "${SUBNET_PREFIX}.${host_third}.${host_fourth}" "${SUBNET_PREFIX}.${local_third}.${local_fourth}"
}

# -- Commands -----------------------------------------------------------------

cmd_build() {
    ensure_root
    ensure_agent_dir

    info "Building agent system closure..."
    local path
    path=$(nix build "${FLAKE_DIR}#nixosConfigurations.agent.config.system.build.toplevel" --no-link --print-out-paths)
    echo "$path" > "$SYSTEM_PATH_CACHE"
    success "System closure built and cached: $path"
}

cmd_create() {
    local name="${1:-}"
    if [[ -z "$name" ]]; then
        fatal "Usage: agent create <name>"
    fi

    ensure_root
    ensure_agent_dir

    # Check if already exists
    if [[ -f "$AGENT_DIR/$name" ]]; then
        fatal "Agent '$name' already exists. Use 'agent destroy $name' first."
    fi

    # Get pre-built system path (builds on first call, cached after)
    local system_path
    system_path=$(get_system_path)

    # Allocate IP
    local slot
    slot=$(next_slot)
    read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"

    info "Creating agent '$name' (host=$host_ip, container=$local_ip)..."

    # Create the container using pre-built system path (no nix evaluation)
    nixos-container create "$name" \
        --system-path "$system_path" \
        --host-address "$host_ip" \
        --local-address "$local_ip"

    # Copy credentials into container filesystem
    local container_root="/var/lib/nixos-containers/${name}"
    local creds_dest="${container_root}/mnt/claude-creds"
    mkdir -p "$creds_dest"
    cp -r "${CREDS_DIR}"/. "$creds_dest/"

    # Track the agent (this also reserves the slot for future allocations)
    echo "${slot} ${host_ip} ${local_ip}" > "$AGENT_DIR/$name"

    # Start the container
    nixos-container start "$name"

    success "Agent '$name' created and started."
    echo ""
    echo -e "  ${BOLD}Container IP:${NC}  $local_ip"
    echo -e "  ${BOLD}SSH:${NC}           ssh agent@$local_ip"
    echo -e "  ${BOLD}From outside:${NC}  ssh -J root@<server-ip> agent@$local_ip"
    echo ""
}

cmd_destroy() {
    local name="${1:-}"
    if [[ -z "$name" ]]; then
        fatal "Usage: agent destroy <name>"
    fi

    ensure_root

    if [[ ! -f "$AGENT_DIR/$name" ]]; then
        fatal "Agent '$name' not found."
    fi

    info "Destroying agent '$name'..."

    # Stop if running
    nixos-container stop "$name" 2>/dev/null || true

    # Destroy the container
    nixos-container destroy "$name"

    # Remove tracking file
    rm -f "$AGENT_DIR/$name"

    success "Agent '$name' destroyed."
}

cmd_list() {
    ensure_agent_dir

    local found=0
    echo -e "${BOLD}NAME            STATUS          HOST IP         CONTAINER IP${NC}"

    for f in "$AGENT_DIR"/*; do
        [[ -f "$f" ]] || continue
        local name
        name=$(basename "$f")
        # Skip hidden files (like .next_slot)
        [[ "$name" == .* ]] && continue

        found=1
        read -r _slot host_ip local_ip < "$f"

        # Check container status
        local status
        if nixos-container status "$name" 2>/dev/null | grep -q "running"; then
            status="${GREEN}running${NC}"
        else
            status="${YELLOW}stopped${NC}"
        fi

        printf "%-15s %-23b %-15s %s\n" "$name" "$status" "$host_ip" "$local_ip"
    done

    if [[ $found -eq 0 ]]; then
        echo -e "  ${CYAN}(no agents)${NC}"
    fi
}

cmd_ssh() {
    local name="${1:-}"
    if [[ -z "$name" ]]; then
        fatal "Usage: agent ssh <name>"
    fi

    if [[ ! -f "$AGENT_DIR/$name" ]]; then
        fatal "Agent '$name' not found. Run 'agent list' to see available agents."
    fi

    read -r _slot _host_ip local_ip < "$AGENT_DIR/$name"
    exec ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null agent@"$local_ip"
}

cmd_help() {
    echo -e "${BOLD}Usage:${NC} agent <command> [args]"
    echo ""
    echo -e "${BOLD}Commands:${NC}"
    echo "  create <name>      Create and start a new agent container"
    echo "  destroy <name>     Stop and remove an agent container"
    echo "  list               List all agent containers"
    echo "  ssh <name>         SSH into an agent container"
    echo "  build              Pre-build the agent system closure"
    echo "  help               Show this help"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo "  agent build              # pre-build (optional, done automatically)"
    echo "  agent create myagent"
    echo "  agent ssh myagent"
    echo "  agent destroy myagent"
}

# -- Main ---------------------------------------------------------------------
command="${1:-help}"
shift || true

case "$command" in
    create)       cmd_create "$@" ;;
    destroy)      cmd_destroy "$@" ;;
    list)         cmd_list ;;
    ssh)          cmd_ssh "$@" ;;
    build)        cmd_build ;;
    help|--help|-h) cmd_help ;;
    *)       fatal "Unknown command: $command. Run 'agent help' for usage." ;;
esac
