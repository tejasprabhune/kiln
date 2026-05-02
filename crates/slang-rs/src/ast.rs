//! Typed wrapper over slang's `--ast-json` output.
//!
//! Slang's elaborated AST is a tree of nodes, each with a `kind`
//! discriminator (`"Root"`, `"Instance"`, `"InstanceBody"`, `"Port"`,
//! `"Variable"`, `"ProceduralBlock"`, …) and a `name`. Some nodes have a
//! `members` array (compilation unit, instance body, package). Others
//! carry kind-specific fields (e.g., `Instance` has a `body`, `Port` has
//! `direction` and `type`).
//!
//! M1 deliberately keeps the typed surface shallow: every node is an
//! [`AstNode`] with a `kind`, optional `name`, optional `members`, and an
//! [`ExtraFields`] map for everything else. This mirrors the milestones
//! doc's "extensible via `serde_json::Value` escape hatch on every node so
//! unknown fields don't break us."
//!
//! Lint and doc rules in M3 / M8 will grow typed accessors on top of this
//! base; M1's contract is that round-tripping unknown JSON does not lose
//! information.

use std::collections::BTreeMap;
use std::io::Read;

use serde::{Deserialize, Serialize};

use crate::error::SlangError;

/// The `--ast-json` document. The top-level wrapper is a single key
/// `"design"` whose value is the elaborated `Root` node.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ast {
    pub design: AstNode,
}

/// A single node in slang's elaborated AST.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AstNode {
    pub kind: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub members: Vec<AstNode>,
    /// Every other field slang produces. Includes `addr`, `body`, `type`,
    /// `direction`, `lifetime`, kind-specific subnodes, and so on. Unknown
    /// or future fields land here without breaking deserialization.
    #[serde(flatten)]
    pub extra: ExtraFields,
}

/// Map of unknown / kind-specific fields on an AST node.
pub type ExtraFields = BTreeMap<String, serde_json::Value>;

impl Ast {
    /// Parse a complete AST JSON document from a string.
    pub fn parse(json: &str) -> Result<Self, SlangError> {
        serde_json::from_str(json).map_err(|e| SlangError::ParseAst(e.to_string()))
    }

    /// Parse a complete AST JSON document from a buffered reader. Preferred
    /// over [`Ast::parse`] when the input could be multi-MB, which is the
    /// usual case for real designs.
    pub fn parse_reader<R: Read>(reader: R) -> Result<Self, SlangError> {
        serde_json::from_reader(reader).map_err(|e| SlangError::ParseAst(e.to_string()))
    }

    /// Top-level instances elaborated by slang. In SystemVerilog terms,
    /// these are the modules (or programs) named in `--top` that survived
    /// elaboration. Slang represents them as nodes with `kind == "Instance"`.
    pub fn top_instances(&self) -> impl Iterator<Item = &AstNode> {
        self.design.members.iter().filter(|m| m.kind == "Instance")
    }
}

impl AstNode {
    /// Lookup a field that wasn't promoted to a typed property.
    pub fn extra(&self, key: &str) -> Option<&serde_json::Value> {
        self.extra.get(key)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_minimal_root() {
        let json = r#"{"design": {"name": "$root", "kind": "Root", "members": []}}"#;
        let ast = Ast::parse(json).unwrap();
        assert_eq!(ast.design.kind, "Root");
        assert_eq!(ast.design.name, "$root");
        assert!(ast.design.members.is_empty());
    }

    #[test]
    fn unknown_fields_round_trip_into_extra() {
        let json = r#"{
            "design": {
                "name": "$root",
                "kind": "Root",
                "addr": 12345,
                "futureField": "fine",
                "members": []
            }
        }"#;
        let ast = Ast::parse(json).unwrap();
        assert_eq!(
            ast.design.extra.get("addr").and_then(|v| v.as_u64()),
            Some(12345)
        );
        assert_eq!(
            ast.design.extra.get("futureField").and_then(|v| v.as_str()),
            Some("fine")
        );
    }

    #[test]
    fn parses_real_valid_module_capture() {
        let json = include_str!("../tests/fixtures/captured/valid_module.ast.json");
        let ast = Ast::parse(json).unwrap();
        assert_eq!(ast.design.kind, "Root");
        let tops: Vec<_> = ast.top_instances().collect();
        assert_eq!(
            tops.len(),
            1,
            "expected exactly one top instance (`counter`)"
        );
        assert_eq!(tops[0].name, "counter");
        // `body` is a kind-specific field, not a typed accessor at M1.
        assert!(
            tops[0].extra("body").is_some(),
            "Instance should carry a body"
        );
    }

    #[test]
    fn parses_real_package_capture() {
        // Slang nests `Package` nodes inside a `CompilationUnit`. This test
        // also acts as a regression check for the recursion shape: descend
        // through `members` until we find the package.
        let json = include_str!("../tests/fixtures/captured/package_pkg.ast.json");
        let ast = Ast::parse(json).unwrap();
        fn find_kind<'a>(n: &'a AstNode, kind: &str, out: &mut Vec<&'a AstNode>) {
            if n.kind == kind {
                out.push(n);
            }
            for child in &n.members {
                find_kind(child, kind, out);
            }
        }
        let mut pkgs = Vec::new();
        find_kind(&ast.design, "Package", &mut pkgs);
        assert_eq!(pkgs.len(), 1, "expected exactly one Package node");
        assert_eq!(pkgs[0].name, "my_pkg");
        let mut params = Vec::new();
        find_kind(pkgs[0], "Parameter", &mut params);
        assert!(
            params.iter().any(|p| p.name == "WIDTH"),
            "expected WIDTH parameter"
        );
    }

    #[test]
    fn rejects_invalid_json() {
        let err = Ast::parse("not json").unwrap_err();
        assert!(matches!(err, SlangError::ParseAst(_)));
    }
}
