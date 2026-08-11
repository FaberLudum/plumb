# C# and Dart Scanning Implementation Plan (PR E)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** The boundary gate and the index cover C# and Dart: two new `Language` impls, plus the one resolution change C# namespaces force (module segments split on dots).

**Architecture:** Two files on the `Language` seam (PR #21): `languages/csharp.rs`, `languages/dart.rs`, one `ALL` line each. Extractors stay line-based heuristics (no parser deps — Tier B gate, documented trade). One surgical change in `boundaries.rs::target_of`: module path segments split on `.` as well as `/`, or `## src/Ummarell.Api` can never match `using Ummarell.Api.Services;`.

**Tech Stack:** Rust 1.97, no new dependencies.

## Global Constraints

- No existing test assertion changes except where a task explicitly mandates an extension. SPDX headers. No `unwrap()`/`expect()` outside tests. English. Conventional Commits, red-test commit before green, never amend, trailer: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- Gates (bash): `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo run -q -p plumb-cli --locked -- check --diff --offline`. index-stale → regenerate + chore commit.
- Umarell repos read-only (copy file contents for fixtures, never write there).
- Branch: `feat/csharp-dart-scanning`.

---

### Task 1: C# — `languages/csharp.rs` + the dot-segment resolution change

**Files:**
- Create: `crates/plumb-core/src/languages/csharp.rs`
- Modify: `crates/plumb-core/src/languages/mod.rs` (`mod csharp;`, `ALL` grows to 4)
- Modify: `crates/plumb-core/src/boundaries.rs` (`target_of` module segments split on `['/', '.']`)

**Interfaces:**
- Consumes: `trait Language`, table, `Symbol` — mould: `languages/python.rs` and `rust.rs`.
- Produces: `pub struct CSharp;`, `extensions() == &["cs"]`.

**Imports rule** (line-based, full path kept, mirroring the JS/Rust doc honesty):
- `using X.Y.Z;` → `X.Y.Z`; `global using X.Y;` → `X.Y`; `using static X.Y.Z;` → `X.Y.Z` (the static member's TYPE path); `using Alias = X.Y.Z;` → `X.Y.Z` (right-hand side).
- `using (var f = ...)` and `using var f = ...;` are the STATEMENT form, not imports — excluded by requiring the token after `using`(+modifiers) to be a plain identifier path ending `;` with no `(`/`=` except the alias form. Get the discrimination right; there are tests for both statement forms.
- `namespace A.B;`/`namespace A.B {` lines are not imports.

**Symbols rule** (public surface, any indentation — C# nests in namespaces/classes):
- Lines whose modifiers include `public` and declare `class`/`interface`/`struct`/`record`/`enum`/`delegate` → kind = that keyword, name = identifier after it (strip generic `<...>`). `record struct`/`record class` → kind `record`.
- Methods/properties deliberately NOT extracted (noise; the index maps types, same altitude as Python's classes). Say so in the module doc.

**The `target_of` change** (the load-bearing piece): module path segments currently split on `/` only, so heading `## src/Ummarell.Api` yields segment `ummarell.api` which can never equal import segment `ummarell`. Change: split module paths on `['/', '.']` (then `-`/`_` normalise as today). Red tests FIRST in boundaries.rs:

```rust
a_dotted_module_heading_matches_a_csharp_namespace_import   // module "src/Ummarell.Api", import "Ummarell.Api.Services" → resolves, score 2
a_dotted_module_heading_still_matches_by_plain_path         // module "src/Ummarell.Api" still wins for import "Ummarell.Api" over module "src"
existing_resolution_is_unchanged_for_undotted_modules       // pin one existing case verbatim (copy an existing test's fixture) — proves no regression class
```

Also verify: no existing boundaries test breaks (a directory literally named like `v1.2` in a heading would now split into two segments — if an existing test covers that shape, flag to controller instead of forcing; none is believed to exist).

**Extractor tests** (red first, one behaviour each):

```rust
using_directives_yield_the_full_namespace_path      // using Ummarell.Api.Services; + global using X.Y; → both
using_static_and_alias_yield_the_type_path
a_using_statement_is_not_an_import                  // using (var f = File.Open(...)) / using var f = ...; → nothing
public_type_declarations_are_extracted              // public class Foo, public sealed record Bar<T>, public enum Baz → kinds class/record/enum, names Foo/Bar/Baz
non_public_types_are_skipped                        // internal class X, class Y → nothing
```

Commits: `test(languages): pin C# import extraction and dotted-module resolution` → `feat(languages): C# behind the Language trait — and dotted module segments`.

### Task 2: Dart — `languages/dart.rs`

**Files:**
- Create: `crates/plumb-core/src/languages/dart.rs`
- Modify: `crates/plumb-core/src/languages/mod.rs` (`ALL` grows to 5)

**Imports rule:**
- `import 'package:umarell/features/chat/chat_service.dart';` → `umarell/features/chat/chat_service` (strip `package:`, strip `.dart`). Same for `export`. Double or single quotes.
- `import 'dart:async';` → skipped (SDK builtin, the JS `node:` precedent). Relative imports (`'chat_service.dart'`, `'../x.dart'`) → skipped, the JS relative-spec precedent — same trade, documented the same way.
- `part`/`part of` → skipped (not dependencies, file inclusion).

**Symbols rule** (top level, column 0):
- `class X` / `abstract class X` / `enum X` / `mixin X` / `extension X on ...` / `typedef X = ...` → kinds class/enum/mixin/extension/typedef.
- Top-level functions: a column-0 line shaped `Type name(` or `void name(` → kind fn. `void main(` included.
- Names starting `_` skipped everywhere — Dart's underscore IS the privacy rule, objective and documented.

**Tests** (red first): `package_imports_yield_the_stripped_path`, `sdk_and_relative_imports_are_skipped`, `part_directives_are_not_imports`, `top_level_declarations_are_extracted`, `underscore_private_names_are_skipped`.

Commits: `test(languages): pin Dart extraction` → `feat(languages): Dart behind the Language trait`.

### Task 3: docs, dogfood, real-repo smoke

**Files:**
- Modify: `docs/gates.md` (languages list + C#/Dart heuristic notes: C# namespaces map to paths by convention not contract — a repo whose namespaces diverge from folders gets missed edges, said honestly; Dart relative imports invisible to boundaries)
- Modify: `CHANGELOG.md`
- Regenerate: index/adapters as demanded

Steps: docs commit; full gates + dogfood; then smoke — scratch temp repo with verbatim copies of 2-3 real files from each Umarell repo (e.g. a controller .cs with usings, a Flutter feature .dart with package: imports), a minimal `.plumb/architecture.md` declaring 2 modules, run `plumb index` (symbols appear, kinds right) and `plumb check --diff` after adding a cross-boundary import (boundary warning fires with resolved modules). Transcript in report. Commit docs as `docs: C# and Dart in the scanning surface`.

---

## Self-review notes

- The dot-split is scoped to `target_of` resolution only — `module_of` (path-prefix attribution) untouched; stated in Task 1.
- Method-level C# symbols and Dart relative imports are declared trades, documented in gates.md, consistent with the heuristic-not-parser stance the boundary gate's Tier B rests on.
- Both extractors follow the underscore/visibility rule that is objective in their language (C#: `public` keyword; Dart: leading `_`), no style judgement.
