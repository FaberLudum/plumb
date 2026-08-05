# Tassonomia empirica dei difetti del codice generato da LLM

**Versione:** 1.0
**Data:** 2026-08-05
**Stato:** base di evidenza per lo Strato 0 del framework

---

## 1. A cosa serve questo documento

Questo è il fondamento empirico del framework. Ogni regola dello Strato 0 deve
essere tracciabile a una riga di questa tassonomia. **Regola senza fonte, regola
che non entra.**

Il documento serve anche al contrario: elenca i difetti che il framework
**non** deve combattere, perché i dati dicono che non esistono o che il codice
AI se la cava meglio dell'umano. Enforcement sprecato su un non-difetto costa
attrito e credibilità.

## 2. Metodo

Ogni voce riporta fonte citabile, metrica, dimensione dell'effetto e limiti.
Le fonti sono classificate su due assi indipendenti.

**Forza dell'evidenza:**

| Categoria | Significato |
|---|---|
| MISURATO | Dato pubblicato con metrica esplicita e campione dichiarato |
| CONTESTATO | Misurato, ma un'altra fonte con metrica diversa conclude l'opposto |
| QUALITATIVO | Osservato e descritto, non quantificato con una metrica consolidata |
| NON MISURATO | Nessuno studio noto lo misura. Non equivale a "falso" |

**Stato di verifica** (chi ha controllato la fonte primaria):

| Stato | Significato |
|---|---|
| VERIFICATA | Fonte primaria aperta e controllata il 2026-08-05: titolo, autori e cifre corrispondono |
| RIPORTATA | Proviene dalla ricerca di letteratura, fonte primaria non ancora aperta |

Una fonte RIPORTATA non è sospetta, è solo non ancora controllata. Prima di
usarla per giustificare un gate bloccante va promossa a VERIFICATA.

## 3. Il risultato centrale

> **A livello di unità il codice AI è pari o migliore di quello umano.
> A livello di sistema degrada.**

Complessità ciclomatica: 2,62 AI contro 2,47 umana, differenza trascurabile.
Funzioni AI più piccole di quelle umane. Leggibilità giudicata superiore
nell'RCT GitHub. Duplicazione su file reali più bassa che nel codice umano.

Sull'altro versante: il 22,7% del debito introdotto dall'AI è ancora vivo
all'ultima versione, le dipendenze caricate a runtime sono 13,5 volte quelle
dichiarate, e il decadimento architetturale cresce col volume di codice
prodotto.

Da qui discendono due decisioni di progetto, entrambe non negoziabili.

**Prima: questo non è un framework di code quality.** I vincoli operano a
livello di sistema, non di riga. Le metriche classiche — lunghezza funzione,
complessità ciclomatica, nesting — sono tarate sui difetti umani e sul codice
AI non trovano nulla mentre il sistema si degrada.

**Seconda: i vincoli devono essere eseguibili, non scritti.** Zhu et al.
verificano che né la correttezza funzionale né un prompting dettagliato
mitigano il decadimento architetturale. Un framework fatto di sole regole in
markdown è già stato provato e non funziona.

## 4. Difetti misurati

### 4.1 Package hallucination

| Campo | Valore |
|---|---|
| Categoria | MISURATO |
| Verifica | VERIFICATA |
| Fonte | Spracklen et al., *We Have a Package for You!*, USENIX Security 2025 (peer-reviewed) |
| Replica | Churilov, arXiv 2605.17062 (2026-05), modelli frontier |
| Metrica | Percentuale di pacchetti inesistenti sui pacchetti citati |
| Effetto 2025 | 440.445 su 2,23M campioni (19,7%); open-source 21,7% contro commerciali 5,2% |
| Effetto 2026 | 4,62% (Claude Haiku 4.5) – 6,10% (GPT-5.4-mini) su circa 200k prompt |
| Limiti | Python e JavaScript; sensibile alla temperatura; la replica copre 5 modelli |

Il tasso è calato di 3-4 volte sui modelli recenti ma non è a zero, e la
replica 2026 segnala che 53 nomi allucinati restano registrabili da un
attaccante. Il 43% dei nomi allucinati si ripete identico su tutte e dieci le
run, il che rende lo slopsquatting un attacco praticabile e non teorico.

**Nota di manutenzione:** questa è la voce che invecchia più in fretta. Va
ricontrollata a ogni cambio di modello di riferimento.

### 4.2 Vulnerabilità di sicurezza

| Campo | Valore |
|---|---|
| Categoria | MISURATO |
| Verifica | RIPORTATA |
| Fonti | Pearce et al., IEEE S&P 2022 (peer-reviewed); Perry et al., ACM CCS 2023 (peer-reviewed); Veracode 2025 (vendor) |
| Metrica | Percentuale di programmi o campioni con CWE rilevata da analisi statica |
| Effetto | Pearce: 39,33% delle opzioni top e 40,73% del totale su 1.689 programmi. Perry: codice meno sicuro su 4 task su 5. Veracode: 45% dei campioni fallisce i test |
| Limiti | Pearce e Perry usano modelli 2021-2022, oggi obsoleti; scenari costruiti per essere sensibili; nessun prompt orientato alla sicurezza |

Tre fonti indipendenti, di cui due peer-reviewed, convergono nella fascia
40-45%. La convergenza vale più dei singoli numeri. Perry aggiunge un dato che
conta per il design: chi usa l'assistente **crede** di aver scritto codice più
sicuro. La percezione va nella direzione sbagliata, quindi il controllo non può
essere lasciato al giudizio di chi scrive.

Il valore assoluto va però trattato come limite superiore: gli studi usano
scenari scelti per essere rischiosi e modelli vecchi.

### 4.3 Dipendenze non dichiarate

| Campo | Valore |
|---|---|
| Categoria | MISURATO |
| Verifica | VERIFICATA |
| Fonte | Vangala, Adibifar, Gehani, Malik, arXiv 2512.22387 (preprint AAAI) |
| Metrica | Pacchetti dichiarati contro pacchetti effettivamente caricati a runtime |
| Effetto | Moltiplicatore medio 13,5x; solo il 68,3% dei progetti esegue senza interventi; Python 89,2%, Java 44,0% |
| Limiti | 300 progetti, 3 agent, ambiente controllato; task sintetici |

Attenzione all'attribuzione: la ricerca iniziale citava "Bhandari et al.", il
primo autore reale è **Vangala**.

Difetto pulito da rilevare: la distanza fra manifest e closure runtime è
calcolabile in modo deterministico, senza euristiche.

### 4.4 Debito tecnico persistente

| Campo | Valore |
|---|---|
| Categoria | MISURATO |
| Verifica | VERIFICATA |
| Fonte | Liu, Widyasari, Zhao, Irsan, Chen, Lo, arXiv 2603.28592 (preprint) |
| Metrica | Analisi statica prima e dopo commit con autore AI verificato |
| Effetto | 484.366 issue su 302,6k commit in 6.299 repo; code smell l'89,3%; oltre il 15% dei commit di ogni assistente introduce almeno una issue; **22,7% ancora vivo all'ultima versione** |
| Limiti | Solo commit con firma AI esplicita in Git, quindi campione auto-selezionato; solo debito a livello di codice, esclude quello architetturale |

Il 22,7% è il numero che detta il design del gate: il debito introdotto **non
viene ripulito dopo**. Un controllo che gira a valle arriva tardi. Deve
intercettare al momento dell'introduzione, cioè sul diff.

### 4.5 Decadimento architetturale col volume

| Campo | Valore |
|---|---|
| Categoria | MISURATO (preprint) |
| Verifica | VERIFICATA |
| Fonte | Zhu, Tsantalis, Rigby, *AI-Generated Smells*, arXiv 2605.02741 (preprint) |
| Metrica | Smell architetturali e accoppiamento, su task a file singolo e su sistemi generati da agent |
| Effetto | **Volume-Quality Inverse Law**: il volume di codice è un predittore quasi perfetto del degrado strutturale. **Reasoning-Complexity Trade-off**: modelli più capaci producono codice più gonfio e accoppiato |
| Limiti | Preprint maggio 2026, non ancora peer-reviewed; nessuna replica indipendente |

È la fonte più importante per il framework, per tre affermazioni:

1. Il degrado **scala col volume**, non con la qualità del singolo pezzo.
2. Modelli migliori peggiorano il problema, non lo risolvono. Aspettare il
   prossimo modello non è una strategia.
3. **Né la correttezza funzionale né il prompting dettagliato lo mitigano.**

Il punto 3 è il più consequenziale. Esclude empiricamente il framework fatto di
sole istruzioni e giustifica il costo dei gate eseguibili. Gli autori concludono
che serve dotare gli agent di "explicit architectural foresight", che è la
formulazione accademica dell'architettura come contratto verificabile.

L'affermazione "Modular Mirage" attribuita a questo paper dalla ricerca
iniziale **non compare nell'abstract** e non è stata confermata nel corpo. Non
va citata finché non è verificata.

### 4.6 Verbosità e nesting

| Campo | Valore |
|---|---|
| Categoria | MISURATO, effetto piccolo |
| Verifica | VERIFICATA (titolo e impianto; cifre di dettaglio RIPORTATE) |
| Fonte | Mao, Zhao, Tang, Wang, Zhang, *A Large-Scale Comprehensive Measurement of AI-Generated Code in Real-World Repositories*, arXiv 2603.27130 |
| Metrica | PLOC, SLOC, densità lessicale, profondità di nesting; 19.816 file AI contro 36.467 file umani abbinati |
| Effetto | PLOC 256,6 contro 192,7; densità lessicale 0,531 contro 0,692 (d = -0,555); nesting massimo d = 0,172 |
| Limiti | L'abstract stesso conclude che le differenze code-level nel mondo reale sono **piccole**; pipeline di detection euristica |

Il codice AI è più prolisso e leggermente più annidato, ma gli effect size sono
da trascurabili a medi. **Non giustifica gate bloccanti.** L'unico segnale
azionabile è la densità lessicale: gli umani comprimono di più, l'AI diluisce.

## 5. Difetti che il framework NON deve combattere

Questa sezione esiste per impedire di reintrodurre contromisure inutili.

| Presunto difetto | Perché no |
|---|---|
| Funzioni troppo lunghe | Funzioni AI 138,97 nodi AST contro 201,82 umani. L'AI scrive funzioni **più piccole** |
| File monolitici | 6,62 funzioni per file AI contro 11,67 umane |
| Complessità ciclomatica alta | 2,62 contro 2,47, d = 0,030, trascurabile. Altri studi trovano l'AI nettamente sotto l'umano |
| Bassa leggibilità | RCT GitHub 2024: +3,62% leggibilità, +2,47% manutenibilità con assistente |
| Duplicazione come metrica primaria | CONTESTATO: GitClear misura +8x sui commit, Mao misura 18,69% AI contro 25,89% umano sui file. Metriche diverse, conclusioni opposte |

Sulla duplicazione: il disaccordo è reale e non è un artefatto. GitClear
osserva il flusso dei commit nel tempo, Mao lo stato dei file. Possono essere
vere entrambe — si può duplicare molto durante la scrittura e avere comunque
meno duplicazione a riposo. Finché la contraddizione non si scioglie, la
duplicazione resta un segnale osservato, mai un gate bloccante.

Nota trasversale sulla distribuzione, utile a capire perché i linter esistenti
non bastano: il codice umano ha una **coda pesante**, poche funzioni enormi in
mezzo a molte normali. Il codice AI è uniformemente moderato. Gli strumenti
tradizionali cercano i picchi, e sul codice AI non ne trovano, mentre il
sistema si degrada in modo diffuso e invisibile a quelle soglie.

## 6. Verdetto sulle cinque ipotesi iniziali

| ID | Ipotesi | Verdetto | Conseguenza |
|---|---|---|---|
| H1 | Astrazione speculativa | QUALITATIVO, supporto parziale | Protocollo, nessun gate bloccante |
| H2 | Crescita monolitica | **CONTRADDETTA** | Rimossa dal design |
| H3 | Deriva tra sessioni | **NON MISURATA** | Ipotesi dichiarata; opportunità di misura |
| H4 | Dependency sprawl | **CONFERMATA** | Gate bloccante Tier A |
| H5 | Confini architetturali assenti | QUALITATIVO, rafforzato da Zhu | Gate Tier B |

**Su H3.** Nessuno studio peer-reviewed né preprint misura la deriva di stile
fra sessioni successive sullo stesso progetto. Esistono cose vicine ma diverse:
l'incoerenza stilistica su singola generazione (Wang et al. 2025), i trend di
naming a livello di ecosistema (Xu et al.), la consistenza nel refactoring.
Nessuna è la deriva cross-sessione.

Non è stata cercata e non trovata: non è stata cercata affatto, perché servono
repository longitudinali con attribuzione di sessione, un dato che quasi nessuno
conserva. Resta plausibile ed è un buco di ricerca aperto.

Il detector del framework **può misurarla** — la coerenza delle convenzioni fra
commit nel tempo è calcolabile. Costruiamo lo strumento senza rivendicare il
risultato. Se dai nostri dati emerge un segnale, sarà un contributo; finché non
emerge, H3 resta etichettata come ipotesi.

## 7. Da tassonomia a regole

| Tier | Regola | Fonte | Enforcement |
|---|---|---|---|
| A | Ogni pacchetto verificato nel registry prima dell'uso | 4.1 | Bloccante |
| A | SAST obbligatorio sui diff a contributo AI | 4.2 | Bloccante |
| A | Dipendenze dichiarate coincidono con la closure runtime | 4.3 | Bloccante |
| A | Nuova dipendenza esterna richiede giustificazione esplicita | 4.3 | Bloccante |
| B | Controllo del volume come indicatore anticipatore di degrado | 4.5 | Warning |
| B | Confini architetturali dichiarati e verificati sugli import | 4.5, H5 | Warning, promuovibile |
| B | Analisi del debito sul diff, al momento dell'introduzione | 4.4 | Warning |
| C | Coerenza delle convenzioni fra sessioni | H3 | Solo misura |

I gate Tier A sono bloccanti perché poggiano su fonti peer-reviewed e rilevano
condizioni oggettive: un pacchetto esiste o non esiste, una dipendenza è
dichiarata o non lo è. Nessun giudizio, nessun falso positivo di merito.

I Tier B poggiano su preprint non ancora replicati e su soglie che vanno
calibrate per progetto. Restano warning finché non abbiamo dati nostri.

## 8. Condizioni di invalidamento

Questa tassonomia è datata per costruzione. Va rivista se:

- Zhu et al. viene smentito o non replicato: cade la giustificazione dei Tier B
- Il tasso di package hallucination scende sotto l'1% sui modelli di
  riferimento: il gate 4.1 passa da bloccante a warning
- Uno studio peer-reviewed su progetti greenfield reali misura i confini
  architetturali o la deriva cross-sessione con d > 0,5: H3 e H5 salgono a
  MISURATO
- I preprint del 2026 qui citati vengono pubblicati con numeri diversi da
  quelli riportati

**Prossima revisione: 2027-02-05**, o prima se cambia il modello di riferimento.

## 9. Avvertenze permanenti

- Quattro fonti su sei sono preprint 2025-2026 non ancora peer-reviewed.
- Gli studi di sicurezza usano modelli 2021-2022. Il 40-45% è verosimilmente
  un limite superiore per i modelli attuali.
- I report vendor hanno conflitti d'interesse in direzioni opposte: GitClear e
  Veracode hanno interesse a mostrare problemi, GitHub a mostrarne pochi.
  Inclusi solo dove la metodologia è ispezionabile, sempre etichettati.
- La differenza fra risultati su benchmark e risultati su repository reali è la
  prima causa di conclusioni opposte in questa letteratura. Va esplicitata ogni
  volta che si cita un numero.
- In Mao et al. molte differenze sono statisticamente significative ma
  trascurabili come magnitudine, per via del campione enorme. Leggere sempre
  p-value e Cohen's d insieme.
