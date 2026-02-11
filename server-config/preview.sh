# =============================================================================
# Preview deployment lifecycle manager
# Deploys GitHub PR branches into nspawn containers as running web apps
# =============================================================================

AGENT_DIR="/var/lib/claude-agents"
PREVIEW_DIR="/var/lib/preview-deploys"
CADDY_DIR="/etc/caddy/previews"
FLAKE_DIR="/etc/nixos"
SECRETS_FILE="/var/secrets/preview.env"
SUBNET="10.100.0"
SYSTEM_PATH_CACHE="$PREVIEW_DIR/.system-path"

# -- Load secrets -------------------------------------------------------------
load_secrets() {
    if [[ -f "$SECRETS_FILE" ]]; then
        set -a
        # shellcheck source=/dev/null
        source "$SECRETS_FILE"
        set +a
    fi
}

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

ensure_dirs() {
    mkdir -p "$PREVIEW_DIR" "$CADDY_DIR" "$AGENT_DIR"
}

# -- System path cache --------------------------------------------------------

get_system_path() {
    if [[ -f "$SYSTEM_PATH_CACHE" ]]; then
        local cached
        cached=$(cat "$SYSTEM_PATH_CACHE")
        if [[ -d "$cached" ]]; then
            echo "$cached"
            return
        fi
    fi
    info "Building preview system closure (first time only)..." >&2
    local path
    path=$(nix build "${FLAKE_DIR}#nixosConfigurations.preview.config.system.build.toplevel" --no-link --print-out-paths)
    echo "$path" > "$SYSTEM_PATH_CACHE"
    echo "$path"
}

# -- IP allocation (shared with agents) --------------------------------------

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

# -- PostgreSQL helpers -------------------------------------------------------

generate_password() {
    head -c 32 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 24
}

create_db() {
    local slug="$1"
    local db_name="preview_${slug//-/_}"
    local db_user="preview_${slug//-/_}"
    local db_pass
    db_pass=$(generate_password)

    info "Creating PostgreSQL database '$db_name'..."

    # Create user and database
    sudo -u postgres psql -c "CREATE USER ${db_user} WITH PASSWORD '${db_pass}';" 2>/dev/null || \
        sudo -u postgres psql -c "ALTER USER ${db_user} WITH PASSWORD '${db_pass}';"
    sudo -u postgres psql -c "CREATE DATABASE ${db_name} OWNER ${db_user};" 2>/dev/null || true

    echo "$db_pass"
}

drop_db() {
    local slug="$1"
    local db_name="preview_${slug//-/_}"
    local db_user="preview_${slug//-/_}"

    info "Dropping PostgreSQL database '$db_name'..."
    sudo -u postgres psql -c "DROP DATABASE IF EXISTS ${db_name};"
    sudo -u postgres psql -c "DROP USER IF EXISTS ${db_user};"
}

# -- Caddy route management ---------------------------------------------------

write_caddy_route() {
    local slug="$1"
    local container_ip="$2"
    local domain="${PREVIEW_DOMAIN:-preview.example.com}"

    cat > "${CADDY_DIR}/${slug}.caddy" <<EOF
@${slug} host ${slug}.${domain}
handle @${slug} {
    reverse_proxy ${container_ip}:3000
}
EOF

    systemctl reload caddy
}

remove_caddy_route() {
    local slug="$1"
    rm -f "${CADDY_DIR}/${slug}.caddy"
    systemctl reload caddy
}

# -- Commands -----------------------------------------------------------------

cmd_build() {
    ensure_root
    ensure_dirs

    info "Building preview system closure..."
    local path
    path=$(nix build "${FLAKE_DIR}#nixosConfigurations.preview.config.system.build.toplevel" --no-link --print-out-paths)
    echo "$path" > "$SYSTEM_PATH_CACHE"
    success "System closure built and cached: $path"
}

cmd_create() {
    local repo=""
    local branch=""
    local slug=""

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --slug)
                slug="$2"
                shift 2
                ;;
            *)
                if [[ -z "$repo" ]]; then
                    repo="$1"
                elif [[ -z "$branch" ]]; then
                    branch="$1"
                else
                    fatal "Unknown argument: $1"
                fi
                shift
                ;;
        esac
    done

    if [[ -z "$repo" ]] || [[ -z "$branch" ]]; then
        fatal "Usage: preview create <owner/repo> <branch> [--slug <slug>]"
    fi

    ensure_root
    ensure_dirs
    load_secrets

    # Generate slug from repo+branch if not provided
    if [[ -z "$slug" ]]; then
        local repo_name="${repo##*/}"
        local safe_branch="${branch//\//-}"
        slug="${repo_name}-${safe_branch}"
    fi

    # Sanitize slug (lowercase, alphanumeric + hyphens only)
    slug=$(echo "$slug" | tr '[:upper:]' '[:lower:]' | sed 's/[^a-z0-9-]/-/g' | sed 's/--*/-/g' | sed 's/^-//' | sed 's/-$//')

    if [[ -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' already exists. Use 'preview destroy $slug' first."
    fi

    local domain="${PREVIEW_DOMAIN:-preview.example.com}"
    local github_token="${GITHUB_TOKEN:-}"

    if [[ -z "$github_token" ]]; then
        fatal "GITHUB_TOKEN not set. Check $SECRETS_FILE"
    fi

    # Get pre-built system path
    local system_path
    system_path=$(get_system_path)

    # Allocate IP
    local slot
    slot=$(next_slot)
    read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"

    info "Creating preview '$slug' (host=$host_ip, container=$local_ip)..."

    # Create PostgreSQL database
    local db_pass
    db_pass=$(create_db "$slug")
    local db_name="preview_${slug//-/_}"
    local db_user="preview_${slug//-/_}"

    # Create the container
    nixos-container create "$slug" \
        --system-path "$system_path" \
        --host-address "$host_ip" \
        --local-address "$local_ip"

    # Write /etc/preview.env into the container filesystem
    local container_root="/var/lib/nixos-containers/${slug}"
    local env_file="${container_root}/etc/preview.env"
    mkdir -p "$(dirname "$env_file")"

    local clone_url="https://x-access-token:${github_token}@github.com/${repo}.git"
    local preview_url="https://${slug}.${domain}"

    cat > "$env_file" <<EOF
PREVIEW_REPO_URL=${clone_url}
PREVIEW_BRANCH=${branch}
DATABASE_URL=postgresql://${db_user}:${db_pass}@${host_ip}:5432/${db_name}
PORT=3000
NODE_ENV=production
APP_URL=${preview_url}
NEXT_PUBLIC_APP_URL=${preview_url}
EOF
    chmod 644 "$env_file"

    # Track the preview
    echo "${slot} ${host_ip} ${local_ip} ${repo} ${branch}" > "$PREVIEW_DIR/$slug"
    bump_slot

    # Start the container
    nixos-container start "$slug"

    # Write Caddy route
    write_caddy_route "$slug" "$local_ip"

    success "Preview '$slug' created and starting."
    echo ""
    echo -e "  ${BOLD}URL:${NC}           ${preview_url}"
    echo -e "  ${BOLD}Container IP:${NC}  $local_ip"
    echo -e "  ${BOLD}Repo:${NC}          $repo"
    echo -e "  ${BOLD}Branch:${NC}        $branch"
    echo ""
    echo -e "  ${CYAN}The app is building. Check progress with: preview logs $slug --follow${NC}"
    echo ""
}

cmd_destroy() {
    local slug="${1:-}"
    if [[ -z "$slug" ]]; then
        fatal "Usage: preview destroy <slug>"
    fi

    ensure_root

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    info "Destroying preview '$slug'..."

    # Remove Caddy route
    remove_caddy_route "$slug"

    # Stop and destroy container
    nixos-container stop "$slug" 2>/dev/null || true
    nixos-container destroy "$slug"

    # Drop PostgreSQL database
    drop_db "$slug"

    # Remove tracking file
    rm -f "$PREVIEW_DIR/$slug"

    success "Preview '$slug' destroyed."
}

cmd_update() {
    local slug="${1:-}"
    if [[ -z "$slug" ]]; then
        fatal "Usage: preview update <slug>"
    fi

    ensure_root

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    read -r _slot _host_ip local_ip _repo _branch < "$PREVIEW_DIR/$slug"

    info "Updating preview '$slug' (pulling latest code and rebuilding)..."

    # Restart setup-preview (triggers git fetch + rebuild) then restart the app
    ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@"$local_ip" \
        "systemctl restart setup-preview && systemctl restart preview-app"

    success "Preview '$slug' is rebuilding. Check progress with: preview logs $slug --follow"
}

cmd_list() {
    ensure_dirs
    load_secrets

    local domain="${PREVIEW_DOMAIN:-preview.example.com}"
    local found=0
    echo -e "${BOLD}SLUG                STATUS          BRANCH                          URL${NC}"

    for f in "$PREVIEW_DIR"/*; do
        [[ -f "$f" ]] || continue
        local name
        name=$(basename "$f")
        [[ "$name" == .* ]] && continue

        found=1
        read -r _slot _host_ip _local_ip repo branch < "$f"

        local status
        if nixos-container status "$name" 2>/dev/null | grep -q "running"; then
            status="${GREEN}running${NC}"
        else
            status="${YELLOW}stopped${NC}"
        fi

        local url="https://${name}.${domain}"
        printf "%-19s %-23b %-31s %s\n" "$name" "$status" "$branch" "$url"
    done

    if [[ $found -eq 0 ]]; then
        echo -e "  ${CYAN}(no previews)${NC}"
    fi
}

cmd_logs() {
    local slug="${1:-}"
    if [[ -z "$slug" ]]; then
        fatal "Usage: preview logs <slug> [--follow]"
    fi
    shift

    local follow_flag=""
    if [[ "${1:-}" == "--follow" ]] || [[ "${1:-}" == "-f" ]]; then
        follow_flag="-f"
    fi

    ensure_root

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    read -r _slot _host_ip local_ip _repo _branch < "$PREVIEW_DIR/$slug"

    ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null root@"$local_ip" \
        "journalctl -u setup-preview -u preview-app --no-pager $follow_flag"
}

cmd_help() {
    echo -e "${BOLD}Usage:${NC} preview <command> [args]"
    echo ""
    echo -e "${BOLD}Commands:${NC}"
    echo "  create <owner/repo> <branch> [--slug <slug>]   Deploy a branch as a preview"
    echo "  destroy <slug>                                  Remove a preview deployment"
    echo "  update <slug>                                   Pull latest code and rebuild"
    echo "  list                                            List all preview deployments"
    echo "  logs <slug> [--follow]                          View preview build/app logs"
    echo "  build                                           Pre-build the preview system closure"
    echo "  help                                            Show this help"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo "  preview build"
    echo "  preview create myorg/myapp feature-branch"
    echo "  preview create myorg/myapp feature-branch --slug myapp-pr-42"
    echo "  preview logs myapp-pr-42 --follow"
    echo "  preview update myapp-pr-42"
    echo "  preview destroy myapp-pr-42"
}

# -- Main ---------------------------------------------------------------------
command="${1:-help}"
shift || true

case "$command" in
    create)  cmd_create "$@" ;;
    destroy) cmd_destroy "$@" ;;
    update)  cmd_update "$@" ;;
    list)    cmd_list ;;
    logs)    cmd_logs "$@" ;;
    build)   cmd_build ;;
    help|--help|-h) cmd_help ;;
    *)       fatal "Unknown command: $command. Run 'preview help' for usage." ;;
esac
