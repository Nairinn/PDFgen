#!/usr/bin/env bash
# Build the Rust cdylib and run the Java 22+ FFM binding test.
set -euo pipefail
cd "$(dirname "$0")/../.."

JAVA22=${JAVA22:-$(/usr/libexec/java_home -v 23 2>/dev/null || /usr/libexec/java_home -v 22 2>/dev/null || /usr/libexec/java_home -v 21)}
echo "using JDK: $JAVA22"

echo "building libpdfloom_ffi (dylib)..."
cargo build -p pdfloom-ffi -q
LIB="target/debug/libpdfloom_ffi.dylib"
if [ ! -f "$LIB" ]; then
  # Linux naming
  LIB="target/debug/libpdfloom_ffi.so"
fi
echo "lib: $LIB"

BIND=bindings/java
mkdir -p "$BIND/classes"
"$JAVA22/bin/javac" -d "$BIND/classes" "$BIND/PdfLoom.java" "$BIND/TestFfm.java"

mkdir -p tests/output
"$JAVA22/bin/java" --enable-native-access=ALL-UNNAMED -cp "$BIND/classes" TestFfm "$(pwd)/$LIB" "tests/output/java_ffm.pdf"

echo "done"
