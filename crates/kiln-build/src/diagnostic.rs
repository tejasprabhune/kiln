//! A simulator-agnostic diagnostic shape.
//!
//! Backend parsers (verilator today, others later) translate their tools'
//! native diagnostic format into [`BuildDiagnostic`]. Higher layers (the
//! CLI's ariadne renderer, M3's `kiln check`) consume this shape.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Error,
    Warning,
    Note,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildDiagnostic {
    pub severity: Severity,
    /// Tool-specific diagnostic code (e.g. Verilator's `PROCASSINIT` or
    /// slang's `width-trunc`). May be absent for syntax errors that the
    /// tool emits without a category.
    pub code: Option<String>,
    pub file: Option<PathBuf>,
    pub line: Option<u32>,
    pub column: Option<u32>,
    pub message: String,
}
