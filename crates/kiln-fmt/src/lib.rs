// `FmtError` carries paths and captured stderr from the verible invocation.
#![allow(clippy::result_large_err)]
//! Formatting for `kiln`: subprocess wrapper around
//! [`verible-verilog-format`](https://github.com/chipsalliance/verible).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum FmtError {
    #[error(
        "could not find `verible-verilog-format` on PATH.\n\
         Install verible (e.g. `brew install verible`, or download a release \
         from https://github.com/chipsalliance/verible/releases) and ensure \
         the binary is on your PATH."
    )]
    BinaryNotFound,

    #[error("failed to invoke verible-verilog-format at {path}: {source}")]
    Invocation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("verible-verilog-format exited with code {code}.\nstderr:\n{stderr}")]
    Cli { code: i32, stderr: String },

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

const BIN: &str = "verible-verilog-format";

fn locate() -> Result<PathBuf, FmtError> {
    let path_var = std::env::var_os("PATH").ok_or(FmtError::BinaryNotFound)?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(BIN);
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(FmtError::BinaryNotFound)
}

/// Format `path` in place. Returns true if the file was changed.
pub fn format_in_place(path: &Path) -> Result<bool, FmtError> {
    let formatted = run_format(path)?;
    let original = std::fs::read_to_string(path).map_err(|source| FmtError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    if formatted == original {
        return Ok(false);
    }
    std::fs::write(path, &formatted).map_err(|source| FmtError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(true)
}

/// Check `path` without modifying it. Returns the unified diff (empty
/// string when the file is already formatted) and a flag indicating
/// whether the file is in canonical form.
pub fn check(path: &Path) -> Result<CheckOutcome, FmtError> {
    let original = std::fs::read_to_string(path).map_err(|source| FmtError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let formatted = run_format(path)?;
    if formatted == original {
        return Ok(CheckOutcome {
            file: path.to_path_buf(),
            ok: true,
            diff: String::new(),
        });
    }
    Ok(CheckOutcome {
        file: path.to_path_buf(),
        ok: false,
        diff: unified_diff(&path.to_string_lossy(), &original, &formatted),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CheckOutcome {
    pub file: PathBuf,
    pub ok: bool,
    pub diff: String,
}

fn run_format(path: &Path) -> Result<String, FmtError> {
    let bin = locate()?;
    let output = Command::new(&bin)
        .arg(path)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| FmtError::Invocation {
            path: bin.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(FmtError::Cli {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Tiny unified-diff renderer. Adequate for "show me what changed";
/// not byte-perfect against `diff -u`, but stable across platforms.
fn unified_diff(file: &str, before: &str, after: &str) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "--- {file}");
    let _ = writeln!(s, "+++ {file} (formatted)");
    let before_lines: Vec<&str> = before.lines().collect();
    let after_lines: Vec<&str> = after.lines().collect();
    let max = before_lines.len().max(after_lines.len());
    for i in 0..max {
        let b = before_lines.get(i);
        let a = after_lines.get(i);
        match (b, a) {
            (Some(bl), Some(al)) if bl == al => {
                let _ = writeln!(s, " {bl}");
            }
            (Some(bl), Some(al)) => {
                let _ = writeln!(s, "-{bl}");
                let _ = writeln!(s, "+{al}");
            }
            (Some(bl), None) => {
                let _ = writeln!(s, "-{bl}");
            }
            (None, Some(al)) => {
                let _ = writeln!(s, "+{al}");
            }
            (None, None) => {}
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diff_empty_when_identical() {
        let d = unified_diff("a.sv", "module a; endmodule\n", "module a; endmodule\n");
        // We always emit the header, but body is unchanged-context only.
        assert!(d.contains("--- a.sv"));
        assert!(d.contains(" module a; endmodule"));
        assert!(!d.contains("-module"));
        assert!(!d.contains("+module"));
    }

    #[test]
    fn diff_marks_changed_lines() {
        let d = unified_diff("a.sv", "x\ny\nz\n", "x\nY\nz\n");
        assert!(d.contains("-y"));
        assert!(d.contains("+Y"));
        // Unchanged context preserved.
        assert!(d.contains(" x"));
        assert!(d.contains(" z"));
    }

    #[test]
    fn diff_handles_added_lines() {
        let d = unified_diff("a.sv", "x\n", "x\ny\nz\n");
        assert!(d.contains("+y"));
        assert!(d.contains("+z"));
    }

    #[test]
    fn missing_verible_yields_clear_error() {
        // Override PATH so verible is not findable. Use a non-existent
        // dir to be safe.
        let original = std::env::var_os("PATH");
        unsafe {
            std::env::set_var("PATH", "/tmp/no/such/dir/i/swear");
        }
        let err = locate().unwrap_err();
        if let Some(p) = original {
            unsafe {
                std::env::set_var("PATH", p);
            }
        }
        assert!(matches!(err, FmtError::BinaryNotFound));
        let msg = err.to_string();
        assert!(msg.contains("verible-verilog-format"));
        assert!(msg.contains("brew install verible") || msg.contains("releases"));
    }
}
