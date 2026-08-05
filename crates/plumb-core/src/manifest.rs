// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Parses dependency manifests into distribution names, and into the parts of
//! the file those names did not come from.
//!
//! Names are normalised per PEP 503 — lowercased, with runs of `-`, `_` and
//! `.` collapsed to a single `-` — so that `Flask-SQLAlchemy` and
//! `flask_sqlalchemy` are one package rather than two, and the gate does not
//! report a phantom addition when a manifest is merely respelled.
//!
//! A name this module does not extract is not a name that is absent, and a
//! [`Manifest`] says which of the two it is. Poetry's `[tool.poetry]`, PEP 735
//! dependency groups and requirements includes are all unread here, and each is
//! reported as a [`Gap`] instead of passing for a file with nothing in it. The
//! rule that keeps a gap worth reading is that it is raised on evidence of an
//! unread declaration and never on the absence of one: a `pyproject.toml`
//! holding only a build system and some tool settings declares no runtime
//! dependencies and produces no gap.

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
    // Strip a UTF-8 BOM before trimming: U+FEFF is not `char::is_whitespace`,
    // so without this the first requirement of a BOM-prefixed file fails the
    // character check and is silently dropped. `toml` strips the BOM itself,
    // so this only matters for the requirements path.
    let spec = spec.trim_start_matches('\u{feff}');
    // Comments need no handling of their own. pip treats `#` as a comment
    // only at line start or after whitespace: after whitespace the terminator
    // search has already ended the name, and at line start the extracted text
    // begins with `#`, which the character check rejects. The remaining case,
    // `ruff#comment`, is malformed for pip, and dropping the whole line is the
    // conservative reading — an earlier `split('#')` here recovered `ruff` and
    // sent it to the registry instead, which is the wrong direction for a gate
    // that blocks on an unresolved name.
    //
    // This function is also called on `pyproject.toml` array entries, where
    // pip's comment rule does not apply at all. The conclusion survives: `#`
    // appears there only inside direct references such as
    // `mypkg@https://…#sha256=…`, where `@` ends the name first.
    let spec = spec.trim();
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

/// A manifest Plumb recognised: what it read, and what it did not.
///
/// An empty `gaps` is the only state that entitles a caller to say the file was
/// checked. Names and gaps coexist because a manifest can be partly understood
/// — a requirements file holding both direct requirements and an `-r` include
/// yields the direct names *and* a gap. An enum of mutually exclusive outcomes
/// would have to discard names that were read correctly, trading a silent false
/// negative for a loud loss of coverage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub names: BTreeSet<String>,
    pub gaps: Vec<Gap>,
}

/// A dependency declaration this module did not read.
///
/// Each variant names what was missed rather than carrying a flag, because the
/// warning has to tell a reader what to act on. "This file may declare more
/// dependencies" is true, useless, and the kind of message people learn to skip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gap {
    Unparseable,
    PoetryDependencies,
    DependencyGroups,
    Include(String),
}

impl Gap {
    /// A sentence naming what was not read, for a human reading a finding.
    pub fn describe(&self) -> String {
        match self {
            Gap::Unparseable => "the file does not parse as TOML".to_string(),
            Gap::PoetryDependencies => {
                "dependencies are declared in [tool.poetry.dependencies], which Plumb does not read"
                    .to_string()
            }
            Gap::DependencyGroups => {
                "dependencies are declared in [dependency-groups], which Plumb does not read"
                    .to_string()
            }
            Gap::Include(path) => {
                format!("requirements are included from `{path}`, which Plumb does not follow")
            }
        }
    }
}

/// Returns the path a `-r` or `--requirement` line includes.
///
/// pip accepts four spellings and all of them appear in the wild: `-r path`,
/// `-rpath`, `--requirement path` and `--requirement=path`.
///
/// The long form has to be followed by `=` or whitespace before its argument.
/// Requiring that is what keeps the match narrow: a bare `strip_prefix` would
/// read any future `--requirement`-prefixed option as an include. The short
/// form needs no such guard because pip glues its argument straight on.
fn include_target(line: &str) -> Option<String> {
    let line = line.trim_start_matches('\u{feff}').trim();

    let rest = if let Some(rest) = line.strip_prefix("--requirement") {
        match rest.strip_prefix('=') {
            Some(rest) => rest,
            None if rest.starts_with(char::is_whitespace) => rest,
            None => return None,
        }
    } else {
        line.strip_prefix("-r")?
    };

    let target = rest.trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}

/// Parses a `requirements.txt`-style file.
///
/// An `-r`/`--requirement` include is left unfollowed — following it would mean
/// reading another file, which this crate does not do — and recorded as a gap so
/// the omission is not silent.
pub fn parse_requirements(text: &str) -> Manifest {
    let mut names = BTreeSet::new();
    let mut gaps = Vec::new();

    for line in text.lines() {
        if let Some(target) = include_target(line) {
            gaps.push(Gap::Include(target));
            continue;
        }
        if let Some(name) = requirement_name(line) {
            names.insert(name);
        }
    }

    Manifest { names, gaps }
}

/// Parses `[project].dependencies` and `[project.optional-dependencies]` from
/// a `pyproject.toml`. Malformed input yields no names and a gap rather than an
/// error: a manifest we cannot read is not a violation, but it is not a clean
/// bill of health either.
pub fn parse_pyproject(text: &str) -> Manifest {
    let mut names = BTreeSet::new();
    let mut gaps = Vec::new();

    // `toml::Table`, not `toml::Value`: in toml 1.x `FromStr for Value` parses
    // a single value expression, so a whole document fails with "unexpected
    // content, expected nothing" and every pyproject.toml would look
    // malformed. `Table` is the document-level type.
    let Ok(document) = text.parse::<toml::Table>() else {
        return Manifest {
            names,
            gaps: vec![Gap::Unparseable],
        };
    };

    let project = document.get("project");

    if let Some(project) = project {
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
    }

    if document.get("dependency-groups").is_some() {
        gaps.push(Gap::DependencyGroups);
    }

    // Only a gap when nothing authoritative was read. A Poetry table beside a
    // PEP 621 `[project].dependencies` mirrors it, and warning there would fire
    // on projects Plumb reads correctly.
    //
    // `[build-system].requires` is deliberately absent from this list. It
    // appears in nearly every pyproject.toml, so treating it as a gap would
    // fire the warning universally — and a warning that always fires is a
    // warning nobody reads. Build-time dependencies are a separate rule with
    // separate evidence, not an unread part of the runtime set.
    let project_declares_dependencies = project.and_then(|p| p.get("dependencies")).is_some();
    let poetry_declares_dependencies = document
        .get("tool")
        .and_then(|t| t.get("poetry"))
        .and_then(|p| p.get("dependencies"))
        .is_some();

    if poetry_declares_dependencies && !project_declares_dependencies {
        gaps.push(Gap::PoetryDependencies);
    }

    Manifest { names, gaps }
}

/// Dispatches on filename. Returns `None` when the path is not a manifest
/// this version understands.
///
/// Note the two kinds of "not understood" this function keeps apart. `None`
/// means the path is not a manifest at all, and the caller should move on.
/// `Some(manifest)` with a non-empty [`Manifest::gaps`] means the file *is* a
/// manifest and part of it was not read, which the caller has to say out loud.
pub fn parse(path: &str, text: &str) -> Option<Manifest> {
    let file_name = path.rsplit('/').next().unwrap_or(path);

    if file_name == "pyproject.toml" {
        return Some(parse_pyproject(text));
    }
    // The character after `requirements` must be a separator. A bare prefix
    // match pulls in any prose file whose name merely starts that way —
    // `requirements_notes.txt` — and the pip parser then turns the first word
    // of every line into a distribution name. Each is an unresolvable lookup,
    // so English text manufactures blocking findings.
    //
    // Only `-` is accepted as that separator. This rejects two real
    // conventions, `requirements_dev.txt` and `requirements.dev.txt`, and
    // both are given up knowingly: admitting `_` would re-admit
    // `requirements_notes.txt` and admitting `.` would re-admit
    // `requirements.notes.txt`, so each would make the rule a no-op against
    // the prose it exists to reject. Prose and manifest are indistinguishable
    // by name, so one has to be given up, and the costs are not symmetric:
    // missing a manifest leaves dependencies unchecked, while parsing prose
    // blocks a build over packages that never existed. This module already
    // prefers the first error — malformed TOML yields no names, and so no
    // violation, only a gap.
    //
    // `-` itself is no cleaner; it is kept only because `requirements-dev.txt`
    // is too common to lose. `requirements-and-design.txt` still gets through,
    // as the tests record.
    if let Some(rest) = file_name.strip_prefix("requirements")
        && rest.ends_with(".txt")
        && (rest == ".txt" || rest.starts_with('-'))
    {
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
        assert_eq!(
            parse_requirements(text).names,
            set(&["httpx", "pytest", "ruff"])
        );
    }

    #[test]
    fn requirements_ignores_comments_blanks_and_flags() {
        let text = "# pinned\n\nruff==0.4.0\n-r other.txt\n-e .\n--index-url https://example.com\n";
        let manifest = parse_requirements(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        // The `-r` line contributes no name, as it always did, but it is no
        // longer *ignored*: it is the one option here that hides dependencies,
        // so it leaves a gap behind. `-e` and `--index-url` do not.
        assert_eq!(manifest.gaps, vec![Gap::Include("other.txt".to_string())]);
    }

    #[test]
    fn requirements_strips_extras_and_markers() {
        let text = "celery[redis]==5.3.0\nuvicorn ; python_version >= '3.9'\n";
        assert_eq!(parse_requirements(text).names, set(&["celery", "uvicorn"]));
    }

    #[test]
    fn requirements_rejects_lines_that_cannot_be_package_names() {
        let text = "<<<<<<< HEAD\nruff==0.4.0\n=======\nhttpx==0.27.0\n>>>>>>> feature\n";
        assert_eq!(parse_requirements(text).names, set(&["httpx", "ruff"]));
    }

    #[test]
    fn names_are_normalised_per_pep_503() {
        let text = "Flask_SQLAlchemy==3.0\nzope.interface==6.0\n";
        assert_eq!(
            parse_requirements(text).names,
            set(&["flask-sqlalchemy", "zope-interface"])
        );
    }

    #[test]
    fn normalisation_collapses_runs_of_separators() {
        // PEP 503 collapses a *run* of separators to one hyphen. Mapping each
        // separator individually would yield `foo--bar---baz` and every lookup
        // for such a name would 404 and block the build.
        let text = "Foo--Bar_.-baz==1.0\n";
        assert_eq!(parse_requirements(text).names, set(&["foo-bar-baz"]));
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
        assert_eq!(parse_requirements(text).names, set(&["ruff"]));
    }

    #[test]
    fn requirements_do_not_look_up_an_unspaced_hash() {
        // pip treats `#` as a comment only at line start or after whitespace,
        // so `ruff#comment` is malformed rather than a commented requirement.
        // Splitting on `#` would recover `ruff` from it and send that to the
        // registry; letting the character check reject the whole line is the
        // conservative reading, and on a blocking gate that is the one to
        // take. Legal comments are unaffected — whitespace already ends the
        // name — which the neighbouring tests cover.
        assert_eq!(parse_requirements("ruff#comment\n").names, set(&[]));
        assert_eq!(
            parse_requirements("ruff==0.4.0  # comment\n# whole line\n").names,
            set(&["ruff"])
        );
    }

    #[test]
    fn requirements_survive_a_leading_utf8_bom() {
        // U+FEFF is not `char::is_whitespace`, so `trim()` leaves it in place
        // and the first requirement of a BOM-prefixed file would fail the
        // character check and vanish — a hallucinated package on line one
        // would go unreported.
        let text = "\u{feff}ruff==0.4.0\nhttpx\n";
        assert_eq!(parse_requirements(text).names, set(&["httpx", "ruff"]));
    }

    #[test]
    fn requirements_tolerate_leading_whitespace() {
        // Pins the `trim()` in `requirement_name`. Without it the terminator
        // search stops on the leading space at index 0, the extracted name is
        // empty, and an indented requirement is dropped.
        let text = "    ruff==0.4.0\n\thttpx\n";
        assert_eq!(parse_requirements(text).names, set(&["httpx", "ruff"]));
    }

    #[test]
    fn parse_requires_a_separator_after_requirements() {
        // Prose really does yield names, which is what makes the dispatch
        // rule load-bearing rather than cosmetic.
        const PROSE: &str = "The system must support X\nfast delivery\n";
        assert_eq!(parse_requirements(PROSE).names, set(&["fast", "the"]));

        assert!(parse("requirements_notes.txt", PROSE).is_none());
        assert!(parse("requirements.notes.txt", PROSE).is_none());
        assert!(parse("requirementstxt.txt", PROSE).is_none());
        // `_` and `.` are both excluded deliberately, giving up these two real
        // conventions to reject the prose above: see `parse`.
        assert!(parse("requirements_dev.txt", "ruff\n").is_none());
        assert!(parse("requirements.dev.txt", "ruff\n").is_none());

        assert_eq!(
            parse("requirements.txt", "ruff\n").map(|m| m.names),
            Some(set(&["ruff"]))
        );
        assert_eq!(
            parse("requirements-dev.txt", "ruff\n").map(|m| m.names),
            Some(set(&["ruff"]))
        );

        // Recorded, not endorsed. `requirements-dev.txt` is the dominant real
        // manifest name, so `-` has to be accepted, and no filename rule can
        // then tell `requirements-and-design.txt` apart from it. The separator
        // requirement narrows this hole; it does not close it.
        assert_eq!(
            parse("requirements-and-design.txt", PROSE).map(|m| m.names),
            Some(set(&["fast", "the"]))
        );
    }

    #[test]
    fn pyproject_reads_project_dependencies() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff==0.4.0", "httpx>=0.27"]
"#;
        assert_eq!(parse_pyproject(text).names, set(&["httpx", "ruff"]));
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
        assert_eq!(
            parse_pyproject(text).names,
            set(&["mkdocs", "pytest", "ruff"])
        );
    }

    #[test]
    fn parse_dispatches_on_filename_and_ignores_other_files() {
        assert_eq!(
            parse("requirements.txt", "ruff\n").map(|m| m.names),
            Some(set(&["ruff"]))
        );
        assert_eq!(
            parse("requirements-dev.txt", "pytest>=8.0\n").map(|m| m.names),
            Some(set(&["pytest"]))
        );
        // Asserting the names, not just `is_some`, pins which parser each name
        // reaches: run through parse_requirements this document would yield
        // `dependencies` from the assignment's left-hand side.
        assert_eq!(
            parse("pyproject.toml", "[project]\ndependencies = [\"httpx\"]\n").map(|m| m.names),
            Some(set(&["httpx"]))
        );
        assert_eq!(
            parse(
                "packages/api/pyproject.toml",
                "[project]\ndependencies = []\n"
            )
            .map(|m| m.names),
            Some(BTreeSet::new())
        );
        assert!(parse("src/app.py", "import os\n").is_none());
    }

    #[test]
    fn an_ordinary_pyproject_has_no_gaps() {
        let text = r#"
[build-system]
requires = ["setuptools>=61"]
build-backend = "setuptools.build_meta"

[project]
name = "demo"
dependencies = ["ruff==0.4.0"]

[tool.ruff]
line-length = 100
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn a_pyproject_declaring_nothing_has_no_gaps() {
        let text =
            "[build-system]\nrequires = [\"setuptools\"]\n\n[tool.ruff]\nline-length = 100\n";
        let manifest = parse_pyproject(text);
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn poetry_dependencies_without_a_project_table_are_a_gap() {
        let text = "[tool.poetry.dependencies]\nrequests = \"^2.0\"\n";
        let manifest = parse_pyproject(text);
        assert!(manifest.names.is_empty());
        assert_eq!(manifest.gaps, vec![Gap::PoetryDependencies]);
    }

    #[test]
    fn a_poetry_table_beside_project_dependencies_is_not_a_gap() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff==0.4.0"]

[tool.poetry.dependencies]
ruff = "^0.4"
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn dependency_groups_are_a_gap_even_beside_a_read_project_table() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff==0.4.0"]

[dependency-groups]
dev = ["pytest"]
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        assert_eq!(manifest.gaps, vec![Gap::DependencyGroups]);
    }

    #[test]
    fn malformed_toml_is_a_gap_rather_than_an_empty_manifest() {
        let manifest = parse_pyproject("this is not toml {{{");
        assert!(manifest.names.is_empty());
        assert_eq!(manifest.gaps, vec![Gap::Unparseable]);
    }

    #[test]
    fn a_requirements_include_is_a_gap_and_the_direct_names_survive() {
        let text = "ruff==0.4.0\n-r requirements/base.txt\nhttpx==0.27.0\n";
        let manifest = parse_requirements(text);
        assert_eq!(manifest.names, set(&["httpx", "ruff"]));
        assert_eq!(
            manifest.gaps,
            vec![Gap::Include("requirements/base.txt".to_string())]
        );
    }

    #[test]
    fn every_spelling_of_an_include_is_recognised() {
        for line in [
            "-r base.txt",
            "-rbase.txt",
            "--requirement base.txt",
            "--requirement=base.txt",
        ] {
            let manifest = parse_requirements(&format!("{line}\n"));
            assert_eq!(
                manifest.gaps,
                vec![Gap::Include("base.txt".to_string())],
                "line: {line}"
            );
        }
    }

    #[test]
    fn an_editable_install_is_not_an_include() {
        let manifest = parse_requirements("-e .\n--index-url https://example.com\n");
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn a_gap_says_what_was_not_read() {
        assert!(
            Gap::PoetryDependencies
                .describe()
                .contains("tool.poetry.dependencies")
        );
        assert!(
            Gap::DependencyGroups
                .describe()
                .contains("dependency-groups")
        );
        assert!(
            Gap::Include("base.txt".to_string())
                .describe()
                .contains("base.txt")
        );
        assert!(!Gap::Unparseable.describe().is_empty());
    }
}
