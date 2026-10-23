## Conventions

Scope: every row of `[keywords]` (line 5, 159 rows) and `[types]` (line 170, 47 rows)
in `locale/th-TH/pack.toml`, read against the concept each row renders in
`locale/en/pack.toml` (`[keywords]` line 5, `[types]` line 185) and
`locale/la/pack.toml` (`[keywords]` line 10, `[types]` line 180), the per-key
concept and rationale in `docs/design/llm-locale.md` (lines 64 to 290), the verb
form law in `docs/stdlib/morphologia.md` (lines 79 to 150 and 288 to 316), and the
definition of a pack row in `docs/design/reader-locale.md` (lines 1 to 60). This
review edits no pack: the rulings below are proposals for the pack owner. The
operator ruling of 2026-10-11 governs all of them, quoted: a row must be the
semantically accurate translation, and being the compiled spelling does not make
it right.

**(a) Part of speech and verb form.** A statement keyword is a verb in the plain
imperative stem, the form Thai uses to command an action, which is the
`Imperativus` posture of `docs/stdlib/morphologia.md`: bare stem, no polite
particle, no sentence-final particle, no invented direct object. A keyword that
names a category (`genus`, `typus`, `ordo`, `discretio`, `implendum`) is a noun,
because it declares what a thing is rather than doing something. A modifier
(`immutata`, `sponte`, `commune`) is a bare adjective or participle. The only
affixes a row may carry are the two aspect markers the pack already uses
systematically: `จะ` for the async pair (`praeparabit`, `postparabit`) and `รอ`
for the await posture (`figendum`, `variandum`, `reddet`, `tacebit`); neither is
register, so neither is a violation of the bare-stem rule. What the pack does
today: `sit` (line 15) is `กำหนด`, a verb meaning to determine or set, so the let
binding reads as an assignment command standing where a binder belongs, and the
proposal is `ผูก` (bind); `iacit` (line 24) is `โยนผล`, literally throws results,
a verb phrase that invents a result object for a modifier that only says the
function may throw, and the proposal is `อาจโยน` (may throw). The same two
questions were put to every one of the 206 rows: is this row an action, a
category or a modifier, and is the word in the form that role takes. Every row
whose value is a verb phrase in a modifier slot or an adjective in a statement
slot is called out in the findings.

**(b) Register.** Formal, technical Thai for everything the compiler names; an
everyday word only where the everyday word is also the technical one (`อ่าน`
read, `เขียน` write, `หยุด` stop, `เลือก` select, `ทดสอบ` test). Three classes of
loan are kept. First, the fixed Thai computing loanwords, written the way Thai
computing text writes them (`ฟังก์ชัน`, `โมดูลัส`, `อาร์กิวเมนต์`, `ไบต์`,
`เทนเซอร์`, `เวกเตอร์`, `เมทริกซ์`, `เคอร์เนล`, `คิว`, `สแต็ก`, `เวอร์ชัน`), and
corrected in spelling where the pack's form is a misspelling (`สแต็ก`). Second,
international standard names that nobody translates and Thai technical text
writes in Latin (`ascii`, `json`, `regex`, `cli`, `nan`). Third, the formal
Pali-Sanskrit coinage where the Thai computing literature uses it and the
everyday word is taken or ambiguous: `อันตรภาค` for the numeric interval, because
`ช่วง` already names the `ab` range operator, and `ตัวถูกดำเนินการ` for operand,
which is the standard textbook term. A transliteration is not acceptable when it
transliterates the wrong concept: `instans` is a point in time, not an instance,
so `อินสแตนซ์` is wrong and `ชั่วขณะ` is proposed; `modulus` is modular arithmetic,
which `โมดูลัส` does state. Latin is never kept merely because it is what the
compiler spells; if the row's concept has an accurate Thai word, the Thai word
wins.

**(c) Script.** Thai is written without spaces between words, so a value of
several words is still one token and needs no separator in order to lex. Vowel
and tone marks are combining marks on the base consonant. The one hazard that
matters here is สระอำ: it may be written composed (U+0E33) or decomposed (U+0E4D
U+0E32), and NFKC, which `docs/design/reader-locale.md` records as a global lex
rule (goal and decision G1), folds the composed form to the decomposed one. Pack
matching survives either spelling, because the keyword and type reverse maps are
NFKC-normalised as well (`crates/radix-semantic/src/locale.rs`), but the pack
should not hold two spellings of one word. It does today: sixteen of the 206
values store the composed form (`importa` `นำเข้า`, `ordo` `ลำดับ`, `sit` `กำหนด`,
`fac` `ทำ`, `minima` `ต่ำสุด`, `imperium` `คำสั่ง`, `promissum` `คำมั่น`, `census`
`สำมะโน` and their class), while `numerus` `จํานวน` and `argminima` `ดัชนีค่าต่ําสุด`
already store the NFKC form, so `minima` and `argminima` spell the same word two
ways inside one pack. The ruling is one NFKC form for every value in the pack,
because that is the form the lexer holds and the form the reverse maps are keyed
by; the eleven rows that change for no other reason carry an NFKC-only proposal
that renders identically, and the five rows that are replaced on other grounds
are normalised in the same pass. Every Thai form in the prose of this document is
written in its composed spelling because that is what a reader sees on screen;
the proposed column of the findings carries the NFKC form a pack value has to
store, so where the two differ they differ only in the encoding of สระอำ. There is
no case to fold and no bidi ordering,
so Thai escapes the two ambiguities that dominated the Arabic and Vietnamese
reviews. On the separator question: only two rows carry an underscore,
`primus_quem` (line 91) and `erratur` (line 116). For `primus_quem` the underscore
is part of the Latin key itself and the row is a Latin identity row, so it must
stay. For `erratur` as `คาดหวัง_ล้มเหลว` the underscore is the wrong device: Thai
has no inter-word boundary to mark, so a compound is simply written together, and
the underscore adds a Latin typographic habit to a script that does not need it.
It is tolerated only where a Latin identity requires the exact Latin key, and
`คาดล้มเหลว` is proposed for `erratur`.

**(d) One key spelled in two sections.** Two rows may share a stem when they name
the same concept in two roles, and must not share a stem when they name different
concepts. In either case the surface strings must differ, so that each slot stays
decidable. Same concept, shared stem, allowed: `modulus` is a keyword at line 13
(`โมดูลัส`) and a type at line 174 (`ค่ามอดุลัส`); both name modular arithmetic, and
the `ค่า` head keeps the strings apart, exactly as `en` writes `wrapping` and
`wrapping_ty` and `la` writes `modulus` and `modulus_t`. Both rows are kept.
Different concepts, shared stem, a defect: `genus` at line 9 is `ชนิด`, which
means kind or type, and `typus` at line 16 is `ชนิดนามแฝง`, alias type. A class
and a type are different concepts, so `genus` must not borrow `typus`'s root;
`typus` keeps `ชนิดนามแฝง` because a `typus` declaration is a type alias
(`grammar/EBNF.md`, `typus_decl`), and `genus` takes `คลาส`. Cross-section
near-hits that are correct and stay: `quilibet` `ใดก็ได้` and `quidlibet`
`อะไรก็ได้`, `cursor` `เคอร์เซอร์` and `cursor` `ตัววนซ้ำ`, `imperium` `คำสั่ง` and
`imperia` `ชุดคำสั่ง`, `tabula` `ตาราง` and `tabulae` `สมุดบัญชี`. A repeat of the
identical string in two different rows of the same section, which is always a
defect, exists once today: `varia` and `in` both read `แก้ไขได้`, and `in` takes
`ยืมแก้ไขได้`.

**(e) Keeping Latin, and how it is declared.** A row may keep its Latin spelling
in three cases: the word is an international standard name; the word is the
identity of a compiler-owned annotation family that the grammar spells; or the
row is a Latin identity row that every pack keeps (`primus_quem`). A pack
declares that decision simply by copying the Latin key as the row value, which is
what the identity rows do; there is no separate marker. The rows whose value
equals the Latin key today are `ascii` (171), `cli` (133), `json` (191),
`primus_quem` (90), `radix` (148), `ratio` (217), `regex` (190) and `ubi` (91).
Ruling on each: `ascii`, `json`, `regex`, `cli` and `primus_quem` keep the Latin
form as standard or identity names; `ubi` takes `ซึ่ง` because the row introduces
the where predicate and the Latin word tells a Thai reader nothing; `ratio` takes
`เรกคอร์ด` because the concept is a relational record and the Latin `ratio` does
not state it; `radix` keeps the Latin form, for the reason in the next paragraph.
`nonnumerus` (line 64) is not a Latin identity row but is byte equal to the `en`
value `nan`; that is correct and is kept, because NaN is an IEEE name that Thai
technical text writes `nan`, so it stands as a standard name rather than as a
Latin identity. On `radix`: the keyword row at line 148 names the compiler-owned
`@ radix` metadata family (`grammar/EBNF.md`, `radix_directive`;
`docs/design/annotation-sugar.md`), which is a morphological stem, while the row
of the same name in this pack's `[intrinsics]` section (line 3664) is `รากที่สอง`,
square root. One word does not have to serve both: they are different concepts,
the intrinsic row is outside this unit, and keeping the annotation root Latin is
what keeps the two apart in one rendered source. A translation such as `ราก` would
put a bare `ราก` one word away from `รากที่สอง`, which is the kind of near homonym
a locale exists to remove. The keyword row keeps `radix`.

**(f) What this reviewer could not settle.** The `await` family
(`figendum` `รอคง`, `variandum` `รอแปร`, `reddet` `รอคืน`, `tacebit` `รอทิ้ง`) is
marked medium rather than high: the scheme is systematic and survives reading,
but a native author may prefer a fuller phrase, and picking between the terse
scheme and a fuller one needs a native speaker, not an argument. The `conversion`
row (line 140) is the pack's only key with no `en` and no `la` counterpart, so its
concept cannot be checked against a source row here; it is marked low rather than
guessed, and its value `แปลง` is left alone. `stack` `สแตก` against `สแต็ก` is a
spelling question a Thai reviewer should sign off, so it is medium. Whether
`โกลบอล` should give way to a native phrase for the `ubique` annotation is a
register preference with no right answer in the sources, so it is kept. Nothing
here rules on the `[intrinsics]` and `[diagnostics.*]` sections, which are outside
this unit, including the many intrinsics values that are not NFKC-normalised
(`planata` `ทําให้แบน`) and the near homonym `radix`/`radix_cubica` there. One
observation outside the write scope: the exemplar
`locale/th-TH/exemplars/salve-munde.th-TH.fab` uses `แสดง` for `nota` and `เริ่ม`
for `incipit`, and `แสดง` is not this pack's spelling for `nota` (`บันทึก` today,
`พิมพ์` proposed), so the exemplar and the pack disagree; the exemplar is not
edited here. Finally, no row in this pack uses a space inside a value, and this
review does not decide whether a value ever may, so the underscore rule above
covers multi-word names only.

## Findings

- row: discretio - section: keywords - current: สหภาพแยก - proposed: ยูเนียน - confidence: medium - equals-latin-key: no - reason: the concept is a tagged union type, while สหภาพ names a political union and แยก contradicts it inside one compound
- row: fixum - section: keywords - current: คงที่ - proposed: ค่าคงที่ - confidence: medium - equals-latin-key: no - reason: คงที่ is a bare adjective, but the row declares a compile-time constant and must not read like the immutata modifier
- row: functio - section: keywords - current: ฟังก์ชัน - proposed: ฟังก์ชัน - confidence: high - equals-latin-key: no - reason: ฟังก์ชัน is the fixed Thai computing noun for a callable declaration
- row: genus - section: keywords - current: ชนิด - proposed: คลาส - confidence: high - equals-latin-key: no - reason: ชนิด means kind or type, so it names the typus concept rather than a class declaration
- row: implendum - section: keywords - current: สัญญา - proposed: สัญญา - confidence: high - equals-latin-key: no - reason: a signature-only contract is exactly สัญญา, the agreement a genus fulfils
- row: importa - section: keywords - current: นำเข้า - proposed: นําเข้า - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: regio - section: keywords - current: โมดูล - proposed: โมดูล - confidence: high - equals-latin-key: no - reason: โมดูล is the established Thai computing noun for a module
- row: modulus - section: keywords - current: โมดูลัส - proposed: โมดูลัส - confidence: high - equals-latin-key: no - reason: the keyword and the type row name one concept, modular arithmetic, and share the มอดุลัส stem behind distinct surfaces
- row: ordo - section: keywords - current: ลำดับ - proposed: การแจกแจง - confidence: medium - equals-latin-key: no - reason: ลำดับ is an ordering, while the declaration enumerates named variants
- row: sit - section: keywords - current: กำหนด - proposed: ผูก - confidence: medium - equals-latin-key: no - reason: กำหนด is the command to determine or set, so it reads as an assignment where the row is a binder
- row: typus - section: keywords - current: ชนิดนามแฝง - proposed: ชนิดนามแฝง - confidence: high - equals-latin-key: no - reason: typus declares a type alias, which ชนิดนามแฝง states exactly
- row: schema - section: keywords - current: สคีมา - proposed: สคีมา - confidence: high - equals-latin-key: no - reason: สคีมา is the fixed Thai loan for a schema declaration
- row: columna - section: keywords - current: คอลัมน์ - proposed: คอลัมน์ - confidence: high - equals-latin-key: no - reason: คอลัมน์ is the fixed Thai loan for a column
- row: varia - section: keywords - current: แก้ไขได้ - proposed: เปลี่ยนค่าได้ - confidence: medium - equals-latin-key: no - reason: the row declares a writable binding, while แก้ไขได้ is a bare ability adjective that in already uses
- row: ceteri - section: keywords - current: ที่เหลือ - proposed: ที่เหลือ - confidence: high - equals-latin-key: no - reason: ที่เหลือ is the rest-of-list reading that the rest binding needs
- row: errata - section: keywords - current: ข้อผิดพลาด - proposed: ข้อผิดพลาด - confidence: high - equals-latin-key: no - reason: ข้อผิดพลาด is the noun for declared error types, distinct from the iacit verb
- row: exitus - section: keywords - current: ทางออก - proposed: ทางออก - confidence: high - equals-latin-key: no - reason: ทางออก names the exit-code direction the declaration sets
- row: generis - section: keywords - current: ของชนิด - proposed: ของคลาส - confidence: medium - equals-latin-key: no - reason: a class-level member takes the class word, and ชนิด is the root this review assigns to typus
- row: iacit - section: keywords - current: โยนผล - proposed: อาจโยน - confidence: medium - equals-latin-key: no - reason: โยนผล invents a result object for a modifier that only states the function may throw
- row: immutata - section: keywords - current: ไม่เปลี่ยนแปลง - proposed: อ่านอย่างเดียว - confidence: medium - equals-latin-key: no - reason: ไม่เปลี่ยนแปลง states a behaviour that does not change, where the modifier grants read-only permission
- row: interna - section: keywords - current: ภายในองค์กร - proposed: ภายในโมดูล - confidence: medium - equals-latin-key: no - reason: ภายในองค์กร means inside the organisation, not the visibility inside a module
- row: magnitudo - section: keywords - current: ขนาด - proposed: ขนาด - confidence: high - equals-latin-key: no - reason: ขนาด is the size of a const-generic parameter
- row: iuncta - section: keywords - current: ทูเพิล - proposed: ทูเพิล - confidence: high - equals-latin-key: no - reason: ทูเพิล is the fixed Thai loan for a tuple
- row: optiones - section: keywords - current: ทางเลือก - proposed: ชุดตัวเลือก - confidence: medium - equals-latin-key: no - reason: ทางเลือก means an alternative, not the bundle of CLI options that optio forms
- row: praefixum - section: keywords - current: นำหน้า - proposed: เวลาคอมไพล์ - confidence: medium - equals-latin-key: no - reason: นำหน้า translates the Latin prefix morpheme while the modifier means compile-time evaluation
- row: insere - section: keywords - current: ฝัง - proposed: ฝัง - confidence: high - equals-latin-key: no - reason: ฝัง is the imperative embed that the declaration performs
- row: privata - section: keywords - current: ส่วนตัว - proposed: ส่วนตัว - confidence: high - equals-latin-key: no - reason: ส่วนตัว is the Thai visibility word for private
- row: protecta - section: keywords - current: ป้องกัน - proposed: ป้องกัน - confidence: high - equals-latin-key: no - reason: ป้องกัน is the Thai visibility verb for protected and stays distinct from the custodi word คุ้มครอง
- row: publica - section: keywords - current: สาธารณะ - proposed: สาธารณะ - confidence: high - equals-latin-key: no - reason: สาธารณะ is the Thai visibility word for public
- row: sponte - section: keywords - current: สมัครใจ - proposed: ไม่บังคับ - confidence: medium - equals-latin-key: no - reason: สมัครใจ describes a willing person, not an optional argument slot
- row: casu - section: keywords - current: กรณี - proposed: กรณี - confidence: high - equals-latin-key: no - reason: กรณี is the case-arm noun the dispatch selects
- row: ceterum - section: keywords - current: อื่น - proposed: โดยปริยาย - confidence: medium - equals-latin-key: no - reason: อื่น alone is the adjective other and does not name the catch-all arm
- row: custodi - section: keywords - current: คุ้มครอง - proposed: คุ้มครอง - confidence: high - equals-latin-key: no - reason: คุ้มครอง is the bare imperative guard that opens the precondition statement
- row: discerne - section: keywords - current: แยก - proposed: จับคู่ - confidence: medium - equals-latin-key: no - reason: แยก means separate and reuses a morpheme of the discretio value, where the statement matches a pattern
- row: dum - section: keywords - current: ขณะ - proposed: ขณะที่ - confidence: medium - equals-latin-key: no - reason: bare ขณะ is the noun moment and needs ที่ to read as the conjunction while
- row: elige - section: keywords - current: เลือก - proposed: เลือก - confidence: high - equals-latin-key: no - reason: เลือก is the imperative select that the dispatch statement performs
- row: ergo - section: keywords - current: ดังนั้น - proposed: จึง - confidence: medium - equals-latin-key: no - reason: ดังนั้น is the inferential therefore of a conclusion, not the body joint that starts the statement
- row: fac - section: keywords - current: ทำ - proposed: ทํา - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: itera - section: keywords - current: วน - proposed: วนซ้ํา - confidence: medium - equals-latin-key: no - reason: bare วน is the verb go round, which reads as an unbounded spin rather than a per-element loop
- row: apud - section: keywords - current: ที่ - proposed: ที่ - confidence: high - equals-latin-key: no - reason: ที่ is the locative particle the at clause needs
- row: secus - section: keywords - current: มิฉะนั้น - proposed: มิฉะนั้น - confidence: high - equals-latin-key: no - reason: มิฉะนั้น is the idiomatic otherwise of the alternative branch
- row: si - section: keywords - current: ถ้า - proposed: ถ้า - confidence: high - equals-latin-key: no - reason: ถ้า is the standard Thai if conjunction
- row: sin - section: keywords - current: ถ้าไม่ก็ - proposed: มิฉะนั้นถ้า - confidence: medium - equals-latin-key: no - reason: ถ้าไม่ก็ reads as if not then, which loses the elif cascade
- row: perge - section: keywords - current: ไปต่อ - proposed: ไปต่อ - confidence: high - equals-latin-key: no - reason: ไปต่อ is the imperative go on that continue needs
- row: redde - section: keywords - current: คืน - proposed: คืน - confidence: high - equals-latin-key: no - reason: คืน is the imperative give back that return performs
- row: rumpe - section: keywords - current: หยุด - proposed: หยุด - confidence: high - equals-latin-key: no - reason: หยุด is the imperative halt that leaves the loop
- row: tacet - section: keywords - current: เงียบ - proposed: ปล่อยผ่าน - confidence: medium - equals-latin-key: no - reason: เงียบ is the adjective quiet, not the no-op statement that lets control pass
- row: adfirma - section: keywords - current: ยืนยัน - proposed: ยืนยัน - confidence: high - equals-latin-key: no - reason: ยืนยัน is the imperative confirm an assertion performs
- row: cape - section: keywords - current: จับ - proposed: จับ - confidence: high - equals-latin-key: no - reason: จับ is the imperative catch that opens the handler
- row: capta - section: keywords - current: ดัก - proposed: ดัก - confidence: high - equals-latin-key: no - reason: ดัก is the imperative trap that opens the trap block
- row: cede - section: keywords - current: ให้ - proposed: ให้ - confidence: high - equals-latin-key: no - reason: ให้ is the imperative give out that yields one stream item
- row: iace - section: keywords - current: โยน - proposed: โยน - confidence: high - equals-latin-key: no - reason: โยน is the imperative throw of the error statement
- row: mori - section: keywords - current: ตาย - proposed: ตาย - confidence: high - equals-latin-key: no - reason: ตาย is the die verb behind the unrecoverable abort
- row: requirit - section: keywords - current: ต้องการ - proposed: ต้องมี - confidence: medium - equals-latin-key: no - reason: ต้องการ is a wish verb of an agent and does not state the precondition enforced here
- row: reice - section: keywords - current: ปฏิเสธ - proposed: ปฏิเสธ - confidence: high - equals-latin-key: no - reason: ปฏิเสธ is the imperative reject of the recovery statement
- row: clausura - section: keywords - current: ปิดล้อม - proposed: แลมบ์ดา - confidence: medium - equals-latin-key: no - reason: ปิดล้อม means besiege, a literal calque that names no anonymous function
- row: falsum - section: keywords - current: เท็จ - proposed: เท็จ - confidence: high - equals-latin-key: no - reason: เท็จ is the standard Thai false literal
- row: verum - section: keywords - current: จริง - proposed: จริง - confidence: high - equals-latin-key: no - reason: จริง is the standard Thai true literal
- row: nonnumerus - section: keywords - current: nan - proposed: nan - confidence: medium - equals-latin-key: no - reason: nan tracks the en row byte for byte and is the IEEE name Thai technical text writes
- row: aut - section: keywords - current: หรือ - proposed: หรือ - confidence: high - equals-latin-key: no - reason: หรือ is the standard logical or
- row: est - section: keywords - current: เป็น - proposed: เป็น - confidence: high - equals-latin-key: no - reason: เป็น is the be verb a type test reads as
- row: et - section: keywords - current: และ - proposed: และ - confidence: high - equals-latin-key: no - reason: และ is the standard logical and
- row: non - section: keywords - current: ไม่ - proposed: ไม่ - confidence: high - equals-latin-key: no - reason: ไม่ is the standard negation
- row: vel - section: keywords - current: หรือว่าง - proposed: หรือว่าง - confidence: high - equals-latin-key: no - reason: หรือว่าง states the or-if-empty fallback that coalescing performs
- row: ego - section: keywords - current: ตัวฉัน - proposed: ตัวเอง - confidence: medium - equals-latin-key: no - reason: ตัวฉัน is the first-person pronoun I, not the receiver that names the object itself
- row: finge - section: keywords - current: สร้าง - proposed: สร้างกรณี - confidence: medium - equals-latin-key: no - reason: สร้าง is plain create and would also read as genus construction, which variant construction is kept apart from
- row: implet - section: keywords - current: เติมเต็ม - proposed: ทําตามสัญญา - confidence: medium - equals-latin-key: no - reason: เติมเต็ม is the literal fill up calque and states no class-to-contract relation
- row: mone - section: keywords - current: เตือน - proposed: เตือน - confidence: high - equals-latin-key: no - reason: เตือน is the imperative warn the diagnostic call performs
- row: nota - section: keywords - current: บันทึก - proposed: พิมพ์ - confidence: high - equals-latin-key: no - reason: บันทึก means record or save, so the primary output statement reads as persistence rather than printing
- row: scribe - section: keywords - current: เขียน - proposed: เขียน - confidence: high - equals-latin-key: no - reason: เขียน is the imperative write of the raw output
- row: vide - section: keywords - current: ดู - proposed: ตรวจสอบ - confidence: medium - equals-latin-key: no - reason: ดู is the perception verb look at and carries no diagnostic sense
- row: argumenta - section: keywords - current: อาร์กิวเมนต์ - proposed: อาร์กิวเมนต์ - confidence: high - equals-latin-key: no - reason: อาร์กิวเมนต์ is the fixed Thai loan for command-line arguments
- row: incipiet - section: keywords - current: เริ่มอะซิงก์ - proposed: เริ่มอะซิงก์ - confidence: high - equals-latin-key: no - reason: เริ่มอะซิงก์ composes the entry word with the อะซิงก์ async marker that fiet also uses
- row: incipit - section: keywords - current: เริ่ม - proposed: เริ่ม - confidence: high - equals-latin-key: no - reason: เริ่ม is the imperative start of the program entry
- row: ad - section: keywords - current: ถึง - proposed: เรียก - confidence: high - equals-latin-key: no - reason: ถึง is the preposition to and does not name the route call the keyword performs
- row: fiet - section: keywords - current: อะซิงก์ - proposed: อะซิงก์ - confidence: high - equals-latin-key: no - reason: อะซิงก์ is the fixed Thai loan for async
- row: fiunt - section: keywords - current: สตรีม - proposed: สตรีม - confidence: medium - equals-latin-key: no - reason: สตรีม names the synchronous generator as the stream it produces
- row: fient - section: keywords - current: สตรีมอะซิงก์ - proposed: สตรีมอะซิงก์ - confidence: high - equals-latin-key: no - reason: สตรีมอะซิงก์ composes the stream word with the async marker
- row: figendum - section: keywords - current: รอคง - proposed: รอคง - confidence: medium - equals-latin-key: no - reason: the รอ prefix marks await and คง marks the fixed binding, the terse scheme this pack uses for the await family
- row: variandum - section: keywords - current: รอแปร - proposed: รอแปร - confidence: medium - equals-latin-key: no - reason: รอแปร marks awaiting into a mutable binding by the same terse scheme
- row: reddet - section: keywords - current: รอคืน - proposed: รอคืน - confidence: medium - equals-latin-key: no - reason: รอคืน marks await then return and shares คืน with the redde statement
- row: tacebit - section: keywords - current: รอทิ้ง - proposed: รอทิ้ง - confidence: medium - equals-latin-key: no - reason: รอทิ้ง marks await then discard, which is what the await row without a binding does
- row: vacua - section: keywords - current: เซตว่าง - proposed: ว่าง - confidence: medium - equals-latin-key: no - reason: เซตว่าง commits the marker to the set type that copia also spells ชุด, while the marker covers any collection
- row: primus_quem - section: keywords - current: primus_quem - proposed: primus_quem - confidence: high - equals-latin-key: yes - reason: the selection head is a Latin identity row that en and la keep, and Thai has no standard phrase for it
- row: ubi - section: keywords - current: ubi - proposed: ซึ่ง - confidence: medium - equals-latin-key: no - reason: the row introduces the where predicate and the Latin identity gives a Thai reader nothing
- row: summa - section: keywords - current: ผลรวม - proposed: ผลรวม - confidence: high - equals-latin-key: no - reason: ผลรวม is the Thai noun for a sum reduction
- row: filum - section: keywords - current: เส้นใย - proposed: เธรด - confidence: medium - equals-latin-key: no - reason: เส้นใย is a textile fibre, not the execution thread the row names
- row: maxima - section: keywords - current: สูงสุด - proposed: สูงสุด - confidence: high - equals-latin-key: no - reason: สูงสุด is the Thai maximum
- row: minima - section: keywords - current: ต่ำสุด - proposed: ต่ําสุด - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: de - section: keywords - current: จาก - proposed: อ้างอิง - confidence: high - equals-latin-key: no - reason: จาก is the preposition from and does not name the immutable borrow parameter
- row: ex - section: keywords - current: ออก - proposed: จาก - confidence: medium - equals-latin-key: no - reason: ออก is the verb to go out, while the row introduces the import source that จาก states
- row: in - section: keywords - current: แก้ไขได้ - proposed: ยืมแก้ไขได้ - confidence: medium - equals-latin-key: no - reason: the row is a mutable borrow parameter and today repeats the varia string exactly
- row: own - section: keywords - current: เป็นเจ้าของ - proposed: เป็นเจ้าของ - confidence: high - equals-latin-key: no - reason: เป็นเจ้าของ states the ownership transfer the modifier marks
- row: copy - section: keywords - current: สำเนา - proposed: สําเนา - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: axis - section: keywords - current: แกน - proposed: แกน - confidence: high - equals-latin-key: no - reason: แกน is the Thai axis
- row: lege - section: keywords - current: อ่าน - proposed: อ่าน - confidence: high - equals-latin-key: no - reason: อ่าน is the imperative read
- row: lineam - section: keywords - current: บรรทัด - proposed: บรรทัด - confidence: high - equals-latin-key: no - reason: บรรทัด is the Thai noun for a line
- row: omnia - section: keywords - current: ทั้งหมด - proposed: ทั้งหมด - confidence: high - equals-latin-key: no - reason: ทั้งหมด is the Thai all of a wildcard import
- row: scriptum - section: keywords - current: จารึก - proposed: จัดรูปแบบ - confidence: medium - equals-latin-key: no - reason: จารึก is the archaic noun inscription, not the format operation
- row: sparge - section: keywords - current: กระจาย - proposed: กระจาย - confidence: high - equals-latin-key: no - reason: กระจาย is the imperative spread of a destructuring
- row: ut - section: keywords - current: ในชื่อ - proposed: ในนาม - confidence: medium - equals-latin-key: no - reason: ในชื่อ means in the name field and does not state the aliasing relation
- row: via - section: keywords - current: ผ่านทาง - proposed: ผ่านทาง - confidence: high - equals-latin-key: no - reason: ผ่านทาง is the Thai by way of for the convert hint clause
- row: ante - section: keywords - current: ก่อน - proposed: ก่อน - confidence: high - equals-latin-key: no - reason: ก่อน is the Thai before that opens the range
- row: inter - section: keywords - current: ระหว่าง - proposed: ระหว่าง - confidence: high - equals-latin-key: no - reason: ระหว่าง is the Thai between kept as the retired recogniser
- row: intra - section: keywords - current: ภายใน - proposed: ภายใน - confidence: high - equals-latin-key: no - reason: ภายใน is the Thai within kept as the retired recogniser
- row: per - section: keywords - current: ต่อ - proposed: ต่อ - confidence: high - equals-latin-key: no - reason: ต่อ is the Thai per used for a stride
- row: ab - section: keywords - current: ช่วง - proposed: ช่วง - confidence: high - equals-latin-key: no - reason: ช่วง is the Thai span that names the range operator
- row: usque - section: keywords - current: จนถึง - proposed: จนถึง - confidence: high - equals-latin-key: no - reason: จนถึง is the Thai until that closes the range
- row: erratur - section: keywords - current: คาดหวัง_ล้มเหลว - proposed: คาดล้มเหลว - confidence: medium - equals-latin-key: no - reason: the underscore splits two Thai words that Thai writes without a boundary
- row: fragilis - section: keywords - current: เปราะบาง - proposed: เปราะบาง - confidence: high - equals-latin-key: no - reason: เปราะบาง is the Thai fragile behind a known flaky test
- row: futurum - section: keywords - current: ค้าง - proposed: ค้าง - confidence: high - equals-latin-key: no - reason: ค้าง is the Thai pending that marks outstanding work
- row: metior - section: keywords - current: วัด - proposed: วัด - confidence: high - equals-latin-key: no - reason: วัด is the imperative measure a benchmark performs
- row: omitte - section: keywords - current: ละเว้น - proposed: ละเว้น - confidence: high - equals-latin-key: no - reason: ละเว้น is the imperative omit of a skip
- row: postpara - section: keywords - current: หลังเตรียม - proposed: เก็บกวาด - confidence: medium - equals-latin-key: no - reason: หลังเตรียม names a time after setup rather than the cleanup action the hook performs
- row: postparabit - section: keywords - current: จะหลังเตรียม - proposed: จะเก็บกวาด - confidence: medium - equals-latin-key: no - reason: the async pair takes the จะ prefix in front of the same cleanup verb
- row: praepara - section: keywords - current: เตรียม - proposed: เตรียม - confidence: high - equals-latin-key: no - reason: เตรียม is the imperative prepare of the setup hook
- row: praeparabit - section: keywords - current: จะเตรียม - proposed: จะเตรียม - confidence: high - equals-latin-key: no - reason: จะเตรียม marks the async setup by the จะ prefix, the pack device for the async pair
- row: proba - section: keywords - current: ทดสอบ - proposed: ทดสอบ - confidence: high - equals-latin-key: no - reason: ทดสอบ is the Thai test
- row: probandum - section: keywords - current: ทดสอบชุด - proposed: ชุดทดสอบ - confidence: medium - equals-latin-key: no - reason: ทดสอบชุด is in the wrong compound order and reads as a verb phrase
- row: repete - section: keywords - current: ทำซ้ำ - proposed: ทําซ้ํา - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: solum - section: keywords - current: เฉพาะ - proposed: เฉพาะ - confidence: high - equals-latin-key: no - reason: เฉพาะ is the Thai only
- row: solum_in - section: keywords - current: เฉพาะใน - proposed: เฉพาะใน - confidence: high - equals-latin-key: no - reason: เฉพาะใน composes only with the in particle
- row: tag - section: keywords - current: แท็ก - proposed: แท็ก - confidence: high - equals-latin-key: no - reason: แท็ก is the Thai tag
- row: temporis - section: keywords - current: เวลา - proposed: หมดเวลา - confidence: medium - equals-latin-key: no - reason: เวลา is time in general, while the attribute bounds a duration
- row: nulla - section: keywords - current: ขาดหาย - proposed: ขาดหาย - confidence: high - equals-latin-key: no - reason: ขาดหาย renders the null literal as absent and stays distinct from nihil ไม่มี and vacuum เปล่า
- row: cli - section: keywords - current: cli - proposed: cli - confidence: high - equals-latin-key: yes - reason: cli is the acronym every pack keeps
- row: commune - section: keywords - current: ใช้ร่วม - proposed: ร่วม - confidence: medium - equals-latin-key: no - reason: ใช้ร่วม is the verb phrase use jointly where the row is a shared modifier
- row: cursor - section: keywords - current: เคอร์เซอร์ - proposed: เคอร์เซอร์ - confidence: high - equals-latin-key: no - reason: เคอร์เซอร์ names the annotation family while the cursor type row spells ตัววนซ้ำ separately
- row: futura - section: keywords - current: อนาคต - proposed: อนาคต - confidence: high - equals-latin-key: no - reason: อนาคต is the Thai future
- row: imperium - section: keywords - current: คำสั่ง - proposed: คําสั่ง - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: operandus - section: keywords - current: ตัวถูกดำเนินการ - proposed: ตัวถูกดําเนินการ - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: optio - section: keywords - current: ตัวเลือก - proposed: ตัวเลือก - confidence: high - equals-latin-key: no - reason: ตัวเลือก is the Thai option
- row: conversion - section: keywords - current: แปลง - proposed: แปลง - confidence: low - equals-latin-key: no - reason: no en or la pack carries a conversion row, so this key's concept is unverified here
- row: conversio - section: keywords - current: การแปลง - proposed: การแปลง - confidence: high - equals-latin-key: no - reason: การแปลง is the noun form of conversion
- row: nucleum - section: keywords - current: เคอร์เนล - proposed: เคอร์เนล - confidence: high - equals-latin-key: no - reason: เคอร์เนล is the fixed Thai loan for a kernel
- row: vertex - section: keywords - current: จุดยอด - proposed: จุดยอด - confidence: high - equals-latin-key: no - reason: จุดยอด is the Thai vertex
- row: fragment - section: keywords - current: ส่วนย่อย - proposed: ส่วนย่อย - confidence: medium - equals-latin-key: no - reason: ส่วนย่อย states the sub part a fragment is
- row: nondum - section: keywords - current: ไม่เสถียร - proposed: ไม่เสถียร - confidence: high - equals-latin-key: no - reason: ไม่เสถียร is the Thai unstable
- row: verte - section: keywords - current: เปลี่ยนชื่อ - proposed: เปลี่ยนชื่อ - confidence: high - equals-latin-key: no - reason: เปลี่ยนชื่อ is the imperative rename
- row: nomen - section: keywords - current: ชื่อ - proposed: ชื่อ - confidence: high - equals-latin-key: no - reason: ชื่อ is the Thai name
- row: radix - section: keywords - current: radix - proposed: radix - confidence: medium - equals-latin-key: yes - reason: the row names the compiler owned radix metadata family and must stay apart from the intrinsic รากที่สอง
- row: brevis - section: keywords - current: สั้น - proposed: สั้น - confidence: high - equals-latin-key: no - reason: สั้น is the Thai short
- row: descriptio - section: keywords - current: คำอธิบาย - proposed: คําอธิบาย - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: longum - section: keywords - current: ยาว - proposed: ยาว - confidence: high - equals-latin-key: no - reason: ยาว is the Thai long
- row: lane - section: keywords - current: เลน - proposed: เลน - confidence: high - equals-latin-key: no - reason: เลน is the Thai lane
- row: ubique - section: keywords - current: โกลบอล - proposed: โกลบอล - confidence: high - equals-latin-key: no - reason: โกลบอล is the Thai loan for global and stays distinct from omnia ทั้งหมด
- row: annotatio - section: keywords - current: คำอธิบายประกอบ - proposed: หมายเหตุ - confidence: medium - equals-latin-key: no - reason: it shares the คำอธิบาย stem with descriptio although annotation and description differ
- row: versio - section: keywords - current: เวอร์ชัน - proposed: เวอร์ชัน - confidence: high - equals-latin-key: no - reason: เวอร์ชัน is the fixed Thai loan for version
- row: imperia - section: keywords - current: ชุดคำสั่ง - proposed: ชุดคําสั่ง - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: reducta - section: keywords - current: ลดรูป - proposed: ลดรูป - confidence: high - equals-latin-key: no - reason: ลดรูป is the Thai reduce
- row: pro - section: keywords - current: ตาม - proposed: ตาม - confidence: medium - equals-latin-key: no - reason: ตาม is the Thai according to for the reduction operator slot
- row: factum - section: keywords - current: ผลคูณ - proposed: ผลคูณ - confidence: high - equals-latin-key: no - reason: ผลคูณ is the arithmetic product the fold produces
- row: numeratio - section: keywords - current: นับ - proposed: นับ - confidence: high - equals-latin-key: no - reason: นับ is the imperative count
- row: quilibet - section: keywords - current: ใดก็ได้ - proposed: ใดก็ได้ - confidence: high - equals-latin-key: no - reason: ใดก็ได้ is the Thai any for a value
- row: argmaxima - section: keywords - current: ดัชนีค่าสูงสุด - proposed: ดัชนีค่าสูงสุด - confidence: high - equals-latin-key: no - reason: ดัชนีค่าสูงสุด is the index of the maximum
- row: argminima - section: keywords - current: ดัชนีค่าต่ําสุด - proposed: ดัชนีค่าต่ําสุด - confidence: high - equals-latin-key: no - reason: the value is already NFKC while minima stores the composed spelling, so one word has two spellings in the pack
- row: ordinata - section: keywords - current: เรียง - proposed: เรียง - confidence: high - equals-latin-key: no - reason: เรียง is the imperative sort
- row: ascendens - section: keywords - current: น้อยไปมาก - proposed: น้อยไปมาก - confidence: high - equals-latin-key: no - reason: น้อยไปมาก states ascending order
- row: descendens - section: keywords - current: มากไปน้อย - proposed: มากไปน้อย - confidence: high - equals-latin-key: no - reason: มากไปน้อย states descending order
- row: ascii - section: types - current: ascii - proposed: ascii - confidence: high - equals-latin-key: yes - reason: ascii is the encoding standard name kept as Latin
- row: textus - section: types - current: ข้อความ - proposed: ข้อความ - confidence: high - equals-latin-key: no - reason: ข้อความ is the Thai string
- row: numerus - section: types - current: จํานวน - proposed: จํานวน - confidence: high - equals-latin-key: no - reason: the value is already NFKC while sixteen rows store the composed spelling of the same vowel
- row: modulus - section: types - current: ค่ามอดุลัส - proposed: ค่ามอดุลัส - confidence: high - equals-latin-key: no - reason: ค่ามอดุลัส heads the type row so it differs from the keyword โมดูลัส while sharing its stem
- row: saturatus - section: types - current: อิ่มตัว - proposed: อิ่มตัว - confidence: high - equals-latin-key: no - reason: อิ่มตัว is the Thai saturated
- row: exactus - section: types - current: แม่นยำ - proposed: หยุดเมื่อเกิน - confidence: medium - equals-latin-key: no - reason: แม่นยำ means precise, while the type traps when the value overflows
- row: fractus - section: types - current: เศษ - proposed: ทศนิยม - confidence: high - equals-latin-key: no - reason: เศษ is a division remainder, and Thai names a floating point number ทศนิยม
- row: bivalens - section: types - current: ตรรกะ - proposed: บูลีน - confidence: high - equals-latin-key: no - reason: ตรรกะ names logic in general, not the two valued boolean type
- row: nihil - section: types - current: ไม่มี - proposed: ไม่มี - confidence: high - equals-latin-key: no - reason: ไม่มี is the none of the null type, distinct from the null value literal ขาดหาย
- row: vacuum - section: types - current: เปล่า - proposed: เปล่า - confidence: high - equals-latin-key: no - reason: เปล่า is the empty of the void type
- row: numquam - section: types - current: ไม่เคย - proposed: ไม่เคย - confidence: high - equals-latin-key: no - reason: ไม่เคย is the Thai never of the bottom type
- row: ignotum - section: types - current: ไม่รู้ - proposed: ไม่รู้จัก - confidence: medium - equals-latin-key: no - reason: ไม่รู้ is a verb whose subject is a person, where the row is an attributive type name
- row: octeti - section: types - current: ไบต์ - proposed: ไบต์ - confidence: high - equals-latin-key: no - reason: ไบต์ is the fixed Thai loan for bytes
- row: octetus - section: types - current: ไบต์เดี่ยว - proposed: ไบต์เดี่ยว - confidence: high - equals-latin-key: no - reason: ไบต์เดี่ยว names the single byte beside the bytes type
- row: clampa - section: types - current: ตรึงขอบ - proposed: ตรึงขอบ - confidence: high - equals-latin-key: no - reason: ตรึงขอบ is the pin at border policy the marker names
- row: reflexa - section: types - current: สะท้อนขอบ - proposed: สะท้อนขอบ - confidence: high - equals-latin-key: no - reason: สะท้อนขอบ is the mirror at border policy
- row: involuta - section: types - current: วนขอบ - proposed: วนขอบ - confidence: high - equals-latin-key: no - reason: วนขอบ is the wrap around border policy, distinct from the keyword โมดูลัส
- row: littera - section: types - current: อักขระ - proposed: อักขระ - confidence: high - equals-latin-key: no - reason: อักขระ is the Thai character type
- row: regex - section: types - current: regex - proposed: regex - confidence: high - equals-latin-key: yes - reason: regex is the standard name and no Thai form is in use
- row: json - section: types - current: json - proposed: json - confidence: high - equals-latin-key: yes - reason: json is the format name kept as Latin
- row: valor - section: types - current: ค่า - proposed: ค่า - confidence: high - equals-latin-key: no - reason: ค่า is the Thai value
- row: instans - section: types - current: อินสแตนซ์ - proposed: ชั่วขณะ - confidence: high - equals-latin-key: no - reason: อินสแตนซ์ transliterates instance, the object concept, where instans is a point in time
- row: objectum - section: types - current: ออบเจ็กต์ - proposed: ออบเจ็กต์ - confidence: high - equals-latin-key: no - reason: ออบเจ็กต์ is the fixed Thai loan for an object
- row: quidlibet - section: types - current: อะไรก็ได้ - proposed: อะไรก็ได้ - confidence: high - equals-latin-key: no - reason: อะไรก็ได้ is the Thai any for the escape type and stays apart from quilibet ใดก็ได้
- row: lista - section: types - current: รายการ - proposed: รายการ - confidence: high - equals-latin-key: no - reason: รายการ is the Thai list
- row: tabula - section: types - current: ตาราง - proposed: ตาราง - confidence: high - equals-latin-key: no - reason: ตาราง renders a map as a table and stays distinct from tabulae สมุดบัญชี as the Latin pair is
- row: tabulae - section: types - current: สมุดบัญชี - proposed: สมุดบัญชี - confidence: high - equals-latin-key: no - reason: สมุดบัญชี is the account book the ledger row means
- row: copia - section: types - current: ชุด - proposed: ชุด - confidence: high - equals-latin-key: no - reason: ชุด is the Thai set
- row: promissum - section: types - current: คำมั่น - proposed: คํามั่น - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: cursor - section: types - current: ตัวชี้ - proposed: ตัววนซ้ํา - confidence: high - equals-latin-key: no - reason: ตัวชี้ is the pointer term, while the row is the iterator type
- row: queue - section: types - current: คิว - proposed: คิว - confidence: high - equals-latin-key: no - reason: คิว is the fixed Thai loan for a queue
- row: stack - section: types - current: สแตก - proposed: สแต็ก - confidence: medium - equals-latin-key: no - reason: the Thai loan is spelled สแต็ก with ไม้ไต่คู้ and สแตก also reads as the verb to crack
- row: tensor - section: types - current: เทนเซอร์ - proposed: เทนเซอร์ - confidence: high - equals-latin-key: no - reason: เทนเซอร์ is the fixed Thai loan for a tensor
- row: vector - section: types - current: เวกเตอร์ - proposed: เวกเตอร์ - confidence: high - equals-latin-key: no - reason: เวกเตอร์ is the fixed Thai loan for a vector
- row: matrix - section: types - current: เมทริกซ์ - proposed: เมทริกซ์ - confidence: high - equals-latin-key: no - reason: เมทริกซ์ is the fixed Thai loan for a matrix
- row: sparsa - section: types - current: กระจัดกระจาย - proposed: กระจัดกระจาย - confidence: high - equals-latin-key: no - reason: กระจัดกระจาย is the Thai sparse
- row: atomic - section: types - current: อะตอมิก - proposed: อะตอมิก - confidence: high - equals-latin-key: no - reason: อะตอมิก is the Thai loan for atomic
- row: intervallum - section: types - current: อันตรภาค - proposed: อันตรภาค - confidence: high - equals-latin-key: no - reason: อันตรภาค is the formal Pali Sanskrit term for a numeric interval, leaving ช่วง to the ab range operator
- row: filtrum - section: types - current: ตัวกรอง - proposed: ตัวกรอง - confidence: high - equals-latin-key: no - reason: ตัวกรอง is the Thai filter
- row: series - section: types - current: ซีรีส์ - proposed: ซีรีส์ - confidence: high - equals-latin-key: no - reason: ซีรีส์ is the fixed Thai loan for a series
- row: census - section: types - current: สำมะโน - proposed: สํามะโน - confidence: high - equals-latin-key: no - reason: the stored value is not NFKC normalised, so it is not the form the lexer holds, and the NFKC spelling of the same word renders identically
- row: ratio - section: types - current: ratio - proposed: เรกคอร์ด - confidence: medium - equals-latin-key: no - reason: the value equals the Latin key while the concept is a relational record and บันทึก is taken by nota
- row: sermo - section: types - current: ช่องทาง - proposed: ช่องทาง - confidence: high - equals-latin-key: no - reason: ช่องทาง is the Thai channel
- row: scrinium - section: types - current: เฟรม - proposed: เฟรม - confidence: high - equals-latin-key: no - reason: เฟรม is the Thai frame
- row: trapped - section: types - current: ติดกับดัก - proposed: ติดกับดัก - confidence: high - equals-latin-key: no - reason: ติดกับดัก is the trapped state and stays distinct from the capta word ดัก
- row: meus - section: types - current: ส่ง - proposed: ส่ง - confidence: high - equals-latin-key: no - reason: ส่ง states the sending direction the view method performs
- row: tuus - section: types - current: รับ - proposed: รับ - confidence: high - equals-latin-key: no - reason: รับ states the receiving direction the view method performs
