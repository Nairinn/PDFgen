"""End-to-end Python binding test: build the same kitchen-sink document as
the Rust tests and verify the report + warning behavior."""

import pathlib
import warnings

import pdfgen

OUT = pathlib.Path(__file__).resolve().parents[2] / "tests" / "output"
OUT.mkdir(parents=True, exist_ok=True)

# --- 1. Compliant document, system font by name -------------------------
doc = pdfgen.Document(pdfgen.Profile.UA1, title="Python bindings test", lang="en-US")
arial = doc.font("Arial", "Bold")
doc.heading(1, "Hello from Python")
doc.paragraph_in(arial, 14.0, "This paragraph uses system Arial Bold, resolved by name.")
doc.bullet_list(["First item", "Second item"])
doc.table(
    ["Pin", "Signal", "Color"],
    [["1", "VCC", "Red"], ["2", "GND", "Black"]],
    [60.0, 120.0, 240.0],
)
doc.figure(
    str(pathlib.Path(__file__).resolve().parents[2] / "tests" / "fixtures" / "red_box.png"),
    "A solid red square",
    72.0,
    72.0,
)
report = doc.save(str(OUT / "python_binding_ua1.pdf"))

assert report.compliant, f"expected compliant, got: {report.violations}"
assert report.note == "", f"compliant report has no note, got {report.note!r}"
print(f"python: compliant doc OK ({report.profile_name})")

# --- 2. Non-compliant document: still saves, warns, no claim -------------
with warnings.catch_warnings(record=True) as caught:
    warnings.simplefilter("always")
    doc2 = pdfgen.Document(pdfgen.Profile.UA1)  # no title, no lang
    doc2.paragraph("Untitled.")
    report2 = doc2.save(str(OUT / "python_noncompliant.pdf"))

assert not report2.compliant
assert report2.note and "not PDF/UA-1 compliant yet" in report2.note, report2.note
ua_warnings = [w for w in caught if issubclass(w.category, pdfgen.PdfUaWarning)]
assert ua_warnings, "expected a PdfUaWarning"
print(f"python: non-compliant doc warns OK ({ua_warnings[0].message})")

# --- 3. UA-2 profile round trip -------------------------------------------
doc3 = pdfgen.Document(pdfgen.Profile.UA2, title="Python UA2", lang="en-US")
doc3.heading(1, "PDF/UA-2 from Python")
doc3.paragraph("Modern profile, same API.")
report3 = doc3.save(str(OUT / "python_binding_ua2.pdf"))
assert report3.compliant, report3.violations
print(f"python: UA-2 OK ({report3.profile_name})")

print("PYTHON BINDINGS: ALL PASS")
