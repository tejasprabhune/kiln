//! `Kiln.toml` manifest schema.
//!
//! See `docs/manifest-spec.md` for the full schema reference.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};
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
    pub wave: WaveConfig,
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

/// `[lint]` table. Severity overrides per slang diagnostic ID.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct LintConfig {
    /// Map of slang diagnostic ID → severity override.
    /// Keys are slang's `optionName` strings (e.g. `width-trunc`).
    /// `error | warn | allow`. All entries in `[lint]` land here via
    /// `#[serde(flatten)]`; that's why we don't use
    /// `deny_unknown_fields` on this struct (we *want* every key).
    #[serde(flatten)]
    pub rules: BTreeMap<String, LintSeverity>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LintSeverity {
    Error,
    Warn,
    Allow,
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
    pub include_dirs: Vec<PathBuf>,
    #[serde(default)]
    pub defines: BTreeMap<String, String>,
    /// Raw arguments forwarded verbatim to slang. Use for flags kiln does
    /// not expose directly (e.g. `--timescale 1ns/1ps`).
    #[serde(default)]
    pub slang_args: Vec<String>,
    /// Raw arguments forwarded verbatim to verilator. Use for flags kiln
    /// does not expose directly (e.g. `--timing`, `-Wno-TIMESCALEMOD`).
    #[serde(default)]
    pub verilator_args: Vec<String>,
    /// Glob patterns for testbench files. Overrides the default `tests/*.sv`
    /// discovery when your testbenches live elsewhere (e.g. `sim/*_tb.sv`).
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
    fn validate_static(&self) -> Result<(), ManifestError> {
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
        // Additional snapshot: the formatted message of our own error type.
        // Stable, so safe to snapshot (unlike toml::de::Error formatting).
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
}
