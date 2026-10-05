// Java 22+ FFM end-to-end test for the pdfgen bindings: build a tagged
// kitchen-sink document, save, verify the report and the file.
//
// Run: see bindings/java/run.sh

import java.nio.file.Files;
import java.nio.file.Path;

public class TestFfm {
    public static void main(String[] args) throws Exception {
        String lib = args[0];
        String out = args[1];

        try (PdfGen pdfgen = PdfGen.load(lib)) {
            try (PdfGen.Document doc = pdfgen.create(PdfGen.Profile.UA1)) {
                doc.setTitle("Java FFM bindings test")
                   .setLang("en-US");

                // System font by name (registry + aliases).
                int arial = doc.font("Arial", "Bold");

                doc.heading(1, "PDFgen from Java 22+ FFM");
                doc.paragraphIn(arial, 14.0,
                    "This paragraph uses Arial Bold resolved through the font registry.");
                doc.paragraph("A default-font paragraph with enough words to exercise word "
                    + "wrapping across a line boundary in the flow layout, because a single "
                    + "line would not prove very much about the layout engine at all.");

                doc.save(out);
            }

            // Re-open just to read the report of a compliant save.
            try (PdfGen.Document doc = pdfgen.create(PdfGen.Profile.UA1)) {
                doc.setTitle("Report probe").setLang("en-US");
                doc.heading(2, "Probe");
                try (PdfGen.Report r = doc.save(out + ".probe.pdf")) {
                    if (!r.compliant()) throw new AssertionError("expected compliant: " + r.note());
                }
            }

            if (!Files.exists(Path.of(out))) throw new AssertionError("file not written");
            long size = Files.size(Path.of(out));
            if (size < 1000) throw new AssertionError("suspiciously small: " + size);
            System.out.println("OK: " + out + " (" + size + " bytes)");
        }
    }
}
