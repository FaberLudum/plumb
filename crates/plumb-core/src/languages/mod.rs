// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! The language table: one file per language, one registration line here.
//! Adding a language = implement `Language` in a new file, add it to `ALL`.

mod csharp;
mod dart;
mod javascript;
mod python;
mod rust;

use crate::index::Symbol;

pub trait Language {
    /// Lowercased extensions this language claims.
    fn extensions(&self) -> &'static [&'static str];
    /// Import paths as written, line-based heuristic (see boundaries.rs docs).
    fn imports(&self, text: &str) -> Vec<String>;
    /// Public symbols, line-based heuristic (see index.rs docs).
    fn symbols(&self, text: &str) -> Vec<Symbol>;
}

static ALL: [&(dyn Language + Sync); 5] = [
    &rust::Rust,
    &python::Python,
    &javascript::JavaScript,
    &csharp::CSharp,
    &dart::Dart,
];

pub fn all() -> &'static [&'static (dyn Language + Sync)] {
    &ALL
}

pub fn for_extension(extension: &str) -> Option<&'static (dyn Language + Sync)> {
    all()
        .iter()
        .copied()
        .find(|language| language.extensions().contains(&extension))
}

/// The lowercased extension of `path`'s final path segment, or empty when it
/// has none.
///
/// Two things a caller must not have to know: a path with no `.` in its
/// final segment has no extension — `rsplit('.')` alone answers that with the
/// whole remaining string instead, which a dotless file named exactly `rs`
/// would then match through [`for_extension`]'s table dispatch as if it were
/// a Rust file — and matching must be case-insensitive, since `LIB.RS` is as
/// much a Rust file as `lib.rs`.
pub(crate) fn extension_of(path: &str) -> String {
    let name = path.rsplit('/').next().unwrap_or(path);
    match name.rsplit_once('.') {
        Some((_, ext)) if !ext.is_empty() => ext.to_lowercase(),
        _ => String::new(),
    }
}

/// Extract an identifier from the start of a string.
///
/// Shared across the language extractors that need it (`rust.rs`,
/// `javascript.rs`, reached as `super::identifier`); private items of a
/// module are visible to its descendants, so a plain `fn` here is already
/// visible to every submodule without widening it further.
fn identifier(text: &str) -> String {
    text.chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_')
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_extension_is_claimed_by_two_languages() {
        let mut seen: Vec<&str> = Vec::new();
        for language in all() {
            for ext in language.extensions() {
                assert!(!seen.contains(ext), "{ext} claimed twice");
                seen.push(ext);
            }
        }
    }

    #[test]
    fn every_language_claims_at_least_one_extension() {
        for language in all() {
            assert!(!language.extensions().is_empty());
        }
    }

    #[test]
    fn for_extension_answers_for_every_declared_extension() {
        for language in all() {
            for ext in language.extensions() {
                // Probe through the real path-lowering path a caller
                // actually takes (`extension_of` a file named `x.<ext>`),
                // not `for_extension(ext)` directly — the direct probe
                // bypasses `extension_of`'s lowercasing, so a declared
                // extension with an uppercase letter would pass this test
                // while never dispatching in production.
                assert!(
                    for_extension(&extension_of(&format!("x.{ext}"))).is_some(),
                    "{ext} not dispatched through extension_of"
                );
            }
        }
    }

    /// Every declared extension must already be lowercase. `extension_of`
    /// lowercases whatever it reads off a path before `for_extension` ever
    /// sees it, so an uppercase entry in a `Language::extensions()` table
    /// can never be reached in production even though it would satisfy a
    /// test that probes `for_extension` directly with the declared string.
    /// This pins that invariant for extensions declared in the future.
    #[test]
    fn declared_extensions_are_already_lowercase() {
        for language in all() {
            for ext in language.extensions() {
                assert_eq!(
                    *ext,
                    ext.to_lowercase().as_str(),
                    "{ext} declares an uppercase extension, which extension_of's \
                     lowercasing means for_extension can never dispatch to in production"
                );
            }
        }
    }

    #[test]
    fn for_extension_answers_nothing_for_an_unknown_extension() {
        assert!(for_extension("toml").is_none());
        assert!(for_extension("").is_none());
    }
}
