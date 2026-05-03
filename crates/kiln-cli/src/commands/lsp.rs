//! `kiln lsp`: thin wrapper around `hudson-trading/slang-server`.
//!
//! Spawned by editors over stdio. Generates a project-rooted
//! `.slang/server.json` from `Kiln.toml` and exec's slang-server.
//! See ADR `docs/decisions/0004-lsp-strategy.md`.
//!
//! What we generate:
//!
//! ```jsonc
//! {
//!   "_generator": "kiln lsp",
//!   "flags": "-I path/to/include +define+FOO=1 --top tb -Wwidth-trunc",
//!   "index": [{ "dirs": ["src", "/abs/dep/path"] }]
//! }
//! ```
//!
//! `flags` mirrors what `kiln check` already passes to slang. `index.dirs`
//! is the union of the project's source-glob roots plus every
//! bender-resolved dependency package's directory, so symbol navigation
//! crosses into deps without further config.

use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, bail, Context, Result};

use kiln_build::SourceSet;
use kiln_core::{find_manifest, LintSeverity, Manifest};

use crate::reporter;

const ENV_OVERRIDE: &str = "KILN_SLANG_SERVER_PATH";
const KILN_GENERATOR_MARK: &str = "kiln lsp";

pub fn run() -> Result<()> {
    let cwd = std::env::current_dir().context("reading current directory")?;
    let manifest_path = find_manifest(&cwd)?;
    let project_root = manifest_path
        .parent()
        .ok_or_else(|| anyhow!("manifest path {} has no parent", manifest_path.display()))?
        .to_path_buf();
    let manifest = Manifest::load(&manifest_path)?;

    let bin = locate_slang_server()?;
    reporter::status(
        "Starting",
        format!("kiln lsp ({})", reporter::dim(&bin.display().to_string())),
    );

    let config = build_server_config(&project_root, &manifest)?;
    write_managed_config(&project_root, &config)?;

    // Replace this process with slang-server so the editor's stdin/
    // stdout pass through with no buffering. On non-Unix, fall back to
    // spawn+wait.
    exec_slang_server(&bin)?;
    Ok(())
}

/// Locate `slang-server` on `PATH` or `$KILN_SLANG_SERVER_PATH`.
pub(crate) fn locate_slang_server() -> Result<PathBuf> {
    if let Some(env) = std::env::var_os(ENV_OVERRIDE) {
        let p = PathBuf::from(env);
        if !p.is_file() {
            bail!(
                "${ENV_OVERRIDE} = {} but that file doesn't exist",
                p.display()
            );
        }
        return Ok(p);
    }
    let path_var = std::env::var_os("PATH").context("$PATH is not set")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join("slang-server");
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    Err(anyhow!(
        "could not find `slang-server` on PATH.\n\
         Install it with `kiln install-tools --tools slang-server`,\n\
         or set ${ENV_OVERRIDE} to a slang-server binary path."
    ))
}

/// The shape kiln writes to `.slang/server.json`. Public-ish for the unit
/// tests; not re-exported through `lib.rs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ServerConfig {
    pub flags: String,
    pub index_dirs: Vec<PathBuf>,
}

impl ServerConfig {
    /// Render to the JSON shape slang-server reads. We emit it by hand
    /// (rather than pulling in `serde_json`'s pretty printer) to keep
    /// the leading `_generator` field stable and human-diff-friendly.
    pub fn to_json(&self) -> String {
        let mut s = String::new();
        s.push_str("{\n");
        s.push_str(&format!("  \"_generator\": \"{KILN_GENERATOR_MARK}\",\n"));
        s.push_str(&format!("  \"flags\": {},\n", json_string(&self.flags)));
        s.push_str("  \"index\": [\n    {\n      \"dirs\": [");
        if self.index_dirs.is_empty() {
            s.push_str("]\n    }\n  ]\n");
        } else {
            s.push('\n');
            for (i, d) in self.index_dirs.iter().enumerate() {
                let comma = if i + 1 == self.index_dirs.len() {
                    ""
                } else {
                    ","
                };
                s.push_str(&format!(
                    "        {}{comma}\n",
                    json_string(&d.display().to_string())
                ));
            }
            s.push_str("      ]\n    }\n  ]\n");
        }
        s.push_str("}\n");
        s
    }
}

pub(crate) fn build_server_config(
    project_root: &Path,
    manifest: &Manifest,
) -> Result<ServerConfig> {
    let mut flag_parts: Vec<String> = Vec::new();

    // --top
    flag_parts.push("--top".to_string());
    flag_parts.push(manifest.design.top.clone());

    // -I per include dir (relative entries resolved against project root).
    for inc in &manifest.design.include_dirs {
        let resolved = if inc.is_absolute() {
            inc.clone()
        } else {
            project_root.join(inc)
        };
        flag_parts.push("-I".to_string());
        flag_parts.push(resolved.display().to_string());
    }

    // +define+ per define.
    for (k, v) in &manifest.design.defines {
        if v.is_empty() {
            flag_parts.push(format!("+define+{k}"));
        } else {
            flag_parts.push(format!("+define+{k}={v}"));
        }
    }

    // -W<id> per [lint] rule the user wants surfaced (warn or error).
    // Allow rules don't get -W; they're filtered post-hoc. This mirrors
    // kiln-lint::check.
    for (id, sev) in &manifest.lint.rules {
        if matches!(sev, LintSeverity::Error | LintSeverity::Warn) {
            flag_parts.push(format!("-W{id}"));
        }
    }

    // Build index.dirs: project source-glob roots + bender-resolved deps.
    let mut index_dirs: Vec<PathBuf> = Vec::new();
    for raw in &manifest.design.sources {
        // Take the prefix of the glob up to the first wildcard.
        let prefix = take_glob_prefix(raw);
        let resolved = if Path::new(&prefix).is_absolute() {
            PathBuf::from(prefix)
        } else if prefix.is_empty() {
            project_root.to_path_buf()
        } else {
            project_root.join(prefix)
        };
        push_unique(&mut index_dirs, resolved);
    }

    if !manifest.dependencies.is_empty() {
        // Best-effort: if bender resolution fails, log and continue with
        // just the project-local index. The LSP is still useful without
        // dep navigation.
        match kiln_deps::resolve(project_root, manifest) {
            Ok(resolved) => {
                for f in resolved.all_files() {
                    if let Some(parent) = f.parent() {
                        push_unique(&mut index_dirs, parent.to_path_buf());
                    }
                }
                for d in resolved.all_include_dirs() {
                    push_unique(&mut index_dirs, d.clone());
                    flag_parts.push("-I".to_string());
                    flag_parts.push(d.display().to_string());
                }
            }
            Err(e) => {
                reporter::warning(format!(
                    "bender resolution failed; LSP will not see dep symbols: {e:#}"
                ));
            }
        }
    }

    // Also include the resolved project-local source set so non-`src/`
    // file layouts still index (the glob-prefix heuristic above misses
    // `src/**/*.sv` style globs that resolve to multiple roots).
    if let Ok(set) = SourceSet::resolve(project_root, manifest) {
        for f in set.files() {
            if let Some(parent) = f.parent() {
                push_unique(&mut index_dirs, parent.to_path_buf());
            }
        }
    }

    Ok(ServerConfig {
        flags: flag_parts.join(" "),
        index_dirs,
    })
}

/// Write `.slang/server.json`. If the file exists without our generator
/// marker, refuse to clobber.
pub(crate) fn write_managed_config(project_root: &Path, cfg: &ServerConfig) -> Result<()> {
    let dir = project_root.join(".slang");
    let path = dir.join("server.json");
    if path.is_file() {
        let existing = std::fs::read_to_string(&path)
            .with_context(|| format!("reading existing {}", path.display()))?;
        if !existing.contains(KILN_GENERATOR_MARK) {
            bail!(
                "{} exists and does not look kiln-generated.\n\
                 Move or delete it (or rename your hand-written config) and \
                 re-run `kiln lsp`. kiln lsp regenerates this file on every \
                 launch from Kiln.toml.",
                path.display()
            );
        }
    }
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    std::fs::write(&path, cfg.to_json()).with_context(|| format!("writing {}", path.display()))?;
    reporter::debug("Wrote", path.display());
    Ok(())
}

/// On Unix, `exec(2)` replaces the kiln process with slang-server so
/// stdin/stdout pass through cleanly. On other platforms, spawn and
/// wait.
fn exec_slang_server(bin: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = Command::new(bin).exec();
        // If exec() returns at all, it's an error.
        Err(anyhow!("failed to exec {}: {err}", bin.display()))
    }
    #[cfg(not(unix))]
    {
        let status = Command::new(bin)
            .status()
            .with_context(|| format!("invoking {}", bin.display()))?;
        if !status.success() {
            bail!("slang-server exited with {:?}", status.code());
        }
        Ok(())
    }
}

fn take_glob_prefix(s: &str) -> String {
    let mut out = String::new();
    for ch in s.chars() {
        if matches!(ch, '*' | '?' | '[' | '{') {
            break;
        }
        out.push(ch);
    }
    // Trim trailing path separator if present.
    while out.ends_with('/') {
        out.pop();
    }
    out
}

fn push_unique(out: &mut Vec<PathBuf>, p: PathBuf) {
    if !out.contains(&p) {
        out.push(p);
    }
}

fn json_string(s: &str) -> String {
    // Minimal JSON string escaping: backslash, quote, control chars.
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest_for(toml: &str) -> Manifest {
        toml.parse().unwrap()
    }

    #[test]
    fn config_includes_top_and_includes() {
        let m = manifest_for(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "tb"
            include_dirs = ["inc", "/abs/inc"]
            "#,
        );
        let tmp = tempfile::tempdir().unwrap();
        let cfg = build_server_config(tmp.path(), &m).unwrap();
        assert!(cfg.flags.contains("--top tb"));
        assert!(cfg
            .flags
            .contains(&format!("-I {}/inc", tmp.path().display())));
        assert!(cfg.flags.contains("-I /abs/inc"));
    }

    #[test]
    fn config_includes_defines() {
        let m = manifest_for(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            defines = { WIDTH = "8", DEBUG = "" }
            "#,
        );
        let tmp = tempfile::tempdir().unwrap();
        let cfg = build_server_config(tmp.path(), &m).unwrap();
        assert!(cfg.flags.contains("+define+WIDTH=8"));
        // Empty value renders without `=`.
        assert!(cfg.flags.contains("+define+DEBUG"));
        assert!(!cfg.flags.contains("+define+DEBUG="));
    }

    #[test]
    fn config_promotes_warn_and_error_lint_rules() {
        let m = manifest_for(
            r#"
            [package]
            name = "p"
            version = "0.1.0"
            [design]
            top = "t"
            [lint]
            width-trunc = "error"
            unused-net = "warn"
            implicit-net = "off"
            "#,
        );
        let tmp = tempfile::tempdir().unwrap();
        let cfg = build_server_config(tmp.path(), &m).unwrap();
        assert!(cfg.flags.contains("-Wwidth-trunc"));
        assert!(cfg.flags.contains("-Wunused-net"));
        // `allow` must NOT appear; it's a post-filter, not a slang flag.
        assert!(!cfg.flags.contains("-Wimplicit-net"));
    }

    #[test]
    fn json_output_has_generator_marker() {
        let cfg = ServerConfig {
            flags: "--top t".to_string(),
            index_dirs: vec![PathBuf::from("/proj/src")],
        };
        let json = cfg.to_json();
        assert!(json.starts_with("{\n  \"_generator\": \"kiln lsp\""));
        assert!(json.contains("\"flags\": \"--top t\""));
        assert!(json.contains("\"/proj/src\""));
    }

    #[test]
    fn json_escapes_special_chars() {
        let cfg = ServerConfig {
            flags: "-D \"quoted\\value\"".to_string(),
            index_dirs: vec![],
        };
        let json = cfg.to_json();
        // The backslash and quote should be escaped.
        assert!(json.contains("\\\\"));
        assert!(json.contains("\\\""));
        // Round-trip via a real JSON parser to make sure the output is
        // valid even with weird input.
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
    }

    #[test]
    fn json_handles_empty_index() {
        let cfg = ServerConfig {
            flags: "".to_string(),
            index_dirs: vec![],
        };
        let json = cfg.to_json();
        let _: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(json.contains("\"dirs\": []"));
    }

    #[test]
    fn glob_prefix_extraction() {
        assert_eq!(take_glob_prefix("src/**/*.sv"), "src");
        assert_eq!(take_glob_prefix("src/*.sv"), "src");
        assert_eq!(take_glob_prefix("rtl/foo.sv"), "rtl/foo.sv");
        assert_eq!(take_glob_prefix("**/*.sv"), "");
        assert_eq!(take_glob_prefix("/abs/path/**/*.sv"), "/abs/path");
    }

    #[test]
    fn write_managed_config_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let cfg = ServerConfig {
            flags: "--top tb".to_string(),
            index_dirs: vec![tmp.path().join("src")],
        };
        write_managed_config(tmp.path(), &cfg).unwrap();
        let written = std::fs::read_to_string(tmp.path().join(".slang/server.json")).unwrap();
        assert!(written.contains("\"_generator\": \"kiln lsp\""));
        // Second write succeeds because the marker is present.
        write_managed_config(tmp.path(), &cfg).unwrap();
    }

    #[test]
    fn refuses_to_clobber_hand_written_config() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join(".slang")).unwrap();
        std::fs::write(
            tmp.path().join(".slang/server.json"),
            "{\n  \"flags\": \"hand-tuned\"\n}\n",
        )
        .unwrap();
        let cfg = ServerConfig {
            flags: String::new(),
            index_dirs: vec![],
        };
        let err = write_managed_config(tmp.path(), &cfg).unwrap_err();
        let msg = format!("{err:#}");
        assert!(msg.contains("kiln-generated") || msg.contains("kiln lsp"));
        // The hand-written config must be preserved.
        let after = std::fs::read_to_string(tmp.path().join(".slang/server.json")).unwrap();
        assert!(after.contains("hand-tuned"));
    }
}
