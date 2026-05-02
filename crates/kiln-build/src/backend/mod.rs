//! Simulator backends. Today: [`verilator`]. M5 will plug Cocotb here.

use std::path::PathBuf;

use thiserror::Error;

pub mod verilator;

#[derive(Debug, Error)]
pub enum BackendError {
    #[error(
        "could not find the `{tool}` binary on PATH.\n\
         Install {tool} (e.g., `{install_hint}`) and ensure it is on your PATH."
    )]
    BinaryNotFound { tool: String, install_hint: String },

    #[error("failed to invoke {tool} at {path}: {source}")]
    Invocation {
        tool: String,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error(
        "{tool} exited with code {code} and reported errors. \
         See diagnostics for details. stderr tail:\n{stderr_tail}"
    )]
    NonZero {
        tool: String,
        code: i32,
        stderr_tail: String,
    },

    #[error(
        "{tool} exited successfully but did not produce the expected output `{expected}`.\n\
         stdout tail:\n{stdout_tail}\nstderr tail:\n{stderr_tail}"
    )]
    MissingOutput {
        tool: String,
        expected: PathBuf,
        stdout_tail: String,
        stderr_tail: String,
    },

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}
