//! `gdmerge check`

use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::Serialize;
use tscn::{CheckReport, Document, Severity};

use crate::io;

#[derive(Serialize)]
struct FileReport {
    file: String,
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_error: Option<String>,
    #[serde(flatten)]
    report: CheckReport,
}

pub fn run(files: &[PathBuf], json: bool) -> Result<i32> {
    let mut reports = Vec::new();
    let mut failed = 0usize;

    for path in files {
        let report = check_one(path);
        if !report.ok {
            failed += 1;
        }
        reports.push(report);
    }

    if json {
        println!("{}", serde_json::to_string_pretty(&reports)?);
    } else {
        for r in &reports {
            if let Some(err) = &r.parse_error {
                println!("FAIL {}\n  parse error: {err}", r.file);
                continue;
            }
            if r.report.issues.is_empty() {
                println!("ok   {}", r.file);
                continue;
            }
            println!("{} {}", if r.ok { "warn" } else { "FAIL" }, r.file);
            for issue in &r.report.issues {
                let tag = match issue.severity {
                    Severity::Error => "error",
                    Severity::Warning => "warning",
                };
                println!("  {tag}: {}", issue.message);
            }
        }
        let n = files.len();
        println!("\n{n} file{} checked, {failed} failed", if n == 1 { "" } else { "s" });
    }

    Ok(if failed > 0 { 1 } else { 0 })
}

fn check_one(path: &Path) -> FileReport {
    let file = path.display().to_string();
    let src = match io::read(path) {
        Ok(s) => s,
        Err(e) => {
            return FileReport {
                file,
                ok: false,
                parse_error: Some(format!("{e:#}")),
                report: CheckReport::default(),
            }
        }
    };
    match Document::parse(&src) {
        Err(e) => FileReport {
            file,
            ok: false,
            parse_error: Some(e.to_string()),
            report: CheckReport::default(),
        },
        Ok(doc) => {
            let report = tscn::check(&doc, &src);
            FileReport { file, ok: !report.has_errors(), parse_error: None, report }
        }
    }
}
