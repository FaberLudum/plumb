# Plumb Core and Package Existence Gate — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a working `plumb check --diff` that reads the working diff, extracts newly added Python dependencies, verifies each one exists on PyPI, and fails the build when one does not.

**Architecture:** A Cargo workspace with two crates. `plumb-core` holds pure logic — diff parsing, manifest parsing, dependency set computation, findings and reporting — and is fully unit-testable without a filesystem or a network. `plumb-cli` is a thin binary that wires argument parsing, git invocation and the registry client around the core. Everything that touches the outside world (git, HTTP, cache files) lives behind a small trait so tests never hit the network.

**Tech Stack:** Rust 1.97, edition 2024, `clap` for arguments, `serde` and `serde_json` for the JSON report and cache, `toml` for `pyproject.toml`, `ureq` for blocking HTTP, `anyhow` for error handling, `assert_cmd` and `tempfile` for integration tests.

## Global Constraints

- Rust edition 2024. MSRV 1.97, declared as `rust-version` in the workspace package table and inherited by every crate manifest.
- Dependencies track current releases. No version is pinned by hand, ever. `Cargo.lock` is committed and builds use `--locked`. Plumb ships as a binary, so a low MSRV serves no consumer and buys nothing but dependency-resolution friction.
- Before writing code against any crate, check its current version and its current API. Guessing a version from memory is how this plan first shipped a two-year-old MSRV and a `ureq` 2 error match that no longer compiles.
- License Apache-2.0. Every source file starts with the two-line SPDX header: `// SPDX-License-Identifier: Apache-2.0` followed by `// Copyright 2026 The Plumb Authors`.
- Exit codes are fixed: `0` clean, `1` Tier A violation, `2` warnings only. Nothing else.
- No telemetry. The binary makes exactly one class of outbound request — registry lookups — and nothing else, ever.
- Offline mode degrades a check to `Unknown`, it never fails the run. A network error is not a violation.
- Every new third-party dependency gets a row in `docs/dependencies.md` stating why the standard library is not enough. This is the project's own Tier A rule applied to itself.
- All user-facing strings, code comments, documentation and commit messages are in English.
- No `unwrap()` or `expect()` outside `#[cfg(test)]` code. Errors propagate with `anyhow::Result`.

---

## File Structure

| File | Responsibility |
|---|---|
| `Cargo.toml` | Workspace root, shared dependency versions |
| `crates/plumb-core/src/lib.rs` | Public surface of the core crate, re-exports |
| `crates/plumb-core/src/diff.rs` | Unified diff parsing into changed files |
| `crates/plumb-core/src/manifest.rs` | Python manifest parsing into dependency sets |
| `crates/plumb-core/src/deps.rs` | Added-dependency computation from two sets |
| `crates/plumb-core/src/finding.rs` | Finding, Tier, Status types and exit-code mapping |
| `crates/plumb-core/src/report.rs` | Text and JSON rendering of findings |
| `crates/plumb-cli/src/main.rs` | Argument parsing, command dispatch |
| `crates/plumb-cli/src/git.rs` | git subprocess invocation |
| `crates/plumb-cli/src/registry.rs` | PyPI client, cache, offline handling |
| `crates/plumb-cli/src/check.rs` | The `check` command, wiring core to the outside |
| `docs/dependencies.md` | Justification for every third-party crate |

---

### Task 1: Workspace skeleton and version command

**Files:**
- Create: `Cargo.toml`
- Create: `crates/plumb-core/Cargo.toml`
- Create: `crates/plumb-core/src/lib.rs`
- Create: `crates/plumb-cli/Cargo.toml`
- Create: `crates/plumb-cli/src/main.rs`
- Create: `crates/plumb-cli/tests/cli.rs`
- Create: `docs/dependencies.md`
- Create: `rust-toolchain.toml`

**Interfaces:**
- Consumes: nothing.
- Produces: a `plumb` binary that responds to `--version`. Workspace members `plumb-core` and `plumb-cli`.

- [ ] **Step 1: Write the failing test**

Create `crates/plumb-cli/tests/cli.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use assert_cmd::Command;

#[test]
fn reports_its_version() {
    let mut cmd = Command::cargo_bin("plumb").expect("binary builds");
    cmd.arg("--version");
    cmd.assert().success().stdout(predicates::str::contains("plumb 0.1.0"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-cli`
Expected: FAIL. The workspace does not exist yet, so cargo reports that it could not find a `Cargo.toml`.

- [ ] **Step 3: Write the workspace and binary**

Create `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.97"
components = ["rustfmt", "clippy"]
```

Create `Cargo.toml` at the repository root:

```toml
[workspace]
members = ["crates/plumb-core", "crates/plumb-cli"]
resolver = "3"

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.97"
license = "Apache-2.0"

[workspace.dependencies]
anyhow = "1.0.104"
serde = { version = "1.0.229", features = ["derive"] }
serde_json = "1.0.151"
toml = "1.1.4"
clap = { version = "4.6.5", features = ["derive"] }
ureq = "3.3.0"
assert_cmd = "2.2.2"
predicates = "3.1.4"
tempfile = "3.27.0"
```

Create `crates/plumb-core/Cargo.toml`:

```toml
[package]
name = "plumb-core"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[dependencies]
anyhow.workspace = true
serde.workspace = true
serde_json.workspace = true
toml.workspace = true
```

Create `crates/plumb-core/src/lib.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Pure logic for Plumb's gates. Nothing in this crate touches the network,
//! the filesystem, or subprocesses; that all lives in `plumb-cli`.
```

Create `crates/plumb-cli/Cargo.toml`:

```toml
[package]
name = "plumb-cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true

[[bin]]
name = "plumb"
path = "src/main.rs"

[dependencies]
plumb-core = { path = "../plumb-core" }
anyhow.workspace = true
clap.workspace = true
serde.workspace = true
serde_json.workspace = true
ureq.workspace = true

[dev-dependencies]
assert_cmd.workspace = true
predicates.workspace = true
tempfile.workspace = true
```

Create `crates/plumb-cli/src/main.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use clap::Parser;

#[derive(Parser)]
#[command(name = "plumb", version, about = "Structural gates for AI-generated code")]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
```

Create `docs/dependencies.md`:

```markdown
# Third-party dependencies

Plumb's own Tier A rule applies to Plumb: every external crate needs a reason
the standard library is not enough.

| Crate | Why not std |
|---|---|
| `anyhow` | Error context chains across module boundaries. std has no equivalent for propagating human-readable context. |
| `clap` | Argument parsing with subcommands, help text and version. Hand-rolling this is a known source of bugs and no faster to write. |
| `serde` / `serde_json` | JSON report output and cache persistence. No serialisation in std. |
| `toml` | `pyproject.toml` parsing. No TOML parser in std. |
| `ureq` | Blocking HTTP for registry lookups. No HTTP client in std. Chosen over `reqwest` because it is blocking, has no async runtime, and pulls a far smaller tree. |
| `assert_cmd`, `predicates`, `tempfile` | Test-only. Integration testing of a binary and temporary git repositories. |
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-cli`
Expected: PASS. The `reports_its_version` test finds `plumb 0.1.0` in stdout.

- [ ] **Step 5: Commit**

```bash
git add Cargo.toml rust-toolchain.toml crates docs/dependencies.md
git commit -m "feat: add cargo workspace and plumb binary skeleton"
```

---

### Task 2: Unified diff parsing

**Files:**
- Create: `crates/plumb-core/src/diff.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `plumb_core::diff::ChangedFile { pub path: String, pub added_lines: Vec<String> }` and `plumb_core::diff::parse_unified_diff(diff: &str) -> Vec<ChangedFile>`.

A note on scope: this parser only needs to answer "which files changed, and what lines were added". It does not need hunk offsets, renames or binary detection. Resist adding them.

- [ ] **Step 1: Write the failing test**

Create `crates/plumb-core/src/diff.rs` containing only the test module for now:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/requirements.txt b/requirements.txt
index 1234567..89abcde 100644
--- a/requirements.txt
+++ b/requirements.txt
@@ -1,2 +1,3 @@
 ruff==0.4.0
+requestz==2.31.0
 pytest==8.0.0
diff --git a/src/app.py b/src/app.py
index 2222222..3333333 100644
--- a/src/app.py
+++ b/src/app.py
@@ -1 +1,2 @@
 import os
+import requestz
";

    #[test]
    fn extracts_one_entry_per_changed_file() {
        let files = parse_unified_diff(SAMPLE);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["requirements.txt", "src/app.py"]);
    }

    #[test]
    fn collects_added_lines_without_the_marker() {
        let files = parse_unified_diff(SAMPLE);
        assert_eq!(files[0].added_lines, vec!["requestz==2.31.0"]);
        assert_eq!(files[1].added_lines, vec!["import requestz"]);
    }

    #[test]
    fn does_not_emit_the_post_image_header_as_an_added_line() {
        let files = parse_unified_diff(SAMPLE);
        for file in &files {
            for line in &file.added_lines {
                assert!(
                    !line.starts_with("++ b/"),
                    "header leaked into added lines: {line}"
                );
            }
        }
    }

    #[test]
    fn attributes_a_header_shaped_added_line_to_the_enclosing_file() {
        const DIFF: &str = "\
diff --git a/notes.md b/notes.md
index 1111111..2222222 100644
--- a/notes.md
+++ b/notes.md
@@ -1 +1,3 @@
 changelog
+++ b/phantom.txt
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "notes.md");
        assert_eq!(
            files[0].added_lines,
            vec!["++ b/phantom.txt", "requestz==2.31.0"]
        );
    }

    #[test]
    fn attributes_added_lines_correctly_after_a_deletion() {
        const DIFF: &str = "\
diff --git a/old.py b/old.py
deleted file mode 100644
index 3333333..0000000
--- a/old.py
+++ /dev/null
@@ -1 +0,0 @@
-print('gone')
diff --git a/requirements.txt b/requirements.txt
index 4444444..5555555 100644
--- a/requirements.txt
+++ b/requirements.txt
@@ -1 +1,2 @@
 ruff==0.4.0
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "requirements.txt");
        assert_eq!(files[0].added_lines, vec!["requestz==2.31.0"]);
    }

    #[test]
    fn ignores_header_shaped_lines_inside_a_hunk_body() {
        const DIFF: &str = "\
diff --git a/fixtures/sample.patch b/fixtures/sample.patch
index 1111111..2222222 100644
--- a/fixtures/sample.patch
+++ b/fixtures/sample.patch
@@ -1,2 +1,2 @@
--- old
+++ b/phantom.txt
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "fixtures/sample.patch");
        assert_eq!(files[0].added_lines, vec!["++ b/phantom.txt"]);
    }

    #[test]
    fn records_a_newly_added_file() {
        const DIFF: &str = "\
diff --git a/requirements.txt b/requirements.txt
new file mode 100644
index 0000000..6666666
--- /dev/null
+++ b/requirements.txt
@@ -0,0 +1,2 @@
+ruff==0.4.0
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "requirements.txt");
        assert_eq!(
            files[0].added_lines,
            vec!["ruff==0.4.0", "requestz==2.31.0"]
        );
    }

    #[test]
    fn records_a_combined_diff_section_from_a_merge() {
        const DIFF: &str = "\
diff --git a/requirements.txt b/requirements.txt
index 1111111..2222222 100644
--- a/requirements.txt
+++ b/requirements.txt
@@ -1 +1,2 @@
 ruff==0.4.0
+httpx==0.27.0
diff --cc setup.py
index 3333333,4444444..5555555
--- a/setup.py
+++ b/setup.py
@@@ -1,1 -1,1 +1,3 @@@
  import setuptools
++import requestz
 +from_one_parent
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "requirements.txt");
        assert_eq!(files[0].added_lines, vec!["httpx==0.27.0"]);
        assert_eq!(files[1].path, "setup.py");
        assert_eq!(
            files[1].added_lines,
            vec!["import requestz", "from_one_parent"]
        );
    }

    #[test]
    fn returns_empty_for_an_empty_diff() {
        assert!(parse_unified_diff("").is_empty());
    }

    #[test]
    fn keeps_an_added_line_whose_content_starts_with_a_plus() {
        const DIFF: &str = "\
diff --git a/src/counter.c b/src/counter.c
index 1111111..2222222 100644
--- a/src/counter.c
+++ b/src/counter.c
@@ -1 +1,2 @@
 int total = 0;
++i;
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].added_lines, vec!["+i;"]);
    }

    #[test]
    fn keeps_an_added_line_whose_content_starts_with_two_pluses() {
        const DIFF: &str = "\
diff --git a/src/counter.c b/src/counter.c
index 1111111..2222222 100644
--- a/src/counter.c
+++ b/src/counter.c
@@ -1 +1,2 @@
 int total = 0;
+++i;
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].added_lines, vec!["++i;"]);
    }

    #[test]
    fn skips_a_file_the_diff_deletes() {
        const DIFF: &str = "\
diff --git a/old.py b/old.py
deleted file mode 100644
index 3333333..0000000
--- a/old.py
+++ /dev/null
@@ -1 +0,0 @@
-print('gone')
";
        assert!(parse_unified_diff(DIFF).is_empty());
    }
}
```

The header guard is deliberately absent from the implementation below. The
`+++ ` branch already consumes post-image headers, which git always emits with
a trailing space. A second `starts_with("+++")` guard inside the added-line
branch would catch nothing git produces and would silently drop any real added
line whose content begins with `++`.

Mind the plus count, because the first version of this note got it wrong.
Content `+i;` appears in the diff as `++i;`, which the guard never matched —
that case is safe either way and is pinned by
`keeps_an_added_line_whose_content_starts_with_a_plus`. The defect needs
content `++i;`, which appears as `+++i;`; that is what
`keeps_an_added_line_whose_content_starts_with_two_pluses` pins, and it is the
test that goes red against the guard.

The added-line branch is a let-chain rather than a nested `if let`. Nesting
trips clippy's `collapsible_if` under `-D warnings`; let-chains are stable in
edition 2024 and the semantics are identical.

Header recognition is confined to a file section's header block, and this is
the third attempt at that rule — the first two chased individual cases and both
leaked.

A line is only read as a header between a `diff --git ` line and the section's
first `@@ `. Inside a hunk body no line can be a header, because every hunk
line carries a prefix: added lines start with `+`, removed with `-`, context
with a space. So an unprefixed `diff --git ` or `@@ ` at column zero is always
structural, never content.

What the two earlier attempts missed, in order. Matching `+++ ` anywhere let an
added line whose content begins with `++ ` render as `+++ foo`, invent a
`ChangedFile` for a path no commit touched, and swallow every subsequent added
line of the real file. Requiring a preceding `--- ` narrowed that but did not
close it: a *removed* line whose content begins with `-- ` renders as `--- …`
and reopens the gate, so an adjacent removed/added pair inside a committed
`.patch` fixture reproduces the same phantom file.

The failure mode is the same each time and it is the worst one this parser can
produce: a false negative on a blocking gate. The real file ends up with no
added lines, so a hallucinated package added to a manifest goes unreported.

`records_a_newly_added_file` pins the case the rule most depends on. New files
arrive as `--- /dev/null` followed by `+++ b/…`, and a rule that tightened the
pre-image match would make every newly added manifest vanish from the gate with
every other test still green.

The fourth correction came from a re-reviewer who compiled the function and ran
real git captures against it. Matching `diff --git ` and `@@ ` exactly dropped
**combined diffs** entirely: plain `git diff` on a conflicted tree emits
`diff --cc` sections with `@@@` hunk headers. Those files vanished, and their
added lines were misattributed to whichever file preceded them.

A correction to that correction, from the following review: `git diff HEAD` —
the command Task 8 actually runs — never emits a combined diff. It compares the
worktree against HEAD and always produces plain `diff --git` sections. Bare
`git diff` and `git log -p --cc` do. The handling is still right, because this
is a public function whose contract is "git diff output", but the urgency
claimed for it was based on a command that does not behave that way.

Widening the two prefixes alone would record the files but leave an extra
marker column in the content — `++requestz==2.31.0` parses as a package named
`+requestz`, turning a false negative into a false positive on a blocking gate.
So the marker width is read from the hunk header's `@` count instead, which
handles any arity from one rule rather than from another special case.

`pub mod diff;` belongs in the **test** commit, not the implementation commit.
Left to step 3, `diff.rs` is not part of the crate during the RED run, so
`cargo test -p plumb-core diff` reports success over zero tests — a vacuous
failure that proves nothing.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-core diff`
Expected: FAIL with "cannot find function `parse_unified_diff` in this scope".

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-core/src/diff.rs`:

```rust
/// A file touched by a diff, with the lines the diff adds to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub added_lines: Vec<String>,
}

/// Returns the content of `line` when it is an added line, given a marker
/// column `marker_width` wide.
///
/// A combined diff from a merge carries one marker column per parent, so a
/// line added relative to both parents reads `++foo` and one added relative to
/// a single parent reads ` +foo`.
fn added_content(line: &str, marker_width: usize) -> Option<&str> {
    if !line.is_char_boundary(marker_width) {
        return None;
    }
    let (markers, content) = line.split_at(marker_width);
    if !markers.contains('+') {
        return None;
    }
    if !markers.chars().all(|c| matches!(c, '+' | '-' | ' ')) {
        return None;
    }
    Some(content)
}

/// Parses `git diff` output into one entry per changed file.
///
/// Headers are only recognised inside a file section's header block — between
/// a `diff --` line and the section's first `@@`. Inside a hunk body every line
/// carries a marker column, so an unprefixed line at column zero is always
/// structural and content can never be mistaken for a header.
///
/// Combined diffs from a merge — `diff --cc`, emitted by plain `git diff` on a
/// conflicted tree and by `git log -p --cc` — carry one marker column per
/// parent and open their hunks with `@@@`. The marker width is read from the
/// hunk header so their content is sliced correctly.
///
/// Only the post-image path and added lines are extracted; hunk offsets,
/// removals and renames are deliberately ignored. Input is assumed to be git
/// output: a bare `diff -u` patch, which has no `diff --` opener, yields
/// nothing.
pub fn parse_unified_diff(diff: &str) -> Vec<ChangedFile> {
    let mut files: Vec<ChangedFile> = Vec::new();
    let mut in_file_header = false;
    let mut marker_width = 1usize;

    for line in diff.lines() {
        if line.starts_with("diff --") {
            in_file_header = true;
            marker_width = 1;
        } else if let Some(rest) = line.strip_prefix("@@") {
            in_file_header = false;
            marker_width = 1 + rest.chars().take_while(|c| *c == '@').count();
        } else if in_file_header {
            // Do not collapse this into a let-chain on the arm above. The
            // nested form swallows every other header-block line; a let-chain
            // would let `index …`, `new file mode …` and the `--- ` line fall
            // through to the added-line branch below.
            if let Some(path) = line.strip_prefix("+++ ") {
                let path = path.strip_prefix("b/").unwrap_or(path);
                if path != "/dev/null" {
                    files.push(ChangedFile {
                        path: path.to_string(),
                        added_lines: Vec::new(),
                    });
                }
            }
        } else if let Some(added) = added_content(line, marker_width)
            && let Some(current) = files.last_mut()
        {
            current.added_lines.push(added.to_string());
        }
    }

    files
}
```

Add to `crates/plumb-core/src/lib.rs`:

```rust
pub mod diff;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-core diff`
Expected: PASS, twelve tests.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-core/src/diff.rs crates/plumb-core/src/lib.rs
git commit -m "feat: parse unified diffs into changed files and added lines"
```

---

### Task 3: Git subprocess layer

**Files:**
- Create: `crates/plumb-cli/src/git.rs`
- Modify: `crates/plumb-cli/src/main.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `git::working_diff(repo: &Path) -> anyhow::Result<String>` returning `git diff HEAD` output, and `git::show_at_head(repo: &Path, path: &str) -> anyhow::Result<Option<String>>` returning a file's contents at HEAD, or `None` when the file did not exist there.

`show_at_head` returning `None` rather than an error is load-bearing: a manifest added in this change has no HEAD version, and that is the normal case, not a failure.

That same `Ok(None)` also swallows a genuinely broken git, since exit 128 does not distinguish "no such path at HEAD" from "not a repository". It is sound here for two reasons, and both are conditions rather than guarantees: `working_diff` runs first in Task 8's flow and surfaces a broken git as an error, and the residual failure is fail-safe for this gate — an unreadable HEAD manifest makes every dependency look newly added, so the gate over-checks rather than skips. If `show_at_head` ever gains a caller not preceded by `working_diff`, switch to discriminating with `git ls-tree`.

Both functions carry `#[allow(dead_code, reason = "wired into the gate in a later task")]` until Task 8 calls them, because `dead_code` is a hard error under `-D warnings` and nothing calls them yet. `#[expect]` would be self-cleaning but does not work here: the tests exercise both functions, so the expectation goes unfulfilled in the test target. **Task 8 must delete both attributes**, and its own clippy run is what catches the omission.

The `HEAD` argument to `git diff` needs its own test. Editing an unstaged file produces an identical diff with or without it, so the obvious tests pass against an implementation that omits `HEAD` — verified by mutation, not assumed. The case that discriminates is staged work, which is what a pre-commit hook sees: without `HEAD` the diff comes back empty and the gate passes in silence.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `crates/plumb-cli/src/git.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    /// Runs a git command in `dir` and asserts it succeeded.
    ///
    /// Asserting matters more than it looks. `.output().expect(..)` only checks
    /// that git *spawned*; a command that runs and exits 128 is accepted
    /// silently. A global `commit.gpgsign` with no usable key, or a global
    /// `core.hooksPath`, then leaves the fixture with an unborn HEAD — and
    /// `show_at_head_returns_none_for_a_new_file` passes against that, because
    /// `git show HEAD:…` exits 128 and is mapped to `Ok(None)`. The one test
    /// pinning the absence contract is the one that survives a fixture that
    /// never ran.
    ///
    /// Ambient config is neutralised for the same reason: point
    /// `GIT_CONFIG_GLOBAL` and `GIT_CONFIG_SYSTEM` at a path that does not
    /// exist so the fixture cannot inherit this machine's settings.
    fn run_git(dir: &Path, args: &[&str]) {
        let no_config = dir.join("no-such-gitconfig");
        let output = Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", &no_config)
            .env("GIT_CONFIG_SYSTEM", &no_config)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        run_git(dir.path(), &["init", "-b", "main"]);
        run_git(dir.path(), &["config", "user.email", "test@example.com"]);
        run_git(dir.path(), &["config", "user.name", "Test"]);
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\n").expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "initial"]);
        dir
    }

    #[test]
    fn working_diff_reports_uncommitted_changes() {
        let dir = init_repo();
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\nrequestz==2.31.0\n")
            .expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(diff.contains("+requestz==2.31.0"), "diff was: {diff}");
    }

    /// Pins the `HEAD` argument. Without it `git diff` reports only unstaged
    /// work, so a pre-commit hook would see an empty diff and pass silently.
    /// The other tests all pass against that mutant; this one does not.
    #[test]
    fn working_diff_reports_staged_changes() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");
        run_git(dir.path(), &["add", "-A"]);

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(diff.contains("+requestz==2.31.0"), "diff was: {diff}");
    }

    #[test]
    fn working_diff_tolerates_non_utf8_content() {
        let dir = init_repo();
        let path = dir.path().join("latin1.txt");
        fs::write(&path, [0xE9u8, b'\n']).expect("write");
        run_git(dir.path(), &["add", "-A"]);
        run_git(dir.path(), &["commit", "-m", "add latin1"]);
        fs::write(&path, [0xE9u8, 0xE9, b'\n']).expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds despite non-UTF-8 content");

        // Assert on the replacement character, not on the path. A diff whose
        // hunk is suppressed — which a global `core.attributesFile` marking
        // `*.txt binary` produces, since `working_diff` honours ambient config
        // while the fixture does not — still contains the path, so a path
        // assertion would pass without the lossy decoding being exercised.
        assert!(diff.contains('\u{FFFD}'), "diff was: {diff}");
    }

    #[test]
    fn show_at_head_returns_the_committed_contents() {
        let dir = init_repo();
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\nrequestz==2.31.0\n")
            .expect("write");

        let head = show_at_head(dir.path(), "requirements.txt").expect("show succeeds");

        assert_eq!(head.as_deref(), Some("ruff==0.4.0\n"));
    }

    #[test]
    fn show_at_head_returns_none_for_a_new_file() {
        let dir = init_repo();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").expect("write");

        let head = show_at_head(dir.path(), "pyproject.toml").expect("show succeeds");

        assert_eq!(head, None);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-cli git`
Expected: FAIL with "cannot find function `working_diff` in this scope".

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-cli/src/git.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use anyhow::{bail, Context, Result};
use std::path::Path;
use std::process::Command;

/// Returns `git diff HEAD` for the given repository.
///
/// Output is decoded lossily, so it can contain U+FFFD wherever the diff
/// carried bytes that are not valid UTF-8. See the body for why.
// `allow` rather than `expect`: the tests below call both functions, so an
// `expect(dead_code)` would go unfulfilled in the test target. Task 8 deletes
// this attribute when it wires the functions into the gate.
#[allow(dead_code, reason = "wired into the gate in a later task")]
pub fn working_diff(repo: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["diff", "HEAD"])
        .current_dir(repo)
        .output()
        .context("failed to run git; is it installed and on PATH?")?;

    if !output.status.success() {
        bail!(
            "git diff failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }

    // Lossy on purpose. `git diff` writes hunk bytes verbatim for anything its
    // NUL heuristic calls text, so one Latin-1 or Shift-JIS source file changed
    // anywhere in the repository would otherwise fail the whole gate — for a
    // file the gate would never have looked at, with an error the user cannot
    // act on. Only manifest paths and added lines are read from this string,
    // and replacement characters in an unrelated file change none of them.
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Returns the contents of `path` as committed at HEAD.
///
/// `path` must be relative to the repository root and separated with forward
/// slashes, the form `git diff` emits. A backslash-separated path resolves to
/// nothing and reads as absence.
///
/// `Ok(None)` means the file does not exist at HEAD, which is the normal case
/// for a file added by the change under inspection. It also absorbs a broken
/// git, since exit 128 does not discriminate; that is acceptable only because
/// `working_diff` runs first and surfaces such a failure, and because the
/// residual case is fail-safe — an unreadable HEAD manifest makes every
/// dependency look newly added, so the gate over-checks rather than skips.
#[allow(dead_code, reason = "wired into the gate in a later task")]
pub fn show_at_head(repo: &Path, path: &str) -> Result<Option<String>> {
    let output = Command::new("git")
        .arg("show")
        .arg(format!("HEAD:{path}"))
        .current_dir(repo)
        .output()
        .context("failed to run git; is it installed and on PATH?")?;

    if !output.status.success() {
        return Ok(None);
    }

    let contents = String::from_utf8(output.stdout).context("git produced non-UTF-8 output")?;
    Ok(Some(contents))
}
```

Add to `crates/plumb-cli/src/main.rs`, above the `Cli` struct:

```rust
mod git;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-cli git`
Expected: PASS, five tests.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-cli/src/git.rs crates/plumb-cli/src/main.rs
git commit -m "feat: read the working diff and HEAD file contents from git"
```

---

### Task 4: Python manifest parsing

**Files:**
- Create: `crates/plumb-core/src/manifest.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `manifest::Ecosystem` (an enum with `PyPi`), `manifest::parse_requirements(text: &str) -> BTreeSet<String>`, `manifest::parse_pyproject(text: &str) -> BTreeSet<String>`, and `manifest::parse(path: &str, text: &str) -> Option<BTreeSet<String>>` which dispatches on filename and returns `None` for files that are not manifests.

Names are normalised per PEP 503: lowercased, with runs of `-`, `_` and `.` collapsed to a single `-`. Without this, `Flask-SQLAlchemy` and `flask_sqlalchemy` look like two different packages and the gate reports a phantom addition.

- [ ] **Step 1: Write the failing test**

Create `crates/plumb-core/src/manifest.rs` with only the test module:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

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
        assert_eq!(parse_requirements(text), set(&["flask-sqlalchemy", "zope-interface"]));
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
        assert!(parse("requirements.txt", "ruff\n").is_some());
        assert!(parse("requirements-dev.txt", "ruff\n").is_some());
        assert!(parse("pyproject.toml", "[project]\ndependencies = []\n").is_some());
        assert!(parse("src/app.py", "import os\n").is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-core manifest`
Expected: FAIL with "cannot find function `parse_requirements` in this scope".

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-core/src/manifest.rs`:

```rust
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
fn requirement_name(spec: &str) -> Option<String> {
    let spec = spec.split('#').next().unwrap_or(spec).trim();
    if spec.is_empty() || spec.starts_with('-') {
        return None;
    }
    let end = spec
        .find(|c: char| c.is_whitespace() || "[<>=!~;@(".contains(c))
        .unwrap_or(spec.len());
    let name = normalise(&spec[..end]);
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
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

    let Ok(value) = text.parse::<toml::Value>() else {
        return names;
    };
    let Some(project) = value.get("project") else {
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
            let Some(list) = group.as_array() else { continue };
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
```

Add to `crates/plumb-core/src/lib.rs`:

```rust
pub mod manifest;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-core manifest`
Expected: PASS, nine tests.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-core/src/manifest.rs crates/plumb-core/src/lib.rs
git commit -m "feat: parse Python manifests into normalised dependency sets"
```

---

### Task 5: Added-dependency computation

**Files:**
- Create: `crates/plumb-core/src/deps.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Consumes: `manifest::Ecosystem`.
- Produces: `deps::AddedDependency { pub name: String, pub ecosystem: Ecosystem, pub manifest: String }` and `deps::added(manifest_path: &str, before: Option<&BTreeSet<String>>, after: &BTreeSet<String>) -> Vec<AddedDependency>`.

Set difference rather than reading `+` lines from the diff. A reformatted manifest produces a diff full of added lines that add nothing; comparing parsed sets is immune to that.

- [ ] **Step 1: Write the failing test**

Create `crates/plumb-core/src/deps.rs` with only the test module:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

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
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-core deps`
Expected: FAIL with "cannot find function `added` in this scope".

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-core/src/deps.rs`:

```rust
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
            ecosystem: Ecosystem::PyPi,
            manifest: manifest_path.to_string(),
        })
        .collect()
}
```

Add to `crates/plumb-core/src/lib.rs`:

```rust
pub mod deps;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-core deps`
Expected: PASS, five tests.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-core/src/deps.rs crates/plumb-core/src/lib.rs
git commit -m "feat: compute added dependencies by manifest set difference"
```

---

### Task 6: Findings, tiers and exit codes

**Files:**
- Create: `crates/plumb-core/src/finding.rs`
- Create: `crates/plumb-core/src/report.rs`
- Modify: `crates/plumb-core/src/lib.rs`

**Interfaces:**
- Consumes: nothing.
- Produces: `finding::Tier` (`A`, `B`, `C`, `Hygiene`), `finding::Finding { pub rule: String, pub tier: Tier, pub message: String, pub location: String, pub source: String }`, `finding::exit_code(findings: &[Finding]) -> i32`, `report::render_text(findings: &[Finding]) -> String`, `report::render_json(findings: &[Finding]) -> String`.

`Finding::source` carries the taxonomy section that justifies the rule, and it is not optional. A finding that cannot say why it exists has no business blocking a build.

- [ ] **Step 1: Write the failing test**

Create `crates/plumb-core/src/finding.rs` with only the test module:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(tier: Tier) -> Finding {
        Finding {
            rule: "package-exists".to_string(),
            tier,
            message: "package `requestz` does not exist on PyPI".to_string(),
            location: "requirements.txt".to_string(),
            source: "taxonomy §4.1".to_string(),
        }
    }

    #[test]
    fn no_findings_exits_zero() {
        assert_eq!(exit_code(&[]), 0);
    }

    #[test]
    fn a_tier_a_finding_exits_one() {
        assert_eq!(exit_code(&[finding(Tier::A)]), 1);
    }

    #[test]
    fn warnings_alone_exit_two() {
        assert_eq!(exit_code(&[finding(Tier::B)]), 2);
    }

    #[test]
    fn hygiene_alone_exits_zero() {
        assert_eq!(exit_code(&[finding(Tier::Hygiene)]), 0);
    }

    #[test]
    fn tier_a_wins_over_warnings() {
        assert_eq!(exit_code(&[finding(Tier::B), finding(Tier::A)]), 1);
    }
}
```

Create `crates/plumb-core/src/report.rs` with only the test module:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;
    use crate::finding::{Finding, Tier};

    fn sample() -> Vec<Finding> {
        vec![Finding {
            rule: "package-exists".to_string(),
            tier: Tier::A,
            message: "package `requestz` does not exist on PyPI".to_string(),
            location: "requirements.txt".to_string(),
            source: "taxonomy §4.1".to_string(),
        }]
    }

    #[test]
    fn text_output_names_the_rule_location_and_source() {
        let text = render_text(&sample());
        assert!(text.contains("package-exists"), "was: {text}");
        assert!(text.contains("requirements.txt"), "was: {text}");
        assert!(text.contains("taxonomy §4.1"), "was: {text}");
    }

    #[test]
    fn text_output_says_so_when_clean() {
        assert!(render_text(&[]).contains("no findings"));
    }

    #[test]
    fn json_output_is_an_array_of_findings() {
        let json: serde_json::Value =
            serde_json::from_str(&render_json(&sample())).expect("valid json");
        assert_eq!(json[0]["rule"], "package-exists");
        assert_eq!(json[0]["tier"], "A");
        assert_eq!(json[0]["source"], "taxonomy §4.1");
    }

    #[test]
    fn json_output_is_an_empty_array_when_clean() {
        assert_eq!(render_json(&[]).trim(), "[]");
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-core`
Expected: FAIL with "cannot find type `Tier` in this scope" and "cannot find function `render_text` in this scope".

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-core/src/finding.rs`:

```rust
use serde::Serialize;

/// How strongly a rule is enforced, and on what strength of evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Tier {
    /// Peer-reviewed evidence, objective detection. Blocks.
    A,
    /// Preprint evidence or uncalibrated thresholds. Warns.
    B,
    /// Instrumentation only. Never affects the exit code.
    C,
    /// Style consistency. Autofixed, excluded from defect counts.
    Hygiene,
}

/// A single rule violation, with the evidence that justifies the rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    pub rule: String,
    pub tier: Tier,
    pub message: String,
    pub location: String,
    /// The taxonomy section justifying this rule. Never empty.
    pub source: String,
}

/// Maps findings to a process exit code: 0 clean, 1 Tier A, 2 warnings only.
pub fn exit_code(findings: &[Finding]) -> i32 {
    if findings.iter().any(|f| f.tier == Tier::A) {
        return 1;
    }
    if findings.iter().any(|f| f.tier == Tier::B) {
        return 2;
    }
    0
}
```

Add above the test module in `crates/plumb-core/src/report.rs`:

```rust
use crate::finding::{Finding, Tier};

/// Renders findings for a terminal.
pub fn render_text(findings: &[Finding]) -> String {
    if findings.is_empty() {
        return "plumb: no findings\n".to_string();
    }

    let mut out = String::new();
    for finding in findings {
        let label = match finding.tier {
            Tier::A => "blocking",
            Tier::B => "warning",
            Tier::C => "note",
            Tier::Hygiene => "hygiene",
        };
        out.push_str(&format!(
            "{}: {} [{}]\n  {}\n  evidence: {}\n",
            label, finding.message, finding.rule, finding.location, finding.source
        ));
    }
    out
}

/// Renders findings as a JSON array for machine consumption.
pub fn render_json(findings: &[Finding]) -> String {
    serde_json::to_string_pretty(findings).unwrap_or_else(|_| "[]".to_string())
}
```

Add to `crates/plumb-core/src/lib.rs`:

```rust
pub mod finding;
pub mod report;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-core`
Expected: PASS, all tests across diff, manifest, deps, finding and report.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-core/src/finding.rs crates/plumb-core/src/report.rs crates/plumb-core/src/lib.rs
git commit -m "feat: add findings, tier-based exit codes and report rendering"
```

---

### Task 7: PyPI registry client with cache and offline mode

**Files:**
- Create: `crates/plumb-cli/src/registry.rs`
- Modify: `crates/plumb-cli/src/main.rs`

**Interfaces:**
- Consumes: nothing from earlier tasks.
- Produces: `registry::Existence` (`Exists`, `Missing`, `Unknown`), the trait `registry::Registry { fn exists(&self, name: &str) -> Existence; }`, `registry::PyPiRegistry::new(offline: bool, cache_path: PathBuf) -> PyPiRegistry`, and `registry::Cache` persisted as JSON.

`Unknown` is a first-class outcome, not an error. A network failure must not turn into a build failure, per the spec's offline rule.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `crates/plumb-cli/src/registry.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn offline_mode_never_makes_a_request_and_reports_unknown() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, dir.path().join("registry.json"));

        assert_eq!(registry.exists("requestz"), Existence::Unknown);
    }

    #[test]
    fn a_cached_answer_is_returned_without_a_request() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, r#"{"known":{"ruff":true,"requestz":false}}"#)
            .expect("write cache");

        // Offline proves no request is made: the answers can only come from cache.
        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Exists);
        assert_eq!(registry.exists("requestz"), Existence::Missing);
    }

    #[test]
    fn a_missing_cache_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let registry = PyPiRegistry::new(true, dir.path().join("does-not-exist.json"));

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
    }

    #[test]
    fn a_corrupt_cache_file_is_not_an_error() {
        let dir = tempfile::tempdir().expect("temp dir");
        let cache_path = dir.path().join("registry.json");
        std::fs::write(&cache_path, "not json at all").expect("write cache");

        let registry = PyPiRegistry::new(true, cache_path);

        assert_eq!(registry.exists("ruff"), Existence::Unknown);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-cli registry`
Expected: FAIL with "cannot find type `PyPiRegistry` in this scope".

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-cli/src/registry.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;

/// Whether a package could be found in its registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Existence {
    Exists,
    Missing,
    /// The registry could not be consulted. Never a violation.
    Unknown,
}

/// A source of truth for whether a package name resolves.
pub trait Registry {
    fn exists(&self, name: &str) -> Existence;
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Cache {
    known: BTreeMap<String, bool>,
}

/// Resolves names against PyPI, with an on-disk cache and an offline mode.
pub struct PyPiRegistry {
    offline: bool,
    cache_path: PathBuf,
    cache: RefCell<Cache>,
}

impl PyPiRegistry {
    pub fn new(offline: bool, cache_path: PathBuf) -> Self {
        let cache = std::fs::read_to_string(&cache_path)
            .ok()
            .and_then(|text| serde_json::from_str::<Cache>(&text).ok())
            .unwrap_or_default();

        Self {
            offline,
            cache_path,
            cache: RefCell::new(cache),
        }
    }

    fn persist(&self) {
        let Ok(text) = serde_json::to_string(&*self.cache.borrow()) else {
            return;
        };
        if let Some(parent) = self.cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let _ = std::fs::write(&self.cache_path, text);
    }
}

impl Registry for PyPiRegistry {
    fn exists(&self, name: &str) -> Existence {
        if let Some(&known) = self.cache.borrow().known.get(name) {
            return if known { Existence::Exists } else { Existence::Missing };
        }

        if self.offline {
            return Existence::Unknown;
        }

        let url = format!("https://pypi.org/pypi/{name}/json");
        let outcome = match ureq::get(&url).call() {
            Ok(_) => Existence::Exists,
            Err(ureq::Error::StatusCode(404)) => Existence::Missing,
            Err(_) => Existence::Unknown,
        };

        if outcome != Existence::Unknown {
            self.cache
                .borrow_mut()
                .known
                .insert(name.to_string(), outcome == Existence::Exists);
            self.persist();
        }

        outcome
    }
}
```

Add to `crates/plumb-cli/src/main.rs`, next to the existing `mod git;`:

```rust
mod registry;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test -p plumb-cli registry`
Expected: PASS, four tests. None of them touch the network.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-cli/src/registry.rs crates/plumb-cli/src/main.rs
git commit -m "feat: resolve package names against PyPI with cache and offline mode"
```

---

### Task 8: Wire up `plumb check --diff`

**Files:**
- Create: `crates/plumb-cli/src/check.rs`
- Modify: `crates/plumb-cli/src/main.rs`
- Modify: `crates/plumb-cli/tests/cli.rs`

**Interfaces:**
- Consumes: `git::working_diff`, `git::show_at_head`, `plumb_core::diff::parse_unified_diff`, `plumb_core::manifest::parse`, `plumb_core::deps::added`, `plumb_core::finding::{Finding, Tier, exit_code}`, `plumb_core::report::{render_text, render_json}`, `registry::{Registry, Existence, PyPiRegistry}`.
- Produces: `check::run(repo: &Path, registry: &dyn Registry, json: bool) -> anyhow::Result<(String, i32)>` returning the rendered report and the exit code.

`run` takes a `&dyn Registry` so the integration test can inject a stub and stay offline.

- [ ] **Step 1: Write the failing test**

Add to the bottom of `crates/plumb-cli/src/check.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    struct StubRegistry;

    impl Registry for StubRegistry {
        fn exists(&self, name: &str) -> Existence {
            match name {
                "requestz" => Existence::Missing,
                "ruff" | "httpx" => Existence::Exists,
                _ => Existence::Unknown,
            }
        }
    }

    fn repo_with_requirements(initial: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("git runs");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        fs::write(dir.path().join("requirements.txt"), initial).expect("write");
        run(&["add", "-A"]);
        run(&["commit", "-m", "initial"]);
        dir
    }

    #[test]
    fn flags_a_dependency_that_does_not_exist() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let (output, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 1, "output was: {output}");
        assert!(output.contains("requestz"), "output was: {output}");
    }

    #[test]
    fn passes_when_every_added_dependency_exists() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\nhttpx==0.27.0\n")
            .expect("write");

        let (output, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 0, "output was: {output}");
    }

    #[test]
    fn ignores_a_removed_dependency() {
        let dir = repo_with_requirements("ruff==0.4.0\nrequestz==2.31.0\n");
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\n").expect("write");

        let (_, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 0);
    }

    #[test]
    fn an_unknown_result_does_not_block() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\nmystery==1.0\n")
            .expect("write");

        let (_, code) = run(dir.path(), &StubRegistry, false).expect("check runs");

        assert_eq!(code, 0);
    }

    #[test]
    fn json_output_parses_and_carries_the_evidence() {
        let dir = repo_with_requirements("ruff==0.4.0\n");
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let (output, _) = run(dir.path(), &StubRegistry, true).expect("check runs");
        let parsed: serde_json::Value = serde_json::from_str(&output).expect("valid json");

        assert_eq!(parsed[0]["rule"], "package-exists");
        assert_eq!(parsed[0]["source"], "taxonomy §4.1");
    }
}
```

Replace the contents of `crates/plumb-cli/tests/cli.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use assert_cmd::Command;

#[test]
fn reports_its_version() {
    let mut cmd = Command::cargo_bin("plumb").expect("binary builds");
    cmd.arg("--version");
    cmd.assert().success().stdout(predicates::str::contains("plumb 0.1.0"));
}

#[test]
fn check_runs_offline_and_reports_no_findings_on_a_clean_repo() {
    let dir = tempfile::tempdir().expect("temp dir");
    let run_git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .output()
            .expect("git runs");
    };
    run_git(&["init", "-b", "main"]);
    run_git(&["config", "user.email", "test@example.com"]);
    run_git(&["config", "user.name", "Test"]);
    std::fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\n").expect("write");
    run_git(&["add", "-A"]);
    run_git(&["commit", "-m", "initial"]);

    let mut cmd = Command::cargo_bin("plumb").expect("binary builds");
    cmd.args(["check", "--diff", "--offline"]).current_dir(dir.path());

    cmd.assert().success().stdout(predicates::str::contains("no findings"));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p plumb-cli`
Expected: FAIL with "cannot find function `run` in this scope" and an unrecognised `check` subcommand.

- [ ] **Step 3: Write minimal implementation**

Add above the test module in `crates/plumb-cli/src/check.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use crate::git;
use crate::registry::{Existence, Registry};
use anyhow::Result;
use plumb_core::deps;
use plumb_core::diff::parse_unified_diff;
use plumb_core::finding::{exit_code, Finding, Tier};
use plumb_core::manifest;
use plumb_core::report::{render_json, render_text};
use std::path::Path;

/// Runs the diff-scoped checks and returns the rendered report and exit code.
pub fn run(repo: &Path, registry: &dyn Registry, json: bool) -> Result<(String, i32)> {
    let diff = git::working_diff(repo)?;
    let changed = parse_unified_diff(&diff);
    let mut findings: Vec<Finding> = Vec::new();

    for file in &changed {
        let current = std::fs::read_to_string(repo.join(&file.path)).unwrap_or_default();
        let Some(after) = manifest::parse(&file.path, &current) else {
            continue;
        };

        let before = git::show_at_head(repo, &file.path)?
            .and_then(|text| manifest::parse(&file.path, &text));

        for dependency in deps::added(&file.path, before.as_ref(), &after) {
            if registry.exists(&dependency.name) == Existence::Missing {
                findings.push(Finding {
                    rule: "package-exists".to_string(),
                    tier: Tier::A,
                    message: format!(
                        "package `{}` does not exist on PyPI",
                        dependency.name
                    ),
                    location: dependency.manifest.clone(),
                    source: "taxonomy §4.1".to_string(),
                });
            }
        }
    }

    let output = if json {
        render_json(&findings)
    } else {
        render_text(&findings)
    };

    Ok((output, exit_code(&findings)))
}
```

Replace the contents of `crates/plumb-cli/src/main.rs`:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

mod check;
mod git;
mod registry;

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(name = "plumb", version, about = "Structural gates for AI-generated code")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run gates against the working diff.
    Check {
        /// Check the working diff. Currently the only supported scope.
        #[arg(long)]
        diff: bool,
        /// Never contact a registry; unresolved names are reported as unknown.
        #[arg(long)]
        offline: bool,
        /// Emit JSON instead of text.
        #[arg(long)]
        json: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    match cli.command {
        Commands::Check { diff: _, offline, json } => {
            let repo = PathBuf::from(".");
            let registry =
                registry::PyPiRegistry::new(offline, repo.join(".plumb/cache/registry.json"));

            match check::run(&repo, &registry, json) {
                Ok((output, code)) => {
                    print!("{output}");
                    ExitCode::from(code as u8)
                }
                Err(error) => {
                    eprintln!("plumb: {error:#}");
                    ExitCode::from(70)
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cargo test`
Expected: PASS across both crates. The five `check` tests and both CLI tests succeed, and no test contacts the network.

- [ ] **Step 5: Commit**

```bash
git add crates/plumb-cli/src/check.rs crates/plumb-cli/src/main.rs crates/plumb-cli/tests/cli.rs
git commit -m "feat: add plumb check --diff with the package existence gate"
```

---

### Task 9: CI, README and contribution policy

**Files:**
- Create: `.github/workflows/ci.yml`
- Create: `README.md`
- Create: `CONTRIBUTING.md`

**Interfaces:**
- Consumes: the `plumb` binary and the test suite from earlier tasks.
- Produces: nothing consumed by later tasks.

- [ ] **Step 1: Write the failing check**

There is no unit test for documentation. The verification is that CI passes on a clean checkout, so write the workflow first and let the first push be the failing run.

Create `.github/workflows/ci.yml`:

```yaml
name: ci

on:
  push:
    branches: [main]
  pull_request:

jobs:
  test:
    strategy:
      matrix:
        os: [ubuntu-latest, windows-latest, macos-latest]
    runs-on: ${{ matrix.os }}
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@1.97
        with:
          components: rustfmt, clippy
      - run: cargo fmt --all -- --check
      - run: cargo clippy --all-targets --locked -- -D warnings
      - run: cargo test --all --locked
```

- [ ] **Step 2: Run the checks locally to see them fail or pass**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test --all`
Expected: `cargo fmt` likely FAILS on formatting drift from the plan's code blocks. Run `cargo fmt --all` to fix, then re-run until all three pass.

- [ ] **Step 3: Write the documentation**

Create `README.md`:

```markdown
# Plumb

A plumb line checks whether a structure stands true. This tool does the same
for codebases written with AI assistance: it does not judge whether each
function is well made, it checks whether the system is still upright.

## Why this exists

The evidence says something counterintuitive. At the unit level, AI-generated
code is **equal to or better than** human code — smaller functions, simpler
branches, comparable or better readability. At the system level it degrades:
introduced debt persists, dependencies multiply, architecture erodes as volume
grows.

Every quality tool in common use is calibrated on human failure modes. Human
code has a heavy tail — a few enormous functions among many ordinary ones — so
those tools hunt for peaks. AI code is uniformly moderate. The tools find
nothing while the system decays below their thresholds.

Plumb targets the failure modes those tools structurally cannot see.

Every rule cites its evidence. See [the taxonomy](docs/research/taxonomy.md)
for the full basis, including which sources we have verified against the
primary paper and which we have not.

## Status

Early. Today `plumb check --diff` verifies that Python dependencies added by a
change actually exist on PyPI — the defect measured by Spracklen et al.
(USENIX Security 2025) at 19.7% on 2024 models and by Churilov (2026) at
4.6-6.1% on frontier models. Still non-zero, and still a supply-chain attack
surface.

## Usage

```bash
plumb check --diff            # check the working diff
plumb check --diff --json     # machine-readable output
plumb check --diff --offline  # never contact a registry
```

Exit codes: `0` clean, `1` blocking violation, `2` warnings only.

## Network and privacy

Plumb makes exactly one class of outbound request: resolving package names
against public registries. Nothing else leaves your machine, and there is no
telemetry. `--offline` disables even that; unresolved names are reported as
unknown rather than as violations.

## License

Apache-2.0.
```

Create `CONTRIBUTING.md`:

```markdown
# Contributing

## The one rule that matters

**A pull request that adds or changes a rule must cite its evidence.**

Include the source, the metric, the effect size and the study's limits, and
say whether you verified the primary source or are relying on a secondary
report. Add the row to `docs/research/taxonomy.md` in the same pull request.

A rule without a source is closed, however sensible it sounds. This is not
bureaucracy — it is the only thing separating this project from another
opinionated linter, and it applies to the maintainers equally.

Removals follow the same discipline. If evidence is refuted, the rule goes and
the removal is recorded.

## Tiers

- **Tier A** blocks. Peer-reviewed evidence and objective detection only.
- **Tier B** warns. Preprint evidence, or thresholds that need calibration.
- **Tier C** measures and never affects the exit code.
- **Hygiene** is delegated to existing formatters and excluded from defect counts.

Proposing a Tier A rule on a preprint will be rejected. Propose it as Tier B.

## Dependencies

Plumb applies its own rule to itself. A new crate needs a row in
`docs/dependencies.md` explaining why the standard library is not enough.

## Before opening a pull request

```bash
cargo fmt --all
cargo clippy --all-targets -- -D warnings
cargo test --all
```

Tests must not contact the network. Inject a stub `Registry` instead.
```

- [ ] **Step 4: Verify the checks pass**

Run: `cargo fmt --all -- --check && cargo clippy --all-targets -- -D warnings && cargo test --all`
Expected: all three PASS.

- [ ] **Step 5: Commit**

```bash
git add .github/workflows/ci.yml README.md CONTRIBUTING.md
git commit -m "ci: add cross-platform workflow, README and contribution policy"
```

---

## Out of scope for this plan

Named here so the next plan has somewhere to start, and so nobody adds them to this one.

- Import-level package resolution. Mapping `import yaml` to the distribution `PyYAML` is a genuinely hard problem and is deliberately deferred; the manifest is where installation actually happens.
- TypeScript and .NET manifests. The same shape of code, one ecosystem at a time.
- The remaining Tier A gates: SAST delegation and runtime dependency closure.
- All of Tier B: volume and coupling tracking, boundary checks, diff-scoped debt analysis.
- Layer 1 entirely — `plumb init`, the five artefacts, adapter generation.
- Release binaries and package manager distribution.
