#!/usr/bin/env bash
# Publish every crate to crates.io in dependency order, then build the
# Python wheel for PyPI. Requires: cargo login (crates.io token) and
# maturin for the wheel.
#
# crates.io publish order matters: every crate's internal dependencies
# must already be on crates.io when its turn comes.
set -euo pipefail
cd "$(dirname "$0")/.."

CRATES=(
    pdfgen-core
    pdfgen-font
    pdfgen-parse
    pdfgen-canvas
    pdfgen-profile
    pdfgen-fonts
    pdfgen-validate
    pdfgen-revision
    pdfgen-draw
    pdfgen-render
    pdfgen
    pdfgen-api
)

if [ "${1:-}" = "--dry-run" ]; then
    MODE="--dry-run"
    echo "== dry run: packing order check only =="
else
    MODE=""
    if ! cargo whoami >/dev/null 2>&1; then
        echo "Not logged in to crates.io. Run: cargo login <token>" >&2
        exit 2
    fi
fi

for c in "${CRATES[@]}"; do
    echo "== $c =="
    # --allow-dirty: the workspace is one tree; sibling crates are not
    # "dirty" after the first publishes.
    cargo publish -p "$c" --allow-dirty $MODE
done

echo "== Python wheel (PyPI) =="
maturin build -r -o bindings/python/dist

echo
echo "Done. To upload the wheel:  twine upload bindings/python/dist/*.whl"
echo "(API crates pdfgen-py/pdfgen-uniffi/pdfgen-ffi publish per-platform separately.)"
