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
VERTEX_SYSTEM_PATH_CACHE="$PREVIEW_DIR/.vertex-system-path"

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

get_vertex_system_path() {
    if [[ -f "$VERTEX_SYSTEM_PATH_CACHE" ]]; then
        local cached
        cached=$(cat "$VERTEX_SYSTEM_PATH_CACHE")
        if [[ -d "$cached" ]]; then
            echo "$cached"
            return
        fi
    fi
    info "Building vertex preview system closure (first time only)..." >&2
    local path
    path=$(nix build "${FLAKE_DIR}#nixosConfigurations.vertex-preview.config.system.build.toplevel" --no-link --print-out-paths)
    echo "$path" > "$VERTEX_SYSTEM_PATH_CACHE"
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

    info "Creating PostgreSQL database '$db_name'..." >&2

    # Create user and database (suppress stdout to avoid polluting the password return)
    sudo -u postgres psql -c "CREATE USER ${db_user} WITH PASSWORD '${db_pass}';" &>/dev/null || \
        sudo -u postgres psql -c "ALTER USER ${db_user} WITH PASSWORD '${db_pass}';" &>/dev/null
    sudo -u postgres psql -c "CREATE DATABASE ${db_name} OWNER ${db_user};" &>/dev/null || true

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
${slug}.${domain} {
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

write_vertex_caddy_route() {
    local slug="$1"
    local container_ip="$2"
    local domain="${PREVIEW_DOMAIN:-preview.example.com}"

    cat > "${CADDY_DIR}/${slug}.caddy" <<EOF
${slug}.${domain} {
    handle /api/* {
        reverse_proxy ${container_ip}:4000
    }
    handle /admin/* {
        reverse_proxy ${container_ip}:3000
    }
    reverse_proxy ${container_ip}:3001
}
EOF

    systemctl reload caddy
}

# -- Secret generation helpers ------------------------------------------------

generate_secret_hex() {
    local length="${1:-64}"
    head -c $(( length / 2 + 1 )) /dev/urandom | od -An -tx1 | tr -d ' \n' | head -c "$length"
}

generate_secret_base64() {
    local bytes="${1:-32}"
    head -c "$bytes" /dev/urandom | base64 | tr -d '\n'
}

# -- Preview type detection ---------------------------------------------------

get_preview_type() {
    local slug="$1"
    local type_file="$PREVIEW_DIR/${slug}.type"
    if [[ -f "$type_file" ]]; then
        cat "$type_file"
    else
        echo "node"
    fi
}

# -- Commands -----------------------------------------------------------------

cmd_build() {
    local type="node"

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --type) type="$2"; shift 2 ;;
            *) fatal "Unknown argument: $1" ;;
        esac
    done

    ensure_root
    ensure_dirs

    case "$type" in
        node)
            info "Building preview system closure..."
            local path
            path=$(nix build "${FLAKE_DIR}#nixosConfigurations.preview.config.system.build.toplevel" --no-link --print-out-paths)
            echo "$path" > "$SYSTEM_PATH_CACHE"
            success "System closure built and cached: $path"
            ;;
        vertex)
            info "Building vertex preview system closure..."
            local path
            path=$(nix build "${FLAKE_DIR}#nixosConfigurations.vertex-preview.config.system.build.toplevel" --no-link --print-out-paths)
            echo "$path" > "$VERTEX_SYSTEM_PATH_CACHE"
            success "Vertex system closure built and cached: $path"
            ;;
        *)
            fatal "Unknown preview type: $type. Supported: node, vertex"
            ;;
    esac
}

cmd_create() {
    local repo=""
    local branch=""
    local slug=""
    local type="node"

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --slug)
                slug="$2"
                shift 2
                ;;
            --type)
                type="$2"
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
        fatal "Usage: preview create <owner/repo> <branch> [--slug <slug>] [--type <node|vertex>]"
    fi

    if [[ "$type" != "node" ]] && [[ "$type" != "vertex" ]]; then
        fatal "Unknown preview type: $type. Supported: node, vertex"
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

    # Get pre-built system path (type-specific)
    local system_path
    if [[ "$type" == "vertex" ]]; then
        system_path=$(get_vertex_system_path)
    else
        system_path=$(get_system_path)
    fi

    # Allocate IP
    local slot
    slot=$(next_slot)
    read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"

    info "Creating preview '$slug' (type=$type, host=$host_ip, container=$local_ip)..."

    # Create PostgreSQL database (only for node type — vertex runs its own PostgreSQL)
    local db_pass=""
    local db_name="preview_${slug//-/_}"
    local db_user="preview_${slug//-/_}"
    if [[ "$type" != "vertex" ]]; then
        db_pass=$(create_db "$slug")
    fi

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

    if [[ "$type" == "vertex" ]]; then
        # Generate secrets for vertex
        local secret_key_base
        secret_key_base=$(generate_secret_hex 64)
        local jwt_secret
        jwt_secret=$(generate_secret_hex 64)
        local db_encryption_key
        db_encryption_key=$(generate_secret_base64 32)

        # Check for optional credential overrides from host secrets
        local postmark_key="${VERTEX_POSTMARK_API_KEY:-dummy-not-configured}"
        local google_client_id="${VERTEX_GOOGLE_CLIENT_ID:-dummy-not-configured}"
        local google_client_secret="${VERTEX_GOOGLE_CLIENT_SECRET:-dummy-not-configured}"

        cat > "$env_file" <<EOF
PREVIEW_REPO_URL='${clone_url}'
PREVIEW_BRANCH='${branch}'
DATABASE_URL='postgresql://vertex@localhost:5432/vertex'
SECRET_KEY_BASE='${secret_key_base}'
JWT_SECRET='${jwt_secret}'
DATABASE_ENCRYPTION_KEY='${db_encryption_key}'
PHX_HOST='${slug}.${domain}'
PORT=4000
FRONTEND_URL='${preview_url}'
POSTMARK_API_KEY='${postmark_key}'
GOOGLE_CLIENT_ID='${google_client_id}'
GOOGLE_CLIENT_SECRET='${google_client_secret}'
REDIS_URL='redis://localhost:6379'
DEPLOY_ENV='testing'
CORS_ALLOWED_ORIGINS='${preview_url}'
EOF
    else
        cat > "$env_file" <<EOF
PREVIEW_REPO_URL='${clone_url}'
PREVIEW_BRANCH='${branch}'
DATABASE_URL='postgresql://${db_user}:${db_pass}@${host_ip}:5432/${db_name}'
PORT=3000
NODE_ENV=production
APP_URL='${preview_url}'
NEXT_PUBLIC_APP_URL='${preview_url}'
EOF
    fi
    chmod 644 "$env_file"

    # Track the preview (include type in tracking file)
    echo "${slot} ${host_ip} ${local_ip} ${repo} ${branch}" > "$PREVIEW_DIR/$slug"
    echo "$type" > "$PREVIEW_DIR/${slug}.type"
    bump_slot

    # Start the container
    nixos-container start "$slug"

    # Kick off setup and services (type-specific)
    if [[ "$type" == "vertex" ]]; then
        nixos-container run "$slug" -- systemctl start setup-vertex vertex-backend vertex-frontend-admin vertex-frontend-foods &
        write_vertex_caddy_route "$slug" "$local_ip"
    else
        nixos-container run "$slug" -- systemctl start setup-preview preview-app &
        write_caddy_route "$slug" "$local_ip"
    fi

    success "Preview '$slug' created and starting."
    echo ""
    echo -e "  ${BOLD}URL:${NC}           ${preview_url}"
    echo -e "  ${BOLD}Type:${NC}          $type"
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

    # Drop PostgreSQL database (only for node type — vertex DB is destroyed with the container)
    local type
    type=$(get_preview_type "$slug")
    if [[ "$type" != "vertex" ]]; then
        drop_db "$slug"
    fi

    # Remove tracking files
    rm -f "$PREVIEW_DIR/$slug"
    rm -f "$PREVIEW_DIR/${slug}.type"

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
    local type
    type=$(get_preview_type "$slug")

    info "Updating preview '$slug' (type=$type, pulling latest code and rebuilding)..."

    if [[ "$type" == "vertex" ]]; then
        nixos-container run "$slug" -- bash -c \
            "systemctl restart setup-vertex && systemctl restart vertex-backend vertex-frontend-admin vertex-frontend-foods"
    else
        nixos-container run "$slug" -- bash -c \
            "systemctl restart setup-preview && systemctl restart preview-app"
    fi

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
        [[ "$name" == *.type ]] && continue

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

    local -a follow_args=()
    if [[ "${1:-}" == "--follow" ]] || [[ "${1:-}" == "-f" ]]; then
        follow_args=(-f)
    fi

    ensure_root

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    read -r _slot _host_ip local_ip _repo _branch < "$PREVIEW_DIR/$slug"

    local type
    type=$(get_preview_type "$slug")

    local -a units
    if [[ "$type" == "vertex" ]]; then
        units=(-u setup-vertex -u vertex-backend -u vertex-frontend-admin -u vertex-frontend-foods)
    else
        units=(-u setup-preview -u preview-app)
    fi

    nixos-container run "$slug" -- journalctl "${units[@]}" --no-pager "${follow_args[@]}"
}

cmd_help() {
    echo -e "${BOLD}Usage:${NC} preview <command> [args]"
    echo ""
    echo -e "${BOLD}Commands:${NC}"
    echo "  create <owner/repo> <branch> [options]          Deploy a branch as a preview"
    echo "    --slug <slug>                                 Custom slug for the preview URL"
    echo "    --type <node|vertex>                          Preview type (default: node)"
    echo "  destroy <slug>                                  Remove a preview deployment"
    echo "  update <slug>                                   Pull latest code and rebuild"
    echo "  list                                            List all preview deployments"
    echo "  logs <slug> [--follow]                          View preview build/app logs"
    echo "  build [--type <node|vertex>]                    Pre-build the preview system closure"
    echo "  help                                            Show this help"
    echo ""
    echo -e "${BOLD}Preview types:${NC}"
    echo "  node     Node.js app (npm ci, npm build, npm start on port 3000)"
    echo "  vertex   Elixir/Phoenix + React SPA monorepo (backend:4000, frontends:3000/3001)"
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo "  preview build"
    echo "  preview build --type vertex"
    echo "  preview create myorg/myapp feature-branch"
    echo "  preview create myorg/myapp feature-branch --slug myapp-pr-42"
    echo "  preview create lambdaclass/vertex feature-branch --type vertex --slug vtx-pr-42"
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
    build)   cmd_build "$@" ;;
    help|--help|-h) cmd_help ;;
    *)       fatal "Unknown command: $command. Run 'preview help' for usage." ;;
esac
