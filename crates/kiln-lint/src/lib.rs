// `LintError` carries paths and captured stderr from the slang invocation.
#![allow(clippy::result_large_err)]
//! Linting for `kiln`. Drives `slang-rs` for fast (sub-second) elaboration
//! checks; reuses [`kiln_build::BuildDiagnostic`] so `kiln check` and `kiln
//! build` render identically.

use std::collections::BTreeMap;

use thiserror::Error;

use kiln_build::{BuildDiagnostic, Severity, SourceSet};
use kiln_core::{LintSeverity, Manifest};
use slang_rs::{CompileRequest, Severity as SlangSeverity, Slang, SlangError};

#[derive(Debug, Error)]
pub enum LintError {
    #[error(transparent)]
    Slang(#[from] SlangError),
}

/// Run a Slang elaboration over the project's source set and apply the
/// `[lint]` severity overrides from the manifest.
pub fn check(
    slang: &Slang,
    manifest: &Manifest,
    source_set: &SourceSet,
) -> Result<Vec<BuildDiagnostic>, LintError> {
    let mut req = CompileRequest::builder().top(&manifest.design.top);
    for s in source_set.files() {
        req = req.source(s.clone());
    }
    for d in &manifest.design.include_dirs {
        req = req.include_dir(source_set.project_root.join(d));
    }
    for (k, v) in &manifest.design.defines {
        req = req.define(k.clone(), v.clone());
    }
    // Enable every `-W` knob the user asked us to surface; that keeps the
    // override map a no-op for things slang would otherwise silently drop.
    for (id, sev) in &manifest.lint.rules {
        if matches!(sev, LintSeverity::Error | LintSeverity::Warn) {
            req = req.extra_arg(format!("-W{id}"));
        }
    }
    // We do *not* pass `--parse-only` here. Slang skips writing the
    // `--diag-json` file when parse-only is set, and we want full
    // elaboration anyway so semantic warnings (width-trunc, etc.) fire.
    let req = req.build();

    let result = slang.compile(&req)?;
    let diagnostics = result
        .diagnostics
        .into_iter()
        .filter_map(|d| convert(d, &manifest.lint.rules))
        .collect();
    Ok(diagnostics)
}

fn convert(
    d: slang_rs::Diagnostic,
    rules: &BTreeMap<String, LintSeverity>,
) -> Option<BuildDiagnostic> {
    let mut severity = match d.severity {
        SlangSeverity::Error => Severity::Error,
        SlangSeverity::Warning => Severity::Warning,
        SlangSeverity::Note => Severity::Note,
    };
    // Apply the per-rule override, if any. `allow` drops the diagnostic.
    if let Some(name) = &d.option_name {
        if let Some(over) = rules.get(name) {
            match over {
                LintSeverity::Error => severity = Severity::Error,
                LintSeverity::Warn => severity = Severity::Warning,
                LintSeverity::Allow => return None,
            }
        }
    }
    let (file, line, column) = match d.location {
        Some(loc) => (Some(loc.file), Some(loc.line), Some(loc.column)),
        None => (None, None, None),
    };
    Some(BuildDiagnostic {
        severity,
        code: d.option_name,
        file,
        line,
        column,
        message: d.message,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use slang_rs::{Diagnostic as SlangDiag, Location};

    fn diag(option: Option<&str>, sev: SlangSeverity) -> SlangDiag {
        SlangDiag {
            severity: sev,
            message: "msg".to_string(),
            option_name: option.map(String::from),
            location: Some(Location {
                file: "f.sv".into(),
                line: 1,
                column: 1,
            }),
            symbol_path: None,
        }
    }

    #[test]
    fn convert_no_rule_passes_severity_through() {
        let rules = BTreeMap::new();
        let d = convert(diag(Some("foo"), SlangSeverity::Warning), &rules).unwrap();
        assert_eq!(d.severity, Severity::Warning);
        assert_eq!(d.code.as_deref(), Some("foo"));
    }

    #[test]
    fn convert_error_override_promotes_warning() {
        let mut rules = BTreeMap::new();
        rules.insert("width-trunc".to_string(), LintSeverity::Error);
        let d = convert(diag(Some("width-trunc"), SlangSeverity::Warning), &rules).unwrap();
        assert_eq!(d.severity, Severity::Error);
    }

    #[test]
    fn convert_warn_override_demotes_error() {
        let mut rules = BTreeMap::new();
        rules.insert("foo".to_string(), LintSeverity::Warn);
        let d = convert(diag(Some("foo"), SlangSeverity::Error), &rules).unwrap();
        assert_eq!(d.severity, Severity::Warning);
    }

    #[test]
    fn convert_allow_drops_diagnostic() {
        let mut rules = BTreeMap::new();
        rules.insert("foo".to_string(), LintSeverity::Allow);
        assert!(convert(diag(Some("foo"), SlangSeverity::Warning), &rules).is_none());
    }

    #[test]
    fn convert_without_option_name_uses_native_severity() {
        let rules = BTreeMap::new();
        let d = convert(diag(None, SlangSeverity::Error), &rules).unwrap();
        assert_eq!(d.severity, Severity::Error);
        assert_eq!(d.code, None);
    }

    #[test]
    fn lint_config_round_trips_in_manifest() {
        let m: Manifest = r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [lint]
            width-trunc = "error"
            unused-net = "warn"
            implicit-net = "allow"
        "#
        .parse()
        .unwrap();
        assert_eq!(m.lint.rules.len(), 3);
        assert_eq!(m.lint.rules.get("width-trunc"), Some(&LintSeverity::Error));
        assert_eq!(m.lint.rules.get("implicit-net"), Some(&LintSeverity::Allow));
    }
}
