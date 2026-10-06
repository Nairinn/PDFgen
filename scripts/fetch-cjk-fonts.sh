#!/usr/bin/env bash
# Fetch the optional CJK fonts (not committed: 32 MB together) from the
# notofonts releases. Idempotent - safe to run repeatedly, and the CI
# runners call it before the test suite. The CJK test skips when the
# font is absent.
set -euo pipefail

DIR="$(cd "$(dirname "$0")/.." && pwd)/fonts/vendor/noto"
mkdir -p "$DIR"

fetch() {
    local name="$1"
    local out="$DIR/$name"
    if [ -f "$out" ]; then
        echo "have: $name"
        return 0
    fi
    echo "fetching: $name"
    curl -fsSL -o "$out.tmp" \
        "https://github.com/notofonts/noto-cjk/raw/main/Sans/OTF/Japanese/$name"
    mv "$out.tmp" "$out"
}

fetch NotoSansCJKjp-Regular.otf
fetch NotoSansCJKjp-Bold.otf
echo "CJK fonts ready."
