//! Profile resolution: merges a named profile overlay onto base manifest values.

use crate::manifest::{
    Design, LintConfig, Manifest, ToolSlang, ToolVerible, ToolVerilator, TraceFormat,
};

/// The fully-resolved configuration for a given profile.
pub struct ResolvedConfig {
    pub design: Design,
    pub lint: LintConfig,
    pub tool_slang: ToolSlang,
    pub tool_verilator: ToolVerilator,
    pub tool_verible: ToolVerible,
}

impl ResolvedConfig {
    /// Resolves `profile_name` on top of `manifest`'s base values.
    ///
    /// Vec fields in tool overrides REPLACE (not append). Map fields merge
    /// with the overlay winning on key conflicts.
    pub fn resolve(manifest: &Manifest, profile_name: &str) -> Self {
        let design = manifest.design.clone();
        let mut lint = manifest.lint.clone();
        let mut tool_slang = manifest.tool.slang.clone().unwrap_or_default();
        let mut tool_verilator = manifest.tool.verilator.clone().unwrap_or_default();
        let mut tool_verible = manifest.tool.verible.clone().unwrap_or_default();

        if let Some(overlay) = manifest.profile.get(profile_name) {
            if let Some(t) = &overlay.tool {
                if let Some(s) = &t.slang {
                    tool_slang.extra_args = s.extra_args.clone();
                    if s.path.is_some() {
                        tool_slang.path = s.path.clone();
                    }
                }
                if let Some(v) = &t.verilator {
                    tool_verilator.extra_args = v.extra_args.clone();
                    if v.path.is_some() {
                        tool_verilator.path = v.path.clone();
                    }
                    tool_verilator.threads = v.threads.or(tool_verilator.threads);
                    if !matches!(v.trace, TraceFormat::Off) {
                        tool_verilator.trace = v.trace;
                    }
                    if v.coverage {
                        tool_verilator.coverage = true;
                    }
                }
                if let Some(vb) = &t.verible {
                    tool_verible.extra_args = vb.extra_args.clone();
                    if vb.path.is_some() {
                        tool_verible.path = vb.path.clone();
                    }
                }
            }
            if let Some(lint_overlay) = &overlay.lint {
                for (k, v) in &lint_overlay.rules {
                    lint.rules.insert(k.clone(), *v);
                }
                for (k, v) in &lint_overlay.slang {
                    lint.slang.insert(k.clone(), *v);
                }
                for (k, v) in &lint_overlay.verilator {
                    lint.verilator.insert(k.clone(), *v);
                }
            }
        }

        ResolvedConfig {
            design,
            lint,
            tool_slang,
            tool_verilator,
            tool_verible,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_manifest() -> Manifest {
        r#"
        [package]
        name = "demo"
        version = "0.1.0"

        [design]
        top = "t"

        [tool.verilator]
        threads = 2

        [lint]
        width-trunc = "warn"

        [profile.test.tool.verilator]
        trace = "fst"
        coverage = true

        [profile.release.tool.verilator]
        extra_args = ["-O3"]

        [profile.test.lint]
        unused = "error"
        "#
        .parse()
        .unwrap()
    }

    #[test]
    fn test_profile_enables_trace_and_coverage() {
        let m = base_manifest();
        let r = ResolvedConfig::resolve(&m, "test");
        assert_eq!(r.tool_verilator.trace, TraceFormat::Fst);
        assert!(r.tool_verilator.coverage);
        assert_eq!(r.tool_verilator.threads, Some(2));
    }

    #[test]
    fn release_profile_replaces_extra_args() {
        let m = base_manifest();
        let r = ResolvedConfig::resolve(&m, "release");
        assert_eq!(r.tool_verilator.extra_args, vec!["-O3"]);
    }

    #[test]
    fn dev_profile_uses_base_values() {
        let m = base_manifest();
        let r = ResolvedConfig::resolve(&m, "dev");
        assert_eq!(r.tool_verilator.trace, TraceFormat::Off);
        assert!(!r.tool_verilator.coverage);
    }

    #[test]
    fn profile_lint_overlay_merges() {
        let m = base_manifest();
        let r = ResolvedConfig::resolve(&m, "test");
        use crate::manifest::LintSeverity;
        assert_eq!(r.lint.rules.get("unused"), Some(&LintSeverity::Error));
        assert_eq!(r.lint.rules.get("width-trunc"), Some(&LintSeverity::Warn));
    }
}
