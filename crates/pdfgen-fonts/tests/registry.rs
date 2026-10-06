//! Font registry tests: by-name resolution, standard-14 aliases, system
//! fonts, and substitution fallback.

use pdfgen_fonts::{standard14_family, FontRegistry};

const VENDOR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fonts/vendor/liberation/LiberationSans-Regular.ttf"
);

#[test]
fn registry_resolves_catalog_and_aliases() {
    let mut reg = FontRegistry::new();

    // Built-in catalog by family name.
    let r = reg.resolve("Liberation Serif", "Bold").unwrap();
    assert!(r.path.exists(), "catalog file exists: {:?}", r.path);
    assert!(!r.substituted);

    // Standard-14 alias resolves to the look-alike family.
    let r = reg.resolve("Helvetica", "Regular").unwrap();
    assert!(r.path.to_string_lossy().contains("LiberationSans"));
    assert!(r.substituted, "alias counts as substitution");

    // The alias table itself.
    assert_eq!(
        standard14_family("Times New Roman"),
        Some(("Liberation Serif", "regular"))
    );
    assert_eq!(
        standard14_family("Courier"),
        Some(("Liberation Mono", "regular"))
    );
    assert_eq!(standard14_family("Bogus Font"), None);
}

#[test]
#[cfg(target_os = "macos")]
fn registry_resolves_system_fonts() {
    let mut reg = FontRegistry::new();
    // Arial is installed on every macOS.
    let r = reg.resolve("Arial", "Regular").unwrap();
    assert!(r.path.exists(), "system Arial: {:?}", r.path);
    assert!(!r.substituted, "real system font is not a substitution");
}

#[test]
fn registry_falls_back_with_substitution() {
    let mut reg = FontRegistry::new();
    let r = reg.resolve("Nonexistent Garamond", "Regular").unwrap();
    assert!(r.substituted, "fallback must be flagged");
    assert!(r.path.exists());
}

#[test]
fn user_registered_font_wins() {
    let mut reg = FontRegistry::new();
    reg.register("My Brand Font", "Regular", VENDOR);
    let r = reg.resolve("My Brand Font", "Regular").unwrap();
    assert!(!r.substituted);
    assert_eq!(r.family, "My Brand Font");
}

/// P0-11: standard-14 aliases must honor the style encoded in the name.
#[test]
fn standard14_alias_style_is_honored() {
    let mut r = FontRegistry::new();
    // Helvetica-Bold must resolve to the BOLD look-alike, not Regular.
    let bold = r.resolve("Helvetica-Bold", "").expect("helvetica bold");
    let path = bold.path.to_string_lossy().into_owned();
    assert!(
        path.contains("LiberationSans-Bold.ttf"),
        "Helvetica-Bold must resolve to the bold face, got {path}"
    );
    assert!(bold.substituted, "look-alike is a substitution");

    // Times-Italic -> italic face.
    let it = r.resolve("Times-Italic", "").expect("times italic");
    let it_path = it.path.to_string_lossy().into_owned();
    assert!(
        it_path.contains("LiberationSerif-Italic.ttf"),
        "Times-Italic must resolve to the italic face, got {it_path}"
    );

    // Helvetica-BoldItalic -> bold italic face.
    let bi = r
        .resolve("Helvetica-BoldOblique", "")
        .expect("helvetica bold oblique");
    let bi_path = bi.path.to_string_lossy().into_owned();
    assert!(
        bi_path.contains("LiberationSans-BoldItalic.ttf"),
        "Helvetica-BoldOblique must resolve to the bold-italic face, got {bi_path}"
    );
}
