# =============================================================================
# Agent container lifecycle manager
# Uses imperative nixos-container with nspawn for instant agent creation
# =============================================================================

AGENT_DIR="/var/lib/claude-agents"
CREDS_DIR="/var/secrets/claude"
FLAKE_DIR="/etc/nixos"
SUBNET="10.100.0"
SYSTEM_PATH_CACHE="$AGENT_DIR/.system-path"
POOL_FILE="$AGENT_DIR/.pool"
POOL_SIZE_FILE="$AGENT_DIR/.pool-size"
DEFAULT_POOL_SIZE=3

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
# Counter stored in $AGENT_DIR/.next_slot, starts at 1

next_slot() {
    local slot_file="$AGENT_DIR/.next_slot"
    local slot
    if [[ -f "$slot_file" ]]; then
        slot=$(cat "$slot_file")
    else
        slot=1
    fi
    echo "$slot"
}

bump_slot() {
    local slot_file="$AGENT_DIR/.next_slot"
    local current
    current=$(next_slot)
    echo $(( current + 1 )) > "$slot_file"
}

slot_to_ips() {
    local slot="$1"
    local host_last=$(( slot * 2 ))
    local local_last=$(( slot * 2 + 1 ))
    echo "${SUBNET}.${host_last}" "${SUBNET}.${local_last}"
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

    # Track the agent
    echo "${slot} ${host_ip} ${local_ip}" > "$AGENT_DIR/$name"
    bump_slot

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

# -- Pool helpers -------------------------------------------------------------

get_pool_size() {
    if [[ -f "$POOL_SIZE_FILE" ]]; then
        cat "$POOL_SIZE_FILE"
    else
        echo "$DEFAULT_POOL_SIZE"
    fi
}

pool_available() {
    if [[ -f "$POOL_FILE" ]]; then
        grep -c . "$POOL_FILE" 2>/dev/null || echo 0
    else
        echo 0
    fi
}

# -- Pool commands ------------------------------------------------------------

cmd_pool_init() {
    local size="${1:-$(get_pool_size)}"
    ensure_root
    ensure_agent_dir

    echo "$size" > "$POOL_SIZE_FILE"
    > "$POOL_FILE"  # truncate

    info "Initializing pool with $size containers..."
    local system_path
    system_path=$(get_system_path)

    for i in $(seq 1 "$size"); do
        local name="pool-${i}"
        if [[ -f "$AGENT_DIR/$name" ]]; then
            info "Pool container '$name' already exists, skipping."
            echo "$name" >> "$POOL_FILE"
            continue
        fi

        local slot
        slot=$(next_slot)
        read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"

        info "Creating pool container '$name' ($i/$size)..."
        nixos-container create "$name" \
            --system-path "$system_path" \
            --host-address "$host_ip" \
            --local-address "$local_ip"

        local container_root="/var/lib/nixos-containers/${name}"
        local creds_dest="${container_root}/mnt/claude-creds"
        mkdir -p "$creds_dest"
        cp -r "${CREDS_DIR}"/. "$creds_dest/"

        echo "${slot} ${host_ip} ${local_ip}" > "$AGENT_DIR/$name"
        bump_slot

        nixos-container start "$name"
        echo "$name" >> "$POOL_FILE"
    done

    success "Pool initialized: $size containers ready."
}

cmd_pool_claim() {
    ensure_root
    ensure_agent_dir

    local avail
    avail=$(pool_available)

    if [[ "$avail" -eq 0 ]]; then
        # Pool empty — fall back to on-demand creation
        info "Pool empty, creating container on demand..."
        # Find next pool index for naming
        local idx=1
        while [[ -f "$AGENT_DIR/pool-${idx}" ]]; do
            idx=$(( idx + 1 ))
        done
        local name="pool-${idx}"
        cmd_create "$name" >/dev/null 2>&1
        echo "$name"
        return
    fi

    # Take the first available container from the pool
    local name
    name=$(head -n1 "$POOL_FILE")
    # Remove it from the pool list
    sed -i "1d" "$POOL_FILE"

    # Verify the container is still running, restart if needed
    if ! nixos-container status "$name" 2>/dev/null | grep -q "running"; then
        info "Pool container '$name' was stopped, restarting..." >&2
        nixos-container start "$name" 2>/dev/null || true
    fi

    echo "$name"
}

cmd_pool_release() {
    local name="${1:-}"
    if [[ -z "$name" ]]; then
        fatal "Usage: agent pool-release <name>"
    fi

    ensure_root

    if [[ ! -f "$AGENT_DIR/$name" ]]; then
        fatal "Agent '$name' not found."
    fi

    local target_size
    target_size=$(get_pool_size)
    local current_avail
    current_avail=$(pool_available)

    if [[ "$current_avail" -ge "$target_size" ]]; then
        # Pool is full — destroy instead of recycling
        info "Pool at capacity ($current_avail/$target_size), destroying '$name'..."
        cmd_destroy "$name"
        return
    fi

    info "Recycling '$name' back into pool..."

    # Stop the container
    nixos-container stop "$name" 2>/dev/null || true

    # Reset work files: wipe /home/agent contents inside the container
    local container_root="/var/lib/nixos-containers/${name}"
    rm -rf "${container_root}/home/agent/"* 2>/dev/null || true
    rm -rf "${container_root}/home/agent/".* 2>/dev/null || true

    # Re-copy credentials (they may have been modified during the task)
    local creds_dest="${container_root}/mnt/claude-creds"
    mkdir -p "$creds_dest"
    cp -r "${CREDS_DIR}"/. "$creds_dest/"

    # Restart the container
    nixos-container start "$name"

    # Add back to the pool
    echo "$name" >> "$POOL_FILE"
    success "Agent '$name' recycled into pool."
}

cmd_pool_status() {
    ensure_agent_dir

    local target_size
    target_size=$(get_pool_size)
    local avail
    avail=$(pool_available)

    echo -e "${BOLD}Agent Pool Status${NC}"
    echo -e "  Target size:  $target_size"
    echo -e "  Available:    $avail"
    echo -e "  Deficit:      $(( target_size - avail > 0 ? target_size - avail : 0 ))"

    if [[ "$avail" -gt 0 ]]; then
        echo -e "\n${BOLD}Available containers:${NC}"
        while IFS= read -r name; do
            [[ -z "$name" ]] && continue
            if [[ -f "$AGENT_DIR/$name" ]]; then
                read -r _slot host_ip local_ip < "$AGENT_DIR/$name"
                echo -e "  $name  ($local_ip)"
            else
                echo -e "  $name  ${RED}(tracking file missing)${NC}"
            fi
        done < "$POOL_FILE"
    fi
}

cmd_pool_refill() {
    ensure_root
    ensure_agent_dir

    local target_size
    target_size=$(get_pool_size)
    local avail
    avail=$(pool_available)

    if [[ "$avail" -ge "$target_size" ]]; then
        info "Pool already at target size ($avail/$target_size)."
        return
    fi

    local needed=$(( target_size - avail ))
    info "Refilling pool: $needed container(s) needed (have $avail, target $target_size)..."

    local system_path
    system_path=$(get_system_path)

    local created=0
    local idx=1
    while [[ $created -lt $needed ]]; do
        local name="pool-${idx}"
        idx=$(( idx + 1 ))

        # Skip if this name is already in the pool or in use
        if [[ -f "$AGENT_DIR/$name" ]]; then
            continue
        fi

        local slot
        slot=$(next_slot)
        read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"

        info "Creating pool container '$name' ($(( created + 1 ))/$needed)..."
        nixos-container create "$name" \
            --system-path "$system_path" \
            --host-address "$host_ip" \
            --local-address "$local_ip"

        local container_root="/var/lib/nixos-containers/${name}"
        local creds_dest="${container_root}/mnt/claude-creds"
        mkdir -p "$creds_dest"
        cp -r "${CREDS_DIR}"/. "$creds_dest/"

        echo "${slot} ${host_ip} ${local_ip}" > "$AGENT_DIR/$name"
        bump_slot

        nixos-container start "$name"
        echo "$name" >> "$POOL_FILE"

        created=$(( created + 1 ))
    done

    success "Pool refilled: $created container(s) created. Now at $(pool_available)/$target_size."
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
    echo "  pool-init [size]   Initialize pool with N prewarmed containers"
    echo "  pool-claim         Claim a container from the pool (prints name)"
    echo "  pool-release <n>   Recycle a container back into the pool"
    echo "  pool-status        Show pool size and available containers"
    echo "  pool-refill        Top up the pool to target size"
    echo "  help               Show this help"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo "  agent build              # pre-build (optional, done automatically)"
    echo "  agent create myagent"
    echo "  agent ssh myagent"
    echo "  agent destroy myagent"
    echo "  agent pool-init 5        # create 5 prewarmed containers"
    echo "  agent pool-claim         # grab a container from the pool"
    echo "  agent pool-release pool-3  # recycle container back"
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
    pool-init)    cmd_pool_init "$@" ;;
    pool-claim)   cmd_pool_claim "$@" ;;
    pool-release) cmd_pool_release "$@" ;;
    pool-status)  cmd_pool_status "$@" ;;
    pool-refill)  cmd_pool_refill "$@" ;;
    help|--help|-h) cmd_help ;;
    *)       fatal "Unknown command: $command. Run 'agent help' for usage." ;;
esac
