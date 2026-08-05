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
pub fn added(
    manifest_path: &str,
    before: Option<&BTreeSet<String>>,
    after: &BTreeSet<String>,
) -> Vec<AddedDependency> {
    let empty = BTreeSet::new();
    let before = before.unwrap_or(&empty);

    after
        .difference(before)
        .map(|name| AddedDependency {
            name: name.clone(),
            // Correct while `manifest::parse` recognises only Python
            // manifests, and silently wrong the moment it does not: nothing
            // here fails to compile when a second ecosystem appears, and a
            // name looked up in the wrong registry either blocks a build or
            // passes a hallucinated package. The fix then is for
            // `manifest::parse` to return the ecosystem alongside the names
            // and for this function to take it as an argument — not a second
            // filename switch here, which would duplicate a dispatch rule
            // that is already subtle.
            ecosystem: Ecosystem::PyPi,
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

        let added = added("requirements.txt", Some(&before), &after);

        assert_eq!(added.len(), 1);
        assert_eq!(added[0].name, "requestz");
        assert_eq!(added[0].manifest, "requirements.txt");
        assert_eq!(added[0].ecosystem, Ecosystem::PyPi);
    }

    #[test]
    fn treats_every_name_as_added_when_the_manifest_is_new() {
        let after = set(&["ruff", "httpx"]);

        let added = added("pyproject.toml", None, &after);

        let names: Vec<&str> = added.iter().map(|d| d.name.as_str()).collect();
        assert_eq!(names, vec!["httpx", "ruff"]);
    }

    #[test]
    fn reports_nothing_when_a_dependency_is_only_removed() {
        let before = set(&["ruff", "httpx"]);
        let after = set(&["ruff"]);

        assert!(added("requirements.txt", Some(&before), &after).is_empty());
    }

    #[test]
    fn reports_nothing_when_only_the_version_changed() {
        let before = set(&["ruff"]);
        let after = set(&["ruff"]);

        assert!(added("requirements.txt", Some(&before), &after).is_empty());
    }
}
