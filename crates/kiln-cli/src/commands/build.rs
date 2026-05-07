//! `kiln build`, `kiln run`, `kiln clean`.

use std::path::Path;
use std::process::Command;
use std::time::Instant;

use anyhow::{anyhow, bail, Context, Result};

use kiln_build::backend::verilator;
use kiln_build::{BuildPlan, SourceSet};
use kiln_core::ResolvedConfig;
use kiln_core::{find_manifest, HookPhase, Manifest};
use kiln_deps::ResolvedSources;

use crate::commands::{apply_feature_flags, FeatureFlags};
use crate::hooks;
use crate::render;
use crate::reporter;

pub fn run_build(
    profile_name: &str,
    verbose: bool,
    features: &FeatureFlags,
) -> Result<BuildArtifacts> {
    if verbose {
        bump_log_level();
    }
    let started = Instant::now();
    let project_root = current_project_root()?;
    let manifest_path = find_manifest(&project_root)?;
    let mut manifest = Manifest::load(&manifest_path)
        .with_context(|| format!("loading manifest from {}", manifest_path.display()))?;
    apply_feature_flags(&mut manifest, features)?;

    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();

    hooks::run_pre_hook(&project_root, &manifest.hooks, HookPhase::PreBuild)?;

    let resolved = ResolvedConfig::resolve(&manifest, profile_name);

    let mut source_set = SourceSet::resolve(&project_root, &manifest)?;
    let mut dep_include_dirs: Vec<std::path::PathBuf> = Vec::new();
    if !manifest.dependencies.is_empty() {
        reporter::status("Resolving", "dependencies via bender");
        let resolved_srcs: ResolvedSources = kiln_deps::resolve(&project_root, &manifest)?;
        reporter::debug(
            "Resolved",
            format!(
                "{} package(s) from `Kiln.lock`",
                resolved_srcs.packages.len()
            ),
        );
        for f in resolved_srcs.all_files() {
            if !source_set.files.contains(&f) {
                source_set.files.push(f);
            }
        }
        dep_include_dirs = resolved_srcs.all_include_dirs();
    }
    let profile = if profile_name == "release" {
        kiln_build::Profile::Release
    } else {
        kiln_build::Profile::Debug
    };
    let mut plan = BuildPlan::from_resolved(&resolved, &source_set, profile);
    plan.blackbox_modules = kiln_build::aggregate_blackbox_modules(&manifest);
    for d in dep_include_dirs {
        if !plan.include_dirs.contains(&d) {
            plan.include_dirs.push(d);
        }
    }

    reporter::status(
        "Compiling",
        format!("`{}` with verilator ({profile_name} profile)", plan.top),
    );
    let outcome = verilator::compile(&plan)?;

    let rendered = render::render(&outcome.diagnostics);
    if !rendered.is_empty() {
        // Diagnostics go to stdout so callers can pipe them; reporter
        // status lines stay on stderr.
        print!("{rendered}");
    }

    let has_errors = outcome
        .diagnostics
        .iter()
        .any(|d| matches!(d.severity, kiln_build::Severity::Error));
    if has_errors {
        bail!("build failed");
    }

    let binary = match outcome.binary {
        Some(p) => p,
        None => bail!("verilator did not produce a binary; see diagnostics above"),
    };

    let elapsed = started.elapsed();
    if outcome.cache_hit {
        reporter::info(
            "Cache hit",
            format!(
                "`{}` ({profile_name} profile) at {}",
                plan.top,
                reporter::dim(&binary.display().to_string())
            ),
        );
    } else {
        reporter::status(
            "Finished",
            format!(
                "`{}` ({profile_name} profile) in {}",
                plan.top,
                fmt_elapsed(elapsed)
            ),
        );
    }

    Ok(BuildArtifacts {
        binary,
        top: plan.top,
        cache_hit: outcome.cache_hit,
    })
}

pub fn run_run(
    profile_name: &str,
    verbose: bool,
    features: &FeatureFlags,
    forwarded: Vec<String>,
) -> Result<()> {
    let artifacts = run_build(profile_name, verbose, features)?;
    reporter::status(
        "Running",
        format!(
            "{}{}",
            artifacts.binary.display(),
            if forwarded.is_empty() {
                String::new()
            } else {
                format!(" {}", forwarded.join(" "))
            }
        ),
    );
    let status = Command::new(&artifacts.binary)
        .args(&forwarded)
        .status()
        .with_context(|| format!("invoking {}", artifacts.binary.display()))?;
    if !status.success() {
        bail!(
            "`{}` exited with code {:?}",
            artifacts.binary.display(),
            status.code()
        );
    }
    Ok(())
}

pub fn run_clean() -> Result<()> {
    let project_root = current_project_root()?;
    let manifest_path = find_manifest(&project_root)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?;
    verilator::clean(project_root).with_context(|| {
        format!(
            "removing build cache under {}/target/kiln",
            project_root.display()
        )
    })?;
    reporter::status("Removed", "build cache");
    Ok(())
}

fn current_project_root() -> Result<std::path::PathBuf> {
    std::env::current_dir().context("reading current directory")
}

fn bump_log_level() {
    // Already initialised in main, but env-based filter can be widened
    // post-hoc by callers that pass `-v`. SAFETY: env mutation is only
    // safe when no other thread is reading the environment; we are the
    // top-level CLI and have not spawned threads at this point.
    unsafe {
        std::env::set_var("KILN_LOG", "debug");
    }
}

pub fn fmt_elapsed(d: std::time::Duration) -> String {
    if d.as_secs() == 0 {
        format!("{}ms", d.as_millis())
    } else {
        format!("{:.2}s", d.as_secs_f32())
    }
}

pub struct BuildArtifacts {
    pub binary: std::path::PathBuf,
    #[allow(dead_code)]
    pub top: String,
    #[allow(dead_code)]
    pub cache_hit: bool,
}

#[allow(dead_code)]
fn _ensure_path_imports_used(p: &Path) -> bool {
    p.exists()
}
