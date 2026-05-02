//! `kiln doc`: generate a static documentation site under `target/doc/`.

use std::time::Instant;

use anyhow::{anyhow, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, Manifest};
use slang_rs::Slang;

use crate::commands::build::fmt_elapsed;
use crate::reporter;

pub fn run(open: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let manifest = Manifest::load(&manifest_path)?;
    let source_set = SourceSet::resolve(&project_root, &manifest)?;

    reporter::status(
        "Generating",
        format!("docs for `{}`", manifest.package.name),
    );
    let started = Instant::now();
    let slang = Slang::new()?;
    let out_dir = project_root.join("target").join("doc");
    let docset = kiln_doc::generate(&slang, &manifest.package.name, &source_set, &out_dir)?;
    let index = out_dir.join("index.html");
    reporter::status(
        "Generated",
        format!(
            "{} item(s) at {} in {}",
            docset.items.len(),
            reporter::dim(&index.display().to_string()),
            fmt_elapsed(started.elapsed())
        ),
    );

    if open {
        let url = format!("file://{}", index.display());
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        reporter::debug("Opening", &url);
        let _ = std::process::Command::new(opener).arg(&url).spawn();
    }

    Ok(())
}
