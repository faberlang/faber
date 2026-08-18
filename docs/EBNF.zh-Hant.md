# Faber 語言規格

> **Reader-locale EBNF (Traditional Chinese).** Latin/source-of-truth grammar remains [`EBNF.md`](EBNF.md).
> This file is the Traditional Chinese reader surface of that grammar (keywords, commentary, examples).
> Pack keyword/type spellings are extracted from the glossary appendix at the end.
> Glyphs (`← → ∴ ≡ ∪ ⇥` …) never localize; `ergo` localizes, `∴` is clausura-only.


Faber 原始碼是由驅動程式在詞法分析前處理的純文字。選用的 TOML 前置資料不是 token grammar 的一部分。本文是 Faber 語言的正式文法與規格說明；可執行的參考程式位於公開的 `../examples/corpus/`，工具可從磁碟載入參考套件。

---

## 程式結構

Faber 檔案可選擇以 `+++` TOML 前置資料開始，之後接著程式本體。前置資料只由編譯器驅動程式解析，不會被當作 Faber 陳述式解析。

```ebnf
fabFile       := frontmatter? program
frontmatter   := '+++' NEWLINE tomlBody NEWLINE '+++' NEWLINE?
program       := statement*
statement     := importDecl | varDecl | funcDecl | genusDecl | implendumDecl
               | typeAliasDecl | enumDecl | discretioDecl
               | ifStmt | whileStmt | iteraStmt
               | eligeStmt | discerneStmt | guardStmt | curaStmt | facBlockStmt
               | returnStmt | breakStmt | continueStmt | noopStmt | throwStmt
               | assertStmt | requiritStmt | outputStmt | adStmt | incipitStmt
               | incipietStmt | extractStmt
               | probandumDecl | probaStmt | blockStmt | incDecStmt | exprStmt
blockStmt     := '{' statement* '}'
```

### 檔案前置資料（`+++`）

前置資料若存在，必須在第 1 行以完全相同的 `+++` 開始。後續去除前後空白後仍完全等於 `+++` 的行會結束區塊。結束分隔符後的位元組就是 Faber `program`。空白主體是合法的空程式。

前置資料是通用 TOML 文件。作者可附加任意 metadata key；工具會透過 accessor 讀取 `group`、`sectio` 和 `[probanda]` 等已知 key。套件模式仍以 `faber.toml` 的 `[package]`、`[paths]` 和 `[build]` 為準；衝突值會被拒絕。

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

入口 {}
```

行首 `§` 檔案指令已移除。檔案 metadata 請放在 `+++` 前置資料中。引號字串內的 `§` 仍是字串範本洞（見「呼叫與成員存取」）。

---

## 宣告

### 變數

```ebnf
varDecl      := ('定值' | '變值') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
sitDecl      := '設為' IDENTIFIER ('←' expression)?
arrayDestruct := ('定值' | '變值') arrayPattern '←' expression
objectDestruct := ('定值' | '變值') objectPattern '←' expression
```

- `定值` 建立不可變 binding（只能寫入一次）：可以不帶初始值宣告，之後恰好指定一次，指定後即凍結。`變值` 建立可重新指定的 binding，類似 `let`。
- 初始化值能決定型別時，型別標註可使用 `_`：`定值 _ name ← value`。
- `設為 name ← value` 是 `定值 _ name ← value` 的語法糖（推導型別的不可變區域量）。
- `設為 name`（沒有初始值）是 `定值 _ name` 的語法糖，表示延後初始化的推導型別不可變量；任何讀取前都必須恰好指定一次。
- 延後初始化：`定值 整數 x` 或 `設為 x` 宣告未初始化的不可變槽位。任何讀取前都必須恰好指定一次；第二次指定會被拒絕。確定性指定分析（語意階段 3a）會強制執行這項規則。

### 函式

```ebnf
funcDecl     := '函式' IDENTIFIER genericParams? '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam  := IDENTIFIER | '尺寸' IDENTIFIER
typeArgs      := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := ('從' | '傳入' | '取自')? '其餘'? typeAnnotation IDENTIFIER '可選'? ('作為' IDENTIFIER)? ('或取' expression)?
funcModifier := '引數' IDENTIFIER | '管理' IDENTIFIER ('作為' IDENTIFIER)? | '錯誤' IDENTIFIER | '出口' (IDENTIFIER | NUMBER) | '不變' | '拋出' | '選項' IDENTIFIER
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := '則'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := '執行' blockStmt catchClause?
legacyClausuraExpr := '閉包' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

- `→` 宣告正常成功型別。具有函式主體但沒有 `→` 的函式是效果限定（`空值`），且不得包含 `傳回`。語句主體閉包（`執行 { ... }` 或舊式區塊主體）若要使用 `傳回`，也必須明確寫出 `→ T`；運算式主體閉包可由運算式推導結果。
- `⇥` 宣告可復原的替代出口型別。它可出現在 `→ T` 後，也可單獨出現在效果限定的可失敗函式或閉包上。使用逃逸 `拋出` 的閉包必須宣告自己的 `⇥ E`，不能繼承外層函式的錯誤通道。
- 在宣告 `⇥` 的函式內，可失敗呼叫（`→ T ⇥ E`）會直接傳播到函式的替代出口，不需要 `執行`/`捕捉` 包裹；它會降級為 Rust `?`。閉包仍必須宣告自己的 `⇥`，因為外層函式的錯誤通道不會跨越閉包邊界。
- `執行 { ... } 捕捉 err { ... }` 是標準的一次性區域可復原錯誤邊界。
- 參數前綴：`從`（讀取）、`傳入`（變更）、`取自`（消費）。
- 名稱後標記：`可選`（自願／可選提供）。`其餘` 標記剩餘參數。`管理 NAME ('作為' LOCAL)` 宣告配置器需求；`LOCAL` 是函式主體內的別名。

`⇥` 宣告錯誤通道型別。它可出現在 `→ T` 之後，也可獨立出現在 effect-only 的可失敗函式或閉包上。使用逃逸 `拋出` 的閉包必須宣告自己的 `⇥ E`，不能繼承外層函式的錯誤通道。`執行 { ... } 捕捉 err { ... }` 是單次區域復原錯誤邊界。`則` 只用於單一語句主體；`∴` 只用於 compact clausura，兩者不是別名。

### 類別

```ebnf
genusDecl    := '抽象'?'類型' IDENTIFIER typeParams? ('子' IDENTIFIER)? ('實作' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := '靜態'? '綁定'? typeAnnotation IDENTIFIER '可選'? ('=' expression)?
methodDecl   := '函式' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
```

### 註解

```ebnf
annotation            := '@' annotationName annotationBody?
annotationName        := IDENTIFIER | '公開' | '保護' | '私有' | '未來' | '游標'
                        | '標籤' | '僅限' | '略過' | '測量'
annotationBody        := bracedAnnotation | annotationArgs
bracedAnnotation      := '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)* ','?
annotationField       := IDENTIFIER '=' (expression | typeAnnotation)
cliProgramAnnotation := '@' 'cli' STRING
imperiumAnnotation := '@' 'imperium' STRING
optioAnnotation    := '@' 'optio' IDENTIFIER optioModifier*
optioModifier      := '短' STRING | '長' STRING | '型別' typeAnnotation
                    | '說明' STRING | '全域' | '或取' expression
operandusAnnotation := '@' '位置引數' ('其餘')? typeAnnotation IDENTIFIER operandusModifier*
operandusModifier  := '說明' STRING | '全域' | '或取' expression
annotationArgs     := (STRING | IDENTIFIER | expression)+
annotatioMarker    := '@' 'annotatio' ( '{' annotatioFieldList? '}' )?
annotatioFieldList := annotatioField (',' annotatioField)* ','?
annotatioField     := 'target' '=' annotatioTarget
annotatioTarget    := '函式' | STRING | IDENTIFIER
contractApplication := '@' IDENTIFIER ( '{' annotationFieldList? '}' )?
jsonGenusAnnotation := '@' 'json'
jsonFieldAnnotation := '@' 'json' '{' '名稱' '=' STRING '}'
```

`@ annotatio` 將頂層 `genus` 標記為編譯期註解契約。普通 `genus` 不是註解 schema。應用程式使用 `@ ContractName { field = constant }`，並透過本地宣告或匯入的檔案介面匯出解析。解析後的應用會降級為 `HirAnnotation`，其中含有 `contract_id: Some(DefId)` 與常數欄位值。v1 的附加目標僅限 `functio`；載荷純量型別為 `textus`、`numerus`、`fractus` 和 `bivalens`，可透過 `sponte` 或 `T ∪ nihil` 表示可選。編譯器不提供自有的 `@ web`、controller 或 route 族。

**JSON genus：**`@ json` 套用於 `genus` 時，是編譯器擁有的資料模型契約，不是通用註解 schema。欄位必須是 JSON 安全型別：`textus`、`ascii`、`numerus`、`fractus`、`bivalens`、`instans`、`nihil`、`lista<T>`、`tabula<textus, T>`、可空的 `T ∪ nihil`，或另一個 `@ json genus`。欄位中 `@ json { nomen = "wire_name" }` 會變更 `value ↦ valor`、`value ↦ json` 與 `json ↦ Genus` 輸出的物件鍵。JSON 文字仍是 Norma 的線上操作，例如 `json.pange(value ↦ json)`。

- `@ radix` 保留給編譯器擁有的 metadata。歷史上的詞幹意義已退役；形態學仍是來源命名規範，不是編譯器產生的變位。接受的形式是套用在頂層函式上的 `@ radix lane "air"`、`"mir"` 或 `"hir-direct"`，用於明確的編譯器 lane 路由；不支援的 lane/target 組合會以診斷拒絕，不會靜默忽略。
- `@ verte` 定義程式碼生成轉換（方法名稱或範本）。
- `@ nondum [TARGET] ["REASON"]` 表示宣告存在於介面中，但目前目標不可用。
- `@ cli "NAME"` 將 `incipit` 入口標記為 CLI 程式。
- `@ imperium "NAME"` 將函式標記為 CLI 命令入口。
- `@ optio NAME ...` 定義 CLI 選項；布林旗標使用 `型別 布林`。
- `@ operandus [ceteri] TYPE NAME ...` 定義 CLI 位置引數。
- `@ futura` 標記非同步函式；`@ cursor` 標記產生器。
- `@ publica` 標記匯出、`@ interna` 標記套件內部、`@ privata` 為明確的模組私有標記；未標記的頂層宣告預設為模組私有，混用不同可見性層級會觸發 `SEM019`。
- `@ protecta` 保留並會以語意診斷拒絕；它不代表套件、子類別或同檔案可見性。

- `sub` = 延伸；`implet` = 實作。
- `generis` = 靜態；`nexum` = 綁定或屬性。

### 介面

```ebnf
implendumDecl   := '待實作介面' IDENTIFIER typeParams? '{' implendumMethod* '}'
implendumMethod := annotation* '函式' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause?
```

`待實作介面` 是只含簽章的方法契約，供 `實作` 使用。匯入 namespace 以 `.fab` 檔案為邊界；匯出的宣告位於檔案頂層。

### 型別別名

```ebnf
typeAliasDecl := '型別' IDENTIFIER genericParams? '=' typeAnnotation
```

### 列舉與標籤聯集

```ebnf
enumDecl   := '列舉' IDENTIFIER '{' enumMember (',' enumMember)* ','? '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
discretioDecl := '分支聯集' IDENTIFIER typeParams? '{' variant (',' variant)* ','? '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER (',' typeAnnotation IDENTIFIER)*)?
```

### 識別符命名

混合大小寫且首字母小寫的名稱在語法上接受，但不是 Faber 語言、標準函式庫、主機路由或編譯器 intrinsic API 的偏好。優先使用單一字詞；單一字詞不足以承載語意時，才在少數情況使用 snake_case。若兩種形式都不合適，除非方法不可或缺，否則它可能不屬於核心表面。標準函式庫的編解碼在各模組使用機械動詞三件組 `pange` / `solve` / `tempta`；公開文字函式庫是 `norma:chorda`。

### 匯入

```ebnf
importDecl     := importRecord | importSugar
importRecord   := '匯入' '{' importFieldList? '}'
importFieldList := importField (',' importField)* ','?
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := '取自' '=' STRING
importVisibilityField := '可見性' '=' visibility
importNameField := '名稱' '=' IDENTIFIER
importAliasField := '作為' '=' IDENTIFIER
importWildcardField := '全部' '=' IDENTIFIER
importSugar    := '匯入' '取自' STRING visibility? (namedImport | wildcardImport)?
visibility    := '公開'
namedImport   := IDENTIFIER ('作為' IDENTIFIER)?
wildcardImport := '*' '作為' IDENTIFIER
```

```fab
匯入取自 "hono" Hono
匯入取自 "norma:chorda"
匯入 { 取自 = "norma:json/solve", 作為 = solve_mod }
匯入取自 "./types" 公開 User
```

匯入的 `私有` 標記已移除（VM-U3）：無標記的匯入預設不 re-export，`公開` 是 re-export 標記。未指定 binding 時，若最後一段路徑是合法且不衝突的識別符，就使用該名稱。

---

## 型別

```ebnf
typeAnnotation := ('從' | '傳入')? baseType ( '∪' typeAnnotation )*
baseType       := holeType | functionType | qualifiedType typeParams? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
typeParams     := genericParams
```

陣列寫成 `列表<T>`；不接受 postfix `T[]`。`從` 與 `傳入` 是型別前綴的所有權標記。`T ∪ 無` 是標準 nullable 型別形式。`可選` 是宣告標記，不是型別前綴。限定型別路徑必須透過匯入 namespace 解析。

```fab
函式 篩選((T) → 布林 預測) → 列表<T>
函式 組合((A) → B f, (B) → C g) → (A) → C
函式 套用((整數) → 整數 ⇥ 文字 op, 整數 n) → 整數 ⇥ 文字
```

### 原始型別

| Faber | 意義 |
|---|---|
| `文字` | Unicode 字串 |
| `ascii` | 僅 ASCII 的字串 |
| `forma` | 擷取的範本與參數 |
| `整數` | 整數，預設 `i64` |
| `模數<W>` | 無號模數字，算術以 `2^W` 取模 |
| `小數` | 浮點數，預設 `f64` |
| `布林` | 布林值 |
| `無` | null |
| `空值` | void |
| `永不` | never |
| `未知` | unknown |
| `位元組` | bytes |

大小型別使用寬度標記。`整數` / `小數` 可省略寬度而分別代表 `i64` / `f64`。`模數<W>` 是獨立語意家族，不會與 `整數<W>` 隱式混用。

### 泛型集合

| Faber | 意義 |
|---|---|
| `列表<T>` | 陣列 |
| `表格<K,V>` | map |
| `副本<T>` | set |
| `承諾<T>` | promise |
| `游標<T>` | iterator |
| `tensor<T, Figura>` | 具有靜態形狀的密集同質緩衝區 |
| `vector<T, N>` | 固定寬度的數值向量 |
| `matrix<T, [R, C]>` | 恰有兩個靜態維度的數值矩陣 |
| `atomic<T>` | 儲存敏感的 atomic cell |
| `sparsa<T, Figura>` | 具有靜態形狀的稀疏同質緩衝區 |

`Figura` := `_`、自然數、識別符或 `[` 形狀列表 `]`；空 `[]` 表示 rank-0。裸 `tensor<T>` 不完整：rank-0 使用 `tensor<T, []>`，推導形狀使用 `tensor<T, _>`。

`空值` 用於 `tensor<T, []>` 會產生 rank-0 tensor（含一個預設初始化的元素槽）。`空值` 用於任意形狀的 `sparsa<T, Figura>` 會產生沒有儲存項目的全零稀疏 tensor。`matrix<T, Figura>` 必須恰好有兩個維度；裸 `matrix<T>` 以及一維或三維形狀都會被拒絕。

`atomic<T>` 在 v1 只接受 `i32` 或 `u32`；atomic cell 不可與元素型別互換，必須使用 `load`、`store`、`exchange` 和 `compare_exchange` 接收者方法。多維 tensor 使用 `crea`、`structa` 或 `↦` 建構。`Type(...)` 不是建構形式：`vector<f32, 4>(...)`、`matrix<f32, [2, 2]>(...)`、`tensor<f32, [2, 2]>(...)` 以及 `numerus("42")` 等純量形式都會被拒絕。請使用 `value ↦ Type`、具名函式庫建構子或 `Genus { field = value }` 記錄。

Tensor 的索引／形狀 intrinsic 槽（`accipe`、`ponde`、`forma`、`crea`、`structa`）在呼叫點接受符合 `lista<numerus>`／`&[i64]` 執行期邊界的整數列表，例如 GPU thread id 可使用 `lista<u32>`，但不可使用 `lista<u64>`。這是限定於這些槽位的結構性例外，不會放寬有號與無號數值格。

值聯集使用 inline `T ∪ U`（可空形式：`T ∪ 無`）；標籤聯集使用 `分支聯集`。`副本.unio()` 是集合方法，不是型別建構子。`vacua` 是上下文相關的空集合標記，必須搭配明確集合型別。

### 型別糖

型別糖只在型別位置有效，與長格式語意相同。寬度標記為 `i8`、`i16`、`i32`、`i64`、`u8`、`u16`、`u32`、`u64`、`f16`、`f32`、`f64`。`lf32` 等表示列表，`tf32[2, 3]` 表示 tensor，`sf32[2, 3]` 表示 sparsa，`vf32[4]` 表示 vector，`mf32[4, 4]` 表示 matrix。matrix 必須指定兩個維度。`模數<W>` 沒有糖，請使用完整形式。

---

## 控制流程

### 條件式

```ebnf
ifStmt     := '若' expression arm ('否則若' ifStmt | elseClause)?
elseClause := '否則' elseArm
arm        := (blockStmt | stmtBodyJoint statement) catchClause?
elseArm    := (blockStmt | stmtBodyJoint statement) catchClause?
```

`則` 用於單一語句主體，例如 `則 傳回`、`則 拋出`、`則 崩潰`、`則 靜默`。`靜默` 是明確 no-op。

### 迴圈

```ebnf
whileStmt  := '當' expression (blockStmt | stmtBodyJoint statement) catchClause?
iteraStmt  := '遍歷' (('取自' | '從') expression | '自' expression) ('定值' | '變值') IDENTIFIER (blockStmt | stmtBodyJoint statement) catchClause?
```

`遍歷 取自 ... 定值` 是 for-of；`遍歷 從 ... 定值` 是 for-in；`遍歷 自 range 定值 i` 是範圍迭代。`每` 屬於範圍運算式。

### Switch / Match

```ebnf
eligeStmt    := '選擇' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase    := '分支' expression (blockStmt | stmtBodyJoint statement)
defaultCase  := '預設' (blockStmt | stmtBodyJoint statement)
discerneStmt := '比對' '全部'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase  := '分支' patterns (blockStmt | stmtBodyJoint statement)
patterns     := pattern ((',' | '且') pattern)*
pattern      := '_' | literal | (IDENTIFIER patternBind?)
patternBind  := ('作為' IDENTIFIER) | (('定值' | '變值') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('作為' IDENTIFIER)?
```

### 守衛與資源

```ebnf
guardStmt   := '守衛' '{' guardClause+ '}'
guardClause := '若' expression (blockStmt | stmtBodyJoint statement)
curaStmt    := '資源' STRING ('定值' | '變值') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### 解構擷取與控制轉移

```ebnf
extractStmt   := '取自' expression ('定值' | '變值') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField  := IDENTIFIER ('作為' IDENTIFIER)?
restField     := '其餘' IDENTIFIER
returnStmt   := '傳回' expression?
breakStmt    := '中斷'
continueStmt := '繼續'
noopStmt     := '靜默'
```

---

## 錯誤處理

```ebnf
throwStmt   := ('拋出' | '崩潰') expression ['若' expression]
catchClause := '捕捉' IDENTIFIER blockStmt
assertStmt  := '斷言' expression ('崩潰' expression)?
requiritStmt := '需要' expression '拋出' expression
```

`捕捉` 可附加於結構化語句與條件分支，不可附加於任意裸區塊。`執行 { ... } 捕捉 err { ... }` 是標準的區域復原錯誤邊界。`嘗試` 與 `最後` 是舊語法並會被遷移診斷拒絕。`拋出` 可復原；`崩潰` 是致命錯誤。`拋出 value 若 condition` 會在解析時展開為條件式。

---

## 運算式

### 運算子（由低至高）

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
ternary    := or (('?' expression ':' | '如此' expression '否則') ternary)?
or         := and (('或') and)*
and        := equality (('且') equality)*
equality   := comparison (('≡' | '≠' | '≈' | '≉' | '是' | '非' '是') comparison)*
comparison := bitwiseOr (('<' | '>' | '≤' | '≥' | '內含' | '之間') bitwiseOr)*
bitwiseOr  := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift      := range (('⇐' | '⇒') range)*
range      := additive (('‥' | '…' | '之前' | '直到') additive ('每' additive)?)?
additive   := multiplicative (('+' | '-') multiplicative)*
multiplicative := coalesce (('*' | '/' | '%') coalesce)*
coalesce   := unary ('或取' velRhs)*
velRhs     := unary (('‥' | '…' | '之前' | '直到') unary ('每' unary)?)?
unary      := ('-' | '¬' | '非' | '讓出' | '虛構') unary | cast
cast       := call ('∷' typeAnnotation | conversio)*
conversio  := '↦' typeAnnotation typeParams? inlineRecovery?
inlineRecovery := '⇥' unary
```

`↤` 是定向轉換賦值：先求右側值，透過 `↦` 路徑轉換為左側位置的靜態型別，再賦值。`⇥` 內聯恢復僅在 `↤` 上合法，不允許接在 `←` 之後。

`∴` 不在上述運算子文法中，永遠只表示 clausura joint。`∷` 是編譯期靜態型別標註；`↦` 是會執行實際解析或轉換的執行期運算子。`⇥` 可在轉換目標後提供同型別的復原值。`或取` 是 nullable elimination，不是邏輯 OR。

### 呼叫與成員存取

```ebnf
call          := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix    := typeArgs? '(' argumentList ')'
memberSuffix  := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList  := (argument (',' argument)*)?
argument      := '展開'? expression
```

### 字串與範本字面值

Faber 使用分隔符語意；每種引號形式都代表不同的來源形狀。

| 形式 | 型別 | 用途 |
|---|---|---|
| `'...'` | `ascii` | 固定機器 token |
| `"..."` | `文字` | 短 Unicode 行字串 |
| `«...»` | `文字` | 區塊或多行 Unicode 字串 |
| `` `...` `` | `forma` | 擷取範本 |
| `{ ... }` | `json` | 編譯期 JSON 文件 |
| `\|...\|` | `位元組` | 編譯期十六進位位元組 |
| `"..." ↦ regex` | `regex` | 從文字轉換為編譯後模式 |
| `[ ... ]` | `列表<T>` | Faber 列表 |

Unicode 字串範本中的 `§` 是洞。`文字` 的格式範本呼叫會產生 `scriptum`；`forma` 會擷取文字與參數，不會立即渲染。

```fab
定值 _ 標籤 ← «inline»
定值 _ 查詢 ← `select * from accounts where id = §`(accountId)
定值 _ 簽章 ← |de ad be ef|
"status: § (§)"(sample_status(), "ok")
"hello world"[0‥5]
```

對 `文字` 而言，方括號索引以 Unicode scalar 為單位。文字切片接受完整 range 形式，包括 `每`。

對 `列表<T>` 而言，方括號索引是單一元素存取；索引必須是單一整數，不接受 range slice（要複製範圍請使用 `sectio(start, end)`）。列表方括號存取是**一般存取**，不是 nullable：回傳裸元素 `T`，越界時觸發陷阱。若需要 nullable 列表存取，使用 `xs.accipe(i) → T ∪ 無` 搭配 `或取`。

對 `tensor<T, Figura>` 而言，方括號索引是 tensor intrinsic 表面的語法糖：

```fab
vector[id]        # vector.accipe([id])
vector[id] ← v    # vector.ponde([id], v)
grid[[r, c]]      # grid.accipe([r, c])
grid[[r, c]] ← v  # grid.ponde([r, c], v)
```

讀取回傳 `T ∪ 無`，與 `accipe` 一致；參與算術前請先使用 `或取` 或其他一般選項處理。rank-1 tensor 接受符合 tensor `i64` 執行期邊界的純量整數索引（拒絕 `u64`）。rank-N tensor 使用列表形狀的索引，例如 `[[r, c]]` 或繫結的 `lista<integer>` 值。`grid[r, c]` 不是語法；`memberSuffix` 的方括號內仍只有一個 `expression`。

`位元組` 是位元組緩衝原語，不是陣列，因此不接受方括號索引（讀取或寫入）。位元組存取使用 `accipe`、`appende` 與 `longitudo` 方法；`accipe` 越界時回傳 nullable 值。對高密度位元組索引，內部可使用 `lista<numerus<u8>>`，並在邊界保留 `位元組`。

### 基本運算式

```ebnf
primary := IDENTIFIER | NUMBER | STRING | ASCII_STRING | BACKTICK_STRING
         | '自身' | '真' | '假' | '無'
         | 'vacua' | arrayLiteral | jsonLiteral | typedConstructor
         | adExpr
         | '(' expression ')'
adExpr    := '對' asciiLiteral adOpener?
arrayLiteral := '[' argumentList? ']'
jsonLiteral := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonMember  := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('展開' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonArray  := '[' (jsonValue (',' jsonValue)* ','?)? ']'
jsonString := STRING
jsonNumber := NUMBER
```

裸 `{ ... }` 是 JSON 文件。key 必須是帶引號的 JSON 字串，值只能是 JSON 常數。重複 key 會報錯。類型或變體建構使用 `Type { field = value }`，`=` 的 Faber grammar 保持不變。

### 特殊運算式

```ebnf
fingeExpr     := '虛構' IDENTIFIER ('{' fieldList '}')? ('∷' IDENTIFIER)?
praefixumExpr := '前綴' (blockStmt | '(' expression ')')
formatStringExpr := STRING '(' argumentList ')'
formaTemplateExpr := BACKTICK_STRING '(' argumentList ')'
scriptumExpr  := '格式文字' '(' STRING (',' expression)* ')'
legeExpr      := '讀取' '行'?
regexFromText := (STRING | ASCII_STRING) '↦' 'regex'
```

---

## 模式

```ebnf
objectPattern  := '{' patternProperty (',' patternProperty)* '}'
patternProperty := '其餘'? IDENTIFIER ('作為' IDENTIFIER)?
arrayPattern   := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | '其餘'? IDENTIFIER
```

---

## 診斷

```ebnf
outputStmt := ('註記' | '檢視' | '警告' | '寫出') expression (',' expression)*
```

`註記` 是中性的診斷 note，`檢視` 用於 debug 或 inspect，`警告` 用於 warn，`寫出` 是診斷通道拼寫。真正的輸出請使用目前標準函式庫方法。

### 註解

Faber 只接受**行註解**：`#` 延伸至行尾。`#` 必須是邏輯行上的第一個非空白 token；只略過前導 ASCII 空格與 tab，其他 Unicode 空白分隔符不會由 lexer 略過。同一行若在其他 token 後出現 `#`，會產生詞法錯誤：`# comments must start a line; move this comment above the code`。

合法的行首註解會以前置 trivia 附加到下一個陳述式或宣告（見 comment-preservation）。字串、`ascii` 字面值、`forma` 範本與其他分隔字面值內的 `#` 都不是註解。

---

## 入口點

```ebnf
incipitStmt  := '入口' blockStmt
incipietStmt := '非同步入口' blockStmt
```

`入口` 是同步入口；`非同步入口` 是非同步入口。

---

## 測試

```ebnf
probandumDecl := '測試規格' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt     := '測試' STRING probaModifier* blockStmt
probaModifier := '略過' STRING | '未來' STRING | '僅限' | '標籤' STRING
              | '時限' NUMBER | '測量' | '重複' NUMBER | '脆弱' NUMBER
              | '需要' STRING | '僅限於' STRING
praeparaBlock := ('準備' | '準備非同步' | '後置準備' | '後置準備非同步') '全部'? blockStmt
```

---

## CLI 框架

```ebnf
cliDecl       := annotation* (incipitStmt | funcDecl)
cliAnnotation := cliProgramAnnotation | imperiumAnnotation | optioAnnotation | operandusAnnotation
```

Faber 支援建立 CLI 應用程式，並自動解析引數與產生 help。

```fab
@ cli "faber"
@ optio verbose 長 "verbose" 型別 bivalens
入口 引數 args {
    # CLI 框架會自動解析引數
}
```

```fab
@ imperium "deploy"
@ optio target 短 "t" 長 "target" 型別 文字 說明 "部署目標"
@ optio verbose 短 "v" 長 "verbose" 型別 布林 說明 "啟用詳細輸出"
@ 位置引數 文字 file 說明 "要部署的檔案"
函式 deploy() 引數 args {
    # 引數會自動解析並傳入
}
```

---

## 能力呼叫

expression-form `對` 是唯一支援的 `ad` 表面。舊式具型別 `ad` 和區塊串流形式已在解析時拒絕。

```ebnf
adExpr        := '對' asciiLiteral adOpener?
adOpener      := '(' expression ')'
conversioExpr := expr '↦' typeAnnotation
```

- route 必須是 `asciiLiteral`，例如 `'solum:lege'`，不能使用雙引號 `STRING`。
- opener 是可選的單一 `expression`，會作為 Request `data` 的 `valor`。
- **運算式形式的 `對`** 不帶區塊；求值結果是 `sermo` 對話 handle。可使用 postfix `↦ T` 物化、指定給 `sermo`，或開啟即時方向 view：`s.meus<T>()`（輸出 `da`／`fini`）與 `s.tuus<T>()`（輸入 `accipe`／`cursor`／`exhauri`／`fini`）。輸入內容 frame 請使用 `s.tuus<T>().cursor()` 迭代，不要直接寫 `遍歷 取自 s.tuus<T>()`。
- 已移除且會產生解析錯誤的表面：舊式具型別 `對 "route"`、區塊 `meus`／`tuus` 分支，以及陳述式層級的 `發出`。
- 型別包括編譯器擁有的 `scrinium`、`status`，以及不透明的 `sermo` 對話 handle。
- `sermo ↦ T` 會使用針對 `T` 的型別導向 collector，把輸入 frame 物化為單一 `T` 值。

詳見 [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md)。

---

## 集合操作

舊的 `ab` 集合 pipeline DSL 已淘汰。篩選、切片與聚合應使用 `文字`、`列表`、`表格`、`副本` 方法與閉包。`prima`、`ultima` 是普通方法名稱；`ubi` 不是有效集合語法。`取自` 用於迭代與匯入。

---

## Fac 區塊

```ebnf
facBlockStmt := '執行' blockStmt catchClause? ('當' expression)?
```

`執行 { ... }` 執行一次作用域區塊。`執行 { ... } 捕捉 err { ... }` 是區域復原錯誤邊界。`執行 { ... } 當 condition` 是 post-test loop；postfix `當` 只附加到 `執行`。

---

## 目標支援

目標支援不是文法的一部分。本文只定義語言；各編譯目標可降低的文法與 runtime policy 請參考 `EBNF_MATRIX.md` 與 target capability 文件。

---

## 關鍵字參考

文法中的關鍵字按宣告、控制流程、錯誤處理、非同步、端點、布林、物件、型別形狀、型別轉換、位元運算與診斷分類。`∴`、`∷`、`↦`、`⇥` 等 glyph 永遠不翻譯，也不應在 reader pack 中建立 glyph 別名。

| 分類 | 讀者表面 | 說明 |
|---|---|---|
| 宣告 | `分支聯集`、`定值`、`函式`、`類型`、`介面`、`型別` | 宣告資料、函式、型別與契約 |
| 控制流程 | `若` / `否則若` / `否則`、`當`、`遍歷`、`選擇` / `分支`、`比對` | 條件、迴圈、分派與模式比對 |
| 錯誤處理 | `捕捉`、`斷言`、`需要`、`拋出`、`崩潰`、`執行` | 區域錯誤邊界與控制轉移 |
| 非同步 | `@ 未來`、`@ 游標`、`讓出` | 非同步函式、產生器與上下文等待／讓出 |
| 端點 | `對`、`發出` | `對` 是能力呼叫運算式；`發出` 已退役 |
| 布林 | `真`、`假`、`或`、`且`、`非`、`是`、`或取` | 布林、比較與區域 nullable 消除 |
| 物件 | `自身`、`虛構` | self 與 variant 建構 |
| 形狀／轉換 | `∷`、`↦`、`⇥` | 靜態標註、執行期轉換與復原值 |
| 位元 | `∧`、`∨`、`⊻`、`¬`、`⇐`、`⇒` | 位元 AND、OR、XOR、NOT 與移位 |
| 診斷 | `註記`、`檢視`、`警告`、`寫出` | 診斷通道，不等同於一般輸出 |

---

## 關鍵語法規則

1. 參數採型別優先：`函式 f(整數 n)`，不是名稱後置型別。
2. 宣告採型別優先：`定值 文字 name`，不是 `name: 文字`。
3. 迭代語句使用 `遍歷 取自/從 collection 定值/變值 item { }` 或 `遍歷 自 range 定值/變值 item { }`。
4. 條件可加括號，但慣用形式是不加括號。
5. 診斷關鍵字是語句，不是可呼叫值。

## Reader pack glossary (machine extract)

### Keywords
| Latin | Localized |
|---|---|
| discretio | 分支聯集 |
| fixum | 定值 |
| functio | 函式 |
| genus | 類型 |
| implendum | 待實作介面 |
| importa | 匯入 |
| modulus | 模數 |
| ordo | 列舉 |
| sit | 設為 |
| typus | 型別 |
| varia | 變值 |
| abstractus | 抽象 |
| ceteri | 其餘 |
| curata | 管理 |
| errata | 錯誤 |
| exitus | 出口 |
| generis | 靜態 |
| iacit | 可拋|
| immutata | 不變 |
| interna | 內部 |
| magnitudo | 尺寸 |
| nexum | 綁定 |
| optiones | 選項 |
| prae | 前置 |
| privata | 私有 |
| protecta | 保護 |
| publica | 公開 |
| sponte | 可選 |
| casu | 分支 |
| ceterum | 預設 |
| custodi | 守衛 |
| discerne | 比對 |
| dum | 當 |
| elige | 選擇 |
| ergo | 則 |
| fac | 執行 |
| itera | 遍歷 |
| secus | 否則 |
| si | 若 |
| sic | 如此 |
| sin | 否則若 |
| perge | 繼續 |
| redde | 傳回 |
| rumpe | 中斷 |
| tacet | 靜默 |
| adfirma | 斷言 |
| cape | 捕捉 |
| cede | 讓出 |
| iace | 拋出|
| mori | 崩潰 |
| clausura | 閉包 |
| falsum | 假 |
| nihil | 空|
| verum | 真 |
| aut | 或 |
| est | 是 |
| et | 且 |
| non | 非 |
| vel | 或取 |
| ego | 自身 |
| finge | 虛構 |
| implet | 實作 |
| sub | 子 |
| mone | 警告 |
| nota | 註記 |
| scribe | 寫出 |
| vide | 檢視 |
| argumenta | 引數 |
| cura | 資源 |
| incipiet | 非同步入口 |
| incipit | 入口 |
| ad | 端點|
| de | 從 |
| ex | 取自 |
| in | 傳入 |
| lege | 讀取 |
| lineam | 行 |
| omnia | 全部 |
| praefixum | 前綴 |
| scriptum | 格式文字 |
| sparge | 展開 |
| ut | 作為 |
| ante | 之前 |
| inter | 之間 |
| intra | 內含 |
| per | 每 |
| usque | 直到 |
| fragilis | 脆弱 |
| futurum | 未來 |
| metior | 測量 |
| omitte | 略過 |
| postpara | 後置準備 |
| postparabit | 後置準備非同步 |
| praepara | 準備 |
| praeparabit | 準備非同步 |
| proba | 測試 |
| probandum | 測試規格 |
| repete | 重複 |
| requirit | 需要 |
| solum | 僅限 |
| solum_in | 僅限於 |
| tag | 標籤 |
| temporis | 時限 |
| negativum | 負 |
| nonnihil | 非空 |
| nonnulla | 若干 |
| nulla | 可空|
| positivum | 正 |

### Types
| Latin | Localized |
|---|---|
| ascii | ascii |
| textus | 文字 |
| numerus | 整數 |
| modulus | 模數 |
| fractus | 小數 |
| bivalens | 布林 |
| nihil | 無 |
| vacuum | 空值 |
| numquam | 永不 |
| ignotum | 未知 |
| octeti | 位元組 |
| regex | 正規表示式 |
| json | JSON |
| valor | 值 |
| instans | 執行個體 |
| objectum | 物件 |
| quidlibet | 任意值 |
| lista | 列表 |
| tabula | 表格 |
| copia | 副本 |
| promissum | 承諾 |
| cursor | 游標 |

### Glossary changes vs existing pack
| Latin | Old pack | New (this EBNF) | Why |
|---|---|---|---|
| functio | 函式 | 函式 | 保留既有繁體用語。 |
| fixum | 常數 | 定值 | 與不可變 binding 的語意一致，避免與數學常數混淆。 |
| sin | 否則如果 | 否則若 | 臺灣技術文件較自然，且保持單一 token。 |
| secus | 否則 | 否則 | 保留既有用語。 |
| redde | 傳回 | 傳回 | 保留既有用語。 |
| nota | 顯示 | 註記 | 與診斷 note 的語意一致，不與一般輸出混淆。 |
| textus | 文字 | 文字 | 保留既有用語。 |
| numerus | 整數 | 整數 | 保留既有用語。 |
