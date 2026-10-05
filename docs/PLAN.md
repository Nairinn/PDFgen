# PDFgen — Project Plan

A free (MIT), from-scratch PDF library written in Rust. It is meant to cover what PDFBox and iText do,
and it is built around four goals:

1. **Complete PDF/UA, end to end.** PDF/UA-1 and PDF/UA-2 both ship in the same release. Nothing is staged.
2. **Easier versioning.** You pick a target PDF version and conformance profile, and every edit is stored as a revision you can list, diff and roll back.
3. **Easier drawing.** A layout layer for documents and a drawing kit for engineering drawings (ASME first), both sitting on one vector canvas.
4. **Usable from Rust, Kotlin, Java and Python.**

---

## 1. Ground rules

### Clean-room, no iText or PDFBox
- **No code, ports, dependencies or translated logic from iText or PDFBox.** We are recreating this functionality from scratch. Contributors should not read their source while implementing equivalent features.
- **We implement from the specifications.** Most are free to download:
  - ISO 32000-2 (PDF 2.0): free from the PDF Association.
  - ISO 14289-1 (PDF/UA-1), ISO 14289-2 (PDF/UA-2), ISO TS 32005: free from the PDF Association.
  - ISO 32000-1 (PDF 1.7): Adobe's free copy.
  - Matterhorn Protocol 1.1.
  - ASME Y14 drawing standards: these are paid. We implement from public summaries plus the purchased standards if available.
- **Third-party Rust crates** are allowed only for well-solved subproblems (compression, image decoding, font parsing and shaping). Allowed licenses are MIT, Apache-2.0, BSD, ISC and Zlib. Fonts may be OFL-1.1 or Apache-2.0. `cargo-deny` enforces the license allow-list in CI, so nothing copyleft or commercial can slip in.
- **veraPDF is used only as an external, black-box test tool.** It is never linked or bundled.

### Accessibility guides you; it doesn't block you
- **Any PDF can be produced and viewed**, including inaccessible ones. The API never refuses to output.
- **Every save returns an accessibility report.** If the document is not compliant, the user gets a clear note:
  > ⚠ This document is not PDF/UA compliant yet: 3 issues (13-004 Figure on page 2 has no alt text, …)
  - The note arrives through each language's normal warning mechanism: Rust `tracing`/log warning, Python `warnings.warn(PdfUaWarning)`, a JVM logger warning, and on stderr from the CLI.
  - The report lists each issue's Matterhorn or clause ID, its page, the object, and how to fix it.
- **The file never claims compliance it doesn't have.**
  - The PDF/UA identifier (`pdfuaid:part`) is written only when all machine checks pass.
  - Otherwise an XMP property, `pdfgen:accessibilityStatus = not-compliant` plus an issue count, travels with the file, so our CLI and validator can show the note later.
- **Optional visible notice:** `draft_notice(true)` stamps "Draft – not yet PDF/UA compliant" in the page margin. It is marked as an artifact, so screen readers skip it.
- **Modes:**
  - `Report` (default): always outputs and warns.
  - `Strict` (opt-in, for CI): save returns an error when there are violations.
  - `Untagged`: a plain PDF with no structure. It still carries the note.
- **The defaults are accessible.** The layout layer tags automatically, marks headers and footers as artifacts, builds bookmarks from headings and sets table header scopes. In practice, violations only come from information only the author can supply, such as alt text, the title and the language, or from raw canvas drawing left untagged.

---

## 2. Scope (one release, complete)

**Profiles in 1.0:** PDF/UA-1 (PDF 1.7) and PDF/UA-2 (PDF 2.0, including the PDF 2.0 structure namespace and ISO TS 32005 mapping).
PDF/A is a separate standard and is not part of this plan. The profile system leaves room for it later.

**The writer covers everything PDF/UA governs:**
- Text, headings, paragraphs, lists, block quotes, code.
- Tables: header rows and columns, row and column spans, Headers/IDs, header rows repeated on each page as artifacts.
- Figures with alt text, captions, decorative images.
- Formulas: alt text for UA-1; MathML attached as associated files for UA-2.
- Links (with structure destinations in UA-2), footnotes and endnotes, references, table of contents, bookmarks.
- Annotations, interactive forms (AcroForm fields with `TU`, tagged Form elements, tab order), signature fields.
- Embedded files (with descriptions and associated-file relationships), optional content / layers (named OCGs).
- Encryption: AES-256 with the accessibility permission bit set.

**Read-only coverage:** the writer never emits JavaScript, multimedia or XFA (XFA is deprecated in PDF 2.0). The validator still detects them and checks their PDF/UA rules in files it reads.

**The validator:**
- **PDF/UA-1:**
  - All 87 Matterhorn machine checks.
  - The 47 human checks appear as a "needs review" checklist with the context a reviewer needs (for example, every figure next to its alt text, and the reading order flattened to plain text).
- **PDF/UA-2:** a rule set derived from the ISO 14289-2 clauses (there is no Matterhorn for UA-2), cross-checked against veraPDF's `ua2` results.

---

## 3. Architecture (Cargo workspace)

| Crate | Responsibility |
|---|---|
| `pdfgen-core` | PDF object model, object IDs and generations, serializer (xref tables, xref streams, object streams), filters, content-stream reading and writing |
| `pdfgen-parse` | Lexer, xref parsing (classic, stream, hybrid), lazy loading, repair mode for broken files, decryption |
| `pdfgen-font` | Font engine: load TTF/OTF/TTC/WOFF/WOFF2, shape text, handle bidi and line breaking, subset, embed fonts, generate the Unicode maps, check embedding permissions |
| `pdfgen-fonts-*` | Built-in font catalog, split into packages (see §4) |
| `pdfgen-canvas` | Vector drawing (paths, transforms, color/ICC, images, clipping, reusable graphics) plus the tag tree: UA-1 and UA-2 namespaces, role map, attributes, Alt/ActualText/E/Lang, IDs, artifacts |
| `pdfgen-layout` | Layout on the canvas: paragraph flow, headings, lists, tables, figures, formulas, page templates, columns, keep-together rules, footnotes, links, table of contents, bookmarks, forms |
| `pdfgen-draw` | Engineering drawing kit (ASME first, then ISO; see §6) |
| `pdfgen-profile` | Profiles (UA-1, UA-2). Each one fills in required entries automatically (XMP identifiers, MarkInfo, DisplayDocTitle, page tab order, `Suspects=false`) and defines the rules the reporter and validator use |
| `pdfgen-validate` | Machine checks plus the human-review checklist. Reports in JSON, HTML and JUnit (for CI) |
| `pdfgen-revision` | Incremental saves, list revisions, open any revision, diff, revert and truncate (see §5) |
| `pdfgen` | Single crate that re-exports the public Rust API |
| `pdfgen-api` | A simplified copy of the API for bindings: Arc handles, owned types, builder objects, no lifetimes or closures. Both binding layers wrap it |
| `pdfgen-uniffi` | UniFFI layer that generates the Kotlin and Java bindings |
| `pdfgen-py` | PyO3 layer for Python |
| `pdfgen-cli` | `pdfgen validate / history / diff / revert / truncate / fonts / render` |

```
PDFgen/
  Cargo.toml            # workspace
  crates/               # crates above
  fonts/manifest.toml   # font catalog: name, version, upstream URL, sha256, license
  bindings/kotlin/      # Gradle: Kotlin/JNA artifact
  bindings/java/        # Gradle: Java artifact (newer FFM API)
  bindings/python/      # maturin + PyO3, .pyi type stubs
  tests/corpus/ tests/golden/
  docs/PLAN.md docs/adr/
  .github/workflows/
```

---

## 4. Fonts

### 4a. Built-in catalog: call fonts by name, free ones only
Users write `doc.font("Liberation Sans").bold()` or `Font::builtin(Builtin::AtkinsonHyperlegible)` and never deal with font files.
Each font's license is checked before it enters `fonts/manifest.toml`, and its license text ships with the package.

| Group | Fonts (initial list) |
|---|---|
| **Free look-alikes for paid fonts** (same metrics, so layout matches) | Liberation Sans (Arial/Helvetica), Liberation Serif (Times New Roman/Times), Liberation Mono (Courier New/Courier), Carlito (Calibri), Caladea (Cambria) |
| Sans | Noto Sans, Inter, Roboto, Open Sans, Source Sans 3, IBM Plex Sans, Lato, Fira Sans, DejaVu Sans |
| Serif | Noto Serif, Source Serif 4, IBM Plex Serif, Merriweather, EB Garamond, Libre Baskerville, Crimson Pro |
| Mono | Noto Sans Mono, JetBrains Mono, Source Code Pro, IBM Plex Mono, Fira Code, DejaVu Sans Mono |
| **Accessibility / readability** | Atkinson Hyperlegible (Next + Mono), Lexend, OpenDyslexic |
| Math | STIX Two Math, Noto Sans Math |
| Symbols | Noto Sans Symbols, Noto Sans Symbols 2, Noto Emoji (monochrome) |
| World scripts | Noto Sans/Serif for Arabic, Hebrew, Devanagari, Bengali, Tamil, Telugu, Thai, Lao, Khmer, **Myanmar**, Ethiopic, Armenian, Georgian, and more |
| CJK | Noto Sans/Serif CJK SC, TC, JP, KR |
| Technical lettering | osifont (ISO 3098). It is GPL with a font exception, so it ships only as a separate opt-in package. ASME's default lettering is an uppercase sans from the main catalog |

- **Automatic fallback:** font stacks (for example, Noto Sans → script-specific Noto → Symbols) give every character a real glyph. PDF/UA does not allow the empty `.notdef` glyph.
- **Standard-14 names map to the look-alikes.** `Helvetica` resolves to Liberation Sans and `Times-Roman` to Liberation Serif, with a note. PDF/UA requires embedded fonts, and the standard 14 are never embedded.
- **Packaging:**
  - crates.io limits a crate to 10 MB, so the catalog is split into `pdfgen-fonts-core` (look-alikes, Noto Latin/Greek/Cyrillic, accessibility, math, symbols), `pdfgen-fonts-scripts`, `pdfgen-fonts-cjk-*` and `pdfgen-fonts-osifont`. These are Rust feature flags: `fonts-core` is on by default, the rest are opt-in.
  - JVM: the same packages as separate Maven artifacts. Python: pip extras such as `pdfgen[fonts-cjk]`.
  - Font files stay out of git. `cargo xtask fetch-fonts` downloads them from the pinned upstream URLs and checks each SHA-256.

### 4b. Paid / commercial fonts: bring your own
We can't bundle paid fonts, but any font the user owns works exactly like a built-in one: same shaping, subsetting and Unicode maps, and the output can still be fully compliant.
- **Import:** `doc.load_font_file(path)` or `load_font_bytes(bytes)`. Supported formats:
  - TrueType (`.ttf`), OpenType/CFF (`.otf`).
  - Collections (`.ttc`/`.otc`, choosing the face by index).
  - WOFF and WOFF2.
  - Variable fonts, converted to a fixed weight/width before embedding.
  - Legacy Type 1 (`.pfb`/`.pfa` with `.afm`).
- **System fonts:** `FontRegistry::system()` scans the operating system's font folders on macOS, Windows and Linux (fontconfig), so fonts already licensed on the machine (Arial, Calibri, Segoe, …) work by name.
- **One registry:** `doc.font("Arial")` finds the font whether it was imported, installed on the system or built in. If it's missing, it falls back to the look-alike (Liberation Sans) and adds a note to the report.
- **Embedding permissions:** we read the font's license flag (`OS/2 fsType`):
  - restricted-license or bitmap-only fonts produce a report warning (an error in `Strict` mode);
  - "no subsetting" fonts are embedded whole.
  Licensing remains the user's responsibility. The library makes it visible.

---

## 5. Version control

- **Import and retag:** open any existing PDF — including untagged ones — and tag it interactively: mark blocks as headings, paragraphs, lists or figures, add alt text, classify headers/footers as artifacts, and set table headers. Saving writes the new structure tree and the same accessibility report as a fresh document. (See §5a.)
- **Profiles and versions:** `Profile::PdfUa1` writes PDF 1.7 and `Profile::PdfUa2` writes PDF 2.0. Upgrading an existing file's version is done with an incremental save that sets the catalog `/Version`, so the original bytes are untouched.
- **Revisions:** each incremental save is one revision, stored inside the PDF itself. Nothing lives beside the file, and existing signatures stay valid.
- **Commit messages:** each revision's message, author and timestamp go into the XMP history (`xmpMM:History`), plus a private dictionary for labels and tags.
- **Diff has two levels:**
  - Object level: which objects were added, changed or deleted.
  - Meaning level: pages, text (taken from the tag tree, so it's reliable), metadata, annotations, form values, the tag tree itself, and **whether accessibility got worse** (for example, "revision 4 broke UA compliance: 2 new issues").
- **Undo has two options:**
  - `revert` appends a new revision that restores the old state, like git revert, without destroying anything.
  - `truncate` cuts the file back to an earlier end-of-file marker for an exact byte-for-byte restore.
- **Signatures:** detect any changes made after a signature (DocMDP permissions).
- **CLI:** `pdfgen history f.pdf`, `pdfgen diff f.pdf --from 2 --to 5`, `pdfgen revert f.pdf --to 3 -m "undo pinout change"`.

---

## 6. Drawing

**Vector canvas:** paths, shapes, transforms, color/ICC, images, clipping and reusable graphics. Content is placed inside `tag(...)` or `artifact(...)`. Untagged content is allowed, but it shows up in the report.

**Layout layer:** documents (see §2). Reading order follows the order of API calls, not the order things are painted.

**Engineering drawings, ASME first:**
- **Y14.1:** sheet sizes A–F and frames, with zoning.
- **Y14.2:** line types and weights, lettering.
- **Y14.5:** linear, angular, radial and ordinate dimensions; tolerances; feature control frames (GD&T); datum symbols.
- **Y14.35:** revision block. **It is linked to §5**, so revisions recorded in the PDF fill the block.
- **Y14.34:** parts list / bill of materials, as a real tagged table.
- **Y14.100:** general practices (title block fields).
- Plus leaders, callouts, hatching and a units/scale system.
- **ISO second:** ISO 5457 sheets, ISO 7200 title block, ISO 128 lines, ISO 129 dimensions.

**How a drawing is tagged:**
- The whole drawing is a Figure with alt text. A generated draft of the alt text comes from the title block and the parts list.
- The title block, parts list and notes are tagged text and tables.
- Pure geometry is an artifact or sits inside the Figure.

---

## 7. Bindings

| Target | Approach | Works on |
|---|---|---|
| Rust | `pdfgen` crate on crates.io | stable Rust |
| **Kotlin** (and Java 11/17) | UniFFI's official Kotlin bindings (JNA) | Kotlin 1.9+, **Java 11+** |
| **Java 22+** | `uniffi-bindgen-java` (newer FFM API, no JNA, faster) | Java 22+ |
| **Python** | **PyO3 + maturin** | CPython 3.9+ |

- **Why PyO3 for Python:** keyword arguments, context managers (`with Document(...) as doc:`), real exceptions and warnings, and `.pyi` type stubs, which make it feel like a normal Python library. One `abi3` wheel per platform covers every Python 3.9+ version. Because PyO3 wraps the same `pdfgen-api` crate as UniFFI, there's one place to change the API.
- **Native libraries** ship inside the jars and wheels for macOS (arm64/x64), Linux (x64/arm64) and Windows x64.
- **Local JDKs are already installed:** 23 (for the Java FFM bindings), 21 Temurin (for veraPDF), and 11 Corretto (to test the Kotlin/JNA artifact on Java 11).

---

## 8. API sketch

```rust
let mut doc = Document::new(Profile::PdfUa2)
    .title("Cable Assembly CA-102").lang("en-US");          // optional, but reported if missing
let body = doc.font("Atkinson Hyperlegible");
let brand = doc.load_font_file("fonts/CorporateSans.otf")?; // user's own licensed font

doc.flow(|f| {
    f.heading(1, "Cable Assembly CA-102");
    f.paragraph("Pinout and wiring for…");
    f.table(Table::new().header_row(["Pin", "Signal", "Color"]).row(["1", "VCC", "Red"]));
    f.figure(Alt::text("Wiring diagram: J1 pin 1 to P2 pin 4"), |c| {
        c.line((0.0, 0.0), (120.0, 0.0)).stroke(Stroke::asme_visible());
    });
});
let report = doc.save("ca102.pdf")?;  // always writes the file
// report.status == NotCompliantYet → warning printed, no PDF/UA claim in the file
```

```kotlin
Document(Profile.PDF_UA_2).title("Cable Assembly CA-102").lang("en-US").use { doc ->
    doc.flow().heading(1, "Cable Assembly CA-102").paragraph("Pinout and wiring for…")
    val report = doc.save("ca102.pdf")
}
```

```python
with pdfgen.Document(profile="ua2", title="Cable Assembly CA-102", lang="en-US") as doc:
    doc.flow.heading(1, "Cable Assembly CA-102")
    report = doc.save("ca102.pdf")   # warns with PdfUaWarning if not compliant
```

---

## 9. Testing

- **External checker:** veraPDF in CI and locally. Every sample and golden file must pass both `-f ua1` and `-f ua2` for its profile.
- **Our validator:** scored against the veraPDF test corpus (whose files are labelled pass or fail). Our results must agree with the labels.
- **Accessibility report:** tests make sure inaccessible input still saves, gets the right note, and has no PDF/UA claim in its XMP.
- **Parser:** fuzzing with `cargo-fuzz`, and round-trip tests (parse → write → parse should give the same document).
- **Fonts:** every catalog font renders and embeds and the Unicode maps round-trip. The CI license gate (`cargo-deny` plus a manifest check) passes.
- **Bindings:** smoke tests on Kotlin (Java 11 and 21), Java 22+ and Python 3.9 and 3.13.
- **Manual:** PAC 2024 (Windows) and screen readers (NVDA, VoiceOver) for reading order and the human checks.

---

## 10. Build order (milestones toward one complete 1.0)

These are steps in the build. They are not separate releases: 1.0 ships when every one of them is done.

| # | Deliverable | Done when |
|---|---|---|
| M0 | Workspace, CI (fmt, clippy, tests, cargo-deny), local veraPDF, decision records | CI green, `verapdf --version` runs locally |
| M1 | **Thin end-to-end slice:** object model + writer + one catalog font + tagged paragraph + XMP | A hello-world passes veraPDF `ua1` **and** `ua2` |
| M2 | Font engine and catalog: packaging, bring-your-own and system fonts, shaping, fallback, embedding-permission checks | Every catalog font and a sample commercial-style font embed and pass |
| M3 | Canvas and tags (UA-1 and UA-2), artifacts, images, links, annotations, bookmarks, report/strict/untagged modes | Samples pass. Inaccessible samples save, show the note and carry no claim |
| M4 | **Bindings spike:** Kotlin/JNA and Python/PyO3 over `pdfgen-api` | Kotlin, Java 11 and Python scripts each generate the M1 PDF |
| M5 | Layout engine (flow, headings, lists, complex tables, table of contents, page templates, footnotes) | A multi-page report passes |
| M6 | Parser (real-world files, repair mode, decryption, content streams, tag tree) | Fuzz and round-trip tests clean |
| M7 | Validator: UA-1 (87 machine checks plus the human checklist) and UA-2 rule set, CLI | Results agree with the veraPDF corpus labels |
| M8 | Forms, signature fields, embedded files, formulas/MathML, layers, AES-256 encryption | Samples for each feature pass `ua1`/`ua2` |
| M9 | Revisions: incremental save, history, diff (including accessibility regressions), revert, truncate, DocMDP | Round-trip tests; signatures still valid |
| M10 | Drawing kit, ASME (Y14.1/.2/.5/.34/.35/.100), then ISO | Sample drawings pass `ua1`/`ua2` |
| M11 | Full bindings and packaging: Kotlin/JNA, Java FFM, Python abi3 wheels, crates.io; font packages | Install and run from Maven Central, PyPI and crates.io |
| **1.0** | Everything above | — |

---

## 11. What makes it free and better than iText/PDFBox

- **MIT, with no AGPL or commercial license**, and no code from either project.
- **Accessibility is built in**, not an add-on: PDF/UA-1 and -2, with a report and note on every save.
- **Revision history and diffs are built in**, including detecting when an edit makes accessibility worse.
- **A free font catalog** with look-alikes for common paid fonts, plus full support for bringing your own fonts.
- **An engineering drawing kit** (ASME/ISO) that produces accessible drawings.
- **Memory-safe Rust core**, usable from Rust, Kotlin, Java and Python.

## 12. Risks

- **Text layout and fonts are the biggest piece of work:** right-to-left text, complex scripts, CJK and variable fonts. Mitigation: proven permissively licensed crates for shaping and bidi, and test files for each script.
- **Real-world PDFs are often malformed.** The parser needs repair mode and continuous fuzzing.
- **There is no Matterhorn equivalent for UA-2**, so we derive the rules from ISO 14289-2 and cross-check against veraPDF.
- **`uniffi-bindgen-java` is pre-1.0.** We pin its version; Kotlin/JNA is the fallback for Java.
- **Font package sizes** (CJK) are bigger than crates.io allows, so they are split into multiple crates and shipped as separate Maven/PyPI packages.
- **Software alone can't certify PDF/UA.** The 47 human checks are surfaced with context, never claimed as passed.
