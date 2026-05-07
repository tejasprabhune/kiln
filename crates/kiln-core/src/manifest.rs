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

    #[error("unknown feature `{0}` (not declared in `[features]`)")]
    UnknownFeature(String),

    #[error(
        "feature name `{0}` is not a valid SystemVerilog identifier (must \
         start with a letter or `_` and contain only letters, digits, or `_`)"
    )]
    InvalidFeatureName(String),

    #[error(
        "firmware name `{0}` is not a valid SystemVerilog identifier (must \
         start with a letter or `_` and contain only letters, digits, or `_`)"
    )]
    InvalidFirmwareName(String),

    #[error("duplicate firmware name `{0}`")]
    DuplicateFirmwareName(String),
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
    #[serde(default)]
    pub features: FeaturesConfig,
    /// Vendor libraries (Xilinx unisims, Altera megafunctions, custom
    /// stub sets). Keyed by vendor name; each block contributes
    /// sim-model sources, stub sources, and verilator blackbox names.
    #[serde(default)]
    pub vendor: BTreeMap<String, Vendor>,
    /// Embedded firmware artifacts produced by an external build
    /// system and consumed by RTL tests. `kiln test` runs each
    /// declared firmware build (deduped) once before the test pass.
    #[serde(default)]
    pub firmware: Vec<Firmware>,
    /// Project-level shell escapes for kiln subcommand lifecycle
    /// phases. See [`Hooks`] for the supported phases.
    #[serde(default)]
    pub hooks: Hooks,
}

/// One `[vendor.<name>]` block.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Vendor {
    /// Glob patterns for vendor sim-model sources, appended to the
    /// resolved source set.
    #[serde(default)]
    pub sim_models: Vec<String>,
    /// Glob patterns for synth-only stubs, appended to the resolved
    /// source set today; future synthesis backends will keep these
    /// out of simulation.
    #[serde(default)]
    pub stubs: Vec<String>,
    /// Module names to pass as `--bbox <name>` to verilator so its
    /// body is treated as a black box during compilation.
    #[serde(default)]
    pub blackbox_modules: Vec<String>,
}

/// One `[[firmware]]` entry.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Firmware {
    /// Free-form identifier; must be a valid SystemVerilog identifier.
    pub name: String,
    /// Directory containing the firmware build, relative to the
    /// project root.
    pub path: PathBuf,
    /// Shell command run inside `path` to build the firmware.
    pub build: String,
    /// Glob (relative to `path`) describing produced artifacts, used
    /// for documentation and future `kiln firmware list <name>`.
    #[serde(default)]
    pub artifacts: Option<String>,
}

/// `[hooks]` table — single-shell-line escapes per lifecycle phase.
///
/// Empty strings are treated as unset.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Hooks {
    /// Runs before slang elaboration in `kiln check`.
    #[serde(default)]
    pub pre_check: Option<String>,
    /// Runs before verilator in `kiln build` (and the build phase of
    /// `kiln run` / `kiln test`).
    #[serde(default)]
    pub pre_build: Option<String>,
    /// Runs before any testbench is started by `kiln test`.
    #[serde(default)]
    pub pre_test: Option<String>,
    /// Runs after `kiln test` finishes, regardless of pass/fail.
    /// Failures are logged but do not change the test outcome.
    #[serde(default)]
    pub post_test: Option<String>,
}

impl Hooks {
    /// Returns the hook command for a phase, treating empty strings as
    /// "no hook configured."
    pub fn for_phase(&self, phase: HookPhase) -> Option<&str> {
        let raw = match phase {
            HookPhase::PreCheck => self.pre_check.as_deref(),
            HookPhase::PreBuild => self.pre_build.as_deref(),
            HookPhase::PreTest => self.pre_test.as_deref(),
            HookPhase::PostTest => self.post_test.as_deref(),
        };
        raw.and_then(|s| {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t)
            }
        })
    }
}

/// Lifecycle phases recognised by [`Hooks::for_phase`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookPhase {
    PreCheck,
    PreBuild,
    PreTest,
    PostTest,
}

/// `[features]` table — cargo-style conditional compilation toggles.
///
/// Each named feature contributes additional `+define+`s and source globs
/// when active. The `default` key lists feature names enabled when no
/// explicit `--features` / `--no-default-features` selection is made.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct FeaturesConfig {
    /// Features enabled by default. Names must appear in `features`.
    #[serde(default)]
    pub default: Vec<String>,
    /// Defined features keyed by name. The TOML form is
    /// `[features.<name>]` with `defines` and `sources` sub-keys.
    #[serde(flatten)]
    pub features: BTreeMap<String, Feature>,
}

/// A single named feature.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(deny_unknown_fields)]
pub struct Feature {
    /// `+define+NAME` or `+define+NAME=VALUE` entries to add when this
    /// feature is active. A bare identifier becomes `+define+NAME`;
    /// `NAME=VALUE` form is also accepted.
    #[serde(default)]
    pub defines: Vec<String>,
    /// Additional source glob patterns to include when this feature is
    /// active.
    #[serde(default)]
    pub sources: Vec<String>,
}

/// Selection of active features for one build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureSelection {
    pub active: Vec<String>,
}

impl FeatureSelection {
    /// Build a selection from CLI flags. Cargo-shaped semantics:
    ///
    /// - `--all-features` → every defined feature is active.
    /// - `--no-default-features` → starts empty; only `--features`
    ///   entries are added.
    /// - `--features a,b` → adds `a` and `b` on top of the default set
    ///   (or on top of empty if `--no-default-features`).
    /// - When no flags are passed, the manifest's `default` list is
    ///   used.
    ///
    /// Returns an error if any requested feature is not defined.
    pub fn resolve(
        cfg: &FeaturesConfig,
        explicit: &[String],
        all: bool,
        no_default: bool,
    ) -> Result<Self, ManifestError> {
        let mut active: Vec<String> = if all {
            cfg.features.keys().cloned().collect()
        } else if no_default {
            Vec::new()
        } else {
            cfg.default.clone()
        };
        for name in explicit {
            for n in name.split([',', ' ']).filter(|s| !s.is_empty()) {
                if !cfg.features.contains_key(n) {
                    return Err(ManifestError::UnknownFeature(n.to_string()));
                }
                if !active.iter().any(|a| a == n) {
                    active.push(n.to_string());
                }
            }
        }
        for name in &active {
            if !cfg.features.contains_key(name) {
                return Err(ManifestError::UnknownFeature(name.clone()));
            }
        }
        Ok(Self { active })
    }
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

    /// Default pass/fail detection rule for tests that don't override it.
    /// Defaults to [`Detect::ExitCode`] — exit 0 = pass.
    #[serde(default)]
    pub detect: Option<Detect>,

    /// Default per-test wallclock timeout (e.g. `"30s"`, `"2m"`). Tests
    /// exceeding this are killed and reported as `TIMEOUT`. Unset = no
    /// timeout.
    #[serde(default)]
    pub timeout: Option<DurationSpec>,

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

    /// Glob-driven parameterized test cases. Each `[[test.matrix]]` entry
    /// expands a glob into one [`TestCase`]-equivalent per matched file,
    /// with `{stem}`, `{path}`, `{abs_path}`, `{name}`, `{parent}` template
    /// substitution available in `args`, `plusargs`, `prebuild`, and the
    /// generated test name.
    #[serde(default)]
    pub matrix: Vec<TestMatrix>,
}

/// Pass/fail detection rule for a test. The default
/// [`Detect::ExitCode`] checks the simulator's exit status. The
/// [`Detect::Patterns`] variant lets testbenches that always
/// `$finish()` cleanly (regardless of pass/fail) signal outcome via
/// stdout markers.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub enum Detect {
    /// Exit code 0 = pass, anything else = fail. The default.
    #[default]
    ExitCode,
    /// Pattern match on stdout. A test passes if every
    /// `stdout_contains` substring is present and no
    /// `stdout_must_not_contain` substring appears. Exit code is
    /// ignored when this variant is used.
    Patterns {
        #[serde(default)]
        stdout_contains: Vec<String>,
        #[serde(default)]
        stdout_must_not_contain: Vec<String>,
    },
}

/// A duration that round-trips through TOML as a string like `"30s"`,
/// `"500ms"`, or `"2m"`. Parsed once at manifest load time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DurationSpec(pub std::time::Duration);

impl DurationSpec {
    pub fn as_duration(&self) -> std::time::Duration {
        self.0
    }
}

impl<'de> Deserialize<'de> for DurationSpec {
    fn deserialize<D: Deserializer<'de>>(de: D) -> Result<Self, D::Error> {
        let s = String::deserialize(de)?;
        parse_duration(&s)
            .map(DurationSpec)
            .map_err(serde::de::Error::custom)
    }
}

impl Serialize for DurationSpec {
    fn serialize<S: Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        // Prefer the most natural unit for round-trip readability.
        let d = self.0;
        let s = if !d.as_millis().is_multiple_of(1000) {
            format!("{}ms", d.as_millis())
        } else if !d.as_secs().is_multiple_of(60) || d.as_secs() == 0 {
            format!("{}s", d.as_secs())
        } else if !d.as_secs().is_multiple_of(3600) {
            format!("{}m", d.as_secs() / 60)
        } else {
            format!("{}h", d.as_secs() / 3600)
        };
        ser.serialize_str(&s)
    }
}

fn parse_duration(s: &str) -> Result<std::time::Duration, String> {
    let s = s.trim();
    let (num_part, unit) = if let Some(stripped) = s.strip_suffix("ms") {
        (stripped, "ms")
    } else if let Some(stripped) = s.strip_suffix('s') {
        (stripped, "s")
    } else if let Some(stripped) = s.strip_suffix('m') {
        (stripped, "m")
    } else if let Some(stripped) = s.strip_suffix('h') {
        (stripped, "h")
    } else {
        return Err(format!("duration `{s}` must end in `ms`, `s`, `m`, or `h`"));
    };
    let n: u64 = num_part
        .trim()
        .parse()
        .map_err(|e| format!("duration `{s}` has non-integer count: {e}"))?;
    let d = match unit {
        "ms" => std::time::Duration::from_millis(n),
        "s" => std::time::Duration::from_secs(n),
        "m" => std::time::Duration::from_secs(n * 60),
        "h" => std::time::Duration::from_secs(n * 3600),
        _ => unreachable!(),
    };
    Ok(d)
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
    /// Shell command to run before the simulator binary. Useful for
    /// regenerating `$readmemh` hex files from C/asm sources. Runs
    /// once per unique command per `kiln test` invocation (deduped by
    /// command string), executed at the project root.
    #[serde(default)]
    pub prebuild: Option<String>,
    /// Per-case pass/fail detection rule, overrides `[test] detect`.
    #[serde(default)]
    pub detect: Option<Detect>,
    /// Per-case wallclock timeout, overrides `[test] timeout`.
    #[serde(default)]
    pub timeout: Option<DurationSpec>,
    /// Tags for selection via `--tag`.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Per-case working directory override (relative to project root).
    /// Falls back to `[test] working_dir`, then to the project root.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
}

/// A glob-driven parameterized test family. Each matched input becomes
/// a synthetic [`TestCase`] with template substitution applied to the
/// generated name and to every arg.
///
/// Supported substitutions in `args`, `prebuild`, and (the suffix of)
/// `name_prefix`:
///
/// - `{stem}` — matched file's stem (e.g. `"add"` for `add.hex`)
/// - `{name}` — matched file's basename (e.g. `"add.hex"`)
/// - `{path}` — matched file's path relative to the project root
/// - `{abs_path}` — matched file's absolute path
/// - `{parent}` — matched file's parent directory, relative to project root
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TestMatrix {
    /// File stem of the testbench source.
    pub testbench: String,
    /// Glob pattern (relative to project root) selecting input files.
    pub inputs: String,
    /// Prefix prepended to `{stem}` to form the generated test name.
    /// Defaults to `""` (i.e., name == stem).
    #[serde(default)]
    pub name_prefix: String,
    /// Argument templates appended to the simulation invocation. Each
    /// string is run through template substitution before passing to
    /// the binary.
    #[serde(default)]
    pub args: Vec<String>,
    /// Shell command template run once per matrix row before the
    /// corresponding test, deduped by final substituted string.
    #[serde(default)]
    pub prebuild: Option<String>,
    /// Detection rule applied to every generated case.
    #[serde(default)]
    pub detect: Option<Detect>,
    /// Wallclock timeout applied to every generated case.
    #[serde(default)]
    pub timeout: Option<DurationSpec>,
    /// Tags applied to every generated case.
    #[serde(default)]
    pub tags: Vec<String>,
    /// Working directory applied to every generated case.
    #[serde(default)]
    pub working_dir: Option<PathBuf>,
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
    /// Auxiliary top modules elaborated alongside `top`. Slang accepts
    /// multiple tops via repeated `--top` flags; this is how a project
    /// keeps non-instantiated helpers (e.g. Xilinx `glbl`) in scope for
    /// `kiln check` / `kiln doc`. Verilator only supports one
    /// `--top-module`, so this list is informational for Verilator.
    #[serde(default)]
    pub aux_tops: Vec<String>,
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
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
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
    /// Enables `--timing` for designs that use delays / event control.
    #[serde(default)]
    pub timing: bool,
    /// X-assignment policy. Maps to `--x-assign <value>`.
    #[serde(default)]
    pub x_assign: Option<XAssign>,
    /// Black-box unsupported constructs (e.g. vendor primitives).
    /// Maps to `--bbox-unsup`.
    #[serde(default)]
    pub bbox_unsup: bool,
    /// Include struct fields in the trace. Only effective when `trace`
    /// is `vcd` or `fst`. Maps to `--trace-structs`.
    #[serde(default)]
    pub trace_structs: bool,
    /// Include parameter values in the trace. Only effective when
    /// `trace` is `vcd` or `fst`. Maps to `--trace-params`.
    #[serde(default)]
    pub trace_params: bool,
    /// Maximum trace depth. Only effective when `trace` is `vcd` or
    /// `fst`. Maps to `--trace-depth N`.
    #[serde(default)]
    pub trace_depth: Option<u32>,
    #[serde(default)]
    pub extra_args: Vec<String>,
}

/// X-assignment policy for verilator's `--x-assign` flag. Controls how
/// uninitialised signals are treated in simulation: `Zero`/`One` force a
/// constant, `Fast` lets verilator pick whichever is cheaper at each use
/// site, and `Unique` picks a randomly-seeded constant per net to surface
/// X-propagation bugs that constant-zero would mask.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum XAssign {
    #[serde(rename = "0")]
    Zero,
    #[serde(rename = "1")]
    One,
    Fast,
    Unique,
}

impl XAssign {
    pub fn as_flag_value(&self) -> &'static str {
        match self {
            XAssign::Zero => "0",
            XAssign::One => "1",
            XAssign::Fast => "fast",
            XAssign::Unique => "unique",
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
        for name in self.features.features.keys() {
            if !is_valid_sv_identifier(name) {
                return Err(ManifestError::InvalidFeatureName(name.clone()));
            }
        }
        for name in &self.features.default {
            if !self.features.features.contains_key(name) {
                return Err(ManifestError::UnknownFeature(name.clone()));
            }
        }
        let mut firmware_names: std::collections::BTreeSet<&str> =
            std::collections::BTreeSet::new();
        for fw in &self.firmware {
            if !is_valid_sv_identifier(&fw.name) {
                return Err(ManifestError::InvalidFirmwareName(fw.name.clone()));
            }
            if !firmware_names.insert(&fw.name) {
                return Err(ManifestError::DuplicateFirmwareName(fw.name.clone()));
            }
        }
        Ok(())
    }

    /// Apply a feature selection to the resolved design: merges feature
    /// `defines` into `design.defines` and appends feature `sources` to
    /// `design.sources`. Later features override earlier ones on
    /// conflicting define keys (last write wins, in selection order).
    pub fn apply_features(&self, design: &mut Design, selection: &FeatureSelection) {
        for name in &selection.active {
            let Some(feat) = self.features.features.get(name) else {
                continue;
            };
            for entry in &feat.defines {
                let (k, v) = match entry.split_once('=') {
                    Some((k, v)) => (k.to_string(), v.to_string()),
                    None => (entry.clone(), String::new()),
                };
                design.defines.insert(k, v);
            }
            for src in &feat.sources {
                if !design.sources.contains(src) {
                    design.sources.push(src.clone());
                }
            }
        }
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
    fn tool_verilator_first_class_knobs() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.verilator]
            timing = true
            x_assign = "unique"
            bbox_unsup = true
            trace = "fst"
            trace_structs = true
            trace_params = true
            trace_depth = 8
            "#,
        )
        .unwrap();
        let v = m.tool.verilator.as_ref().unwrap();
        assert!(v.timing);
        assert_eq!(v.x_assign, Some(XAssign::Unique));
        assert!(v.bbox_unsup);
        assert!(v.trace_structs);
        assert!(v.trace_params);
        assert_eq!(v.trace_depth, Some(8));
    }

    #[test]
    fn x_assign_accepts_zero_one_fast_unique() {
        for (s, expected) in [
            ("\"0\"", XAssign::Zero),
            ("\"1\"", XAssign::One),
            ("\"fast\"", XAssign::Fast),
            ("\"unique\"", XAssign::Unique),
        ] {
            let src = format!(
                r#"
                [package]
                name = "demo"
                version = "0.1.0"

                [design]
                top = "t"

                [tool.verilator]
                x_assign = {s}
                "#
            );
            let m: Manifest = src.parse().unwrap();
            assert_eq!(m.tool.verilator.unwrap().x_assign, Some(expected));
        }
    }

    #[test]
    fn x_assign_rejects_unknown() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [tool.verilator]
            x_assign = "five"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
    }

    #[test]
    fn design_aux_tops_round_trips() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "z1top"
            aux_tops = ["glbl", "BUFG_helper"]
            "#,
        )
        .unwrap();
        assert_eq!(m.design.aux_tops, vec!["glbl", "BUFG_helper"]);
    }

    #[test]
    fn design_aux_tops_default_empty() {
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
        assert!(m.design.aux_tops.is_empty());
    }

    #[test]
    fn features_parse_and_resolve_default() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [features]
            default = ["sim"]

            [features.sim]
            defines = ["SIM"]

            [features.debug]
            defines = ["DEBUG=1"]
            sources = ["src/debug/**/*.sv"]
            "#,
        )
        .unwrap();
        assert_eq!(m.features.default, vec!["sim"]);
        assert_eq!(m.features.features.len(), 2);
        let sel = FeatureSelection::resolve(&m.features, &[], false, false).unwrap();
        assert_eq!(sel.active, vec!["sim"]);
    }

    #[test]
    fn features_resolve_no_default() {
        let m = parse(
            r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [features]
            default = ["a"]

            [features.a]
            defines = ["A"]

            [features.b]
            defines = ["B"]
            "#,
        )
        .unwrap();
        let sel = FeatureSelection::resolve(&m.features, &[], false, true).unwrap();
        assert!(sel.active.is_empty());
        let sel = FeatureSelection::resolve(&m.features, &["b".to_string()], false, true).unwrap();
        assert_eq!(sel.active, vec!["b"]);
    }

    #[test]
    fn features_resolve_all() {
        let m = parse(
            r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [features]
            default = []

            [features.a]
            defines = ["A"]

            [features.b]
            defines = ["B"]
            "#,
        )
        .unwrap();
        let sel = FeatureSelection::resolve(&m.features, &[], true, false).unwrap();
        assert_eq!(sel.active.len(), 2);
        assert!(sel.active.contains(&"a".to_string()));
        assert!(sel.active.contains(&"b".to_string()));
    }

    #[test]
    fn features_resolve_unknown_errors() {
        let m = parse(
            r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [features]
            default = []

            [features.a]
            "#,
        )
        .unwrap();
        let err = FeatureSelection::resolve(&m.features, &["nope".to_string()], false, false)
            .unwrap_err();
        assert!(matches!(err, ManifestError::UnknownFeature(_)));
    }

    #[test]
    fn features_default_must_exist() {
        let err = parse(
            r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [features]
            default = ["ghost"]
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::UnknownFeature(_)));
    }

    #[test]
    fn features_apply_merges_defines_and_sources() {
        let m = parse(
            r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"
            sources = ["src/**/*.sv"]
            defines = { BASE = "1" }

            [features]
            default = []

            [features.debug]
            defines = ["DEBUG", "VERBOSITY=2"]
            sources = ["src/debug/**/*.sv"]
            "#,
        )
        .unwrap();
        let sel =
            FeatureSelection::resolve(&m.features, &["debug".to_string()], false, false).unwrap();
        let mut design = m.design.clone();
        m.apply_features(&mut design, &sel);
        assert_eq!(design.defines.get("BASE"), Some(&"1".to_string()));
        assert_eq!(design.defines.get("DEBUG"), Some(&"".to_string()));
        assert_eq!(design.defines.get("VERBOSITY"), Some(&"2".to_string()));
        assert!(design.sources.contains(&"src/debug/**/*.sv".to_string()));
    }

    #[test]
    fn features_invalid_name_errors() {
        let err = parse(
            r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "t"

            [features]
            default = []

            [features."1bad"]
            defines = []
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::InvalidFeatureName(_)));
    }

    #[test]
    fn vendor_block_round_trips() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [vendor.xilinx]
            sim_models = ["hardware/sim_models/BUFG.sv", "hardware/sim_models/glbl.sv"]
            stubs = ["hardware/stubs/PLLE2_ADV.sv"]
            blackbox_modules = ["MMCME2_ADV", "PLLE2_ADV"]

            [vendor.altera]
            sim_models = []
            "#,
        )
        .unwrap();
        let xilinx = m.vendor.get("xilinx").unwrap();
        assert_eq!(xilinx.sim_models.len(), 2);
        assert_eq!(xilinx.stubs, vec!["hardware/stubs/PLLE2_ADV.sv"]);
        assert_eq!(xilinx.blackbox_modules, vec!["MMCME2_ADV", "PLLE2_ADV"]);
        assert!(m.vendor.contains_key("altera"));
    }

    #[test]
    fn vendor_block_default_empty() {
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
        assert!(m.vendor.is_empty());
    }

    #[test]
    fn firmware_round_trips() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"

            [design]
            top = "t"

            [[firmware]]
            name = "isa_tests"
            path = "software/riscv-isa-tests"
            build = "make"
            artifacts = "*.hex"

            [[firmware]]
            name = "c_tests"
            path = "software/c_tests"
            build = "make all"
            "#,
        )
        .unwrap();
        assert_eq!(m.firmware.len(), 2);
        assert_eq!(m.firmware[0].name, "isa_tests");
        assert_eq!(m.firmware[0].artifacts.as_deref(), Some("*.hex"));
        assert_eq!(m.firmware[1].build, "make all");
        assert!(m.firmware[1].artifacts.is_none());
    }

    #[test]
    fn firmware_invalid_name_errors() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            [design]
            top = "t"
            [[firmware]]
            name = "1bad"
            path = "."
            build = "true"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::InvalidFirmwareName(_)));
    }

    #[test]
    fn firmware_duplicate_name_errors() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            [design]
            top = "t"
            [[firmware]]
            name = "fw"
            path = "a"
            build = "true"
            [[firmware]]
            name = "fw"
            path = "b"
            build = "true"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::DuplicateFirmwareName(_)));
    }

    #[test]
    fn hooks_round_trip_and_empty_strings_treated_as_unset() {
        let m = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            [design]
            top = "t"
            [hooks]
            pre-build = "make -C ip/"
            pre-test = "echo hi"
            post-test = ""
            "#,
        )
        .unwrap();
        assert_eq!(m.hooks.for_phase(HookPhase::PreBuild), Some("make -C ip/"));
        assert_eq!(m.hooks.for_phase(HookPhase::PreTest), Some("echo hi"));
        assert_eq!(m.hooks.for_phase(HookPhase::PostTest), None);
        assert_eq!(m.hooks.for_phase(HookPhase::PreCheck), None);
    }

    #[test]
    fn hooks_unknown_phase_rejected_by_deny_unknown_fields() {
        let err = parse(
            r#"
            [package]
            name = "demo"
            version = "0.1.0"
            [design]
            top = "t"
            [hooks]
            "post-build" = "echo nope"
            "#,
        )
        .unwrap_err();
        assert!(matches!(err, ManifestError::Parse(_)));
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
