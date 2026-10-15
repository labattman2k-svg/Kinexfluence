#!/usr/bin/env sh
#
# Redact personal data from a provider export before committing it as a
# fixture. Named by docs/STANDARDS.md section 1, which requires that all
# fixtures be sanitized.
#
# This is BEST EFFORT, not a guarantee. It catches the shapes that are easy
# to recognise mechanically. It cannot recognise a person's name in free
# prose, and it will not catch a secret in an unusual format. Always read the
# output before committing it.
#
# Usage:
#   scripts/sanitize-fixture.sh <export.json>
#
# Writes <export.json>.sanitized and prints a per-rule summary. The input is
# never modified.

set -eu

usage() {
    cat <<'EOF'
Redact personal data from a provider export.

Usage:
  sanitize-fixture.sh <export.json>
  sanitize-fixture.sh --help

Reads <export.json>, writes <export.json>.sanitized, and prints how many
replacements each rule made. The input file is never modified.

Rules applied:
  email        addresses              -> user@example.com
  url          http(s) links          -> https://example.com/redacted
  api-key      sk-... / ghp_... / AIza... style keys -> REDACTED
  bearer       Bearer <token>         -> Bearer REDACTED
  named-field  "name": "..."          -> "name": "User"
  auth-field   "access_token": "..."  -> "access_token": "REDACTED"
  home-path    /Users/... and C:\Users\... -> /Users/user

Best effort only. Review the output before committing it.
EOF
}

if [ "${1:-}" = "-h" ] || [ "${1:-}" = "--help" ]; then
    usage
    exit 0
fi

if [ $# -ne 1 ]; then
    usage >&2
    exit 2
fi

input=$1

if [ ! -f "$input" ]; then
    printf 'error: no such file: %s\n' "$input" >&2
    exit 1
fi

output="${input}.sanitized"
work=$(mktemp)
next=$(mktemp)
trap 'rm -f "$work" "$next"' EXIT INT TERM

cp "$input" "$work"

total=0

# apply <label> <extended-regex> <replacement>
#
# Counts matches, then rewrites. Uses a plain temp file rather than `sed -i`
# because GNU and BSD sed disagree about in-place editing.
#
# The sed delimiter is `^`. It cannot be `/`, `|`, `#`, `@`, `%`, `,`, `;`
# or `~`, because the url and api-key patterns contain all of those. No
# pattern or replacement here contains `^`, so it is the one safe choice.
apply() {
    label=$1
    pattern=$2
    replacement=$3

    count=$(grep -E -o "$pattern" "$work" 2>/dev/null | wc -l | tr -d ' ')
    if [ "$count" -gt 0 ]; then
        sed -E "s^$pattern^$replacement^g" "$work" > "$next"
        mv "$next" "$work"
    fi
    total=$((total + count))
    printf '  %-12s %s\n' "$label" "$count"
}

printf 'sanitizing %s\n\n' "$input"
printf '  %-12s %s\n' "rule" "replacements"

apply "email" \
    '[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}' \
    'user@example.com'

apply "url" \
    'https?://[A-Za-z0-9._~:/?#@!$&*+,;=%-]+' \
    'https://example.com/redacted'

apply "api-key" \
    '(sk|ghp|gho|AIza|xox[baprs])[-_][A-Za-z0-9_-]{16,}' \
    'REDACTED'

apply "bearer" \
    'Bearer[[:space:]]+[A-Za-z0-9._~+/-]{16,}=*' \
    'Bearer REDACTED'

apply "named-field" \
    '"name"[[:space:]]*:[[:space:]]*"[^"]*"' \
    '"name": "User"'

apply "auth-field" \
    '"(access_token|refresh_token|id_token|api_key|client_secret)"[[:space:]]*:[[:space:]]*"[^"]*"' \
    '"\1": "REDACTED"'

apply "home-path" \
    '(/Users|/home)/[A-Za-z0-9._-]+' \
    '/Users/user'

apply "win-path" \
    '[A-Za-z]:\\\\Users\\\\[A-Za-z0-9._-]+' \
    'C:\\\\Users\\\\user'

cp "$work" "$output"

printf '\n%s replacements written to %s\n' "$total" "$output"
printf '\nThis is best effort. Read %s before committing it:\n' "$output"
printf 'a name in prose, or a secret in an unusual format, will not have been caught.\n'
