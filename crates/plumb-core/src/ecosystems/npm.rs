// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! npm: `package.json`.

use crate::manifest::{Ecosystem, Gap, Manifest};
use std::collections::BTreeSet;

/// The four npm dependency tables whose keys this module reads as package
/// names. Order matches `package.json`'s own conventional ordering, so a
/// file with more than one malformed table reports its gaps in the order a
/// reader would meet them.
const DEPENDENCY_TABLES: [&str; 4] = [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
];

/// Parses the four dependency tables of a `package.json`: `dependencies`,
/// `devDependencies`, `peerDependencies` and `optionalDependencies`. Only the
/// keys are read — a table's values are version ranges, which this gate does
/// not validate — and an absent table declares nothing, the same non-signal
/// an absent `[project.optional-dependencies]` group is.
///
/// Mirrors `ecosystems::pypi::parse_pyproject`'s reading of
/// `[project].dependencies`: a
/// malformed table is positive evidence of an intended declaration nothing
/// came of, so it is a [`Gap::MalformedTable`] rather than silence, and a
/// file that does not parse as JSON at all is [`Gap::Unparseable`], exactly
/// as a `pyproject.toml` that does not parse as TOML is.
pub fn parse_package_json(text: &str) -> Manifest {
    let mut names = BTreeSet::new();
    let mut gaps = Vec::new();

    // An empty body is not valid JSON on its own — unlike an empty TOML
    // document, which `text.parse::<toml::Table>()` reads as a document with
    // nothing in it — but it is what `parse`'s probe hands this function to
    // ask "is this filename recognised", not evidence that a real file
    // failed to parse. Without this the probe would attach a gap to every
    // changed package.json in every repository, the JSON-specific failure
    // the TOML probe never had to guard against.
    //
    // The check is against the raw string, not `text.trim()`. The probe
    // always sends the literal empty string (see `check.rs`'s use of
    // `manifest::parse(&path, "")`), so trimming buys it nothing; what it
    // did buy was exempting a real, non-empty file that holds only
    // whitespace — a `package.json` reduced to blank lines by a bad merge or
    // a botched edit — from `Gap::Unparseable`, even though whitespace is
    // exactly as invalid to a JSON parser as any other non-JSON text.
    if text.is_empty() {
        return Manifest {
            names,
            gaps,
            ecosystem: Ecosystem::Npm,
        };
    }

    let Ok(document) = serde_json::from_str::<serde_json::Value>(text) else {
        return Manifest {
            names,
            gaps: vec![Gap::Unparseable],
            ecosystem: Ecosystem::Npm,
        };
    };

    // Every `as_object()` below is a filter, the same shape `parse_pyproject`
    // takes for `as_array()`/`as_table()`: a table that is present but not
    // the shape the reader takes is positive evidence of an intended
    // declaration nothing came of, so it leaves a `MalformedTable` gap
    // rather than passing over it in silence.
    for table in DEPENDENCY_TABLES {
        if let Some(value) = document.get(table) {
            match value.as_object() {
                Some(map) => names.extend(map.keys().cloned()),
                None => gaps.push(Gap::MalformedTable(table.to_string())),
            }
        }
    }

    Manifest {
        names,
        gaps,
        ecosystem: Ecosystem::Npm,
    }
}

/// Whether `path` places a `package.json` under a vendored `node_modules/`
/// tree.
///
/// A vendored `package.json` belongs to a dependency, not to this project —
/// it was not written by anyone here, so checking it would block a build over
/// a package nobody added. Checked against the whole path, at any depth,
/// because npm nests dependencies:
/// `node_modules/foo/node_modules/bar/package.json` is exactly as vendored as
/// the top-level case.
///
/// This is a full-path property, and it is why `manifest::parse` checks it
/// itself rather than folding it into [`Npm::matches`]: `EcosystemSpec`'s
/// `matches` only ever sees the final path segment (see
/// `ecosystems::for_path`), which is not enough information to tell a
/// vendored `package.json` from a project's own.
pub(crate) fn is_vendored(path: &str) -> bool {
    path.split('/').any(|segment| segment == "node_modules")
}

/// npm: `package.json`.
pub struct Npm;

impl super::EcosystemSpec for Npm {
    fn ecosystem(&self) -> Ecosystem {
        Ecosystem::Npm
    }

    fn matches(&self, filename: &str) -> bool {
        filename == "package.json"
    }

    /// npm has one manifest syntax, so `filename` carries nothing `text`
    /// does not already say — unlike PyPI's dispatch, which needs it to tell
    /// two syntaxes apart.
    fn parse(&self, _filename: &str, text: &str) -> Manifest {
        parse_package_json(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn package_json_reads_the_four_dependency_tables() {
        let text = r#"{
            "dependencies": { "react": "^18.0.0" },
            "devDependencies": { "eslint": "^9.0.0" },
            "peerDependencies": { "react-dom": "^18.0.0" },
            "optionalDependencies": { "fsevents": "^2.0.0" }
        }"#;
        let manifest = parse_package_json(text);
        assert_eq!(
            manifest.names,
            set(&["react", "eslint", "react-dom", "fsevents"])
        );
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
        assert_eq!(manifest.ecosystem, Ecosystem::Npm);
    }

    #[test]
    fn package_json_with_no_dependency_tables_declares_nothing() {
        let text = r#"{ "name": "demo", "version": "1.0.0" }"#;
        let manifest = parse_package_json(text);
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// A declaration in the wrong shape is a declaration, and it was not
    /// read — the JSON sibling of
    /// `a_project_dependencies_that_is_not_an_array_is_a_gap`.
    #[test]
    fn a_non_object_dependency_table_is_a_gap() {
        let text = r#"{ "dependencies": ["react"] }"#;
        let manifest = parse_package_json(text);
        assert!(manifest.names.is_empty());
        assert_eq!(
            manifest.gaps,
            vec![Gap::MalformedTable("dependencies".to_string())]
        );
        assert!(manifest.gaps[0].describe().contains("dependencies"));
    }

    /// A malformed table beside a well-formed one still yields the names the
    /// well-formed table declares: a partly understood file keeps its names,
    /// the same rule `a_malformed_optional_dependency_group_is_a_gap` pins
    /// for `pyproject.toml`.
    #[test]
    fn a_malformed_table_does_not_hide_names_from_a_well_formed_one() {
        let text = r#"{
            "dependencies": { "react": "^18.0.0" },
            "devDependencies": ["eslint"]
        }"#;
        let manifest = parse_package_json(text);
        assert_eq!(manifest.names, set(&["react"]));
        assert_eq!(
            manifest.gaps,
            vec![Gap::MalformedTable("devDependencies".to_string())]
        );
    }

    #[test]
    fn package_json_that_is_not_json_is_unparseable() {
        let manifest = parse_package_json("not json at all");
        assert!(manifest.names.is_empty());
        assert_eq!(manifest.gaps, vec![Gap::Unparseable]);
    }

    /// The empty-body probe `check.rs`'s dispatch relies on: an empty string
    /// is not valid JSON on its own, unlike an empty TOML document, so
    /// without a special case every changed `package.json` in every
    /// repository would carry a manufactured gap. Mirrors
    /// `a_manifest_is_recognised_from_its_path_alone` in `plumb-cli`, which
    /// pins the same contract through `parse`.
    #[test]
    fn an_empty_package_json_body_has_no_gaps() {
        let manifest = parse_package_json("");
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// A body that is *not* empty but holds only whitespace is not the
    /// probe [`an_empty_package_json_body_has_no_gaps`] pins — it is invalid
    /// JSON to npm itself, exactly as `"not json at all"` is, and a real
    /// `package.json` reduced to blank lines by a bad merge or a botched
    /// edit deserves the same [`Gap::Unparseable`] any other unparseable
    /// file gets, not the silent pass the empty-string probe is exempted
    /// into.
    #[test]
    fn a_whitespace_only_non_empty_package_json_body_is_unparseable() {
        let manifest = parse_package_json("   \n\t  \n");
        assert!(manifest.names.is_empty());
        assert_eq!(manifest.gaps, vec![Gap::Unparseable]);
    }
}
