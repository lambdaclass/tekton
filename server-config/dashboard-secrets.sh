#!/usr/bin/env bash
# =============================================================================
# Dashboard secrets: SECRETS_ENCRYPTION_KEY, JWT_SECRET, GITHUB_CLIENT_SECRET.
# They live in 1Password and on the server only in /run/dashboard-secrets/
# secrets.env (tmpfs). See docs/deployment-guide.md, "Dashboard secrets".
#
# From your machine (repo root, signed in with `op signin`):
#   server-config/dashboard-secrets.sh push <server>
#       Fill the template from 1Password and install it on the server.
#   server-config/dashboard-secrets.sh copy <server> <NAME> | pbcopy
#       A value from the server's /run file (fresh install: move it to 1Password).
#   server-config/dashboard-secrets.sh copy-live <server> <NAME> | pbcopy
#       The value the running dashboard uses (upgrade: move it to 1Password).
#   server-config/dashboard-secrets.sh verify-key <server>
#       Check that 1Password's SECRETS_ENCRYPTION_KEY is the one in use.
#   server-config/dashboard-secrets.sh check <server> [--unit]
#       Preflight run by deploy.sh.
# On the server:
#   server-config/dashboard-secrets.sh check-file <file>
#
# Values only travel through pipes: never argv, shell history or the terminal.
# =============================================================================

set -euo pipefail

TEMPLATE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/dashboard-secrets.env.tpl"

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

remote_install() {
    local d=/run/dashboard-secrets f=/run/dashboard-secrets/secrets.env tmp missing
    install -d -m 700 "$d" || return 1
    umask 077
    tmp=$(mktemp "$d/.secrets.env.XXXXXX") || return 1
    cat > "$tmp"
    missing=$(missing_values "$tmp" | tr '\n' ' ')
    if [ -n "$missing" ]; then
        rm -f "$tmp"
        echo "Refusing to install: no value for: $missing(did op inject fail?). $f is unchanged." >&2
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
        if grep -Eq "^[[:space:]]*(export[[:space:]]+)?$v[[:space:]]*=" "$env" 2>/dev/null; then
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
    remote_install remote_copy remote_live_value remote_preflight"

# ── Local side ─────────────────────────────────────────────────────────────

die() { echo "Error: $*" >&2; exit 1; }

on_server() {
    local server=$1
    shift
    local code
    # shellcheck disable=SC2086
    code="$(declare -f $REMOTE_FUNCS); $*"
    ssh "root@${server}" "bash -c $(printf '%q' "$code")"
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

sha256() { if command -v sha256sum >/dev/null; then sha256sum; else shasum -a 256; fi; }

op_ref() {
    sed -n -E "s/^$1=\{\{ *(op:[^ }]+) *\}\}$/\1/p" "$TEMPLATE"
}

push_command() { echo "server-config/dashboard-secrets.sh push $1"; }

main() {
    local cmd=${1:-} server=${2:-}
    case "$cmd" in
        push)
            [ -n "$server" ] || die "usage: $0 push <server>"
            command -v op >/dev/null || die "the 1Password CLI (op) is not installed"
            op inject -i "$TEMPLATE" | on_server "$server" remote_install
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
        verify-key)
            [ -n "$server" ] || die "usage: $0 verify-key <server>"
            local ref ours live
            ref=$(op_ref SECRETS_ENCRYPTION_KEY)
            [ -n "$ref" ] || die "no SECRETS_ENCRYPTION_KEY reference in $TEMPLATE"
            ours=$(op read --no-newline "$ref" | sha256)
            live=$(on_server "$server" remote_live_value SECRETS_ENCRYPTION_KEY | sha256)
            if [ "$ours" = "$live" ]; then
                echo "OK: 1Password's SECRETS_ENCRYPTION_KEY is the key the dashboard uses."
            else
                die "MISMATCH: 1Password's SECRETS_ENCRYPTION_KEY is not the key the dashboard uses. Do not delete anything on the server; fix the 1Password value (copy-live) and verify again."
            fi
            ;;
        check)
            [ -n "$server" ] || die "usage: $0 check <server> [--unit]"
            local problems
            problems=$(on_server "$server" remote_preflight "${3:-}")
            if [ -n "$problems" ]; then
                {
                    echo "Dashboard secrets are not set up correctly:"
                    sed 's/^/  /' <<< "$problems"
                    echo "Push the secrets from 1Password (repo root on your machine):"
                    echo "  $(push_command "$server")"
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
            sed -n '3,21p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
            exit 1
            ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
