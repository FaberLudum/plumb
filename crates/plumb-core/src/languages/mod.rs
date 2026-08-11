// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! The language table: one file per language, one registration line here.
//! Adding a language = implement `Language` in a new file, add it to `ALL`.

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

static ALL: [&(dyn Language + Sync); 1] = [&rust::Rust];

pub fn all() -> &'static [&'static (dyn Language + Sync)] {
    &ALL
}

pub fn for_extension(extension: &str) -> Option<&'static (dyn Language + Sync)> {
    all()
        .iter()
        .copied()
        .find(|language| language.extensions().contains(&extension))
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
                assert!(for_extension(ext).is_some(), "{ext} not dispatched");
            }
        }
    }

    #[test]
    fn for_extension_answers_nothing_for_an_unknown_extension() {
        assert!(for_extension("toml").is_none());
        assert!(for_extension("").is_none());
    }
}
