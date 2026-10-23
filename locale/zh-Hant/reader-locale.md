# zh-Hant Reader Locale — Review

Review of `locale/zh-Hant/pack.toml` against the concept every row must render,
the sibling `locale/zh-Hans` pack, and the register of Traditional Chinese as
written in Taiwan and Hong Kong. No pack row is edited by this review: the file
records the conventions plus one finding per row. Sources: `docs/design/llm-locale.md`
(per-key concept, part of speech and rationale), `docs/design/reader-locale.md`
(what a pack row is; NFKC and punctuation rules), `docs/stdlib/morphologia.md`
(verb form), and the `en`, `la` and `zh-Hans` packs.

## Conventions

**(a) Part of speech and verb form.** A statement keyword reads in the pack’s verbal
register. `sit` at line 15 is 設為, a predicative verb phrase naming the act of
binding, not a noun. A modifier that qualifies a declaration reads as a 可- adjective:
`iacit` at line 24 is 可拋, literally throwable, even though the Latin is a finite
verb. The rule applied to every row of this pack is therefore: a row placed
where a statement or a clause begins, and naming an action, is a verb phrase (設為,
傳回, 拋出, 讓出) or a 可- modifier (可拋, 可變, 可選); a row that names a declaration
head, a type, a value or a marker stays nominal (模組, 型別, 整數, 空值), and no row
may be a bare noun in an action slot. Verb form follows `morphologia.md`: the
surface names the action or posture the construct performs, not the dictionary gloss
of the Latin word. That is why 前綴 (a literal gloss of praefixum as prefix) fails
here: the keyword marks compile-time evaluation, so the row must read as comptime,
not as prefix.

**(b) Register.** Rows are written in the formal technical prose of Taiwan and Hong
Kong. Mainland-register compounds are avoided where a settled Taiwan word exists
(走訪 not 遍歷, 共用 not 共享, 全域 not 全局, 命令列 not 命令行, 核心 not 內核, 疊代器
not 迭代器). A technical Latin term is kept when the term is an acronym or product
name in universal use, or when no Traditional word is settled; it is never kept as
a substitute for a word that is settled. JSON is the first case and stays JSON;
正規表示式 is a settled word, so regex must stay translated even though the sibling
Simplified pack keeps Latin `regex`. The async family states its register explicitly:
異步 is the simplified-region shorthand, while the full form 非同步 is the
Traditional register and is already used by incipiet 非同步入口, praeparabit
準備非同步 and postparabit 後置準備非同步, so 非同步 is the word this pack must use.

**(c) Script, tokens and normalization.** Every row is written in traditional
characters only; no simplified character may appear inside a row. The review finds
one violation, the type row 模數类型 at line 174, which carries simplified 类 where
this pack writes 類型 (genus, line 9) and 型別 (typus, line 16). Every row is a single
XID token with no whitespace and no separator; the only row whose value contains a
separator is primus_quem at line 90, and its underscore is present because the value
is byte-equal to its Latin key. Normalization is global, not per pack: NFKC applies
at lex entry before keyword matching (`reader-locale.md`, A3), so each row is
compared in NFKC form; full-width punctuation collapses under NFKC (A4), while
ideographic punctuation with no compatibility decomposition (。 and 、) remains a hard
lex error in every locale. A row may therefore carry neither full-width nor
half-width punctuation.

**(d) One Latin key in two sections.** `modulus` is a keyword at line 13, 模數, and a
type at line 174, 模數类型. The rule is that the type row keeps the keyword’s head
spelling byte for byte and appends the type marker; the head is never re-translated,
and the marker must be one of the pack’s traditional markers, 類型 or 型別. 模數类型
breaks the script rule in (c), so the finding proposes 模數類型. `cursor` is the
other key present in both sections, and it breaks the same-head rule the other way
(迭代器 in keywords against 游標 in types); its two findings converge on 疊代器.

**(e) Retaining Latin.** A row may keep its Latin spelling only when the value is
byte-equal to the Latin key and the row is a name rather than a concept with a settled
Traditional word; the retention is then declared by the finding line recorded here with
equals-latin-key: yes, and the pack may add a comment naming the reason. Five rows
qualify today: ascii at line 171, primus_quem at line 90, radix at line 148, ratio at
line 217 and ubi at line 91. No pack comment currently states those five reasons, so
the finding line is the only declaration and a future pack change should add the
comment. Case-variant Latin values are Latin-retained but not byte-equal, so JSON at
line 191 and nan at line 64 are recorded with equals-latin-key: no and carry their
reason instead. There is no separate schema flag for retention — schema_version 1 has
only the keywords, types and diagnostics tables.

**(f) Unsettled.** This review could not settle the null-value word: nihil 無 (none)
and vacuum 空值 (void) already occupy the family, and nulla 缺失 reads as a missing
field, so that row is recorded at low confidence. The ledger row tabulae 帳本 is
marked provisional by the pack itself. The row conversion 變換 has no `en` concept row,
so its distinction from conversio 轉換 is unverified. The 等- await family is
internally consistent but a pack coinage. The apud question below is answered at
medium confidence. Nothing in this file edits the pack.

### Named question: apud 在 versus 於

Current word: 在 (line 45). Traditional candidate: 於. Simplified sibling: 于.

apud means at (`docs/design/llm-locale.md`), and the pack uses it for a coordinate or
index position: `primus_quem(a apud [i], …)` names the axis position, `itera apud`
iterates at a position, and the list read method `xs.apud(i)` reads at an index. Each
of the three reads as the preposition at before a position. 在 is the everyday locative
verb and preposition: it reads first as to be at or
to exist, so a keyword spelled 在 in a statement position reads as a verb and breaks
the (a) rule. 於 is the monosyllabic formal preposition for at, in or on; it is never
a verb, so a bare 於 before a coordinate reads as a position marker and nothing else.
于 is the simplified spelling of 於 and also a distinct classical preposition; it
cannot appear in a Traditional pack because a simplified character inside a row breaks
the (c) script rule, so it is excluded regardless of merit.

Recommendation: adopt 於 for apud. Confidence: medium — the form argument is strong,
but 在 is not wrong in Taiwan prose and no Traditional-language reader has been
consulted. No pack edit is made here; the row stays 在 until a pack change lands.

## Findings

One line per row of `locale/zh-Hant/pack.toml`: 159 keyword rows then 47 type rows,
206 in total, each key once per section. `proposed` repeats `current` when the review
finds nothing to change. A row marked low is one this review could not settle rather
than one it guessed.

- row: discretio - section: keywords - current: 分支聯集 - proposed: 分支聯集 - confidence: high - equals-latin-key: no - reason: no mismatch: 分支聯集 is the settled Traditional rendering of the union keyword
- row: fixum - section: keywords - current: 定值 - proposed: 定值 - confidence: high - equals-latin-key: no - reason: no mismatch: 定值 is the settled Traditional rendering of the const keyword
- row: functio - section: keywords - current: 函式 - proposed: 函式 - confidence: high - equals-latin-key: no - reason: no mismatch: 函式 is the settled Traditional rendering of the fn keyword
- row: genus - section: keywords - current: 類型 - proposed: 類型 - confidence: high - equals-latin-key: no - reason: no mismatch: 類型 is the settled Traditional rendering of the class keyword
- row: implendum - section: keywords - current: 待實作介面 - proposed: 待實作介面 - confidence: high - equals-latin-key: no - reason: no mismatch: 待實作介面 is the settled Traditional rendering of the interface keyword
- row: importa - section: keywords - current: 匯入 - proposed: 匯入 - confidence: high - equals-latin-key: no - reason: no mismatch: 匯入 is the settled Traditional rendering of the import keyword
- row: regio - section: keywords - current: 模組 - proposed: 模組 - confidence: high - equals-latin-key: no - reason: no mismatch: 模組 is the settled Traditional rendering of the module keyword
- row: modulus - section: keywords - current: 模數 - proposed: 模數 - confidence: high - equals-latin-key: no - reason: no mismatch: 模數 is the settled Traditional rendering of the wrapping modulus keyword, and the type row carries its own marked form
- row: ordo - section: keywords - current: 列舉 - proposed: 列舉 - confidence: high - equals-latin-key: no - reason: no mismatch: 列舉 is the settled Traditional rendering of the enum keyword
- row: sit - section: keywords - current: 設為 - proposed: 設為 - confidence: high - equals-latin-key: no - reason: no mismatch: 設為 is the verb phrase that names the binding act, which is the statement-position verb rule
- row: typus - section: keywords - current: 型別 - proposed: 型別 - confidence: high - equals-latin-key: no - reason: no mismatch: 型別 is the settled Traditional rendering of the type keyword
- row: schema - section: keywords - current: 結構 - proposed: 結構 - confidence: high - equals-latin-key: no - reason: no mismatch: 結構 is the settled Traditional rendering of the schema keyword
- row: columna - section: keywords - current: 欄位 - proposed: 欄位 - confidence: high - equals-latin-key: no - reason: no mismatch: 欄位 is the settled Traditional rendering of the column keyword
- row: varia - section: keywords - current: 可變 - proposed: 可變 - confidence: high - equals-latin-key: no - reason: no mismatch: 可變 is the settled Traditional rendering of the mut keyword
- row: ceteri - section: keywords - current: 其餘 - proposed: 其餘 - confidence: high - equals-latin-key: no - reason: no mismatch: 其餘 is the settled Traditional rendering of the rest keyword
- row: errata - section: keywords - current: 錯誤 - proposed: 錯誤 - confidence: high - equals-latin-key: no - reason: 錯誤 is the noun for declared error types, distinct from iace 拋出 the verb
- row: exitus - section: keywords - current: 出口 - proposed: 出口 - confidence: high - equals-latin-key: no - reason: no mismatch: 出口 is the settled Traditional rendering of the exit keyword
- row: generis - section: keywords - current: 靜態 - proposed: 靜態 - confidence: high - equals-latin-key: no - reason: 靜態 is the adjective retained as the recognizer for the retired static member spelling
- row: iacit - section: keywords - current: 可拋 - proposed: 可拋 - confidence: high - equals-latin-key: no - reason: no mismatch: 可拋 is the 可- modifier form for the throws posture; a finite verb here would be a form mismatch
- row: immutata - section: keywords - current: 不變 - proposed: 唯讀 - confidence: medium - equals-latin-key: no - reason: 不變 reads as invariant, while the modifier is the set-once read-only posture that 唯讀 names
- row: interna - section: keywords - current: 內部 - proposed: 內部 - confidence: high - equals-latin-key: no - reason: no mismatch: 內部 is the settled Traditional rendering of the internal keyword
- row: magnitudo - section: keywords - current: 尺寸 - proposed: 尺寸 - confidence: high - equals-latin-key: no - reason: no mismatch: 尺寸 is the settled Traditional rendering of the size keyword
- row: iuncta - section: keywords - current: 元組 - proposed: 元組 - confidence: high - equals-latin-key: no - reason: no mismatch: 元組 is the settled Traditional rendering of the tuple keyword
- row: optiones - section: keywords - current: 可選項 - proposed: 選項集 - confidence: medium - equals-latin-key: no - reason: 可選項 is nearly the same word as sponte 可選, while the keyword is the options bundle that 選項集 names
- row: praefixum - section: keywords - current: 前綴 - proposed: 編譯期 - confidence: high - equals-latin-key: no - reason: 前綴 translates the Latin word as prefix, but the keyword marks compile-time evaluation (comptime), which is a different concept
- row: insere - section: keywords - current: 嵌入 - proposed: 嵌入 - confidence: high - equals-latin-key: no - reason: no mismatch: 嵌入 is the settled Traditional rendering of the embed keyword
- row: privata - section: keywords - current: 私有 - proposed: 私有 - confidence: high - equals-latin-key: no - reason: no mismatch: 私有 is the settled Traditional rendering of the private keyword
- row: protecta - section: keywords - current: 保護 - proposed: 保護 - confidence: high - equals-latin-key: no - reason: no mismatch: 保護 is the settled Traditional rendering of the protected keyword
- row: publica - section: keywords - current: 公開 - proposed: 公開 - confidence: high - equals-latin-key: no - reason: no mismatch: 公開 is the settled Traditional rendering of the public keyword
- row: sponte - section: keywords - current: 可選 - proposed: 可選 - confidence: high - equals-latin-key: no - reason: 可選 is the 可- modifier for the optional parameter slot
- row: casu - section: keywords - current: 分支 - proposed: 分支 - confidence: high - equals-latin-key: no - reason: no mismatch: 分支 is the settled Traditional rendering of the case keyword
- row: ceterum - section: keywords - current: 預設 - proposed: 預設 - confidence: high - equals-latin-key: no - reason: no mismatch: 預設 is the settled Traditional rendering of the default keyword
- row: custodi - section: keywords - current: 守衛 - proposed: 守衛 - confidence: high - equals-latin-key: no - reason: no mismatch: 守衛 is the settled Traditional rendering of the guard keyword
- row: discerne - section: keywords - current: 比對 - proposed: 比對 - confidence: high - equals-latin-key: no - reason: no mismatch: 比對 is the settled Traditional rendering of the match keyword
- row: dum - section: keywords - current: 當 - proposed: 當 - confidence: high - equals-latin-key: no - reason: no mismatch: 當 is the settled Traditional rendering of the while keyword
- row: elige - section: keywords - current: 選擇 - proposed: 切換 - confidence: medium - equals-latin-key: no - reason: 選擇 reads as choose, while the keyword is value dispatch (switch)
- row: ergo - section: keywords - current: 則 - proposed: 則 - confidence: high - equals-latin-key: no - reason: no mismatch: 則 is the settled Traditional rendering of the then keyword
- row: fac - section: keywords - current: 執行 - proposed: 執行 - confidence: medium - equals-latin-key: no - reason: 執行 reads as execute, while the keyword introduces a scoped do block; kept because the block runs its body
- row: itera - section: keywords - current: 遍歷 - proposed: 走訪 - confidence: medium - equals-latin-key: no - reason: 遍歷 is the mainland register for traversal, while Taiwan writes 走訪 for iteration over elements
- row: apud - section: keywords - current: 在 - proposed: 於 - confidence: medium - equals-latin-key: no - reason: 在 reads as the locative verb to be at, while apud is a prepositional coordinate marker; 於 is its formal monosyllabic counterpart and 于 is simplified-only
- row: secus - section: keywords - current: 否則 - proposed: 否則 - confidence: high - equals-latin-key: no - reason: no mismatch: 否則 is the settled Traditional rendering of the else keyword
- row: si - section: keywords - current: 若 - proposed: 若 - confidence: high - equals-latin-key: no - reason: no mismatch: 若 is the settled Traditional rendering of the if keyword
- row: sin - section: keywords - current: 否則若 - proposed: 否則若 - confidence: high - equals-latin-key: no - reason: 否則若 is one token that carries both else and if, matching the single-token rule
- row: perge - section: keywords - current: 繼續 - proposed: 繼續 - confidence: high - equals-latin-key: no - reason: 繼續 is the settled continue verb and does not collide with cede 讓出
- row: redde - section: keywords - current: 傳回 - proposed: 傳回 - confidence: high - equals-latin-key: no - reason: 傳回 is the Taiwan term for return and does not collide with reddet 等返
- row: rumpe - section: keywords - current: 中斷 - proposed: 跳出 - confidence: medium - equals-latin-key: no - reason: 中斷 names interrupting a running operation, while the keyword leaves the enclosing loop
- row: tacet - section: keywords - current: 靜默 - proposed: 靜默 - confidence: high - equals-latin-key: no - reason: 靜默 names the silent no-op posture and does not collide with omitte 略過
- row: adfirma - section: keywords - current: 斷言 - proposed: 斷言 - confidence: high - equals-latin-key: no - reason: no mismatch: 斷言 is the settled Traditional rendering of the assert keyword
- row: cape - section: keywords - current: 捕捉 - proposed: 捕捉 - confidence: high - equals-latin-key: no - reason: no mismatch: 捕捉 is the settled Traditional rendering of the catch keyword
- row: capta - section: keywords - current: 陷阱 - proposed: 陷阱 - confidence: high - equals-latin-key: no - reason: 陷阱 is the settled rendering of trap and stays distinct from cape 捕捉
- row: cede - section: keywords - current: 讓出 - proposed: 讓出 - confidence: high - equals-latin-key: no - reason: 讓出 is the cede or yield verb the keyword requires
- row: iace - section: keywords - current: 拋出 - proposed: 拋出 - confidence: high - equals-latin-key: no - reason: no mismatch: 拋出 is the settled Traditional rendering of the throw keyword
- row: mori - section: keywords - current: 崩潰 - proposed: 崩潰 - confidence: high - equals-latin-key: no - reason: no mismatch: 崩潰 is the settled Traditional rendering of the panic keyword
- row: requirit - section: keywords - current: 需要 - proposed: 需要 - confidence: high - equals-latin-key: no - reason: no mismatch: 需要 is the settled Traditional rendering of the require keyword
- row: reice - section: keywords - current: 拒絕 - proposed: 拒絕 - confidence: high - equals-latin-key: no - reason: no mismatch: 拒絕 is the settled Traditional rendering of the reject keyword
- row: clausura - section: keywords - current: 閉包 - proposed: 閉包 - confidence: high - equals-latin-key: no - reason: 閉包 is the conventional Chinese name for the anonymous function the keyword introduces
- row: falsum - section: keywords - current: 假 - proposed: 假 - confidence: high - equals-latin-key: no - reason: no mismatch: 假 is the settled Traditional rendering of the false keyword
- row: verum - section: keywords - current: 真 - proposed: 真 - confidence: high - equals-latin-key: no - reason: no mismatch: 真 is the settled Traditional rendering of the true keyword
- row: nonnumerus - section: keywords - current: nan - proposed: nan - confidence: medium - equals-latin-key: no - reason: the value is byte-equal to en by design and NaN is written in Latin in Traditional technical prose, so the literal stays untranslated
- row: aut - section: keywords - current: 或 - proposed: 或 - confidence: high - equals-latin-key: no - reason: no mismatch: 或 is the settled Traditional rendering of the or keyword
- row: est - section: keywords - current: 是 - proposed: 是 - confidence: high - equals-latin-key: no - reason: no mismatch: 是 is the settled Traditional rendering of the is keyword
- row: et - section: keywords - current: 且 - proposed: 且 - confidence: high - equals-latin-key: no - reason: no mismatch: 且 is the settled Traditional rendering of the and keyword
- row: non - section: keywords - current: 非 - proposed: 非 - confidence: high - equals-latin-key: no - reason: no mismatch: 非 is the settled Traditional rendering of the not keyword
- row: vel - section: keywords - current: 或取 - proposed: 或取 - confidence: low - equals-latin-key: no - reason: unsettled: 或取 is a pack-coined compound whose coalesce sense is not evident, and no Traditional fallback word is settled
- row: ego - section: keywords - current: 自身 - proposed: 自身 - confidence: high - equals-latin-key: no - reason: no mismatch: 自身 is the settled Traditional rendering of the self keyword
- row: finge - section: keywords - current: 虛構 - proposed: 變體 - confidence: high - equals-latin-key: no - reason: 虛構 means to fabricate, but the keyword constructs a tagged-union variant case that 變體 names
- row: implet - section: keywords - current: 實作 - proposed: 實作 - confidence: high - equals-latin-key: no - reason: no mismatch: 實作 is the settled Traditional rendering of the implements keyword
- row: mone - section: keywords - current: 警告 - proposed: 警告 - confidence: high - equals-latin-key: no - reason: no mismatch: 警告 is the settled Traditional rendering of the warn keyword
- row: nota - section: keywords - current: 註記 - proposed: 顯示 - confidence: medium - equals-latin-key: no - reason: 註記 reads as a written annotation and shares the note sense of annotatio 註解, while the keyword is primary print output
- row: scribe - section: keywords - current: 寫出 - proposed: 寫出 - confidence: high - equals-latin-key: no - reason: no mismatch: 寫出 is the settled Traditional rendering of the write keyword
- row: vide - section: keywords - current: 檢視 - proposed: 檢視 - confidence: high - equals-latin-key: no - reason: no mismatch: 檢視 is the settled Traditional rendering of the debug keyword
- row: argumenta - section: keywords - current: 引數 - proposed: 引數 - confidence: high - equals-latin-key: no - reason: no mismatch: 引數 is the settled Traditional rendering of the args keyword
- row: incipiet - section: keywords - current: 非同步入口 - proposed: 非同步入口 - confidence: high - equals-latin-key: no - reason: no mismatch: 非同步入口 is the settled Traditional rendering of the async_main keyword
- row: incipit - section: keywords - current: 入口 - proposed: 入口 - confidence: high - equals-latin-key: no - reason: no mismatch: 入口 is the settled Traditional rendering of the main keyword
- row: ad - section: keywords - current: 端點 - proposed: 呼叫 - confidence: medium - equals-latin-key: no - reason: 端點 names the capability endpoint, while the keyword is the route call (call)
- row: fiet - section: keywords - current: 異步 - proposed: 非同步 - confidence: high - equals-latin-key: no - reason: 異步 is the simplified-region shorthand while incipiet, praeparabit and postparabit all write 非同步, so one async word must serve this pack
- row: fiunt - section: keywords - current: 流 - proposed: 流 - confidence: high - equals-latin-key: no - reason: no mismatch: 流 is the settled Traditional rendering of the generator keyword
- row: fient - section: keywords - current: 異流 - proposed: 非同步流 - confidence: medium - equals-latin-key: no - reason: 異 is the same simplified-region shorthand as fiet, and 非同步流 keeps the async stream parallel to fiunt 流
- row: figendum - section: keywords - current: 等定 - proposed: 等定 - confidence: medium - equals-latin-key: no - reason: 等 plus 定 spells awaiting and binding an immutable result, but a reader must learn the 等- family as a unit
- row: variandum - section: keywords - current: 等變 - proposed: 等變 - confidence: medium - equals-latin-key: no - reason: 等 plus 變 spells awaiting and binding a mutable result, but the 等- family is a pack coinage
- row: reddet - section: keywords - current: 等返 - proposed: 等返 - confidence: medium - equals-latin-key: no - reason: 等 plus 返 spells awaiting and returning the result, but the 等- family is a pack coinage
- row: tacebit - section: keywords - current: 等棄 - proposed: 等棄 - confidence: medium - equals-latin-key: no - reason: 等 plus 棄 spells awaiting and discarding the result, but the 等- family is a pack coinage
- row: vacua - section: keywords - current: 空集 - proposed: 空集 - confidence: high - equals-latin-key: no - reason: no mismatch: 空集 is the settled Traditional rendering of the empty keyword
- row: primus_quem - section: keywords - current: primus_quem - proposed: primus_quem - confidence: low - equals-latin-key: yes - reason: retained Latin: byte-equal to the key, the only row with a separator, and no reviewed Traditional gloss exists for the first-iteration marker
- row: ubi - section: keywords - current: ubi - proposed: ubi - confidence: medium - equals-latin-key: yes - reason: retained Latin: byte-equal to the key, and the where or index marker has no settled Traditional spelling in this pack
- row: summa - section: keywords - current: 求和 - proposed: 求和 - confidence: high - equals-latin-key: no - reason: no mismatch: 求和 is the settled Traditional rendering of the sum keyword
- row: filum - section: keywords - current: 執行緒 - proposed: 執行緒 - confidence: high - equals-latin-key: no - reason: 執行緒 is the Taiwan term for thread and matches the pack register
- row: maxima - section: keywords - current: 最大 - proposed: 最大 - confidence: high - equals-latin-key: no - reason: no mismatch: 最大 is the settled Traditional rendering of the max keyword
- row: minima - section: keywords - current: 最小 - proposed: 最小 - confidence: high - equals-latin-key: no - reason: no mismatch: 最小 is the settled Traditional rendering of the min keyword
- row: de - section: keywords - current: 從 - proposed: 唯讀借用 - confidence: medium - equals-latin-key: no - reason: 從 renders the source sense that ex 取自 already holds, while de is an immutable borrow parameter
- row: ex - section: keywords - current: 取自 - proposed: 取自 - confidence: high - equals-latin-key: no - reason: no mismatch: 取自 is the settled Traditional rendering of the from keyword
- row: in - section: keywords - current: 可變 - proposed: 可變借用 - confidence: medium - equals-latin-key: no - reason: 可變 duplicates varia 可變 for a different concept, while in is a mutable borrow parameter
- row: own - section: keywords - current: 擁有 - proposed: 擁有 - confidence: high - equals-latin-key: no - reason: no mismatch: 擁有 is the settled Traditional rendering of the own keyword
- row: copy - section: keywords - current: 拷貝 - proposed: 拷貝 - confidence: high - equals-latin-key: no - reason: no mismatch: 拷貝 is the settled Traditional rendering of the copy keyword
- row: axis - section: keywords - current: 軸 - proposed: 軸 - confidence: high - equals-latin-key: no - reason: no mismatch: 軸 is the settled Traditional rendering of the axis keyword
- row: lege - section: keywords - current: 讀取 - proposed: 讀取 - confidence: high - equals-latin-key: no - reason: no mismatch: 讀取 is the settled Traditional rendering of the read keyword
- row: lineam - section: keywords - current: 行 - proposed: 行 - confidence: high - equals-latin-key: no - reason: no mismatch: 行 is the settled Traditional rendering of the line keyword
- row: omnia - section: keywords - current: 全部 - proposed: 全部 - confidence: high - equals-latin-key: no - reason: no mismatch: 全部 is the settled Traditional rendering of the all keyword
- row: scriptum - section: keywords - current: 格式文字 - proposed: 格式文字 - confidence: high - equals-latin-key: no - reason: no mismatch: 格式文字 is the settled Traditional rendering of the format keyword
- row: sparge - section: keywords - current: 展開 - proposed: 展開 - confidence: high - equals-latin-key: no - reason: no mismatch: 展開 is the settled Traditional rendering of the spread keyword
- row: ut - section: keywords - current: 作為 - proposed: 作為 - confidence: high - equals-latin-key: no - reason: no mismatch: 作為 is the settled Traditional rendering of the as keyword
- row: via - section: keywords - current: 經由 - proposed: 經由 - confidence: medium - equals-latin-key: no - reason: the pack marks via a provisional proposal, and 經由 is an accurate preposition but the spelling is not yet ratified
- row: ante - section: keywords - current: 之前 - proposed: 之前 - confidence: high - equals-latin-key: no - reason: no mismatch: 之前 is the settled Traditional rendering of the before keyword
- row: inter - section: keywords - current: 之間 - proposed: 之間 - confidence: high - equals-latin-key: no - reason: 之間 is the retained recognizer for the retired between spelling, kept only for the migration diagnostic
- row: intra - section: keywords - current: 內含 - proposed: 內含 - confidence: high - equals-latin-key: no - reason: 內含 is the retained recognizer for the retired within spelling, kept only for the migration diagnostic
- row: per - section: keywords - current: 每 - proposed: 步長 - confidence: medium - equals-latin-key: no - reason: 每 is the every-quantifier, while the keyword sets the range stride (step)
- row: ab - section: keywords - current: 範圍 - proposed: 範圍 - confidence: high - equals-latin-key: no - reason: no mismatch: 範圍 is the settled Traditional rendering of the range keyword
- row: usque - section: keywords - current: 直到 - proposed: 直到 - confidence: high - equals-latin-key: no - reason: no mismatch: 直到 is the settled Traditional rendering of the until keyword
- row: erratur - section: keywords - current: 預期失敗 - proposed: 預期失敗 - confidence: high - equals-latin-key: no - reason: no mismatch: 預期失敗 is the settled Traditional rendering of the expect_failure keyword
- row: fragilis - section: keywords - current: 脆弱 - proposed: 脆弱 - confidence: high - equals-latin-key: no - reason: 脆弱 keeps flaky distinct from nondum 不穩定, so the two rows do not collapse
- row: futurum - section: keywords - current: 預期 - proposed: 待辦 - confidence: high - equals-latin-key: no - reason: 預期 reads as expect, which erratur 預期失敗 already uses, while the keyword marks pending future work (todo)
- row: metior - section: keywords - current: 測量 - proposed: 基準 - confidence: medium - equals-latin-key: no - reason: 測量 is the verb measure, while the keyword declares a benchmark
- row: omitte - section: keywords - current: 略過 - proposed: 略過 - confidence: high - equals-latin-key: no - reason: no mismatch: 略過 is the settled Traditional rendering of the skip keyword
- row: postpara - section: keywords - current: 後置準備 - proposed: 後置準備 - confidence: high - equals-latin-key: no - reason: no mismatch: 後置準備 is the settled Traditional rendering of the teardown keyword
- row: postparabit - section: keywords - current: 後置準備非同步 - proposed: 後置準備非同步 - confidence: high - equals-latin-key: no - reason: no mismatch: 後置準備非同步 is the settled Traditional rendering of the async_teardown keyword
- row: praepara - section: keywords - current: 準備 - proposed: 準備 - confidence: high - equals-latin-key: no - reason: no mismatch: 準備 is the settled Traditional rendering of the setup keyword
- row: praeparabit - section: keywords - current: 準備非同步 - proposed: 準備非同步 - confidence: high - equals-latin-key: no - reason: no mismatch: 準備非同步 is the settled Traditional rendering of the async_setup keyword
- row: proba - section: keywords - current: 測試 - proposed: 測試 - confidence: high - equals-latin-key: no - reason: no mismatch: 測試 is the settled Traditional rendering of the test keyword
- row: probandum - section: keywords - current: 測試規格 - proposed: 測試規格 - confidence: high - equals-latin-key: no - reason: no mismatch: 測試規格 is the settled Traditional rendering of the describe keyword
- row: repete - section: keywords - current: 重複 - proposed: 重複 - confidence: high - equals-latin-key: no - reason: no mismatch: 重複 is the settled Traditional rendering of the repeat keyword
- row: solum - section: keywords - current: 僅限 - proposed: 僅限 - confidence: high - equals-latin-key: no - reason: no mismatch: 僅限 is the settled Traditional rendering of the only keyword
- row: solum_in - section: keywords - current: 僅限於 - proposed: 僅限於 - confidence: high - equals-latin-key: no - reason: 僅限於 is one token that carries the platform scope, and the separator is inside the Latin key only
- row: tag - section: keywords - current: 標籤 - proposed: 標籤 - confidence: high - equals-latin-key: no - reason: no mismatch: 標籤 is the settled Traditional rendering of the tag keyword
- row: temporis - section: keywords - current: 時限 - proposed: 時限 - confidence: high - equals-latin-key: no - reason: no mismatch: 時限 is the settled Traditional rendering of the timeout keyword
- row: nulla - section: keywords - current: 缺失 - proposed: 缺失 - confidence: low - equals-latin-key: no - reason: unsettled: 缺失 reads as a missing required field, while nihil 無 and vacuum 空值 already occupy the null family and no value word is settled
- row: cli - section: keywords - current: 命令行 - proposed: 命令列 - confidence: medium - equals-latin-key: no - reason: 命令行 is the mainland register, while Taiwan writes 命令列 for the command line
- row: commune - section: keywords - current: 共享 - proposed: 共用 - confidence: medium - equals-latin-key: no - reason: 共享 is the mainland register for shared, while Taiwan writes 共用
- row: cursor - section: keywords - current: 迭代器 - proposed: 疊代器 - confidence: medium - equals-latin-key: no - reason: 迭代 is the mainland register, while Taiwan writes 疊代器 for the iterator that the keyword names
- row: futura - section: keywords - current: 未來 - proposed: 未來 - confidence: high - equals-latin-key: no - reason: no mismatch: 未來 is the settled Traditional rendering of the future keyword
- row: imperium - section: keywords - current: 命令 - proposed: 命令 - confidence: high - equals-latin-key: no - reason: no mismatch: 命令 is the settled Traditional rendering of the command keyword
- row: operandus - section: keywords - current: 操作數 - proposed: 操作數 - confidence: high - equals-latin-key: no - reason: no mismatch: 操作數 is the settled Traditional rendering of the operand keyword
- row: optio - section: keywords - current: 選項 - proposed: 選項 - confidence: high - equals-latin-key: no - reason: no mismatch: 選項 is the settled Traditional rendering of the option keyword
- row: conversion - section: keywords - current: 變換 - proposed: 變換 - confidence: low - equals-latin-key: no - reason: no en concept row exists for conversion, so the row distinction from conversio 轉換 is unverified and the spelling is kept as written
- row: conversio - section: keywords - current: 轉換 - proposed: 轉換 - confidence: high - equals-latin-key: no - reason: no mismatch: 轉換 is the settled Traditional rendering of the conversion keyword
- row: nucleum - section: keywords - current: 內核 - proposed: 核心 - confidence: medium - equals-latin-key: no - reason: 內核 is the mainland register for kernel, while Taiwan writes 核心
- row: vertex - section: keywords - current: 頂點 - proposed: 頂點 - confidence: high - equals-latin-key: no - reason: no mismatch: 頂點 is the settled Traditional rendering of the vertex keyword
- row: fragment - section: keywords - current: 片段 - proposed: 片段 - confidence: high - equals-latin-key: no - reason: no mismatch: 片段 is the settled Traditional rendering of the fragment keyword
- row: nondum - section: keywords - current: 不穩定 - proposed: 不穩定 - confidence: high - equals-latin-key: no - reason: 不穩定 is the settled rendering of unstable, kept distinct from fragilis 脆弱
- row: verte - section: keywords - current: 改名 - proposed: 改名 - confidence: high - equals-latin-key: no - reason: no mismatch: 改名 is the settled Traditional rendering of the rename keyword
- row: nomen - section: keywords - current: 名稱 - proposed: 名稱 - confidence: high - equals-latin-key: no - reason: no mismatch: 名稱 is the settled Traditional rendering of the name keyword
- row: radix - section: keywords - current: radix - proposed: radix - confidence: high - equals-latin-key: yes - reason: retained Latin: byte-equal to the key, and radix is the pack identifier and the root-numeral term
- row: brevis - section: keywords - current: 簡 - proposed: 簡 - confidence: high - equals-latin-key: no - reason: 簡 is the single-character short-form marker and pairs with longum 詳
- row: descriptio - section: keywords - current: 描述 - proposed: 描述 - confidence: high - equals-latin-key: no - reason: no mismatch: 描述 is the settled Traditional rendering of the description keyword
- row: longum - section: keywords - current: 詳 - proposed: 詳 - confidence: high - equals-latin-key: no - reason: 詳 is the single-character long-form marker and pairs with brevis 簡
- row: lane - section: keywords - current: 車道 - proposed: 車道 - confidence: medium - equals-latin-key: no - reason: 車道 is the literal traffic lane; the workspace sense is carried only by context, but no settled term exists
- row: ubique - section: keywords - current: 全局 - proposed: 全域 - confidence: medium - equals-latin-key: no - reason: 全局 is the mainland register for global, while Taiwan writes 全域
- row: annotatio - section: keywords - current: 註解 - proposed: 註解 - confidence: high - equals-latin-key: no - reason: 註解 is the settled term for annotation and no longer collides once nota reads as output
- row: versio - section: keywords - current: 版本 - proposed: 版本 - confidence: high - equals-latin-key: no - reason: no mismatch: 版本 is the settled Traditional rendering of the version keyword
- row: imperia - section: keywords - current: 命令集 - proposed: 命令集 - confidence: high - equals-latin-key: no - reason: no mismatch: 命令集 is the settled Traditional rendering of the commands keyword
- row: reducta - section: keywords - current: 歸約 - proposed: 歸約 - confidence: high - equals-latin-key: no - reason: no mismatch: 歸約 is the settled Traditional rendering of the reduce keyword
- row: pro - section: keywords - current: 按 - proposed: 逐一 - confidence: medium - equals-latin-key: no - reason: 按 reads as according to, while the keyword marks per-element iteration (each)
- row: factum - section: keywords - current: 求積 - proposed: 乘積 - confidence: medium - equals-latin-key: no - reason: 求積 reads as computing an area (quadrature), while the reduction computes a product
- row: numeratio - section: keywords - current: 計數 - proposed: 計數 - confidence: high - equals-latin-key: no - reason: no mismatch: 計數 is the settled Traditional rendering of the count keyword
- row: quilibet - section: keywords - current: 任一 - proposed: 任一 - confidence: high - equals-latin-key: no - reason: no mismatch: 任一 is the settled Traditional rendering of the any keyword
- row: argmaxima - section: keywords - current: 最大值索引 - proposed: 最大值索引 - confidence: high - equals-latin-key: no - reason: no mismatch: 最大值索引 is the settled Traditional rendering of the argmax keyword
- row: argminima - section: keywords - current: 最小值索引 - proposed: 最小值索引 - confidence: high - equals-latin-key: no - reason: no mismatch: 最小值索引 is the settled Traditional rendering of the argmin keyword
- row: ordinata - section: keywords - current: 排序 - proposed: 排序 - confidence: high - equals-latin-key: no - reason: no mismatch: 排序 is the settled Traditional rendering of the sort keyword
- row: ascendens - section: keywords - current: 升序 - proposed: 升序 - confidence: high - equals-latin-key: no - reason: no mismatch: 升序 is the settled Traditional rendering of the asc keyword
- row: descendens - section: keywords - current: 降序 - proposed: 降序 - confidence: high - equals-latin-key: no - reason: no mismatch: 降序 is the settled Traditional rendering of the desc keyword
- row: ascii - section: types - current: ascii - proposed: ascii - confidence: high - equals-latin-key: yes - reason: retained Latin: byte-equal to the key because ascii names the ASCII character range itself
- row: textus - section: types - current: 文字 - proposed: 字串 - confidence: high - equals-latin-key: no - reason: 文字 names text content, while the type is the string and 字串 is the computing term that pairs with littera 字元
- row: numerus - section: types - current: 整數 - proposed: 整數 - confidence: high - equals-latin-key: no - reason: no mismatch: 整數 is the settled Traditional name for the int type
- row: modulus - section: types - current: 模數类型 - proposed: 模數類型 - confidence: high - equals-latin-key: no - reason: the current 模數类型 mixes the traditional 數 with the simplified 类, while the pack writes 類型 (genus) and 型別 (typus), so the type marker must be the traditional 類型
- row: saturatus - section: types - current: 飽和 - proposed: 飽和 - confidence: high - equals-latin-key: no - reason: no mismatch: 飽和 is the settled Traditional name for the saturating type
- row: exactus - section: types - current: 精確 - proposed: 精確 - confidence: medium - equals-latin-key: no - reason: 精確 reads as precise, but exact arithmetic trapping overflow is the posture the type names
- row: fractus - section: types - current: 小數 - proposed: 浮點數 - confidence: high - equals-latin-key: no - reason: 小數 is a decimal fraction, while the type is floating point
- row: bivalens - section: types - current: 布林 - proposed: 布林 - confidence: high - equals-latin-key: no - reason: no mismatch: 布林 is the settled Traditional name for the bool type
- row: nihil - section: types - current: 無 - proposed: 無 - confidence: high - equals-latin-key: no - reason: 無 is the none type and stays distinct from vacuum 空值 and nulla 缺失
- row: vacuum - section: types - current: 空值 - proposed: 空值 - confidence: high - equals-latin-key: no - reason: 空值 is the void type and stays distinct from nihil 無
- row: numquam - section: types - current: 永不 - proposed: 永不 - confidence: high - equals-latin-key: no - reason: no mismatch: 永不 is the settled Traditional name for the never type
- row: ignotum - section: types - current: 未知 - proposed: 未知 - confidence: high - equals-latin-key: no - reason: no mismatch: 未知 is the settled Traditional name for the unknown type
- row: octeti - section: types - current: 位元組 - proposed: 位元組 - confidence: high - equals-latin-key: no - reason: no mismatch: 位元組 is the settled Traditional name for the bytes type
- row: octetus - section: types - current: 單位元組 - proposed: 單位元組 - confidence: high - equals-latin-key: no - reason: 單位元組 is the single-byte type and pairs with octeti 位元組
- row: clampa - section: types - current: 鉗制 - proposed: 鉗制 - confidence: high - equals-latin-key: no - reason: no mismatch: 鉗制 is the settled Traditional name for the clamp type
- row: reflexa - section: types - current: 反射 - proposed: 反射 - confidence: high - equals-latin-key: no - reason: no mismatch: 反射 is the settled Traditional name for the reflect type
- row: involuta - section: types - current: 環繞 - proposed: 環繞 - confidence: high - equals-latin-key: no - reason: no mismatch: 環繞 is the settled Traditional name for the wrap type
- row: littera - section: types - current: 字元 - proposed: 字元 - confidence: high - equals-latin-key: no - reason: no mismatch: 字元 is the settled Traditional name for the char type
- row: regex - section: types - current: 正規表示式 - proposed: 正規表示式 - confidence: high - equals-latin-key: no - reason: 正規表示式 is the settled Taiwan term, and the Simplified sibling keeping Latin regex is a sibling divergence rather than a translation requirement
- row: json - section: types - current: JSON - proposed: JSON - confidence: high - equals-latin-key: no - reason: JSON is the acronym in universal use and the Simplified sibling also keeps it
- row: valor - section: types - current: 值 - proposed: 值 - confidence: high - equals-latin-key: no - reason: no mismatch: 值 is the settled Traditional name for the value type
- row: instans - section: types - current: 執行個體 - proposed: 時刻 - confidence: high - equals-latin-key: no - reason: 執行個體 reads as an object instance, while the type is a timestamp
- row: objectum - section: types - current: 物件 - proposed: 物件 - confidence: high - equals-latin-key: no - reason: no mismatch: 物件 is the settled Traditional name for the object type
- row: quidlibet - section: types - current: 任意值 - proposed: 任意值 - confidence: high - equals-latin-key: no - reason: no mismatch: 任意值 is the settled Traditional name for the any type
- row: lista - section: types - current: 列表 - proposed: 列表 - confidence: high - equals-latin-key: no - reason: no mismatch: 列表 is the settled Traditional name for the list type
- row: tabula - section: types - current: 表格 - proposed: 對應表 - confidence: medium - equals-latin-key: no - reason: 表格 reads as a laid-out table, while the type is a key-value map that 對應表 names
- row: tabulae - section: types - current: 帳本 - proposed: 帳本 - confidence: low - equals-latin-key: no - reason: the pack marks the spelling provisional and unvalidated by a reader, and no better Traditional ledger word is settled
- row: copia - section: types - current: 副本 - proposed: 集合 - confidence: high - equals-latin-key: no - reason: 副本 reads as a duplicate copy and collides with copy 拷貝, while the type is a set
- row: promissum - section: types - current: 承諾 - proposed: 承諾 - confidence: high - equals-latin-key: no - reason: no mismatch: 承諾 is the settled Traditional name for the promise type
- row: cursor - section: types - current: 游標 - proposed: 疊代器 - confidence: medium - equals-latin-key: no - reason: 游標 is the screen or database cursor that the concept rationale warns against, while the type is the iterator
- row: queue - section: types - current: 佇列 - proposed: 佇列 - confidence: high - equals-latin-key: no - reason: no mismatch: 佇列 is the settled Traditional name for the queue type
- row: stack - section: types - current: 堆疊 - proposed: 堆疊 - confidence: high - equals-latin-key: no - reason: no mismatch: 堆疊 is the settled Traditional name for the stack type
- row: tensor - section: types - current: 張量 - proposed: 張量 - confidence: high - equals-latin-key: no - reason: no mismatch: 張量 is the settled Traditional name for the tensor type
- row: vector - section: types - current: 向量 - proposed: 向量 - confidence: high - equals-latin-key: no - reason: no mismatch: 向量 is the settled Traditional name for the vector type
- row: matrix - section: types - current: 矩陣 - proposed: 矩陣 - confidence: high - equals-latin-key: no - reason: no mismatch: 矩陣 is the settled Traditional name for the matrix type
- row: sparsa - section: types - current: 稀疏 - proposed: 稀疏 - confidence: high - equals-latin-key: no - reason: no mismatch: 稀疏 is the settled Traditional name for the sparse type
- row: atomic - section: types - current: 原子 - proposed: 原子 - confidence: high - equals-latin-key: no - reason: no mismatch: 原子 is the settled Traditional name for the atomic type
- row: intervallum - section: types - current: 區間 - proposed: 區間 - confidence: high - equals-latin-key: no - reason: no mismatch: 區間 is the settled Traditional name for the range type
- row: filtrum - section: types - current: 篩選器 - proposed: 篩選器 - confidence: high - equals-latin-key: no - reason: no mismatch: 篩選器 is the settled Traditional name for the filter type
- row: series - section: types - current: 序列 - proposed: 序列 - confidence: high - equals-latin-key: no - reason: no mismatch: 序列 is the settled Traditional name for the series type
- row: census - section: types - current: 普查 - proposed: 普查 - confidence: high - equals-latin-key: no - reason: no mismatch: 普查 is the settled Traditional name for the census type
- row: ratio - section: types - current: ratio - proposed: ratio - confidence: high - equals-latin-key: yes - reason: retained Latin: byte-equal to the key and spelled ratio in every pack
- row: sermo - section: types - current: 通道 - proposed: 通道 - confidence: high - equals-latin-key: no - reason: no mismatch: 通道 is the settled Traditional name for the channel type
- row: scrinium - section: types - current: 幀 - proposed: 幀 - confidence: high - equals-latin-key: no - reason: no mismatch: 幀 is the settled Traditional name for the frame type
- row: trapped - section: types - current: 受困 - proposed: 受困 - confidence: high - equals-latin-key: no - reason: no mismatch: 受困 is the settled Traditional name for the trapped type
- row: meus - section: types - current: 發送 - proposed: 發送 - confidence: high - equals-latin-key: no - reason: no mismatch: 發送 is the settled Traditional name for the send type
- row: tuus - section: types - current: 接收 - proposed: 接收 - confidence: high - equals-latin-key: no - reason: no mismatch: 接收 is the settled Traditional name for the recv type
