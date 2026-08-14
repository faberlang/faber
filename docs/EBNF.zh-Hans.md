# Faber 语言规范

> **Reader-locale EBNF (Simplified Chinese).** Latin/source-of-truth grammar remains [`EBNF.md`](EBNF.md).
> This file is the Simplified Chinese reader surface of that grammar (keywords, commentary, examples).
> Pack keyword/type spellings are extracted from the glossary appendix at the end.
> Glyphs (`← → ∴ ≡ ∪ ⇥` …) never localize; `ergo` localizes, `∴` is clausura-only.



Faber 编程语言的形文法（grammar）。现行实现是根 Rust workspace：`crates/faber` 负责包/工程工具，`crates/radix` 负责编译流水线。

文档契约：本文件是 Faber 的权威语法与说明评注面。可运行的语言参考程序位于公开的同级目录 [`../examples/corpus/`](../examples/corpus/)，可带 `+++` frontmatter（`term`、`syntax`、`related` 等）；生成的清单是 [`../examples/corpus/index.toml`](../examples/corpus/index.toml)。`faber explain` 从磁盘加载 exempla 参考包。新的参考工作请优先使用语言语料库 + EBNF。

---

## 程序结构

Faber 源文件是纯文本，由驱动在词法分析前剥离。可选的 TOML frontmatter 不属于 token 语法。

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

### 文件 frontmatter（`+++`）

若存在，frontmatter 必须在第 1 行以恰好 `+++` 开头。后续某行在去除空白后恰好等于 `+++` 即结束该块。闭合分隔符之后的字节是 Faber `program`。空正文（仅空白）是合法的空程序。

Frontmatter 由编译器驱动按通用 TOML 文档解析，而非按 Faber 语句解析。作者可附加任意元数据键；工具通过访问器读取已知键，例如 `group`、`sectio` 与 `[probanda]`。`faber` 包工具消费这些包键。`[package]`、`[paths]` 与 `[build]` 的包权威仍属 `faber.toml`；包模式下会拒绝相互冲突的 frontmatter 值。

示例：

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

入口 {}
```

行首 `§` 文件指令已移除。请把文件元数据放进 `+++` frontmatter。在引号字符串内部，`§` 仍是字符串模板占位符（见下文**调用与成员访问**）。

---

## 声明

### 变量

```ebnf
varDecl      := ('常量' | '变量') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
sitDecl      := '设' IDENTIFIER ('←' expression)?
arrayDestruct := ('常量' | '变量') arrayPattern '←' expression
objectDestruct := ('常量' | '变量') objectPattern '←' expression
```

- `常量` = 不可变绑定（只写一次）：可以不带初值声明，之后赋值恰好一次，随即冻结。`变量` = 可变绑定（可重新赋值），类似 `let`。
- 当初值决定类型时，用 `_` 作为类型标注：`常量 _ name ← value`
- `设 name ← value` 是 `常量 _ name ← value` 的语法糖（推断型不可变局部量）
- `设 name`（无初值）是 `常量 _ name` 的语法糖——推断型延后不可变量。在任意读取之前必须赋值恰好一次。
- 延后初始化：`常量 整数 x` 或 `设 x` 声明一个未初始化的不可变槽位，在任意读取之前必须赋值恰好一次；第二次赋值会被拒绝。确定性赋值分析（语义阶段 3a）负责强制执行。

### 函数

```ebnf
funcDecl     := '函数' IDENTIFIER genericParams? '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam  := IDENTIFIER | '维度' IDENTIFIER
typeArgs      := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := ('借自' | '传入' | '取自')? '其余'? typeAnnotation IDENTIFIER '可选'? ('作为' IDENTIFIER)? ('兜底' expression)?
funcModifier := '参数' IDENTIFIER | '委派' IDENTIFIER ('作为' IDENTIFIER)? | '勘误' IDENTIFIER | '退出' (IDENTIFIER | NUMBER) | '不变' | '可抛' | '选项' IDENTIFIER
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := '则'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := '执行' blockStmt catchClause?
legacyClausuraExpr := '闭包' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

- 返回语法：`→` 声明正常成功类型。带函数体但没有 `→` 的函数是仅副作用型（`无值`），且不得包含 `返回`。语句体闭包（`执行 { ... }` 或旧式块体）若要使用 `返回`，也必须显式写 `→ T`；表达式体闭包可从表达式推断结果。
- 可恢复的备用出口语法：`⇥` 声明错误通道类型。它可出现在 `→ T` 之后，或单独出现在仅副作用且可失败的函数或闭包上。若闭包体使用了逃逸型 `抛错`，则必须声明自己的 `⇥ E`；它不能继承外层函数的错误通道。局部的 `执行 { ... } 捕获 err { ... }` 可以在没有外层 `⇥` 的情况下捕获 `抛错`。在声明了 `⇥` 的函数内部，可失败的函数调用（`→ T ⇥ E`）会传播到该函数的备用出口，无需 `执行`/`捕获` 包裹；这与裸 `↦` conversio 和 `抛错` 抛出本来的行为一致；该调用会降级为 Rust `?`。闭包若要传播可失败调用，仍必须声明自己的 `⇥`——外层函数的错误通道不会跨越闭包边界。
- 参数前缀：`借自`（读）、`传入`（改）、`取自`（消费）
- 名称后标记：`可选`（自愿/可选供给）
- `其余` 标记剩余参数
- `委派 NAME ('作为' LOCAL)?` 声明一个分配器要求；`LOCAL` 是函数体内的别名
- `则` 仅作为紧凑的**语句体**连接符（单语句的 `如果`/`当`/`情况`/… 分支体）。
- `∴` 仅作为紧凑的**闭包**连接符。两者并非别名。
- 紧凑闭包块体必须使用 `执行 { ... }`；闭包局部的 `执行` 体可附加 `捕获`，但不能使用后缀 `当`。

### 类

```ebnf
genusDecl    := '抽象'? '类' IDENTIFIER typeParams? ('继承' IDENTIFIER)? ('实现' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := '静态'? '属性'? typeAnnotation IDENTIFIER '可选'? ('=' expression)?
methodDecl   := '函数' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
```

### 注解

```ebnf
annotation            := '@' annotationName annotationBody?
annotationName        := IDENTIFIER | '公开' | '保护' | '私有' | 'futura' | 'cursor'
                        | '标签' | '仅' | '跳过' | '计量'
annotationBody        := bracedAnnotation | annotationArgs
bracedAnnotation      := '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)* ','?
annotationField       := IDENTIFIER '=' (expression | typeAnnotation)

cliProgramAnnotation := '@' cli STRING
imperiumAnnotation := '@' imperium STRING
optioAnnotation    := '@' optio IDENTIFIER optioModifier*
optioModifier      := 'brevis' STRING | 'longum' STRING | '类型' typeAnnotation
                    | 'descriptio' STRING | 'ubique' | '兜底' expression
operandusAnnotation := '@' operandus ('其余')? typeAnnotation IDENTIFIER operandusModifier*
operandusModifier  := 'descriptio' STRING | 'ubique' | '兜底' expression
annotationArgs     := (STRING | IDENTIFIER | expression)+

(* 注解契约——编译期元数据模式 *)
annotatioMarker     := '@' annotatio ( '{' annotatioFieldList? '}' )?
annotatioFieldList  := annotatioField (',' annotatioField)* ','?
annotatioField      := 'target' '=' annotatioTarget
annotatioTarget     := '函数' | STRING | IDENTIFIER
contractApplication := '@' IDENTIFIER ( '{' annotationFieldList? '}' )?

jsonGenusAnnotation := '@' json
jsonFieldAnnotation := '@' json '{' 'nomen' '=' STRING '}'
```

Radix 解析花括号注解记录（`@ futura { }`、`@ optio { binding = verbose, ... }`）以及既有的语法糖写法。语法糖形式与花括号形式会降级为同样的 `HirAnnotation` 记录，用于已提升的族。某些未提升的 token 族可能仍保留原始注解实参，待消费者迁移后再处理。

**注解契约：**`@ annotatio`（可选 `@ annotatio { target = 函数 }`）把一个顶层 `类` 标记为编译期注解契约。普通类不是注解模式。应用时使用 `@ ContractName { field = constant }`，并通过本地声明或导入的文件接口导出来解析。解析后的应用会降级为 `HirAnnotation`，其中 `contract_id: Some(DefId)`，字段为常量值。v1 的附着目标仅限 `函数`；载荷标量类型为 `文本`、`整数`、`小数` 与 `布尔`（可通过 `可选` 或 `T ∪ 空` 设为可选）。不存在编译器自有的 `@ web` / controller / route 族。

**JSON 类：**`genus` 上的 `@ json` 是编译器自有的数据模型契约，而非通用注解模式。字段必须 JSON 安全（`文本`、`窄字串`、`整数`、`小数`、`布尔`、`时刻`、`空`、`列表<T>`、`映射<文本, T>`、可空 `T ∪ 空`，或另一个 `@ json 类`）。字段元数据 `@ json { nomen = "wire_name" }` 会改变 `value ↦ 动态值`、`value ↦ json` 与 `json ↦ 类` 所输出的对象键；JSON 文本仍是 Norma 的线上操作，例如 `json.pange(value ↦ json)`。

- `@ radix` 保留给编译器自有的元数据。历史上的词干派生含义已废弃；词干派生仍是一种源码命名约定，而非编译器生成的变位。被接受的指令形式是顶层函数上的 `@ radix lane "air"` / `"mir"` / `"hir-direct"`，用于显式编译器通道路由；不支持的通道/目标组合会以诊断拒绝，而非被忽略。
- `@ verte` 定义代码生成变换（方法名或模板）
- `@ nondum [TARGET] ["REASON"]` 标记某声明存在于接口中，但当前目标不可用
- `@ cli "NAME"` 把一个 `入口` 标记为 CLI 程序
- `@ imperium "NAME"` 把一个函数标记为 CLI 命令入口
- `@ optio NAME ...` 定义一个 CLI 选项；布尔标志使用 `类型 布尔`
- `@ operandus [其余] TYPE NAME ...` 定义一个 CLI 位置参数
- `@ futura` 标记函数为异步
- `@ cursor` 标记函数为生成器
- `@ 公开` 标记导出，`@ interna` 标记包内可见，`@ 私有` 为显式模块私有标记；未标记的顶层声明默认为模块私有，混用不同可见性级别会触发 `SEM019`。
- `@ 保护` 被保留并以语义诊断拒绝；它没有包、子类或同级文件可见性含义

- `继承` = 继承自，`实现` = 实现
- `静态` = static，`属性` = 绑定/属性

### 接口

```ebnf
implendumDecl   := '契约' IDENTIFIER typeParams? '{' implendumMethod* '}'
implendumMethod := annotation* '函数' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause?
```

`契约` 是**契约**构造：供 `实现` 使用的纯签名方法（*implendum* 是 *implere* 的动形词，意为“必须被满足者”）。导入命名空间以 `.fab` 文件边界为准；导出声明位于文件顶层。

### 类型别名

```ebnf
typeAliasDecl := '类型' IDENTIFIER genericParams? '=' typeAnnotation
```

### 枚举

```ebnf
enumDecl   := '枚举' IDENTIFIER '{' enumMember (',' enumMember)* ','? '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
```

### 标签联合

```ebnf
discretioDecl := '判别' IDENTIFIER typeParams? '{' variant (',' variant)* ','? '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER (',' typeAnnotation IDENTIFIER)*)?
```

### 标识符命名

混 合大小写且小写起首的名称在语法上被接受，但在语言、标准库、宿主路由或编译器自有内建 API 中并非 Faber 推荐写法。优先使用单个词。若单个词无法承载含义，仅在少数情况下使用 snake_case。若两种形态都不合适，那该方法多半不属于核心表面，除非它至关重要。标准库的编解码在所有模块中使用机械动词三件套 `pange` / `solve` / `tempta`——见 `docs/stdlib/stdlib-mechanical-verbs.md`。公开文本库是 `norma:chorda`——见 `docs/stdlib/chorda-methods.md`。

### 导入

```ebnf
importDecl     := importRecord | importSugar
importRecord   := '导入' '{' importFieldList? '}'
importFieldList := importField (',' importField)* ','?
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := '取自' '=' STRING
importVisibilityField := 'visibilitas' '=' visibility
importNameField := 'nomen' '=' IDENTIFIER
importAliasField := '作为' '=' IDENTIFIER
importWildcardField := '全部' '=' IDENTIFIER

importSugar    := '导入' '取自' STRING visibility? (namedImport | wildcardImport)?
visibility    := '公开'
namedImport   := IDENTIFIER ('作为' IDENTIFIER)?
wildcardImport := '*' '作为' IDENTIFIER
```

示例：

```fab
导入 取自 "hono" Hono
导入 取自 "hono" Context
导入 取自 "norma:chorda"                         # 默认不重导出
导入 { 取自 = "norma:json/solve", 作为 = solve_mod }
导入 取自 "norma:consolum" consolum
导入 取自 "faber:*" faber              # 内核清单 glob
导入 取自 "lodash" * 作为 _
导入 取自 "./types" 公开 User               # 重导出
```

导入的 `私有` 标记已移除（VM-U3）：无标记的导入默认不重导出，`公开` 是重导出标记。缺省命名绑定时，若导入路径末段是合法且不冲突的标识符，则默认取该段为绑定名。若推断名非法或与既有顶层绑定冲突，请显式写出 `nomen` 或 `作为` 绑定。

`导入 取自 "faber:*" faber` 是内核专属语法糖：glob 位于导入路径字符串内部，把发布二进制文件中的内核清单展开为 `faber.<module>.<verb>` 调用。它不是通配重导出，也不会创建运行期聚合值。

---

## 类型

```ebnf
typeAnnotation := ('借自' | '传入')? baseType ( '∪' typeAnnotation )*
baseType       := holeType | functionType | qualifiedType typeParams? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
typeParams     := genericParams
```

- 数组写作 `列表<T>`。后缀 `T[]` 不被接受。
- `借自`/`传入` 作为类型前缀，标记所有权（借用/可变借用）。
- 内联联合 `T ∪ U`（cup）用于即席值联合；`T ∪ 空` 是规范的可空类型形态（降级为 Option<T>）。
- 联合在语法中是右结合，但按扁平方式解析；在语义降级时会对重复项与仅 `空` 的情况发出诊断。
- `可选` 是声明标记（参数/字段名称之后），绝不是类型前缀。
- 限定类型路径（如 `terminus.Terminus`）通过导入的命名空间绑定来命名类型。前缀必须解析为命名空间；末段必须解析为承载类型的声明。

函数类型支持高阶函数签名：

```fab
函数 filtrata((T) → 布尔 pred) → 列表<T>
函数 compose((A) → B f, (B) → C g) → (A) → C
函数 apply((整数) → 整数 ⇥ 文本 op, 整数 n) → 整数 ⇥ 文本
```

### 基础类型

| Faber      | 含义 |
| ---------- | ------- |
| `文本`     | Unicode 字符串 |
| `窄字串`   | 仅 ASCII 字符串 |
| `形式`     | 捕获的模板与参数 |
| `整数`     | 整数（默认 `i64`） |
| `模数<W>`  | 无符号模数字；算术按 2^W 取模回绕 |
| `小数`     | 浮点（默认 `f64`） |
| `布尔`     | 布尔 |
| `空`       | null |
| `无值`     | void |
| `永不`     | never |
| `未知`     | unknown |
| `字节`     | 字节序列 |

带尺寸的基础类型接受一个可选的**宽度标记**（不是用户类型参数）：

| 族 | 标记 | 非法示例 |
| ------ | ------- | --------------- |
| `整数<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` | `整数<f32>` → 改用 `小数<f32>` |
| `小数<W>` | `f16`, `f32`, `f64` | `小数<i32>` → 改用 `整数<i32>`；`bf16` 暂缓 |
| `模数<W>` | `u8`, `u16`, `u32`, `u64` | `模数<i32>` → 有符号宽度不是模数字 |

裸 `整数` / `小数` 仍是 `整数<i64>` / `小数<f64>` 的简写。

`模数<W>` 是独立的语义族：算术不会与 `整数<W>` 隐式混用，但显式的同宽转换仍可用。字面量必须在 `0..=2^W-1` 范围内（对于 `模数<u64>`，上限为 `18446744073709551615`）。移位计数本身是模数的：`x ⇐ W` 是完整回绕。跨宽度的模数算术被拒绝。

### 泛型集合

| Faber          | 含义  |
| -------------- | -------- |
| `列表<T>`      | 数组 |
| `映射<K,V>`    | map |
| `集合<T>`      | set |
| `期约<T>`      | promise |
| `游标<T>`      | 迭代器 |
| `tensor<T, Figura>` | 静态形状 `Figura` 的稠密同质缓冲；数值方法要求数值元素类型 |
| `vector<T, N>` | 寄存器级数值向量，静态宽度 `N`（单一维度，非缓冲支持） |
| `matrix<T, [R, C]>` | 寄存器级数值矩阵，恰好两个静态维度（非缓冲支持，也非 tensor 别名） |
| `atomic<T>` | 对存储敏感的原子单元；v1 仅接受 `i32` / `u32` 元素，且访问必须经过原子方法 |
| `sparsa<T, Figura>` | 静态形状 `Figura` 的稀疏同质缓冲；省略的坐标视为零；数值方法要求数值元素类型 |

`Figura` := `_` | 自然数 | 标识符 | `[` figura-list `]`（空 `[]` 为秩 0）。裸 `tensor<T>` 不完整——秩 0 请用 `tensor<T, []>`，推断形状请用 `tensor<T, _>`。

`无值` 用于 `tensor<T, []>` 会生成秩 0 张量（一个默认初始化的元素槽）。
`无值` 用于 `sparsa<T, Figura>`（任意形状）会生成全零稀疏张量，无任何存储项。
`matrix<T, Figura>` 要求恰好两个维度；裸 `matrix<T>` 以及一维或三维 matrix 形状会被拒绝。
`atomic<T>` 在 v1 中要求 `T` 为 `i32` 或 `u32`。原子单元不可与其元素类型互换；请使用 `load`、`store`、`exchange` 与 `compare_exchange` 接收者方法。
通过 `crea` / `structa` / `↦` 构造多维张量。
`Type(...)` 不是构造形式：`vector<f32, 4>(...)`、`matrix<f32, [2, 2]>(...)`、`tensor<f32, [2, 2]>(...)` 以及诸如 `整数("42")` 的标量形式都会被拒绝。请使用 `value ↦ Type`、命名的库构造器，或 `类 { field = value }` 记录。

张量索引/形状内建槽（`accipe`、`ponde`、`forma`、`crea`、`structa`）接受在调用点符合规范 `列表<整数>` / `&[i64]` 运行边界的整数列表（例如 GPU 线程 id 用 `列表<u32>`；不可用 `列表<u64>`）。这是仅限于这些槽的结构性例外，不会放宽有符号↔无符号的数值格（详见 `tensor-intrinsics.md` 中的 Index vector parameter policy）。

值联合使用内联 `T ∪ U`（可空：`T ∪ 空`）。标签联合使用 `判别`。
`集合.unio()` 是一个 set 方法，不是类型构造器。

### 类型语法糖

类型语法糖是数值与集合类型的另一种写法。它**仅出现在类型位置**，且**语义上完全等同**于长形式——编译器对二者一视同仁。这是语法糖的唯一权威参考；规范的其余部分使用长形式。

语法糖把一个宽度标记与一个可选的单字母族前缀组合。宽度标记有 `i8`/`i16`/`i32`/`i64`（有符号）、`u8`/`u16`/`u32`/`u64`（无符号），以及 `f16`/`f32`/`f64`（浮点）。裸宽度标记（无前缀）糖化标量数值类型；族前缀糖化对应宽度的集合。

| 语法糖 | 长形式 | 方括号规则 |
| ----- | --------- | ------------ |
| `i8` … `u64`, `f16`/`f32`/`f64` | `整数<W>`, `小数<W>` | 无（裸标记） |
| `lf32`, `lu32`, `li64`, … | `列表<f32>`, `列表<u32>`, `列表<i64>`, … | 无 |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | 可选 `Figura` |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | 可选 `Figura` |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 4>`, `vector<u32, 3>` | 可选单一宽度 |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **必填**，两个维度 |

方括号形状：`[]` 为秩 0，`[2, 3]` 为固定形状，无方括号则推断形状（`_`）。matrix 要求恰好两个维度。语法糖从不使用 `<>`。对于非宽度的元素类型（如 `tensor<文本, [3]>`），请使用完整形式。

语法糖仅在类型语法中保留——名为 `tf32`、`lf32` 等的值标识符不受影响。

`模数<W>` 没有语法糖；请写完整的 `模数<u32>`。

**拼写偏好（作者约定，非语法）：**一般 Faber 代码倾向于长形式以利可读性；以数值/张量为主的模块可偏好语法糖。请按模块或文件选择。

---

## 控制流

### 条件

```ebnf
ifStmt     := '如果' expression arm ('否则如果' ifStmt | elseClause)?
elseClause := '否则' elseArm
arm        := (blockStmt | stmtBodyJoint statement) catchClause?
elseArm    := (blockStmt | stmtBodyJoint statement) catchClause?
```

- `如果` = if，`否则如果` = else-if，`否则` = else
- `则` 用于单语句体，包括 `则 返回`、`则 抛错`、`则 崩溃` 与 `则 静默`（此处不接受 `∴`）
- `静默` 用于显式空操作（源自乐谱记号：意为“静默”）

### 循环

```ebnf
whileStmt  := '当' expression (blockStmt | stmtBodyJoint statement) catchClause?
iteraStmt  := '遍历' (('取自' | '借自') expression | 'ab' expression) ('常量' | '变量') IDENTIFIER (blockStmt | stmtBodyJoint statement) catchClause?
```

- `当` = while
- `遍历 取自...常量`/`遍历 取自...变量` = for-of（取值）
- `遍历 借自...常量`/`遍历 借自...变量` = for-in（取键）
- `遍历 ab range 常量/变量 i` = 区间迭代（如 `遍历 ab 0‥10 步 2 常量 i { 显示 i }`；`步` 属于区间表达式）

### 开关 / 分派

```ebnf
eligeStmt    := '选择' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase    := '情况' expression (blockStmt | stmtBodyJoint statement)
defaultCase  := '默认' (blockStmt | stmtBodyJoint statement)
```

### 模式匹配

```ebnf
discerneStmt := '匹配' '全部'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase  := '情况' patterns (blockStmt | stmtBodyJoint statement)
patterns     := pattern ((',' | '且') pattern)*
pattern      := '_' | literal | (IDENTIFIER patternBind?)
patternBind  := ('作为' IDENTIFIER) | (('常量' | '变量') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('作为' IDENTIFIER)?
```

### 守卫

```ebnf
guardStmt   := '守护' '{' guardClause+ '}'
guardClause := '如果' expression (blockStmt | stmtBodyJoint statement)
```

### 资源管理

```ebnf
curaStmt    := '管护' STRING ('常量' | '变量') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### 解构提取

```ebnf
extractStmt   := '取自' expression ('常量' | '变量') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField  := IDENTIFIER ('作为' IDENTIFIER)?
restField     := '其余' IDENTIFIER
```

### 控制转移

```ebnf
returnStmt   := '返回' expression?
breakStmt    := '中断'
continueStmt := '继续'
noopStmt     := '静默'
```

---

## 错误处理

```ebnf
throwStmt   := ('抛错' | '崩溃') expression ['如果' expression]
catchClause := '捕获' IDENTIFIER blockStmt
assertStmt  := '断言' expression ('secus' expression)?
requiritStmt := '需求' expression 'secus' expression
```

- `捕获` 附着于结构化语句与条件分支。它不附着于任意的裸块。
- `执行 { ... } 捕获 err { ... }` 是规范的一次性局部可恢复错误边界。
- `尝试` 是旧式 try/catch 接口，会以迁移诊断拒绝。
- `最终` 是旧式 finally 接口，会以迁移诊断拒绝。
- `抛错` = throw（可恢复），`崩溃` = panic（致命）。
- `抛错` 与 `崩溃` 上的可选 `如果 <expr>` 守卫是解析器语法糖：`抛错 val 如果 cond` 在解析期脱糖为 `如果 cond { 抛错 val }`。缺省守卫时该语句无条件执行（行为不变）。
- `断言` 是运行时不变量检查，概念上脱糖为 `崩溃 "msg" 如果 !cond`，源码中保留正向条件。`secus` 引入假路径消息，与其在 `si/secus` 和 `sic/secus` 三元运算中的角色一致。

---

## 表达式

### 运算符（按优先级，由低到高）

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
ternary    := or (('?' expression ':' | '乃' expression '否则') ternary)?
or         := and (('或') and)*
and        := equality (('且') equality)*
equality   := comparison (('≡' | '≠' | '≈' | '≉' | '是' | '非' '是') comparison)*
comparison := bitwiseOr (('<' | '>' | '≤' | '≥' | '内' | '间') bitwiseOr)*
# 排序运算符使用 Unicode 字形；成员关系使用拉丁关键字 内/间
# （Faber 评注中的身份判断）。字形别名如 ∈ 不在现行契约中。
bitwiseOr  := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift      := range (('⇐' | '⇒') range)*
range      := additive (('‥' | '…' | '迄' | '到') additive ('步' additive)?)?
additive   := multiplicative (('+' | '-') multiplicative)*
multiplicative := coalesce (('*' | '/' | '%') coalesce)*
# 兜底 是局部可空消除（T ∪ 空 兜底 T → T），不是逻辑 或。
# 它比算术绑得更紧，故 prefix + item 兜底 "" 等价于 prefix + (item 兜底 "")。
# 兜底 的右端可构成一个区间构造（maybeRange 兜底 0‥0）。
coalesce   := unary ('兜底' velRhs)*
velRhs     := unary (('‥' | '…' | '迄' | '到') unary ('步' unary)?)?
unary      := ('-' | '¬' | '非' | '等候' | '构造') unary | cast
cast       := call ('∷' typeAnnotation | conversio)*
conversio        := '↦' typeAnnotation typeParams? inlineRecovery?
inlineRecovery   := '⇥' unary
```

`↤` 是定向转换赋值：先求右侧值，通过 `↦` 路径转换为左侧位置的静态类型，再赋值。`⇥` 内联恢复仅在 `↤` 上合法，`←` 之后不允许。

已退役的谓词关键字不是前缀一元语法。请使用 `expr 是 真`、`expr 是 假`、`expr 是 空`、`expr 非 是 空`、`expr < 0` 或 `expr > 0`。

**静态类型指派（`∷` / verte）：**

`∷` 字形（U+2237，proportion）为表达式显式指派目标类型。当源表达式已存在、且编译器需要一个静态目标形状时使用：

- 基础/别名 → 转型（无运行期效果）：`data ∷ 文本` → TypeScript：`(data as string)`
- 内建集合 → 目标形状的集合值：`[1, 2, 3] ∷ 列表<整数>`
- variant 表达式 → enum/接口目标指派：`构造 Click { x = 10 } ∷ Event`

对于普通 `类` 值，请优先使用类型化构造；对于普通空集合值，请优先使用 `无值`：

```fab
常量 _ point ← Point { x = 10 }
常量 列表<整数> xs ← 无值
```

只有 `∷` 字形被接受为后缀静态类型指派运算符。拉丁形式 `qua`、`innatum` 与 `novum` 曾是别名，现已移除（见 verte-alias-clean-break）。

**运行期转换（`↦` / conversio）：**

`↦` 字形（U+21A6，rightwards arrow from bar）是运行期值转换运算符。与 `∷`（编译期转型）不同，它执行可能失败的实际解析/转换：

- `"22" ↦ 整数` → Rust：`"22".parse::<i64>().unwrap()`
- `"bad" ↦ 整数 ⇥ 0` → Rust：`"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ 文本` → Rust：`42.to_string()`

内联失败恢复使用 `⇥`，紧跟在 conversio 目标之后（`↦ T ⇥ recovery-expr`）。恢复表达式必须是 `T` 类型的值。

将 `兜底` 用作 conversio 恢复会以迁移诊断拒绝。`兜底` 仅用于局部可空消除（`x 兜底 y`、参数默认值），不是逻辑 `或`。带括号的 conversio 结果仍可与 `兜底` 组合为普通默认。

### 调用与成员访问

```ebnf
call          := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix    := typeArgs? '(' argumentList ')'
memberSuffix  := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList  := (argument (',' argument)*)?
argument      := '展开'? expression
```

### 字符串与模板字面量

Faber 采用**分隔符语义**：每种引号形式代表不同的源形态。它们并非可互换的同义词。

| 形式 | 类型 | 角色 |
| --- | --- | --- |
| `'...'` | `窄字串` | 固定机器 token；无 `§`；无 `(...)` |
| `"..."` | `文本` | 短 Unicode 单行字符串；`(...)` 渲染 |
| `«...»` | `文本` | 块/多行 Unicode；`(...)` 渲染 |
| `` `...` `` | `形式` | 捕获的模板；`(...)` 捕获 |
| `{ ... }` | `json` | 编译期以对象为根的 JSON 文档（内部用 `:`） |
| `\|...\|` | `字节` | 编译期十六进制字节 |
| `"..." ↦ regex` | `regex` | 由文本转换编译出的模式 |
| `[ ... ]` | `列表<T>` | Faber 列表（非 JSON 数组，也非字节） |

`§`（U+00A7）在 Unicode 形式（`"`、`«`、`` ` ``）中作为模板占位符。它不能出现在 `窄字串` 字面量中。

**渲染模板**（`文本`）：`"..."(...)` 与 `«...»(...)` 降级为 `scriptum("...", args...)`。

**捕获模板**（`形式`）：`` `...`(args) `` 捕获模板文本与参数而不渲染。适合绑定的 SQL/URL 载荷；不要为此使用 `«...»(...)`。

块 `文本` 使用书名号 `«...»`。重型引号对已退役（在许多字体中与 `"` 视觉上太接近）。

实现状态（2026-06-30）：

- 已交付：`"..."`、`«...»` 块 `文本`、`'...'` → `窄字串`、`` `...` `` → `形式`、`|...|` → `字节`、`{ ... }` → `json`，以及 text/ascii `↦ regex`。
- 待 factory 交付：斜杠分隔的 `/.../` 正则字面量。

内联块示例：

```fab
常量 _ tag ← «inline»
```

多行块示例（开 `«` 后换行）：

```fab
常量 _ blob ← «
    select id, email
    from accounts
»
```

捕获模板示例：

```fab
常量 _ q ← `select * from accounts where id = §`(accountId)
```

字节十六进制字面量示例：

```fab
常量 _ sig ← |de ad be ef|
常量 _ hello ← |48 65 6c 6c 6f|
```

### 格式化模板应用

字符串字面量调用语法是格式化模板应用的规范源形式：

```fab
"status: § (§)"(sample_status(), "ok")
"status: §1 (§0)"("ok", sample_status())
```

它降级为编译器的 `scriptum("...", args...)` 形式。在普通源码中使用字符串模板形式；将 `scriptum(...)` 留给显式脱糖示例和面向编译器的文档。

对于 `文本`，方括号索引以 Unicode 标量为单位：

```fab
"Salve, §!"[7]            # "§"
"hello world"[0‥5]        # "hello"
"hello world"[0 到 10]    # "hello world"
"abcdef"[0‥6 步 2]       # "ace"
```

文本切片接受完整的区间形式，包括 `步`。

对于 `列表<T>`，方括号索引是单元素访问。索引必须是单个整数；不接受区间切片（拷贝区间请用 `sectio(start, end)`）：

```fab
xs[i]        # 位置 i 处的元素
xs[i] ← v    # 写入位置 i 处的元素
```

列表方括号访问是**普通**的，而非可空：它返回裸元素 `T`，越界时陷阱。这与 `tensor` 不同——后者的方括号读取是 `accipe` 语法糖，返回 `T ∪ 空`。可空列表访问请用 `xs.accipe(i) → T ∪ 空` 配合 `兜底`。

对于 `tensor<T, Figura>`，方括号索引是张量内建表面的语法糖：

```fab
vector[id]        # vector.accipe([id])
vector[id] ← v    # vector.ponde([id], v)
grid[[r, c]]      # grid.accipe([r, c])
grid[[r, c]] ← v  # grid.ponde([r, c], v)
```

读取返回 `T ∪ 空`，与 `accipe` 一致；请先使用 `兜底` 或其他普通选项处理形式，再参与算术。秩 1 张量接受符合张量 `i64` 运行边界的标量整数索引（`u64` 被拒绝）。秩 N 张量使用列表形态的索引表达式，如 `[[r, c]]`，或一个绑定的 `列表<integer>` 值。`grid[r, c]` 不是合法语法；`memberSuffix` 在方括号之间仍只包含一个 `expression`。

`字节` 是字节缓冲原语，不是数组，因此不接受（读或写）方括号索引。字节访问基于方法：

```fab
buf.accipe(i)      # → 整数<u8> ∪ 空（可空；越界安全）
buf.appende(b)     # 原地追加单字节
buf.longitudo      # 字节长度
```

这是有意为之。`字节` 是 HAL、加密与 `|hex|` 字面量所用的不透明边界字节缓冲；其读取默认可空，方括号语法保留给陷阱式访问模型。对于字节密集的索引，内部请使用 `列表<整数<u8>>`（方括号读/写、越界陷阱），并在边界处保留 `字节`。

### 基本表达式

`无值` 是上下文相关的空集合标记（标识符形态，不是保留关键字）。请与显式集合类型一起使用：`常量 列表<整数> xs ← 无值`，或 `常量 tensor<小数<f32>, []> t ← 无值`。

```ebnf
primary := IDENTIFIER | NUMBER | STRING | ASCII_STRING | BACKTICK_STRING
         | '自身' | '真' | '假' | '空'
         | '无值' | arrayLiteral | jsonLiteral | typedConstructor
         | adExpr
         | '(' expression ')'
adExpr    := '调用' asciiLiteral adOpener?
arrayLiteral := '[' argumentList? ']'
# 裸 { ... } 是 JSON 文档字面量。键是用 `:` 分隔的带引号 JSON 字符串；
# 值是 JSON 常量。匿名 Faber 对象（{ key = expr }）已退役（字面量族 Stage 6）。
# 类构造使用 typedConstructor。
jsonLiteral := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonMember  := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('展开' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
# JSON 值：仅常量（无 Faber 表达式，无变量引用）。
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonArray  := '[' (jsonValue (',' jsonValue)* ','?)? ']'
jsonString := STRING
jsonNumber := NUMBER                       # 无 '.'/e/E 时为 整数，否则为 小数
```

`STRING` 既包括 `"` 分隔的短字符串，也包括 `«` 与 `»` 分隔的块字符串。`'...'`（`窄字串`）与反引号 `` `...` ``（`形式`）是独立的字面量形式（见上文**字符串与模板字面量**）。

裸 `{ ... }` 现在生成以对象为根、类型为 `json` 的 JSON 文档：`{ "name": "Alice", "age": 30, "active": true }`。键是用 `:` 分隔的带引号 JSON 字符串；值仅限 JSON 常量。重复键会报错（第二次出现即出错）。指派为 `映射<K,V>` 会降级为真正的常量 map。显式宽化到宽动态载体请用 `↦ 动态值`。类/variant 构造 `Type { field = expr }` 仍使用 Faber 的 `=` 语法。

### 特殊表达式

```ebnf
// verte（∷）是后缀——在上面的 cast 产生式中解析
fingeExpr     := '构造' IDENTIFIER ('{' fieldList '}')? ('∷' IDENTIFIER)?
praefixumExpr := '前缀' (blockStmt | '(' expression ')')
formatStringExpr := STRING '(' argumentList ')'                # 通过 scriptum 渲染 文本
formaTemplateExpr := BACKTICK_STRING '(' argumentList ')'      # 捕获 形式
scriptumExpr  := '格式化' '(' STRING (',' expression)* ')'   # 显式/脱糖形式
legeExpr      := '读取' '行'?
regexFromText   := (STRING | ASCII_STRING) '↦' 'regex'
# 斜杠分隔的正则字面量尚不是现行语法。`/` 词法为除法运算符，
# 而 `//` 与 `/* ... */` 作为非法注释被拒绝。
# 旧式 'sed' STRING [IDENT] 已移除；请使用 "..." ↦ regex。
```

---

## 模式

```ebnf
objectPattern  := '{' patternProperty (',' patternProperty)* '}'
patternProperty := '其余'? IDENTIFIER ('作为' IDENTIFIER)?
arrayPattern   := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | '其余'? IDENTIFIER
```

---

## 诊断输出

```ebnf
outputStmt := ('显示' | '查看' | '警告' | '写入') expression (',' expression)*
```

- `显示` = 中性诊断注记，`查看` = debug/inspect，`警告` = warn
- `写入` 是一种诊断通道拼写；真实输出请使用当前标准库方法

### 注释

Faber 只接受**行注释**：`#` 至行尾。`#` 必须是逻辑行上第一个非空白 token（仅允许可选的前导 ASCII 空格或制表符——其他 Unicode 空白分隔符不会被词法分析跳过）。若 `#` 在同一行上跟在任意其他 token 之后，则是**词法错误**，消息为 `# comments must start a line; move this comment above the code`。

合法的行首注释会作为 `leading_trivia` 向前附着到下一条语句或声明上（见 comment-preservation）。位于字符串字面量、`窄字串` 字面量、`形式` 模板及其他分隔字面量内部的 `#` **不是**注释。

---

## 入口点

```ebnf
incipitStmt  := '入口' blockStmt
incipietStmt := '异步入口' blockStmt
```

- `入口` = 同步入口，`异步入口` = 异步入口

---

## 测试

```ebnf
probandumDecl := '验题' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt     := '测试' STRING probaModifier* blockStmt
probaModifier := '跳过' STRING | '预期' STRING | '仅' | '标签' STRING
              | '时限' NUMBER | '计量' | '重复' NUMBER | '易碎' NUMBER
              | '需求' STRING | '仅于' STRING
praeparaBlock := ('备置' | '异步备置' | '收尾' | '异步收尾') '全部'? blockStmt
```

---

## CLI 框架

```ebnf
cliDecl       := annotation* (incipitStmt | funcDecl)
cliAnnotation := cliProgramAnnotation | imperiumAnnotation | optioAnnotation | operandusAnnotation
```

Faber 支持构建 CLI 应用，自动解析参数并生成帮助。

### CLI 入口点

```fab
@ cli "faber"
@ optio verbose longum "verbose" 类型 布尔
入口 参数 args {
    # CLI 框架自动解析参数
}
```

### CLI 选项与参数

```fab
@ imperium "deploy"
@ optio target brevis "t" longum "target" 类型 文本 descriptio "Deployment target"
@ optio verbose brevis "v" longum "verbose" 类型 布尔 descriptio "Enable verbose output"
@ operandus 文本 file descriptio "File to deploy"
函数 deploy() 参数 args {
    # 参数自动解析并传入
}
```

---

## 能力调用

表达式形式的 `调用` 是唯一受支持的 `调用` 表面。旧式带类型的 `调用 "route" (args) → T { }` 与语句级流式块 `ad 'route' { meus/tuus … }` 在解析期即被拒绝。

```ebnf
adExpr        := '调用' asciiLiteral adOpener?
adOpener      := '(' expression ')'
conversioExpr := expr '↦' typeAnnotation
```

- 路由：`asciiLiteral`（`'solum:lege'`），而非双引号 `STRING`。
- Opener：可选的单个 `expression` → 作为 `动态值` 的 Request `data`。
- **表达式 `调用`**：无块；求值为一个 `sermo` 会话句柄。请使用后缀 `↦ T`（物化）、赋值给 `sermo`，或打开实时方向视图：`s.meus<T>()`（出站 `da` / `fini`）与 `s.tuus<T>()`（入站 `accipe` / `cursor` / `exhauri` / `fini`）。入站内容帧请用 `s.tuus<T>().cursor()` 迭代，而不是直接 `遍历 取自 s.tuus<T>()`。
- **已移除（解析错误）：**旧式带类型 `调用 "route"`、块 `meus`/`tuus` 分支，以及语句级 `发送`。
- 类型：编译器自有的 `scrinium`、`status`；不透明的 `sermo` 会话句柄。
- `sermo ↦ T` 使用面向 `T` 的类型导向收集器，把入站帧物化为单个 `T` 类型值。

见 [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md)。

---

## 集合操作

原先的 `ab` 集合管道 DSL 已退役。集合的过滤、切片与聚合通过普通的 `文本`/`列表`/`映射`/`集合` 方法与闭包来表达，而不是语法层的查询表达式。`文本`、`整数`、`小数`、`列表<T>`、`映射<K,V>` 与 `集合<T>` 是编译器自有的核心类型；其规范方法表面记录在 `docs/design/textus-intrinsics.md`、`docs/design/numerus-intrinsics.md`、`docs/design/fractus-intrinsics.md`、`docs/design/lista-intrinsics.md`、`docs/design/tabula-intrinsics.md` 与 `docs/design/copia-intrinsics.md` 中，而非 Norma 声明。

`prima` 与 `ultima` 是普通方法名，不是变换关键字。`ubi` 不是现行集合语法。

`取自` 用于迭代（`遍历 取自 items 常量 x`）与导入（`导入 取自 "path"`）。

---

## Fac 块

```ebnf
facBlockStmt := '执行' blockStmt catchClause? ('当' expression)?
```

- `执行 { ... }` 把作用域块执行一次。
- `执行 { ... } 捕获 err { ... }` 是规范的局部可恢复错误边界。
- `执行 { ... } 当 condition` 是后测试循环形式；后缀 `当` 仅附着于 `执行`，而非任意前置块。

---

## 目标支持

目标支持**不属于**语法——本文件只定义语言本身。各编译目标降级到何种语法，以及相关运行期策略，见：

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — 生成的语法×目标可降级矩阵（权威行）。
- [`docs/design/target-capability-matrix.md`](docs/design/target-capability-matrix.md) — 运行期/契约策略（erase/warn/defer）、流水线路由、按目标的契约。

---

## 关键字参考

| 类别                | Faber                         | 含义             |
| ------------------- | ----------------------------- | ------------------- |
| **声明**            | `判别`                        | 标签联合 |
|                     | `常量`                        | const               |
|                     | `函数`                        | function            |
|                     | `类`                          | class               |
|                     | `契约`                        | interface contract  |
|                     | `维度`                        | size/index 泛型参数（在 `<>` 列表中） |
|                     | `枚举`                        | enum                |
|                     | `设`                          | 推断型不可变局部量 |
|                     | `可选`                        | 可选声明槽位（名称之后） |
|                     | `类型`                        | type alias          |
|                     | `无值`                        | 上下文相关的空集合标记 |
|                     | `变量`                        | let                 |
| **控制流**          | `如果` / `否则如果` / `否则`  | if / else-if / else |
|                     | `守护`                        | guard               |
|                     | `匹配`                        | pattern match       |
|                     | `当`                          | while               |
|                     | `选择` / `情况`               | switch / case       |
|                     | `执行`                        | 作用域块 / 局部错误边界 |
|                     | `遍历 取自...常量`            | for-of（取值）     |
|                     | `遍历 借自...常量`            | for-in（取键）     |
|                     | `遍历 ab...常量`              | 区间迭代     |
|                     | `继续`                        | continue            |
|                     | `返回`                        | return              |
|                     | `中断`                        | break               |
|                     | `静默`                        | no-op（静默）     |
|                     | `则`                          | 紧凑单语句体连接符 |
|                     | `∴`                           | 仅紧凑闭包连接符 |
| **错误处理**        | `捕获`                        | 结构化局部处理 |
|                     | `断言`                        | assert              |
|                     | `需求`                        | require（可恢复） |
|                     | `抛错`                        | throw               |
|                     | `可抛`                        | throws 修饰符     |
|                     | `崩溃`                        | panic               |
|                     | `执行` / `捕获`               | 局部可恢复错误边界 |
| **异步**            | `@ futura`                    | async 注解    |
|                     | `@ cursor`                    | generator 注解 |
|                     | `等候`                        | 按上下文 await/yield |
| **端点**            | `调用`                        | 能力调用表达式 |
|                     | `发送`                        | 已退役的语句级帧发射 |
| **布尔**            | `真`                          | true                |
|                     | `或`                          | or                  |
|                     | `且`                          | and                 |
|                     | `假`                          | false               |
|                     | `非`                          | not                 |
|                     | `兜底`                        | 局部可空默认 |
| **对象**            | `自身`                        | this/self           |
|                     | `构造`                        | 构造 variant   |
| **类型形状**        | `∷`                           | 静态类型指派 / 编译期转型 |
| **类型转换**        | `↦ target`                    | 运行期值转换 |
|                     | `↦ T ⇥ expr`                  | conversio 带内联恢复，类型 `T` |
|                     | `↦ 整数`                      | 解析为整数    |
|                     | `↦ 小数`                      | 解析为浮点    |
|                     | `↦ 文本`                      | 转换为字符串   |
|                     | `↦ 布尔`                      | 转换为布尔  |
| **位运算**          | `∧` / `∨` / `⊻` / `¬`         | and/or/xor/not      |
|                     | `⇐` / `⇒`                     | 左/右移    |
| **诊断**            | `显示`                        | 中性注记        |
|                     | `警告`                        | warn                |
|                     | `写入`                        | 诊断通道  |
|                     | `查看`                        | debug/inspect       |

---

## 关键语法规则

1. **类型在前的参数**：`函数 f(整数 x)`，而非 `函数 f(x: 整数)`
2. **类型在前的声明**：`常量 文本 name`，而非 `常量 name: 文本`
3. **迭代循环**：`遍历 取自/借自 collection 常量/变量 item { }` 或 `遍历 ab range 常量/变量 item { }`（动词在前，之后是来源，再是绑定）
4. **条件加括号合法但不地道**：优先 `如果 x > 0 { }` 或 `如果 flag 是 真 { }`，而非 `如果 (x > 0) { }`
5. **诊断关键字是语句**，不是函数——`显示 x` 可用，`显示(x)` 也可用（括号只是组合表达式），但 `显示` 不是可调用值

---

## Reader pack glossary (machine extract)

### Keywords
| Latin | Localized |
|---|---|
| discretio | 判别 |
| fixum | 常量 |
| functio | 函数 |
| genus | 类 |
| implendum | 契约 |
| importa | 导入 |
| modulus | 模数 |
| ordo | 枚举 |
| sit | 设 |
| typus | 类型 |
| varia | 变量 |
| abstractus | 抽象 |
| ceteri | 其余 |
| curata | 委派 |
| errata | 勘误 |
| exitus | 退出 |
| generis | 静态 |
| iacit | 可抛 |
| immutata | 不变 |
| interna | 内部 |
| magnitudo | 维度 |
| nexum | 属性 |
| optiones | 选项 |
| prae | 前 |
| privata | 私有 |
| protecta | 保护 |
| publica | 公开 |
| sponte | 可选 |
| casu | 情况 |
| ceterum | 默认 |
| custodi | 守护 |
| discerne | 匹配 |
| dum | 当 |
| elige | 选择 |
| ergo | 则 |
| fac | 执行 |
| itera | 遍历 |
| secus | 否则 |
| si | 如果 |
| sic | 乃 |
| sin | 否则如果 |
| perge | 继续 |
| redde | 返回 |
| rumpe | 中断 |
| tacet | 静默 |
| adfirma | 断言 |
| cape | 捕获 |
| cede | 等候 |
| iace | 抛错 |
| mori | 崩溃 |
| clausura | 闭包 |
| falsum | 假 |
| nihil | 空 |
| verum | 真 |
| aut | 或 |
| est | 是 |
| et | 且 |
| non | 非 |
| vel | 兜底 |
| ego | 自身 |
| finge | 构造 |
| implet | 实现 |
| sub | 继承 |
| mone | 警告 |
| nota | 显示 |
| scribe | 写入 |
| vide | 查看 |
| argumenta | 参数 |
| cura | 管护 |
| incipiet | 异步入口 |
| incipit | 入口 |
| ad | 调用 |
| de | 借自 |
| ex | 取自 |
| in | 传入 |
| lege | 读取 |
| lineam | 行 |
| omnia | 全部 |
| praefixum | 前缀 |
| scriptum | 格式化 |
| sparge | 展开 |
| ut | 作为 |
| ante | 迄 |
| inter | 间 |
| intra | 内 |
| per | 步 |
| usque | 到 |
| fragilis | 易碎 |
| futurum | 预期 |
| metior | 计量 |
| omitte | 跳过 |
| postpara | 收尾 |
| postparabit | 异步收尾 |
| praepara | 备置 |
| praeparabit | 异步备置 |
| proba | 测试 |
| probandum | 验题 |
| requirit | 需求 |
| repete | 重复 |
| solum | 仅 |
| solum_in | 仅于 |
| tag | 标签 |
| temporis | 时限 |
| negativum | 负 |
| nonnihil | 有值 |
| nonnulla | 诸值 |
| nulla | 皆无 |
| positivum | 正 |

### Types
| Latin | Localized |
|---|---|
| ascii | 窄字串 |
| textus | 文本 |
| numerus | 整数 |
| modulus | 模数 |
| fractus | 小数 |
| bivalens | 布尔 |
| nihil | 空 |
| vacuum | 无值 |
| numquam | 永不 |
| ignotum | 未知 |
| octeti | 字节 |
| regex | regex |
| json | json |
| valor | 动态值 |
| instans | 时刻 |
| objectum | 对象 |
| quidlibet | 任意 |
| lista | 列表 |
| tabula | 映射 |
| copia | 集合 |
| promissum | 期约 |
| cursor | 游标 |

### Glossary changes vs existing pack
| Latin | Old pack | New (this EBNF) | Why |
|---|---|---|---|
| genus | 类型 | 类 | `类型` 改用于 `typus`（type alias 关键字）；`genus`（class）改用 `类` 以消除歧义并匹配关键字参考表语义 |
| modulus | (缺) | 模数 | 新增：`modulus<W>` 无符号模数字族需要独立词面，且与 `numerus` 区分 |
| ordo | (缺) | 枚举 | 新增：enum 声明关键字 |
| sit | (缺) | 设 | 新增：推断型不可变局部量（`fixum _` 的语法糖），取单字以匹配 `fixum`/`varia` 的单字密度 |
| typus | (缺) | 类型 | 新增：type alias 声明关键字（释放自原 `genus`） |
| abstractus | (缺) | 抽象 | 新增：抽象类前缀 |
| ceteri | (缺) | 其余 | 新增：rest 参数/模式标记 |
| curata | (缺) | 委派 | 新增：分配器要求修饰符 |
| errata | (缺) | 勘误 | 新增：错误通道修饰符 |
| exitus | (缺) | 退出 | 新增：退出码修饰符 |
| generis | (缺) | 静态 | 新增：static 成员标记 |
| iacit | (缺) | 可抛 | 新增：throws 修饰符（与 `iace`=抛错 区分） |
| immutata | (缺) | 不变 | 新增：不可变修饰符 |
| magnitudo | (缺) | 维度 | 新增：泛型尺寸/索引参数 |
| nexum | (缺) | 属性 | 新增：bound/property 成员标记 |
| optiones | (缺) | 选项 | 新增：选项修饰符 |
| prae | (缺) | 前 | 新增：已移除的泛型前缀（保留以诊断） |
| protecta | (缺) | 保护 | 新增：保留的可见性注解名 |
| sponte | (缺) | 可选 | 新增：可选声明槽位标记 |
| custodi | (缺) | 守护 | 新增：guard 语句 |
| ergo | (缺) | 则 | 新增：紧凑单语句体连接符（与 `∴` 区分；后者不本地化） |
| tacet | (缺) | 静默 | 新增：显式 no-op |
| adfirma | (缺) | 断言 | 新增：assert 语句 |
| cede | (缺) | 等候 | 新增：await/yield 上下文关键字 |
| iace | (缺) | 抛错 | 新增：throw（与 `mori`=崩溃、`iacit`=可抛 区分） |
| mori | (缺) | 崩溃 | 新增：panic（致命） |
| clausura | (缺) | 闭包 | 新增：旧式闭包字面量关键字 |
| vel | (缺) | 兜底 | 新增：局部可空消除（与 `aut`=或 区分） |
| ego | (缺) | 自身 | 新增：this/self |
| finge | (缺) | 构造 | 新增：variant 构造 |
| implet | (缺) | 实现 | 新增：implements |
| sub | (缺) | 继承 | 新增：extends |
| incipiet | (缺) | 异步入口 | 新增：异步入口（与 `incipit`=入口 区分） |
| ad | (缺) | 调用 | 新增：能力调用表达式 |
| lege | (缺) | 读取 | 新增：读取输入 |
| lineam | (缺) | 行 | 新增：`lege lineam` 的行标记 |
| omnia | (缺) | 全部 | 新增：通配/全部标记 |
| praefixum | (缺) | 前缀 | 新增：前缀表达式 |
| scriptum | (缺) | 格式化 | 新增：格式化模板的显式/脱糖形式 |
| sparge | (缺) | 展开 | 新增：参数展开 |
| ante | (缺) | 迄 | 新增：区间运算符的词形（与 `usque` 区分） |
| inter | (缺) | 间 | 新增：成员关系“之间”（与 `intra`=内 区分） |
| intra | (缺) | 内 | 新增：成员关系“之内” |
| per | (缺) | 步 | 新增：区间步长 |
| usque | (缺) | 到 | 新增：区间运算符的词形 |
| fragilis | (缺) | 易碎 | 新增：测试易碎性修饰符 |
| futurum | (缺) | 预期 | 新增：测试的 future-target 标记（与注解 `@ futura` 区分，后者保留为注解名） |
| metior | (缺) | 计量 | 新增：基准测试修饰符 |
| omitte | (缺) | 跳过 | 新增：跳过测试修饰符 |
| postpara | (缺) | 收尾 | 新增：teardown 块 |
| postparabit | (缺) | 异步收尾 | 新增：异步 teardown |
| praepara | (缺) | 备置 | 新增：setup 块 |
| praeparabit | (缺) | 异步备置 | 新增：异步 setup |
| proba | (缺) | 测试 | 新增：测试用例语句 |
| probandum | (缺) | 验题 | 新增：测试用例声明 |
| repete | (缺) | 重复 | 新增：重复次数修饰符 |
| requirit | (缺) | 需求 | 新增：require 语句 |
| solum | (缺) | 仅 | 新增：only 测试标记 |
| solum_in | (缺) | 仅于 | 新增：only-in 测试标记 |
| tag | (缺) | 标签 | 新增：tag 测试标记 |
| temporis | (缺) | 时限 | 新增：timeout 测试修饰符 |
| negativum | (缺) | 负 | 新增：负数标记 |
| nonnihil | (缺) | 有值 | 新增：some（单数）标记 |
| nonnulla | (缺) | 诸值 | 新增：some（复数）标记 |
| nulla | (缺) | 皆无 | 新增：none 标记 |
| positivum | (缺) | 正 | 新增：正数标记 |

新增类型（相对现有 pack 的 11 项）：`modulus`→模数、`numquam`→永不、`octeti`→字节、`ascii`→窄字串、`valor`→动态值、`instans`→时刻、`objectum`→对象、`quidlibet`→任意、`promissum`→期约；`regex`/`json` 保持拉丁词面（与 body 中作为类型 token 的 `regex`/`json` 一致，且为通用计算术语，参照已通过质量门槛的 `EBNF.hi.md`）。既有 pack 类型（`textus`/`numerus`/`fractus`/`bivalens`/`nihil`/`vacuum`/`ignotum`/`lista`/`tabula`/`copia`/`cursor`）保持不变。
