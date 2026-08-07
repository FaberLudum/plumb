# Empirical taxonomy of defects in LLM-generated code

**Version:** 1.0
**Date:** 2026-08-05
**Status:** evidence base for the framework's Layer 0

---

## 1. What this document is for

This is the framework's empirical foundation. Every Layer 0 rule must trace
back to a row in this taxonomy. **No source, no rule.**

The document also works in reverse: it lists the defects the framework must
**not** fight, because the data says they either do not exist or the AI
outperforms humans on them. Enforcement spent on a non-defect costs friction
and credibility.

## 2. Method

Each entry records a citable source, the metric, the effect size and the
study's limits. Sources are classified along two independent axes.

**Strength of evidence:**

| Category | Meaning |
|---|---|
| MEASURED | Published data with an explicit metric and a stated sample |
| CONTESTED | Measured, but another source using a different metric concludes the opposite |
| QUALITATIVE | Observed and described, not quantified with an established metric |
| UNMEASURED | No known study measures it. This is not the same as "false" |

**Verification status** (who checked the primary source):

| Status | Meaning |
|---|---|
| VERIFIED | Primary source opened and checked on 2026-08-05: title, authors and figures match |
| REPORTED | Comes from the literature sweep, primary source not yet opened |

A REPORTED source is not suspect, merely unchecked. It must be promoted to
VERIFIED before it can justify a blocking gate.

## 3. The central finding

> **At the unit level, AI code is equal to or better than human code.
> At the system level, it degrades.**

Cyclomatic complexity: 2.62 for AI against 2.47 for humans, a negligible
difference. AI functions are smaller than human ones. Readability was judged
higher in the GitHub RCT. Duplication measured on real files is lower than in
human code.

On the other side: 22.7% of AI-introduced debt is still alive at the latest
version, runtime dependencies outnumber declared ones by 13.5x, and
architectural decay grows with the volume of code produced.

Two design decisions follow, both non-negotiable.

**First: this is not a code quality framework.** Constraints operate at the
system level, not the line level. The classic metrics — function length,
cyclomatic complexity, nesting — are calibrated on human defects. On AI code
they find nothing while the system decays underneath them.

**Second: constraints must be executable, not written.** Zhu et al. verify
that neither functional correctness nor detailed prompting mitigates
architectural decay. A framework made only of markdown rules has already been
tried and does not work.

## 4. Measured defects

### 4.1 Package hallucination

| Field | Value |
|---|---|
| Category | MEASURED |
| Verification | VERIFIED |
| Source | Spracklen et al., *We Have a Package for You!*, USENIX Security 2025 (peer-reviewed) |
| Replication | Churilov, arXiv 2605.17062 (2026-05), frontier models |
| Metric | Share of non-existent packages among packages referenced |
| Effect, 2025 | 440,445 of 2.23M samples (19.7%); open-source 21.7% against commercial 5.2% |
| Effect, 2026 | 4.62% (Claude Haiku 4.5) to 6.10% (GPT-5.4-mini) across roughly 200k prompts |
| Limits | Python and JavaScript only; temperature-sensitive; the replication covers 5 models |

The rate has dropped three- to fourfold on recent models but is not zero, and
the 2026 replication notes that 53 hallucinated names remain registrable by an
attacker. 43% of hallucinated names recur identically across all ten runs,
which makes slopsquatting a practical attack rather than a theoretical one.

**Maintenance note:** this is the entry that ages fastest. Recheck it whenever
the reference model changes.

### 4.2 Security vulnerabilities

| Field | Value |
|---|---|
| Category | MEASURED |
| Verification | REPORTED |
| Sources | Pearce et al., IEEE S&P 2022 (peer-reviewed); Perry et al., ACM CCS 2023 (peer-reviewed); Veracode 2025 (vendor) |
| Metric | Share of programs or samples with a CWE flagged by static analysis |
| Effect | Pearce: 39.33% of top options and 40.73% of all options across 1,689 programs. Perry: less secure code on 4 of 5 tasks. Veracode: 45% of samples fail |
| Limits | Pearce and Perry use 2021-2022 models, now obsolete; scenarios built to be security-sensitive; no security-oriented prompting |

Three independent sources, two of them peer-reviewed, converge on the 40-45%
band. The convergence matters more than any single number. Perry adds a
finding that shapes the design: developers using an assistant **believe** they
wrote more secure code. Perception moves the wrong way, so the check cannot be
left to the judgement of whoever wrote the code.

The absolute value should be read as an upper bound: the studies use scenarios
chosen to be risky and models that are now several generations old.

### 4.3 Undeclared dependencies

| Field | Value |
|---|---|
| Category | MEASURED |
| Verification | VERIFIED |
| Source | Vangala, Adibifar, Gehani, Malik, arXiv 2512.22387 (AAAI preprint) |
| Metric | Declared packages against packages actually loaded at runtime |
| Effect | 13.5x average multiplier; only 68.3% of projects run out of the box; Python 89.2%, Java 44.0% |
| Limits | 300 projects, 3 agents, controlled environment; synthetic tasks |

Watch the attribution: the initial sweep cited "Bhandari et al."; the actual
first author is **Vangala**.

A clean defect to detect: the gap between manifest and runtime closure is
computable deterministically, with no heuristics involved.

### 4.4 Persistent technical debt

| Field | Value |
|---|---|
| Category | MEASURED |
| Verification | VERIFIED |
| Source | Liu, Widyasari, Zhao, Irsan, Chen, Lo, arXiv 2603.28592 (preprint) |
| Metric | Static analysis before and after commits with a verified AI author |
| Effect | 484,366 issues across 302.6k commits in 6,299 repos; code smells are 89.3%; over 15% of commits from every assistant introduce at least one issue; **22.7% still alive at the latest version** |
| Limits | Only commits with an explicit AI signature in Git, so a self-selected sample; only code-level debt, architectural debt excluded |

The 22.7% figure dictates the gate design: introduced debt **does not get
cleaned up later**. A check that runs downstream arrives too late. It has to
intercept at the moment of introduction, which means on the diff.

### 4.5 Architectural decay scaling with volume

| Field | Value |
|---|---|
| Category | MEASURED (preprint) |
| Verification | VERIFIED |
| Source | Zhu, Tsantalis, Rigby, *AI-Generated Smells*, arXiv 2605.02741 (preprint) |
| Metric | Architectural smells and coupling, across single-file tasks and agent-generated systems |
| Effect | **Volume-Quality Inverse Law**: code volume is a near-perfect predictor of structural degradation. **Reasoning-Complexity Trade-off**: more capable models produce more bloated and coupled code |
| Limits | May 2026 preprint, not yet peer-reviewed; no independent replication |

This is the most important source for the framework, for three claims:

1. Decay **scales with volume**, not with the quality of any individual piece.
2. Better models make the problem worse, not better. Waiting for the next
   model is not a strategy.
3. **Neither functional correctness nor detailed prompting mitigates it.**

Point 3 carries the most weight. It empirically rules out the
instructions-only framework and justifies the cost of executable gates. The
authors conclude that agents need "explicit architectural foresight", which is
the academic phrasing of architecture as a verifiable contract.

The phrase "Modular Mirage" attributed to this paper by the initial sweep
**does not appear in the abstract** and has not been confirmed in the body. Do
not cite it until verified.

### 4.6 Verbosity and nesting

| Field | Value |
|---|---|
| Category | MEASURED, small effect |
| Verification | VERIFIED (title and design; detailed figures REPORTED) |
| Source | Mao, Zhao, Tang, Wang, Zhang, *A Large-Scale Comprehensive Measurement of AI-Generated Code in Real-World Repositories*, arXiv 2603.27130 |
| Metric | PLOC, SLOC, lexical density, nesting depth; 19,816 AI files against 36,467 matched human files |
| Effect | PLOC 256.6 against 192.7; lexical density 0.531 against 0.692 (d = -0.555); max nesting d = 0.172 |
| Limits | The abstract itself concludes that real-world code-level differences are **small**; heuristic detection pipeline |

AI code is wordier and slightly more nested, but the effect sizes run from
negligible to medium. **This does not justify blocking gates.** The only
actionable signal is lexical density: humans compress more, AI dilutes.

## 5. Defects the framework must NOT fight

This section exists to stop useless countermeasures from creeping back in.

| Alleged defect | Why not |
|---|---|
| Overly long functions | AI functions average 138.97 AST nodes against 201.82 for humans. AI writes **smaller** functions |
| Monolithic files | 6.62 functions per file for AI against 11.67 for humans |
| High cyclomatic complexity | 2.62 against 2.47, d = 0.030, negligible. Other studies place AI well below humans |
| Poor readability | GitHub RCT 2024: +3.62% readability, +2.47% maintainability with an assistant |
| Duplication as a primary metric | CONTESTED: GitClear measures +8x across commits, Mao measures 18.69% for AI against 25.89% for humans across files. Different metrics, opposite conclusions |

On duplication: the disagreement is real, not an artefact. GitClear observes
the flow of commits over time, Mao observes the state of files. Both can be
true — code can be duplicated heavily while being written and still end up
less duplicated at rest. Until the contradiction resolves, duplication stays
an observed signal and never a blocking gate.

A cross-cutting note on distribution, which explains why existing tools fall
short: human code has a **heavy tail**, a few enormous functions among many
ordinary ones. AI code is uniformly moderate. Traditional tools hunt for peaks;
on AI code they find none, while the system decays diffusely and invisibly
below those thresholds.

### 5.1 Quality is not consistency

This table rules out micro-quality *judgements*. It does not rule out style
*consistency*, and the two must not be collapsed.

What the evidence refutes is "AI produces low-quality units". What it leaves
open — and partly supports — is "AI produces units that are inconsistent with
each other". Wang et al. (S15) catalogue 24 kinds of stylistic inconsistency
between AI and human code, and the largest single effect in Mao et al. is
lexical density at d = -0.555: humans compress, AI dilutes. That is a
consistency signal, not a quality verdict.

Consistency also carries instrumental weight the quality metrics do not.
Volume and coupling deltas are measured on diffs, so stylistic noise corrupts
them. And cross-session drift (H3) is by definition a consistency problem:
it cannot be detected without a stable baseline to detect it against.

The framework therefore treats style as **hygiene** — delegated to existing
formatters, autofixed, never a judgement — and keeps it out of defect counts.

## 6. Verdict on the five initial hypotheses

| ID | Hypothesis | Verdict | Consequence |
|---|---|---|---|
| H1 | Speculative abstraction | QUALITATIVE, partial support | Protocol, no blocking gate |
| H2 | Monolithic growth | **CONTRADICTED** | Removed from the design |
| H3 | Cross-session drift | **UNMEASURED** | Declared hypothesis; measurement opportunity |
| H4 | Dependency sprawl | **CONFIRMED** | Tier A blocking gate |
| H5 | Missing architectural boundaries | QUALITATIVE, strengthened by Zhu | Tier B gate |

**On H3.** No peer-reviewed study or preprint measures style drift across
successive sessions on the same project. Adjacent but different things exist:
stylistic inconsistency within a single generation (Wang et al. 2025),
ecosystem-level naming trends (Xu et al.), behavioural consistency in
refactoring. None of them is cross-session drift.

It was not looked for and missed; it was not looked for at all, because
measuring it needs longitudinal repositories with session attribution, a kind
of data almost nobody keeps. It remains plausible and is an open research gap.

The framework's detector **can measure it** — convention consistency across
commits over time is computable. Build the instrument without claiming the
finding. If a signal emerges from our own data it will be a contribution;
until then H3 stays labelled as a hypothesis.

## 7. From taxonomy to rules

| Tier | Rule | Source | Enforcement |
|---|---|---|---|
| A | Every package verified against the registry before use | 4.1 | Blocking |
| A | SAST mandatory on AI-contributed diffs | 4.2 | Blocking |
| A | Declared dependencies match the runtime closure | 4.3 | Blocking |
| A | A new external dependency requires explicit justification | 4.3 | Blocking |
| B | Volume control as a leading indicator of decay | 4.5 | Warning |
| B | Architectural boundaries declared and checked against imports | 4.5, H5 | Warning, promotable |
| B | Debt analysis on the diff, at the moment of introduction | 4.4 | Warning |
| C | Convention consistency across sessions | H3 | Measurement only |
| H | Style consistency against the project baseline | §5.1, §4.6 | Delegated, autofixed, excluded from defect counts |

Tier A gates block because they rest on peer-reviewed sources and detect
objective conditions: a package either exists or it does not, a dependency is
either declared or it is not. No judgement calls, no false positives of
substance.

Tier B rests on preprints without independent replication and on thresholds
that need per-project calibration. These stay warnings until we have data of
our own.

Tier H is not a defect tier. It exists because clean style makes the Tier B
and Tier C signals readable, and because the enforcement is free — an
existing formatter fixing its own findings. It never counts as a defect,
which keeps it from polluting the comparison between AI-authored and
human-authored code.

## 8. Invalidation conditions

This taxonomy is dated by construction. Revisit it if:

- Zhu et al. is refuted or fails to replicate: the justification for Tier B falls
- Package hallucination drops below 1% on the reference models: gate 4.1 moves
  from blocking to warning
- A peer-reviewed study on real greenfield projects measures architectural
  boundaries or cross-session drift with d > 0.5: H3 and H5 rise to MEASURED
- The 2026 preprints cited here are published with figures different from
  those reported

**Next review: 2027-02-05**, or sooner if the reference model changes.

## 9. Standing caveats

- Four of the six primary sources are 2025-2026 preprints, not yet peer-reviewed.
- The security studies use 2021-2022 models. The 40-45% band is most likely an
  upper bound for current models.
- Vendor reports carry conflicts of interest in opposite directions: GitClear
  and Veracode benefit from showing problems, GitHub from showing few. Included
  only where the methodology is inspectable, and always labelled.
- The gap between benchmark results and real-repository results is the leading
  cause of opposite conclusions in this literature. State it every time a
  number is cited.
- In Mao et al. many differences are statistically significant yet negligible
  in magnitude, because the sample is enormous. Always read the p-value and
  Cohen's d together.

## 10. Coverage reporting

The findings in this section are not derived from evidence, and saying so is
the point of the section.

Everything above describes defects in AI-generated code, each traced to a
study. A coverage finding describes Plumb instead: it reports that Plumb did
not read something, so that a run which established nothing cannot be mistaken
for a run which found nothing.

They exist because the alternative is worse. A manifest in a format Plumb does
not parse used to yield an empty dependency set, which the gate reported as a
clean result — a silent false negative on a blocking gate, which is the exact
failure this project was built to catch. Reporting the gap is the honest
reading; filing it among the evidence rows would break the project's first rule
in the act of honouring it.

Coverage findings are Tier B. They warn and never block, because a gate that
fails on every project in a format it has not learned yet is uninstalled before
it catches anything.

| Finding | Emitted when |
|---|---|
| `manifest-understood` | A recognised manifest declares dependencies in a form Plumb did not read, or could not be read at all |

A coverage finding reports a gap; it does not close it, and it does not claim
the gap list is complete. Two silences remain and neither is expressible as a
`manifest-understood` finding, because in both cases nothing was read to have a
finding attached to it:

- A file not recognised as a manifest at all. Recognition is by filename, so
  `requirements.in`, `setup.py`, `setup.cfg`, `Pipfile` and an included
  `requirements/base.txt` are never opened.
- A dependency declaration inside a recognised `pyproject.toml` that is not
  spelled `dependencies`. The rule for tool tables keys on that word, so
  `[tool.tox.env_run_base].deps`, `[tool.briefcase.app.<name>].requires`,
  `[tool.cibuildwheel].test-requires`, `[tool.flit.metadata].dev-requires` and
  PEP 725 `[external]` produce no finding. `[build-system].requires` is
  excluded deliberately — it is in nearly every `pyproject.toml`, and a warning
  that always fires is one nobody reads.

Both are silent false negatives on a blocking gate. They are recorded here
rather than in the README alone because the README describes a version and this
describes the rule.

A third silence is a decision rather than a gap, and belongs here for the same
reason `[build-system].requires` does. **Conda dependencies are out of scope.**
The rule in §4.1 is about a package that does not exist in the registry the
project installs from, and Plumb reads one registry, PyPI. Conda tables —
pixi's `dependencies`, `build-`, `host-` and `run-dependencies` at any scope,
and `[tool.conda-lock.dependencies]` — produce no finding, while pixi's
`pypi-dependencies` tables still do.

A hallucinated conda package is a real defect and this is not a claim
otherwise; it is a claim that it needs its own rule with its own evidence and
its own registry. It is excluded rather than warned about because it is the one
gap that could never be closed by writing a parser, so the warning would be
permanent and unactionable — the failure mode the Tier B argument above depends
on avoiding.

This section has no invalidation condition, because it rests on no claim that
new evidence could refute. It goes away when the formats are read.

## 11. Self-consistency

Like §10, the findings in this section describe Plumb's own artefacts, not
defects in the code under inspection, and rest on no evidence row.

The agent instruction files (`CLAUDE.md`, `AGENTS.md`, `.cursorrules`,
`.github/copilot-instructions.md`) are generated from `.plumb/` and declare
so in their first line. Between generations they can drift: someone edits
`.plumb/` and forgets to regenerate, or edits a generated file directly
despite the marker. Either way, the next agent session reads rules that no
longer match the working agreement — rules nobody chose.

| Finding | Emitted when |
|---|---|
| `adapter-drift` | A generated instruction file exists and does not match what `plumb adapters` would produce from the current `.plumb/` |
| `index-stale` | A scanned `.plumb/index.md` no longer matches a fresh scan of the tracked source files |

Tier B: drift is a maintenance fault in this repository, not a defect in the
change under review, and blocking a diff over it would punish the wrong
commit. The remedy is one command, and the finding names it. The same
reasoning covers the index: a stale map misleads the next agent into
re-implementing what already exists, and the remedy is again one command.
