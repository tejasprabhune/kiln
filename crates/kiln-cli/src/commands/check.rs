//! `kiln check`: fast slang-driven elaboration check, no Verilator.

use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, HookPhase, Manifest, ResolvedConfig};
use slang_rs::Slang;

use crate::commands::build::fmt_elapsed;
use crate::commands::{apply_feature_flags, FeatureFlags};
use crate::hooks;
use crate::render;
use crate::reporter;

pub fn run(
    deny_warnings: bool,
    verbose: bool,
    profile_name: &str,
    features: &FeatureFlags,
) -> Result<()> {
    if verbose {
        unsafe {
            std::env::set_var("KILN_LOG", "debug");
        }
    }
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let mut manifest = Manifest::load(&manifest_path)?;
    apply_feature_flags(&mut manifest, features)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    hooks::run_pre_hook(&project_root, &manifest.hooks, HookPhase::PreCheck)?;
    let resolved = ResolvedConfig::resolve(&manifest, profile_name);
    let source_set = SourceSet::resolve(&project_root, &manifest)?;

    reporter::status("Checking", format!("`{}` with slang", manifest.design.top));
    let started = Instant::now();
    let slang = Slang::new()?;
    reporter::debug("Using", format!("slang {}", slang.version()));
    let diagnostics = kiln_lint::check(&slang, &resolved, &source_set)?;

    let rendered = render::render(&diagnostics);
    if !rendered.is_empty() {
        print!("{rendered}");
    }

    let n_errors = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, kiln_build::Severity::Error))
        .count();
    let n_warnings = diagnostics
        .iter()
        .filter(|d| matches!(d.severity, kiln_build::Severity::Warning))
        .count();
    let elapsed = fmt_elapsed(started.elapsed());

    if n_errors > 0 {
        reporter::status(
            "Result",
            reporter::red(&format!("{n_errors} error(s) in {elapsed}")),
        );
        bail!("check failed");
    }
    if n_warnings > 0 {
        if deny_warnings {
            reporter::status(
                "Result",
                reporter::red(&format!(
                    "{n_warnings} warning(s) in {elapsed} (--deny-warnings)"
                )),
            );
            std::process::exit(1);
        }
        reporter::status(
            "Result",
            reporter::yellow(&format!("{n_warnings} warning(s) in {elapsed}")),
        );
        return Ok(());
    }
    reporter::status("Result", reporter::green(&format!("clean in {elapsed}")));
    Ok(())
}
