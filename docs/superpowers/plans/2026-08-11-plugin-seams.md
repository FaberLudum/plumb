# Plugin Seams Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One trait per extension axis — `Language` and `EcosystemSpec` — so adding a language or an ecosystem means one new file plus one table line, not four to six scattered dispatch points.

**Architecture:** Pure code motion plus two new traits, all compiled-in. Per-language extractor code moves verbatim from `boundaries.rs`/`index.rs` into `languages/{rust,python,javascript}.rs` behind `trait Language`; per-ecosystem manifest parsing moves verbatim from `manifest.rs` into `ecosystems/{pypi,npm}.rs` behind `trait EcosystemSpec`. Observable behaviour is bit-identical; the existing suite proves it by passing unchanged. Spec: `docs/superpowers/specs/2026-08-11-plugin-seams-design.md`.

**Tech Stack:** Rust 1.97 edition 2024, no new dependencies.

## Global Constraints

- No existing test assertion may change. A moved test moves whole; a deleted pin test is deleted only in the same commit that deletes the duplication it guarded.
- Public CLI behaviour, findings, exit codes, generated files, cache format: bit-identical.
- Every Rust file opens with `// SPDX-License-Identifier: Apache-2.0` then `// Copyright 2026 The Plumb Authors`.
- No `unwrap()`/`expect()` outside tests. English everywhere. Conventional Commits, one commit per step, every message ends with the `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>` trailer.
- Gates before any completion claim: `cargo fmt --all --check && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` (PATH needs `export PATH="/c/Users/Andre/.cargo/bin:$PATH"`).
- Branch A (`feat/language-seam`): Tasks 1-4. Branch B (`feat/ecosystem-seam`, cut after A merges): Tasks 5-6.

---

### Task 1: `languages` module — trait, table, Rust moved in

**Files:**
- Create: `crates/plumb-core/src/languages/mod.rs`
- Create: `crates/plumb-core/src/languages/rust.rs`
- Modify: `crates/plumb-core/src/lib.rs` (add `pub mod languages;`)
- Modify: `crates/plumb-core/src/boundaries.rs` (delegate the `"rs"` arm)
- Modify: `crates/plumb-core/src/index.rs` (delegate the `"rs"` arm)

**Interfaces:**
- Consumes: `crate::index::Symbol` (existing struct, stays in `index.rs`).
- Produces (later tasks and both gates rely on these exact signatures):
  ```rust
  pub trait Language {
      fn extensions(&self) -> &'static [&'static str]; // lowercased
      fn imports(&self, text: &str) -> Vec<String>;
      fn symbols(&self, text: &str) -> Vec<crate::index::Symbol>;
  }
  pub fn all() -> &'static [&'static dyn Language];
  pub fn for_extension(extension: &str) -> Option<&'static dyn Language>;
  ```

- [ ] **Step 1: Write the failing structural tests** in `languages/mod.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_extension_is_claimed_by_two_languages() {
        let mut seen: Vec<&str> = Vec::new();
        for language in all() {
            for ext in language.extensions() {
                assert!(!seen.contains(ext), "{ext} claimed twice");
                seen.push(ext);
            }
        }
    }

    #[test]
    fn every_language_claims_at_least_one_extension() {
        for language in all() {
            assert!(!language.extensions().is_empty());
        }
    }

    #[test]
    fn for_extension_answers_for_every_declared_extension() {
        for language in all() {
            for ext in language.extensions() {
                assert!(for_extension(ext).is_some(), "{ext} not dispatched");
            }
        }
    }

    #[test]
    fn for_extension_answers_nothing_for_an_unknown_extension() {
        assert!(for_extension("toml").is_none());
        assert!(for_extension("").is_none());
    }
}
```

The module skeleton around them, minimal to compile:

```rust
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! The language table: one file per language, one registration line here.
//! Adding a language = implement `Language` in a new file, add it to `ALL`.

mod rust;

use crate::index::Symbol;

pub trait Language {
    /// Lowercased extensions this language claims.
    fn extensions(&self) -> &'static [&'static str];
    /// Import paths as written, line-based heuristic (see boundaries.rs docs).
    fn imports(&self, text: &str) -> Vec<String>;
    /// Public symbols, line-based heuristic (see index.rs docs).
    fn symbols(&self, text: &str) -> Vec<Symbol>;
}

static ALL: [&(dyn Language + Sync); 1] = [&rust::Rust];

pub fn all() -> &'static [&'static (dyn Language + Sync)] {
    &ALL
}

pub fn for_extension(extension: &str) -> Option<&'static (dyn Language + Sync)> {
    all()
        .iter()
        .copied()
        .find(|language| language.extensions().contains(&extension))
}
```

(`+ Sync` because the table is a `static`; callers only see `&dyn Language` methods, so the bound is invisible at use sites.)

- [ ] **Step 2: Run the structural tests, watch them fail to compile** (no `rust::Rust` yet): `cargo test -p plumb-core --locked languages` → expected: compile error `cannot find rust::Rust`.

- [ ] **Step 3: Create `languages/rust.rs`** — code motion, not rewriting:
  - Move from `boundaries.rs`: `rust_imports` and its helper `rust_name`, bodies verbatim.
  - Move from `index.rs`: `rust_symbols` and every Rust-only helper it calls (follow the call graph; a helper shared with Python/JS extractors stays in `index.rs` for now and moves in the task of its last user).
  - Wrap them (file opens with the two SPDX/copyright lines, then):

```rust
use super::Language;
use crate::index::Symbol;

pub struct Rust;

impl Language for Rust {
    fn extensions(&self) -> &'static [&'static str] {
        &["rs"]
    }
    fn imports(&self, text: &str) -> Vec<String> {
        rust_imports(text)
    }
    fn symbols(&self, text: &str) -> Vec<Symbol> {
        rust_symbols(text)
    }
}
```

  - Move the Rust-specific unit tests from `boundaries.rs`/`index.rs` test modules into `languages/rust.rs`'s test module, assertions untouched.

- [ ] **Step 4: Delegate the `"rs"` arms.** In `boundaries.rs::imports`: `"rs" => crate::languages::for_extension("rs").map(|l| l.imports(text)).unwrap_or_default(),` and the same shape for `index.rs::symbols` with `.symbols(text)`. Delete the now-unused `rust_imports`/`rust_name`/`rust_symbols` from their old files (they moved; the compiler's dead-code warnings are the checklist).

- [ ] **Step 5: Run the full gates, everything green**: `cargo fmt --all && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked` → expected: all suites pass, test count unchanged or higher (structural tests added, none removed).

- [ ] **Step 6: Commit** — two commits, test-first then implementation:

```bash
git add crates/plumb-core/src/languages/mod.rs crates/plumb-core/src/lib.rs
git commit -m "test(languages): pin the language table's structural invariants"
git add -A
git commit -m "refactor(languages): move the Rust extractors behind the Language trait"
```

### Task 2: Python moves in

**Files:**
- Create: `crates/plumb-core/src/languages/python.rs`
- Modify: `crates/plumb-core/src/languages/mod.rs` (`mod python;`, `ALL` grows to 2)
- Modify: `crates/plumb-core/src/boundaries.rs`, `crates/plumb-core/src/index.rs` (delegate `"py"` arm, delete moved code)

**Interfaces:**
- Consumes: `trait Language`, `Symbol` — exactly as Task 1 produced them.
- Produces: `pub struct Python;` implementing `Language` with `extensions() == &["py"]`.

- [ ] **Step 1:** Same mould as Task 1 steps 3-4: move `python_imports` (from `boundaries.rs`), `python_symbols` and Python-only helpers (from `index.rs`) verbatim into `languages/python.rs`; implement `Language` for `struct Python`; register in `ALL` (`static ALL: [&(dyn Language + Sync); 2] = [&rust::Rust, &python::Python];`); delegate the `"py"` arms; move Python-specific tests with assertions untouched.
- [ ] **Step 2: Run the full gates** — all green, no assertion changed.
- [ ] **Step 3: Commit**: `refactor(languages): move the Python extractors behind the Language trait`.

### Task 3: JavaScript moves in; dispatch becomes the table

**Files:**
- Create: `crates/plumb-core/src/languages/javascript.rs`
- Modify: `crates/plumb-core/src/languages/mod.rs` (`ALL` grows to 3)
- Modify: `crates/plumb-core/src/boundaries.rs`, `crates/plumb-core/src/index.rs` (whole dispatch replaced, duplicated lists and pin tests deleted)

**Interfaces:**
- Consumes: Task 1's trait and table.
- Produces: `pub struct JavaScript;` with `extensions() == &["ts", "tsx", "js", "jsx", "mjs", "cjs"]`. After this task `boundaries.rs` and `index.rs` contain **no** per-language code and **no** extension list.

- [ ] **Step 1:** Move `javascript_imports`, `javascript_symbols` and remaining shared helpers into `languages/javascript.rs` (a helper genuinely shared across two language files goes into `languages/mod.rs` as a private fn instead — not back into the old files). Register in `ALL`. Move JS-specific tests.
- [ ] **Step 2: Replace the dispatches.** `boundaries.rs::imports` body becomes:

```rust
pub fn imports(path: &str, text: &str) -> Vec<String> {
    match crate::languages::for_extension(&extension(path)) {
        Some(language) => language.imports(text),
        None => Vec::new(),
    }
}
```

`index.rs::symbols` becomes the same shape with `.symbols(text)`. `boundaries.rs::is_known_extension` and `index.rs::is_known_extension` are **deleted**; `is_scannable` and `has_scannable_extension` test `crate::languages::for_extension(&extension(path)).is_some()` instead.

- [ ] **Step 3: Delete the pin tests** `every_known_extension_is_dispatched_by_imports` (boundaries.rs) and `every_known_extension_is_dispatched_by_symbols` (index.rs) — the duplication they guarded no longer exists; the commit message says so.
- [ ] **Step 4: Run the full gates** — green; test count drops by exactly the two deleted pins net of moves.
- [ ] **Step 5: Commit**: `refactor(languages): drive both dispatches from the language table` with a body naming the deleted pin tests and why deletion is now safe.

### Task 4: cleanup, docs, dogfood

**Files:**
- Modify: `crates/plumb-core/src/boundaries.rs`, `crates/plumb-core/src/index.rs` (deduplicate `extension()` if both copies survive: keep one in `languages/mod.rs` as `pub(crate) fn extension_of(path: &str) -> String`, both files call it)
- Modify: `CONTRIBUTING.md` ("Adding a language" section)
- Regenerate: `.plumb/index.md`, `CLAUDE.md`, `AGENTS.md`, `.cursorrules`, `.github/copilot-instructions.md`

**Interfaces:**
- Consumes: everything above.
- Produces: the branch as reviewed.

- [ ] **Step 1:** Move the duplicated `extension()` helper to `languages/mod.rs` as `pub(crate) fn extension_of`, delete both old copies, update the two call sites.
- [ ] **Step 2:** Rewrite CONTRIBUTING's "Adding a language" numbered list to the new truth: implement `Language` in `crates/plumb-core/src/languages/<name>.rs`, add one line to `ALL`, structural tests enforce coherence automatically; extractors stay heuristic and line-based.
- [ ] **Step 3:** Run the full gates, then `cargo run -q -p plumb-cli --locked -- check --diff --offline`; when it reports `index-stale` (new public module), run `cargo run -q -p plumb-cli --locked -- index && cargo run -q -p plumb-cli --locked -- adapters --force` and re-run check to `no findings`.
- [ ] **Step 4: Commit** (`refactor(languages): single extension helper; docs and regenerated artefacts` — artefact regeneration may be its own `chore:` commit if check demanded it).

### Task 5: `ecosystems` module — trait, table, PyPI moved in

**Files:**
- Create: `crates/plumb-core/src/ecosystems/mod.rs`
- Create: `crates/plumb-core/src/ecosystems/pypi.rs`
- Modify: `crates/plumb-core/src/lib.rs` (add `pub mod ecosystems;`)
- Modify: `crates/plumb-core/src/manifest.rs` (delegate PyPI filenames)

**Interfaces:**
- Consumes: `manifest::{Manifest, Ecosystem, Gap}` — existing types, unmoved.
- Produces:
  ```rust
  pub trait EcosystemSpec {
      fn ecosystem(&self) -> crate::manifest::Ecosystem;
      /// Whether `filename` (final path segment, as-is) is a manifest of this ecosystem.
      fn matches(&self, filename: &str) -> bool;
      fn parse(&self, text: &str) -> crate::manifest::Manifest;
  }
  pub fn specs() -> &'static [&'static (dyn EcosystemSpec + Sync)];
  pub fn for_path(path: &str) -> Option<&'static (dyn EcosystemSpec + Sync)>;
  ```

- [ ] **Step 1: Write the failing structural tests** in `ecosystems/mod.rs` (same shape as Task 1: no filename matched by two specs — probe with the known set `["requirements.txt", "requirements-dev.txt", "pyproject.toml", "package.json"]`; every spec matches at least one probe; `for_path` extracts the final path segment before asking `matches`, pinned with `for_path("a/b/package.json")`).
- [ ] **Step 2: Watch them fail to compile**, then create `ecosystems/pypi.rs`: move `parse_requirements` and `parse_pyproject` (bodies verbatim) plus their private helpers and their tests out of `manifest.rs`; `pub struct PyPi;` implements the trait, `matches` reproducing exactly the current filename rule for requirements files and `pyproject.toml` (copy the existing predicate code, do not re-derive it — `requirements_dev.txt` staying unmatched is a documented decision).
- [ ] **Step 3:** `manifest::parse` delegates PyPI filenames through `ecosystems::for_path`, npm arm untouched. Full gates green.
- [ ] **Step 4: Commits**: `test(ecosystems): pin the ecosystem table's structural invariants` then `refactor(ecosystems): move PyPI manifest parsing behind EcosystemSpec`.

### Task 6: npm moves in; dispatch becomes the table; docs

**Files:**
- Create: `crates/plumb-core/src/ecosystems/npm.rs`
- Modify: `crates/plumb-core/src/ecosystems/mod.rs` (table grows to 2)
- Modify: `crates/plumb-core/src/manifest.rs` (`parse` body becomes a `for_path` lookup; per-ecosystem code and tests gone)
- Modify: `CONTRIBUTING.md` ("Adding a package ecosystem" section, steps renumbered to the new seam)
- Regenerate: `.plumb/index.md` and the four instruction files (new public module)

**Interfaces:**
- Consumes: Task 5's trait and table.
- Produces: `pub struct Npm;` (moves `parse_package_json` and the `node_modules` exclusion predicate verbatim). `manifest::parse` keeps its existing public signature exactly; only its body changes to the table lookup.

- [ ] **Step 1:** Move npm parsing + tests into `ecosystems/npm.rs`, register, collapse `manifest::parse` to the `for_path` lookup (a path no spec matches returns what the current code returns for an unrecognised file — copy that behaviour, do not invent).
- [ ] **Step 2:** Update CONTRIBUTING: adding an ecosystem = spec file in `ecosystems/` + table line + registry impl in `plumb-cli/src/registry.rs` + `RegistryRouter` arm + README blind-spots entry; the three registry properties (Unknown never a violation; illegal name never reaches the URL; cache outside the repo) stay verbatim.
- [ ] **Step 3:** Full gates; `plumb check` → regenerate index/adapters when `index-stale` fires; re-check to `no findings`.
- [ ] **Step 4: Commits**: `refactor(ecosystems): drive manifest parsing from the ecosystem table` (+ `chore:` regeneration commit if demanded).

---

## Self-review notes

- Spec §1 → Tasks 1-4; §2 → Tasks 5-6 (router deliberately untouched, per spec); §3 held by the Global Constraints; §4's structural tests are Task 1 Step 1 and Task 5 Step 1; §5's PR A/B split is the branch note in Global Constraints. PR C (NuGet) is out of scope here — own plan after B merges.
- No placeholder steps; moved-code steps name their functions explicitly and forbid rewriting.
- Type check: `for_extension` returns `Option<&'static (dyn Language + Sync)>` consistently (Task 1 declares, Task 3 consumes); `Symbol` never moves; `manifest::parse`'s public signature never changes.
