//! Build plan: the inputs, flags, top, and profile for one compilation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use kiln_core::{Manifest, ResolvedConfig};

use crate::source_set::SourceSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Profile {
    Debug,
    Release,
}

impl Profile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Profile::Debug => "debug",
            Profile::Release => "release",
        }
    }
}

/// What to build, where the inputs came from, and how.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildPlan {
    pub project_root: PathBuf,
    pub top: String,
    pub sources: Vec<PathBuf>,
    pub include_dirs: Vec<PathBuf>,
    pub defines: BTreeMap<String, String>,
    pub profile: Profile,
    /// When true, the simulator binary dumps an FST trace. The
    /// testbench is responsible for `$dumpfile`/`$dumpvars` calls;
    /// `kiln-build` passes `+define+KILN_TRACE` so testbenches can
    /// gate the dump on `\`ifdef KILN_TRACE`.
    #[serde(default)]
    pub trace: bool,
    /// Passed as `--timescale` to verilator.
    #[serde(default)]
    pub timescale: Option<String>,
    /// Passed as `--default-language` to verilator.
    #[serde(default)]
    pub language: Option<String>,
    /// Library search directories, passed as `-y <dir>` to verilator.
    /// Resolved to absolute paths relative to the project root.
    #[serde(default)]
    pub libraries: Vec<PathBuf>,
    /// Translated lint flags: `-Wno-NAME`, `-Wwarn-NAME`, `-Werror-NAME`.
    /// Derived from `[lint.verilator]` in the manifest.
    #[serde(default)]
    pub verilator_lint_flags: Vec<String>,
    /// Extra flags forwarded verbatim to verilator.
    #[serde(default)]
    pub extra_verilator_args: Vec<String>,
    /// First-class `[tool.verilator]` knobs, lifted out of `extra_args`.
    #[serde(default)]
    pub verilator_options: VerilatorOptions,
    /// Module names to pass as `--bbox <name>` to verilator. Aggregated
    /// from every `[vendor.<name>].blackbox_modules` block.
    #[serde(default)]
    pub blackbox_modules: Vec<String>,
}

/// First-class verilator flags surfaced in `[tool.verilator]`. Each field
/// here corresponds to a typed knob in the manifest; raw `extra_args`
/// continues to handle anything not covered.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct VerilatorOptions {
    /// Adds `--timing`.
    #[serde(default)]
    pub timing: bool,
    /// Adds `--x-assign <value>`. When `None`, the release profile still
    /// emits its historical `--x-assign 0` default.
    #[serde(default)]
    pub x_assign: Option<String>,
    /// Adds `--bbox-unsup`.
    #[serde(default)]
    pub bbox_unsup: bool,
    /// Adds `--trace-structs` when tracing is enabled.
    #[serde(default)]
    pub trace_structs: bool,
    /// Adds `--trace-params` when tracing is enabled.
    #[serde(default)]
    pub trace_params: bool,
    /// Adds `--trace-depth N` when tracing is enabled.
    #[serde(default)]
    pub trace_depth: Option<u32>,
    /// Adds `--threads N`.
    #[serde(default)]
    pub threads: Option<u32>,
    /// Adds `--coverage` (the kitchen-sink `--coverage-line --coverage-toggle`
    /// shorthand).
    #[serde(default)]
    pub coverage: bool,
}

impl BuildPlan {
    pub fn new(manifest: &Manifest, source_set: &SourceSet, profile: Profile) -> Self {
        let resolved = ResolvedConfig::resolve(manifest, profile.as_str());
        let mut plan = Self::from_resolved(&resolved, source_set, profile);
        plan.blackbox_modules = aggregate_blackbox_modules(manifest);
        plan
    }

    /// Construct a plan from an already-resolved config.
    pub fn from_resolved(
        resolved: &ResolvedConfig,
        source_set: &SourceSet,
        profile: Profile,
    ) -> Self {
        use kiln_core::{SvLanguage, TraceFormat};
        let trace = matches!(
            resolved.tool_verilator.trace,
            TraceFormat::Vcd | TraceFormat::Fst
        );
        let language = resolved.design.language.map(|lang| {
            match lang {
                SvLanguage::Sv2005 => "1364-2005",
                SvLanguage::Sv2009 => "1800-2009",
                SvLanguage::Sv2012 => "1800-2012",
                SvLanguage::Sv2017 => "1800-2017",
                SvLanguage::Sv2023 => "1800-2023",
            }
            .to_string()
        });
        Self {
            project_root: source_set.project_root.clone(),
            top: resolved.design.top.clone(),
            sources: source_set.files.clone(),
            include_dirs: resolved
                .design
                .include_dirs
                .iter()
                .map(|p| source_set.project_root.join(p))
                .collect(),
            defines: resolved.design.defines.clone(),
            profile,
            trace,
            timescale: resolved.design.timescale.clone(),
            language,
            libraries: resolved
                .design
                .libraries
                .iter()
                .map(|s| source_set.project_root.join(s))
                .collect(),
            verilator_lint_flags: build_verilator_lint_flags(&resolved.lint),
            extra_verilator_args: resolved.tool_verilator.extra_args.clone(),
            blackbox_modules: Vec::new(),
            verilator_options: VerilatorOptions {
                timing: resolved.tool_verilator.timing,
                x_assign: resolved
                    .tool_verilator
                    .x_assign
                    .map(|x| x.as_flag_value().to_string()),
                bbox_unsup: resolved.tool_verilator.bbox_unsup,
                trace_structs: resolved.tool_verilator.trace_structs,
                trace_params: resolved.tool_verilator.trace_params,
                trace_depth: resolved.tool_verilator.trace_depth,
                threads: resolved.tool_verilator.threads,
                coverage: resolved.tool_verilator.coverage,
            },
        }
    }
}

impl BuildPlan {
    /// Builder-style: enable tracing.
    pub fn with_trace(mut self, on: bool) -> Self {
        self.trace = on;
        if on {
            self.defines.insert("KILN_TRACE".to_string(), String::new());
        } else {
            self.defines.remove("KILN_TRACE");
        }
        self
    }
}

/// Collect every `[vendor.<name>].blackbox_modules` entry into a
/// deduplicated `Vec<String>`, preserving first-seen order.
pub fn aggregate_blackbox_modules(manifest: &Manifest) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for vendor in manifest.vendor.values() {
        for name in &vendor.blackbox_modules {
            if seen.insert(name.clone()) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// Translate `[lint.verilator]` rules into `-Wno-NAME` / `-Wwarn-NAME` /
/// `-Werror-NAME` flags for the verilator command line.
fn build_verilator_lint_flags(lint: &kiln_core::LintConfig) -> Vec<String> {
    use kiln_core::LintSeverity;
    let mut flags = Vec::new();
    for (name, sev) in &lint.verilator {
        let flag = match sev {
            LintSeverity::Off | LintSeverity::Deny => format!("-Wno-{name}"),
            LintSeverity::Warn => format!("-Wwarn-{name}"),
            LintSeverity::Error => format!("-Werror-{name}"),
        };
        flags.push(flag);
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_carries_top_and_profile() {
        let m: Manifest = r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "top"
            include_dirs = ["inc"]
            defines = { FOO = "1" }
        "#
        .parse()
        .unwrap();
        let set = SourceSet {
            project_root: PathBuf::from("/proj"),
            files: vec![PathBuf::from("/proj/src/top.sv")],
        };
        let plan = BuildPlan::new(&m, &set, Profile::Release);
        assert_eq!(plan.top, "top");
        assert_eq!(plan.profile, Profile::Release);
        assert_eq!(plan.include_dirs, vec![PathBuf::from("/proj/inc")]);
        assert_eq!(plan.defines.get("FOO"), Some(&"1".to_string()));
    }

    #[test]
    fn profile_as_str() {
        assert_eq!(Profile::Debug.as_str(), "debug");
        assert_eq!(Profile::Release.as_str(), "release");
    }

    #[test]
    fn plan_carries_timescale_language_libraries() {
        let m: Manifest = r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "top"
            timescale = "1ns/1ps"
            language = "sv2017"
            libraries = ["vendor/lib"]
        "#
        .parse()
        .unwrap();
        let set = SourceSet {
            project_root: PathBuf::from("/proj"),
            files: vec![],
        };
        let plan = BuildPlan::new(&m, &set, Profile::Debug);
        assert_eq!(plan.timescale.as_deref(), Some("1ns/1ps"));
        assert_eq!(plan.language.as_deref(), Some("1800-2017"));
        assert_eq!(plan.libraries, vec![PathBuf::from("/proj/vendor/lib")]);
    }

    #[test]
    fn aggregate_blackbox_modules_dedupes_across_vendors() {
        let m: Manifest = r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            [vendor.xilinx]
            blackbox_modules = ["MMCME2_ADV", "PLLE2_ADV"]
            [vendor.altera]
            blackbox_modules = ["PLLE2_ADV", "altpll"]
        "#
        .parse()
        .unwrap();
        let names = aggregate_blackbox_modules(&m);
        assert!(names.contains(&"MMCME2_ADV".to_string()));
        assert!(names.contains(&"altpll".to_string()));
        // PLLE2_ADV listed twice across vendors should appear once.
        assert_eq!(
            names.iter().filter(|n| n.as_str() == "PLLE2_ADV").count(),
            1
        );
    }

    #[test]
    fn plan_carries_verilator_options() {
        let m: Manifest = r#"
            [package]
            name = "p"
            version = "0.1.0"

            [design]
            top = "top"

            [tool.verilator]
            threads = 4
            timing = true
            x_assign = "unique"
            bbox_unsup = true
            trace = "fst"
            trace_structs = true
            trace_params = true
            trace_depth = 6
        "#
        .parse()
        .unwrap();
        let set = SourceSet {
            project_root: PathBuf::from("/proj"),
            files: vec![],
        };
        let plan = BuildPlan::new(&m, &set, Profile::Debug);
        let v = &plan.verilator_options;
        assert!(v.timing);
        assert_eq!(v.x_assign.as_deref(), Some("unique"));
        assert!(v.bbox_unsup);
        assert!(v.trace_structs);
        assert!(v.trace_params);
        assert_eq!(v.trace_depth, Some(6));
        assert_eq!(v.threads, Some(4));
        assert!(plan.trace);
    }

    #[test]
    fn plan_language_maps_all_standards() {
        let cases = [
            ("sv2005", "1364-2005"),
            ("sv2009", "1800-2009"),
            ("sv2012", "1800-2012"),
            ("sv2017", "1800-2017"),
            ("sv2023", "1800-2023"),
        ];
        for (toml_val, expected_flag) in cases {
            let src = format!(
                r#"
                [package]
                name = "p"
                version = "0.1.0"
                [design]
                top = "t"
                language = "{toml_val}"
                "#
            );
            let m: Manifest = src.parse().unwrap();
            let set = SourceSet {
                project_root: PathBuf::from("/p"),
                files: vec![],
            };
            let plan = BuildPlan::new(&m, &set, Profile::Debug);
            assert_eq!(
                plan.language.as_deref(),
                Some(expected_flag),
                "language = {toml_val}"
            );
        }
    }
}
