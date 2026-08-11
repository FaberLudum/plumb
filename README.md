# Plumb

A plumb line checks whether a structure stands true. Plumb does the same for
codebases written with AI assistance. It doesn't grade individual functions;
it checks that the system as a whole is still upright.

## Why

Research on AI-generated code keeps finding the same asymmetry. At the unit
level the code is fine: functions come out smaller than human ones, no more
complex, and GitHub's own randomized trial scored them slightly ahead on
readability. The damage shows up at the system level. 22.7% of AI-introduced
technical debt never gets cleaned up. Runtime dependencies outnumber declared
ones 13.5 to 1. Architectural decay grows with the volume of code produced,
and volume is exactly what AI assistants are good at.

The quality tools everyone already runs were calibrated on human failure
modes. They hunt for the giant function, the deep nesting. AI code doesn't
have those peaks, so the tools stay green while the system rots underneath.
Plumb goes after what they can't see.

## The one rule

Every rule Plumb enforces traces back to a row in
[docs/research/taxonomy.md](docs/research/taxonomy.md). No source, no rule.
This applies to the maintainers too. The taxonomy also lists what Plumb
refuses to fight (function length, cyclomatic complexity, readability
thresholds), because the evidence says AI already matches or beats humans
there, and enforcement spent on a non-defect is pure friction.

Rules are tiered by strength of evidence, not by severity:

| Tier | Enforcement | Evidence required |
|---|---|---|
| A | Blocks the build | Peer-reviewed, objective detection |
| B | Warns | Preprint, or thresholds needing calibration |
| C | Measures only | None. Instrumentation for open hypotheses |
| Hygiene | Autofixed, excluded from defect counts | Style consistency, delegated to formatters |

Proposing a Tier A rule on a preprint gets rejected. Propose it as Tier B.

## Installing

Grab a binary from the [releases](https://github.com/FaberLudum/plumb/releases)
(one archive per platform, named by host triple) and put `plumb` on your
`PATH`. Or build from source:

```bash
cargo install --git https://github.com/FaberLudum/plumb plumb-cli --locked
```

Not on crates.io yet.

## Quick start

In the repository you want checked:

```bash
plumb init      # short Q&A, writes .plumb/ (the working agreement)
plumb index     # scans the sources, writes .plumb/index.md
plumb adapters  # renders CLAUDE.md, AGENTS.md, .cursorrules, copilot-instructions.md
```

Review what `init` wrote. `.plumb/architecture.md` in particular is a
contract, and only you know which module may depend on which. Then gate your
changes:

```bash
plumb check --diff
```

Exit `0` is clean, `1` is a blocking violation, `2` is warnings only. A run
that couldn't happen at all exits outside that range, so a broken gate never
reads as a clean one. Wire it into a pre-commit hook:

```bash
echo 'plumb check --diff' > .git/hooks/pre-commit && chmod +x .git/hooks/pre-commit
```

## What it checks

| Finding | Tier | What it means |
|---|---|---|
| `package-exists` | A, blocks | A package the change adds doesn't exist on its registry (PyPI, npm, NuGet or pub.dev). This is how hallucinated dependencies get in. |
| `manifest-understood` | B, warns | Part of a manifest was in a shape Plumb couldn't read. Said out loud so a run that established nothing isn't mistaken for a clean one. |
| `boundary` | B, warns | An import crosses a module boundary that `.plumb/architecture.md` doesn't allow. Only violations the change introduces are reported; legacy ones stay quiet. |
| `adapter-drift` | B, warns | A generated instruction file no longer matches `.plumb/`. One command fixes it. |
| `index-stale` | B, warns | `.plumb/index.md` no longer matches the sources. Same remedy. |

A sixth command, `plumb report`, prints volume, coupling and deletion
metrics over recent history. It measures and never blocks.

```console
$ plumb check --diff
blocking: package `reqeusts` does not exist on PyPI [package-exists]
  requirements.txt
  evidence: taxonomy §4.1
$ echo $?
1
```

## Honest limits

Two things you should know before trusting a green run.

`--diff` compares the working tree against HEAD. On a fresh clone or a CI
checkout the tree is clean, so the manifest and boundary checks have nothing
to compare and pass by default. Today those two checks earn their keep in a
developer's working copy, not in CI. The drift and staleness checks don't
have this limit; they look at current state and fire anywhere.

The gates have known blind spots (lockfiles aren't read, private registries
false-positive, conda is out of scope, and more). They're all written down in
[docs/gates.md](docs/gates.md), which also explains exactly what each gate
reads and why. A gate that stays quiet about its gaps is worse than no gate,
so the list is long on purpose.

Plumb talks to package registries and to nothing else. No telemetry.
`--offline` cuts even that. Details, including why the cache lives outside
your repository, are in [docs/gates.md](docs/gates.md#network-and-privacy).

## Building

Rust 1.97, edition 2024. The toolchain is pinned by `rust-toolchain.toml`,
so plain `cargo test --workspace --locked` does the right thing. Every
third-party crate has a row in [docs/dependencies.md](docs/dependencies.md)
saying why the standard library wasn't enough. That's Plumb's own Tier A
rule, applied to Plumb.

## Contributing

A pull request that adds or changes a rule must cite its evidence: source,
metric, effect size, limits, and whether the primary source was verified.
The row goes into the taxonomy in the same PR. A rule without a source is
closed, however sensible it sounds. That discipline is the only thing
separating this project from another opinionated linter.

Recipes for adding a language or a package ecosystem are in
[CONTRIBUTING.md](CONTRIBUTING.md).

## Mascot

Plumby. The friendly name belongs on the mascot, not on the command.

## License

Apache-2.0. See [LICENSE](LICENSE).
