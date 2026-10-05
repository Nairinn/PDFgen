# PDFgen

A free, MIT-licensed PDF library written in Rust from scratch — no iText, no
PDFBox, no AGPL, no copyleft. Built to make **PDF/UA accessibility the default**
and **PDF versioning** something you don't have to think about.

```rust
let mut doc = pdfgen::Document::new(pdfgen::Profile::PdfUa1);
doc.title("Quarterly Report").lang("en-US");

let arial = doc.font("Arial", "Bold")?;          // system font by name
let mut flow = doc.flow();
flow.heading(1, "Quarterly Report")?;
flow.paragraph_in(arial, 14.0, "Revenue is up.")?;
flow.bullet_list(&["Costs are flat", "Margins grew"])?;
flow.table(
    &["Pin", "Signal", "Color"],                   // header row (TH + Scope)
    &[vec!["1", "VCC", "Red"], vec!["2", "GND", "Black"]],
    &[60.0, 120.0, 240.0],
)?;

let report = doc.save("report.pdf")?;              // ALWAYS writes the file
```

## Why

iText is AGPL (or very expensive). PDFBox is Apache-2.0 but Java-only and its
accessibility support is partial. Neither makes accessible output easy, and
neither treats a PDF as a versioned document. PDFgen is a clean-room
reimplementation from the ISO specifications with different priorities:

- **Accessibility guides you, it never blocks you.** Every save writes the
  file and returns a report. If the document isn't compliant yet you get a
  plain note — `⚠ not PDF/UA compliant yet: 2 issues (13-004 Figure has no
  alt text, …)` — with Matterhorn IDs and fixes. The file simply carries no
  conformance claim until it actually passes.
- **Both PDF/UA-1 and PDF/UA-2**, end to end, validated against veraPDF.
- **Version control for PDFs**: revisions, history, diff, revert (in progress).
- **Fonts by name.** Call `doc.font("Arial", "Bold")` and get the real Arial
  from your system — licensed fonts you own work; the built-in catalog
  ships free look-alikes (Liberation = Helvetica/Times/Courier metrics) so
  documents work everywhere.
- **Read and retag existing PDFs.** Open any file — even untagged ones —
  tag it, and save it fully compliant.

## Status

Everything below is **verified by veraPDF 1.30.2**, not self-graded:

| Area | What works today |
|---|---|
| Writer | Tagged PDF/UA-1 + PDF/UA-2 output; structure tree; embedded TrueType fonts; XMP metadata |
| Layout | Word wrap with real font metrics, automatic page breaks, paragraphs that continue across pages as one element |
| Content | Headings, paragraphs, bullet lists (`L/LI/Lbl/LBody`), tables (`Table/TR/TH` with `Scope`/`TD`), figures with alt text, PNG + JPEG images, header/footer artifacts |
| Fonts | By-name registry: built-in catalog, system fonts (recursive scan), standard-14 aliases, user-registered files, substitution notes |
| Reader | Classic + xref-stream + hybrid xref, lazy resolution, repair mode for broken files |
| Retag | Import any PDF, extract text, auto/manual tagging, save compliant |
| Reports | Machine checks with Matterhorn IDs + a human-review checklist on every save |

Not yet: revision history/diff, forms, encryption, CID/complex-script shaping,
bindings (Kotlin/Java/Python), the engineering-drawing kit. See
[`docs/PLAN.md`](docs/PLAN.md) for the full roadmap.

## Layout

```
crates/
  pdfgen-core      PDF object model, serializer, xref writer
  pdfgen-parse     reader: lexer, xref (classic/stream/hybrid), repair mode
  pdfgen-font      font loading (ttf-parser), WinAnsi, embedding permissions
  pdfgen-fonts     registry: built-in catalog, system fonts, name resolution
  pdfgen-canvas    content streams, marked content (BDC/EMC), artifacts
  pdfgen-profile   PDF/UA-1 + UA-2 profiles, XMP, the save report
  pdfgen           the public API: Document, Flow, TagSession
fonts/vendor/      bundled OFL fonts (Liberation family)
tests/output/      generated PDFs (gitignored) — all veraPDF-validated
tools/verapdf/     local veraPDF install used as the external checker
```

## Development

```bash
cargo test                     # generates tests/output/*.pdf and checks them
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
tools/verapdf/verapdf -f ua1 tests/output/hello_ua1.pdf
tools/verapdf/verapdf -f ua2 tests/output/hello_ua2.pdf
```

Pass = `isCompliant="true"`, `failedChecks="0"`.

- **Commits**: Conventional Commits (`feat(writer): …`), enforced by review.
- **License hygiene**: `deny.toml` allow-lists only MIT/Apache/BSD/ISC/Zlib
  dependencies; vendored fonts are OFL-1.1 with their license files.
- **veraPDF is a test tool only** — never linked, never bundled in output.

## License

MIT — see [LICENSE](LICENSE). Vendored fonts keep their own licenses in
`fonts/vendor/*/LICENSE` (SIL OFL 1.1).

PDF/UA-1 (ISO 14289-1), PDF/UA-2 (ISO 14289-2) and PDF 2.0 (ISO 32000-2) are
freely available from the [PDF Association](https://pdfa.org/sponsored-standards/);
this project implements from those specifications, not from iText or PDFBox
source code.
