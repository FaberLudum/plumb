// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Parses dependency manifests into sets of distribution names.
//!
//! Names are normalised per PEP 503 — lowercased, with runs of `-`, `_` and
//! `.` collapsed to a single `-` — so that `Flask-SQLAlchemy` and
//! `flask_sqlalchemy` are one package rather than two, and the gate does not
//! report a phantom addition when a manifest is merely respelled.

use std::collections::BTreeSet;

/// The package registry a dependency belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecosystem {
    PyPi,
}

/// Normalises a distribution name per PEP 503.
fn normalise(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    let mut last_was_separator = false;
    for ch in name.trim().chars() {
        if ch == '-' || ch == '_' || ch == '.' {
            if !last_was_separator {
                out.push('-');
                last_was_separator = true;
            }
        } else {
            out.push(ch.to_ascii_lowercase());
            last_was_separator = false;
        }
    }
    out
}

/// Extracts the distribution name from a PEP 508 requirement string.
///
/// Every name returned here becomes a registry lookup, and a name that does
/// not resolve becomes a blocking finding, so anything that is not certainly a
/// name is dropped. PEP 508 admits only ASCII alphanumerics, `-`, `_` and `.`
/// in a name, and requires the first and last character to be alphanumeric, so
/// after PEP 503 normalisation a name is ASCII letters and digits with hyphens
/// only in the interior. That rejects lines which are not requirements at all
/// — notably the `<<<<<<<`, `=======` and `>>>>>>>` markers git writes into a
/// conflicted manifest, which a diff reports as added lines.
///
/// Anchoring both ends is what rules out a name made only of separators: `.`
/// is a legal requirements.txt line naming the project in the current
/// directory, and it normalises to `-`, which a check on the character set
/// alone would accept and send to the registry as a package.
fn requirement_name(spec: &str) -> Option<String> {
    let spec = spec.split('#').next().unwrap_or(spec).trim();
    if spec.is_empty() || spec.starts_with('-') {
        return None;
    }
    let end = spec
        .find(|c: char| c.is_whitespace() || "[<>=!~;@(".contains(c))
        .unwrap_or(spec.len());
    let name = normalise(&spec[..end]);
    if !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
        return None;
    }
    let anchored = name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.ends_with(|c: char| c.is_ascii_alphanumeric());
    if !anchored {
        return None;
    }
    Some(name)
}

/// Parses a `requirements.txt`-style file into a set of distribution names.
pub fn parse_requirements(text: &str) -> BTreeSet<String> {
    text.lines().filter_map(requirement_name).collect()
}

/// Parses `[project].dependencies` and `[project.optional-dependencies]` from
/// a `pyproject.toml`. Malformed input yields an empty set rather than an
/// error: a manifest we cannot read is not a violation.
pub fn parse_pyproject(text: &str) -> BTreeSet<String> {
    let mut names = BTreeSet::new();

    // `toml::Table`, not `toml::Value`: in toml 1.x `FromStr for Value` parses
    // a single value expression, so a whole document fails with "unexpected
    // content, expected nothing" and every pyproject.toml would look
    // malformed. `Table` is the document-level type.
    let Ok(document) = text.parse::<toml::Table>() else {
        return names;
    };
    let Some(project) = document.get("project") else {
        return names;
    };

    if let Some(list) = project.get("dependencies").and_then(|d| d.as_array()) {
        for entry in list {
            if let Some(name) = entry.as_str().and_then(requirement_name) {
                names.insert(name);
            }
        }
    }

    if let Some(groups) = project
        .get("optional-dependencies")
        .and_then(|d| d.as_table())
    {
        for group in groups.values() {
            let Some(list) = group.as_array() else {
                continue;
            };
            for entry in list {
                if let Some(name) = entry.as_str().and_then(requirement_name) {
                    names.insert(name);
                }
            }
        }
    }

    names
}

/// Dispatches on filename. Returns `None` when the path is not a manifest
/// this version understands.
pub fn parse(path: &str, text: &str) -> Option<BTreeSet<String>> {
    let file_name = path.rsplit('/').next().unwrap_or(path);

    if file_name == "pyproject.toml" {
        return Some(parse_pyproject(text));
    }
    if file_name.starts_with("requirements") && file_name.ends_with(".txt") {
        return Some(parse_requirements(text));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn requirements_strips_version_specifiers() {
        let text = "ruff==0.4.0\npytest>=8.0\nhttpx\n";
        assert_eq!(parse_requirements(text), set(&["httpx", "pytest", "ruff"]));
    }

    #[test]
    fn requirements_ignores_comments_blanks_and_flags() {
        let text = "# pinned\n\nruff==0.4.0\n-r other.txt\n-e .\n--index-url https://example.com\n";
        assert_eq!(parse_requirements(text), set(&["ruff"]));
    }

    #[test]
    fn requirements_strips_extras_and_markers() {
        let text = "celery[redis]==5.3.0\nuvicorn ; python_version >= '3.9'\n";
        assert_eq!(parse_requirements(text), set(&["celery", "uvicorn"]));
    }

    #[test]
    fn requirements_rejects_lines_that_cannot_be_package_names() {
        let text = "<<<<<<< HEAD\nruff==0.4.0\n=======\nhttpx==0.27.0\n>>>>>>> feature\n";
        assert_eq!(parse_requirements(text), set(&["httpx", "ruff"]));
    }

    #[test]
    fn names_are_normalised_per_pep_503() {
        let text = "Flask_SQLAlchemy==3.0\nzope.interface==6.0\n";
        assert_eq!(
            parse_requirements(text),
            set(&["flask-sqlalchemy", "zope-interface"])
        );
    }

    #[test]
    fn normalisation_collapses_runs_of_separators() {
        // PEP 503 collapses a *run* of separators to one hyphen. Mapping each
        // separator individually would yield `foo--bar---baz` and every lookup
        // for such a name would 404 and block the build.
        let text = "Foo--Bar_.-baz==1.0\n";
        assert_eq!(parse_requirements(text), set(&["foo-bar-baz"]));
    }

    #[test]
    fn requirements_rejects_a_bare_path_requirement() {
        // `.` is a legal requirements.txt line meaning "the project in this
        // directory". It normalises to `-`, which is made only of characters a
        // name may contain, so a check on characters alone would emit it and
        // block the build on a package that cannot exist.
        // `numpy_` pins the trailing anchor and `_leading` the leading one;
        // with either half dropped one of these reaches the registry.
        let text = ".\n..\nnumpy_\n_leading==1.0\nruff==0.4.0\n";
        assert_eq!(parse_requirements(text), set(&["ruff"]));
    }

    #[test]
    fn pyproject_reads_project_dependencies() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff==0.4.0", "httpx>=0.27"]
"#;
        assert_eq!(parse_pyproject(text), set(&["httpx", "ruff"]));
    }

    #[test]
    fn pyproject_reads_optional_dependency_groups() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff"]

[project.optional-dependencies]
dev = ["pytest>=8.0"]
docs = ["mkdocs"]
"#;
        assert_eq!(parse_pyproject(text), set(&["mkdocs", "pytest", "ruff"]));
    }

    #[test]
    fn pyproject_returns_empty_when_malformed() {
        assert!(parse_pyproject("this is not toml {{{").is_empty());
    }

    #[test]
    fn parse_dispatches_on_filename_and_ignores_other_files() {
        assert_eq!(parse("requirements.txt", "ruff\n"), Some(set(&["ruff"])));
        assert_eq!(
            parse("requirements-dev.txt", "pytest>=8.0\n"),
            Some(set(&["pytest"]))
        );
        // Asserting the names, not just `is_some`, pins which parser each name
        // reaches: run through parse_requirements this document would yield
        // `dependencies` from the assignment's left-hand side.
        assert_eq!(
            parse("pyproject.toml", "[project]\ndependencies = [\"httpx\"]\n"),
            Some(set(&["httpx"]))
        );
        assert_eq!(
            parse(
                "packages/api/pyproject.toml",
                "[project]\ndependencies = []\n"
            ),
            Some(BTreeSet::new())
        );
        assert_eq!(parse("src/app.py", "import os\n"), None);
    }
}
