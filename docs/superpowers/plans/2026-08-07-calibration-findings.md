# Calibration Findings Fix Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development or superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Fix the three defects and document the one limitation that phase-6 calibration on a real repository (AI-Energy-Manager: Python monolith, 218 commits, hand-written CLAUDE.md/AGENTS.md) surfaced.

**Context — how each was found:** `.plumb/` was bootstrapped on that repo, all gates were exercised with scratch edits, and the full transcript of what fired lives in the session. Every finding below was observed live, not hypothesised.

**Tech Stack:** Rust 1.97, edition 2024. No new dependencies.

## Global Constraints

- SPDX + copyright header pair; no `unwrap()`/`expect()` outside `#[cfg(test)]`; plumb-core pure; no new deps; no network in tests; exit codes 0/1/2 (+70); TDD (failing test commit precedes fix commit, per finding); Conventional Commits, never amend, trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`; fmt/clippy/full suite clean before each fix commit; cargo needs `export PATH="/c/Users/Andre/.cargo/bin:$PATH" &&`.

## Findings

### F1 — Important: the drift gate fires on hand-written instruction files

Observed: on a repo with a hand-written `CLAUDE.md`/`AGENTS.md` and a fresh `.plumb/`, `plumb check --diff` warned `adapter-drift ... run \`plumb adapters\`` on both — permanently, and the named remedy is the command that would overwrite them (its guard refuses, but the gate keeps pointing at it). A hand-written file is non-adoption, not drift.

Fix in `crates/plumb-cli/src/check.rs` (drift block): a target file is a drift candidate ONLY if its first line carries the generated marker — reuse `plumb_core::adapters::opens_with_marker` on the disk file's first line (lossy-decoded), exactly as `adapters_cmd`'s overwrite guard does. Marker absent → not adopted → no finding.

Tests: (a) unit — full `.plumb/` present, hand-written `CLAUDE.md` (no marker) on disk → exit 0, no `adapter-drift`; (b) the existing drifted/matching tests stay green (their files carry the marker).

### F2 — Minor: `init` lists dot-directories as modules

Observed: the derived `architecture.md` on the real repo contained `## .venv` and `## .worktrees`.

Fix in `crates/plumb-cli/src/init.rs` (`detect_modules`): skip any top-level directory whose name starts with `.` (in addition to `SKIPPED_DIRS`); also skip them inside `contains_source`'s recursion (a nested `.venv` full of Python must not make its parent a module).

Test: unit — a temp dir with `.venv/lib/x.py` and `api/app.py` → modules are `["api"]` only.

### F3 — Minor: `index`'s adapters reminder fires for hand-written instruction files

Observed: `plumb index` on the real repo printed `run \`plumb adapters\` to refresh the instruction files` although both instruction files are hand-written and `plumb adapters` would refuse.

Fix in `crates/plumb-cli/src/index_cmd.rs`: print the reminder only when at least one existing instruction file opens with the marker (same `opens_with_marker` check as F1).

Test: e2e — init + hand-written CLAUDE.md, run `index` → stdout does NOT contain `plumb adapters`; and (existing behaviour) with generated files present it still does.

### F4 — Documentation: Python boundary granularity, coupling inert on single-package monoliths

Observed: the repo's real boundaries live between sub-packages of one top-level package (`ai_energy_manager.documents` vs `.facts` …). The extractor keeps only the top import segment, so every internal import resolves to the whole package: intra-package boundaries are invisible, and `plumb report` showed coupling 0 across 25 commits — the signal is inert on this repo class.

Fix: README — one bullet in the boundary blind-spots register and one caveat sentence in the report section, stating both facts and that sub-package resolution is future work, not a config error. No code change.

## Execution

- [ ] F1: failing tests → commit → fix → commit
- [ ] F2: failing test → commit → fix → commit
- [ ] F3: failing test → commit → fix → commit
- [ ] F4: README edit → commit
- [ ] Full verification (fmt, clippy, workspace tests, `check --diff --offline` exit 0 on clean tree)
