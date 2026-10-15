//! The byte-identical claim, proven end to end: the same pdfloom-doc
//! config rebuilds to the exact same bytes through every path - the
//! `pdfloom build` CLI (twice) and the library builder. The output is
//! written into the shared tests/output/ so the veraPDF gate covers
//! it like every other fixture.

use std::process::Command;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../pdfloom-def/tests/fixtures/golden_doc.json"
);
const DEF_CRATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../pdfloom-def");
const OUT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../tests/output/cli_golden.pdf"
);

fn build_via_cli(out: &str) -> Vec<u8> {
    let status = Command::new(env!("CARGO_BIN_EXE_pdfloom"))
        .args(["build", FIXTURE, "--out", out])
        .current_dir(DEF_CRATE)
        .status()
        .expect("spawn the pdfloom binary");
    assert!(
        status.success(),
        "`pdfloom build` must succeed on the golden config"
    );
    std::fs::read(out).expect("read the CLI output")
}

#[test]
fn same_config_rebuilds_byte_identical_cli_and_library() {
    // Two CLI runs of the same config must be byte-identical.
    let first = build_via_cli(OUT);
    let tmp = std::env::temp_dir().join("pdfloom-cli-bi-second.pdf");
    let second = build_via_cli(tmp.to_str().unwrap());
    assert_eq!(
        first, second,
        "two `pdfloom build` runs of the same config differ (nondeterminism?)"
    );

    // The library path must agree with the CLI bytes: same config, same
    // writer, no timestamps or object-id ordering leaks.
    std::env::set_current_dir(DEF_CRATE).expect("cd to the def crate (fixture paths)");
    let json = std::fs::read(FIXTURE).expect("golden fixture");
    let def = pdfloom_def::parse_def(&json).expect("valid pdfloom-doc");
    let lib_out = std::env::temp_dir().join("pdfloom-cli-bi-lib.pdf");
    let _report =
        pdfloom_def::build_to_file(&def, lib_out.to_str().unwrap()).expect("library build");
    let lib = std::fs::read(lib_out).expect("read the library output");
    assert_eq!(
        first, lib,
        "the CLI and the library produce different bytes for the same config"
    );
}
