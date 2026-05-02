//! `kiln add`, `kiln remove`, `kiln update`, `kiln tree`.

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};

use kiln_core::{find_manifest, Manifest};
use kiln_deps::Dependency;

fn project_paths() -> Result<(PathBuf, PathBuf)> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    Ok((project_root, manifest_path))
}

pub fn run_add(
    name: String,
    git: Option<String>,
    rev: Option<String>,
    branch: Option<String>,
    version: Option<String>,
    path: Option<PathBuf>,
) -> Result<()> {
    let (project_root, manifest_path) = project_paths()?;
    let dep = match (git, path) {
        (Some(g), None) => Dependency::Git {
            git: g,
            version,
            rev,
            branch,
        },
        (None, Some(p)) => Dependency::Path { path: p },
        (Some(_), Some(_)) => {
            anyhow::bail!("`--git` and `--path` are mutually exclusive")
        }
        (None, None) => {
            anyhow::bail!("provide either `--git <url>` or `--path <dir>`")
        }
    };
    kiln_deps::add(&project_root, &manifest_path, &name, dep)?;
    println!("Added dependency `{name}`");
    Ok(())
}

pub fn run_remove(name: String) -> Result<()> {
    let (project_root, manifest_path) = project_paths()?;
    kiln_deps::remove(&project_root, &manifest_path, &name)?;
    println!("Removed dependency `{name}`");
    Ok(())
}

pub fn run_update() -> Result<()> {
    let (project_root, manifest_path) = project_paths()?;
    let manifest = Manifest::load(&manifest_path)?;
    kiln_deps::update(&project_root, &manifest)?;
    println!("Updated `Kiln.lock`");
    Ok(())
}

pub fn run_tree() -> Result<()> {
    let (project_root, manifest_path) = project_paths()?;
    let manifest = Manifest::load(&manifest_path)?;
    let tree = kiln_deps::tree(&project_root, &manifest)?;
    print!("{tree}");
    Ok(())
}
