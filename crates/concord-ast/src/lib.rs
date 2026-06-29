//! `concord-ast` — AST symbol extraction + call graph for Concord's symbol-level leases
//! (WP12 S2). The only crate that pulls a native parser (`tree-sitter`); kept separate so
//! `concord-core` stays std-only / zero-dep.
//!
//! - **Symbols** (S2.1, all langs here): what top-level symbols a file defines, with byte
//!   ranges — so a symbol-lease (`<file>:<symbol>`) can be validated.
//! - **Call graph** (S2.2, Rust): caller→callee edges, so a claim can surface an
//!   *advisory* DEP_CHAIN warning ("the symbol you're claiming calls one another session
//!   holds"). The warning is advisory (a call edge is a hint, not mutual exclusion); the
//!   symbol-lease itself stays enforced — the Concord vision line vs. the prior-art `wit`,
//!   whose symbol locks are advisory.
//!
//! Rust first (the dogfood), with TypeScript + Python for breadth. Native tree-sitter
//! (not WASM like `wit`, which is TypeScript) — faster and dependency-light.

use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Parser, Query, QueryCursor};

/// A supported source language.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lang {
    Rust,
    TypeScript,
    Python,
}

impl Lang {
    /// Infer the language from a file path's extension, or `None`.
    pub fn from_path(path: &str) -> Option<Lang> {
        let ext = path.rsplit('.').next()?;
        match ext {
            "rs" => Some(Lang::Rust),
            "ts" | "tsx" | "js" | "jsx" | "mts" | "cts" => Some(Lang::TypeScript),
            "py" | "pyi" => Some(Lang::Python),
            _ => None,
        }
    }

    fn language(self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }

    fn symbol_query(self) -> &'static str {
        match self {
            Lang::Rust => RUST_SYMBOLS,
            Lang::TypeScript => TS_SYMBOLS,
            Lang::Python => PY_SYMBOLS,
        }
    }
}

/// A top-level symbol definition and its source span.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// Coarse kind derived from the node type (`function`, `struct`, `class`, …).
    pub kind: String,
    pub start_byte: usize,
    pub end_byte: usize,
    pub start_row: usize,
    pub end_row: usize,
}

impl Symbol {
    /// Does this symbol's byte span overlap `other`'s? (Nested-symbol claim conflict.)
    pub fn byte_overlaps(&self, other: &Symbol) -> bool {
        self.start_byte < other.end_byte && other.start_byte < self.end_byte
    }
}

/// A call-graph edge: `caller` (the enclosing symbol) calls `callee` (a bare name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dep {
    pub caller: String,
    pub callee: String,
}

const RUST_SYMBOLS: &str = r#"
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

const TS_SYMBOLS: &str = r#"
(function_declaration name: (identifier) @name) @def
(variable_declarator name: (identifier) @name value: (arrow_function)) @def
(method_definition name: (property_identifier) @name) @def
(class_declaration name: (type_identifier) @name) @def
(interface_declaration name: (type_identifier) @name) @def
(type_alias_declaration name: (type_identifier) @name) @def
(enum_declaration name: (identifier) @name) @def
"#;

const PY_SYMBOLS: &str = r#"
(function_definition name: (identifier) @name) @def
(class_definition name: (identifier) @name) @def
"#;

fn coarse_kind(node_kind: &str) -> String {
    node_kind
        .trim_end_matches("_item")
        .trim_end_matches("_definition")
        .trim_end_matches("_declaration")
        .trim_end_matches("_declarator")
        .to_string()
}

fn parse(lang: Language, source: &str) -> Option<tree_sitter::Tree> {
    let mut parser = Parser::new();
    parser.set_language(&lang).ok()?;
    parser.parse(source, None)
}

/// Extract the top-level symbols defined in `source` for `lang`. Empty on parse failure.
pub fn extract_symbols(lang: Lang, source: &str) -> Vec<Symbol> {
    let language = lang.language();
    let Some(tree) = parse(language.clone(), source) else {
        return Vec::new();
    };
    let Ok(query) = Query::new(&language, lang.symbol_query()) else {
        return Vec::new();
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
            out.push(Symbol {
                name: String::from_utf8_lossy(&src[nm.byte_range()]).into_owned(),
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

/// Find a symbol by name (first match) in `source` for `lang`.
pub fn resolve_symbol(lang: Lang, source: &str, name: &str) -> Option<Symbol> {
    extract_symbols(lang, source)
        .into_iter()
        .find(|s| s.name == name)
}

// Rust-only convenience wrappers (S2.1 call sites).
pub fn extract_rust_symbols(source: &str) -> Vec<Symbol> {
    extract_symbols(Lang::Rust, source)
}
pub fn resolve_rust_symbol(source: &str, name: &str) -> Option<Symbol> {
    resolve_symbol(Lang::Rust, source, name)
}

/// The symbol-lease area string for a file + symbol (`<file>:<symbol>`).
pub fn symbol_path(file: &str, symbol: &str) -> String {
    format!("{file}:{symbol}")
}

/// The normalized **signature** of a symbol (F5 enforced contracts) — what an interface
/// contract pins, as opposed to the implementation. For a function/method it is the
/// declaration up to the body delimiter (`{` for Rust/TS, the `:` for Python), so changing
/// the *body* does not break the contract; for a type/struct/class/enum/interface it is the
/// full declaration (the shape itself IS the interface). Whitespace is collapsed so
/// reformatting is not a contract change.
pub fn signature(lang: Lang, source: &str, sym: &Symbol) -> String {
    let text = source.get(sym.start_byte..sym.end_byte).unwrap_or("");
    let is_fn = sym.kind == "function" || sym.kind == "method";
    let raw = if is_fn {
        match lang {
            Lang::Python => cut_at_body_colon(text),
            _ => match text.find('{') {
                Some(p) => &text[..p],
                None => text, // trait-method decl ending in `;` — keep the whole decl
            },
        }
    } else {
        text
    };
    raw.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Python: the function body starts at the first `:` at bracket-depth 0 (parameter
/// annotations' `:` are inside the parens, depth > 0). Returns the text up to it.
fn cut_at_body_colon(text: &str) -> &str {
    let mut depth = 0i32;
    for (i, c) in text.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth -= 1,
            ':' if depth == 0 => return &text[..i],
            _ => {}
        }
    }
    text
}

/// Extract intra-file call edges (Rust): for each call, the callee's bare name and the
/// enclosing top-level symbol (the caller). Cross-file resolution is out of scope; the
/// callee name is matched against held symbol-leases for the advisory DEP_CHAIN warning.
pub fn extract_rust_calls(source: &str) -> Vec<Dep> {
    let language: Language = tree_sitter_rust::LANGUAGE.into();
    let Some(tree) = parse(language.clone(), source) else {
        return Vec::new();
    };
    let Ok(query) = Query::new(&language, "(call_expression function: (_) @fn) @call") else {
        return Vec::new();
    };
    let fn_idx = query.capture_index_for_name("fn");
    let src = source.as_bytes();
    let symbols = extract_rust_symbols(source);

    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(&query, tree.root_node(), src);
    let mut out = Vec::new();
    while let Some(m) = matches.next() {
        let mut fn_node = None;
        let mut call_start = None;
        for cap in m.captures {
            if Some(cap.index) == fn_idx {
                fn_node = Some(cap.node);
            } else {
                call_start = Some(cap.node.start_byte());
            }
        }
        let (Some(fnn), Some(start)) = (fn_node, call_start) else {
            continue;
        };
        let callee = callee_name(&String::from_utf8_lossy(&src[fnn.byte_range()]));
        if callee.is_empty() {
            continue;
        }
        // caller = the innermost top-level symbol whose byte range contains the call.
        if let Some(caller) = symbols
            .iter()
            .filter(|s| s.start_byte <= start && start < s.end_byte)
            .min_by_key(|s| s.end_byte - s.start_byte)
        {
            out.push(Dep {
                caller: caller.name.clone(),
                callee,
            });
        }
    }
    out
}

/// The bare callee name from a call's function expression text: strip generic args, then
/// take the last `.`/`::` path segment (`self.foo`→`foo`, `A::b`→`b`, `foo`→`foo`).
fn callee_name(func_text: &str) -> String {
    let base = func_text.split('<').next().unwrap_or(func_text);
    base.rsplit(['.', ':'])
        .find(|s| !s.is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RS: &str = r#"
pub struct Auth { token: String }
pub fn validate_token(t: &str) -> bool { !t.is_empty() }
impl Auth { pub fn login(&self) -> bool { validate_token(&self.token) } }
enum Role { Admin }
const MAX: u32 = 10;
"#;

    #[test]
    fn rust_symbols() {
        let names: Vec<String> = extract_rust_symbols(RS).into_iter().map(|s| s.name).collect();
        for n in ["Auth", "validate_token", "login", "Role", "MAX"] {
            assert!(names.contains(&n.to_string()), "missing {n}");
        }
    }

    #[test]
    fn signature_pins_decl_not_body() {
        // A function's signature stops at the body; reformatting/body changes don't alter it.
        let s = resolve_rust_symbol(RS, "validate_token").unwrap();
        assert_eq!(signature(Lang::Rust, RS, &s), "pub fn validate_token(t: &str) -> bool");

        // A struct's signature IS its full shape (the interface).
        let st = resolve_rust_symbol(RS, "Auth").unwrap();
        assert_eq!(signature(Lang::Rust, RS, &st), "pub struct Auth { token: String }");

        // Python: cut at the body colon, not the parameter-annotation colons.
        let py = "def serve(host: str, port: int) -> bool:\n    return True\n";
        let psym = resolve_symbol(Lang::Python, py, "serve").unwrap();
        assert_eq!(signature(Lang::Python, py, &psym), "def serve(host: str, port: int) -> bool");
    }

    #[test]
    fn typescript_symbols() {
        let src = "export function foo(){}\nclass Bar {}\ninterface Baz {}\nconst qux = () => 1;";
        let names: Vec<String> =
            extract_symbols(Lang::TypeScript, src).into_iter().map(|s| s.name).collect();
        for n in ["foo", "Bar", "Baz", "qux"] {
            assert!(names.contains(&n.to_string()), "missing {n} in {names:?}");
        }
    }

    #[test]
    fn python_symbols() {
        let src = "def foo():\n    pass\nclass Bar:\n    def meth(self):\n        pass\n";
        let names: Vec<String> =
            extract_symbols(Lang::Python, src).into_iter().map(|s| s.name).collect();
        for n in ["foo", "Bar", "meth"] {
            assert!(names.contains(&n.to_string()), "missing {n} in {names:?}");
        }
    }

    #[test]
    fn lang_from_path() {
        assert_eq!(Lang::from_path("a/b.rs"), Some(Lang::Rust));
        assert_eq!(Lang::from_path("a/b.ts"), Some(Lang::TypeScript));
        assert_eq!(Lang::from_path("a/b.py"), Some(Lang::Python));
        assert_eq!(Lang::from_path("a/b.txt"), None);
    }

    #[test]
    fn rust_call_graph() {
        // login calls validate_token → edge (login → validate_token).
        let deps = extract_rust_calls(RS);
        assert!(
            deps.iter()
                .any(|d| d.caller == "login" && d.callee == "validate_token"),
            "expected login→validate_token in {deps:?}"
        );
    }

    #[test]
    fn callee_name_strips_paths_and_generics() {
        assert_eq!(callee_name("self.foo"), "foo");
        assert_eq!(callee_name("A::b"), "b");
        assert_eq!(callee_name("foo"), "foo");
        assert_eq!(callee_name("collect::<Vec<_>>"), "collect");
    }
}
