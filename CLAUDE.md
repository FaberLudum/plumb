# Plumb

> **Temporary file.** Per the design spec §8, this file is meant to be
> *generated* from `.plumb/` artefacts by `plumb adapters`, alongside
> `AGENTS.md` and the Cursor and Copilot equivalents. Layer 1 does not exist
> yet, so this is hand-maintained for now. When `plumb adapters` ships, this
> file becomes generated output and hand edits to it stop being read.

A plumb line checks whether a structure stands true. Plumb does the same for
codebases written with AI assistance: it does not judge whether each function
is well made, it checks whether the system is still upright.

## The one rule

**Every rule Plumb enforces must trace back to a row in
[docs/research/taxonomy.md](docs/research/taxonomy.md). No source, no rule.**

This applies to the maintainers, not only to contributors. If you are about to
add a check because it seems sensible, stop — that is how the project turns
into another opinionated linter.

The taxonomy also lists defects Plumb must **not** fight (§5): function
length, cyclomatic complexity, readability thresholds. The evidence says AI
already matches or beats humans on those. Do not reintroduce them.

## Why the design looks the way it does

The evidence says AI-generated code is equal to or better than human code at
the unit level and degrades at the system level. So:

- Constraints operate on the system, not on the line.
- Constraints are executable gates, not written rules — Zhu et al. found
  detailed prompting does not mitigate architectural decay.

## Tiers

| Tier | Enforcement | Evidence required |
|---|---|---|
| A | Blocks the build | Peer-reviewed, objective detection |
| B | Warns | Preprint, or thresholds needing calibration |
| C | Measures only | None — instrumentation for open hypotheses |
| Hygiene | Autofixed, excluded from defect counts | Style consistency, delegated to formatters |

Proposing a Tier A rule on a preprint is a rejection. Propose it as Tier B.

## Layout

```
crates/plumb-cli/     binary entry point, git, registry, commands
crates/plumb-core/    pure logic — no network, no filesystem, no subprocesses
docs/research/        taxonomy.md, sources.md — the evidence base
docs/superpowers/     specs and implementation plans
```

The `plumb-core` boundary is load-bearing: anything that touches the outside
world belongs in `plumb-cli`, behind a trait so tests can stub it. Tests must
never contact the network.

## Toolchain

Rust 1.97, edition 2024. `cargo` is not on the inherited bash PATH on this
machine — prefix invocations:

```bash
export PATH="/c/Users/Andre/.cargo/bin:$PATH"
```

```bash
cargo fmt --all
cargo clippy --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Conventions

- Every Rust source file opens with, in this order:
  `// SPDX-License-Identifier: Apache-2.0` then
  `// Copyright 2026 The Plumb Authors`
- No `unwrap()` or `expect()` outside test code. Errors propagate with
  `anyhow::Result`. (`unwrap_or` and `unwrap_or_else` are different functions
  and are fine anywhere.)
- Every third-party crate needs a row in
  [docs/dependencies.md](docs/dependencies.md) saying why the standard library
  is not enough. Plumb's own Tier A rule, applied to Plumb.
- All code, comments, documentation and commit messages in English.
- Exit codes are fixed: `0` clean, `1` Tier A violation, `2` warnings only.

## Versions

**Check the current version and current API of a crate before writing code
against it.** Do not write a version from memory.

This is not pedantry. The first implementation plan shipped an MSRV two years
stale and a `ureq` error match that no longer compiled, both because a version
was recalled rather than looked up. Dependencies track current releases,
nothing is pinned by hand, `Cargo.lock` is committed, and builds use
`--locked`.

## Commits

One commit per step that changes files — a TDD cycle produces at least two,
the failing test and the implementation. Conventional Commits. Never amend,
never squash. Every commit message ends with:

```
Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
```
