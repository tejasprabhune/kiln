//! Verilator backend. Wraps `verilator --binary` and parses its output
//! into [`crate::BuildDiagnostic`]s.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::backend::BackendError;
use crate::cache::{cache_dir, BuildCacheKey};
use crate::diagnostic::{BuildDiagnostic, Severity};
use crate::plan::{BuildPlan, Profile};

/// Result of a Verilator invocation.
#[derive(Debug)]
pub struct VerilatorOutcome {
    pub diagnostics: Vec<BuildDiagnostic>,
    /// Path to the produced binary, when the build succeeded.
    pub binary: Option<PathBuf>,
    /// Whether this invocation hit the cache (i.e., did not re-run verilator).
    pub cache_hit: bool,
    pub exit_code: Option<i32>,
}

const TOOL_NAME: &str = "verilator";

fn install_hint() -> &'static str {
    if cfg!(target_os = "macos") {
        "brew install verilator"
    } else if cfg!(target_os = "linux") {
        "sudo apt-get install verilator (Debian/Ubuntu) or build from source"
    } else {
        "see https://www.veripool.org/verilator/"
    }
}

/// Locate the verilator binary on PATH.
fn locate() -> Result<PathBuf, BackendError> {
    let path_var = std::env::var_os("PATH").ok_or_else(|| BackendError::BinaryNotFound {
        tool: TOOL_NAME.to_string(),
        install_hint: install_hint().to_string(),
    })?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(TOOL_NAME);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(BackendError::BinaryNotFound {
        tool: TOOL_NAME.to_string(),
        install_hint: install_hint().to_string(),
    })
}

/// Compile `plan` with Verilator, caching the result under
/// `<project_root>/target/kiln/<hash>/`.
pub fn compile(plan: &BuildPlan) -> Result<VerilatorOutcome, BackendError> {
    let key = BuildCacheKey::for_plan(plan).map_err(|source| BackendError::Io {
        path: PathBuf::new(),
        source,
    })?;
    let dir = cache_dir(&plan.project_root, &key);
    let binary_path = dir.join(format!("V{}", plan.top));

    if binary_path.is_file() {
        tracing::debug!(target: "kiln-build", path = %binary_path.display(), "verilator cache hit");
        return Ok(VerilatorOutcome {
            diagnostics: Vec::new(),
            binary: Some(binary_path),
            cache_hit: true,
            exit_code: Some(0),
        });
    }

    std::fs::create_dir_all(&dir).map_err(|source| BackendError::Io {
        path: dir.clone(),
        source,
    })?;

    let verilator = locate()?;
    let mut cmd = Command::new(&verilator);
    cmd.current_dir(&dir);
    cmd.arg("--binary")
        .arg("--top-module")
        .arg(&plan.top)
        .arg("--sv")
        .arg("--Mdir")
        .arg(".")
        .arg("-o")
        .arg(format!("V{}", plan.top));
    if matches!(plan.profile, Profile::Release) {
        cmd.arg("-O3");
        cmd.arg("--x-assign").arg("0");
    }
    if plan.trace {
        cmd.arg("--trace").arg("--trace-fst");
    }
    for inc in &plan.include_dirs {
        cmd.arg("-I").arg(inc);
    }
    for (k, v) in &plan.defines {
        let combined = if v.is_empty() {
            k.clone()
        } else {
            format!("{k}={v}")
        };
        let mut arg = OsString::from("+define+");
        arg.push(combined);
        cmd.arg(arg);
    }
    for arg in &plan.extra_verilator_args {
        cmd.arg(arg);
    }
    for src in &plan.sources {
        cmd.arg(src);
    }

    let output = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| BackendError::Invocation {
            tool: TOOL_NAME.to_string(),
            path: verilator.clone(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code();

    // Verilator emits diagnostics to *both* stdout and stderr depending on
    // version. Parse both, dedupe by (file, line, col, message).
    let mut diagnostics = parse_output(&stdout);
    diagnostics.extend(parse_output(&stderr));
    diagnostics.sort_by(|a, b| {
        (a.file.clone(), a.line, a.column, &a.message).cmp(&(
            b.file.clone(),
            b.line,
            b.column,
            &b.message,
        ))
    });
    diagnostics.dedup();

    let binary = if binary_path.is_file() {
        Some(binary_path)
    } else if exit_code == Some(0) {
        // Verilator succeeded but did not produce a binary at the expected
        // path. That's unusual; surface it.
        return Err(BackendError::MissingOutput {
            tool: TOOL_NAME.to_string(),
            expected: binary_path,
            stdout_tail: tail_of(&stdout),
            stderr_tail: tail_of(&stderr),
        });
    } else {
        None
    };

    Ok(VerilatorOutcome {
        diagnostics,
        binary,
        cache_hit: false,
        exit_code,
    })
}

fn tail_of(s: &str) -> String {
    s.lines()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parse Verilator stdout/stderr text into diagnostics.
///
/// The shape of a diagnostic line is:
///
/// ```text
/// %<Severity>(-<CODE>)?: (<file>:<line>:<col>: )?<message>
/// ```
///
/// Plus indented continuation lines (notes / source quotes / hints) which
/// we currently drop. The summary line `%Error: Exiting due to N error(s)`
/// is skipped because it duplicates information already in the per-error
/// entries.
pub(crate) fn parse_output(text: &str) -> Vec<BuildDiagnostic> {
    let mut out = Vec::new();
    for line in text.lines() {
        // Skip continuation lines (they start with whitespace and aren't
        // diagnostic openers).
        if !line.starts_with('%') {
            continue;
        }
        if let Some(diag) = parse_diagnostic_line(line) {
            // Drop the "Exiting due to N error(s)" summary line.
            if diag.message.starts_with("Exiting due to ") {
                continue;
            }
            out.push(diag);
        }
    }
    out
}

fn parse_diagnostic_line(line: &str) -> Option<BuildDiagnostic> {
    // %<Severity>(-<CODE>)?: <rest>
    let rest = line.strip_prefix('%')?;
    let (head, after_colon) = rest.split_once(": ")?;
    let (severity, code) = match head.split_once('-') {
        Some((sev, code)) => (parse_severity(sev)?, Some(code.to_string())),
        None => (parse_severity(head)?, None),
    };

    // Optional file:line:col prefix. Verilator uses ASCII digits and
    // colons; the file portion is everything up to the first `:<digits>:`
    // sequence. Use a from-the-right scan because filenames on Windows or
    // odd configurations may contain a `:`.
    if let Some(diag) = try_parse_with_location(after_colon, severity, code.clone()) {
        return Some(diag);
    }
    // No location: the rest is the message verbatim.
    Some(BuildDiagnostic {
        severity,
        code,
        file: None,
        line: None,
        column: None,
        message: after_colon.trim().to_string(),
    })
}

fn try_parse_with_location(
    s: &str,
    severity: Severity,
    code: Option<String>,
) -> Option<BuildDiagnostic> {
    // Walk: find the first occurrence of `:<digits>:<digits>:` after at
    // least one non-empty file segment.
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b':' {
            // Try to read line and column.
            let mut j = i + 1;
            let line_start = j;
            while j < bytes.len() && bytes[j].is_ascii_digit() {
                j += 1;
            }
            if j > line_start && j < bytes.len() && bytes[j] == b':' {
                let line_end = j;
                let mut k = j + 1;
                let col_start = k;
                while k < bytes.len() && bytes[k].is_ascii_digit() {
                    k += 1;
                }
                if k > col_start && k < bytes.len() && bytes[k] == b':' {
                    let line: u32 = std::str::from_utf8(&bytes[line_start..line_end])
                        .ok()?
                        .parse()
                        .ok()?;
                    let col: u32 = std::str::from_utf8(&bytes[col_start..k])
                        .ok()?
                        .parse()
                        .ok()?;
                    let file_part = std::str::from_utf8(&bytes[..i]).ok()?;
                    if file_part.trim().is_empty() {
                        return None;
                    }
                    let msg_start = k + 1;
                    if msg_start >= bytes.len() {
                        return None;
                    }
                    let msg = std::str::from_utf8(&bytes[msg_start..]).ok()?.trim();
                    return Some(BuildDiagnostic {
                        severity,
                        code,
                        file: Some(PathBuf::from(file_part.trim())),
                        line: Some(line),
                        column: Some(col),
                        message: msg.to_string(),
                    });
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_severity(s: &str) -> Option<Severity> {
    match s.trim() {
        "Error" => Some(Severity::Error),
        "Warning" => Some(Severity::Warning),
        "Note" => Some(Severity::Note),
        _ => None,
    }
}

/// Remove the cache directory under `project_root/target/kiln/`.
pub fn clean(project_root: &Path) -> std::io::Result<()> {
    let dir = project_root.join("target").join("kiln");
    if dir.exists() {
        std::fs::remove_dir_all(&dir)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_warning_with_code_and_location() {
        let line = "%Warning-PROCASSINIT: tb.sv:2:17: Procedural assignment to declaration with initial value: 'clk'";
        let d = parse_diagnostic_line(line).unwrap();
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code.as_deref(), Some("PROCASSINIT"));
        assert_eq!(d.file.unwrap().to_string_lossy(), "tb.sv");
        assert_eq!(d.line, Some(2));
        assert_eq!(d.column, Some(17));
        assert!(d.message.contains("Procedural assignment"));
    }

    #[test]
    fn parses_error_without_code() {
        let line = "%Error: syntax_err.sv:2:5: syntax error, unexpected input, expecting ';'";
        let d = parse_diagnostic_line(line).unwrap();
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, None);
        assert_eq!(d.line, Some(2));
        assert_eq!(d.column, Some(5));
    }

    #[test]
    fn parses_error_without_location() {
        let line = "%Error: Some catastrophe";
        let d = parse_diagnostic_line(line).unwrap();
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.file, None);
        assert_eq!(d.line, None);
        assert_eq!(d.message, "Some catastrophe");
    }

    #[test]
    fn parse_output_drops_exit_summary() {
        let text = "\
%Error: foo.sv:5:3: bad syntax\n\
%Error: Exiting due to 1 error(s)\n";
        let diags = parse_output(text);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].line, Some(5));
    }

    #[test]
    fn parse_output_skips_continuation_lines() {
        let text = "\
%Warning-PROCASSINIT: tb.sv:2:17: Procedural assignment to declaration with initial value: 'clk'\n\
                                : ... note: In instance 'tb'\n\
                                : ... Location of variable initialization\n\
    2 |     logic clk = 0;\n\
      |                 ^\n";
        let diags = parse_output(text);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code.as_deref(), Some("PROCASSINIT"));
    }

    #[test]
    fn parse_output_real_syntax_error_capture() {
        let text = include_str!("../../tests/fixtures/captured/syntax_error.verilator.txt");
        let diags = parse_output(text);
        let errors: Vec<_> = diags
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .collect();
        assert!(!errors.is_empty(), "expected ≥1 error");
        let first = errors[0];
        assert_eq!(first.line, Some(2));
        assert!(first.file.is_some());
    }

    #[test]
    fn unknown_severity_is_ignored() {
        let line = "%Mystery: foo.sv:1:1: ?";
        assert!(parse_diagnostic_line(line).is_none());
    }
}
