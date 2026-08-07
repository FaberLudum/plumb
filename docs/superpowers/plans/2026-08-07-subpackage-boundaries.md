# Sub-Package Boundary Resolution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Boundaries declared between sub-packages (`## src/ai_energy_manager/documents` vs `## src/ai_energy_manager/facts`) become enforceable: the extractor keeps the full dotted import path and resolution matches import segments against module path segments, so the boundary gate and `report`'s coupling delta finally see inside single-package monoliths — the structural limit phase-6 calibration measured (coupling 0 across 25 commits on a real repo of exactly that shape).

**Architecture:** Two changes in `plumb-core/src/boundaries.rs`, both generalising what exists. (1) The three import matchers stop truncating to the first segment and return the import path as written (`ai_energy_manager.documents.domain`, `plumb_core::manifest`, `react-dom/client`). (2) `target_of` becomes segment-based: an import resolves to the module whose path's **trailing segments** match the import's **leading segments** for the longest run, `-`/`_` normalised — today's last-segment rule is exactly the run-length-1 case, so every current resolution is preserved and sub-package headings simply win by being longer matches. `violations`, `edges` and the CLI inherit the gain untouched (they call the same resolver). No API signature changes.

**Tech Stack:** Rust 1.97, edition 2024. No new dependencies.

## Global Constraints

- SPDX + copyright pair; no `unwrap()`/`expect()` outside `#[cfg(test)]`; plumb-core pure; no new deps; no network in tests; exit codes 0/1/2 (+70); TDD (the existing extractor tests are the spec of the changed behaviour — updating them IS the failing-test step, commit them red first); Conventional Commits, never amend, trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; fmt/clippy/full suite clean before each implementation commit; cargo needs `export PATH="/c/Users/Andre/.cargo/bin:$PATH" &&`.

## Verification

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo fmt --all --check
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo clippy --workspace --all-targets --locked -- -D warnings
export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked
```

## Contract details

**Extraction (changed):** each matcher returns the import path as written in the source, not just its head — Rust `use plumb_core::manifest::parse;` → `plumb_core::manifest::parse` (stop at `;`, whitespace, `{`, `(`); `use crate::`/`self::`/`super::` still skipped; `extern crate NAME` stays `NAME`. Python `from ai_energy_manager.documents import x` → `ai_energy_manager.documents`; `import os.path` → `os.path`; relative still skipped. JS `from 'react-dom/client'` → `react-dom/client`; `@scope/pkg/deep` → `@scope/pkg/deep`; relative still skipped.

**Segments:** an import splits on `::`, `.` and `/`; a module path splits on `/`. Each segment normalises `-` → `_` for comparison (existing rule).

**Resolution (changed):** import segments `I`, module path segments `M`. A module matches with score `k` when `k ≥ 1` and the last `k` segments of `M` equal the first `k` segments of `I` (normalised). The import resolves to the module with the highest `k`; ties go to the first-declared module (existing precedent). No match → not ours, ignored. Score-1 matches reproduce today's behaviour exactly.

**Consequences to preserve and pin:** a file in `src/pkg/facts` importing `pkg.facts.helpers` resolves to its own module (self-import, allowed); importing `pkg.documents` resolves to `src/pkg/documents` (needs permission); `use plumb_core::manifest` from plumb-cli still resolves to `crates/plumb-core` (score 1, unchanged); `std`, `serde`, `react` still resolve to nothing. Rust modules nested below a crate (`crates/x/src/y`) stay unreachable by crate-qualified imports because `src` interposes — documented limitation, Rust remains crate-level.

**Violation.import** carries the import as written (now the full path) — messages get more precise for free.

---

### Task 1: extraction keeps the full import path (plumb-core)

**Files:** Modify: `crates/plumb-core/src/boundaries.rs`

- [ ] **Step 1: Update the extractor tests to the new contract (they are the spec of the change)**

In the existing test module, change the extraction assertions:

```rust
    #[test]
    fn rust_use_lines_yield_the_full_path() {
        let text = "use plumb_core::manifest;\npub use serde::Serialize;\nuse std::path::Path;\n";
        assert_eq!(
            imports("crates/x/src/main.rs", text),
            vec!["plumb_core::manifest", "serde::Serialize", "std::path::Path"]
        );
    }

    #[test]
    fn python_imports_yield_the_dotted_path() {
        let text = "import requests\nfrom flask import Flask\nfrom . import sibling\nimport os.path\nfrom ai_energy_manager.documents import domain\n";
        assert_eq!(
            imports("api/app.py", text),
            vec!["requests", "flask", "os.path", "ai_energy_manager.documents"]
        );
    }

    #[test]
    fn javascript_imports_yield_the_full_spec_and_skip_relative() {
        let text = "import x from 'react-dom/client';\nconst y = require(\"lodash\");\nimport z from './local';\nimport s from '@scope/pkg/deep';\n";
        assert_eq!(
            imports("web/index.ts", text),
            vec!["react-dom/client", "lodash", "@scope/pkg/deep"]
        );
    }
```

Rename the old tests to these (same coverage, new expectations). Keep every other extraction test (crate/self/super skipped, comma-separated Python `import os, sys` → now `os` and `sys` as written — multi-name splitting must survive, brace-group Rust `use a::{b, c}` → `a` as before? NO: with full paths a brace group stops at `{`, yielding `plumb_core::` trailing? Contract: stop BEFORE `{`, trim any trailing `::` — `use plumb_core::{a, b};` → `plumb_core`. Add that as an explicit test:

```rust
    #[test]
    fn a_rust_brace_group_yields_the_path_before_the_brace() {
        let text = "use plumb_core::{manifest, finding};\n";
        assert_eq!(imports("crates/x/src/main.rs", text), vec!["plumb_core"]);
    }
```

- [ ] **Step 2: Run to verify the changed tests fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked boundaries`
Expected: FAIL — extractors still truncate.

- [ ] **Step 3: Commit the red tests**

```bash
git add crates/plumb-core/src/boundaries.rs
git commit -m "test: pin full-path import extraction"
```

- [ ] **Step 4: Implement**

Rust: `rust_name` keeps consuming through `::` and identifier chars, stopping at `;`, whitespace, `{`, `(`; strip a trailing `::`. Python: `python_name` keeps `.`-joined segments (stop at whitespace, `,`, `(`; comma-splitting for `import a, b` intact, each name now full). JS: return the whole quoted spec verbatim (relative-skip logic unchanged, drop the first-segment truncation and the special `@scope` two-segment logic — the full spec supersedes both).

NOTE: `violations`/`edges`/`target_of` still expect head-only names at this point and will mis-resolve full paths — that is Task 2's job. To keep the workspace GREEN between tasks, adapt `target_of` minimally in THIS task: split the incoming import on `::`/`.`/`/` and use only the first segment for resolution (behaviour identical to today, now explicit). Add a one-line comment that Task 2 replaces this with segment matching.

- [ ] **Step 5: Full suite green**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS — resolution behaviour unchanged (head-segment), extraction richer. Some cli/check tests assert violation messages containing the import name — if any asserts `imports \`cli\`` style text against what is now `cli::main`, update those assertions in the same commit and say so in the commit body (they pin the message format, and the message just got more precise).

- [ ] **Step 6: Commit**

```bash
git add -A
git commit -m "feat: extract the full import path, resolve by its head"
```

---

### Task 2: segment-based resolution (plumb-core)

**Files:** Modify: `crates/plumb-core/src/boundaries.rs`

- [ ] **Step 1: Write the failing resolution tests**

```rust
    fn monolith() -> architecture::Architecture {
        architecture::parse(
            "## src/pkg/documents\n\nMay depend on: (none).\n\n\
             ## src/pkg/facts\n\nMay depend on: documents.\n\n\
             ## src/pkg\n\nMay depend on: (none).\n",
        )
    }

    #[test]
    fn a_subpackage_import_resolves_to_the_deepest_matching_module() {
        let found = violations(
            &monolith(),
            "src/pkg/documents/store.py",
            "from pkg.facts import totals\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "src/pkg/facts");
        assert_eq!(found[0].import, "pkg.facts");
    }

    #[test]
    fn a_permitted_subpackage_dependency_is_not_a_violation() {
        let found = violations(
            &monolith(),
            "src/pkg/facts/totals.py",
            "from pkg.documents import domain\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn an_intra_subpackage_import_is_a_self_import() {
        let found = violations(
            &monolith(),
            "src/pkg/facts/totals.py",
            "from pkg.facts.helpers import round_kwh\n",
        );
        assert!(found.is_empty(), "{found:?}");
    }

    #[test]
    fn a_longer_match_beats_a_shorter_one() {
        // `pkg.documents` must reach src/pkg/documents (score 2), not stop at
        // src/pkg (score 1), even though src/pkg is also declared.
        let found = violations(
            &monolith(),
            "src/pkg/facts/totals.py",
            "import pkg.documents.domain\n",
        );
        assert!(found.is_empty(), "resolved to src/pkg instead of the deeper module: {found:?}");
    }

    #[test]
    fn crate_level_rust_resolution_is_unchanged() {
        let architecture = architecture::parse(
            "## crates/plumb-core\n\nMay depend on: (none).\n\n## crates/plumb-cli\n\nMay depend on: plumb-core.\n",
        );
        let found = violations(
            &architecture,
            "crates/plumb-cli/src/main.rs",
            "use plumb_core::manifest;\n",
        );
        assert!(found.is_empty(), "{found:?}");
        let found = violations(
            &architecture,
            "crates/plumb-core/src/lib.rs",
            "use plumb_cli::check;\n",
        );
        assert_eq!(found.len(), 1, "{found:?}");
    }

    #[test]
    fn a_tie_goes_to_the_first_declared_module() {
        let architecture = architecture::parse(
            "## apps/api\n\nMay depend on: (none).\n\n## services/api\n\nMay depend on: (none).\n",
        );
        let found = violations(&architecture, "services/api/x.py", "import api.routes\n");
        // `api` matches both at score 1; first declared wins, so the target is
        // apps/api and the import from services/api is a violation against it.
        assert_eq!(found.len(), 1, "{found:?}");
        assert_eq!(found[0].to_module, "apps/api");
    }
```

- [ ] **Step 2: Run to verify they fail**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-core --locked boundaries`
Expected: FAIL — head-only resolution stops at `src/pkg` or misses sub-packages.

- [ ] **Step 3: Commit the red tests**

```bash
git add crates/plumb-core/src/boundaries.rs
git commit -m "test: pin segment-based sub-package resolution"
```

- [ ] **Step 4: Implement**

Replace `target_of`'s head-segment interim with the contract's rule: split the import into segments (`::`, `.`, `/`), for each module compute the longest `k` where the module path's last `k` segments equal the import's first `k` (normalised), keep the module with max `k` (`Iterator::max_by_key` is stable-by-last for equal keys — verify and, if it keeps the LAST maximum, iterate with an explicit fold keeping the first). Update `permits` only if a test proves it needs it (last-segment matching should already cover `May depend on: documents`). `edges` and `violations` share the resolver — no separate change.

- [ ] **Step 5: Full suite green**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test --workspace --locked`
Expected: PASS, including every pre-existing boundary/edges/report test.

- [ ] **Step 6: Commit**

```bash
git add crates/plumb-core/src/boundaries.rs
git commit -m "feat: resolve imports to the deepest matching module"
```

---

### Task 3: end-to-end proof and documentation

**Files:** Modify: `crates/plumb-cli/tests/cli.rs`, `README.md`

- [ ] **Step 1: Failing e2e test — the calibration scenario, reproduced**

```rust
#[test]
fn a_cross_subpackage_import_warns_when_declared() {
    let dir = repo_with_requirements("ruff==0.4.0\n");
    let plumb_dir = in_repo(dir.path(), ".plumb");
    std::fs::create_dir_all(&plumb_dir).expect("mkdir");
    std::fs::write(
        plumb_dir.join("architecture.md"),
        "## src/pkg/documents\n\nMay depend on: (none).\n\n## src/pkg/facts\n\nMay depend on: (none).\n",
    )
    .expect("write");
    let facts = in_repo(dir.path(), "src/pkg/facts");
    std::fs::create_dir_all(&facts).expect("mkdir");
    std::fs::write(facts.join("totals.py"), "VALUE = 1\n").expect("write");
    run_git(&repo(dir.path()), &["add", "-A"]);
    run_git(&repo(dir.path()), &["commit", "-m", "scaffold"]);
    std::fs::write(
        facts.join("totals.py"),
        "from pkg.documents import domain\nVALUE = 1\n",
    )
    .expect("write");

    plumb_in(dir.path())
        .args(["check", "--diff", "--offline"])
        .assert()
        .code(2)
        .stdout(
            predicates::str::contains("boundary")
                .and(predicates::str::contains("src/pkg/facts"))
                .and(predicates::str::contains("src/pkg/documents")),
        );
}
```

- [ ] **Step 2: Red, commit**

Run: `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo test -p plumb-cli --locked --test cli subpackage`
Expected: FAIL (exit 0 — sub-package invisible before Task 2 lands; if Tasks 1-2 are already merged into the branch it passes, in which case commit it as a pinning test with an adjusted message).

```bash
git add crates/plumb-cli/tests/cli.rs
git commit -m "test: pin the cross-subpackage boundary end to end"
```

- [ ] **Step 3: README**

Rewrite the F4 caveat and the blind-spots bullets this feature obsoletes: boundaries can now be declared per sub-package; the resolver matches import segments against module path trailing segments, longest match wins. State what remains true: Rust stays crate-level (`src` interposes between crate dir and module file — a crate-qualified import cannot reach `crates/x/src/y` headings); resolution needs the filesystem layout to mirror the import path (true for Python packages and JS, not for exotic source roots); ambiguity between equal-scoring modules goes to the first declared. Update the `plumb report` caveat: coupling is no longer structurally inert on single-package monoliths once sub-package modules are declared — but the numbers only appear after the architecture declares them.

- [ ] **Step 4: Full verification + commit**

```bash
git add README.md
git commit -m "docs: boundaries reach sub-packages now"
```

---

### Task 4: dogfood on the calibration repository

**Files (external, C:\Lavoro\EnerLoop\AI-Energy-Manager):** `.plumb/architecture.md` only. NOTHING else; every source edit is scratch and reverted; NO commits in that repository.

- [ ] **Step 1: Declare the real sub-package architecture**

Rewrite that repo's `.plumb/architecture.md`: modules `src/ai_energy_manager/documents`, `src/ai_energy_manager/facts`, `src/ai_energy_manager/conversations`, `src/ai_energy_manager/identity`, plus a catch-all `src` module, `apps`, `alembic` (May depend on: src). Give the sub-packages honest `May depend on:` lists by INSPECTING actual imports first (grep `from ai_energy_manager\.` across each sub-package and write what is genuinely imported today — the declaration must describe reality, then tighten later; a declaration that instantly warns on legacy code on every touch is the muting hazard).

- [ ] **Step 2: Prove the gate**

Rebuild the release binary. Scratch edit: add `from ai_energy_manager.identity import models` (or another genuinely-undeclared cross-sub-package import — pick one that is NOT in the declared list) to a file under `documents/`, run `check --diff --offline`, expect exit 2 with a `boundary` finding naming both sub-package modules. Paste output. Revert.

- [ ] **Step 3: Prove the coupling signal**

Run `report --commits 25`. Compare with the calibration baseline (coupling was 0 across all 25). Any non-zero coupling row is the proof the signal came alive. Paste the output.

- [ ] **Step 4: Record**

Append the evidence (both outputs) to the PR body when it is opened — no README change for this step; the e2e test from Task 3 already pins the behaviour in-repo.
