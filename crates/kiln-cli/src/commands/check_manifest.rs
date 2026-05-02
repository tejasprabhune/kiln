use std::path::Path;

use anyhow::{Context, Result};

use kiln_core::{find_manifest, Manifest};

/// `kiln check-manifest`: parse and print the manifest. Hidden command,
/// used by integration tests as a smoke check.
pub fn run(path: Option<&Path>) -> Result<()> {
    let manifest_path = match path {
        Some(p) => p.to_path_buf(),
        None => {
            let cwd = std::env::current_dir().context("reading current directory")?;
            find_manifest(&cwd)?
        }
    };
    let manifest = Manifest::load(&manifest_path)
        .with_context(|| format!("loading manifest from {}", manifest_path.display()))?;
    println!("{}", toml::to_string_pretty(&manifest)?);
    Ok(())
}
