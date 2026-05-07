// `BenderError` carries paths and captured stderr from the bender invocation.
#![allow(clippy::result_large_err)]
//! Dependency resolution for `kiln`. Subprocess wrapper around the
//! [`bender`](https://github.com/pulp-platform/bender) binary.
//!
//! See `docs/decisions/0003-bender-integration.md` for the why.

mod runner;
mod schema;
mod sources;

pub use runner::BenderError;
pub use schema::{Dependency, DependencyTable};
pub use sources::{ResolvedPackage, ResolvedSources};

use std::path::{Path, PathBuf};

use kiln_core::Manifest;

/// CI-determinism mode for dependency resolution.
///
/// - [`Free`](LockMode::Free) (default): mutate `Kiln.lock` as needed.
/// - [`Locked`](LockMode::Locked): error if a refresh would change
///   `Kiln.lock`. The pre-existing lockfile is restored on detection.
/// - [`Frozen`](LockMode::Frozen): implies `Locked` and additionally
///   refuses to run `bender update`. The existing `Kiln.lock` /
///   `Bender.lock` are used as-is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LockMode {
    #[default]
    Free,
    Locked,
    Frozen,
}

#[derive(Debug, thiserror::Error)]
#[error(
    "lockfile drift: `Kiln.lock` would change to match `Kiln.toml`. \
     Run `kiln update` and commit the result, or drop `--locked` / `--frozen`."
)]
pub struct LockDriftError;

impl From<LockDriftError> for BenderError {
    fn from(_: LockDriftError) -> Self {
        BenderError::LockDrift
    }
}

/// Synchronise dependencies: write a generated `Bender.yml`, run
/// `bender update`, and copy the resulting `Bender.lock` to
/// `<project_root>/Kiln.lock`.
pub fn update(project_root: &Path, manifest: &Manifest) -> Result<(), BenderError> {
    let bender_dir = bender_dir(project_root);
    std::fs::create_dir_all(&bender_dir).map_err(|source| BenderError::Io {
        path: bender_dir.clone(),
        source,
    })?;
    let bender_yml = generate_bender_yml(manifest, project_root)?;
    let yml_path = bender_dir.join("Bender.yml");
    std::fs::write(&yml_path, bender_yml).map_err(|source| BenderError::Io {
        path: yml_path,
        source,
    })?;

    runner::run_bender(&bender_dir, &["update"])?;

    let bender_lock = bender_dir.join("Bender.lock");
    let kiln_lock = project_root.join("Kiln.lock");
    if bender_lock.is_file() {
        std::fs::copy(&bender_lock, &kiln_lock).map_err(|source| BenderError::Io {
            path: kiln_lock,
            source,
        })?;
    }
    Ok(())
}

/// Resolve the dependency graph and return the per-package source list.
/// Runs `bender update` first so the lockfile is consistent.
pub fn resolve(project_root: &Path, manifest: &Manifest) -> Result<ResolvedSources, BenderError> {
    resolve_with_mode(project_root, manifest, LockMode::Free)
}

/// Like [`resolve`] but honours a [`LockMode`] for CI determinism.
///
/// - `Free`: behaves exactly like [`resolve`].
/// - `Locked`: snapshots `Kiln.lock`, runs `update`, errors on any
///   change. Restores the original lock contents if drift is detected.
/// - `Frozen`: skips `update` entirely; uses the existing lockfile.
///   Errors if no `Kiln.lock` exists.
pub fn resolve_with_mode(
    project_root: &Path,
    manifest: &Manifest,
    mode: LockMode,
) -> Result<ResolvedSources, BenderError> {
    update_with_mode(project_root, manifest, mode)?;
    let bender_dir = bender_dir(project_root);
    let output = runner::run_bender_capture(&bender_dir, &["sources", "--flatten"])?;
    sources::parse(&output.stdout)
}

/// Like [`update`] but honours a [`LockMode`].
pub fn update_with_mode(
    project_root: &Path,
    manifest: &Manifest,
    mode: LockMode,
) -> Result<(), BenderError> {
    match mode {
        LockMode::Free => update(project_root, manifest),
        LockMode::Locked => {
            let kiln_lock = project_root.join("Kiln.lock");
            let before = std::fs::read_to_string(&kiln_lock).ok();
            update(project_root, manifest)?;
            let after = std::fs::read_to_string(&kiln_lock).ok();
            if before != after {
                if let Some(prev) = before {
                    let _ = std::fs::write(&kiln_lock, prev);
                }
                return Err(BenderError::LockDrift);
            }
            Ok(())
        }
        LockMode::Frozen => {
            let kiln_lock = project_root.join("Kiln.lock");
            if !kiln_lock.is_file() && !manifest.dependencies.is_empty() {
                return Err(BenderError::FrozenWithoutLock { path: kiln_lock });
            }
            // Make sure Bender.yml + Bender.lock are in place so
            // `bender sources` works without going to the network.
            let bender_dir = bender_dir(project_root);
            std::fs::create_dir_all(&bender_dir).map_err(|source| BenderError::Io {
                path: bender_dir.clone(),
                source,
            })?;
            let bender_yml = generate_bender_yml(manifest, project_root)?;
            let yml_path = bender_dir.join("Bender.yml");
            std::fs::write(&yml_path, bender_yml).map_err(|source| BenderError::Io {
                path: yml_path,
                source,
            })?;
            if kiln_lock.is_file() {
                let bender_lock = bender_dir.join("Bender.lock");
                std::fs::copy(&kiln_lock, &bender_lock).map_err(|source| BenderError::Io {
                    path: bender_lock,
                    source,
                })?;
            }
            Ok(())
        }
    }
}

/// Return a stable, snapshot-friendly textual dependency tree.
pub fn tree(project_root: &Path, manifest: &Manifest) -> Result<String, BenderError> {
    tree_with_mode(project_root, manifest, LockMode::Free)
}

/// Like [`tree`] but honours a [`LockMode`].
pub fn tree_with_mode(
    project_root: &Path,
    manifest: &Manifest,
    mode: LockMode,
) -> Result<String, BenderError> {
    update_with_mode(project_root, manifest, mode)?;
    let bender_dir = bender_dir(project_root);
    let output = runner::run_bender_capture(&bender_dir, &["packages"])?;
    Ok(output.stdout)
}

/// `kiln add <name> ...` – mutate the manifest's `[dependencies]` table
/// in place, then re-resolve.
pub fn add(
    project_root: &Path,
    manifest_path: &Path,
    name: &str,
    dep: Dependency,
) -> Result<(), BenderError> {
    schema::edit_manifest(manifest_path, |table| {
        schema::insert_dependency(table, name, &dep);
    })?;
    let manifest = Manifest::load(manifest_path).map_err(BenderError::Manifest)?;
    update(project_root, &manifest)
}

/// `kiln remove <name>` – drop the named dep, then re-resolve.
pub fn remove(project_root: &Path, manifest_path: &Path, name: &str) -> Result<(), BenderError> {
    schema::edit_manifest(manifest_path, |table| {
        schema::remove_dependency(table, name);
    })?;
    let manifest = Manifest::load(manifest_path).map_err(BenderError::Manifest)?;
    update(project_root, &manifest)
}

fn bender_dir(project_root: &Path) -> PathBuf {
    project_root.join("target").join("kiln").join("bender")
}

/// Translate `Kiln.toml`'s `[dependencies]` into a Bender.yml document
/// that bender can consume. The root package's source files are listed
/// inline (resolved from the manifest's source globs) so that
/// `bender sources --flatten` returns *both* dep and root files in
/// dependency order.
fn generate_bender_yml(manifest: &Manifest, project_root: &Path) -> Result<String, BenderError> {
    use std::fmt::Write as _;
    let mut s = String::new();
    let _ = writeln!(s, "package:");
    let _ = writeln!(s, "  name: {}", manifest.package.name);
    let _ = writeln!(s);
    if !manifest.dependencies.is_empty() {
        let _ = writeln!(s, "dependencies:");
        let parsed = schema::parse_dependencies(&manifest.dependencies)?;
        for (name, dep) in &parsed {
            match dep {
                Dependency::Git {
                    git,
                    version,
                    rev,
                    branch,
                } => {
                    let _ = writeln!(s, "  {name}:");
                    let _ = writeln!(s, "    git: \"{git}\"");
                    if let Some(v) = version {
                        let _ = writeln!(s, "    version: \"{v}\"");
                    }
                    if let Some(r) = rev {
                        let _ = writeln!(s, "    rev: \"{r}\"");
                    }
                    if let Some(b) = branch {
                        let _ = writeln!(s, "    branch: \"{b}\"");
                    }
                }
                Dependency::Path { path } => {
                    let abs = if path.is_absolute() {
                        path.clone()
                    } else {
                        project_root.join(path)
                    };
                    let _ = writeln!(s, "  {name}:");
                    let _ = writeln!(s, "    path: \"{}\"", abs.display());
                }
            }
        }
        let _ = writeln!(s);
    }
    let root_sources = resolve_root_sources(manifest, project_root);
    if root_sources.is_empty() {
        let _ = writeln!(s, "sources: []");
    } else {
        let _ = writeln!(s, "sources:");
        for src in root_sources {
            let _ = writeln!(s, "  - \"{}\"", src.display());
        }
    }
    Ok(s)
}

fn resolve_root_sources(manifest: &Manifest, project_root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for raw in &manifest.design.sources {
        let pattern = if std::path::Path::new(raw).is_absolute() {
            raw.clone()
        } else {
            project_root.join(raw).to_string_lossy().into_owned()
        };
        let Ok(entries) = glob::glob(&pattern) else {
            continue;
        };
        for entry in entries.flatten() {
            if entry.is_file() {
                let canon = entry.canonicalize().unwrap_or(entry);
                if seen.insert(canon.clone()) {
                    out.push(canon);
                }
            }
        }
    }
    out
}
