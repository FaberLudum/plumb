# Manifest Coverage Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `plumb check --diff` incapable of reporting success on a manifest it did not read.

**Architecture:** `manifest::parse` gains a third outcome. It returns a `Manifest` carrying both the names it read and a list of `Gap`s naming what it did not, so a partly understood file reports both. Each gap becomes a Tier B finding, so a run with gaps and no missing packages exits 2 rather than 0. The existing unreadable-file path, which reports on stderr and exits 0, folds into the same mechanism.

**Tech Stack:** Rust 1.97, edition 2024. No new dependencies.

## Global Constraints

- Rust edition 2024, MSRV 1.97, declared at workspace level. Do not touch crate manifests. This work adds no dependencies.
- Every Rust source file opens with, in this order: `// SPDX-License-Identifier: Apache-2.0` then `// Copyright 2026 The Plumb Authors`.
- No `unwrap()` or `expect()` outside `#[cfg(test)]` code. Errors propagate with `anyhow::Result`. (`unwrap_or` and `unwrap_or_else` are different functions and are fine anywhere.)
- `plumb-core` is pure logic: no network, no filesystem, no subprocesses.
- Exit codes are a fixed public contract: `0` clean, `1` Tier A violation, `2` warnings only.
- No test may make a network request. Prove it by panic injection at the request site in `crates/plumb-cli/src/registry.rs`, not by assuming.
- Builds run with `--locked`. Never hand-pin a dependency version.
- All code, comments, documentation and commit messages in English.
- One commit per plan step that changes files. Conventional Commits. Never amend, never squash. Every commit message ends with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

## Verification

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo fmt --all --check
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo clippy --workspace --all-targets --locked -- -D warnings
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked
```

`cargo` is not on the inherited bash PATH; the export is required in the same command.

---

## File Structure

| File | Change |
|---|---|
| `crates/plumb-core/src/manifest.rs` | `Manifest` and `Gap` types; `parse`, `parse_pyproject`, `parse_requirements` return them; gap detection |
| `crates/plumb-cli/src/check.rs` | Consume `.names`; turn gaps into Tier B findings; fold the unreadable path into the same mechanism |
| `docs/research/taxonomy.md` | New `## 10. Coverage reporting` section, cited by the new findings |
| `README.md` | Blind-spot list updated: these formats are still unread, but no longer silent |

---

### Task 1: `Manifest` and `Gap` in plumb-core

**Files:**
- Modify: `crates/plumb-core/src/manifest.rs`
- Modify: `crates/plumb-cli/src/check.rs` (call sites only, to keep the workspace compiling)

**Interfaces:**
- Consumes: nothing from other tasks.
- Produces:
  - `plumb_core::manifest::Manifest { pub names: BTreeSet<String>, pub gaps: Vec<Gap> }`
  - `plumb_core::manifest::Gap` with variants `Unparseable`, `PoetryDependencies`, `DependencyGroups`, `Include(String)`
  - `Gap::describe(&self) -> String`
  - `parse(path: &str, text: &str) -> Option<Manifest>`
  - `parse_pyproject(text: &str) -> Manifest`
  - `parse_requirements(text: &str) -> Manifest`

**The rule that keeps this survivable:** warn on evidence of unread dependencies, never on absence. A `pyproject.toml` holding only `[build-system]` and `[tool.ruff]` declares no runtime dependencies and must produce no gap. `[build-system].requires` is deliberately never a gap — it appears in nearly every `pyproject.toml`, and a warning that always fires is a warning nobody reads.

- [ ] **Step 1: Write the failing tests**

Add to the test module in `crates/plumb-core/src/manifest.rs`:

```rust
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
        let text = "[build-system]\nrequires = [\"setuptools\"]\n\n[tool.ruff]\nline-length = 100\n";
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
        assert!(Gap::PoetryDependencies.describe().contains("tool.poetry.dependencies"));
        assert!(Gap::DependencyGroups.describe().contains("dependency-groups"));
        assert!(Gap::Include("base.txt".to_string()).describe().contains("base.txt"));
        assert!(!Gap::Unparseable.describe().is_empty());
    }
```

Every existing test in this module that calls `parse_requirements`, `parse_pyproject` or `parse` needs its assertion updated to reach through `.names`. Do that in this same step — the tests are the specification of the old contract and changing them is part of changing it.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked manifest`
Expected: FAIL to compile, `cannot find type Manifest in this scope` and `cannot find type Gap in this scope`. A compile failure is the correct RED here: the type does not exist yet.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-core/src/manifest.rs
git commit -m "test: pin what a manifest did not read"
```

- [ ] **Step 4: Write the implementation**

Add above `parse_requirements` in `crates/plumb-core/src/manifest.rs`:

```rust
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
fn include_target(line: &str) -> Option<String> {
    let line = line.trim_start_matches('\u{feff}').trim();

    let rest = if let Some(rest) = line.strip_prefix("--requirement") {
        rest.strip_prefix('=').or_else(|| rest.strip_prefix(' '))?
    } else if let Some(rest) = line.strip_prefix("-r") {
        rest
    } else {
        return None;
    };

    let target = rest.trim();
    if target.is_empty() {
        None
    } else {
        Some(target.to_string())
    }
}
```

Replace the body of `parse_requirements` with:

```rust
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
```

Replace the body of `parse_pyproject` with:

```rust
pub fn parse_pyproject(text: &str) -> Manifest {
    let mut names = BTreeSet::new();
    let mut gaps = Vec::new();

    // `toml::Table`, not `toml::Value`: in toml 1.x `FromStr for Value` parses
    // a single value expression, so a whole document fails and every
    // pyproject.toml would look malformed.
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
                let Some(list) = group.as_array() else { continue };
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
```

Change `parse`'s return type to `Option<Manifest>`. Its body is unchanged apart from the type.

In `crates/plumb-cli/src/check.rs`, keep the workspace compiling with the smallest possible edit: `let Some(after) = manifest::parse(&path, &current)` becomes `let Some(after) = manifest::parse(&path, &current)` followed by using `&after.names` where `&after` was passed to `deps::added`, and `before` becomes `.map(|m| m.names)`. Gaps are not consumed yet — Task 2 does that.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS, every test in the workspace.

- [ ] **Step 6: Verify the probe still works**

`check.rs` calls `manifest::parse(&path, "")` as a path-only probe, pinned by `a_manifest_is_recognised_from_its_path_alone`. An empty string parses as an empty TOML document, so it must yield `Some(Manifest)` with no gaps rather than `Some` with `Unparseable`.

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked a_manifest_is_recognised_from_its_path_alone`
Expected: PASS. If it fails, the empty-document case is being treated as unparseable and the probe now manufactures a gap for every changed file.

- [ ] **Step 7: Commit**

```bash
git add crates/plumb-core/src/manifest.rs crates/plumb-cli/src/check.rs
git commit -m "feat: let a manifest say what it did not read"
```

---

### Task 2: Gaps reach the user as Tier B findings

**Files:**
- Modify: `crates/plumb-cli/src/check.rs`
- Modify: `docs/research/taxonomy.md`
- Modify: `README.md`

**Interfaces:**
- Consumes: `plumb_core::manifest::{Manifest, Gap}` and `Gap::describe` from Task 1.
- Produces: nothing later tasks depend on. This is the last task.

**The coherence this task also fixes:** `check.rs` already has an `Unreadable` path for a manifest whose bytes are not UTF-8, which reports on stderr and leaves the exit code at 0. That is the same class as a gap — Plumb could not establish anything about this file — reported two different ways. This task folds it into the finding mechanism so both exit 2, and removes the separate stderr channel for it.

- [ ] **Step 1: Add the taxonomy section the findings cite**

`Finding::source` must name a heading that exists in `docs/research/taxonomy.md`; a test added in the previous plan reads the file and checks. Append to `docs/research/taxonomy.md`:

```markdown
## 10. Coverage reporting

The findings in this section are not derived from evidence, and saying so is
the point of the section.

Everything above describes defects in AI-generated code, each traced to a
study. A coverage finding describes Plumb instead: it reports that Plumb did
not read something, so that a run which established nothing cannot be mistaken
for a run which found nothing.

They exist because the alternative is worse. A manifest in a format Plumb does
not parse used to yield an empty dependency set, which the gate reported as a
clean result — a silent false negative on a blocking gate, which is the exact
failure this project was built to catch. Reporting the gap is the honest
reading; filing it among the evidence rows would break the project's first rule
in the act of honouring it.

Coverage findings are Tier B. They warn and never block, because a gate that
fails on every project in a format it has not learned yet is uninstalled before
it catches anything.

| Finding | Emitted when |
|---|---|
| `manifest-understood` | A recognised manifest declares dependencies in a form Plumb did not read, or could not be read at all |

This section has no invalidation condition, because it rests on no claim that
new evidence could refute. It goes away when the formats are read.
```

- [ ] **Step 2: Write the failing tests**

Add to the test module in `crates/plumb-cli/src/check.rs`:

```rust
    #[test]
    fn a_manifest_in_an_unread_format_warns_rather_than_passing() {
        let dir = repo_with(&[("pyproject.toml", "[project]\nname = \"demo\"\n")]);
        write(
            dir.path(),
            "pyproject.toml",
            "[tool.poetry.dependencies]\nrequests = \"^2.0\"\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("pyproject.toml"), "output was: {output}");
        assert!(
            output.contains("tool.poetry.dependencies"),
            "output was: {output}"
        );
    }

    #[test]
    fn an_ordinary_manifest_still_passes_silently() {
        let dir = repo_with(&[("requirements.txt", "ruff==0.4.0\n")]);
        write(dir.path(), "requirements.txt", "ruff==0.4.0\nhttpx==0.27.0\n");

        let (output, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
    }

    #[test]
    fn a_missing_package_still_blocks_when_the_same_file_has_a_gap() {
        let dir = repo_with(&[("requirements.txt", "ruff==0.4.0\n")]);
        write(
            dir.path(),
            "requirements.txt",
            "ruff==0.4.0\nrequestz==2.31.0\n-r base.txt\n",
        );

        let (output, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 1, "output was: {output}");
        assert!(output.contains("requestz"), "output was: {output}");
        assert!(output.contains("base.txt"), "output was: {output}");
    }

    #[test]
    fn a_manifest_that_cannot_be_decoded_warns_rather_than_reporting_on_stderr() {
        let dir = repo_with(&[("requirements.txt", "ruff==0.4.0\n")]);
        std::fs::write(
            dir.path().join("requirements.txt"),
            [b'#', b' ', 0xE9, b'\n', b'r', b'u', b'f', b'f', b'\n'],
        )
        .expect("write");

        let (output, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("requirements.txt"), "output was: {output}");
    }
```

`repo_with` and `write` are the fixture helpers already in this module; if their names differ, use the existing ones rather than adding new ones. `StubRegistry` already answers `Missing` for `requestz` and `Exists` for `ruff` and `httpx`.

- [ ] **Step 3: Run the tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-cli --locked check`
Expected: FAIL. The first three fail on the exit code — 0 where 2 or 1 is asserted — because gaps are computed and dropped. The fourth fails because the unreadable path still reports on stderr and exits 0.

- [ ] **Step 4: Commit the failing tests**

```bash
git add crates/plumb-cli/src/check.rs docs/research/taxonomy.md
git commit -m "test: pin that an unread manifest warns"
```

- [ ] **Step 5: Write the implementation**

Add the finding constructor beside the existing `package_does_not_exist` in `crates/plumb-cli/src/check.rs`:

```rust
/// A manifest Plumb recognised and did not fully read.
///
/// Tier B, so it warns and never blocks: a gate that fails on every project in
/// a format it has not learned yet is uninstalled before it catches anything.
/// The message names the file and what was missed, because a warning a reader
/// cannot act on is one they learn to skip.
fn manifest_not_fully_read(path: &str, detail: &str) -> Finding {
    Finding {
        rule: "manifest-understood".to_string(),
        tier: Tier::B,
        message: format!("{path} was not fully read: {detail}"),
        location: path.to_string(),
        source: "taxonomy §10".to_string(),
    }
}
```

In `collect`, replace the `unreadable.push(...)` arm with a finding:

```rust
            Err(reason) => {
                findings.push(manifest_not_fully_read(&path, &reason));
                continue;
            }
```

After `let Some(after) = manifest::parse(&path, &current) else { continue; };`, emit a finding per gap:

```rust
        for gap in &after.gaps {
            findings.push(manifest_not_fully_read(&path, &gap.describe()));
        }
```

Change the `before` binding to reach through `.names`, and report a HEAD-side manifest that does not parse:

```rust
        let before = match git::show_at_head(repo, &path)? {
            None => None,
            Some(text) => match manifest::parse(&path, &text) {
                None => None,
                Some(head) => {
                    if head.gaps.contains(&Gap::Unparseable) {
                        findings.push(manifest_not_fully_read(
                            &path,
                            "the version at HEAD does not parse, so every dependency in it reads as newly added",
                        ));
                    }
                    Some(head.names)
                }
            },
        };

        for dependency in deps::added(&path, before.as_ref(), &after.names) {
```

Then delete the `Unreadable` struct, the `unreadable` vector, `collect`'s second return value, and the stderr reporting of it in `crates/plumb-cli/src/main.rs`. `collect` returns `Result<Vec<Finding>>`. Leave the stale-cache stderr note alone — that one is about the registry, not about a file, and still belongs on stderr.

Import `Gap` alongside the existing `manifest` import.

- [ ] **Step 6: Run the tests to verify they pass**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS. Tests asserting the old stderr behaviour for an undecodable manifest will need updating in the same step; they are the specification of the behaviour being changed.

- [ ] **Step 7: Prove no test reaches the network**

Replace the body of the request site in `crates/plumb-cli/src/registry.rs` with `panic!("a test reached the network: {url}")`, run the full suite, confirm it still passes, then restore the file and confirm `git status --porcelain` is empty.

- [ ] **Step 8: Commit**

```bash
git add crates/plumb-cli/src/check.rs crates/plumb-cli/src/main.rs
git commit -m "feat: warn when a manifest was not fully read"
```

- [ ] **Step 9: Update the README**

The blind-spot list currently says Poetry and `requirements/` layouts are silent false negatives and "the next thing to fix". They are no longer silent. Rewrite that bullet to say the formats are still unread but now reported, name the exit code, and move them out of the "worse than unread" framing — that framing was earned by the silence, not by the gap.

Verify every sentence you write by running the binary and pasting what it printed into your report.

- [ ] **Step 10: Commit**

```bash
git add README.md
git commit -m "docs: say that an unread manifest is now reported"
```

---

## Out of scope for this plan

- Parsing Poetry, PEP 735 or `-r` includes. Each is a real parser with its own syntax; adding them shrinks the surface and never removes it.
- Resolving an include rather than reporting it. That needs filesystem access, which `plumb-core` does not have.
- `[build-system].requires`. A separate rule with separate evidence.
- Suppressing the "every dependency looks new" effect of an unparseable manifest at HEAD. The previous plan judged suppression wrong because it would also suppress the first dependency added to an empty project; this plan reports the cause instead, which was the missing half.
