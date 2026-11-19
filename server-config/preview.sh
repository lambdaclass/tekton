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
SUBNET_PREFIX="10.100"
CONFIG_CACHE_DIR="$PREVIEW_DIR/.config-cache"
CLOSURE_CACHE_DIR="$PREVIEW_DIR/.closure-cache"
# Root-only bare mirrors of preview repos (see "Code sync" below)
REPO_CACHE_DIR="$PREVIEW_DIR/.repo-cache"
CONTAINERS_DIR="/var/lib/nixos-containers"

# -- Load secrets -------------------------------------------------------------
load_secrets() {
    if [[ -f "$SECRETS_FILE" ]]; then
        # A leftover PAT would override the caller's token; refuse instead.
        if grep -q '^[[:space:]]*\(export[[:space:]]\+\)\?GITHUB_TOKEN=' "$SECRETS_FILE"; then
            fatal "$SECRETS_FILE still sets GITHUB_TOKEN. PAT auth has been removed: delete that line and revoke the PAT (see docs/deployment-guide.md)."
        fi
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
    mkdir -p "$PREVIEW_DIR" "$CADDY_DIR" "$AGENT_DIR" "$CONFIG_CACHE_DIR" "$CLOSURE_CACHE_DIR" "$REPO_CACHE_DIR"
    chmod 700 "$REPO_CACHE_DIR"
}

# -- IP allocation (shared with agents) --------------------------------------
# Slots are recycled: we scan tracking files to find used slots, then pick
# the lowest available one.  Uses a /16 subnet (10.100.0.0–10.100.255.255).
# Uses flock for atomic claim to prevent concurrent creates from being
# assigned the same slot.

MAX_SLOT=32512
SLOT_LOCK="$AGENT_DIR/.slot.lock"

used_slots() {
    local dir file slot
    for dir in "$AGENT_DIR" "$PREVIEW_DIR"; do
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

claim_slot() {
    local tracking_file="$1"
    shift
    local extra_fields="$*"
    (
        flock 9
        local slot
        slot=$(next_slot)
        read -r host_ip local_ip <<< "$(slot_to_ips "$slot")"
        if [[ -n "$extra_fields" ]]; then
            echo "${slot} ${host_ip} ${local_ip} ${extra_fields}" > "$tracking_file"
        else
            echo "${slot} ${host_ip} ${local_ip}" > "$tracking_file"
        fi
        echo "$slot"
    ) 9>"$SLOT_LOCK"
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

valid_repo() {
    local repo="$1" part
    [[ "$repo" =~ ^[A-Za-z0-9._-]+/[A-Za-z0-9._-]+$ && "$repo" != *.git ]] || return 1
    for part in "${repo%%/*}" "${repo#*/}"; do
        [[ "$part" != .* ]] || return 1
    done
}

# Branch names go into git refspecs, paths and single-quoted values in
# /etc/preview.env, so only allow names none of those treat specially.
valid_branch() {
    local branch="$1"
    [[ "$branch" =~ ^[A-Za-z0-9._/+#@,=-]+$ ]] || return 1
    case "$branch" in
        [-/.]*|*/|*.|*.lock|*..*|*//*|*@\{*) return 1 ;;
    esac
}

validate_repo_branch() {
    valid_repo "$1" || fatal "Invalid repo name '$1' (expected owner/repo)."
    valid_branch "$2" || fatal "Invalid branch name '$2'."
}

plain_repo_url() {
    echo "https://github.com/$1.git"
}

# Prints a GitHub token that can read <repo>: a contents:read GitHub App
# installation token scoped to <repo>, from the preview webhook, with at least
# min_ttl seconds left. It is only ever used on the host.
# Args: repo [min_ttl_seconds]
get_github_token() {
    local repo="$1" min_ttl="${2:-}"
    local webhook_port="${WEBHOOK_PORT:-3100}"
    local -a query=(--data-urlencode "repo=${repo}")
    if [[ -n "$min_ttl" ]]; then
        query+=(--data-urlencode "min_ttl=${min_ttl}")
    fi
    local response http_code body
    response=$(curl -s -G -w '\n%{http_code}' "${query[@]}" \
        "http://127.0.0.1:${webhook_port}/internal/token" 2>/dev/null) || response=$'\n000'
    http_code="${response##*$'\n'}"
    body="${response%$'\n'*}"
    if [[ "$http_code" == "200" ]]; then
        local expires_at
        expires_at=$(jq -r '.expires_at' <<< "$body")
        info "Using GitHub App token for ${repo} (expires ${expires_at})" >&2
        jq -r '.token' <<< "$body"
        return
    fi
    local reason
    reason=$(jq -r '.error // empty' <<< "$body" 2>/dev/null || true)
    fatal "Could not get a GitHub App token for ${repo} from the preview webhook (HTTP ${http_code}${reason:+: $reason}). Check 'systemctl status preview-webhook' and the GITHUB_APP_* settings in $SECRETS_FILE."
}

get_commit_sha() {
    local repo="$1" branch="$2" token="$3"
    # URL-encode the branch name so slashes (e.g. feature/foo) don't break the API path
    local encoded_branch
    encoded_branch=$(printf '%s' "$branch" | jq -sRr @uri)
    local sha http_code body
    body=$(curl -s -w '\n%{http_code}' \
        -H "Authorization: Bearer ${token}" \
        -H "Accept: application/vnd.github+json" \
        "https://api.github.com/repos/${repo}/commits/${encoded_branch}")
    http_code=$(echo "$body" | tail -1)
    body=$(echo "$body" | sed '$d')
    sha=$(echo "$body" | jq -r '.sha')
    if [[ -z "$sha" ]] || [[ "$sha" == "null" ]]; then
        local api_msg
        api_msg=$(echo "$body" | jq -r '.message // empty' 2>/dev/null)
        fatal "Could not get commit SHA for ${repo}@${branch} (HTTP ${http_code}${api_msg:+: $api_msg})"
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

# -- Per-preview lock ---------------------------------------------------------

valid_slug() {
    [[ "$1" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]]
}

# Serialises create/update/destroy of one preview. The dashboard and the
# webhook can run them at once, and the code sync that finishes last would
# otherwise win even with an older commit. Held until the script exits, on fd
# 7, which every command that enters a container closes (a process left
# inside could otherwise hold the lock forever).
lock_slug() {
    local slug="$1"
    valid_slug "$slug" || fatal "Invalid preview slug '$slug'."
    [[ "${LOCKED_SLUG:-}" == "$slug" ]] && return 0
    mkdir -p "$PREVIEW_DIR/.locks"
    exec 7>"$PREVIEW_DIR/.locks/${slug}.lock"
    if ! flock -n 7; then
        info "Waiting for another operation on preview '$slug' to finish..." >&2
        flock 7
    fi
    LOCKED_SLUG="$slug"
}

# -- Container access ---------------------------------------------------------

# nixos-container run with an empty environment. Its `su` stays in the
# container's PID namespace while the command runs, so whatever this script
# inherited (the dashboard's or webhook's secrets) would be readable there.
# Args: slug -- cmd...
nc_run() {
    env -i PATH="$PATH" nixos-container run "$@" 7>&-
}

# Absolute /nix/store path of a host binary. Containers share the host's
# /nix/store, so the path works inside them whatever the repo's config installs.
store_bin() {
    local bin
    bin=$(command -v "$1") || { error "'$1' not found on the host."; return 1; }
    [[ "$bin" == /nix/store/* ]] || bin=$(readlink -f "$bin")
    if [[ "$bin" != /nix/store/* ]]; then
        error "'$1' does not resolve into /nix/store (got '$bin')."
        return 1
    fi
    echo "$bin"
}

# Runs a bash script as `preview` inside a running container, in /home/preview
# with host git and coreutils on PATH. stdin is passed through. Killed after
# `seconds` so a wedged container can't hang the caller.
# Args: seconds slug script script-args...
run_as_preview() {
    local seconds="$1" slug="$2" script="$3"
    shift 3
    local bash_bin runuser_bin git_bin cat_bin tool_path
    bash_bin=$(store_bin bash) || return 1
    runuser_bin=$(store_bin runuser) || return 1
    git_bin=$(store_bin git) || return 1
    cat_bin=$(store_bin cat) || return 1
    tool_path="$(dirname "$git_bin"):$(dirname "$cat_bin")"
    check_machine_leader "$slug" || return 1
    timeout "$seconds" env -i PATH="$PATH" nixos-container run "$slug" -- \
        "$runuser_bin" -u preview -- \
        "$bash_bin" -c "export PATH='${tool_path}' HOME=/home/preview; cd /home/preview; ${script}" \
        preview-sync "$@" 7>&-
}

# `nixos-container start` can return before activation has created the
# preview user and its home.
wait_for_preview_user() {
    local slug="$1" i
    for i in {1..30}; do
        # A stale registration doesn't clear itself, so don't wait it out.
        check_machine_leader "$slug" || return 1
        # Fails until `preview` exists (runuser) and has a home (cd).
        run_as_preview 30 "$slug" true </dev/null &>/dev/null && return 0
        sleep 1
    done
    error "User 'preview' or its home /home/preview doesn't exist in '$slug' (preview-config.nix must define them)."
    return 1
}

# -- Code sync ----------------------------------------------------------------
# Tekton never hands a GitHub credential to a preview container. (The repo's
# preview-config.nix is still evaluated as root with --impure, so a malicious
# config can read host files during the build; that needs the build sandboxed
# and is tracked separately.) The host fetches each
# branch into a root-only bare mirror with the GitHub App token, which reaches
# git as an HTTP header through GIT_CONFIG_* env vars (never argv, a URL or a
# file). It then streams a git bundle over stdin into a bare repo in the
# container owned by `preview` (~/.tekton-src.git). A git insteadOf rule there
# points the repo's GitHub URL at that bare repo, so the setup service clones
# and fetches PREVIEW_REPO_URL as before and gets the host's commits.
#
# The host never writes this into the container's rootfs, so a symlink planted
# inside the container can't redirect a root write on the host.

mirror_path() {
    echo "$REPO_CACHE_DIR/${1%%/*}/${1#*/}.git"
}

# The mirror ref that tracks <branch>. Branch names are hex-encoded into one
# flat name: under refs/heads/, a previewed (later deleted) `feature` would
# block `feature/x` from ever being fetched.
mirror_branch_ref() {
    echo "refs/tekton-branch/$(printf '%s' "$1" | od -An -tx1 | tr -d ' \n')"
}

# Drops <branch>'s mirror ref once no preview of <repo> tracks it, so the
# mirror doesn't keep deleted branches' history forever. Best effort.
# Args: repo branch
mirror_forget_branch() {
    local repo="$1" branch="$2" mirror slug _slot _host_ip _local_ip r b
    mirror=$(mirror_path "$repo")
    [[ -f "$mirror/HEAD" ]] || return 0
    while IFS= read -r slug; do
        read -r _slot _host_ip _local_ip r b 2>/dev/null < "$PREVIEW_DIR/$slug" || continue
        [[ "$r" == "$repo" && "$b" == "$branch" ]] && return 0
    done < <(list_slugs)
    (
        flock 8 || exit 1
        git --git-dir "$mirror" update-ref -d "$(mirror_branch_ref "$branch")"
    ) 8>"${mirror}.lock" 2>/dev/null || true
}

# Runs host git with the token as an HTTP header for this one process.
# Args: token git-args...
git_with_token() {
    local token="$1" basic
    shift
    basic=$(printf 'x-access-token:%s' "$token" | base64 -w0)
    GIT_TERMINAL_PROMPT=0 \
        GIT_HTTP_LOW_SPEED_LIMIT=1000 \
        GIT_HTTP_LOW_SPEED_TIME=60 \
        GIT_CONFIG_COUNT=1 \
        GIT_CONFIG_KEY_0=http.https://github.com/.extraHeader \
        GIT_CONFIG_VALUE_0="Authorization: Basic ${basic}" \
        timeout 600 git "$@"
}

# Fetches <branch> into the repo's mirror and makes sure <commit_sha> is in it.
# Args: repo branch commit_sha token
mirror_fetch() {
    local repo="$1" branch="$2" commit_sha="$3" token="$4"
    [[ "$commit_sha" =~ ^[0-9a-f]{40}$ ]] || fatal "Invalid commit id '$commit_sha'."
    local mirror url ref
    mirror=$(mirror_path "$repo")
    url=$(plain_repo_url "$repo")
    ref=$(mirror_branch_ref "$branch")
    mkdir -p "$(dirname "$mirror")"

    info "Fetching ${repo}@${branch} into the host mirror..." >&2
    # Each step checks its status: errexit is off when a caller tests this.
    (
        flock 8 || exit 1
        [[ -f "$mirror/HEAD" ]] || git init --quiet --bare "$mirror" || exit 1
        git_with_token "$token" --git-dir "$mirror" fetch --quiet --no-tags -- \
            "$url" "+refs/heads/${branch}:${ref}" || exit 1
        if ! git --git-dir "$mirror" cat-file -e "${commit_sha}^{commit}" 2>/dev/null; then
            # Force-pushed away since we read the branch head: fetch it by id.
            git_with_token "$token" --git-dir "$mirror" fetch --quiet --no-tags -- \
                "$url" "$commit_sha" || exit 1
        fi
    ) 8>"${mirror}.lock" || fatal "Could not fetch ${repo}@${branch} (${commit_sha:0:8}) on the host."
}

# Writes a bundle of <commit_sha> (as <ref>) from the mirror. With base_sha,
# the bundle only holds what base_sha lacks, and fails if base_sha isn't an
# ancestor of commit_sha.
# Args: mirror ref commit_sha bundle [base_sha]
make_bundle() {
    local mirror="$1" ref="$2" commit_sha="$3" bundle="$4" base_sha="${5:-}"
    local -a revs=("$ref")
    (
        flock 8 || exit 1
        if [[ -n "$base_sha" ]]; then
            git --git-dir "$mirror" merge-base --is-ancestor "$base_sha" "$commit_sha" 2>/dev/null || exit 1
            revs+=("^${base_sha}")
        fi
        git --git-dir "$mirror" update-ref "$ref" "$commit_sha" || exit 1
        rc=0
        git --git-dir "$mirror" bundle create --quiet "$bundle" "${revs[@]}" || rc=1
        git --git-dir "$mirror" update-ref -d "$ref" || true
        exit "$rc"
    ) 8>"${mirror}.lock"
}

# Runs inside the container as `preview`, with the bundle on stdin.
# Args: branch commit bundle-ref url url-with-empty-token
# shellcheck disable=SC2016  # expanded by the container-side bash, not here
CONTAINER_SYNC_SCRIPT='
set -euo pipefail
branch=$1 commit=$2 ref=$3 url=$4 empty_token_url=$5
src=$HOME/.tekton-src.git
[ -f "$src/HEAD" ] || git init --quiet --bare "$src"
bundle=$(mktemp "$HOME/.tekton-bundle.XXXXXX")
trap '\''rm -f "$bundle"'\'' EXIT
cat > "$bundle"
# Fails when the bundle is incremental and its base commit is missing here.
git -C "$src" bundle verify --quiet "$bundle" >/dev/null 2>&1
git -C "$src" fetch --quiet --no-tags "$bundle" "+$ref:refs/heads/$branch"
[ "$(git -C "$src" rev-parse "refs/heads/$branch")" = "$commit" ]
git -C "$src" symbolic-ref HEAD "refs/heads/$branch"
# Clones and fetches of the repo read from $src. The second URL is what
# configs written for /etc/preview-token build now that the file is gone.
git config --global --unset-all "url.$src.insteadOf" || true
git config --global --add "url.$src.insteadOf" "$url"
git config --global --add "url.$src.insteadOf" "$empty_token_url"
# Previews from older tekton versions kept a token in the app remote.
if [ -d app/.git ] && git -C app remote get-url origin >/dev/null 2>&1; then
    git -C app remote set-url origin "$url"
fi
echo "Preview code is at ${commit:0:8}."
'

# Puts <commit_sha> of <branch> into the container's ~/.tekton-src.git, where
# the setup service clones or fetches it from. The container must be running
# and mirror_fetch must have brought the commit in. With base_sha (the commit
# the container already has), sends only the new objects, falling back to the
# full history if that fails.
# Runs in a subshell so the EXIT trap removes the host bundle on any failure.
# Args: slug repo branch commit_sha [base_sha]
sync_code() (
    local slug="$1" repo="$2" branch="$3" commit_sha="$4" base_sha="${5:-}"
    local mirror url empty_token_url bundle ref
    mirror=$(mirror_path "$repo")
    url=$(plain_repo_url "$repo")
    empty_token_url="https://x-access-token:@github.com/${repo}.git"
    ref="refs/tekton-preview/${slug}"
    bundle=$(mktemp "$REPO_CACHE_DIR/.bundle.XXXXXX")
    trap 'rm -f "$bundle"' EXIT

    wait_for_preview_user "$slug" || exit 1

    info "Syncing ${commit_sha:0:8} into preview '$slug'..."
    if [[ -n "$base_sha" && "$base_sha" != "$commit_sha" ]]; then
        if make_bundle "$mirror" "$ref" "$commit_sha" "$bundle" "$base_sha" &&
            run_as_preview 600 "$slug" "$CONTAINER_SYNC_SCRIPT" \
                "$branch" "$commit_sha" "$ref" "$url" "$empty_token_url" < "$bundle"; then
            exit 0
        fi
        warn "Incremental sync into '$slug' failed; sending the full history."
    fi
    make_bundle "$mirror" "$ref" "$commit_sha" "$bundle" || {
        error "Could not bundle ${repo}@${commit_sha:0:8} from the host mirror."
        exit 1
    }
    run_as_preview 600 "$slug" "$CONTAINER_SYNC_SCRIPT" \
        "$branch" "$commit_sha" "$ref" "$url" "$empty_token_url" < "$bundle" || {
        error "Could not sync code into preview '$slug'."
        exit 1
    }
)

# -- Token scrub --------------------------------------------------------------
# Previews created by older tekton versions hold a GitHub token in
# /etc/preview-token and in the app's git remote URL.

# Resolves a path under a container root, following symlinks, and fails if it
# doesn't exist or resolves outside the root.
# Args: root relative-path
resolve_in_root() {
    local root="$1" rel="$2" resolved
    resolved=$(realpath -e -P -- "$root/$rel" 2>/dev/null) || return 1
    if [[ "$resolved" != "$root"/* ]]; then
        warn "Refusing ${root}/${rel}: it resolves to ${resolved}, outside the container root." >&2
        return 1
    fi
    echo "$resolved"
}

# Args: slug repo
# Called in conditionals, so it checks every step instead of relying on errexit.
scrub_preview() {
    local slug="$1" repo="$2"
    local containers root
    containers=$(realpath -e -P -- "$CONTAINERS_DIR" 2>/dev/null) || return 0
    root=$(realpath -e -P -- "$containers/$slug" 2>/dev/null) || return 0
    if [[ "$root" != "$containers/"* ]]; then
        warn "Refusing to scrub '$slug': its root resolves to ${root}." >&2
        return 1
    fi
    if ! valid_repo "$repo"; then
        warn "Can't scrub '$slug': invalid repo '$repo'." >&2
        return 1
    fi
    local url
    url=$(plain_repo_url "$repo")

    # Decide from the unit itself: `nixos-container status` says "down" while
    # the container is still starting (or auto-restarting), and its own
    # services already run then. Only a unit that is fully inactive or failed
    # takes the host-side path; anything in between is unknown.
    local state
    state=$(timeout 30 systemctl show -p ActiveState --value "container@${slug}" </dev/null 2>/dev/null) || state=""
    case "$state" in
        active)
            # A running container could swap a path between a host-side check
            # and the write, so the container cleans up after itself.
            check_machine_leader "$slug" || return 1
            timeout 120 env -i PATH="$PATH" nixos-container run "$slug" -- \
                rm -f /etc/preview-token </dev/null 7>&- || return 1
            # shellcheck disable=SC2016  # expanded by the container-side bash
            run_as_preview 120 "$slug" '
                [ -e app/.git ] || exit 0
                git -C app remote get-url origin >/dev/null 2>&1 || exit 0
                git -C app remote set-url origin "$1"' \
                "$url" </dev/null || return 1
            ;;
        inactive|failed)
            # rm doesn't follow a symlink in the last component; the directory
            # is resolved and checked first.
            local etc_dir git_config current
            if etc_dir=$(resolve_in_root "$root" etc); then
                rm -f -- "$etc_dir/preview-token" || return 1
            fi
            [[ -e "$root/home/preview/app/.git" || -L "$root/home/preview/app/.git" ]] || return 0
            git_config=$(resolve_in_root "$root" home/preview/app/.git/config) || return 1
            current=$(git config --file "$git_config" --get remote.origin.url 2>/dev/null || true)
            if [[ -n "$current" && "$current" != "$url" ]]; then
                # --file runs no hooks and reads no other config.
                git config --file "$git_config" remote.origin.url "$url" || return 1
            fi
            ;;
        *)
            warn "Can't scrub '$slug': its container is ${state:-in an unknown state}; try again once it's up or stopped." >&2
            return 1
            ;;
    esac
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

# -- Container environment helpers --------------------------------------------

# Writes /etc/preview.env into the container root. Runs right after
# `nixos-container create`, before the container first boots.
# Args: slug repo branch domain host_ip db_mode db_pass meta_file
write_env_files() {
    local slug="$1" repo="$2" branch="$3" domain="$4" host_ip="$5"
    local db_mode="$6" db_pass="$7" meta_file="$8"

    local container_root="${CONTAINERS_DIR}/${slug}"
    mkdir -p "${container_root}/etc"

    local preview_host="${slug}.${domain}"
    local preview_url="https://${preview_host}"

    local db_name="preview_${slug//-/_}"
    local db_user="preview_${slug//-/_}"

    # /etc/preview.env — readable by preview user (644); no GitHub credential.
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
        local db_secrets
        db_secrets=$(curl -sf "http://127.0.0.1:3200/internal/secrets/${repo}" 2>/dev/null || true)
        if [[ -n "$db_secrets" ]]; then
            echo "$db_secrets"
        fi
    } > "$env_file"
    chmod 644 "$env_file"
}

# Fails if systemd-machined's registration for the container is stale, i.e. its
# Leader isn't the container's real init. `nixos-container run` resolves the
# leader through machined, so with a stale registration every run fails with
# "nsenter: reassociate to namespaces failed: No such process" (see #201).
# Skips the check when either PID can't be read (e.g. the container is booting).
# Also fails for a slug `preview create` couldn't have produced, since the slug
# goes into a sysfs path and a machinectl argument.
# Args: slug
check_machine_leader() {
    local slug="$1"
    if [[ ! "$slug" =~ ^[a-z0-9]+(-[a-z0-9]+)*$ ]]; then
        error "Invalid preview slug '$slug'."
        return 1
    fi
    local procs="/sys/fs/cgroup/machine.slice/container@${slug}.service/payload/init.scope/cgroup.procs"
    local leader init_procs init
    leader=$(machinectl show -p Leader --value -- "$slug" 2>/dev/null || true)
    init_procs=$(cat "$procs" 2>/dev/null || true)
    [[ -n "$leader" && -n "$init_procs" ]] || return 0
    # init.scope can briefly hold other PIDs while the container boots, and the
    # file isn't ordered, so look for the leader anywhere in it.
    if ! grep -qx -- "$leader" <<< "$init_procs"; then
        init=$(head -1 <<< "$init_procs")
        error "Container '$slug' has a stale systemd-machined registration (Leader $leader, but its init is $init)."
        error "nixos-container run can't reach it; until the host is rebooted, use: nsenter -t $init -m -u -i -n -p -- <cmd>"
        return 1
    fi
}

# nixos-container run, after checking the container is reachable through machined.
# Args: slug -- cmd...
container_run() {
    local slug="$1"
    shift
    check_machine_leader "$slug" || return 1
    nc_run "$slug" "$@"
}

# Reads service names from meta JSON and starts them via nixos-container run.
# Returns non-zero, after reporting why, if the services couldn't be started.
# Args: slug meta_file
start_container_services() {
    local slug="$1" meta_file="$2"
    local setup_service
    setup_service=$(jq -r '.setupService' "$meta_file")
    local -a app_services
    readarray -t app_services < <(jq -r '.appServices[]' "$meta_file")

    # `nixos-container start` returns before the container's network is usable;
    # setup services that download dependencies fail with "Could not resolve host".
    local i resolved=0
    for i in {1..30}; do
        # A stale registration doesn't clear itself, so don't wait it out.
        check_machine_leader "$slug" || return 1
        if nc_run "$slug" -- getent hosts github.com &>/dev/null; then
            resolved=1
            break
        fi
        sleep 2
    done
    if [[ $resolved -eq 0 ]]; then
        warn "Container '$slug' couldn't resolve github.com after 60s; starting its services anyway."
    fi

    # --no-block queues the jobs and returns, instead of waiting for setup to finish.
    if ! container_run "$slug" -- systemctl start --no-block "$setup_service" "${app_services[@]}"; then
        error "Failed to start services for '$slug': $setup_service ${app_services[*]}"
        return 1
    fi
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
    validate_repo_branch "$repo" "$branch"

    ensure_root
    ensure_dirs
    load_secrets

    local github_token
    github_token=$(get_github_token "$repo")

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
    validate_repo_branch "$repo" "$branch"

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

    lock_slug "$slug"

    if [[ -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' already exists. Use 'preview destroy $slug' first."
    fi

    local domain="${PREVIEW_DOMAIN:-preview.example.com}"
    local admin_ssh_key="${ADMIN_SSH_KEY:-}"
    if [[ -z "$admin_ssh_key" ]]; then
        fatal "ADMIN_SSH_KEY not set in $SECRETS_FILE"
    fi

    # Get GitHub token (host-side only: it never enters the container)
    local github_token
    github_token=$(get_github_token "$repo")

    # Get commit SHA (cache key for this deploy)
    local commit_sha
    commit_sha=$(get_commit_sha "$repo" "$branch" "$github_token")
    info "Deploying commit ${commit_sha:0:8} of ${repo}@${branch}"

    # Fetch config (cached by commit SHA)
    local config_file
    config_file=$(get_or_fetch_config "$repo" "$branch" "$commit_sha" "$github_token")

    # Fetch the code now, while the token is fresh; it's sent in after the build.
    mirror_fetch "$repo" "$branch" "$commit_sha" "$github_token"

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
    # claim_slot writes the tracking file under flock so no other create can
    # race for the same slot.
    local slot
    slot=$(claim_slot "$PREVIEW_DIR/$slug" "$repo" "$branch")
    read -r _slot host_ip local_ip _rest < "$PREVIEW_DIR/$slug"

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
    write_env_files "$slug" "$repo" "$branch" "$domain" "$host_ip" "$db_mode" "$db_pass" "$meta_file"
    echo "$commit_sha" > "$PREVIEW_DIR/${slug}.sha"

    # Start the container
    nixos-container start "$slug"

    # Put the code in the container before setup runs, then queue setup and
    # app services.
    local started=1
    if ! sync_code "$slug" "$repo" "$branch" "$commit_sha"; then
        started=0
    elif ! start_container_services "$slug" "$meta_file"; then
        started=0
    fi

    # Write Caddy config from meta routes
    write_caddy_config "$slug" "$local_ip" "$domain" "$meta_file"

    if [[ $started -eq 0 ]]; then
        # Don't leave a half-created preview behind: the caller sees a failure
        # and its retry would stop at "already exists".
        error "Preview '$slug' was created, but couldn't be started (see above). Removing it."
        cmd_destroy "$slug"
        exit 1
    fi

    local preview_url="https://${slug}.${domain}"

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
    lock_slug "$slug"

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    local _slot _host_ip _local_ip repo="" branch=""
    read -r _slot _host_ip _local_ip repo branch < "$PREVIEW_DIR/$slug" || true

    info "Destroying preview '$slug'..."

    # Remove Caddy config
    remove_caddy_config "$slug"

    # Stop and destroy container
    nixos-container stop "$slug" 2>/dev/null || true
    nixos-container destroy "$slug"
    # Clean up stale veth interface if the kernel didn't remove it
    ip link delete "ve-${slug}" 2>/dev/null || true

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

    if valid_repo "$repo" && valid_branch "$branch"; then
        mirror_forget_branch "$repo" "$branch"
    fi

    success "Preview '$slug' destroyed."
}

cmd_update() {
    local slug="${1:-}"
    if [[ -z "$slug" ]]; then
        fatal "Usage: preview update <slug>"
    fi

    ensure_root
    ensure_dirs
    load_secrets
    # Waits for a running create/update of this preview, then reads its state.
    lock_slug "$slug"

    if [[ ! -f "$PREVIEW_DIR/$slug" ]]; then
        fatal "Preview '$slug' not found."
    fi

    # Read tracking file
    local slot host_ip local_ip repo branch
    read -r slot host_ip local_ip repo branch < "$PREVIEW_DIR/$slug"

    # Remove any token left by older tekton versions, even if nothing changed.
    scrub_preview "$slug" "$repo" || warn "Could not fully scrub GitHub tokens from '$slug'."
    validate_repo_branch "$repo" "$branch"

    local domain="${PREVIEW_DOMAIN:-preview.example.com}"
    local admin_ssh_key="${ADMIN_SSH_KEY:-}"
    if [[ -z "$admin_ssh_key" ]]; then
        fatal "ADMIN_SSH_KEY not set in $SECRETS_FILE"
    fi

    local github_token
    github_token=$(get_github_token "$repo")

    # Fetch latest commit SHA from GitHub
    local new_sha
    new_sha=$(get_commit_sha "$repo" "$branch" "$github_token")

    # Compare with stored SHA — exit early if unchanged
    local old_sha=""
    local sha_file="$PREVIEW_DIR/${slug}.sha"
    if [[ -f "$sha_file" ]]; then
        old_sha=$(cat "$sha_file")
    fi

    if [[ -n "$old_sha" ]] && [[ "$old_sha" == "$new_sha" ]]; then
        info "No new commits for '$slug' (${new_sha:0:8}). Nothing to do."
        return
    fi

    info "Updating preview '$slug': ${old_sha:0:8} → ${new_sha:0:8}"

    # Fetch config, code and build closure for the new commit
    local config_file
    config_file=$(get_or_fetch_config "$repo" "$branch" "$new_sha" "$github_token")
    mirror_fetch "$repo" "$branch" "$new_sha" "$github_token"
    build_preview_closure "$config_file" "$new_sha" "$admin_ssh_key"

    # Determine whether the NixOS closure changed
    local old_toplevel="" new_toplevel=""
    new_toplevel=$(cat "$CLOSURE_CACHE_DIR/${new_sha}.toplevel")

    if [[ -n "$old_sha" ]] && [[ -f "$CLOSURE_CACHE_DIR/${old_sha}.toplevel" ]]; then
        old_toplevel=$(cat "$CLOSURE_CACHE_DIR/${old_sha}.toplevel")
    fi

    if [[ -n "$old_toplevel" ]] && [[ "$old_toplevel" == "$new_toplevel" ]]; then
        # ── CODE-ONLY UPDATE (fast path) ──────────────────────────────────
        info "NixOS closure unchanged — doing code-only update"

        # Send the new commits in; setup fetches them on its rebuild.
        sync_code "$slug" "$repo" "$branch" "$new_sha" "$old_sha" \
            || fatal "Failed to sync code into '$slug'."

        # Signal the setup service to do a full rebuild
        container_run "$slug" -- su -s /bin/sh preview -c "touch /tmp/force-rebuild" \
            || fatal "Failed to signal a rebuild in '$slug'."

        local SETUP_SERVICE
        local -a APP_SERVICES
        read_service_names "$slug"
        container_run "$slug" -- systemctl restart "$SETUP_SERVICE" "${APP_SERVICES[@]}" \
            || fatal "Failed to restart services in '$slug'."

        # Record the commit only now, so a failed update is retried by the next one.
        echo "$new_sha" > "$sha_file"

        success "Preview '$slug' is rebuilding (code-only). Check progress with: preview logs $slug --follow"
    else
        # ── CLOSURE CHANGED — full container rebuild ──────────────────────
        info "NixOS closure changed — rebuilding container"

        # Copy new meta to preview dir and validate
        local new_meta_store
        new_meta_store=$(cat "$CLOSURE_CACHE_DIR/${new_sha}.meta-path")
        local meta_file="$PREVIEW_DIR/${slug}.meta"
        local old_meta_file="${meta_file}.old"

        # Preserve old meta for DB-mode comparison
        if [[ -f "$meta_file" ]]; then
            cp "$meta_file" "$old_meta_file"
        fi

        cp "$new_meta_store" "$meta_file"

        # Validate required meta fields
        for field in setupService appServices database routes; do
            if ! jq -e ".${field}" "$meta_file" >/dev/null 2>&1; then
                # Restore old meta on failure
                if [[ -f "$old_meta_file" ]]; then
                    mv "$old_meta_file" "$meta_file"
                fi
                fatal "preview-config.nix is missing required meta field: ${field}"
            fi
        done

        local new_db_mode old_db_mode
        new_db_mode=$(jq -r '.database' "$meta_file")
        old_db_mode="none"
        if [[ -f "$old_meta_file" ]]; then
            old_db_mode=$(jq -r '.database' "$old_meta_file")
        fi

        # Stop + destroy old container
        nixos-container stop "$slug" 2>/dev/null || true
        nixos-container destroy "$slug"
        # Clean up stale veth interface if the kernel didn't remove it
        ip link delete "ve-${slug}" 2>/dev/null || true

        # systemd needs to forget the old unit files after destroy
        systemctl daemon-reload

        # Handle DB mode transitions
        if [[ "$old_db_mode" == "host" ]]; then
            drop_db "$slug"
        fi

        local db_pass=""
        if [[ "$new_db_mode" == "host" ]]; then
            db_pass=$(create_db "$slug")
        fi

        # Create new container reusing existing IPs
        nixos-container create "$slug" \
            --system-path "$new_toplevel" \
            --host-address "$host_ip" \
            --local-address "$local_ip"

        # systemd needs to pick up the new container unit files before start
        systemctl daemon-reload

        write_env_files "$slug" "$repo" "$branch" "$domain" "$host_ip" "$new_db_mode" "$db_pass" "$meta_file"

        # Start the container
        nixos-container start "$slug"

        # Fresh rootfs: send the full history in before setup runs, then start
        # services.
        local services_started=1
        if ! sync_code "$slug" "$repo" "$branch" "$new_sha"; then
            services_started=0
        elif ! start_container_services "$slug" "$meta_file"; then
            services_started=0
        fi

        # Rewrite Caddy config from new meta
        write_caddy_config "$slug" "$local_ip" "$domain" "$meta_file"

        # Clean up
        rm -f "$old_meta_file"

        if [[ $services_started -eq 0 ]]; then
            fatal "Preview '$slug' was rebuilt, but couldn't be started (see above)."
        fi

        # Record the commit only now, so a failed update is retried by the next one.
        echo "$new_sha" > "$sha_file"

        success "Preview '$slug' rebuilt with new config. Check progress with: preview logs $slug --follow"
    fi
}

# Prints the slug of every preview (one per tracking file).
list_slugs() {
    local f name
    for f in "$PREVIEW_DIR"/*; do
        [[ -f "$f" ]] || continue
        name=$(basename "$f")
        # Skip hidden files and metadata files
        [[ "$name" == .* ]]         && continue
        [[ "$name" == *.type ]]     && continue
        [[ "$name" == *.meta ]]     && continue
        [[ "$name" == *.meta.old ]] && continue
        [[ "$name" == *.sha ]]      && continue
        echo "$name"
    done
}

cmd_scrub_tokens() {
    ensure_root

    local slug repo _slot _host_ip _local_ip _branch failed=0 total=0
    while IFS= read -r slug; do
        # Destroyed (or still being created) since it was listed.
        if ! read -r _slot _host_ip _local_ip repo _branch 2>/dev/null < "$PREVIEW_DIR/$slug" ||
            [[ -z "$repo" ]]; then
            continue
        fi
        total=$((total + 1))
        if ! scrub_preview "$slug" "$repo"; then
            warn "Could not fully scrub '$slug'."
            failed=$((failed + 1))
        fi
    done < <(list_slugs)

    if [[ $failed -gt 0 ]]; then
        fatal "Scrubbed $((total - failed)) of $total previews; $failed failed (see warnings above)."
    fi
    success "Scrubbed GitHub tokens from $total previews."
}

cmd_list() {
    ensure_dirs
    load_secrets

    local domain="${PREVIEW_DOMAIN:-preview.example.com}"
    local found=0 name f
    echo -e "${BOLD}SLUG                STATUS          BRANCH                          URL${NC}"

    while IFS= read -r name; do
        f="$PREVIEW_DIR/$name"
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
    done < <(list_slugs)

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

    container_run "$slug" -- journalctl "${units[@]}" --no-pager "${follow_args[@]}"
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
    echo "  scrub-tokens                                     Remove GitHub tokens older versions left in previews"
    echo "  help                                             Show this help"
    echo ""
    echo -e "${BOLD}How it works:${NC}"
    echo "  Each repo ships a preview-config.nix at its root. tekton fetches it from"
    echo "  GitHub, builds a NixOS container closure from it (cached by commit SHA),"
    echo "  and runs the container. No repo-specific knowledge lives in tekton."
    echo "  The host fetches the code with a GitHub App token and sends it into the"
    echo "  container; the container is never given a GitHub credential."
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
# GitHub access goes through the webhook's App token, never a caller's token,
# and the repo's config is evaluated with --impure, so keep any out of reach.
unset GITHUB_TOKEN

command="${1:-help}"
shift || true

case "$command" in
    create)  cmd_create "$@" ;;
    destroy) cmd_destroy "$@" ;;
    update)  cmd_update "$@" ;;
    list)    cmd_list ;;
    logs)    cmd_logs "$@" ;;
    build)   cmd_build "$@" ;;
    scrub-tokens) cmd_scrub_tokens ;;
    help|--help|-h) cmd_help ;;
    *)       fatal "Unknown command: $command. Run 'preview help' for usage." ;;
esac
