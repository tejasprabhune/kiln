//! End-to-end tests that invoke the real `slang` binary.
//!
//! Gated behind the `e2e` feature (and `cfg(feature = "e2e")`) so that
//! `cargo test -p slang-rs` works without slang installed; CI runs with
//! `--features e2e` once it has installed slang.

#![cfg(feature = "e2e")]

use std::path::PathBuf;

use slang_rs::{CompileRequest, Severity, Slang, SvStandard};

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn slang() -> Slang {
    Slang::new().expect("slang must be on PATH for e2e tests; install slang first")
}

#[test]
fn version_is_at_least_min() {
    let s = slang();
    let v = s.version();
    assert!(v.major >= 10, "slang version {v} should be >= 10");
}

#[test]
fn clean_module_reports_no_diagnostics() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("valid_module.sv"))
        .top("counter")
        .build();
    let result = s.compile(&req).unwrap();
    assert!(result.is_clean(), "diagnostics: {:?}", result.diagnostics);
    assert_eq!(result.exit_code, Some(0));
}

#[test]
fn syntax_error_pinpoints_missing_semicolon_line() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("syntax_error.sv"))
        .build();
    let result = s.compile(&req).unwrap();
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(!errors.is_empty(), "expected error diagnostics");
    let first = errors[0];
    let loc = first
        .location
        .as_ref()
        .expect("first error should carry a location");
    assert_eq!(loc.line, 1, "missing semicolon is on line 1");
    assert!(
        first.message.contains(';'),
        "expected message about a missing `;`, got: {}",
        first.message
    );
}

#[test]
fn width_warning_emitted_when_enabled() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("width_mismatch.sv"))
        .extra_arg("-Wwidth-trunc")
        .build();
    let result = s.compile(&req).unwrap();
    let warnings: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .collect();
    assert_eq!(
        warnings.len(),
        1,
        "expected exactly one width-trunc warning"
    );
    assert_eq!(
        warnings[0].option_name.as_deref(),
        Some("width-trunc"),
        "warning should be tagged with the -W option name"
    );
}

#[test]
fn requested_ast_is_present_for_clean_compile() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("valid_module.sv"))
        .top("counter")
        .want_ast(true)
        .build();
    let result = s.compile(&req).unwrap();
    let ast = result.ast.expect("AST should be returned when requested");
    assert_eq!(ast.design.kind, "Root");
    let tops: Vec<_> = ast.top_instances().collect();
    assert!(tops.iter().any(|t| t.name == "counter"));
}

#[test]
fn defines_are_passed_through() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("with_defines.sv"))
        .define("FOO", "42")
        .build();
    let result = s.compile(&req).unwrap();
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "with FOO defined, no errors expected; got {errors:?}"
    );
}

#[test]
fn missing_define_produces_error() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("with_defines.sv"))
        .build();
    let result = s.compile(&req).unwrap();
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        !errors.is_empty(),
        "without FOO defined, slang should error on the `FOO macro reference"
    );
}

#[test]
fn include_dir_is_searched() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("with_includes/uses_include.sv"))
        .include_dir(fixture_dir().join("with_includes/include"))
        .build();
    let result = s.compile(&req).unwrap();
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(
        errors.is_empty(),
        "include should resolve `header.svh`; got errors: {errors:?}"
    );
}

#[test]
fn package_consumer_compiles_when_both_files_passed() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("package_pkg.sv"))
        .source(fixture_dir().join("consumer.sv"))
        .top("consumer")
        .build();
    let result = s.compile(&req).unwrap();
    let errors: Vec<_> = result
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "expected clean compile, got {errors:?}");
}

#[test]
fn standard_flag_passes_through() {
    let s = slang();
    let req = CompileRequest::builder()
        .source(fixture_dir().join("valid_module.sv"))
        .top("counter")
        .std(SvStandard::Sv2017)
        .build();
    let result = s.compile(&req).unwrap();
    assert!(result.is_clean(), "{:?}", result.diagnostics);
}
