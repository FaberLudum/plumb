# Layer 1 Artefacts and Agent Adapters Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** `plumb init` writes the `.plumb/` working agreement for a repository, and `plumb adapters` generates every agent instruction file from it — one source, N harnesses.

**Architecture:** Rendering is pure and lives in `plumb-core` (`layer1::greenfield` turns bootstrap answers into artefact strings; `adapters::render` turns artefact strings into target files). Everything that touches the filesystem or the terminal lives in `plumb-cli`. Brownfield is not a second code path: it only changes the *default answers* — the module list is prefilled from the directories that contain source files, and the human corrects at the prompt.

**Tech Stack:** Rust 1.97, edition 2024. No new dependencies.

## Global Constraints

- Rust edition 2024, MSRV 1.97, declared at workspace level. This work adds no dependencies.
- Every Rust source file opens with, in this order: `// SPDX-License-Identifier: Apache-2.0` then `// Copyright 2026 The Plumb Authors`.
- No `unwrap()` or `expect()` outside `#[cfg(test)]` code. Errors propagate with `anyhow::Result`. (`unwrap_or` and `unwrap_or_else` are fine anywhere.)
- `plumb-core` is pure logic: no network, no filesystem, no subprocesses, no prompts.
- Exit codes are a fixed public contract: `0` clean, `1` Tier A violation, `2` warnings only. `init` and `adapters` are not gates: they exit `0` on success and use the existing `EXIT_INTERNAL_ERROR` (70) path for a run that could not happen.
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
| `crates/plumb-core/src/layer1.rs` | `Bootstrap` answers; `Artefacts` struct; `greenfield()` rendering the four artefact bodies |
| `crates/plumb-core/src/adapters.rs` | `render()`: artefact strings → the four generated target files, marker included |
| `crates/plumb-core/src/lib.rs` | export the two new modules |
| `crates/plumb-cli/src/init.rs` | prompt loop, source-directory detection, `.plumb/` writing |
| `crates/plumb-cli/src/adapters_cmd.rs` | read `.plumb/`, call core render, write targets, overwrite guard |
| `crates/plumb-cli/src/main.rs` | two new subcommands wired to the above |
| `crates/plumb-cli/tests/cli.rs` | end-to-end tests for both commands |
| `.plumb/*` (this repository) | real content, hand-written: the dogfood |
| `CLAUDE.md`, `AGENTS.md`, `.cursorrules`, `.github/copilot-instructions.md` | become generated output |

---

### Task 1: `layer1` in plumb-core — answers to artefacts

**Files:**
- Create: `crates/plumb-core/src/layer1.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Produces (Task 3 and 5 rely on these exact names):

```rust
pub struct Bootstrap {
    pub project_name: String,
    pub modules: Vec<String>,
    pub error_handling: String,
    pub test_shape: String,
}
pub struct Artefacts {
    pub architecture: String,
    pub conventions: String,
    pub invariants: String,
    pub index: String,
}
pub fn greenfield(bootstrap: &Bootstrap) -> Artefacts
```

- [ ] **Step 1: Write the failing tests**

In `crates/plumb-core/src/layer1.rs` (module skeleton plus tests; `greenfield` not yet implemented — declare it with `todo!()` so the tests compile and fail):

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Layer 1 artefact rendering: bootstrap answers to the four `.plumb/` files.

pub struct Bootstrap {
    pub project_name: String,
    pub modules: Vec<String>,
    pub error_handling: String,
    pub test_shape: String,
}

pub struct Artefacts {
    pub architecture: String,
    pub conventions: String,
    pub invariants: String,
    pub index: String,
}

pub fn greenfield(bootstrap: &Bootstrap) -> Artefacts {
    let _ = bootstrap;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Bootstrap {
        Bootstrap {
            project_name: "demo".to_string(),
            modules: vec!["api".to_string(), "storage".to_string()],
            error_handling: "errors propagate; no swallowed exceptions".to_string(),
            test_shape: "one behaviour per test, named as a sentence".to_string(),
        }
    }

    #[test]
    fn architecture_names_every_module_with_a_boundary_line() {
        let artefacts = greenfield(&demo());
        assert!(artefacts.architecture.contains("## api"));
        assert!(artefacts.architecture.contains("## storage"));
        assert!(
            artefacts.architecture.contains("May depend on:"),
            "every module section carries an explicit dependency line:\n{}",
            artefacts.architecture
        );
    }

    #[test]
    fn conventions_freeze_the_answers_verbatim() {
        let artefacts = greenfield(&demo());
        assert!(
            artefacts
                .conventions
                .contains("errors propagate; no swallowed exceptions")
        );
        assert!(
            artefacts
                .conventions
                .contains("one behaviour per test, named as a sentence")
        );
    }

    #[test]
    fn index_says_it_is_generated_and_starts_empty() {
        let artefacts = greenfield(&demo());
        assert!(artefacts.index.contains("generated"));
        assert!(
            artefacts.index.contains("plumb index"),
            "the empty index must name the command that fills it:\n{}",
            artefacts.index
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let a = greenfield(&demo());
        let b = greenfield(&demo());
        assert_eq!(a.architecture, b.architecture);
        assert_eq!(a.conventions, b.conventions);
        assert_eq!(a.invariants, b.invariants);
        assert_eq!(a.index, b.index);
    }
}
```

Add `pub mod layer1;` to `crates/plumb-core/src/lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked layer1`
Expected: FAIL — panics on `todo!()`.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-core/src/layer1.rs crates/plumb-core/src/lib.rs
git commit -m "test: pin the shape of the Layer 1 artefacts"
```

- [ ] **Step 4: Implement `greenfield`**

Replace the `todo!()` body:

```rust
pub fn greenfield(bootstrap: &Bootstrap) -> Artefacts {
    let mut architecture = format!(
        "# Architecture — {}\n\n\
         Modules and the dependencies they are permitted. An import that\n\
         crosses a boundary not declared here is a finding, not a style\n\
         choice. Edit this file when the architecture changes on purpose.\n",
        bootstrap.project_name
    );
    for module in &bootstrap.modules {
        architecture.push_str(&format!(
            "\n## {module}\n\nResponsibility: (one sentence — fill in)\n\nMay depend on: (none)\n"
        ));
    }

    let conventions = format!(
        "# Conventions — {}\n\n\
         Frozen at bootstrap. A convention that lives in a head instead of a\n\
         file drifts between sessions; this file is the fix.\n\n\
         ## Error handling\n\n{}\n\n\
         ## Tests\n\n{}\n",
        bootstrap.project_name, bootstrap.error_handling, bootstrap.test_shape
    );

    let invariants = format!(
        "# Invariants — {}\n\n\
         Project-specific non-negotiables, one per heading, written as they\n\
         emerge. Empty is honest; aspirational is not.\n",
        bootstrap.project_name
    );

    let index = "# Index\n\n\
         <!-- generated: do not edit by hand -->\n\n\
         What this repository can already do, and where. Regenerated by\n\
         `plumb index`; empty until the first scan.\n"
        .to_string();

    Artefacts {
        architecture,
        conventions,
        invariants,
        index,
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked layer1`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-core/src/layer1.rs
git commit -m "feat: render Layer 1 artefacts from bootstrap answers"
```

---

### Task 2: `adapters` in plumb-core — artefacts to instruction files

**Files:**
- Create: `crates/plumb-core/src/adapters.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Consumes: `layer1::Artefacts` from Task 1.
- Produces (Task 4 relies on these exact names):

```rust
pub struct Adapter {
    pub path: &'static str,   // repo-relative, `/`-separated
    pub content: String,
}
pub const MARKER: &str = "generated by `plumb adapters`";
pub fn render(project_name: &str, artefacts: &Artefacts) -> Vec<Adapter>
```

- [ ] **Step 1: Write the failing tests**

`crates/plumb-core/src/adapters.rs` with `render` as `todo!()` and:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::layer1::Artefacts;

    fn artefacts() -> Artefacts {
        Artefacts {
            architecture: "# Architecture\n\n## api\n\nMay depend on: storage\n".to_string(),
            conventions: "# Conventions\n\nerrors propagate\n".to_string(),
            invariants: "# Invariants\n\nno network in tests\n".to_string(),
            index: "# Index\n\nempty\n".to_string(),
        }
    }

    #[test]
    fn renders_all_four_targets() {
        let paths: Vec<&str> = render("demo", &artefacts())
            .iter()
            .map(|adapter| adapter.path)
            .collect();
        assert_eq!(
            paths,
            vec![
                "CLAUDE.md",
                "AGENTS.md",
                ".cursorrules",
                ".github/copilot-instructions.md"
            ]
        );
    }

    #[test]
    fn every_target_opens_with_the_marker() {
        for adapter in render("demo", &artefacts()) {
            let first_line = adapter.content.lines().next().unwrap_or("");
            assert!(
                first_line.contains(MARKER),
                "{} does not open with the marker: {first_line:?}",
                adapter.path
            );
        }
    }

    #[test]
    fn every_target_carries_artefacts_and_protocols() {
        for adapter in render("demo", &artefacts()) {
            for expected in [
                "May depend on: storage",
                "errors propagate",
                "no network in tests",
                "Before writing",
                "While writing",
                "After writing",
            ] {
                assert!(
                    adapter.content.contains(expected),
                    "{} is missing {expected:?}",
                    adapter.path
                );
            }
        }
    }

    #[test]
    fn rendering_is_deterministic() {
        let a = render("demo", &artefacts());
        let b = render("demo", &artefacts());
        assert_eq!(a.len(), b.len());
        for (left, right) in a.iter().zip(&b) {
            assert_eq!(left.path, right.path);
            assert_eq!(left.content, right.content);
        }
    }
}
```

Add `pub mod adapters;` to `lib.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked adapters`
Expected: FAIL on `todo!()`.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-core/src/adapters.rs crates/plumb-core/src/lib.rs
git commit -m "test: pin the four adapter targets and their marker"
```

- [ ] **Step 4: Implement `render`**

```rust
pub const MARKER: &str = "generated by `plumb adapters`";

/// The instruction body shared by every harness.
///
/// One body on purpose: the harnesses differ in where they read instructions
/// from, not in what the project needs from an agent. A per-harness body would
/// drift the first time one of them was edited in a hurry.
fn body(project_name: &str, artefacts: &Artefacts) -> String {
    format!(
        "# {project_name} — working agreement\n\n\
         ## Protocols\n\n\
         **Before writing.** State which module owns the change, which\n\
         declared boundaries apply, and whether the change needs a new\n\
         external dependency.\n\n\
         **While writing.** Prefer the standard library to a new dependency,\n\
         and justify every dependency that survives that preference. Do not\n\
         create a module without declaring its boundary in the architecture.\n\
         Do not generalise before the third real use.\n\n\
         **After writing.** Run the gates. No completion claim without\n\
         command output.\n\n\
         {architecture}\n\n{conventions}\n\n{invariants}\n\n{index}\n",
        architecture = artefacts.architecture,
        conventions = artefacts.conventions,
        invariants = artefacts.invariants,
        index = artefacts.index,
    )
}

/// Renders every agent instruction file from the Layer 1 artefacts.
///
/// The marker line comes first in every target so `plumb adapters` can tell
/// its own output from a hand-written file it must not overwrite, and so a
/// future drift gate can tell both apart from an edited copy.
pub fn render(project_name: &str, artefacts: &Artefacts) -> Vec<Adapter> {
    let body = body(project_name, artefacts);
    let markdown = format!(
        "<!-- {MARKER}; edits will be overwritten — edit .plumb/ instead -->\n\n{body}"
    );
    let plain = format!("# {MARKER}; edits will be overwritten — edit .plumb/ instead\n\n{body}");

    vec![
        Adapter {
            path: "CLAUDE.md",
            content: markdown.clone(),
        },
        Adapter {
            path: "AGENTS.md",
            content: markdown.clone(),
        },
        Adapter {
            path: ".cursorrules",
            content: plain,
        },
        Adapter {
            path: ".github/copilot-instructions.md",
            content: markdown,
        },
    ]
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked adapters`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-core/src/adapters.rs
git commit -m "feat: render agent instruction files from one source"
```

---

### Task 3: `plumb init` in plumb-cli

**Files:**
- Create: `crates/plumb-cli/src/init.rs`
- Modify: `crates/plumb-cli/src/main.rs`
- Test: `crates/plumb-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `plumb_core::layer1::{Bootstrap, greenfield}`.
- Produces: `init::run(repo: &Path, defaults: bool, input: impl BufRead, output: impl Write) -> anyhow::Result<()>` — called from `main.rs`.

Behaviour to implement:
- Refuses to run if `.plumb/` already exists: error naming the directory, exit through the existing `failed()` path (70). Init is a bootstrap, not a sync; overwriting a working agreement silently is the defect class this project hunts.
- Four prompts, each with a default shown in brackets; empty line accepts the default. `--defaults` skips prompting entirely.
- Default project name: the repository directory's file name.
- Default module list: the top-level directories that contain at least one source file (extensions: `rs`, `py`, `ts`, `js`, `cs`, `go`, `java`), skipping `.git`, `target`, `node_modules`, `.plumb`, `docs`, `tests`. Files at the root count as a module named `root`. Empty repository → default `["core"]`. This is the whole brownfield story in v1: brownfield changes the defaults, not the code path.
- Writes `.plumb/architecture.md`, `.plumb/conventions.md`, `.plumb/invariants.md`, `.plumb/index.md`, and creates `.plumb/decisions/`.
- Prints one line per file written, and a final line telling the user to review `architecture.md` and fill in each module's `May depend on:`.

- [ ] **Step 1: Write the failing end-to-end tests**

Add to `crates/plumb-cli/tests/cli.rs`:

```rust
#[test]
fn init_with_defaults_writes_the_working_agreement() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success()
        .stdout(predicates::str::contains(".plumb/architecture.md"));

    for file in [
        ".plumb/architecture.md",
        ".plumb/conventions.md",
        ".plumb/invariants.md",
        ".plumb/index.md",
    ] {
        assert!(in_repo(dir.path(), file).exists(), "{file} was not written");
    }
    assert!(in_repo(dir.path(), ".plumb/decisions").is_dir());
}

#[test]
fn init_refuses_a_repository_that_already_has_an_agreement() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    std::fs::create_dir_all(in_repo(dir.path(), ".plumb")).expect("create .plumb");

    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .code(70)
        .stderr(predicates::str::contains(".plumb"));
}

#[test]
fn init_reads_answers_from_stdin() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .arg("init")
        .write_stdin("demo\napi, storage\n\n\n")
        .assert()
        .success();

    let architecture =
        std::fs::read_to_string(in_repo(dir.path(), ".plumb/architecture.md")).expect("read");
    assert!(architecture.contains("## api"), "{architecture}");
    assert!(architecture.contains("## storage"), "{architecture}");
}
```


Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-cli --locked --test cli init`
Expected: FAIL — `init` is not a subcommand yet, clap exits 64.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-cli/tests/cli.rs
git commit -m "test: pin plumb init end to end"
```

- [ ] **Step 4: Implement**

`crates/plumb-cli/src/init.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! `plumb init`: write the `.plumb/` working agreement.
//!
//! Greenfield and brownfield are one code path. A repository with code in it
//! changes the *defaults* — the module list is read from the directories that
//! hold source files — and the human corrects them at the prompt. Deriving
//! intent from code is an authoring problem, not a verification problem, and
//! this binary only automates what is deterministic.

use anyhow::{Context, Result, bail};
use plumb_core::layer1::{Bootstrap, greenfield};
use std::io::{BufRead, Write};
use std::path::Path;

const SOURCE_EXTENSIONS: &[&str] = &["rs", "py", "ts", "js", "cs", "go", "java"];
const SKIPPED_DIRS: &[&str] = &[".git", "target", "node_modules", ".plumb", "docs", "tests"];

pub fn run(repo: &Path, defaults: bool, mut input: impl BufRead, mut output: impl Write) -> Result<()> {
    let plumb_dir = repo.join(".plumb");
    if plumb_dir.exists() {
        bail!(
            ".plumb already exists in this repository; init writes a new working \
             agreement and will not overwrite one"
        );
    }

    let suggested = Bootstrap {
        project_name: repo
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".to_string()),
        modules: detect_modules(repo),
        error_handling: "errors propagate to the caller; no silently swallowed failures".to_string(),
        test_shape: "one behaviour per test, named as a sentence about that behaviour".to_string(),
    };

    let answers = if defaults {
        suggested
    } else {
        ask(&mut input, &mut output, suggested)?
    };

    let artefacts = greenfield(&answers);
    std::fs::create_dir_all(plumb_dir.join("decisions")).context("creating .plumb/decisions")?;
    for (name, content) in [
        ("architecture.md", &artefacts.architecture),
        ("conventions.md", &artefacts.conventions),
        ("invariants.md", &artefacts.invariants),
        ("index.md", &artefacts.index),
    ] {
        let path = plumb_dir.join(name);
        std::fs::write(&path, content).with_context(|| format!("writing {}", path.display()))?;
        writeln!(output, "wrote .plumb/{name}")?;
    }
    writeln!(
        output,
        "review .plumb/architecture.md and fill in each module's `May depend on:` line"
    )?;
    Ok(())
}

/// One prompt per answer, default shown in brackets, empty line keeps it.
fn ask(input: &mut impl BufRead, output: &mut impl Write, suggested: Bootstrap) -> Result<Bootstrap> {
    let project_name = prompt(input, output, "project name", &suggested.project_name)?;
    let modules_line = prompt(input, output, "modules (comma-separated)", &suggested.modules.join(", "))?;
    let error_handling = prompt(input, output, "error handling", &suggested.error_handling)?;
    let test_shape = prompt(input, output, "test shape", &suggested.test_shape)?;

    let modules: Vec<String> = modules_line
        .split(',')
        .map(|module| module.trim().to_string())
        .filter(|module| !module.is_empty())
        .collect();

    Ok(Bootstrap {
        project_name,
        modules: if modules.is_empty() { suggested.modules } else { modules },
        error_handling,
        test_shape,
    })
}

fn prompt(input: &mut impl BufRead, output: &mut impl Write, label: &str, default: &str) -> Result<String> {
    write!(output, "{label} [{default}]: ")?;
    output.flush()?;
    let mut line = String::new();
    input.read_line(&mut line).context("reading an answer")?;
    let answer = line.trim();
    Ok(if answer.is_empty() { default.to_string() } else { answer.to_string() })
}

/// Top-level directories holding at least one source file; root files count
/// as a module named `root`; an empty repository suggests `core`.
fn detect_modules(repo: &Path) -> Vec<String> {
    let mut modules = Vec::new();
    let Ok(entries) = std::fs::read_dir(repo) else {
        return vec!["core".to_string()];
    };
    let mut root_has_sources = false;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if !SKIPPED_DIRS.contains(&name.as_str()) && contains_source(&path) {
                modules.push(name);
            }
        } else if has_source_extension(&path) {
            root_has_sources = true;
        }
    }
    modules.sort();
    if root_has_sources {
        modules.push("root".to_string());
    }
    if modules.is_empty() {
        modules.push("core".to_string());
    }
    modules
}

fn contains_source(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if contains_source(&path) {
                return true;
            }
        } else if has_source_extension(&path) {
            return true;
        }
    }
    false
}

fn has_source_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| SOURCE_EXTENSIONS.contains(&extension))
}
```

In `main.rs`: `mod init;`, a subcommand variant, and dispatch:

```rust
    /// Write the .plumb/ working agreement for this repository.
    Init {
        /// Accept every default without asking.
        #[arg(long)]
        defaults: bool,
    },
```

```rust
        Commands::Init { defaults } => {
            let repo = match git::repo_root(Path::new(".")) {
                Ok(repo) => repo,
                Err(error) => return failed(&error),
            };
            let stdin = std::io::stdin();
            match init::run(&repo, defaults, stdin.lock(), std::io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => failed(&error),
            }
        }
```

- [ ] **Step 5: Run the full suite**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS, including the three new end-to-end tests.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-cli/src/init.rs crates/plumb-cli/src/main.rs
git commit -m "feat: plumb init writes the working agreement"
```

---

### Task 4: `plumb adapters` in plumb-cli

**Files:**
- Create: `crates/plumb-cli/src/adapters_cmd.rs`
- Modify: `crates/plumb-cli/src/main.rs`
- Test: `crates/plumb-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `plumb_core::layer1::Artefacts`, `plumb_core::adapters::{render, MARKER}`.
- Produces: `adapters_cmd::run(repo: &Path, force: bool, output: impl Write) -> anyhow::Result<()>`.

Behaviour to implement:
- Errors (70) if `.plumb/` or any of the four artefact files is missing, naming what is missing and pointing at `plumb init`.
- Project name = repository directory file name (same rule as init).
- For each rendered target: if the file exists, does **not** open with a line containing `MARKER`, and `--force` was not passed → error naming the file and the flag, before writing anything (check all targets first, then write). A hand-written CLAUDE.md is someone's work; destroying it silently is exactly the class of failure Plumb exists to catch.
- Writes each target, creating `.github/` if needed, printing one line per file.

- [ ] **Step 1: Write the failing end-to-end tests**

```rust
#[test]
fn adapters_generates_every_instruction_file() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();

    plumb_in(dir.path()).arg("adapters").assert().success();

    for file in [
        "CLAUDE.md",
        "AGENTS.md",
        ".cursorrules",
        ".github/copilot-instructions.md",
    ] {
        let content =
            std::fs::read_to_string(in_repo(dir.path(), file)).unwrap_or_else(|_| panic!("{file} missing"));
        assert!(
            content.contains("generated by `plumb adapters`"),
            "{file} lacks the marker"
        );
        assert!(content.contains("Before writing"), "{file} lacks protocols");
    }
}

#[test]
fn adapters_without_an_agreement_points_at_init() {
    let dir = repo_with_requirements("ruff==0.4.0\n");

    plumb_in(dir.path())
        .arg("adapters")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("plumb init"));
}

#[test]
fn adapters_refuses_to_overwrite_a_hand_written_file() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    plumb_in(dir.path())
        .args(["init", "--defaults"])
        .assert()
        .success();
    std::fs::write(in_repo(dir.path(), "CLAUDE.md"), "hand-written notes\n").expect("write");

    plumb_in(dir.path())
        .arg("adapters")
        .assert()
        .code(70)
        .stderr(predicates::str::contains("CLAUDE.md").and(predicates::str::contains("--force")));

    let content = std::fs::read_to_string(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert_eq!(content, "hand-written notes\n", "the file was clobbered");

    plumb_in(dir.path())
        .args(["adapters", "--force"])
        .assert()
        .success();
    let content = std::fs::read_to_string(in_repo(dir.path(), "CLAUDE.md")).expect("read");
    assert!(content.contains("generated by `plumb adapters`"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-cli --locked --test cli adapters`
Expected: FAIL — unknown subcommand, exit 64 where 70 or 0 is asserted.

- [ ] **Step 3: Commit the failing tests**

```bash
git add crates/plumb-cli/tests/cli.rs
git commit -m "test: pin plumb adapters end to end"
```

- [ ] **Step 4: Implement**

`crates/plumb-cli/src/adapters_cmd.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! `plumb adapters`: regenerate every agent instruction file from `.plumb/`.
//!
//! The artefacts are the single source of truth; the four generated files are
//! output. The one decision this module owns is the overwrite guard: a target
//! that exists without the marker is somebody's hand-written work, and a
//! generator that silently replaces it commits the exact defect class the
//! rest of this binary hunts.

use anyhow::{Context, Result, bail};
use plumb_core::adapters::{MARKER, render};
use plumb_core::layer1::Artefacts;
use std::io::Write;
use std::path::Path;

pub fn run(repo: &Path, force: bool, mut output: impl Write) -> Result<()> {
    let artefacts = read_artefacts(repo)?;
    let project_name = repo
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "project".to_string());
    let targets = render(&project_name, &artefacts);

    // Guard every target before writing any, so a refusal leaves the
    // repository exactly as it was rather than half-regenerated.
    if !force {
        for target in &targets {
            let path = repo.join(target.path);
            if let Ok(existing) = std::fs::read_to_string(&path) {
                let first_line = existing.lines().next().unwrap_or("");
                if !first_line.contains(MARKER) {
                    bail!(
                        "{} exists and was not generated by plumb; pass --force to \
                         overwrite it, or move its content into .plumb/ first",
                        target.path
                    );
                }
            }
        }
    }

    for target in &targets {
        let path = repo.join(target.path);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(&path, &target.content)
            .with_context(|| format!("writing {}", path.display()))?;
        writeln!(output, "wrote {}", target.path)?;
    }
    Ok(())
}

fn read_artefacts(repo: &Path) -> Result<Artefacts> {
    let read = |name: &str| -> Result<String> {
        std::fs::read_to_string(repo.join(".plumb").join(name)).with_context(|| {
            format!(".plumb/{name} is missing; run `plumb init` to write the working agreement")
        })
    };
    Ok(Artefacts {
        architecture: read("architecture.md")?,
        conventions: read("conventions.md")?,
        invariants: read("invariants.md")?,
        index: read("index.md")?,
    })
}
```

Wire in `main.rs`: `mod adapters_cmd;`, variant:

```rust
    /// Regenerate agent instruction files from .plumb/.
    Adapters {
        /// Overwrite target files that plumb did not generate.
        #[arg(long)]
        force: bool,
    },
```

```rust
        Commands::Adapters { force } => {
            let repo = match git::repo_root(Path::new(".")) {
                Ok(repo) => repo,
                Err(error) => return failed(&error),
            };
            match adapters_cmd::run(&repo, force, std::io::stdout()) {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => failed(&error),
            }
        }
```

- [ ] **Step 5: Run the full suite**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-cli/src/adapters_cmd.rs crates/plumb-cli/src/main.rs
git commit -m "feat: plumb adapters generates instruction files from one source"
```

---

### Task 5: Dogfood — Plumb's own working agreement, and a generated CLAUDE.md

**Files:**
- Create: `.plumb/architecture.md`, `.plumb/conventions.md`, `.plumb/invariants.md`, `.plumb/index.md`, `.plumb/decisions/` (this repository)
- Regenerate: `CLAUDE.md`, `AGENTS.md`, `.cursorrules`, `.github/copilot-instructions.md`
- Modify: `README.md` (status section: name the two new commands)

The `.plumb/` content is hand-written here, not `init` defaults: this is the
project's real working agreement, and the current hand-maintained `CLAUDE.md`
already says it is temporary until `plumb adapters` ships. Carry its normative
content over: the one rule (taxonomy traceability), the tiers table, the
plumb-core purity boundary, the toolchain and PATH note, the conventions
(SPDX headers, no unwrap, dependency justification, English, exit codes), the
version-checking rule, and the commit discipline — reorganised into the four
artefacts (rules about modules → `architecture.md`; style and commit rules →
`conventions.md`; "no source no rule", "tests never touch the network", "exit
codes are a contract" → `invariants.md`).

- [ ] **Step 1: Write `.plumb/` by hand, then generate**

Write the four artefact files carrying the content described above. Then:

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo run --locked -p plumb-cli -- adapters --force
```

Expected output: four `wrote …` lines. `--force` is needed once: the current
`CLAUDE.md` is hand-written and this run is the planned replacement.

- [ ] **Step 2: Verify the generated files**

Read the generated `CLAUDE.md` end to end. It must still tell an agent
everything the hand-written one did (Step 1's carry-over list). If something
is missing, fix it in `.plumb/`, regenerate, and re-read. Never edit the
generated files directly.

Run the full suite once more:

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked
```

- [ ] **Step 3: Update the README status section**

Add `plumb init` and `plumb adapters` to the commands the Status section
shows, with one sentence each; note that this repository's own `CLAUDE.md`,
`AGENTS.md`, `.cursorrules` and Copilot instructions are generated output and
`.plumb/` is where to edit.

- [ ] **Step 4: Commit**

```bash
git add .plumb CLAUDE.md AGENTS.md .cursorrules .github/copilot-instructions.md README.md
git commit -m "feat: Plumb runs under its own working agreement"
```
