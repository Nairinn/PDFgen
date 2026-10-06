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

For massive documents, the streaming writer flushes pages to disk as they
close — flat memory at any scale:

```rust
let mut w = pdfgen::StreamWriter::create("mega.pdf", Profile::PdfUa1,
                                         "Mega Report", "en-US")?;
w.push(vec![
    StreamEvent::Begin { tag: "P".into(), alt: None },
    StreamEvent::Text { text: chunk_of_body_text, font: 0, size: 11.0 },
    StreamEvent::End,
])?;                                              // large batches
let report = w.finish()?;                          // pages already on disk
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
- **Streaming, like iText: flat memory at any scale.** Completed pages are
  flushed to the output file the moment they close — content streams
  Flate-compressed — instead of holding the document tree in memory. The
  layout loop is optimized the same way: each word's width is measured once
  and packed greedily (O(n), no candidate re-measurement), operators go
  through reusable buffers, and object ids are preallocated so pages never
  need patching. This is the path for high-volume and massive documents.
- **Fonts by name.** Call `doc.font("Arial", "Bold")` and get the real Arial
  from your system — licensed fonts you own work; the built-in catalog ships
  free look-alikes (Liberation = Helvetica/Times/Courier metrics) so
  documents work everywhere.
- **Read and retag existing PDFs.** Open any file — even untagged ones —
  tag it, and save it fully compliant.
- **Rust, Python and Kotlin** from one core (Java works via the same JNA jar).

## What's done — verified by veraPDF 1.30.2, not self-graded

| Area | What works today |
|---|---|
| Writer (in-memory) | Tagged PDF/UA-1 + PDF/UA-2 output; structure tree; embedded TrueType; XMP |
| Writer (streaming) | Event-oriented chunk API; pages flush to disk as they close; Flate-compressed content; O(n) incremental wrap; 500-section doc → **91,931/91,931 checks** |
| Revisions | `commit`/`history`/`diff`/`revert` inside the file itself: appended incremental saves with message + author, byte-exact restore, object-level diff — a 3-revision file passes veraPDF 198/198 |
| Validator + CLI | Matterhorn machine checks with checkpoint IDs (`pdfgen validate`), cross-checked against veraPDF verdicts on every fixture |
| Forms | Interactive AcroForm text fields with `/TU` accessible names, widget annotations, and incremental-save `fill_text_field` |
| Outline | Bookmarks generated from headings, nested by level |
| Extraction | `extract_text` pulls text blocks from any PDF, tagged or not, Flate-compressed included |
| HTML-to-PDF | Structural HTML subset → streamed tagged PDF/UA (headings, lists, tables, images with alt) |
| Drawing kit | ASME Y14 sheets A–F, tagged title-block table, Y14.5 dimensions, Y14.35 revision block wired to file history — bracket drawing passes veraPDF **942/942** |
| Layout | Word wrap with real font metrics, page breaks, cross-page paragraphs (MCR), keep-with-next |
| Content | Headings, paragraphs, bullet lists (`L/LI/Lbl/LBody`), tables (`Table/TR/TH` with `Scope`/`TD`), figures with alt text, PNG + JPEG, header/footer artifacts |
| Fonts | By-name registry: built-in catalog (Liberation + Noto incl. Myanmar/Thai/Arabic/CJK JP + accessibility faces, OFL), system fonts (recursive scan), standard-14 aliases, user-registered files, TrueType CID subsetting, substitution notes |
| Reader | Classic + xref-stream + hybrid xref, lazy resolution, repair mode for broken files |
| Retag | Import any PDF, extract text, auto/manual tagging, save compliant |
| Reports | Machine checks with Matterhorn IDs + human-review checklist on every save |
| Bindings | Python (PyO3, abi3 ≥ 3.9, `PdfUaWarning` on non-compliant saves), Kotlin/Java (UniFFI + JNA, Java 11+), and Java 22+ (`java.lang.foreign` FFM, no JNI/JNA) — all generate veraPDF-valid PDFs |
| CID fonts | Text WinAnsi can't encode flows through Type0 Identity-H composite fonts automatically (Myanmar, Korean, Greek, CJK); TTC collections sliced to standalone programs |

## Optional CJK fonts

The two Noto Sans CJK JP faces (16 MB each) are **not committed** — clone
size stays small. Fetch them when needed:

```bash
bash scripts/fetch-cjk-fonts.sh
```

CI fetches them automatically; the CJK test skips when they are absent.

## Fonts in packaged installs

The bundled font files (OFL) live in `fonts/vendor/` at the repo root — outside any crate directory, so **crate packages never ship the fonts** (crates.io caps packages at 10 MB; the CJK faces alone are 16 MB each). Resolution order at runtime:

1. `PDFGEN_FONTS_DIR` (build-time env var, relocated installs)
2. The repo checkout (`fonts/` relative to the workspace)
3. System fonts by family name (licensed fonts you own work here)
4. A bundled-catalog look-alike (with a substitution note in the save report)
5. Any system sans-serif, flagged as substituted — the save always succeeds

So a `cargo add pdfgen`-style install degrades gracefully: documents still build with system fonts and the report shows what was substituted. Clone the repo (or point `PDFGEN_FONTS_DIR` at our `fonts/`) to use the full bundled catalog.

## What's coming next

- ~~More Matterhorn checks~~ (28-001/28-002 form-field checks added; negative-tested)
- ~~CID subsetting~~ (done: TrueType CID fonts subset to used glyphs via glyf/loca surgery; CFF embeds stay whole-font for now)
- ~~CJK fonts~~ (done: Noto Sans CJK JP cataloged, FontFile3/ CIDFontType0 embedding)
- ~~Accessibility fonts~~ (done: Atkinson Hyperlegible + OpenDyslexic cataloged, veraPDF-verified)
- ~~Rendering~~ (done: `pdfgen render` / `pdfgen print`, embedded-font glyph rasterizer)
- Maven Central, PyPI and crates.io publishing
- Full plan with milestones: [`docs/PLAN.md`](docs/PLAN.md)

## Layout

```
crates/
  pdfgen-core      PDF object model, serializer, xref writer
  pdfgen-parse     reader: lexer, xref (classic/stream/hybrid), repair mode
  pdfgen-font      font loading (ttf-parser), WinAnsi, embedding permissions
  pdfgen-fonts     registry: built-in catalog, system fonts, name resolution
  pdfgen-canvas    content streams, marked content (BDC/EMC), artifacts
  pdfgen-profile   PDF/UA-1 + UA-2 profiles, XMP, the save report
  pdfgen           the public API: Document, Flow, StreamWriter, TagSession,
                   extract_text, HTML-to-PDF, forms, bookmarks
  pdfgen-api       bindings-friendly facade (owned types, no lifetimes)
  pdfgen-py        Python bindings (PyO3)
  pdfgen-uniffi    Kotlin/Java bindings (UniFFI)
  pdfgen-validate  Matterhorn machine checks (the `pdfgen validate` engine)
  pdfgen-revision  commit / history / diff / revert inside the PDF
  pdfgen-draw      ASME Y14 + ISO 5457 drawing kit (sheets, title block,
                  dimensions, revision block)
  pdfgen-ffi       C-ABI layer (cdylib) consumed by the Java FFM binding
  pdfgen-cli       the `pdfgen` command-line tool
fonts/vendor/      bundled OFL fonts (Liberation, Noto incl. Myanmar/Thai/Arabic)
bindings/          kotlin (UniFFI/JNA), java (FFM), python (PyO3 wheel)
tests/output/      generated PDFs (gitignored) — all veraPDF-validated
tools/verapdf/     local veraPDF install used as the external checker
```

## Development

```bash
cargo test                     # generates tests/output/*.pdf and checks them
cargo run -p pdfgen-cli -- validate tests/output/hello_ua1.pdf   # our checker
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
tools/verapdf/verapdf -f ua1 tests/output/hello_ua1.pdf           # external gate
tools/verapdf/verapdf -f ua2 tests/output/hello_ua2.pdf
```

Pass = `isCompliant="true"`, `failedChecks="0"`.

- **Commits**: Conventional Commits (`feat(writer): …`).
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
