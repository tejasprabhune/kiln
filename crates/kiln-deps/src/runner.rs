//! The single point through which every `bender` subprocess invocation flows.
//!
//! Mirrors `slang_rs::run_slang` and `kiln-build::backend::verilator`'s
//! invocation discipline: one helper, captured stderr, structured errors.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use thiserror::Error;

use kiln_core::ManifestError;

#[derive(Debug, Error)]
pub enum BenderError {
    #[error(
        "could not find the `bender` binary on PATH.\n\
         Install bender (e.g. `cargo install bender`) and ensure it is on your PATH."
    )]
    BinaryNotFound,

    #[error("failed to invoke bender at {path}: {source}")]
    Invocation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("bender exited with code {code}.\nstderr:\n{stderr}")]
    Cli { code: i32, stderr: String },

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("invalid `[dependencies]` entry for `{name}`: {reason}")]
    BadDependency { name: String, reason: String },

    #[error("could not parse bender output as JSON: {0}")]
    ParseOutput(String),

    #[error(transparent)]
    Manifest(#[from] ManifestError),

    #[error(
        "lockfile drift: `Kiln.lock` would change to match `Kiln.toml`. \
         Run `kiln update` and commit the result, or drop `--locked` / `--frozen`."
    )]
    LockDrift,

    #[error(
        "`--frozen` forbids dependency resolution; \
         existing `Kiln.lock` not found at {path}"
    )]
    FrozenWithoutLock { path: PathBuf },
}

pub(crate) struct RunOutput {
    pub stdout: String,
    #[allow(dead_code)]
    pub stderr: String,
}

fn locate() -> Result<PathBuf, BenderError> {
    let path_var = std::env::var_os("PATH").ok_or(BenderError::BinaryNotFound)?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("bender");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(BenderError::BinaryNotFound)
}

/// Run `bender <args...>` in `cwd`, surface non-zero exits as
/// [`BenderError::Cli`]. Bender's stdout/stderr are captured and
/// discarded — kiln owns the terminal output and shows its own status lines.
pub(crate) fn run_bender(cwd: &Path, args: &[&str]) -> Result<(), BenderError> {
    let bin = locate()?;
    let output = Command::new(&bin)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| BenderError::Invocation {
            path: bin.clone(),
            source,
        })?;
    if !output.status.success() {
        return Err(BenderError::Cli {
            code: output.status.code().unwrap_or(-1),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(())
}

/// Run `bender <args...>` in `cwd`, capture stdout and stderr, return
/// both on success.
pub(crate) fn run_bender_capture(cwd: &Path, args: &[&str]) -> Result<RunOutput, BenderError> {
    let bin = locate()?;
    let output = Command::new(&bin)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|source| BenderError::Invocation {
            path: bin.clone(),
            source,
        })?;
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    if !output.status.success() {
        return Err(BenderError::Cli {
            code: output.status.code().unwrap_or(-1),
            stderr,
        });
    }
    Ok(RunOutput { stdout, stderr })
}
