//! `kiln check`: fast slang-driven elaboration check, no Verilator.

use anyhow::{anyhow, bail, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, Manifest};
use slang_rs::Slang;

use crate::render;

pub fn run(deny_warnings: bool, verbose: bool) -> Result<()> {
    if verbose {
        unsafe {
            std::env::set_var("KILN_LOG", "debug");
        }
    }
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let manifest = Manifest::load(&manifest_path)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let source_set = SourceSet::resolve(&project_root, &manifest)?;

    let slang = Slang::new()?;
    let diagnostics = kiln_lint::check(&slang, &manifest, &source_set)?;

    let rendered = render::render(&diagnostics);
    if !rendered.is_empty() {
        print!("{rendered}");
    }

    let has_errors = diagnostics
        .iter()
        .any(|d| matches!(d.severity, kiln_build::Severity::Error));
    let has_warnings = diagnostics
        .iter()
        .any(|d| matches!(d.severity, kiln_build::Severity::Warning));

    if has_errors {
        bail!("check failed");
    }
    if deny_warnings && has_warnings {
        std::process::exit(1);
    }
    Ok(())
}
