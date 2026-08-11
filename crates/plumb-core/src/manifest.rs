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
//! [`Manifest`] says which of the two it is. Poetry's dependency tables — the
//! main one and the per-group ones — PEP 735 `[dependency-groups]`, PEP 621
//! `dynamic` dependencies, any dependency table under `[tool]`, a declaration
//! written in the wrong shape, and requirements includes are all unread here,
//! and each is reported as a [`Gap`] instead of passing for a file with nothing
//! in it. The rule that keeps a gap worth reading is that it is raised on
//! evidence of an unread declaration and never on the absence of one: a
//! `pyproject.toml` holding only a build system and some tool settings declares
//! no runtime dependencies and produces no gap.
//!
//! The `[tool]` rule is structural — a key named `dependencies` or ending in
//! `-dependencies`, at any depth — rather than a list of tools, and
//! `ecosystems::pypi::tool_dependency_paths` carries the argument for why.
//! What it does not cover is a tool that spells its dependency list some
//! other way: `[tool.tox.env_run_base].deps`,
//! `[tool.briefcase.app.<name>].requires` and
//! `[tool.cibuildwheel].test-requires` are read as ordinary configuration and
//! produce no gap. That is a real remaining silence, not a case this module has
//! an answer for.
//!
//! Conda is silent too, but by decision rather than by omission: this module
//! reads one registry, and a conda name is not an unread part of the PyPI set.
//! See `ecosystems::pypi::is_conda_dependency_path`.
//!
//! One exemption is worth knowing about, because it rests on someone else's
//! documented behaviour rather than on anything checkable here.
//! `[tool.poetry.dependencies]` raises no gap when `[project].dependencies` is
//! present, on Poetry's statement that in that configuration the Poetry table
//! only enriches the PEP 621 one for locking and cannot introduce a name. If
//! that ever stops holding, such a file reads as fully understood while it is
//! not. Group tables carry no such exemption — see
//! `ecosystems::pypi::parse_pyproject`.
//!
//! A file this module does not dispatch on at all — `setup.py`, `Pipfile`, a
//! lock file — is a separate silence and not one a [`Gap`] can express: [`parse`]
//! returning `None` says "not a manifest I handle", never "this project has no
//! dependencies".
//!
//! The same three-way split extends to `package.json`. The four npm
//! dependency tables read as name-to-version-range maps and only the keys
//! matter; a table that is declared but is not an object at all is a
//! [`Gap::MalformedTable`], the JSON sibling of [`Gap::Malformed`]. A
//! `package.json` under `node_modules/`, at any depth, belongs to a
//! dependency rather than to this project, so it is not recognised at all —
//! [`parse`] returns `None` for it, the same answer a lock file gets.

use std::collections::BTreeSet;

/// The package registry a dependency belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ecosystem {
    PyPi,
    Npm,
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
    /// The registry `names` belongs to, so a caller does not have to guess
    /// it back from the manifest's filename. `deps::added` takes this as an
    /// argument for exactly that reason — see its doc comment.
    pub ecosystem: Ecosystem,
}

/// A dependency declaration this module did not read.
///
/// Each variant names what was missed rather than carrying a flag, because the
/// warning has to tell a reader what to act on. "This file may declare more
/// dependencies" is true, useless, and the kind of message people learn to skip.
///
/// # Why this warns on every run, and why that is the intended cost
///
/// A repository that declares its dependencies with Poetry or PEP 735 gets a
/// gap on every change that touches its `pyproject.toml`, for as long as those
/// formats go unparsed. A caller that turns gaps into warnings therefore exits
/// `2` every time. That is deliberate, and it is not the failure mode a warning
/// that always fires is.
///
/// The distinction is what the warning is keyed to. `[build-system].requires`
/// is excluded from these variants precisely because it is in nearly every
/// `pyproject.toml`, so a gap for it would fire independently of whether Plumb
/// could see anything — noise carrying no information. A gap here fires exactly
/// when Plumb cannot see a project's dependencies, which is information, and it
/// is information the person reading a clean run most needs: a gate reporting
/// success over a file it did not understand is the silent false negative this
/// whole project exists to catch. It is what these formats did for the entire
/// previous version.
///
/// The exit code is the right channel for it rather than a side note. `2` means
/// warnings only and the contract blocks on `1`, so a pipeline wired to that
/// contract is unaffected; one that treats any non-zero as failure has decided
/// warnings block, which is its decision to make. The alternative — a channel
/// that never touches the exit code — is what the unreadable-manifest note
/// already is, and it is what let these formats pass as clean.
///
/// The noise is proportional to the blindness and has an obvious off switch: it
/// stops when someone teaches Plumb to read the format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gap {
    Unparseable,
    /// Dependencies under `[tool.poetry]`, in the main table or in a group.
    ///
    /// One variant for two tables. They are separate declarations with
    /// different conditions — see `parse_pyproject` — but the reader's action
    /// is identical, and a project with both is the ordinary Poetry layout, so
    /// splitting them would put two findings on one file saying the same thing
    /// twice. `describe` names both tables, so the sentence stays specific
    /// enough to act on whichever one the file actually has.
    PoetryDependencies,
    DependencyGroups,
    /// `[project].dynamic` lists a dependency field.
    ///
    /// The listed fields, in the order PEP 621 documents them, never empty.
    /// One gap naming both rather than one per field, for the reason
    /// [`Gap::PoetryDependencies`] gives: it is one unread layout, so it is one
    /// action.
    Dynamic(Vec<String>),
    /// A declaration whose value is not the shape the reader takes, named by
    /// its dotted TOML path.
    ///
    /// The key is positive evidence of an intended declaration; the shape is
    /// why nothing came of it. Without this the reader's `as_array()` and
    /// `as_table()` are silent filters, and a file reports as fully understood
    /// because of a typo.
    Malformed(String),
    /// Dependency declarations under `[tool]` that this module does not read,
    /// named by their dotted TOML paths, sorted and never empty.
    ///
    /// One variant rather than one per tool, and the path is what makes that
    /// work: the reader is told `tool.pdm.dev-dependencies`, not "some tool
    /// declares dependencies". A variant per tool would be the list of tools
    /// this rule exists to avoid, and it would put a second name in the code
    /// for every name already in the file.
    ToolDependencies(Vec<String>),
    Include(String),
    /// A `package.json` dependency table (`dependencies`, `devDependencies`,
    /// `peerDependencies` or `optionalDependencies`) whose value is not a
    /// JSON object, named by the table's key.
    ///
    /// The JSON sibling of [`Gap::Malformed`]: the key is positive evidence
    /// of an intended declaration, and the shape is why nothing came of it.
    /// A separate variant rather than reusing `Malformed` because the two
    /// formats name their tables differently — TOML's dotted paths and
    /// JSON's bare keys — and a message built from the wrong convention
    /// reads as a typo rather than as help.
    MalformedTable(String),
}

impl Gap {
    /// A sentence naming what was not read, for a human reading a finding.
    pub fn describe(&self) -> String {
        match self {
            Gap::Unparseable => "the file does not parse as TOML".to_string(),
            Gap::PoetryDependencies => "dependencies are declared under [tool.poetry] — in \
                 [tool.poetry.dependencies] or a [tool.poetry.group.<name>.dependencies] table \
                 — which Plumb does not read"
                .to_string(),
            Gap::DependencyGroups => {
                "dependencies are declared in [dependency-groups], which Plumb does not read"
                    .to_string()
            }
            Gap::Dynamic(fields) => format!(
                "[project].dynamic lists {}, so the build backend supplies them and \
                 PEP 621 forbids declaring them in [project] — Plumb does not read where \
                 they come from",
                fields
                    .iter()
                    .map(|field| format!("`{field}`"))
                    .collect::<Vec<_>>()
                    .join(" and ")
            ),
            Gap::Malformed(path) => format!(
                "`{path}` is declared but is not the shape it has to be, so Plumb read \
                 nothing from it"
            ),
            Gap::ToolDependencies(paths) => format!(
                "dependencies are declared in {}, which Plumb does not read",
                paths
                    .iter()
                    .map(|path| format!("[{path}]"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            Gap::Include(path) => {
                format!("requirements are included from `{path}`, which Plumb does not follow")
            }
            Gap::MalformedTable(table) => format!(
                "`{table}` in package.json is declared but is not an object, so Plumb read no \
                 names from it"
            ),
        }
    }
}

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

/// Dispatches on filename. Returns `None` when the path is not a manifest
/// this version understands.
///
/// Note the two kinds of "not understood" this function keeps apart. `None`
/// means the path is not a manifest at all, and the caller should move on.
/// `Some(manifest)` with a non-empty [`Manifest::gaps`] means the file *is* a
/// manifest and part of it was not read, which the caller has to say out loud.
pub fn parse(path: &str, text: &str) -> Option<Manifest> {
    let file_name = path.rsplit('/').next().unwrap_or(path);

    if file_name == "package.json" {
        // A vendored package.json under node_modules/ belongs to a
        // dependency, not to this project — it was not written by anyone
        // here, so checking it would block a build over a package nobody
        // added. Checked against the whole path, at any depth, because npm
        // nests dependencies: node_modules/foo/node_modules/bar/package.json
        // is exactly as vendored as the top-level case.
        if path.split('/').any(|segment| segment == "node_modules") {
            return None;
        }
        return Some(parse_package_json(text));
    }

    // PyPI manifest recognition — both `pyproject.toml` and the
    // requirements*.txt family, with the `requirements_dev.txt` /
    // `requirements.dev.txt` exclusion that keeps prose files like
    // `requirements_notes.txt` from being read as a manifest — lives in
    // `ecosystems::pypi::PyPi::matches` now. `for_path` is the recognition
    // gate: it answers "is this filename a PyPI manifest at all". The
    // equality check below is not a re-derivation of that rule; `matches`
    // only ever returns `true` for a `pyproject.toml` or a requirements-family
    // name, so once `for_path` has confirmed one of the two, this check is
    // only choosing which of PyPI's two unrelated manifest syntaxes —
    // pyproject.toml's TOML and requirements.txt's line format — parses it.
    if crate::ecosystems::for_path(path).is_some() {
        return Some(if file_name == "pyproject.toml" {
            crate::ecosystems::pypi::parse_pyproject(text)
        } else {
            crate::ecosystems::pypi::parse_requirements(text)
        });
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
    fn parse_requires_a_separator_after_requirements() {
        // Prose really does yield names, which is what makes the dispatch
        // rule load-bearing rather than cosmetic.
        const PROSE: &str = "The system must support X\nfast delivery\n";
        assert_eq!(
            crate::ecosystems::pypi::parse_requirements(PROSE).names,
            set(&["fast", "the"])
        );

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
    fn a_gap_says_what_was_not_read() {
        // One variant covers two tables, so the sentence has to name both:
        // a reader whose file has only groups must not be sent looking for a
        // `[tool.poetry.dependencies]` that is not there.
        assert!(
            Gap::PoetryDependencies
                .describe()
                .contains("tool.poetry.dependencies")
        );
        assert!(
            Gap::PoetryDependencies
                .describe()
                .contains("tool.poetry.group")
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
        assert!(
            Gap::MalformedTable("dependencies".to_string())
                .describe()
                .contains("dependencies")
        );
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

    #[test]
    fn parse_dispatches_package_json_at_any_depth() {
        assert_eq!(
            parse("package.json", r#"{"dependencies": {"react": "^18.0.0"}}"#).map(|m| m.names),
            Some(set(&["react"]))
        );
        assert_eq!(
            parse(
                "packages/api/package.json",
                r#"{"dependencies": {"react": "^18.0.0"}}"#
            )
            .map(|m| m.names),
            Some(set(&["react"]))
        );
    }

    /// A vendored `package.json` under `node_modules/` belongs to a
    /// dependency, not to this project, and is not recognised at all —
    /// `None`, the same "not a manifest I handle" answer a lock file gets.
    /// At any depth, because npm nests dependencies.
    #[test]
    fn a_package_json_under_node_modules_is_not_recognised() {
        assert!(parse("node_modules/left-pad/package.json", "{}").is_none());
        assert!(parse("packages/api/node_modules/left-pad/package.json", "{}").is_none());
    }
}
