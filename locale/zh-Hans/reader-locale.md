# zh-Hans Reader Locale Review

**Locale:** `zh-Hans` (Simplified Chinese) — review only, no pack edit.
**Pack reviewed:** `locale/zh-Hans/pack.toml` at schema version 1: `[keywords]` (159
rows, line 5) and `[types]` (47 rows, line 170). `[diagnostics.*]` and the
`[intrinsics]` rows are outside this review.
**Concepts:** the per-key English concept in `locale/en/pack.toml` (`[keywords]`
line 5, `[types]` line 185) and the Latin authority in `locale/la/pack.toml`
(`[keywords]` line 10, `[types]` line 180).
**Rulings:** the operator ruling of 2026-10-11 (language-consistency audit item
3): a pack row must be the semantically accurate translation, and being the
compiled spelling does not make it right.
**Disposition:** findings only. No row in `locale/zh-Hans/pack.toml` is edited by
this review.

## Conventions

**(a) Part of speech and verb form.** A `[keywords]` row names what the token
*does* in the clause, so a statement keyword is a verb or a clause joint in its
imperative/conditional form, never an adjective that describes the surrounding
type. The pack is already inconsistent here. `sit` (line 15) is the
single-character imperative 设 — "suppose/let" as in 设 x = 0 — so it reads as
the let-binding it spells. `iacit` (line 24) is 可抛, which is the adjective
"throwable" (a property of a type), not the verb "throws" (the effect the
clause declares); the finding below proposes 抛出. The rule applied to every
row in this review: read the keyword in a clause frame in Chinese; if the word
can only be a modifier of a type name, or is a noun where the clause is a
command, it is a form mismatch and is flagged. A row already spelled as the
right form is left alone and marked high confidence.

**(b) Register and kept loanwords.** The register is technical written Chinese:
standard CS terms where Chinese has settled one (函数, 模块, 枚举, 断言, 张量),
not literary or classical vocabulary and not a translation of the Latin
etymology. A Latin or Japanese-kanji-derived term is kept only when the concept
*is* the loan (ASCII, JSON, regex, NaN, `radix` as the annotation family,
`primus_quem`/`ubi` as Latin-kept words), never when Chinese has a settled word
for the concept. Two rows show the failure this rule prevents: `praefixum` is
spelled 前缀 ("prefix"), which translates the Latin word while the concept is
comptime evaluation (编译期); `errata` is 勘误 ("errata/corrections"), not the
declared error types (错误).

**(c) Script, token shape, width.** Rows are Simplified Chinese in Han script,
one token per row, with no separator character inside a row — including the
paired words, so `sin` is the single token 否则如果 and `solum_in` is 仅于; the
only row whose value carries a separator (`_`) is `primus_quem` (line 90), a
multi-part Latin name that is kept Latin (see e); it is spelled the same in
every pack. Punctuation width is not a pack concern:
the lexer applies NFKC globally before keyword matching (see
`docs/design/reader-locale.md`, which names full/half-width punctuation as
handled by global NFKC, not per locale), so a full-width comma or bracket in
authored source collapses to its half-width keyword-matching form. A row that
needs two characters but whose concept is one word keeps a *two-character
compound whose second character carries the distinguishing sense* (等定 /
等变 / 等返 / 等弃, and the proposed 选项集 for 可选项): do not split the word
into two lexical tokens and do not borrow a longer literary phrase.
Four-character compounds already in the pack (否则如果, 最大值索引) are kept
because NFKC and the lexer treat them as one token.

**(d) One Latin key that is both a keyword and a type.** When one Latin key
appears in both sections (`modulus`, `cursor`), the `[types]` row is the
`[keywords]` row plus the suffix 类型, and a correction to the base word is
applied to both rows in the same proposal. Today `modulus` is 模数 at line 13
and 模数类型 at line 174, so the two are related but share the same base error:
模数 names the modulus m, not the modular-integer type head. The review therefore
proposes 模整数 and 模整数类型 together. `cursor` is the counter-example of the
same rule applied wrongly: the keyword (line 135) is 迭代器 (iterator) and the
type (line 203) is 游标 (cursor), i.e. the two rows exchanged their concepts;
the review proposes 游标 for the cursor annotation and 迭代器 for the iterator
type, restoring both the en concept and the keyword/type relation. A method
surface that shares the key (`s.meus<T>()`, `s.tuus<T>()`) follows the keyword
row, so its row is judged with that row.

**(e) When a row keeps its Latin spelling.** A row may keep the Latin key when
(i) the concept has no settled Chinese word, (ii) the Latin spelling is itself
the technical name the reader will meet in every other language, and (iii) the
row says so: the keep-Latin rows are declared here and marked
`equals-latin-key: yes` in the findings. This review keeps `json` (line 191),
`regex` (line 190), `radix` (line 148), `primus_quem` (line 90), `ubi` (line 91)
and `ascii` (line 171, proposed from 窄字串) on that ground. `ratio` at line 217
is *not* kept: the en concept is record and the pack already translates its
siblings (`filtrum`, `series`, `census`), so keeping `ratio` there is an omission,
not a loan. `nonnumerus` (line 64) is kept as `nan` — byte-equal to the en row at
line 70 — and that is a kept loan, not a Latin-key copy: the key is `nonnumerus`,
so `equals-latin-key` is `no` for it. A row that copies the Latin key while
Chinese has a word (`ratio` for record) is a finding, not a loan, and a row
that renders a Latin root rather than the concept (闭包 for lambda, 前缀 for
comptime) is a translation finding.

**(f) What this reviewer could not settle.** Three things. First, the exact
census/runtime word for the relational views (`ratio` vs 记录, `census` vs
普查): the pack's S2-U5 comments give no ruling, and the en concept names are
`record`/`census` with no gloss. Second, `tabulae` (line 200) is flagged
PROVISIONAL in the pack itself as unvalidated by a reader of this language; 账本
is plausible but stays low confidence rather than guessed. Third, whether the
await family (等定/等变/等返/等弃) should expose the awaited *value posture*
(定/变) or the *binding* (`let`/`mut`), an operator call this reviewer did not
make.

### Named question: apud 在 versus 于 or 於

`apud` is 在 at line 45 today. The en concept is `at` and the la word is `apud`;
`docs/design/llm-locale.md` gives the concept as at, and the surface is used by
`itera ... apud [i]` — the index/coordinate position of an iteration
(`itera ex m apud [k] ...`) and of the coordinate read/method (`apud [i, j]`).
The two alternatives are 于 and 於: 於 is the traditional/classical form and is
not written in Simplified Chinese prose, so it is listed for completeness only.
在 is the modern locative verb/preposition: 在 x 读作 "is located at x", which is
a spatial position and, in running prose, a full verb, so a coordinate clause
`apud [i]` reads as "located at [i]" — a place, not a bound index. 于 is the
bound, literary preposition: it attaches to the preceding clause
(遍历于 [i]) and reads as the "at (index)" of a coordinate or position without
introducing a place, which is exactly the apud slot. Recommendation: 于, at
medium confidence — 在 is not wrong for a coordinate in speech, but no
zh-Hans pack reader has validated 于 in authored source, so this is a register
and binding recommendation rather than a defect claim. 於 is not recommended for
zh-Hans. No pack edit is made here: the operator asked for the question by name,
and `locale/zh-Hans/pack.toml` stays untouched.

## Findings

Each line is one row of this pack. `- section:` is `keywords` or `types`;
`current:` is the word in the pack today; `proposed:` repeats the same word when
nothing changes; `equals-latin-key: yes` marks a kept-Latin row declared in
Conventions (e). Every row of this pack is judged; no `[intrinsics]` row is
included, so the pack's `apud` at line 3595 is deliberately unjudged here.

- row: discretio - section: keywords - current: 判别 - proposed: 联合 - confidence: high - equals-latin-key: no - reason: 判别 names the act of discriminating, not the tagged-union type; the word belongs to discerne (match)
- row: fixum - section: keywords - current: 常量 - proposed: 常量 - confidence: high - equals-latin-key: no - reason: compile-time constant; 常量 is the fixed Chinese term and matches the en word const
- row: functio - section: keywords - current: 函数 - proposed: 函数 - confidence: high - equals-latin-key: no - reason: function; 函数 is the standard declaration noun and matches fn
- row: genus - section: keywords - current: 类 - proposed: 类 - confidence: high - equals-latin-key: no - reason: class; 类 is the settled Chinese class word, so the genus declaration reads as a class
- row: implendum - section: keywords - current: 契约 - proposed: 接口 - confidence: high - equals-latin-key: no - reason: 契约 reads as a business contract; the row is a signature-only interface, whose settled word is 接口
- row: importa - section: keywords - current: 导入 - proposed: 导入 - confidence: high - equals-latin-key: no - reason: import; 导入 is the standard import verb and names the clause it introduces
- row: regio - section: keywords - current: 模块 - proposed: 模块 - confidence: high - equals-latin-key: no - reason: module; 模块 is the standard module noun and matches the en word
- row: modulus - section: keywords - current: 模数 - proposed: 模整数 - confidence: medium - equals-latin-key: no - reason: 模数 names the modulus m, not the modular-integer type head it spells; 模整数<u8> reads as the integer that wraps
- row: ordo - section: keywords - current: 枚举 - proposed: 枚举 - confidence: high - equals-latin-key: no - reason: enum; 枚举 is the standard Chinese word and matches the en word
- row: sit - section: keywords - current: 设 - proposed: 设 - confidence: high - equals-latin-key: no - reason: let-binding; the single-character imperative 设 reads as the mathematical 设 x = …, so the verb form is right
- row: typus - section: keywords - current: 类型 - proposed: 类型 - confidence: high - equals-latin-key: no - reason: type; 类型 is the standard noun and matches the en word
- row: schema - section: keywords - current: 架构 - proposed: 模式 - confidence: medium - equals-latin-key: no - reason: 架构 reads as system architecture; the row is a relational heading, and a relation schema is a 模式
- row: columna - section: keywords - current: 列 - proposed: 列 - confidence: high - equals-latin-key: no - reason: relational column; 列 is the exact Chinese word and matches the en word
- row: varia - section: keywords - current: 可变 - proposed: 可变 - confidence: high - equals-latin-key: no - reason: mutable binding; 可变 is the modifier form that matches mut as a binding word
- row: ceteri - section: keywords - current: 其余 - proposed: 其余 - confidence: high - equals-latin-key: no - reason: rest binding; 其余 names the remaining elements and matches rest
- row: errata - section: keywords - current: 勘误 - proposed: 错误 - confidence: high - equals-latin-key: no - reason: 勘误 means a list of corrections/errata, not declared error types; the row declares errors, so 错误
- row: exitus - section: keywords - current: 退出 - proposed: 退出 - confidence: high - equals-latin-key: no - reason: exit; 退出 is the standard process-exit word and matches exit
- row: generis - section: keywords - current: 静态 - proposed: 静态 - confidence: high - equals-latin-key: no - reason: static member; 静态 matches static and the row survives only as the retired-spelling recognizer
- row: iacit - section: keywords - current: 可抛 - proposed: 抛出 - confidence: high - equals-latin-key: no - reason: 可抛 is the adjective throwable, not the effect verb throws; the verb form for the clause is 抛出
- row: immutata - section: keywords - current: 不变 - proposed: 只读 - confidence: medium - equals-latin-key: no - reason: 不变 reads as an immutable value; the row means set once and never reassigned, which Chinese spells 只读 (read-only)
- row: interna - section: keywords - current: 内部 - proposed: 内部 - confidence: high - equals-latin-key: no - reason: internal visibility; 内部 is the standard visibility word and matches internal
- row: magnitudo - section: keywords - current: 维度 - proposed: 规模 - confidence: medium - equals-latin-key: no - reason: 维度 is a tensor dimension; the row is a const-generic size parameter, whose Chinese word is 规模
- row: iuncta - section: keywords - current: 元组 - proposed: 元组 - confidence: high - equals-latin-key: no - reason: tuple; 元组 is the standard Chinese word and matches tuple
- row: optiones - section: keywords - current: 可选项 - proposed: 选项集 - confidence: medium - equals-latin-key: no - reason: 可选项 reads as optional items and collides with sponte (可选); the row is the CLI options bundle, so 选项集
- row: praefixum - section: keywords - current: 前缀 - proposed: 编译期 - confidence: high - equals-latin-key: no - reason: 前缀 translates the Latin word prefix, not the concept; the row is comptime evaluation, which Chinese spells 编译期
- row: insere - section: keywords - current: 嵌入 - proposed: 嵌入 - confidence: high - equals-latin-key: no - reason: embed; 嵌入 is the standard verb and matches embed
- row: privata - section: keywords - current: 私有 - proposed: 私有 - confidence: high - equals-latin-key: no - reason: private visibility; 私有 is the standard word and matches private
- row: protecta - section: keywords - current: 保护 - proposed: 保护 - confidence: high - equals-latin-key: no - reason: protected visibility; 保护 is the standard short form and matches protected
- row: publica - section: keywords - current: 公开 - proposed: 公开 - confidence: high - equals-latin-key: no - reason: public visibility; 公开 is the standard word and matches public
- row: sponte - section: keywords - current: 可选 - proposed: 可选 - confidence: high - equals-latin-key: no - reason: optional parameter slot; 可选 is the modifier form and matches optional
- row: casu - section: keywords - current: 情况 - proposed: 情形 - confidence: medium - equals-latin-key: no - reason: 情况 names a general situation; a match arm is a 情形, which keeps it distinct from elige (选择)
- row: ceterum - section: keywords - current: 默认 - proposed: 默认 - confidence: high - equals-latin-key: no - reason: default catch-all arm; 默认 is the standard word and matches default
- row: custodi - section: keywords - current: 守护 - proposed: 守护 - confidence: high - equals-latin-key: no - reason: guard clause; 守护 is the standard word and matches guard
- row: discerne - section: keywords - current: 匹配 - proposed: 匹配 - confidence: high - equals-latin-key: no - reason: pattern match; 匹配 is the standard word and matches match
- row: dum - section: keywords - current: 当 - proposed: 当 - confidence: high - equals-latin-key: no - reason: while loop; 当 reads as 当 cond (while cond) in Chinese algorithmic prose, so the single word carries the loop head
- row: elige - section: keywords - current: 选择 - proposed: 选择 - confidence: high - equals-latin-key: no - reason: value-dispatch head; 选择 reads as choose-among-values and stays distinct from casu (情形)
- row: ergo - section: keywords - current: 则 - proposed: 则 - confidence: high - equals-latin-key: no - reason: then joint; 则 is the classical consequence joint and matches then in if cond 则 stmt
- row: fac - section: keywords - current: 执行 - proposed: 执行 - confidence: high - equals-latin-key: no - reason: do block; 执行 is the imperative do verb and matches the en word
- row: itera - section: keywords - current: 遍历 - proposed: 遍历 - confidence: high - equals-latin-key: no - reason: for iteration; 遍历 is the standard traversal verb and matches for
- row: apud - section: keywords - current: 在 - proposed: 于 - confidence: medium - equals-latin-key: no - reason: 在 is the modern locative verb and reads as a place; apud marks an index or coordinate position, where the bound 于 reads as at-index
- row: secus - section: keywords - current: 否则 - proposed: 否则 - confidence: high - equals-latin-key: no - reason: else arm; 否则 is the standard word and matches else
- row: si - section: keywords - current: 如果 - proposed: 如果 - confidence: high - equals-latin-key: no - reason: if head; 如果 is the standard conditional word and matches if
- row: sin - section: keywords - current: 否则如果 - proposed: 否则如果 - confidence: high - equals-latin-key: no - reason: elif head; the paired single token 否则如果 says else-if without a separator, matching the en pairing
- row: perge - section: keywords - current: 继续 - proposed: 继续 - confidence: high - equals-latin-key: no - reason: continue; 继续 is the standard loop-continue verb and matches continue
- row: redde - section: keywords - current: 返回 - proposed: 返回 - confidence: high - equals-latin-key: no - reason: return; 返回 is the standard return verb and matches return
- row: rumpe - section: keywords - current: 中断 - proposed: 跳出 - confidence: medium - equals-latin-key: no - reason: 中断 reads as interrupt/abort; the row leaves a loop, which Chinese spells 跳出
- row: tacet - section: keywords - current: 静默 - proposed: 静默 - confidence: high - equals-latin-key: no - reason: no-op pass; 静默 is the no-op word and matches pass
- row: adfirma - section: keywords - current: 断言 - proposed: 断言 - confidence: high - equals-latin-key: no - reason: assert; 断言 is the standard assertion word and matches assert
- row: cape - section: keywords - current: 捕获 - proposed: 捕获 - confidence: high - equals-latin-key: no - reason: catch; 捕获 is the standard catch verb and matches catch
- row: capta - section: keywords - current: 陷阱 - proposed: 陷阱 - confidence: high - equals-latin-key: no - reason: trap; 陷阱 is the standard trap word and matches trap
- row: cede - section: keywords - current: 让出 - proposed: 让出 - confidence: high - equals-latin-key: no - reason: yield one stream item; 让出 is the standard yield verb and matches yield
- row: iace - section: keywords - current: 抛错 - proposed: 抛错 - confidence: high - equals-latin-key: no - reason: throw a failure; 抛错 is the verb form and matches throw
- row: mori - section: keywords - current: 崩溃 - proposed: 崩溃 - confidence: high - equals-latin-key: no - reason: unrecoverable abort; 崩溃 is the panic word and matches panic
- row: requirit - section: keywords - current: 需求 - proposed: 要求 - confidence: medium - equals-latin-key: no - reason: 需求 is the noun requirement; the row is a recoverable guard verb, so the verb form 要求
- row: reice - section: keywords - current: 拒绝 - proposed: 拒绝 - confidence: high - equals-latin-key: no - reason: reject; 拒绝 is the standard verb and matches reject
- row: clausura - section: keywords - current: 闭包 - proposed: 匿名函数 - confidence: medium - equals-latin-key: no - reason: 闭包 names a closure, not an anonymous function; the row is a lambda, whose Chinese word is 匿名函数
- row: falsum - section: keywords - current: 假 - proposed: 假 - confidence: high - equals-latin-key: no - reason: false literal; 假 is the standard truth word and matches false
- row: verum - section: keywords - current: 真 - proposed: 真 - confidence: high - equals-latin-key: no - reason: true literal; 真 is the standard truth word and matches true
- row: nonnumerus - section: keywords - current: nan - proposed: nan - confidence: high - equals-latin-key: no - reason: nan is the universal IEEE float sentinel and is kept Latin by the declaration rule; it equals the en value, not the Latin key
- row: aut - section: keywords - current: 或 - proposed: 或 - confidence: high - equals-latin-key: no - reason: logical or; 或 is the standard disjunction and matches or
- row: est - section: keywords - current: 是 - proposed: 是 - confidence: high - equals-latin-key: no - reason: type test; 是 reads as x 是 T (x is a T), which is the type-test meaning and not value equality
- row: et - section: keywords - current: 且 - proposed: 且 - confidence: high - equals-latin-key: no - reason: logical and; 且 is the standard conjunction and matches and
- row: non - section: keywords - current: 非 - proposed: 非 - confidence: high - equals-latin-key: no - reason: logical not; 非 is the standard negation and matches not
- row: vel - section: keywords - current: 兜底 - proposed: 兜底 - confidence: high - equals-latin-key: no - reason: nullish coalesce; 兜底 is the fallback reading and matches coalesce as a value fallback
- row: ego - section: keywords - current: 自身 - proposed: 自身 - confidence: high - equals-latin-key: no - reason: method receiver; 自身 is the standard self word and matches self
- row: finge - section: keywords - current: 构造 - proposed: 变体 - confidence: medium - equals-latin-key: no - reason: 构造 reads as generic construction and collides with the class constructor; the row constructs a tagged-union variant, so 变体
- row: implet - section: keywords - current: 实现 - proposed: 实现 - confidence: high - equals-latin-key: no - reason: implements relation; 实现 is the standard verb and matches implements
- row: mone - section: keywords - current: 警告 - proposed: 警告 - confidence: high - equals-latin-key: no - reason: warn output; 警告 is the standard warning word and matches warn
- row: nota - section: keywords - current: 显示 - proposed: 显示 - confidence: high - equals-latin-key: no - reason: primary print output; 显示 is the print word and stays distinct from vide (查看, debug)
- row: scribe - section: keywords - current: 写入 - proposed: 写入 - confidence: high - equals-latin-key: no - reason: raw write channel; 写入 is the write verb and matches write
- row: vide - section: keywords - current: 查看 - proposed: 查看 - confidence: high - equals-latin-key: no - reason: debug inspect; 查看 is the inspect verb and matches debug
- row: argumenta - section: keywords - current: 参数 - proposed: 参数 - confidence: high - equals-latin-key: no - reason: CLI args; 参数 is the argument word and matches args
- row: incipiet - section: keywords - current: 异步入口 - proposed: 异步入口 - confidence: high - equals-latin-key: no - reason: async main; 异步入口 says async entry in one token and matches async_main
- row: incipit - section: keywords - current: 入口 - proposed: 入口 - confidence: high - equals-latin-key: no - reason: program entry; 入口 is the entry word and matches main
- row: ad - section: keywords - current: 调用 - proposed: 调用 - confidence: high - equals-latin-key: no - reason: capability route call; 调用 is the call verb and matches call
- row: fiet - section: keywords - current: 异步 - proposed: 异步 - confidence: high - equals-latin-key: no - reason: async posture; 异步 is the async modifier and matches async
- row: fiunt - section: keywords - current: 流 - proposed: 流 - confidence: high - equals-latin-key: no - reason: sync generator; 流 is the stream word and matches generator as a sync stream
- row: fient - section: keywords - current: 异流 - proposed: 异流 - confidence: high - equals-latin-key: no - reason: async generator; 异流 is the regular async-stream compound and matches async_generator
- row: figendum - section: keywords - current: 等定 - proposed: 等定 - confidence: high - equals-latin-key: no - reason: the compound is the regular await-const form (等 await + 定 fixed binding) and matches await_const
- row: variandum - section: keywords - current: 等变 - proposed: 等变 - confidence: high - equals-latin-key: no - reason: the compound is the regular await-var form (等 await + 变 mutable binding) and matches await_var
- row: reddet - section: keywords - current: 等返 - proposed: 等返 - confidence: high - equals-latin-key: no - reason: the compound is the regular return-await form (等 await + 返 return) and matches return_await
- row: tacebit - section: keywords - current: 等弃 - proposed: 等弃 - confidence: high - equals-latin-key: no - reason: the compound is the regular await-and-discard form (等 await + 弃 discard) and matches await
- row: vacua - section: keywords - current: 空集 - proposed: 空集 - confidence: high - equals-latin-key: no - reason: empty-collection marker; 空集 says empty collection in one token and matches empty
- row: primus_quem - section: keywords - current: primus_quem - proposed: primus_quem - confidence: high - equals-latin-key: yes - reason: Latin kept by declaration: the first-match reduction has no settled Chinese word, and the en row also keeps the Latin
- row: ubi - section: keywords - current: ubi - proposed: ubi - confidence: high - equals-latin-key: yes - reason: Latin kept by declaration: the find-index method names a predicate position and the en row also keeps the Latin
- row: summa - section: keywords - current: 求和 - proposed: 求和 - confidence: high - equals-latin-key: no - reason: sum reduce; 求和 is the standard summation verb and matches sum
- row: filum - section: keywords - current: 线程 - proposed: 线程 - confidence: high - equals-latin-key: no - reason: thread; 线程 is the standard Chinese word and matches thread
- row: maxima - section: keywords - current: 最大 - proposed: 最大 - confidence: high - equals-latin-key: no - reason: max reduction; 最大 is the standard maximum word and matches max
- row: minima - section: keywords - current: 最小 - proposed: 最小 - confidence: high - equals-latin-key: no - reason: min reduction; 最小 is the standard minimum word and matches min
- row: de - section: keywords - current: 借自 - proposed: 借用 - confidence: medium - equals-latin-key: no - reason: 借自 reads as a provenance phrase borrowed from; the row is the immutable borrow parameter, so 借用
- row: ex - section: keywords - current: 取自 - proposed: 取自 - confidence: high - equals-latin-key: no - reason: import source / extraction; 取自 reads as taken from and matches from
- row: in - section: keywords - current: 可变 - proposed: 可变借用 - confidence: medium - equals-latin-key: no - reason: 可变 duplicates varia exactly; the row is the mutable borrow parameter, so 可变借用 names the borrow and keeps one word per key
- row: own - section: keywords - current: 拥有 - proposed: 拥有 - confidence: high - equals-latin-key: no - reason: owning parameter; 拥有 is the ownership verb and matches own
- row: copy - section: keywords - current: 拷贝 - proposed: 拷贝 - confidence: high - equals-latin-key: no - reason: copy parameter; 拷贝 is the standard copy word and matches copy
- row: axis - section: keywords - current: 轴 - proposed: 轴 - confidence: high - equals-latin-key: no - reason: tensor axis; 轴 is the axis word and matches axis
- row: lege - section: keywords - current: 读取 - proposed: 读取 - confidence: high - equals-latin-key: no - reason: read input; 读取 is the read verb and matches read
- row: lineam - section: keywords - current: 行 - proposed: 行 - confidence: high - equals-latin-key: no - reason: line unit; 行 is the line word and matches line
- row: omnia - section: keywords - current: 全部 - proposed: 全部 - confidence: high - equals-latin-key: no - reason: wildcard import; 全部 is the all word and matches all
- row: scriptum - section: keywords - current: 格式化 - proposed: 格式化 - confidence: high - equals-latin-key: no - reason: template formatting; 格式化 is the format verb and matches format
- row: sparge - section: keywords - current: 展开 - proposed: 展开 - confidence: high - equals-latin-key: no - reason: spread; 展开 is the standard spread word and matches spread
- row: ut - section: keywords - current: 作为 - proposed: 作为 - confidence: high - equals-latin-key: no - reason: import alias; 作为 reads as as/alias and matches as
- row: via - section: keywords - current: 经由 - proposed: 经由 - confidence: high - equals-latin-key: no - reason: convert-hint clause; 经由 reads as via and matches the en word via
- row: ante - section: keywords - current: 迄 - proposed: 之前 - confidence: medium - equals-latin-key: no - reason: 迄 reads as up-to/until, which is the usque end bound, not the range start; ante is the before side, so 之前
- row: inter - section: keywords - current: 间 - proposed: 间 - confidence: high - equals-latin-key: no - reason: between; 间 is the between word and the row survives only as a retired-membership recognizer
- row: intra - section: keywords - current: 内 - proposed: 内 - confidence: high - equals-latin-key: no - reason: within; 内 is the containment word and the row survives only as a retired-membership recognizer
- row: per - section: keywords - current: 步 - proposed: 步 - confidence: high - equals-latin-key: no - reason: range stride; 步 is the step word and matches step
- row: ab - section: keywords - current: 范围 - proposed: 范围 - confidence: high - equals-latin-key: no - reason: range marker; 范围 is the range noun and matches range
- row: usque - section: keywords - current: 到 - proposed: 到 - confidence: high - equals-latin-key: no - reason: exclusive range end; 到 is the until word and matches until
- row: erratur - section: keywords - current: 预期失败 - proposed: 预期失败 - confidence: high - equals-latin-key: no - reason: expect-failure marker; 预期失败 says the case must fail and matches expect_failure
- row: fragilis - section: keywords - current: 易碎 - proposed: 偶发 - confidence: medium - equals-latin-key: no - reason: 易碎 reads as physical brittleness; a flaky test fails intermittently, which Chinese spells 偶发, and 不稳定 is already nondum
- row: futurum - section: keywords - current: 预期 - proposed: 待办 - confidence: high - equals-latin-key: no - reason: 预期 reads as expectation and collides with erratur (预期失败); a pending todo is 待办
- row: metior - section: keywords - current: 计量 - proposed: 基准 - confidence: medium - equals-latin-key: no - reason: 计量 reads as metering; the row names a benchmark block, whose Chinese word is 基准
- row: omitte - section: keywords - current: 跳过 - proposed: 跳过 - confidence: high - equals-latin-key: no - reason: skip a test; 跳过 is the skip verb and matches skip
- row: postpara - section: keywords - current: 收尾 - proposed: 收尾 - confidence: high - equals-latin-key: no - reason: teardown; 收尾 is the wrap-up word and pairs with praepara
- row: postparabit - section: keywords - current: 异步收尾 - proposed: 异步收尾 - confidence: high - equals-latin-key: no - reason: async teardown; 异步收尾 keeps the async prefix on the teardown word and matches async_teardown
- row: praepara - section: keywords - current: 备置 - proposed: 准备 - confidence: medium - equals-latin-key: no - reason: 备置 is a rare, non-term coinage; test setup is 准备, which pairs with postpara (收尾)
- row: praeparabit - section: keywords - current: 异步备置 - proposed: 异步准备 - confidence: medium - equals-latin-key: no - reason: async setup; 异步准备 follows the corrected praepara word and matches async_setup
- row: proba - section: keywords - current: 测试 - proposed: 测试 - confidence: high - equals-latin-key: no - reason: test; 测试 is the standard test word and matches the en test
- row: probandum - section: keywords - current: 验题 - proposed: 描述 - confidence: high - equals-latin-key: no - reason: 验题 is not a Chinese term; the describe block names a suite grouping, which Chinese spells 描述
- row: repete - section: keywords - current: 重复 - proposed: 重复 - confidence: high - equals-latin-key: no - reason: repeat; 重复 is the standard word and matches repeat
- row: solum - section: keywords - current: 仅 - proposed: 仅 - confidence: high - equals-latin-key: no - reason: only-test modifier; 仅 is the only word and matches only
- row: solum_in - section: keywords - current: 仅于 - proposed: 仅于 - confidence: high - equals-latin-key: no - reason: platform-scoped only; 仅于 joins only and in as one token and matches only_in
- row: tag - section: keywords - current: 标签 - proposed: 标签 - confidence: high - equals-latin-key: no - reason: test metadata tag; 标签 is the tag noun and matches tag
- row: temporis - section: keywords - current: 时限 - proposed: 时限 - confidence: high - equals-latin-key: no - reason: test timeout; 时限 is the time limit and matches timeout
- row: nulla - section: keywords - current: 缺失 - proposed: 空 - confidence: medium - equals-latin-key: no - reason: 缺失 reads as an absence/error state; the row is the null value literal, which Chinese spells 空
- row: cli - section: keywords - current: 命令行 - proposed: 命令行 - confidence: high - equals-latin-key: no - reason: CLI annotation; 命令行 is the command line word and matches cli
- row: commune - section: keywords - current: 共享 - proposed: 共享 - confidence: high - equals-latin-key: no - reason: shared annotation; 共享 is the shared word and matches shared
- row: cursor - section: keywords - current: 迭代器 - proposed: 游标 - confidence: medium - equals-latin-key: no - reason: the row is the annotation cursor, whose Chinese word is 游标; 迭代器 names the iterator type and is used by the type row
- row: futura - section: keywords - current: 未来 - proposed: 未来 - confidence: high - equals-latin-key: no - reason: future annotation; 未来 is the future word and matches future
- row: imperium - section: keywords - current: 命令 - proposed: 命令 - confidence: high - equals-latin-key: no - reason: command annotation; 命令 is the command word and matches command
- row: operandus - section: keywords - current: 操作数 - proposed: 操作数 - confidence: high - equals-latin-key: no - reason: operand annotation; 操作数 is the standard operand word and matches operand
- row: optio - section: keywords - current: 选项 - proposed: 选项 - confidence: high - equals-latin-key: no - reason: option annotation; 选项 is the option noun and matches option
- row: conversion - section: keywords - current: 变换 - proposed: 转换方向 - confidence: medium - equals-latin-key: no - reason: 变换 reads as a value transform and duplicates conversio (转换); this row marks a conversion direction, so 转换方向
- row: conversio - section: keywords - current: 转换 - proposed: 转换 - confidence: high - equals-latin-key: no - reason: conversion annotation; 转换 is the standard conversion word and matches the en row
- row: nucleum - section: keywords - current: 内核 - proposed: 内核 - confidence: high - equals-latin-key: no - reason: kernel annotation; 内核 is the kernel word and matches kernel
- row: vertex - section: keywords - current: 顶点 - proposed: 顶点 - confidence: high - equals-latin-key: no - reason: vertex annotation; 顶点 is the standard vertex word and matches vertex
- row: fragment - section: keywords - current: 片段 - proposed: 片段 - confidence: high - equals-latin-key: no - reason: fragment annotation; 片段 is the fragment word and matches fragment
- row: nondum - section: keywords - current: 不稳定 - proposed: 不稳定 - confidence: high - equals-latin-key: no - reason: unstable annotation; 不稳定 is the unstable word and matches unstable
- row: verte - section: keywords - current: 改名 - proposed: 改名 - confidence: high - equals-latin-key: no - reason: rename annotation; 改名 is the rename verb and matches rename
- row: nomen - section: keywords - current: 名称 - proposed: 名称 - confidence: high - equals-latin-key: no - reason: name annotation; 名称 is the name noun and matches name
- row: radix - section: keywords - current: radix - proposed: radix - confidence: high - equals-latin-key: yes - reason: Latin kept by declaration: the row names the Radix annotation family and module, and the en row also keeps the Latin
- row: brevis - section: keywords - current: 简 - proposed: 简 - confidence: high - equals-latin-key: no - reason: short form marker; 简 is the short adjective and pairs with longum (详)
- row: descriptio - section: keywords - current: 描述 - proposed: 描述 - confidence: high - equals-latin-key: no - reason: description annotation; 描述 is the description word and matches description
- row: longum - section: keywords - current: 详 - proposed: 详 - confidence: high - equals-latin-key: no - reason: long form marker; 详 is the detailed adjective and pairs with brevis (简)
- row: lane - section: keywords - current: 车道 - proposed: 泳道 - confidence: medium - equals-latin-key: no - reason: 车道 is a road traffic lane; the row is an execution lane, whose Chinese word is 泳道
- row: ubique - section: keywords - current: 全局 - proposed: 全局 - confidence: high - equals-latin-key: no - reason: global annotation; 全局 is the global word and matches global
- row: annotatio - section: keywords - current: 注解 - proposed: 注解 - confidence: high - equals-latin-key: no - reason: annotation; 注解 is the annotation word and matches annotation
- row: versio - section: keywords - current: 版本 - proposed: 版本 - confidence: high - equals-latin-key: no - reason: version annotation; 版本 is the version noun and matches version
- row: imperia - section: keywords - current: 命令集 - proposed: 命令集 - confidence: high - equals-latin-key: no - reason: commands bundle; 命令集 is the regular plural compound of imperium (命令) and matches commands
- row: reducta - section: keywords - current: 归约 - proposed: 归约 - confidence: high - equals-latin-key: no - reason: reduce expression; 归约 is the standard reduction word and matches reduce
- row: pro - section: keywords - current: 按 - proposed: 按 - confidence: high - equals-latin-key: no - reason: per-element reduction head; 按 reads as per/according to and matches per
- row: factum - section: keywords - current: 求积 - proposed: 求积 - confidence: high - equals-latin-key: no - reason: product reduction; 求积 is the standard product-sum verb and matches product
- row: numeratio - section: keywords - current: 计数 - proposed: 计数 - confidence: high - equals-latin-key: no - reason: count reduction; 计数 is the count verb and matches count
- row: quilibet - section: keywords - current: 任一 - proposed: 任一 - confidence: high - equals-latin-key: no - reason: any reduction; 任一 is the any word and matches any
- row: argmaxima - section: keywords - current: 最大值索引 - proposed: 最大值索引 - confidence: high - equals-latin-key: no - reason: argmax; 最大值索引 says max-value index and matches argmax
- row: argminima - section: keywords - current: 最小值索引 - proposed: 最小值索引 - confidence: high - equals-latin-key: no - reason: argmin; 最小值索引 says min-value index and matches argmin
- row: ordinata - section: keywords - current: 排序 - proposed: 排序 - confidence: high - equals-latin-key: no - reason: sort expression; 排序 is the sort verb and matches sort
- row: ascendens - section: keywords - current: 升序 - proposed: 升序 - confidence: high - equals-latin-key: no - reason: ascending order; 升序 is the standard term and matches asc
- row: descendens - section: keywords - current: 降序 - proposed: 降序 - confidence: high - equals-latin-key: no - reason: descending order; 降序 is the standard term and matches desc
- row: ascii - section: types - current: 窄字串 - proposed: ascii - confidence: high - equals-latin-key: yes - reason: ASCII is an acronym with no accepted Chinese shortening; 窄字串 mistranslates it as a narrow string, so the row keeps the Latin by declaration
- row: textus - section: types - current: 文本 - proposed: 文本 - confidence: high - equals-latin-key: no - reason: string type; 文本 is the text word and matches string
- row: numerus - section: types - current: 整数 - proposed: 整数 - confidence: high - equals-latin-key: no - reason: int type; 整数 is the integer word and matches int
- row: modulus - section: types - current: 模数类型 - proposed: 模整数类型 - confidence: medium - equals-latin-key: no - reason: the type row follows the corrected keyword base (模整数) plus 类型, so the two rows stay related
- row: saturatus - section: types - current: 饱和 - proposed: 饱和 - confidence: high - equals-latin-key: no - reason: saturating arithmetic; 饱和 is the standard saturating word and matches saturating
- row: exactus - section: types - current: 精确 - proposed: 陷阱 - confidence: medium - equals-latin-key: no - reason: 精确 reads as exactness; the row is the trapping arithmetic policy, sibling of 饱和, so 陷阱 (trap)
- row: fractus - section: types - current: 小数 - proposed: 浮点 - confidence: high - equals-latin-key: no - reason: 小数 is a decimal fraction; the row is the floating-point type, whose Chinese word is 浮点
- row: bivalens - section: types - current: 布尔 - proposed: 布尔 - confidence: high - equals-latin-key: no - reason: bool type; 布尔 is the standard transliteration and matches bool
- row: nihil - section: types - current: 无 - proposed: 无 - confidence: high - equals-latin-key: no - reason: none type; 无 is the none word and reads as the empty type once the null value row is fixed
- row: vacuum - section: types - current: 无值 - proposed: 无值 - confidence: high - equals-latin-key: no - reason: void type; 无值 says no value and matches void
- row: numquam - section: types - current: 永不 - proposed: 永不 - confidence: high - equals-latin-key: no - reason: never type; 永不 is the never word and matches never
- row: ignotum - section: types - current: 未知 - proposed: 未知 - confidence: high - equals-latin-key: no - reason: unknown type; 未知 is the unknown word and matches unknown
- row: octeti - section: types - current: 字节 - proposed: 字节 - confidence: high - equals-latin-key: no - reason: bytes type; 字节 is the byte noun and matches bytes
- row: octetus - section: types - current: 单字节 - proposed: 单字节 - confidence: high - equals-latin-key: no - reason: byte type; 单字节 says one byte and stays distinct from octeti (字节)
- row: clampa - section: types - current: 钳制 - proposed: 钳制 - confidence: high - equals-latin-key: no - reason: clamp edge policy; 钳制 is the clamp verb and matches clamp
- row: reflexa - section: types - current: 反射 - proposed: 反射 - confidence: high - equals-latin-key: no - reason: reflect edge policy; 反射 is the reflect word and matches reflect
- row: involuta - section: types - current: 环绕 - proposed: 环绕 - confidence: high - equals-latin-key: no - reason: wrap edge policy; 环绕 is the wrap word and matches wrap
- row: littera - section: types - current: 字符 - proposed: 字符 - confidence: high - equals-latin-key: no - reason: char type; 字符 is the character word and matches char
- row: regex - section: types - current: regex - proposed: regex - confidence: high - equals-latin-key: yes - reason: Latin kept by declaration: regex is the token itself and 正则 alone is not a settled type word; the en row also keeps the Latin
- row: json - section: types - current: json - proposed: json - confidence: high - equals-latin-key: yes - reason: Latin kept by declaration: JSON is the wire format name and has no Chinese spelling; the en row also keeps the Latin
- row: valor - section: types - current: 动态值 - proposed: 动态值 - confidence: high - equals-latin-key: no - reason: dynamic carrier; 动态值 says dynamic value and matches value
- row: instans - section: types - current: 时刻 - proposed: 时间戳 - confidence: medium - equals-latin-key: no - reason: 时刻 reads as a moment; the row is a timestamp value, whose Chinese word is 时间戳
- row: objectum - section: types - current: 对象 - proposed: 对象 - confidence: high - equals-latin-key: no - reason: object type; 对象 is the standard word and matches object
- row: quidlibet - section: types - current: 任意 - proposed: 任意 - confidence: high - equals-latin-key: no - reason: unrestricted escape type; 任意 is the any word and matches any
- row: lista - section: types - current: 列表 - proposed: 列表 - confidence: high - equals-latin-key: no - reason: ordered sequence; 列表 is the list word and matches list
- row: tabula - section: types - current: 映射 - proposed: 映射 - confidence: high - equals-latin-key: no - reason: key-value map; 映射 is the mapping word and matches map
- row: tabulae - section: types - current: 账本 - proposed: 账本 - confidence: low - equals-latin-key: no - reason: 账本 reads as an account book, which fits the insertion-ordered ledger metaphor, but the row is flagged provisional and no reader has validated it
- row: copia - section: types - current: 集合 - proposed: 集合 - confidence: high - equals-latin-key: no - reason: unique-element collection; 集合 is the set word and matches set
- row: promissum - section: types - current: 期约 - proposed: 承诺 - confidence: medium - equals-latin-key: no - reason: 期约 is a rare agreement word; the async result type is 承诺, which is the settled Chinese promise term
- row: cursor - section: types - current: 游标 - proposed: 迭代器 - confidence: medium - equals-latin-key: no - reason: 游标 names a cursor; the type row is the iterator, whose Chinese word is 迭代器, so it must not swap with the cursor annotation row
- row: queue - section: types - current: 队列 - proposed: 队列 - confidence: high - equals-latin-key: no - reason: queue type; 队列 is the standard word and matches queue
- row: stack - section: types - current: 栈 - proposed: 栈 - confidence: high - equals-latin-key: no - reason: stack type; 栈 is the standard word and matches stack
- row: tensor - section: types - current: 张量 - proposed: 张量 - confidence: high - equals-latin-key: no - reason: tensor type; 张量 is the standard word and matches tensor
- row: vector - section: types - current: 向量 - proposed: 向量 - confidence: high - equals-latin-key: no - reason: vector type; 向量 is the standard word and matches vector
- row: matrix - section: types - current: 矩阵 - proposed: 矩阵 - confidence: high - equals-latin-key: no - reason: matrix type; 矩阵 is the standard word and matches matrix
- row: sparsa - section: types - current: 稀疏 - proposed: 稀疏 - confidence: high - equals-latin-key: no - reason: sparse type; 稀疏 is the sparse word and matches sparse
- row: atomic - section: types - current: 原子 - proposed: 原子 - confidence: high - equals-latin-key: no - reason: atomic type; 原子 is the atomic word and matches atomic
- row: intervallum - section: types - current: 区间 - proposed: 区间 - confidence: high - equals-latin-key: no - reason: range type; 区间 is the interval word and matches range
- row: filtrum - section: types - current: 过滤器 - proposed: 过滤器 - confidence: high - equals-latin-key: no - reason: filter view; 过滤器 is the filter noun and matches filter
- row: series - section: types - current: 序列 - proposed: 序列 - confidence: high - equals-latin-key: no - reason: series view; 序列 is the series word and matches series
- row: census - section: types - current: 普查 - proposed: 普查 - confidence: high - equals-latin-key: no - reason: census view; 普查 is the census word and matches census
- row: ratio - section: types - current: ratio - proposed: 记录 - confidence: medium - equals-latin-key: no - reason: ratio is kept Latin but the en row is record; the census view names a record, whose Chinese word is 记录
- row: sermo - section: types - current: 通道 - proposed: 通道 - confidence: high - equals-latin-key: no - reason: channel type; 通道 is the channel word and matches channel
- row: scrinium - section: types - current: 帧 - proposed: 帧 - confidence: high - equals-latin-key: no - reason: frame type; 帧 is the frame word and matches frame
- row: trapped - section: types - current: 被困 - proposed: 被困 - confidence: high - equals-latin-key: no - reason: trapped state type; 被困 is the trapped word and matches trapped
- row: meus - section: types - current: 发送 - proposed: 发送 - confidence: high - equals-latin-key: no - reason: send view method; 发送 is the send verb and matches send
- row: tuus - section: types - current: 接收 - proposed: 接收 - confidence: high - equals-latin-key: no - reason: recv view method; 接收 is the receive verb and matches recv

## Summary

206 rows reviewed (159 keywords, 47 types): 169 keep their word, 37 are
proposed for change, and `tabulae` is the single low-confidence row. The
recurring defect is the same in every medium/high finding: the word translates
the Latin etymology or the English surface instead of the concept
(`praefixum` 前缀, `errata` 勘误, `fractus` 小数), or it is the wrong part of
speech for the clause position (`iacit` 可抛, `requirit` 需求).
The `modulus`/`cursor` keyword-type pairs are the only rows where a correction
must land in both sections together.

No pack file is edited; the changes above are proposals for a pack-owning unit
with a Simplified-Chinese reader to validate.
