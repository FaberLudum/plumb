# NuGet and pub.dev ecosystems — design

**Date:** 2026-08-11
**Status:** approved for implementation (Andrea supplied the driving
repositories; seams merged as PRs #21/#22)
**Driving repositories (read-only):** `C:\Lavoro\Umarell\backend` (.NET 10,
two `.csproj`), `C:\Lavoro\Umarell\umarell-flutterapp` (Flutter, one
`pubspec.yaml`)

## Scope

Extend the `package-exists` gate (Tier A) to two ecosystems on the
`EcosystemSpec` seam: **NuGet** (PR C — the seam's first real consumer,
as the plugin-seams plan scheduled) and **pub.dev** (PR D, same mould).
Language scanning for C# and Dart (boundary gate + index) is out of scope
here — it rides the `Language` seam and gets its own plan.

## Evidence discipline

No new taxonomy row: §4.1 covers package hallucination as the defect and
CONTRIBUTING already records that another registry does not need a new row.
One honesty edit rides along: §4.1's Limits row says "Python and JavaScript
only" — add that the NuGet/pub extension rests on the defect class plus
objective detection (a package exists or it does not), with hallucination
rates in those ecosystems unmeasured.

## NuGet (PR C)

**Manifests read.** `*.csproj` at any depth (excluding `bin/`, `obj/`) and
`Directory.Packages.props`. From a `.csproj`: every `PackageReference`'s
`Include` attribute, both self-closing and element form; `ProjectReference`
is not a package and is ignored; `Version` is irrelevant (existence gate,
same stance as npm's ranges). From `Directory.Packages.props`:
`PackageVersion Include` (central package management's declaration site)
and `GlobalPackageReference Include`. A UTF-8 BOM must not break parsing —
the driving repo's test project carries one.

**Gaps (Tier B `manifest-understood`), not silence:** `packages.config`
(legacy format, recognised by filename, never parsed); a `PackageReference`
whose `Include` is an MSBuild expression (`$(...)`) — a name Plumb cannot
resolve is reported, not guessed; `<Import Project=...>` indirection is
out of scope and undocumented shapes stay silent only if they are not
dependency declarations.

**Parser: `quick-xml`, a new dependency.** A line heuristic cannot serve a
blocking gate: a commented-out `<!-- <PackageReference Include="Fake"/> -->`
would manufacture a blocking finding, and XML comments/CDATA are not
line-shaped. The standard library has no XML parser; `quick-xml` is the
narrowest maintained option (no DOM, pull events, no transitive tree).
Needs its row in docs/dependencies.md; exact version checked at
implementation time, not recalled.

**Registry.** NuGet V3 flat container:
`GET https://api.nuget.org/v3-flat2/{id}/index.json` with `{id}`
lowercased — 200 exists, 404 missing. IDs are case-insensitive; valid
charset `[A-Za-z0-9._-]`, and an ID that violates it is `Missing` without
a request (the npm lesson: an illegal name interpolated into a URL can
resolve wrong and poison the cache). Cache keys `nuget:` + lowercased id —
two spellings of one ID must share an entry. Unreachable registry is
`Unknown`, never a violation. The three registry properties in CONTRIBUTING
bind verbatim.

## pub.dev (PR D)

**Manifest read.** `pubspec.yaml` (filename exact, any depth). Sections
`dependencies`, `dev_dependencies`, `dependency_overrides`. A dependency is
looked up only when it is registry-hosted: a scalar version (`dio: ^5.9.2`),
an empty value, or a `hosted:` block naming the default host. `sdk:`
(`flutter`, `flutter_localizations`), `git:` and `path:` dependencies are
never looked up — they do not live on pub.dev, and blocking on them would
be a false positive by construction. A `hosted:` block with a custom `url`
is a Gap (private host, same class as a private PyPI index).

**Parser: to be settled at plan time by checking current crates** — the
standard library has no YAML. Two acceptable outcomes, in preference order:
a maintained minimal YAML crate (needs its dependencies.md row), or a
narrow indentation-based reader for exactly the pubspec subset above if the
crate landscape is unmaintained (serde_yaml's archiving is why this is not
recalled from memory). Whichever wins, malformed YAML is `Gap::Unparseable`,
never an empty manifest — the PyPI precedent.

**Registry.** `GET https://pub.dev/api/packages/{name}` — 200 exists,
404 missing. Package names: lowercase `[a-z0-9_]`, must not start with a
digit; illegal name is `Missing` without a request. Cache prefix `pub:`.

## What both PRs inherit unchanged

One spec file in `ecosystems/`, one table line, one registry impl in
`plumb-cli/src/registry.rs`, one `RegistryRouter` arm, one `Ecosystem`
variant; blind-spots documented in docs/gates.md in the same PR; tests
never touch the network (stub registry + seeded cache, no-route-out CI job
proves it); exit-code contract untouched.

## Validation

After both PRs: retrospective replay of the two driving repositories'
full histories (111 and 147 commits) through the gate, EnerLoop-method:
every warning classified as Plumb bug, real finding, or false positive.
The acceptance bar is the same — zero false positives on real history.
