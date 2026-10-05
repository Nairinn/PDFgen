//! `pdfgen` — command-line interface.
//!
//! `pdfgen validate <file.pdf>` — run the Matterhorn machine checks and
//! print findings with checkpoint IDs (exit 1 when any are found).

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
        Some(other) => {
            eprintln!("unknown command: {other}\nusage: pdfgen validate <file.pdf>");
            ExitCode::from(2)
        }
        None => {
            eprintln!("usage: pdfgen <command>\ncommands: validate");
            ExitCode::from(2)
        }
    }
}
