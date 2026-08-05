# Contributing

## The one rule that matters

**A pull request that adds or changes a rule must cite its evidence.**

Include the source, the metric, the effect size and the study's limits, and
say whether you verified the primary source or are relying on a secondary
report. Add the row to [docs/research/taxonomy.md](docs/research/taxonomy.md)
in the same pull request.

A rule without a source is closed, however sensible it sounds. This is not
bureaucracy — it is the only thing separating this project from another
opinionated linter, and it applies to the maintainers equally.

Removals follow the same discipline. If evidence is refuted, the rule goes and
the removal is recorded.

The taxonomy also lists, in §5, the defects Plumb must **not** fight — function
length, file size, cyclomatic complexity and readability among them. The
evidence says AI-generated code already matches or beats human code on those,
so enforcement spent there is friction with no return. A pull request
reintroducing them is closed too.

## Tiers

- **Tier A** blocks. Peer-reviewed evidence and objective detection only.
- **Tier B** warns. Preprint evidence, or thresholds that need calibration.
- **Tier C** measures and never affects the exit code.
- **Hygiene** is delegated to existing formatters and excluded from defect counts.

Proposing a Tier A rule on a preprint will be rejected. Propose it as Tier B.

Exit codes are a fixed public contract: `0` clean, `1` Tier A violation, `2`
warnings only. Anything that stops a run before it can reach a verdict exits
outside that range, so that a gate which did not run is never mistaken for a
gate that found nothing. Do not add a meaning to `0`, `1` or `2`.

## Dependencies

Plumb applies its own rule to itself. A new crate needs a row in
[docs/dependencies.md](docs/dependencies.md) explaining why the standard
library is not enough.

## Where code goes

`plumb-core` is pure: no network, no filesystem, no subprocesses. Anything that
touches the outside world lives in `plumb-cli`, behind a trait so a test can
substitute it. That boundary is what makes the core testable without a machine
to test against, and it is load-bearing rather than decorative.

Other conventions, all checked by review rather than by tooling:

- Every Rust source file opens with, in this order,
  `// SPDX-License-Identifier: Apache-2.0` then
  `// Copyright 2026 The Plumb Authors`.
- No `unwrap()` or `expect()` outside test code. Errors propagate with
  `anyhow::Result`. (`unwrap_or` and `unwrap_or_else` are different functions
  and are fine anywhere.)
- All code, comments, documentation and commit messages in English.
- Conventional Commits, one commit per step. A test and the change that makes
  it pass are two commits, in that order.

## Tests must not contact the network

Every test either injects a stub `Registry` or runs the binary with `--offline`
against a seeded cache. Nothing in the suite may depend on a live PyPI, both
because a test that fails on a train is a test people stop running, and because
a gate whose own tests need the thing it is gating cannot be trusted about it.

CI holds this by running the whole suite a second time in a network namespace
with no route out, which proves the isolation in both directions before it
believes the result. It catches a test that *depends* on a real answer. It does
not catch a test that makes a request and ignores the answer — the gate treats
an unreachable registry as no evidence, so such a test would still pass. Do not
write one.

## Before opening a pull request

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI runs exactly those three, on Linux, Windows and macOS. `Cargo.lock` is
committed, so `--locked` is not optional; `cargo fmt` is the one command that
rejects the flag, because it never resolves a dependency.

The toolchain comes from `rust-toolchain.toml` and is not written down anywhere
else. If you need a different one, change that file — CI reads it and fails if
the compiler in front of cargo is not the one it names.
