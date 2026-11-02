#!/usr/bin/env bash
# =============================================================================
# Dashboard secrets: SECRETS_ENCRYPTION_KEY, JWT_SECRET, GITHUB_CLIENT_SECRET.
# They live in 1Password (vault Tekton, item key-tekton-00) and on the server
# only in /run/dashboard-secrets/secrets.env (tmpfs). No 1Password CLI: values
# are copied from the app and pasted at hidden prompts on the server. See
# docs/deployment-guide.md, "Dashboard secrets".
#
# From your machine (repo root):
#   server-config/dashboard-secrets.sh set <server> [--new-key]
#       Prompt (hidden) for the three values and install them on the server.
#       The key must match the running dashboard's unless --new-key.
#   server-config/dashboard-secrets.sh verify-key <server>
#       Prompt (hidden) for 1Password's key; check it is the one in use.
#   server-config/dashboard-secrets.sh check-live <server>
#       Whether the running dashboard's values can be entered with set.
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

# The value systemd's EnvironmentFile parser hands the process for NAME (last
# assignment wins), following env-file.c: quotes only open a value, "..."
# unescapes \" \\ \` \$, '...' is literal, unquoted text drops trailing blanks.
# Not modelled: backslash-continued comments.
sd_env_get() {
    local data n i c state key val kws vws found="" result=""
    [ -r "$1" ] || return 1
    data=$(cat "$1"; printf x)
    data=${data%x}
    n=${#data}
    state=PRE_KEY key="" val="" kws=-1 vws=-1
    for ((i = 0; i <= n; i++)); do
        if ((i == n)); then c=$'\n'; else c=${data:i:1}; fi
        case $state in
            PRE_KEY)
                case $c in
                    '#' | ';') state=COMMENT ;;
                    ' ' | $'\t' | $'\n' | $'\r') ;;
                    *) state=KEY key=$c kws=-1 ;;
                esac ;;
            COMMENT)
                case $c in $'\n' | $'\r') state=PRE_KEY ;; esac ;;
            KEY)
                case $c in
                    $'\n' | $'\r') state=PRE_KEY ;;
                    =)
                        ((kws >= 0)) && key=${key:0:kws}
                        state=PRE_VALUE val="" vws=-1 ;;
                    ' ' | $'\t') ((kws < 0)) && kws=${#key}; key+=$c ;;
                    *) kws=-1; key+=$c ;;
                esac ;;
            PRE_VALUE)
                case $c in
                    $'\n' | $'\r')
                        [ "$key" = "$2" ] && found=1 result=""
                        state=PRE_KEY ;;
                    "'") state=SQ ;;
                    '"') state=DQ ;;
                    \\) state=ESC ;;
                    ' ' | $'\t') ;;
                    *) state=VALUE val=$c ;;
                esac ;;
            VALUE)
                case $c in
                    $'\n' | $'\r')
                        ((vws >= 0)) && val=${val:0:vws}
                        [ "$key" = "$2" ] && found=1 result=$val
                        state=PRE_KEY ;;
                    \\) state=ESC vws=-1 ;;
                    ' ' | $'\t') ((vws < 0)) && vws=${#val}; val+=$c ;;
                    *) vws=-1; val+=$c ;;
                esac ;;
            ESC)
                state=VALUE
                case $c in $'\n' | $'\r') ;; *) val+=$c ;; esac ;;
            SQ)
                case $c in "'") state=VALUE ;; *) val+=$c ;; esac ;;
            DQ)
                case $c in '"') state=VALUE ;; \\) state=DQESC ;; *) val+=$c ;; esac ;;
            DQESC)
                state=DQ
                case $c in '"' | \\ | '`' | '$') val+=$c ;; $'\n') ;; *) val+="\\$c" ;; esac ;;
        esac
    done
    [ -n "$found" ] || return 1
    printf '%s' "$result"
}

# Double-quoted for EnvironmentFile: exactly \ " ` $ need a backslash there.
sd_quote() {
    local v=$1 bs=\\ dq='"' bt='`' dl='$'
    v=${v//"$bs"/"$bs$bs"}
    v=${v//"$dq"/"$bs$dq"}
    v=${v//"$bt"/"$bs$bt"}
    v=${v//"$dl"/"$bs$dl"}
    printf '"%s"' "$v"
}

# True when set can't take the value: a control character other than tab
# (newline included), or invalid UTF-8, which systemd drops.
bad_chars() {
    local LC_ALL=C v=${1//$'\t'/}
    [[ "$v" == *[[:cntrl:]]* ]] && return 0
    if command -v iconv >/dev/null 2>&1; then
        printf '%s' "$1" | iconv -f UTF-8 -t UTF-8 >/dev/null 2>&1 && return 1
        return 0
    fi
    return 1
}

# Non-empty and not only blanks, as the dashboard requires.
has_value() {
    local v
    v=$(sd_env_get "$1" "$2") || return 1
    [[ "$v" == *[![:space:]]* ]]
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

has_tty() { { : < /dev/tty; } 2>/dev/null; }

# Sets LIVE_VALUE to what the running dashboard uses for NAME: its environment
# (read NUL-safely), or for the key, the key file an earlier build read
# (SECRETS_ENCRYPTION_KEY_FILE or the default), trimmed as that build did.
# Returns 0 found, 1 absent, 2 dashboard not running.
get_live() {
    local pid entry kf="" ws
    LIVE_VALUE=""
    pid=$(systemctl show -p MainPID --value dashboard)
    if [ -z "$pid" ] || [ "$pid" = 0 ]; then
        return 2
    fi
    while IFS= read -r -d '' entry; do
        case "$entry" in
            "$1="*) [ -z "$LIVE_VALUE" ] && LIVE_VALUE=${entry#"$1="} ;;
            SECRETS_ENCRYPTION_KEY_FILE=*) [ -z "$kf" ] && kf=${entry#SECRETS_ENCRYPTION_KEY_FILE=} ;;
        esac
    done < "/proc/$pid/environ"
    [ -n "$LIVE_VALUE" ] && return 0
    [ "$1" = SECRETS_ENCRYPTION_KEY ] || return 1
    kf=${kf:-/var/lib/dashboard/secrets-key}
    case "$kf" in /*) ;; *) kf="$(readlink "/proc/$pid/cwd")/$kf" ;; esac
    [ -f "$kf" ] || return 1
    LIVE_VALUE=$(cat "$kf"; printf x)
    LIVE_VALUE=${LIVE_VALUE%x}
    ws=$'[ \t\n\r\v\f]'
    while [[ "$LIVE_VALUE" == $ws* ]]; do LIVE_VALUE=${LIVE_VALUE:1}; done
    while [[ "$LIVE_VALUE" == *$ws ]]; do LIVE_VALUE=${LIVE_VALUE%?}; done
    [ -n "$LIVE_VALUE" ]
}

live_hash() { printf '%s' "$LIVE_VALUE" | sha256sum; }

# Hidden prompt on the server's tty; the value lands in SECRET_VALUE. Only what
# a paste adds is removed: bracketed-paste markers and a trailing CR. With a
# second argument, the value's SHA-256 must equal it. Echo stays off for the
# whole prompt, not just inside each read, so no part of a paste can show.
read_secret() {
    local rc
    TTY_SAVED=$(stty -g < /dev/tty 2>/dev/null) || TTY_SAVED=""
    stty -echo < /dev/tty 2>/dev/null
    read_secret_loop "$@"
    rc=$?
    [ -n "$TTY_SAVED" ] && stty "$TTY_SAVED" < /dev/tty 2>/dev/null
    TTY_SAVED=""
    return $rc
}

read_secret_loop() {
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
        extra=${extra//$'\e[200~'/}
        extra=${extra//$'\e[201~'/}
        v=${v#$'\e[200~'}
        v=${v%$'\e[201~'}
        v=${v%$'\r'}
        if [ -n "${extra//[$'\r\t ']/}" ]; then
            echo "  The paste had more than one line; try again." > /dev/tty
        elif [[ "$v" != *[![:space:]]* ]]; then
            echo "  Empty; try again." > /dev/tty
        elif bad_chars "$v"; then
            echo "  Contains a control character (or invalid UTF-8), which can't go in the file; try again." > /dev/tty
        elif [ -n "${2:-}" ] && [ "$(printf '%s' "$v" | sha256sum)" != "$2" ]; then
            echo "  Doesn't match the key the running dashboard uses. Paste it exactly: the key can't be changed without losing every stored secret." > /dev/tty
        else
            SECRET_VALUE=$v
            return 0
        fi
    done
    echo "Giving up after 3 attempts." > /dev/tty
    return 1
}

remote_set() {
    local d=/run/dashboard-secrets f=/run/dashboard-secrets/secrets.env name missing expected=""
    has_tty || { echo "No terminal; run this from an interactive shell." >&2; return 1; }
    if [ "${1:-}" = --new-key ]; then
        echo "--new-key: the key is not compared with the running dashboard's. A different key makes every stored secret unreadable." > /dev/tty
    else
        get_live SECRETS_ENCRYPTION_KEY
        case $? in
            0) expected=$(live_hash)
               echo "The key must match the one the running dashboard uses." > /dev/tty ;;
            2) echo "No dashboard is running, so the key can't be compared with a live one. If it is wrong, the dashboard refuses to start." > /dev/tty ;;
            *) echo "The running dashboard has no key yet; nothing to compare it with." > /dev/tty ;;
        esac
        LIVE_VALUE=""
    fi
    install -d -m 700 "$d" || return 1
    umask 077
    SECRETS_TMP=$(mktemp "$d/.secrets.env.XXXXXX") || return 1
    trap 'rm -f "$SECRETS_TMP"' EXIT
    trap '[ -n "${TTY_SAVED:-}" ] && stty "$TTY_SAVED" < /dev/tty; rm -f "$SECRETS_TMP"; exit 130' INT TERM HUP
    for name in $(secret_names); do
        if [ "$name" = SECRETS_ENCRYPTION_KEY ]; then
            read_secret "$name" "$expected"
        else
            read_secret "$name"
        fi || {
            rm -f "$SECRETS_TMP"
            echo "Nothing changed; $f is as it was." >&2
            return 1
        }
        printf '%s=%s\n' "$name" "$(sd_quote "$SECRET_VALUE")" >> "$SECRETS_TMP"
    done
    SECRET_VALUE=""
    missing=$(missing_values "$SECRETS_TMP" | tr '\n' ' ')
    if [ -n "$missing" ]; then
        rm -f "$SECRETS_TMP"
        echo "Refusing to install: no value for: $missing. $f is unchanged." >&2
        return 1
    fi
    mv -f "$SECRETS_TMP" "$f" || { rm -f "$SECRETS_TMP"; return 1; }
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
    sd_env_get "$f" "$1"
}

remote_copy_live() {
    get_live "$1"
    case $? in
        0) ;;
        2) echo "The dashboard is not running; start it first, its live value is the source of truth." >&2; return 1 ;;
        *) echo "The running dashboard has no $1 (neither in its environment nor, for the key, in a key file)." >&2; return 1 ;;
    esac
    if bad_chars "$LIVE_VALUE"; then
        LIVE_VALUE=""
        echo "The live $1 contains a newline or another control character (or invalid UTF-8); set can't take it, so it isn't copied." >&2
        return 1
    fi
    printf '%s' "$LIVE_VALUE"
}

remote_check_live() {
    local name rc=0 status
    for name in $(secret_names); do
        get_live "$name"
        status=$?
        if [ "$status" = 2 ]; then
            echo "The dashboard is not running; start it first, its live values are the source of truth."
            return 1
        elif [ "$status" != 0 ]; then
            echo "$name: not set in the running dashboard."
        elif bad_chars "$LIVE_VALUE"; then
            echo "$name: contains a newline or another control character (or invalid UTF-8); set can't take it."
            rc=1
        elif [[ "$LIVE_VALUE" == [[:space:]]* || "$LIVE_VALUE" == *[[:space:]] ]]; then
            echo "$name: OK, but it starts or ends with a space or tab; keep that exactly in 1Password."
        else
            echo "$name: OK"
        fi
    done
    LIVE_VALUE=""
    return $rc
}

remote_verify_key() {
    local live
    has_tty || { echo "No terminal; run this from an interactive shell." >&2; return 1; }
    get_live SECRETS_ENCRYPTION_KEY
    case $? in
        0) ;;
        2) echo "The dashboard is not running; start it first, its live key is the source of truth." >&2; return 1 ;;
        *) echo "The running dashboard has no SECRETS_ENCRYPTION_KEY to compare with." >&2; return 1 ;;
    esac
    live=$(live_hash)
    LIVE_VALUE=""
    read_secret SECRETS_ENCRYPTION_KEY || return 1
    if [ "$(printf '%s' "$SECRET_VALUE" | sha256sum)" = "$live" ]; then
        SECRET_VALUE=""
        echo "OK"
    else
        SECRET_VALUE=""
        echo "MISMATCH: that is not the key the dashboard uses. Don't delete anything on the server; fix the 1Password field (copy-live) and verify again."
        return 1
    fi
}

remote_preflight() {
    local f=/run/dashboard-secrets/secrets.env env=/var/secrets/dashboard.env v kf t
    if [ ! -f "$f" ]; then
        echo "$f is missing (it is on tmpfs, so a reboot wipes it)."
    else
        for v in $(missing_values "$f"); do
            echo "$f has no value for $v."
        done
    fi
    for t in /run/dashboard-secrets/.secrets.env.*; do
        [ -e "$t" ] && echo "Leftover temp file $t from an interrupted set; it holds secrets, delete it."
    done
    for v in $(secret_names); do
        if grep -Eq "^[[:space:]]*(export[[:space:]]+)?${v}[[:space:]]*=" "$env" 2>/dev/null; then
            echo "$env still defines $v; save it in 1Password, then remove the line."
        fi
    done
    for kf in /var/lib/dashboard/secrets-key "$(sd_env_get "$env" SECRETS_ENCRYPTION_KEY_FILE)"; do
        [ -n "$kf" ] || continue
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

REMOTE_FUNCS="secret_names sd_env_get sd_quote bad_chars has_value missing_values dashboard_started
    has_tty get_live live_hash read_secret read_secret_loop remote_set remote_copy remote_copy_live
    remote_check_live remote_verify_key remote_preflight"

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
            [ -n "$server" ] || die "usage: $0 set <server> [--new-key]"
            case "${3:-}" in '' | --new-key) ;; *) die "usage: $0 set <server> [--new-key]" ;; esac
            need_terminal
            echo "Copy each field from 1Password (vault Tekton, item key-tekton-00) and paste it at the prompt."
            on_server -t "$server" remote_set "${3:-}"
            ;;
        verify-key)
            [ -n "$server" ] || die "usage: $0 verify-key <server>"
            need_terminal
            echo "Paste SECRETS_ENCRYPTION_KEY from 1Password (vault Tekton, item key-tekton-00)."
            on_server -t "$server" remote_verify_key
            ;;
        check-live)
            [ -n "$server" ] || die "usage: $0 check-live <server>"
            on_server "$server" remote_check_live
            ;;
        copy | copy-live)
            [ -n "$server" ] && [ -n "${3:-}" ] || die "usage: $0 $cmd <server> <NAME> | pbcopy"
            check_name "$3"
            refuse_terminal
            if [ "$cmd" = copy ]; then
                on_server "$server" remote_copy "$3"
            else
                on_server "$server" remote_copy_live "$3"
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
            sed -n '3,26p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//' >&2
            exit 1
            ;;
    esac
}

if [[ "${BASH_SOURCE[0]}" == "$0" ]]; then
    main "$@"
fi
