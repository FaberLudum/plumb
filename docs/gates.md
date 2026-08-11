# The gates in detail

Reference for what each check actually reads, what it rules out, and every
known gap. The short version is in the [README](../README.md); this file is
the long one, and the Blind spots section is the part worth reading before
trusting a clean run.

## The package gate

One rule blocks: every package a change adds must actually exist on its
registry. `plumb check --diff` asks git for the paths a change touched and
reads each manifest among them, as it stands and as it stands at HEAD. On
the PyPI side that means `requirements.txt`, `requirements-*.txt` and
`pyproject.toml`; on the npm side, `package.json`; on the NuGet side,
`.csproj`/`.fsproj`/`.vbproj` and `Directory.Packages.props`; on the pub.dev
side, `pubspec.yaml`. It looks up only the names the change adds, and blocks
on a name its own registry does not have.

```console
$ plumb check --diff
blocking: package `reqeusts` does not exist on PyPI [package-exists]
  requirements.txt
  evidence: taxonomy §4.1
$ echo $?
1
```

npm reads the same way, against the four dependency tables in
`package.json` (`dependencies`, `devDependencies`, `peerDependencies`,
`optionalDependencies`), and blocks on the same shape of finding with `npm`
in place of `PyPI`:

```console
$ plumb check --diff
blocking: package `plumb-nonexistent-calibration-xyz` does not exist on npm [package-exists]
  package.json
  evidence: taxonomy §4.1
$ echo $?
1
```

NuGet reads the same way again, against a `PackageReference`, `PackageVersion`
or `GlobalPackageReference` element's `Include` attribute in a `.csproj`,
`.fsproj`, `.vbproj` or `Directory.Packages.props`, and blocks on the same
shape of finding with `NuGet` in place of `PyPI`:

```console
$ plumb check --diff
blocking: package `surely-not-a-real-package-xyz` does not exist on NuGet [package-exists]
  Api.csproj
  evidence: taxonomy §4.1
$ echo $?
1
```

pub.dev reads `pubspec.yaml`'s `dependencies`, `dev_dependencies` and
`dependency_overrides`, and blocks with `pub.dev` in place of `PyPI`:

```console
$ plumb check --diff
blocking: package `surely_not_a_real_package_xyz` does not exist on pub.dev [package-exists]
  pubspec.yaml
  evidence: taxonomy §4.1
$ echo $?
1
```

Only registry-hosted entries are looked up. A dependency declared with
`sdk:`, `git:` or `path:` is not a pub.dev package at all — `flutter` ships
with the SDK and has no pub.dev page — so it is dropped before any lookup
happens rather than checked and forgiven. Package names are matched
case-sensitively, because pub.dev treats them that way: a handful of
packages published before pub required lowercase names, `Autolinker` among
them, resolve only under their original spelling.

A name already in the manifest is not re-checked and does not appear in
that output at all. Only a name the change actually adds is looked up, on
any of the four registries.

`--json` emits the same findings as an array, and `--offline` never
contacts a registry. Exit codes are a fixed contract: `0` clean, `1`
blocking, `2` warnings only. A run that could not happen at all (not a
repository, no git) exits outside that contract instead, so a broken gate
can never be read as a clean one.

What the rule rests on: Spracklen et al. (USENIX Security 2025) measured
19.7% of packages referenced by code-generating models as non-existent,
across 2.23 million samples. Churilov (2026) re-measured 4.62 to 6.10% on
frontier models. That is a three- to fourfold drop, and still not zero.
43% of hallucinated names recur identically across runs, which is what
makes slopsquatting a practical attack rather than a theoretical one.

## The Tier B gates

Three warning gates run alongside the manifest check.

**Boundary.** Imports are validated against the module boundaries declared
in `.plumb/architecture.md`. Each changed file is attributed to a module by
path, and its imports are extracted with a heuristic, line-based matcher
covering five languages: Rust, Python, the JavaScript/TypeScript family,
C# (`using` directives) and Dart (`package:` and relative imports and
exports). An
import reaching a module outside the file's declared
`May depend on:` list warns rather than blocks. The comparison is
before-versus-after, the same shape as the manifest check's: **the gate
reports only violations the change introduces.** A violation already
present at HEAD is legacy, not something the change did, so a comment-only
edit to a file that has always had an undeclared import does not re-warn
on it.

```console
warning: crates/plumb-core/src/lib.rs imports `plumb_cli`, but crates/plumb-core may not depend on crates/plumb-cli [boundary]
  crates/plumb-core/src/lib.rs
  evidence: taxonomy §4.5
```

**A file under no declared module is not checked, and the skip is
silent.** Attribution is by path prefix against the headings in
`.plumb/architecture.md`; a source file no heading covers is skipped
before a single import is extracted — deliberately, because flagging
every unattributed file would make the gate unadoptable on any repository
that declares its modules incrementally. The cost is that the silence is
indistinguishable from a clean check. It is not hypothetical: on the
Flutter repository used for calibration, 59 of 138 swept Dart files —
`test/` and `integration_test/`, carrying 180 `package:` imports between
them — sat under no declared module and could never have produced a
finding. If you want a directory checked, declare it: a heading like
`## test` with `May depend on: <your app module>` brings its files into
the gate with one line. What the architecture file does not name, Plumb
does not police.

**Adapter drift.** Warns when the generated instruction files no longer
match `.plumb/`. The same four files `plumb adapters` writes are compared
against what re-rendering `.plumb/` right now would produce, byte for byte
apart from line endings:

```console
warning: CLAUDE.md does not match .plumb/; run `plumb adapters` [adapter-drift]
  CLAUDE.md
  evidence: taxonomy §11
```

**Index staleness.** Warns when `.plumb/index.md` no longer matches the
tracked source files it claims to describe. A fresh scan of every tracked
source is compared against what is committed, on every invocation rather
than only over changed paths:

```console
warning: .plumb/index.md does not match the tracked sources; run `plumb index` [index-stale]
  .plumb/index.md
  evidence: taxonomy §11
```

## plumb report

`plumb report --commits <n>` (default 30, first-parent from HEAD) prints
three quantities per commit: **volume** (lines added and removed, from
`git show --numstat`), **deletion ratio** (removed over added), and
**coupling delta** (the change in cross-module import edges the commit's
touched files resolve to, read against `.plumb/architecture.md`). A commit
where volume and coupling delta both rise is flagged with a **decay
signature**, the pattern taxonomy §4.5 names straight off: volume and
coupling growing together.

```console
$ plumb report --commits 20
commits: 20, added: 20896, removed: 462
mean deletion ratio: 0.06
decay signature: 4 of 20 (c8e497d, 1ed73ca, d4e8fc0, 46b0918)
```

The signature is uncalibrated. `boundaries::edges` counts a permitted edge
exactly like a violation, and counts it per file, so any commit adding a
file that uses a declared dependency scores coupling +1 by construction.
That is ordinary growth against a boundary the project itself declared, not
the §4.5 decay pattern the signature is named for. It needs calibration
against real history before it can tell the two apart.

Coupling reads at module granularity, and it resolves sub-package
boundaries once they are declared. The resolver matches an import against a
module's *sub-package* headings, not only its top-level one, so a
repository whose real boundaries sit between sub-packages of one top-level
package resolves those boundaries after `.plumb/architecture.md` declares
the sub-packages as modules in their own right. An architecture that still
declares only the top-level package sees an inert `0`, because there is
nothing deeper to resolve against.

**It is not a gate.** No finding, no threshold, no exit-code effect.
`plumb report` always exits `0` regardless of what the numbers say. It is
Tier C instrumentation for an open hypothesis, not a calibrated rule.

## What `--diff` compares, and what that rules out

**`--diff` compares the working tree against HEAD.** It runs
`git diff --name-only HEAD`, and a path is listed only when the working tree
differs. Anything already committed is outside that comparison: there is
nothing left to compare, so the run checks nothing and exits `0`. The same
manifest blocks while it is uncommitted and passes the moment it is committed.

The consequence is worth stating plainly, because it is the opposite of what a
gate is usually assumed to do. **On a fresh clone or a CI checkout the tree is
clean, so the manifest and boundary checks examine nothing and exit `0`
whatever the commit under test added.** Wiring this into a CI job that clones
a branch gives those two checks a permanently green result that is not
checking anything. Checking a commit or a commit range needs
`plumb check --repo`, which the design spec reserves and **which is not
implemented yet**. Until it is, the useful place for the manifest and
boundary checks is a developer's working copy.

The adapter-drift and index-stale checks are the exception to that promise,
by design: neither reads `git diff` at all, and both run once per invocation
over the repository's current state rather than over changed paths. So a
fresh clone of a repository whose committed generated files or index have
already drifted still exits `2` on the very first, otherwise-untouched run.

The pre-commit story is not honest yet either, for a reason worth stating
precisely: **`git commit` commits the index, and this gate never reads the
index.** Two consequences follow.

- An addition that is staged and then reverted on disk is **not listed at
  all**, so the manifest is never opened.
  `git diff --cached --name-only HEAD` prints the path;
  `git diff --name-only HEAD` prints nothing.
- A manifest staged one way and edited another **is** listed, and the
  after-side is read from the working tree. The gate checks content that is
  not about to be committed, and misses the content that is.

Either way the commit can land a package the gate never looked up. Staging
does matter, but only in one narrower way than it sounds: it makes an
untracked file *tracked*, and an untracked file is not reported at all.

Fixing this means reading the after-side as the union of the index and the
working tree, which changes what "added" means. `git show :<path>` alone is
not the fix: an unstaged edit has an index copy identical to HEAD, so it
would add nothing and report nothing.

## Blind spots

What it does not do yet, listed because a gate that stays quiet about them is
worse than one that has none. The list is what has been found, not a proof
that nothing else is there.

- **Only PyPI, npm, NuGet and pub.dev are read**, added one at a time.
  Maven/Gradle (`pom.xml`, `build.gradle`), Go (`go.mod`) and crates.io
  (`Cargo.toml`) are the frontier now, and none of them is read: a
  dependency declared only in one of those is invisible to Plumb — never
  looked up, in either direction. Nothing is scheduled, so treat every
  ecosystem outside the four as unchecked rather than as coming soon.
- **pub.dev checks existence only, and only what `pubspec.yaml` states
  literally.** Concretely:
  - Version constraints are unchecked, the same trade npm's ranges and
    NuGet's `Version` get: the gate confirms a package name resolves, not
    that any particular version does, so `^999.0.0` on a real package
    passes exactly as cleanly as a constraint that actually resolves.
  - **A custom `hosted:` dependency is a `manifest-understood` gap, not a
    lookup** — the private-registry false-positive class again, handled the
    other way round. Where a private PyPI index or NuGet feed silently
    false-positives (Plumb asks the public registry and blocks on a name
    that exists only privately), a `pubspec.yaml` names its alternate host
    inline, so Plumb can see that it is being asked about a registry it
    does not know and warn instead of guessing. A `hosted:` that names
    pub.dev itself — including the retired `pub.dartlang.org` — is an
    ordinary lookup, not a gap.
  - **`pubspec.lock` is invisible.** The same blind spot npm's and NuGet's
    lockfiles have: a name that exists only inside the lockfile, never
    declared in `pubspec.yaml`, is never looked up in either direction.
  - **`pubspec_overrides.yaml` is not read.** It is recognised by neither
    filename nor content, so it produces no finding and no gap — a change
    that adds a nonexistent package there passes silently. This is a
    stronger gap than the lockfile one: an override is hand-written, so it
    is exactly the kind of file a hallucinated name reaches.
  - **Workspace and mono-repo layouts are not resolved.** A `resolution:`
    key is not read and pub workspaces are not followed. Each `pubspec.yaml`
    the diff touches is read on its own; a package resolved through a
    workspace root rather than declared in the file Plumb opened is not
    seen.
- **NuGet checks existence only, and only what a `.csproj`,
  `.fsproj`/`.vbproj` or `Directory.Packages.props` states literally.**
  Concretely:
  - `Version` ranges are unchecked, the same trade npm's version ranges get:
    the gate confirms a package id resolves, not that any particular
    version does, so `Version="999.0.0"` on a real package id passes exactly
    as cleanly as a version that actually shipped.
  - An `Include` that is an MSBuild property expression (`$(SomeProp)`) is a
    `manifest-understood` gap, not a lookup — the literal value is resolved
    by MSBuild at build time, which Plumb does not evaluate, so there is no
    name to check.
  - `<Import>` indirection is not followed. A `.csproj` that pulls in
    `PackageReference` elements from an imported `.props` or `.targets` file
    is read as if that file were not there; only the elements literally
    present in the file Plumb opened are seen.
  - **A private NuGet feed false-positives**, the same gap a private PyPI
    index or a private npm registry has (see below): a package id resolves
    against `https://api.nuget.org` alone, the public flat-container index.
    Plumb cannot tell "does not exist anywhere" from "does not exist on the
    one feed it asked."
  - `packages.config` is recognised by filename and deliberately never
    parsed, even when it is well-formed and holds what looks like an
    ordinary package declaration — always a `manifest-understood` warning
    naming the migration path (`PackageReference`), never a lookup. The
    tests pin this directly: a `packages.config` with a real-looking
    `<package id="…" version="…" />` still yields no name.
  - **Lockfiles are not read.** `packages.lock.json` is invisible to Plumb,
    the same blind spot npm's lockfiles have: a name that exists only inside
    it, never declared in a `PackageReference`, is never looked up in either
    direction.
- npm's coverage is one filename, `package.json`, at any depth, with any
  path segment named `node_modules` excluded entirely. A vendored
  `package.json` belongs to a dependency, not to this project, and checking
  it would block a build over a package nobody here added. npm nests
  dependencies, so `node_modules/foo/node_modules/bar/package.json` is
  excluded exactly like the top-level case. Four tables are read
  (`dependencies`, `devDependencies`, `peerDependencies`,
  `optionalDependencies`) and only their keys: a table's values are version
  *ranges*, and this gate checks existence, not whether a range is
  satisfiable, so `"left-pad": "^999.0.0"` passes exactly as cleanly as
  `"left-pad": "^1.0.0"` does. A table present but not an object
  (`"dependencies": []`) is a `manifest-understood` warning naming the
  table, the JSON sibling of the PEP 621 shape-mismatch warning below.
- **Lockfiles are not read.** `package-lock.json`, `yarn.lock` and
  `pnpm-lock.yaml` are invisible to Plumb. A name that exists only inside
  one of them, never declared in `package.json` itself, is never looked up
  in either direction: it does not block if the name is hallucinated, and
  it does not confirm if the name is real.
- **A private npm registry false-positives.** A name published only to a
  company's private registry or scope resolves against
  `https://registry.npmjs.org` alone, the public registry. This is the same
  gap a package that exists only on a private PyPI index has (see the third
  false positive below): Plumb cannot tell "does not exist anywhere" from
  "does not exist on the one registry it asked."
- On the Python side, three filenames are recognised, at any depth:
  `requirements.txt`, `requirements-*.txt` and `pyproject.toml`. From a
  `pyproject.toml` it reads PEP 621 `[project]` `dependencies` and
  `optional-dependencies`, nothing else. `requirements_dev.txt` and
  `requirements.dev.txt` are knowingly given up, because admitting `_` or
  `.` as the separator would also admit `requirements_notes.txt`, and prose
  read as a manifest manufactures blocking findings. Restricting it to `-`
  narrows that problem without solving it; see the first of the false
  positives below.
- Several declaration shapes inside a recognised manifest are still unread,
  but no longer silently. Each produces a Tier B `manifest-understood`
  warning naming the file and the shape that was missed, and the run exits
  `2`:
  - Poetry's tables, `[tool.poetry.dependencies]` and
    `[tool.poetry.group.<name>.dependencies]`.
  - PEP 735 `[dependency-groups]`.
  - An `-r`/`--requirement` include in a requirements file.
  - PEP 621 `dynamic` listing `dependencies` or `optional-dependencies`,
    the standard's own way of saying the dependencies are declared
    somewhere else.
  - Any key named `dependencies`, or ending in `-dependencies`, anywhere
    under `[tool]`, whose value is an array or a table. This is a rule
    about the shape of the key, not a list of tools, and it is what covers
    `[tool.pdm.dev-dependencies]`, `[tool.hatch.envs.<name>].dependencies`
    and `.extra-dependencies`, `[tool.uv].dev-dependencies` and
    `.extra-build-dependencies`, `[tool.pixi.pypi-dependencies]` and its
    conda tables, `[tool.setuptools.dynamic].dependencies` and the pre-1.2
    `[tool.poetry.dev-dependencies]`. None of those is named anywhere in
    the code.

    Four things keep it off ordinary configuration. A matched key is not
    descended into, so a package called `dependencies` inside a pixi or
    Poetry table is not read as a second declaration. Only an array or a
    table counts, which is what excludes hatch's boolean
    `require-runtime-dependencies`. Two exclusions are carried explicitly,
    both against a published schema:
    `[tool.ruff.analyze.include-dependencies]` maps file paths to file
    globs rather than to packages, and uv's `constraint-`,
    `build-constraint-`, `override-`, `exclude-` and rye's
    `excluded-dependencies` are documented as never installing anything, so
    a project using them is read in full and does not warn. And the conda
    namespaces are out of scope; see below.
  - A declaration whose value is not the shape the reader takes, such as
    `dependencies = "ruff"` instead of a list.

  On a Poetry manifest it prints `pyproject.toml was not fully read:
  dependencies are declared under [tool.poetry] — in
  [tool.poetry.dependencies] or a [tool.poetry.group.<name>.dependencies]
  table — which Plumb does not read`; on a PDM one, `pyproject.toml was not
  fully read: dependencies are declared in [tool.pdm.dev-dependencies],
  which Plumb does not read`. The dependencies in those shapes are still
  not looked up. The warning reports the gap; it does not close it.
- What is still silent, measured rather than assumed. Two kinds.

  **Files not recognised as manifests at all**, because recognition is by
  filename: `requirements.in` (the hand-edited pip-compile source, which is
  exactly where a hallucinated name is introduced, before it is ever
  compiled into a pinned `requirements.txt`), `requirements/base.txt`
  reached through an include (the include warns, but the file it names is
  never read on its own), `requirements_dev.txt` and
  `requirements.dev.txt`, `setup.py`, `setup.cfg` and `Pipfile`.

  **Declarations inside a recognised `pyproject.toml` that are not spelled
  `dependencies`.** The structural rule above keys on that word, so a tool
  that calls its dependency list something else is not covered. Measured,
  each exiting `0` with `plumb: no findings` over a package that does not
  exist: `[tool.tox.env_run_base].deps`,
  `[tool.briefcase.app.<name>].requires`,
  `[tool.cibuildwheel].test-requires`, `[tool.flit.metadata].dev-requires`,
  PEP 725 `[external].dependencies` (a top-level table, not under
  `[tool]`), and `[build-system].requires`. The last is deliberate: it
  appears in nearly every `pyproject.toml`, and a warning that always fires
  is one nobody reads.

  These are silent false negatives on a blocking gate, which is the failure
  this project exists to catch. The list is what was measured, not what was
  assumed, and it is not a proof that nothing else is missing: any tool
  that spells its dependency list some third way is silent until someone
  finds it.
- **Conda dependencies are out of scope, and are not checked at all.** This
  is a decision rather than a gap, and it is stated here because the
  difference is invisible from the outside: a pixi or conda-lock project
  gets a clean run, and a clean run must not be read as "these dependencies
  were checked".

  Plumb checks PyPI and npm, not conda. A hallucinated conda package is a
  real defect and Plumb does not look for it. Concretely, there is no
  warning and no lookup for `[tool.pixi.dependencies]`,
  `[tool.pixi.feature.<name>.dependencies]`, pixi's `build-`, `host-` and
  `run-dependencies` under `package`, its per-platform
  `[tool.pixi.target.<platform>.…]` variants, or
  `[tool.conda-lock.dependencies]`. The PyPI side of the same file *is*
  checked: `[tool.pixi.pypi-dependencies]` and its scoped forms warn like
  any other unread table.

  Excluded rather than warned about because this gap has no resolution
  path. Every other one ends when someone teaches Plumb the format; this
  one never would, since Plumb will not look a conda name up on PyPI. It
  would be a permanent warning that no action could clear, landing on
  projects that did nothing wrong. Checking conda needs a conda rule with
  its own evidence.
- A manifest that is not valid UTF-8 is not checked, and says so the same
  way: one Latin-1 byte costs the whole file, nothing in it is looked up,
  and the run warns (`requirements.txt was not fully read: not valid UTF-8
  at byte 5`) and exits `2`. Not a blocking `1`, because a file the tool
  could not decode is not evidence that a package does not exist.
- Three known false positives, each of which blocks a build that is fine.
  The worst is prose: a file named `requirements-and-design.txt` is read as
  a manifest, and the first word of every line becomes a name to look up.
  Four lines of English about a service and its latency budget block the
  build on `latency`. Second, a requirement given as a direct URL
  (`name @ https://…`) yields a name the index does not have. Third, moving
  a manifest without editing it re-checks every name in it, because nothing
  stands at its new path in HEAD, so a package from a private index blocks
  a rename that changed no dependency at all. Only the third is a trade
  taken on purpose: before it, a manifest moved into place carried its
  additions past the gate unlooked-at.
- `git diff HEAD` reports tracked changes only, so a manifest that has
  never been staged is invisible.
- The boundary gate's import extraction is line-based, one `text.lines()`
  pass per file. An import split across multiple lines (a parenthesised
  Python `from x import (a, b)`, a multi-line Rust `use` group) is missed
  entirely, and so is one produced by macro expansion: a `use` a Rust macro
  generates never exists as a literal source line for the extractor to
  match. It also does not know about strings or comments, so an
  import-shaped line inside either can false-positive. Module attribution
  is path-prefix-based (the longest declared module path that prefixes the
  file's path wins), so a module boundary that does not line up with the
  directory layout is not seen. The drift comparison deliberately ignores
  line-ending differences: git normalises CRLF and LF at checkout, so
  comparing raw bytes would warn on every clean Windows clone for a
  difference nobody introduced.
- Boundaries reach sub-packages now, not only top-level packages. A dotted
  or scoped import (`ai_energy_manager.documents`) is split into segments
  and resolved against every declared module's path, also split into
  segments. The import matches a module when the module path's *trailing*
  segments equal the import's *leading* segments, and the module scoring
  the longest such run wins. A heading `## src/pkg/documents` therefore
  beats `## src/pkg` for the import `pkg.documents.domain`, because it
  matches two segments instead of one. A plain top-level import still
  resolves exactly as before, since matching one segment is what the old
  top-segment rule did. Three caveats:
  - **The filesystem layout has to mirror the import path.** The resolver
    matches on the module's declared *path*, not on where its files
    physically live. A Python package (`src/pkg/facts/` importable as
    `pkg.facts`) or a JS package under a matching directory name resolves;
    a project whose import name and directory name diverge (an exotic
    source root, a package renamed at publish time) does not, because
    nothing here reads `sys.path` or a bundler's alias config.
  - **A tie between equally-scoring modules goes to the first one declared**
    in `.plumb/architecture.md`, not the more specific or more recently
    added one. Two modules named `api` at different depths (`apps/api` and
    `services/api`) both score 1 against a bare `import api`, and whichever
    heading comes first in the file wins.
  - **A bare one-segment match is still just a name match**, unchanged from
    the old rule: nothing checks that a resolved module and the package the
    import actually names are the same thing, only that their text lines
    up. Splitting one top-level package into finer sub-packages multiplies
    how many short, common leaf names (`utils`, `core`, `models`) a
    declared module can carry, and each is as exposed as a top-level module
    name always was to colliding with an unrelated third-party package of
    the same name. At two or more segments a declared module path that
    happens to *mirror* a third-party package's own path can collide too: a
    module heading `## services/google/cloud` scores 2 against
    `from google.cloud import storage`, exactly as if the project depended
    on `google.cloud`, which the old top-segment rule could never do since
    it only ever compared one segment.

  What is still true regardless: Rust stays crate-level only. A Rust import
  never spells the `src` directory (`use plumb_core::manifest`, not
  `use plumb_core::src::manifest`), but every module file still sits under
  it, so `src` interposes between the crate directory a module heading
  names and the module file inside it. A crate-qualified import can reach a
  heading like `## crates/plumb-core` but never one shaped
  `## crates/plumb-core/src/manifest`, because no import segment will ever
  equal the literal `src`.
- **C# and Dart are read by the same line-based heuristic as the other
  three, and each has gaps the other languages do not.** Both extractors
  decide visibility on something objective in their own language — C#'s
  `public` keyword, Dart's leading `_` — so neither makes a style
  judgement. What they miss:

  **C#.** Imports are `using` directives: a plain `using X.Y.Z;`, a
  `global using`, a `using static X.Y.Z;` (which keeps the type name, inert
  because resolution matches leading segments) and the alias form
  `using Db = X.Y.Z;`. The `using` *statement* — `using (var f = …)`,
  `using var scope = …;`, `await using …` — is excluded by shape, not by a
  list of exceptions.
  - **A C# namespace maps to a directory path by convention, not by
    contract**, and this is the gap worth knowing before trusting a clean
    boundary run on a .NET repository. Resolution splits a declared module's
    path on `.` as well as `/`, so a heading `## src/Ummarell.Api` matches
    `using Ummarell.Api.Services;`. That works because most .NET projects
    keep namespace and folder in step. A project where they diverge — a
    `RootNamespace` set in the `.csproj`, a namespace flattened at the
    assembly root, files moved without renaming their namespace — resolves
    to no module at all, and its cross-boundary edges are **silently
    missed**, no finding and no warning. Nothing here reads the `.csproj`,
    so Plumb cannot tell the two cases apart.
  - Splitting module paths on `.` is a repository-wide rule, not a C#-local
    one. Any future heading carrying a dot (`## vendor/lib.v2`,
    `## config/app.settings`) now resolves on its parts. No such heading
    exists today.
  - Only `public` **type** declarations are indexed — `class`, `interface`,
    `struct`, `record`, `enum`, `delegate`. Methods, properties and fields
    are deliberately not: a C# file's members outnumber its types several
    times over and would bury the surface the index exists to show. The
    cost is that a file whose whole contribution is methods on a `partial`
    class indexes as nothing — but only when that part omits the
    accessibility modifier. A part written `public partial class Foo`
    still indexes as `class Foo`, so a type split across parts shows up
    once per part that repeats `public`, and only the bare
    `partial class Foo` parts disappear.
  - An attribute on the same line as the declaration
    (`[ApiController] public class HomeController`) yields nothing, because
    the first token is neither `public` nor a known modifier. The common
    style puts the attribute on its own line.

  **Dart.** Both `package:` URIs and relative specifiers become edges.
  `import 'dart:async';` is an SDK builtin and is skipped, the `node:`
  precedent; `part` and `part of` are never imports, since the two halves
  are one library.

  This paragraph used to say the Dart boundary gate was inert, and it was:
  a `package:` URI kept its first segment and a relative specifier was
  dropped on the floor, so on the Flutter application this support was
  built against every internal import fell into one of those two holes and
  the gate reported nothing at all. Both holes are now closed, and what
  follows is what closing them costs.
  - **A `package:` URI loses its first segment**, because that segment is
    the pub package's own name and not a directory. Dart requires a
    package's library code to live under `lib/`, which no import path ever
    spells, so `package:umarell/features/session/chat.dart` addresses the
    file `lib/features/session/chat.dart` and the leading `umarell` names
    nothing on disk. Dropping it yields `features/session/chat`, which
    reaches a heading `## lib/features` on its first segment. Nothing reads
    `pubspec.yaml` to confirm the name — plumb-core may not touch the
    filesystem — so the first segment is *dropped*, not *resolved*.
  - **The trade for that drop is a leaf collision on external packages.**
    `package:flutter/material.dart` becomes the bare `material`, so a
    repository that also declares a module whose last path segment is
    `material` will see the Flutter import resolve to it and warn about a
    crossing that does not exist. That is the same leaf-collision risk
    every language's resolution already carries — segment matching cannot
    tell a vendor name from a directory name — and it is the deliberate
    side to err on, because the alternative was a segment that matched
    nothing and a gate that reported nothing.
  - **A relative specifier is joined against the importing file's own
    directory**, so the same line in two files means two different targets:
    `import '../../shared/models/m.dart';` in
    `lib/features/chat/a.dart` resolves to `lib/shared/models/m`.
  - **`.` and `..` are normalised lexically, against the path text and not
    against the filesystem.** No `canonicalize`, no symlink resolution, no
    check that the target exists. Two reasons, and both are structural
    rather than an economy: plumb-core is forbidden the filesystem, and the
    text being scanned often arrives from `git show` — the HEAD side of the
    before/after comparison — where there is no worktree to canonicalise
    against even where it would be allowed. A `..` chain that climbs past
    the repository root names no file this repository contains, so it
    yields nothing and the specifier is skipped rather than guessed at.
    A `package:` URI containing `../` is not normalised at all, since its
    path is package-internal rather than file-relative; that shape is legal
    Dart, uncommon, and still silently missed.
  - **The two shapes resolve to two different spellings of the same
    module, and both spellings appear in the same report.** A `package:`
    import resolves *lib-relative* (`features/session/chat`) because the
    package name it dropped stood where `lib/` would be; a relative import
    resolves *repo-relative* (`lib/features/session/chat`) because it was
    joined against a repository path. Resolution matches a module's
    trailing segments against an import's leading ones, so both land on
    `## lib/features` — the first at score 1, the second at score 2 — and
    the gate's verdict is the same either way. What differs is the text a
    finding quotes, since it quotes the import as resolved — so one run
    over one repository prints `features/session/chat` on one line and
    `lib/features/session/chat` on the next, both naming
    `## lib/features`. Cosmetic, and worth knowing before reading it as a
    bug.
  - **A malformed specifier that only looks `.dart`-adjacent keeps its
    tail.** The suffix strip is a literal `.dart` at the end of the string,
    so a `'…/project.dart?v=1'` — legal in no Dart compiler — is not
    stripped and the query tail survives into the resolved path. It costs
    nothing but noise: the target module is matched on the *leading*
    segments, which are unaffected, so the edge is still found and still
    reported; only the import quoted in the message carries the garbage.
    Measured on the Flutter repository below, both the `package:` and the
    relative form of that specifier produced the same finding a well-formed
    one would, with `project.dart?v=1` in the text.
  - **A directive split across lines is missed entirely**, the same
    `text.lines()` trade the parenthesised Python import and the multi-line
    Rust `use` group take. Extraction needs the `import`/`export` keyword
    and the opening quote on one line, so a URI wrapped onto the next line
    yields nothing at all — not a partial edge, no edge.
  - Symbols are column-0 declarations only, which in Dart *is* the
    top-level filter, as in Python. Two consequences: a top-level function
    written without a return type (`main() { … }`, `myWidget() => …`) is
    not indexed, because one token before a parameter list is
    indistinguishable from a call; and top-level getters and setters
    (`String get name => …`) are not indexed either, having no parameter
    list for the function rule to find.
  - **An unformatted column-0 initialiser can index as a function that is
    not there.** `int counter=f(x);`, with no spaces around the `=`, leaves
    two tokens before the parameter list — `int` and `counter=f` — which is
    exactly the shape the function rule matches, so it is indexed as
    `fn counter`. Written the way `dart format` writes it,
    `int counter = f(x);` has three tokens and is rejected. The line
    heuristic paying its price in the other direction for once: the rest of
    these gaps drop real symbols, this one invents one.

  **What that adds up to, measured.** The same Flutter application the
  inertness was found on — 138 tracked `.dart` files, 271 internal imports
  under `lib/`, 262 of them relative and 9 `package:` naming the app's own
  package — was swept a second time against a `.plumb/architecture.md`
  declaring `## lib/core`, `## lib/features`, `## lib/shared` and
  `## lib/l10n`, with `lib/shared` permitted nothing. Every tracked `.dart`
  file was put through the gate in one run — but "put through" is not
  "checked": 59 of those 138 files live under `test/` and
  `integration_test/`, outside every declared module, and their 180
  `package:` imports are attributed to no module and so can never produce a
  finding. That skip is by design — `module_of` returns `None` for a path no
  module heading covers, and a file the gate cannot attribute is a file it
  will not judge — but it belongs next to the measurement rather than only
  in the design notes. It reported four findings, all
  four in `lib/shared/models/enum_mappers.dart`, all four the
  `shared → features` crossings the repository genuinely has, and nothing
  else anywhere in the tree. Declaring each feature its own module and
  permitting nothing to anything — a fixture, not a design — turned the
  same sweep into 160 findings across 39 files, every one of which names an
  import that resolves to a file that exists on disk. The number to
  remember is that the previous count was zero.

  One thing both changes do beyond their own languages: `.cs` and `.dart`
  files under a declared module are now scannable, so a repository
  containing them sees different `boundary` and `index-stale` results than
  it did before, whether or not it asked for the new languages.
- The index scanner's symbol extraction is line-based, the same shape of
  matcher as the boundary gate's imports. A macro-generated declaration is
  missed because it never exists as a literal source line, and a multi-line
  declaration is missed past its first line. It does not know about strings
  or comments either, so a `pub fn` spelled out inside one can
  false-positive. Dogfooding on Plumb's own source caught this directly:
  `crates/plumb-core/src/index.rs` has tests whose expected-output strings
  contain the literal text `pub fn run`, `pub struct Outcome`, `pub fn
  method`, `pub const MARKER`, `pub const fn new`, `pub async fn go`, `pub
  unsafe fn raw` and `pub const unsafe fn x`, and every one of them is
  listed in `.plumb/index.md` as if it were a real public item of that
  module, alongside the ones that actually are. Rust's own `pub` is
  over-inclusive on purpose here: `pub` inside a private (non-`pub`) module
  still counts as public surface, because the scanner reads declarations,
  not effective visibility. It over-reports rather than staying silent on
  re-exports it cannot trace. Rust's `const`, `async` and `unsafe`
  modifiers on a `pub fn` are recognised: `pub const fn`, `pub async fn`,
  `pub unsafe fn` and any combination of the three extract as kind `fn`,
  not as a fabricated `const` entry named `fn`; a plain `pub const NAME: …`
  or `pub static NAME: …` keeps `const`/`static` as its kind. Python's
  `async def` and JavaScript's `export default async function` are
  recognised the same way, each keeping its non-async kind (`def`,
  `function`).
- `plumb report`'s coupling delta resolves every commit's touched files
  against **today's** `.plumb/architecture.md`, not the architecture that
  commit's author actually saw. A module renamed or split afterward reads
  older commits under boundaries that did not exist yet at the time.
- The commit walk is `git log --first-parent`, the same reference `numstat`
  reads a commit's diff against. A pull request landed as a merge commit
  counts once, as the merge's own diff, not once per inner commit it
  carried. The two agree with each other, but a history of many small
  merges reports very differently from a rebase-and-fast-forward one over
  the same work.
- `numstat` runs `git show --first-parent --numstat`, and `--first-parent`
  on `git show` requires git 2.31 or newer. On an older git a merge
  commit's diff comes back empty, so numstat reads `+0/-0` for the whole
  commit and a merged pull request silently vanishes from both volume and
  coupling.
- `git show --numstat` quotes a path containing unusual characters (tabs,
  quotes, non-ASCII control bytes). That quoted form is still summed into
  volume, but coupling resolution matches paths as literal text, so a
  quoted path's line counts land in the volume total while its edges can go
  uncounted.
- A renamed file appears in `numstat` as `{a => b}/path.rs`. The scan
  rejects that literal string as a path, so its line counts still enter
  volume (rename-reduced, as git reports them) but its coupling edges
  silently vanish. A commit that renames a file and adds a new cross-module
  import in the same change reports coupling `0`.
- Whenever `.plumb/architecture.md` is absent, coupling renders as `-` for
  every row and the report falls back to volume and deletion ratio alone.
  There is nothing to resolve an import edge against, and one stderr line
  says so once, not per commit.
- Coupling delta is `child_edges − parent_edges`, a plain count. A commit
  that swaps one cross-module edge for a different one nets to zero:
  replacing an undeclared dependency with an equally undeclared one shows
  no coupling change at all.
- Coupling delta counts a permitted edge exactly like a violation, because
  `plumb report` measures coupling rather than judging it. Edges are
  counted per file, so coupling delta tracks how many touched files import
  across a module boundary, not whether the import is allowed.

## Network and privacy

Plumb makes exactly one class of outbound request: resolving package names
against public registries. Nothing else leaves your machine, and there is no
telemetry. `--offline` disables lookups entirely.

Registry answers are cached on disk, **outside the repository being
checked**: under `%LOCALAPPDATA%\plumb\registry\` on Windows, and
`$XDG_CACHE_HOME/plumb/` or `~/.cache/plumb/` elsewhere, keyed per
repository.

The location is a correctness property, not housekeeping. The cache is
unauthenticated, and a fresh entry answers without contacting the registry.
A cache file that travels with a change can therefore assert that a
nonexistent package exists, and the gate will believe it, silently, for up
to thirty days. Earlier versions kept the cache at
`<repo>/.plumb/cache/registry.json`, where committing one file disabled the
gate. **Never keep a Plumb cache inside a repository.** If one is still
there from an earlier version, the run says so on stderr; it is not read,
and it can be deleted.

If no cache directory can be resolved at all (a container with no `HOME`),
the run does not cache. It says so and works from the network. There is
deliberately no fallback to the temporary directory: a predictable path in
a world-writable directory is a trust store any other local user can
pre-create, which is the same hole by a different route. A cold cache costs
a round trip; a poisoned one costs the gate.

`PLUMB_CACHE_FILE` names the cache file outright, overriding all of the
above. It exists because the test suite has to point the binary somewhere
isolated to stay off the network, and because a persistent worker or a
developer with an unusual home directory may need to say where the file
goes. **It is not containment-checked**: pointed inside a repository it
reinstates exactly the hole described above, and the run will warn on
stderr and carry on. It also does not make a CI check meaningful; see
[what `--diff` compares](#what---diff-compares-and-what-that-rules-out).
On a clean checkout there is nothing to look up, so there is nothing to
cache.

An unreachable network is never a rule violation. A blocking gate that
fails when the network hiccups gets disabled within a week, and then
catches nothing at all. So a name that cannot be resolved yields no
finding, including every name in an offline run with a cold cache, which
will report nothing and exit `0`. An offline run that answered from cache
entries past their lifetime says so on stderr; one that could not answer at
all does not, and that is a gap worth knowing about before wiring
`--offline` into CI.
