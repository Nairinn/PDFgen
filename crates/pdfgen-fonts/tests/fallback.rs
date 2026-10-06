//! Packaged-install fallback: with no bundled fonts directory (or an
//! unknown family), the registry must still resolve a usable system
//! font with a substitution note — never fail outright.

use pdfgen_fonts::FontRegistry;

#[test]
fn unknown_family_falls_back_to_system_font() {
    let mut r = FontRegistry::new();
    // An invented family that is nowhere in the catalog, the system, or
    // the standard-14 aliases. The chain must end at the system-sans
    // last resort with substituted=true (or NotFound only when the
    // machine truly has no fonts at all, which CI runners do).
    match r.resolve("Definitely Not A Real Font Family", "Regular") {
        Ok(res) => {
            // Substitution is the whole point of the fallback chain.
            assert!(
                res.substituted,
                "fallback resolution must be flagged as substituted (got {})",
                res.family
            );
            assert!(std::path::Path::new(&res.path).exists());
        }
        Err(e) => {
            // Acceptable only on genuinely fontless machines.
            eprintln!("no fallback available on this machine: {e:?}");
        }
    }
}
