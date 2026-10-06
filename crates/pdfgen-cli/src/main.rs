//! `pdfgen` — command-line interface.
//!
//! `pdfgen validate <file.pdf>` — run the Matterhorn machine checks and
//! print findings with checkpoint IDs (exit 1 when any are found).
//!
//! `pdfgen render <file.pdf> [--dpi N] [--out DIR]` — rasterize pages to
//! `<stem>-page<N>.png` files.
//!
//! `pdfgen print <file.pdf>` — rasterize and spool every page to the
//! system printer via `lpr` (macOS/Linux) or `lp` (fallback).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("validate") => {
            let Some(path) = args.get(2) else {
                eprintln!("usage: pdfgen validate <file.pdf>");
                return ExitCode::from(2);
            };
            match pdfgen_validate::validate(path) {
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
            let Some(path) = args.get(2) else {
                eprintln!("usage: pdfgen render <file.pdf> [--dpi N] [--out DIR]");
                return ExitCode::from(2);
            };
            let mut opts = pdfgen_render::RenderOptions::default();
            let mut out_dir = std::path::PathBuf::from(".");
            let mut i = 3;
            while i + 1 < args.len() + 1 {
                match args.get(i).map(String::as_str) {
                    Some("--dpi") => {
                        if let Some(v) = args.get(i + 1).and_then(|s| s.parse::<f64>().ok()) {
                            opts.dpi = v;
                        }
                        i += 2;
                    }
                    Some("--out") => {
                        if let Some(d) = args.get(i + 1) {
                            out_dir = std::path::PathBuf::from(d);
                        }
                        i += 2;
                    }
                    _ => break,
                }
            }
            match render_to_pngs(path, opts, &out_dir) {
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
            let Some(path) = args.get(2) else {
                eprintln!("usage: pdfgen print <file.pdf> [--dpi N] [--printer NAME]");
                return ExitCode::from(2);
            };
            let mut printer = None;
            let mut i = 3;
            while i + 1 < args.len() + 1 {
                match args.get(i).map(String::as_str) {
                    Some("--printer") => {
                        printer = args.get(i + 1).cloned();
                        i += 2;
                    }
                    Some("--dpi") => {
                        i += 2;
                    }
                    _ => break,
                }
            }
            match print_file(path, printer) {
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
            eprintln!("usage: pdfgen <command>");
            eprintln!("commands: validate, render, print");
            ExitCode::from(2)
        }
        None => {
            eprintln!("usage: pdfgen <command>");
            eprintln!("commands: validate, render, print");
            ExitCode::from(2)
        }
    }
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

/// Rasterize pages to a temp dir and hand them to the system spooler.
fn print_file(path: &str, printer: Option<String>) -> Result<usize, String> {
    let opts = pdfgen_render::RenderOptions {
        dpi: 150.0,
        white_background: true,
    };
    let tmp = std::env::temp_dir().join(format!(
        "pdfgen-print-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let files = render_to_pngs(path, opts, &tmp)?;
    // lpr takes multiple files in one job; lp needs one command per file.
    let lpr = std::process::Command::new("lpr")
        .args(printer.as_deref().map(|p| vec!["-P", p]).unwrap_or_default())
        .args(files.iter().map(|f| f.as_os_str()))
        .status();
    match lpr {
        Ok(s) if s.success() => Ok(files.len()),
        _ => {
            // Fallback: lp, one job per page.
            let mut ok = 0;
            for f in &files {
                let mut cmd = std::process::Command::new("lp");
                if let Some(p) = &printer {
                    cmd.arg("-d").arg(p);
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
        }
    }
}
