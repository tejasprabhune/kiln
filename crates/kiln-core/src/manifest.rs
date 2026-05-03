//! `Kiln.toml` manifest schema.
//!
//! See `docs/manifest-spec.md` for the full schema reference.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

/// Errors that can occur when loading or validating a manifest.
#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("failed to read manifest at {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("failed to parse manifest: {0}")]
    Parse(#[from] toml::de::Error),

    #[error(
        "package name `{0}` is not a valid SystemVerilog identifier (must start \
         with a letter or `_` and contain only letters, digits, or `_`)"
    )]
    InvalidPackageName(String),

    #[error("package version `{value}` is not valid semver: {source}")]
    InvalidVersion {
        value: String,
        #[source]
        source: semver::Error,
    },

    #[error("include directory `{0}` does not exist")]
    MissingIncludeDir(PathBuf),

    #[error(
        "unknown lint rule `{name}`{}",
        suggestion.as_deref().map(|s| format!("; did you mean `{s}`?")).unwrap_or_default()
    )]
    UnknownLintRule {
        name: String,
        suggestion: Option<String>,
    },
}

/// A `Kiln.toml` manifest.
///
/// `Eq` is intentionally not derived: `dependencies` holds `toml::Value`,
/// which contains floats and so only implements `PartialEq`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub package: Package,
    pub design: Design,
    #[serde(default)]
    pub dependencies: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub lint: LintConfig,
    #[serde(default)]
    pub tool: Tools,
    #[serde(default)]
    pub profile: BTreeMap<String, ProfileOverride>,
    #[serde(default)]
    pub wave: WaveConfig,
    #[serde(default)]
    pub test: TestConfig,
}

/// `[test]` table — options that apply when running testbenches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct TestConfig {
    /// Working directory for simulation binaries, relative to the project
    /// root. Defaults to the project root when not set.
    ///
    /// Set this when your testbenches use relative paths (e.g. `$readmemh`)
    /// that are relative to the testbench source file rather than to the
    /// project root.
    ///
    /// Example: `working_dir = "hardware/sim"` for a project whose
    /// testbenches live in `hardware/sim/` and reference files with
    /// `../../software/...` paths.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,

    /// Explicit test cases. Each entry names a testbench (by file stem) and
    /// provides runtime arguments (plusargs, etc.) for that invocation.
    /// Multiple entries can reuse the same testbench with different args —
    /// useful for parameterized testbenches like `c_tests_tb`.
    ///
    /// When this list is non-empty it is merged with the auto-discovered
    /// testbenches: cases listed here use the named testbench's compiled
    /// binary but are run as separate named tests with their own args.
    #[serde(default)]
    pub cases: Vec<TestCase>,
}

/// A single explicit test case that references a testbench by name and
/// supplies runtime arguments for that invocation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TestCase {
    /// Test name shown in output (e.g. `"fib"`).
    pub name: String,
    /// File stem of the testbench source (e.g. `"c_tests_tb"`).
    pub testbench: String,
    /// Extra arguments appended to the simulation binary invocation
    /// (e.g. `["+hex_file=../../software/c_tests/fib/fib.hex", "+test_name=fib"]`).
    #[serde(default)]
    pub args: Vec<String>,
}

/// `[package]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Package {
    pub name: String,
    pub version: String,
    #[serde(default)]
    pub authors: Vec<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub license: Option<String>,
}

/// `[design]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Design {
    pub top: String,
    #[serde(default = "Design::default_sources")]
    pub sources: Vec<String>,
    #[serde(default)]
    pub timescale: Option<String>,
    #[serde(default)]
    pub language: Option<SvLanguage>,
    #[serde(default)]
    pub include_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub defines: BTreeMap<String, String>,
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Glob patterns for testbench files. Overrides the default `tests/*.sv`
    /// discovery when testbenches live elsewhere.
    #[serde(default)]
    pub test_sources: Vec<String>,
}

impl Design {
    fn default_sources() -> Vec<String> {
        vec![
            "src/**/*.sv".into(),
            "src/**/*.svh".into(),
            "src/**/*.v".into(),
        ]
    }
}

/// SystemVerilog language standard.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SvLanguage {
    Sv2005,
    Sv2009,
    Sv2012,
    Sv2017,
    Sv2023,
}

/// `[lint]` table. Severity overrides keyed by canonical rule name,
/// with optional tool-specific sub-tables.
///
/// Cannot use `deny_unknown_fields` here because `#[serde(flatten)]`
/// is incompatible with it in serde's TOML backend.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LintConfig {
    /// Canonical cross-tool lint rules.
    #[serde(flatten)]
    pub rules: BTreeMap<String, LintSeverity>,
    /// Slang-specific lint options (under `[lint.slang]`).
    #[serde(default)]
    pub slang: BTreeMap<String, LintSeverity>,
    /// Verilator-specific lint options (under `[lint.verilator]`).
    #[serde(default)]
    pub verilator: BTreeMap<String, LintSeverity>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    Error,
    Warn,
    Off,
    Deny,
}

/// `[tool]` table with per-tool config structs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Tools {
    #[serde(default)]
    pub slang: Option<ToolSlang>,
    #[serde(default)]
    pub verilator: Option<ToolVerilator>,
    #[serde(default)]
    pub verible: Option<ToolVerible>,
}

/// `[tool.slang]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ToolSlang {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

/// `[tool.verilator]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ToolVerilator {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub threads: Option<u32>,
    #[serde(default)]
    pub trace: TraceFormat,
    #[serde(default)]
    pub coverage: bool,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

impl Default for ToolVerilator {
    fn default() -> Self {
        Self {
            path: None,
            threads: None,
            trace: TraceFormat::Off,
            coverage: false,
            extra_args: Vec::new(),
        }
    }
}

/// `[tool.verible]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct ToolVerible {
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

/// Trace output format for verilator.
///
/// Accepts `false` (off), `"vcd"`, or `"fst"` in TOML. Custom
/// Deserialize/Serialize handles the mixed-type encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TraceFormat {
    #[default]
    Off,
    Vcd,
    Fst,
}

impl<'de> Deserialize<'de> for TraceFormat {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = TraceFormat;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "false, \"vcd\", or \"fst\"")
            }
            fn visit_bool<E: serde::de::Error>(self, v: bool) -> Result<TraceFormat, E> {
                if v {
                    Err(E::custom("use \"vcd\" or \"fst\" instead of true"))
                } else {
                    Ok(TraceFormat::Off)
                }
            }
            fn visit_str<E: serde::de::Error>(self, v: &str) -> Result<TraceFormat, E> {
                match v {
                    "vcd" => Ok(TraceFormat::Vcd),
                    "fst" => Ok(TraceFormat::Fst),
                    other => Err(E::unknown_variant(other, &["vcd", "fst"])),
                }
            }
        }
        de.deserialize_any(Visitor)
    }
}

impl Serialize for TraceFormat {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        match self {
            TraceFormat::Off => ser.serialize_bool(false),
            TraceFormat::Vcd => ser.serialize_str("vcd"),
            TraceFormat::Fst => ser.serialize_str("fst"),
        }
    }
}

/// `[profile.<name>]` override table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ProfileOverride {
    #[serde(default)]
    pub design: Option<DesignOverride>,
    #[serde(default)]
    pub lint: Option<LintConfig>,
    #[serde(default)]
    pub tool: Option<ToolsOverride>,
}

/// Partial design fields for profile overrides.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct DesignOverride {
    #[serde(default)]
    pub top: Option<String>,
    #[serde(default)]
    pub timescale: Option<String>,
    #[serde(default)]
    pub language: Option<SvLanguage>,
    #[serde(default)]
    pub include_dirs: Option<Vec<PathBuf>>,
    #[serde(default)]
    pub defines: Option<BTreeMap<String, String>>,
    #[serde(default)]
    pub libraries: Option<Vec<String>>,
}

/// Per-tool overrides inside a profile.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(deny_unknown_fields)]
pub struct ToolsOverride {
    #[serde(default)]
    pub slang: Option<ToolSlang>,
    #[serde(default)]
    pub verilator: Option<ToolVerilator>,
    #[serde(default)]
    pub verible: Option<ToolVerible>,
}

/// `[wave]` table.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WaveConfig {
    /// Trace format. `fst` (default) or `vcd`.
    #[serde(default)]
    pub format: WaveFormat,
    /// If true, every `kiln test` enables `--trace` automatically.
    #[serde(default)]
    pub enabled_by_default: bool,
}

impl Default for WaveConfig {
    fn default() -> Self {
        Self {
            format: WaveFormat::Fst,
            enabled_by_default: false,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum WaveFormat {
    #[default]
    Fst,
    Vcd,
}

/// Toggles for `Manifest::validate`. `kiln new` disables filesystem checks
/// because the project doesn't exist yet at validation time.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidateOptions {
    /// If true, verify that every `include_dirs` entry exists on disk
    /// relative to `project_root`.
    pub check_include_dirs: bool,
}

impl FromStr for Manifest {
    type Err = ManifestError;

    /// Parse a manifest from a TOML string. Runs only validation that doesn't
    /// require filesystem context. Use [`Manifest::validate`] for the rest.
    fn from_str(s: &str) -> Result<Self, ManifestError> {
        let manifest: Self = toml::from_str(s)?;
        manifest.validate_static()?;
        Ok(manifest)
    }
}

impl Manifest {
    /// Load a manifest from a path, validating it against the on-disk project
    /// rooted at the manifest's parent directory.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let text = std::fs::read_to_string(path).map_err(|source| ManifestError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let manifest: Self = toml::from_str(&text)?;
        manifest.validate_static()?;
        let project_root = path.parent().unwrap_or_else(|| Path::new("."));
        manifest.validate(
            project_root,
            ValidateOptions {
                check_include_dirs: true,
            },
        )?;
        Ok(manifest)
    }

    /// Validation that needs no filesystem context.
    pub fn validate_static(&self) -> Result<(), ManifestError> {
        if !is_valid_sv_identifier(&self.package.name) {
            return Err(ManifestError::InvalidPackageName(self.package.name.clone()));
        }
        if let Err(source) = semver::Version::parse(&self.package.version) {
            return Err(ManifestError::InvalidVersion {
                value: self.package.version.clone(),
                source,
            });
        }
        Ok(())
    }

    /// Validation that depends on `project_root` (e.g., include-dir existence).
    pub fn validate(
        &self,
        project_root: &Path,
        opts: ValidateOptions,
    ) -> Result<(), ManifestError> {
        if opts.check_include_dirs {
            for dir in &self.design.include_dirs {
                let full = project_root.join(dir);
                if !full.is_dir() {
                    return Err(ManifestError::MissingIncludeDir(dir.clone()));
                }
            }
        }
        Ok(())
    }

    /// Merged slang lint rules for a given profile. Canonical rules are
    /// translated to slang option names; profile overlay wins on conflict.
    pub fn resolved_lint_for_slang(&self, profile: &str) -> BTreeMap<String, LintSeverity> {
        let mut out: BTreeMap<String, LintSeverity> = self.lint.rules.clone();
        for (k, v) in &self.lint.slang {
            out.insert(k.clone(), *v);
        }
        if let Some(overlay) = self.profile.get(profile) {
            if let Some(lint) = &overlay.lint {
                for (k, v) in &lint.rules {
                    out.insert(k.clone(), *v);
                }
                for (k, v) in &lint.slang {
                    out.insert(k.clone(), *v);
                }
            }
        }
        out
    }

    /// Merged verilator lint rules for a given profile.
    pub fn resolved_lint_for_verilator(&self, profile: &str) -> BTreeMap<String, LintSeverity> {
        let mut out: BTreeMap<String, LintSeverity> = self.lint.rules.clone();
        for (k, v) in &self.lint.verilator {
            out.insert(k.clone(), *v);
        }
        if let Some(overlay) = self.profile.get(profile) {
            if let Some(lint) = &overlay.lint {
                for (k, v) in &lint.rules {
                    out.insert(k.clone(), *v);
                }
                for (k, v) in &lint.verilator {
                    out.insert(k.clone(), *v);
                }
            }
        }
        out
    }
}

/// Returns true if `s` is a valid SystemVerilog simple identifier.
fn is_valid_sv_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == '_') {
        return false;
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<Manifest, ManifestError> {
        s.parse::<Manifest>()
    }

    #[test]
    fn valid_minimal_manifest() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "demo_top"
            "#,
        )
        .unwrap();
        insta::assert_yaml_snapshot!("valid_minimal", m);
    }

    #[test]
    fn valid_full_manifest() {
        let m = parse(
            r#"
            [package]
            name = "widget_v2"
            version = "1.2.3"
            authors = ["Jane <jane@example.com>"]
            description = "A widget"
            license = "MIT OR Apache-2.0"

            [design]
            top = "widget_top"
            sources = ["rtl/**/*.sv"]
            include_dirs = ["rtl/include"]
            defines = { FOO = "1", BAR = "" }

            [dependencies]
            "#,
        )
        .unwrap();
        insta::assert_yaml_snapshot!("valid_full", m);
    }

    #[test]
    fn valid_underscore_name() {
        let m = parse(
            r#"
            [package]
            name = "_internal"
            version = "0.0.1"

            [design]
            top = "t"
            "#,
        )
        .unwrap();
        insta::assert_yaml_snapshot!("valid_underscore_name", m);
    }

    #[test]
    fn invalid_bad_semver() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "not-semver"

            [design]
            top = "t"
            "#,
        )
        .unwrap_err();
        insta::assert_snapshot!("invalid_bad_semver", err.to_string());
    }

    #[test]
    fn invalid_bad_identifier() {
        let err = parse(
            r#"
            [package]
            name = "1bad"
            version = "0.1.0"

            [design]
            top = "t"
            "#,
        )
        .unwrap_err();
        insta::assert_snapshot!("invalid_bad_identifier", err.to_string());
    }

    #[test]
    fn invalid_unknown_key() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            color = "blue"

            [design]
            top = "t"
            "#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ManifestError::Parse(_)), "got: {msg}");
        assert!(
            msg.contains("color"),
            "expected mention of unknown key: {msg}"
        );
        assert!(
            msg.contains("unknown field") || msg.contains("not allowed"),
            "expected unknown-field mention: {msg}"
        );
    }

    #[test]
    fn invalid_missing_design() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            "#,
        )
        .unwrap_err();
        let msg = err.to_string();
        assert!(matches!(err, ManifestError::Parse(_)), "got: {msg}");
        assert!(
            msg.contains("design") || msg.contains("missing field"),
            "expected mention of missing [design]: {msg}"
        );
    }

    #[test]
    fn defaults_sources_when_omitted() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"
            "#,
        )
        .unwrap();
        assert_eq!(
            m.design.sources,
            vec![
                "src/**/*.sv".to_string(),
                "src/**/*.svh".to_string(),
                "src/**/*.v".to_string(),
            ]
        );
    }

    #[test]
    fn validate_rejects_missing_include_dir() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("Kiln.toml"),
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"
            include_dirs = ["does/not/exist"]
            "#,
        )
        .unwrap();
        let err = Manifest::load(&tmp.path().join("Kiln.toml")).unwrap_err();
        assert!(matches!(err, ManifestError::MissingIncludeDir(_)));
        insta::assert_snapshot!(
            "invalid_missing_include_dir",
            ManifestError::MissingIncludeDir(std::path::PathBuf::from("does/not/exist"))
                .to_string()
        );
    }

    #[test]
    fn validate_skip_include_dirs_for_kiln_new() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"
            include_dirs = ["does/not/exist"]
            "#,
        )
        .unwrap();
        m.validate(
            Path::new("."),
            ValidateOptions {
                check_include_dirs: false,
            },
        )
        .unwrap();
    }

    #[test]
    fn identifier_validator() {
        for ok in ["a", "_x", "abc", "a_1", "_", "Foo123"] {
            assert!(is_valid_sv_identifier(ok), "{ok} should be valid");
        }
        for bad in ["", "1abc", "a-b", "a.b", "a b", "ä"] {
            assert!(!is_valid_sv_identifier(bad), "{bad} should be invalid");
        }
    }

    #[test]
    fn design_timescale_and_language() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"
            timescale = "1ns/1ps"
            language = "sv2017"
            "#,
        )
        .unwrap();
        assert_eq!(m.design.timescale.as_deref(), Some("1ns/1ps"));
        assert_eq!(m.design.language, Some(SvLanguage::Sv2017));
    }

    #[test]
    fn tool_slang_extra_args() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.slang]
            extra_args = ["--allow-hierarchical-const"]
            "#,
        )
        .unwrap();
        let slang = m.tool.slang.as_ref().unwrap();
        assert_eq!(slang.extra_args, vec!["--allow-hierarchical-const"]);
    }

    #[test]
    fn tool_verilator_options() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.verilator]
            threads = 4
            trace = "fst"
            coverage = true
            extra_args = ["--x-assign", "0"]
            "#,
        )
        .unwrap();
        let v = m.tool.verilator.as_ref().unwrap();
        assert_eq!(v.threads, Some(4));
        assert_eq!(v.trace, TraceFormat::Fst);
        assert!(v.coverage);
        assert_eq!(v.extra_args, vec!["--x-assign", "0"]);
    }

    #[test]
    fn trace_format_from_false() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.verilator]
            trace = false
            "#,
        )
        .unwrap();
        assert_eq!(m.tool.verilator.as_ref().unwrap().trace, TraceFormat::Off);
    }

    #[test]
    fn trace_format_from_vcd() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.verilator]
            trace = "vcd"
            "#,
        )
        .unwrap();
        assert_eq!(m.tool.verilator.as_ref().unwrap().trace, TraceFormat::Vcd);
    }

    #[test]
    fn lint_slang_and_verilator_subtables() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [lint]
            width-trunc = "error"

            [lint.slang]
            relax-enum-conversions = "off"

            [lint.verilator]
            GENUNNAMED = "warn"
            "#,
        )
        .unwrap();
        assert_eq!(m.lint.rules.get("width-trunc"), Some(&LintSeverity::Error));
        assert_eq!(
            m.lint.slang.get("relax-enum-conversions"),
            Some(&LintSeverity::Off)
        );
        assert_eq!(
            m.lint.verilator.get("GENUNNAMED"),
            Some(&LintSeverity::Warn)
        );
    }

    #[test]
    fn profile_tool_verilator_override() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [profile.test.tool.verilator]
            trace = "fst"
            coverage = true
            "#,
        )
        .unwrap();
        let overlay = m.profile.get("test").unwrap();
        let vt = overlay.tool.as_ref().unwrap().verilator.as_ref().unwrap();
        assert_eq!(vt.trace, TraceFormat::Fst);
        assert!(vt.coverage);
    }

    #[test]
    fn deny_unknown_fields_package() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            unknown_key = "oops"

            [design]
            top = "t"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn deny_unknown_fields_design() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"
            slang_args = []
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn deny_unknown_fields_tool_slang() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.slang]
            weird = true
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn deny_unknown_fields_tool_verilator() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.verilator]
            bad_field = 99
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn round_trip_parse_serialize_parse() {
        let src = r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"
            timescale = "1ns/1ps"
            language = "sv2017"

            [tool.verilator]
            threads = 2
            trace = "fst"
            coverage = false

            [lint]
            width-trunc = "error"

            [lint.slang]
            relax-enum-conversions = "off"
        "#;
        let m1: Manifest = src.parse().unwrap();
        let serialized = toml::to_string(&m1).unwrap();
        let m2: Manifest = serialized.parse().unwrap();
        assert_eq!(m1, m2);
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
            implicit-net = "off"
        "#
        .parse()
        .unwrap();
        assert_eq!(m.lint.rules.len(), 3);
        assert_eq!(m.lint.rules.get("width-trunc"), Some(&LintSeverity::Error));
        assert_eq!(m.lint.rules.get("implicit-net"), Some(&LintSeverity::Off));
    }
}
