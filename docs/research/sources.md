# Sources

**Verification date:** 2026-08-05

Status: **VERIFIED** = primary source opened and checked; title, authors and
figures match. **REPORTED** = came from the literature sweep, primary source
not yet opened.

A REPORTED source must be promoted to VERIFIED before it can justify a
blocking gate.

## Verified

| Ref | Source | Type | Status |
|---|---|---|---|
| S1 | Spracklen et al., *We Have a Package for You! A Comprehensive Analysis of Package Hallucinations by Code Generating LLMs*, USENIX Security 2025, pp. 3687-3706 — https://www.usenix.org/conference/usenixsecurity25/presentation/spracklen | Peer-reviewed | VERIFIED |
| S2 | Churilov, *The Range Shrinks, the Threat Remains: Re-evaluating LLM Package Hallucinations on the 2026 Frontier-Model Cohort*, arXiv 2605.17062 (2026-05-16, rev. 2026-06-11) | Preprint | VERIFIED |
| S3 | Vangala, Adibifar, Gehani, Malik, *AI-Generated Code Is Not Reproducible (Yet): An Empirical Study of Dependency Gaps in LLM-Based Coding Agents*, arXiv 2512.22387 (2025-12-26, rev. 2026-03-23) | AAAI preprint | VERIFIED |
| S4 | Liu, Widyasari, Zhao, Irsan, Chen, Lo, *Debt Behind the AI Boom: A Large-Scale Empirical Study of AI-Generated Code in the Wild*, arXiv 2603.28592 (2026-03-30, rev. 2026-04-26) | Preprint | VERIFIED |
| S5 | Zhu, Tsantalis, Rigby, *AI-Generated Smells: An Analysis of Code and Architecture in LLM and Agent-Driven Development*, arXiv 2605.02741 (2026-05-04) | Preprint | VERIFIED |
| S6 | Mao, Zhao, Tang, Wang, Zhang, *A Large-Scale Comprehensive Measurement of AI-Generated Code in Real-World Repositories*, arXiv 2603.27130 (2026-03-28, rev. 2026-07-01) | Preprint | VERIFIED (title, authors, design; detailed figures from the body not yet checked) |

## Reported, not yet verified

| Ref | Source | Type | Used for |
|---|---|---|---|
| S7 | Pearce et al., *Asleep at the Keyboard? Assessing the Security of GitHub Copilot's Code Contributions*, IEEE S&P 2022 | Peer-reviewed | Security, 39.33% / 40.73% |
| S8 | Perry et al., *Do Users Write More Insecure Code with AI Assistants?*, ACM CCS 2023 | Peer-reviewed | Security, 4 of 5 tasks |
| S9 | Veracode, *2025 GenAI Code Security Report* | Vendor report | Security, 45% |
| S10 | GitClear, *AI Copilot Code Quality* 2025 and 2026 (Harding & Kloster) | Vendor report | Duplication and churn (contested) |
| S11 | DORA, *State of DevOps* 2024 and 2025 | Industry report | Delivery stability |
| S12 | GitHub, RCT with 202 developers, 2024 | Vendor report | Contrary evidence, micro-quality |
| S13 | METR, 2025 RCT with 16 developers | Research report | Contrary evidence, task time |
| S14 | Watanabe et al. (2026) | To identify | 9.9% of methods deleted in review |
| S15 | Wang et al. (2025), stylistic inconsistency | To identify | Context for H3 |
| S16 | Xu et al., ecosystem naming trends | To identify | Context for H3 |
| S17 | *TODO: Fix the Mess Gemini Created*, arXiv 2601.07786 | Preprint | GIST / SATD, qualitative |
| S18 | *Mining Architectural Quality Under Agentic AI Adoption*, arXiv 2606.13298 | Preprint | Context for H5 |

## Corrections to the initial sweep

1. **Attribution of S3.** Cited as "Bhandari et al."; the actual first author is
   **Vangala**.
2. **Title of S6.** Cited as *A Large-Scale Empirical Study of...*; the real
   title is *A Large-Scale **Comprehensive Measurement** of...*.
3. **"Modular Mirage" (S5).** Attributed to Zhu et al., but **absent from the
   abstract**. Do not cite until verified in the body of the paper.
4. **S1 is superseded by S2 for current models.** The 19.7% figure refers to
   late-2024 models. On 2026 frontier models the rate is 4.62-6.10%. Citing
   19.7% as current would be wrong.

## Verification priority

Promote these first, because they carry blocking gates: **S7, S8** (security,
Tier A). Then S14 if the 9.9% figure is to be used for H1.

S9-S13 are vendor reports, used only as contextual signal and never as the
foundation of a rule. They need no promotion.
