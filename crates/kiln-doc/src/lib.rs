// `DocError` carries paths and captured stderr from the slang invocation.
#![allow(clippy::result_large_err)]
//! Documentation generation for `kiln`.
//!
//! Two-pass extractor:
//! 1. **Source pass** scans every source file line-by-line, collecting
//!    `///` (item) and `//!` (file/scope) comment blocks plus the token
//!    that immediately follows them.
//! 2. **AST pass** asks `slang-rs` for the typed list of top-level items
//!    (modules, packages) and their names.
//! 3. **Join**: each item picks up the `///` block whose end-line is
//!    immediately above its declaration. Unattached `///` blocks are
//!    dropped (with a debug log).
//!
//! Output: a small static site under `target/doc/` with one HTML
//! file per item plus an `index.html`. Pages cross-link.

mod extract;
mod site;

pub use extract::{DocItem, DocSet, ItemKind};
pub use site::write_site;

use std::path::{Path, PathBuf};

use thiserror::Error;

use kiln_build::SourceSet;
use kiln_core::Manifest;
use slang_rs::SlangError;

#[derive(Debug, Error)]
pub enum DocError {
    #[error(transparent)]
    Slang(#[from] SlangError),

    #[error("I/O error at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Generate the documentation site under `out_dir`.
///
/// `manifest_top` is the package name, used as the site title.
pub fn generate(
    slang: &slang_rs::Slang,
    manifest: &Manifest,
    source_set: &SourceSet,
    out_dir: &Path,
) -> Result<DocSet, DocError> {
    let docset = extract::extract(slang, manifest, source_set)?;
    site::write_site(&manifest.design.top, &docset, out_dir)?;
    Ok(docset)
}
