//! `kiln doc`: generate a static documentation site under `target/doc/`.

use anyhow::{anyhow, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, Manifest};
use slang_rs::Slang;

pub fn run(open: bool) -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let manifest = Manifest::load(&manifest_path)?;
    let source_set = SourceSet::resolve(&project_root, &manifest)?;

    let slang = Slang::new()?;
    let out_dir = project_root.join("target").join("doc");
    let docset = kiln_doc::generate(&slang, &manifest.package.name, &source_set, &out_dir)?;
    println!(
        "Generated docs for {} item(s) at {}",
        docset.items.len(),
        out_dir.join("index.html").display()
    );

    if open {
        let url = format!("file://{}", out_dir.join("index.html").display());
        // Best-effort open. macOS uses `open`; Linux uses `xdg-open`.
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        let _ = std::process::Command::new(opener).arg(&url).spawn();
    }

    Ok(())
}
