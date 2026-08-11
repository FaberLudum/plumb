# Dart Boundary Resolution Implementation Plan (PR F — closes #28)

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Dart boundary checking stops being inert on the standard Flutter layout: relative imports resolve against the importing file, `package:` imports resolve against `lib/`-rooted module headings. The 4 real `shared → features` crossings measured at Umarell's HEAD must surface.

**Architecture:** Two changes. (1) `Language::imports` gains the importing file's path — `fn imports(&self, path: &str, text: &str)`; four languages ignore it, Dart uses it to join relative specifiers. `boundaries::imports(path, text)`'s public signature is unchanged (it already has the path; it just forwards it). (2) The Dart extractor drops a `package:` URI's first segment (the pub package name), so `package:umarell/features/chat/x.dart` becomes `features/chat/x`, which `target_of`'s existing trailing-module/leading-import match resolves against `## lib/features/chat` at k=2. No pubspec read, core stays pure.

**Accepted trade (document, Tier B):** a foreign package's inner path (`package:flutter/material` → `material`) can now collide with a like-named declared module — the same name-collision class every language's resolution already carries (a Python module named `requests` collides with the pypi package). Stated in gates.md.

**Tech Stack:** Rust 1.97, no new dependencies.

## Global Constraints

- No existing test assertion changes except assertions this plan explicitly extends. SPDX. No `unwrap()`/`expect()` outside tests. English. Conventional Commits, red before green, never amend, trailer: `Co-Authored-By: Claude Opus 5 <noreply@anthropic.com>`.
- Gates (bash): `export PATH="/c/Users/Andre/.cargo/bin:$PATH" && cargo fmt --all && cargo clippy --workspace --all-targets --locked -- -D warnings && cargo test --workspace --locked && cargo run -q -p plumb-cli --locked -- check --diff --offline`; index-stale → regenerate + chore commit.
- Umarell repos read-only.
- Branch: `feat/dart-boundary-resolution`.

---

### Task 1: the trait gains the path

**Files:**
- Modify: `crates/plumb-core/src/languages/mod.rs` (trait + doc), `rust.rs`, `python.rs`, `javascript.rs`, `csharp.rs` (`_path` ignored), `dart.rs` (signature only this task), `crates/plumb-core/src/boundaries.rs` (dispatcher forwards path)

**Interfaces:**
- Produces: `fn imports(&self, path: &str, text: &str) -> Vec<String>` on `trait Language`. Only the trait and its impls change; `boundaries::imports`'s public signature is already `(path, text)` and merely forwards.

- [ ] Mechanical refactor, single commit (no behaviour change — the suite is the proof; any test calling `language.imports(text)` directly gets the path argument added with a dummy like `"x.rs"`, assertion untouched): `refactor(languages): imports() receives the importing file's path`. Full gates.

### Task 2: Dart resolution

**Files:**
- Modify: `crates/plumb-core/src/languages/dart.rs`

**Rules:**
- `package:` URI: strip `package:`, strip `.dart`, drop the FIRST segment → `umarell/features/chat/x.dart` → `features/chat/x`. (`package:flutter/material.dart` → `material` — the documented collision trade.)
- Relative specifier (`'chat_state.dart'`, `'../models/enum_mappers.dart'`, `'./x.dart'`): join against the importing file's directory, normalise `.`/`..` segments lexically (no fs), strip `.dart` → a repo-rooted path like `lib/features/chat/chat_state`. A `..` that escapes the path root yields nothing (skip, as before).
- `dart:` builtins and `part`/`part of` stay skipped. `as`/`show`/`hide`/`deferred` handling unchanged.

**Red tests first** (assertions on the new forms; the two existing import tests' assertions ARE extended by this plan — mandated exception, name it in the commit body):

```rust
package_imports_drop_the_package_name_segment   // package:umarell/features/chat/x.dart from lib/a.dart → "features/chat/x"
relative_imports_resolve_against_the_importer   // 'chat_state.dart' + '../shared/models/m.dart' from lib/features/chat/a.dart → "lib/features/chat/chat_state", "lib/shared/models/m"
a_relative_import_escaping_the_root_is_skipped  // '../../../x.dart' from lib/a.dart → nothing
sdk_imports_are_still_skipped                   // dart:async → nothing (pin unchanged behaviour)
```

Plus an end-to-end boundary pin in `boundaries.rs` tests: architecture `## lib/features` / `## lib/shared` (shared May depend on: (none)); file `lib/shared/models/enum_mappers.dart` importing `package:umarell/features/chat/chat_models.dart` → violation `(lib/shared → lib/features)` — this is literally Umarell's real defect shape. Commits: `test(languages): pin Dart import resolution against the Flutter layout` → `feat(languages): Dart boundary resolution — closes the #28 inertness`.

### Task 3: docs, re-measure, close the loop

**Files:**
- Modify: `docs/gates.md` (rewrite the Dart-inertness paragraph: now live; state the first-segment drop, the collision trade, the lexical `..` rule; relative imports no longer invisible), `CHANGELOG.md`
- Regenerate: index/adapters if demanded

Steps: docs commit; full gates; then re-measure on a fresh clone of C:\Lavoro\Umarell\umarell-flutterapp (clone to scratch, Umarell read-only): `.plumb/architecture.md` declaring `## lib/features`, `## lib/shared` (shared depends on none), `## lib/app` etc. per the real layout; whole-tree sweep — **expected: the 4 known `shared → features` crossings surface, and nothing else false-fires**; paste the findings in the report. Reference the measurement in the PR body and close #28 with it (`gh issue close 28 --comment ...` after merge — controller does the close). Commit: `docs: Dart boundary resolution is live`.

---

## Self-review notes

- Trait change is additive-mechanical (Task 1 alone is behaviour-neutral, provable by untouched suite); all Dart behaviour change is Task 2 with red-first tests.
- The mandated assertion extensions are exactly two existing dart.rs import tests (they assert the old unstripped forms) plus none elsewhere — named so the reviewer can verify nothing else moved.
- Core purity kept: no pubspec read; the package-name segment is dropped, not resolved.
