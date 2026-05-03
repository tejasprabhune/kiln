//! Build plan: the inputs, flags, top, and profile for one compilation.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use kiln_core::Manifest;

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
    /// Extra flags forwarded verbatim to verilator.
    #[serde(default)]
    pub extra_verilator_args: Vec<String>,
}

impl BuildPlan {
    pub fn new(manifest: &Manifest, source_set: &SourceSet, profile: Profile) -> Self {
        Self {
            project_root: source_set.project_root.clone(),
            top: manifest.design.top.clone(),
            sources: source_set.files.clone(),
            include_dirs: manifest
                .design
                .include_dirs
                .iter()
                .map(|p| source_set.project_root.join(p))
                .collect(),
            defines: manifest.design.defines.clone(),
            profile,
            trace: false,
            extra_verilator_args: manifest.design.verilator_args.clone(),
        }
    }

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
}
