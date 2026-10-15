//! Image loading: PNG (decoded to RGB + optional alpha /SMask) and JPEG
//! (embedded as DCTDecode pass-through, dimensions and component count
//! parsed from the SOF marker).

use thiserror::Error;

/// Errors while loading an image.
#[derive(Debug, Error)]
pub enum ImageError {
    /// IO failure.
    #[error("cannot read image {path}: {source}")]
    Io {
        /// Path attempted.
        path: String,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// Unrecognized or unsupported format.
    #[error("unsupported image format: {0}")]
    Unsupported(String),
    /// The decoder failed.
    #[error("decode failed: {0}")]
    Decode(String),
}

/// The decoded pixel payload ready for embedding.
#[derive(Debug, Clone)]
pub enum ImageKind {
    /// Raw RGB8 bytes (row-major, no padding) plus the alpha channel as
    /// a separate grayscale /SMask payload, when the source had alpha.
    Rgb {
        /// RGB bytes, w*h*3.
        rgb: Vec<u8>,
        /// Alpha bytes, w*h, when the source carried transparency.
        smask: Option<Vec<u8>>,
    },
    /// Original JPEG bytes; embedded via DCTDecode without re-encoding.
    Jpeg {
        /// Component count from the SOF marker: 1 = gray, 3 = RGB,
        /// 4 = CMYK (usually Adobe-inverted).
        components: u8,
        /// Original file bytes.
        bytes: Vec<u8>,
    },
}

/// A loaded, embedding-ready image.
#[derive(Debug, Clone)]
pub struct Image {
    /// Pixel width.
    pub w: u32,
    /// Pixel height.
    pub h: u32,
    /// Payload.
    pub kind: ImageKind,
    /// Alt text (PDF/UA 13-004); set at registration time.
    #[allow(dead_code)]
    pub(crate) alt: String,
}

impl Image {
    /// Load a PNG or JPEG file from disk.
    pub fn load(path: &str) -> Result<Self, ImageError> {
        let bytes = std::fs::read(path).map_err(|source| ImageError::Io {
            path: path.into(),
            source,
        })?;
        if bytes.len() > 8 && &bytes[..8] == b"\x89PNG\r\n\x1a\n" {
            Self::decode_png(&bytes)
        } else if bytes.len() > 3 && bytes[0] == 0xff && bytes[1] == 0xd8 {
            Self::parse_jpeg(&bytes)
        } else {
            Err(ImageError::Unsupported(path.into()))
        }
    }

    fn decode_png(bytes: &[u8]) -> Result<Self, ImageError> {
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder
            .read_info()
            .map_err(|e| ImageError::Decode(format!("{e}")))?;
        let mut buf = vec![0u8; reader.output_buffer_size()];
        let info = reader
            .next_frame(&mut buf)
            .map_err(|e| ImageError::Decode(format!("{e}")))?;
        buf.truncate(info.buffer_size());
        let (w, h) = (info.width, info.height);

        // Split color and alpha; alpha becomes a PDF /SMask (a real
        // soft mask) instead of being composited away over white.
        match info.color_type {
            png::ColorType::Rgb => Ok(Image {
                w,
                h,
                kind: ImageKind::Rgb {
                    rgb: buf,
                    smask: None,
                },
                alt: String::new(),
            }),
            png::ColorType::Rgba => {
                let n = (w as usize) * (h as usize);
                let mut rgb = Vec::with_capacity(n * 3);
                let mut alpha = Vec::with_capacity(n);
                for px in buf.chunks_exact(4) {
                    rgb.extend_from_slice(&px[..3]);
                    alpha.push(px[3]);
                }
                Ok(Image {
                    w,
                    h,
                    kind: ImageKind::Rgb {
                        rgb,
                        smask: Some(alpha),
                    },
                    alt: String::new(),
                })
            }
            png::ColorType::Grayscale => {
                let mut rgb = Vec::with_capacity(buf.len() * 3);
                for &g in &buf {
                    rgb.extend_from_slice(&[g, g, g]);
                }
                Ok(Image {
                    w,
                    h,
                    kind: ImageKind::Rgb { rgb, smask: None },
                    alt: String::new(),
                })
            }
            png::ColorType::GrayscaleAlpha => {
                let n = (w as usize) * (h as usize);
                let mut rgb = Vec::with_capacity(n * 3);
                let mut alpha = Vec::with_capacity(n);
                for px in buf.chunks_exact(2) {
                    rgb.extend_from_slice(&[px[0], px[0], px[0]]);
                    alpha.push(px[1]);
                }
                Ok(Image {
                    w,
                    h,
                    kind: ImageKind::Rgb {
                        rgb,
                        smask: Some(alpha),
                    },
                    alt: String::new(),
                })
            }
            png::ColorType::Indexed => {
                Err(ImageError::Unsupported("indexed png not supported".into()))
            }
        }
    }

    /// JPEG: dimensions and component count come from the SOF marker;
    /// bytes pass through. 4-component files usually carry Adobe CMYK
    /// (inverted); the embedder sets /Decode [1 0 1 0 1 0 1 0] for them
    /// when the APP14 Adobe marker is present.
    fn parse_jpeg(bytes: &[u8]) -> Result<Self, ImageError> {
        let mut i = 2usize;
        let mut adobe = false;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            // SOF0-15 except DHT (C4), DAC (CC), JPG (C8), RSTn.
            let is_sof = matches!(
                marker,
                0xc0 | 0xc1
                    | 0xc2
                    | 0xc3
                    | 0xc5
                    | 0xc6
                    | 0xc7
                    | 0xc9
                    | 0xca
                    | 0xcb
                    | 0xcd
                    | 0xce
                    | 0xcf
            );
            if is_sof {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
                let components = bytes[i + 9];
                // Keep scanning briefly for the APP14 Adobe marker only
                // when it matters (4-component CMYK).
                if components == 4 {
                    adobe = has_adobe_marker(bytes);
                }
                let _ = adobe;
                return Ok(Image {
                    w: u32::from(w),
                    h: u32::from(h),
                    kind: ImageKind::Jpeg {
                        components,
                        bytes: bytes.to_vec(),
                    },
                    alt: String::new(),
                });
            }
            // Skip this marker segment.
            let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            i += 2 + seg_len;
        }
        Err(ImageError::Decode("no SOF marker found".into()))
    }
}

/// True when the file carries the APP14 "Adobe" marker (inverted CMYK).
fn has_adobe_marker(bytes: &[u8]) -> bool {
    // APP14 = 0xFF 0xEE, and the 12-byte payload starts with "Adobe".
    let mut i = 2usize;
    while i + 14 < bytes.len() {
        if bytes[i] == 0xff && bytes[i + 1] == 0xee {
            return &bytes[i + 4..i + 9] == b"Adobe";
        }
        if bytes[i] == 0xff {
            let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
            i += 2 + seg_len;
        } else {
            i += 1;
        }
    }
    false
}
