//! Small end-to-end example: build a service-request form and
//! print the accessibility report status.

fn main() {
    let mut doc = pdfgen::Document::new(pdfgen::Profile::PdfUa1);
    doc.title("Service Request Form").lang("en-US");
    doc.load_font("/System/Library/Fonts/Supplemental/Arial.ttf")
        .unwrap();
    let mut flow = doc.flow();
    flow.heading(1, "Form").unwrap();
    flow.text_field("Full name", "fullname").unwrap();
    drop(flow);
    let r = doc.save("/tmp/form_prefill.pdf").unwrap();
    println!("compliant: {}", r.status == pdfgen::Status::Compliant);
}
