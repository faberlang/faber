# Fusion containment: one source, CPU or GPU

This package uses a prescribed toroidal magnetic field and Boris integration to
compare the same particle workload across the reference runner, the multicore
CPU device and Metal. It is an illustrative numerical simulation, not a
plasma-equilibrium solver or a prediction of a fusion device. It is the
headline CPU/GPU clustering example: Faber runs the same particle-lane source
on the CPU worker device or the Metal GPU. This is one host's CPU or one GPU,
not a multi-node/multi-GPU claim. The Latin source files and the existing Latin
README remain below; `src/magna.fab` is the small same-source route example.

## Route example and fixture

From the repository root, the checked fixture is the argument list in
[`fixtures/magna-256.args`](fixtures/magna-256.args), with the retained reference
transcript in [`fixtures/magna-256.expected`](fixtures/magna-256.expected): 256
particles, one batch, starting at particle ID 0. All routes below run the same
Faber source and compare against that expected transcript plus the independent
binary64 oracle.

```sh
cargo build -p faber

target/debug/faber run --device reference corpus-simulations/fusion-containment/src/magna.fab -- 256 1 0
target/debug/faber run --device cpu --threads 4 corpus-simulations/fusion-containment/src/magna.fab -- 256 1 0
target/debug/faber run --device metal corpus-simulations/fusion-containment/src/magna.fab -- 256 1 0

python3 acceptance/fusion-containment/smoke_routes.py --require-metal \
  --output acceptance/fusion-containment/receipts/released-1.17.0-route-smoke-2026-10-30
python3 acceptance/fusion-containment/run_partitions.py \
  --output acceptance/fusion-containment/receipts/partition-audit-public-2026-10-30
```

`--device reference` is the plain sequential oracle route. The CPU and Metal
device routes need the released Faber binary: `crates/faber/src/package/device/`
is excluded from the open-source export. The installed Faber 1.17.0 executable
reported the Metal and CUDA device paths, and the reference, CPU and Metal
fixture runs passed. CUDA was not run on an NVIDIA host and is not claimed for
this example. This checkout does not contain a 1.18.0 released archive, so the
retained route receipt verifies the installed 1.17.0 executable only.

The single timing table below reports end-to-end process wall time, not kernel
time or a portable performance claim. The first three rows use the released 1.17.0 executable and the 256-particle
fixture. The partition-driver rows use the in-tree debug executable and the
same `magna.fab` source with 1,024 particles: they all produced wall-bin counts
`[142, 387, 334, 13, 148]` and matched the sampled binary64 oracle. The
partition driver retained stdout, stderr, stats and a hash-bound receipt.

| Route | Threads / partitions | Input | Process wall time (s) | Retained evidence |
| --- | ---: | --- | ---: | --- |
| `--device reference` | sequential | 256 particles | 2.960 | [release smoke receipt](../../acceptance/fusion-containment/receipts/released-1.17.0-route-smoke-2026-10-30/receipt.json) |
| `--device cpu` | 4 threads | 256 particles | 1.218 | [release smoke receipt](../../acceptance/fusion-containment/receipts/released-1.17.0-route-smoke-2026-10-30/receipt.json) |
| `--device metal` | 1 device | 256 particles | 0.271 | [release smoke receipt](../../acceptance/fusion-containment/receipts/released-1.17.0-route-smoke-2026-10-30/receipt.json) |
| `run_partitions.py`, CPU p1 | 4 threads / 1 partition | 1,024 particles | 5.818 | [partition receipt](../../acceptance/fusion-containment/receipts/partition-audit-public-2026-10-30/receipt.json) |
| `run_partitions.py`, CPU p2 | 4 threads / 2 partitions | 1,024 particles | 5.760 | [partition receipt](../../acceptance/fusion-containment/receipts/partition-audit-public-2026-10-30/receipt.json) |
| `run_partitions.py`, CPU p4 | 4 threads / 4 partitions | 1,024 particles | 5.874 | [partition receipt](../../acceptance/fusion-containment/receipts/partition-audit-public-2026-10-30/receipt.json) |
| `run_partitions.py`, Metal control | 1 device / 1 partition | 1,024 particles | 0.211 | [partition receipt](../../acceptance/fusion-containment/receipts/partition-audit-public-2026-10-30/receipt.json) |

`run_partitions.py`'s measured runs and population checks passed, but its overall
readiness verdict remains **blocked**: the receipt does not establish physical
multi-GPU execution or the required per-buffer span placement/capacity. CPU
logical partitions are local CPU execution only. Do not interpret these rows as
a multi-host or multi-GPU clustering result. The headline demonstration is
that one Faber source runs on the CPU worker device or Metal GPU, with the same
population result—not that partitioning increases hardware count.

The requested English source rendering is currently blocked by the existing
converter refusal: `faber convert --from la --to en --stdout
corpus-simulations/fusion-containment/src/main.fab` returns
`SEM057.call_marker_on_read_only` at the nine mutable output arguments to the
imported `orbita.integra`. The Latin sources are preserved; no translation was
hand-written or substituted for the requested `faber convert` output. This is
the same converter/formatter gap recorded in the acceptance material and needs
a compiler-tooling follow-up, not a Faber language change.

---

# Orbitae particularum in campo magnetico

Experimentum primum: programma Latinum, duae unitates, unus nucleus per
particulam. Campus magneticus in tabula communi 33 × 33 positus est. Nucleus
campum apud locum particulae per interpolationem bilinearem legit. Motus per
rotationem Boris computatur; particula ad primum gradum extra murum sistit.

## Exempla et mensurae

Omnia sunt sine dimensionibus: q/m = 1, R₀ = 3, radius minor = 1,
Δt = 0.05. Utrumque exemplum habet 256 particulas et summum 1,000 gradus.
Initia determinata sunt; velocitates ad tempus dimidii gradus pertinent.
Initia omnia apud y = z = 0 sunt, non distributio isotropa.

- `uniform`: Bᵣ = Bφ = 0, Bz = 1; exemplum ad gyrationem probandam.
- `torus`: Bᵣ = −0.3 z/R, Bφ = 3/R, Bz = 0.3(R−3)/R.

Campus secundus praescriptus est. Non est aequilibrium ex aequatione
Grad–Shafranov computatum. Murus est (R−3)² + z² = 1. Collisiones, campus
electricus, energia 3.5 MeV et turbatio desunt. Experimentum machinam veram
non praedicit.

Referentia analytica separata: [`EQUILIBRIUM.md`](EQUILIBRIUM.md) describit
campum Grad–Shafranov exactum, probationes Python et mensuras reproducibiles.
[`BIRTH_POPULATION.md`](BIRTH_POPULATION.md) definit fontem idealizatum
reproducibilem, contractum seminum/ID, fixture et probationem qualitatum.
Haec referentia exempla `uniform` et `torus` non mutat.

## Usus

Ex radice repositorii:

```sh
cargo build -p faber
target/debug/faber check corpus-simulations/fusion-containment --device metal
target/debug/faber run corpus-simulations/fusion-containment --device metal
python3 acceptance/fusion-containment/run_spike.py --output /private/tmp/fusion-orbit-proof
```

Via `--device metal` GPU expresse postulat. Via `--device cpu --threads 4`
idem opus in CPU probat. Probatio Python bibliotheca ordinaria sola eget.
Directorium exitus novum esse debet. Optio `--build` compilatorem aedificat.

## Experimentum maius

```sh
target/debug/faber run corpus-simulations/fusion-containment/src/magna.fab --device metal -- 1048576 1 0
python3 acceptance/fusion-containment/run_scale.py --output /private/tmp/fusion-orbitae-magnae
```

Argumenta sunt particulae per gregem, numerus gregum, et primus numerus
particulae. `run_scale.py` primum GPU metitur, deinde saltem unum secundum
operis GPU colligit. Si probationes valent, triginta secunda addita colligit.
Unus grex continet 1,048,576 particulas; usque ad 32 greges in una vocatione.
Tempus GPU ex mensuris singulorum nucleorum sumitur, non ex tempore hospitis.

Semina in GPU fiunt. Post omnem periodum 17,809,936 numerorum, distantia
radialis initialis 0.001 additur, ne status initiales iterentur. Tempus
physicum cuiusque orbitae manet 1,000 gradus: hoc est experimentum plurium
particularum, non orbitarum longiorum.

Census totius gregis in GPU computatur. Octo orbitae per gregem cum referentia
Python conferuntur. Haec comparatio exempla probat, non omnes particulas
independenter. Exitus et mensurae cuiusque vocationis in directorio servato sunt.

## Eadem programma in CPU

```sh
target/debug/faber run corpus-simulations/fusion-containment/src/magna.fab --device cpu --threads 8 -- 1024 1 0
python3 acceptance/fusion-containment/run_scale.py --device cpu --threads 8 --particles 1024 --output /private/tmp/fusion-cpu
python3 acceptance/fusion-containment/run_cpu.py --workers 1 2 4 8 --particles 1024 --output /private/tmp/fusion-cpu-probatio
```

Codex Faber idem manet. `--device cpu` eligit CPU; `--threads` numerum
operariorum eligit. Ansae `filum` easdem particulas inter operarios dividunt.
Optio `--device reference` sequitur viam simplicem, non hanc viam concurrentem.

In CPU `run_scale.py` numerum fixum particularum et gregum probat, non tempus
GPU. Optiones `--particles`, `--batches` et `--offset` pertinent ad CPU;
via Metal adhuc tempora unius et triginta secundorum probat.
`run_cpu.py` comparat exitus et tempora apud numeros operariorum diversos.
Ratio temporis CPU ad tempus elapsum usum plurium nucleorum demonstrat.

## Auditio partitionum

Eadem particulae 1,024 et idem numerus operariorum CPU (4) probantur cum
partitionibus 1, 2 et 4; adest etiam experimentum Metal sine partitione et
probatio Metal duarum partitionum logicarum ut metadata `device.runtime.span`
reperiatur. Ex radice repositorii, post `cargo build -p faber`:

```sh
python3 acceptance/fusion-containment/run_partitions.py \\
  --output acceptance/fusion-containment/receipts/partition-audit-YYYY-MM-DD
```

Directorium novum continet stdout, stderr, mensuras cuiusque vocationis et
`receipt.json` cum hash, verificatum per `receipt.sha256`. Partitio logica CPU
est tantum substitutum locale pro grege CPU; non est probatio plurium
hospitum neque GPU physicaliter multiplicitatis. Census particularum et
ordines muri aequaliter exacti esse debent; summa graduum `f32` admittit
errorem absolutum 0.5 gradus. Omnis transcriptio etiam contra exempla
referentiae `binary64` verificatur. Defectus probationis partitionum vel
locorum memoriae exitum falsum producit, sed receptaculum mensurae servatur.

`partition_compare.py` et `test_partition_compare.py` probationes metadatorum
span et census continent. Candidati munerum ex magnitudine memoriae tantum
indicantur: plures tensores `[N]` eiusdem magnitudinis sunt, et schema
mensurarum hodiernum IDs bufferorum ad bindinges Faber non coniungit. Relatio
cum sententia de promptitudine segmentorum in `PARTITION_AUDIT.md` est.

Exitus CSV habet locum, velocitatem, gradum ultimum et errorem energiae.
Numerus muri: 0 intus, 1 extra/supra, 2 intra/supra, 3 intra/infra,
4 extra/infra. “Extra” et “intra” hic R ≥ 3 et R < 3 significant.
Locus scriptus est post transitum muri, non punctum exactum intersectionis.

Comparatio postulat eandem fractionem intra murum et easdem quattuor
fractiones ictuum, errore unius particulae permisso in singulis ordinibus.
Error relativus energiae debet esse ≤ 10⁻⁴. Distantia locorum ultimorum
refertur, sed non est lex comparationis orbitarum longarum.

## Convergentia temporis ad horizonem fixum

`run_convergence.py` est studium finitum et reproducibile: 256 particulae,
eadem tabula 33 × 33, `dt = 0.05, 0.025, 0.0125`, atque horizon toroidalis
`T = 50` exactus (1,000, 2,000, 4,000 gradus). Singulus gradus mittitur in
CPU octo operariis et in una machina Metal; nullus numerus particularum,
reticuli, campi vel distributionis iactus generalizatur. Exemplum campi
uniformis sine muro habet `T = 2` et solutionem continuam analyticam.

Velocitas initialis declarata est `v(0)` physica. Pro quolibet `dt`, nucleus
conversionem inversam dimidii anguli gyrationis Boris, non simplicem
`dt/2`-push, facit ut velocitatem interpositam `v(-dt/2)` obtineat. Situs
incipit apud `t=0`; situs redditus est ad `T`, velocitas autem interposita ad
`T-dt/2`. Probatio uniformis errores positionis et phaseos refert, atque
ante experimentum ordinem secundum cum rationibus inter 3 et 5 (per
reductionem dimidiae magnitudinis) definit.

Usus a radice repositorii; directorium exitus novum esse debet:

```sh
python3 acceptance/fusion-containment/run_convergence.py --output /private/tmp/fusion-convergence
```

`receipt.json` continet hashes fontium et cuiusque exitus, horizontes, errores
analyticos, census reliquorum et fractiones omnium quinque generum muri, ac
Differentias temporum evasionis ad `dt=0.0125`. Discrepantiae backend CPU/Metal
et discrepantiae utriusque cum referentia binaria64 separatam sectionem habent;
non sunt confundendae cum mutatione temporis. `receipt.sha256` signat
recipissum JSON. Exitus CSV singulorum processuum in directorio dati manent;
quisque sub 256 KiB est. Mensura huius mutationis et probatio veteris
imperii servantur in `receipts/convergence-study-2026-10-30/`, cum SHA-256.

Proprietas `dt=0.05` pro observabilibus toroidalibus non praesumitur:
fractiones reliquorum et quinque generum muri, necnon differentiae temporum
evasionis ad optimam resolutionem, referuntur. In mensura servata fractiones
muri eadem manent; maximum discrimen temporis inter communes evasiones est
unus gradus grossioris resolutionis (`0.05`). Ergo `dt=0.05` sufficit ad has
fractiones in hoc exemplo, sed non ad tempora evasionis subtiliora quam unus
gradus. Hoc iudicium observatis innititur, non limine ad exitum accommodato.

## Examen aequilibrii exati nativi

`src/aequilibrium.fab` est portus Latinus campi aequilibrii exati. Tabula
33 × 33 in hospite per f64 construitur (a = 1/32, b = 1/8, R0 = 3, F0 = 3,
iuxta `EQUILIBRIUM.md`) et semel ad f32 rotundatur; deinde omnes 1,089 nodi
per `nota` scribuntur, ut Python possit contra oraculum independentem
comparare. Nucleus integrationis et murus absorbens veteres (`orbita.fab`)
manent; solum tabula campi mutatur. Populatio 256 particularum eadem est
atque in exemplo veteri; initia sunt v(0) physica, quae nucleus, more operis
convergentiae, conversione inversa dimidii anguli Boris ad v(−dt/2) pervertit.
Tempus gradus 0.05, horizon 1,000 graduum (T = 50).

Haec populatio examen moderationis est, non distributio plasma scientifica:
experimentum machinam veram non praedicit. Aequilibrium non solvitur (campus
datus ex familia exata est), physica muri non mutatur, neque exempla
`uniform`/`torus`, receptacula scalae aut opus scalae default tanguntur.

Usus a radice repositorii; directorium exitus novum esse debet:

```sh
python3 acceptance/fusion-containment/run_equilibrium.py --output /private/tmp/fusion-aequilibrium-proof
```

Directorium novum retinet imperia, identitates backend, hashes fontium et
binarii, stdout, stats et comparationes numericas utriusque backend (CPU octo
operariis et Metal). Omnes 1,089 nodi contra oraculum binary64 semel ad f32
rotundatum comparantur sub lege exposita: `nota` valorem f32 octo digitis
significantibus scribit, ergo unus ULP binary32 (dimidius ULP pro
decimalizatione impressa et dimidius ULP pro una rotatione hospitis)
admittitur; index Python `[z][r]` contra Faber `[r, z, component]` explicite
convertitur, et probationes comparationis tabulam transpositam et Br reversum
repellunt. Maxima pars admissa mensa: 0.47 ULP.

Omnis 256 ordines orbitarum per backend examinantur: status finitus,
consistentia terminationis et muri, energia (limites 10⁻⁴) et discrepantia a
referentia binary64, scilicet differentiae classium muri ≤ 2, differentia
gradus evasionis ≤ 1, locus ultimus — si graduatione evasionis differunt, ad
tempus commune (prior gradus, reflexo transitu ulterioris cum velocitate
dimidii gradus reddita) — tolerantia 5 × 10⁻⁴ comparatur. `receipt.sha256`
recipissum JSON signat. Mensura huius mutationis servatur in
`receipts/equilibrium-control-2026-10-30/`, cum SHA-256. Probationes
comparationis (`test_equilibrium_compare.py`) sine compilatore currunt.

## Fides et fines

Referentia separatim scripta est in Python, numeris binary64; nucleus utitur
`f32`, tabula in hospite per `f64` constructa et semel rotundata. Probationes
referentiae gyrationem discretam analyticam, motum parallelum, interpolationem
et conservationem energiae examinant.

Forma rotationis et tempora dimidii gradus sequuntur aequationes (3)–(4):
[Zafar et Khan, *Energy behavior of Boris algorithm*, 2021](https://doi.org/10.1088/1674-1056/abd161).
Hic campus electricus nullus est; conservatio energiae hic tantum affirmatur.

`GAPS.md` describit impedimenta instrumentorum. `receipts/` servat mensuras
locales. Hoc experimentum neque verificatorem omnium linguarum neque
solutorem aequilibrii neque probationem CUDA complet. In porta cotidiana
nondum inclusum est.
