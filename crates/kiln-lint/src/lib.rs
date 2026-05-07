// `LintError` carries paths and captured stderr from the slang invocation.
#![allow(clippy::result_large_err)]
//! Linting for `kiln`. Drives `slang-rs` for fast (sub-second) elaboration
//! checks; reuses [`kiln_build::BuildDiagnostic`] so `kiln check` and `kiln
//! build` render identically.

use std::collections::BTreeMap;

use thiserror::Error;

use kiln_build::{BuildDiagnostic, Severity, SourceSet};
use kiln_core::{LintSeverity, ResolvedConfig};
use slang_rs::{CompileRequest, Severity as SlangSeverity, Slang, SlangError};

#[derive(Debug, Error)]
pub enum LintError {
    #[error(transparent)]
    Slang(#[from] SlangError),
}

/// Run a Slang elaboration over the project's source set and apply the
/// `[lint]` severity overrides from the resolved config.
pub fn check(
    slang: &Slang,
    resolved: &ResolvedConfig,
    source_set: &SourceSet,
) -> Result<Vec<BuildDiagnostic>, LintError> {
    let req = build_request(resolved, source_set);
    let result = slang.compile(&req)?;
    let diagnostics = result
        .diagnostics
        .into_iter()
        .filter_map(|d| convert(d, &resolved.lint.rules))
        .collect();
    Ok(diagnostics)
}

/// Build the slang CompileRequest for a resolved config. Extracted for testing.
pub(crate) fn build_request(resolved: &ResolvedConfig, source_set: &SourceSet) -> CompileRequest {
    use kiln_core::SvLanguage;

    let mut req = CompileRequest::builder().top(&resolved.design.top);
    // Slang accepts multiple `--top` flags. Auxiliary tops (e.g. Xilinx
    // `glbl`) are passed via extra_arg to keep the slang-rs builder API
    // single-top.
    for aux in &resolved.design.aux_tops {
        req = req.extra_arg("--top".to_string());
        req = req.extra_arg(aux.clone());
    }
    for s in source_set.files() {
        req = req.source(s.clone());
    }
    for d in &resolved.design.include_dirs {
        req = req.include_dir(source_set.project_root.join(d));
    }
    for (k, v) in &resolved.design.defines {
        req = req.define(k.clone(), v.clone());
    }
    if let Some(ts) = &resolved.design.timescale {
        req = req.extra_arg("--timescale".to_string());
        req = req.extra_arg(ts.clone());
    }
    if let Some(lang) = resolved.design.language {
        let flag = match lang {
            SvLanguage::Sv2005 => "1364-2005",
            SvLanguage::Sv2009 => "1800-2009",
            SvLanguage::Sv2012 => "1800-2012",
            SvLanguage::Sv2017 => "1800-2017",
            SvLanguage::Sv2023 => "1800-2023",
        };
        req = req.extra_arg("--std".to_string());
        req = req.extra_arg(flag.to_string());
    }
    for lib in &resolved.design.libraries {
        req = req.extra_arg("-y".to_string());
        req = req.extra_arg(lib.clone());
    }
    // Enable every `-W` knob the user asked us to surface.
    for (id, sev) in &resolved.lint.rules {
        if matches!(sev, LintSeverity::Error | LintSeverity::Warn) {
            req = req.extra_arg(format!("-W{id}"));
        }
    }
    for arg in &resolved.tool_slang.extra_args {
        req = req.extra_arg(arg.clone());
    }
    // We do *not* pass `--parse-only` here. Slang skips writing the
    // `--diag-json` file when parse-only is set, and we want full
    // elaboration anyway so semantic warnings fire.
    req.build()
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
                LintSeverity::Off | LintSeverity::Deny => return None,
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
    fn convert_off_drops_diagnostic() {
        let mut rules = BTreeMap::new();
        rules.insert("foo".to_string(), LintSeverity::Off);
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
        use kiln_core::Manifest;
        let m: Manifest = r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [lint]
            width-trunc = "error"
            unused-net = "warn"
            implicit-net = "off"
        "#
        .parse()
        .unwrap();
        assert_eq!(m.lint.rules.len(), 3);
        assert_eq!(m.lint.rules.get("width-trunc"), Some(&LintSeverity::Error));
        assert_eq!(m.lint.rules.get("implicit-net"), Some(&LintSeverity::Off));
    }

    fn resolved(manifest_str: &str) -> (kiln_core::ResolvedConfig, kiln_build::SourceSet) {
        let m: kiln_core::Manifest = manifest_str.parse().unwrap();
        let resolved = kiln_core::ResolvedConfig::resolve(&m, "dev");
        let ss = kiln_build::SourceSet {
            project_root: std::path::PathBuf::from("/p"),
            files: vec![],
        };
        (resolved, ss)
    }

    #[test]
    fn timescale_in_design_reaches_slang_args() {
        let (r, ss) = resolved(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            timescale = "1ns/1ps"
            "#,
        );
        let req = build_request(&r, &ss);
        let args = req.extra_args();
        let pos = args.iter().position(|a| a == "--timescale");
        assert!(
            pos.is_some(),
            "--timescale not found in slang args: {args:?}"
        );
        assert_eq!(args[pos.unwrap() + 1], "1ns/1ps");
    }

    #[test]
    fn language_in_design_reaches_slang_std_arg() {
        let (r, ss) = resolved(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            language = "sv2017"
            "#,
        );
        let req = build_request(&r, &ss);
        let args = req.extra_args();
        let pos = args.iter().position(|a| a == "--std");
        assert!(pos.is_some(), "--std not found in slang args: {args:?}");
        assert_eq!(args[pos.unwrap() + 1], "1800-2017");
    }

    #[test]
    fn libraries_in_design_reach_slang_y_flag() {
        let (r, ss) = resolved(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            libraries = ["vendor/lib"]
            "#,
        );
        let req = build_request(&r, &ss);
        let args = req.extra_args();
        let pos = args.iter().position(|a| a == "-y");
        assert!(pos.is_some(), "-y not found in slang args: {args:?}");
        assert_eq!(args[pos.unwrap() + 1], "vendor/lib");
    }

    #[test]
    fn aux_tops_become_additional_top_flags() {
        let (r, ss) = resolved(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "z1top"
            aux_tops = ["glbl", "BUFG_helper"]
            "#,
        );
        let req = build_request(&r, &ss);
        let args = req.extra_args();
        // Two `--top` extra args, with the aux names following.
        let positions: Vec<usize> = args
            .iter()
            .enumerate()
            .filter(|(_, a)| a.as_str() == "--top")
            .map(|(i, _)| i)
            .collect();
        assert_eq!(positions.len(), 2, "expected 2 extra `--top`: {args:?}");
        assert_eq!(args[positions[0] + 1], "glbl");
        assert_eq!(args[positions[1] + 1], "BUFG_helper");
    }

    #[test]
    fn tool_slang_extra_args_appended_last() {
        let (r, ss) = resolved(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            timescale = "1ns/1ps"
            [tool.slang]
            extra_args = ["--allow-hierarchical-const"]
            "#,
        );
        let req = build_request(&r, &ss);
        let args = req.extra_args();
        // extra_args must come after timescale so users can override
        let ts_pos = args.iter().position(|a| a == "--timescale").unwrap();
        let extra_pos = args
            .iter()
            .position(|a| a == "--allow-hierarchical-const")
            .unwrap();
        assert!(
            extra_pos > ts_pos,
            "extra_args should come after design args"
        );
    }
}
