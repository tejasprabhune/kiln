//! Doc-comment extractor: source pass + AST pass + join.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use slang_rs::{CompileRequest, Slang};

use crate::DocError;
use kiln_build::SourceSet;
use kiln_core::Manifest;

/// One documented item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocItem {
    pub name: String,
    pub kind: ItemKind,
    pub doc: String,
    pub source_file: Option<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ItemKind {
    Module,
    Package,
    Interface,
}

impl ItemKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ItemKind::Module => "module",
            ItemKind::Package => "package",
            ItemKind::Interface => "interface",
        }
    }
}

/// Map from item name → DocItem. Stable iteration via BTreeMap.
#[derive(Debug, Clone, Default)]
pub struct DocSet {
    pub items: BTreeMap<String, DocItem>,
}

/// Run the two-pass extractor and return the joined map.
///
/// Today the primary list of items comes from a source-level scan
/// for `module|package|interface <name>;` declarations, *not* from
/// slang's AST. Slang's `--ast-json` only includes elaborated items
/// (those reachable from `--top`); modules that exist in the codebase
/// but are not instantiated would otherwise be missing. The slang
/// pass is run anyway so that future cross-references (port types,
/// instance hierarchy) can hang off real elaborated AST data; M8's
/// acceptance criteria don't depend on it yet.
pub fn extract(
    slang: &Slang,
    manifest: &Manifest,
    source_set: &SourceSet,
) -> Result<DocSet, DocError> {
    // Source pass: enumerate items + their attached docs.
    let mut items: BTreeMap<String, DocItem> = BTreeMap::new();
    for f in source_set.files() {
        let blocks = scan_doc_blocks(f).map_err(|source| DocError::Io {
            path: f.clone(),
            source,
        })?;
        for block in blocks {
            let kind = match block.keyword.as_str() {
                "module" => ItemKind::Module,
                "package" => ItemKind::Package,
                "interface" => ItemKind::Interface,
                _ => continue,
            };
            items
                .entry(block.target.clone())
                .or_insert_with(|| DocItem {
                    name: block.target.clone(),
                    kind,
                    doc: block.doc.clone(),
                    source_file: Some(f.clone()),
                });
        }
    }

    // AST pass: best-effort sanity check + future home for cross-references.
    let req = {
        use kiln_core::SvLanguage;
        let mut b = CompileRequest::builder().top(&manifest.design.top);
        for f in source_set.files() {
            b = b.source(f.clone());
        }
        for d in &manifest.design.include_dirs {
            b = b.include_dir(source_set.project_root.join(d));
        }
        for (k, v) in &manifest.design.defines {
            b = b.define(k.clone(), v.clone());
        }
        if let Some(ts) = &manifest.design.timescale {
            b = b.extra_arg("--timescale".to_string());
            b = b.extra_arg(ts.clone());
        }
        if let Some(lang) = manifest.design.language {
            let flag = match lang {
                SvLanguage::Sv2005 => "1364-2005",
                SvLanguage::Sv2009 => "1800-2009",
                SvLanguage::Sv2012 => "1800-2012",
                SvLanguage::Sv2017 => "1800-2017",
                SvLanguage::Sv2023 => "1800-2023",
            };
            b = b.extra_arg("--std".to_string());
            b = b.extra_arg(flag.to_string());
        }
        for lib in &manifest.design.libraries {
            b = b.extra_arg("-y".to_string());
            b = b.extra_arg(lib.clone());
        }
        for arg in manifest
            .tool
            .slang
            .as_ref()
            .map(|s| s.extra_args.as_slice())
            .unwrap_or(&[])
        {
            b = b.extra_arg(arg.clone());
        }
        b.want_ast(true).build()
    };
    let _ = slang.compile(&req);

    Ok(DocSet { items })
}

/// One scanned doc block + the item it attaches to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocBlock {
    pub keyword: String,
    pub target: String,
    pub doc: String,
}

/// Scan a source file for `module|package|interface <name>` declarations,
/// optionally preceded by a `///` block. Returns one `DocBlock` per
/// declaration. Items without a `///` block still appear, with
/// `doc = ""`.
pub fn scan_doc_blocks(path: &Path) -> std::io::Result<Vec<DocBlock>> {
    let text = std::fs::read_to_string(path)?;
    let mut out = Vec::new();
    let mut current: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("///") {
            current.push(rest.trim().to_string());
            continue;
        }
        if let Some((keyword, target)) = item_target(trimmed) {
            let doc = current.join("\n");
            out.push(DocBlock {
                keyword,
                target,
                doc,
            });
            current.clear();
            continue;
        }
        if !trimmed.starts_with("//") && !trimmed.is_empty() {
            current.clear();
        }
    }
    Ok(out)
}

fn item_target(line: &str) -> Option<(String, String)> {
    for keyword in ["module", "package", "interface"] {
        if let Some(rest) = line.strip_prefix(keyword) {
            // Require a whitespace boundary so `module_a` doesn't match.
            let first = rest.chars().next();
            if !matches!(first, Some(c) if c.is_whitespace()) {
                continue;
            }
            let rest = rest.trim_start();
            let id: String = rest
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                .collect();
            if !id.is_empty() {
                return Some((keyword.to_string(), id));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_to_tmp(body: &str) -> tempfile::NamedTempFile {
        let f = tempfile::Builder::new().suffix(".sv").tempfile().unwrap();
        std::fs::write(f.path(), body).unwrap();
        f
    }

    #[test]
    fn scans_basic_doc_comment() {
        let f =
            write_to_tmp("/// A widget that ticks.\n/// Two lines.\nmodule widget;\nendmodule\n");
        let blocks = scan_doc_blocks(f.path()).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].target, "widget");
        assert_eq!(blocks[0].keyword, "module");
        assert!(blocks[0].doc.contains("widget"));
        assert!(blocks[0].doc.contains("Two lines"));
    }

    #[test]
    fn scans_package_doc() {
        let f = write_to_tmp("/// A package of types.\npackage my_pkg;\nendpackage\n");
        let blocks = scan_doc_blocks(f.path()).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].target, "my_pkg");
        assert_eq!(blocks[0].keyword, "package");
    }

    #[test]
    fn finds_modules_without_doc_comments() {
        let f = write_to_tmp("module no_doc;\nendmodule\n");
        let blocks = scan_doc_blocks(f.path()).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].target, "no_doc");
        assert_eq!(blocks[0].doc, "");
    }

    #[test]
    fn drops_orphan_blocks() {
        // `orphan` is followed by a blank line, then real code. The blank
        // line preserves `current` (it's not a code line), but the next
        // non-comment non-blank line that *isn't* an item declaration
        // clears it. This ensures stray `///` blocks not attached to a
        // declaration don't bleed into the next item.
        let f =
            write_to_tmp("/// orphan\n\nlogic something;\n/// real\nmodule real_one;\nendmodule\n");
        let blocks = scan_doc_blocks(f.path()).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].target, "real_one");
        assert_eq!(blocks[0].doc, "real");
    }

    #[test]
    fn extracts_target_after_keyword() {
        assert_eq!(
            item_target("module foo;"),
            Some(("module".into(), "foo".into()))
        );
        assert_eq!(
            item_target("module    bar (input clk);"),
            Some(("module".into(), "bar".into()))
        );
        assert_eq!(
            item_target("package my_pkg;"),
            Some(("package".into(), "my_pkg".into()))
        );
        assert_eq!(
            item_target("interface axi_if;"),
            Some(("interface".into(), "axi_if".into()))
        );
        assert_eq!(item_target("// not an item"), None);
        assert_eq!(item_target("module"), None);
        assert_eq!(item_target("modulebar foo;"), None);
    }

    #[test]
    fn item_kind_str() {
        assert_eq!(ItemKind::Module.as_str(), "module");
        assert_eq!(ItemKind::Package.as_str(), "package");
        assert_eq!(ItemKind::Interface.as_str(), "interface");
    }
}
