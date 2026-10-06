//! Minimal, spec-shaped XMP packet builder for PDF/UA identifiers.

use crate::Metadata;

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Drop control characters that are illegal in XML 1.0.
            c if u32::from(c) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Build the XMP packet for a document under the given PDF/UA part.
///
/// The packet always carries the PDF/UA identifier when the document is
/// compliant; when it is not, `claim_ua` is false and the packet instead
/// records the document's accessibility status without claiming conformance.
#[must_use] 
pub fn build(meta: &Metadata, ua_part: u32, claim_ua: bool) -> Vec<u8> {
    let mut rdf = String::new();

    // PDF/UA-1 needs no revision attribute; PDF/UA-2 requires rev = 2024.
    if claim_ua {
        if ua_part >= 2 {
            rdf.push_str(&format!(
                "<rdf:Description rdf:about=\"\" xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\" pdfuaid:part=\"{ua_part}\" pdfuaid:rev=\"2024\"/>\n"
            ));
        } else {
            rdf.push_str(&format!(
                "<rdf:Description rdf:about=\"\" xmlns:pdfuaid=\"http://www.aiim.org/pdfua/ns/id/\" pdfuaid:part=\"{ua_part}\"/>\n"
            ));
        }
    }

    // Dublin Core: title, creator, description.
    if meta.title.as_deref().is_none_or(|x| !str::is_empty(x)) {
        rdf.push_str(
            "<rdf:Description rdf:about=\"\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\">\n",
        );
        if let Some(t) = &meta.title {
            rdf.push_str(&format!(
                "<dc:title><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:title>\n",
                xml_escape(t)
            ));
        }
        if !meta.authors.is_empty() {
            rdf.push_str("<dc:creator><rdf:Seq>");
            for a in &meta.authors {
                rdf.push_str(&format!("<rdf:li>{}</rdf:li>", xml_escape(a)));
            }
            rdf.push_str("</rdf:Seq></dc:creator>\n");
        }
        if let Some(d) = &meta.description {
            rdf.push_str(&format!(
                "<dc:description><rdf:Alt><rdf:li xml:lang=\"x-default\">{}</rdf:li></rdf:Alt></dc:description>\n",
                xml_escape(d)
            ));
        }
        rdf.push_str("</rdf:Description>\n");
    }

    // XMP basic (dates skipped for now; xmpMM:History arrives with revisions).
    if !claim_ua {
        rdf.push_str("<rdf:Description rdf:about=\"\" xmlns:pdfgen=\"https://pdfgen.dev/ns/1.0/\" pdfgen:accessibilityStatus=\"not-compliant\"/>\n");
    }

    let xmp = format!(
        "<?xpacket begin=\"\u{feff}\" id=\"W5M0MpCehiHzreSzNTczkc9d\"?>\n\
         <x:xmpmeta xmlns:x=\"adobe:ns:meta/\">\n\
         <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n\
         {rdf}\
         </rdf:RDF>\n\
         </x:xmpmeta>\n\
         <?xpacket end=\"w\"?>"
    );
    xmp.into_bytes()
}
