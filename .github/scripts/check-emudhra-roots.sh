#!/usr/bin/env bash
# Fails if a trust store still contains an eMudhra root that was removed after
# the October 2026 intermediate key compromise. emSign Root CA - G1 is still in
# Mozilla's store, so it is reported but not failed.
#
# Usage:
#   check-emudhra-roots.sh os              # the OS bundle (git, cargo, curl, docker)
#   check-emudhra-roots.sh node            # Node's bundled store (npm, Playwright downloads)
#   check-emudhra-roots.sh binary <path>   # roots compiled into a Rust binary (webpki-roots)

set -euo pipefail

REMOVED=(
    "emSign ECC Root CA - G3"
    "emSign Root CA - C1"
    "emSign ECC Root CA - C3"
)
STILL_TRUSTED="emSign Root CA - G1"

subjects=""
case "${1:-}" in
    os)
        bundle=/etc/ssl/certs/ca-certificates.crt
        dpkg-query -W -f='ca-certificates ${Version}\n' ca-certificates
        subjects=$(openssl crl2pkcs7 -nocrl -certfile "$bundle" |
            openssl pkcs7 -print_certs -noout | grep '^subject=')
        ;;
    node)
        node --version
        subjects=$(node -e '
            const { rootCertificates } = require("tls");
            const { X509Certificate } = require("crypto");
            for (const pem of rootCertificates) console.log(new X509Certificate(pem).subject.replace(/\n/g, ", "));
        ')
        ;;
    binary)
        path="${2:?usage: check-emudhra-roots.sh binary <path>}"
        # webpki-roots embeds each root's DER subject, so the names appear verbatim.
        for name in "${REMOVED[@]}" "$STILL_TRUSTED" "ISRG Root X1"; do
            if LC_ALL=C grep -a -q -F "$name" "$path"; then subjects+="$name"$'\n'; fi
        done
        if ! grep -q -F "ISRG Root X1" <<<"$subjects"; then
            echo "Control root ISRG Root X1 not found in $path; the check itself is broken." >&2
            exit 2
        fi
        ;;
    *)
        echo "usage: $0 os | node | binary <path>" >&2
        exit 2
        ;;
esac

if grep -q -F "$STILL_TRUSTED" <<<"$subjects"; then
    echo "info: $STILL_TRUSTED is present (still in Mozilla's store)."
fi

found=0
for name in "${REMOVED[@]}"; do
    if grep -q -F "$name" <<<"$subjects"; then
        echo "FAIL: removed root still trusted: $name"
        found=1
    else
        echo "ok: $name not trusted"
    fi
done
exit "$found"
