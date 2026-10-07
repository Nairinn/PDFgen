//! Fuzz the full reader pipeline: raw bytes -> PdfReader -> catalog,
//! pages, object streams. Any panic or hang in lexer/xref/predictor/
//! object-stream code fires here.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(mut reader) = pdfgen_parse::PdfReader::from_bytes(data.to_vec()) {
        let _ = reader.catalog();
        let _ = reader.pages();
        let _ = reader.object_offsets();
        // Resolve a spread of objects to exercise stream and object-stream
        // paths (most xref tables map objects 1-20).
        for id in 1..20u32 {
            let _ = reader.get(id);
        }
    }
});
