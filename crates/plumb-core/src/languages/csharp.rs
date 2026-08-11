// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! C#: `using` directives as imports, public type declarations as symbols.
//!
//! Methods and properties are deliberately not extracted. The index maps
//! types, the same altitude Python's extractor works at with classes; a C#
//! file's methods and auto-properties would outnumber its types several times
//! over and bury the surface the index exists to show.

use super::Language;
use crate::index::Symbol;

/// The modifiers that may precede a type keyword, `public` excluded — it is
/// what [`csharp_symbol`] is looking for, so it is matched separately rather
/// than skipped with the rest.
const MODIFIERS: &[&str] = &[
    "private",
    "protected",
    "internal",
    "file",
    "static",
    "sealed",
    "abstract",
    "partial",
    "readonly",
    "ref",
    "unsafe",
    "new",
];

/// The declaration keywords this extractor reports, each used verbatim as the
/// symbol's kind.
const KINDS: &[&str] = &["class", "interface", "struct", "record", "enum", "delegate"];

/// Whether `text` is a single unqualified C# identifier.
fn is_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|c| c.is_alphabetic() || c == '_' || c == '@')
        && chars.all(|c| c.is_alphanumeric() || c == '_')
}

/// Whether `text` is a dotted namespace-or-type path, every segment an
/// identifier: `Ummarell.Api.Services`, `System`, but not `x = y` or `f(…)`.
fn is_namespace_path(text: &str) -> bool {
    !text.is_empty() && text.split('.').all(is_identifier)
}

/// The namespace path a `using` *directive* names, or `None` for the `using`
/// *statement*, which shares the keyword and is not an import.
///
/// The discrimination is the shape of what follows: a directive is a plain
/// dotted path terminated by `;`, optionally preceded by `static` or by an
/// `Alias = `. Anything else — `using (var f = File.Open(path))`, `using var
/// scope = provider.CreateScope();` — fails that shape, because `(var f = …)`
/// is not an identifier path and the declarator `var scope` is not a single
/// identifier standing where an alias would.
///
/// `global using` is the same directive with file-wide scope, so it yields
/// the same path. `using static X.Y.Z;` yields `X.Y.Z`, the type's path:
/// resolution matches leading segments, and the namespace is what those
/// segments are.
fn csharp_import(line: &str) -> Option<String> {
    let trimmed = line.trim();
    let rest = trimmed
        .strip_prefix("global using ")
        .or_else(|| trimmed.strip_prefix("using "))?
        .trim_start();
    let rest = rest
        .strip_prefix("static ")
        .map(str::trim_start)
        .unwrap_or(rest);
    // No `;` on the line is the `using (…) { … }` block form, never a
    // directive.
    let body = rest[..rest.find(';')?].trim();
    let body = match body.split_once('=') {
        // `using Alias = X.Y.Z;` — the alias must be a bare identifier, which
        // is what rejects the `using var scope = …;` declarator.
        Some((alias, target)) => {
            if !is_identifier(alias.trim()) {
                return None;
            }
            target.trim()
        }
        None => body,
    };
    // An alias may target a constructed generic (`using Ints =
    // System.Collections.Generic.List<int>;`); the namespace path is what
    // precedes the type arguments.
    let path = body.split('<').next().unwrap_or(body).trim();
    is_namespace_path(path).then(|| path.to_string())
}

fn csharp_imports(text: &str) -> Vec<String> {
    text.lines().filter_map(csharp_import).collect()
}

/// A public type declaration on one line, at any indentation — C# types nest
/// inside namespaces and inside each other, so a leading-whitespace filter of
/// the kind Python's extractor uses would drop nearly everything.
///
/// `record class` and `record struct` both report kind `record`: the second
/// keyword picks the underlying representation, not a different declaration.
fn csharp_symbol(line: &str) -> Option<Symbol> {
    let mut tokens = line.split_whitespace().peekable();
    let mut public = false;
    let kind = loop {
        let token = tokens.next()?;
        if token == "public" {
            public = true;
        } else if !MODIFIERS.contains(&token) {
            break token;
        }
    };
    if !public || !KINDS.contains(&kind) {
        return None;
    }
    let name = if kind == "delegate" {
        delegate_name(&tokens.collect::<Vec<_>>().join(" "))?
    } else {
        if kind == "record" {
            let _ = tokens.next_if(|token| *token == "class" || *token == "struct");
        }
        super::identifier(tokens.next()?)
    };
    (!name.is_empty()).then(|| Symbol {
        kind: kind.to_string(),
        name,
    })
}

/// The name in what follows `delegate`. Unlike every other kind here, a
/// delegate declares a return type before its name — `public delegate TResult
/// Func<in T, out TResult>(T arg);` — so the name is the identifier
/// immediately before the parameter list, once the type arguments (which may
/// themselves contain whitespace) are out of the way.
fn delegate_name(rest: &str) -> Option<String> {
    let stripped = strip_generic_groups(rest);
    let head = stripped.split('(').next().unwrap_or(stripped.as_str());
    let name = super::identifier(head.split_whitespace().next_back()?);
    (!name.is_empty()).then_some(name)
}

/// `text` with every `<…>` group removed, nesting included.
fn strip_generic_groups(text: &str) -> String {
    let mut kept = String::new();
    let mut depth = 0usize;
    for c in text.chars() {
        match c {
            '<' => depth += 1,
            '>' => depth = depth.saturating_sub(1),
            _ if depth == 0 => kept.push(c),
            _ => {}
        }
    }
    kept
}

fn csharp_symbols(text: &str) -> Vec<Symbol> {
    text.lines().filter_map(csharp_symbol).collect()
}

pub struct CSharp;

impl Language for CSharp {
    fn extensions(&self) -> &'static [&'static str] {
        &["cs"]
    }
    fn imports(&self, _path: &str, text: &str) -> Vec<String> {
        csharp_imports(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        csharp_symbols(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn using_directives_yield_the_full_namespace_path() {
        let text = "using Ummarell.Api.Services;\n\
                    global using System.Text.Json;\n\
                    namespace Ummarell.Api.Web;\n";
        assert_eq!(
            imports("src/Ummarell.Api/Home.cs", text),
            vec!["Ummarell.Api.Services", "System.Text.Json"]
        );
    }

    #[test]
    fn using_static_and_alias_yield_the_type_path() {
        let text = "using static System.Math;\nusing Db = Ummarell.Api.Data;\n";
        assert_eq!(
            imports("src/Ummarell.Api/Home.cs", text),
            vec!["System.Math", "Ummarell.Api.Data"]
        );
    }

    #[test]
    fn a_using_statement_is_not_an_import() {
        let text = "using (var f = File.Open(path))\n\
                    {\n\
                    }\n\
                    using var scope = provider.CreateScope();\n\
                    await using var conn = OpenAsync();\n";
        let found = imports("src/Ummarell.Api/Home.cs", text);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn public_type_declarations_are_extracted() {
        let text = "public class Foo\n    \
                    public sealed record Bar<T>\n\
                    public enum Baz\n\
                    public record struct Point\n\
                    public interface IThing\n\
                    public void Run()\n\
                    public int Count { get; set; }\n";
        let found = symbols("src/Ummarell.Api/Home.cs", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("class", "Foo"),
                ("record", "Bar"),
                ("enum", "Baz"),
                ("record", "Point"),
                ("interface", "IThing"),
            ]
        );
    }

    /// A delegate is the one declared kind whose name is not the token after
    /// the keyword: the return type sits between them. Taking the next token
    /// blindly would index the type `void` as a symbol named `void`.
    #[test]
    fn a_delegate_is_named_before_its_parameter_list() {
        let text = "public delegate void Handler(int x);\n    \
                    public delegate TResult Func<in T, out TResult>(T arg);\n";
        let found = symbols("src/Ummarell.Api/Home.cs", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(pairs, vec![("delegate", "Handler"), ("delegate", "Func")]);
    }

    #[test]
    fn non_public_types_are_skipped() {
        let text = "internal class X\nclass Y\nprivate sealed class Z\n";
        let found = symbols("src/Ummarell.Api/Home.cs", text);
        assert!(found.is_empty(), "{found:?}");
    }
}
