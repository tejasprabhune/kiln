//! Manifest, project model, and shared error types for `kiln`.
//!
//! The crate is intentionally narrow: it parses and validates `Kiln.toml`,
//! resolves the project root from a working directory, and exposes typed
//! errors that consumer crates (and the CLI) bubble up.
//!
//! # Example
//!
//! ```
//! use kiln_core::Manifest;
//!
//! let toml = r#"
//! [package]
//! name = "demo"
//! version = "0.1.0"
//!
//! [design]
//! top = "demo_top"
//! "#;
//! let manifest: Manifest = toml.parse().unwrap();
//! assert_eq!(manifest.package.name, "demo");
//! assert_eq!(manifest.design.top, "demo_top");
//! ```

pub mod manifest;
pub mod project;

pub use manifest::{
    Design, LintConfig, LintSeverity, Manifest, ManifestError, Package, ValidateOptions,
    WaveConfig, WaveFormat,
};
pub use project::{find_manifest, ProjectError, MANIFEST_FILENAME};
