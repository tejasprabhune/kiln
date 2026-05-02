//! `kiln fmt` and `kiln fmt --check`.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, Manifest};

use crate::reporter;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Plain,
    Json,
}

pub fn run(check: bool, format: OutputFormat) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let manifest = Manifest::load(&manifest_path)?;
    let source_set = SourceSet::resolve(&project_root, &manifest)?;

    if check {
        run_check(&source_set, format)
    } else {
        run_format(&source_set, format)
    }
}

fn run_format(source_set: &SourceSet, format: OutputFormat) -> Result<()> {
    if matches!(format, OutputFormat::Plain) {
        reporter::status(
            "Formatting",
            format!("{} source file(s)", source_set.files().len()),
        );
    }
    let mut changed: Vec<PathBuf> = Vec::new();
    let mut unchanged: Vec<PathBuf> = Vec::new();
    for f in source_set.files() {
        if kiln_fmt::format_in_place(f)? {
            changed.push(f.clone());
        } else {
            unchanged.push(f.clone());
        }
    }
    match format {
        OutputFormat::Plain => {
            for c in &changed {
                reporter::debug("Formatted", c.display());
            }
            let summary = format!(
                "{} formatted, {} already canonical",
                changed.len(),
                unchanged.len()
            );
            reporter::status("Result", reporter::green(&summary));
        }
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "formatted": changed,
                "unchanged": unchanged,
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }
    Ok(())
}

fn run_check(source_set: &SourceSet, format: OutputFormat) -> Result<()> {
    if matches!(format, OutputFormat::Plain) {
        reporter::status(
            "Checking",
            format!("formatting of {} source file(s)", source_set.files().len()),
        );
    }
    let mut outcomes = Vec::new();
    let mut bad = 0usize;
    for f in source_set.files() {
        let outcome = kiln_fmt::check(f)?;
        if !outcome.ok {
            bad += 1;
        }
        outcomes.push(outcome);
    }
    match format {
        OutputFormat::Plain => {
            for o in &outcomes {
                if !o.ok {
                    print!("{}", o.diff);
                }
            }
            let summary = format!(
                "{} canonical, {} need formatting",
                outcomes.len() - bad,
                bad
            );
            if bad > 0 {
                reporter::status("Result", reporter::red(&summary));
            } else {
                reporter::status("Result", reporter::green(&summary));
            }
        }
        OutputFormat::Json => {
            let payload = serde_json::json!({
                "results": outcomes,
                "summary": {
                    "total": outcomes.len(),
                    "needs_formatting": bad,
                }
            });
            println!("{}", serde_json::to_string_pretty(&payload)?);
        }
    }
    if bad > 0 {
        bail!("{bad} file(s) need formatting");
    }
    Ok(())
}

#[allow(dead_code)]
fn _ensure_path_imports_used(p: &Path) -> bool {
    p.exists()
}
