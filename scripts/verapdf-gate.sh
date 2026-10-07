#!/usr/bin/env bash
# veraPDF gate: validate every generated fixture against its profile.
# Used by CI. The name encodes the profile: *_ua2.pdf -> -f ua2,
# everything else -> -f ua1. Files named *_noncompliant* and probe
# artifacts are excluded by design (they assert the never-blocks report).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
OUT="$ROOT/tests/output"
VERA="$ROOT/tools/verapdf/verapdf"

if [ ! -x "$VERA" ]; then
    echo "veraPDF not found at $VERA" >&2
    exit 2
fi

fail=0
count=0
for pdf in "$OUT"/*.pdf; do
    base="$(basename "$pdf")"
    case "$base" in
        *noncompliant*|*probe*|*headerless*|*unnamed*|fill_field*|*broken*|*truncated*|*fixture*|untagged*|robust_base*|render_hex_tj*|render_source*)
            # Deliberately non-compliant, minimal, or incrementally-updated
            # fixtures: they assert behavior veraPDF is not the judge of
            # (parser round-trips, renderer probes, retag inputs).
            continue
            ;;
    esac
    if [[ "$base" == *_ua2.pdf ]]; then
        flag="ua2"
    else
        flag="ua1"
    fi
    count=$((count + 1))
    if ! result=$("$VERA" -f "$flag" "$pdf" 2>&1); then
        echo "veraPDF could not process $base" >&2
        fail=1
        continue
    fi
    if ! grep -q 'isCompliant="true"' <<<"$result"; then
        echo "FAIL: $base ($flag)" >&2
        grep -E 'isCompliant|failedChecks|clause=' <<<"$result" | head -5 >&2
        fail=1
    else
        checks=$(grep -oE 'failedChecks="[0-9]+"' <<<"$result" | head -1)
        echo "ok: $base ($flag) $checks"
    fi
done

echo "----"
echo "validated $count files"
if [ "$fail" -ne 0 ]; then
    echo "veraPDF gate FAILED" >&2
    exit 1
fi
echo "veraPDF gate passed"
