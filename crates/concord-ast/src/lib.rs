//! `concord-ast` — AST symbol extraction for Concord's symbol-level leases (WP12 S2).
//!
//! This is the only crate that pulls a native parser (`tree-sitter`); it is kept
//! separate so `concord-core` stays std-only / zero-dep. It answers one question for the
//! lease layer: *what symbols does this source define, and where?* — so a symbol-lease
//! (`<file>:<symbol>`) can be validated and (later, S2.2) a call graph derived.
//!
//! Rust first (the dogfood — Concord coordinates its own Rust development at symbol
//! granularity); TypeScript/Python follow in S2.2. Native tree-sitter (not WASM like
//! the prior-art `wit`, which is TypeScript) — faster and dependency-light.

use streaming_iterator::StreamingIterator;
use tree_sitter::{Parser, Query, QueryCursor};

/// A top-level symbol definition and its source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The symbol name (e.g. `validate_token`).
    pub name: String,
    /// A coarse kind (`function`, `struct`, `enum`, `trait`, `type`, `const`, `static`,
    /// `mod`, `macro`) derived from the tree-sitter node type.
    pub kind: String,
    /// Byte span of the whole definition (the `@def` node).
    pub start_byte: usize,
    pub end_byte: usize,
    /// 0-based line span (for display).
    pub start_row: usize,
    pub end_row: usize,
}

impl Symbol {
    /// Does this symbol's byte span overlap `other`'s? (Used to detect a nested-symbol
    /// claim conflict — an outer fn containing an inner one — which the pure path/symbol
    /// string rule in `concord-core` cannot see.)
    pub fn byte_overlaps(&self, other: &Symbol) -> bool {
        self.start_byte < other.end_byte && other.start_byte < self.end_byte
    }
}

const RUST_QUERY: &str = r#"
(function_item   name: (identifier)      @name) @def
(struct_item     name: (type_identifier) @name) @def
(enum_item       name: (type_identifier) @name) @def
(union_item      name: (type_identifier) @name) @def
(trait_item      name: (type_identifier) @name) @def
(type_item       name: (type_identifier) @name) @def
(const_item      name: (identifier)      @name) @def
(static_item     name: (identifier)      @name) @def
(mod_item        name: (identifier)      @name) @def
(macro_definition name: (identifier)     @name) @def
"#;

fn rust_language() -> tree_sitter::Language {
    tree_sitter_rust::LANGUAGE.into()
}

/// Map a tree-sitter node kind (`function_item`, …) to a coarse symbol kind.
fn coarse_kind(node_kind: &str) -> String {
    node_kind
        .trim_end_matches("_item")
        .trim_end_matches("_definition")
        .to_string()
}

/// Extract the top-level symbols defined in a Rust source string. Returns an empty
/// vec if the source cannot be parsed. (Function items include both free functions and
/// methods inside `impl` blocks.)
pub fn extract_rust_symbols(source: &str) -> Vec<Symbol> {
    let lang = rust_language();
    let mut parser = Parser::new();
    if parser.set_language(&lang).is_err() {
        return Vec::new();
    }
    let tree = match parser.parse(source, None) {
        Some(t) => t,
        None => return Vec::new(),
    };
    let query = match Query::new(&lang, RUST_QUERY) {
        Ok(q) => q,
        Err(_) => return Vec::new(),
    };
    let name_idx = query.capture_index_for_name("name");
    let src = source.as_bytes();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), src);

    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let mut name_node = None;
        let mut def_node = None;
        for cap in m.captures {
            if Some(cap.index) == name_idx {
                name_node = Some(cap.node);
            } else {
                def_node = Some(cap.node);
            }
        }
        if let (Some(nm), Some(def)) = (name_node, def_node) {
            let name = String::from_utf8_lossy(&src[nm.byte_range()]).into_owned();
            out.push(Symbol {
                name,
                kind: coarse_kind(def.kind()),
                start_byte: def.start_byte(),
                end_byte: def.end_byte(),
                start_row: def.start_position().row,
                end_row: def.end_position().row,
            });
        }
    }
    out
}

/// Find a top-level Rust symbol by name (first match), or `None`.
pub fn resolve_rust_symbol(source: &str, name: &str) -> Option<Symbol> {
    extract_rust_symbols(source)
        .into_iter()
        .find(|s| s.name == name)
}

/// The symbol-lease area string for a file + symbol (`<file>:<symbol>`).
pub fn symbol_path(file: &str, symbol: &str) -> String {
    format!("{file}:{symbol}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = r#"
use std::fmt;

pub struct Auth { token: String }

pub fn validate_token(t: &str) -> bool { !t.is_empty() }

impl Auth {
    pub fn login(&self) -> bool { validate_token(&self.token) }
}

enum Role { Admin, User }
const MAX: u32 = 10;
"#;

    #[test]
    fn extracts_top_level_symbols() {
        let syms = extract_rust_symbols(SRC);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"Auth"));
        assert!(names.contains(&"validate_token"));
        assert!(names.contains(&"login")); // method inside impl
        assert!(names.contains(&"Role"));
        assert!(names.contains(&"MAX"));
    }

    #[test]
    fn kinds_are_coarse() {
        let syms = extract_rust_symbols(SRC);
        let f = syms.iter().find(|s| s.name == "validate_token").unwrap();
        assert_eq!(f.kind, "function");
        let s = syms.iter().find(|s| s.name == "Auth").unwrap();
        assert_eq!(s.kind, "struct");
    }

    #[test]
    fn resolve_finds_byte_range() {
        let s = resolve_rust_symbol(SRC, "validate_token").unwrap();
        assert!(s.end_byte > s.start_byte);
        assert_eq!(&SRC[s.start_byte..s.start_byte + 6], "pub fn");
    }

    #[test]
    fn resolve_missing_is_none() {
        assert!(resolve_rust_symbol(SRC, "nonexistent").is_none());
    }

    #[test]
    fn byte_overlap_detects_nesting() {
        let outer = Symbol {
            name: "o".into(),
            kind: "function".into(),
            start_byte: 0,
            end_byte: 100,
            start_row: 0,
            end_row: 9,
        };
        let inner = Symbol {
            name: "i".into(),
            kind: "function".into(),
            start_byte: 20,
            end_byte: 40,
            start_row: 2,
            end_row: 3,
        };
        let sep = Symbol {
            name: "s".into(),
            kind: "function".into(),
            start_byte: 200,
            end_byte: 250,
            start_row: 20,
            end_row: 25,
        };
        assert!(outer.byte_overlaps(&inner));
        assert!(!outer.byte_overlaps(&sep));
    }
}
