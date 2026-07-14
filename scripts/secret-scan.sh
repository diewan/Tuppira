#!/usr/bin/env bash
# Reject committed private keys, credential-bearing URLs, and literal API
# credentials. Environment-variable references are permitted in examples.
set -euo pipefail

root="${SECRET_SCAN_ROOT:-$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)}"
scan_root="$root"

if [[ "${1:-}" == "--self-test" ]]; then
  scan_root="$(mktemp -d)"
  trap 'rm -rf "$scan_root"' EXIT
  printf 'PRIVATE_KEY=0x' > "$scan_root/seeded-secret.env"
  printf '0%.0s' {1..64} >> "$scan_root/seeded-secret.env"
  printf '\n' >> "$scan_root/seeded-secret.env"
  if SECRET_SCAN_ROOT="$scan_root" "$0"; then
    printf '%s\n' 'secret scanner accepted its seeded credential' >&2
    exit 1
  fi
  exit 0
fi

matches="$(find "$scan_root" \
  -type d \( -name .git -o -name target -o -name node_modules -o -name dist \) -prune -o \
  -type f -print0 \
  | xargs -0 -r grep -nEI \
    '(https?://[^/@[:space:]]+:[^/@[:space:]]+@|(^|[^A-Z0-9_])(API[_-]?KEY|PRIVATE[_-]?KEY|SECRET|ACCESS[_-]?TOKEN)[[:space:]]*[:=][[:space:]]*[[:alnum:]][[:alnum:]._-]{7,})' \
  || true)"

if [[ -n "$matches" ]]; then
  printf '%s\n%s\n' 'credential-shaped content detected:' "$matches" >&2
  exit 1
fi
