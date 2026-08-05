# Plumb — Design Spec

**Date:** 2026-08-05
**Status:** approved
**Evidence base:** [docs/research/taxonomy.md](../../research/taxonomy.md)
**License:** Apache-2.0

A plumb line checks whether a structure stands true. That is the tool's job:
not judging whether each brick is well made, but whether the building is
still vertical.

The mascot is Plumby. The tool is `plumb` — the friendly name belongs on the
mascot, not on the command.

---

## 1. Problem

AI-generated code fails differently from human code. Human code fails in the
detail — the thousand-line function, the fifteen-branch conditional, the
unreadable name. AI code is clean in the detail and decays in the aggregate:
debt that never gets cleaned up, dependencies nobody declared, architecture
that erodes as volume grows.

Every quality tool in common use — linters, complexity metrics, size
thresholds — is calibrated on human failure modes. Run them on AI code and
they report a clean bill of health while the system degrades below their
thresholds. The taxonomy documents this in detail; the short version is that
AI writes smaller functions, simpler branches and more readable code than
humans, and still produces systems that rot faster.

This framework targets the failure modes that the existing tools structurally
cannot see.

## 2. Goals

- Prevent the defects that the evidence shows are real, at the point where
  they are introduced.
- Work on greenfield projects from the first commit, not only on mature
  codebases with something to analyse.
- Stay agent-agnostic: Claude Code, Cursor, Copilot, Codex and whatever
  replaces them.
- Make architectural intent executable, so it cannot silently erode.

## 3. Non-goals

- **Quality metrics calibrated on human failure modes.** Function length,
  cyclomatic complexity, readability thresholds. The evidence says AI already
  matches or beats humans on these. Enforcing them finds nothing and costs
  friction.
- **Micro-quality as the framework's value proposition.** Style *consistency*
  is in scope, as signal hygiene (§5.4). Style *quality* is not, and is not
  what the framework is for.
- **Replacing existing linters.** Where ruff, eslint or Roslyn already do the
  job, the framework defers to them rather than reimplementing them.
- **Detecting whether code was AI-written.** The rules apply to all code in the
  repository. Attribution is a measurement concern, not an enforcement one.
- **Being a prompt library.** Zhu et al. show detailed prompting does not
  mitigate architectural decay. Instructions alone are a known dead end.

## 4. Architecture

Two layers, with different obligations.

```
┌─────────────────────────────────────────────────────────┐
│  Layer 0 — Baseline                                     │
│  Evidence-derived rules. No project context needed.     │
│  Active from commit zero on any repository.             │
├─────────────────────────────────────────────────────────┤
│  Layer 1 — Project context                              │
│  Architecture, conventions, capability index.           │
│  Declared on greenfield, derived on brownfield.         │
└─────────────────────────────────────────────────────────┘
             │                        │
             ▼                        ▼
      ┌───────────────┐       ┌───────────────┐
      │ plumb CLI     │       │ Agent adapters│
      │ (gates)       │       │ (generated)   │
      └───────────────┘       └───────────────┘
```

Layer 0 works with zero configuration. Layer 1 adds project-specific
constraints and is where the framework earns its keep over time. A repository
that installs Layer 0 and never fills in Layer 1 still gets the Tier A gates.

## 5. Layer 0 — Baseline rules

Each rule cites the taxonomy section that justifies it. A rule that loses its
source is removed, not grandfathered.

### 5.1 Tier A — blocking

Peer-reviewed evidence, objective detection, no judgement calls.

| Rule | Detection | Source |
|---|---|---|
| Every referenced package exists in its registry | Resolve each new import against the registry before install | §4.1 |
| No unreviewed security findings on the diff | Delegate to the stack's SAST, fail on new findings | §4.2 |
| Declared dependencies match the runtime closure | Compare manifest against actually-loaded modules | §4.3 |
| New external dependencies carry a written justification | Require a justification entry when the manifest grows | §4.3 |

The package rule is the clearest case for blocking. A hallucinated package name
is not a style opinion; it either resolves or it does not, and 43% of
hallucinated names recur identically across runs, which is what makes
slopsquatting practical.

The security rule delegates rather than reimplements. The framework's
contribution is that the gate runs at all and that its findings cannot be
waved through by the author — Perry et al. found developers using assistants
believe their code is more secure while it is measurably less so, so
self-assessment is exactly the wrong control.

### 5.2 Tier B — warning

Preprint evidence, thresholds needing per-project calibration. These report
and record; they do not block until calibrated against the project's own
history.

**Volume and coupling control.** Zhu et al.'s Volume-Quality Inverse Law makes
volume a leading indicator of structural decay. The framework tracks three
quantities per merged change:

- net volume delta (lines added minus removed)
- coupling delta (change in inter-module fan-out)
- deletion ratio (lines removed over lines added)

The decay signature is volume and coupling rising **together**, particularly
when coupling grows faster than volume. A change that adds code within existing
boundaries is ordinary growth. A change that adds code while widening the
dependency graph is the pattern the evidence associates with decay.

The deletion ratio is the counterpart signal. Healthy projects delete code;
accretion without deletion is how volume compounds. This is measured, not
enforced — there is no defensible universal threshold yet.

**Architectural boundary check.** Imports are validated against the module
boundaries declared in Layer 1. This makes the architecture document
executable: a boundary nobody can violate without a failing check stops being
decoration. Zhu et al. conclude agents need "explicit architectural
foresight"; this is the mechanical form of that.

Promotable to blocking once a project has calibrated it and accepted its false
positive rate.

**Debt analysis on the diff.** Static analysis runs on the change, not on the
repository. Liu et al. found 22.7% of AI-introduced issues still alive at HEAD,
which means introduced debt does not get cleaned up later. Downstream checks
arrive after the decision that mattered.

### 5.3 Tier C — measurement only

**Cross-session convention consistency.** H3 is unmeasured in the literature —
not refuted, never studied, because it needs longitudinal repositories with
session attribution that almost nobody keeps. The framework records the data
that would answer it: convention fingerprints per commit, tracked over time.
No rule, no gate, no claim. If a signal emerges from real projects it becomes
a contribution; until then it stays a hypothesis with instrumentation attached.

### 5.4 Hygiene — delegated and autofixed

Style consistency is in scope. Style quality is not. The distinction matters
and collapsing it was an early mistake in this design.

What the evidence refutes is "AI produces low-quality units". What it does not
refute — and partly supports — is "AI produces units that are inconsistent
with each other". Wang et al. catalogue 24 kinds of stylistic inconsistency
between AI and human code, and the largest single effect in Mao et al. is
lexical density (d = -0.555): humans compress, AI dilutes.

Three reasons this belongs in the framework despite not being a defect the
research condemns:

**It is nearly free.** The argument against micro-quality enforcement was that
it costs friction for no return. That argument holds when enforcement is a
judgement call. It does not hold when enforcement is `ruff format` or
`dotnet format` — tools that already exist, are deterministic, and fix their
own findings.

**It serves the system-level metrics.** Volume and coupling deltas are measured
on diffs. Stylistic noise in diffs corrupts that signal. A stable style
produces clean deltas, so consistency is instrumental to Tier B working at all.

**It is a precondition for Tier C.** Cross-session drift is a consistency
problem. Drift cannot be detected without a stable baseline to detect it
against. Measuring drift while leaving style outside the framework would be
incoherent.

Three constraints on how it is enforced:

- **Delegated, never reimplemented.** The framework selects and freezes the
  formatter configuration. It does not write a formatter.
- **Autofixed where possible.** Never an error a human has to resolve by hand.
- **Consistency, not judgement.** No threshold on how good a unit is, only on
  whether it resembles the rest of the project.

Configuration lives in `conventions.md` (§6) and is frozen at bootstrap. These
checks are labelled as hygiene rather than defect gates, and are excluded from
success criterion 3 (§10) for reasons given there.

## 6. Layer 1 — Project context

Five artefacts under `.plumb/`.

| Artefact | Content | Greenfield | Brownfield |
|---|---|---|---|
| `architecture.md` | Modules, responsibilities, permitted dependencies | Declared at bootstrap, grows by delta | Derived from code, then corrected |
| `conventions.md` | Error handling, test shape, naming, frozen formatter config | Chosen from defaults, then frozen | Extracted from existing code |
| `invariants.md` | Project-specific non-negotiables | Written as they emerge | Written as they emerge |
| `index.md` | What the repo can already do, and where | Starts empty, grows automatically | Generated by the initial scan |
| `decisions/` | Short ADRs, one per structural choice | On demand | On demand |

`index.md` is generated by scanning public exports, entry points and routes,
and regenerated on every gate run. It is never hand-written. A hand-maintained
index goes stale within a few sessions and then actively misleads, which is
worse than having none.

Note that generation suits greenfield better than brownfield: on a new project
the index grows alongside the code and is never out of date, because there is
no legacy state to reconcile.

### Bootstrap

`plumb init` on an empty repository asks the minimum: which modules, which
boundaries, which error-handling style, which test shape. The answers are
frozen into `conventions.md` and `architecture.md`, and the boundary check is
live from commit one.

**Hard constraint: bootstrap must cost under two minutes.** A longer setup gets
skipped, and a framework that gets skipped at bootstrap never applies to the
project at all. Aggressive defaults, few questions, everything editable later.

On an existing repository, `plumb init` derives a first draft of
`architecture.md` and `conventions.md` from the code and presents it for
correction. Brownfield is the common case; a framework that only installs on
new projects installs almost nowhere.

## 7. Protocols

Three phases, each producing something checkable. Protocols are the part an
agent follows; gates are the part that verifies it followed them.

**Recon, before writing.** State which module owns the change, which
boundaries apply, and whether it needs a new external dependency. Short and
concrete. An empty recon note is itself a signal the gate records.

**Constraints, while writing.** Prefer the standard library over a new
dependency. Justify every dependency that survives that preference. Do not
create a module without declaring its boundary. Do not generalise before the
third real use.

**Verification, after writing.** Gates run. No completion claim without
command output. This mirrors the Perry finding directly: the author's
confidence is not evidence.

## 8. Agent adapters

The five Layer 1 artefacts are the single source of truth. A generator emits
per-agent instruction files from them:

```
.plumb/*.md  ──▶  CLAUDE.md
                   ──▶  AGENTS.md
                   ──▶  .cursorrules
                   ──▶  .github/copilot-instructions.md
```

One source, N harnesses. Adding support for a new agent means adding a
template, not restating the rules. Generated files carry a header marking them
as generated, and the gate fails if they have drifted from their source.

## 9. The `plumb` CLI

```
plumb init            bootstrap or derive Layer 1 artefacts
plumb check --diff    run gates against the working diff
plumb check --repo    full-repository run, for baselining
plumb check --fix     apply hygiene autofixes, then re-run
plumb index           regenerate index.md
plumb adapters        regenerate agent instruction files
plumb report          longitudinal metrics: volume, coupling, deletion ratio
```

`check` outputs human-readable text and JSON. Exit codes: 0 clean, 1 Tier A
violation, 2 Tier B warnings only. CI blocks on 1; 2 is recorded and
reported. Hygiene findings are autofixed and do not affect the exit code;
the rare unfixable one is reported as a warning and tagged as hygiene so it
stays out of defect counts.

Stack gate packs (Python, TypeScript, .NET) plug in underneath and supply the
language-specific analysis — SAST, dependency resolution, import graphs. A
repository with no matching pack still gets the language-agnostic checks:
boundary validation, volume and coupling tracking, adapter drift, index
freshness.

## 10. Success criteria

The framework is worth its friction if, on a control repository:

1. Tier A violations per thousand lines drop measurably after adoption.
2. Cycle time does not degrade. A 40% slower loop for 10% fewer defects is a
   failure and will be reported as one.
3. The **defect gates** fire on AI-authored and human-authored code at
   comparable rates. If they fire far more on human code, they are
   miscalibrated and are detecting style, not the documented defects.

Criterion 3 is the honest one. It is how we find out whether the framework
targets the failure modes the research identified or has quietly drifted back
into being a linter.

Hygiene checks (§5.4) are excluded from this measurement by construction, and
the exclusion is not a convenience. Human code has a heavy-tailed
distribution, so style checks legitimately fire more often on it. Counting
them here would guarantee a failing result for the wrong reason and destroy
the only test that keeps the framework honest.

## 11. Repository layout

```
plumb/
  docs/
    research/          taxonomy.md, sources.md
    superpowers/specs/ design specs
  core/
    rules/             one file per rule, each citing its source
    protocols/         recon, constraints, verification
    templates/         Layer 1 artefact templates
  adapters/            claude-code, cursor, copilot, codex
  gates/
    agnostic/          boundaries, volume, coupling, index, drift
    python/
    typescript/
    dotnet/
  crates/
    plumb-cli/         binary entry point
    plumb-core/        rules, protocols, gate orchestration
    plumb-graph/       tree-sitter parsing, import and boundary graphs
```

## 12. Build phases

1. **Research** — complete. Taxonomy and sources committed.
2. **Core** — rules and protocols, each traced to a taxonomy row.
3. **Layer 1 templates and adapter generation** — the artefacts and the
   single-source-to-N-harnesses generator.
4. **Agnostic gates** — boundaries, volume and coupling, index, adapter drift.
5. **Tier A stack packs** — dependency resolution and SAST delegation for
   Python, TypeScript and .NET.
6. **Dogfood and calibration** — install on a real repository, baseline, and
   calibrate Tier B thresholds against actual history.

Phase 5 has a hard prerequisite: **S7 and S8 must be promoted to VERIFIED
before any Tier A gate ships.** Those two studies carry the blocking security
gate and are still second-hand. Shipping a blocking rule on an unchecked
citation in a project whose entire premise is evidence discipline would be
self-refuting, and it is the first thing a reader will check.

From phase 3 onward the framework is built under its own rules. It is itself
AI-generated code and is exactly the kind of project the taxonomy describes.
If it cannot survive its own gates, that is a finding worth having early.

### 12.1 Self-application, and its limit

Self-application is partial by construction. Plumb is written in Rust while
the planned gate packs target Python, TypeScript and .NET, so the Tier A
dependency gate cannot run on Plumb until a crates.io ecosystem pack exists.
The language-agnostic gates — boundaries, volume and coupling, adapter drift,
index freshness — apply from the moment they are built.

**Dogfooding proves usability, not efficacy.** A tool whose only evidence is
that its own repository passes its own gates is a sample of one, judged by its
authors. Worse, it invites tuning Tier B thresholds until the home repository
goes green, which is precisely how a tool becomes self-confirming.

The rule that prevents this: **thresholds are calibrated on external
repositories, never on this one.** Success criterion 3 (§10) requires a
control repository we did not write.

What self-application does earn is credibility of a different kind. A quality
tool whose own repository fails its own gates is dead on first inspection, and
running the gates in this project's CI is the cheapest possible demonstration
that they work at all.

### 12.2 This repository as an H3 dataset

Plumb is being built by AI across many sessions, with full git history. That
is exactly the data H3 needs and almost nobody keeps: a longitudinal record
with session boundaries, on a single project, over time.

The Tier C instrumentation (§5.3) runs here first. It is not dogfooding — the
project becomes the first dataset for a hypothesis the literature has never
tested. Whatever it shows, including nothing, is worth publishing alongside
the tool.

## 13. Risks

**Tier B thresholds are uncalibrated.** Volume and coupling deltas have no
defensible universal values yet. This is why they warn rather than block, and
why phase 6 exists. The risk is shipping thresholds that feel right and are
arbitrary.

**The boundary check depends on a document humans must maintain.** If
`architecture.md` is wrong, the check enforces the wrong thing, confidently.
Mitigation: derive the first draft from real code, and treat boundary
violations as prompts to reconsider the document, not only the code.

**Zhu et al. is a single unreplicated preprint.** It carries most of Tier B.
If it fails to replicate, Tier B loses its foundation. The invalidation
conditions in the taxonomy cover this explicitly.

**Adoption friction.** Every gate is a chance for someone to disable the
framework. The two-minute bootstrap limit and the blocking/warning split are
both there to keep total friction under the threshold where people opt out.

## 14. Public distribution

The project ships publicly on GitHub. That is a design constraint, not a
release detail, and it changes four things.

### 14.1 The evidence base is the attack surface

The framework's claim to authority is that every rule cites a source. That
makes the citations the first thing a skeptical reader checks, and a single
misattributed paper discredits the whole taxonomy. Three consequences:

- **Verification status is published, not internal.** `sources.md` ships as is,
  including the REPORTED rows. Showing which sources are unchecked is more
  credible than implying all of them are.
- **Corrections are visible.** The corrections section in `sources.md` already
  records three errors caught during verification. It stays. A project that
  documents its own mistakes is harder to attack than one that appears
  flawless.
- **Contested findings are presented as contested.** Duplication is the live
  example: GitClear and Mao reach opposite conclusions. The taxonomy says so
  rather than picking the convenient side.

### 14.2 Contribution policy

The rule for contributors is the rule for us: **a pull request that adds or
changes a rule must cite its evidence**, with metric, effect size and limits,
and state whether the source is verified. A rule without a source is closed,
however sensible it sounds.

This is the project's identity and its main defence against becoming another
opinionated linter. It also gives reviewers an objective criterion, which
keeps rule debates short.

Removals follow the same discipline. If evidence is refuted, the rule is
removed and the removal is recorded — not silently dropped.

### 14.3 Taxonomy versioning

Rules change when evidence changes. Public consumers need that to be legible:

- The taxonomy carries a version, and every rule pins the taxonomy version
  that justifies it.
- A changelog records evidence-driven changes separately from code changes, so
  a user can see why a rule appeared, moved tier, or disappeared.
- The invalidation conditions in the taxonomy are the trigger for a version
  bump.

A tool whose rules shift without explanation gets uninstalled. A tool that
says "this rule moved from blocking to warning because the 2026 replication
found the rate dropped fourfold" earns trust.

### 14.4 Adoption and defaults

Public distribution makes friction existential rather than annoying. Three
constraints follow:

- **Zero-config must do something useful.** Running `plumb check` on a fresh
  clone, with no Layer 1 artefacts and no bootstrap, must produce real value
  from Tier A alone. Bootstrap is an upgrade, never a gate to first use.
- **Network behaviour is documented and optional.** Registry resolution sends
  package names to public registries. That is a privacy consideration for
  private codebases and must be disclosed, with an offline mode that degrades
  the check rather than failing it.
- **No telemetry by default.** Measurement data stays local unless the user
  opts in.

### 14.5 The opt-in that matters

H3 — cross-session convention drift — is unmeasured because it needs
longitudinal repositories with session attribution, which almost nobody keeps.
The framework records exactly that data.

An opt-in, aggregated contribution channel would let adopters close a genuine
research gap collectively. That is a real reason for the project to be public
rather than a marketing angle, and it is the one thing this framework can do
that a private tool cannot.

Strictly opt-in, aggregated, with the collected fields documented. If it
becomes anything users have to think about, it has failed.

## 15. Open questions

- Should Tier A blocking be opt-out per repository, or absolute? Leaning
  opt-out with the choice recorded, so the decision is visible rather than
  silent.
- How is a module boundary expressed for languages without a package system
  strong enough to express it? Directory-based is the obvious fallback but is
  weak in practice.
- Does the volume and coupling signal survive contact with a real repository,
  or is the correlation Zhu found an artefact of agent-generated systems
  specifically? Phase 6 answers this, and a negative answer removes a Tier B
  rule.
- Package identifier if the CLI is ever distributed through a language
  registry. `plumb` on PyPI is held by an abandoned 2018 messaging library.
  Moot while distribution is GitHub releases plus Homebrew and Scoop.
- Which tree-sitter grammars ship in the binary. Bundling Python, TypeScript
  and C# covers the target stacks but grows the binary; loading grammars
  dynamically keeps it small at the cost of a harder install.

## 16. Decisions taken

- **Name:** `plumb`. A plumb line checks structural trueness, which is what the
  boundary check does. The mascot is Plumby; the diminutive stays on the
  mascot, where it costs nothing, rather than on a command that has to read as
  a serious tool.
- **License:** Apache-2.0, for the explicit patent grant. Many corporate legal
  teams prefer or require it for internal tooling, and it costs adopters
  nothing over MIT.
- **Implementation language:** Rust. The CLI ships as a single static binary
  with no runtime for users to install, which is the only way one tool serves
  Python, TypeScript and .NET repositories equally. Multi-language parsing is
  the core of the boundary check, and tree-sitter is native to Rust.

  The cost is honest: slower iteration than Go or Python in the early phases,
  when the design is still moving. Accepted because the parsing surface is
  permanent and the language choice is the hardest decision to reverse later.
