import java.lang.foreign.*;
import java.lang.invoke.MethodHandle;
import java.nio.charset.StandardCharsets;
import java.util.ArrayList;
import java.util.List;

import static java.lang.foreign.ValueLayout.*;

/**
 * Java 22+ FFM bindings for pdfgen. No JNA, no JNI: calls go through the
 * standard {@link java.lang.foreign} linker straight into the Rust cdylib.
 *
 * <p>Usage:</p>
 * <pre>{@code
 * try (PdfGen pdfgen = PdfGen.load()) {
 *     PdfGen.Document doc = pdfgen.create(PdfGen.Profile.UA1);
 *     doc.setTitle("FFM test");
 *     doc.setLang("en-US");
 *     doc.heading(1, "Hello from Java");
 *     doc.paragraph("Rendered through the foreign linker.");
 *     PdfGen.Report r = doc.save("out.pdf");
 *     System.out.println("compliant: " + r.compliant());
 * }
 * }</pre>
 */
public final class PdfGen implements AutoCloseable {

    /** Conformance profile selector. */
    public enum Profile {
        UA1(0), UA2(1);
        final int id;
        Profile(int id) { this.id = id; }
    }

    private final SymbolLookup lookup;
    private final Linker linker;

    // downcall handles
    private final MethodHandle mhCreate, mhDestroy, mhSetTitle, mhSetLang;
    private final MethodHandle mhFont, mhLoadFontFile, mhRegisterFont;
    private final MethodHandle mhHeading, mhParagraph, mhParagraphIn;
    private final MethodHandle mhBulletList, mhTable, mhFigure;
    private final MethodHandle mhPageHeader, mhPageFooter;
    private final MethodHandle mhSave, mhFree, mhFreeReport;
    private final MethodHandle mhReportField, mhReportCompliant;
    private final Arena arena;

    private PdfGen(SymbolLookup lookup) {
        this.lookup = lookup;
        this.linker = Linker.nativeLinker();
        this.arena = Arena.global();

        FunctionDescriptor desc = null; // placeholder to satisfy compiler
        mhCreate = down("pdfgen_create", FunctionDescriptor.of(ADDRESS, JAVA_INT, ADDRESS));
        mhDestroy = down("pdfgen_destroy", FunctionDescriptor.ofVoid(ADDRESS));
        mhSetTitle = down("pdfgen_set_title", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));
        mhSetLang = down("pdfgen_set_lang", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));
        mhFont = down("pdfgen_font", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, ADDRESS, ADDRESS));
        mhLoadFontFile = down("pdfgen_load_font_file", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, ADDRESS));
        mhRegisterFont = down("pdfgen_register_font", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, ADDRESS, ADDRESS));
        mhHeading = down("pdfgen_heading", FunctionDescriptor.of(ADDRESS, ADDRESS, JAVA_BYTE, ADDRESS));
        mhParagraph = down("pdfgen_paragraph", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));
        mhParagraphIn = down("pdfgen_paragraph_in", FunctionDescriptor.of(ADDRESS, ADDRESS, JAVA_INT, JAVA_DOUBLE, ADDRESS));
        mhBulletList = down("pdfgen_bullet_list", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, JAVA_INT));
        // rows: *const *const *const c_char -> ADDRESS of the row-pointer array
        mhTable = down("pdfgen_table", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, JAVA_INT, ADDRESS, JAVA_INT, JAVA_INT));
        mhFigure = down("pdfgen_figure", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, ADDRESS, JAVA_DOUBLE, JAVA_DOUBLE));
        mhPageHeader = down("pdfgen_page_header", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));
        mhPageFooter = down("pdfgen_page_footer", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS));
        mhSave = down("pdfgen_save", FunctionDescriptor.of(ADDRESS, ADDRESS, ADDRESS, ADDRESS));
        mhFree = down("pdfgen_free", FunctionDescriptor.ofVoid(ADDRESS));
        mhFreeReport = down("pdfgen_free_report", FunctionDescriptor.ofVoid(ADDRESS));
        mhReportField = down("pdfgen_report_field", FunctionDescriptor.of(ADDRESS, ADDRESS, JAVA_INT));
        mhReportCompliant = down("pdfgen_report_compliant", FunctionDescriptor.of(JAVA_BOOLEAN, ADDRESS));
        if (desc != null) throw new IllegalStateException();
    }

    private MethodHandle down(String name, FunctionDescriptor d) {
        return linker.downcallHandle(lookup.find(name).orElseThrow(
            () -> new IllegalStateException("missing symbol " + name)), d);
    }

    /** Load the pdfgen shared library by path. */
    public static PdfGen load(String libraryPath) {
        SymbolLookup lookup = SymbolLookup.libraryLookup(libraryPath, Arena.global());
        return new PdfGen(lookup);
    }

    @Override public void close() {
        // Arena.global() owns the lookup lifetime; nothing else to free.
    }

    // -------------------------------------------------------------- helpers

    private MemorySegment cstr(String s) {
        byte[] bytes = (s + "\0").getBytes(StandardCharsets.UTF_8);
        MemorySegment seg = arena.allocate(bytes.length);
        seg.copyFrom(MemorySegment.ofArray(bytes));
        return seg;
    }

    private String takeString(MemorySegment p) {
        if (p == null || p.equals(MemorySegment.NULL)) return "";
        return new String(p.getString(0).getBytes(StandardCharsets.UTF_8), StandardCharsets.UTF_8);
    }

    private String errOr(MemorySegment errPtr) {
        return errPtr.equals(MemorySegment.NULL) ? null : takeString(errPtr);
    }

    // ------------------------------------------------------------ document

    public final class Document implements AutoCloseable {
        private final MemorySegment handle;

        private Document(MemorySegment handle) { this.handle = handle; }

        @Override public void close() {
            try { mhDestroy.invoke(handle); } catch (Throwable t) {
                throw new RuntimeException(t);
            }
        }

        private void check(MemorySegment err) {
            String msg = errOr(err);
            if (msg != null) throw new PdfGenException(msg);
        }

        public Document setTitle(String title) {
            try { check((MemorySegment) mhSetTitle.invoke(handle, cstr(title))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document setLang(String lang) {
            try { check((MemorySegment) mhSetLang.invoke(handle, cstr(lang))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public int font(String family, String style) {
            try (Arena a = Arena.ofConfined()) {
                MemorySegment out = a.allocate(JAVA_INT);
                MemorySegment err = (MemorySegment) mhFont.invoke(handle, cstr(family), cstr(style), out);
                check(err);
                return out.get(JAVA_INT, 0);
            } catch (Throwable t) { throw new RuntimeException(t); }
        }

        public int loadFontFile(String path) {
            try (Arena a = Arena.ofConfined()) {
                MemorySegment out = a.allocate(JAVA_INT);
                MemorySegment err = (MemorySegment) mhLoadFontFile.invoke(handle, cstr(path), out);
                check(err);
                return out.get(JAVA_INT, 0);
            } catch (Throwable t) { throw new RuntimeException(t); }
        }

        public void registerFont(String family, String style, String path) {
            try { check((MemorySegment) mhRegisterFont.invoke(handle, cstr(family), cstr(style), cstr(path))); }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document heading(int level, String text) {
            try { check((MemorySegment) mhHeading.invoke(handle, (byte) level, cstr(text))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document paragraph(String text) {
            try { check((MemorySegment) mhParagraph.invoke(handle, cstr(text))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document paragraphIn(int font, double size, String text) {
            try { check((MemorySegment) mhParagraphIn.invoke(handle, font, size, cstr(text))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document bulletList(List<String> items) {
            try (Arena a = Arena.ofConfined()) {
                MemorySegment arr = ((java.lang.foreign.SegmentAllocator) a).allocate(ADDRESS, items.size());
                List<MemorySegment> keep = new ArrayList<>();
                for (int i = 0; i < items.size(); i++) {
                    MemorySegment cs = a.allocateFrom(items.get(i));
                    arr.set(ADDRESS, i * ADDRESS.byteSize(), cs);
                    keep.add(cs);
                }
                MemorySegment err = (MemorySegment) mhBulletList.invoke(handle, arr, items.size());
                check(err);
                return this;
            } catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document table(List<String> header, List<List<String>> rows) {
            try (Arena a = Arena.ofConfined()) {
                MemorySegment harr = ((java.lang.foreign.SegmentAllocator) a).allocate(ADDRESS, header.size());
                for (int i = 0; i < header.size(); i++)
                    harr.set(ADDRESS, i * ADDRESS.byteSize(), a.allocateFrom(header.get(i)));
                int cols = header.size();
                MemorySegment rarr = ((java.lang.foreign.SegmentAllocator) a).allocate(ADDRESS, rows.size());
                for (int r = 0; r < rows.size(); r++) {
                    List<String> row = rows.get(r);
                    MemorySegment carr = ((java.lang.foreign.SegmentAllocator) a).allocate(ADDRESS, cols);
                    for (int c = 0; c < cols; c++)
                        carr.set(ADDRESS, c * ADDRESS.byteSize(), a.allocateFrom(row.get(c)));
                    rarr.set(ADDRESS, r * ADDRESS.byteSize(), carr);
                }
                MemorySegment err = (MemorySegment) mhTable.invoke(handle, harr, header.size(), rarr, rows.size(), cols);
                check(err);
                return this;
            } catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document figure(String path, String alt, double width, double height) {
            try { check((MemorySegment) mhFigure.invoke(handle, cstr(path), cstr(alt), width, height)); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document pageHeader(String text) {
            try { check((MemorySegment) mhPageHeader.invoke(handle, cstr(text))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        public Document pageFooter(String text) {
            try { check((MemorySegment) mhPageFooter.invoke(handle, cstr(text))); return this; }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        /** Save and return the accessibility report. ALWAYS writes the file. */
        public Report save(String path) {
            try (Arena a = Arena.ofConfined()) {
                MemorySegment out = a.allocate(ADDRESS);
                MemorySegment err = (MemorySegment) mhSave.invoke(handle, cstr(path), out);
                check(err);
                return new Report(out.get(ADDRESS, 0));
            } catch (Throwable t) { throw new RuntimeException(t); }
        }
    }

    /** Handle around the Rust-owned save report. */
    public final class Report implements AutoCloseable {
        private final MemorySegment handle;
        private boolean closed;

        private Report(MemorySegment handle) { this.handle = handle; }

        public boolean compliant() {
            try { return (boolean) mhReportCompliant.invoke(handle); }
            catch (Throwable t) { throw new RuntimeException(t); }
        }

        /** 0 = profile name, 1 = note, 2 = violations, 3 = human review. */
        public String field(int which) {
            try {
                MemorySegment s = (MemorySegment) mhReportField.invoke(handle, which);
                String v = takeString(s);
                if (!s.equals(MemorySegment.NULL)) mhFree.invoke(s);
                return v;
            } catch (Throwable t) { throw new RuntimeException(t); }
        }

        public String profileName() { return field(0); }
        public String note() { return field(1); }

        public List<String[]> violations() {
            List<String[]> out = new ArrayList<>();
            for (String line : field(2).split("\n")) {
                if (line.isBlank()) continue;
                String[] parts = line.split("\\|", 3);
                out.add(parts.length == 3 ? parts : new String[]{line, "", ""});
            }
            return out;
        }

        public List<String> humanReview() {
            List<String> out = new ArrayList<>();
            for (String line : field(3).split("\n")) if (!line.isBlank()) out.add(line);
            return out;
        }

        @Override public synchronized void close() {
            if (!closed) {
                closed = true;
                try { mhFreeReport.invoke(handle); } catch (Throwable t) {
                    throw new RuntimeException(t);
                }
            }
        }
    }

    /** Create a document for the given profile. */
    public Document create(Profile profile) {
        try (Arena a = Arena.ofConfined()) {
            MemorySegment out = a.allocate(ADDRESS);
            MemorySegment err = (MemorySegment) mhCreate.invoke(profile.id, out);
            check0(err);
            return new Document(out.get(ADDRESS, 0));
        } catch (Throwable t) { throw new RuntimeException(t); }
    }

    private void check0(MemorySegment err) {
        String msg = errOr(err);
        if (msg != null) throw new PdfGenException(msg);
    }

    /** Error raised when the Rust side returns an error string. */
    public static final class PdfGenException extends RuntimeException {
        public PdfGenException(String msg) { super(msg); }
    }
}
