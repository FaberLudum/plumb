# Boundary Check and Adapter Drift Gates Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `plumb check --diff` validates imports in changed source files against the module boundaries declared in `.plumb/architecture.md` (Tier B `boundary` findings), and warns when a generated instruction file has drifted from its `.plumb/` source (Tier B `adapter-drift` findings).

**Architecture:** All parsing and evaluation is pure and lives in `plumb-core`: `architecture::parse` reads the declared modules out of `architecture.md` (module = `##` heading naming a path prefix; `May depend on:` line names permitted dependencies), `boundaries::imports` extracts imported top-level names from a source file with per-language line matchers (Rust/Python/JS-TS — heuristic by design, documented as such), and `boundaries::violations` joins the two. The drift gate reuses `adapters::render` and compares rendered content against the files on disk. `plumb-cli`'s `check` wires both into the existing diff loop: `.plumb/` absent → both gates silently off (the framework is not installed; that is not a gap).

**Out of scope (later plans):** `plumb index` (export scanner) and `plumb report` (volume/coupling longitudinal metrics). Phase 4 continues after this branch.

**Tech Stack:** Rust 1.97, edition 2024. No new dependencies.

## Global Constraints

- Rust edition 2024, MSRV 1.97. This work adds no dependencies.
- Every Rust source file opens with, in this order: `// SPDX-License-Identifier: Apache-2.0` then `// Copyright 2026 The Plumb Authors`.
- No `unwrap()` or `expect()` outside `#[cfg(test)]` code (`unwrap_or`/`unwrap_or_else` fine anywhere). Errors propagate with `anyhow::Result`.
- `plumb-core` is pure logic: no network, no filesystem, no subprocesses.
- Exit codes are a fixed public contract: `0` clean, `1` Tier A violation, `2` warnings only. Both new findings are Tier B: they produce exit `2`, never `1`.
- Every finding cites a taxonomy section that exists — `every_finding_names_a_section_of_the_taxonomy` in `crates/plumb-cli/src/check.rs` enforces it and needs a new scenario per new emission site.
- No test may make a network request.
- Builds run with `--locked`. All code, comments, docs and commit messages in English.
- One commit per plan step that changes files. Conventional Commits. Never amend, never squash. Every commit message ends with `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.

## Verification

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo fmt --all --check
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo clippy --workspace --all-targets --locked -- -D warnings
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked
```

## File Structure

| File | Responsibility |
|---|---|
| `crates/plumb-core/src/architecture.rs` | `Module`, `Architecture`; `parse()` from architecture.md text |
| `crates/plumb-core/src/boundaries.rs` | `imports()` per-language extraction; `violations()` evaluation |
| `crates/plumb-core/src/lib.rs` | export the two new modules |
| `crates/plumb-cli/src/check.rs` | wire boundary + drift findings into `collect`; new scenarios in the taxonomy test |
| `docs/research/taxonomy.md` | new `## 11. Self-consistency` section for `adapter-drift` |
| `crates/plumb-cli/tests/cli.rs` | end-to-end tests |
| `.plumb/architecture.md`, generated files, `README.md` | dogfood |

## Contract details shared by the tasks

**architecture.md machine-readable shape** (already how the dogfood file is written):
- A module is a `## <path-prefix>` heading, e.g. `## crates/plumb-core`. Trailing `/` on the heading is tolerated and stripped.
- Inside a module section, a line starting with `May depend on:` declares permitted dependencies: the rest of the line up to an em dash `—` (if any) is split on commas; each item is trimmed of whitespace and trailing periods; `(none)` (alone or with prose after) means the empty list. Items name other modules by full heading (`crates/plumb-core`) or by last path segment (`plumb-core`) — both resolve.
- A module with no `May depend on:` line is treated as `(none)`.

**Module attribution:** a repository-relative file path belongs to the module whose heading is the longest matching path prefix (component-wise: `crates/plumb-core` matches `crates/plumb-core/src/lib.rs`, not `crates/plumb-core-extras/x.rs`). A file under no module is unattributed and skipped.

**Import-target resolution:** an extracted import name targets the module whose last path segment, with `-` normalised to `_`, equals the import name with `-` normalised to `_`. (`plumb_core` → module `crates/plumb-core`.) An import name matching no module (std, serde, …) is not ours and is ignored. An import targeting the file's own module is allowed. Anything else must appear in the file's module's permitted list (matched by heading or last segment) or it is a violation.

**Import extraction (documented heuristic, line-based):** on the trimmed line —
- `.rs`: `use NAME::…;` / `use NAME;` → NAME (also `pub use`); `extern crate NAME;` → NAME. NAME stops at `::`, `;`, whitespace or `{`. Skip `use crate::`, `use self::`, `use super::`.
- `.py`: `import NAME[.rest]` and `from NAME[.rest] import …` → NAME. Skip relative (`from .`).
- `.ts`/`.tsx`/`.js`/`.jsx`/`.mjs`/`.cjs`: `from 'SPEC'` / `from "SPEC"` / `require('SPEC')` / `require("SPEC")` → first path segment of SPEC (for `@scope/name`, the first two segments joined). Skip relative SPECs starting with `.`.
- Other extensions: no imports extracted.

---

### Task 1: `architecture` in plumb-core — parse the declared modules

**Files:**
- Create: `crates/plumb-core/src/architecture.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Produces (Tasks 2–3 rely on these exact names):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Module {
    /// The heading as written, trailing slash stripped: "crates/plumb-core".
    pub path: String,
    /// Permitted dependencies, as written after "May depend on:", cleaned.
    pub may_depend_on: Vec<String>,
}
pub struct Architecture {
    pub modules: Vec<Module>,
}
pub fn parse(text: &str) -> Architecture
```

- [ ] **Step 1: Write the failing tests**

`crates/plumb-core/src/architecture.rs` with `parse` as `todo!()` and:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    const DOGFOOD_SHAPE: &str = "\
# Architecture — Plumb

Prose that is not a module.

## crates/plumb-core

Responsibility: pure logic.

May depend on: (none) — plumb-core has no internal dependencies.

## crates/plumb-cli

Responsibility: binary entry point.

May depend on: plumb-core.

## docs/research/

Responsibility: the evidence base.
";

    #[test]
    fn every_heading_becomes_a_module_with_trailing_slash_stripped() {
        let architecture = parse(DOGFOOD_SHAPE);
        let paths: Vec<&str> = architecture
            .modules
            .iter()
            .map(|module| module.path.as_str())
            .collect();
        assert_eq!(
            paths,
            vec!["crates/plumb-core", "crates/plumb-cli", "docs/research"]
        );
    }

    #[test]
    fn none_with_trailing_prose_is_the_empty_list() {
        let architecture = parse(DOGFOOD_SHAPE);
        assert!(architecture.modules[0].may_depend_on.is_empty());
    }

    #[test]
    fn dependencies_are_trimmed_of_periods_and_whitespace() {
        let architecture = parse(DOGFOOD_SHAPE);
        assert_eq!(architecture.modules[1].may_depend_on, vec!["plumb-core"]);
    }

    #[test]
    fn a_module_without_the_line_depends_on_nothing() {
        let architecture = parse(DOGFOOD_SHAPE);
        assert!(architecture.modules[2].may_depend_on.is_empty());
    }

    #[test]
    fn a_comma_separated_list_yields_every_item() {
        let text = "## a\n\nMay depend on: b, c/d.\n\n## b\n\n## c/d\n";
        let architecture = parse(text);
        assert_eq!(architecture.modules[0].may_depend_on, vec!["b", "c/d"]);
    }

    #[test]
    fn top_level_heading_is_not_a_module() {
        let architecture = parse("# Architecture — X\n\n## m\n");
        assert_eq!(architecture.modules.len(), 1);
    }
}
```

Add `pub mod architecture;` to `lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked architecture`
Expected: FAIL on `todo!()`.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-core/src/architecture.rs crates/plumb-core/src/lib.rs
git commit -m "test: pin the machine-readable shape of architecture.md"
```

- [ ] **Step 4: Implement `parse`**

```rust
pub fn parse(text: &str) -> Architecture {
    let mut modules: Vec<Module> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim_end();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            let path = heading.trim().trim_end_matches('/').to_string();
            if !path.is_empty() {
                modules.push(Module {
                    path,
                    may_depend_on: Vec::new(),
                });
            }
        } else if let Some(rest) = trimmed.trim_start().strip_prefix("May depend on:")
            && let Some(current) = modules.last_mut()
        {
            current.may_depend_on = parse_dependencies(rest);
        }
    }
    Architecture { modules }
}

/// The text after "May depend on:", cleaned into a list.
///
/// Prose after an em dash is commentary, not a dependency; "(none)" is the
/// explicit empty list. Trailing periods are punctuation, not path syntax.
fn parse_dependencies(rest: &str) -> Vec<String> {
    let before_dash = rest.split('—').next().unwrap_or("");
    if before_dash.trim_start().starts_with("(none)") {
        return Vec::new();
    }
    before_dash
        .split(',')
        .map(|item| item.trim().trim_end_matches('.').trim().to_string())
        .filter(|item| !item.is_empty())
        .collect()
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked architecture`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-core/src/architecture.rs
git commit -m "feat: parse module boundaries out of architecture.md"
```

---

### Task 2: `boundaries` in plumb-core — extract imports, find violations

**Files:**
- Create: `crates/plumb-core/src/boundaries.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Consumes: `architecture::{Architecture, Module}` from Task 1.
- Produces (Task 3 relies on these exact names):

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Repository-relative path of the importing file.
    pub file: String,
    /// The module the file belongs to (heading as declared).
    pub from_module: String,
    /// The module the import targets (heading as declared).
    pub to_module: String,
    /// The import name as written in the source.
    pub import: String,
}
/// Top-level names imported by `text`, per the extension of `path`.
pub fn imports(path: &str, text: &str) -> Vec<String>
/// Boundary violations committed by one file's imports.
pub fn violations(architecture: &Architecture, path: &str, text: &str) -> Vec<Violation>
```

- [ ] **Step 1: Write the failing tests**

`crates/plumb-core/src/boundaries.rs` with both functions as `todo!()` and:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::architecture;

    #[test]
    fn rust_use_lines_yield_the_crate_name() {
        let text = "use plumb_core::manifest;\npub use serde::Serialize;\nuse std::path::Path;\n";
        assert_eq!(imports("crates/x/src/main.rs", text), vec!["plumb_core", "serde", "std"]);
    }

    #[test]
    fn rust_crate_self_and_super_are_not_imports() {
        let text = "use crate::git;\nuse self::inner;\nuse super::outer;\n";
        assert!(imports("crates/x/src/main.rs", text).is_empty());
    }

    #[test]
    fn python_imports_yield_the_top_name() {
        let text = "import requests\nfrom flask import Flask\nfrom . import sibling\nimport os.path\n";
        assert_eq!(imports("api/app.py", text), vec!["requests", "flask", "os"]);
    }

    #[test]
    fn javascript_imports_yield_the_first_segment_and_skip_relative() {
        let text = "import x from 'react-dom/client';\nconst y = require(\"lodash\");\nimport z from './local';\nimport s from '@scope/pkg/deep';\n";
        assert_eq!(imports("web/index.ts", text), vec!["react-dom", "lodash", "@scope/pkg"]);
    }

    #[test]
    fn an_unknown_extension_has_no_imports() {
        assert!(imports("README.md", "use x::y;\n").is_empty());
    }

    fn two_crates() -> architecture::Architecture {
        architecture::parse(
            "## crates/plumb-core\n\nMay depend on: (none).\n\n## crates/plumb-cli\n\nMay depend on: plumb-core.\n",
        )
    }

    #[test]
    fn a_permitted_import_is_not_a_violation() {
        let found = violations(
            &two_crates(),
            "crates/plumb-cli/src/main.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_undeclared_import_is_a_violation_naming_both_modules() {
        let found = violations(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use plumb_cli::check;\n",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].from_module, "crates/plumb-core");
        assert_eq!(found[0].to_module, "crates/plumb-cli");
        assert_eq!(found[0].import, "plumb_cli");
        assert_eq!(found[0].file, "crates/plumb-core/src/lib.rs");
    }

    #[test]
    fn imports_of_foreign_crates_are_ignored() {
        let found = violations(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use serde::Serialize;\nuse std::fmt;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_file_under_no_module_is_skipped() {
        let found = violations(&two_crates(), "tools/gen.rs", "use plumb_cli::check;\n");
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn attribution_is_component_wise_not_string_prefix() {
        let architecture = architecture::parse(
            "## crates/plumb-core\n\nMay depend on: (none).\n\n## crates/plumb-cli\n\nMay depend on: (none).\n",
        );
        let found = violations(
            &architecture,
            "crates/plumb-core-extras/src/lib.rs",
            "use plumb_cli::check;\n",
        );
        assert!(found.is_empty(), "a hyphen sibling was attributed by string prefix: {found:?}");
    }

    #[test]
    fn a_self_import_is_allowed() {
        let found = violations(
            &two_crates(),
            "crates/plumb-core/src/lib.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }
}
```

Add `pub mod boundaries;` to `lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked boundaries`
Expected: FAIL on `todo!()`.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-core/src/boundaries.rs crates/plumb-core/src/lib.rs
git commit -m "test: pin import extraction and boundary evaluation"
```

- [ ] **Step 4: Implement**

Implementation notes (the contract details section above governs; key helpers):

```rust
fn extension(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}

/// The module owning `path`: longest component-wise prefix match.
fn module_of<'a>(architecture: &'a Architecture, path: &str) -> Option<&'a Module> {
    architecture
        .modules
        .iter()
        .filter(|module| {
            path.strip_prefix(module.path.as_str())
                .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|module| module.path.len())
}

/// `-` and `_` are the same letter for matching: a crate named
/// `plumb-core` is imported as `plumb_core`.
fn normalise(name: &str) -> String {
    name.replace('-', "_")
}

/// The module an import name targets: last path segment match.
fn target_of<'a>(architecture: &'a Architecture, import: &str) -> Option<&'a Module> {
    let wanted = normalise(import);
    architecture.modules.iter().find(|module| {
        module
            .path
            .rsplit('/')
            .next()
            .is_some_and(|segment| normalise(segment) == wanted)
    })
}

/// Whether `permitted` (an entry of May-depend-on) names `module`.
fn permits(permitted: &str, module: &Module) -> bool {
    permitted == module.path
        || module
            .path
            .rsplit('/')
            .next()
            .is_some_and(|segment| normalise(segment) == normalise(permitted))
}
```

`imports` dispatches on `extension(path)`: `rs` → the Rust matcher; `py` → Python; `ts|tsx|js|jsx|mjs|cjs` → JavaScript; anything else → empty. Each matcher follows the "Import extraction" contract above, line by line over `text.lines()`, order-preserving, duplicates allowed.

`violations`: attribute the file (`module_of`; none → empty). For each import: resolve `target_of` (none → not ours, skip); target is the same module → skip; target permitted by any `permits` entry → skip; otherwise push a `Violation`.

- [ ] **Step 5: Run tests to verify they pass**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked boundaries`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-core/src/boundaries.rs
git commit -m "feat: evaluate imports against declared module boundaries"
```

---

### Task 3: boundary findings in `plumb check --diff`

**Files:**
- Modify: `crates/plumb-cli/src/check.rs`
- Test: `crates/plumb-cli/tests/cli.rs` (one e2e), `crates/plumb-cli/src/check.rs` (unit + taxonomy scenario)

**Interfaces:**
- Consumes: `plumb_core::architecture`, `plumb_core::boundaries::{violations, Violation}`.
- Produces: Tier B findings with `rule: "boundary"`, `source: "taxonomy §4.5"`.

Behaviour: in `collect`, before the per-path loop, try to read `.plumb/architecture.md` from the repository root (filesystem read stays in plumb-cli). Absent → no boundary checking (framework not installed; silent by design — record this in a code comment). Present → parse once; for every changed path that is NOT a manifest, read the file (reusing `read_manifest`'s tolerance: unreadable source files are skipped silently here — they are not manifests, and the coverage story for source files belongs to a future gate), extract violations, emit one finding per violation:

```rust
/// A boundary crossing the architecture does not declare.
///
/// Tier B: the evidence (taxonomy §4.5) is a preprint, and path-based module
/// attribution plus line-based import extraction is a heuristic that needs
/// per-project calibration before it can block anything.
fn boundary_violation(violation: &plumb_core::boundaries::Violation) -> Finding {
    Finding {
        rule: "boundary".to_string(),
        tier: Tier::B,
        message: format!(
            "{} imports `{}`, but {} may not depend on {}",
            violation.file, violation.import, violation.from_module, violation.to_module
        ),
        location: violation.file.clone(),
        source: "taxonomy §4.5".to_string(),
    }
}
```

- [ ] **Step 1: Write the failing tests**

Unit tests in `check.rs`'s test module:

```rust
    fn write_architecture(dir: &Path, body: &str) {
        fs::create_dir_all(dir.join(".plumb")).expect("create .plumb");
        fs::write(dir.join(".plumb").join("architecture.md"), body).expect("write");
    }

    #[test]
    fn an_undeclared_cross_module_import_warns() {
        let dir = repo_with(&[("crates/core/src/lib.rs", "// empty\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("boundary"), "output was: {output}");
        assert!(output.contains("crates/core/src/lib.rs"), "output was: {output}");
    }

    #[test]
    fn a_declared_dependency_does_not_warn() {
        let dir = repo_with(&[("crates/cli/src/main.rs", "// empty\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/cli/src/main.rs", "use core::check;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    #[test]
    fn without_an_architecture_no_boundary_checking_happens() {
        let dir = repo_with(&[("crates/core/src/lib.rs", "// empty\n")]);
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }
```

Plus: add a boundary scenario to `every_finding_names_a_section_of_the_taxonomy`. That test's scenario table is `(name, before, after)` triples driving manifests; the boundary finding needs an architecture file too, so extend the test with a second, separate loop after the existing one (do not force the manifest table to carry it):

```rust
        // The boundary finding is emitted from a different site than the
        // manifest scenarios above and needs an architecture in place.
        let dir = repo_with(&[("crates/core/src/lib.rs", "// empty\n")]);
        write_architecture(
            dir.path(),
            "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
        );
        write(dir.path(), "crates/core/src/lib.rs", "use cli::main;\n");
        let boundary_findings = collect(dir.path(), &registry).expect("check runs");
        assert_eq!(
            boundary_findings.len(),
            1,
            "the boundary scenario emitted no finding: {boundary_findings:#?}"
        );
        findings.extend(boundary_findings);
```

(Place it before the existing per-finding assertion loop so the citations get checked.)

One e2e test in `tests/cli.rs`:

```rust
#[test]
fn an_undeclared_import_warns_end_to_end() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    let plumb_dir = in_repo(dir.path(), ".plumb");
    std::fs::create_dir_all(&plumb_dir).expect("create .plumb");
    std::fs::write(
        plumb_dir.join("architecture.md"),
        "## crates/core\n\nMay depend on: (none).\n\n## crates/cli\n\nMay depend on: core.\n",
    )
    .expect("write architecture");
    let src = in_repo(dir.path(), "crates/core/src");
    std::fs::create_dir_all(&src).expect("create src");
    std::fs::write(src.join("lib.rs"), "use cli::main;\n").expect("write source");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "add architecture"]);
    std::fs::write(src.join("lib.rs"), "use cli::main;\nuse cli::extra;\n").expect("edit source");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(
            predicates::str::contains("boundary").and(predicates::str::contains("crates/core")),
        );
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked boundary`
Expected: FAIL — exit 0 where 2 asserted; scenario loop finds 0 findings.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-cli/src/check.rs crates/plumb-cli/tests/cli.rs
git commit -m "test: pin that an undeclared cross-module import warns"
```

- [ ] **Step 4: Implement**

In `collect`: read `.plumb/architecture.md` once before the loop (`std::fs::read_to_string(repo.join(".plumb").join("architecture.md")).ok()`), parse to `Option<Architecture>`. In the per-path loop, the manifest probe currently `continue`s on non-manifests; restructure so a non-manifest changed path, when an architecture is present, is read (bytes → lossy is fine here; unreadable → skip) and passed to `boundaries::violations`, each violation becoming `boundary_violation(...)`. Keep the manifest path untouched. Update the module doc comment to name the second gate.

- [ ] **Step 5: Run the full suite**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-cli/src/check.rs
git commit -m "feat: warn on imports that cross undeclared boundaries"
```

---

### Task 4: adapter drift findings in `plumb check --diff`

**Files:**
- Modify: `crates/plumb-cli/src/check.rs`, `docs/research/taxonomy.md`
- Test: `crates/plumb-cli/tests/cli.rs` (one e2e), `crates/plumb-cli/src/check.rs` (unit + taxonomy scenario)

**Interfaces:**
- Consumes: `plumb_core::layer1::{Artefacts, project_name_of}`, `plumb_core::adapters::render`.
- Produces: Tier B findings with `rule: "adapter-drift"`, `source: "taxonomy §11"`.

Behaviour: after the per-path loop in `collect`, if all four `.plumb/` artefact files are readable, render the adapters in memory (project name via `project_name_of` with the repository directory-name fallback — same rule as `adapters_cmd`) and compare each rendered target against the file on disk. A target that exists and differs → one finding. A target that does not exist → no finding (not adopted is not drift). `.plumb/` incomplete or absent → no drift checking.

```rust
/// A generated instruction file that no longer matches its source.
///
/// Tier B, and it describes this repository's artefacts rather than the code
/// under inspection — the same honesty class as taxonomy §10. An agent reads
/// the generated file; when the file and `.plumb/` disagree, the agent is
/// working from rules nobody chose.
fn adapter_drift(path: &str) -> Finding {
    Finding {
        rule: "adapter-drift".to_string(),
        tier: Tier::B,
        message: format!("{path} does not match .plumb/; run `plumb adapters`"),
        location: path.to_string(),
        source: "taxonomy §11".to_string(),
    }
}
```

- [ ] **Step 1: Add the taxonomy section the finding cites**

Append to `docs/research/taxonomy.md`:

```markdown
## 11. Self-consistency

Like §10, the findings in this section describe Plumb's own artefacts, not
defects in the code under inspection, and rest on no evidence row.

The agent instruction files (`CLAUDE.md`, `AGENTS.md`, `.cursorrules`,
`.github/copilot-instructions.md`) are generated from `.plumb/` and declare
so in their first line. Between generations they can drift: someone edits
`.plumb/` and forgets to regenerate, or edits a generated file directly
despite the marker. Either way, the next agent session reads rules that no
longer match the working agreement — rules nobody chose.

| Finding | Emitted when |
|---|---|
| `adapter-drift` | A generated instruction file exists and does not match what `plumb adapters` would produce from the current `.plumb/` |

Tier B: drift is a maintenance fault in this repository, not a defect in the
change under review, and blocking a diff over it would punish the wrong
commit. The remedy is one command, and the finding names it.
```

- [ ] **Step 2: Write the failing tests**

Unit tests in `check.rs` (the fixture needs a full `.plumb/`; build it with the real generator output so the comparison is honest):

```rust
    fn write_full_plumb(dir: &Path) {
        use plumb_core::layer1::{Bootstrap, greenfield};
        let artefacts = greenfield(&Bootstrap {
            project_name: "demo".to_string(),
            modules: vec!["core".to_string()],
            error_handling: "errors propagate".to_string(),
            test_shape: "one behaviour per test".to_string(),
        });
        fs::create_dir_all(dir.join(".plumb")).expect("create .plumb");
        for (name, content) in [
            ("architecture.md", &artefacts.architecture),
            ("conventions.md", &artefacts.conventions),
            ("invariants.md", &artefacts.invariants),
            ("index.md", &artefacts.index),
        ] {
            fs::write(dir.join(".plumb").join(name), content).expect("write artefact");
        }
    }

    fn generate_adapters(dir: &Path) {
        use plumb_core::adapters::render;
        use plumb_core::layer1::Artefacts;
        let read = |name: &str| {
            fs::read_to_string(dir.join(".plumb").join(name)).expect("read artefact")
        };
        let artefacts = Artefacts {
            architecture: read("architecture.md"),
            conventions: read("conventions.md"),
            invariants: read("invariants.md"),
            index: read("index.md"),
        };
        for adapter in render("demo", &artefacts) {
            let path = dir.join(adapter.path);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).expect("create parent");
            }
            fs::write(path, adapter.content).expect("write adapter");
        }
    }

    #[test]
    fn a_generated_file_that_drifted_warns() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        generate_adapters(dir.path());
        // Drift: the working agreement changes, nobody regenerates.
        let conventions = dir.path().join(".plumb").join("conventions.md");
        let mut text = fs::read_to_string(&conventions).expect("read");
        text.push_str("\n## New rule\n\nAdded after generation.\n");
        fs::write(&conventions, text).expect("write");

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");
        let (output, code) = (outcome.report, outcome.code);

        assert_eq!(code, 2, "output was: {output}");
        assert!(output.contains("adapter-drift"), "output was: {output}");
        assert!(output.contains("plumb adapters"), "output was: {output}");
    }

    #[test]
    fn matching_generated_files_do_not_warn() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        generate_adapters(dir.path());

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }

    #[test]
    fn an_absent_generated_file_is_not_drift() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        write_full_plumb(dir.path());
        // No adapters generated at all: adoption is optional, absence is not drift.

        let outcome = run(dir.path(), &StubRegistry::default(), false).expect("check runs");

        assert_eq!(outcome.code, 0, "output was: {}", outcome.report);
    }
```

CAREFUL with the project name in `write_full_plumb`/`generate_adapters` vs. what `collect` derives: `collect` uses `project_name_of(architecture)` — `greenfield` writes `# Architecture — demo`, so both produce "demo" and the comparison holds. The fixture repository directory name differs, which is exactly why the fallback must NOT be taken; if `matching_generated_files_do_not_warn` fails on a title mismatch, the implementation is using the directory name instead of `project_name_of` — fix the implementation, not the test.

Plus a drift scenario appended to `every_finding_names_a_section_of_the_taxonomy` (same pattern as the boundary scenario: build the drifted fixture, `collect`, assert exactly one finding, extend `findings`).

One e2e test in `tests/cli.rs`, using the real commands end to end:

```rust
#[test]
fn drift_between_plumb_and_generated_files_warns_end_to_end() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path()).args(["init", "--defaults"]).assert().success();
    plumb_in(dir.path()).arg("adapters").assert().success();
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "adopt plumb"]);
    let invariants = in_repo(dir.path(), ".plumb/invariants.md");
    let mut text = std::fs::read_to_string(&invariants).expect("read");
    text.push_str("\n## New invariant\n\nAdded after generation.\n");
    std::fs::write(&invariants, text).expect("write");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(predicates::str::contains("adapter-drift"));
}
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked drift`
Expected: FAIL — exit 0 where 2 asserted.

- [ ] **Step 4: Commit the failing tests**

```bash
git add crates/plumb-cli/src/check.rs crates/plumb-cli/tests/cli.rs docs/research/taxonomy.md
git commit -m "test: pin that a drifted instruction file warns"
```

- [ ] **Step 5: Implement**

In `collect`, after the per-path loop: read the four artefacts (`.plumb/{architecture,conventions,invariants,index}.md`); all present → build `Artefacts`, derive the project name with `project_name_of(&artefacts.architecture)` falling back to the repository directory name, `render`, and for each `Adapter` whose target file exists on disk (`std::fs::read` — bytes, compared against `adapter.content.as_bytes()`, so a non-UTF-8 disk file counts as drifted rather than crashing) and differs, push `adapter_drift(adapter.path)`. Any artefact missing → skip silently (partial adoption is init's business, not check's). Update the module doc comment.

- [ ] **Step 6: Run the full suite**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/plumb-cli/src/check.rs
git commit -m "feat: warn when a generated instruction file drifts from .plumb"
```

---

### Task 5: Dogfood and README

**Files:**
- Modify: `.plumb/architecture.md` (dependency line hygiene only, if needed), `README.md`
- Verify: the branch's own gates run clean on this repository

- [ ] **Step 1: Run the gate on this repository**

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo run --locked -p plumb-cli -- check --diff --offline; echo "EXIT=$?"
```

On a clean tree this exits 0 (nothing changed). Then verify each new gate fires here for real:
1. Boundary: temporarily add `use plumb_cli::check;` to a scratch line of `crates/plumb-core/src/lib.rs`, run the command, expect exit 2 with a `boundary` finding naming `crates/plumb-core`; revert the line.
2. Drift: temporarily append a line to `.plumb/invariants.md`, run, expect exit 2 with `adapter-drift` naming all four targets; revert.
Paste both outputs into the README wording work below, then confirm `git status --porcelain` is clean apart from README/architecture edits you intend.

- [ ] **Step 2: Update the README**

Status section: the gate now also validates module boundaries declared in `.plumb/architecture.md` (Tier B warning, heuristic import matching — name the three language families) and warns when generated instruction files drift from `.plumb/` (quote a real `adapter-drift` line from Step 1). Blind spots: add that import extraction is line-based (a multi-line or macro-generated import is missed; an import inside a comment or string can false-positive) and module attribution is path-based.

- [ ] **Step 3: Commit**

```bash
git add README.md .plumb/architecture.md
git commit -m "docs: document the boundary and drift gates"
```

(If `.plumb/architecture.md` needed no edit, commit README alone; if it changed, regenerate the instruction files with `cargo run --locked -p plumb-cli -- adapters` and include them — the drift gate you just built will hold you to it.)
