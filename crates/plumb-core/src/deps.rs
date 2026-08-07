// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Computes which dependencies a change adds.
//!
//! The comparison is a set difference between the names parsed from the
//! manifest at HEAD and the names parsed from the manifest on disk, not a read
//! of the `+` lines in the diff. Reformatting a manifest — reordering it,
//! respelling a name, repinning a version — produces a diff full of added
//! lines that add no dependency, and every name reported here becomes a
//! registry lookup that can block a build, so a phantom addition is a failed
//! build for no reason.

use crate::manifest::Ecosystem;
use std::collections::BTreeSet;

/// A dependency present after a change but not before it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddedDependency {
    pub name: String,
    pub ecosystem: Ecosystem,
    pub manifest: String,
}

/// Computes dependencies added by a change.
///
/// `before` is `None` when the manifest did not exist at HEAD, in which case
/// every name in `after` counts as added.
///
/// Results come out in the sorted order of the underlying `BTreeSet`, so the
/// findings a run reports do not depend on how the manifest was written.
///
/// `ecosystem` is the caller's, taken from the same [`crate::manifest::Manifest`]
/// that produced `after` — `manifest::parse` returns it alongside the names —
/// rather than guessed again from `manifest_path` here. A second filename
/// switch in this function would duplicate the dispatch rule `manifest::parse`
/// already owns, and the two would drift the first time either one learns a
/// new name. This used to hard-code `Ecosystem::PyPi` outright, which was
/// correct only while `Ecosystem` had one variant; a name added to a
/// package.json would otherwise have been looked up on PyPI and either
/// blocked a build over nothing or passed a hallucinated npm package.
pub fn added(
    manifest_path: &str,
    before: Option<&BTreeSet<String>>,
    after: &BTreeSet<String>,
    ecosystem: Ecosystem,
) -> Vec<AddedDependency> {
    let empty = BTreeSet::new();
    let before = before.unwrap_or(&empty);

    after
        .difference(before)
        .map(|name| AddedDependency {
            name: name.clone(),
            ecosystem,
            manifest: manifest_path.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reports_names_present_after_but_not_before() {
        let before = set(&["ruff"]);
        let after = set(&["ruff", "requestz"]);

        let added = added("requirements.txt", Some(&before), &after, Ecosystem::PyPi);

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "requestz");
        assert_eq!(added[0].manifest, "requirements.txt");
        assert_eq!(added[0].ecosystem, Ecosystem::PyPi);
    }

    #[test]
    fn treats_every_name_as_added_when_the_manifest_is_new() {
        let after = set(&["ruff", "httpx"]);

        let added = added("pyproject.toml", None, &after, Ecosystem::PyPi);

        let names: Vec<&str> = added.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["httpx", "ruff"]);
    }

    #[test]
    fn treats_an_empty_before_set_the_same_as_a_missing_manifest() {
        let before = BTreeSet::new();
        let after = set(&["ruff", "httpx"]);

        let added = added("pyproject.toml", Some(&before), &after, Ecosystem::PyPi);

        let names: Vec<&str> = added.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["httpx", "ruff"]);
    }

    #[test]
    fn reports_nothing_when_a_dependency_is_only_removed() {
        let before = set(&["ruff", "httpx"]);
        let after = set(&["ruff"]);

        assert!(added("requirements.txt", Some(&before), &after, Ecosystem::PyPi).is_empty());
    }

    #[test]
    fn reports_nothing_when_only_the_version_changed() {
        let before = set(&["ruff"]);
        let after = set(&["ruff"]);

        assert!(added("requirements.txt", Some(&before), &after, Ecosystem::PyPi).is_empty());
    }

    /// The npm sibling of `reports_names_present_after_but_not_before`:
    /// `added` tags each result with the ecosystem the caller passes, taken
    /// from the manifest that produced `after`, not guessed from the
    /// filename here.
    #[test]
    fn tags_added_dependencies_with_the_given_ecosystem() {
        let before = set(&["react"]);
        let after = set(&["react", "left-pad-typo"]);

        let added = added("package.json", Some(&before), &after, Ecosystem::Npm);

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "left-pad-typo");
        assert_eq!(added[0].manifest, "package.json");
        assert_eq!(added[0].ecosystem, Ecosystem::Npm);
    }
}
