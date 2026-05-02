//! The [`Slang`] handle: locates the binary, validates its version, and
//! exposes a typed API on top of the CLI.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::ast::Ast;
use crate::compile::{CompileRequest, CompileResult};
use crate::diagnostic;
use crate::error::{install_hint, SlangError};
use crate::runner::run_slang;
use crate::version::SlangVersion;

/// Minimum slang version `slang-rs` supports. See ADR
/// `docs/decisions/0002-slang-version-policy.md`.
pub const MIN_VERSION: SlangVersion = SlangVersion::new(10, 0, 0);

const KILN_SLANG_PATH_ENV: &str = "KILN_SLANG_PATH";

/// Handle on the user's `slang` binary. Construct once per CLI run; cache
/// the binary path and version so subsequent calls don't re-`which`.
#[derive(Debug, Clone)]
pub struct Slang {
    binary: PathBuf,
    version: SlangVersion,
}

impl Slang {
    /// Discover the `slang` binary on `PATH` (or via `KILN_SLANG_PATH`),
    /// query its version, and validate it against [`MIN_VERSION`].
    pub fn new() -> Result<Self, SlangError> {
        let binary = match std::env::var_os(KILN_SLANG_PATH_ENV) {
            Some(p) if !p.is_empty() => PathBuf::from(p),
            _ => locate_on_path("slang").ok_or_else(|| SlangError::BinaryNotFound {
                reason: "no `slang` on PATH".to_string(),
                install_hint: install_hint(),
            })?,
        };
        Self::with_path(binary)
    }

    /// Construct a handle with an explicit `slang` binary path. The
    /// binary is invoked with `--version` to discover the version, and the
    /// result is validated against [`MIN_VERSION`].
    pub fn with_path(binary: impl Into<PathBuf>) -> Result<Self, SlangError> {
        let binary = binary.into();
        if !binary.is_file() {
            return Err(SlangError::BinaryNotFound {
                reason: format!("`{}` is not a file", binary.display()),
                install_hint: install_hint(),
            });
        }
        let result = run_slang(&binary, &[OsString::from("--version")])?;
        if result.exit_code != Some(0) {
            return Err(SlangError::NonZeroExit {
                code: result.exit_code.unwrap_or(-1),
                stderr: result.stderr,
            });
        }
        let version = SlangVersion::parse(&result.stdout)?;
        let handle = Slang { binary, version };
        handle.check_version()?;
        Ok(handle)
    }

    /// Returns the resolved binary path.
    pub fn binary(&self) -> &Path {
        &self.binary
    }

    /// Returns the discovered slang version.
    pub fn version(&self) -> &SlangVersion {
        &self.version
    }

    /// Validate the discovered version against [`MIN_VERSION`].
    pub fn check_version(&self) -> Result<(), SlangError> {
        let v = (self.version.major, self.version.minor, self.version.patch);
        let min = (MIN_VERSION.major, MIN_VERSION.minor, MIN_VERSION.patch);
        if v < min {
            Err(SlangError::UnsupportedVersion {
                path: self.binary.clone(),
                found: self.version.clone(),
                required: MIN_VERSION,
                install_hint: install_hint(),
            })
        } else {
            Ok(())
        }
    }

    /// Run slang with the given request. Diagnostics are always returned;
    /// the AST is returned only when [`crate::CompileRequestBuilder::want_ast`]
    /// is set.
    ///
    /// Slang's exit code is *not* converted into an error when slang
    /// successfully reports diagnostics: a syntax error in the user's
    /// design produces `Severity::Error` entries in `diagnostics`, not a
    /// [`SlangError`]. A [`SlangError::NonZeroExit`] is returned only when
    /// slang exited non-zero *and* failed to write a diagnostic file.
    pub fn compile(&self, req: &CompileRequest) -> Result<CompileResult, SlangError> {
        let tmp = tempdir_in_target()?;
        let diag_path = tmp.path().join("diag.json");
        let ast_path = if req.want_ast {
            Some(tmp.path().join("ast.json"))
        } else {
            None
        };

        let args = build_args(req, &diag_path, ast_path.as_deref());
        let result = run_slang(&self.binary, &args)?;

        let diagnostics = match std::fs::read_to_string(&diag_path) {
            Ok(text) => diagnostic::parse_diagnostics(&text)?,
            Err(_) => {
                // Slang did not even produce a diag file. That's a hard
                // failure; surface stdout/stderr so the user can see why.
                let combined = if !result.stderr.trim().is_empty() {
                    result.stderr
                } else {
                    result.stdout
                };
                return Err(SlangError::NonZeroExit {
                    code: result.exit_code.unwrap_or(-1),
                    stderr: combined,
                });
            }
        };

        let ast = match ast_path {
            Some(p) => match std::fs::read_to_string(&p) {
                Ok(text) if !text.trim().is_empty() => Some(Ast::parse(&text)?),
                _ => None,
            },
            None => None,
        };

        Ok(CompileResult {
            ast,
            diagnostics,
            exit_code: result.exit_code,
        })
    }
}

/// Build the slang argument vector for a request, with diag/ast outputs
/// pinned to the given paths.
fn build_args(req: &CompileRequest, diag_path: &Path, ast_path: Option<&Path>) -> Vec<OsString> {
    let mut args: Vec<OsString> = Vec::new();
    args.push(OsString::from("--diag-json"));
    args.push(diag_path.as_os_str().to_owned());
    if let Some(p) = ast_path {
        args.push(OsString::from("--ast-json"));
        args.push(p.as_os_str().to_owned());
    }
    if let Some(top) = &req.top {
        args.push(OsString::from("--top"));
        args.push(OsString::from(top));
    }
    if let Some(std) = &req.std {
        args.push(OsString::from("--std"));
        args.push(OsString::from(std.as_flag()));
    }
    if req.parse_only {
        args.push(OsString::from("--parse-only"));
    }
    for inc in &req.include_dirs {
        args.push(OsString::from("-I"));
        args.push(inc.as_os_str().to_owned());
    }
    for (k, v) in &req.defines {
        args.push(OsString::from("-D"));
        let combined = if v.is_empty() {
            k.clone()
        } else {
            format!("{k}={v}")
        };
        args.push(OsString::from(combined));
    }
    for extra in &req.extra_args {
        args.push(OsString::from(extra));
    }
    for src in &req.sources {
        args.push(src.as_os_str().to_owned());
    }
    args
}

/// Walk `$PATH` looking for a binary called `name`.
fn locate_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable(&candidate) {
            return Some(candidate);
        }
        // On macOS / Linux, no extension. On Windows we'd loop over PATHEXT;
        // not relevant for our supported platforms today.
    }
    None
}

#[cfg(unix)]
fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file()
        && p.metadata()
            .map(|m| m.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(p: &Path) -> bool {
    p.is_file()
}

/// Create a tempdir for slang JSON outputs. Uses the system tempdir; the
/// `_in_target` name is aspirational; once `kiln-build` lands a build
/// cache (M2), we'll point this at `target/kiln/` for reuse.
fn tempdir_in_target() -> Result<tempfile::TempDir, SlangError> {
    tempfile::tempdir().map_err(|e| SlangError::Invocation {
        path: PathBuf::new(),
        source: e,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_args_minimal() {
        let req = CompileRequest::builder().source("foo.sv").build();
        let args = build_args(&req, Path::new("/tmp/diag.json"), None);
        let strs: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            strs,
            vec![
                "--diag-json".to_string(),
                "/tmp/diag.json".into(),
                "foo.sv".into(),
            ]
        );
    }

    #[test]
    fn build_args_full() {
        let req = CompileRequest::builder()
            .source("a.sv")
            .source("b.sv")
            .include_dir("inc")
            .define("X", "1")
            .define("Y", "")
            .top("top")
            .std(crate::SvStandard::Sv2017)
            .parse_only(true)
            .want_ast(true)
            .extra_arg("-Wwidth-trunc")
            .build();
        let args = build_args(
            &req,
            Path::new("/tmp/d.json"),
            Some(Path::new("/tmp/a.json")),
        );
        let strs: Vec<String> = args
            .iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        let expected: Vec<String> = [
            "--diag-json",
            "/tmp/d.json",
            "--ast-json",
            "/tmp/a.json",
            "--top",
            "top",
            "--std",
            "1800-2017",
            "--parse-only",
            "-I",
            "inc",
            "-D",
            "X=1",
            "-D",
            "Y",
            "-Wwidth-trunc",
            "a.sv",
            "b.sv",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        assert_eq!(strs, expected);
    }

    #[test]
    fn missing_binary_yields_clear_error() {
        let err = Slang::with_path("/nonexistent/path/to/slang").unwrap_err();
        match &err {
            SlangError::BinaryNotFound { .. } => {}
            other => panic!("unexpected error: {other:?}"),
        }
        let msg = err.to_string();
        // The message should name the binary, name the env var, and end with
        // platform-specific install instructions.
        assert!(msg.contains("KILN_SLANG_PATH"));
        assert!(msg.contains("slang"));
        // Snapshot the error message with the platform-specific install hint
        // redacted to a stable token, so this assertion isn't macOS-only.
        let normalized = redact_install_hint(&msg);
        insta::assert_snapshot!("missing_binary_error", normalized);
    }

    fn redact_install_hint(s: &str) -> String {
        let mut lines: Vec<&str> = s.lines().collect();
        // Drop everything after the first install-hint cue line.
        if let Some(idx) = lines.iter().position(|l| {
            l.starts_with("On macOS")
                || l.starts_with("On Linux")
                || l.starts_with("Build from source")
        }) {
            lines.truncate(idx);
            lines.push("[install hint redacted for snapshot]");
        }
        lines.join("\n")
    }
}
