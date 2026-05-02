//! Typed errors for the `slang-rs` crate.

use std::path::PathBuf;

use thiserror::Error;

use crate::version::SlangVersion;

/// Errors produced by `slang-rs`.
#[derive(Debug, Error)]
pub enum SlangError {
    /// The `slang` binary was not found on `PATH` or via `KILN_SLANG_PATH`.
    #[error(
        "could not find the `slang` binary: {reason}.\n\
         Install slang and ensure it is on your PATH, or set `KILN_SLANG_PATH`.\n\
         {install_hint}"
    )]
    BinaryNotFound {
        reason: String,
        install_hint: String,
    },

    /// The slang at the given path is older than the supported floor.
    #[error(
        "slang at {path} reports version {found}, but slang-rs requires {required} or newer.\n\
         {install_hint}"
    )]
    UnsupportedVersion {
        path: PathBuf,
        found: SlangVersion,
        required: SlangVersion,
        install_hint: String,
    },

    /// Failed to spawn or wait on the slang process.
    #[error("failed to invoke slang at {path}: {source}")]
    Invocation {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    /// slang exited non-zero. Stderr is captured.
    #[error("slang exited with code {code}.\nstderr:\n{stderr}")]
    NonZeroExit { code: i32, stderr: String },

    /// slang was killed by a signal or could not produce an exit code.
    #[error("slang was terminated without an exit code (signal? OOM?).\nstderr:\n{stderr}")]
    NoExitCode { stderr: String },

    /// We could not parse slang's `--version` output.
    #[error("could not parse slang version output: {reason}.\nraw output:\n{raw}")]
    ParseVersion { reason: String, raw: String },

    /// We could not parse slang's diagnostic JSON.
    #[error("could not parse slang diagnostic JSON: {0}")]
    ParseDiagnostics(String),

    /// We could not parse slang's AST JSON.
    #[error("could not parse slang AST JSON: {0}")]
    ParseAst(String),
}

/// Returns a platform-specific install hint for the `slang` binary.
///
/// Surfaced inline in [`SlangError::BinaryNotFound`] and
/// [`SlangError::UnsupportedVersion`] so users see a concrete next step.
pub(crate) fn install_hint() -> String {
    let url = "https://github.com/MikePopoloski/slang";
    if cfg!(target_os = "macos") {
        format!(
            "On macOS, build from source: `git clone {url} && cd slang && cmake -B build && cmake --build build -j` then add `slang/build/bin` to your PATH."
        )
    } else if cfg!(target_os = "linux") {
        format!(
            "On Linux, build from source: `git clone {url} && cd slang && cmake -B build && cmake --build build -j` then add `slang/build/bin` to your PATH (or install via your distro's package manager if available)."
        )
    } else {
        format!("Build from source: {url}")
    }
}
