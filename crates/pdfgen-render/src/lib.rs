//! Rasterize PDF pages to in-memory RGBA bitmaps — the display side of the
//! "printing/displaying PDFs" pipeline.
//!
//! Scope: the operator subset our own writer (and most text documents)
//! emits — text (Tf/Td/Tj with WinAnsi or Identity-H CID fonts), rects,
//! lines, fills, and image XObjects — rendered onto a white background.
//! Glyphs rasterize from the embedded FontFile2 program via a scanline
//! outline filler.

use pdfgen_core::Object;
use pdfgen_parse::PdfReader;

/// A rendered page: width/height in pixels, RGBA data (row-major,
/// top-down).
#[derive(Debug, Clone)]
pub struct Bitmap {
    pub width: u32,
    pub height: u32,
    /// RGBA, 4 bytes per pixel.
    pub rgba: Vec<u8>,
}

impl Bitmap {
    fn new(width: u32, height: u32) -> Self {
        Bitmap {
            width,
            height,
            rgba: vec![0u8; (width as usize) * (height as usize) * 4],
        }
    }

    /// Set a pixel (device coords, y down); clips silently. Alpha blends.
    pub fn set(&mut self, x: i64, y: i64, r: u8, g: u8, b: u8, a: u8) {
        if x < 0 || y < 0 || x >= self.width as i64 || y >= self.height as i64 {
            return;
        }
        let off = (y as usize * self.width as usize + x as usize) * 4;
        let dst = &mut self.rgba[off..off + 4];
        if a == 255 {
            dst[0] = r;
            dst[1] = g;
            dst[2] = b;
            dst[3] = 255;
        } else if a > 0 {
            let af = f64::from(a) / 255.0;
            dst[0] = (f64::from(dst[0]) * (1.0 - af) + f64::from(r) * af) as u8;
            dst[1] = (f64::from(dst[1]) * (1.0 - af) + f64::from(g) * af) as u8;
            dst[2] = (f64::from(dst[2]) * (1.0 - af) + f64::from(b) * af) as u8;
            dst[3] = u8::max(dst[3], a);
        }
    }

    /// Encode as a PNG (no external crates: raw chunk writer).
    pub fn to_png(&self) -> Result<Vec<u8>, String> {
        let mut out = Vec::new();
        out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a]);
        // IHDR
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&self.width.to_be_bytes());
        ihdr.extend_from_slice(&self.height.to_be_bytes());
        ihdr.push(8); // bit depth
        ihdr.push(6); // color type RGBA
        ihdr.push(0); // compression
        ihdr.push(0); // filter
        ihdr.push(0); // interlace
        chunk(&mut out, b"IHDR", &ihdr);
        // IDAT: filter byte 0 per row, then zlib (store-mode deflate via
        // flate2 for correctness).
        let mut raw = Vec::with_capacity(self.rgba.len() + self.height as usize);
        for row in self.rgba.chunks_exact(self.width as usize * 4) {
            raw.push(0u8);
            raw.extend_from_slice(row);
        }
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write as _;
        let mut enc = ZlibEncoder::new(Vec::new(), Compression::default());
        enc.write_all(&raw).map_err(|e| e.to_string())?;
        let idat = enc.finish().map_err(|e| e.to_string())?;
        chunk(&mut out, b"IDAT", &idat);
        chunk(&mut out, b"IEND", &[]);
        Ok(out)
    }
}

fn chunk(out: &mut Vec<u8>, tag: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    out.extend_from_slice(tag);
    out.extend_from_slice(data);
    let mut crc_data = Vec::with_capacity(4 + data.len());
    crc_data.extend_from_slice(tag);
    crc_data.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_data).to_be_bytes());
}

fn crc32(data: &[u8]) -> u32 {
    // Standard PNG CRC-32 (polynomial 0xEDB88320), table computed inline.
    let mut table = [0u32; 256];
    for (i, t) in table.iter_mut().enumerate() {
        let mut c = i as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 {
                0xEDB8_8320 ^ (c >> 1)
            } else {
                c >> 1
            };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ u32::from(b)) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

/// Render options.
#[derive(Debug, Clone, Copy)]
pub struct RenderOptions {
    /// Raster density; 72 = 1 point per pixel.
    pub dpi: f64,
    /// White background instead of transparent.
    pub white_background: bool,
}

impl Default for RenderOptions {
    fn default() -> Self {
        RenderOptions {
            dpi: 150.0,
            white_background: true,
        }
    }
}

/// Render one page (0-based index) of a PDF file.
pub fn render_page(path: &str, page_index: usize, opts: RenderOptions) -> Result<Bitmap, String> {
    let mut reader = PdfReader::open(path).map_err(|e| e.to_string())?;
    render_page_reader(&mut reader, page_index, opts)
}

/// Render with an existing reader (parse once, render many pages).
pub fn render_page_reader(
    reader: &mut PdfReader,
    page_index: usize,
    opts: RenderOptions,
) -> Result<Bitmap, String> {
    let pages = collect_pages(reader)?;
    let page_id = *pages
        .get(page_index)
        .ok_or_else(|| format!("page {page_index} out of range ({} pages)", pages.len()))?;
    let Object::Dict(page) = reader.get(page_id).map_err(|e| e.to_string())? else {
        return Err("page object is not a dict".into());
    };

    let (pw, ph) = media_box(reader, &page)?;
    let scale = opts.dpi / 72.0;
    let width = (pw * scale).round().max(1.0) as u32;
    let height = (ph * scale).round().max(1.0) as u32;
    let mut bmp = Bitmap::new(width, height);
    if opts.white_background {
        for px in bmp.rgba.chunks_exact_mut(4) {
            px.copy_from_slice(&[255, 255, 255, 255]);
        }
    }

    let content: Vec<u8> = page_content(reader, &page)?;
    if content.is_empty() {
        return Ok(bmp);
    }
    let fonts = collect_fonts(reader, &page)?;
    let xobjects = collect_xobjects(reader, &page)?;

    run_ops(&content, &mut bmp, scale, ph, &fonts, &xobjects)?;
    Ok(bmp)
}

/// Render every page.
pub fn render_all(path: &str, opts: RenderOptions) -> Result<Vec<Bitmap>, String> {
    let mut reader = PdfReader::open(path).map_err(|e| e.to_string())?;
    let n = collect_pages(&mut reader)?.len();
    (0..n)
        .map(|i| render_page_reader(&mut reader, i, opts))
        .collect()
}

// ------------------------------------------------------------- page model

fn collect_pages(reader: &mut PdfReader) -> Result<Vec<u32>, String> {
    let catalog = reader.catalog().map_err(|e| e.to_string())?;
    let Some(Object::Ref(pages_ref)) = catalog.get("Pages").cloned() else {
        return Err("catalog has no /Pages".into());
    };
    let mut out = Vec::new();
    walk_pages(reader, pages_ref.id, &mut out)?;
    Ok(out)
}

fn walk_pages(reader: &mut PdfReader, node_id: u32, out: &mut Vec<u32>) -> Result<(), String> {
    let node = reader.get(node_id).map_err(|e| e.to_string())?;
    let Object::Dict(d) = node else {
        return Ok(());
    };
    match d.get("Kids").cloned() {
        Some(Object::Array(kids)) => {
            for k in kids {
                if let Object::Ref(r) = k {
                    walk_pages(reader, r.id, out)?;
                }
            }
        }
        _ => out.push(node_id),
    }
    Ok(())
}

fn media_box(reader: &mut PdfReader, page: &pdfgen_core::Dict) -> Result<(f64, f64), String> {
    let mut node = page.clone();
    loop {
        if let Some(Object::Array(a)) = node.get("MediaBox").cloned() {
            let nums: Vec<f64> = a
                .iter()
                .filter_map(|o| match o {
                    Object::Int(v) => Some(*v as f64),
                    Object::Real(v) => Some(v.0),
                    _ => None,
                })
                .collect();
            if nums.len() == 4 {
                return Ok((nums[2] - nums[0], nums[3] - nums[1]));
            }
        }
        match node.get("Parent").cloned() {
            Some(Object::Ref(p)) => {
                let Object::Dict(d) = reader.get(p.id).map_err(|e| e.to_string())? else {
                    return Ok((612.0, 792.0));
                };
                node = d;
            }
            _ => return Ok((612.0, 792.0)),
        }
    }
}

fn page_content(reader: &mut PdfReader, page: &pdfgen_core::Dict) -> Result<Vec<u8>, String> {
    let mut joined = Vec::new();
    let contents = match page.get("Contents").cloned() {
        Some(Object::Ref(r)) => vec![Object::Ref(r)],
        Some(Object::Array(a)) => a,
        _ => return Ok(joined),
    };
    for c in contents {
        let Object::Ref(r) = c else { continue };
        let Ok(Object::Stream(s)) = reader.get(r.id) else {
            continue;
        };
        let data: Vec<u8> = match s.dict.get("Filter") {
            Some(Object::Name(n)) if n.0 == "FlateDecode" => inflate(&s.data)?,
            _ => s.data.clone(),
        };
        joined.extend_from_slice(&data);
    }
    Ok(joined)
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    use flate2::read::ZlibDecoder;
    use std::io::Read as _;
    let mut out = Vec::new();
    ZlibDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| format!("zlib: {e}"))?;
    Ok(out)
}

/// A resolved font for glyph rasterization.
struct RasterFont {
    program: Vec<u8>,
    /// Identity-H Type0 (2-byte codes) vs simple WinAnsi.
    cid: bool,
    /// CIDToGIDMap stream bytes, when present.
    cid_to_gid: Option<Vec<u8>>,
}

fn collect_fonts(
    reader: &mut PdfReader,
    page: &pdfgen_core::Dict,
) -> Result<Vec<(String, RasterFont)>, String> {
    let mut out = Vec::new();
    let Some(Object::Dict(res)) = page.get("Resources").cloned() else {
        return Ok(out);
    };
    let Some(Object::Dict(fonts)) = res.get("Font").cloned() else {
        return Ok(out);
    };
    for (name, spec) in fonts.0.iter() {
        let Object::Ref(r) = spec else { continue };
        let Ok(Object::Dict(fd)) = reader.get(r.id) else {
            continue;
        };
        let subtype = match fd.get("Subtype") {
            Some(Object::Name(n)) => n.0.clone(),
            _ => continue,
        };
        let cid = subtype == "Type0";
        let mut program: Option<Vec<u8>> = None;
        let mut cid_to_gid = None;
        if cid {
            if let Some(Object::Array(kids)) = fd.get("DescendantFonts").cloned() {
                if let Some(Object::Ref(d)) = kids.first() {
                    if let Ok(Object::Dict(dd)) = reader.get(d.id) {
                        if let Some(Object::Ref(pd)) = dd.get("FontDescriptor").cloned() {
                            program = load_font_file2(reader, pd.id);
                        }
                        if let Some(Object::Ref(cm)) = dd.get("CIDToGIDMap").cloned() {
                            if let Ok(Object::Stream(s)) = reader.get(cm.id) {
                                cid_to_gid = Some(match s.dict.get("Filter") {
                                    Some(Object::Name(n)) if n.0 == "FlateDecode" => {
                                        inflate(&s.data).unwrap_or_else(|_| s.data.clone())
                                    }
                                    _ => s.data.clone(),
                                });
                            }
                        }
                    }
                }
            }
        } else if let Some(Object::Ref(pd)) = fd.get("FontDescriptor").cloned() {
            program = load_font_file2(reader, pd.id);
        }
        let Some(program) = program else { continue };
        out.push((
            name.0.clone(),
            RasterFont {
                program,
                cid,
                cid_to_gid,
            },
        ));
    }
    Ok(out)
}

fn load_font_file2(reader: &mut PdfReader, desc_id: u32) -> Option<Vec<u8>> {
    let Object::Dict(desc) = reader.get(desc_id).ok()? else {
        return None;
    };
    let Object::Ref(ff) = desc.get("FontFile2").cloned()? else {
        return None;
    };
    let Object::Stream(s) = reader.get(ff.id).ok()? else {
        return None;
    };
    Some(match s.dict.get("Filter") {
        Some(Object::Name(n)) if n.0 == "FlateDecode" => {
            inflate(&s.data).unwrap_or_else(|_| s.data.clone())
        }
        _ => s.data.clone(),
    })
}

struct PageImage {
    width: u32,
    height: u32,
    /// RGB8 rows.
    rgb: Vec<u8>,
}

fn collect_xobjects(
    reader: &mut PdfReader,
    page: &pdfgen_core::Dict,
) -> Result<Vec<(String, PageImage)>, String> {
    let mut out = Vec::new();
    let Some(Object::Dict(res)) = page.get("Resources").cloned() else {
        return Ok(out);
    };
    let Some(Object::Dict(xo)) = res.get("XObject").cloned() else {
        return Ok(out);
    };
    for (name, spec) in xo.0.iter() {
        let Object::Ref(r) = spec else { continue };
        let Ok(Object::Stream(s)) = reader.get(r.id) else {
            continue;
        };
        let Some(Object::Name(st)) = s.dict.get("Subtype").cloned() else {
            continue;
        };
        if st.0 != "Image" {
            continue;
        }
        let w = match s.dict.get("Width") {
            Some(Object::Int(v)) => *v as u32,
            _ => continue,
        };
        let h = match s.dict.get("Height") {
            Some(Object::Int(v)) => *v as u32,
            _ => continue,
        };
        let bpc = match s.dict.get("BitsPerComponent") {
            Some(Object::Int(v)) => *v as u32,
            _ => 8,
        };
        let cs = match s.dict.get("ColorSpace").cloned() {
            Some(Object::Name(n)) => n.0,
            Some(Object::Array(a)) => match a.first() {
                Some(Object::Name(n)) => n.0.clone(),
                _ => "DeviceRGB".into(),
            },
            _ => "DeviceRGB".into(),
        };
        let data: Vec<u8> = match s.dict.get("Filter") {
            Some(Object::Name(n)) if n.0 == "FlateDecode" => inflate(&s.data)?,
            _ => s.data.clone(),
        };
        let rgb = decode_image(&data, w, h, bpc, &cs)?;
        out.push((
            name.0.clone(),
            PageImage {
                width: w,
                height: h,
                rgb,
            },
        ));
    }
    Ok(out)
}

fn decode_image(data: &[u8], w: u32, h: u32, bpc: u32, cs: &str) -> Result<Vec<u8>, String> {
    if bpc != 8 {
        return Err("only 8bpc images render for now".into());
    }
    match cs {
        "DeviceRGB" | "RGB" => Ok(data.to_vec()),
        "DeviceGray" | "G" => {
            let n = (w as usize) * (h as usize);
            if data.len() < n {
                return Err("short image data".into());
            }
            let mut out = Vec::with_capacity(n * 3);
            for &g in &data[..n] {
                out.extend_from_slice(&[g, g, g]);
            }
            Ok(out)
        }
        _ => Err(format!("unsupported colorspace {cs}")),
    }
}

// ------------------------------------------------------------- operators

/// Graphics state subset our writer emits.
struct Gfx {
    x: f64,
    y: f64,
    font: Option<String>,
    font_size: f64,
    fill: (u8, u8, u8),
    /// Path start (m) and last point for l.
    path_x: f64,
    path_y: f64,
    /// Whether a re should fill or stroke (set by the last f/S).
    last_fill: bool,
    /// Numeric operands for the current operator.
    nums: Vec<f64>,
    /// The name operand (/F0, /Im1) seen before Tf or Do.
    pending_name: Option<String>,
}

fn run_ops(
    content: &[u8],
    bmp: &mut Bitmap,
    scale: f64,
    page_h: f64,
    fonts: &[(String, RasterFont)],
    xobjects: &[(String, PageImage)],
) -> Result<(), String> {
    let mut g = Gfx {
        x: 0.0,
        y: 0.0,
        font: None,
        font_size: 12.0,
        fill: (0, 0, 0),
        path_x: 0.0,
        path_y: 0.0,
        last_fill: false,
        nums: Vec::new(),
        pending_name: None,
    };

    let mut i = 0usize;
    while i < content.len() {
        if content[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let b = content[i];

        // String literal: (...) — find the operator after it.
        if b == b'(' {
            let (bytes, next) = read_string(content, i);
            i = next;
            // Skip whitespace, then read the operator word.
            while i < content.len() && content[i].is_ascii_whitespace() {
                i += 1;
            }
            let ws = i;
            while i < content.len() && content[i].is_ascii_alphabetic() {
                i += 1;
            }
            let op = &content[ws..i];
            if op == b"Tj" || op == b"'" || op == b"\"" {
                show_text(&mut g, &bytes, bmp, scale, page_h, fonts);
            }
            g.nums.clear();
            continue;
        }

        // Name: /Word
        if b == b'/' {
            let mut j = i + 1;
            while j < content.len() && !content[j].is_ascii_whitespace() && content[j] != b'(' {
                j += 1;
            }
            g.pending_name = Some(String::from_utf8_lossy(&content[i + 1..j]).into_owned());
            i = j;
            continue;
        }

        // Number or operator word.
        if b.is_ascii_alphanumeric() || b == b'-' || b == b'+' || b == b'.' {
            let mut j = i;
            while j < content.len()
                && !content[j].is_ascii_whitespace()
                && content[j] != b'('
                && content[j] != b'/'
            {
                j += 1;
            }
            let word = &content[i..j];
            i = j;

            let is_number = word
                .first()
                .is_some_and(|c| c.is_ascii_digit() || *c == b'-' || *c == b'+' || *c == b'.');
            if is_number {
                g.nums.push(
                    std::str::from_utf8(word)
                        .ok()
                        .and_then(|v| v.parse::<f64>().ok())
                        .unwrap_or(0.0),
                );
                continue;
            }

            match word {
                b"BT" => {
                    g.x = 0.0;
                    g.y = 0.0;
                    g.nums.clear();
                }
                b"ET" => {
                    g.nums.clear();
                }
                b"Tf" => {
                    g.font_size = g.nums.pop().unwrap_or(12.0);
                    g.font = g.pending_name.take();
                    g.nums.clear();
                }
                b"Td" | b"TD" => {
                    let y = g.nums.pop().unwrap_or(0.0);
                    let x = g.nums.pop().unwrap_or(0.0);
                    g.x += x;
                    g.y += y;
                    g.nums.clear();
                }
                b"Tm" => {
                    // a b c d e f: position is e, f.
                    if g.nums.len() >= 6 {
                        let f = g.nums.pop().unwrap_or(0.0);
                        let e = g.nums.pop().unwrap_or(0.0);
                        g.nums.clear();
                        g.x = e;
                        g.y = f;
                    } else {
                        g.nums.clear();
                    }
                }
                b"TL" | b"Tc" | b"Tw" | b"Tz" | b"Ts" | b"Tr" => {
                    g.nums.clear();
                }
                b"m" => {
                    if g.nums.len() >= 2 {
                        g.path_y = g.nums.pop().unwrap_or(0.0);
                        g.path_x = g.nums.pop().unwrap_or(0.0);
                    }
                    g.nums.clear();
                }
                b"l" => {
                    if g.nums.len() >= 2 {
                        let y2 = g.nums.pop().unwrap_or(0.0);
                        let x2 = g.nums.pop().unwrap_or(0.0);
                        draw_line(bmp, scale, page_h, g.path_x, g.path_y, x2, y2, g.fill);
                        g.path_x = x2;
                        g.path_y = y2;
                    }
                    g.nums.clear();
                }
                b"re" => {
                    if g.nums.len() >= 4 {
                        let h4 = g.nums.pop().unwrap_or(0.0);
                        let w4 = g.nums.pop().unwrap_or(0.0);
                        let y4 = g.nums.pop().unwrap_or(0.0);
                        let x4 = g.nums.pop().unwrap_or(0.0);
                        draw_rect(bmp, scale, page_h, x4, y4, w4, h4, g.fill, g.last_fill);
                        g.path_x = x4;
                        g.path_y = y4;
                    }
                    g.nums.clear();
                }
                b"f" | b"F" | b"f*" | b"B" | b"B*" | b"b" | b"b*" => {
                    g.last_fill = true;
                    g.nums.clear();
                }
                b"S" | b"s" => {
                    g.last_fill = false;
                    g.nums.clear();
                }
                b"g" => {
                    if let Some(v) = g.nums.pop() {
                        let c = (v * 255.0).clamp(0.0, 255.0) as u8;
                        g.fill = (c, c, c);
                    }
                    g.nums.clear();
                }
                b"rg" => {
                    if g.nums.len() >= 3 {
                        let b3 = g.nums.pop().unwrap_or(0.0);
                        let g3 = g.nums.pop().unwrap_or(0.0);
                        let r3 = g.nums.pop().unwrap_or(0.0);
                        g.fill = (
                            (r3 * 255.0).clamp(0.0, 255.0) as u8,
                            (g3 * 255.0).clamp(0.0, 255.0) as u8,
                            (b3 * 255.0).clamp(0.0, 255.0) as u8,
                        );
                    }
                    g.nums.clear();
                }
                b"Do" => {
                    if let Some(name) = g.pending_name.take() {
                        if let Some((_, img)) = xobjects.iter().find(|(n, _)| *n == name) {
                            draw_image(bmp, scale, page_h, img);
                        }
                    }
                    g.nums.clear();
                }
                b"cm" | b"q" | b"Q" | b"W" | b"W*" | b"n" | b"gs" | b"cs" | b"CS" | b"sc"
                | b"scn" | b"SC" | b"SCN" | b"G" | b"RG" | b"K" | b"k" | b"sh" | b"BI" | b"ID"
                | b"EI" => {
                    g.nums.clear();
                }
                _ => {
                    // Unknown operator: clear operands so they cannot leak
                    // into the next one.
                    g.nums.clear();
                }
            }
            continue;
        }

        // Anything else (dicts << >>, arrays, hex strings): skip a byte.
        i += 1;
    }
    Ok(())
}

/// Read a (…)-string starting at `i` (content[i] == b'('). Returns the
/// unescaped bytes and the index just past the closing paren.
fn read_string(content: &[u8], i: usize) -> (Vec<u8>, usize) {
    let mut j = i + 1;
    let mut depth = 1;
    let mut bytes = Vec::new();
    while j < content.len() {
        match content[j] {
            b'\\' => {
                if j + 1 < content.len() {
                    bytes.push(content[j + 1]);
                    j += 2;
                    continue;
                }
                j += 1;
            }
            b'(' => {
                depth += 1;
                bytes.push(content[j]);
                j += 1;
            }
            b')' => {
                depth -= 1;
                if depth == 0 {
                    j += 1;
                    break;
                }
                bytes.push(content[j]);
                j += 1;
            }
            c => {
                bytes.push(c);
                j += 1;
            }
        }
    }
    (bytes, j)
}

fn show_text(
    g: &mut Gfx,
    bytes: &[u8],
    bmp: &mut Bitmap,
    scale: f64,
    page_h: f64,
    fonts: &[(String, RasterFont)],
) {
    let Some(fname) = g.font.clone() else { return };
    let Some((_, rf)) = fonts.iter().find(|(n, _)| *n == fname) else {
        return;
    };
    let Ok(face) = ttf_parser::Face::parse(&rf.program, 0) else {
        return;
    };
    let upem = f64::from(face.units_per_em());
    let size_px = g.font_size * scale;

    // Decode the byte stream to glyph IDs.
    let gids: Vec<u16> = if rf.cid {
        bytes
            .chunks_exact(2)
            .map(|p| {
                let cid = u16::from_be_bytes([p[0], p[1]]);
                match &rf.cid_to_gid {
                    Some(m) if (cid as usize) * 2 + 2 <= m.len() => {
                        u16::from_be_bytes([m[cid as usize * 2], m[cid as usize * 2 + 1]])
                    }
                    _ => cid,
                }
            })
            .collect()
    } else {
        bytes
            .iter()
            .filter_map(|&b| {
                let unit = pdfgen_font::winansi::unit_for_byte(b)?;
                let ch = char::from_u32(u32::from(unit))?;
                face.glyph_index(ch).map(|gid| gid.0)
            })
            .collect()
    };

    // Baseline in device space (PDF y-up -> bitmap y-down).
    let baseline_dev = ((page_h - g.y) * scale) as i64;
    let mut pen_dev = (g.x * scale) as i64;

    for gid_v in gids {
        let gid = ttf_parser::GlyphId(gid_v);
        if let Some((w, h, top, left, cov)) = rasterize_glyph(&face, gid, size_px, upem) {
            let dx = pen_dev + left;
            let dy = baseline_dev - top - h as i64;
            for (row, row_bytes) in cov.chunks_exact(w as usize).enumerate() {
                for (col, &a) in row_bytes.iter().enumerate() {
                    if a > 0 {
                        let (r, gg, b) = g.fill;
                        bmp.set(dx + col as i64, dy + row as i64, r, gg, b, a);
                    }
                }
            }
        }
        let advance = face
            .glyph_hor_advance(gid)
            .map(|a| f64::from(a))
            .unwrap_or(upem / 2.0)
            * size_px
            / upem;
        pen_dev += advance.round() as i64;
    }
    g.x = pen_dev as f64 / scale;
}

/// Rasterize one glyph outline to a coverage bitmap.
/// Returns (width, height, top_bearing_px, left_bearing_px, coverage rows).
fn rasterize_glyph(
    face: &ttf_parser::Face,
    gid: ttf_parser::GlyphId,
    size_px: f64,
    upem: f64,
) -> Option<(u32, u32, i64, i64, Vec<u8>)> {
    let mut sink = OutlineSink::default();
    face.outline_glyph(gid, &mut sink)?;
    if sink.edges.is_empty() {
        return None;
    }
    let s = size_px / upem;
    // Font units (y up) -> pixel space (y down).
    let to_px = |(x, y): (f64, f64)| (x * s, -y * s);

    let (min_x, max_x) = sink
        .edges
        .iter()
        .flat_map(|e| [to_px(e.0), to_px(e.1)])
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), (x, _)| {
            (mn.min(x), mx.max(x))
        });
    let (min_y, max_y) = sink
        .edges
        .iter()
        .flat_map(|e| [to_px(e.0), to_px(e.1)])
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(mn, mx), (_, y)| {
            (mn.min(y), mx.max(y))
        });

    let pad = 1.0;
    let w = ((max_x - min_x) + 2.0 * pad).ceil().max(1.0) as u32;
    let h = ((max_y - min_y) + 2.0 * pad).ceil().max(1.0) as u32;
    if w > 4096 || h > 4096 {
        return None;
    }
    let x0 = min_x - pad;
    let y0 = min_y - pad;

    let mut cov = vec![0u8; (w as usize) * (h as usize)];

    // Nonzero-winding scanline fill.
    for py in 0..h {
        let sy = y0 + f64::from(py) + 0.5;
        let mut xs: Vec<(f64, i32)> = Vec::new();
        for e in &sink.edges {
            let (x1, y1) = to_px(e.0);
            let (x2, y2) = to_px(e.1);
            let (y_lo, y_hi, xa, xb, dir) = if y1 < y2 {
                (y1, y2, x1, x2, 1)
            } else {
                (y2, y1, x2, x1, -1)
            };
            if sy >= y_lo && sy < y_hi {
                let t = (sy - y_lo) / (y_hi - y_lo);
                xs.push((xa + (xb - xa) * t, dir));
            }
        }
        if xs.len() < 2 {
            continue;
        }
        xs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());
        let mut winding = 0i32;
        let mut span_start = 0f64;
        for (cx, dir) in xs {
            let before = winding;
            winding += dir;
            if before == 0 && winding != 0 {
                span_start = cx;
            } else if before != 0 && winding == 0 {
                let a = ((span_start - x0).round() as i64).max(0) as u32;
                let b = ((cx - x0).round() as i64).max(0) as u32;
                for px in a.min(w)..b.min(w) {
                    cov[(py * w + px) as usize] = 255;
                }
            }
        }
    }

    let top_px = (-max_y - pad).round() as i64; // distance from baseline up
    let left_px = (min_x - pad).round() as i64;
    Some((w, h, top_px, left_px, cov))
}

#[derive(Default)]
struct OutlineSink {
    edges: Vec<((f64, f64), (f64, f64))>,
    cur: (f64, f64),
    started: bool,
}

impl ttf_parser::OutlineBuilder for OutlineSink {
    fn move_to(&mut self, x: f32, y: f32) {
        self.cur = (f64::from(x), f64::from(y));
        self.started = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        if !self.started {
            return;
        }
        let next = (f64::from(x), f64::from(y));
        self.edges.push((self.cur, next));
        self.cur = next;
    }
    fn quad_to(&mut self, cx: f32, cy: f32, x: f32, y: f32) {
        if !self.started {
            return;
        }
        let p0 = self.cur;
        let (cx, cy) = (f64::from(cx), f64::from(cy));
        let end = (f64::from(x), f64::from(y));
        let n = 8;
        let mut last = p0;
        for k in 1..=n {
            let t = f64::from(k) / f64::from(n);
            let mt = 1.0 - t;
            let bx = mt * mt * p0.0 + 2.0 * mt * t * cx + t * t * end.0;
            let by = mt * mt * p0.1 + 2.0 * mt * t * cy + t * t * end.1;
            self.edges.push((last, (bx, by)));
            last = (bx, by);
        }
        self.cur = end;
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if !self.started {
            return;
        }
        let p0 = self.cur;
        let (c1, c2) = (
            (f64::from(x1), f64::from(y1)),
            (f64::from(x2), f64::from(y2)),
        );
        let end = (f64::from(x), f64::from(y));
        let n = 12;
        let mut last = p0;
        for k in 1..=n {
            let t = f64::from(k) / f64::from(n);
            let mt = 1.0 - t;
            let bx = mt * mt * mt * p0.0
                + 3.0 * mt * mt * t * c1.0
                + 3.0 * mt * t * t * c2.0
                + t * t * t * end.0;
            let by = mt * mt * mt * p0.1
                + 3.0 * mt * mt * t * c1.1
                + 3.0 * mt * t * t * c2.1
                + t * t * t * end.1;
            self.edges.push((last, (bx, by)));
            last = (bx, by);
        }
        self.cur = end;
    }
    fn close(&mut self) {
        self.started = false;
    }
}

fn draw_rect(
    bmp: &mut Bitmap,
    scale: f64,
    page_h: f64,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    fill: (u8, u8, u8),
    filled: bool,
) {
    let px = (x * scale) as i64;
    let py = ((page_h - y - h) * scale) as i64;
    let pw = ((w * scale).ceil() as i64).max(1);
    let ph = ((h * scale).ceil() as i64).max(1);
    if filled {
        for yy in py..py + ph {
            for xx in px..px + pw {
                bmp.set(xx, yy, fill.0, fill.1, fill.2, 255);
            }
        }
    } else {
        for xx in px..px + pw {
            bmp.set(xx, py, fill.0, fill.1, fill.2, 255);
            bmp.set(xx, py + ph - 1, fill.0, fill.1, fill.2, 255);
        }
        for yy in py..py + ph {
            bmp.set(px, yy, fill.0, fill.1, fill.2, 255);
            bmp.set(px + pw - 1, yy, fill.0, fill.1, fill.2, 255);
        }
    }
}

fn draw_line(
    bmp: &mut Bitmap,
    scale: f64,
    page_h: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    fill: (u8, u8, u8),
) {
    let ax = (x1 * scale) as i64;
    let ay = ((page_h - y1) * scale) as i64;
    let bx = (x2 * scale) as i64;
    let by = ((page_h - y2) * scale) as i64;
    let (dx, dy) = (bx - ax, by - ay);
    let steps = dx.abs().max(dy.abs()).max(1);
    for s in 0..=steps {
        let t = s as f64 / steps as f64;
        let xx = (ax as f64 + dx as f64 * t).round() as i64;
        let yy = (ay as f64 + dy as f64 * t).round() as i64;
        bmp.set(xx, yy, fill.0, fill.1, fill.2, 255);
    }
}

fn draw_image(bmp: &mut Bitmap, scale: f64, page_h: f64, img: &PageImage) {
    // Draw at the page origin, one image pixel per point (how our writer
    // sizes Do boxes for untorn placements).
    let w = (f64::from(img.width) * scale) as i64;
    let h = (f64::from(img.height) * scale) as i64;
    let y0 = (page_h * scale) as i64 - h;
    for yy in 0..h {
        let sy = ((yy as f64 / h as f64) * f64::from(img.height)) as u32;
        for xx in 0..w {
            let sx = ((xx as f64 / w as f64) * f64::from(img.width)) as u32;
            let off = ((sy * img.width + sx) as usize) * 3;
            let (r, g, b) = (
                img.rgb.get(off).copied().unwrap_or(255),
                img.rgb.get(off + 1).copied().unwrap_or(255),
                img.rgb.get(off + 2).copied().unwrap_or(255),
            );
            bmp.set(xx, y0 + yy, r, g, b, 255);
        }
    }
}
