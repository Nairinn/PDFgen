//! pdfloom-def: the `pdfloom-doc` document-definition format.
//!
//! One versioned JSON schema serves three roles: the visual editor's
//! save state, the portable config file you can download alongside a
//! PDF, and the input to [`build_to_file`]/the `pdfloom build` command.
//! Building the same config always produces the same tagged PDF.
//!
//! The layout model is explicit pages with absolutely positioned,
//! semantically typed elements (heading, paragraph, list, table, image,
//! form field, decorations). Reading order comes from each element's
//! `order` field, not from position on the page, because accessibility
//! is about the logical tree, not visual geometry.

use pdfloom::structure::Node;
use pdfloom::Profile;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Errors from parsing or building a document definition.
#[derive(Debug, thiserror::Error)]
pub enum DefError {
    /// The JSON is not a valid `pdfloom-doc` document.
    #[error("config: {0}")]
    Json(#[from] serde_json::Error),
    /// The file-level `format`/`version` fields are wrong.
    #[error("not a pdfloom-doc document (format must be \"pdfloom-doc\", version 1)")]
    NotPdfloomDoc,
    /// A referenced element/font/asset could not be resolved.
    #[error("build: {0}")]
    Build(String),
    /// The underlying writer failed.
    #[error("pdf: {0}")]
    Pdf(#[from] pdfloom::FontError),
    /// IO failure (config file, embedded image data).
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

// ------------------------------------------------------------------ schema

/// The root of a `pdfloom-doc` document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentDef {
    /// Must be `"pdfloom-doc"`.
    pub format: String,
    /// Schema version; currently `1`.
    pub version: u32,
    /// Conformance profile the PDF claims when every check passes.
    pub profile: DefProfile,
    /// Document title (dc:title).
    pub title: String,
    /// Primary natural language, e.g. `"en-US"`.
    pub lang: String,
    /// Pages, in order.
    #[serde(default)]
    pub pages: Vec<PageDef>,
}

/// Profile selector.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DefProfile {
    /// PDF/UA-1 (PDF 1.7).
    Ua1,
    /// PDF/UA-2 (PDF 2.0).
    Ua2,
}

impl From<DefProfile> for Profile {
    fn from(p: DefProfile) -> Self {
        match p {
            DefProfile::Ua1 => Profile::PdfUa1,
            DefProfile::Ua2 => Profile::PdfUa2,
        }
    }
}

/// One page with explicitly placed elements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageDef {
    /// Page size in points: `[width, height]`. Defaults to US Letter
    /// portrait `[612, 792]`.
    #[serde(default = "letter_size")]
    pub size: [f64; 2],
    /// Elements on this page.
    #[serde(default)]
    pub elements: Vec<ElementDef>,
}

/// Default page size: US Letter portrait.
fn letter_size() -> [f64; 2] {
    [612.0, 792.0]
}

/// A typed, placed element. `rect` is `[x, y, w, h]` in PDF points
/// with the origin at the page's BOTTOM-left; editors drawing with a
/// top-left origin convert on render.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ElementDef {
    /// Heading structure element (`H1`..`H6`).
    Heading(HeadingDef),
    /// Paragraph structure element, wrapped inside its frame.
    Paragraph(ParagraphDef),
    /// Bullet list (`L > LI > (Lbl, LBody)`).
    List(ListDef),
    /// Tagged table with a header row.
    Table(TableDef),
    /// Image (Figure when `alt` is set; decorative artifact when empty).
    Image(ImageDef),
    /// Interactive text field with a drawn label.
    FormField(FormFieldDef),
    /// Filled/stroked rectangle (decoration, artifact).
    Rect(RectDef),
    /// Straight line (decoration, artifact).
    Line(LineDef),
}

/// Common placement for every element variant.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Placement {
    /// `[x, y, width, height]` in PDF points, bottom-left origin.
    pub rect: [f64; 4],
    /// Reading order within the page (lower = earlier). Elements with
    /// equal order keep array order.
    #[serde(default)]
    pub order: u32,
}

/// Font spec inside an element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FontDef {
    /// Family name resolved through the registry (system fonts,
    /// bundled catalog, standard-14 aliases).
    #[serde(default = "default_family")]
    pub family: String,
    /// `"Regular"`, `"Bold"`, `"Italic"`, `"Bold Italic"`.
    #[serde(default = "default_style")]
    pub style: String,
    /// Size in points.
    pub size: f64,
    /// `#rrggbb` fill color.
    #[serde(default = "default_color")]
    pub color: String,
}

/// Default font family.
fn default_family() -> String {
    "Liberation Sans".into()
}
/// Default font style.
fn default_style() -> String {
    "Regular".into()
}
/// Default text color (black).
fn default_color() -> String {
    "#000000".into()
}

/// Heading element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HeadingDef {
    /// Position and reading order.
    #[serde(flatten)]
    pub place: Placement,
    /// Heading level, 1..=6.
    pub level: u8,
    /// Heading text.
    pub text: String,
    /// Font; defaults to Liberation Sans Bold at a level-based size.
    #[serde(default)]
    pub font: Option<FontDef>,
}

/// Paragraph element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParagraphDef {
    /// Position and reading order.
    #[serde(flatten)]
    pub place: Placement,
    /// Paragraph text (wrapped inside the frame).
    pub text: String,
    /// Font; defaults to Liberation Sans Regular 11pt.
    #[serde(default)]
    pub font: Option<FontDef>,
}

/// Bullet-list element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListDef {
    /// Position and reading order.
    #[serde(flatten)]
    pub place: Placement,
    /// Item texts (each becomes `LI > (Lbl, LBody)`).
    pub items: Vec<String>,
    /// Label text; default is a bullet.
    #[serde(default)]
    pub bullet: Option<String>,
    /// Font; defaults to Liberation Sans Regular 11pt.
    #[serde(default)]
    pub font: Option<FontDef>,
}

/// Table element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableDef {
    /// Position and reading order.
    #[serde(flatten)]
    pub place: Placement,
    /// Header row cell texts (rendered as `TH`, Scope Column).
    pub header: Vec<String>,
    /// Body rows.
    #[serde(default)]
    pub rows: Vec<Vec<String>>,
    /// Column widths in points; omitted = equal split across the rect.
    #[serde(default)]
    pub widths: Option<Vec<f64>>,
    /// Cell font; defaults to Liberation Sans Regular 10pt.
    #[serde(default)]
    pub font: Option<FontDef>,
}

/// Image element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageDef {
    /// Position and reading order.
    #[serde(flatten)]
    pub place: Placement,
    /// Either a filesystem path or a `data:image/...;base64,...` URL
    /// (PNG or JPEG).
    pub src: String,
    /// Alt text. Empty string renders the image as a decorative
    /// artifact instead of a Figure.
    #[serde(default)]
    pub alt: String,
}

/// Interactive form-field element.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormFieldDef {
    /// Position and reading order.
    #[serde(flatten)]
    pub place: Placement,
    /// Visible label (drawn as a Caption; screen readers announce it).
    pub label: String,
    /// Field name (`/T`); unique per document.
    pub name: String,
    /// Label font; defaults to Liberation Sans Regular 11pt.
    #[serde(default)]
    pub font: Option<FontDef>,
}

/// Decoration rectangle (always an artifact).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RectDef {
    /// Position (order is ignored for decorations).
    #[serde(flatten)]
    pub place: Placement,
    /// Fill color `#rrggbb`; omit for none.
    #[serde(default)]
    pub fill: Option<String>,
    /// Stroke color `#rrggbb`; omit for none.
    #[serde(default)]
    pub stroke: Option<String>,
    /// Stroke width in points.
    #[serde(default = "default_stroke_w")]
    pub stroke_width: f64,
}

/// Default stroke width.
fn default_stroke_w() -> f64 {
    1.0
}

/// Decoration line (always an artifact).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineDef {
    /// Bounding placement (editors keep this covering both points).
    #[serde(flatten)]
    pub place: Placement,
    /// Start point `[x, y]` (the rect is recomputed by editors to
    /// bound both points; builders only use these).
    pub from: [f64; 2],
    /// End point `[x, y]`.
    pub to: [f64; 2],
    /// Stroke color.
    #[serde(default = "default_color")]
    pub stroke: String,
    /// Stroke width in points.
    #[serde(default = "default_stroke_w")]
    pub stroke_width: f64,
}

// ---------------------------------------------------------------- builders

/// Parse a `pdfloom-doc` document from JSON bytes, validating the
/// format/version envelope.
pub fn parse_def(json: &[u8]) -> Result<DocumentDef, DefError> {
    let def: DocumentDef = serde_json::from_slice(json)?;
    if def.format != "pdfloom-doc" || def.version != 1 {
        return Err(DefError::NotPdfloomDoc);
    }
    Ok(def)
}

/// Build the PDF described by `def` at `out_path`. Returns the save
/// report (never blocks: a non-compliant file is written and reported).
pub fn build_to_file(
    def: &DocumentDef,
    out_path: &str,
) -> Result<pdfloom_profile::SaveReport, DefError> {
    let mut doc = pdfloom::Document::new(def.profile.into());
    doc.title(&def.title).lang(&def.lang);
    doc.ensure_default_font();

    // Resolve one font handle per (family, style); size and color are
    // draw-time parameters, not part of the loaded font.
    let mut fonts: HashMap<(String, String), usize> = HashMap::new();
    let mut resolve =
        |doc: &mut pdfloom::Document, f: &Option<FontDef>| -> Result<usize, DefError> {
            let f = f.clone().unwrap_or(FontDef {
                family: default_family(),
                style: default_style(),
                size: 11.0,
                color: default_color(),
            });
            let key = (f.family.clone(), f.style.clone());
            if let Some(&h) = fonts.get(&key) {
                return Ok(h);
            }
            let h = doc
                .font(&f.family, &f.style)
                .map_err(|e| DefError::Build(e.to_string()))?;
            fonts.insert(key, h);
            Ok(h)
        };

    for (page_index, page) in def.pages.iter().enumerate() {
        let pi = doc.add_draw_page(page.size[0], page.size[1]);
        debug_assert_eq!(pi, page_index);

        // Decorations paint first (z-order bottom), content in reading
        // order after, so content always paints above decorations.
        for el in &page.elements {
            match el {
                ElementDef::Rect(r) => draw_rect(&mut doc, pi, r),
                ElementDef::Line(l) => draw_line(&mut doc, pi, l),
                _ => {}
            }
        }
        let mut content: Vec<&ElementDef> = page
            .elements
            .iter()
            .filter(|e| !matches!(e, ElementDef::Rect(_) | ElementDef::Line(_)))
            .collect();
        content.sort_by_key(|e| order_of(e));

        for el in content {
            build_element(&mut doc, pi, el, &mut resolve)?;
        }
    }

    let report = doc
        .save(out_path)
        .map_err(|e| DefError::Build(e.to_string()))?;
    Ok(report)
}

/// Reading-order key of an element.
fn order_of(e: &ElementDef) -> u32 {
    match e {
        ElementDef::Heading(d) => d.place.order,
        ElementDef::Paragraph(d) => d.place.order,
        ElementDef::List(d) => d.place.order,
        ElementDef::Table(d) => d.place.order,
        ElementDef::Image(d) => d.place.order,
        ElementDef::FormField(d) => d.place.order,
        ElementDef::Rect(d) => d.place.order,
        ElementDef::Line(d) => d.place.order,
    }
}

// ------------------------------------------------------------ text frames

/// Wrap `text` at `max_w` with the font's real metrics and draw the
/// lines top-down from the frame's top edge. Returns the y below the
/// last line. All draws happen INSIDE the currently open marked
/// content (caller opened the tag/artifact).
fn draw_wrapped(
    doc: &mut pdfloom::Document,
    page: usize,
    font: usize,
    size: f64,
    rgb: (f64, f64, f64),
    text: &str,
    rect: [f64; 4],
) -> Result<f64, DefError> {
    let [x, y, w, h] = rect;
    let top = y + h;
    let lines = doc.wrap_text(font, size, text, w)?;
    let lh = size * 1.45;
    let mut baseline = top - size * 0.8;
    for line in &lines {
        doc.frame_text(page, x, baseline, font, size, rgb, line);
        baseline -= lh;
    }
    Ok(baseline)
}

// ------------------------------------------------------------- elements

/// Draw and tag one content element.
// Rect math reads clearest in x/y/w/h; clippy's many_single_char_names
// is scoped to this one function on purpose.
#[allow(clippy::many_single_char_names)]
fn build_element(
    doc: &mut pdfloom::Document,
    page: usize,
    el: &ElementDef,
    resolve: &mut impl FnMut(&mut pdfloom::Document, &Option<FontDef>) -> Result<usize, DefError>,
) -> Result<(), DefError> {
    match el {
        ElementDef::Heading(hdr) => {
            let font = resolve(doc, &hdr.font)?;
            let f = hdr.font.clone().unwrap_or(FontDef {
                family: default_family(),
                style: default_style(),
                size: match hdr.level {
                    1 => 18.0,
                    2 => 15.0,
                    3 => 13.0,
                    _ => 11.0,
                },
                color: default_color(),
            });
            let tag = format!("H{}", hdr.level.clamp(1, 6));
            let mcid = doc.begin_tag(page, &tag);
            let rgb = hex_rgb(&f.color)?;
            draw_wrapped(doc, page, font, f.size, rgb, &hdr.text, hdr.place.rect)?;
            doc.end_tag(page);
            doc.push_node(
                page,
                Node::leaf(tag, hdr.text.clone(), font, f.size).with_pieces(vec![(page, mcid)]),
            );
        }
        ElementDef::Paragraph(para) => {
            let font = resolve(doc, &para.font)?;
            let f = para.font.clone().unwrap_or(FontDef {
                family: default_family(),
                style: default_style(),
                size: 11.0,
                color: default_color(),
            });
            let mcid = doc.begin_tag(page, "P");
            let rgb = hex_rgb(&f.color)?;
            draw_wrapped(doc, page, font, f.size, rgb, &para.text, para.place.rect)?;
            doc.end_tag(page);
            doc.push_node(
                page,
                Node::leaf("P", para.text.clone(), font, f.size).with_pieces(vec![(page, mcid)]),
            );
        }
        ElementDef::List(list) => {
            let font = resolve(doc, &list.font)?;
            let f = list.font.clone().unwrap_or(FontDef {
                family: default_family(),
                style: default_style(),
                size: 11.0,
                color: default_color(),
            });
            let rgb = hex_rgb(&f.color)?;
            let bullet = list.bullet.clone().unwrap_or_else(|| "\u{2022}".into());
            let [x, y, w, h] = list.place.rect;
            let top = y + h;
            let lh = f.size * 1.45;

            // L structure node with ListNumbering (UA-2 8.2.5.25).
            let mut list_node = Node::group("L");
            list_node
                .attrs
                .push(("ListNumbering".into(), "Disc".into()));

            let mut y = top;
            let indent = f.size * 1.2;
            for item in &list.items {
                // Lbl: the bullet, on its own line at the frame's left.
                let lbl_mcid = doc.begin_tag(page, "Lbl");
                doc.frame_text(page, x, y - f.size * 0.8, font, f.size, rgb, &bullet);
                doc.end_tag(page);
                // LBody: wrapped text, indented past the bullet.
                let lines = doc.wrap_text(font, f.size, item, w - indent)?;
                let body_mcid = doc.begin_tag(page, "LBody");
                let mut baseline = y - f.size * 0.8;
                for line in &lines {
                    doc.frame_text(page, x + indent, baseline, font, f.size, rgb, line);
                    baseline -= lh;
                }
                doc.end_tag(page);
                let n_lines = lines.len().max(1);
                y -= lh * n_lines as f64;

                list_node
                    .children
                    .push(Node::group("LI").with_children(vec![
                        Node::leaf("Lbl", bullet.clone(), font, f.size)
                            .with_pieces(vec![(page, lbl_mcid)]),
                        Node::leaf("LBody", item.clone(), font, f.size)
                            .with_pieces(vec![(page, body_mcid)]),
                    ]));
            }
            doc.push_node(page, list_node);
        }
        ElementDef::Table(tbl) => {
            let font = resolve(doc, &tbl.font)?;
            let f = tbl.font.clone().unwrap_or(FontDef {
                family: default_family(),
                style: default_style(),
                size: 10.0,
                color: default_color(),
            });
            let rgb = hex_rgb(&f.color)?;
            let [x0, y0, w, _h] = tbl.place.rect;
            let n_cols = tbl.header.len().max(tbl.rows.first().map_or(0, Vec::len));
            if n_cols == 0 {
                return Ok(());
            }
            let col_w: Vec<f64> = match &tbl.widths {
                Some(ws) if ws.len() == n_cols => ws.clone(),
                _ => vec![w / n_cols as f64; n_cols],
            };
            let lh = f.size * 1.45;
            let cell_pad = 4.0;

            // Grid (artifact) drawn above the decorations layer: it's
            // part of THIS element, so draw it right here inside an
            // artifact before the tagged cells.
            let rows_total = 1 + tbl.rows.len();
            let row_h = lh + 2.0 * cell_pad;
            let grid_h = row_h * rows_total as f64;
            doc.begin_artifact(page, "");
            let mut gx = x0;
            for cw in &col_w {
                doc.raw_ops(
                    page,
                    &format!(
                        "0.5 w {} {} {} {} re S\n",
                        pdfloom_core::fmt_real(gx),
                        pdfloom_core::fmt_real(y0),
                        pdfloom_core::fmt_real(*cw),
                        pdfloom_core::fmt_real(grid_h)
                    ),
                );
                gx += cw;
            }
            let mut gy = y0;
            for _ in 0..=rows_total {
                doc.raw_ops(
                    page,
                    &format!(
                        "0.5 w {} {} {} 0 re S\n",
                        pdfloom_core::fmt_real(x0),
                        pdfloom_core::fmt_real(gy),
                        pdfloom_core::fmt_real(w)
                    ),
                );
                gy += row_h;
            }
            doc.end_artifact(page);

            let mut table = Node::group("Table");
            // Header row.
            let mut hrow = Node::group("TR");
            let mut cx = x0;
            let hdr_top = y0 + grid_h;
            for (ci, cell) in tbl.header.iter().enumerate() {
                let cw = col_w[ci.min(col_w.len() - 1)];
                let mcid = doc.begin_tag(page, "TH");
                let lines = doc.wrap_text(font, f.size, cell, cw - 2.0 * cell_pad)?;
                let mut baseline = hdr_top - cell_pad - f.size * 0.8;
                for line in &lines {
                    doc.frame_text(page, cx + cell_pad, baseline, font, f.size, rgb, line);
                    baseline -= lh;
                }
                doc.end_tag(page);
                hrow.children.push(
                    Node::leaf("TH", cell.clone(), font, f.size)
                        .with_pieces(vec![(page, mcid)])
                        .with_scope("Column"),
                );
                cx += cw;
            }
            table.children.push(hrow);

            // Body rows.
            for (ri, row) in tbl.rows.iter().enumerate() {
                let row_top = hdr_top - row_h * (ri + 1) as f64;
                let mut tr = Node::group("TR");
                let mut cx = x0;
                for ci in 0..n_cols {
                    let cw = col_w[ci.min(col_w.len() - 1)];
                    let cell = row.get(ci).cloned().unwrap_or_default();
                    let mcid = doc.begin_tag(page, "TD");
                    let lines = doc.wrap_text(font, f.size, &cell, cw - 2.0 * cell_pad)?;
                    let mut baseline = row_top - cell_pad - f.size * 0.8;
                    for line in &lines {
                        doc.frame_text(page, cx + cell_pad, baseline, font, f.size, rgb, line);
                        baseline -= lh;
                    }
                    doc.end_tag(page);
                    tr.children
                        .push(Node::leaf("TD", cell, font, f.size).with_pieces(vec![(page, mcid)]));
                    cx += cw;
                }
                table.children.push(tr);
            }
            doc.push_node(page, table);
        }
        ElementDef::Image(imgdef) => {
            let [x, y, w, h] = imgdef.place.rect;
            let loaded = load_image(&imgdef.src)?;
            let res = doc.register_image(&loaded);
            if imgdef.alt.is_empty() {
                // Decorative: artifact, no Figure element.
                doc.begin_artifact(page, "");
                doc.place_image(page, &res, x, y, w, h);
                doc.end_artifact(page);
            } else {
                let mcid = doc.begin_tag(page, "Figure");
                doc.place_image(page, &res, x, y, w, h);
                doc.end_tag(page);
                let mut fig =
                    Node::leaf("Figure", String::new(), 0, 0.0).with_pieces(vec![(page, mcid)]);
                fig.alt = Some(imgdef.alt.clone());
                doc.push_node(page, fig);
            }
        }
        ElementDef::FormField(field) => {
            let font = resolve(doc, &field.font)?;
            let font_spec = field.font.clone().unwrap_or(FontDef {
                family: default_family(),
                style: default_style(),
                size: 11.0,
                color: default_color(),
            });
            let [x, y, w, h] = field.place.rect;
            // Label on the left ~40% of the frame, vertically centered.
            let label_w = w * 0.4;
            let baseline = y + h / 2.0 - font_spec.size * 0.35;
            let label_mcid = doc.begin_tag(page, "Caption");
            let rgb = hex_rgb(&font_spec.color)?;
            let label_lines = doc.wrap_text(font, font_spec.size, &field.label, label_w - 4.0)?;
            let mut b = baseline + font_spec.size * 0.8;
            for line in &label_lines {
                doc.frame_text(page, x, b, font, font_spec.size, rgb, line);
                b -= font_spec.size * 1.45;
            }
            doc.end_tag(page);
            // Widget box on the right (artifact decoration).
            let bx = x + label_w;
            let bw = (w - label_w).max(20.0);
            doc.begin_artifact(page, "");
            doc.raw_ops(
                page,
                &format!(
                    "0.8 w {} {} {} {} re S\n",
                    pdfloom_core::fmt_real(bx),
                    pdfloom_core::fmt_real(y),
                    pdfloom_core::fmt_real(bw),
                    pdfloom_core::fmt_real(h)
                ),
            );
            doc.end_artifact(page);
            // The field itself.
            doc.push_field(pdfloom::form::FieldSpec {
                name: field.name.clone(),
                tu: field.label.clone(),
                page,
                x: bx,
                y,
                w: bw,
                h,
            });
            // Structure: same shape the flow API uses for UA-1/UA-2.
            let caption = Node::leaf("Caption", field.label.clone(), font, font_spec.size)
                .with_pieces(vec![(page, label_mcid)]);
            let node = if doc.profile == Profile::PdfUa2 {
                Node::group("Form").with_children(vec![caption])
            } else {
                caption
            };
            doc.push_node(page, node);
        }
        ElementDef::Rect(_) | ElementDef::Line(_) => {
            // Drawn in the decorations pass already.
        }
    }
    Ok(())
}

/// Draw a decoration rectangle as an artifact.
fn draw_rect(doc: &mut pdfloom::Document, page: usize, rect: &RectDef) {
    let [x, y, w, h] = rect.place.rect;
    doc.begin_artifact(page, "");
    if let Some(fill) = &rect.fill {
        if let Ok((fr, fg, fb)) = hex_rgb(fill) {
            doc.raw_ops(
                page,
                &format!(
                    "{} {} {} rg {} {} {} {} re f\n",
                    pdfloom_core::fmt_real(fr),
                    pdfloom_core::fmt_real(fg),
                    pdfloom_core::fmt_real(fb),
                    pdfloom_core::fmt_real(x),
                    pdfloom_core::fmt_real(y),
                    pdfloom_core::fmt_real(w),
                    pdfloom_core::fmt_real(h)
                ),
            );
        }
    }
    if let Some(stroke) = &rect.stroke {
        if let Ok((sr, sg, sb)) = hex_rgb(stroke) {
            doc.raw_ops(
                page,
                &format!(
                    "{} {} {} RG {} w {} {} {} {} re S\n",
                    pdfloom_core::fmt_real(sr),
                    pdfloom_core::fmt_real(sg),
                    pdfloom_core::fmt_real(sb),
                    pdfloom_core::fmt_real(rect.stroke_width),
                    pdfloom_core::fmt_real(x),
                    pdfloom_core::fmt_real(y),
                    pdfloom_core::fmt_real(w),
                    pdfloom_core::fmt_real(h)
                ),
            );
        }
    }
    doc.end_artifact(page);
}

/// Draw a decoration line as an artifact.
fn draw_line(doc: &mut pdfloom::Document, page: usize, line: &LineDef) {
    let (sr, sg, sb) = hex_rgb(&line.stroke).unwrap_or((0.0, 0.0, 0.0));
    doc.begin_artifact(page, "");
    doc.raw_ops(
        page,
        &format!(
            "{} {} {} RG {} w {} {} m {} {} l S\n",
            pdfloom_core::fmt_real(sr),
            pdfloom_core::fmt_real(sg),
            pdfloom_core::fmt_real(sb),
            pdfloom_core::fmt_real(line.stroke_width),
            pdfloom_core::fmt_real(line.from[0]),
            pdfloom_core::fmt_real(line.from[1]),
            pdfloom_core::fmt_real(line.to[0]),
            pdfloom_core::fmt_real(line.to[1])
        ),
    );
    doc.end_artifact(page);
}

// -------------------------------------------------------------- helpers

/// `#rrggbb` -> 0..1 RGB. Accepts `#rgb` shorthand too.
fn hex_rgb(color: &str) -> Result<(f64, f64, f64), DefError> {
    let digits = color.strip_prefix('#').unwrap_or(color);
    let (r, g, b) = if digits.len() == 6 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        (
            u8::from_str_radix(&digits[0..2], 16),
            u8::from_str_radix(&digits[2..4], 16),
            u8::from_str_radix(&digits[4..6], 16),
        )
    } else if digits.len() == 3 && digits.chars().all(|c| c.is_ascii_hexdigit()) {
        (
            u8::from_str_radix(&digits[0..1].repeat(2), 16),
            u8::from_str_radix(&digits[1..2].repeat(2), 16),
            u8::from_str_radix(&digits[2..3].repeat(2), 16),
        )
    } else {
        return Err(DefError::Build(format!(
            "bad color \"{color}\" (want #rrggbb)"
        )));
    };
    let (r, g, b) = (
        r.map_err(|e| DefError::Build(e.to_string()))?,
        g.map_err(|e| DefError::Build(e.to_string()))?,
        b.map_err(|e| DefError::Build(e.to_string()))?,
    );
    Ok((
        f64::from(r) / 255.0,
        f64::from(g) / 255.0,
        f64::from(b) / 255.0,
    ))
}

/// Load an image from a path or `data:image/...;base64,...` URL.
fn load_image(src: &str) -> Result<pdfloom::Image, DefError> {
    if let Some(rest) = src.strip_prefix("data:") {
        let b64 = rest.split(',').nth(1).unwrap_or("");
        let bytes = decode_base64(b64)
            .ok_or_else(|| DefError::Build("invalid base64 image data".into()))?;
        // Image::load reads from a path; write the decoded bytes to a
        // short-lived temp file. The decode happens eagerly, so the
        // file can go right after.
        let tmp = std::env::temp_dir().join(format!(
            "pdfloom-def-img-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::write(&tmp, bytes)?;
        let img = pdfloom::Image::load(
            tmp.to_str()
                .ok_or_else(|| DefError::Build("temp path not UTF-8".into()))?,
        );
        let _ = std::fs::remove_file(&tmp);
        img.map_err(|e| DefError::Build(e.to_string()))
    } else {
        pdfloom::Image::load(src).map_err(|e| DefError::Build(e.to_string()))
    }
}

/// Minimal standard-alphabet base64 decoder (no padding required).
fn decode_base64(s: &str) -> Option<Vec<u8>> {
    fn val(c: u8) -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    }
    let cleaned: Vec<u8> = s
        .bytes()
        .filter(|&b| b != b'=' && b != b'\n' && b != b'\r' && b != b' ')
        .collect();
    let mut out = Vec::with_capacity(cleaned.len() * 3 / 4);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for &c in &cleaned {
        acc = (acc << 6) | val(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}
