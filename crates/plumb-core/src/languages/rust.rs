// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use super::Language;
use crate::index::Symbol;

/// Stops the path at `;`, whitespace, `{` or `(` — whichever comes first —
/// and strips a trailing `::` left behind when a brace group followed
/// (`plumb_core::{a, b}` stops at `{`, leaving `plumb_core::` to trim).
fn rust_name(rest: &str) -> &str {
    let end = rest
        .find(|c: char| c == ';' || c.is_whitespace() || c == '{' || c == '(')
        .unwrap_or(rest.len());
    let name = &rest[..end];
    name.strip_suffix("::").unwrap_or(name)
}

/// `use NAME[::…];` / `use NAME;` (also `pub use`), `extern crate NAME;`,
/// kept as the full path written in the source. `crate`, `self` and `super`
/// name the current crate, not a dependency, so a path is skipped whenever
/// its *first* segment is one of them.
fn rust_imports(text: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let rest = trimmed
            .strip_prefix("pub use ")
            .or_else(|| trimmed.strip_prefix("use "))
            .or_else(|| trimmed.strip_prefix("extern crate "));
        if let Some(rest) = rest {
            let name = rust_name(rest);
            let head = name.split("::").next().unwrap_or("");
            if !name.is_empty() && head != "crate" && head != "self" && head != "super" {
                names.push(name.to_string());
            }
        }
    }
    names
}

/// Extract public symbols from Rust source code.
fn rust_symbols(text: &str) -> Vec<Symbol> {
    const RUST_KINDS: &[&str] = &[
        "fn", "struct", "enum", "trait", "const", "static", "mod", "type",
    ];

    let mut found = Vec::new();
    for line in text.lines() {
        if let Some(symbol) = rust_symbol(line, RUST_KINDS) {
            found.push(symbol);
        }
    }
    found
}

/// Match a single line for a Rust public symbol.
fn rust_symbol(line: &str, kinds: &[&str]) -> Option<Symbol> {
    let rest = line.trim_start().strip_prefix("pub ")?;
    // `pub(crate)`/`pub(super)` never reach here: their prefix is `pub(`,
    // not `pub `, so the strip above already rejected them.
    let rest = strip_modifiers(rest);
    let (kind, rest) = kinds.iter().find_map(|kind| {
        rest.strip_prefix(kind)
            .filter(|tail| tail.starts_with(|c: char| c.is_whitespace()))
            .map(|tail| (*kind, tail))
    })?;
    let name = super::identifier(rest.trim_start());
    (!name.is_empty()).then(|| Symbol {
        kind: kind.to_string(),
        name,
    })
}

/// Peels `const`/`async`/`unsafe` modifiers off the front of `rest` (what
/// follows `pub `) so the kind match below sees the keyword actually being
/// declared.
///
/// `async ` and `unsafe ` are always modifiers — neither names a kind in
/// [`rust_symbols`]'s `RUST_KINDS` — so they are peeled unconditionally.
/// `const ` is ambiguous: `pub const fn new` has `const` modifying `fn`, but
/// `pub const MARKER: &str` has `const` *as* the kind. It is peeled only
/// when doing so still leaves a keyword behind — `fn`, or another modifier
/// that itself leads to one, as in `pub const unsafe fn x` — so a plain
/// `const`/`static` declaration is left untouched for the kind match to
/// claim.
fn strip_modifiers(mut rest: &str) -> &str {
    loop {
        if let Some(tail) = rest.strip_prefix("async ") {
            rest = tail;
        } else if let Some(tail) = rest.strip_prefix("unsafe ") {
            rest = tail;
        } else if let Some(tail) = rest
            .strip_prefix("const ")
            .filter(|tail| starts_with_modifiable_keyword(tail))
        {
            rest = tail;
        } else {
            break;
        }
    }
    rest
}

/// Whether `text` opens with `fn`, `async` or `unsafe` followed by
/// whitespace — the keywords a peeled `const ` may legally have been
/// modifying.
fn starts_with_modifiable_keyword(text: &str) -> bool {
    ["fn ", "async ", "unsafe "]
        .iter()
        .any(|keyword| text.starts_with(keyword))
}

pub struct Rust;

impl Language for Rust {
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }
    fn imports(&self, text: &str) -> Vec<String> {
        rust_imports(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        rust_symbols(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn rust_use_lines_yield_the_full_path() {
        let text = "use plumb_core::manifest;\npub use serde::Serialize;\nuse std::path::Path;\n";
        assert_eq!(
            imports("crates/x/src/main.rs", text),
            vec![
                "plumb_core::manifest",
                "serde::Serialize",
                "std::path::Path"
            ]
        );
    }

    #[test]
    fn a_rust_brace_group_yields_the_path_before_the_brace() {
        let text = "use plumb_core::{manifest, finding};\n";
        assert_eq!(imports("crates/x/src/main.rs", text), vec!["plumb_core"]);
    }

    #[test]
    fn rust_crate_self_and_super_are_not_imports() {
        let text = "use crate::git;\nuse self::inner;\nuse super::outer;\n";
        assert!(imports("crates/x/src/main.rs", text).is_empty());
    }

    #[test]
    fn rust_public_items_are_extracted_with_their_kind() {
        let text = "\
pub fn run(repo: &Path) -> Result<()> {\n\
pub struct Outcome {\n\
pub(crate) fn hidden() {}\n\
fn private() {}\n\
    pub fn method(&self) {}\n\
pub const MARKER: &str = \"x\";\n";
        let found = symbols("crates/x/src/lib.rs", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("fn", "run"),
                ("struct", "Outcome"),
                ("fn", "method"),
                ("const", "MARKER"),
            ]
        );
    }

    /// `const`, `async` and `unsafe` modifiers on `fn` must not shift the
    /// extracted kind or name: `pub const fn new` is kind `fn` name `new`,
    /// not kind `const` name `fn` — the fabricated entry a naive keyword
    /// match used to produce.
    #[test]
    fn rust_modifier_keywords_on_fn_are_not_mistaken_for_the_kind() {
        let text = "\
pub const fn new(x: i32) -> Self {\n\
pub async fn go() {\n\
pub unsafe fn raw() {\n\
pub const unsafe fn x() {\n";
        let found = symbols("crates/x/src/lib.rs", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![("fn", "new"), ("fn", "go"), ("fn", "raw"), ("fn", "x")]
        );
    }

    /// A plain `const`/`static` declaration — no `fn` following — is still
    /// its own kind: `const` here is what is being declared, not a modifier
    /// on something else.
    #[test]
    fn rust_plain_const_and_static_are_unaffected_by_modifier_stripping() {
        let text = "pub const MARKER: &str = \"x\";\npub static X: i32 = 1;\n";
        let found = symbols("crates/x/src/lib.rs", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("const", "MARKER"), ("static", "X")]);
    }
}
