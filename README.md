# PDFgen

A PDF library in Rust, MIT licensed, written from the ISO specifications.
It exists because the established free options have drawbacks: iText is
AGPL (or commercially licensed), and PDFBox is Java-only. PDFgen aims to
cover the same ground with PDF/UA accessibility and in-file versioning
built in.

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

For large documents, the streaming writer flushes completed pages to
disk instead of holding the whole document in memory:

```rust
let mut w = pdfgen::StreamWriter::create("big.pdf", Profile::PdfUa1,
                                         "Big Report", "en-US")?;
w.push(vec![
    StreamEvent::Begin { tag: "P".into(), alt: None, attrs: None },
    StreamEvent::Text { text: chunk_of_body_text, font: 0, size: 11.0 },
    StreamEvent::End,
])?;                                              // large batches
let report = w.finish()?;                          // pages already on disk
```

## Why

Saving never blocks on accessibility. `save()` always writes the file and
returns a report. If the document isn't compliant yet, the report says
what's wrong (Matterhorn checkpoint IDs and fixes) and the file simply
carries no PDF/UA conformance claim. You can see the output and fix the
report at your own pace.

Revisions live in the file itself. You can commit, list history, diff
and revert a PDF without any external VCS, using appended incremental
saves. The revision block of an engineering drawing can be wired
directly to that history.

Both PDF/UA-1 and PDF/UA-2 are implemented end to end, and complex
scripts (Arabic, Myanmar, Thai) are shaped with HarfBuzz plus Unicode
bidi, so they render correctly and extract back as the text you wrote.

Verification is reproducible. CI installs veraPDF 1.30.2 and validates
every generated fixture against the profile it claims, failing on any
check.

## What works

| Area | Status |
|---|---|
| Writers | Tagged PDF/UA-1 + UA-2 output, in-memory or streaming; structure trees, XMP, embedded TrueType |
| Streaming | Event API; completed pages flush to disk; a 500-section document passes 91,931/91,931 veraPDF checks |
| Revisions | `commit` / `history` / `diff` / `revert` inside the file, byte-exact restore, object-level diff |
| Layout | Word wrap with real metrics, page breaks, cross-page paragraphs, keep-with-next |
| Content | Headings, paragraphs, lists, tables (TH with Scope), figures with alt, PNG + JPEG, header/footer artifacts |
| Fonts | Registry by name: bundled catalog (Liberation, Noto incl. Myanmar/Thai/Arabic, Atkinson Hyperlegible, OpenDyslexic), system fonts, standard-14 aliases, TrueType CID subsetting |
| Shaping | HarfBuzz + bidi; RTL runs, contextual forms, reordering; ActualText keeps extraction logically ordered |
| Forms | AcroForm text fields with /TU accessible names; incremental-save `fill_text_field` |
| HTML to PDF | Structural subset (headings, lists, tables, images, implied paragraphs, entities) to streamed tagged PDF |
| Drawing kit | ASME Y14 sheets A-F, tagged title block, Y14.5 dimensions, Y14.35 revision block from file history |
| Reader | Classic, xref-stream and hybrid xref; lazy resolution; repair mode |
| Retag | Open any PDF (even untagged), tag it, save it compliant |
| Extraction | Text blocks from any PDF, tagged or not; honors ActualText |
| Validator | Matterhorn machine checks (`pdfgen validate`), cross-checked against veraPDF |
| Bindings | Python (PyO3, abi3 >= 3.9), Kotlin/Java 11+ (UniFFI + JNA), Java 22+ (FFM, no JNI); all produce veraPDF-valid files |

## How to install veraPDF locally

`tools/verapdf/` is a local install, not part of the repo. CI downloads
veraPDF 1.30.2 from
`https://software.verapdf.org/releases/1.30/verapdf-greenfield-1.30.2-installer.zip`
and installs it headlessly (see `.github/workflows/ci.yml`). For a local
install, download the same zip and run the installer, or use the izpack
auto-install XML from the CI job.

## Development

```bash
cargo test                     # generates tests/output/*.pdf and checks them
cargo run -p pdfgen-cli -- validate tests/output/hello_ua1.pdf   # our checker
export JAVA_HOME=$(/usr/libexec/java_home -v 21)
tools/verapdf/verapdf -f ua1 tests/output/hello_ua1.pdf           # external gate
tools/verapdf/verapdf -f ua2 tests/output/hello_ua2.pdf
```

Pass means `isCompliant="true"` and `failedChecks="0"`. The gate script
`scripts/verapdf-gate.sh` validates every generated fixture except the
files listed (with reasons) in `tests/verapdf-skip.txt`; all other
`tests/output/` files are veraPDF-validated. Nightly cargo-fuzz runs
cover the parser and font loader (`.github/workflows/fuzz.yml`).

Commits follow Conventional Commits. `deny.toml` allow-lists only
permissive dependency licenses (MIT/Apache/BSD/ISC/Zlib); vendored fonts
are OFL-1.1 with their license files kept alongside. veraPDF is a test
tool only, never linked or bundled.

## Optional CJK fonts

The two Noto Sans CJK JP faces (16 MB each) are not committed. Fetch them
with `bash scripts/fetch-cjk-fonts.sh`. CI fetches them automatically;
the CJK tests skip when they are absent.

## Fonts in packaged installs

Bundled fonts live in `fonts/vendor/` at the repo root, outside any crate
directory, so crate packages never ship them (crates.io caps packages at
10 MB). Resolution order at runtime:

1. `PDFGEN_FONTS_DIR` (build-time env var, relocated installs)
2. The repo checkout (`fonts/` relative to the workspace)
3. System fonts by family name
4. A bundled look-alike, with a substitution note in the save report
5. Any system sans-serif, flagged as substituted, so the save always
   succeeds

## Internals

Implementation notes that don't belong in the pitch:

- The layout wrapper is O(n): each word's width is measured once and
  packed greedily, no candidate re-measurement.
- The streaming writer preallocates object ids, so finished pages never
  need patching, and content streams are Flate-compressed as they flush.
- Shaped text whose glyphs don't map one-to-one to characters is wrapped
  in `/Span <</ActualText (...)>>` with the logical source string, so
  extraction returns what was written, not the glyph order.
- The structure-tree walker normalizes /K to a child list (arrays,
  single refs and direct dicts) before descending, including THead/TBody
  row groups.

## Not done yet

- CFF subsetting (CJK OTF faces embed whole; TrueType subsets fine)
- Encryption and signatures
- Publishing to crates.io, PyPI and Maven Central (script ready:
  `scripts/publish.sh`, needs credentials)
- Full plan with milestones: [`docs/PLAN.md`](docs/PLAN.md)

## Limitations

Early project. Not on crates.io or PyPI yet, so you build from source.
No encryption or signature support. CFF/OTF fonts (including the CJK
faces) embed whole, which makes CJK-heavy documents large. Complex
scripts are shaped and verified for Arabic, Myanmar and Thai; other
scripts should work through the same HarfBuzz path but aren't tested.

## Layout

```
crates/
  pdfgen-core      PDF object model, serializer, xref writer
  pdfgen-parse     reader: lexer, xref (classic/stream/hybrid), repair mode
  pdfgen-font      font loading (ttf-parser), WinAnsi, shaping
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
  pdfgen-draw      ASME Y14 + ISO 5457 drawing kit
  pdfgen-ffi       C-ABI layer (cdylib) consumed by the Java FFM binding
  pdfgen-render    software rasterizer (PNG output, print pipeline)
  pdfgen-cli       the `pdfgen` command-line tool
fonts/vendor/      bundled OFL fonts (Liberation, Noto incl. Myanmar/Thai/Arabic)
bindings/          kotlin (UniFFI/JNA), java (FFM), python (PyO3 wheel)
tests/output/      generated PDFs (gitignored)
```

## License

MIT, see [LICENSE](LICENSE). Vendored fonts keep their own licenses in
`fonts/vendor/*/LICENSE` (SIL OFL 1.1).

PDF/UA-1 (ISO 14289-1), PDF/UA-2 (ISO 14289-2) and PDF 2.0 (ISO 32000-2)
are freely available from the [PDF Association](https://pdfa.org/sponsored-standards/);
this project implements from those specifications, not from iText or
PDFBox source code.
