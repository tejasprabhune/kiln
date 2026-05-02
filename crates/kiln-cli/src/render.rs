//! Render `BuildDiagnostic`s for the terminal.
//!
//! Plain-text format that includes the source line and a caret pointing
//! at the offending column when the diagnostic carries a span. Falls back
//! to a one-line label when it doesn't.
//!
//! Ariadne is in the dependency tree (per the milestones doc) but the
//! plain renderer here is more than sufficient for the M2 acceptance
//! criterion ("diagnostic with correct file/line/col that visually points
//! at the offending token"). M3 swaps in a richer renderer.

use std::fmt::Write as _;

use kiln_build::{BuildDiagnostic, Severity};

pub fn render(diags: &[BuildDiagnostic]) -> String {
    let mut out = String::new();
    for diag in diags {
        match (&diag.file, diag.line, diag.column) {
            (Some(file), Some(line), Some(col)) => {
                let code = diag
                    .code
                    .as_deref()
                    .map(|c| format!(" [{c}]"))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{} {}:{}:{}: {}{}",
                    severity_label(diag.severity),
                    file.display(),
                    line,
                    col,
                    diag.message,
                    code
                );
                if let Ok(text) = std::fs::read_to_string(file) {
                    if let Some(line_text) = text.lines().nth((line as usize).saturating_sub(1)) {
                        let _ = writeln!(out, "    {line:>4} | {line_text}");
                        let pad = " ".repeat(col.saturating_sub(1) as usize);
                        let _ = writeln!(out, "         | {pad}^");
                    }
                }
            }
            _ => {
                let code = diag
                    .code
                    .as_deref()
                    .map(|c| format!(" [{c}]"))
                    .unwrap_or_default();
                let _ = writeln!(
                    out,
                    "{} {}{}",
                    severity_label(diag.severity),
                    diag.message,
                    code
                );
            }
        }
    }
    out
}

fn severity_label(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error:",
        Severity::Warning => "warning:",
        Severity::Note => "note:",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn render_handles_diagnostics_without_location() {
        let d = BuildDiagnostic {
            severity: Severity::Error,
            code: None,
            file: None,
            line: None,
            column: None,
            message: "no location here".to_string(),
        };
        let rendered = render(std::slice::from_ref(&d));
        assert!(rendered.contains("error:"));
        assert!(rendered.contains("no location here"));
    }

    #[test]
    fn render_includes_caret_when_file_readable() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), "line one\nline two\nline three\n").unwrap();
        let d = BuildDiagnostic {
            severity: Severity::Warning,
            code: Some("CODE".to_string()),
            file: Some(tmp.path().to_path_buf()),
            line: Some(2),
            column: Some(6),
            message: "something".to_string(),
        };
        let rendered = render(std::slice::from_ref(&d));
        assert!(rendered.contains("warning:"));
        assert!(rendered.contains("line two"));
        assert!(rendered.contains("^"));
    }

    #[test]
    fn render_skips_caret_when_file_missing() {
        let d = BuildDiagnostic {
            severity: Severity::Error,
            code: None,
            file: Some(PathBuf::from("/definitely/not/here.sv")),
            line: Some(1),
            column: Some(1),
            message: "boom".to_string(),
        };
        let rendered = render(std::slice::from_ref(&d));
        assert!(rendered.contains("error:"));
    }
}
