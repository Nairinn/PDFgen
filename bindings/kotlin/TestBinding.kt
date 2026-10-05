// End-to-end Kotlin binding test: same kitchen-sink document as the Rust
// and Python tests, verified by the returned report.
//
// Run: see bindings/kotlin/run.sh — compiles against the generated
// pdfgen_uniffi.kt + the Rust cdylib via JNA.

import uniffi.pdfgen_uniffi.*

fun main() {
    val outDir = java.nio.file.Paths.get("../..", "tests", "output").toAbsolutePath().normalize()
    java.nio.file.Files.createDirectories(outDir)

    // --- 1. Compliant document, system font by name -----------------------
    val doc = Document.newWith(Profile.UA1, "Kotlin bindings test", "en-US")
    val arial = doc.font("Arial", "Bold")
    doc.heading(1u, "Hello from Kotlin")
    doc.paragraphIn(arial, 14.0, "This paragraph uses system Arial Bold, resolved by name.")
    doc.bulletList(listOf("First item", "Second item"))
    doc.table(
        listOf("Pin", "Signal", "Color"),
        listOf(
            listOf("1", "VCC", "Red"),
            listOf("2", "GND", "Black"),
        ),
        listOf(60.0, 120.0, 240.0),
    )
    val fixture = java.nio.file.Paths.get("../..", "tests", "fixtures", "red_box.png")
        .toAbsolutePath().normalize().toString()
    doc.figure(fixture, "A solid red square", 72.0, 72.0)

    val report = doc.save(outDir.resolve("kotlin_binding_ua1.pdf").toString())
    check(report.compliant) { "expected compliant, got: ${report.violations}" }
    check(report.note.isEmpty()) { "compliant report has no note" }
    println("kotlin: compliant doc OK (${report.profileName})")

    // --- 2. Non-compliant: still saves, note is present ------------------
    val doc2 = Document(Profile.UA1)   // no title, no lang
    doc2.paragraph("Untitled.")
    val report2 = doc2.save(outDir.resolve("kotlin_noncompliant.pdf").toString())
    check(!report2.compliant)
    check(report2.note.contains("not PDF/UA-1 compliant yet")) { report2.note }
    println("kotlin: non-compliant doc note OK (${report2.note})")

    // --- 3. UA-2 profile --------------------------------------------------
    val doc3 = Document.newWith(Profile.UA2, "Kotlin UA2", "en-US")
    doc3.heading(1u, "PDF/UA-2 from Kotlin")
    doc3.paragraph("Modern profile, same API.")
    val report3 = doc3.save(outDir.resolve("kotlin_binding_ua2.pdf").toString())
    check(report3.compliant) { "UA2 violations: ${report3.violations}" }
    println("kotlin: UA-2 OK (${report3.profileName})")

    println("KOTLIN BINDINGS: ALL PASS")
}
