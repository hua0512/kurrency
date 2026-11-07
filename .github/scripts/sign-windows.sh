#!/usr/bin/env bash
# Authenticode-signs files with the PFX certificate in $WINDOWS_CERTIFICATE
# (base64) and its password in $WINDOWS_CERTIFICATE_PASSWORD, using the newest
# signtool from the Windows SDK on GitHub's Windows image.
#
# usage: .github/scripts/sign-windows.sh <file>...
set -euo pipefail

: "${WINDOWS_CERTIFICATE:?}" "${WINDOWS_CERTIFICATE_PASSWORD:?}"
timestamp_url=${WINDOWS_TIMESTAMP_URL:-http://timestamp.digicert.com}

signtool=$(printf '%s\n' "/c/Program Files (x86)/Windows Kits/10/bin/"*/x64/signtool.exe | sort -V | tail -n 1)
[ -x "$signtool" ] || { echo "sign-windows: signtool.exe not found" >&2; exit 1; }
pfx="$RUNNER_TEMP/kurrency-signing.pfx"
trap 'rm -f "$pfx"' EXIT
echo "$WINDOWS_CERTIFICATE" | base64 --decode > "$pfx"

for file in "$@"; do
  "$signtool" sign -f "$(cygpath -w "$pfx")" -p "$WINDOWS_CERTIFICATE_PASSWORD" \
    -fd sha256 -tr "$timestamp_url" -td sha256 -d Kurrency "$(cygpath -w "$file")"
  "$signtool" verify -pa "$(cygpath -w "$file")"
done
