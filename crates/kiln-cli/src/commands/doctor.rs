//! `kiln doctor` — environment + project sanity checks.
//!
//! Builds on `kiln env` (tool discovery) and adds:
//! - Manifest parsability
//! - Lockfile / dependency sync
//! - Vendor source globs match at least one file
//! - Firmware paths exist
//!
//! Exit code 0 if everything is fine, non-zero on the first hard error.

use std::path::Path;

use anyhow::{Context, Result};

use kiln_core::{find_manifest, Manifest};

use crate::commands::env;
use crate::reporter;

pub fn run() -> Result<()> {
    // 1. Tool discovery
    env::run()?;
    println!();

    // 2. Project-level checks (only if we are inside a kiln project).
    let cwd = std::env::current_dir().context("reading current directory")?;
    match find_manifest(&cwd) {
        Ok(manifest_path) => check_project(&manifest_path)?,
        Err(_) => {
            println!(
                "project: {}",
                reporter::dim("no Kiln.toml found in cwd ancestors (skipping project checks)")
            );
        }
    }

    Ok(())
}

fn check_project(manifest_path: &Path) -> Result<()> {
    let project_root = manifest_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();

    println!(
        "project: {}",
        reporter::dim(&manifest_path.display().to_string())
    );

    let mut soft_errors: Vec<String> = Vec::new();

    let manifest = match Manifest::load(manifest_path) {
        Ok(m) => {
            println!(
                "  manifest          {}",
                reporter::green(&format!("ok ({})", m.package.name))
            );
            m
        }
        Err(e) => {
            println!("  manifest          {}", reporter::red(&e.to_string()));
            anyhow::bail!("manifest validation failed");
        }
    };

    let lock = project_root.join("Kiln.lock");
    if !manifest.dependencies.is_empty() {
        if lock.is_file() {
            println!("  lockfile          {}", reporter::green("present"));
        } else {
            println!(
                "  lockfile          {}",
                reporter::yellow("missing — run `kiln update`")
            );
        }
    } else {
        println!(
            "  lockfile          {}",
            reporter::dim("n/a (no dependencies)")
        );
    }

    if manifest.vendor.is_empty() {
        println!("  vendor blocks     {}", reporter::dim("none"));
    } else {
        for (name, vendor) in &manifest.vendor {
            let mut counts = (0usize, 0usize);
            for g in &vendor.sim_models {
                counts.0 += glob_count(&project_root, g);
            }
            for g in &vendor.stubs {
                counts.1 += glob_count(&project_root, g);
            }
            let total = counts.0 + counts.1;
            let label = if total == 0 && (!vendor.sim_models.is_empty() || !vendor.stubs.is_empty())
            {
                soft_errors.push(format!(
                    "vendor `{name}`: no files matched any glob in sim_models/stubs",
                ));
                reporter::yellow("0 files (none matched globs)")
            } else {
                reporter::green(&format!(
                    "{} sim-model + {} stub file(s)",
                    counts.0, counts.1
                ))
            };
            println!("  vendor.{:<10}  {}", name, label);
        }
    }

    if manifest.firmware.is_empty() {
        println!("  firmware          {}", reporter::dim("none"));
    } else {
        for fw in &manifest.firmware {
            let dir = project_root.join(&fw.path);
            if dir.is_dir() {
                println!(
                    "  firmware.{:<8}  {} ({})",
                    fw.name,
                    reporter::green("ok"),
                    reporter::dim(&dir.display().to_string())
                );
            } else {
                soft_errors.push(format!(
                    "firmware `{}`: path `{}` does not exist or is not a directory",
                    fw.name,
                    dir.display()
                ));
                println!(
                    "  firmware.{:<8}  {}",
                    fw.name,
                    reporter::red(&format!("path missing: {}", dir.display()))
                );
            }
        }
    }

    let any_hook = manifest
        .hooks
        .pre_check
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .is_some()
        || manifest
            .hooks
            .pre_build
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        || manifest
            .hooks
            .pre_test
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some()
        || manifest
            .hooks
            .post_test
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .is_some();
    println!(
        "  hooks             {}",
        if any_hook {
            reporter::green("configured")
        } else {
            reporter::dim("none")
        }
    );

    if !soft_errors.is_empty() {
        println!();
        for w in &soft_errors {
            println!("  {} {}", reporter::yellow("warning:"), w);
        }
        anyhow::bail!("doctor found {} issue(s); see above", soft_errors.len());
    }

    Ok(())
}

fn glob_count(root: &Path, raw: &str) -> usize {
    let pattern = if std::path::Path::new(raw).is_absolute() {
        raw.to_string()
    } else {
        root.join(raw).to_string_lossy().into_owned()
    };
    let Ok(it) = glob::glob(&pattern) else {
        return 0;
    };
    let mut n = 0usize;
    for entry in it.flatten() {
        if entry.is_file() {
            n += 1;
        }
    }
    n
}
