//! `pdfgen` — command-line interface.
//!
//! `pdfgen validate <file.pdf>` — run the Matterhorn machine checks and
//! print findings with checkpoint IDs (exit 1 when any are found).
//!
//! `pdfgen render <file.pdf> [--dpi N] [--out DIR]` — rasterize pages to
//! `<stem>-page<N>.png` files.
//!
//! `pdfgen print <file.pdf> [--dpi N] [--printer NAME]` — spool the PDF
//! itself to the system printer (lpr/lp); only falls back to rasterizing
//! with our renderer when the spooler refuses the PDF.

use std::process::ExitCode;

const HELP: &str = "\
pdfgen — free, MIT-licensed PDF toolkit

USAGE:
    pdfgen <COMMAND> [OPTIONS]

COMMANDS:
    validate <file.pdf>              Run the Matterhorn machine checks
                                     (exit 1 on any finding)
    render <file.pdf>                Rasterize pages to PNG files
        --dpi N                      Resolution (default 96)
        --out DIR                    Output directory (default .)
    print <file.pdf>                 Send to the system printer
        --dpi N                      Raster fallback resolution
        --printer NAME               Target a named printer/queue
    help                             Print this message

PDF/UA conformance: pdfgen save() never blocks output. Files claim
conformance only when every machine check passes; otherwise the save
report lists the violations and the file omits the claim.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("help" | "--help" | "-h") => {
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("validate") => {
            let path = match parse_args(&args[1..], &[]) {
                Ok((p, _)) => p,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            match pdfgen_validate::validate(&path) {
                Ok(report) => {
                    if report.is_clean() {
                        println!("OK: no machine-check failures in {path}");
                        if !report.review.is_empty() {
                            println!("Human review:");
                            for r in &report.review {
                                println!("  - {r}");
                            }
                        }
                        ExitCode::SUCCESS
                    } else {
                        println!("{} finding(s) in {path}:", report.findings.len());
                        for f in &report.findings {
                            println!("  {}: {} ({})", f.id, f.message, f.context);
                        }
                        ExitCode::FAILURE
                    }
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Some("render") => {
            let (path, opts, out_dir) = match parse_render(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            match render_to_pngs(&path, opts, &out_dir) {
                Ok(files) => {
                    for f in &files {
                        println!("wrote {}", f.display());
                    }
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Some("print") => {
            let (path, dpi, printer) = match parse_print(&args[1..]) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(2);
                }
            };
            match print_file(&path, dpi, printer.as_deref()) {
                Ok(n) => {
                    println!("spooled {n} page(s) of {path} to the print queue");
                    ExitCode::SUCCESS
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(2)
                }
            }
        }
        Some(other) => {
            eprintln!("unknown command: {other}");
            eprint!("{HELP}");
            ExitCode::from(2)
        }
        None => {
            eprint!("{HELP}");
            ExitCode::from(2)
        }
    }
}

/// Walk `rest` as `<file> [--flag value]...`. Returns the positional
/// path and a map of the flags that were given. Unknown flags error.
fn parse_args(rest: &[String], flags: &[&str]) -> Result<(String, Vec<(String, String)>), String> {
    let mut path: Option<String> = None;
    let mut pairs = Vec::new();
    let mut i = 0;
    while i < rest.len() {
        let a = &rest[i];
        if let Some(flag) = a.strip_prefix("--") {
            let full = format!("--{flag}");
            if !flags.contains(&full.as_str()) {
                return Err(format!("unknown option {full} (see `pdfgen help`)"));
            }
            let v = rest
                .get(i + 1)
                .ok_or_else(|| format!("{full} needs a value"))?;
            if v.starts_with("--") {
                return Err(format!("{full} needs a value, got another flag"));
            }
            pairs.push((full, v.clone()));
            i += 2;
        } else {
            if path.is_some() {
                return Err("takes exactly one file".into());
            }
            path = Some(a.clone());
            i += 1;
        }
    }
    path.ok_or_else(|| "missing <file.pdf> (see `pdfgen help`)".to_string())
        .map(|p| (p, pairs))
}

fn parse_render(
    rest: &[String],
) -> Result<(String, pdfgen_render::RenderOptions, std::path::PathBuf), String> {
    let (path, pairs) = parse_args(rest, &["--dpi", "--out"])?;
    let mut opts = pdfgen_render::RenderOptions::default();
    let mut out_dir = std::path::PathBuf::from(".");
    for (k, v) in &pairs {
        match k.as_str() {
            "--dpi" => {
                opts.dpi = v
                    .parse::<f64>()
                    .map_err(|_| format!("--dpi expects a number, got {v}"))?;
            }
            "--out" => out_dir = std::path::PathBuf::from(v),
            _ => unreachable!("filtered by parse_args"),
        }
    }
    Ok((path, opts, out_dir))
}

fn parse_print(rest: &[String]) -> Result<(String, f64, Option<String>), String> {
    let (path, pairs) = parse_args(rest, &["--dpi", "--printer"])?;
    let mut dpi = 150.0;
    let mut printer = None;
    for (k, v) in &pairs {
        match k.as_str() {
            "--dpi" => {
                dpi = v
                    .parse::<f64>()
                    .map_err(|_| format!("--dpi expects a number, got {v}"))?;
            }
            "--printer" => printer = Some(v.clone()),
            _ => unreachable!("filtered by parse_args"),
        }
    }
    Ok((path, dpi, printer))
}

/// Rasterize every page of `path` into `<out_dir>/<stem>-page<N>.png`.
fn render_to_pngs(
    path: &str,
    opts: pdfgen_render::RenderOptions,
    out_dir: &std::path::Path,
) -> Result<Vec<std::path::PathBuf>, String> {
    let pages = pdfgen_render::render_all(path, opts)?;
    if pages.is_empty() {
        return Err("no pages rendered".into());
    }
    std::fs::create_dir_all(out_dir).map_err(|e| e.to_string())?;
    let stem = std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("page")
        .to_string();
    let mut files = Vec::with_capacity(pages.len());
    for (i, bmp) in pages.iter().enumerate() {
        let file = out_dir.join(format!("{stem}-page{}.png", i + 1));
        let png = bmp.to_png()?;
        std::fs::write(&file, png).map_err(|e| e.to_string())?;
        files.push(file);
    }
    Ok(files)
}

/// Send the file to the system printer. Tries spooling the PDF itself
/// first (`lpr file.pdf`, then `lp file.pdf`) — modern spoolers accept
/// PDFs natively — and falls back to rasterizing with our renderer only
/// when that fails. The raster temp dir is always cleaned up.
fn print_file(path: &str, dpi: f64, printer: Option<&str>) -> Result<usize, String> {
    // 1. Spool the PDF directly.
    for spooler in ["lpr", "lp"] {
        let mut cmd = std::process::Command::new(spooler);
        if let Some(p) = printer {
            match spooler {
                "lpr" => {
                    cmd.arg("-P").arg(p);
                }
                _ => {
                    cmd.arg("-d").arg(p);
                }
            }
        }
        if let Ok(s) = cmd.arg(path).status() {
            if s.success() {
                return Ok(1);
            }
        }
    }

    // 2. Rasterize and spool per-page PNGs.
    let opts = pdfgen_render::RenderOptions {
        dpi,
        white_background: true,
    };
    let tmp = std::env::temp_dir().join(format!(
        "pdfgen-print-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_millis())
    ));
    let files = render_to_pngs(path, opts, &tmp)?;
    let result = (|| {
        let mut ok = 0;
        for f in &files {
            let mut cmd = std::process::Command::new("lpr");
            if let Some(p) = printer {
                cmd.arg("-P").arg(p);
            }
            cmd.arg(f);
            if let Ok(s) = cmd.status() {
                if s.success() {
                    ok += 1;
                }
            }
        }
        if ok == 0 {
            return Err("no print spooler available (tried lpr and lp)".into());
        }
        Ok(ok)
    })();
    // Always remove the raster temp dir, success or not.
    let _ = std::fs::remove_dir_all(&tmp);
    result
}
