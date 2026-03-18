# =============================================================================
# Preview deployment lifecycle manager
# Deploys GitHub PR branches into nspawn containers as running web apps.
#
# Each repo self-describes its deployment via a preview-config.nix at its root.
# tekton fetches that file, builds a NixOS container closure from it, and runs
# the container.  No repo-specific knowledge lives in this script.
# =============================================================================

AGENT_DIR="/var/lib/claude-agents"
PREVIEW_DIR="/var/lib/preview-deploys"
CADDY_DIR="/etc/caddy/previews"
FLAKE_DIR="/etc/nixos"
SECRETS_FILE="/var/secrets/preview.env"
SUBNET="10.100.0"
CONFIG_CACHE_DIR="$PREVIEW_DIR/.config-cache"
CLOSURE_CACHE_DIR="$PREVIEW_DIR/.closure-cache"

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
warn()    { echo -e "${YELLOW}[WARN]${NC} $*"; }
error()   { echo -e "${RED}[ERROR]${NC} $*" >&2; }
fatal()   { error "$@"; exit 1; }

ensure_root() {
    if [[ $EUID -ne 0 ]]; then
        fatal "This command must be run as root."
    fi
}

ensure_dirs() {
    mkdir -p "$PREVIEW_DIR" "$CADDY_DIR" "$AGENT_DIR" "$CONFIG_CACHE_DIR" "$CLOSURE_CACHE_DIR"
}

# -- IP allocation (shared with agents) --------------------------------------
# Uses flock for atomic read-and-increment to prevent concurrent creates
# from being assigned the same slot.

claim_slot() {
    local slot_file="$AGENT_DIR/.next_slot"
    flock "$slot_file" bash -c '
        slot_file="'"$slot_file"'"
        if [[ -f "$slot_file" ]]; then
            slot=$(<"$slot_file")
        else
            slot=1
        fi
        echo $(( slot + 1 )) > "$slot_file"
        echo "$slot"
    '
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

# -- GitHub helpers -----------------------------------------------------------

get_github_token() {
    local webhook_port="${WEBHOOK_PORT:-3100}"
    local token_response
    if token_response=$(curl -sf "http://127.0.0.1:${webhook_port}/internal/token" 2>/dev/null); then
        local auth_mode
        auth_mode=$(echo "$token_response" | jq -r '.mode')
        info "Using token from webhook (auth mode: $auth_mode)" >&2
        echo "$token_response" | jq -r '.token'
        return
    fi
    if [[ -n "${GITHUB_TOKEN:-}" ]]; then
        echo "$GITHUB_TOKEN"
        return
    fi
    fatal "No GitHub token available. Check $SECRETS_FILE or ensure the webhook is running."
}

get_commit_sha() {
    local repo="$1" branch="$2" token="$3"
    local sha
    sha=$(curl -sf \
        -H "Authorization: Bearer ${token}" \
        -H "Accept: application/vnd.github+json" \
        "https://api.github.com/repos/${repo}/commits/${branch}" | jq -r '.sha')
    if [[ -z "$sha" ]] || [[ "$sha" == "null" ]]; then
        fatal "Could not get commit SHA for ${repo}@${branch}"
    fi
    echo "$sha"
}

# Fetches preview-config.nix from GitHub, cached by commit SHA.
get_or_fetch_config() {
    local repo="$1" branch="$2" commit_sha="$3" token="$4"
    local cached_config="$CONFIG_CACHE_DIR/${commit_sha}.nix"

    if [[ -f "$cached_config" ]]; then
        info "Using cached config (${commit_sha:0:8})" >&2
        echo "$cached_config"
        return
    fi

    info "Fetching preview-config.nix from ${repo}@${branch} (${commit_sha:0:8})..." >&2
    if ! curl -sf \
        -H "Authorization: Bearer ${token}" \
        "https://raw.githubusercontent.com/${repo}/${commit_sha}/preview-config.nix" \
        -o "$cached_config"; then
        rm -f "$cached_config"
        fatal "preview-config.nix not found in ${repo}@${branch}. Add it to your repo root."
    fi

    echo "$cached_config"
}

# -- Nix closure building -----------------------------------------------------

# Builds toplevel + previewMeta for a repo's preview-config.nix.
# Results are cached by commit SHA.  Both builds run in parallel.
build_preview_closure() {
    local config_file="$1" commit_sha="$2" admin_ssh_key="$3"

    # Validate SSH key doesn't contain characters that would break the Nix string literal
    if [[ "$admin_ssh_key" =~ [\$\`\\] ]]; then
        fatal "ADMIN_SSH_KEY contains unsafe characters (\$, \`, \\). Check $SECRETS_FILE."
    fi

    local toplevel_cache="$CLOSURE_CACHE_DIR/${commit_sha}.toplevel"
    local meta_cache="$CLOSURE_CACHE_DIR/${commit_sha}.meta-path"

    local need_toplevel=1 need_meta=1
    if [[ -f "$toplevel_cache" ]] && [[ -d "$(cat "$toplevel_cache" 2>/dev/null)" ]]; then
        info "Using cached closure (${commit_sha:0:8})" >&2
        need_toplevel=0
    fi
    if [[ -f "$meta_cache" ]] && [[ -f "$(cat "$meta_cache" 2>/dev/null)" ]]; then
        need_meta=0
    fi
    if [[ $need_toplevel -eq 0 ]] && [[ $need_meta -eq 0 ]]; then
        return
    fi

    # Both builds share the same nix expression structure; only the last `in` clause differs.
    # Use a heredoc so variable expansion is clean and the nix strings are unambiguous.
    local nix_modules
    nix_modules=$(cat <<NIXEOF
let
  flake    = builtins.getFlake "path:${FLAKE_DIR}";
  nixpkgs  = flake.inputs.nixpkgs;
  pkgs     = import nixpkgs { system = "x86_64-linux"; config.allowUnfree = true; };
  nixosCfg = nixpkgs.lib.nixosSystem {
    system  = "x86_64-linux";
    modules = [
      { nixpkgs.pkgs = pkgs; }
      (import ${config_file})
      { users.users.root.openssh.authorizedKeys.keys = [ "${admin_ssh_key}" ]; }
    ];
  };
in
NIXEOF
)

    # Run both nix builds in parallel; track PIDs so we can check individual exit codes
    local toplevel_out meta_out
    local toplevel_tmp meta_tmp toplevel_err meta_err
    toplevel_tmp=$(mktemp)
    meta_tmp=$(mktemp)
    toplevel_err=$(mktemp)
    meta_err=$(mktemp)
    local toplevel_pid=-1 meta_pid=-1

    if [[ $need_toplevel -eq 1 ]]; then
        info "Building container closure (${commit_sha:0:8})..." >&2
        nix build --impure --no-link --print-out-paths --expr \
            "${nix_modules} nixosCfg.config.system.build.toplevel" \
            > "$toplevel_tmp" 2>"$toplevel_err" &
        toplevel_pid=$!
    fi

    if [[ $need_meta -eq 1 ]]; then
        nix build --impure --no-link --print-out-paths --expr \
            "${nix_modules} nixosCfg.config.system.build.previewMeta" \
            > "$meta_tmp" 2>"$meta_err" &
        meta_pid=$!
    fi

    local toplevel_rc=0 meta_rc=0
    if [[ $toplevel_pid -ne -1 ]]; then
        wait "$toplevel_pid" || toplevel_rc=$?
    fi
    if [[ $meta_pid -ne -1 ]]; then
        wait "$meta_pid" || meta_rc=$?
    fi

    if [[ $need_toplevel -eq 1 ]] && [[ $toplevel_rc -ne 0 || ! -s "$toplevel_tmp" ]]; then
        cat "$toplevel_err" >&2
        rm -f "$toplevel_tmp" "$meta_tmp" "$toplevel_err" "$meta_err"
        fatal "nix build failed for toplevel (${commit_sha:0:8})"
    fi

    if [[ $need_meta -eq 1 ]] && [[ $meta_rc -ne 0 || ! -s "$meta_tmp" ]]; then
        cat "$meta_err" >&2
        rm -f "$toplevel_tmp" "$meta_tmp" "$toplevel_err" "$meta_err"
        fatal "nix build failed for previewMeta (${commit_sha:0:8})"
    fi

    if [[ $need_toplevel -eq 1 ]]; then
        toplevel_out=$(cat "$toplevel_tmp")
        echo "$toplevel_out" > "$toplevel_cache"
        success "Container closure built: ${toplevel_out}" >&2
    fi

    if [[ $need_meta -eq 1 ]]; then
        meta_out=$(cat "$meta_tmp")
        echo "$meta_out" > "$meta_cache"
    fi

    rm -f "$toplevel_tmp" "$meta_tmp" "$toplevel_err" "$meta_err"
}

# -- Caddy route management ---------------------------------------------------

# Writes Caddy config blocks for a route array from the meta JSON.
# Routes are sorted by path length descending (most specific first, "/" last).
generate_caddy_route_blocks() {
    local container_ip="$1"
    local meta_file="$2"
    local routes_sel="$3"

    while IFS= read -r route; do
        local path port strip_prefix
        path=$(echo "$route" | jq -r '.path')
        port=$(echo "$route" | jq -r '.port')
        strip_prefix=$(echo "$route" | jq -r '.stripPrefix // false')

        if [[ "$path" == "/" ]]; then
            # Catch-all: wrapped in handle { } so it participates in Caddy's
            # mutual-exclusivity group with the other handle blocks above it.
            echo "    handle {"
            echo "        reverse_proxy ${container_ip}:${port}"
            echo "    }"
        elif [[ "$strip_prefix" == "true" ]]; then
            # Strip-prefix routing (e.g. /admin/*): redir bare path + handle_path
            local base_path="${path%/\*}"
            echo "    redir ${base_path} ${base_path}/"
            echo "    handle_path ${path} {"
            echo "        reverse_proxy ${container_ip}:${port}"
            echo "    }"
        else
            echo "    handle ${path} {"
            echo "        reverse_proxy ${container_ip}:${port}"
            echo "    }"
        fi
    done < <(jq -c "${routes_sel} | sort_by([(.path | length), .path]) | reverse | .[]" "$meta_file")
}

write_caddy_config() {
    local slug="$1"
    local container_ip="$2"
    local domain="$3"
    local meta_file="$4"

    {
        echo "${slug}.${domain} {"
        echo "    import cloudflare_tls"
        generate_caddy_route_blocks "$container_ip" "$meta_file" ".routes"
        echo "}"

        # Extra host blocks (e.g. landing subdomains)
        local n_extra
        n_extra=$(jq '.extraHosts | length' "$meta_file")
        for ((i=0; i<n_extra; i++)); do
            local prefix
            prefix=$(jq -r ".extraHosts[${i}].prefix" "$meta_file")
            echo ""
            echo "${prefix}-${slug}.${domain} {"
            echo "    import cloudflare_tls"
            generate_caddy_route_blocks "$container_ip" "$meta_file" ".extraHosts[${i}].routes"
            echo "}"
        done
    } > "${CADDY_DIR}/${slug}.caddy"

    systemctl reload caddy
}

remove_caddy_config() {
    local slug="$1"
    rm -f "${CADDY_DIR}/${slug}.caddy"
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

# -- Meta helpers -------------------------------------------------------------

get_meta_file() {
    local slug="$1"
    echo "$PREVIEW_DIR/${slug}.meta"
}

# Reads setup/app service names from .meta file, with legacy .type fallback.
read_service_names() {
    local slug="$1"
    local meta_file="$PREVIEW_DIR/${slug}.meta"

    if [[ -f "$meta_file" ]]; then
        SETUP_SERVICE=$(jq -r '.setupService' "$meta_file")
        readarray -t APP_SERVICES < <(jq -r '.appServices[]' "$meta_file")
    else
        # Legacy fallback for previews created before the meta-based architecture.
        # All new previews use preview-config.nix and produce a .meta file.
        SETUP_SERVICE="setup-preview"
        APP_SERVICES=("preview-app")
    fi
}

# -- Commands -----------------------------------------------------------------

cmd_build() {
    local repo="" branch=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            *)
                if [[ -z "$repo" ]]; then repo="$1"
                elif [[ -z "$branch" ]]; then branch="$1"
                else fatal "Unknown argument: $1"
                fi
                shift
                ;;
        esac
    done

    if [[ -z "$repo" ]] || [[ -z "$branch" ]]; then
        fatal "Usage: preview build <owner/repo> <branch>"
    fi

    ensure_root
    ensure_dirs
    load_secrets

    local github_token
    github_token=$(get_github_token)

    local commit_sha
    commit_sha=$(get_commit_sha "$repo" "$branch" "$github_token")
    info "Building from commit ${commit_sha:0:8}"

    local config_file
    config_file=$(get_or_fetch_config "$repo" "$branch" "$commit_sha" "$github_token")

    local admin_ssh_key="${ADMIN_SSH_KEY:-}"
    if [[ -z "$admin_ssh_key" ]]; then
        fatal "ADMIN_SSH_KEY not set in $SECRETS_FILE"
    fi

    build_preview_closure "$config_file" "$commit_sha" "$admin_ssh_key"
    success "Preview closure for ${repo}@${branch} (${commit_sha:0:8}) built and cached."
}

cmd_create() {
    local repo="" branch="" slug=""

    while [[ $# -gt 0 ]]; do
        case "$1" in
            --slug) slug="$2"; shift 2 ;;
            *)
                if [[ -z "$repo" ]]; then repo="$1"
                elif [[ -z "$branch" ]]; then branch="$1"
                else fatal "Unknown argument: $1"
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
    local admin_ssh_key="${ADMIN_SSH_KEY:-}"
    if [[ -z "$admin_ssh_key" ]]; then
        fatal "ADMIN_SSH_KEY not set in $SECRETS_FILE"
    fi

    # Get GitHub token
    local github_token
    github_token=$(get_github_token)

    # Get commit SHA (cache key for this deploy)
    local commit_sha
    commit_sha=$(get_commit_sha "$repo" "$branch" "$github_token")
    info "Deploying commit ${commit_sha:0:8} of ${repo}@${branch}"

    # Fetch config (cached by commit SHA)
    local config_file
    config_file=$(get_or_fetch_config "$repo" "$branch" "$commit_sha" "$github_token")

    # Build closure (two parallel nix builds: toplevel + previewMeta)
    build_preview_closure "$config_file" "$commit_sha" "$admin_ssh_key"

    # Read meta from nix store and copy to preview dir
    local meta_store_path
    meta_store_path=$(cat "$CLOSURE_CACHE_DIR/${commit_sha}.meta-path")
    local meta_file="$PREVIEW_DIR/${slug}.meta"
    cp "$meta_store_path" "$meta_file"

    # Validate required meta fields
    for field in setupService appServices database routes; do
        if ! jq -e ".${field}" "$meta_file" >/dev/null 2>&1; then
            fatal "preview-config.nix is missing required meta field: ${field}"
        fi
    done

    # Get the built system path
    local system_path
    system_path=$(cat "$CLOSURE_CACHE_DIR/${commit_sha}.toplevel")

    # Allocate IP (atomic to prevent concurrent creates getting the same slot)
    local slot
    slot=$(claim_slot)
    read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"

    info "Creating preview '$slug' (host=$host_ip, container=$local_ip)..."

    # Create PostgreSQL database on the host if requested by meta
    local db_pass="" db_name="" db_user=""
    local db_mode
    db_mode=$(jq -r '.database' "$meta_file")
    if [[ "$db_mode" == "host" ]]; then
        db_name="preview_${slug//-/_}"
        db_user="preview_${slug//-/_}"
        db_pass=$(create_db "$slug")
    fi

    # Guard against concurrent creates that raced past the earlier file-existence check
    if nixos-container status "$slug" &>/dev/null; then
        fatal "Preview '$slug' was already created by a concurrent request. Aborting."
    fi

    # Create the container
    nixos-container create "$slug" \
        --system-path "$system_path" \
        --host-address "$host_ip" \
        --local-address "$local_ip"


    # Write container environment files
    local container_root="/var/lib/nixos-containers/${slug}"
    mkdir -p "${container_root}/etc"

    local preview_host="${slug}.${domain}"
    local preview_url="https://${preview_host}"

    # /etc/preview-token — readable by preview user (604); contains the GitHub token
    # for git operations inside the container.  The setup service runs as the 'preview'
    # user so it needs read access.  644 is also acceptable; the container is isolated
    # and the token expires with the GitHub App session anyway.
    local token_file="${container_root}/etc/preview-token"
    echo "$github_token" > "$token_file"
    chmod 604 "$token_file"

    # /etc/preview.env — readable by preview user (644); safe values only (no token).
    local env_file="${container_root}/etc/preview.env"
    {
        echo "PREVIEW_REPO_URL='https://github.com/${repo}.git'"
        echo "PREVIEW_BRANCH='${branch}'"
        echo "PREVIEW_HOST='${preview_host}'"
        echo "PREVIEW_URL='${preview_url}'"

        if [[ "$db_mode" == "host" ]]; then
            echo "DATABASE_URL='postgresql://${db_user}:${db_pass}@${host_ip}:5432/${db_name}'"
        fi

        # Forward host secrets declared by the repo's preview-config.nix
        local n_secrets
        n_secrets=$(jq '.hostSecrets | length' "$meta_file")
        if [[ $n_secrets -gt 0 ]]; then
            while IFS= read -r secret_key; do
                local secret_val
                secret_val=$(grep "^${secret_key}=" "$SECRETS_FILE" | head -1 | cut -d= -f2- || true)
                if [[ -n "$secret_val" ]]; then
                    echo "${secret_key}=${secret_val}"
                else
                    warn "hostSecret ${secret_key} not found in $SECRETS_FILE — writing empty value" >&2
                    echo "${secret_key}="
                fi
            done < <(jq -r '.hostSecrets[]' "$meta_file")
        fi

        # Append secrets from the dashboard DB (admin panel).
        # The dashboard runs on port 3200 and has a localhost-only endpoint.
        local db_secrets
        db_secrets=$(curl -sf "http://127.0.0.1:3200/internal/secrets/${repo}" 2>/dev/null || true)
        if [[ -n "$db_secrets" ]]; then
            echo "$db_secrets"
        fi
    } > "$env_file"
    chmod 644 "$env_file"

    # Track the preview
    echo "${slot} ${host_ip} ${local_ip} ${repo} ${branch}" > "$PREVIEW_DIR/$slug"
    echo "$commit_sha" > "$PREVIEW_DIR/${slug}.sha"

    # Start the container
    nixos-container start "$slug"

    # Kick off setup and app services in background
    local setup_service
    setup_service=$(jq -r '.setupService' "$meta_file")
    local -a app_services
    readarray -t app_services < <(jq -r '.appServices[]' "$meta_file")
    nixos-container run "$slug" -- systemctl start "$setup_service" "${app_services[@]}" &

    # Write Caddy config from meta routes
    write_caddy_config "$slug" "$local_ip" "$domain" "$meta_file"

    success "Preview '$slug' created and starting."
    echo ""
    echo -e "  ${BOLD}URL:${NC}           ${preview_url}"

    # Print extra host URLs
    local n_extra
    n_extra=$(jq '.extraHosts | length' "$meta_file")
    for ((i=0; i<n_extra; i++)); do
        local prefix
        prefix=$(jq -r ".extraHosts[${i}].prefix" "$meta_file")
        echo -e "  ${BOLD}URL:${NC}           https://${prefix}-${slug}.${domain}"
    done

    echo -e "  ${BOLD}Container IP:${NC}  $local_ip"
    echo -e "  ${BOLD}Repo:${NC}          $repo"
    echo -e "  ${BOLD}Branch:${NC}        $branch"
    echo -e "  ${BOLD}Commit:${NC}        ${commit_sha:0:8}"
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

    # Remove Caddy config
    remove_caddy_config "$slug"

    # Stop and destroy container
    nixos-container stop "$slug" 2>/dev/null || true
    nixos-container destroy "$slug"

    # Drop PostgreSQL database if it was on the host
    local meta_file="$PREVIEW_DIR/${slug}.meta"
    local db_mode="container"
    if [[ -f "$meta_file" ]]; then
        db_mode=$(jq -r '.database' "$meta_file")
    else
        # Legacy fallback: previews before meta-based architecture used host DB
        db_mode="host"
    fi
    if [[ "$db_mode" == "host" ]]; then
        drop_db "$slug"
    fi

    # Remove tracking files (including legacy .type)
    rm -f "$PREVIEW_DIR/$slug" \
          "$PREVIEW_DIR/${slug}.meta" \
          "$PREVIEW_DIR/${slug}.sha" \
          "$PREVIEW_DIR/${slug}.type"

    success "Preview '$slug' destroyed."
}

cmd_update() {
    local slug="${1:-}"
    if [[ -z "$slug" ]]; then
        fatal "Usage: preview update <slug>"
    fi

    ensure_root
    load_secrets

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    read -r _slot _host_ip _local_ip _repo _branch < "$PREVIEW_DIR/$slug"

    # Resolve service names from meta (or legacy .type file)
    local SETUP_SERVICE
    local -a APP_SERVICES
    read_service_names "$slug"

    info "Updating preview '$slug' (pulling latest code and rebuilding)..."

    # Refresh the GitHub token so git can pull latest changes
    local github_token
    github_token=$(get_github_token)

    if [[ -n "$github_token" ]]; then
        local container_root="/var/lib/nixos-containers/${slug}"
        # Overwrite /etc/preview-token with the fresh token (setup scripts read this for git auth)
        local token_file="${container_root}/etc/preview-token"
        if [[ -f "$token_file" ]]; then
            echo "$github_token" > "$token_file"
        fi
        # Also refresh any already-cloned git remote URL so ongoing fetches work immediately
        local git_config="${container_root}/home/preview/app/.git/config"
        if [[ -f "$git_config" ]]; then
            sed -i "s|x-access-token:[^@]*|x-access-token:${github_token}|" "$git_config"
        fi
    fi

    # Signal the setup service to do a full rebuild
    nixos-container run "$slug" -- su -s /bin/sh preview -c "touch /tmp/force-rebuild"

    nixos-container run "$slug" -- systemctl restart "$SETUP_SERVICE" "${APP_SERVICES[@]}"

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
        # Skip hidden files and metadata files
        [[ "$name" == .* ]]    && continue
        [[ "$name" == *.type ]] && continue
        [[ "$name" == *.meta ]] && continue
        [[ "$name" == *.sha ]]  && continue

        found=1
        read -r _slot _host_ip _local_ip _repo branch < "$f"

        local status
        if nixos-container status "$name" 2>/dev/null | grep -q "up"; then
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

    local SETUP_SERVICE
    local -a APP_SERVICES
    read_service_names "$slug"

    local -a units=("-u" "$SETUP_SERVICE")
    for svc in "${APP_SERVICES[@]}"; do
        units+=("-u" "$svc")
    done

    nixos-container run "$slug" -- journalctl "${units[@]}" --no-pager "${follow_args[@]}"
}

cmd_help() {
    echo -e "${BOLD}Usage:${NC} preview <command> [args]"
    echo ""
    echo -e "${BOLD}Commands:${NC}"
    echo "  create <owner/repo> <branch> [--slug <slug>]    Deploy a branch as a preview"
    echo "  destroy <slug>                                   Remove a preview deployment"
    echo "  update <slug>                                    Pull latest code and rebuild"
    echo "  list                                             List all preview deployments"
    echo "  logs <slug> [--follow]                           View preview build/app logs"
    echo "  build <owner/repo> <branch>                      Pre-build the preview closure"
    echo "  help                                             Show this help"
    echo ""
    echo -e "${BOLD}How it works:${NC}"
    echo "  Each repo ships a preview-config.nix at its root. tekton fetches it from"
    echo "  GitHub, builds a NixOS container closure from it (cached by commit SHA),"
    echo "  and runs the container. No repo-specific knowledge lives in tekton."
    echo ""
    echo -e "${BOLD}Examples:${NC}"
    echo "  preview build myorg/myapp feature-branch"
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
    build)   cmd_build "$@" ;;
    help|--help|-h) cmd_help ;;
    *)       fatal "Unknown command: $command. Run 'preview help' for usage." ;;
esac
