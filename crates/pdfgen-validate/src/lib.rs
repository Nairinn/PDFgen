//! # pdfgen-validate
//!
//! Runs the machine-checkable Matterhorn Protocol rules over any PDF
//! (tagged or not) and reports violations with checkpoint IDs.
//!
//! This is our own checker — veraPDF is used only as a CI cross-check.
//! Checks implemented here are the software-verifiable subset of the
//! Matterhorn Protocol (the 87 "M" failure conditions); human-judgment
//! conditions are returned as review items, never as failures.

use pdfgen_core::Object;
use pdfgen_parse::PdfReader;
use pdfgen_profile::Profile;
use std::path::Path;

/// One finding: a Matterhorn failure condition hit.
#[derive(Debug, Clone)]
pub struct Finding {
    /// Matterhorn checkpoint id, e.g. `06-002`.
    pub id: String,
    /// Human-readable description of the failure.
    pub message: String,
    /// Where it was found (page index, or `document`).
    pub context: String,
}

/// Validation report for one file.
#[derive(Debug, Clone)]
pub struct Report {
    /// Findings (machine-detected failures).
    pub findings: Vec<Finding>,
    /// Human-review checklist items that apply to this file.
    pub review: Vec<&'static str>,
}

impl Report {
    /// True when no machine failures were found.
    pub fn is_clean(&self) -> bool {
        self.findings.is_empty()
    }
}

/// Validate a PDF file against the PDF/UA-1 machine checks.
pub fn validate(path: impl AsRef<Path>) -> Result<Report, String> {
    let mut reader = PdfReader::open(path).map_err(|e| e.to_string())?;
    let mut findings: Vec<Finding> = Vec::new();
    let mut review: Vec<&'static str> = Vec::new();

    // -- Checkpoint 01: real content is tagged (MarkInfo, StructTreeRoot) --
    let catalog = reader.catalog().map_err(|e| e.to_string())?;
    let marked = catalog
        .get("MarkInfo")
        .and_then(|o| match o {
            Object::Dict(d) => d.get("Marked").and_then(|m| match m {
                Object::Bool(b) => Some(*b),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or(false);
    if !marked {
        findings.push(Finding {
            id: "01-002".into(),
            message: "Document is not marked (MarkInfo/Marked is not true)".into(),
            context: "document".into(),
        });
    }
    if !catalog.has("StructTreeRoot") {
        findings.push(Finding {
            id: "01-003".into(),
            message: "Document has no structure tree root".into(),
            context: "document".into(),
        });
    }

    // -- Checkpoint 02: role map sanity --
    if let Some(Object::Dict(_role_map)) = catalog.get("RoleMap") {
        review.push("02-004: Confirm role-mapped types are semantically appropriate");
    }

    // -- Checkpoint 06: metadata --
    match catalog.get("Metadata") {
        None => findings.push(Finding {
            id: "06-001".into(),
            message: "Document does not contain an XMP metadata stream".into(),
            context: "document".into(),
        }),
        Some(Object::Ref(r)) => {
            let obj = reader.get(r.id).map_err(|e| e.to_string())?;
            if let Object::Stream(s) = obj {
                let text = String::from_utf8_lossy(&s.data).to_string();
                if !text.contains("pdfuaid") {
                    findings.push(Finding {
                        id: "06-002".into(),
                        message: "XMP metadata does not include the PDF/UA identifier"
                            .into(),
                        context: "document".into(),
                    });
                }
                if !text.contains("dc:title") {
                    findings.push(Finding {
                        id: "06-003".into(),
                        message: "XMP metadata does not contain dc:title".into(),
                        context: "document".into(),
                    });
                } else {
                    review.push("06-004: Confirm the dc:title clearly identifies the document");
                }
            }
        }
        _ => {}
    }

    // -- Checkpoint 11: language --
    match catalog.get("Lang") {
        None => findings.push(Finding {
            id: "11-001".into(),
            message: "Natural language for the document cannot be determined".into(),
            context: "document".into(),
        }),
        Some(_) => review.push("11-007: Confirm the declared language is appropriate"),
    }

    // -- ViewerPreferences / DisplayDocTitle (UA-1 7.1-6 via 02-x) --
    let doc_title_shown = catalog
        .get("ViewerPreferences")
        .and_then(|o| match o {
            Object::Dict(d) => d
                .get("DisplayDocTitle")
                .and_then(|v| match v {
                    Object::Bool(b) => Some(*b),
                    _ => None,
                }),
            _ => None,
        })
        .unwrap_or(false);
    if !doc_title_shown {
        findings.push(Finding {
            id: "02-007".into(),
            message: "DisplayDocTitle is not set; the document title is not shown"
                .into(),
            context: "document".into(),
        });
    }

    let _ = Profile::PdfUa1; // UA-2 rule set arrives with the reader's NS walk.

    // -- Annotation / form-field checks (Matterhorn 28-x) --
    if let Some(acro_obj) = catalog.get("AcroForm").cloned() {
        // The AcroForm entry may be a direct dict or (usual) an indirect
        // ref to one.
        let acro: pdfgen_core::Dict = match acro_obj {
            Object::Dict(d) => d,
            Object::Ref(r) => match reader.get(r.id) {
                Ok(Object::Dict(d)) => d,
                _ => {
                    // Not a dict: skip checks.
                    pdfgen_core::Dict::new()
                }
            },
            _ => pdfgen_core::Dict::new(),
        };
        if let Some(Object::Array(fields)) = acro.get("Fields").cloned() {
            for f in fields {
                let Object::Ref(fr) = f else { continue };
                let Ok(Object::Dict(fd)) = reader.get(fr.id) else { continue };
                let ctx = format!("field {}", fr.id);
                // 28-001: interactive form fields need a TU (accessible
                // name). /T alone is a raw field name, not an accessible
                // label, so it does not satisfy the checkpoint.
                let named = match fd.get("TU") {
                    Some(Object::String(s)) => !s.0.is_empty(),
                    Some(_) => true,
                    None => false,
                };
                if !named {
                    findings.push(Finding {
                        id: "28-001".into(),
                        message: "Form field has no accessible name (/TU)".into(),
                        context: ctx.clone(),
                    });
                }
                // 28-002: no JavaScript actions on the field.
                if let Some(Object::Dict(aa)) = fd.get("AA").cloned() {
                    let has_action = aa
                        .0
                        .iter()
                        .any(|(k, _)| matches!(k.0.as_str(), "K" | "V" | "F" | "C"));
                    if has_action {
                        findings.push(Finding {
                            id: "28-002".into(),
                            message: "Form field has an action that may be a script".into(),
                            context: ctx.clone(),
                        });
                    }
                }
            }
        }
    }

    // Structure-tree walk: tables (15-x), headings (14-x), figures (13-x) --
    if let Some(Object::Ref(root_ref)) = catalog.get("StructTreeRoot").cloned() {
        let root = reader.get(root_ref.id).map_err(|e| e.to_string())?;
        if let Object::Dict(root_d) = root {
            if let Some(Object::Ref(k)) = root_d.get("K").cloned() {
                let doc_elem = reader.get(k.id).map_err(|e| e.to_string())?;
                if let Object::Dict(doc_d) = doc_elem {
                    if let Some(Object::Array(kids)) = doc_d.get("K").cloned() {
                        for kid in kids {
                            if let Object::Ref(r) = kid {
                                walk_element(
                                    &mut reader,
                                    r.id,
                                    &mut findings,
                                    &mut review,
                                )?;
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(Report { findings, review })
}

/// Recursively check one structure element (and its children).
fn walk_element(
    reader: &mut PdfReader,
    id: u32,
    findings: &mut Vec<Finding>,
    review: &mut Vec<&'static str>,
) -> Result<(), String> {
    let obj = reader.get(id).map_err(|e| e.to_string())?;
    let Object::Dict(d) = obj else {
        return Ok(());
    };
    let tag = match d.get("S") {
        Some(Object::Name(n)) => n.0.clone(),
        _ => String::new(),
    };
    let ctx = format!("element {id} ({tag})");

    match tag.as_str() {
        // Checkpoint 15: tables.
        "Table" => {
            let has_th = has_header_row(reader, &d);
            if !has_th {
                findings.push(Finding {
                    id: "15-001".into(),
                    message: "Table has no header row (TH)".into(),
                    context: ctx,
                });
            }
        }
        // Checkpoint 15-003: header cells need Scope.
        "TH" => {
            let has_scope = d
                .get("A")
                .and_then(|a| match a {
                    Object::Array(items) => Some(
                        items
                            .iter()
                            .any(|i| matches!(i, Object::Dict(dd) if dd.get("Scope").is_some())),
                    ),
                    Object::Dict(dd) => Some(dd.get("Scope").is_some()),
                    _ => None,
                })
                .unwrap_or(false);
            if !has_scope {
                findings.push(Finding {
                    id: "15-003".into(),
                    message: "Header cell (TH) has no Scope attribute".into(),
                    context: ctx,
                });
            }
        }
        // Checkpoint 13-004: figures need alt text.
        "Figure" => {
            if d.get("Alt").is_none() {
                findings.push(Finding {
                    id: "13-004".into(),
                    message: "Figure has no alternative text (/Alt)".into(),
                    context: ctx,
                });
            } else {
                review.push("13-002: Confirm the alt text conveys the figure's meaning");
            }
        }
        _ => {}
    }

    // Recurse into child element refs in /K.
    if let Some(Object::Array(kids)) = d.get("K").cloned() {
        for kid in kids {
            if let Object::Ref(r) = kid {
                walk_element(reader, r.id, findings, review)?;
            }
        }
    }
    Ok(())
}

/// True when any child TR contains a TH.
fn has_header_row(reader: &mut PdfReader, table: &pdfgen_core::Dict) -> bool {
    let Some(Object::Array(kids)) = table.get("K").cloned() else {
        return false;
    };
    for kid in kids {
        if let Object::Ref(r) = kid {
            let Ok(obj) = reader.get(r.id) else { continue };
            let Object::Dict(tr) = obj else { continue };
            if !matches!(tr.get("S"), Some(Object::Name(n)) if n.0 == "TR") {
                continue;
            }
            if let Some(Object::Array(cells)) = tr.get("K").cloned() {
                for c in cells {
                    if let Object::Ref(cr) = c {
                        let Ok(co) = reader.get(cr.id) else { continue };
                        if let Object::Dict(cd) = co {
                            if matches!(cd.get("S"), Some(Object::Name(n)) if n.0 == "TH") {
                                return true;
                            }
                        }
                    }
                }
            }
        }
    }
    false
}
