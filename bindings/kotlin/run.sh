#!/bin/bash
# Build the UniFFI cdylib, generate Kotlin bindings, compile and run the
# smoke test. Requires: cargo, uniffi-bindgen 0.32.x, kotlinc, JNA jar.
set -euo pipefail
cd "$(dirname "$0")/../.."

echo "== building pdfgen-uniffi (cdylib) =="
cargo build -p pdfgen-uniffi

echo "== generating Kotlin bindings =="
mkdir -p bindings/kotlin/generated
uniffi-bindgen generate \
    --language kotlin \
    --config crates/pdfgen-uniffi/uniffi.toml \
    --out-dir bindings/kotlin/generated \
    target/debug/libpdfgen_uniffi.dylib

echo "== compiling Kotlin smoke test =="
cd bindings/kotlin
kotlinc TestBinding.kt generated/uniffi/pdfgen_uniffi/pdfgen_uniffi.kt -cp ../../tools/jvm/jna.jar -include-runtime -d test.jar

echo "== running =="
java -cp "test.jar:../../tools/jvm/jna.jar" \
     -Djna.library.path="../../target/debug" \
     TestBindingKt
