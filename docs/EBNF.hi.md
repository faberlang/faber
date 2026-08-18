# Faber भाषा विनिर्देश

> **Reader-locale EBNF (Hindi).** Latin/source-of-truth grammar remains [`EBNF.md`](EBNF.md).
> This file is the Hindi reader surface of that grammar (keywords, commentary, examples).
> Pack keyword/type spellings are extracted from the glossary appendix at the end.
> Glyphs (`← → ∴ ≡ ∪ ⇥` …) never localize; `ergo` localizes, `∴` is clausura-only.

Faber प्रोग्रामिंग भाषा का औपचारिक व्याकरण। सक्रिय कार्यान्वयन मूल Rust workspace में है: `crates/faber` पैकेज और प्रोजेक्ट tooling के लिए तथा `crates/radix` compiler pipeline के लिए।

Latin [`EBNF.md`](EBNF.md) canonical grammar है; यह फ़ाइल उसकी Hindi reader surface है। चलने वाले भाषा-संदर्भ programs public sibling [`../examples/corpus/`](../examples/corpus/) में रहते हैं; उनके साथ वैकल्पिक `+++` frontmatter (`term`, `syntax`, `related` आदि) हो सकता है। Generated manifest [`../examples/corpus/index.toml`](../examples/corpus/index.toml) है। `faber explain` reference pack को disk से load करता है। नए संदर्भ काम के लिए language corpus और यह EBNF साथ पढ़ें।

---

## प्रोग्राम संरचना

Faber source files को lexer चलने से पहले driver raw text के रूप में पढ़ता है। वैकल्पिक TOML frontmatter token grammar का भाग नहीं है।

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

### फ़ाइल frontmatter (`+++`)

Frontmatter होने पर वह **पंक्ति 1** पर ठीक `+++` से खुलना चाहिए। बाद की वह पंक्ति जो trim करने पर ठीक `+++` हो, block बंद करती है। Closing delimiter के बाद के bytes Faber `program` हैं। केवल whitespace वाला body वैध खाली program है।

Frontmatter compiler driver में generic TOML document के रूप में parse होता है, Faber statements के रूप में नहीं। लेखक मनमाने metadata keys रख सकते हैं। Tooling `group`, `sectio` और `[probanda]` जैसे ज्ञात keys accessors से पढ़ता है। Package tooling package keys consume करता है। `[package]`, `[paths]` और `[build]` की authority `faber.toml` ही रहती है; package mode में विरोधी frontmatter values अस्वीकार होती हैं।

उदाहरण:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

आरंभ {}
```

पंक्ति-आरंभ `§` file directives हटा दिए गए हैं। File metadata के लिए `+++` frontmatter रखें। Quoted strings में `§` अभी भी string-template hole है।

---

## घोषणाएँ

### चर

```ebnf
varDecl      := ('स्थिर' | 'चर') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
sitDecl      := 'बैठा' IDENTIFIER ('←' expression)?
arrayDestruct := ('स्थिर' | 'चर') arrayPattern '←' expression
objectDestruct := ('स्थिर' | 'चर') objectPattern '←' expression
```

- `स्थिर` immutable binding है। इसे initializer के बिना घोषित किया जा सकता है, बाद में ठीक एक बार assign किया जा सकता है और फिर frozen रहता है।
- `चर` mutable binding है, जैसे `let`।
- यदि initializer type निर्धारित करता है तो type annotation के लिए `_` लिखें: `स्थिर _ नाम ← मान`।
- `बैठा नाम ← मान`, inferred immutable local `स्थिर _ नाम ← मान` का sugar है।
- `बैठा नाम` बिना initializer का inferred deferred immutable है। किसी read से पहले ठीक एक assignment दें।
- `स्थिर संख्या x` या `बैठा x` uninitialized immutable slot बनाता है। उसे किसी read से पहले ठीक एक बार assign करना होगा; दूसरी assignment अस्वीकार होगी। Definite-assignment pass (semantic Phase 3a) यह नियम लागू करता है।

### फलन

```ebnf
funcDecl     := 'फलन' IDENTIFIER genericParams? '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam  := IDENTIFIER | 'आकार' IDENTIFIER
typeArgs      := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := ('से' | 'में' | 'सेवन')? 'बाकी'? typeAnnotation IDENTIFIER 'स्वेच्छा'? ('रूपमें' IDENTIFIER)? ('या' expression)?
funcModifier := 'तर्क' IDENTIFIER | 'आवंटक' IDENTIFIER ('रूपमें' IDENTIFIER)? | 'त्रुटि' IDENTIFIER | 'निर्गम' (IDENTIFIER | NUMBER) | 'अपरिवर्तित' | 'फेंकता' | 'विकल्प' IDENTIFIER
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := 'अतः'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := 'करो' blockStmt catchClause?
legacyClausuraExpr := 'समापन' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

`→` सामान्य success type घोषित करता है। Body वाले फलन में `→` न हो तो वह effect-only (`रिक्त`) है और उसमें `लौटाओ` नहीं हो सकता। Statement-bodied closure (`करो { ... }` या legacy block body) को `लौटाओ` से पहले `→ T` लिखना होगा। Expression-bodied closure अपना result expression से infer कर सकता है।

`⇥` recoverable alternate-exit channel का type घोषित करता है। यह `→ T` के बाद या अकेले effect-only failable फलन या closure पर आ सकता है। Escaping `इधरफेंको` वाली closure को अपना `⇥ E` घोषित करना होगा; enclosing फलन का error channel closure सीमा पार नहीं करता। Local `करो { ... } पकड़ो त्रुटि { ... }` enclosing `⇥` के बिना भी `इधरफेंको` पकड़ सकता है। `→ T ⇥ E` वाला failable call किसी `⇥`-declaring function के भीतर सीधे alternate exit में propagate होता है; यह Rust `?` में lower होता है। Closure को propagation के लिए अपना `⇥` फिर भी घोषित करना होगा।

- Parameter prefixes `से`, `में`, `सेवन` क्रमशः read, mutate और consume हैं।
- नाम के बाद `स्वेच्छा` voluntary/optional provision marker है।
- `बाकी` rest parameter बताता है।
- `आवंटक NAME ('रूपमें' LOCAL)?` allocator requirement और function-body alias बताता है।
- `अतः` केवल compact **statement-body** joint है: one-statement `यदि`/`जबतक`/`स्थिति` आदि arms के लिए।
- `∴` केवल compact **clausura** joint है। दोनों aliases नहीं हैं।
- Compact closure block body को `करो { ... }` लिखना होगा। Closure-local `करो` body `पकड़ो` ले सकता है, पर postfix `जबतक` नहीं।

### वर्ग

```ebnf
genusDecl    := 'अमूर्त'? 'वर्ग' IDENTIFIER typeParams? ('अधीन' IDENTIFIER)? ('लागू' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := 'स्थैतिक'? 'संबद्ध'? typeAnnotation IDENTIFIER 'स्वेच्छा'? ('=' expression)?
methodDecl   := 'फलन' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
```

### एनोटेशन

```ebnf
annotation            := '@' annotationName annotationBody?
annotationName        := IDENTIFIER | 'सार्वजनिक' | 'संरक्षित' | 'निजी' | 'भविष्य' | 'कर्सर'
                        | 'टैग' | 'केवल' | 'छोड़ो' | 'मापो'
annotationBody        := bracedAnnotation | annotationArgs
bracedAnnotation      := '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)* ','?
annotationField       := IDENTIFIER '=' (expression | typeAnnotation)

cliProgramAnnotation := '@' 'cli' STRING
imperiumAnnotation := '@' 'आज्ञा' STRING
optioAnnotation    := '@' 'विकल्प' IDENTIFIER optioModifier*
optioModifier      := 'लघु' STRING | 'दीर्घ' STRING | 'प्रकार' typeAnnotation
                    | 'विवरण' STRING | 'सर्वत्र' | 'या' expression
operandusAnnotation := '@' 'ऑपरैंड' ('बाकी')? typeAnnotation IDENTIFIER operandusModifier*
operandusModifier  := 'विवरण' STRING | 'सर्वत्र' | 'या' expression
annotationArgs     := (STRING | IDENTIFIER | expression)+

(* एनोटेशन contracts — compile-time metadata schemas *)
annotatioMarker     := '@' 'एनोटेशन' ( '{' annotatioFieldList? '}' )?
annotatioFieldList  := annotatioField (',' annotatioField)* ','?
annotatioField      := 'लक्ष्य' '=' annotatioTarget
annotatioTarget     := 'फलन' | STRING | IDENTIFIER
contractApplication := '@' IDENTIFIER ( '{' annotationFieldList? '}' )?

jsonGenusAnnotation := '@' 'json'
jsonFieldAnnotation := '@' 'json' '{' 'नाम' '=' STRING '}'
```

Braced annotation records और उनके sugar forms एक ही `HirAnnotation` records में lower होते हैं। `@ एनोटेशन` (वैकल्पिक `@ एनोटेशन { लक्ष्य = फलन }`) top-level `वर्ग` को compile-time annotation contract बनाता है। सामान्य genera annotation schemas नहीं हैं। `@ ContractName { field = constant }` local declarations या imported file-interface exports से resolve होकर constant field values वाले `HirAnnotation` में lower होता है। v1 attachment target केवल `फलन` है; payload scalars `पाठ`, `संख्या`, `भिन्न` और `तार्किक` हैं, और `स्वेच्छा` या `T ∪ शून्य` से optional हो सकते हैं। Compiler-owned `@ web`, controller या route families नहीं हैं।

`@ json` compiler-owned data-model contract है, generic annotation schema नहीं। उसके fields JSON-safe होने चाहिए: `पाठ`, `ascii`, `संख्या`, `भिन्न`, `तार्किक`, `क्षण`, `शून्य`, `सूची<T>`, `तालिका<पाठ, T>`, nullable `T ∪ शून्य`, या कोई दूसरा `@ json वर्ग`। `@ json { नाम = "wire_name" }` emitted object key बदलता है। `value ↦ valor`, `value ↦ json` और `json ↦ Genus` में यही key प्रयुक्त होती है। JSON text Norma wire operation जैसे `json.pange(value ↦ json)` से बनता है।

- `@ radix` compiler-owned metadata के लिए आरक्षित है। पुराना morphology-stem अर्थ समाप्त है। मान्य forms top-level functions पर `@ radix lane "air"`, `"mir"` या `"hir-direct"` हैं। Unsupported lane/target combinations diagnostic देते हैं।
- `@ verte` codegen transformation (method name या template) परिभाषित करता है।
- `@ nondum [TARGET] ["REASON"]` interface में declaration मौजूद पर target के लिए unavailable होने का संकेत है।
- `@ cli "NAME"` `आरंभ` entry को CLI program बनाता है।
- `@ आज्ञा "NAME"` function को CLI command entry point बनाता है।
- `@ विकल्प NAME ...` CLI option बनाता है; boolean flag के लिए `प्रकार तार्किक` लिखें।
- `@ ऑपरैंड [बाकी] TYPE NAME ...` CLI positional argument बनाता है।
- `@ भविष्य` function को async और `@ कर्सर` generator बनाता है।
- `@ सार्वजनिक` export सतह चिह्नित करता है, `@ interna` package-internal, और `@ निजी` स्पष्ट module-private मार्कर; बिना चिह्नित शीर्ष-स्तरीय घोषणाएँ डिफ़ॉल्ट रूप से module-private हैं, और अलग-अलग visibility स्तरों का मिश्रण `SEM019` देता है।
- `@ संरक्षित` आरक्षित है और semantic diagnostic देता है।

`अधीन` extends है, `लागू` implements है, `स्थैतिक` static है और `संबद्ध` bound/property है।

### इंटरफ़ेस

```ebnf
implendumDecl   := 'अनुबन्ध' IDENTIFIER typeParams? '{' implendumMethod* '}'
implendumMethod := annotation* 'फलन' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause?
```

`अनुबन्ध` contract construct है। इसमें `लागू` करने वाले genera के लिए केवल signature वाले methods होते हैं। Import namespaces `.fab` file boundaries हैं; exported declarations file top level पर रहती हैं।

### प्रकार उपनाम

```ebnf
typeAliasDecl := 'प्रकार' IDENTIFIER genericParams? '=' typeAnnotation
```

### एनम

```ebnf
enumDecl   := 'क्रम' IDENTIFIER '{' enumMember (',' enumMember)* ','? '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
```

### टैगयुक्त यूनियन

```ebnf
discretioDecl := 'विभेद' IDENTIFIER typeParams? '{' variant (',' variant)* ','? '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER (',' typeAnnotation IDENTIFIER)*)?
```

### पहचानकर्ता नामकरण

Mixed-case lower-initial names syntactically स्वीकार हैं, पर language, stdlib, host routes और compiler-owned intrinsic APIs में Faber एक शब्द पसंद करता है। अर्थ बड़ा हो तो snake_case केवल दुर्लभ मामलों में उपयोग करें। यदि वह भी पर्याप्त न हो, तो method core surface का हिस्सा नहीं होना चाहिए जब तक वह आवश्यक न हो। Stdlib encode/decode के mechanical verbs modules में `pange` / `solve` / `tempta` हैं। Public text library `norma:chorda` है।

### आयात

```ebnf
importDecl     := importRecord | importSugar
importRecord   := 'आयात' '{' importFieldList? '}'
importFieldList := importField (',' importField)* ','?
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := 'सेवन' '=' STRING
importVisibilityField := 'दृश्यता' '=' visibility
importNameField := 'नाम' '=' IDENTIFIER
importAliasField := 'रूपमें' '=' IDENTIFIER
importWildcardField := 'सब' '=' IDENTIFIER

importSugar    := 'आयात' 'सेवन' STRING visibility? (namedImport | wildcardImport)?
visibility    := 'सार्वजनिक'
namedImport   := IDENTIFIER ('रूपमें' IDENTIFIER)?
wildcardImport := '*' 'रूपमें' IDENTIFIER
```

उदाहरण:

```fab
आयात सेवन "hono" Hono
आयात सेवन "hono" Context
आयात सेवन "norma:chorda"
आयात { सेवन = "norma:json/solve", रूपमें = solve_mod }
आयात सेवन "norma:consolum" consolum
आयात सेवन "faber:*" faber
आयात सेवन "lodash" * रूपमें _
आयात सेवन "./types" सार्वजनिक User
```

Import marker `निजी` हटा दिया गया (VM-U3): बिना marker वाला import re-export नहीं करता, और `सार्वजनिक` re-export marker है। Named binding न देने पर import path का अंतिम segment लिया जाता है, यदि वह valid और non-conflicting identifier हो। Invalid या colliding inferred name के लिए explicit `नाम` या `रूपमें` लिखें। `आयात सेवन "faber:*" faber` kernel-विशिष्ट sugar है: glob path string के भीतर है और released binary के kernel manifest को `faber.<module>.<verb>` calls में फैलाता है। यह wildcard re-export नहीं है और runtime aggregate value नहीं बनाता।

---

## प्रकार

```ebnf
typeAnnotation := ('से' | 'में')? baseType ( '∪' typeAnnotation )*
baseType       := holeType | functionType | qualifiedType typeParams? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
typeParams     := genericParams
```

Arrays `सूची<T>` लिखे जाते हैं; postfix `T[]` अस्वीकार है। `से`/`में` ownership prefixes हैं। Inline union `T ∪ U` ad-hoc value union के लिए है; canonical nullable form `T ∪ शून्य` है। Grammar में unions right-associative हैं, पर parser उन्हें flat पढ़ता है; duplicates और केवल `शून्य` वाले cases semantic lowering में diagnostic देते हैं। `स्वेच्छा` declaration marker है, type prefix नहीं। Qualified paths जैसे `terminus.Terminus` imported namespace binding के भीतर type का नाम लेते हैं।

### मूल प्रकार

| Faber | अर्थ |
|---|---|
| `पाठ` | Unicode string |
| `ascii` | ASCII-only string |
| `forma` | captured template और parameters |
| `संख्या` | integer, default `i64` |
| `मॉड्यूल<W>` | unsigned modular word; arithmetic `2^W` modulo wrap करता है |
| `भिन्न` | float, default `f64` |
| `तार्किक` | boolean |
| `शून्य` | null |
| `रिक्त` | void |
| `कभीनहीं` | never |
| `अज्ञात` | unknown |
| `बाइट` | bytes |
| `regex` | compiled pattern |
| `json` | JSON value |
| `मान` | dynamic value carrier |
| `क्षण` | time/instance value |
| `वस्तु` | JSON object |
| `कुछभी` | unconstrained value |

Sized primitives एक optional width marker लेते हैं। `संख्या<W>` के लिए `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`; `भिन्न<W>` के लिए `f16`, `f32`, `f64`; `मॉड्यूल<W>` के लिए केवल unsigned markers मान्य हैं। Bare `संख्या` और `भिन्न` क्रमशः `संख्या<i64>` और `भिन्न<f64>` के shorthand हैं।

`मॉड्यूल<W>` अलग semantic family है। यह `संख्या<W>` से implicit arithmetic mix नहीं करता; explicit same-width conversion उपलब्ध है। Literals `0..=2^W-1` में होने चाहिए। Shift counts modular हैं: `x ⇐ W` पूर्ण wrap है। Cross-width modular arithmetic अस्वीकार है।

### सामान्य संग्रह

| Faber | अर्थ |
|---|---|
| `सूची<T>` | array |
| `तालिका<K,V>` | map |
| `समुच्चय<T>` | set |
| `वादा<T>` | promise |
| `कर्सर<T>` | iterator |
| `tensor<T, Figura>` | static-shape dense homogeneous buffer |
| `vector<T, N>` | static-width register-class numeric vector |
| `matrix<T, [R, C]>` | ठीक दो static dimensions वाली register-class numeric matrix |
| `atomic<T>` | storage-sensitive atomic cell; v1 में केवल `i32`/`u32` |
| `sparsa<T, Figura>` | static-shape sparse homogeneous buffer |

`Figura := _ | natural | ident | [ figura-list ]`; खाली `[]` rank-0 है। Bare `tensor<T>` अधूरा है; rank-0 के लिए `tensor<T, []>` या inferred shape के लिए `tensor<T, _>` लिखें। `vacua` rank-0 tensor के लिए एक default-initialized slot और किसी भी shape के `sparsa` के लिए बिना stored entries वाला all-zero sparse tensor बनाता है। `matrix<T, Figura>` को ठीक दो dimensions चाहिए; bare `matrix<T>`, एक-axis और तीन-axis shapes अस्वीकार हैं। `atomic<T>` में v1 पर `T` केवल `i32` या `u32` हो सकता है; atomic methods के बिना element type से interchange नहीं करें।

Multi-dimensional tensors `crea` / `structa` / `↦` से बनते हैं। `Type(...)` construction form नहीं है; `vector<f32, 4>(...)`, `matrix<f32, [2, 2]>(...)`, `tensor<f32, [2, 2]>(...)` और scalar `संख्या("42")` अस्वीकार हैं। `value ↦ Type`, named library constructors या `Genus { field = value }` records का उपयोग करें। Tensor index/shape intrinsic slots (`accipe`, `ponde`, `forma`, `crea`, `structa`) call sites पर canonical `सूची<संख्या>` / `&[i64]` boundary लेते हैं। यह structural exception केवल उन slots तक सीमित है।

Value unions inline `T ∪ U` हैं। Tagged unions `विभेद` से बनते हैं। `copia.unio()` set method है, type constructor नहीं।

### प्रकार sugar

Type sugar type-position only है और long form से semantically identical है। Width markers `i8`/`i16`/`i32`/`i64`, `u8`/`u16`/`u32`/`u64`, `f16`/`f32`/`f64` हैं। Bare width scalar numeric type देता है; family prefix उसी width की collection देता है।

| Sugar | Long form | Bracket rule |
|---|---|---|
| `i8` … `u64`, `f16`/`f32`/`f64` | `संख्या<W>`, `भिन्न<W>` | कोई bracket नहीं |
| `lf32`, `lu32`, `li64`, … | `सूची<f32>`, `सूची<u32>`, `सूची<i64>`, … | कोई bracket नहीं |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | वैकल्पिक `Figura` |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | वैकल्पिक `Figura` |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 4>`, `vector<u32, 3>` | वैकल्पिक एक width |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **अनिवार्य**, दो dimensions |

`[]` rank-0 है, `[2, 3]` fixed shape है और बिना bracket shape infer होती है (`_`)। Matrix में ठीक दो dimensions चाहिए। Non-width element types के लिए full form लिखें, जैसे `tensor<पाठ, [3]>`। Value identifiers `tf32`, `lf32` आदि type sugar से नहीं बदलते। `मॉड्यूल<W>` का sugar नहीं है; `मॉड्यूल<u32>` पूरा लिखें।

---

## नियंत्रण प्रवाह

### शर्तें

```ebnf
ifStmt     := 'यदि' expression arm ('अन्यथायदि' ifStmt | elseClause)?
elseClause := 'अन्यथा' elseArm
arm        := (blockStmt | stmtBodyJoint statement) catchClause?
elseArm    := (blockStmt | stmtBodyJoint statement) catchClause?
```

`यदि` if, `अन्यथायदि` else-if और `अन्यथा` else हैं। एक-statement body में `अतः लौटाओ`, `अतः इधरफेंको`, `अतः मरोजाओ` और `अतः मौन` मान्य हैं। `मौन` explicit no-op है।

### लूप

```ebnf
whileStmt  := 'जबतक' expression (blockStmt | stmtBodyJoint statement) catchClause?
iteraStmt  := 'दोहराओ' (('सेवन' | 'से') expression | 'पूर्व' expression) ('स्थिर' | 'चर') IDENTIFIER (blockStmt | stmtBodyJoint statement) catchClause?
```

`दोहराओ सेवन...स्थिर/चर` values पर for-of है। `दोहराओ से...स्थिर/चर` keys पर for-in है। Range iteration `दोहराओ पूर्व range स्थिर/चर i` रूप में होती है; उदाहरण `दोहराओ पूर्व 0‥10 प्रति 2 स्थिर i { दिखाओ i }` में `प्रति` range expression का भाग है।

### चयन/मिलान

```ebnf
eligeStmt    := 'चुनो' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase    := 'स्थिति' expression (blockStmt | stmtBodyJoint statement)
defaultCase  := 'अन्यतम' (blockStmt | stmtBodyJoint statement)
```

### पैटर्न मिलान

```ebnf
discerneStmt := 'मिलाओ' 'सब'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase  := 'स्थिति' patterns (blockStmt | stmtBodyJoint statement)
patterns     := pattern ((',' | 'और') pattern)*
pattern      := '_' | literal | (IDENTIFIER patternBind?)
patternBind  := ('रूपमें' IDENTIFIER) | (('स्थिर' | 'चर') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('रूपमें' IDENTIFIER)?
```

### गार्ड

```ebnf
guardStmt   := 'रक्षक' '{' guardClause+ '}'
guardClause := 'यदि' expression (blockStmt | stmtBodyJoint statement)
```

### संसाधन प्रबंधन

```ebnf
curaStmt    := 'देखभाल' STRING ('स्थिर' | 'चर') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### विघटन निष्कर्षण

```ebnf
extractStmt   := 'सेवन' expression ('स्थिर' | 'चर') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField  := IDENTIFIER ('रूपमें' IDENTIFIER)?
restField     := 'बाकी' IDENTIFIER
```

### नियंत्रण स्थानांतरण

```ebnf
returnStmt   := 'लौटाओ' expression?
breakStmt    := 'तोड़ो'
continueStmt := 'जारी'
noopStmt     := 'मौन'
```

---

## त्रुटि प्रबंधन

```ebnf
throwStmt   := ('इधरफेंको' | 'मरोजाओ') expression ['यदि' expression]
catchClause := 'पकड़ो' IDENTIFIER blockStmt
assertStmt  := 'पुष्टि' expression ('मरोजाओ' expression)?
requiritStmt := 'आवश्यक' expression 'इधरफेंको' expression
```

`पकड़ो` structured statements और conditional arms से जुड़ता है। यह मनमाने bare block से नहीं जुड़ता। `करो { ... } पकड़ो त्रुटि { ... }` canonical one-shot local recoverable-error boundary है। `प्रयास` legacy try/catch surface है और `अंततः` legacy finally surface; दोनों migration diagnostic के साथ अस्वीकार हैं। `इधरफेंको` recoverable throw है और `मरोजाओ` fatal panic। Optional `यदि <expr>` guard parser sugar है: `इधरफेंको मान यदि शर्त` parse समय पर `यदि शर्त { इधरफेंको मान }` बनता है। `पुष्टि` रनटाइम इनवेरिएंट चेक है; यह संकल्पात्मक रूप से `मरोजाओ "msg" यदि !cond` में अवमूदन होता है, स्रोत में सकारात्मक स्थिति रखते हुए। `secus` गलत-पथ संदेश पेश करता है।

---

## अभिव्यक्तियाँ

### ऑपरेटर — precedence में निम्न से उच्च

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
ternary    := or (('?' expression ':' | 'ऐसा' expression 'अन्यथा') ternary)?
or         := and (('या') and)*
and        := equality (('और') equality)*
equality   := comparison (('≡' | '≠' | '≈' | '≉' | 'है' | 'नहीं' 'है') comparison)*
comparison := bitwiseOr (('<' | '>' | '≤' | '≥' | 'भीतर' | 'बीच') bitwiseOr)*
bitwiseOr  := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift      := range (('⇐' | '⇒') range)*
range      := additive (('‥' | '…' | 'पहले' | 'तक') additive ('प्रति' additive)?)?
additive   := multiplicative (('+' | '-') multiplicative)*
multiplicative := coalesce (('*' | '/' | '%') coalesce)*
coalesce   := unary ('डिफ़ॉल्ट' velRhs)*
velRhs     := unary (('‥' | '…' | 'पहले' | 'तक') unary ('प्रति' unary)?)?
unary      := ('-' | '¬' | 'नहीं' | 'आगेबढ़ो' | 'गढ़ो') unary | cast
cast       := call ('∷' typeAnnotation | conversio)*
conversio        := '↦' typeAnnotation typeParams? inlineRecovery?
inlineRecovery   := '⇥' unary
```

`↤` परिवर्तन-निर्देशित असाइनमेंट है: दाएँ पक्ष का मूल्यांकन करें, उसे `↦` मार्ग से बाएँ स्थान के स्थिर प्रकार में परिवर्तित करें, फिर असाइन करें। `⇥` रिकवरी केवल `↤` पर मान्य है, `←` के बाद नहीं।

`डिफ़ॉल्ट` local nullable elimination है: `T ∪ शून्य डिफ़ॉल्ट T → T`। यह logical `या` नहीं है। इसका binding arithmetic से tighter है; `prefix + item डिफ़ॉल्ट ""` का अर्थ `prefix + (item डिफ़ॉल्ट "")` है। RHS interval constructor पूरा कर सकता है। Retired predicate keywords prefix unary syntax नहीं हैं। `expr है सत्य`, `expr है असत्य`, `expr है शून्य`, `expr नहीं है शून्य`, `expr < 0` या `expr > 0` लिखें।

**Static type ascription (`∷` / verte):** `∷` glyph expression पर target type का compile-time ascription देता है। Primitive/alias पर runtime effect नहीं होता; built-in collection पर target-shaped collection value मिलता है; variant expression पर enum/interface target ascription मिलता है। साधारण genus values के लिए typed construction और empty collections के लिए `vacua` पसंद करें। केवल glyph `∷` स्वीकार है; पुराने Latin aliases `qua`, `innatum` और `novum` हट चुके हैं।

**Runtime conversion (`↦` / conversio):** `↦` actual parsing/conversion करता है और fail हो सकता है। उदाहरण: `"22" ↦ संख्या`, `"bad" ↦ संख्या ⇥ 0`, `42 ↦ पाठ`। Inline recovery में `⇥` target के तुरंत बाद आता है और recovery expression का type target type से मिलना चाहिए। `डिफ़ॉल्ट` conversio recovery के लिए अस्वीकार है; यह केवल local nullable elimination है।

### कॉल और सदस्य अभिगम

```ebnf
call          := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix    := typeArgs? '(' argumentList ')'
memberSuffix  := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList  := (argument (',' argument)*)?
argument      := 'फैलाओ'? expression
```

### स्ट्रिंग और टेम्पलेट शाब्दिक

Faber delimiter semantics उपयोग करता है; हर quote form अलग source shape बताता है। ये interchangeable synonyms नहीं हैं।

| Form | Type | भूमिका |
|---|---|---|
| `'...'` | `ascii` | fixed machine tokens; `§` और `(...)` नहीं |
| `"..."` | `पाठ` | short Unicode line string; `(...)` render करता है |
| `«...»` | `पाठ` | block/multiline Unicode; `(...)` render करता है |
| `` `...` `` | `forma` | captured template; `(...)` capture करता है |
| `{ ... }` | `json` | compile-time object-rooted JSON document |
| `\|...\|` | `बाइट` | compile-time hex bytes |
| `"..." ↦ regex` | `regex` | text से compiled pattern |
| `[ ... ]` | `सूची<T>` | Faber list, JSON array या bytes नहीं |

Unicode forms (`"`, `«`, `` ` ``) में `§` template hole है। `ascii` literals में `§` नहीं आ सकता। Rendered `पाठ` templates `scriptum("...", args...)` में lower होते हैं। Captured `forma` templates template text और parameters को render किए बिना capture करते हैं; bound SQL/URL payload के लिए इन्हें उपयोग करें। Block text के लिए `«...»` है।

Implementation status: `"..."`, `«...»`, `'...'`, `` `...` ``, `|...|`, `{ ... }`, और text/ascii `↦ regex` shipped हैं। Slash-delimited `/.../` regex literals अभी pending हैं।

उदाहरण:

```fab
स्थिर _ टैग ← «inline»
स्थिर _ blob ← «
    select id, email
    from accounts
»
स्थिर _ प्रश्न ← `select * from accounts where id = §`(accountId)
स्थिर _ हस्ताक्षर ← |de ad be ef|
स्थिर _ नमस्ते ← |48 65 6c 6c 6f|
```

### फ़ॉर्मैट-टेम्पलेट अनुप्रयोग

String literal call syntax canonical source form है:

```fab
"स्थिति: § (§)"(sample_status(), "ठीक")
"स्थिति: §1 (§0)"("ठीक", sample_status())
```

यह compiler के `scriptum("...", args...)` form में lower होता है। सामान्य source में string-template form रखें; `scriptum(...)` केवल explicit desugaring और compiler-facing documentation के लिए है।

`पाठ` bracket indexing Unicode-scalar आधारित है:

```fab
"Salve, §!"[7]
"hello world"[0‥5]
"hello world"[0 तक 10]
"abcdef"[0‥6 प्रति 2]
```

Text slices में `प्रति` सहित full range form मान्य है। `सूची<T>` bracket access एक element देता है; index एक integer होना चाहिए; out-of-bounds पर trap होता है। Copied range के लिए `sectio(start, end)` उपयोग करें। Nullable list access के लिए `xs.accipe(i) → T ∪ शून्य` और `डिफ़ॉल्ट` उपयोग करें।

`tensor<T, Figura>` bracket indexing intrinsic surface का sugar है:

```fab
vector[id]        # vector.accipe([id])
vector[id] ← v    # vector.ponde([id], v)
grid[[r, c]]      # grid.accipe([r, c])
grid[[r, c]] ← v  # grid.ponde([r, c], v)
```

Reads `T ∪ शून्य` लौटाते हैं। Rank-1 tensor scalar integer index लेता है; rank-N tensor list-shaped index expression लेता है। `grid[r, c]` syntax नहीं है।

`बाइट` byte-buffer primitive है, array नहीं। Bracket indexing स्वीकार नहीं है; byte access method-based है:

```fab
buf.accipe(i)      # → संख्या<u8> ∪ शून्य
buf.appende(b)     # एक byte in place जोड़ें
buf.longitudo      # byte length
```

### प्राथमिक अभिव्यक्तियाँ

`vacua` contextual empty-collection marker है, reserved keyword नहीं। Explicit collection type दें।

```ebnf
primary := IDENTIFIER | NUMBER | STRING | ASCII_STRING | BACKTICK_STRING
         | 'मैं' | 'सत्य' | 'असत्य' | 'शून्य'
         | 'vacua' | arrayLiteral | jsonLiteral | typedConstructor
         | adExpr
         | '(' expression ')'
adExpr    := 'सेवा' asciiLiteral adOpener?
arrayLiteral := '[' argumentList? ']'
# Bare { ... } JSON document literal है। Keys quoted JSON strings हैं और : से जुड़ते हैं।
jsonLiteral := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonMember  := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('फैलाओ' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
# JSON values केवल constants हैं; Faber expressions या variable references नहीं।
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonArray  := '[' (jsonValue (',' jsonValue)* ','?)? ']'
jsonString := STRING
jsonNumber := NUMBER
```

Bare `{ ... }` object-rooted JSON document (`json`) बनाता है। Keys quoted JSON strings हैं और duplicate key error है। `↦ मान` broad dynamic carrier में explicit widening देता है। `Type { field = expr }` Faber typed construction है।

### विशेष अभिव्यक्तियाँ

```ebnf
// verte (∷) cast production में postfix है
fingeExpr     := 'गढ़ो' IDENTIFIER ('{' fieldList '}')? ('∷' IDENTIFIER)?
praefixumExpr := 'उपसर्ग' (blockStmt | '(' expression ')')
formatStringExpr := STRING '(' argumentList ')'      # scriptum से पाठ render करता है
formaTemplateExpr := BACKTICK_STRING '(' argumentList ')' # forma capture करता है
scriptumExpr  := 'लिखित' '(' STRING (',' expression)* ')'   # explicit/desugared form
legeExpr      := 'पढ़ो' 'पंक्ति'?
regexFromText := (STRING | ASCII_STRING) '↦' 'regex'
# Slash-delimited regex literals अभी active grammar नहीं हैं। / division operator है।
```

---

## पैटर्न

```ebnf
objectPattern  := '{' patternProperty (',' patternProperty)* '}'
patternProperty := 'बाकी'? IDENTIFIER ('रूपमें' IDENTIFIER)?
arrayPattern   := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | 'बाकी'? IDENTIFIER
```

---

## नैदानिकी

```ebnf
outputStmt := ('दिखाओ' | 'देखो' | 'चेताओ' | 'लिखो') expression (',' expression)*
```

`दिखाओ` neutral diagnostic note है, `देखो` debug/inspect है, `चेताओ` warning है और `लिखो` diagnostic channel spelling है। वास्तविक output के लिए current stdlib methods उपयोग करें।

### टिप्पणियाँ

Faber केवल line comments स्वीकार करता है: logical line के अंत तक `#`। `#` logical line का पहला non-whitespace token होना चाहिए; केवल leading ASCII spaces या tabs skip होते हैं। किसी अन्य token के बाद `#` lex error है। Valid line-start comments अगले statement/declaration पर `leading_trivia` की तरह attach होते हैं। String, `ascii`, `forma` और दूसरे delimited literals में `#` comment नहीं है।

---

## प्रवेश बिंदु

```ebnf
incipitStmt  := 'आरंभ' blockStmt
incipietStmt := 'आरंभasync' blockStmt
```

`आरंभ` sync entry है और `आरंभasync` async entry है।

---

## परीक्षण

```ebnf
probandumDecl := 'परीक्षणसमूह' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt     := 'परीक्षण' STRING probaModifier* blockStmt
probaModifier := 'छोड़ो' STRING | 'भविष्य' STRING | 'केवल' | 'टैग' STRING
              | 'समय' NUMBER | 'मापो' | 'दोहराओ' NUMBER | 'नाज़ुक' NUMBER
              | 'आवश्यक' STRING | 'केवलमें' STRING
praeparaBlock := ('पूर्वतैयार' | 'पूर्वतैयारasync' | 'पश्चतैयार' | 'पश्चतैयारasync') 'सब'? blockStmt
```

---

## CLI ढाँचा

```ebnf
cliDecl       := annotation* (incipitStmt | funcDecl)
cliAnnotation := cliProgramAnnotation | imperiumAnnotation | optioAnnotation | operandusAnnotation
```

Faber automatic argument parsing और help generation के साथ CLI applications बना सकता है।

### CLI प्रवेश बिंदु

```fab
@ cli "faber"
@ विकल्प verbose दीर्घ "verbose" प्रकार तार्किक
आरंभ तर्क args {
    # CLI ढाँचा arguments अपने-आप parse करता है
}
```

### CLI विकल्प और तर्क

```fab
@ आज्ञा "deploy"
@ विकल्प target लघु "t" दीर्घ "target" प्रकार पाठ विवरण "Deployment target"
@ विकल्प verbose लघु "v" दीर्घ "verbose" प्रकार तार्किक विवरण "Enable verbose output"
@ ऑपरैंड पाठ file विवरण "File to deploy"
फलन deploy() तर्क args {
    # Arguments अपने-आप parse होकर pass होते हैं
}
```

---

## क्षमता कॉल

Expression-form `सेवा` ही समर्थित `ad` surface है। Legacy typed `ad "route" (args) → T { }` और statement-level stream blocks (`meus`/`tuus`) parse time पर अस्वीकार हैं।

```ebnf
adExpr        := 'सेवा' asciiLiteral adOpener?
adOpener      := '(' expression ')'
conversioExpr := expr '↦' typeAnnotation
```

Route `asciiLiteral` है, double-quoted `STRING` नहीं। Opener optional single expression है और request `data` को `valor` बनाता है। Expression `सेवा` blockless है और `sermo` conversation handle देता है। `s.meus<T>()` outbound `da`/`fini` view है। `s.tuus<T>()` inbound `accipe`/`cursor`/`exhauri`/`fini` view है। Inbound frames पर `s.tuus<T>().cursor()` iterate करें; सीधे `दोहराओ सेवन s.tuus<T>()` न लिखें। `sermo ↦ T` inbound frames को `T` के type-directed collector से एक value में materialize करता है। Compiler-owned types `scrinium`, `status` और opaque `sermo` handle हैं।

---

## संग्रह संचालन

पुराना `ab` collection-pipeline DSL retired है। Filtering, slicing और aggregation ordinary `पाठ`/`सूची`/`तालिका`/`समुच्चय` methods और closures से व्यक्त होते हैं। `textus`, `संख्या`, `भिन्न`, `सूची<T>`, `तालिका<K,V>` और `समुच्चय<T>` compiler-owned core types हैं; उनके method surfaces अलग intrinsic design documents में tracked हैं। `prima` और `ultima` ordinary method names हैं। `ubi` active collection syntax नहीं है। `सेवन` iteration और imports दोनों में प्रयुक्त होता है।

---

## Fac ब्लॉक

```ebnf
facBlockStmt := 'करो' blockStmt catchClause? ('जबतक' expression)?
```

`करो { ... }` scoped block को एक बार चलाता है। `करो { ... } पकड़ो err { ... }` canonical local recoverable-error boundary है। `करो { ... } जबतक condition` post-test loop form है। Postfix `जबतक` केवल `करो` पर attach होता है, किसी भी पिछले block पर नहीं।

---

## लक्ष्य समर्थन

लक्ष्य समर्थन grammar का भाग नहीं है। कौन-सा grammar किस compilation target पर lower होता है और runtime policy क्या है, इसके लिए `EBNF_MATRIX.md` और target-capability matrix पढ़ें।

---

## कीवर्ड संदर्भ

| श्रेणी | Faber | अर्थ |
|---|---|---|
| घोषणाएँ | `विभेद` | tagged union |
|  | `स्थिर` | const |
|  | `फलन` | function |
|  | `वर्ग` | class |
|  | `अनुबन्ध` | interface contract |
|  | `आकार` | size/index generic parameter |
|  | `क्रम` | enum |
|  | `बैठा` | inferred immutable local |
|  | `स्वेच्छा` | optional declaration slot |
|  | `प्रकार` | type alias |
|  | `vacua` | contextual empty collection marker |
|  | `चर` | let |
| Control flow | `यदि` / `अन्यथायदि` / `अन्यथा` | if / else-if / else |
|  | `रक्षक` | guard |
|  | `मिलाओ` | pattern match |
|  | `जबतक` | while |
|  | `चुनो` / `स्थिति` | switch / case |
|  | `करो` | scoped block / local error boundary |
|  | `दोहराओ सेवन...स्थिर` | for-of values |
|  | `दोहराओ से...स्थिर` | for-in keys |
|  | `दोहराओ पूर्व...स्थिर` | range iteration |
|  | `जारी` | continue |
|  | `लौटाओ` | return |
|  | `तोड़ो` | break |
|  | `मौन` | no-op |
|  | `अतः` | compact one-statement body joint |
|  | `∴` | compact clausura joint only |
| Error handling | `पकड़ो` | structured local handler |
|  | `पुष्टि` | assert |
|  | `आवश्यक` | require (recoverable) |
|  | `इधरफेंको` | recoverable throw |
|  | `फेंकता` | throws modifier |
|  | `मरोजाओ` | panic |
| Async | `@ भविष्य` | async annotation |
|  | `@ कर्सर` | generator annotation |
|  | `आगेबढ़ो` | await/yield by context |
| Endpoints | `सेवा` | capability call expression |
|  | `उत्सर्जित` | retired statement-level frame emit |
| Boolean | `सत्य` | true |
|  | `या` | or |
|  | `और` | and |
|  | `असत्य` | false |
|  | `नहीं` | not |
|  | `डिफ़ॉल्ट` | local nullable defaulting |
| Objects | `मैं` | this/self |
|  | `गढ़ो` | construct variant |
| Type shape | `∷` | static type ascription / compile-time cast |
| Type conversion | `↦ target` | runtime value conversion |
| Bitwise | `∧` / `∨` / `⊻` / `¬` | and/or/xor/not |
|  | `⇐` / `⇒` | left/right shift |
| Diagnostics | `दिखाओ` | neutral note |
|  | `चेताओ` | warn |
|  | `लिखो` | diagnostic channel |
|  | `देखो` | debug/inspect |

---

## महत्वपूर्ण syntax नियम

1. **Type-first parameters:** `फलन f(संख्या x)`, `फलन f(x: संख्या)` नहीं।
2. **Type-first declarations:** `स्थिर पाठ नाम`, `स्थिर नाम: पाठ` नहीं।
3. **Iteration loops:** `दोहराओ सेवन/से collection स्थिर/चर item { }` या `दोहराओ पूर्व range स्थिर/चर item { }`।
4. Conditions के चारों ओर parentheses मान्य हैं, पर idiomatic नहीं; `यदि x > 0 { }` या `यदि flag है सत्य { }` लिखें।
5. Diagnostic keywords statements हैं, functions नहीं। `दिखाओ x` और grouped expression `दिखाओ(x)` parse होते हैं, पर `दिखाओ` callable value नहीं है।

---

## रीडर पैक शब्दावली (मशीन निष्कर्षण)

### कीवर्ड

| Latin | Localized |
|---|---|
| discretio | विभेद |
| fixum | स्थिर |
| functio | फलन |
| genus | वर्ग |
| implendum | अनुबन्ध |
| importa | आयात |
| modulus | मॉड्यूल |
| ordo | क्रम |
| sit | बैठा |
| typus | प्रकार |
| varia | चर |
| abstractus | अमूर्त |
| ceteri | बाकी |
| curata | आवंटक |
| errata | त्रुटि |
| exitus | निर्गम |
| generis | स्थैतिक |
| iacit | फेंकता |
| immutata | अपरिवर्तित |
| interna | आंतरिक |
| magnitudo | आकार |
| nexum | संबद्ध |
| optiones | विकल्प |
| prae | पूर्व |
| privata | निजी |
| protecta | संरक्षित |
| publica | सार्वजनिक |
| sponte | स्वेच्छा |
| casu | स्थिति |
| ceterum | अन्यतम |
| custodi | रक्षक |
| discerne | मिलाओ |
| dum | जबतक |
| elige | चुनो |
| ergo | अतः |
| fac | करो |
| itera | दोहराओ |
| secus | अन्यथा |
| si | यदि |
| sic | ऐसा |
| sin | अन्यथायदि |
| perge | जारी |
| redde | लौटाओ |
| rumpe | तोड़ो |
| tacet | मौन |
| adfirma | पुष्टि |
| cape | पकड़ो |
| cede | आगेबढ़ो |
| iace | इधरफेंको |
| mori | मरोजाओ |
| clausura | समापन |
| falsum | असत्य |
| nihil | शून्य |
| verum | सत्य |
| aut | या |
| est | है |
| et | और |
| non | नहीं |
| vel | डिफ़ॉल्ट |
| ego | मैं |
| finge | गढ़ो |
| implet | लागूकरता |
| sub | अधीन |
| mone | चेताओ |
| nota | दिखाओ |
| scribe | लिखो |
| vide | देखो |
| argumenta | तर्क |
| cura | देखभाल |
| incipiet | आरंभasync |
| incipit | आरंभ |
| ad | सेवा |
| de | से |
| ex | सेवन |
| in | में |
| lege | पढ़ो |
| lineam | पंक्ति |
| omnia | सब |
| praefixum | उपसर्ग |
| scriptum | लिखित |
| sparge | फैलाओ |
| ut | रूपमें |
| ante | पहले |
| inter | बीच |
| intra | भीतर |
| per | प्रति |
| usque | तक |
| fragilis | नाज़ुक |
| futurum | भविष्य |
| metior | मापो |
| omitte | छोड़ो |
| postpara | पश्चतैयार |
| postparabit | पश्चतैयारasync |
| praepara | पूर्वतैयार |
| praeparabit | पूर्वतैयारasync |
| proba | परीक्षण |
| probandum | परीक्षणसमूह |
| repete | पुनरावृत्ति |
| requirit | आवश्यक |
| solum | केवल |
| solum_in | केवलमें |
| tag | टैग |
| temporis | समय |
| negativum | ऋणात्मक |
| nonnihil | अशून्य |
| nonnulla | कुछ |
| nulla | शून्यवत् |
| positivum | धनात्मक |

### प्रकार

| Latin | Localized |
|---|---|
| ascii | ascii |
| textus | पाठ |
| numerus | संख्या |
| modulus | मॉड्यूल |
| fractus | भिन्न |
| bivalens | तार्किक |
| nihil | शून्य |
| vacuum | रिक्त |
| numquam | कभीनहीं |
| ignotum | अज्ञात |
| octeti | बाइट |
| regex | regex |
| json | json |
| valor | मान |
| instans | क्षण |
| objectum | वस्तु |
| quidlibet | कुछभी |
| lista | सूची |
| tabula | तालिका |
| copia | समुच्चय |
| promissum | वादा |
| cursor | कर्सर |

### pass1 / पैक की तुलना में शब्दावली परिवर्तन

| Latin | Pass1 / pack | New (this EBNF) | Why |
|---|---|---|---|
| genus | `वर्ग` / pack `प्रकार` | `वर्ग` | `प्रकार` को `typus` के लिए सुरक्षित रखकर reverse-map collision हटाया। |
| implendum | `भरना` | `अनुबन्ध` | Interface contract के लिए स्पष्ट, स्वतंत्र single-token Hindi surface। |
| ceterum | `डिफ़ॉल्ट` | `अन्यतम` | `default` को `ceterum` के लिए अलग किया; `vel` के local defaulting surface से भी collision हटाया। |
| vel | `या_डिफ़ॉल्ट` | `डिफ़ॉल्ट` | Keyword identifier में underscore हटाया और semantic भूमिका स्पष्ट की। |
| de | `से` | `से` | Ownership/import context में pass1 का अच्छा surface रखा। |
| ex | `बाहर` | `सेवन` | `de`/`ex` के reverse-map collision और extraction/iteration ambiguity से बचने के लिए अलग surface। |
| cede | `त्यागो` | `आगेबढ़ो` | await/yield अर्थ को discard अर्थ से अलग किया। |
| repete | `पुनरावृत्त` | `पुनरावृत्ति` | Testing modifier में noun-like repetition अर्थ को `itera` के `दोहराओ` से अलग रखा। |
| curata | `साफ` | `आवंटक` | Allocator requirement का technical अर्थ सीधे बताता है। |
| iacit | `फेंकता` | `फेंकता` | throws modifier के लिए स्थिर surface रखा। |
| अन्य canonical keys | आंशिक/मिश्रित | पूर्ण 111 keyword और 22 type mappings | Source grammar के सभी sections और machine extraction के लिए completeness। |

`ergo` का localized surface `अतः` है। `∴` clausura glyph है और हर जगह अपरिवर्तित रखा गया है। Glyphs `← → ∴ ≡ ∪ ⇥ ‥ … ≤ ≥ ≠ ≈ ∷ ↦` को localize नहीं किया गया है।
