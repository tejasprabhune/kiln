//! Ariadne-based renderer for [`BuildDiagnostic`] slices.
//!
//! Produces rustc-style source snippets with colored underlines. Falls back
//! to a plain `severity: message at file:line:col` line when the source file
//! cannot be read or location info is absent.

use ariadne::{Color, Config, Label, Report, ReportKind, Source};

use crate::diagnostic::{BuildDiagnostic, Severity};

/// Render `diags` to stderr using ariadne source snippets where possible.
pub fn print_diagnostics(diags: &[BuildDiagnostic]) {
    for d in diags {
        if !try_print_snippet(d) {
            eprintln!("{}", plain_line(d));
        }
    }
}

/// Render `diags` to a `String` (for capture in tests or buffered output).
/// Colors are disabled so the returned string is free of ANSI escape codes.
pub fn format_diagnostics(diags: &[BuildDiagnostic]) -> String {
    let mut out = String::new();
    for d in diags {
        if let Some(s) = try_render_snippet(d, false) {
            out.push_str(&s);
        } else {
            out.push_str(&plain_line(d));
            out.push('\n');
        }
    }
    out
}

fn severity_to_kind(s: Severity) -> ReportKind<'static> {
    match s {
        Severity::Error => ReportKind::Error,
        Severity::Warning => ReportKind::Warning,
        Severity::Note => ReportKind::Advice,
    }
}

fn severity_color(s: Severity) -> Color {
    match s {
        Severity::Error => Color::Red,
        Severity::Warning => Color::Yellow,
        Severity::Note => Color::Cyan,
    }
}

fn try_print_snippet(d: &BuildDiagnostic) -> bool {
    let Some(s) = try_render_snippet(d, true) else {
        return false;
    };
    eprint!("{s}");
    true
}

fn try_render_snippet(d: &BuildDiagnostic, color: bool) -> Option<String> {
    let file = d.file.as_ref()?;
    let line = d.line? as usize;
    let src = std::fs::read_to_string(file).ok()?;

    // Compute byte offset of start of `line` (1-based).
    let line_start: usize = src
        .lines()
        .take(line.saturating_sub(1))
        .map(|l| l.len() + 1)
        .sum();
    let col = d.column.unwrap_or(1) as usize;
    let span_start = line_start + col.saturating_sub(1);
    let span_end = (span_start + 1).min(src.len());

    let file_str = file.display().to_string();
    let kind = severity_to_kind(d.severity);
    let label_color = severity_color(d.severity);

    let mut buf = Vec::<u8>::new();
    Report::build(kind, &file_str, span_start)
        .with_config(Config::default().with_color(color))
        .with_message(&d.message)
        .with_label(Label::new((&file_str, span_start..span_end)).with_color(label_color))
        .finish()
        .write((&file_str, Source::from(&src)), &mut buf)
        .ok()?;

    String::from_utf8(buf).ok()
}

fn plain_line(d: &BuildDiagnostic) -> String {
    let loc = match (&d.file, d.line, d.column) {
        (Some(f), Some(l), Some(c)) => format!(" at {}:{l}:{c}", f.display()),
        (Some(f), Some(l), None) => format!(" at {}:{l}", f.display()),
        (Some(f), None, _) => format!(" at {}", f.display()),
        _ => String::new(),
    };
    format!("{:?}: {}{loc}", d.severity, d.message)
}
