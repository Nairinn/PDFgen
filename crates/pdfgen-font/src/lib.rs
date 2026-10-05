pub use winansi::encode as winansi_encode;
pub use winansi::encode_char as winansi_encode_char;
pub use winansi::unit_for_byte as winansi_unit_for_byte;

pub mod winansi;
pub mod cid;
pub mod ttc;

use std::path::Path;
use thiserror::Error;
use ttf_parser::Face;

/// Errors while loading a font.
#[derive(Debug, Error)]
pub enum FontError {
    /// The file could not be read.
    #[error("cannot read font file {path}: {source}")]
    Io {
        /// Path attempted.
        path: String,
        /// Underlying IO error.
        #[source]
        source: std::io::Error,
    },
    /// The bytes are not a parsable font.
    #[error("cannot parse font: {0}")]
    Parse(String),
    /// The font has no usable Unicode character map.
    #[error("font has no Unicode cmap (format 4 / 3,1 or 0,x)")]
    NoUnicodeMap,
    /// A character cannot be encoded for this font.
    #[error("character {0:?} (U+{1:04X}) cannot be encoded in WinAnsi")]
    MissingGlyph(char, u32),
}

/// The parsed, embedding-ready representation of one font face.
pub struct LoadedFont {
    /// Original raw file bytes, embedded as `/FontFile2`.
    pub raw: Vec<u8>,
    /// Font units per em from `head`.
    pub units_per_em: u16,
    /// PostScript name (used as `/BaseFont`).
    pub postscript_name: String,
    /// Family name for the font registry.
    pub family: String,
    /// `OS/2` embedding permission, if the table exists.
    pub permissions: Option<ttf_parser::Permissions>,
    /// Glyph-space ascent from `hhea`.
    pub ascent: i16,
    /// Glyph-space descent from `hhea` (negative).
    pub descent: i16,
    /// Cap height from `OS/2` (fallback: 70% of ascent).
    pub cap_height: i16,
    /// Font bounding box in glyph units: `[xMin yMin xMax yMax]`.
    pub bbox: [f32; 4],
    /// Advance width (glyph units) for every WinAnsi byte 0..=255,
    /// or `None` when the byte has no mapping.
    pub win_ansi_widths: [Option<u16>; 256],
}

impl LoadedFont {
    /// Load and analyze a font file.
    pub fn load(path: impl AsRef<Path>) -> Result<Self, FontError> {
        let path = path.as_ref();
        let raw = std::fs::read(path).map_err(|source| FontError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_bytes(raw)
    }

    /// Load from in-memory bytes.
    pub fn from_bytes(raw: Vec<u8>) -> Result<Self, FontError> {
        // TTC collections: slice face 0 out into a standalone sfnt so the
        // embedded FontFile2 is a single TrueType program (PDF requires
        // this; veraPDF rejects collections).
        let raw = match crate::ttc::extract_face(&raw, 0) {
            Some(standalone) => standalone,
            None => raw,
        };
        let face = Face::parse(&raw, 0).map_err(|e| FontError::Parse(format!("{e:?}")))?;

        let units_per_em = face.units_per_em();
        let postscript_name = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::POST_SCRIPT_NAME && n.is_unicode())
            .and_then(|n| n.to_string())
            .unwrap_or_else(|| "Custom".to_string());
        let family = face
            .names()
            .into_iter()
            .find(|n| n.name_id == ttf_parser::name_id::FAMILY && n.is_unicode())
            .and_then(|n| n.to_string())
            .unwrap_or_else(|| postscript_name.clone());

        let permissions = face
            .tables()
            .os2
            .and_then(|o| o.permissions());
        let ascent = face.ascender();
        let descent = face.descender();
        let cap_height = face
            .tables()
            .os2
            .and_then(|o| o.capital_height())
            .filter(|v| *v > 0)
            .unwrap_or(((ascent as f32) * 0.7) as i16);
        let bbox = {
            let b = face.global_bounding_box();
            [b.x_min as f32, b.y_min as f32, b.x_max as f32, b.y_max as f32]
        };

        // WinAnsi byte -> Unicode -> glyph -> advance.
        let mut win_ansi_widths = [None::<u16>; 256];
        let mut any = false;
        for byte in 0..=255usize {
            let Some(unit) = winansi::unit_for_byte(byte as u8) else {
                continue;
            };
            let ch = char::from_u32(u32::from(unit)).expect("unit is a scalar");
            let Some(gid) = face.glyph_index(ch) else {
                continue;
            };
            if let Some(adv) = face.glyph_hor_advance(gid) {
                win_ansi_widths[byte] = Some(adv);
                any = true;
            }
        }
        if !any {
            return Err(FontError::NoUnicodeMap);
        }

        Ok(LoadedFont {
            raw,
            units_per_em,
            postscript_name,
            family,
            permissions,
            ascent,
            descent,
            cap_height,
            bbox,
            win_ansi_widths,
        })
    }

    /// Width of a WinAnsi-encoded byte in glyph units, if mapped.
    pub fn width_for_byte(&self, byte: u8) -> Option<u16> {
        self.win_ansi_widths[usize::from(byte)]
    }

    /// Width of a byte in points at the given font size.
    pub fn byte_width_pt(&self, byte: u8, size: f64) -> Option<f64> {
        self.width_for_byte(byte)
            .map(|w| f64::from(w) * size / f64::from(self.units_per_em))
    }

    /// Advance width of a whole string (WinAnsi encoding) in points.
    pub fn text_width_pt(&self, text: &str, size: f64) -> Result<f64, FontError> {
        // WinAnsi-encodable text measures via the byte table (fast path).
        // Anything else falls back to per-glyph metrics via the cmap.
        if let Ok(bytes) = winansi::encode(text) {
            let mut total = 0.0;
            for b in bytes {
                if let Some(w) = self.byte_width_pt(b, size) {
                    total += w;
                }
            }
            return Ok(total);
        }
        let face = ttf_parser::Face::parse(&self.raw, 0)
            .map_err(|e| FontError::Parse(format!("{e:?}")))?;
        let scale = f64::from(self.units_per_em);
        let mut total = 0.0;
        for ch in text.chars() {
            match face.glyph_index(ch) {
                Some(g) => {
                    if let Some(w) = face.glyph_hor_advance(g) {
                        total += f64::from(w) * size / scale;
                    }
                }
                None => return Err(FontError::MissingGlyph(ch, u32::from(ch))),
            }
        }
        Ok(total)
    }

    /// True when the font's license flags restrict embedding.
    pub fn embedding_restricted(&self) -> bool {
        matches!(
            self.permissions,
            Some(ttf_parser::Permissions::Restricted)
                | Some(ttf_parser::Permissions::PreviewAndPrint)
        )
    }

    /// Glyph ID for a Unicode character, for CID (Type0) encoding.
    pub fn glyph_index(&self, ch: char) -> Option<u16> {
        let face = ttf_parser::Face::parse(&self.raw, 0).ok()?;
        face.glyph_index(ch).map(|g| g.0)
    }

    /// Advance width of a glyph (by GID) in font units.
    pub fn glyph_width_units(&self, gid: u16) -> Option<i64> {
        let face = ttf_parser::Face::parse(&self.raw, 0).ok()?;
        let g = ttf_parser::GlyphId(gid);
        face.glyph_hor_advance(g).map(i64::from)
    }

    /// Width of a glyph (by GID) in points at the given size.
    pub fn glyph_width_pt(&self, gid: u16, size: f64) -> Option<f64> {
        let face = ttf_parser::Face::parse(&self.raw, 0).ok()?;
        let g = ttf_parser::GlyphId(gid);
        let w = face.glyph_hor_advance(g)?;
        Some(f64::from(w) * size / f64::from(self.units_per_em))
    }

    /// Flag bits for `/Flags` in the font descriptor.
    pub fn descriptor_flags(&self) -> i64 {
        // 32 = non-symbolic.
        32
    }
}

/// Decode a name-table string for the common platforms:
/// pid 0/3 (Unicode/Windows) = UTF-16BE; pid 1 (Macintosh) = MacRoman.
fn decode_name_record(data: &[u8], str_off: usize, rec_off: usize) -> Option<String> {
    let pid = u16::from_be_bytes([data[rec_off], data[rec_off + 1]]);
    let eid = u16::from_be_bytes([data[rec_off + 2], data[rec_off + 3]]);
    let len = u16::from_be_bytes([data[rec_off + 8], data[rec_off + 9]]) as usize;
    let off = u16::from_be_bytes([data[rec_off + 10], data[rec_off + 11]]) as usize;
    let raw = data.get(str_off + off..str_off + off + len)?;
    match (pid, eid) {
        (0, _) | (3, 1) | (3, 10) => Some(decode_utf16be(raw)),
        (1, 0) => Some(raw.iter().map(|&b| b as char).collect()),
        _ => None,
    }
}

/// Decode UTF-16BE bytes to a String. Local helper (do not name it
/// `from_utf16be` at the call site through String — newer rustc has a
/// stable `String::from_utf16be` whose signature differs).
fn decode_utf16be(b: &[u8]) -> String {
    (0..b.len() / 2)
        .map(|i| u16::from_be_bytes([b[2 * i], b[2 * i + 1]]))
        .collect::<Vec<u16>>()
        .iter()
        .map(|&u| char::from_u32(u32::from(u)).unwrap_or('\u{fffd}'))
        .collect()
}

/// Probe a font file for its family and style names without a full load.
/// Returns `(family, style)`; used by the registry to index system fonts.
/// Decodes Unicode (pid 0/3) and Macintosh (pid 1) records manually because
/// some system fonts (e.g. macOS Arial) only carry non-Windows-platform
/// names that ttf-parser refuses to decode.
pub fn probe(path: impl AsRef<Path>) -> Result<(String, String), FontError> {
    let path = path.as_ref();
    let data = std::fs::read(path).map_err(|source| FontError::Io {
        path: path.display().to_string(),
        source,
    })?;
    // Locate the name table.
    if data.len() < 12 {
        return Err(FontError::Parse("truncated font".into()));
    }
    let num_tables = u16::from_be_bytes([data[4], data[5]]) as usize;
    let mut name_off = None;
    for i in 0..num_tables {
        let rec = 12 + i * 16;
        if &data[rec..rec + 4] == b"name" {
            name_off = Some(u32::from_be_bytes([
                data[rec + 8],
                data[rec + 9],
                data[rec + 10],
                data[rec + 11],
            ]) as usize);
        }
    }
    let toff = name_off.ok_or_else(|| FontError::Parse("no name table".into()))?;
    if toff + 6 > data.len() {
        return Err(FontError::Parse("truncated name table".into()));
    }
    let count = u16::from_be_bytes([data[toff + 2], data[toff + 3]]) as usize;
    let str_off = toff + u16::from_be_bytes([data[toff + 4], data[toff + 5]]) as usize;
    let mut family = String::new();
    let mut style = String::new();
    for j in 0..count {
        let rec = toff + 6 + j * 12;
        if rec + 12 > data.len() {
            break;
        }
        let nid = u16::from_be_bytes([data[rec + 6], data[rec + 7]]);
        match nid {
            1 if family.is_empty() => {
                if let Some(s) = decode_name_record(&data, str_off, rec) {
                    family = s;
                }
            }
            2 if style.is_empty() => {
                if let Some(s) = decode_name_record(&data, str_off, rec) {
                    style = s;
                }
            }
            _ => {}
        }
    }
    if family.is_empty() {
        return Err(FontError::Parse("no family name".into()));
    }
    if style.is_empty() {
        style = "Regular".into();
    }
    Ok((family, style))
}
