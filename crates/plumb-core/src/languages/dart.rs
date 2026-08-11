// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Dart: `package:` imports and exports, column-0 declarations as symbols.
//!
//! Members are deliberately not extracted, the same altitude Python's and
//! C#'s extractors work at: a Dart file's methods, fields and `build`
//! overrides live inside a class and are found by the column-0 filter
//! already, so the index maps the declarations a file exports rather than
//! everything it contains.

use super::Language;
use crate::index::Symbol;

/// The modifiers that may precede a declaration keyword. Dart 3's class
/// modifiers (`base`, `final`, `interface`, `sealed`) are as much part of a
/// declaration as `abstract`, and a `sealed class Result` left unmatched is
/// a real symbol lost.
const MODIFIERS: &[&str] = &["abstract", "base", "final", "interface", "sealed"];

/// The declaration keywords this extractor reports, each used verbatim as
/// the symbol's kind.
const KINDS: &[&str] = &["class", "enum", "mixin", "extension", "typedef"];

/// Tokens that may stand where a return type would but never are one, so a
/// stray column-0 continuation line (`} catch (e) {`, `else if (x) {`) is
/// not read as a function declaration.
const NON_TYPES: &[&str] = &[
    "if", "else", "for", "while", "switch", "do", "try", "catch", "return", "await", "assert",
    "throw", "case", "yield",
];

/// The contents of the first single- or double-quoted string in `rest`.
fn quoted(rest: &str) -> Option<&str> {
    let start = rest.find(['\'', '"'])?;
    let quote = rest[start..].chars().next()?;
    let after = &rest[start + quote.len_utf8()..];
    let end = after.find(quote)?;
    Some(&after[..end])
}

/// The URI an `import` or `export` directive names, quotes stripped.
///
/// The `as`/`show`/`hide` suffixes fall away with everything after the
/// closing quote. `part` and `part of` are not matched at all: they splice
/// one file into another's declaration space, so the two halves are the
/// same library and an edge between them would be an edge to itself.
fn dart_spec(line: &str) -> Option<&str> {
    let trimmed = line.trim_start();
    let rest = trimmed
        .strip_prefix("import ")
        .or_else(|| trimmed.strip_prefix("export "))?;
    quoted(rest)
}

/// The module path a `package:` URI names, `package:` and the `.dart`
/// suffix removed: `package:umarell/features/chat/chat_service.dart` →
/// `umarell/features/chat/chat_service`.
///
/// Every other URI scheme is skipped, which covers two cases the other
/// extractors already skip for the same reason. `dart:async` names an SDK
/// builtin, the JS `node:` precedent. `'chat_state.dart'` and `'../x.dart'`
/// are relative, the JS relative-spec precedent: they name a file, not a
/// package, and left unskipped they would hand the resolver a leading
/// segment (`chat_state`, `..`) that can collide with a declared module
/// whose leaf happens to match. The trade is the same one documented there
/// — a relative import inside a package is invisible to the boundary gate.
fn dart_segment(spec: &str) -> Option<String> {
    let path = spec.strip_prefix("package:")?;
    let path = path.strip_suffix(".dart").unwrap_or(path);
    (!path.is_empty()).then(|| path.to_string())
}

fn dart_imports(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(dart_spec)
        .filter_map(dart_segment)
        .collect()
}

/// `text` with every `<…>` group removed, nesting included, so a generic
/// return type spanning whitespace (`Map<String, dynamic> parse(`) counts
/// as the single token it is.
fn strip_type_arguments(text: &str) -> String {
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

/// Whether `token` could be a type name: an identifier, optionally
/// nullable, and not one of the statement keywords in [`NON_TYPES`].
fn is_type_token(token: &str) -> bool {
    let token = token.strip_suffix('?').unwrap_or(token);
    !NON_TYPES.contains(&token)
        && !token.is_empty()
        && token.chars().all(|c| c.is_alphanumeric() || c == '_')
}

/// A top-level declaration on a column-0 line. Dart nests members inside
/// classes and functions inside functions, so unlike C# the indentation
/// filter is exactly the public-surface filter, as it is in Python.
fn dart_symbol(line: &str) -> Option<Symbol> {
    if line.starts_with(|c: char| c.is_whitespace()) {
        return None;
    }
    let mut tokens = line.split_whitespace().peekable();
    let kind = loop {
        let token = tokens.next()?;
        if !MODIFIERS.contains(&token) {
            break token;
        }
    };
    if !KINDS.contains(&kind) {
        return dart_function(line);
    }
    let name = match kind {
        "typedef" => typedef_name(&tokens.collect::<Vec<_>>().join(" "))?,
        _ => {
            // `mixin class Foo` declares a mixin; without this the name
            // would come back as `class`.
            if kind == "mixin" {
                let _ = tokens.next_if(|token| *token == "class");
            }
            super::identifier(tokens.next()?)
        }
    };
    // `extension on String { … }` is an unnamed extension: the token after
    // the keyword is the `on` clause, not a name.
    if name.is_empty() || name.starts_with('_') || (kind == "extension" && name == "on") {
        return None;
    }
    Some(Symbol {
        kind: kind.to_string(),
        name,
    })
}

/// The name in what follows `typedef`. Both forms end at the same place:
/// `typedef Json = Map<String, dynamic>;` stops at the `=`, and the older
/// function-type form `typedef void Callback(int x);` stops at the
/// parameter list, leaving the name as the last token either way.
fn typedef_name(rest: &str) -> Option<String> {
    let head = rest
        .split(['=', '('])
        .next()
        .unwrap_or(rest)
        .trim_end_matches(|c: char| c == ';' || c.is_whitespace());
    let name = super::identifier(strip_type_arguments(head).split_whitespace().next_back()?);
    (!name.is_empty()).then_some(name)
}

/// A top-level function: `Type name(`, exactly two tokens before the
/// parameter list once type arguments are out of the way. A function
/// written without a return type (`main() {}`) is not matched — one token
/// before the parameter list is indistinguishable from a call.
fn dart_function(line: &str) -> Option<Symbol> {
    let head = strip_type_arguments(&line[..line.find('(')?]);
    let mut tokens = head.split_whitespace();
    let returns = tokens.next()?;
    let name = super::identifier(tokens.next()?);
    if tokens.next().is_some()
        || !is_type_token(returns)
        || name.is_empty()
        || name.starts_with('_')
    {
        return None;
    }
    Some(Symbol {
        kind: "fn".to_string(),
        name,
    })
}

fn dart_symbols(text: &str) -> Vec<Symbol> {
    text.lines().filter_map(dart_symbol).collect()
}

pub struct Dart;

impl Language for Dart {
    fn extensions(&self) -> &'static [&'static str] {
        &["dart"]
    }
    fn imports(&self, _path: &str, text: &str) -> Vec<String> {
        dart_imports(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        dart_symbols(text)
    }
}

#[cfg(test)]
mod tests {
    use crate::boundaries::imports;
    use crate::index::symbols;

    #[test]
    fn package_imports_yield_the_stripped_path() {
        let text = "import 'package:umarell/features/chat/chat_service.dart';\n\
                    export 'package:umarell/core/result.dart';\n\
                    import \"package:flutter/material.dart\";\n\
                    import 'package:provider/provider.dart' as p;\n\
                    import 'package:umarell/models/user.dart' show User, Role;\n\
                    export 'package:umarell/models/role.dart' hide Internal;\n";
        assert_eq!(
            imports("lib/features/chat/chat_view.dart", text),
            vec![
                "umarell/features/chat/chat_service",
                "umarell/core/result",
                "flutter/material",
                "provider/provider",
                "umarell/models/user",
                "umarell/models/role",
            ]
        );
    }

    /// A `dart:` spec names an SDK builtin, the `node:` precedent, and a
    /// relative spec names a file rather than a package, the JS relative
    /// precedent. Neither can resolve to a declared module, and both would
    /// hand the resolver leading segments (`dart`, `..`, a bare file stem)
    /// that can collide with a module whose leaf happens to match.
    #[test]
    fn sdk_and_relative_imports_are_skipped() {
        let text = "import 'dart:async';\n\
                    import 'dart:io' as io;\n\
                    import 'chat_state.dart';\n\
                    import '../core/result.dart';\n\
                    import './widgets/bubble.dart';\n";
        let found = imports("lib/features/chat/chat_view.dart", text);
        assert!(found.is_empty(), "{found:?}");
    }

    /// `part` and `part of` splice one file into another's declaration
    /// space. They are file inclusion, not dependency: the two halves are
    /// the same library, so a boundary edge between them would be an edge
    /// from a module to itself.
    #[test]
    fn part_directives_are_not_imports() {
        let text = "part 'chat_service.g.dart';\n\
                    part of 'chat_service.dart';\n\
                    part 'package:umarell/core/result.g.dart';\n";
        let found = imports("lib/features/chat/chat_service.dart", text);
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn top_level_declarations_are_extracted() {
        let text = "class ChatService {\n\
                    abstract class Repository {\n\
                    sealed class Result {\n\
                    enum Status { idle, busy }\n\
                    mixin Logging {\n\
                    extension StringX on String {\n\
                    typedef Json = Map<String, dynamic>;\n\
                    void main() {\n\
                    Future<void> load(String id) async {\n\
                    Map<String, int> tally(List<String> rows) {\n\
                    final cache = compute(1);\n  \
                    Widget indented(BuildContext context) {\n\
                    }\n";
        let found = symbols("lib/features/chat/chat_service.dart", text);
        let pairs: Vec<(&str, &str)> = found
            .iter()
            .map(|symbol| (symbol.kind.as_str(), symbol.name.as_str()))
            .collect();
        assert_eq!(
            pairs,
            vec![
                ("class", "ChatService"),
                ("class", "Repository"),
                ("class", "Result"),
                ("enum", "Status"),
                ("mixin", "Logging"),
                ("extension", "StringX"),
                ("typedef", "Json"),
                ("fn", "main"),
                ("fn", "load"),
                ("fn", "tally"),
            ]
        );
    }

    /// A leading underscore is Dart's privacy rule, not a naming
    /// convention: `_ChatViewState` is library-private by language
    /// definition, so it is no more part of the file's public surface than
    /// a Rust item without `pub`.
    #[test]
    fn underscore_private_names_are_skipped() {
        let text = "class _ChatViewState {\n\
                    enum _Mode { a }\n\
                    typedef _Json = Map<String, dynamic>;\n\
                    void _helper() {\n\
                    class Public {\n";
        let found = symbols("lib/features/chat/chat_view.dart", text);
        let names: Vec<&str> = found.iter().map(|symbol| symbol.name.as_str()).collect();
        assert_eq!(names, vec!["Public"]);
    }
}
