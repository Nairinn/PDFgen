//! # pdfgen-fonts
//!
//! The font registry: resolve font families by name, with three sources:
//!
//! 1. **Built-in catalog** — free fonts shipped in the crate (Liberation
//!    family now; more groups get their own `pdfgen-fonts-*` crates).
//! 2. **System fonts** — fonts installed on the machine (macOS, Windows,
//!    Linux fontconfig paths), so licensed fonts the user owns work by name.
//! 3. **User-registered fonts** — explicit paths added by the caller.
//!
//! The standard-14 names (`Helvetica`, `Times-Roman`, `Courier`) resolve
//! to Liberation look-alikes, since PDF/UA requires embedded fonts and the
//! standard 14 are never embedded.

use pdfgen_font::LoadedFont;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// One catalog entry: where to find the file and how to describe it.
#[derive(Debug, Clone)]
pub struct CatalogEntry {
    /// Family name callers use, e.g. `Liberation Sans`.
    pub family: &'static str,
    /// Style: `Regular`, `Bold`, `Italic`, `Bold Italic`.
    pub style: &'static str,
    /// Path relative to this crate's `fonts/` directory.
    pub file: &'static str,
}

/// The built-in catalog (Liberation + Noto families, all OFL-1.1).
pub const CATALOG: &[CatalogEntry] = &[
    CatalogEntry { family: "Liberation Sans", style: "Regular", file: "vendor/liberation/LiberationSans-Regular.ttf" },
    CatalogEntry { family: "Liberation Sans", style: "Bold", file: "vendor/liberation/LiberationSans-Bold.ttf" },
    CatalogEntry { family: "Liberation Sans", style: "Italic", file: "vendor/liberation/LiberationSans-Italic.ttf" },
    CatalogEntry { family: "Liberation Sans", style: "Bold Italic", file: "vendor/liberation/LiberationSans-BoldItalic.ttf" },
    CatalogEntry { family: "Liberation Serif", style: "Regular", file: "vendor/liberation/LiberationSerif-Regular.ttf" },
    CatalogEntry { family: "Liberation Serif", style: "Bold", file: "vendor/liberation/LiberationSerif-Bold.ttf" },
    CatalogEntry { family: "Liberation Serif", style: "Italic", file: "vendor/liberation/LiberationSerif-Italic.ttf" },
    CatalogEntry { family: "Liberation Serif", style: "Bold Italic", file: "vendor/liberation/LiberationSerif-BoldItalic.ttf" },
    CatalogEntry { family: "Liberation Mono", style: "Regular", file: "vendor/liberation/LiberationMono-Regular.ttf" },
    CatalogEntry { family: "Liberation Mono", style: "Bold", file: "vendor/liberation/LiberationMono-Bold.ttf" },
    CatalogEntry { family: "Liberation Mono", style: "Italic", file: "vendor/liberation/LiberationMono-Italic.ttf" },
    CatalogEntry { family: "Liberation Mono", style: "Bold Italic", file: "vendor/liberation/LiberationMono-BoldItalic.ttf" },
    CatalogEntry { family: "Noto Sans", style: "Regular", file: "vendor/noto/NotoSans-Regular.ttf" },
    CatalogEntry { family: "Noto Sans", style: "Bold", file: "vendor/noto/NotoSans-Bold.ttf" },
    CatalogEntry { family: "Noto Sans", style: "Italic", file: "vendor/noto/NotoSans-Italic.ttf" },
    CatalogEntry { family: "Noto Sans", style: "Bold Italic", file: "vendor/noto/NotoSans-BoldItalic.ttf" },
    CatalogEntry { family: "Noto Serif", style: "Regular", file: "vendor/noto/NotoSerif-Regular.ttf" },
    CatalogEntry { family: "Noto Serif", style: "Bold", file: "vendor/noto/NotoSerif-Bold.ttf" },
    CatalogEntry { family: "Noto Sans Mono", style: "Regular", file: "vendor/noto/NotoSansMono-Regular.ttf" },
    CatalogEntry { family: "Noto Sans Mono", style: "Bold", file: "vendor/noto/NotoSansMono-Bold.ttf" },
    CatalogEntry { family: "Noto Sans Myanmar", style: "Regular", file: "vendor/noto/NotoSansMyanmar-Regular.ttf" },
    CatalogEntry { family: "Noto Sans Myanmar", style: "Bold", file: "vendor/noto/NotoSansMyanmar-Bold.ttf" },
    CatalogEntry { family: "Noto Sans Thai", style: "Regular", file: "vendor/noto/NotoSansThai-Regular.ttf" },
    CatalogEntry { family: "Noto Sans Thai", style: "Bold", file: "vendor/noto/NotoSansThai-Bold.ttf" },
    CatalogEntry { family: "Noto Sans Arabic", style: "Regular", file: "vendor/noto/NotoSansArabic-Regular.ttf" },
    CatalogEntry { family: "Noto Sans Arabic", style: "Bold", file: "vendor/noto/NotoSansArabic-Bold.ttf" },
];

/// Standard-14 name → look-alike family (PDF/UA needs embedded fonts).
const STANDARD_14: &[(&str, &str)] = &[
    ("Helvetica", "Liberation Sans"),
    ("Helvetica-Bold", "Liberation Sans"),
    ("Helvetica-Oblique", "Liberation Sans"),
    ("Helvetica-BoldOblique", "Liberation Sans"),
    ("Arial", "Liberation Sans"),
    ("Times-Roman", "Liberation Serif"),
    ("Times-Bold", "Liberation Serif"),
    ("Times-Italic", "Liberation Serif"),
    ("Times-BoldItalic", "Liberation Serif"),
    ("Times New Roman", "Liberation Serif"),
    ("Courier", "Liberation Mono"),
    ("Courier-Bold", "Liberation Mono"),
    ("Courier-Oblique", "Liberation Mono"),
    ("Courier-BoldOblique", "Liberation Mono"),
    ("Courier New", "Liberation Mono"),
];

/// Resolve a standard-14 / common name to its look-alike family.
pub fn standard14_family(name: &str) -> Option<&'static str> {
    STANDARD_14
        .iter()
        .find(|(alias, _)| eq_ignore_case(*alias, name))
        .map(|(_, family)| *family)
}

fn eq_ignore_case(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// The resolved location of a font file.
#[derive(Debug, Clone)]
pub struct Resolved {
    /// Absolute path of the font file.
    pub path: PathBuf,
    /// Family that was matched.
    pub family: String,
    /// True when this is a look-alike substitution (name not found).
    pub substituted: bool,
}

/// Errors from the registry.
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// No font matched and no fallback could be loaded.
    #[error("no font found for {0:?}")]
    NotFound(String),
}

/// The registry: name → file resolution over catalog + system + user fonts.
#[derive(Debug, Default)]
pub struct FontRegistry {
    /// User-registered: family name (lowercased) → paths by style.
    user: HashMap<String, HashMap<String, PathBuf>>,
    /// System scan cache: family → paths by style.
    system: Option<HashMap<String, HashMap<String, PathBuf>>>,
}

impl FontRegistry {
    /// Debug/testing accessor: the scanned system font map, if any.
    pub fn system_scan(&self) -> &HashMap<String, HashMap<String, PathBuf>> {
        static EMPTY: std::sync::OnceLock<HashMap<String, HashMap<String, PathBuf>>> =
            std::sync::OnceLock::new();
        self.system
            .as_ref()
            .unwrap_or_else(|| EMPTY.get_or_init(HashMap::new))
    }

    /// New empty registry (no system scan yet).
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a font file under a family name.
    pub fn register(&mut self, family: &str, style: &str, path: impl Into<PathBuf>) {
        self.user
            .entry(family.to_ascii_lowercase())
            .or_default()
            .insert(style.to_ascii_lowercase(), path.into());
    }

    fn fonts_dir() -> Option<PathBuf> {
        // The fonts/ directory lives at the workspace root. CARGO_MANIFEST_DIR
        // for this crate is <root>/crates/pdfgen-fonts, so walk up twice.
        // Also honor PDFGEN_FONTS_DIR for relocated installs.
        let candidates: Vec<PathBuf> = option_env!("PDFGEN_FONTS_DIR")
            .map(PathBuf::from)
            .into_iter()
            .chain([
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fonts"),
                Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fonts"),
                Path::new(env!("CARGO_MANIFEST_DIR")).join("fonts"),
            ])
            .collect();
        for p in candidates {
            if let Ok(c) = p.canonicalize() {
                if c.is_dir() {
                    return Some(c);
                }
            }
        }
        None
    }

    /// Resolve a family+style to a file, checking user fonts, then the
    /// built-in catalog, then system fonts.
    pub fn resolve(&mut self, family: &str, style: &str) -> Result<Resolved, RegistryError> {
        let key = family.to_ascii_lowercase();
        let style_key = style.to_ascii_lowercase();

        // 1. User-registered.
        if let Some(styles) = self.user.get(&key) {
            if let Some(p) = styles.get(&style_key).or_else(|| styles.get("regular")) {
                return Ok(Resolved {
                    path: p.clone(),
                    family: family.to_string(),
                    substituted: false,
                });
            }
        }

        // 2. Standard-14 alias table (used only if 3 and 4 both miss, so a
        // real system font is preferred over a look-alike).
        let cat_family = standard14_family(family).map(str::to_ascii_lowercase);

        // 3. Built-in catalog (exact family names like "Liberation Sans").
        if let Some(dir) = Self::fonts_dir() {
            for e in CATALOG {
                if e.family.to_ascii_lowercase() == key {
                    let want = style_key.is_empty() || e.style.to_ascii_lowercase().contains(&style_key)
                        || (style_key == "regular" && e.style == "Regular")
                        || (style_key == "bold" && e.style == "Bold")
                        || (style_key == "italic" && e.style == "Italic")
                        || (style_key == "bold italic" && e.style == "Bold Italic");
                    if want {
                        let p = dir.join(e.file);
                        if p.exists() {
                            return Ok(Resolved {
                                path: p,
                                family: e.family.to_string(),
                                substituted: false,
                            });
                        }
                    }
                }
            }
        }

        // 4. System fonts (scan once, cache). A licensed font the user owns
        //    is NOT a substitution.
        if self.system.is_none() {
            self.system = Some(scan_system_fonts());
        }
        if let Some(sys) = self.system.as_ref() {
            if let Some(styles) = sys.get(&key) {
                if let Some(p) = styles
                    .get(&style_key)
                    .or_else(|| styles.get("regular"))
                    .or_else(|| styles.values().next())
                {
                    return Ok(Resolved {
                        path: p.clone(),
                        family: family.to_string(),
                        substituted: false,
                    });
                }
            }
        }

        // 5. Standard-14 alias → catalog look-alike (substituted).
        if let Some(aliased) = cat_family.as_deref() {
            if let Some(dir) = Self::fonts_dir() {
                for e in CATALOG {
                    if e.family.to_ascii_lowercase() == aliased {
                        let want = style_key.is_empty()
                            || (style_key == "regular" && e.style == "Regular")
                            || (style_key == "bold" && (e.style == "Bold" || e.style == "Bold Italic"))
                            || (style_key == "italic" && (e.style == "Italic" || e.style == "Bold Italic"))
                            || (style_key == "bold italic" && e.style == "Bold Italic")
                            || e.style == "Regular";
                        if want {
                            let p = dir.join(e.file);
                            if p.exists() {
                                return Ok(Resolved {
                                    path: p,
                                    family: e.family.to_string(),
                                    substituted: true,
                                });
                            }
                        }
                    }
                }
            }
        }

        // 5. Fallback: Liberation Sans Regular (substituted).
        if let Some(dir) = Self::fonts_dir() {
            for e in CATALOG {
                if e.family == "Liberation Sans" && e.style == "Regular" {
                    let p = dir.join(e.file);
                    if p.exists() {
                        return Ok(Resolved {
                            path: p,
                            family: e.family.to_string(),
                            substituted: true,
                        });
                    }
                }
            }
        }
        Err(RegistryError::NotFound(family.to_string()))
    }

    /// Resolve and load in one step.
    pub fn load(&mut self, family: &str, style: &str) -> Result<(LoadedFont, Resolved), Box<dyn std::error::Error>> {
        let r = self.resolve(family, style)?;
        let f = LoadedFont::load(&r.path)?;
        Ok((f, r))
    }
}

/// Scan the platform's font directories for TTF/OTF files, keyed by
/// family name (from the font's own name table) and style. Recurses into
/// subdirectories (macOS keeps fonts in `Supplemental/`, X11 in
/// `truetype/`).
fn scan_system_fonts() -> HashMap<String, HashMap<String, PathBuf>> {
    let mut out: HashMap<String, HashMap<String, PathBuf>> = HashMap::new();
    let roots: Vec<PathBuf> = if cfg!(target_os = "macos") {
        vec![
            PathBuf::from("/System/Library/Fonts"),
            PathBuf::from("/Library/Fonts"),
            PathBuf::from("~/Library/Fonts").expand_home(),
        ]
    } else if cfg!(target_os = "windows") {
        vec![PathBuf::from("C:/Windows/Fonts")]
    } else {
        vec![
            PathBuf::from("/usr/share/fonts"),
            PathBuf::from("/usr/local/share/fonts"),
            PathBuf::from("~/.fonts").expand_home(),
            PathBuf::from("~/.local/share/fonts").expand_home(),
        ]
    };
    for root in roots {
        scan_dir(&root, &mut out);
    }
    out
}

fn scan_dir(dir: &Path, out: &mut HashMap<String, HashMap<String, PathBuf>>) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_dir(&path, out);
            continue;
        }
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "ttf" | "otf") {
            continue;
        }
        if let Ok((family, style)) = pdfgen_font::probe(&path) {
            if family.is_empty() {
                continue;
            }
            out.entry(family.to_ascii_lowercase())
                .or_default()
                .entry(style.to_ascii_lowercase())
                .or_insert(path);
        }
    }
}

trait ExpandHome {
    fn expand_home(self) -> PathBuf;
}

impl ExpandHome for PathBuf {
    fn expand_home(self) -> PathBuf {
        if let Some(rest) = self.to_str().and_then(|s| s.strip_prefix('~')) {
            if let Some(home) = std::env::var_os("HOME") {
                return Path::new(&home).join(rest.trim_start_matches('/'));
            }
        }
        self
    }
}
