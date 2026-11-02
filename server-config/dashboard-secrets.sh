#!/usr/bin/env bash
# =============================================================================
# Dashboard secrets: SECRETS_ENCRYPTION_KEY, JWT_SECRET, GITHUB_CLIENT_SECRET.
# They live in 1Password (vault Tekton, item key-tekton-00) and on the server
# only in /run/dashboard-secrets/secrets.env (tmpfs). No 1Password CLI: values
# are copied from the app and pasted at hidden prompts on the server. See
# docs/deployment-guide.md, "Dashboard secrets".
#
# From your machine (repo root):
#   server-config/dashboard-secrets.sh set <server>
#       Prompt (hidden) for the three values and install them on the server.
#   server-config/dashboard-secrets.sh verify-key <server>
#       Prompt (hidden) for 1Password's key; check it is the one in use.
#   server-config/dashboard-secrets.sh copy <server> <NAME> | pbcopy
#       A value from the server's /run file (fresh install: move it to 1Password).
#   server-config/dashboard-secrets.sh copy-live <server> <NAME> | pbcopy
#       The value the running dashboard uses (upgrade: move it to 1Password).
#   server-config/dashboard-secrets.sh check <server> [--unit]
#       Preflight run by deploy.sh.
# On the server:
#   server-config/dashboard-secrets.sh check-file <file>
#
# Values never reach argv, shell history, the laptop's disk or the terminal.
# =============================================================================

set -euo pipefail

# ── Functions that run on the server (shipped with declare -f) ─────────────

secret_names() { echo "SECRETS_ENCRYPTION_KEY JWT_SECRET GITHUB_CLIENT_SECRET"; }

# Raw value of the last NAME= line, the one systemd uses.
env_file_value() {
    sed -n -E "s/^[[:space:]]*(export[[:space:]]+)?$2[[:space:]]*=[[:space:]]*(.*)$/\2/p" "$1" 2>/dev/null | tail -n 1
}

unquote() { sed -E 's/[[:space:]]+$//; s/^"(.*)"$/\1/; s/^\x27(.*)\x27$/\1/'; }

# Rejects missing, empty, quoted-empty and whitespace-only values.
has_value() {
    [ -n "$(env_file_value "$1" "$2" | tr -d "[:space:]\"'")" ]
}

missing_values() {
    local v
    for v in $(secret_names); do
        has_value "$1" "$v" || echo "$v"
    done
}

dashboard_started() {
    local before after state
    before=$(systemctl show -p NRestarts --value dashboard)
    sleep 8
    after=$(systemctl show -p NRestarts --value dashboard)
    state=$(systemctl is-active dashboard || true)
    if [ "$state" = active ] && [ "$after" = "$before" ]; then
        echo "Dashboard is running."
        return 0
    fi
    echo "Dashboard did not come up (state: $state, restarts: $before -> $after):" >&2
    journalctl -u dashboard -n 20 --no-pager >&2 || true
    return 1
}

# Hidden prompt on the server's tty; the value lands in SECRET_VALUE. Values are
# written single-quoted, which systemd's EnvironmentFile parser takes literally,
# so only a single quote, whitespace and control characters are refused.
read_secret() {
    local v c extra
    SECRET_VALUE=""
    for _ in 1 2 3; do
        printf '%s (paste, hidden): ' "$1" > /dev/tty
        IFS= read -rs v < /dev/tty || { printf '\n' > /dev/tty; return 1; }
        printf '\n' > /dev/tty
        # A paste with a line break leaves the rest queued; CR/LF alone is fine.
        extra=""
        while IFS= read -rs -t 0.3 -n 1 c < /dev/tty; do
            extra+="$c"
        done
        v=${v%$'\r'}
        v=${v#"${v%%[![:blank:]]*}"}
        v=${v%"${v##*[![:blank:]]}"}
        if [ -n "${extra//[$'\r\t ']/}" ]; then
            echo "  The paste had more than one line; try again." > /dev/tty
        elif [ -z "$v" ]; then
            echo "  Empty; try again." > /dev/tty
        elif [[ "$v" == *[[:space:][:cntrl:]\']* ]]; then
            echo "  Contains a single quote, whitespace or a control character; regenerate it without those." > /dev/tty
        else
            SECRET_VALUE=$v
            return 0
        fi
    done
    echo "Giving up after 3 attempts." > /dev/tty
    return 1
}

remote_set() {
    local d=/run/dashboard-secrets f=/run/dashboard-secrets/secrets.env tmp name missing
    [ -r /dev/tty ] || { echo "No terminal; run this from an interactive shell." >&2; return 1; }
    install -d -m 700 "$d" || return 1
    umask 077
    tmp=$(mktemp "$d/.secrets.env.XXXXXX") || return 1
    for name in $(secret_names); do
        if ! read_secret "$name"; then
            rm -f "$tmp"
            echo "Nothing changed; $f is as it was." >&2
            return 1
        fi
        printf "%s='%s'\n" "$name" "$SECRET_VALUE" >> "$tmp"
    done
    SECRET_VALUE=""
    missing=$(missing_values "$tmp" | tr '\n' ' ')
    if [ -n "$missing" ]; then
        rm -f "$tmp"
        echo "Refusing to install: no value for: $missing. $f is unchanged." >&2
        return 1
    fi
    mv -f "$tmp" "$f" || { rm -f "$tmp"; return 1; }
    echo "Installed $f."
    if systemctl cat dashboard >/dev/null 2>&1; then
        systemctl restart dashboard && dashboard_started
    else
        echo "No dashboard unit yet; server-setup.sh will use the file."
    fi
}

remote_copy() {
    local f=/run/dashboard-secrets/secrets.env
    has_value "$f" "$1" || { echo "$f has no value for $1." >&2; return 1; }
    printf '%s' "$(env_file_value "$f" "$1" | unquote)"
}

# What the running dashboard uses: its environment, or for the key, the key
# file an earlier build read (SECRETS_ENCRYPTION_KEY_FILE or the default).
remote_live_value() {
    local pid v kf
    pid=$(systemctl show -p MainPID --value dashboard)
    if [ -z "$pid" ] || [ "$pid" = 0 ]; then
        echo "The dashboard is not running; start it first, its live value is the source of truth." >&2
        return 1
    fi
    v=$(tr '\0' '\n' < "/proc/$pid/environ" | sed -n "s/^$1=//p" | head -n 1)
    if [ -n "$v" ]; then
        printf '%s' "$v"
        return 0
    fi
    if [ "$1" = SECRETS_ENCRYPTION_KEY ]; then
        kf=$(tr '\0' '\n' < "/proc/$pid/environ" | sed -n 's/^SECRETS_ENCRYPTION_KEY_FILE=//p' | head -n 1)
        kf=${kf:-/var/lib/dashboard/secrets-key}
        case "$kf" in /*) ;; *) kf="$(readlink "/proc/$pid/cwd")/$kf" ;; esac
        if [ -f "$kf" ]; then
            tr -d '[:space:]' < "$kf"
            return 0
        fi
    fi
    echo "The running dashboard has no $1 (neither in its environment nor, for the key, in a key file)." >&2
    return 1
}

remote_verify_key() {
    local ours live
    [ -r /dev/tty ] || { echo "No terminal; run this from an interactive shell." >&2; return 1; }
    live=$(remote_live_value SECRETS_ENCRYPTION_KEY) || return 1
    live=$(printf '%s' "$live" | sha256sum)
    read_secret SECRETS_ENCRYPTION_KEY || return 1
    ours=$(printf '%s' "$SECRET_VALUE" | sha256sum)
    SECRET_VALUE=""
    if [ "$ours" = "$live" ]; then
        echo "OK"
    else
        echo "MISMATCH: that is not the key the dashboard uses. Don't delete anything on the server; fix the 1Password field (copy-live) and verify again."
        return 1
    fi
}

remote_preflight() {
    local f=/run/dashboard-secrets/secrets.env env=/var/secrets/dashboard.env v kf
    if [ ! -f "$f" ]; then
        echo "$f is missing (it is on tmpfs, so a reboot wipes it)."
    else
        for v in $(missing_values "$f"); do
            echo "$f has no value for $v."
        done
    fi
    for v in $(secret_names); do
        if grep -Eq "^[[:space:]]*(export[[:space:]]+)?${v}[[:space:]]*=" "$env" 2>/dev/null; then
            echo "$env still defines $v; save it in 1Password, then remove the line."
        fi
    done
    for kf in /var/lib/dashboard/secrets-key $(env_file_value "$env" SECRETS_ENCRYPTION_KEY_FILE | unquote); do
        case "$kf" in /*) ;; *) kf="/opt/dashboard/$kf" ;; esac
        if [ -e "$kf" ]; then
            echo "Old key file $kf still exists; once 1Password holds the key (verify-key), delete it."
        fi
    done
    if ! grep -q /run/dashboard-secrets/secrets.env /etc/nixos/configuration.nix 2>/dev/null; then
        echo "/etc/nixos/configuration.nix doesn't list $f in the dashboard's EnvironmentFile; add it (deploy.sh never copies configuration.nix)."
    elif [ "${1:-}" = --unit ] && ! systemctl show -p EnvironmentFiles dashboard | grep -q /run/dashboard-secrets/secrets.env; then
        echo "The loaded dashboard unit doesn't read $f yet; run nixos-rebuild switch (./deploy.sh <server> nix)."
    fi
}

REMOTE_FUNCS="secret_names env_file_value unquote has_value missing_values dashboard_started
    read_secret remote_set remote_copy remote_live_value remote_verify_key remote_preflight"

# ── Local side ─────────────────────────────────────────────────────────────

die() { echo "Error: $*" >&2; exit 1; }

# With -t first, the remote side gets a terminal for the hidden prompts.
on_server() {
    local tty=()
    if [ "$1" = -t ]; then
        tty=(-t)
        shift
    fi
    local server=$1
    shift
    local code
    # shellcheck disable=SC2086
    code="$(declare -f $REMOTE_FUNCS); $*"
    # shellcheck disable=SC2029
    ssh ${tty[@]+"${tty[@]}"} "root@${server}" "bash -c $(printf '%q' "$code")"
}

check_name() {
    case "$1" in
        SECRETS_ENCRYPTION_KEY | JWT_SECRET | GITHUB_CLIENT_SECRET) ;;
        *) die "unknown secret '$1' (expected one of: $(secret_names))" ;;
    esac
}

refuse_terminal() {
    [ -t 1 ] && die "refusing to print a secret to the terminal; pipe it, e.g. '| pbcopy'"
    return 0
}

need_terminal() {
    [ -t 0 ] && [ -t 1 ] || die "run this from an interactive terminal; it prompts on the server"
}

set_command() { echo "server-config/dashboard-secrets.sh set $1"; }

main() {
    local cmd=${1:-} server=${2:-}
    case "$cmd" in
        set)
            [ -n "$server" ] || die "usage: $0 set <server>"
            need_terminal
            echo "Copy each field from 1Password (vault Tekton, item key-tekton-00) and paste it at the prompt."
            on_server -t "$server" remote_set
            ;;
        verify-key)
            [ -n "$server" ] || die "usage: $0 verify-key <server>"
            need_terminal
            echo "Paste SECRETS_ENCRYPTION_KEY from 1Password (vault Tekton, item key-tekton-00)."
            on_server -t "$server" remote_verify_key
            ;;
        copy | copy-live)
            [ -n "$server" ] && [ -n "${3:-}" ] || die "usage: $0 $cmd <server> <NAME> | pbcopy"
            check_name "$3"
            refuse_terminal
            if [ "$cmd" = copy ]; then
                on_server "$server" remote_copy "$3"
            else
                on_server "$server" remote_live_value "$3"
            fi
            ;;
        check)
            [ -n "$server" ] || die "usage: $0 check <server> [--unit]"
            local problems
            problems=$(on_server "$server" remote_preflight "${3:-}")
            if [ -n "$problems" ]; then
                {
                    echo "Dashboard secrets are not set up correctly:"
                    while IFS= read -r line; do echo "  $line"; done <<< "$problems"
                    echo "To set the secrets from 1Password (repo root on your machine):"
                    echo "  $(set_command "$server")"
                    echo "See docs/deployment-guide.md, \"Dashboard secrets\"."
                } >&2
                exit 1
            fi
            echo "Dashboard secrets present."
            ;;
        check-file)
            [ -n "$server" ] || die "usage: $0 check-file <file>"
            local missing
            missing=$(missing_values "$server" | tr '\n' ' ')
            [ -z "$missing" ] || die "$server has no value for: $missing"
            ;;
        *)
            sed -n '3,23p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
            exit 1
            ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
