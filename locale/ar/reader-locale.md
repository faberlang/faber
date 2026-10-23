# Arabic Reader Locale — Review Conventions And Per-Row Findings

**Pack:** `locale/ar/pack.toml` (id `ar`, schema_version 1).
**Status:** review only. This file proposes; it edits no pack row. Adopting any
row below is a later pack change with `la`/`en` parity in the same commit.
**Sources:** `locale/ar/pack.toml` (`[keywords]` at line 5, 159 rows; `[types]` at
line 170, 47 rows), the concepts in `locale/en/pack.toml` and `locale/la/pack.toml`,
`docs/design/llm-locale.md` (concept, part of speech, rationale),
`docs/stdlib/morphologia.md` (verb form, Axis A and Axis B), and
`docs/design/reader-locale.md` (what a pack row is).

Operator ruling applied throughout (2026-10-11, language-consistency audit
item 3): a locale pack row must be the semantically accurate translation of the
concept, and being the compiled spelling does not make it right.

## Conventions

**(a) Part of speech and verb form.** A row must be one lexical unit whose part
of speech matches the concept its Latin key carries, and the verb form must
match the role, not the Latin tense:

- declaration and type rows are nouns (`functio` دالة, `genus` صنف, `typus` نوع,
  `textus` نص);
- binding, modifier and attribute rows are adjectives matched to the noun they
  describe (`fixum` ثابت, `varia` قابل_للتغيير, `immutata` مقفلة, `fragilis` هش),
  or nouns when the concept is a name (`errata` أخطاء, `temporis` مهلة);
- statement rows that command an action are **imperative** verbs
  (`importa` استورد, `fac` افعل, `cape` التقط, `rumpe` اكسر);
- statement rows that introduce rather than command are **particles or
  jussives** (`si` إذا, `secus` وإلا, `sit` ليكن);
- predicate rows that state a property are **present indicative**, and this is
  deliberate, not drift (`iacit` يرمي says the function throws; `requirit` still
  takes the imperative because its twin `adfirma` is a command);
- async rows mark the future on Axis A (`fiet`, `fiunt`, `fient`,
  `postparabit`, `praeparabit`, `incipiet`), because morphologia makes sync
  present or imperative and async future; nothing on Axis B (Imperativus mutates,
  Perfectum returns a new value) is a statement keyword, so no statement row
  takes a Perfectum form.

This pack mixes forms today: `reice` (line 60) is the imperative ارفض while the
concept is the negated-guard particle (en `reject`, used as `reice cond iace
err`); `tacet` (line 52) is صمت, the past tense *he fell silent*; `ergo` (line 42)
is إذن, primarily the noun *permission*; `requirit` (line 59) is the present
indicative يتطلب where its twin `adfirma` is imperative; `probandum` (line 126) is
the noun مختبر *laboratory* where the concept is a describing block. The rule then
applied to every row is the bullet list above: a row keeps the form its concept
family demands, and the finding line says which form that is.

**(b) Register.** Modern Standard Arabic is the register, without diacritics
except where a diacritic alone distinguishes the word (`تخطَّ`, `نسّق`). Colloquial
forms are not acceptable in a pack row even when they are shorter, because the
pack is read by learners and by models trained on MSA technical prose. Loanwords
are two classes: *transliterated technical names* (`ascii` أسكي, `json` جسون,
`iuncta` توبل) are kept, because Arabic technical writing transliterates them;
*Latin keywords with an existing Arabic term* are translated, because the Latin is
only an internal spelling (`typus` becomes نوع, not a transliteration).

**(c) Script, direction and the separator rule.** Rows are written in Arabic
script, in logical order; right-to-left rendering is presentation and no row
carries a bidi control character (no LRM/RLM, no isolating marks). A multi-word
name joins its words with the underscore `_`, never with a space, so the lexer
sees one spelling: 18 of 206 rows use the join today (`varia` line 19
قابل_للتغيير, `figendum` line 84 انتظر_ثابت, `argmaxima` line 163 فهرس_الأكبر,
`clampa` line 186 تثبيت_الحافة, `factum` line 160 حاصل_الضرب, and their
neighbours). Two rows break the rule by writing two words with no separator at
all — `fiet` (line 81) غيرمتزامن and `fient` (line 83) مولد_غيرمتزامن — and one
row writes two particles run together, `sin` (line 48) وإلاإذا; these are findings,
not variants.

**(d) One Latin key, two rows.** A Latin key may appear in `[keywords]` and in
`[types]`, and the two rows must then be *different* words, because they are
different concepts. `modulus` is the keyword at line 13 (حلقة, the wrapping
arithmetic) and the type at line 174 (نوع_حلقة, the modular integer); the keyword
must keep the ring sense and the type must name the integer, so the type row is a
finding. `cursor` is the keyword at line 135 (value `cursor`, kept as an identity
row) and the type at line 203 (مؤشر, *pointer*, a finding: the type is an
iterator). `meus` (line 223, إرسال) and `tuus` (line 224, استقبال) are the `sermo`
view methods `s.meus<T>()` / `s.tuus<T>()`: the pack renders the en concepts
send/recv rather than the Latin possessives, which is a deliberate deviation and
is declared here so a later reviewer does not translate them as *mine* and
*your*.

**(e) When a row may keep its Latin spelling.** Four routes, and each must be
declared in the row's reason line: (1) the value is an international technical
title that Arabic engineering text prints verbatim (`nonnumerus` = nan, byte-equal
to en line 70; `cli` = cli); (2) the key is the toolchain own name (`radix`);
(3) there is no single Arabic equivalent and en also keeps the Latin
(`primus_quem`, `ubi` in en lines 165–166, `cursor` keyword); (4) the word
spelling is retired and the row survives only as a recognizer for a migration
diagnostic (`inter`, `intra`). A row that keeps Latin for none of these reasons
is a finding — which is why `ratio` (line 217) is: en renders it as *record*, the
sibling view types tensor/vector/matrix/sparsa are Arabic in this same pack, and
`la` line 230 being `ratio` only shows that Latin is the Latin pack, not that
Arabic may keep it. A row whose value equals its Latin key today is flagged
`equals-latin-key: yes`: `cli` (133), `cursor` (135), `primus_quem` (90),
`radix` (148), `ratio` (217), `ubi` (91).

**(f) What this reviewer could not settle.** `conversion` (line 140) has no
counterpart row in en (`en` carries only `conversio`), so there is no concept for
it to render; it duplicates `conversio` in the imperative form and is marked low
rather than guessed. `tabulae` (line 200) is flagged provisional in the pack
itself and its ledger sense is defensible, so it is marked low
too. The `inter`/`intra` recognizer rows have no authoring surface left, so their
Arabic cannot be tested against real source. Finally, whether Arabic technical
prose prefers the definite article on aggregate rows (`summa`, `maxima`,
`minima`, `numeratio`) is a style judgement the operator has not ruled on; they
are kept and marked medium.

## Findings
- row: discretio - section: keywords - current: تمايز - proposed: اتحاد - confidence: medium - equals-latin-key: no - reason: تمايز names differentiation between things, while the keyword introduces a tagged sum of alternatives (en `union`)
- row: fixum - section: keywords - current: ثابت - proposed: ثابت - confidence: high - equals-latin-key: no - reason: a noun or adjective constant matches const and the compile-time-only rule
- row: functio - section: keywords - current: دالة - proposed: دالة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `fn` and the la row `functio`
- row: genus - section: keywords - current: صنف - proposed: صنف - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `class` and the la row `genus`
- row: implendum - section: keywords - current: عقد - proposed: عقد - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `interface` and the la row `implendum`
- row: importa - section: keywords - current: استورد - proposed: استورد - confidence: high - equals-latin-key: no - reason: استورد is the imperative import, so the declaration row follows the imperative convention
- row: regio - section: keywords - current: وحدة - proposed: وحدة_برمجية - confidence: medium - equals-latin-key: no - reason: وحدة is the generic measure word for a unit and carries no module sense, which a two-word join can carry
- row: modulus - section: keywords - current: حلقة - proposed: حلقة - confidence: medium - equals-latin-key: no - reason: حلقة is the algebraic ring, which carries modular arithmetic better than a loop reading; the type row must stay distinct
- row: ordo - section: keywords - current: ترتيب - proposed: تعداد - confidence: medium - equals-latin-key: no - reason: ترتيب names a sort order and collides with ordinata (رتب), while an enum is a closed set of named cases
- row: sit - section: keywords - current: ليكن - proposed: ليكن - confidence: high - equals-latin-key: no - reason: the jussive of كان (let it be) is an acceptable imperative for a binding introduction
- row: typus - section: keywords - current: نمط - proposed: نوع - confidence: high - equals-latin-key: no - reason: نمط means pattern or style, and نوع is the established Arabic word for a type; نمط also reads as the pattern of a match
- row: schema - section: keywords - current: مخطط - proposed: مخطط - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `schema` and the la row `schema`
- row: columna - section: keywords - current: عمود - proposed: عمود - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `column` and the la row `columna`
- row: varia - section: keywords - current: قابل_للتغيير - proposed: قابل_للتغيير - confidence: high - equals-latin-key: no - reason: the adjective changeable names the mutable posture, and it is unique once the in row stops repeating it
- row: ceteri - section: keywords - current: باقي - proposed: باقي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `rest` and the la row `ceteri`
- row: errata - section: keywords - current: مخطئ - proposed: أخطاء - confidence: high - equals-latin-key: no - reason: مخطئ is the adjective erring, said of a person, but the row declares error types and needs the plural noun
- row: exitus - section: keywords - current: مخرج - proposed: مخرج - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `exit` and the la row `exitus`
- row: generis - section: keywords - current: سكوني - proposed: سكوني - confidence: medium - equals-latin-key: no - reason: the keyword is retired as a source spelling (static members are PARSE010), so the row survives only as a recognizer and its stative adjective is left alone
- row: iacit - section: keywords - current: يرمي - proposed: يرمي - confidence: medium - equals-latin-key: no - reason: the present indicative matches the property the function declares (it throws), not a command
- row: immutata - section: keywords - current: ثابتة - proposed: مقفلة - confidence: medium - equals-latin-key: no - reason: ثابتة repeats fixum (ثابت) word for word, so the set-once posture is indistinguishable from a compile-time constant
- row: interna - section: keywords - current: داخلي - proposed: داخلي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `internal` and the la row `interna`
- row: magnitudo - section: keywords - current: حجم - proposed: حجم - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `size` and the la row `magnitudo`
- row: iuncta - section: keywords - current: توبل - proposed: توبل - confidence: high - equals-latin-key: no - reason: توبل is a transliteration, which is how Arabic names a tuple
- row: optiones - section: keywords - current: خيارات - proposed: خيارات - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `options` and the la row `optiones`
- row: praefixum - section: keywords - current: بادئة - proposed: تقييم_مسبق - confidence: medium - equals-latin-key: no - reason: بادئة renders the Latin sense prefix, but the keyword marks compile-time evaluation (en `comptime`)
- row: insere - section: keywords - current: تضمين - proposed: تضمين - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `embed` and the la row `insere`
- row: privata - section: keywords - current: خاص - proposed: خاص - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `private` and the la row `privata`
- row: protecta - section: keywords - current: محمي - proposed: محمي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `protected` and the la row `protecta`
- row: publica - section: keywords - current: عام - proposed: عام - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `public` and the la row `publica`
- row: sponte - section: keywords - current: اختياري - proposed: اختياري - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `optional` and the la row `sponte`
- row: casu - section: keywords - current: حالة - proposed: حالة - confidence: high - equals-latin-key: no - reason: حالة is the noun case, which follows the noun convention for a case label
- row: ceterum - section: keywords - current: افتراضي - proposed: افتراضي - confidence: medium - equals-latin-key: no - reason: افتراضي is the adjectival default, which follows the modifier convention for a catch-all label
- row: custodi - section: keywords - current: احرس - proposed: احرس - confidence: high - equals-latin-key: no - reason: احرس is the imperative guard, so the precondition row follows the imperative convention
- row: discerne - section: keywords - current: طابق - proposed: طابق - confidence: high - equals-latin-key: no - reason: طابق is the imperative match, so the dispatch row follows the imperative convention
- row: dum - section: keywords - current: طالما - proposed: طالما - confidence: high - equals-latin-key: no - reason: طالما is the temporal particle for while, so the row follows the particle convention
- row: elige - section: keywords - current: اختر - proposed: اختر - confidence: high - equals-latin-key: no - reason: اختر is the imperative choose, so the value-dispatch row follows the imperative convention (en `switch`)
- row: ergo - section: keywords - current: إذن - proposed: عندئذ - confidence: medium - equals-latin-key: no - reason: إذن is primarily the noun permission and a particle only in narrow contexts, so the joint reads as a noun in statement position
- row: fac - section: keywords - current: افعل - proposed: افعل - confidence: high - equals-latin-key: no - reason: افعل is the imperative do, so the block row follows the imperative convention
- row: itera - section: keywords - current: كرر - proposed: كرر - confidence: high - equals-latin-key: no - reason: كرر is the imperative repeat, so the loop row follows the imperative convention
- row: apud - section: keywords - current: عند - proposed: عند - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `at` and the la row `apud`
- row: secus - section: keywords - current: وإلا - proposed: وإلا - confidence: high - equals-latin-key: no - reason: وإلا is the alternative-branch particle, so the row follows the particle convention
- row: si - section: keywords - current: إذا - proposed: إذا - confidence: high - equals-latin-key: no - reason: إذا is the conditional particle the convention assigns to a condition, so the row follows the particle convention
- row: sin - section: keywords - current: وإلاإذا - proposed: وإلا_إذا - confidence: medium - equals-latin-key: no - reason: وإلاإذا writes two particles run together with no separator, which the underscore join rule forbids
- row: perge - section: keywords - current: تابع - proposed: تابع - confidence: high - equals-latin-key: no - reason: تابع is the imperative continue, so the row follows the imperative convention
- row: redde - section: keywords - current: أعد - proposed: أعد - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `return` and the la row `redde`
- row: rumpe - section: keywords - current: اكسر - proposed: اكسر - confidence: high - equals-latin-key: no - reason: اكسر is the imperative break, so the row follows the imperative convention
- row: tacet - section: keywords - current: صمت - proposed: تجاهل - confidence: high - equals-latin-key: no - reason: صمت is the past tense he fell silent, neither an imperative nor a noun, so a no-op statement reads as narration
- row: adfirma - section: keywords - current: أكد - proposed: أكد - confidence: high - equals-latin-key: no - reason: أكد is the imperative assert, so the row follows the imperative convention
- row: cape - section: keywords - current: التقط - proposed: التقط - confidence: high - equals-latin-key: no - reason: التقط is the imperative catch, so the row follows the imperative convention
- row: capta - section: keywords - current: فخ - proposed: فخ - confidence: high - equals-latin-key: no - reason: فخ is the noun trap, which follows the noun convention for the inspected error value
- row: cede - section: keywords - current: سلم - proposed: سلم - confidence: high - equals-latin-key: no - reason: سلم is the imperative yield, so the stream row follows the imperative convention
- row: iace - section: keywords - current: ارم - proposed: ارم - confidence: high - equals-latin-key: no - reason: ارم is the imperative throw, so the error row follows the imperative convention
- row: mori - section: keywords - current: انهر - proposed: انهر - confidence: high - equals-latin-key: no - reason: انهر is the imperative panic (collapse), so the row follows the imperative convention
- row: requirit - section: keywords - current: يتطلب - proposed: اطلب - confidence: medium - equals-latin-key: no - reason: يتطلب is the present indicative it requires, while its twin adfirma (assert) is an imperative command
- row: reice - section: keywords - current: ارفض - proposed: ما_لم - confidence: high - equals-latin-key: no - reason: ارفض commands the act of refusing, and reice is a conditional particle (en `reject`, used as reice cond iace err), so the imperative form is the wrong part of speech
- row: clausura - section: keywords - current: إغلاق - proposed: دالة_مجهولة - confidence: medium - equals-latin-key: no - reason: إغلاق names the act of closing or locking, not the anonymous function value the row introduces (en `lambda`)
- row: falsum - section: keywords - current: خطأ - proposed: خطأ - confidence: high - equals-latin-key: no - reason: the صواب and خطأ pair is the established boolean vocabulary
- row: verum - section: keywords - current: صواب - proposed: صواب - confidence: high - equals-latin-key: no - reason: the صواب and خطأ pair is the established boolean vocabulary
- row: nonnumerus - section: keywords - current: nan - proposed: nan - confidence: medium - equals-latin-key: no - reason: nan is the IEEE-754 title that Arabic numeric code prints verbatim, so it is retained as a technical name, byte-equal to the en row by design
- row: aut - section: keywords - current: أو - proposed: أو - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `or` and the la row `aut`
- row: est - section: keywords - current: هو - proposed: هو - confidence: medium - equals-latin-key: no - reason: هو is the copula pronoun, which fits the type test
- row: et - section: keywords - current: و - proposed: و - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `and` and the la row `et`
- row: non - section: keywords - current: ليس - proposed: ليس - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `not` and the la row `non`
- row: vel - section: keywords - current: عوض - proposed: عوض - confidence: medium - equals-latin-key: no - reason: عوض names substitution, which keeps coalesce distinct from أو (aut)
- row: ego - section: keywords - current: ذات - proposed: ذات - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `self` and the la row `ego`
- row: finge - section: keywords - current: أنشئ - proposed: أنشئ - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `variant` and the la row `finge`
- row: implet - section: keywords - current: حقق - proposed: حقق - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `implements` and the la row `implet`
- row: mone - section: keywords - current: نبه - proposed: نبه - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `warn` and the la row `mone`
- row: nota - section: keywords - current: اعرض - proposed: اعرض - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `print` and the la row `nota`
- row: scribe - section: keywords - current: اكتب - proposed: اكتب - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `write` and the la row `scribe`
- row: vide - section: keywords - current: شاهد - proposed: شاهد - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `debug` and the la row `vide`
- row: argumenta - section: keywords - current: وسائط - proposed: وسائط - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `args` and the la row `argumenta`
- row: incipiet - section: keywords - current: استهلال - proposed: بداية_غير_متزامنة - confidence: medium - equals-latin-key: no - reason: استهلال is a noun for initiation and does not mark the async entry that separates it from incipit
- row: incipit - section: keywords - current: بداية - proposed: بداية - confidence: medium - equals-latin-key: no - reason: بداية names the program beginning, and a noun is right for an entry-point name
- row: ad - section: keywords - current: اتصل - proposed: اتصل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `call` and the la row `ad`
- row: fiet - section: keywords - current: غيرمتزامن - proposed: غير_متزامن - confidence: medium - equals-latin-key: no - reason: غيرمتزامن joins two words with no separator, so the async marker is unreadable as one spelling
- row: fiunt - section: keywords - current: مولد - proposed: مولد - confidence: high - equals-latin-key: no - reason: مولد is the noun generator, marking a sync stream on Axis A rather than an imperative
- row: fient - section: keywords - current: مولد_غيرمتزامن - proposed: مولد_غير_متزامن - confidence: medium - equals-latin-key: no - reason: مولد_غيرمتزامن carries the same missing join inside its second element
- row: figendum - section: keywords - current: انتظر_ثابت - proposed: انتظر_ثابت - confidence: medium - equals-latin-key: no - reason: انتظر_ثابت is the imperative await joined to the noun constant, and the compound follows the join rule
- row: variandum - section: keywords - current: انتظر_متغير - proposed: انتظر_قابل_للتغيير - confidence: medium - equals-latin-key: no - reason: متغير spells the mutable posture with a different word than varia, so one posture reads as two concepts
- row: reddet - section: keywords - current: أعد_منتظرا - proposed: أعد_منتظرا - confidence: medium - equals-latin-key: no - reason: أعد_منتظرا is the imperative return joined to the awaited participle, and the compound follows the join rule
- row: tacebit - section: keywords - current: انتظر - proposed: انتظر - confidence: high - equals-latin-key: no - reason: انتظر is the imperative await, so the discard row follows the imperative convention
- row: vacua - section: keywords - current: فارغ - proposed: فارغ - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `empty` and the la row `vacua`
- row: primus_quem - section: keywords - current: primus_quem - proposed: primus_quem - confidence: high - equals-latin-key: yes - reason: a two-word Latin relative-clause name has no single Arabic equivalent, and en keeps it too, so the identity is declared
- row: ubi - section: keywords - current: ubi - proposed: حيث - confidence: medium - equals-latin-key: yes - reason: the Latin adverb is kept only because en makes Latin its own spelling, while an Arabic pack reads حيث for the same clause
- row: summa - section: keywords - current: المجموع - proposed: المجموع - confidence: medium - equals-latin-key: no - reason: the definite article is idiomatic for the aggregate value in Arabic
- row: filum - section: keywords - current: خيط - proposed: خيط - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `thread` and the la row `filum`
- row: maxima - section: keywords - current: الأقصى - proposed: الأقصى - confidence: medium - equals-latin-key: no - reason: the definite article is idiomatic for the aggregate value in Arabic
- row: minima - section: keywords - current: الأدنى - proposed: الأدنى - confidence: medium - equals-latin-key: no - reason: the definite article is idiomatic for the aggregate value in Arabic
- row: de - section: keywords - current: عن - proposed: عن - confidence: medium - equals-latin-key: no - reason: عن marks origin or about, the direction the immutable borrow marker denotes
- row: ex - section: keywords - current: من - proposed: من - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `from` and the la row `ex`
- row: in - section: keywords - current: قابل_للتغيير - proposed: مرجع_متغير - confidence: high - equals-latin-key: no - reason: the row is byte-identical to varia (قابل_للتغيير), so the mutable borrow and the mutable binding resolve to one spelling
- row: own - section: keywords - current: ملك - proposed: ملك - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `own` and the la row `penes`
- row: copy - section: keywords - current: نسخة - proposed: نسخة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `copy` and the la row `exemplum`
- row: axis - section: keywords - current: محور - proposed: محور - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `axis` and the la row `axis`
- row: lege - section: keywords - current: اقرأ - proposed: اقرأ - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `read` and the la row `lege`
- row: lineam - section: keywords - current: سطرا - proposed: سطرا - confidence: medium - equals-latin-key: no - reason: سطرا is the indefinite accusative a line, matching the line-reading unit
- row: omnia - section: keywords - current: الكل - proposed: الكل - confidence: high - equals-latin-key: no - reason: الكل is the idiomatic totaliser for all
- row: scriptum - section: keywords - current: حرر - proposed: نسّق - confidence: medium - equals-latin-key: no - reason: حرر commands editing or setting free, not formatting a template (en `format`)
- row: sparge - section: keywords - current: انشر - proposed: انشر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `spread` and the la row `sparge`
- row: ut - section: keywords - current: كـ - proposed: كـ - confidence: high - equals-latin-key: no - reason: كـ is the particle as, matching the alias role
- row: via - section: keywords - current: عبر - proposed: عبر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `via` and the la row `via`
- row: ante - section: keywords - current: قبل - proposed: قبل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `before` and the la row `ante`
- row: inter - section: keywords - current: بين - proposed: بين - confidence: medium - equals-latin-key: no - reason: the word spelling is retired in favour of the membership glyph, so the row is kept only as a recognizer and no new Arabic word should be minted
- row: intra - section: keywords - current: ضمن - proposed: ضمن - confidence: medium - equals-latin-key: no - reason: the word spelling is retired in favour of the membership glyph, so the row is kept only as a recognizer and no new Arabic word should be minted
- row: per - section: keywords - current: كل - proposed: خطوة - confidence: high - equals-latin-key: no - reason: كل is the quantifier every, while per is the range stride (en `step`); pro already carries the per-element sense
- row: ab - section: keywords - current: نطاق - proposed: نطاق - confidence: high - equals-latin-key: no - reason: نطاق is exactly the en concept range
- row: usque - section: keywords - current: حتى - proposed: حتى - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `until` and the la row `usque`
- row: erratur - section: keywords - current: توقع_الفشل - proposed: توقع_الفشل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `expect_failure` and the la row `erratur`
- row: fragilis - section: keywords - current: هش - proposed: هش - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `flaky` and the la row `fragilis`
- row: futurum - section: keywords - current: مستقبلي - proposed: مستقبلي - confidence: medium - equals-latin-key: no - reason: مستقبلي renders pending future work, the en concept todo
- row: metior - section: keywords - current: قس - proposed: قس - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `bench` and the la row `metior`
- row: omitte - section: keywords - current: أهمل - proposed: تخطَّ - confidence: medium - equals-latin-key: no - reason: أهمل means to neglect, which carries blame, where the marker skips a test deliberately (en `skip`)
- row: postpara - section: keywords - current: لاحق - proposed: إنهاء - confidence: medium - equals-latin-key: no - reason: لاحق is the adjective subsequent and never names the teardown step (en `teardown`)
- row: postparabit - section: keywords - current: سيلحق - proposed: سينهي - confidence: medium - equals-latin-key: no - reason: سيلحق says will follow, keeping only the future tense that marks async and losing the teardown sense
- row: praepara - section: keywords - current: جهز - proposed: جهز - confidence: high - equals-latin-key: no - reason: جهز is the imperative setup, so the test-lifecycle row follows the imperative convention
- row: praeparabit - section: keywords - current: سيهيئ - proposed: سيهيئ - confidence: medium - equals-latin-key: no - reason: سيهيئ is the future of هيأ, so the async setup row marks the future on Axis A
- row: proba - section: keywords - current: اختبر - proposed: اختبر - confidence: high - equals-latin-key: no - reason: اختبر is the imperative test, so the row follows the imperative convention
- row: probandum - section: keywords - current: مختبر - proposed: مجموعة_اختبار - confidence: medium - equals-latin-key: no - reason: مختبر names a laboratory, a place and an instrument, not the block that describes and groups tests (en `describe`)
- row: repete - section: keywords - current: معاد - proposed: تكرار - confidence: medium - equals-latin-key: no - reason: معاد is the passive participle a repeated thing, a state, where the row requests repetition (en `repeat`)
- row: solum - section: keywords - current: فقط - proposed: فقط - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `only` and the la row `solum`
- row: solum_in - section: keywords - current: حصري - proposed: فقط_في - confidence: medium - equals-latin-key: no - reason: حصري is the adjective exclusive and drops the platform-scope relation that only_in carries
- row: tag - section: keywords - current: وسم - proposed: وسم - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `tag` and the la row `tag`
- row: temporis - section: keywords - current: زمني - proposed: مهلة - confidence: high - equals-latin-key: no - reason: زمني is the adjective temporal and never names a timeout value (en `timeout`)
- row: nulla - section: keywords - current: غائب - proposed: غائب - confidence: medium - equals-latin-key: no - reason: غائب names the absence the null value literal carries
- row: cli - section: keywords - current: cli - proposed: cli - confidence: high - equals-latin-key: yes - reason: the acronym is international and en keeps it as an identity row
- row: commune - section: keywords - current: مشترك - proposed: مشترك - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `shared` and the la row `commune`
- row: cursor - section: keywords - current: cursor - proposed: cursor - confidence: medium - equals-latin-key: yes - reason: en keeps this keyword row as cursor while the type row carries the iterator concept, so the Latin name is retained and declared
- row: futura - section: keywords - current: مستقبل - proposed: مستقبل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `future` and the la row `futura`
- row: imperium - section: keywords - current: أمر - proposed: أمر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `command` and the la row `imperium`
- row: operandus - section: keywords - current: معامل - proposed: معامل - confidence: medium - equals-latin-key: no - reason: معامل is the usual Arabic rendering of operand, though it also carries operator and coefficient
- row: optio - section: keywords - current: خيار - proposed: خيار - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `option` and the la row `optio`
- row: conversion - section: keywords - current: حوّل - proposed: حوّل - confidence: low - equals-latin-key: no - reason: en has no keyword row named conversion, so there is no concept for this row to render and it duplicates conversio in the imperative; the reviewer cannot settle it
- row: conversio - section: keywords - current: تحويل - proposed: تحويل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `conversion` and the la row `conversio`
- row: nucleum - section: keywords - current: نواة - proposed: نواة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `kernel` and the la row `nucleum`
- row: vertex - section: keywords - current: رأس - proposed: رأس - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `vertex` and the la row `vertex`
- row: fragment - section: keywords - current: جزء - proposed: جزء - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `fragment` and the la row `fragment`
- row: nondum - section: keywords - current: غير_مستقر - proposed: غير_مستقر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `unstable` and the la row `nondum`
- row: verte - section: keywords - current: إعادة_تسمية - proposed: إعادة_تسمية - confidence: medium - equals-latin-key: no - reason: the noun phrase names the rename operation, which is acceptable for a modifier row
- row: nomen - section: keywords - current: اسم - proposed: اسم - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `name` and the la row `nomen`
- row: radix - section: keywords - current: radix - proposed: radix - confidence: high - equals-latin-key: yes - reason: the term is the toolchain own name and en keeps it as an identity row
- row: brevis - section: keywords - current: مختصر - proposed: مختصر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `short` and the la row `brevis`
- row: descriptio - section: keywords - current: وصف - proposed: وصف - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `description` and the la row `descriptio`
- row: longum - section: keywords - current: مفصل - proposed: مفصل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `long` and the la row `longum`
- row: lane - section: keywords - current: مسار - proposed: مسار - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `lane` and the la row `lane`
- row: ubique - section: keywords - current: عمومي - proposed: عمومي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `global` and the la row `ubique`
- row: annotatio - section: keywords - current: تعليق_توضيحي - proposed: تعليق_توضيحي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `annotation` and the la row `annotatio`
- row: versio - section: keywords - current: إصدار - proposed: إصدار - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `version` and the la row `versio`
- row: imperia - section: keywords - current: أوامر - proposed: أوامر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `commands` and the la row `imperia`
- row: reducta - section: keywords - current: اختزل - proposed: اختزل - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `reduce` and the la row `reducta`
- row: pro - section: keywords - current: حسب - proposed: حسب - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `per` and the la row `pro`
- row: factum - section: keywords - current: حاصل_الضرب - proposed: حاصل_الضرب - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `product` and the la row `factum`
- row: numeratio - section: keywords - current: العدّ - proposed: العدّ - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `count` and the la row `numeratio`
- row: quilibet - section: keywords - current: أي - proposed: أي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `any` and the la row `quilibet`
- row: argmaxima - section: keywords - current: فهرس_الأكبر - proposed: فهرس_الأكبر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `argmax` and the la row `argmaxima`
- row: argminima - section: keywords - current: فهرس_الأصغر - proposed: فهرس_الأصغر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `argmin` and the la row `argminima`
- row: ordinata - section: keywords - current: رتب - proposed: رتب - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `sort` and the la row `ordinata`
- row: ascendens - section: keywords - current: تصاعدي - proposed: تصاعدي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `asc` and the la row `ascendens`
- row: descendens - section: keywords - current: تنازلي - proposed: تنازلي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `desc` and the la row `descendens`
- row: ascii - section: types - current: أسكي - proposed: أسكي - confidence: high - equals-latin-key: no - reason: a transliteration of a technical name, which is the declared retention route
- row: textus - section: types - current: نص - proposed: نص - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `string` and the la row `textus`
- row: numerus - section: types - current: عدد - proposed: عدد - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `int` and the la row `numerus`
- row: modulus - section: types - current: نوع_حلقة - proposed: نوع_حلقة - confidence: medium - equals-latin-key: no - reason: حلقة is the algebraic ring, which carries modular arithmetic better than a loop reading; the type row must stay distinct
- row: saturatus - section: types - current: مشبع - proposed: مشبع - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `saturating` and the la row `saturatus`
- row: exactus - section: types - current: دقيق - proposed: دقيق - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `trapping` and the la row `exactus`
- row: fractus - section: types - current: كسر - proposed: عشري - confidence: medium - equals-latin-key: no - reason: كسر names a fraction, a ratio of integers, while fractus is binary floating point (en `float`)
- row: bivalens - section: types - current: منطقي - proposed: منطقي - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `bool` and the la row `bivalens`
- row: nihil - section: types - current: لاشيء - proposed: لاشيء - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `none` and the la row `nihil`
- row: vacuum - section: types - current: فراغ - proposed: فراغ - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `void` and the la row `vacuum`
- row: numquam - section: types - current: أبدا - proposed: أبدا - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `never` and the la row `numquam`
- row: ignotum - section: types - current: مجهول - proposed: مجهول - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `unknown` and the la row `ignotum`
- row: octeti - section: types - current: بايتات - proposed: بايتات - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `bytes` and the la row `octeti`
- row: octetus - section: types - current: بايت - proposed: بايت - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `byte` and the la row `octetus`
- row: clampa - section: types - current: تثبيت_الحافة - proposed: تثبيت_الحافة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `clamp` and the la row `clampa`
- row: reflexa - section: types - current: انعكاس_الحافة - proposed: انعكاس_الحافة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `reflect` and the la row `reflexa`
- row: involuta - section: types - current: التفاف_الحافة - proposed: التفاف_الحافة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `wrap` and the la row `involuta`
- row: littera - section: types - current: حرف - proposed: حرف - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `char` and the la row `littera`
- row: regex - section: types - current: تعبير - proposed: تعبير_نمطي - confidence: medium - equals-latin-key: no - reason: bare تعبير means any expression, and the row is the pattern expression (en `regex`)
- row: json - section: types - current: جسون - proposed: جسون - confidence: high - equals-latin-key: no - reason: a transliteration of a technical name, which is the declared retention route
- row: valor - section: types - current: قيمة - proposed: قيمة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `value` and the la row `valor`
- row: instans - section: types - current: لحظة - proposed: لحظة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `instant` and the la row `instans`
- row: objectum - section: types - current: كائن - proposed: كائن - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `object` and the la row `objectum`
- row: quidlibet - section: types - current: مهما - proposed: مهما - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `any` and the la row `quidlibet`
- row: lista - section: types - current: قائمة - proposed: قائمة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `list` and the la row `lista`
- row: tabula - section: types - current: جدول - proposed: جدول - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `map` and the la row `tabula`
- row: tabulae - section: types - current: دفتر - proposed: دفتر - confidence: low - equals-latin-key: no - reason: the pack itself flags the row provisional, and the ledger sense دفتر is defensible for an insertion-ordered map
- row: copia - section: types - current: مجموعة - proposed: مجموعة - confidence: medium - equals-latin-key: no - reason: مجموعة is the generic collection word and does not mark the uniqueness that separates copia from lista
- row: promissum - section: types - current: وعد - proposed: وعد - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `promise` and the la row `promissum`
- row: cursor - section: types - current: مؤشر - proposed: مؤشر - confidence: medium - equals-latin-key: no - reason: en keeps this keyword row as cursor while the type row carries the iterator concept, so the Latin name is retained and declared
- row: queue - section: types - current: طابور - proposed: طابور - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `queue` and the la row `queue`
- row: stack - section: types - current: مكدس - proposed: مكدس - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `stack` and the la row `stack`
- row: tensor - section: types - current: موتر - proposed: موتر - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `tensor` and the la row `tensor`
- row: vector - section: types - current: متجه - proposed: متجه - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `vector` and the la row `vector`
- row: matrix - section: types - current: مصفوفة - proposed: مصفوفة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `matrix` and the la row `matrix`
- row: sparsa - section: types - current: متفرقة - proposed: متفرقة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `sparse` and the la row `sparsa`
- row: atomic - section: types - current: ذري - proposed: ذري - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `atomic` and the la row `atomic`
- row: intervallum - section: types - current: فترة - proposed: فترة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `range` and the la row `intervallum`
- row: filtrum - section: types - current: مرشح - proposed: مرشح - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `filter` and the la row `filtrum`
- row: series - section: types - current: سلسلة - proposed: سلسلة - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `series` and the la row `series`
- row: census - section: types - current: تعداد - proposed: تعداد - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `census` and the la row `census`
- row: ratio - section: types - current: ratio - proposed: سجل - confidence: high - equals-latin-key: yes - reason: the sibling view types tensor, vector, matrix and sparsa are Arabic and en renders this row as record, so the Latin spelling isolates one member of the family
- row: sermo - section: types - current: قناة - proposed: قناة - confidence: high - equals-latin-key: no - reason: قناة is exactly the en concept channel
- row: scrinium - section: types - current: إطار - proposed: إطار - confidence: high - equals-latin-key: no - reason: form and sense match the en concept `frame` and the la row `scrinium`
- row: trapped - section: types - current: محتجز - proposed: محتجز - confidence: high - equals-latin-key: no - reason: محتجز matches the en concept trapped exactly
- row: meus - section: types - current: إرسال - proposed: إرسال - confidence: medium - equals-latin-key: no - reason: the row renders the en concept send for the sermo view method rather than the Latin possessive, which is a declared deviation
- row: tuus - section: types - current: استقبال - proposed: استقبال - confidence: medium - equals-latin-key: no - reason: the row renders the en concept recv for the sermo view method rather than the Latin possessive, which is a declared deviation
