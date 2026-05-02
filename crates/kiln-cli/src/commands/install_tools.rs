//! `kiln install-tools`: fetch / build the external tools kiln drives.
//!
//! Three tools install cleanly from prebuilt binaries or pure-Rust crates:
//!
//! - **bender** — `cargo install bender --version 0.31.0 --locked`.
//! - **verible** — prebuilt tarball from chipsalliance/verible releases.
//! - **surfer** — `cargo install --locked --git https://gitlab.com/surfer-project/surfer.git`.
//!
//! Two need a C++ build:
//!
//! - **slang** — cmake + C++17 compiler + ninja.
//! - **verilator** — autoconf + C++ + flex + bison + make + libfl-dev.
//!
//! By default `kiln install-tools` installs the three easy ones and prints
//! instructions for the two source-only ones. `--build-from-source` opts
//! into building slang and verilator too. Before any source build, we
//! pre-flight check for the C++ build tools and print a platform-specific
//! `brew install …` / `apt install …` command if anything's missing.
//!
//! Install root: `$KILN_TOOLS_DIR` if set, else `$HOME/.local/share/kiln`.
//! Verible and slang/verilator binaries are symlinked into
//! `<root>/bin/` so users only add one directory to PATH. Bender and
//! surfer install via cargo, which already places binaries on PATH at
//! `$CARGO_HOME/bin`.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use crate::reporter;

const VERIBLE_TAG: &str = "v0.0-4053-g89d4d98a";
const BENDER_VERSION: &str = "0.31.0";
const SLANG_TAG: &str = "v10.0";
const VERILATOR_TAG: &str = "v5.026";
const SLANG_SERVER_TAG: &str = "v0.2.5";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolName {
    Bender,
    Verible,
    Surfer,
    SlangServer,
    Slang,
    Verilator,
}

impl ToolName {
    fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "bender" => Some(Self::Bender),
            "verible" | "verible-verilog-format" => Some(Self::Verible),
            "surfer" => Some(Self::Surfer),
            "slang-server" | "slang_server" => Some(Self::SlangServer),
            "slang" => Some(Self::Slang),
            "verilator" => Some(Self::Verilator),
            _ => None,
        }
    }

    fn label(&self) -> &'static str {
        match self {
            Self::Bender => "bender",
            Self::Verible => "verible",
            Self::Surfer => "surfer",
            Self::SlangServer => "slang-server",
            Self::Slang => "slang",
            Self::Verilator => "verilator",
        }
    }

    /// Binary name to look up on `PATH` when checking "is this already
    /// installed?". For verible we check the format binary specifically,
    /// since that's the only one kiln-fmt invokes today.
    fn binary(&self) -> &'static str {
        match self {
            Self::Bender => "bender",
            Self::Verible => "verible-verilog-format",
            Self::Surfer => "surfer",
            Self::SlangServer => "slang-server",
            Self::Slang => "slang",
            Self::Verilator => "verilator",
        }
    }

    fn requires_source_build(&self) -> bool {
        matches!(self, Self::Slang | Self::Verilator)
    }
}

pub fn run(
    requested: Option<Vec<String>>,
    build_from_source: bool,
    prefix: Option<PathBuf>,
) -> Result<()> {
    let prefix = resolve_prefix(prefix)?;
    std::fs::create_dir_all(prefix.join("bin"))
        .with_context(|| format!("creating {}", prefix.join("bin").display()))?;

    let tools = parse_tool_list(requested)?;

    reporter::status(
        "Installing",
        format!(
            "{} tool(s) into {}",
            tools.len(),
            reporter::dim(&prefix.display().to_string())
        ),
    );

    let mut succeeded = 0;
    let mut skipped = 0;
    let mut deferred: Vec<ToolName> = Vec::new();

    for tool in &tools {
        if let Some(path) = locate(tool.binary()) {
            reporter::info(
                "Already",
                format!(
                    "`{}` is on PATH at {}",
                    tool.label(),
                    reporter::dim(&path.display().to_string())
                ),
            );
            skipped += 1;
            continue;
        }

        if tool.requires_source_build() && !build_from_source {
            deferred.push(*tool);
            continue;
        }

        match install_one(*tool, &prefix) {
            Ok(()) => succeeded += 1,
            Err(e) => {
                reporter::error(format!("failed to install `{}`: {e:#}", tool.label()));
                let mut source = e.source();
                while let Some(s) = source {
                    eprintln!("       {} {s}", reporter::dim("↳"));
                    source = s.source();
                }
            }
        }
    }

    if !deferred.is_empty() {
        eprintln!();
        reporter::info(
            "Deferred",
            format!(
                "{} tool(s) need a C++ build; pass --build-from-source to opt in",
                deferred.len()
            ),
        );
        for t in &deferred {
            print_source_recipe(*t);
        }
    }

    let bin_dir = prefix.join("bin");
    if path_contains(&bin_dir) {
        reporter::status(
            "Result",
            reporter::green(&format!("{succeeded} installed, {skipped} already present")),
        );
    } else {
        reporter::status(
            "Result",
            reporter::green(&format!("{succeeded} installed, {skipped} already present")),
        );
        reporter::info(
            "Add to PATH",
            format!(
                "{} (e.g. `export PATH={}:$PATH` in your shell rc)",
                reporter::dim(&bin_dir.display().to_string()),
                bin_dir.display()
            ),
        );
    }
    Ok(())
}

fn resolve_prefix(prefix: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(p) = prefix {
        return Ok(p);
    }
    if let Ok(env) = std::env::var("KILN_TOOLS_DIR") {
        return Ok(PathBuf::from(env));
    }
    let home = std::env::var("HOME").context("$HOME is not set; pass --prefix")?;
    Ok(PathBuf::from(home).join(".local/share/kiln"))
}

fn parse_tool_list(requested: Option<Vec<String>>) -> Result<Vec<ToolName>> {
    let default = vec![
        ToolName::Bender,
        ToolName::Verible,
        ToolName::Surfer,
        ToolName::SlangServer,
        ToolName::Slang,
        ToolName::Verilator,
    ];
    match requested {
        None => Ok(default),
        Some(list) => list
            .iter()
            .map(|s| {
                ToolName::from_str(s).ok_or_else(|| {
                    anyhow!(
                        "unknown tool `{s}`; choose from: bender, verible, surfer, slang-server, slang, verilator"
                    )
                })
            })
            .collect(),
    }
}

fn install_one(tool: ToolName, prefix: &Path) -> Result<()> {
    match tool {
        ToolName::Bender => install_via_cargo("bender", BENDER_VERSION),
        ToolName::Surfer => install_surfer(),
        ToolName::Verible => install_verible(prefix),
        ToolName::SlangServer => install_slang_server(prefix),
        ToolName::Slang => install_slang(prefix),
        ToolName::Verilator => install_verilator(prefix),
    }
}

// ---------- bender / surfer (cargo install) ----------

fn install_via_cargo(crate_name: &str, version: &str) -> Result<()> {
    reporter::status(
        "Installing",
        format!("`{crate_name}` via cargo install (version {version})"),
    );
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .args(["install", "--locked", crate_name, "--version", version])
        .status()
        .with_context(|| format!("invoking {}", cargo.to_string_lossy()))?;
    if !status.success() {
        bail!(
            "`cargo install {crate_name}` exited with {:?}",
            status.code()
        );
    }
    reporter::status("Installed", format!("`{crate_name}`"));
    Ok(())
}

fn install_surfer() -> Result<()> {
    // surfer is published on crates.io as `surfer`. If the published
    // package ever lags behind the gitlab tip, users can override with
    // `--git`-based cargo install. The default published version is fine
    // for what `kiln wave` does.
    reporter::status("Installing", "`surfer` via cargo install");
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(&cargo)
        .args(["install", "--locked", "surfer"])
        .status()
        .with_context(|| format!("invoking {}", cargo.to_string_lossy()))?;
    if !status.success() {
        bail!(
            "`cargo install surfer` exited with {:?}; \
             see https://gitlab.com/surfer-project/surfer for build instructions",
            status.code()
        );
    }
    reporter::status("Installed", "`surfer`");
    Ok(())
}

// ---------- verible (prebuilt tarball) ----------

fn install_verible(prefix: &Path) -> Result<()> {
    let triple = host_target_for_verible()?;
    let url = format!(
        "https://github.com/chipsalliance/verible/releases/download/{VERIBLE_TAG}/verible-{VERIBLE_TAG}-{triple}.tar.gz"
    );
    let dest = prefix.join("verible");
    reporter::status("Downloading", format!("`verible` {VERIBLE_TAG} ({triple})"));
    download_and_extract(&url, &dest, /* strip_components */ 1)?;

    // Symlink the format binary into <prefix>/bin/.
    symlink_into_bin(&dest.join("bin/verible-verilog-format"), prefix)?;
    reporter::status(
        "Installed",
        format!(
            "`verible-verilog-format` at {}",
            reporter::dim(
                &prefix
                    .join("bin/verible-verilog-format")
                    .display()
                    .to_string()
            )
        ),
    );
    Ok(())
}

fn host_target_for_verible() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("macOS")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        Ok("linux-static-x86_64")
    } else {
        Err(anyhow!(
            "no verible prebuilt for this platform; install manually from \
             https://github.com/chipsalliance/verible/releases"
        ))
    }
}

// ---------- slang-server (prebuilt tarball) ----------

fn install_slang_server(prefix: &Path) -> Result<()> {
    let asset = host_asset_for_slang_server()?;
    let url = format!(
        "https://github.com/hudson-trading/slang-server/releases/download/{SLANG_SERVER_TAG}/{asset}"
    );
    let dest = prefix.join("slang-server");
    reporter::status(
        "Downloading",
        format!("`slang-server` {SLANG_SERVER_TAG} ({asset})"),
    );
    download_and_extract(&url, &dest, /* strip_components */ 0)?;

    // The release tarball lays out `slang-server` directly in the dest
    // root (no nested `bin/` like verible). Locate it and symlink.
    let bin = find_slang_server_binary(&dest)?;
    symlink_into_bin(&bin, prefix)?;
    reporter::status(
        "Installed",
        format!(
            "`slang-server` at {}",
            reporter::dim(&prefix.join("bin/slang-server").display().to_string())
        ),
    );
    Ok(())
}

fn host_asset_for_slang_server() -> Result<&'static str> {
    if cfg!(target_os = "macos") {
        Ok("slang-server-macos.tar.gz")
    } else if cfg!(target_os = "linux") && cfg!(target_arch = "x86_64") {
        // Prefer the gcc build on Linux (more compatible glibc baseline).
        Ok("slang-server-linux-x64-gcc.tar.gz")
    } else if cfg!(target_os = "windows") {
        Ok("slang-server-windows-x64.zip")
    } else {
        Err(anyhow!(
            "no slang-server prebuilt for this platform; install from \
             https://github.com/hudson-trading/slang-server/releases"
        ))
    }
}

/// The slang-server release tarballs put the binary either at the root
/// of the archive or under a single nested directory. Search for it.
fn find_slang_server_binary(root: &Path) -> Result<PathBuf> {
    fn walk(dir: &Path, depth: usize) -> Option<PathBuf> {
        if depth > 3 {
            return None;
        }
        let entries = std::fs::read_dir(dir).ok()?;
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_file() && p.file_name().and_then(|s| s.to_str()) == Some("slang-server") {
                return Some(p);
            }
            if p.is_dir() {
                if let Some(found) = walk(&p, depth + 1) {
                    return Some(found);
                }
            }
        }
        None
    }
    walk(root, 0).ok_or_else(|| {
        anyhow!(
            "could not find `slang-server` binary inside extracted tarball at {}",
            root.display()
        )
    })
}

// ---------- slang (cmake from source) ----------

fn install_slang(prefix: &Path) -> Result<()> {
    check_build_deps(&["cmake", "ninja", "g++"], "slang")?;
    reporter::status("Building", format!("`slang` {SLANG_TAG} from source"));
    let work = prefix.join("build/slang-src");
    let _ = std::fs::remove_dir_all(&work);
    git_clone(
        "https://github.com/MikePopoloski/slang.git",
        SLANG_TAG,
        &work,
    )?;
    run_in(
        "cmake",
        &["-B", "build", "-DCMAKE_BUILD_TYPE=Release"],
        &work,
    )?;
    run_in("cmake", &["--build", "build", "-j"], &work)?;
    let built = work.join("build/bin/slang");
    let installed = prefix.join("slang/bin/slang");
    if let Some(p) = installed.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::copy(&built, &installed)
        .with_context(|| format!("copying {} -> {}", built.display(), installed.display()))?;
    symlink_into_bin(&installed, prefix)?;
    reporter::status(
        "Installed",
        format!(
            "`slang` at {}",
            reporter::dim(&prefix.join("bin/slang").display().to_string())
        ),
    );
    Ok(())
}

// ---------- verilator (autoconf from source) ----------

fn install_verilator(prefix: &Path) -> Result<()> {
    check_build_deps(&["autoconf", "g++", "flex", "bison", "make"], "verilator")?;
    reporter::status(
        "Building",
        format!("`verilator` {VERILATOR_TAG} from source (this takes ~5 min)"),
    );
    let work = prefix.join("build/verilator-src");
    let _ = std::fs::remove_dir_all(&work);
    git_clone(
        "https://github.com/verilator/verilator.git",
        VERILATOR_TAG,
        &work,
    )?;
    run_in("autoconf", &[], &work)?;
    let install_dir = prefix.join("verilator");
    run_in(
        "sh",
        &[
            "-c",
            &format!("./configure --prefix={}", install_dir.display()),
        ],
        &work,
    )?;
    let nproc = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .to_string();
    run_in("make", &["-j", &nproc], &work)?;
    run_in("make", &["install"], &work)?;
    symlink_into_bin(&install_dir.join("bin/verilator"), prefix)?;
    reporter::status(
        "Installed",
        format!(
            "`verilator` at {}",
            reporter::dim(&prefix.join("bin/verilator").display().to_string())
        ),
    );
    Ok(())
}

// ---------- helpers ----------

/// Verify each binary in `deps` is on PATH. If any are missing, bail with
/// a platform-specific install hint.
fn check_build_deps(deps: &[&str], for_tool: &str) -> Result<()> {
    let missing: Vec<&&str> = deps.iter().filter(|d| locate(d).is_none()).collect();
    if missing.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = missing.iter().map(|s| s.to_string()).collect();
    let install_cmd = if cfg!(target_os = "macos") {
        format!("brew install {}", join_brew_pkgs(&names))
    } else if cfg!(target_os = "linux") {
        format!("sudo apt-get install -y {}", join_apt_pkgs(&names))
    } else {
        format!("install: {}", names.join(" "))
    };
    bail!(
        "missing build tools for `{for_tool}`: {}\n\
         Install them with:\n    {install_cmd}",
        names.join(", ")
    );
}

/// apt sometimes packages with different names than the binary. Map a
/// few that diverge.
fn join_apt_pkgs(names: &[String]) -> String {
    let mut pkgs: Vec<String> = Vec::new();
    for n in names {
        match n.as_str() {
            "g++" => {
                pkgs.push("g++".to_string());
                pkgs.push("libfl-dev".to_string());
                pkgs.push("libfl2".to_string());
            }
            "ninja" => pkgs.push("ninja-build".to_string()),
            other => pkgs.push(other.to_string()),
        }
    }
    pkgs.sort();
    pkgs.dedup();
    pkgs.join(" ")
}

fn join_brew_pkgs(names: &[String]) -> String {
    let mut pkgs: Vec<String> = Vec::new();
    for n in names {
        match n.as_str() {
            // macOS ships clang via Xcode CLI tools, not via brew.
            "g++" => pkgs.push(
                "\n     (run `xcode-select --install` for the C++ compiler)\n     ".to_string(),
            ),
            other => pkgs.push(other.to_string()),
        }
    }
    pkgs.join(" ")
}

fn print_source_recipe(tool: ToolName) {
    let (label, repo, tag, build_deps) = match tool {
        ToolName::Slang => (
            "slang",
            "https://github.com/MikePopoloski/slang",
            SLANG_TAG,
            "cmake, ninja, a C++17 compiler",
        ),
        ToolName::Verilator => (
            "verilator",
            "https://github.com/verilator/verilator",
            VERILATOR_TAG,
            "autoconf, g++, flex, bison, make, libfl-dev",
        ),
        _ => return,
    };
    eprintln!("\n  {} `{label}` {tag} from {repo}", reporter::dim("→"));
    eprintln!("    build deps: {build_deps}");
    eprintln!(
        "    or: {} kiln install-tools --build-from-source --tools {label}",
        reporter::dim("$")
    );
}

fn locate(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn path_contains(dir: &Path) -> bool {
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    let path_var = match std::env::var_os("PATH") {
        Some(v) => v,
        None => return false,
    };
    std::env::split_paths(&path_var).any(|d| {
        let dc = d.canonicalize().unwrap_or(d);
        dc == canon
    })
}

fn download_and_extract(url: &str, dest: &Path, strip_components: usize) -> Result<()> {
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest)?;
    let tmp = std::env::temp_dir().join(format!("kiln-dl-{}.tar.gz", std::process::id()));
    let _ = std::fs::remove_file(&tmp);

    reporter::debug("Fetching", url);
    let curl = Command::new("curl")
        .args(["-sSfL", "-o"])
        .arg(&tmp)
        .arg(url)
        .status()
        .context("invoking curl (is it installed?)")?;
    if !curl.success() {
        bail!("curl failed for {url}");
    }
    let tar = Command::new("tar")
        .arg("-xzf")
        .arg(&tmp)
        .arg("-C")
        .arg(dest)
        .arg(format!("--strip-components={strip_components}"))
        .status()
        .context("invoking tar")?;
    if !tar.success() {
        bail!("tar failed unpacking {}", tmp.display());
    }
    let _ = std::fs::remove_file(&tmp);
    Ok(())
}

fn git_clone(repo: &str, tag: &str, dest: &Path) -> Result<()> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let status = Command::new("git")
        .args(["clone", "--depth", "1", "--branch", tag, repo])
        .arg(dest)
        .status()
        .context("invoking git")?;
    if !status.success() {
        bail!("git clone failed for {repo}@{tag}");
    }
    Ok(())
}

fn run_in(cmd: &str, args: &[&str], cwd: &Path) -> Result<()> {
    let status = Command::new(cmd)
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("invoking {cmd}"))?;
    if !status.success() {
        bail!("{cmd} {} failed in {}", args.join(" "), cwd.display());
    }
    Ok(())
}

fn symlink_into_bin(target: &Path, prefix: &Path) -> Result<()> {
    let bin_dir = prefix.join("bin");
    std::fs::create_dir_all(&bin_dir)?;
    let link_name = target
        .file_name()
        .ok_or_else(|| anyhow!("target {} has no file name", target.display()))?;
    let link = bin_dir.join(link_name);
    let _ = std::fs::remove_file(&link);
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, &link)
            .with_context(|| format!("symlinking {} -> {}", link.display(), target.display()))?;
    }
    #[cfg(not(unix))]
    {
        std::fs::copy(target, &link)
            .with_context(|| format!("copying {} -> {}", target.display(), link.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_tool_names() {
        assert_eq!(ToolName::from_str("bender"), Some(ToolName::Bender));
        assert_eq!(ToolName::from_str("Verible"), Some(ToolName::Verible));
        assert_eq!(
            ToolName::from_str("verible-verilog-format"),
            Some(ToolName::Verible)
        );
        assert_eq!(ToolName::from_str("verilator"), Some(ToolName::Verilator));
        assert_eq!(ToolName::from_str("nope"), None);
    }

    #[test]
    fn requires_source_build() {
        assert!(ToolName::Slang.requires_source_build());
        assert!(ToolName::Verilator.requires_source_build());
        assert!(!ToolName::Bender.requires_source_build());
        assert!(!ToolName::Verible.requires_source_build());
        assert!(!ToolName::Surfer.requires_source_build());
    }

    #[test]
    fn parse_tool_list_default_is_all() {
        let list = parse_tool_list(None).unwrap();
        assert_eq!(list.len(), 6);
    }

    #[test]
    fn parse_tool_list_filters() {
        let list = parse_tool_list(Some(vec!["bender".into(), "slang".into()])).unwrap();
        assert_eq!(list, vec![ToolName::Bender, ToolName::Slang]);
    }

    #[test]
    fn parse_tool_list_rejects_unknown() {
        let err = parse_tool_list(Some(vec!["bogus".into()])).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("bogus"));
        assert!(msg.contains("bender"));
    }

    #[test]
    fn apt_pkg_join_dedupes_and_sorts() {
        let pkgs = join_apt_pkgs(&["g++".into(), "ninja".into(), "g++".into(), "make".into()]);
        // g++ pulls in libfl-dev + libfl2 too; sorted + deduped.
        assert!(pkgs.contains("g++"));
        assert!(pkgs.contains("libfl-dev"));
        assert!(pkgs.contains("ninja-build"));
        assert!(pkgs.contains("make"));
    }

    #[test]
    fn locate_finds_known_binary_and_misses_unknown() {
        // /bin/sh exists on every supported host.
        assert!(locate("sh").is_some());
        assert!(locate("definitely_not_a_real_binary_xyz_123").is_none());
    }

    #[test]
    fn resolve_prefix_uses_arg_when_given() {
        let p = resolve_prefix(Some(PathBuf::from("/tmp/explicit"))).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/explicit"));
    }

    #[test]
    fn resolve_prefix_uses_env_when_set() {
        let prev = std::env::var_os("KILN_TOOLS_DIR");
        // SAFETY: tests in this module are not run concurrently with
        // anything that reads env in this process.
        unsafe {
            std::env::set_var("KILN_TOOLS_DIR", "/tmp/from-env");
        }
        let p = resolve_prefix(None).unwrap();
        assert_eq!(p, PathBuf::from("/tmp/from-env"));
        unsafe {
            match prev {
                Some(v) => std::env::set_var("KILN_TOOLS_DIR", v),
                None => std::env::remove_var("KILN_TOOLS_DIR"),
            }
        }
    }
}
