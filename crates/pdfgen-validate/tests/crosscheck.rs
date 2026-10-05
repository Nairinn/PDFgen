//! Cross-check our validator against veraPDF's verdicts on our fixture
//! files: known-compliant docs must be clean; known-broken ones must
//! produce findings.

use pdfgen_validate::validate;

fn out(name: &str) -> String {
    format!("{}/../../tests/output/{name}", env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn compliant_docs_are_clean() {
    for name in ["hello_ua1.pdf", "hello_ua2.pdf", "kitchensink_ua1.pdf", "form_ua1.pdf"] {
        let report = validate(out(name)).expect(name);
        assert!(
            report.is_clean(),
            "{name}: unexpected findings: {:?}",
            report
                .findings
                .iter()
                .map(|f| format!("{} {}", f.id, f.message))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn noncompliant_doc_produces_findings() {
    // hello_noncompliant.pdf was saved with no title/lang: it must not
    // claim PDF/UA, and our checker must catch why.
    let report = validate(out("hello_noncompliant.pdf")).expect("parse");
    assert!(!report.is_clean(), "must have findings");
    assert!(
        report.findings.iter().any(|f| f.id == "06-002"),
        "missing PDF/UA identifier must be reported: {:?}",
        report.findings
    );
    assert!(
        report.findings.iter().any(|f| f.id == "11-001"),
        "missing language must be reported: {:?}",
        report.findings
    );
}

#[test]
fn untagged_pdf_reports_struct_failures() {
    let report = validate(out("untagged.pdf")).expect("parse");
    assert!(report.findings.iter().any(|f| f.id == "01-003"));
    assert!(report.findings.iter().any(|f| f.id == "01-002"));
}
