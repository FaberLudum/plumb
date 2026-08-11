# Changelog

Notable changes, most recent first. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[SemVer](https://semver.org/). Until 1.0.0 the CLI surface and the exit-code
contract can change in minor versions, and each such change is called out
here explicitly.

## [Unreleased]

- **pub.dev/Dart: the `package-exists` gate's fourth ecosystem.**
  `pubspec.yaml` is read for its `dependencies`, `dev_dependencies` and
  `dependency_overrides` tables; a nonexistent package blocks exactly like
  a PyPI, npm or NuGet one, against pub.dev's package API, cached outside
  the repository under the same 30-day/1-hour split. Names are matched
  case-sensitively — pub.dev resolves `Autolinker` and `autolinker` as
  different packages — so unlike the NuGet cache key, the pub.dev one is
  not lowercased. Only registry-hosted entries are looked up: `sdk:`,
  `git:` and `path:` dependencies are dropped before any lookup, so
  `flutter` and `flutter_test` produce nothing at all rather than a
  forgiven finding. A `hosted:` naming a private registry is a
  `manifest-understood` gap, not a lookup; a `hosted:` naming pub.dev
  itself (or the retired `pub.dartlang.org`) is an ordinary lookup.
  Version constraints are unchecked (existence only); `pubspec.lock`,
  `pubspec_overrides.yaml` and workspace `resolution:` are unread.
  Evidence: taxonomy §4.1, with the same honesty note the NuGet entry
  carries — Spracklen and Churilov measured Python and JavaScript
  hallucination rates, not Dart, so this extension rests on the defect
  class and objective detection alone.
- **NuGet: the `package-exists` gate's third ecosystem.** `.csproj`,
  `.fsproj`, `.vbproj` and `Directory.Packages.props` are read for
  `PackageReference`, `PackageVersion` and `GlobalPackageReference`
  elements' `Include` attribute; a nonexistent package id blocks exactly
  like a PyPI or npm one, against nuget.org's flat-container index, cached
  outside the repository under the same 30-day/1-hour split. `Version`
  ranges are unchecked (existence only); an MSBuild property expression
  (`$(...)`) in `Include` is a `manifest-understood` gap, not a lookup;
  `packages.config` is recognised by filename and deliberately never
  parsed, always `manifest-understood`. Evidence: taxonomy §4.1, with the
  honesty note that Spracklen and Churilov measured Python and JavaScript
  hallucination rates, not C#/.NET — the NuGet extension rests on the
  defect class and objective detection alone.

## [0.1.1] — 2026-08-11

No behaviour change to the gate or the CLI. This release exists to ship
the public-readiness layer and to exercise the release pipeline for the
first time.

- Release workflow: binaries for Linux, Windows and macOS attached to
  every version tag, built from the tag itself with the tests re-run.
- README: Installing and Quick start sections; the Status opener updated
  to the actual surface (five commands, six kinds of finding).
- CONTRIBUTING: recipes for adding a language and a package ecosystem.
- This changelog.

## [0.1.0] — 2026-08-11

First tagged release. Everything below traces to a row in
[docs/research/taxonomy.md](docs/research/taxonomy.md) — no source, no rule.

### The gate — `plumb check --diff`

- **`package-exists` (Tier A, blocks).** Every package a change *adds* must
  exist on its registry. PyPI (`requirements.txt`, `requirements-*.txt`,
  PEP 621 `pyproject.toml`) and npm (`package.json`, four dependency
  tables). Registry answers cached outside the repository for 30 days;
  `--offline` disables lookups. Evidence: taxonomy §4.1.
- **`manifest-understood` (Tier B, warns).** A recognised manifest declaring
  dependencies in a shape Plumb did not read is reported, so a run that
  established nothing is never mistaken for a run that found nothing.
  Taxonomy §10.
- **`boundary` (Tier B, warns).** Imports checked against the module
  boundaries declared in `.plumb/architecture.md`, sub-packages resolved by
  longest segment match, ancestors covering their subtrees. Only violations
  *new* in the change warn, keyed by resolved edge — touching a file with a
  legacy violation does not punish the wrong commit. Taxonomy §4.5, H5.
- **`adapter-drift` / `index-stale` (Tier B, warns).** Generated instruction
  files that no longer match `.plumb/`, and a symbol index that no longer
  matches the tracked sources. Taxonomy §11.

### Layer 1 — the working agreement

- **`plumb init`** writes `.plumb/` (`architecture.md`, `conventions.md`,
  `invariants.md`, empty `index.md`) from a short bootstrap Q&A.
- **`plumb index`** scans tracked Rust, Python and JavaScript/TypeScript
  sources for their public surface and writes `.plumb/index.md`.
- **`plumb adapters`** renders the agreement into `CLAUDE.md`, `AGENTS.md`,
  `.cursorrules` and `.github/copilot-instructions.md`, refusing to
  overwrite hand-written files. An index over 150 lines becomes a pointer
  to `.plumb/index.md` instead of inline content, so a large repository
  does not load its whole symbol table into every agent session.

### Instrumentation

- **`plumb report --commits <n>` (Tier C, never affects exit code).**
  Longitudinal volume, coupling and deletion metrics over recent history.
  The decay signature is explicitly uncalibrated and labelled as such.

### Contract

- Exit codes: `0` clean, `1` Tier A violation, `2` warnings only; a run
  that could not happen exits outside that range (`64` usage, `70`
  internal), so a broken gate is never read as a clean one.
- No test contacts the network; CI proves it in a namespace with no route
  out, on Linux, Windows and macOS.

### Calibration

Validated against a real repository's history by retrospective replay: 19
commits through the boundary gate produced zero false positives after two
parser fixes; the one surviving warning was a real layering violation that
the repository's own later history removed.

[0.1.0]: https://github.com/FaberLudum/plumb/releases/tag/v0.1.0
