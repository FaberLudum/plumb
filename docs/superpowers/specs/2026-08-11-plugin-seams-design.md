# Plugin seams for languages and ecosystems — design

**Date:** 2026-08-11
**Status:** approved (Andrea, 2026-08-11)
**Outcome:** compiled-in trait seams — "plugin-oriented, not plugin-loaded"

## Problem

Adding a language today touches four to six dispatch points spread over
`boundaries.rs` and `index.rs`, kept coherent only by pin tests; adding an
ecosystem touches `manifest.rs`, `deps.rs`, `registry.rs` and the router.
The extension recipe exists in CONTRIBUTING, but the seams it describes are
incidental, not designed. With NuGet, pub.dev, C# and Dart queued behind a
real .NET + Flutter repository, the touch surface is about to be paid four
more times.

## Decision

One trait per axis, one file per implementation, one static table per axis.
Everything stays compiled into the binary.

Two alternatives were considered and rejected:

- **Declarative data tables** (import patterns and manifests as data): the
  matchers carry real logic — line-based heuristics, the npm name
  validator, manifest gap detection — that data cannot express. It would
  end half-data, half-code.
- **External process plugins** (`plumb-plugin-*` on PATH): a tool whose
  blocking gate is about supply-chain trust must not execute arbitrary
  binaries found on the PATH. Also breaks the no-network testing story and
  determinism. Reopen only if real end-user demand appears.

The rule "do not generalise before the third real use" is satisfied, not
bent: three languages exist (Rust, Python, JavaScript/TypeScript) and the
third ecosystem (NuGet) is queued.

## Section 1 — Language seam (`plumb-core`)

New module `languages/` with:

```rust
pub trait Language {
    fn extensions(&self) -> &'static [&'static str];   // lowercased
    fn imports(&self, text: &str) -> Vec<String>;      // boundary gate
    fn symbols(&self, text: &str) -> Vec<Symbol>;      // index
}
```

- One file per language: `languages/rust.rs`, `languages/python.rs`,
  `languages/javascript.rs`. The existing extractor code **moves** there;
  it is not rewritten.
- One static table: `languages::all() -> &'static [&'static dyn Language]`,
  and a single dispatch point `languages::for_extension(ext)`.
- `boundaries::imports`, `index::symbols`, `is_scannable` and
  `has_scannable_extension` all delegate to that table. The two duplicated
  extension lists disappear; the pin tests
  `every_known_extension_is_dispatched_by_{imports,symbols}` become
  structurally impossible to fail and are removed in the same commit that
  removes the lists they guarded.
- Adding a language = one file implementing the trait + one line in
  `all()`.

## Section 2 — Ecosystem seam

- `plumb-core`: trait `EcosystemSpec` with `matches(filename)` and
  `parse(text) -> Manifest`; one file per ecosystem; a static table
  `manifest::specs()`. The `Ecosystem` enum **stays** — stable contracts
  (cache key prefixes such as `npm:`, finding text) live on it.
- `plumb-cli`: the registry side is already trait-based (`Registry`,
  `PyPiRegistry`, `NpmRegistry`, `RegistryRouter`). The router remains a
  `match`: two arms today, one place, and tabling it would be
  generalisation with no gain.
- Adding an ecosystem = one spec file (core) + one registry file (cli) +
  an enum variant + a router arm.

## Section 3 — What does not change

Observable behaviour is bit-identical: same findings, same exit codes, same
generated files, same cache format, same CLI surface. `check.rs`,
`finding.rs`, `adapters.rs`, `layer1.rs` and `report.rs` are untouched.
This is a seam refactoring, not a feature — if any existing test needs its
assertion changed, the refactoring has a bug.

## Section 4 — Testing

- The existing test suite is the proof: all tests pass unchanged, no
  assertion edited. Per-language tests move with their code.
- New structural tests (TDD as usual): no extension claimed by two
  languages; every language declares at least one extension;
  `for_extension` answers for every extension `all()` declares.

## Section 5 — Rollout

| PR | Content | Proof |
|---|---|---|
| A | Language seam (`languages/` + table + delegation) | suite unchanged + new structural tests |
| B | Ecosystem seam (`EcosystemSpec` + table in manifest) | same |
| C | NuGet on the new seam — first real consumer | the seam is validated here; friction in C means a defect in A/B |

A and B are small and separately revertable. C follows the CONTRIBUTING
recipe and updates it where the seam changes it. Dart/pub and C#/Dart
scanning follow C from the same mould, against the real .NET + Flutter
repository.
