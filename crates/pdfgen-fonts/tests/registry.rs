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
        Some("Liberation Serif")
    );
    assert_eq!(standard14_family("Courier"), Some("Liberation Mono"));
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
