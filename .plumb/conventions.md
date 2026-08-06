# Conventions — Plumb

Frozen at bootstrap. A convention that lives in a head instead of a file
drifts between sessions; this file is the fix.

## File headers

Every Rust source file opens with, in this order:

```
// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors
```

## Error handling

No `unwrap()` or `expect()` outside test code. Errors propagate with
`anyhow::Result`. (`unwrap_or` and `unwrap_or_else` are different functions
and are fine anywhere.)

## Dependencies

Every third-party crate needs a row in
[docs/dependencies.md](docs/dependencies.md) saying why the standard
library is not enough. Plumb's own Tier A rule, applied to Plumb.

## Language

All code, comments, documentation and commit messages in English.

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

## Check versions before writing code

Check the current version and current API of a crate before writing code
against it. Do not write a version from memory. This is not pedantry: the
first implementation plan shipped an MSRV two years stale and a `ureq` error
match that no longer compiled, both because a version was recalled rather
than looked up. Dependencies track current releases, nothing is pinned by
hand, `Cargo.lock` is committed, and builds use `--locked`.

## Commits

One commit per step that changes files — a TDD cycle produces at least two,
the failing test and the implementation. Conventional Commits. Never amend,
never squash. Every commit message ends with:

```
Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>
```
