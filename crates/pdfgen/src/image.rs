//! Image loading: PNG (decoded to RGB) and JPEG (embedded as DCTDecode
//! pass-through, dimensions parsed from the SOF marker).

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
    /// Raw RGB8 bytes, row-major, no padding.
    Rgb(Vec<u8>),
    /// Original JPEG bytes; embedded via DCTDecode without re-encoding.
    Jpeg(Vec<u8>),
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
        let rgb = match info.color_type {
            png::ColorType::Rgb => buf,
            png::ColorType::Grayscale => {
                let mut out = Vec::with_capacity(buf.len() * 3);
                for &g in &buf {
                    out.extend_from_slice(&[g, g, g]);
                }
                out
            }
            png::ColorType::Rgba => {
                // Composite over white; PDF images have no alpha here.
                let mut out = Vec::with_capacity(buf.len() / 4 * 3);
                for px in buf.chunks_exact(4) {
                    let a = f64::from(px[3]) / 255.0;
                    for c in &px[..3] {
                        out.push((f64::from(*c) * a + 255.0 * (1.0 - a)) as u8);
                    }
                }
                out
            }
            png::ColorType::GrayscaleAlpha => {
                let mut out = Vec::with_capacity(buf.len() / 2 * 3);
                for px in buf.chunks_exact(2) {
                    let a = f64::from(px[1]) / 255.0;
                    let g = (f64::from(px[0]) * a + 255.0 * (1.0 - a)) as u8;
                    out.extend_from_slice(&[g, g, g]);
                }
                out
            }
            other => return Err(ImageError::Unsupported(format!("png color type {other:?}"))),
        };
        Ok(Image {
            w,
            h,
            kind: ImageKind::Rgb(rgb),
            alt: String::new(),
        })
    }

    /// JPEG: dimensions come from the SOF marker; bytes pass through.
    fn parse_jpeg(bytes: &[u8]) -> Result<Self, ImageError> {
        let mut i = 2usize;
        while i + 9 < bytes.len() {
            if bytes[i] != 0xff {
                i += 1;
                continue;
            }
            let marker = bytes[i + 1];
            // SOF0-15 except DHT (C4), DAC (CC), JPG (C8), RSTn.
            let is_sof = matches!(marker,
                0xc0 | 0xc1 | 0xc2 | 0xc3 | 0xc5 | 0xc6 | 0xc7
                | 0xc9 | 0xca | 0xcb | 0xcd | 0xce | 0xcf);
            if is_sof {
                let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
                let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
                return Ok(Image {
                    w: u32::from(w),
                    h: u32::from(h),
                    kind: ImageKind::Jpeg(bytes.to_vec()),
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
