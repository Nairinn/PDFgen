#!/bin/bash
# Build the UniFFI cdylib, generate Kotlin bindings, compile and run the
# smoke test. Requires: cargo, kotlinc, the JNA jar (tools/jvm/jna.jar).
set -euo pipefail
cd "$(dirname "$0")/../.."

echo "== building pdfgen-uniffi (cdylib) =="
cargo build -p pdfgen-uniffi

echo "== generating Kotlin bindings =="
uniffi-bindgen generate \
    --format kotlin \
    --config crates/pdfgen-uniffi/uniffi.toml \
    --out-dir bindings/kotlin/generated \
    target/debug/libpdfgen_uniffi.dylib

echo "== compiling Kotlin smoke test =="
cd bindings/kotlin
kotlinc TestBinding.kt generated/pdfgen.kt -cp ../../tools/jvm/jna.jar -include-runtime -d test.jar

echo "== running =="
java -cp "test.jar:../../tools/jvm/jna.jar:../../target/debug" \
     -Djna.library.path="../../target/debug" \
     TestBindingKt
