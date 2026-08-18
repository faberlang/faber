# ข้อกำหนดภาษา Faber

> **Reader-locale EBNF (Thai).** Latin/source-of-truth grammar remains [`EBNF.md`](EBNF.md).
> This file is the Thai reader surface of that grammar (keywords, commentary, examples).
> Pack keyword/type spellings are extracted from the glossary appendix at the end.
> Glyphs (`← → ∴ ≡ ∪ ⇥` …) never localize; `ergo` localizes, `∴` is clausura-only.


ไวยากรณ์อย่างเป็นทางการของภาษาโปรแกรม Faber ไฟล์นี้เป็นพื้นผิวอ้างอิงหลักของไวยากรณ์และคำอธิบายประกอบภาษา ตัวอย่างที่รันได้อยู่ในคลังตัวอย่างสาธารณะ

## โครงสร้างโปรแกรม

ไฟล์ต้นฉบับ Faber เป็นข้อความดิบที่ตัวขับจะลอก frontmatter ก่อนการวิเคราะห์คำ โทเค็น TOML ไม่อยู่ในไวยากรณ์โทเค็นของภาษา

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

### Frontmatter ของไฟล์ (`+++`)

ถ้ามี frontmatter ต้องเปิดที่ **บรรทัด 1** ด้วย `+++` ที่ตรงกันทุกตัว บรรทัดภายหลังซึ่งตัดช่องว่างแล้วเหลือ `+++` จะปิดบล็อก ไบต์หลังตัวปิดคือ `program` ของ Faber เนื้อหาว่างหรือมีเพียงช่องว่างถือเป็นโปรแกรมว่างที่ถูกต้อง

ตัวขับคอมไพเลอร์อ่าน frontmatter เป็นเอกสาร TOML ทั่วไป ไม่ใช่คำสั่ง Faber ผู้เขียนใส่คีย์เมทาดาทาได้ตามต้องการ เครื่องมืออ่านคีย์ที่รู้จัก เช่น `group`, `sectio` และ `[probanda]` เครื่องมือแพ็กเกจ `faber` อ่านคีย์แพ็กเกจจากส่วนนี้ แต่ `[package]`, `[paths]` และ `[build]` ยังคงอยู่ใน `faber.toml`; ถ้าค่าใน frontmatter ขัดแย้งในโหมดแพ็กเกจ ระบบจะปฏิเสธ

ตัวอย่าง:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

เริ่ม {}
```

คำสั่งไฟล์ที่เริ่มบรรทัดด้วย `§` ถูกนำออกแล้ว ให้ใส่เมทาดาทาไฟล์ใน frontmatter `+++` แทน ภายในสตริงที่มีเครื่องหมายคำพูด `§` ยังคงเป็นช่องของเทมเพลตสตริง

---

## การประกาศ

### ตัวแปร

```ebnf
varDecl      := ('คงที่' | 'แปร') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
sitDecl      := 'อนุมานคงที่' IDENTIFIER ('←' expression)?
arrayDestruct := ('คงที่' | 'แปร') arrayPattern '←' expression
objectDestruct := ('คงที่' | 'แปร') objectPattern '←' expression
```

- `คงที่` คือการผูกแบบแก้ไม่ได้ เขียนได้ครั้งเดียว; `แปร` คือการผูกที่กำหนดค่าใหม่ได้
- ใช้ `_` เป็นคำอธิบายชนิดเมื่อชนิดมาจากค่าเริ่มต้น: `คงที่ _ name ← value`
- `อนุมานคงที่ name ← value` เป็นรูปย่อของ `คงที่ _ name ← value`
- `อนุมานคงที่ name` โดยไม่มีค่าเริ่มต้นเป็นช่องคงที่ที่อนุมานชนิดภายหลัง ต้องกำหนดก่อนอ่าน
- การเริ่มต้นแบบเลื่อนกำหนดค่าได้เพียงครั้งเดียว ระบบตรวจการกำหนดค่าครบถ้วนใน semantic Phase 3a

### ฟังก์ชัน

```ebnf
funcDecl     := 'ฟังก์ชัน' IDENTIFIER genericParams? '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam := IDENTIFIER | 'ขนาด' IDENTIFIER
typeArgs     := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := ('จาก' | 'ใน' | 'ออก')? 'ที่เหลือ'? typeAnnotation IDENTIFIER 'สมัครใจ'? ('ในชื่อ' IDENTIFIER)? ('หรือ' expression)?
funcModifier := 'อาร์กิวเมนต์' IDENTIFIER | 'จัดการ' IDENTIFIER ('ในชื่อ' IDENTIFIER)? | 'ข้อผิดพลาด' IDENTIFIER | 'ทางออก' (IDENTIFIER | NUMBER) | 'ไม่เปลี่ยนแปลง' | 'โยนผล' | 'ตัวเลือก' IDENTIFIER
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := 'ดังนั้น'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := 'ทำ' blockStmt catchClause?
legacyClausuraExpr := 'ปิดล้อม' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

`→` ประกาศชนิดผลสำเร็จปกติ ฟังก์ชันที่มีบอดีแต่ไม่มี `→` เป็นฟังก์ชันผลข้างเคียง (`ว่างเปล่า`) และห้ามมี `คืน` บอดีแบบคำสั่งต้องเขียน `→ T` ก่อนใช้ `คืน`; บอดีแบบนิพจน์อนุมานชนิดผลได้

`⇥` ประกาศชนิดของช่องทางข้อผิดพลาด ฟังก์ชันหรือ closure ที่ใช้ `โยน` ออกจากขอบเขตต้องประกาศ `⇥ E` ของตนเอง ขอบเขต `ทำ { ... } จับ err { ... }` จับ `โยน` ในพื้นที่ได้โดยไม่ต้องมี `⇥` ภายนอก

คำนำหน้าพารามิเตอร์คือ `จาก` (อ่าน), `ใน` (แก้ไข), `ออก` (ใช้หมด); `สมัครใจ` เป็นตัวทำเครื่องหมายหลังชื่อ; `ที่เหลือ` ทำเครื่องหมายพารามิเตอร์ส่วนที่เหลือ; `จัดการ` ประกาศความต้องการตัวจัดสรร; `ดังนั้น` ใช้เฉพาะบอดีคำสั่งหนึ่งคำสั่ง และ `∴` ใช้เฉพาะข้อต่อ clausura เท่านั้น

### คลาส

```ebnf
genusDecl    := 'นามธรรม'? 'ชนิด' IDENTIFIER typeParams? ('สืบทอด' IDENTIFIER)? ('เติมเต็ม' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := 'ของชนิด'? 'ผูก'? typeAnnotation IDENTIFIER 'สมัครใจ'? ('=' expression)?
methodDecl   := 'ฟังก์ชัน' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
```

### คำกำกับ

```ebnf
annotation            := '@' annotationName annotationBody?
annotationName        := IDENTIFIER | 'สาธารณะ' | 'ป้องกัน' | 'ส่วนตัว' | 'อนาคต' | 'ตัวชี้'
                        | 'แท็ก' | 'เฉพาะ' | 'ละเว้น' | 'วัด'
annotationBody        := bracedAnnotation | annotationArgs
bracedAnnotation      := '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)* ','?
annotationField       := IDENTIFIER '=' (expression | typeAnnotation)
cliProgramAnnotation := '@' 'cli' STRING
imperiumAnnotation := '@' 'คำสั่ง' STRING
optioAnnotation    := '@' 'ตัวเลือก' IDENTIFIER optioModifier*
optioModifier      := 'สั้น' STRING | 'ยาว' STRING | 'ชนิด' typeAnnotation
                    | 'คำอธิบาย' STRING | 'ทั่วถึง' | 'หรือ' expression
operandusAnnotation := '@' 'ตัวถูกดำเนินการ' ('ที่เหลือ')? typeAnnotation IDENTIFIER operandusModifier*
operandusModifier  := 'คำอธิบาย' STRING | 'ทั่วถึง' | 'หรือ' expression
annotationArgs     := (STRING | IDENTIFIER | expression)+
annotatioMarker     := '@' 'annotation' ( '{' annotatioFieldList? '}' )?
annotatioFieldList  := annotatioField (',' annotatioField)* ','?
annotatioField      := 'เป้าหมาย' '=' annotatioTarget
annotatioTarget     := 'ฟังก์ชัน' | STRING | IDENTIFIER
contractApplication := '@' IDENTIFIER ( '{' annotationFieldList? '}' )?
jsonGenusAnnotation := '@' 'json'
jsonFieldAnnotation := '@' 'json' '{' 'ชื่อ' '=' STRING '}'
```

`@ annotation` ใช้ทำเครื่องหมาย `ชนิด` ระดับบนสุดเป็นสัญญา annotation ตอนคอมไพล์ สัญญา JSON ใช้ `@ json` กับ `ชนิด` และต้องมีฟิลด์ที่ปลอดภัยต่อ JSON

คำอธิบาย annotation ที่สำคัญ: `@ radix` สงวนไว้สำหรับเมทาดาทาคอมไพเลอร์, `@ verte` กำหนดการแปลงโค้ด, `@ nondum` ทำเครื่องหมายการประกาศที่ยังใช้ไม่ได้กับเป้าหมาย, `@ cli` ทำเครื่องหมายรายการ `เริ่ม` เป็นโปรแกรม CLI, `@ คำสั่ง` ทำเครื่องหมายจุดเข้า CLI, `@ ตัวเลือก` สร้างออปชัน, `@ ตัวถูกดำเนินการ` สร้างอาร์กิวเมนต์ตำแหน่ง, `@ อนาคต` ทำเครื่องหมาย async และ `@ ตัวชี้` ทำเครื่องหมาย generator

`สืบทอด` หมายถึง extends, `เติมเต็ม` หมายถึง implements, `ของชนิด` หมายถึง static และ `ผูก` หมายถึง bound/property

### อินเทอร์เฟซ

```ebnf
implendumDecl   := 'สัญญา' IDENTIFIER typeParams? '{' implendumMethod* '}'
implendumMethod := annotation* 'ฟังก์ชัน' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause?
```

`สัญญา` คือโครงสร้าง contract: เมธอดมีเฉพาะลายเซ็นและต้องถูกเติมเต็มด้วย `เติมเต็ม` ขอบเขตนำเข้าคือขอบเขตไฟล์ `.fab`; การประกาศที่ส่งออกอยู่ระดับบนสุดของไฟล์

### นามแฝงชนิด

```ebnf
typeAliasDecl := 'ชนิดนามแฝง' IDENTIFIER genericParams? '=' typeAnnotation
```

### ลำดับค่า

```ebnf
enumDecl   := 'ลำดับ' IDENTIFIER '{' enumMember (',' enumMember)* ','? '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
```

### สหภาพแบบติดป้าย

```ebnf
discretioDecl := 'สหภาพแยก' IDENTIFIER typeParams? '{' variant (',' variant)* ','? '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER (',' typeAnnotation IDENTIFIER)*)?
```

### การตั้งชื่อ Identifier

ชื่อแบบผสมตัวพิมพ์เล็กขึ้นต้นยอมรับทางไวยากรณ์ แต่ไม่ใช่รูปแบบที่แนะนำสำหรับภาษา ไลบรารีมาตรฐาน เส้นทางโฮสต์ หรือ API intrinsic ที่คอมไพเลอร์เป็นเจ้าของ ให้ใช้คำเดียวก่อน หากจำเป็นใช้ `snake_case` เฉพาะกรณีหายาก

### การนำเข้า

```ebnf
importDecl     := importRecord | importSugar
importRecord   := 'นำเข้า' '{' importFieldList? '}'
importFieldList := importField (',' importField)* ','?
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := 'ออก' '=' STRING
importVisibilityField := 'ทัศนวิสัย' '=' visibility
importNameField := 'ชื่อ' '=' IDENTIFIER
importAliasField := 'ในชื่อ' '=' IDENTIFIER
importWildcardField := 'ทั้งหมด' '=' IDENTIFIER
importSugar    := 'นำเข้า' 'ออก' STRING visibility? (namedImport | wildcardImport)?
visibility    := 'ส่วนตัว' | 'สาธารณะ'
namedImport   := IDENTIFIER ('ในชื่อ' IDENTIFIER)?
wildcardImport := '*' 'ในชื่อ' IDENTIFIER
```

ตัวอย่าง:

```fab
นำเข้า ออก "hono" ส่วนตัว Hono
นำเข้า ออก "norma:chorda"
นำเข้า { ออก = "norma:json/แก้", ในชื่อ = solve_mod }
นำเข้า ออก "faber:*" ส่วนตัว faber
นำเข้า ออก "./types" สาธารณะ User
```

ถ้าไม่ระบุทัศนวิสัย ค่าเริ่มต้นคือ `ส่วนตัว` ถ้าไม่ระบุ binding ที่มีชื่อ จะอนุมานจากส่วนสุดท้ายของเส้นทางเมื่อเป็น identifier ที่ถูกต้องและไม่ชนกับชื่อเดิม

---

## ชนิด

```ebnf
typeAnnotation := ('จาก' | 'ใน')? baseType ( '∪' typeAnnotation )*
baseType       := holeType | functionType | qualifiedType typeParams? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
typeParams     := genericParams
```

อาร์เรย์เขียนเป็น `รายการ<T>` ไม่รับ `T[]`; `จาก` และ `ใน` ใช้ทำเครื่องหมาย ownership; `T ∪ ว่าง` คือรูป nullable มาตรฐาน; `สมัครใจ` เป็นตัวทำเครื่องหมายการประกาศ ไม่ใช่คำนำหน้าชนิด; เส้นทางชนิดแบบมี namespace ต้องแก้ prefix เป็น namespace ก่อน

ตัวอย่างชนิดฟังก์ชัน:

```fab
ฟังก์ชัน กรอง((T) → ตรรกะ pred) → รายการ<T>
ฟังก์ชัน ผสม((A) → B f, (B) → C g) → (A) → C
ฟังก์ชัน ใช้((จำนวน) → จำนวน ⇥ ข้อความ op, จำนวน n) → จำนวน ⇥ ข้อความ
```

### ชนิดพื้นฐาน

| Faber | ความหมาย |
|---|---|
| `ข้อความ` | สตริง Unicode |
| `ascii` | สตริง ASCII เท่านั้น |
| `รูปแบบ` | เทมเพลตที่จับพร้อมพารามิเตอร์ |
| `จำนวน` | จำนวนเต็ม ค่าเริ่มต้น `i64` |
| `โมดูลัส<W>` | คำแบบไม่ติดลบที่คำนวณแบบโมดูลัส |
| `เศษ` | จำนวนลอยตัว ค่าเริ่มต้น `f64` |
| `ตรรกะ` | บูลีน |
| `ว่าง` | ค่า null |
| `เปล่า` | void |
| `ไม่เคย` | never |
| `ไม่รู้` | unknown |
| `ไบต์` | ไบต์ |

ชนิดที่กำหนดขนาดรับ width marker ตามตระกูล: `จำนวน<W>` ใช้ `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`; `เศษ<W>` ใช้ `f16`, `f32`, `f64`; `โมดูลัส<W>` ใช้ `u8`, `u16`, `u32`, `u64` เท่านั้น

### คอลเลกชันแบบทั่วไป

| Faber | ความหมาย |
|---|---|
| `รายการ<T>` | อาร์เรย์ |
| `ตาราง<K,V>` | แมป |
| `ชุด<T>` | เซต |
| `คำมั่น<T>` | promise |
| `ตัวชี้<T>` | iterator |
| `tensor<T, Figura>` | บัฟเฟอร์เนื้อเดียวแบบหนาแน่น |
| `vector<T, N>` | เวกเตอร์ตัวเลขแบบ register |
| `matrix<T, [R, C]>` | เมทริกซ์ตัวเลขสองมิติ |
| `atomic<T>` | เซลล์อะตอมิก |
| `sparsa<T, Figura>` | บัฟเฟอร์ sparse |

`Figura` คือ `_` หรือขนาดธรรมชาติหรือ identifier หรือรายการใน `[]`; `tensor<T, []>` คือ rank 0; `vacua` ให้คอลเลกชันว่างตามชนิดบริบท; `matrix` ต้องมีสองมิติ; `atomic<T>` ใน v1 รับ `i32` หรือ `u32`; การสร้างค่าคอลเลกชันใช้ `crea`, `structa` หรือ `↦` ไม่ใช้ `Type(...)`

### น้ำตาลชนิด

น้ำตาลอยู่ในตำแหน่งชนิดเท่านั้นและมีความหมายเดียวกับรูปเต็ม ตัวทำเครื่องหมายขนาดคือ `i8`–`u64` และ `f16`–`f64` รูปขึ้นต้นด้วย `l` หมายถึง `รายการ`, `t` หมายถึง `tensor`, `s` หมายถึง `sparsa`, `v` หมายถึง `vector`, `m` หมายถึง `matrix` น้ำตาล `matrix` ต้องมี shape สองมิติ และ `โมดูลัส<W>` ไม่มีน้ำตาล

---

## การควบคุมการไหล

### เงื่อนไข

```ebnf
ifStmt     := 'ถ้า' expression arm ('ถ้าไม่ก็' ifStmt | elseClause)?
elseClause := 'มิฉะนั้น' elseArm
arm        := (blockStmt | 'ดังนั้น' statement) catchClause?
elseArm    := (blockStmt | 'ดังนั้น' statement) catchClause?
```

`ถ้า` = if, `ถ้าไม่ก็` = else-if, `มิฉะนั้น` = else, `ดังนั้น` ใช้กับบอดีหนึ่งคำสั่ง และ `เงียบ` ใช้แทน no-op แบบชัดเจน

### ลูป

```ebnf
whileStmt  := 'ขณะ' expression (blockStmt | 'ดังนั้น' statement) catchClause?
iteraStmt  := 'วน' (('ออก' | 'จาก') expression | 'จาก' expression) ('คงที่' | 'แปร') IDENTIFIER (blockStmt | 'ดังนั้น' statement) catchClause?
```

`วน ออก...คงที่` คือ for-of, `วน จาก...คงที่` คือ for-in และ `วน จาก range คงที่ i` คือการวนช่วง โดย `ต่อ` อยู่ในนิพจน์ช่วง

### การเลือกกรณี

```ebnf
eligeStmt    := 'เลือก' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase    := 'กรณี' expression (blockStmt | 'ดังนั้น' statement)
defaultCase  := 'อื่น' (blockStmt | 'ดังนั้น' statement)
```

### การจับคู่รูปแบบ

```ebnf
discerneStmt := 'แยก' 'ทั้งหมด'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase  := 'กรณี' patterns (blockStmt | 'ดังนั้น' statement)
patterns     := pattern ((',' | 'และ') pattern)*
pattern      := '_' | literal | (IDENTIFIER patternBind?)
patternBind  := ('ในชื่อ' IDENTIFIER) | (('คงที่' | 'แปร') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('ในชื่อ' IDENTIFIER)?
```

### การป้องกัน

```ebnf
guardStmt   := 'คุ้มครอง' '{' guardClause+ '}'
guardClause := 'ถ้า' expression (blockStmt | 'ดังนั้น' statement)
```

### การจัดการทรัพยากร

```ebnf
curaStmt    := 'ดูแล' STRING ('คงที่' | 'แปร') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### การแยกโครงสร้าง

```ebnf
extractStmt   := 'ออก' expression ('คงที่' | 'แปร') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField  := IDENTIFIER ('ในชื่อ' IDENTIFIER)?
restField     := 'ที่เหลือ' IDENTIFIER
```

### การส่งต่อการควบคุม

```ebnf
returnStmt   := 'คืน' expression?
breakStmt    := 'หยุด'
continueStmt := 'ไปต่อ'
noopStmt     := 'เงียบ'
```

---

## การจัดการข้อผิดพลาด

```ebnf
throwStmt   := ('โยน' | 'ตาย') expression ['ถ้า' expression]
catchClause := 'จับ' IDENTIFIER blockStmt
assertStmt  := 'ยืนยัน' expression ('ตาย' expression)?
requiritStmt := 'ต้องการ' expression 'โยน' expression
```

`จับ` ต่อกับคำสั่งแบบมีโครงสร้างและแขนของเงื่อนไข ไม่ต่อกับบล็อกเปล่าโดยตรง `ทำ { ... } จับ err { ... }` เป็นขอบเขต recoverable แบบครั้งเดียว `ลอง` และ `ท้าย` เป็นพื้นผิวเก่าที่ถูกปฏิเสธ `โยน` คือข้อผิดพลาดที่กู้คืนได้ และ `ตาย` คือ panic ร้ายแรง guard `ถ้า <expr>` เป็นน้ำตาลของ parser

---

## นิพจน์

### ตัวดำเนินการ (จากความสำคัญต่ำไปสูง)

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
ternary    := or (('?' expression ':' | 'เช่นนั้น' expression 'มิฉะนั้น') ternary)?
or         := and (('หรือ') and)*
and        := equality (('และ') equality)*
equality   := comparison (('≡' | '≠' | '≈' | '≉' | 'เป็น' | 'ไม่' 'เป็น') comparison)*
comparison := bitwiseOr (('<' | '>' | '≤' | '≥' | 'ภายใน' | 'ระหว่าง') bitwiseOr)*
bitwiseOr  := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift      := range (('⇐' | '⇒') range)*
range      := additive (('‥' | '…' | 'ก่อน' | 'จนถึง') additive ('ต่อ' additive)?)?
additive   := multiplicative (('+' | '-') multiplicative)*
multiplicative := coalesce (('*' | '/' | '%') coalesce)*
coalesce   := unary ('หรือ' velRhs)*
velRhs     := unary (('‥' | '…' | 'ก่อน' | 'จนถึง') unary ('ต่อ' unary)?)?
unary      := ('-' | '¬' | 'ไม่' | 'รอ' | 'สร้าง') unary | cast
cast       := call ('∷' typeAnnotation | conversio)*
conversio   := '↦' typeAnnotation inlineRecovery?
inlineRecovery := '⇥' unary
```

`↤` คือการกำหนดค่าแบบแปลงชนิดตามเป้าหมาย: ประเมินด้านขวา แปลงเป็นชนิดคงที่ของด้านซ้ายผ่านเส้นทาง `↦` แล้วกำหนดค่า `⇥` สำหรับกู้คืนใช้ได้เฉพาะหลัง `↤` เท่านั้น ไม่ใช่หลัง `←`

ตัวดำเนินการ glyph ต้องคงเดิมเสมอ `∷` คือการระบุชนิดแบบคอมไพล์ไทม์; `↦` คือการแปลงค่ารันไทม์; `⇥` ใช้กู้คืนความล้มเหลวแบบอินไลน์ `หรือ` ใช้กำจัด nullable เฉพาะที่ ไม่ใช่คำพ้องของตรรกะ `หรือ`

### การเรียกและการเข้าถึงสมาชิก

```ebnf
call          := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix    := typeArgs? '(' argumentList ')'
memberSuffix  := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList  := (argument (',' argument)*)?
argument      := 'กระจาย'? expression
```

### สตริงและเทมเพลต

Faber ใช้ความหมายตามตัวคั่น: แต่ละรูปแบบมีชนิดและบทบาทต่างกัน ไม่ใช่คำพ้องกัน

| รูปแบบ | ชนิด | บทบาท |
|---|---|---|
| `'...'` | `ascii` | โทเค็นเครื่องคงที่ |
| `"..."` | `ข้อความ` | สตริง Unicode สั้น |
| `«...»` | `ข้อความ` | สตริง Unicode หลายบรรทัด |
| `` `...` `` | `รูปแบบ` | เทมเพลตที่จับ |
| `{ ... }` | `json` | เอกสาร JSON รากออบเจ็กต์ |
| `\|...\|` | `ไบต์` | ไบต์ฐานสิบหก |
| `"..." ↦ regex` | `regex` | แพตเทิร์นคอมไพล์จากข้อความ |
| `[ ... ]` | `รายการ<T>` | รายการ Faber |

`§` เป็นช่องเทมเพลตในรูป Unicode (`"`, `«`, และ backtick) และห้ามอยู่ใน literal `ascii` เทมเพลตที่เรนเดอร์ใช้ `"..."(...)` หรือ `«...»(...)`; เทมเพลตที่จับใช้ `` `...`(...) `` สำหรับ payload ที่ผูกกับ SQL/URL

ตัวอย่าง:

```fab
คงที่ _ tag ← «inline»
คงที่ _ q ← `select * from accounts where id = §`(accountId)
คงที่ _ sig ← |de ad be ef|
"สถานะ: § (§)"(sample_status(), "ok")
```

การเข้าถึงข้อความใช้ดัชนี Unicode-scalar; การเข้าถึง `รายการ<T>` ใช้ดัชนีจำนวนเต็มเดี่ยวและ trap เมื่อเกินขอบเขต; การอ่าน tensor คืน `T ∪ ว่าง`; `ไบต์` ไม่รับวงเล็บเหลี่ยมและใช้เมธอด `accipe`, `appende`, `longitudo`

### นิพจน์หลัก

```ebnf
primary := IDENTIFIER | NUMBER | STRING | ASCII_STRING | BACKTICK_STRING
         | 'ตัวฉัน' | 'จริง' | 'เท็จ' | 'ว่าง'
         | 'ว่างเปล่า' | arrayLiteral | jsonLiteral | typedConstructor
         | adExpr
         | '(' expression ')'
adExpr    := 'ถึง' asciiLiteral adOpener?
arrayLiteral := '[' argumentList? ']'
jsonLiteral := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonMember  := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('กระจาย' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonArray := '[' (jsonValue (',' jsonValue)* ','?)? ']'
jsonString := STRING
jsonNumber := NUMBER
```

### นิพจน์พิเศษ

```ebnf
fingeExpr     := 'สร้าง' IDENTIFIER ('{' fieldList '}')? ('∷' IDENTIFIER)?
praefixumExpr := 'นำหน้า' (blockStmt | '(' expression ')')
formatStringExpr := STRING '(' argumentList ')'
formaTemplateExpr := BACKTICK_STRING '(' argumentList ')'
scriptumExpr  := 'จารึก' '(' STRING (',' expression)* ')'
legeExpr      := 'อ่าน' 'บรรทัด'?
regexFromText := (STRING | ASCII_STRING) '↦' 'regex'
```

รูป regex แบบ `/.../` ยังไม่อยู่ในไวยากรณ์ ตัวดำเนินการ `/` ยังคงเป็นการหาร และคอมเมนต์ `//` กับ `/* ... */` ถูกปฏิเสธ

---

## รูปแบบจับคู่

```ebnf
objectPattern  := '{' patternProperty (',' patternProperty)* '}'
patternProperty := 'ที่เหลือ'? IDENTIFIER ('ในชื่อ' IDENTIFIER)?
arrayPattern   := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | 'ที่เหลือ'? IDENTIFIER
```

---

## การวินิจฉัย

```ebnf
outputStmt := ('บันทึก' | 'ดู' | 'เตือน' | 'เขียน') expression (',' expression)*
```

`บันทึก` คือหมายเหตุทั่วไป, `ดู` คือ debug/inspect, `เตือน` คือ warning และ `เขียน` คือช่องวินิจฉัย การส่งผลจริงให้ใช้เมธอดไลบรารีมาตรฐาน

### คอมเมนต์

Faber รับเฉพาะคอมเมนต์บรรทัด: `#` ถึงจบบรรทัด `#` ต้องเป็นโทเค็นที่ไม่ใช่ช่องว่างตัวแรกของบรรทัดตรรกะ คอมเมนต์ที่อยู่หลังโทเค็นอื่นในบรรทัดเดียวกันเป็น lex error `#` ใน literal ที่มีตัวคั่นไม่ใช่คอมเมนต์

---

## จุดเข้า

```ebnf
incipitStmt  := 'เริ่ม' blockStmt
incipietStmt := 'เริ่มอะซิงก์' blockStmt
```

`เริ่ม` คือจุดเข้าแบบ sync และ `เริ่มอะซิงก์` คือจุดเข้าแบบ async

---

## การทดสอบ

```ebnf
probandumDecl := 'ทดสอบชุด' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt     := 'ทดสอบ' STRING probaModifier* blockStmt
probaModifier := 'ละเว้น' STRING | 'อนาคต' STRING | 'เฉพาะ' | 'แท็ก' STRING
              | 'เวลา' NUMBER | 'วัด' | 'ทำซ้ำ' NUMBER | 'เปราะบาง' NUMBER
              | 'ต้องการ' STRING | 'เฉพาะใน' STRING
praeparaBlock := ('เตรียม' | 'จะเตรียม' | 'หลังเตรียม' | 'จะหลังเตรียม') 'ทั้งหมด'? blockStmt
```

---

## เฟรมเวิร์ก CLI

```ebnf
cliDecl       := annotation* (incipitStmt | funcDecl)
cliAnnotation := cliProgramAnnotation | imperiumAnnotation | optioAnnotation | operandusAnnotation
```

Faber รองรับการสร้างแอป CLI พร้อมการแยกอาร์กิวเมนต์และการสร้างความช่วยเหลืออัตโนมัติ

```fab
@ cli "faber"
@ ตัวเลือก verbose ยาว "verbose" ชนิด ตรรกะ
เริ่ม อาร์กิวเมนต์ args {
    # เฟรมเวิร์ก CLI แยกอาร์กิวเมนต์อัตโนมัติ
}
```

---

## การเรียก Capability

รูปนิพจน์ `ถึง` เป็นพื้นผิว `ad` ที่รองรับเพียงแบบเดียว รูปเก่าแบบมีชนิดและบล็อกสตรีมถูกปฏิเสธตอน parse

```ebnf
adExpr        := 'ถึง' asciiLiteral adOpener?
adOpener      := '(' expression ')'
conversioExpr := expr '↦' typeAnnotation
```

เส้นทางต้องเป็น `asciiLiteral`; opener เป็นนิพจน์เดียวที่ส่งเป็น `data`; นิพจน์ `ถึง` คืน handle การสนทนา `sermo`; ใช้ `↦ T` เพื่อ materialize หรือใช้มุมมอง `s.meus<T>()` และ `s.tuus<T>()` รูปเก่าทั้งหมดเป็น parse error ชนิดที่คอมไพเลอร์เป็นเจ้าของคือ `scrinium`, `status`, และ handle ทึบแสง `sermo`

---

## การดำเนินการกับคอลเลกชัน

DSL pipeline `ab` แบบเก่าถูกยกเลิก การกรอง การตัด และการรวมใช้เมธอดปกติของ `ข้อความ`, `รายการ`, `ตาราง`, `ชุด` และ closure แทน `prima` และ `ultima` เป็นชื่อเมธอดทั่วไป และ `ubi` ไม่ใช่ไวยากรณ์คอลเลกชันที่ใช้งานอยู่

---

## บล็อก `ทำ`

```ebnf
facBlockStmt := 'ทำ' blockStmt catchClause? ('ขณะ' expression)?
```

`ทำ { ... }` ทำงานครั้งเดียว; `ทำ { ... } จับ err { ... }` เป็นขอบเขตข้อผิดพลาดที่กู้คืนได้; `ทำ { ... } ขณะ condition` เป็นลูป post-test และ `ขณะ` ต่อท้ายได้เฉพาะ `ทำ`

---

## การรองรับเป้าหมาย

การรองรับเป้าหมายไม่ใช่ส่วนของไวยากรณ์ ไฟล์นี้กำหนดเฉพาะภาษา สำหรับตาราง grammar×target ให้ดู `EBNF_MATRIX.md` และนโยบายรันไทม์ให้ดูเอกสาร target capability matrix

---

## อ้างอิงคำสำคัญ

ตารางนี้สรุปพื้นผิวคำสำคัญสำหรับผู้อ่านภาษาไทย คำสำคัญในตัวอย่างใช้การสะกดเดียวกับภาคผนวกท้ายเอกสาร คำที่เป็น glyph เช่น `←`, `→`, `∴`, `∪`, `⇥`, `≡`, `≤`, `≥`, `≠`, `≈`, `‥`, `…`, `∷`, `↦`, `⇐`, `⇒`, `∨`, `⊻`, `∧`, `¬` คงเดิมเสมอ

| หมวด | พื้นผิวภาษาไทย | ความหมาย |
|---|---|---|
| การประกาศ | `คงที่`, `แปร`, `ฟังก์ชัน`, `ชนิด`, `สัญญา`, `ลำดับ`, `สหภาพแยก` | การประกาศค่าคงที่ ตัวแปร ฟังก์ชัน ชนิด อินเทอร์เฟซ enum และ tagged union |
| การควบคุม | `ถ้า`, `ถ้าไม่ก็`, `มิฉะนั้น`, `ขณะ`, `วน`, `เลือก`, `กรณี`, `แยก`, `คุ้มครอง`, `ทำ` | เงื่อนไข ลูป การเลือก การจับคู่ และขอบเขตการจัดการข้อผิดพลาด |
| การส่งต่อ | `คืน`, `หยุด`, `ไปต่อ`, `เงียบ`, `โยน`, `ตาย`, `จับ` | return, break, continue, no-op, throw, panic และ handler |
| นิพจน์ | `และ`, `หรือ`, `ไม่`, `เป็น`, `ภายใน`, `ระหว่าง`, `ก่อน`, `จนถึง`, `ต่อ`, `หรือว่าง` | ตัวดำเนินการตรรกะ การเปรียบเทียบ ช่วง และการกำจัด nullable |
| การทดสอบ | `ทดสอบ`, `ทดสอบชุด`, `เตรียม`, `จะเตรียม`, `หลังเตรียม`, `จะหลังเตรียม` | การประกาศและตัวปรับแต่งการทดสอบ |

---

## กฎไวยากรณ์สำคัญ

1. พารามิเตอร์ใช้ชนิดก่อนชื่อ: `ฟังก์ชัน f(จำนวน x)` ไม่ใช่ `ฟังก์ชัน f(x: จำนวน)`
2. การประกาศใช้ชนิดก่อนชื่อ: `คงที่ ข้อความ name` ไม่ใช่ `คงที่ name: ข้อความ`
3. ลูปใช้รูป `วน ออก/จาก collection คงที่/แปร item { }` หรือ `วน จาก range คงที่/แปร item { }`
4. วงเล็บรอบเงื่อนไขใช้ได้ แต่ไม่ใช่รูปที่แนะนำ
5. คำวินิจฉัยเป็นคำสั่ง ไม่ใช่ค่าที่เรียกได้

## Reader pack glossary (machine extract)

### Keywords
| Latin | Localized |
|---|---|
| discretio | สหภาพแยก |
| fixum | คงที่ |
| functio | ฟังก์ชัน |
| genus | ชนิด |
| implendum | สัญญา |
| importa | นำเข้า |
| modulus | โมดูลัส |
| ordo | ลำดับ |
| sit | อนุมานคงที่ |
| typus | ชนิดนามแฝง |
| varia | แปร |
| abstractus | นามธรรม |
| ceteri | ที่เหลือ |
| curata | จัดการ |
| errata | ข้อผิดพลาด |
| exitus | ทางออก |
| generis | ของชนิด |
| iacit | โยนผล |
| immutata | ไม่เปลี่ยนแปลง |
| magnitudo | ขนาด |
| nexum | ผูก |
| optiones | ตัวเลือก |
| prae | ก่อนหน้า |
| privata | ส่วนตัว |
| protecta | ป้องกัน |
| publica | สาธารณะ |
| sponte | สมัครใจ |
| casu | กรณี |
| ceterum | อื่น |
| custodi | คุ้มครอง |
| discerne | แยก |
| dum | ขณะ |
| elige | เลือก |
| ergo | ดังนั้น |
| fac | ทำ |
| itera | วน |
| secus | มิฉะนั้น |
| si | ถ้า |
| sic | เช่นนั้น |
| sin | ถ้าไม่ก็ |
| perge | ไปต่อ |
| redde | คืน |
| rumpe | หยุด |
| tacet | เงียบ |
| adfirma | ยืนยัน |
| cape | จับ |
| cede | รอ |
| iace | โยน |
| mori | ตาย |
| clausura | ปิดล้อม |
| falsum | เท็จ |
| nihil | ว่าง |
| verum | จริง |
| aut | หรือ |
| est | เป็น |
| et | และ |
| non | ไม่ |
| vel | หรือว่าง |
| ego | ตัวฉัน |
| finge | สร้าง |
| implet | เติมเต็ม |
| sub | สืบทอด |
| mone | เตือน |
| nota | บันทึก |
| scribe | เขียน |
| vide | ดู |
| argumenta | อาร์กิวเมนต์ |
| cura | ดูแล |
| incipiet | เริ่มอะซิงก์ |
| incipit | เริ่ม |
| ad | ถึง |
| de | จาก |
| ex | ออก |
| in | ใน |
| lege | อ่าน |
| lineam | บรรทัด |
| omnia | ทั้งหมด |
| praefixum | นำหน้า |
| scriptum | จารึก |
| sparge | กระจาย |
| ut | ในชื่อ |
| ante | ก่อน |
| inter | ระหว่าง |
| intra | ภายใน |
| per | ต่อ |
| usque | จนถึง |
| fragilis | เปราะบาง |
| futurum | อนาคต |
| metior | วัด |
| omitte | ละเว้น |
| postpara | หลังเตรียม |
| postparabit | จะหลังเตรียม |
| praepara | เตรียม |
| praeparabit | จะเตรียม |
| proba | ทดสอบ |
| probandum | ทดสอบชุด |
| repete | ทำซ้ำ |
| requirit | ต้องการ |
| solum | เฉพาะ |
| solum_in | เฉพาะใน |
| tag | แท็ก |
| temporis | เวลา |
| negativum | ลบ |
| nonnihil | ไม่ว่าง |
| nonnulla | ไม่เป็นค่าว่าง |
| nulla | ว่างเปล่า |
| positivum | บวก |

### Types
| Latin | Localized |
|---|---|
| ascii | ascii |
| textus | ข้อความ |
| numerus | จำนวน |
| modulus | โมดูลัส |
| fractus | เศษ |
| bivalens | ตรรกะ |
| nihil | ว่าง |
| vacuum | เปล่า |
| numquam | ไม่เคย |
| ignotum | ไม่รู้ |
| octeti | ไบต์ |
| regex | regex |
| json | json |
| valor | ค่า |
| instans | อินสแตนซ์ |
| objectum | ออบเจ็กต์ |
| quidlibet | อะไรก็ได้ |
| lista | รายการ |
| tabula | ตาราง |
| copia | ชุด |
| promissum | คำมั่น |
| cursor | ตัวชี้ |

### Glossary changes vs pass1 / existing pack
| Latin | Old pack | New (this EBNF) | Why |
|---|---|---|---|
| fixum | คงที่ | คงที่ | คงคำเดิม |
| functio | ฟังก์ชัน | ฟังก์ชัน | คงคำเดิม |
| genus | ชนิด | ชนิด | คงคำเดิม |
| varia | แปร | แปร | คงคำเดิม |
| importa | นำเข้า | นำเข้า | คงคำเดิม |
| si | ถ้า | ถ้า | คงคำเดิม |
| sin | ถ้าไม่ก็ | ถ้าไม่ก็ | คงคำเดิม |
| secus | มิฉะนั้น | มิฉะนั้น | คงคำเดิม |
| dum | ขณะ | ขณะ | คงคำเดิม |
| fac | ทำ | ทำ | คงคำเดิม |
| itera | วน | วน | คงคำเดิม |
| perge | ข้าม | ไปต่อ | ปรับให้ตรงความหมาย continue |
| rumpe | หยุด | หยุด | คงคำเดิม |
| redde | คืน | คืน | คงคำเดิม |
| casu | กรณี | กรณี | คงคำเดิม |
| ceterum | อื่น | อื่น | คงคำเดิม |
| elige | เลือก | เลือก | คงคำเดิม |
| discerne | แยก | แยก | คงคำเดิม |
| cape | จับ | จับ | คงคำเดิม |
| falsum | เท็จ | เท็จ | คงคำเดิม |
| verum | จริง | จริง | คงคำเดิม |
| nihil | ว่าง | ว่าง | คงคำเดิม |
| et | และ | และ | คงคำเดิม |
| aut | หรือ | หรือ | คงคำเดิม |
| non | ไม่ | ไม่ | คงคำเดิม |
| est | เป็น | เป็น | คงคำเดิม |
| nota | แสดง | บันทึก | ปรับให้สอดคล้องกับบทบาท diagnostic note |
| mone | เตือน | เตือน | คงคำเดิม |
| scribe | เขียน | เขียน | คงคำเดิม |
| vide | ดู | ดู | คงคำเดิม |
| incipit | เริ่ม | เริ่ม | คงคำเดิม |
| argumenta | อาร์กิวเมนต์ | อาร์กิวเมนต์ | คงคำเดิม |
| ex | จาก | ออก | แยกความหมาย import source/consume ให้ชัด |
| de | ยืม | จาก | ให้สอดคล้องกับการวนแบบ for-in |
| in | ใน | ใน | คงคำเดิม |
| ut | ในชื่อ | ในชื่อ | คงคำเดิม |
| textus | ข้อความ | ข้อความ | คงคำเดิม |
| numerus | จำนวน | จำนวน | คงคำเดิม |
| fractus | เศษ | เศษ | คงคำเดิม |
| bivalens | ตรรกะ | ตรรกะ | คงคำเดิม |
| vacuum | เปล่า | เปล่า | คงคำเดิม |
| ignotum | ไม่รู้ | ไม่รู้ | คงคำเดิม |
| lista | รายการ | รายการ | คงคำเดิม |
| tabula | ตาราง | ตาราง | คงคำเดิม |
| copia | ชุด | ชุด | คงคำเดิม |
| cursor | ตัวชี้ | ตัวชี้ | คงคำเดิม |

หมายเหตุสั้นเมื่อเทียบกับ pass1: ฉบับนี้เติมส่วนที่ขาดจาก EBNF ต้นฉบับ ปรับหัวข้อและคำอธิบายที่ยังเป็นอังกฤษให้เป็นภาษาไทย ทำให้ `ergo` ใช้เป็นข้อต่อบอดีคำสั่งอย่างสม่ำเสมอ รักษา `∴` เป็น glyph ของ clausura เท่านั้น แก้ `perge` เป็น `ไปต่อ` และเพิ่ม glossary ครบ 111 คำสำคัญกับ 22 ชนิดโดยไม่มีการชนกันของการสะกดในแต่ละกลุ่ม
