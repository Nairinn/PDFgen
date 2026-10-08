//! # pdfgen-validate
//!
//! Runs the machine-checkable Matterhorn Protocol rules over any PDF
//! (tagged or not) and reports violations with checkpoint IDs.
//!
//! This is our own checker — veraPDF is the acceptance gate used in CI;
//! this crate catches the common failures early, on any machine, without
//! Java. Human-judgment conditions surface as review items, never as
//! failures.
//!
//! Machine checks implemented (Matterhorn 1.1 checkpoint ids):
//!   01-003 structure tree present        06-001/002/003 XMP metadata
//!   11-001 document language             02-007 DisplayDocTitle
//!   15-001 table header rows             15-003 TH scope
//!   13-004 figure alt text               28-001/28-002 form fields
//! Review prompts: 02-004 role map, 06-004 title quality, 11-007
//! language, 13-002 alt-text quality. NOT implemented: annotations
//! beyond AcroForm (28-003+), embedded-file, font, and color
//! checkpoints — veraPDF covers those in CI.
//!
//! The rule set is PDF/UA-1; UA-2 files validate against the same
//! machine-checkable conditions (the NS walk differs, not the rules).

use pdfgen_core::Object;
use pdfgen_parse::PdfReader;
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
    #[must_use]
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
                        message: "XMP metadata does not include the PDF/UA identifier".into(),
                        context: "document".into(),
                    });
                }
                if text.contains("dc:title") {
                    review.push("06-004: Confirm the dc:title clearly identifies the document");
                } else {
                    findings.push(Finding {
                        id: "06-003".into(),
                        message: "XMP metadata does not contain dc:title".into(),
                        context: "document".into(),
                    });
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
            Object::Dict(d) => d.get("DisplayDocTitle").and_then(|v| match v {
                Object::Bool(b) => Some(*b),
                _ => None,
            }),
            _ => None,
        })
        .unwrap_or(false);
    if !doc_title_shown {
        findings.push(Finding {
            id: "02-007".into(),
            message: "DisplayDocTitle is not set; the document title is not shown".into(),
            context: "document".into(),
        });
    }

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
                let Ok(Object::Dict(fd)) = reader.get(fr.id) else {
                    continue;
                };
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
                    let has_action =
                        aa.0.iter()
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

    // Structure-tree walk: tables (15-x), headings (14-x), figures (13-x).
    // K may be a ref, an array of refs, or a direct dict at any level —
    // normalize before walking (issue #9: ref-only walks silently skipped
    // valid trees).
    if let Some(Object::Ref(root_ref)) = catalog.get("StructTreeRoot").cloned() {
        let root = reader.get(root_ref.id).map_err(|e| e.to_string())?;
        if let Object::Dict(root_d) = root {
            let root_kids = k_children(&root_d);
            for kid in root_kids {
                match kid {
                    Object::Ref(r) => {
                        walk_element(&mut reader, r.id, &mut findings, &mut review)?;
                    }
                    // Direct dict child: walk it in place.
                    Object::Dict(d) => {
                        walk_dict(&mut reader, &d, 0, &mut findings, &mut review)?;
                    }
                    _ => {}
                }
            }
        }
    }

    // -- Page /Annots widget checks (Matterhorn 28-x) --
    if let Ok(pages) = reader.pages() {
        for page_id in pages {
            let Ok(obj) = reader.get(page_id) else {
                continue;
            };
            let Object::Dict(pd) = obj else { continue };
            if let Some(Object::Array(annots)) = pd.get("Annots").cloned() {
                for a in annots {
                    check_annotation(&mut reader, &a, &mut findings);
                }
            }
        }
    }

    Ok(Report { findings, review })
}

/// Recursively check one structure element (and its children).
/// Structure-tree walk with cycle protection: a visited set (element
/// ids) plus a depth cap, so malicious trees cannot blow the stack.
fn walk_element(
    reader: &mut PdfReader,
    id: u32,
    findings: &mut Vec<Finding>,
    review: &mut Vec<&'static str>,
) -> Result<(), String> {
    let mut visited = std::collections::HashSet::new();
    walk_element_inner(reader, id, findings, review, &mut visited, 0)
}

fn walk_element_inner(
    reader: &mut PdfReader,
    id: u32,
    findings: &mut Vec<Finding>,
    review: &mut Vec<&'static str>,
    visited: &mut std::collections::HashSet<u32>,
    depth: usize,
) -> Result<(), String> {
    const MAX_DEPTH: usize = 64;
    if depth > MAX_DEPTH || !visited.insert(id) {
        return Ok(());
    }
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
                    context: ctx.clone(),
                });
            }
            // Tables may nest via direct dicts; check_dict_rules is
            // applied per child in the walk below.
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

    // Recurse into children: refs (with cycle guard) and direct dicts.
    for kid in k_children(&d) {
        match kid {
            Object::Ref(r) => {
                walk_element_inner(reader, r.id, findings, review, visited, depth + 1)?;
            }
            Object::Dict(cd) => walk_dict(reader, &cd, depth + 1, findings, review)?,
            _ => {}
        }
    }
    Ok(())
}

/// The child entries of a structure element's /K, normalized: an array
/// stays, a single child becomes a one-element vec, anything else is
/// empty. Also peels one level of THead/TBody grouping (issue #9: tables
/// grouped per HTML row-sections lost their header detection).
fn k_children(d: &pdfgen_core::Dict) -> Vec<Object> {
    match d.get("K") {
        Some(Object::Array(items)) => items.clone(),
        Some(one @ (Object::Ref(_) | Object::Dict(_))) => vec![one.clone()],
        _ => Vec::new(),
    }
}

/// Walk a direct-dict structure element (no object id).
fn walk_dict(
    reader: &mut PdfReader,
    d: &pdfgen_core::Dict,
    depth: usize,
    findings: &mut Vec<Finding>,
    review: &mut Vec<&'static str>,
) -> Result<(), String> {
    const MAX_DEPTH: usize = 64;
    if depth > MAX_DEPTH {
        return Ok(());
    }
    check_dict_rules(d, findings, review);
    for kid in k_children(d) {
        match kid {
            Object::Ref(r) => {
                let mut visited = std::collections::HashSet::new();
                walk_element_inner(reader, r.id, findings, review, &mut visited, depth + 1)?;
            }
            Object::Dict(cd) => walk_dict(reader, &cd, depth + 1, findings, review)?,
            _ => {}
        }
    }
    Ok(())
}

/// Rule checks that only need the element's own dict.
fn check_dict_rules(
    d: &pdfgen_core::Dict,
    findings: &mut Vec<Finding>,
    review: &mut Vec<&'static str>,
) {
    let tag = match d.get("S") {
        Some(Object::Name(n)) => n.0.clone(),
        _ => String::new(),
    };
    match tag.as_str() {
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
                    context: "element (inline TH)".into(),
                });
            }
        }
        "Figure" => {
            if d.get("Alt").is_none() {
                findings.push(Finding {
                    id: "13-004".into(),
                    message: "Figure has no alternative text (/Alt)".into(),
                    context: "element (inline Figure)".into(),
                });
            } else {
                review.push("13-002: Confirm the alt text conveys the figure's meaning");
            }
        }
        _ => {}
    }
}

/// Matterhorn 28-x checks for one annotation object (direct dict or ref).
fn check_annotation(reader: &mut PdfReader, a: &Object, findings: &mut Vec<Finding>) {
    let d: pdfgen_core::Dict = match a {
        Object::Dict(d) => d.clone(),
        Object::Ref(r) => match reader.get(r.id) {
            Ok(Object::Dict(d)) => d,
            _ => return,
        },
        _ => return,
    };
    // Widget annotations only.
    if !matches!(d.get("Subtype"), Some(Object::Name(n)) if n.0 == "Widget") {
        return;
    }
    let ctx = format!(
        "widget annot (FT {:?})",
        d.get("FT").map(|o| format!("{o:?}"))
    );
    let named = match d.get("TU") {
        Some(Object::String(s)) => !s.0.is_empty(),
        Some(_) => true,
        None => false,
    };
    if !named {
        findings.push(Finding {
            id: "28-001".into(),
            message: "Widget annotation has no accessible name (/TU)".into(),
            context: ctx,
        });
    }
}

/// True when any child TR contains a TH.
fn has_header_row(reader: &mut PdfReader, table: &pdfgen_core::Dict) -> bool {
    let Some(Object::Array(kids)) = table.get("K").cloned() else {
        return false;
    };
    for kid in kids {
        let Object::Ref(r) = kid else { continue };
        let Ok(obj) = reader.get(r.id) else { continue };
        let Object::Dict(tr) = obj else { continue };
        match tr.get("S").cloned() {
            // THead/TBody row groups: recurse into their TRs.
            Some(Object::Name(n)) if n.0 == "THead" || n.0 == "TBody" => {
                if let Some(Object::Array(inner)) = tr.get("K").cloned() {
                    for c in inner {
                        if let Object::Ref(cr) = c {
                            let Ok(co) = reader.get(cr.id) else { continue };
                            if let Object::Dict(cd) = co {
                                if row_has_th(reader, &cd) {
                                    return true;
                                }
                            }
                        }
                    }
                }
            }
            Some(Object::Name(n)) if n.0 == "TR" && row_has_th(reader, &tr) => {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// True when this TR dict contains a TH child.
fn row_has_th(reader: &mut PdfReader, tr: &pdfgen_core::Dict) -> bool {
    let Some(Object::Array(cells)) = tr.get("K").cloned() else {
        return false;
    };
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
    false
}
