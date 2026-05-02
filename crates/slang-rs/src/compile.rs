//! `CompileRequest` builder and `CompileResult` types.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::ast::Ast;
use crate::diagnostic::Diagnostic;

/// SystemVerilog language standard, mapped onto slang's `--std` flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SvStandard {
    Verilog2005,
    Sv2017,
    Sv2023,
    /// Slang's `latest` keyword. Whatever the installed slang considers
    /// the latest standard. Useful when you don't care about pinning.
    Latest,
}

impl SvStandard {
    /// String form expected by `slang --std`.
    pub(crate) fn as_flag(&self) -> &'static str {
        match self {
            SvStandard::Verilog2005 => "1364-2005",
            SvStandard::Sv2017 => "1800-2017",
            SvStandard::Sv2023 => "1800-2023",
            SvStandard::Latest => "latest",
        }
    }
}

/// What slang should do with a compilation request.
#[derive(Debug, Clone)]
pub struct CompileRequest {
    pub(crate) sources: Vec<PathBuf>,
    pub(crate) include_dirs: Vec<PathBuf>,
    pub(crate) defines: BTreeMap<String, String>,
    pub(crate) top: Option<String>,
    pub(crate) std: Option<SvStandard>,
    pub(crate) parse_only: bool,
    pub(crate) want_ast: bool,
    pub(crate) extra_args: Vec<String>,
}

impl CompileRequest {
    /// Returns a new builder. Equivalent to [`CompileRequestBuilder::new`].
    pub fn builder() -> CompileRequestBuilder {
        CompileRequestBuilder::new()
    }
}

/// Builder for [`CompileRequest`].
#[derive(Debug, Clone, Default)]
pub struct CompileRequestBuilder {
    sources: Vec<PathBuf>,
    include_dirs: Vec<PathBuf>,
    defines: BTreeMap<String, String>,
    top: Option<String>,
    std: Option<SvStandard>,
    parse_only: bool,
    want_ast: bool,
    extra_args: Vec<String>,
}

impl CompileRequestBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add one source file.
    pub fn source(mut self, path: impl Into<PathBuf>) -> Self {
        self.sources.push(path.into());
        self
    }

    /// Add several source files. Order is preserved.
    pub fn sources<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.sources.extend(paths.into_iter().map(Into::into));
        self
    }

    pub fn include_dir(mut self, path: impl Into<PathBuf>) -> Self {
        self.include_dirs.push(path.into());
        self
    }

    pub fn include_dirs<I, P>(mut self, paths: I) -> Self
    where
        I: IntoIterator<Item = P>,
        P: Into<PathBuf>,
    {
        self.include_dirs.extend(paths.into_iter().map(Into::into));
        self
    }

    pub fn define(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.defines.insert(name.into(), value.into());
        self
    }

    pub fn defines<I, K, V>(mut self, items: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        self.defines
            .extend(items.into_iter().map(|(k, v)| (k.into(), v.into())));
        self
    }

    pub fn top(mut self, name: impl Into<String>) -> Self {
        self.top = Some(name.into());
        self
    }

    pub fn std(mut self, std: SvStandard) -> Self {
        self.std = Some(std);
        self
    }

    /// Skip elaboration and type-checking; only parse. Maps onto
    /// `--parse-only`.
    pub fn parse_only(mut self, on: bool) -> Self {
        self.parse_only = on;
        self
    }

    /// Request the elaborated AST. When false, [`CompileResult::ast`] is
    /// `None` and slang is invoked without `--ast-json`. Default: false,
    /// because dumping the AST is expensive on large designs.
    pub fn want_ast(mut self, on: bool) -> Self {
        self.want_ast = on;
        self
    }

    /// Append a verbatim slang argument. Use sparingly: prefer typed setters.
    pub fn extra_arg(mut self, arg: impl Into<String>) -> Self {
        self.extra_args.push(arg.into());
        self
    }

    pub fn build(self) -> CompileRequest {
        CompileRequest {
            sources: self.sources,
            include_dirs: self.include_dirs,
            defines: self.defines,
            top: self.top,
            std: self.std,
            parse_only: self.parse_only,
            want_ast: self.want_ast,
            extra_args: self.extra_args,
        }
    }
}

/// What `Slang::compile` returns.
#[derive(Debug, Clone)]
pub struct CompileResult {
    /// Present only if [`CompileRequestBuilder::want_ast`] was set.
    pub ast: Option<Ast>,
    pub diagnostics: Vec<Diagnostic>,
    /// Slang's exit code, retained for callers that want to report it.
    pub exit_code: Option<i32>,
}

impl CompileResult {
    /// True when slang reported no `Error`-severity diagnostics.
    pub fn is_clean(&self) -> bool {
        !self
            .diagnostics
            .iter()
            .any(|d| matches!(d.severity, crate::diagnostic::Severity::Error))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builder_collects_fields() {
        let req = CompileRequest::builder()
            .source("a.sv")
            .sources(["b.sv", "c.sv"])
            .include_dir("inc1")
            .include_dirs(["inc2", "inc3"])
            .define("FOO", "1")
            .defines([("BAR", "2"), ("BAZ", "")])
            .top("top")
            .std(SvStandard::Sv2017)
            .parse_only(true)
            .want_ast(true)
            .extra_arg("-Wwidth-trunc")
            .build();
        assert_eq!(req.sources.len(), 3);
        assert_eq!(req.include_dirs.len(), 3);
        assert_eq!(req.defines.len(), 3);
        assert_eq!(req.top.as_deref(), Some("top"));
        assert_eq!(req.std, Some(SvStandard::Sv2017));
        assert!(req.parse_only);
        assert!(req.want_ast);
        assert_eq!(req.extra_args, vec!["-Wwidth-trunc".to_string()]);
    }

    #[test]
    fn standard_flag_strings() {
        assert_eq!(SvStandard::Verilog2005.as_flag(), "1364-2005");
        assert_eq!(SvStandard::Sv2017.as_flag(), "1800-2017");
        assert_eq!(SvStandard::Sv2023.as_flag(), "1800-2023");
        assert_eq!(SvStandard::Latest.as_flag(), "latest");
    }

    #[test]
    fn is_clean_distinguishes_warnings_from_errors() {
        use crate::diagnostic::{Diagnostic, Severity};
        let mut result = CompileResult {
            ast: None,
            diagnostics: vec![],
            exit_code: Some(0),
        };
        assert!(result.is_clean());
        result.diagnostics.push(Diagnostic {
            severity: Severity::Warning,
            message: "w".into(),
            option_name: None,
            location: None,
            symbol_path: None,
        });
        assert!(result.is_clean(), "warnings alone are clean");
        result.diagnostics.push(Diagnostic {
            severity: Severity::Error,
            message: "e".into(),
            option_name: None,
            location: None,
            symbol_path: None,
        });
        assert!(!result.is_clean());
    }
}
