#!/usr/bin/env bash
# veraPDF gate: validate every generated fixture against its profile.
# The filename encodes the profile: *_ua2.pdf -> -f ua2, else -f ua1.
#
# Skips come from tests/verapdf-skip.txt (filename | reason). The gate
# FAILS on a manifest entry whose file doesn't exist (stale entry) and
# prints the validated vs skipped counts. POSIX sh compatible (no
# associative arrays; the manifest is small).
set -eu

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/tests/output"
VERA="$ROOT/tools/verapdf/verapdf"
SKIP="$ROOT/tests/verapdf-skip.txt"

if [ ! -x "$VERA" ]; then
    echo "veraPDF not found at $VERA" >&2
    exit 2
fi

# Fail on stale manifest entries before validating anything.
while IFS='|' read -r name _reason; do
    name="$(echo "$name" | tr -d '[:space:]')"
    [ -z "$name" ] && continue
    case "$name" in '#'*) continue ;; esac
    if [ ! -f "$OUT/$name" ]; then
        echo "STALE SKIP ENTRY: $name is listed in verapdf-skip.txt but does not exist" >&2
        exit 1
    fi
done < "$SKIP"

fail=0
count=0
skipped=0
for pdf in "$OUT"/*.pdf; do
    base="$(basename "$pdf")"
    reason="$(grep -E "^${base}[[:space:]]*\|" "$SKIP" | head -1 | cut -d'|' -f2 | sed 's/^[[:space:]]*//')"
    if [ -n "$reason" ]; then
        skipped=$((skipped + 1))
        echo "skip: $base - $reason"
        continue
    fi
    case "$base" in
    *_ua2.pdf) flag="ua2" ;;
    *) flag="ua1" ;;
    esac
    count=$((count + 1))
    if ! result=$("$VERA" -f "$flag" "$pdf" 2>&1); then
        echo "veraPDF could not process $base" >&2
        fail=1
        continue
    fi
    if ! printf '%s' "$result" | grep -q 'isCompliant="true"'; then
        echo "FAIL: $base ($flag)" >&2
        printf '%s\n' "$result" | grep -E 'isCompliant|failedChecks|clause=' | head -5 >&2
        fail=1
    else
        checks=$(printf '%s' "$result" | grep -oE 'failedChecks="[0-9]+"' | head -1)
        echo "ok: $base ($flag) $checks"
    fi
done

echo "----"
echo "validated $count files, skipped $skipped"
if [ "$fail" -ne 0 ]; then
    echo "veraPDF gate FAILED" >&2
    exit 1
fi
echo "veraPDF gate passed"
