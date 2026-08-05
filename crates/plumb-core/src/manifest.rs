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
//! main one and the per-group ones — PEP 735 `[dependency-groups]`, and
//! requirements includes are all unread here, and each is reported as a [`Gap`]
//! instead of passing for a file with nothing in it. The rule that keeps a gap
//! worth reading is that it is raised on evidence of an unread declaration and
//! never on the absence of one: a `pyproject.toml` holding only a build system
//! and some tool settings declares no runtime dependencies and produces no gap.
//!
//! One exemption is worth knowing about, because it rests on someone else's
//! documented behaviour rather than on anything checkable here.
//! `[tool.poetry.dependencies]` raises no gap when `[project].dependencies` is
//! present, on Poetry's statement that in that configuration the Poetry table
//! only enriches the PEP 621 one for locking and cannot introduce a name. If
//! that ever stops holding, such a file reads as fully understood while it is
//! not. Group tables carry no such exemption — see [`parse_pyproject`].
//!
//! A file this module does not dispatch on at all — `setup.py`, `Pipfile`, a
//! lock file — is a separate silence and not one a [`Gap`] can express: [`parse`]
//! returning `None` says "not a manifest I handle", never "this project has no
//! dependencies".

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
}

/// The `[project]` fields whose value being `dynamic` hides dependencies.
///
/// `dynamic` itself is ordinary — nearly every backend-built project marks
/// `version` dynamic — so the trigger is these two names and not the key.
const DYNAMIC_DEPENDENCY_FIELDS: [&str; 2] = ["dependencies", "optional-dependencies"];

/// Whether a key under `[tool]` names a dependency declaration.
///
/// `dev-dependencies` needs no case of its own — it ends in `-dependencies`.
///
/// This is a rule about the shape of a key rather than a list of tools, and
/// that is the whole point. A list of tools is a list that is wrong the day
/// after it is written, and this project has already paid for closing the case
/// it just observed instead of the class: five shapes reached the same silent
/// exit 0, from five different tools, each spelled the same way.
fn is_dependency_key(key: &str) -> bool {
    key == "dependencies" || key.ends_with("-dependencies")
}

/// Whether a dotted path is a Poetry table [`parse_pyproject`] already accounts
/// for, and so must not be reported twice.
fn is_handled_poetry_path(segments: &[&str]) -> bool {
    matches!(segments, ["tool", "poetry", "dependencies"])
        || matches!(segments, ["tool", "poetry", "group", _, "dependencies"])
}

/// Finds dependency declarations anywhere under `[tool]`, by the shape of the
/// key rather than by the name of the tool.
///
/// Returns dotted paths, sorted, so the message can name the table to open.
///
/// Three things keep this from firing on ordinary configuration, and each was
/// chosen against a real file rather than in the abstract:
///
/// 1. **A matched key is never descended into.** The keys *inside* a dependency
///    table are package names, and `[tool.pixi.pypi-dependencies]` or
///    `[tool.poetry.dependencies]` may perfectly well hold a package called
///    `dependencies` or `foo-dependencies`. Descending would read a package
///    name as a second declaration and name a table that does not exist.
/// 2. **Only an array or a table counts.** A scalar under such a key is a
///    setting, not a set of names — a `dependencies = true` switch is
///    configuration, and warning on it would be warning on absence.
/// 3. **The Poetry paths this module already reports are skipped**, so the
///    ordinary Poetry layout keeps producing one finding rather than two.
///
/// The traversal is iterative rather than recursive: this crate parses input
/// nobody vouched for, and a deeply nested table must cost time, not the stack.
fn tool_dependency_paths(document: &toml::Table) -> Vec<String> {
    let Some(tool) = document.get("tool").and_then(|t| t.as_table()) else {
        return Vec::new();
    };

    let mut found = Vec::new();
    let mut pending: Vec<(Vec<&str>, &toml::Table)> = vec![(vec!["tool"], tool)];

    while let Some((path, table)) = pending.pop() {
        for (key, value) in table {
            let mut child = path.clone();
            child.push(key);

            if is_dependency_key(key) {
                // Matched: this is a declaration, not a container of more
                // configuration. Report it and stop — see 1 and 2 above.
                if !is_handled_poetry_path(&child)
                    && (value.is_array() || value.as_table().is_some())
                {
                    found.push(child.join("."));
                }
                continue;
            }

            if let Some(nested) = value.as_table() {
                pending.push((child, nested));
            }
        }
    }

    found.sort();
    found
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
        }
    }
}

/// Drops a pip comment from a requirements line.
///
/// pip starts a comment at `#` only when it begins the line or follows
/// whitespace. That rule is why this is not `split('#')`: `base.txt#fragment`
/// is a path containing a `#`, not a path with a note after it, and truncating
/// it would name a file that does not exist just as surely as keeping the
/// comment does.
///
/// Scoped to the requirements path deliberately. `requirement_name` does its
/// own comment handling — the terminator search ends a name at whitespace, and
/// an unspaced `#` fails the character check — and it is also called on
/// `pyproject.toml` array entries, where pip's rule does not apply at all.
fn strip_comment(line: &str) -> &str {
    let mut previous_was_whitespace = true;
    for (index, ch) in line.char_indices() {
        if ch == '#' && previous_was_whitespace {
            return &line[..index];
        }
        previous_was_whitespace = ch.is_whitespace();
    }
    line
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
///
/// The trailing comment goes before the path is taken, so the gap names a file
/// a reader can open. A gap naming `base.txt  # shared pins` points at nothing.
fn include_target(line: &str) -> Option<String> {
    let line = strip_comment(line.trim_start_matches('\u{feff}')).trim();

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

    // Every `as_array()` and `as_table()` below is a filter, and a filter that
    // drops a declaration in silence is the same false negative as a format
    // this module cannot read. So each one that rejects a value that *is*
    // there leaves a `Malformed` gap: the key is the evidence, the shape is the
    // reason nothing came of it.
    if let Some(project) = project {
        if let Some(declared) = project.get("dependencies") {
            match declared.as_array() {
                Some(list) => {
                    for entry in list {
                        if let Some(name) = entry.as_str().and_then(requirement_name) {
                            names.insert(name);
                        }
                    }
                }
                None => gaps.push(Gap::Malformed("project.dependencies".to_string())),
            }
        }

        if let Some(declared) = project.get("optional-dependencies") {
            match declared.as_table() {
                Some(groups) => {
                    for (name, group) in groups {
                        match group.as_array() {
                            Some(list) => {
                                for entry in list {
                                    if let Some(name) = entry.as_str().and_then(requirement_name) {
                                        names.insert(name);
                                    }
                                }
                            }
                            None => gaps.push(Gap::Malformed(format!(
                                "project.optional-dependencies.{name}"
                            ))),
                        }
                    }
                }
                None => gaps.push(Gap::Malformed("project.optional-dependencies".to_string())),
            }
        }
    }

    // Everything below is computed before anything is pushed, because two of
    // these triggers are about each other: the `dynamic` gap is superseded by a
    // gap that names an actual table, and it cannot know that until the tables
    // have been looked for.

    // The two Poetry tables take different conditions, and the asymmetry is
    // Poetry's rather than a lapse here.
    //
    // The main table is a gap only when nothing authoritative was read. With
    // `[project].dependencies` present, Poetry 2.x documents
    // `[tool.poetry.dependencies]` as enriching it for locking and not as a
    // place to add a name, so the two mirror each other and warning would fire
    // on a project Plumb reads correctly.
    //
    // A group table has no such counterpart: PEP 621 gives `[project]` no
    // notion of dependency groups, so nothing in `[project]` can mirror one and
    // its contents are unread whatever else the file holds. Groups are also
    // where the ordinary Poetry project keeps its dev, test and docs
    // dependencies, so exempting them would leave the common layout silent.
    //
    // `[build-system].requires` is deliberately absent from this list. It
    // appears in nearly every pyproject.toml, so treating it as a gap would
    // fire the warning universally — and a warning that always fires is a
    // warning nobody reads. Build-time dependencies are a separate rule with
    // separate evidence, not an unread part of the runtime set. See [`Gap`] for
    // why a gap that fires on every run of a Poetry project is a different
    // thing from that.
    //
    // Keyed to `as_array()`, the same test the reader takes, and not to the key
    // being present. The exemption's whole justification is that `[project]` is
    // authoritative and Plumb read it — a value the reader could not use makes
    // that false, so a non-array `dependencies` used to suppress the Poetry gap
    // while contributing no name, and the file passed as understood with
    // nothing in it.
    let project_declares_dependencies = project
        .and_then(|p| p.get("dependencies"))
        .and_then(|d| d.as_array())
        .is_some();
    let poetry = document.get("tool").and_then(|t| t.get("poetry"));
    let poetry_declares_dependencies = poetry.and_then(|p| p.get("dependencies")).is_some();
    // Keyed to a group's `dependencies` child, not to the group itself: a
    // `[tool.poetry.group.dev]` holding only `optional = true` declares
    // nothing, and warning on it would be warning on absence.
    let poetry_groups_declare_dependencies = poetry
        .and_then(|p| p.get("group"))
        .and_then(|g| g.as_table())
        .is_some_and(|groups| {
            groups
                .values()
                .any(|group| group.get("dependencies").is_some())
        });
    // One gap however many of these hold, because the reader's action is one
    // action. `describe` names both tables.
    let poetry_is_a_gap = poetry_groups_declare_dependencies
        || (poetry_declares_dependencies && !project_declares_dependencies);

    // Every other dependency declaration under `[tool]`, found by the shape of
    // the key. See `tool_dependency_paths` for why this is not a list of tools.
    let tool_paths = tool_dependency_paths(&document);

    // `dynamic` is the one trigger the file states itself, in the standard's
    // own vocabulary, rather than leaving it to be inferred from a tool table.
    // A backend that supplies `dependencies` may keep them in `setup.py`, in a
    // `requirements.txt` named by `[tool.setuptools.dynamic]`, or anywhere else
    // it likes — and PEP 621 forbids `[project].dependencies` alongside, so
    // there is nothing for the reader above to find.
    //
    // It is also the least specific of the three: it says the dependencies are
    // declared elsewhere without saying where. So a gap that names an actual
    // table supersedes it, and only those two do. `[dependency-groups]` must
    // not — PEP 735 groups are a separate declaration, not the place
    // `[project].dynamic` points to, and letting them swallow this gap would
    // hide real evidence. Poetry 2.x is exactly this shape, and without the
    // supersession it produces two findings for one action.
    let dependencies_are_located_elsewhere = poetry_is_a_gap || !tool_paths.is_empty();
    if !dependencies_are_located_elsewhere
        && let Some(dynamic) = project
            .and_then(|p| p.get("dynamic"))
            .and_then(|d| d.as_array())
    {
        let listed: Vec<String> = DYNAMIC_DEPENDENCY_FIELDS
            .iter()
            .filter(|field| dynamic.iter().any(|entry| entry.as_str() == Some(**field)))
            .map(|field| (*field).to_string())
            .collect();
        if !listed.is_empty() {
            gaps.push(Gap::Dynamic(listed));
        }
    }

    if !tool_paths.is_empty() {
        gaps.push(Gap::ToolDependencies(tool_paths));
    }

    if document.get("dependency-groups").is_some() {
        gaps.push(Gap::DependencyGroups);
    }

    if poetry_is_a_gap {
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

    /// The evidence a Poetry gap needs is a dependency table, not Poetry.
    ///
    /// Without this, the condition can be relaxed from
    /// `[tool.poetry.dependencies]` to `[tool]`, `[tool.poetry]` or "this looks
    /// like a Poetry project" and every test still passes — the two Poetry
    /// tests either have no `[tool]` table at all or have `[project]`
    /// dependencies that suppress the gap anyway. A file whose only Poetry
    /// content is metadata declares no dependencies, and warning on it is the
    /// warning-on-absence this module is not allowed to do.
    #[test]
    fn a_poetry_table_that_declares_no_dependencies_is_not_a_gap() {
        let text = r#"
[tool.poetry]
name = "demo"
version = "0.1.0"

[tool.poetry.scripts]
demo = "demo:main"
"#;
        let manifest = parse_pyproject(text);
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
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

    /// A group table is a gap whenever it exists, which is not the rule the
    /// main Poetry table gets.
    ///
    /// The asymmetry is Poetry's, not an inconsistency here. With
    /// `[project].dependencies` present, `[tool.poetry.dependencies]` "is only
    /// used to enrich project.dependencies for locking" and cannot add a name,
    /// so warning on it would fire on a project Plumb reads correctly. A group
    /// has no `[project]` counterpart to be enriched *into* — PEP 621 has no
    /// notion of dependency groups — so its contents are unread whatever else
    /// the file says.
    #[test]
    fn poetry_group_dependencies_are_a_gap_beside_project_dependencies() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff==0.4.0"]

[tool.poetry.group.dev.dependencies]
pytest = "^8.0"
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        assert_eq!(manifest.gaps, vec![Gap::PoetryDependencies]);
    }

    /// A group that declares no dependencies is not evidence of any.
    ///
    /// `[tool.poetry.group.dev]` carries settings of its own — `optional` is
    /// the common one — and a group with only those declares nothing. Keying
    /// the gap on the group table rather than on its `dependencies` child is
    /// the same warning-on-absence mistake as keying it on `[tool.poetry]`.
    #[test]
    fn a_poetry_group_without_a_dependencies_table_is_not_a_gap() {
        let text = "[tool.poetry.group.dev]\noptional = true\n";
        let manifest = parse_pyproject(text);
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// One unread Poetry layout is one gap, however many tables express it.
    ///
    /// The ordinary Poetry project has both — a main table and a dev group —
    /// and the reader's action is the same for both, so two findings on one
    /// file would be one finding and one repetition.
    #[test]
    fn the_two_poetry_tables_raise_a_single_gap() {
        let text = r#"
[tool.poetry.dependencies]
requests = "^2.0"

[tool.poetry.group.dev.dependencies]
pytest = "^8.0"
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.gaps, vec![Gap::PoetryDependencies]);
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

    /// A declaration in the wrong shape is a declaration, and it was not read.
    ///
    /// The reader takes `as_array()`, so anything else falls through the `if
    /// let` and the file reports as fully understood. That is the same silent
    /// false negative as an unread format, reached by a typo rather than by a
    /// tool: the key is right there in the file, positive evidence of an
    /// intended declaration, and nothing came of it.
    #[test]
    fn a_project_dependencies_that_is_not_an_array_is_a_gap() {
        let manifest = parse_pyproject("[project]\nname = \"demo\"\ndependencies = \"ruff\"\n");
        assert!(manifest.names.is_empty());
        assert_eq!(
            manifest.gaps,
            vec![Gap::Malformed("project.dependencies".to_string())]
        );
    }

    /// The sharper half: a non-array `dependencies` also used to *suppress*
    /// the Poetry gap, because the exemption was keyed on the key being
    /// present while the read required an array. So a single typo turned a
    /// Poetry manifest into a fully understood file with nothing in it — the
    /// exemption firing on a `[project]` table that authoritatively declares
    /// nothing at all.
    #[test]
    fn a_non_array_project_table_does_not_exempt_the_poetry_table() {
        let text = r#"
[project]
name = "demo"
dependencies = "ruff"

[tool.poetry.dependencies]
requests = "^2.0"
"#;
        let manifest = parse_pyproject(text);
        assert!(manifest.names.is_empty());
        assert_eq!(
            manifest.gaps,
            vec![
                Gap::Malformed("project.dependencies".to_string()),
                Gap::PoetryDependencies,
            ]
        );
    }

    #[test]
    fn a_malformed_optional_dependency_group_is_a_gap() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff"]

[project.optional-dependencies]
dev = "pytest"
docs = ["mkdocs"]
"#;
        let manifest = parse_pyproject(text);
        // The groups that are readable are still read: a partly understood
        // file keeps its names.
        assert_eq!(manifest.names, set(&["mkdocs", "ruff"]));
        assert_eq!(
            manifest.gaps,
            vec![Gap::Malformed(
                "project.optional-dependencies.dev".to_string()
            )]
        );
    }

    #[test]
    fn optional_dependencies_that_is_not_a_table_is_a_gap() {
        let text = "[project]\nname = \"demo\"\noptional-dependencies = [\"pytest\"]\n";
        assert_eq!(
            parse_pyproject(text).gaps,
            vec![Gap::Malformed("project.optional-dependencies".to_string())]
        );
    }

    /// The guard: a well-formed file must not acquire a malformed gap, or the
    /// shape check has been widened into a warning on every project.
    #[test]
    fn a_well_formed_project_table_raises_no_malformed_gap() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff"]

[project.optional-dependencies]
dev = ["pytest"]
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["pytest", "ruff"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// PEP 621's own way of saying "my dependencies are declared, but not
    /// here".
    ///
    /// This is the strongest evidence a gap can rest on, because the file
    /// states it in the standard's vocabulary rather than leaving it to be
    /// inferred from a tool table. Listing `dependencies` in `dynamic` means
    /// the build backend supplies them, and PEP 621 forbids also writing
    /// `[project].dependencies` — so the literal-key reader finds nothing and,
    /// without this, reports a fully understood file.
    #[test]
    fn dynamic_dependencies_are_a_gap() {
        let text = r#"
[project]
name = "demo"
dynamic = ["version", "dependencies"]
"#;
        let manifest = parse_pyproject(text);
        assert!(manifest.names.is_empty());
        assert_eq!(
            manifest.gaps,
            vec![Gap::Dynamic(vec!["dependencies".to_string()])]
        );
    }

    #[test]
    fn dynamic_optional_dependencies_are_a_gap() {
        let text = "[project]\nname = \"demo\"\ndynamic = [\"optional-dependencies\"]\n";
        assert_eq!(
            parse_pyproject(text).gaps,
            vec![Gap::Dynamic(vec!["optional-dependencies".to_string()])]
        );
    }

    /// Both fields listed is one unread layout, so it is one gap naming both —
    /// the rule the two Poetry tables already follow.
    #[test]
    fn both_dynamic_dependency_fields_raise_a_single_gap() {
        let text =
            "[project]\nname = \"demo\"\ndynamic = [\"dependencies\", \"optional-dependencies\"]\n";
        let gaps = parse_pyproject(text).gaps;
        assert_eq!(
            gaps,
            vec![Gap::Dynamic(vec![
                "dependencies".to_string(),
                "optional-dependencies".to_string()
            ])]
        );
        assert!(gaps[0].describe().contains("dependencies"));
    }

    /// `dynamic` is ordinary and mostly harmless: nearly every backend-built
    /// project marks `version` dynamic. Only the two dependency fields are
    /// evidence of an unread declaration, and warning on the rest would be
    /// warning on absence — the thing this module is not allowed to do.
    #[test]
    fn dynamic_fields_that_are_not_dependencies_are_not_a_gap() {
        let text = r#"
[project]
name = "demo"
dynamic = ["version", "readme", "classifiers"]
dependencies = ["ruff"]
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    /// The five shapes that reached exit 0 with "no findings" while declaring
    /// a package that does not exist.
    ///
    /// Five tools, one spelling. They are covered by a rule about the shape of
    /// a key — `dependencies`, or anything ending in `-dependencies`, anywhere
    /// under `[tool]` — rather than by five cases, because five cases is a list
    /// that is wrong the day after it is written, and this project has already
    /// spent three fix rounds on one parser closing the case just observed.
    ///
    /// `[tool.poetry.dev-dependencies]` is the sharpest of them: the pre-1.2
    /// spelling sits inside the table family this module reads and matches
    /// neither `poetry.dependencies` nor `poetry.group.*.dependencies`.
    #[test]
    fn a_dependency_table_under_tool_is_a_gap_whatever_the_tool() {
        for (label, text, expected) in [
            (
                "pdm",
                "[tool.pdm.dev-dependencies]\ntest = [\"pytest\"]\n",
                "tool.pdm.dev-dependencies",
            ),
            (
                "hatch",
                "[tool.hatch.envs.default]\ndependencies = [\"pytest\"]\n",
                "tool.hatch.envs.default.dependencies",
            ),
            (
                "uv",
                "[tool.uv]\ndev-dependencies = [\"pytest\"]\n",
                "tool.uv.dev-dependencies",
            ),
            (
                "pixi",
                "[tool.pixi.pypi-dependencies]\npytest = \"*\"\n",
                "tool.pixi.pypi-dependencies",
            ),
            (
                "poetry 1.x legacy",
                "[tool.poetry.dev-dependencies]\npytest = \"^8.0\"\n",
                "tool.poetry.dev-dependencies",
            ),
        ] {
            let manifest = parse_pyproject(text);
            assert_eq!(
                manifest.gaps,
                vec![Gap::ToolDependencies(vec![expected.to_string()])],
                "{label}"
            );
            assert!(
                manifest.gaps[0].describe().contains(expected),
                "the message must name the table to open: {label}"
            );
        }
    }

    /// The rule this module is not allowed to break, checked against the
    /// configuration people actually write.
    ///
    /// A recursive search under `[tool]` is the risk this design takes on, and
    /// false warnings are its main failure mode — not missed gaps. So the
    /// corpus is ordinary tool configuration, drawn from each tool's own
    /// documented `pyproject.toml` schema, and the assertion is silence.
    ///
    /// Every entry here was checked for a key named `dependencies` or ending in
    /// `-dependencies`. None has one, which is why the rule can afford to be
    /// structural: the tools that configure *behaviour* do not spell anything
    /// `dependencies`, and the tools that declare *packages* all spell it the
    /// same way.
    #[test]
    fn ordinary_tool_configuration_raises_no_gap() {
        let text = r#"
[build-system]
requires = ["setuptools>=61", "setuptools-scm"]
build-backend = "setuptools.build_meta"

[project]
name = "demo"
dependencies = ["ruff"]

[tool.ruff]
line-length = 100
target-version = "py311"
extend-exclude = ["build"]

[tool.ruff.lint]
select = ["E", "F", "I"]
ignore = ["E501"]

[tool.ruff.lint.isort]
known-first-party = ["demo"]

[tool.mypy]
strict = true
ignore_missing_imports = true
exclude = ["tests/"]

[tool.black]
line-length = 100

[tool.isort]
profile = "black"

[tool.pytest.ini_options]
addopts = "-ra"
testpaths = ["tests"]

[tool.coverage.run]
branch = true
source = ["demo"]

[tool.coverage.report]
exclude_lines = ["pragma: no cover"]
fail_under = 90

[tool.pyright]
include = ["src"]
reportMissingImports = true

[tool.pylint.main]
jobs = 0

[tool.setuptools]
include-package-data = true

[tool.setuptools.packages.find]
where = ["src"]

[tool.setuptools.package-data]
demo = ["py.typed"]

[tool.setuptools_scm]
version_scheme = "post-release"

[tool.hatch.build.targets.wheel]
packages = ["src/demo"]

[tool.hatch.version]
path = "src/demo/__init__.py"

[tool.hatch.metadata]
allow-direct-references = true

[tool.pdm.build]
package-dir = "src"

[tool.pdm.resolution]
excludes = ["urllib3"]

[tool.uv]
package = true

[tool.uv.sources]
demo-lib = { workspace = true }

[tool.commitizen]
version = "0.1.0"
tag_format = "v$version"

[tool.towncrier]
directory = "changes"
package = "demo"

[tool.cibuildwheel]
build = "cp311-*"
skip = "*-musllinux_*"

[tool.maturin]
features = ["pyo3/extension-module"]

[tool.scikit-build]
cmake.version = ">=3.26"

[tool.bandit]
exclude_dirs = ["tests"]

[tool.poe.tasks]
test = "pytest"

[tool.semantic_release]
version_variable = "src/demo/__init__.py:__version__"

[tool.tox]
env_list = ["py311", "py312"]

[tool.tox.env_run_base]
deps = ["pytest"]
commands = [["pytest"]]
"#;
        let manifest = parse_pyproject(text);
        assert_eq!(manifest.names, set(&["ruff"]));
        assert!(
            manifest.gaps.is_empty(),
            "a false warning on ordinary configuration: {:?}",
            manifest.gaps
        );
    }

    /// A dependency table's keys are package names, and one of them may be
    /// called `dependencies`.
    ///
    /// So a matched key is reported and never descended into. Without that,
    /// `tool.pixi.pypi-dependencies.dependencies` — a real PyPI package called
    /// `dependencies`, pinned by an ordinary pixi project — is reported as a
    /// second declaration, naming a table that does not exist. The same holds
    /// under `[tool.poetry.dependencies]`, where it would manufacture a finding
    /// on a file the module already accounts for.
    #[test]
    fn a_package_named_dependencies_is_not_read_as_a_second_declaration() {
        let text = r#"
[tool.pixi.pypi-dependencies]
dependencies = "*"
build-dependencies = "*"
ruff = "*"
"#;
        assert_eq!(
            parse_pyproject(text).gaps,
            vec![Gap::ToolDependencies(vec![
                "tool.pixi.pypi-dependencies".to_string()
            ])]
        );

        let poetry = "[tool.poetry.dependencies]\ndependencies = \"^1.0\"\n";
        assert_eq!(parse_pyproject(poetry).gaps, vec![Gap::PoetryDependencies]);
    }

    /// A scalar under such a key is a setting, not a set of names, and warning
    /// on it would be warning on absence — the one thing this module may not
    /// do. Nothing can be declared by `dependencies = true`.
    #[test]
    fn a_scalar_dependencies_key_under_tool_is_not_a_gap() {
        for text in [
            "[tool.some-tool]\ndependencies = true\n",
            "[tool.some-tool]\ndev-dependencies = \"warn\"\n",
            "[tool.some-tool]\ndependencies = 3\n",
        ] {
            let manifest = parse_pyproject(text);
            assert!(
                manifest.gaps.is_empty(),
                "gaps: {:?} for {text}",
                manifest.gaps
            );
        }
    }

    /// The Poetry tables this module already reports must not be reported a
    /// second time by the structural rule, or the ordinary Poetry project gets
    /// two findings for one action.
    #[test]
    fn the_poetry_tables_are_not_reported_twice() {
        let text = r#"
[tool.poetry.dependencies]
requests = "^2.0"

[tool.poetry.group.dev.dependencies]
pytest = "^8.0"
"#;
        assert_eq!(parse_pyproject(text).gaps, vec![Gap::PoetryDependencies]);
    }

    /// Several tables in one file are one finding naming all of them, for the
    /// reason the two Poetry tables are: one file, one reader, one action. The
    /// paths are sorted so the message does not depend on document order.
    #[test]
    fn several_tool_tables_raise_one_gap_naming_each() {
        let text = r#"
[tool.uv]
dev-dependencies = ["pytest"]

[tool.pixi.pypi-dependencies]
ruff = "*"

[tool.pixi.feature.docs.pypi-dependencies]
mkdocs = "*"
"#;
        assert_eq!(
            parse_pyproject(text).gaps,
            vec![Gap::ToolDependencies(vec![
                "tool.pixi.feature.docs.pypi-dependencies".to_string(),
                "tool.pixi.pypi-dependencies".to_string(),
                "tool.uv.dev-dependencies".to_string(),
            ])]
        );
    }

    /// A gap that names a table supersedes the one that only says "elsewhere".
    ///
    /// Poetry 2.x is `dynamic = ["dependencies"]` plus
    /// `[tool.poetry.dependencies]`, and both triggers describe the same fact.
    /// The specific one survives, because it is the one a reader can act on.
    #[test]
    fn a_named_table_supersedes_the_dynamic_gap() {
        let poetry = r#"
[project]
name = "demo"
dynamic = ["dependencies"]

[tool.poetry.dependencies]
requests = "^2.0"
"#;
        assert_eq!(parse_pyproject(poetry).gaps, vec![Gap::PoetryDependencies]);

        let setuptools = r#"
[project]
name = "demo"
dynamic = ["dependencies"]

[tool.setuptools.dynamic]
dependencies = { file = ["requirements.txt"] }
"#;
        assert_eq!(
            parse_pyproject(setuptools).gaps,
            vec![Gap::ToolDependencies(vec![
                "tool.setuptools.dynamic.dependencies".to_string()
            ])]
        );
    }

    /// `[dependency-groups]` must not supersede it, though.
    ///
    /// PEP 735 groups are a separate declaration, not the place
    /// `[project].dynamic` points to, so letting them stand in for it would
    /// drop the only evidence that `[project].dependencies` is unread — two
    /// distinct blind spots reported as one.
    #[test]
    fn dependency_groups_do_not_supersede_the_dynamic_gap() {
        let text = r#"
[project]
name = "demo"
dynamic = ["dependencies"]

[dependency-groups]
dev = ["pytest"]
"#;
        assert_eq!(
            parse_pyproject(text).gaps,
            vec![
                Gap::Dynamic(vec!["dependencies".to_string()]),
                Gap::DependencyGroups,
            ]
        );
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

    /// An include names a file, and the finding has to name the same file.
    ///
    /// pip strips a `#` comment that starts a line or follows whitespace, so
    /// `-r base.txt  # shared pins` includes `base.txt`. Carrying the comment
    /// into the gap produces a finding naming `base.txt  # shared pins`, which
    /// is not a path anyone can act on — and naming the file to act on is the
    /// whole value of the message.
    ///
    /// `ruff#comment` is excluded on purpose and for pip's own reason: without
    /// preceding whitespace `#` starts no comment, so `base.txt#fragment` is a
    /// path containing a `#` rather than a path plus a note.
    #[test]
    fn an_include_drops_a_trailing_comment() {
        for (line, target) in [
            ("-r base.txt  # shared pins", "base.txt"),
            ("-r base.txt\t# shared pins", "base.txt"),
            ("--requirement=base.txt # pins", "base.txt"),
            ("-rbase.txt # pins", "base.txt"),
            ("-r base.txt#fragment", "base.txt#fragment"),
            ("-r base.txt", "base.txt"),
        ] {
            assert_eq!(
                parse_requirements(&format!("{line}\n")).gaps,
                vec![Gap::Include(target.to_string())],
                "line: {line}"
            );
        }
    }

    /// A line whose only content after the flag is a comment names no file,
    /// so it is no include and must not produce a gap pointing at nothing.
    #[test]
    fn an_include_flag_with_only_a_comment_is_not_an_include() {
        let manifest = parse_requirements("-r  # nothing here\n");
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
        assert!(manifest.names.is_empty(), "names: {:?}", manifest.names);
    }

    /// `--requirement` has to be followed by `=` or whitespace.
    ///
    /// Both halves of that guard, because each fails a different way. A long
    /// form separated by a tab is an include and a version that only accepts a
    /// space misses it — the silent miss this module exists to stop. An option
    /// whose name merely begins with `--requirement` is not an include, and a
    /// bare `strip_prefix` would report one, naming a path that is really the
    /// tail of an option.
    #[test]
    fn the_long_include_option_is_matched_whole() {
        assert_eq!(
            parse_requirements("--requirement\tbase.txt\n").gaps,
            vec![Gap::Include("base.txt".to_string())]
        );

        let manifest = parse_requirements("--requirements-of-some-future-pip base.txt\n");
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
    }

    #[test]
    fn an_editable_install_is_not_an_include() {
        let manifest = parse_requirements("-e .\n--index-url https://example.com\n");
        assert!(manifest.names.is_empty());
        assert!(manifest.gaps.is_empty(), "gaps: {:?}", manifest.gaps);
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
    }
}
