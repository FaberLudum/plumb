# Fonti

**Data di verifica:** 2026-08-05

Stato: **VERIFICATA** = fonte primaria aperta e controllata, titolo, autori e
cifre corrispondono. **RIPORTATA** = proveniente dalla ricerca di letteratura,
fonte primaria non ancora aperta.

Una fonte RIPORTATA va promossa a VERIFICATA prima di poter giustificare un
gate bloccante.

## Verificate

| Rif | Fonte | Tipo | Stato |
|---|---|---|---|
| S1 | Spracklen et al., *We Have a Package for You! A Comprehensive Analysis of Package Hallucinations by Code Generating LLMs*, USENIX Security 2025, pp. 3687-3706 — https://www.usenix.org/conference/usenixsecurity25/presentation/spracklen | Peer-reviewed | VERIFICATA |
| S2 | Churilov, *The Range Shrinks, the Threat Remains: Re-evaluating LLM Package Hallucinations on the 2026 Frontier-Model Cohort*, arXiv 2605.17062 (2026-05-16, rev. 2026-06-11) | Preprint | VERIFICATA |
| S3 | Vangala, Adibifar, Gehani, Malik, *AI-Generated Code Is Not Reproducible (Yet): An Empirical Study of Dependency Gaps in LLM-Based Coding Agents*, arXiv 2512.22387 (2025-12-26, rev. 2026-03-23) | Preprint AAAI | VERIFICATA |
| S4 | Liu, Widyasari, Zhao, Irsan, Chen, Lo, *Debt Behind the AI Boom: A Large-Scale Empirical Study of AI-Generated Code in the Wild*, arXiv 2603.28592 (2026-03-30, rev. 2026-04-26) | Preprint | VERIFICATA |
| S5 | Zhu, Tsantalis, Rigby, *AI-Generated Smells: An Analysis of Code and Architecture in LLM and Agent-Driven Development*, arXiv 2605.02741 (2026-05-04) | Preprint | VERIFICATA |
| S6 | Mao, Zhao, Tang, Wang, Zhang, *A Large-Scale Comprehensive Measurement of AI-Generated Code in Real-World Repositories*, arXiv 2603.27130 (2026-03-28, rev. 2026-07-01) | Preprint | VERIFICATA (titolo, autori, impianto; cifre di dettaglio dal corpo non ancora controllate) |

## Riportate, non ancora verificate

| Rif | Fonte | Tipo | Usata per |
|---|---|---|---|
| S7 | Pearce et al., *Asleep at the Keyboard? Assessing the Security of GitHub Copilot's Code Contributions*, IEEE S&P 2022 | Peer-reviewed | Sicurezza, 39,33% / 40,73% |
| S8 | Perry et al., *Do Users Write More Insecure Code with AI Assistants?*, ACM CCS 2023 | Peer-reviewed | Sicurezza, 4 task su 5 |
| S9 | Veracode, *2025 GenAI Code Security Report* | Report vendor | Sicurezza, 45% |
| S10 | GitClear, *AI Copilot Code Quality* 2025 e 2026 (Harding & Kloster) | Report vendor | Duplicazione e churn (contestati) |
| S11 | DORA, *State of DevOps* 2024 e 2025 | Report industriale | Stabilità di delivery |
| S12 | GitHub, RCT su 202 sviluppatori, 2024 | Report vendor | Evidenza contraria, micro-qualità |
| S13 | METR, RCT 2025 su 16 sviluppatori | Report ricerca | Evidenza contraria, tempi |
| S14 | Watanabe et al. (2026) | Da identificare | 9,9% metodi cancellati in review |
| S15 | Wang et al. (2025), incoerenza stilistica | Da identificare | Contesto su H3 |
| S16 | Xu et al., trend di naming nell'ecosistema | Da identificare | Contesto su H3 |
| S17 | *TODO: Fix the Mess Gemini Created*, arXiv 2601.07786 | Preprint | GIST / SATD, qualitativo |
| S18 | *Mining Architectural Quality Under Agentic AI Adoption*, arXiv 2606.13298 | Preprint | Contesto su H5 |

## Correzioni rispetto alla ricerca iniziale

1. **Attribuzione S3.** Citata come "Bhandari et al.", il primo autore reale è
   **Vangala**.
2. **Titolo S6.** Citato come *A Large-Scale Empirical Study of...*, il titolo
   reale è *A Large-Scale **Comprehensive Measurement** of...*.
3. **"Modular Mirage" (S5).** Attribuito a Zhu et al., **non compare
   nell'abstract**. Non citare finché non verificato nel corpo del paper.
4. **S1 è superata da S2 sui modelli attuali.** Il 19,7% si riferisce a modelli
   di fine 2024. Sui frontier 2026 il tasso è 4,62-6,10%. Citare il 19,7% come
   dato attuale sarebbe scorretto.

## Priorità di verifica

Da promuovere per prime, perché reggono gate bloccanti: **S7, S8** (sicurezza,
Tier A). Poi S14 se si vuole usare il 9,9% per H1.

S9-S13 sono report vendor: usati solo come segnale di contesto, non come
fondamento di regole. Non richiedono promozione.
