//! The single point through which every `slang` subprocess invocation flows.

use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::error::SlangError;

/// Captured outcome of a slang invocation.
pub(crate) struct RunResult {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Spawn `binary` with `args`, capturing stdout and stderr in full.
///
/// This is the only place in the crate that calls [`Command::spawn`].
/// Centralising it keeps timeout logic, environment scrubbing, and stderr
/// capture out of the higher-level `Slang::compile` flow.
pub(crate) fn run_slang(binary: &Path, args: &[OsString]) -> Result<RunResult, SlangError> {
    let output = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| SlangError::Invocation {
            path: binary.to_path_buf(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    let exit_code = output.status.code();
    Ok(RunResult {
        exit_code,
        stdout,
        stderr,
    })
}
