# Contributing

## The one rule that matters

**A pull request that adds or changes a rule must cite its evidence.**

Include the source, the metric, the effect size and the study's limits, and
say whether you verified the primary source or are relying on a secondary
report. Add the row to [docs/research/taxonomy.md](docs/research/taxonomy.md)
in the same pull request.

A rule without a source is closed, however sensible it sounds. This is not
bureaucracy. It is the only thing separating this project from another
opinionated linter, and it applies to the maintainers equally.

Removals follow the same discipline. If evidence is refuted, the rule goes and
the removal is recorded.

The taxonomy also lists, in §5, the defects Plumb must **not** fight, such as
function length, file size, cyclomatic complexity and readability. The
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
to test against. It is load-bearing, not decorative.

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

## Adding a language

Boundary checking and index scanning both dispatch on a single language
table, `crates/plumb-core/src/languages/mod.rs`. There is one place to
register a language, not two to keep in sync.

1. **Implement `Language`**, in a new file
   `crates/plumb-core/src/languages/<name>.rs`: the trait needs the
   extensions the language claims, an import extractor, and a symbol
   extractor. Look at `languages/python.rs` for the shape.
2. **Register it**: add one line for the new language to the `ALL` table in
   `languages/mod.rs`. That single line is what makes both boundary checking
   and index scanning see the language — there is no second dispatch to
   update.

Structural tests in `languages/mod.rs` enforce table coherence
automatically: no extension is claimed twice, every language claims at
least one extension, and `for_extension` answers for everything declared. A
half-done addition is a red test, not a silent gap.

Extractors are heuristic and line-based on purpose: no parser dependency,
no AST. Recognise the declaration forms that make up a public surface and
skip the rest. Perfect recall is not the bar; a stable, deterministic
surface map is.

This needs no new taxonomy row. It extends rules that already have one
(§4.5 for boundaries, §11 for the index) to another language.

## Adding a package ecosystem

The `package-exists` gate is Tier A and blocks builds, so this path is
stricter. Manifest recognition and parsing both dispatch on a single
ecosystem table, `crates/plumb-core/src/ecosystems/mod.rs` — the same shape
as the language table above. There is one place to register an ecosystem,
not several dispatch points to keep in sync.

1. **Implement `EcosystemSpec`**, in a new file
   `crates/plumb-core/src/ecosystems/<name>.rs`: `matches` recognises the
   manifest filename, and `parse` reads it — dispatching on the `filename`
   it is handed for an ecosystem with more than one manifest syntax (see
   `ecosystems/pypi.rs`; `ecosystems/npm.rs` shows the single-syntax shape).
   Emit a `Gap` for every declaration shape you knowingly do not read — an
   unread shape must warn (`manifest-understood`), never pass silently.
   Then **register it** with one line in the `ALL` table in
   `ecosystems/mod.rs`. That line is what makes `manifest::parse` see the
   new ecosystem — there is no second dispatch to update. Structural tests
   in `ecosystems/mod.rs` enforce table coherence automatically: no probe
   filename is matched by two specs, and every spec matches at least one
   probe.
2. **Added-name extraction**: `crates/plumb-core/src/deps.rs` already
   compares manifests by ecosystem; confirm the diff pairing picks up the
   new filename.
3. **Registry client**, in `crates/plumb-cli/src/registry.rs`: implement a
   `<Name>Registry` with the same contract as `PyPiRegistry` and
   `NpmRegistry`, add the arm in `RegistryRouter::exists`, and prefix cache
   keys with the ecosystem (`npm:` is the precedent; bare keys are legacy
   PyPI). Three properties are non-negotiable:
   - An unreachable registry is `Existence::Unknown`, **never** a
     violation. The gate blocks on proof of absence, not on absence of proof.
   - A name that is illegal in the ecosystem's namespace is `Missing`
     *without* a request. A malformed name interpolated into a URL can
     resolve to the wrong package and cache a poisoned answer. npm's
     validator exists because exactly that happened.
   - Cache entries answer for 30 days and live outside the repository.
4. **Update the blind spots.** The *Blind spots* section of
   [docs/gates.md](docs/gates.md) lists what each ecosystem's reader does
   not cover. Extend it in the same pull request. An undocumented gap in a
   blocking gate is the failure mode this project exists to catch.

Document the registry's lookup endpoint and its failure modes in the pull
request. Reviewers verify the `Unknown`/`Missing` mapping against the
registry's actual behaviour, not against its documentation.

No new taxonomy row is needed for another registry; §4.1 covers package
hallucination as a defect. A genuinely new *rule*, meaning a different
defect rather than a different ecosystem, needs its row first.

## Tests must not contact the network

No test in `plumb-core` can reach a registry. That is what the purity
boundary above is for. Every test that could, injects a stub `Registry` or
runs the binary with `--offline` against a cache seeded by the test itself.
Nothing in the suite may depend on a live PyPI, both because a test that
fails on a train is a test people stop running, and because a gate whose own
tests need the thing it is gating cannot be trusted about it.

CI holds this by running the whole suite a second time in a network namespace
with no route out, which proves the isolation in both directions before it
believes the result. It catches a test that *depends* on a real answer. It
does not catch a test that makes a request and ignores the answer, since the
gate treats an unreachable registry as no evidence and such a test would
still pass. Do not write one.

## Before opening a pull request

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

CI runs exactly those three, on Linux, Windows and macOS. `Cargo.lock` is
committed, so `--locked` is not optional. `cargo fmt` is the one command that
rejects the flag, because it never resolves a dependency.

The toolchain comes from `rust-toolchain.toml`. No build command and no CI file
repeats the version; CI installs whatever that file selects and then fails if
the compiler in front of cargo is not the one it names. The workspace
`Cargo.toml` does state `rust-version` as well, and it means a different thing:
the oldest compiler the crates claim to support, not the one CI uses. Raising
one is a decision about the other.
