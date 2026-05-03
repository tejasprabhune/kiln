// `BuildError` carries paths and captured stderr; surfacing them is the whole
// point of this crate. Suppress the lint here for the same reason we did in
// `slang-rs`.
#![allow(clippy::result_large_err)]
//! Build pipeline and simulator backends for `kiln`.
//!
//! The crate is organised so the high-level `kiln build` flow is a small
//! sequence of steps:
//!
//! 1. [`SourceSet::resolve`] expands the manifest's globs into absolute
//!    source paths.
//! 2. [`BuildPlan::new`] turns a manifest + source set + profile into an
//!    invocation plan with a deterministic content hash.
//! 3. A backend (today: [`backend::verilator`]) compiles that plan and
//!    parses the simulator's output into [`Diagnostic`]s.
//!
//! This crate is intentionally simulator-agnostic at the type level;
//! M5 will plug a Cocotb backend onto the same plan. M3 will reuse
//! [`Diagnostic`] for the slang-driven `kiln check` rendering path.

pub mod backend;
pub mod cache;
pub mod diagnostic;
pub mod plan;
pub mod render;
pub mod source_set;

pub use backend::BackendError;
pub use cache::{cache_dir, BuildCacheKey};
pub use diagnostic::{BuildDiagnostic, Severity};
pub use plan::{BuildPlan, Profile};
pub use source_set::{SourceSet, SourceSetError};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BuildError {
    #[error(transparent)]
    SourceSet(#[from] SourceSetError),

    #[error(transparent)]
    Backend(#[from] BackendError),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
}
