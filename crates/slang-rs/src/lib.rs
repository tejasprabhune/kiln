// `SlangError` is intentionally large (carries paths, version info, captured
// stderr). Boxing every fallible function's return for that 128-byte penalty
// is worse than the lint suggests; surfacing rich errors is the whole point
// of this layer.
#![allow(clippy::result_large_err)]
//! Pure-Rust subprocess wrapper around the `slang` SystemVerilog compiler CLI.
//!
//! `slang-rs` shells out to a `slang` binary that the user installs
//! separately. It is **not** an FFI binding to libslang — see
//! `docs/decisions/0001-slang-integration-strategy.md` in the kiln repo.
//!
//! # Quick start
//!
//! ```no_run
//! use std::path::PathBuf;
//! use slang_rs::{Slang, CompileRequest};
//!
//! let slang = Slang::new()?;
//! let req = CompileRequest::builder()
//!     .source(PathBuf::from("src/top.sv"))
//!     .top("top")
//!     .build();
//! let result = slang.compile(&req)?;
//! for diag in &result.diagnostics {
//!     println!("{:?}: {}", diag.severity, diag.message);
//! }
//! # Ok::<(), slang_rs::SlangError>(())
//! ```
//!
//! # Architecture
//!
//! - [`Slang`] is the entry point. Construct it once per CLI run.
//! - [`CompileRequest`] is a builder for an invocation; pass it to
//!   [`Slang::compile`].
//! - [`CompileResult`] carries the parsed AST (when one was requested) and
//!   the list of [`Diagnostic`]s.
//! - All process invocation funnels through `run_slang()` (private). That
//!   helper centralises timeout handling, stderr capture, and error
//!   conversion.

mod ast;
mod compile;
mod diagnostic;
mod error;
mod handle;
mod runner;
mod version;

pub use ast::{Ast, AstNode, ExtraFields};
pub use compile::{CompileRequest, CompileRequestBuilder, CompileResult, SvStandard};
pub use diagnostic::{Diagnostic, Severity};
pub use error::SlangError;
pub use handle::{Slang, MIN_VERSION};
pub use version::SlangVersion;
