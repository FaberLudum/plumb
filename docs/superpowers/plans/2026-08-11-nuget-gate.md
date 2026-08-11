# NuGet Gate Implementation Plan (PR C)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The `package-exists` gate covers NuGet: every package a change adds to a `.csproj` or `Directory.Packages.props` must exist on nuget.org.

**Architecture:** First real consumer of the `EcosystemSpec` seam (PR #22): one spec file `ecosystems/nuget.rs` + one table line in plumb-core; one `NuGetRegistry` + one router arm in plumb-cli. XML is parsed with `quick-xml` (new dependency, justified — a line heuristic false-blocks on commented-out XML). Spec: `docs/superpowers/specs/2026-08-11-nuget-pub-ecosystems-design.md`.

**Tech Stack:** Rust 1.97 edition 2024; `quick-xml` 0.41 (verify exact current version with `cargo add quick-xml --dry-run` at implementation time — do not recall).

## Global Constraints

- Tests never touch the network: registry tests inject stub `Registry` impls or run `--offline` against caches the test seeds. CI's no-route-out job must stay green.
- Exit-code contract untouched: `0` clean, `1` Tier A, `2` warnings only.
- Unreachable registry = `Existence::Unknown`, never a violation. Illegal ID = `Missing` without a request. Cache outside the repository, 30-day entries.
- Every Rust file opens with `// SPDX-License-Identifier: Apache-2.0` then `// Copyright 2026 The Plumb Authors`. No `unwrap()`/`expect()` outside tests. English.
- Conventional Commits, one commit per step (red test commit before green implementation commit), never amend, every message ends with the trailer `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- Gates before any completion claim (bash): `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo run -q -p plumb-cli --locked -- check --diff --offline`.
- The two Umarell repositories are read-only reference material; nothing is written there.
- Branch: `feat/nuget-gate`.

---

### Task 1: `Ecosystem::NuGet`, `quick-xml`, and the manifest spec

**Files:**
- Create: `crates/plumb-core/src/ecosystems/nuget.rs`
- Modify: `crates/plumb-core/src/ecosystems/mod.rs` (`mod nuget;`, `ALL` grows to 3)
- Modify: `crates/plumb-core/src/manifest.rs` (add `Ecosystem::NuGet`; add `Gap::PackagesConfig` and `Gap::MsBuildExpression(String)` with `describe` arms)
- Modify: `crates/plumb-core/Cargo.toml` + `Cargo.lock` (add `quick-xml`)
- Modify: `docs/dependencies.md` (row for `quick-xml`)

**Interfaces:**
- Consumes: `trait EcosystemSpec { fn ecosystem(&self) -> Ecosystem; fn matches(&self, filename: &str) -> bool; fn parse(&self, filename: &str, text: &str) -> Manifest; }` and the `ALL`/`specs()`/`for_path()` table in `ecosystems/mod.rs` — read `ecosystems/npm.rs` and `ecosystems/pypi.rs` for the established mould, including how tests are laid out.
- Produces: `pub struct NuGet;` registered as `&nuget::NuGet` in `ALL`. `matches(filename)`: true for a filename ending `.csproj`, `.fsproj` or `.vbproj` (ASCII case-insensitive, because Windows filesystems are — F# and VB project files carry the identical PackageReference format), for exactly `Directory.Packages.props` (case-insensitive), and for exactly `packages.config` (case-insensitive; recognised only to Gap). `parse` behaviour below. Also produces `Ecosystem::NuGet` for Task 2's registry.

- [ ] **Step 1: dependency row and crate.** Run `cargo add quick-xml -p plumb-core` (records the current version in Cargo.toml; `--locked` builds keep it). Add the row to `docs/dependencies.md` in the file's existing table style: name, version, and why the standard library is not enough — it has no XML parser, and a blocking gate cannot use a line heuristic because XML comments would manufacture blocking findings on text nobody declared. Commit: `chore(deps): quick-xml for csproj parsing, with its justification row`.

- [ ] **Step 2: red tests for the manifest spec.** In `ecosystems/nuget.rs` (skeleton: SPDX headers, `use super::EcosystemSpec;`, `pub struct NuGet;`, empty impl to be filled — the tests are the contract). Write these tests, mirroring the npm/pypi test style; each names its behaviour:

```rust
// matches
csproj_is_recognised_case_insensitively        // "Api.csproj", "API.CSPROJ" true; "csproj", "x.csproj.bak" false
fsproj_and_vbproj_are_recognised_too           // "App.fsproj", "Lib.vbproj" true — same PackageReference format
directory_packages_props_is_recognised          // "Directory.Packages.props" true, case-insensitive; "Packages.props" false
packages_config_is_recognised                   // "packages.config" true (Gap-only parse)
// parse: .csproj
package_references_yield_their_include_names    // two self-closing PackageReference → both names
element_form_package_reference_is_read          // <PackageReference Include="X" Version="..."><PrivateAssets>all</PrivateAssets></PackageReference> → "X"
project_reference_is_not_a_package              // ProjectReference Include="..\..\x.csproj" → no name, no gap
a_commented_out_reference_is_invisible          // <!-- <PackageReference Include="Fake" /> --> → no name, no gap
a_bom_does_not_break_parsing                    // "\u{feff}<Project ..." → names still read (the driving repo's test csproj carries one)
an_msbuild_expression_is_a_gap_not_a_name       // Include="$(SomeProp)" → Gap::MsBuildExpression("$(SomeProp)"), no lookup name
malformed_xml_is_unparseable                    // "<Project><ItemGroup>" truncated → Gap::Unparseable, no names
an_empty_project_yields_an_empty_manifest       // "<Project></Project>" → no names, no gaps
// parse: Directory.Packages.props
package_version_entries_yield_names             // <PackageVersion Include="X" Version="1.0" /> → "X"
global_package_reference_yields_names           // <GlobalPackageReference Include="Y" /> → "Y"
// parse: packages.config
packages_config_is_a_gap_never_names            // any content → Gap::PackagesConfig, no names, even if well-formed with <package id="Z"/>
// ecosystem stamp
parse_stamps_nuget                              // every parse above → manifest.ecosystem == Ecosystem::NuGet
```

Run: `cargo test -p plumb-core --locked nuget` — expected: compile failure (missing variants/impl), the red state. Commit: `test(ecosystems): pin the NuGet manifest reader's contract`.

- [ ] **Step 3: green.** Add `Ecosystem::NuGet` variant; add `Gap::PackagesConfig` (doc: legacy format, recognised by filename, deliberately never parsed — its own gap so the message can name the migration) and `Gap::MsBuildExpression(String)` (doc: the `Include` is positive evidence of a declaration; the expression is why no name could be resolved) with `describe` arms in manifest.rs following the existing sentence style. Implement `NuGet::parse` with `quick-xml`'s pull reader (`quick_xml::Reader`, `read_event`): on `Start`/`Empty` events named `PackageReference`, `PackageVersion`, `GlobalPackageReference`, read the `Include` attribute; `$(`-containing includes → `Gap::MsBuildExpression`; XML parse error at any point → the names read so far are discarded and the manifest is `Gap::Unparseable` alone (consistency: a file that cannot be fully read must not present as partially clean); dispatch by filename inside `parse` (csproj vs props both read the same three elements — one code path; `packages.config` → Gap only). Names collect into the `BTreeSet` the `Manifest` type carries. Structural tests in `ecosystems/mod.rs` need their probe list extended with `"Api.csproj"`, `"Directory.Packages.props"`, `"packages.config"` (the existing no-double-match and ecosystem-stamp tests then cover NuGet automatically). Run the full gates. Commit: `feat(ecosystems): NuGet manifest reader — csproj and Directory.Packages.props`.

### Task 2: `NuGetRegistry` and the router arm

**Files:**
- Modify: `crates/plumb-cli/src/registry.rs` (add `NuGetRegistry`, router arm; extend cache-key handling with the `nuget:` prefix)
- Test: in-file `#[cfg(test)]` additions, mirroring the npm registry tests

**Interfaces:**
- Consumes: `trait Registry { fn exists(&self, name: &str, ecosystem: Ecosystem) -> Existence; }`, `Existence::{Exists, Missing, Unknown}`, `RegistryRouter`, the cache layer — read `NpmRegistry` end to end first; it is the mould, including `exists_via`, freshness, and the validity guard.
- Produces: `pub struct NuGetRegistry` with `new` + `exists` + `stale_packages` following the npm shapes; `RegistryRouter::exists` gains `Ecosystem::NuGet => self.nuget.exists(name)`.

- [ ] **Step 1: red tests.** Mirror the npm registry test set, adjusted to NuGet semantics:

```rust
an_illegal_nuget_id_is_missing_without_a_request   // "bad name" (space), "x?y", "" → Missing; assert no request recorded by the stub transport
nuget_ids_are_case_insensitive_in_the_cache        // seed cache for "newtonsoft.json", ask "Newtonsoft.Json" → answered from cache, no request
the_lookup_url_lowercases_the_id                   // stub transport captures the URL for "OpenAI" → ".../v3-flat2/openai/index.json"
an_unreachable_registry_is_unknown                 // transport error → Unknown, cached nothing
a_404_is_missing_and_a_200_is_exists               // per the flat-container contract
cache_keys_carry_the_nuget_prefix                  // written entry key starts "nuget:"
```

Valid-ID rule to implement: non-empty, ASCII `[A-Za-z0-9._-]` only. Run the nuget-filtered tests, watch them fail to compile. Commit: `test(registry): pin the NuGet registry client's contract`.

- [ ] **Step 2: green.** Implement against `https://api.nuget.org/v3-flat2/{id_lowercased}/index.json`, cache key `format!("nuget:{}", id.to_ascii_lowercase())`. Follow npm's structure for freshness/`exists_via`/transport injection exactly — the reviewers will diff you against it. Full gates. Commit: `feat(registry): the package gate covers nuget.org`.

### Task 3: end-to-end, docs, honesty notes, dogfood

**Files:**
- Modify: `crates/plumb-cli/tests/cli.rs` (e2e additions)
- Modify: `docs/gates.md` (Blind spots for NuGet)
- Modify: `docs/research/taxonomy.md` (§4.1 Limits honesty note)
- Modify: `CHANGELOG.md` (Unreleased or 0.2.0 section per its own conventions)
- Regenerate: `.plumb/index.md` + instruction files if check demands

**Interfaces:**
- Consumes: everything above; the e2e harness in `cli.rs` (temp git repos + seeded caches — copy the npm e2e shapes).

- [ ] **Step 1: e2e red tests** in `cli.rs`, mirroring the npm e2e set: a temp repo whose diff adds `<PackageReference Include="surely-not-a-real-package-xyz" ...>` to a `.csproj`, cache seeded `Missing` → `check --diff --offline` exits 1 naming the package, the file, `nuget` and `evidence: taxonomy §4.1`; a seeded-`Exists` case exits 0; a `Directory.Packages.props` addition behaves identically; a `packages.config` in the diff warns `manifest-understood` and exits 2. Watch them fail. Commit: `test(cli): pin the NuGet gate end to end`.
- [ ] **Step 2: green** — expected to be wiring-only or nothing (check.rs is table-driven); fix whatever the e2e exposes. Full gates. Commit: `feat(check): the diff gate blocks hallucinated NuGet packages` (or `fix:` if it was wiring).
- [ ] **Step 3: docs.** `docs/gates.md` blind spots, NuGet subsection, honest and specific: `Version` ranges unchecked (existence only); `$(MSBuild)` expressions are gaps; `<Import>` indirection unread; private NuGet feeds false-positive exactly like private PyPI/npm (name the limitation); `packages.config` recognised but never parsed; lockfiles (`packages.lock.json`) invisible. Taxonomy §4.1 Limits row: append that the NuGet extension rests on the defect class plus objective detection, hallucination rates unmeasured in that ecosystem (spec's honesty edit). CHANGELOG entry. Commit: `docs: NuGet blind spots, evidence note and changelog`.
- [ ] **Step 4: dogfood + regenerate.** Full gates; `plumb check --diff --offline`; if `index-stale` fires (new public symbols), regenerate index + adapters, re-check to `no findings`, commit `chore: regenerate index and instruction files for the NuGet reader`. Then a scratch-repo smoke test against the REAL shapes: copy the two Umarell `.csproj` files' contents into a scratch temp git repo (the repos themselves are read-only — copy, never link), run the binary online (`check --diff`, no `--offline`) after adding a garbage package on top, and paste the real blocking output into the task report. This is the same live-proof standard the npm PR set (its README section shows the transcript).

---

## Self-review notes

- Spec coverage: manifests read (§NuGet ¶1) → Task 1; gaps (¶2) → Task 1 tests + Gap variants; parser choice (¶3) → Task 1 Step 1; registry (¶4) → Task 2; inherited seam rules → Task 3 e2e + docs; replay validation is a separate follow-up pass after PR D, per spec.
- No placeholders: every test named with its input and expected outcome; registry URL, charset, cache-key format all exact.
- Type consistency: `Gap::MsBuildExpression(String)` carries the raw include text; `matches` sees bare filenames only (seam contract); `parse` receives the bare filename and dispatches csproj/props/config internally.
