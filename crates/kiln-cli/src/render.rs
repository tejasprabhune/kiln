//! Render `BuildDiagnostic`s for the terminal using ariadne.

use kiln_build::BuildDiagnostic;

pub fn render(diags: &[BuildDiagnostic]) -> String {
    kiln_build::render::format_diagnostics(diags)
}

#[cfg(test)]
mod tests {
    use super::*;
    use kiln_build::Severity;
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
        assert!(rendered.contains("Error") || rendered.contains("error"));
        assert!(rendered.contains("no location here"));
    }

    #[test]
    fn render_includes_source_when_file_readable() {
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
        assert!(rendered.contains("line two"));
    }

    #[test]
    fn render_falls_back_when_file_missing() {
        let d = BuildDiagnostic {
            severity: Severity::Error,
            code: None,
            file: Some(PathBuf::from("/definitely/not/here.sv")),
            line: Some(1),
            column: Some(1),
            message: "boom".to_string(),
        };
        let rendered = render(std::slice::from_ref(&d));
        assert!(rendered.contains("boom"));
    }
}
