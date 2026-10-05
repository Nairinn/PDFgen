//! Font loader needs to expose the WinAnsi helpers used by the facade.
pub use winansi::encode as winansi_encode;
pub use winansi::encode_char as winansi_encode_char;
pub use winansi::unit_for_byte as winansi_unit_for_byte;

pub mod winansi;

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
        let bytes = winansi::encode(text)?;
        let mut total = 0.0;
        for b in bytes {
            if let Some(w) = self.byte_width_pt(b, size) {
                total += w;
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

    /// Flag bits for `/Flags` in the font descriptor.
    pub fn descriptor_flags(&self) -> i64 {
        // 32 = non-symbolic.
        32
    }
}
