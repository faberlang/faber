# Hindi Reader Locale — Review

Review of `locale/hi/pack.toml` against the en concept pack (`locale/en/pack.toml`),
the la heritage pack (`locale/la/pack.toml`), the per-key concepts and rationales in
`docs/design/llm-locale.md`, and the verb-form law in `docs/stdlib/morphologia.md`.
The review records findings only; it edits no pack row.

Scope: 206 rows — `[keywords]` from line 5 (159 rows) and `[types]` from line 170
(47 rows). Diagnostics rows are out of scope.

## Conventions

The rules below are the lens this review applies to every one of the 206 rows. Where a
rule contradicts the current pack word, the finding proposes a replacement; where it
does not, the finding restates the current word and says so.

### (a) Part of speech and verb form

Statement keywords divide by what they do at the statement site:

- A keyword that *issues a command* takes the **Imperativus**, the second-person
  singular imperative in `-ओ` (`करो`, `लौटाओ`, `छोड़ो`, `मापो`). This covers block,
  loop, return, break/continue, throw/catch, IO and test commands. The verb-form law in
  `docs/stdlib/morphologia.md` is the authority: Imperativus is the device/mutating
  posture, and a command keyword is a command.
- A keyword that *marks a property of a declaration* (a modifier such as `iacit`,
  `implet`, `immutata`, `sponte`, `generis`) takes the form that agrees with the
  declared entity: third-person present for a predicate about a function or genus, an
  adjective for a visibility or property marker. It does not take the imperative,
  because it commands nothing.
- A keyword that *names a thing* (a type, a collection marker, a noun role) takes a bare
  nominal, with no verb inflection.
- A binding particle (`sit`) is not a verb and not a noun; it is the word that opens a
  binding and must read as an instruction to take the binding, not as a description of
  the bound value.

What the pack does today on the three rows the brief names:

- `sit` at line 15 is `बैठा`, the past participle of बैठना ("seated"). A participle
  describes a state; a let binding needs the imperative `मानो` ("assume/let"), so the
  form is wrong, not merely unusual.
- `iacit` at line 24 is `फेंकता`, third person present ("throws"). Under the modifier
  rule this form is *correct in person*, but the bare participle reads as a predicate
  missing its object, so the finding keeps it at medium confidence rather than calling
  it settled.
- `implet` at line 72 is `लागूकरता` — the right person, but a verb compound fused with
  no separator; the separator rule in (c) applies to it.

The rule applied to every row: read the en concept, decide whether the row is a
command, a modifier or a name, and require the matching form. Rows whose form is right
but whose word is semantically off are still findings, because a correct participle of
the wrong verb is not a translation.

### (b) Register and technical Latin

The pack targets a **formal written register** suitable for source code a reviewer reads
and an LLM emits: standard Hindi (शुद्ध हिंदी) vocabulary where a settled word exists,
with everyday Hindustani words accepted where the standard word is bookish or ambiguous
(`बाकी`, `जाल`, `धागा`, `कतार`). Colloquial contractions and English verbs are not
register-appropriate.

A **technical Latin name is kept** when it is a fixed proper name or acronym with no
stable Hindi equivalent, and when translating it would create a false friend. Today that
covers `ascii`, `cli`, `json`, `regex`, `radix`, `ratio`, `primus_quem` and `ubi`, plus
the en-equal technical tokens `nan` (`nonnumerus`), `async` (`fiet`), `argmax`
(`argmaxima`) and `argmin` (`argminima`). Loans that are already the ordinary Hindi
technical word (`टेंसर`, `स्टैक`, `कर्सर`, `फ़िल्टर`, `स्कीमा`, `कॉलम`, `ऑपरेंड`) are
treated as Hindi, not as kept Latin.

### (c) Script, matra, virama and separators

The pack is written in **Devanagari**, including matras and virama clusters
(`क्षेत्र`, `संस्करण`, `शृंखला`). Two script rules follow from
`docs/design/reader-locale.md`:

- **NFKC normalization is global**, as that document records: the lexer normalizes
  source to NFKC before matching, so a pack word is compared in its NFKC form. A
  Devanagari cluster and its precomposed or decomposed spellings must therefore
  normalize to the same sequence, and a proposal is only safe if it does. Indic digits
  (०-९) normalize compatibly with ASCII digits under the same rule.
- **Multi-word names join with an underscore**, never by fusing the words and never by
  a space (a space would split the token). 14 of the 206 rows use the underscore join
  today: `fient` (line 83, `async_जनक`), `figendum` (line 84), `variandum` (line 85,
  `रुको_चर`), `reddet` (line 86), `primus_quem` (line 90, where the key itself is the
  joined name), `erratur` (line 116), `verte` (line 146), `imperia` (line 156),
  `ordinata` (line 166), and the type rows `modulus` (line 174), `octetus` (line 184),
  `clampa` (line 186), `reflexa` (line 187), `involuta` (line 188).
  The rule cuts both ways: fused compounds such as `अन्यथायदि` (`sin`), `इधरफेंको`
  (`iace`), `मरोजाओ` (`mori`), `रूपमें` (`ut`), `पूर्वतैयार` (`praepara`),
  `पश्चतैयार` (`postpara`), `परीक्षणसमूह` (`probandum`), `केवलमें` (`solum_in`) and
  `लागूकरता` (`implet`) are form findings even when the words themselves are right.
  `async` fused onto a Devanagari word (`आरंभasync`, `पूर्वतैयारasync`,
  `पश्चतैयारasync`) is the same defect.

### (d) One Latin key, several roles

A Latin key may appear in more than one section. The pack spells each occurrence from
the **concept of that row**, and repeats a word across sections only when the concept is
the same:

- `modulus` is a **keyword** at line 13 and a **type** at line 174. The en concepts are
  `wrapping` and `wrapping_ty`; the la rows are `modulus` and `modulus_t`. The Hindi
  pack currently uses `मॉड्यूल` (the ordinary word for a *module*) for the keyword and
  `मॉड्यूल_टाइप` ("module type") for the type. Both are wrong: `regio` at line 12 is the
  module, and its `क्षेत्र` ("region/field") is likewise wrong. The ruling in this review
  is that **wrapping and the modular integer share the root `मॉड्यूलर`** as long as the
  rows stay distinct: keyword `मॉड्यूलर` (the modular arithmetic mode) and type
  `मॉड्यूलर_पूर्णांक` (the modular integer). `मॉड्यूल` is freed for `regio`, the module.
  Sharing a root is allowed because the concepts are the same family; sharing a whole
  spelling would not be.
- `genus` at line 9 is `वर्ग` (class) against `typus` at line 16 `प्रकार` (type). The
  distinction is deliberate and matches the en pair `class` / `type`: a **class** is a
  genus with instances and members, while a **type alias** names another type and has no
  instances. `वर्ग` is the class noun; `प्रकार` is the type noun. The composed type rows
  use the type word, not the class word.
- `cursor` is a keyword at line 135 (`संकेतक`) and a type at line 203 (`कर्सर`). These
  are **not** the same concept: the keyword marks a stream cursor and the type is the
  iterator object (en `cursor` against en `iterator`). The two spellings are therefore
  kept and declared, not unified; unifying them would give one word two meanings.
- Where a keyword and a type genuinely share one concept (the `modulus` arithmetic),
  the rows share a root and differ by the type head, and the finding says which is which.

### (e) When a row keeps its Latin spelling

A row may keep the Latin spelling when the word is (1) a proper name or acronym
(`ascii`, `cli`, `json`, `regex`, `radix`), (2) a fixed technical token with no stable
Hindi equivalent (`argmax`, `argmin`, `nan`, `async`), or (3) a phrase whose Hindi
rendering would be a false friend (`primus_quem`, `ubi`, `ratio`).

Such a row is **declared**, not silent: its finding says `equals-latin-key: yes` when
the value is byte-equal to the Latin key (exactly 8 rows: `ascii` at line 171, `cli` at
133, `json` at 191, `primus_quem` at 90, `radix` at 148, `ratio` at 217, `regex` at 190
and `ubi` at 91), and its reason names the declaration. Rows byte-equal to the *en* token
but not to the Latin key (`nonnumerus` at line 64 as `nan`, `fiet` at 81 as `async`,
`argmaxima` at 163 as `argmax`, `argminima` at 164 as `argmin`, en lines 70, 91, 178 and
179) are marked `equals-latin-key: no` and their reason says they equal the en token.
`async` is declared as a **technical prefix** that stays Latin inside a joined compound;
`fient` at line 83 (`async_जनक`) is consistent with that declaration, while the fused
`आरंभasync` family is not and is proposed with the underscore join.

### (f) What this reviewer could not settle

- `conversion` is a **Hindi-only keyword** with no row in the en or la pack, so there is
  no source concept to render. Its neighbour `conversio` is the conversion noun; the
  reader cannot tell whether `conversion` is the convert hint, an alias, or a stale row.
  It is marked low confidence and left with its current word.
- Whether `अन्यतम` (`ceterum`) is acceptable as the default arm: the sense is right but the
  superlative form is strained. Medium confidence, no replacement forced.
- Whether `ि` and `ी` matra choices in `रुको_चर` should follow `परिवर्तनीय` (the `varia`
  stem) — the two stems for one concept are noted, but the shorter `चर` is the standard
  mathematical stem and was not overridden.
- The exact NFKC interaction between a virama-final cluster and an appended underscore
  was not tested on a compiled reader here; proposals keep existing clusters untouched
  for that reason.
- Hindi grammatical gender of new compounds (e.g. `अवरोधी`) was chosen by sense, not
  checked against a normative grammar.

## Findings

- row: discretio - section: keywords - current: विभेद - proposed: विभेद - confidence: high - equals-latin-key: no - reason: no mismatch: विभेद renders the tagged-union concept and does not take enum from ordo
- row: fixum - section: keywords - current: स्थिर - proposed: स्थिर - confidence: high - equals-latin-key: no - reason: no mismatch: स्थिर is the settled adjective for a compile-time const binding
- row: functio - section: keywords - current: फलन - proposed: फलन - confidence: high - equals-latin-key: no - reason: no mismatch: फलन is the standard Hindi term for a function declaration
- row: genus - section: keywords - current: वर्ग - proposed: वर्ग - confidence: high - equals-latin-key: no - reason: no mismatch: वर्ग is the settled noun for a class, and typus keeps प्रकार for a type alias
- row: implendum - section: keywords - current: अनुबन्ध - proposed: अनुबन्ध - confidence: medium - equals-latin-key: no - reason: no mismatch: अनुबन्ध names a binding contract, though the everyday loan इंटरफ़ेस is more common
- row: importa - section: keywords - current: आयात - proposed: आयात - confidence: high - equals-latin-key: no - reason: no mismatch: आयात is the settled noun-verb for import
- row: regio - section: keywords - current: क्षेत्र - proposed: मॉड्यूल - confidence: high - equals-latin-key: no - reason: क्षेत्र means region or field, not a module; the module word is मॉड्यूल and belonged here, not on modulus
- row: modulus - section: keywords - current: मॉड्यूल - proposed: मॉड्यूलर - confidence: high - equals-latin-key: no - reason: मॉड्यूल names a module, but the keyword means wrapping modular arithmetic; मॉड्यूलर names the arithmetic and the type row takes the same root
- row: ordo - section: keywords - current: क्रम - proposed: क्रम - confidence: medium - equals-latin-key: no - reason: क्रम means order or sequence, not an enumeration; the enum concept needs क्रम_सूची or विकल्प_सूची
- row: sit - section: keywords - current: बैठा - proposed: मानो - confidence: high - equals-latin-key: no - reason: बैठा is a past participle meaning seated, not the imperative a let binding needs; मानो is the imperative of assuming a binding
- row: typus - section: keywords - current: प्रकार - proposed: प्रकार - confidence: high - equals-latin-key: no - reason: no mismatch: प्रकार is the settled noun for a type alias, distinct from genus वर्ग
- row: schema - section: keywords - current: स्कीमा - proposed: स्कीमा - confidence: high - equals-latin-key: no - reason: no mismatch: स्कीमा is the settled technical loan for a schema
- row: columna - section: keywords - current: कॉलम - proposed: कॉलम - confidence: high - equals-latin-key: no - reason: no mismatch: कॉलम is the settled technical loan for a column
- row: varia - section: keywords - current: परिवर्तनीय - proposed: परिवर्तनीय - confidence: high - equals-latin-key: no - reason: no mismatch: परिवर्तनीय is the adjective for a writable, re-pointable binding
- row: ceteri - section: keywords - current: बाकी - proposed: बाकी - confidence: high - equals-latin-key: no - reason: no mismatch: बाकी is the settled word for the rest binding
- row: errata - section: keywords - current: त्रुटि - proposed: त्रुटि - confidence: high - equals-latin-key: no - reason: no mismatch: त्रुटि is the declared error-type noun, distinct from the throwing verb
- row: exitus - section: keywords - current: निर्गम - proposed: निर्गम - confidence: high - equals-latin-key: no - reason: no mismatch: निर्गम is the exit-code noun
- row: generis - section: keywords - current: स्थैतिक - proposed: स्थैतिक - confidence: medium - equals-latin-key: no - reason: स्थैतिक is a loan of the retired static spelling; the row survives only as a recognizer for the PARSE010 retirement diagnostic, not as an authoring surface
- row: iacit - section: keywords - current: फेंकता - proposed: फेंकता - confidence: medium - equals-latin-key: no - reason: third-person present agreement is the right form for a modifier on a function, but the bare participle-like फेंकता reads as a predicate; फेंकता_है would be unambiguous
- row: immutata - section: keywords - current: अपरिवर्तित - proposed: अपरिवर्तित - confidence: high - equals-latin-key: no - reason: no mismatch: अपरिवर्तित is the readonly adjective
- row: interna - section: keywords - current: आंतरिक - proposed: आंतरिक - confidence: high - equals-latin-key: no - reason: no mismatch: आंतरिक is the internal visibility adjective
- row: magnitudo - section: keywords - current: आकार - proposed: आकार - confidence: medium - equals-latin-key: no - reason: आकार means size in general, not specifically a const-generic size parameter; acceptable but loses the parameter sense
- row: iuncta - section: keywords - current: टपल - proposed: टपल - confidence: high - equals-latin-key: no - reason: no mismatch: टपल is the settled technical loan for a tuple
- row: optiones - section: keywords - current: चयन - proposed: विकल्प_समूह - confidence: medium - equals-latin-key: no - reason: चयन means selection, while the row is the bundle of CLI options; the plural bundle needs the option stem plus a group word
- row: praefixum - section: keywords - current: उपसर्ग - proposed: संकलन_काल - confidence: high - equals-latin-key: no - reason: उपसर्ग means a linguistic prefix, but the row is the compile-time evaluation marker, which the concept comptime names
- row: insere - section: keywords - current: अंतःस्थापित - proposed: अंतःस्थापित - confidence: high - equals-latin-key: no - reason: no mismatch: अंतःस्थापित is the embedded-inline marker
- row: privata - section: keywords - current: निजी - proposed: निजी - confidence: high - equals-latin-key: no - reason: no mismatch: निजी is the private visibility adjective
- row: protecta - section: keywords - current: संरक्षित - proposed: संरक्षित - confidence: high - equals-latin-key: no - reason: no mismatch: संरक्षित is the protected visibility adjective
- row: publica - section: keywords - current: सार्वजनिक - proposed: सार्वजनिक - confidence: high - equals-latin-key: no - reason: no mismatch: सार्वजनिक is the public visibility adjective
- row: sponte - section: keywords - current: स्वेच्छा - proposed: वैकल्पिक - confidence: high - equals-latin-key: no - reason: स्वेच्छा means of one own will, but the row is an optional parameter slot; वैकल्पिक is the optional adjective
- row: casu - section: keywords - current: स्थिति - proposed: प्रकरण - confidence: medium - equals-latin-key: no - reason: स्थिति means situation, while the row is one case arm of a dispatch; प्रकरण is the case-arm noun
- row: ceterum - section: keywords - current: अन्यतम - proposed: अन्यतम - confidence: medium - equals-latin-key: no - reason: अन्यतम is a superlative meaning most other, not the default catch-all arm; the sense is right but the form is strained
- row: custodi - section: keywords - current: रक्षक - proposed: रक्षा_करो - confidence: medium - equals-latin-key: no - reason: रक्षक is a guard noun, but the row is an imperative early-return command, so the imperative verb form is required
- row: discerne - section: keywords - current: मिलाओ - proposed: मिलान_करो - confidence: medium - equals-latin-key: no - reason: मिलाओ means join or mix, but the row is pattern matching, a distinct comparison command
- row: dum - section: keywords - current: जबतक - proposed: जबतक - confidence: high - equals-latin-key: no - reason: no mismatch: जबतक as long as renders the while loop, while until is usque तक
- row: elige - section: keywords - current: चुनो - proposed: चुनो - confidence: medium - equals-latin-key: no - reason: चुनो is an imperative choose but does not carry the multi-way value dispatch the switch concept names
- row: ergo - section: keywords - current: अतः - proposed: अतः - confidence: high - equals-latin-key: no - reason: no mismatch: अतः is the single-statement body joint
- row: fac - section: keywords - current: करो - proposed: करो - confidence: high - equals-latin-key: no - reason: no mismatch: करो is the imperative do for a scoped block
- row: itera - section: keywords - current: दोहराओ - proposed: दोहराओ - confidence: high - equals-latin-key: no - reason: no mismatch: दोहराओ is the imperative for the for loop
- row: apud - section: keywords - current: पर - proposed: पर - confidence: high - equals-latin-key: no - reason: no mismatch: पर is the at marker for an attribute access
- row: secus - section: keywords - current: अन्यथा - proposed: अन्यथा - confidence: high - equals-latin-key: no - reason: no mismatch: अन्यथा is the else branch word
- row: si - section: keywords - current: यदि - proposed: यदि - confidence: high - equals-latin-key: no - reason: no mismatch: यदि is the if conditional word
- row: sin - section: keywords - current: अन्यथायदि - proposed: अन्यथा_यदि - confidence: medium - equals-latin-key: no - reason: the compound is written अन्यथायदि with no separator, which breaks the underscore join rule for multi-word names and hides the two words
- row: perge - section: keywords - current: जारी - proposed: जारी - confidence: medium - equals-latin-key: no - reason: जारी is an adjective meaning continued, not the imperative continue statement; जारी_रखो carries the command form
- row: redde - section: keywords - current: लौटाओ - proposed: लौटाओ - confidence: high - equals-latin-key: no - reason: no mismatch: लौटाओ is the imperative return
- row: rumpe - section: keywords - current: तोड़ो - proposed: तोड़ो - confidence: high - equals-latin-key: no - reason: no mismatch: तोड़ो is the imperative break
- row: tacet - section: keywords - current: मौन - proposed: मौन_रहो - confidence: medium - equals-latin-key: no - reason: मौन is the noun silence, but the row is a no-op statement and needs the imperative form
- row: adfirma - section: keywords - current: पुष्टि - proposed: पुष्टि_करो - confidence: medium - equals-latin-key: no - reason: पुष्टि is the noun confirmation, but the row is the imperative assert command
- row: cape - section: keywords - current: पकड़ो - proposed: पकड़ो - confidence: high - equals-latin-key: no - reason: no mismatch: पकड़ो is the imperative catch
- row: capta - section: keywords - current: जाल - proposed: जाल - confidence: high - equals-latin-key: no - reason: no mismatch: जाल is the trap noun
- row: cede - section: keywords - current: आगेबढ़ो - proposed: प्रदान_करो - confidence: medium - equals-latin-key: no - reason: आगेबढ़ो means move forward, which collides with perge continue; yield emits one stream item, so the give-forth verb is required
- row: iace - section: keywords - current: इधरफेंको - proposed: फेंको - confidence: high - equals-latin-key: no - reason: इधरफेंको fuses a deictic here onto throw with no separator and no semantic content; फेंको is the plain imperative throw
- row: mori - section: keywords - current: मरोजाओ - proposed: मरो_जाओ - confidence: medium - equals-latin-key: no - reason: मरोजाओ is two imperatives run together ungrammatically; the form needs the separator, though the panic sense is carried
- row: requirit - section: keywords - current: आवश्यक - proposed: मांगो - confidence: medium - equals-latin-key: no - reason: आवश्यक is an adjective meaning necessary, not the imperative recoverable guard the row spells
- row: reice - section: keywords - current: अस्वीकार - proposed: अस्वीकार_करो - confidence: medium - equals-latin-key: no - reason: अस्वीकार is the noun rejection, but the row is the imperative reject
- row: clausura - section: keywords - current: समापन - proposed: अनाम_फलन - confidence: high - equals-latin-key: no - reason: समापन means closure as conclusion or shutdown, not the anonymous function the lambda concept names
- row: falsum - section: keywords - current: असत्य - proposed: असत्य - confidence: high - equals-latin-key: no - reason: no mismatch: असत्य is the false literal
- row: verum - section: keywords - current: सत्य - proposed: सत्य - confidence: high - equals-latin-key: no - reason: no mismatch: सत्य is the true literal
- row: nonnumerus - section: keywords - current: nan - proposed: nan - confidence: high - equals-latin-key: no - reason: the value is the en nan token, not a Latin key; NaN is a fixed technical literal and needs no Hindi word
- row: aut - section: keywords - current: या - proposed: या - confidence: high - equals-latin-key: no - reason: no mismatch: या is the logical or
- row: est - section: keywords - current: है - proposed: है - confidence: high - equals-latin-key: no - reason: no mismatch: है renders the is type test, not value equality
- row: et - section: keywords - current: और - proposed: और - confidence: high - equals-latin-key: no - reason: no mismatch: और is the logical and
- row: non - section: keywords - current: नहीं - proposed: नहीं - confidence: high - equals-latin-key: no - reason: no mismatch: नहीं is the logical not
- row: vel - section: keywords - current: डिफ़ॉल्ट - proposed: अभाव_पूर्ति - confidence: high - equals-latin-key: no - reason: डिफ़ॉल्ट means default, but the row is nullish coalescing, a fallback that fills an absence; ceterum already owns the default arm
- row: ego - section: keywords - current: मैं - proposed: स्वयं - confidence: medium - equals-latin-key: no - reason: मैं is the first-person pronoun I, not the method receiver the self concept names; स्वयं is the reflexive
- row: finge - section: keywords - current: गढ़ो - proposed: प्रकार_बनाओ - confidence: medium - equals-latin-key: no - reason: गढ़ो means fabricate or invent, while the row constructs one tagged-union variant case
- row: implet - section: keywords - current: लागूकरता - proposed: लागू_करता - confidence: medium - equals-latin-key: no - reason: लागूकरता runs a verb compound together with no separator, breaking the underscore join rule for multi-word names
- row: mone - section: keywords - current: चेताओ - proposed: चेताओ - confidence: high - equals-latin-key: no - reason: no mismatch: चेताओ is the imperative warn
- row: nota - section: keywords - current: दिखाओ - proposed: छापो - confidence: high - equals-latin-key: no - reason: दिखाओ means show, but the row is the primary print output; छापो names printing
- row: scribe - section: keywords - current: लिखो - proposed: लिखो - confidence: high - equals-latin-key: no - reason: no mismatch: लिखो is the imperative write
- row: vide - section: keywords - current: देखो - proposed: जाँचो - confidence: medium - equals-latin-key: no - reason: देखो means look, which is too close to lege read; the row is diagnostic inspection, so the inspect verb is required
- row: argumenta - section: keywords - current: तर्क - proposed: इनपुट_तर्क - confidence: medium - equals-latin-key: no - reason: तर्क also means reasoning or logic, so the bare word for CLI args is ambiguous; the input qualifier disambiguates
- row: incipiet - section: keywords - current: आरंभasync - proposed: आरंभ_async - confidence: medium - equals-latin-key: no - reason: आरंभasync concatenates the Hindi word and the technical prefix with no separator, breaking the underscore join rule
- row: incipit - section: keywords - current: आरंभ - proposed: आरंभ - confidence: high - equals-latin-key: no - reason: no mismatch: आरंभ is the program entry word
- row: ad - section: keywords - current: सेवा - proposed: आह्वान - confidence: high - equals-latin-key: no - reason: सेवा means service, not the capability route call the row invokes; आह्वान is invocation
- row: fiet - section: keywords - current: async - proposed: async - confidence: high - equals-latin-key: no - reason: the value equals the en async token, not a Latin key; async is kept as a declared technical prefix and is a whole name here
- row: fiunt - section: keywords - current: जनक - proposed: जनक - confidence: high - equals-latin-key: no - reason: no mismatch: जनक is the generator or producer noun
- row: fient - section: keywords - current: async_जनक - proposed: async_जनक - confidence: high - equals-latin-key: no - reason: the compound is correctly joined by underscore and uses the declared async prefix; it is not a Devanagari-only name, which is intentional
- row: figendum - section: keywords - current: रुको_स्थिर - proposed: रुको_स्थिर - confidence: high - equals-latin-key: no - reason: no mismatch: the imperative रुको waits and स्थिर names the immutable binding
- row: variandum - section: keywords - current: रुको_चर - proposed: रुको_चर - confidence: medium - equals-latin-key: no - reason: the await imperative is right, but the variable stem चर differs from the परिवर्तनीय used by varia, so one concept has two stems
- row: reddet - section: keywords - current: रुको_लौटाओ - proposed: रुको_लौटाओ - confidence: high - equals-latin-key: no - reason: no mismatch: the imperative wait plus return names await-and-return
- row: tacebit - section: keywords - current: रुको - proposed: रुको - confidence: high - equals-latin-key: no - reason: no mismatch: रुको is the bare await that discards the result
- row: vacua - section: keywords - current: खाली - proposed: खाली - confidence: high - equals-latin-key: no - reason: no mismatch: खाली is the empty-collection marker
- row: primus_quem - section: keywords - current: primus_quem - proposed: primus_quem - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; the phrase has no settled Hindi equivalent
- row: ubi - section: keywords - current: ubi - proposed: ubi - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; ubi is a relative where-adverb with no fixed Hindi token
- row: summa - section: keywords - current: योग - proposed: योग - confidence: high - equals-latin-key: no - reason: no mismatch: योग is the sum noun
- row: filum - section: keywords - current: धागा - proposed: धागा - confidence: high - equals-latin-key: no - reason: no mismatch: धागा is the thread noun
- row: maxima - section: keywords - current: अधिकतम - proposed: अधिकतम - confidence: high - equals-latin-key: no - reason: no mismatch: अधिकतम is the maximum
- row: minima - section: keywords - current: न्यूनतम - proposed: न्यूनतम - confidence: high - equals-latin-key: no - reason: no mismatch: न्यूनतम is the minimum
- row: de - section: keywords - current: से - proposed: अपरिवर्तनीय_उधार - confidence: high - equals-latin-key: no - reason: से means from or with, not the immutable borrow parameter the ref concept names
- row: ex - section: keywords - current: सेवन - proposed: से - confidence: high - equals-latin-key: no - reason: सेवन means consumption, not the import or extraction source; the from sense is से, which the de row had wrongly taken
- row: in - section: keywords - current: परिवर्तनीय - proposed: परिवर्तनीय_उधार - confidence: high - equals-latin-key: no - reason: परिवर्तनीय duplicates the varia binding word and drops the borrow sense; the mutable borrow parameter needs its own compound
- row: own - section: keywords - current: स्वामित्व - proposed: स्वामित्व - confidence: high - equals-latin-key: no - reason: no mismatch: स्वामित्व is the ownership marker
- row: copy - section: keywords - current: प्रतिलिपि - proposed: प्रतिलिपि - confidence: high - equals-latin-key: no - reason: no mismatch: प्रतिलिपि is the copy marker
- row: axis - section: keywords - current: अक्ष - proposed: अक्ष - confidence: high - equals-latin-key: no - reason: no mismatch: अक्ष is the axis noun
- row: lege - section: keywords - current: पढ़ो - proposed: पढ़ो - confidence: high - equals-latin-key: no - reason: no mismatch: पढ़ो is the imperative read
- row: lineam - section: keywords - current: पंक्ति - proposed: पंक्ति - confidence: high - equals-latin-key: no - reason: no mismatch: पंक्ति is the line unit
- row: omnia - section: keywords - current: सभी - proposed: सभी - confidence: high - equals-latin-key: no - reason: no mismatch: सभी is the wildcard all
- row: scriptum - section: keywords - current: लिखित - proposed: स्वरूपण - confidence: high - equals-latin-key: no - reason: लिखित means written, but the row is template formatting, a rendering operation
- row: sparge - section: keywords - current: फैलाओ - proposed: फैलाओ - confidence: high - equals-latin-key: no - reason: no mismatch: फैलाओ is the imperative spread
- row: ut - section: keywords - current: रूपमें - proposed: रूप_में - confidence: medium - equals-latin-key: no - reason: रूपमें fuses two words with no separator, breaking the underscore join rule for multi-word names
- row: via - section: keywords - current: द्वारा - proposed: द्वारा - confidence: high - equals-latin-key: no - reason: no mismatch: द्वारा is the by-way-of marker, and the row is still an unreviewed proposal
- row: ante - section: keywords - current: पहले - proposed: पहले - confidence: high - equals-latin-key: no - reason: no mismatch: पहले is the range start
- row: inter - section: keywords - current: बीच - proposed: बीच - confidence: medium - equals-latin-key: no - reason: the word is a recognizer kept for the retired membership spelling, not an authoring surface, so it carries only migration duty
- row: intra - section: keywords - current: भीतर - proposed: भीतर - confidence: medium - equals-latin-key: no - reason: the word is a recognizer kept for the retired containment spelling, not an authoring surface, so it carries only migration duty
- row: per - section: keywords - current: प्रति - proposed: कदम - confidence: medium - equals-latin-key: no - reason: प्रति means per or toward, but the row is the stride of a range; कदम is the step noun
- row: ab - section: keywords - current: सीमा - proposed: सीमा_प्रारंभ - confidence: medium - equals-latin-key: no - reason: सीमा names a limit or bound, not the starting bound of a range that the from-above concept marks
- row: usque - section: keywords - current: तक - proposed: तक - confidence: high - equals-latin-key: no - reason: no mismatch: तक is the exclusive range end
- row: erratur - section: keywords - current: अपेक्षित_विफलता - proposed: अपेक्षित_विफलता - confidence: high - equals-latin-key: no - reason: no mismatch: the compound names expected failure in the declared underscore join
- row: fragilis - section: keywords - current: नाज़ुक - proposed: नाज़ुक - confidence: high - equals-latin-key: no - reason: no mismatch: नाज़ुक is the flaky-test adjective
- row: futurum - section: keywords - current: लंबित - proposed: लंबित - confidence: high - equals-latin-key: no - reason: no mismatch: लंबित is the pending todo marker
- row: metior - section: keywords - current: मापो - proposed: मापो - confidence: high - equals-latin-key: no - reason: no mismatch: मापो is the imperative bench
- row: omitte - section: keywords - current: छोड़ो - proposed: छोड़ो - confidence: high - equals-latin-key: no - reason: no mismatch: छोड़ो is the imperative skip
- row: postpara - section: keywords - current: पश्चतैयार - proposed: पश्च_तैयार - confidence: medium - equals-latin-key: no - reason: पश्चतैयार fuses two words with no separator, breaking the underscore join rule for multi-word names
- row: postparabit - section: keywords - current: पश्चतैयारasync - proposed: पश्च_तैयार_async - confidence: medium - equals-latin-key: no - reason: पश्चतैयारasync both fuses the Hindi words and appends the prefix with no separator
- row: praepara - section: keywords - current: पूर्वतैयार - proposed: पूर्व_तैयार - confidence: medium - equals-latin-key: no - reason: पूर्वतैयार fuses two words with no separator, breaking the underscore join rule for multi-word names
- row: praeparabit - section: keywords - current: पूर्वतैयारasync - proposed: पूर्व_तैयार_async - confidence: medium - equals-latin-key: no - reason: पूर्वतैयारasync both fuses the Hindi words and appends the prefix with no separator
- row: proba - section: keywords - current: परीक्षण - proposed: परीक्षण - confidence: high - equals-latin-key: no - reason: no mismatch: परीक्षण is the test noun
- row: probandum - section: keywords - current: परीक्षणसमूह - proposed: परीक्षण_समूह - confidence: medium - equals-latin-key: no - reason: परीक्षणसमूह fuses two words with no separator, hiding the suite relation that the describe concept names
- row: repete - section: keywords - current: पुनरावृत्ति - proposed: पुनरावृत्ति - confidence: medium - equals-latin-key: no - reason: पुनरावृत्ति is the repetition noun and is deliberately not the दोहराओ imperative that itera already owns
- row: solum - section: keywords - current: केवल - proposed: केवल - confidence: high - equals-latin-key: no - reason: no mismatch: केवल is the only modifier
- row: solum_in - section: keywords - current: केवलमें - proposed: केवल_में - confidence: medium - equals-latin-key: no - reason: केवलमें fuses two words with no separator, breaking the underscore join rule for multi-word names
- row: tag - section: keywords - current: टैग - proposed: टैग - confidence: high - equals-latin-key: no - reason: no mismatch: टैग is the test metadata tag
- row: temporis - section: keywords - current: समय - proposed: समय_सीमा - confidence: high - equals-latin-key: no - reason: समय alone means time, not the timeout limit the row declares; सीमा supplies the bound
- row: nulla - section: keywords - current: अभाव - proposed: अभाव - confidence: high - equals-latin-key: no - reason: no mismatch: अभाव is the null value literal
- row: cli - section: keywords - current: cli - proposed: cli - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; CLI is a fixed acronym
- row: commune - section: keywords - current: साझा - proposed: साझा - confidence: high - equals-latin-key: no - reason: no mismatch: साझा is the shared modifier
- row: cursor - section: keywords - current: संकेतक - proposed: संकेतक - confidence: high - equals-latin-key: no - reason: the keyword marks a stream cursor while the type row is an iterator; keeping संकेतक here and कर्सर there keeps two concepts distinct
- row: futura - section: keywords - current: भविष्य - proposed: भविष्य - confidence: high - equals-latin-key: no - reason: no mismatch: भविष्य is the future value noun
- row: imperium - section: keywords - current: आदेश - proposed: आदेश - confidence: high - equals-latin-key: no - reason: no mismatch: आदेश is the command noun
- row: operandus - section: keywords - current: ऑपरेंड - proposed: ऑपरेंड - confidence: high - equals-latin-key: no - reason: no mismatch: ऑपरेंड is the settled operand loan
- row: optio - section: keywords - current: विकल्प - proposed: विकल्प - confidence: high - equals-latin-key: no - reason: no mismatch: विकल्प is the single option noun
- row: conversion - section: keywords - current: बदलें - proposed: बदलें - confidence: low - equals-latin-key: no - reason: this key exists only in the Hindi pack, with no en or la concept row to render against, so the reviewer cannot judge it
- row: conversio - section: keywords - current: रूपांतरण - proposed: रूपांतरण - confidence: high - equals-latin-key: no - reason: no mismatch: रूपांतरण is the conversion noun
- row: nucleum - section: keywords - current: कर्नेल - proposed: कर्नेल - confidence: high - equals-latin-key: no - reason: no mismatch: कर्नेल is the kernel noun
- row: vertex - section: keywords - current: शीर्ष - proposed: शीर्ष - confidence: high - equals-latin-key: no - reason: no mismatch: शीर्ष is the vertex noun
- row: fragment - section: keywords - current: खंड - proposed: खंड - confidence: high - equals-latin-key: no - reason: no mismatch: खंड is the fragment noun
- row: nondum - section: keywords - current: अस्थिर - proposed: अस्थिर - confidence: high - equals-latin-key: no - reason: no mismatch: अस्थिर is the unstable marker
- row: verte - section: keywords - current: नाम_बदलें - proposed: नाम_बदलें - confidence: high - equals-latin-key: no - reason: no mismatch: the imperative बदलें with नाम names rename in the underscore join
- row: nomen - section: keywords - current: नाम - proposed: नाम - confidence: high - equals-latin-key: no - reason: no mismatch: नाम is the name noun
- row: radix - section: keywords - current: radix - proposed: radix - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; radix is the project name and an established borrowing
- row: brevis - section: keywords - current: संक्षिप्त - proposed: संक्षिप्त - confidence: high - equals-latin-key: no - reason: no mismatch: संक्षिप्त is the short form adjective
- row: descriptio - section: keywords - current: विवरण - proposed: विवरण - confidence: high - equals-latin-key: no - reason: no mismatch: विवरण is the description noun
- row: longum - section: keywords - current: विस्तृत - proposed: विस्तृत - confidence: high - equals-latin-key: no - reason: no mismatch: विस्तृत is the long form adjective
- row: lane - section: keywords - current: लेन - proposed: लेन - confidence: high - equals-latin-key: no - reason: no mismatch: लेन is the lane noun
- row: ubique - section: keywords - current: वैश्विक - proposed: वैश्विक - confidence: high - equals-latin-key: no - reason: no mismatch: वैश्विक is the global modifier
- row: annotatio - section: keywords - current: टीका - proposed: टीका - confidence: medium - equals-latin-key: no - reason: टीका means commentary or gloss, a plausible but narrower sense than the annotation metadata row
- row: versio - section: keywords - current: संस्करण - proposed: संस्करण - confidence: high - equals-latin-key: no - reason: no mismatch: संस्करण is the version noun
- row: imperia - section: keywords - current: आदेश_सूची - proposed: आदेश_सूची - confidence: high - equals-latin-key: no - reason: no mismatch: the compound names a command list in the underscore join
- row: reducta - section: keywords - current: समेटो - proposed: समेटो - confidence: high - equals-latin-key: no - reason: no mismatch: समेटो is the imperative reduce
- row: pro - section: keywords - current: अनुसार - proposed: प्रत्येक - confidence: high - equals-latin-key: no - reason: अनुसार means according to, but the row iterates each element of a collection
- row: factum - section: keywords - current: गुणनफल - proposed: गुणनफल - confidence: high - equals-latin-key: no - reason: no mismatch: गुणनफल is the product noun
- row: numeratio - section: keywords - current: गिनती - proposed: गिनती - confidence: high - equals-latin-key: no - reason: no mismatch: गिनती is the count noun
- row: quilibet - section: keywords - current: कोई - proposed: कोई - confidence: high - equals-latin-key: no - reason: no mismatch: कोई is the any quantifier
- row: argmaxima - section: keywords - current: argmax - proposed: argmax - confidence: high - equals-latin-key: no - reason: the value equals the en argmax token, not a Latin key; argmax is a fixed technical name
- row: argminima - section: keywords - current: argmin - proposed: argmin - confidence: high - equals-latin-key: no - reason: the value equals the en argmin token, not a Latin key; argmin is a fixed technical name
- row: ordinata - section: keywords - current: क्रमबद्ध_करो - proposed: क्रमबद्ध_करो - confidence: high - equals-latin-key: no - reason: no mismatch: the imperative करो with क्रमबद्ध names sorting in the underscore join
- row: ascendens - section: keywords - current: आरोही - proposed: आरोही - confidence: high - equals-latin-key: no - reason: no mismatch: आरोही is the ascending modifier
- row: descendens - section: keywords - current: अवरोही - proposed: अवरोही - confidence: high - equals-latin-key: no - reason: no mismatch: अवरोही is the descending modifier
- row: ascii - section: types - current: ascii - proposed: ascii - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; ASCII is a fixed encoding name
- row: textus - section: types - current: पाठ - proposed: पाठ - confidence: high - equals-latin-key: no - reason: no mismatch: पाठ is the string type noun
- row: numerus - section: types - current: संख्या - proposed: संख्या - confidence: high - equals-latin-key: no - reason: no mismatch: संख्या is the integer type noun
- row: modulus - section: types - current: मॉड्यूल_टाइप - proposed: मॉड्यूलर_पूर्णांक - confidence: high - equals-latin-key: no - reason: मॉड्यूल_टाइप says module_type, but the type is the modular wrapping integer; the keyword takes मॉड्यूलर and the type adds पूर्णांक
- row: saturatus - section: types - current: संतृप्त - proposed: संतृप्त - confidence: high - equals-latin-key: no - reason: no mismatch: संतृप्त is the saturating arithmetic type
- row: exactus - section: types - current: सटीक - proposed: अवरोधी - confidence: medium - equals-latin-key: no - reason: सटीक means precise, not the trapping arithmetic type that aborts on overflow; अवरोधी names trapping
- row: fractus - section: types - current: भिन्न - proposed: भिन्न - confidence: high - equals-latin-key: no - reason: no mismatch: भिन्न is the fractional floating type
- row: bivalens - section: types - current: तार्किक - proposed: तार्किक - confidence: high - equals-latin-key: no - reason: no mismatch: तार्किक is the boolean type
- row: nihil - section: types - current: कुछनहीं - proposed: कुछनहीं - confidence: high - equals-latin-key: no - reason: no mismatch: कुछनहीं is the none type
- row: vacuum - section: types - current: रिक्त - proposed: रिक्त - confidence: high - equals-latin-key: no - reason: no mismatch: रिक्त is the void type
- row: numquam - section: types - current: कभीनहीं - proposed: कभीनहीं - confidence: high - equals-latin-key: no - reason: no mismatch: कभीनहीं is the never bottom type
- row: ignotum - section: types - current: अज्ञात - proposed: अज्ञात - confidence: high - equals-latin-key: no - reason: no mismatch: अज्ञात is the unknown top type
- row: octeti - section: types - current: बाइट - proposed: बाइट - confidence: high - equals-latin-key: no - reason: no mismatch: बाइट is the bytes type
- row: octetus - section: types - current: बाइट_मान - proposed: बाइट_मान - confidence: high - equals-latin-key: no - reason: no mismatch: the compound names a byte value in the underscore join
- row: clampa - section: types - current: किनारा_स्थिर - proposed: किनारा_स्थिर - confidence: high - equals-latin-key: no - reason: no mismatch: the compound names the clamp edge policy in the underscore join
- row: reflexa - section: types - current: किनारा_परावर्तन - proposed: किनारा_परावर्तन - confidence: high - equals-latin-key: no - reason: no mismatch: the compound names the reflect edge policy in the underscore join
- row: involuta - section: types - current: किनारा_लपेट - proposed: किनारा_लपेट - confidence: high - equals-latin-key: no - reason: no mismatch: the compound names the wrap edge policy in the underscore join
- row: littera - section: types - current: अक्षर - proposed: अक्षर - confidence: high - equals-latin-key: no - reason: no mismatch: अक्षर is the character type
- row: regex - section: types - current: regex - proposed: regex - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; regex is a fixed technical name
- row: json - section: types - current: json - proposed: json - confidence: high - equals-latin-key: yes - reason: the value equals the Latin key and is kept by declaration; JSON is a fixed acronym
- row: valor - section: types - current: मान - proposed: मान - confidence: high - equals-latin-key: no - reason: no mismatch: मान is the dynamic value type
- row: instans - section: types - current: क्षण - proposed: क्षण - confidence: high - equals-latin-key: no - reason: no mismatch: क्षण is the instant timestamp type
- row: objectum - section: types - current: वस्तु - proposed: वस्तु - confidence: high - equals-latin-key: no - reason: no mismatch: वस्तु is the object type
- row: quidlibet - section: types - current: कुछभी - proposed: कुछभी - confidence: high - equals-latin-key: no - reason: no mismatch: कुछभी is the any escape type
- row: lista - section: types - current: सूची - proposed: सूची - confidence: high - equals-latin-key: no - reason: no mismatch: सूची is the list type
- row: tabula - section: types - current: तालिका - proposed: तालिका - confidence: high - equals-latin-key: no - reason: no mismatch: तालिका is the key-value table type
- row: tabulae - section: types - current: बही - proposed: बही - confidence: medium - equals-latin-key: no - reason: बही is the provisional ledger word and has not been validated by a reader of this language, so confidence stays medium
- row: copia - section: types - current: समुच्चय - proposed: समुच्चय - confidence: high - equals-latin-key: no - reason: no mismatch: समुच्चय is the set type
- row: promissum - section: types - current: वादा - proposed: वादा - confidence: high - equals-latin-key: no - reason: no mismatch: वादा is the promise type
- row: cursor - section: types - current: कर्सर - proposed: कर्सर - confidence: high - equals-latin-key: no - reason: the type is the iterator object while the keyword row is a stream marker; कर्सर here and संकेतक there keeps two concepts distinct
- row: queue - section: types - current: कतार - proposed: कतार - confidence: high - equals-latin-key: no - reason: no mismatch: कतार is the queue type
- row: stack - section: types - current: स्टैक - proposed: स्टैक - confidence: high - equals-latin-key: no - reason: no mismatch: स्टैक is the settled stack loan
- row: tensor - section: types - current: टेंसर - proposed: टेंसर - confidence: high - equals-latin-key: no - reason: no mismatch: टेंसर is the settled tensor loan
- row: vector - section: types - current: सदिश - proposed: सदिश - confidence: high - equals-latin-key: no - reason: no mismatch: सदिश is the vector noun
- row: matrix - section: types - current: आव्यूह - proposed: आव्यूह - confidence: high - equals-latin-key: no - reason: no mismatch: आव्यूह is the matrix noun
- row: sparsa - section: types - current: विरल - proposed: विरल - confidence: high - equals-latin-key: no - reason: no mismatch: विरल is the sparse adjective
- row: atomic - section: types - current: परमाणु - proposed: परमाणु - confidence: medium - equals-latin-key: no - reason: परमाणु is the noun atom, while the row is the atomic type; the noun form is usable but not the adjective
- row: intervallum - section: types - current: अंतराल - proposed: अंतराल - confidence: high - equals-latin-key: no - reason: no mismatch: अंतराल is the interval type
- row: filtrum - section: types - current: फ़िल्टर - proposed: फ़िल्टर - confidence: high - equals-latin-key: no - reason: no mismatch: फ़िल्टर is the settled filter loan
- row: series - section: types - current: शृंखला - proposed: शृंखला - confidence: high - equals-latin-key: no - reason: no mismatch: शृंखला is the series noun
- row: census - section: types - current: गणना - proposed: गणना - confidence: high - equals-latin-key: no - reason: no mismatch: गणना is the census count noun
- row: ratio - section: types - current: ratio - proposed: ratio - confidence: medium - equals-latin-key: yes - reason: the value equals the Latin key, but the en concept is record, so the row keeps a Latin word whose Hindi sense is not the concept
- row: sermo - section: types - current: चैनल - proposed: चैनल - confidence: high - equals-latin-key: no - reason: no mismatch: चैनल is the channel noun
- row: scrinium - section: types - current: फ़्रेम - proposed: फ़्रेम - confidence: high - equals-latin-key: no - reason: no mismatch: फ़्रेम is the frame noun
- row: trapped - section: types - current: फँसा - proposed: फँसा - confidence: high - equals-latin-key: no - reason: no mismatch: फँसा is the trapped marker
- row: meus - section: types - current: भेजना - proposed: भेजो - confidence: medium - equals-latin-key: no - reason: भेजना is the infinitive to send, but the view method is an imperative send command
- row: tuus - section: types - current: पाना - proposed: पाओ - confidence: medium - equals-latin-key: no - reason: पाना is the infinitive to receive, but the view method is an imperative receive command
