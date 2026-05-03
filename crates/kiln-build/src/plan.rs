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
    #[serde(default)]
    pub libraries: Vec<String>,
    /// Translated lint flags: `-Wno-NAME`, `-Wwarn-NAME`, `-Werror-NAME`.
    /// Derived from `[lint.verilator]` in the manifest.
    #[serde(default)]
    pub verilator_lint_flags: Vec<String>,
    /// Extra flags forwarded verbatim to verilator.
    #[serde(default)]
    pub extra_verilator_args: Vec<String>,
}

impl BuildPlan {
    pub fn new(manifest: &Manifest, source_set: &SourceSet, profile: Profile) -> Self {
        let resolved = ResolvedConfig::resolve(manifest, profile.as_str());
        Self::from_resolved(&resolved, source_set, profile)
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
            libraries: resolved.design.libraries.clone(),
            verilator_lint_flags: build_verilator_lint_flags(&resolved.lint),
            extra_verilator_args: resolved.tool_verilator.extra_args.clone(),
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
        assert_eq!(plan.libraries, vec!["vendor/lib".to_string()]);
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
