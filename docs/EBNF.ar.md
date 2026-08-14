# مواصفات لغة Faber

> **Reader-locale EBNF (Arabic).** Latin/source-of-truth grammar remains [`EBNF.md`](EBNF.md).
> This file is the Arabic reader surface of that grammar (keywords, commentary, examples).
> Pack keyword/type spellings are extracted from the glossary appendix at the end.
> Glyphs (`← → ∴ ≡ ∪ ⇥` …) never localize; `ergo` localizes, `∴` is clausura-only.


القواعد الشكلية للغة البرمجة Faber. التنفيذ النشط هو مساحة عمل Rust الجذرية: `crates/faber` لأدوات الحزم والمشاريع و `crates/radix` لخط أنابيب المصرّف.

عقد التوثيق: هذا الملف هو السطح القانوني للقواعد والتعليق على المواصفات. برامج مرجع اللغة القابلة للتشغيل موجودة في المستودع العام [`../examples/corpus/`](../examples/corpus/) مع واجهة أمامية `+++` اختيارية (`term`, `syntax`, `related`, …)؛ السجل المولّد هو [`../examples/corpus/index.toml`](../examples/corpus/index.toml). `faber explain` يحمّل حزمة exempla المرجعية من القرص. فضّل مجموعة اللغة + EBNF لأعمال المرجع الجديدة.

---

## بنية البرنامج

ملفات Faber المصدرية هي نصوص خام تقشّر بواسطة المشغّل قبل التحليل المعجمي. الواجهة الأمامية TOML الاختيارية ليست جزءاً من قواعد الرموز.

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

### الواجهة الأمامية للملف (`+++`)

عند وجودها، يجب أن تفتح الواجهة الأمامية في **السطر 1** بـ `+++` بالضبط. سطر لاحق يُقلّم إلى `+++` بالضبط ينهي الكتلة. البايتات بعد المحدد الختامي هي `program` الخاص بـ Faber. الجسم الفارغ (مسافات بيضاء فقط) هو برنامج فارغ صالح.

تُحلّل الواجهة الأمامية كمستند TOML عام في مشغّل المصرّف — لا تُحلّل كعبارات Faber. يمكن للمؤلفين إرفاق مفاتيح بيانات وصفية اعتباطية؛ تقرأ الأدوات المفاتيح المعروفة مثل `group` و `sectio` و `[probanda]` عبر دوال الوصول. أدوات حزمة `faber` تستهلك مفاتيح الحزمة تلك. سلطة الحزمة لـ `[package]` و `[paths]` و `[build]` تبقى في `faber.toml`؛ قيم الواجهة الأمامية المتعارضة تُرفض في وضع الحزمة.

مثال:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

بداية {}
```

توجيهات الملف `§` في بداية السطر أُزيلت. ضع بيانات الملف الوصفية في الواجهة الأمامية `+++` بدلاً من ذلك. داخل السلاسل المقتبسة، يبقى `§` كثقب قالب السلسلة (انظر **الاستدعاء والوصول إلى الأعضاء** أدناه).

---

## التصريحات

### المتغيرات

```ebnf
varDecl      := ('ثابت' | 'متغير') typeAnnotation IDENTIFIER (('←' expression) | ('↤' assignment inlineRecovery?))?
sitDecl      := 'ليكن' IDENTIFIER ('←' expression)?
arrayDestruct := ('ثابت' | 'متغير') arrayPattern '←' expression
objectDestruct := ('ثابت' | 'متغير') objectPattern '←' expression
```

- `ثابت` = ارتباط غير قابل للتغيير (كتابة مرة واحدة): يمكن التصريح به دون مهيئ وإسناده مرة واحدة بالضبط لاحقاً، ثم يتجمد. `متغير` = ارتباط قابل للتغيير (قابل لإعادة الإسناد)، مثل `let`.
- استخدم `_` كتوصيف نوع عندما يحدد المهيئ النوع: `ثابت _ name ← value`
- `ليكن name ← value` هو اختزال لـ `ثابت _ name ← value` (محلي غير قابل للتغيير مستنتج)
- `ليكن name` (بدون مهيئ) هو اختزال لـ `ثابت _ name` — غير القابل للتغيير المؤجل المستنتج. أسند مرة واحدة بالضبط قبل أي قراءة.
- التهيئة المؤجلة: `ثابت numerus x` أو `ليكن x` يصرح عن خانة غير قابلة للتغيير غير مهيأة يجب إسنادها مرة واحدة بالضبط قبل أي قراءة؛ الإسناد الثاني مرفوض. تمرير الإسناد المحدد (المرحلة الدلالية 3أ) يفرض هذا.

### الدوال

```ebnf
funcDecl     := 'دالة' IDENTIFIER genericParams? '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
paramList    := (parameter (',' parameter)*)?
genericParams := '<' genericParam (',' genericParam)* '>'
genericParam  := IDENTIFIER | 'حجم' IDENTIFIER
typeArgs      := '<' typeAnnotation (',' typeAnnotation)* '>'
parameter    := ('عن' | 'في' | 'من')? 'باقي'? typeAnnotation IDENTIFIER 'اختياري'? ('كـ' IDENTIFIER)? ('عوض' expression)?
funcModifier := 'وسائط' IDENTIFIER | 'مخصص' IDENTIFIER ('كـ' IDENTIFIER)? | 'مخطئ' IDENTIFIER | 'مخرج' (IDENTIFIER | NUMBER) | 'ثابتة' | 'يرمي' | 'خيارات' IDENTIFIER
returnClause := '→' typeAnnotation
alternateExitClause := '⇥' typeAnnotation
stmtBodyJoint  := 'إذن'
clausuraJoint  := '∴'
clausuraExpr   := compactClausuraExpr | legacyClausuraExpr
compactClausuraExpr := clausuraSignature clausuraJoint (expression | closureFacBlock)
clausuraSignature := (clausuraParam | '(' clausuraParams? ')') returnClause? alternateExitClause?
closureFacBlock := 'افعل' blockStmt catchClause?
legacyClausuraExpr := 'إغلاق' clausuraParams? ('→' typeAnnotation)? (':' expression | blockStmt)
clausuraParams := clausuraParam (',' clausuraParam)*
clausuraParam  := typeAnnotation IDENTIFIER
```

- صيغة الإرجاع: `→` يصرح عن نوع النجاح العادي. الدالة ذات الجسم بدون `→` هي تأثير فقط (`فراغ`) ويجب ألا تحتوي على `أعد`. الإغلاق ذو الجسم العباري (`افعل { ... }` أو جسم الكتلة القديم) يجب أيضاً أن يكتب `→ T` قبل أن يتمكن من استخدام `أعد`؛ الإغلاقات ذات جسم التعبير قد تستنتج نتيجتها من التعبير.
- صيغة الخروج البديل القابل للاسترداد: `⇥` يصرح عن نوع قناة الخطأ. يمكن أن يظهر بعد `→ T` أو وحده على دالة قابلة للفشل تأثير فقط أو إغلاق. جسم الإغلاق الذي يستخدم `ارم` هارباً يجب أن يصرح عن `⇥ E` الخاص به؛ لا يمكنه وراثة قناة خطأ الدالة المحيطة. `افعل { ... } التقط err { ... }` محلي قد يلتقط `ارم` بدون `⇥` محيط. استدعاء دالة قابل للفشل (`→ T ⇥ E`) داخل دالة مصرحة بـ `⇥` ينتشر إلى الخروج البديل للدالة بدون غلاف `افعل`/`التقط`، معاكساً كيف يتصرف `↦` conversio المجرد و `ارم` الرمي بالفعل؛ يخفض الاستدعاء إلى `?` الخاص بـ Rust. يجب على الإغلاق أن يصرح عن `⇥` الخاص به لنشر استدعاء قابل للفشل — قناة خطأ الدالة المحيطة لا تعبر حدود الإغلاق.
- بادئات المعامل: `عن` (قراءة)، `في` (تعديل)، `من` (استهلاك)
- علامة بعد الاسم: `اختياري` (توفير طوعي/اختياري)
- `باقي` يعلم معامل الباقي
- `مخصص NAME ('كـ' LOCAL)?` يصرح عن متطلب مخصص؛ `LOCAL` هو الاسم المستعار في جسم الدالة
- `إذن` هو رابط **جسم العبارة** المدمج فقط (أذرع `إذا`/`طالما`/`حالة`/… ذات العبارة الواحدة).
- `∴` هو رابط **الإغلاق** المدمج فقط. الاثنان ليسا مترادفين.
- أجسام كتلة الإغلاق المدمجة يجب أن تستخدم `افعل { ... }`؛ جسم `افعل` المحلي للإغلاق قد يرفق `التقط`، لكن لا يمكنه استخدام `طالما` اللاحق.

### الأصناف

```ebnf
genusDecl    := 'مجرد'? 'صنف' IDENTIFIER typeParams? ('امتد' IDENTIFIER)? ('حقق' IDENTIFIER (',' IDENTIFIER)*)? '{' genusMember* '}'
genusMember  := annotation* (fieldDecl | methodDecl)
fieldDecl    := 'سكوني'? 'مرتبط'? typeAnnotation IDENTIFIER 'اختياري'? ('=' expression)?
methodDecl   := 'دالة' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause? blockStmt?
```

### التعليقات

```ebnf
annotation            := '@' annotationName annotationBody?
annotationName        := IDENTIFIER | 'عام' | 'محمي' | 'خاص' | 'مستقبلي' | 'مؤشر'
                        | 'وسم' | 'فقط' | 'أهمل' | 'قس'
annotationBody        := bracedAnnotation | annotationArgs
bracedAnnotation      := '{' annotationFieldList? '}'
annotationFieldList   := annotationField (',' annotationField)* ','?
annotationField       := IDENTIFIER '=' (expression | typeAnnotation)

cliProgramAnnotation := '@' 'cli' STRING
imperiumAnnotation := '@' 'imperium' STRING
optioAnnotation    := '@' 'optio' IDENTIFIER optioModifier*
optioModifier      := 'brevis' STRING | 'longum' STRING | 'نمط' typeAnnotation
                    | 'descriptio' STRING | 'ubique' | 'عوض' expression
operandusAnnotation := '@' 'operandus' ('باقي')? typeAnnotation IDENTIFIER operandusModifier*
operandusModifier  := 'descriptio' STRING | 'ubique' | 'عوض' expression
annotationArgs     := (STRING | IDENTIFIER | expression)+

(* عقود التعليقات — مخططات بيانات وصفية زمن التصريف *)
annotatioMarker     := '@' 'annotatio' ( '{' annotatioFieldList? '}' )?
annotatioFieldList  := annotatioField (',' annotatioField)* ','?
annotatioField      := 'target' '=' annotatioTarget
annotatioTarget     := 'دالة' | STRING | IDENTIFIER
contractApplication := '@' IDENTIFIER ( '{' annotationFieldList? '}' )?

jsonGenusAnnotation := '@' 'json'
jsonFieldAnnotation := '@' 'json' '{' 'nomen' '=' STRING '}'
```

Radix يحلل سجلات التعليقات المحصورة (`@ مستقبلي { }`، `@ optio { binding = verbose, ... }`) والتهجئات المختزلة الموجودة. تهبط الصيغ المختزلة والمحصورة إلى نفس سجلات `HirAnnotation` للعائلات المرقّاة. بعض عائلات الرموز غير المرقّاة قد تظل تحافظ على وسائط التعليقات الخام حتى يُهاجر مستهلكوها.

**عقود التعليقات:** `@ annotatio` (اختيارياً `@ annotatio { target = دالة }`) يعلم `صنف` مستوى أعلى كعقد تعليق زمن التصريف. الأصناف العادية ليست مخططات تعليق. التطبيقات تستخدم `@ ContractName { field = constant }` وتحل من خلال التصريحات المحلية أو صادرات واجهة الملف المستوردة. التطبيقات المحلولة تهبط إلى `HirAnnotation` مع `contract_id: Some(DefId)` وقيم حقول ثابتة. هدف الإرفاق v1 هو `دالة` فقط؛ كميات الحمولة هي `نص`، `عدد`، `كسر`، و `منطقي` (اختيارية عبر `اختياري` أو `T ∪ لاشيء`). لا توجد عائلات `@ web` / controller / route مملوكة للمصرّف.

**أصناف JSON:** `@ json` على `صنف` هو عقد نموذج بيانات مملوك للمصرّف، وليس مخطط تعليق عام. يجب أن تكون الحقول آمنة لـ JSON (`نص`، `أسكي`، `عدد`، `كسر`، `منطقي`، `لحظة`، `لاشيء`، `قائمة<T>`، `جدول<نص, T>`، قابل للإبطال `T ∪ لاشيء`، أو `@ json صنف` آخر). البيانات الوصفية للحقل `@ json { nomen = "wire_name" }` تغير مفتاح الكائن المنبعث المستخدم بواسطة `value ↦ قيمة`، `value ↦ جسون`، و `جسون ↦ Genus`؛ نص JSON يبقى عملية Norma سلكية مثل `json.pange(value ↦ جسون)`.

- `@ radix` محجوز للبيانات الوصفية المملوكة للمصرّف. المعنى التاريخي لجذع الصرف متقاعد؛ الصرف يبقى تخصص تسمية مصدري، وليس تصريفاً مولّداً من المصرّف. صيغ التوجيه المقبولة هي `@ radix lane "air"` / `"mir"` / `"hir-direct"` على دوال المستوى الأعلى لتوجيه مسار المصرّف الصريح؛ تركيبات المسار/الهدف غير المدعومة تُرفض بتشخيصات بدلاً من تجاهلها.
- `@ verte` يعرّف تحويل توليد الكود (اسم طريقة أو قالب)
- `@ nondum [TARGET] ["REASON"]` يعلم تصريحاً كموجود في واجهة لكن غير متاح للهدف
- `@ cli "NAME"` يعلم مدخل `بداية` كبرنامج CLI
- `@ imperium "NAME"` يعلم دالة كنقطة دخول أمر CLI
- `@ optio NAME ...` يعرّف خيار CLI؛ استخدم `نمط منطقي` للأعلام المنطقية
- `@ operandus [باقي] TYPE NAME ...` يعرّف وسيطاً موضعياً لـ CLI
- `@ مستقبلي` يعلم دالة كغير متزامنة
- `@ مؤشر` يعلم دالة كمولّد
- `@ عام` يعلّم التصدير، و`@ interna` داخلي الحزمة، و`@ خاص` علامة خصوصية صريحة؛ التصريحات العليا غير الموسومة خصوصية افتراضياً، وخلط مستويات الرؤية يعطي `SEM019`
- `@ محمي` محجوز ومرفوض بتشخيص دلالي؛ ليس له معنى رؤية حزمة أو صنف فرعي أو ملف شقيق

- `امتد` = يمتد، `حقق` = يحقق
- `سكوني` = static، `مرتبط` = bound/property

### الواجهات

```ebnf
implendumDecl   := 'عقد' IDENTIFIER typeParams? '{' implendumMethod* '}'
implendumMethod := annotation* 'دالة' IDENTIFIER '(' paramList ')' funcModifier* returnClause? alternateExitClause?
```

`عقد` هو بناء **العقد**: طرق بتوقيع فقط لـ `حقق` (صيغة المصدر المؤنث من *implere* — ما يجب تحقيقه). فضاءات أسماء الاستيراد هي حدود ملف `.fab`؛ التصريحات المصدرة تعيش في المستوى الأعلى للملف.

### أسماء الأنواع المستعارة

```ebnf
typeAliasDecl := 'نمط' IDENTIFIER genericParams? '=' typeAnnotation
```

### التعدادات

```ebnf
enumDecl   := 'ترتيب' IDENTIFIER '{' enumMember (',' enumMember)* ','? '}'
enumMember := IDENTIFIER ('=' ('-'? NUMBER | STRING))?
```

### الاتحادات الموسومة

```ebnf
discretioDecl := 'تمايز' IDENTIFIER typeParams? '{' variant (',' variant)* ','? '}'
variant       := IDENTIFIER ('{' variantFields '}')?
variantFields := (typeAnnotation IDENTIFIER (',' typeAnnotation IDENTIFIER)*)?
```

### تسمية المعرفات

أسماء الحالة المختلطة ذات الأحرف الأولى الصغيرة مقبولة نحوياً لكنها غير مفضلة في Faber للغة أو المكتبة القياسية أو مسارات المضيف أو واجهات API الجوهرية المملوكة للمصرّف. فضّل كلمة واحدة. إذا لم تستطع كلمة واحدة حمل المعنى، استخدم snake_case فقط في حالات نادرة. إذا لم ينجح أي شكل، فالطريقة على الأرجح لا تنتمي للسطح الأساسي ما لم تكن حرجة. تشفير/فك تشفير المكتبة القياسية يستخدم ثلاثي الأفعال الميكانيكية `pange` / `solve` / `tempta` عبر الوحدات — انظر `docs/stdlib/stdlib-mechanical-verbs.md`. مكتبة النصوص العامة هي `norma:chorda` — انظر `docs/stdlib/chorda-methods.md`.

### الاستيرادات

```ebnf
importDecl     := importRecord | importSugar
importRecord   := 'استورد' '{' importFieldList? '}'
importFieldList := importField (',' importField)* ','?
importField    := importSourceField | importVisibilityField | importNameField
                | importAliasField | importWildcardField
importSourceField := 'من' '=' STRING
importVisibilityField := 'visibilitas' '=' visibility
importNameField := 'nomen' '=' IDENTIFIER
importAliasField := 'كـ' '=' IDENTIFIER
importWildcardField := 'جميع' '=' IDENTIFIER

importSugar    := 'استورد' 'من' STRING visibility? (namedImport | wildcardImport)?
visibility    := 'عام'
namedImport   := IDENTIFIER ('كـ' IDENTIFIER)?
wildcardImport := '*' 'كـ' IDENTIFIER
```

مثال:

```fab
استورد من "hono" Hono
استورد من "hono" Context
استورد من "norma:chorda"                         # لا يعيد التصدير افتراضياً
استورد { من = "norma:json/solve", كـ = solve_mod }
استورد من "norma:consolum" consolum
استورد من "faber:*" faber              # kernel manifest glob
استورد من "lodash" * كـ _
استورد من "./types" عام User               # re-export
```

علامة الاستيراد `خاص` أزيلت (VM-U3): الاستيراد بدون علامة لا يعيد التصدير، و`عام` هي علامة إعادة التصدير. غياب الربط المسمى يفترض آخر مقطع من مسار الاستيراد عندما يكون معرفاً صالحاً غير متعارض. إذا كان الاسم المستنتج غير صالح أو يتصادم مع ربط موجود في المستوى الأعلى، اكتب ربط `nomen` أو `كـ` صريحاً.

`استورد من "faber:*" faber` هو اختزال خاص بالنواة: النجمة تعيش داخل سلسلة مسار الاستيراد وتوسع بيان النواة للإصدارة الثنائية إلى استدعاءات `faber.<module>.<verb>`. إنه ليس إعادة تصدير بحرف البدل ولا ينشئ قيمة تجميعية زمن التشغيل.

---

## الأنواع

```ebnf
typeAnnotation := ('عن' | 'في')? baseType ( '∪' typeAnnotation )*
baseType       := holeType | functionType | qualifiedType typeParams? | '(' typeAnnotation ')'
holeType       := '_' | '∪'
qualifiedType  := IDENTIFIER ('.' IDENTIFIER)*
functionType   := '(' typeList? ')' '→' typeAnnotation alternateExitClause?
typeList       := typeAnnotation (',' typeAnnotation)*
typeParams     := genericParams
```

- المصفوفات تُكتب `قائمة<T>`. اللاحق `T[]` غير مقبول.
- `عن`/`في` يعلمان الملكية (استعارة/استعارة-متغيرة) كبادئات على النوع.
- الاتحاد المضمن `T ∪ U` (كأس) لاتحادات القيم المخصصة؛ `T ∪ لاشيء` هو صيغة النوع القابل للإبطال القانونية (تخفض إلى Option<T>).
- الاتحادات ترابطية يميناً في القواعد لكن تُحلل بشكل مسطح؛ التكرارات وحالات `لاشيء` فقط تُشخّص في التخفيض الدلالي.
- `اختياري` هو علامة تصريح (بعد الاسم على المعاملات/الحقول)، ليس بادئة على الأنواع أبداً.
- مسارات الأنواع المؤهلة مثل `terminus.Terminus` تسمي نوعاً من خلال ربط فضاء أسماء مستورد. يجب أن تحل البادئة إلى فضاء أسماء؛ يجب أن يحل المقطع الأخير إلى تصريح حامل للنوع.

أنواع الدوال تمكّن توقيعات الدوال ذات الرتبة الأعلى:

```fab
دالة filtrata((T) → منطقي pred) → قائمة<T>
دالة compose((A) → B f, (B) → C g) → (A) → C
دالة apply((عدد) → عدد ⇥ نص op, عدد n) → عدد ⇥ نص
```

### الأنواع الأولية

| Faber      | المعنى |
| ---------- | ------ |
| `نص`   | سلسلة Unicode |
| `أسكي`    | سلسلة ASCII فقط |
| `forma`    | قالب ملتقط + معاملات |
| `عدد`  | عدد صحيح (افتراضي `i64`) |
| `حلقة<W>` | كلمة حلقية غير موقعة؛ الحساب يلتف بقياس 2^W |
| `كسر`  | عدد كسري (افتراضي `f64`) |
| `منطقي` | منطقي |
| `لاشيء`    | null |
| `فراغ`   | void |
| `أبدا`  | never |
| `مجهول`  | unknown |
| `بايتات`   | bytes |

الأوليات المقاسة تقبل **علامة عرض** اختيارية واحدة (وليس معامل نوع مستخدم):

| العائلة | العلامات | مثال غير صالح |
| ------- | -------- | ------------- |
| `عدد<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` | `عدد<f32>` → استخدم `كسر<f32>` |
| `كسر<W>` | `f16`, `f32`, `f64` | `كسر<i32>` → استخدم `عدد<i32>`؛ `bf16` مؤجل |
| `حلقة<W>` | `u8`, `u16`, `u32`, `u64` | `حلقة<i32>` → العروض الموقعة ليست كلمات حلقية |

`عدد` / `كسر` المجردان يبقيان اختزالاً لـ `عدد<i64>` / `كسر<f64>`.

`حلقة<W>` هو عائلة دلالية متميزة: الحساب لا يمتزج ضمنياً مع `عدد<W>`، بينما يبقى التحويل الصريح بنفس العرض متاحاً. يجب أن تكون الحرفيات في `0..=2^W-1` (لـ `حلقة<u64>` حتى `18446744073709551615`). تعدادات الإزاحة هي نفسها حلقية: `x ⇐ W` هو التفاف كامل. الحساب الحلقي عبر العروض مرفوض.

### المجموعات العامة

| Faber          | المعنى  |
| -------------- | ------ |
| `قائمة<T>`     | مصفوفة |
| `جدول<K,V>`  | خريطة  |
| `مجموعة<T>`     | مجموعة |
| `وعد<T>` | وعد    |
| `مؤشر<T>`    | مكرر   |
| `tensor<T, Figura>` | مخزن مؤقت كثيف متجانس بشكل ساكن `Figura`؛ الطرق العددية تتطلب أنواع عناصر عددية |
| `vector<T, N>` | متجه عددي فئة-سجل بعرض ساكن `N` (بعد واحد، ليس مدعوماً بمخزن مؤقت) |
| `matrix<T, [R, C]>` | مصفوفة عددية فئة-سجل ببعدين ساكنين بالضبط (ليست مدعومة بمخزن مؤقت وليست اسماً مستعاراً لـ tensor) |
| `atomic<T>` | خلية ذرية حساسة للتخزين؛ v1 تقبل عناصر `i32` / `u32` فقط ويجب أن يمر الوصول عبر طرق ذرية |
| `sparsa<T, Figura>` | مخزن مؤقت متفرق متجانس بشكل ساكن `Figura`؛ الإحداثيات المحذوفة تساوي صفراً؛ الطرق العددية تتطلب أنواع عناصر عددية |

`Figura` := `_` | natural | ident | `[` figura-list `]` (`[]` الفارغ هو رتبة-0). `tensor<T>` المجرد غير مكتمل — استخدم `tensor<T, []>` للرتبة-0 أو `tensor<T, _>` لاستنتاج الشكل.

`فارغ` لـ `tensor<T, []>` ينتج موتر رتبة-0 (فتحة عنصر واحدة مهيأة افتراضياً). `فارغ` لـ `sparsa<T, Figura>` (أي شكل) ينتج موتراً متفرقاً كله أصفار بدون مدخلات مخزنة. `matrix<T, Figura>` يتطلب بعدين بالضبط؛ `matrix<T>` المجرد وأشكال المصفوفة ذات محور واحد أو ثلاثة مرفوضة. `atomic<T>` يتطلب أن يكون `T` إما `i32` أو `u32` في v1. الخلايا الذرية غير قابلة للتبادل مع نوع عنصرها؛ استخدم طرق المستقبل `load` و `store` و `exchange` و `compare_exchange`. أنشئ الموترات متعددة الأبعاد عبر `crea` / `structa` / `↦`. `Type(...)` ليست صيغة إنشاء: `vector<f32, 4>(...)`، `matrix<f32, [2, 2]>(...)`، `tensor<f32, [2, 2]>(...)`، والصيغ العددية مثل `عدد("42")` مرفوضة. استخدم `value ↦ Type`، أو دوال المكتبة المسماة، أو سجلات `Genus { field = value }`.

فتحات المؤشر/الشكل الجوهرية للموتر (`accipe`، `ponde`، `forma`، `crea`، `structa`) تقبل قوائم أعداد صحيحة تناسب حد زمن التشغيل القانوني `قائمة<عدد>` / `&[i64]` في مواقع الاستدعاء (مثلاً `قائمة<u32>` لمعرفات خيوط GPU؛ وليس `قائمة<u64>`). هذا استثناء بنيوي محصور بتلك الفتحات — لا يوسع شبيكة الأعداد الموقعة↔غير الموقعة (انظر سياسة معامل متجه الفهرس في `tensor-intrinsics.md`).

اتحادات القيم تستخدم `T ∪ U` المضمن (قابل للإبطال: `T ∪ لاشيء`). الاتحادات الموسومة تستخدم `تمايز`. `مجموعة.unio()` هي طريقة مجموعة، وليس بناء نوع.

### اختزال الأنواع

اختزال الأنواع هو تهجئة بديلة للأنواع العددية وأنواع المجموعات. هو **في موضع النوع فقط** و **متطابق دلالياً** مع الصيغة الطويلة — يعامل المصرّف كلتيهما بنفس الطريقة. هذا هو المرجع القانوني الوحيد للاختزال؛ بقية المواصفات تستخدم الصيغة الطويلة.

يجمع الاختزال علامة عرض مع بادئة عائلة اختيارية من حرف واحد. علامات العرض هي `i8`/`i16`/`i32`/`i64` (موقعة)، `u8`/`u16`/`u32`/`u64` (غير موقعة)، و `f16`/`f32`/`f64` (كسرية). علامة عرض مجردة (بدون بادئة) تختزل النوع العددي العددي؛ بادئة عائلة تختزل مجموعة من ذلك العرض.

| اختزال | صيغة طويلة | قاعدة الأقواس |
| ------ | ---------- | ------------- |
| `i8` … `u64`, `f16`/`f32`/`f64` | `عدد<W>`, `كسر<W>` | لا شيء (علامة مجردة) |
| `lf32`, `lu32`, `li64`, … | `قائمة<f32>`, `قائمة<u32>`, `قائمة<i64>`, … | لا شيء |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | `Figura` اختياري |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | `Figura` اختياري |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 4>`, `vector<u32, 3>` | عرض واحد اختياري |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **مطلوب**، بعدين |

أشكال الأقواس: `[]` هي رتبة-0، `[2, 3]` هو شكل ثابت، وعدم وجود أقواس يستنتج الشكل (`_`). المصفوفة تتطلب بعدين بالضبط. الاختزال لا يستخدم `<>` أبداً. لأنواع العناصر غير العرضية (مثلاً `tensor<نص, [3]>`)، استخدم الصيغة الكاملة.

الاختزال محجوز في صياغة النوع فقط — معرفات القيم المسماة `tf32`، `lf32`، إلخ. تبقى دون تغيير.

`حلقة<W>` ليس له اختزال؛ اكتب `حلقة<u32>` بالكامل.

**تفضيل التهجئة (عرف المؤلف، وليس القواعد):** كود Faber العام يميل نحو الصيغة الطويلة للقراءة؛ الوحدات العددية/الأولية للموتر قد تفضل الاختزال. اختر لكل وحدة أو ملف.

---

## تدفق التحكم

### الشروط

```ebnf
ifStmt     := 'إذا' expression arm ('وإلاإذا' ifStmt | elseClause)?
elseClause := 'وإلا' elseArm
arm        := (blockStmt | stmtBodyJoint statement) catchClause?
elseArm    := (blockStmt | stmtBodyJoint statement) catchClause?
```

- `إذا` = if، `وإلاإذا` = else-if، `وإلا` = else
- `إذن` للأجسام ذات العبارة الواحدة، بما في ذلك `إذن أعد`، `إذن ارم`، `إذن انهر`، و `إذن صمت` (`∴` غير مقبول هنا)
- `صمت` للا-عمل الصريح (من التدوين الموسيقي: "يسكت")

### الحلقات

```ebnf
whileStmt  := 'طالما' expression (blockStmt | stmtBodyJoint statement) catchClause?
iteraStmt  := 'كرر' (('من' | 'عن') expression | 'ab' expression) ('ثابت' | 'متغير') IDENTIFIER (blockStmt | stmtBodyJoint statement) catchClause?
```

- `طالما` = while
- `كرر من...ثابت`/`كرر من...متغير` = for-of (القيم)
- `كرر عن...ثابت`/`كرر عن...متغير` = for-in (المفاتيح)
- `كرر ab range ثابت/متغير i` = تكرار نطاق (مثلاً `كرر ab 0‥10 كل 2 ثابت i { اعرض i }`؛ `كل` ينتمي إلى تعبير النطاق)

### التبديل/المطابقة

```ebnf
eligeStmt    := 'اختر' expression '{' eligeCase* defaultCase? '}' catchClause?
eligeCase    := 'حالة' expression (blockStmt | stmtBodyJoint statement)
defaultCase  := 'افتراضي' (blockStmt | stmtBodyJoint statement)
```

### مطابقة الأنماط

```ebnf
discerneStmt := 'طابق' 'جميع'? discriminants '{' variantCase* defaultCase? '}'
discriminants := expression (',' expression)*
variantCase  := 'حالة' patterns (blockStmt | stmtBodyJoint statement)
patterns     := pattern ((',' | 'و') pattern)*
pattern      := '_' | literal | (IDENTIFIER patternBind?)
patternBind  := ('كـ' IDENTIFIER) | (('ثابت' | 'متغير') patternBinding (',' patternBinding)*)
patternBinding := IDENTIFIER ('كـ' IDENTIFIER)?
```

### الحراس

```ebnf
guardStmt   := 'احرس' '{' guardClause+ '}'
guardClause := 'إذا' expression (blockStmt | stmtBodyJoint statement)
```

### إدارة الموارد

```ebnf
curaStmt    := 'اعتن' STRING ('ثابت' | 'متغير') typeAnnotation IDENTIFIER blockStmt catchClause?
```

### استخراج التفكيك

```ebnf
extractStmt   := 'من' expression ('ثابت' | 'متغير') extractFields
extractFields := extractField (',' extractField)* (',' restField)? | restField
extractField  := IDENTIFIER ('كـ' IDENTIFIER)?
restField     := 'باقي' IDENTIFIER
```

### نقل التحكم

```ebnf
returnStmt   := 'أعد' expression?
breakStmt    := 'اكسر'
continueStmt := 'تابع'
noopStmt     := 'صمت'
```

---

## معالجة الأخطاء

```ebnf
throwStmt   := ('ارم' | 'انهر') expression ['إذا' expression]
catchClause := 'التقط' IDENTIFIER blockStmt
assertStmt  := 'أكد' expression ('secus' expression)?
requiritStmt := 'يتطلب' expression 'secus' expression
```

- `التقط` يرتبط بالعبارات المهيكلة وأذرع الشروط. لا يرتبط بالكتل المجردة الاعتباطية.
- `افعل { ... } التقط err { ... }` هو حد الخطأ المحلي القابل للاسترداد القانوني أحادي اللقطة.
- `حاول` هو سطح try/catch قديم ومرفوض بتشخيص ترحيل.
- `أخيرا` هو سطح finally قديم ومرفوض بتشخيص ترحيل.
- `ارم` = throw (قابل للاسترداد)، `انهر` = panic (قاتل).
- حارس `إذا <expr>` الاختياري على `ارم` و `انهر` هو اختزال محلل: `ارم val إذا cond` يخفض إلى `إذا cond { ارم val }` زمن التحليل. بدون الحارس، العبارة غير مشروطة (سلوك غير متغير).
- `أكد` فحص ثابت وقت التشغيل. يتحول إلى `انهر "msg" إذا !cond` مع إبقاء الشرط في صيغته الموجبة. `secus` يقدم رسالة المسار الخاطئ، مما يعكس دورها في `si/secus` والثلاثي `sic/secus`.

---

## التعابير

### العوامل (حسب الأسبقية، من الأدنى إلى الأعلى)

```ebnf
expression := assignment
assignment := ternary ('←' assignment | '↤' assignment inlineRecovery?)?
incDecStmt := place ('↑' | '↓')
ternary    := or (('?' expression ':' | 'فإذا' expression 'وإلا') ternary)?
or         := and (('أو') and)*
and        := equality (('و') equality)*
equality   := comparison (('≡' | '≠' | '≈' | '≉' | 'هو' | 'ليس' 'هو') comparison)*
comparison := bitwiseOr (('<' | '>' | '≤' | '≥' | 'ضمن' | 'بين') bitwiseOr)*
# عوامل الترتيب تستخدم رموز Unicode؛ العضوية تستخدم الكلمات المفتاحية `ضمن`/`بين`
# (هوية نثر Faber). الأسماء المستعارة الرمزية مثل `∈` ليست في العقد النشط.
bitwiseOr  := bitwiseXor ('∨' bitwiseXor)*
bitwiseXor := bitwiseAnd ('⊻' bitwiseAnd)*
bitwiseAnd := shift ('∧' shift)*
shift      := range (('⇐' | '⇒') range)*
range      := additive (('‥' | '…' | 'قبل' | 'حتى') additive ('كل' additive)?)?
additive   := multiplicative (('+' | '-') multiplicative)*
multiplicative := coalesce (('*' | '/' | '%') coalesce)*
# `عوض` هو إزالة محلية قابلة للإبطال (`T ∪ لاشيء عوض T → T`)، وليس `أو` منطقياً.
# يرتبط بقوة أكبر من الحساب لذا `prefix + item عوض ""` هو `prefix + (item عوض "")`.
# الجانب الأيمن لـ `عوض` قد يكمل بناء فترة (`maybeRange عوض 0‥0`).
coalesce   := unary ('عوض' velRhs)*
velRhs     := unary (('‥' | '…' | 'قبل' | 'حتى') unary ('كل' unary)?)?
unary      := ('-' | '¬' | 'ليس' | 'سلم' | 'أنشئ') unary | cast
cast       := call ('∷' typeAnnotation | conversio)*
conversio        := '↦' typeAnnotation typeParams? inlineRecovery?
inlineRecovery   := '⇥' unary
```

`↤` هي إسناد موجّه للتحويل: تُقيَّم الجهة اليمنى، ثم تُحوَّل إلى النوع الثابت للجهة اليسرى عبر مسار `↦`، ثم يُسنَد إليها. `⇥` للاسترداد الداخلي مسموح فقط بعد `↤`، وليس بعد `←`.

كلمات المسند المتقاعدة ليست صياغة أحادية بادئة. استخدم `expr هو صواب`، `expr هو خطأ`، `expr هو لاشيء`، `expr ليس هو لاشيء`، `expr < 0`، أو `expr > 0`.

**إسناد النوع الساكن (`∷` / verte):**

الرمز `∷` (U+2237، "تناسب") يسند نوع هدف بشكل صريح لتعبير. استخدمه عندما يكون تعبير المصدر موجوداً بالفعل ويحتاج المصرّف إلى شكل هدف ساكن:

- أولي/اسم مستعار → تحويل (بدون تأثير زمن تشغيل): `data ∷ نص` → TypeScript: `(data as string)`
- مجموعة مدمجة → قيمة مجموعة بشكل الهدف: `[1, 2, 3] ∷ قائمة<عدد>`
- تعبير متغير → هدف تعداد/واجهة: `أنشئ Click { x = 10 } ∷ Event`

فضّل الإنشاء المنمط لقيم `صنف` العادية و `فارغ` لقيم المجموعات الفارغة العادية:

```fab
ثابت _ point ← Point { x = 10 }
ثابت قائمة<عدد> xs ← فارغ
```

فقط الرمز `∷` مقبول كعامل إسناد نوع ساكن لاحق. الصيغ اللاتينية `qua`، `innatum`، و `novum` كانت أسماء مستعارة وأُزيلت (انظر verte-alias-clean-break).

**التحويل زمن التشغيل (`↦` / conversio):**

الرمز `↦` (U+21A6، "سهم يمين من شريط") هو عامل تحويل القيم زمن التشغيل. على عكس `∷` (تحويل زمن التصريف)، هذا ينفذ تحليل/تحويل فعلي يمكن أن يفشل:

- `"22" ↦ عدد` → Rust: `"22".parse::<i64>().unwrap()`
- `"bad" ↦ عدد ⇥ 0` → Rust: `"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ نص` → Rust: `42.to_string()`

استرداد الفشل المضمن يستخدم `⇥` مباشرة بعد هدف conversio (`↦ T ⇥ recovery-expr`). يجب أن يكون تعبير الاسترداد قيمة من النوع `T`.

استخدام `عوض` كاسترداد conversio مرفوض بتشخيص ترحيل. `عوض` هو إزالة محلية قابلة للإبطال فقط (`x عوض y`، افتراضيات المعاملات) — وليس `أو` منطقياً. نتيجة conversio المؤشرة قد تظل تتحد مع `عوض` كافتراض عادي.

### الاستدعاء والوصول إلى الأعضاء

```ebnf
call          := primary (callSuffix | memberSuffix | optionalSuffix | nonNullSuffix)*
callSuffix    := typeArgs? '(' argumentList ')'
memberSuffix  := '.' IDENTIFIER | '[' expression ']'
optionalSuffix := '?.' IDENTIFIER | '?[' expression ']' | '?(' argumentList ')'
nonNullSuffix := '!.' IDENTIFIER | '![' expression ']' | '!(' argumentList ')'
argumentList  := (argument (',' argument)*)?
argument      := 'انشر'? expression
```

### حرفيات السلاسل والقوالب

تستخدم Faber **دلالات المحددات**: كل صيغة اقتباس تعني شكلاً مصدرياً مختلفاً. إنها ليست مترادفات قابلة للتبادل.

| الصيغة | النوع | الدور |
| ------ | ---- | ----- |
| `'...'` | `أسكي` | رموز آلة ثابتة؛ لا `§`؛ لا `(...)` |
| `"..."` | `نص` | سلاسل Unicode قصيرة أحادية السطر؛ `(...)` يعرض |
| `«...»` | `نص` | كتلة/متعدد الأسطر Unicode؛ `(...)` يعرض |
| `` `...` `` | `forma` | قوالب ملتقطة؛ `(...)` يلتقط |
| `{ ... }` | `جسون` | مستند JSON كائني الجذر زمن التصريف (`:` بالداخل) |
| `\|...\|` | `بايتات` | بايتات ست عشرية زمن التصريف |
| `"..." ↦ تعبير` | `تعبير` | نمط مجمع من تحويل نص |
| `[ ... ]` | `قائمة<T>` | قائمة Faber (ليست مصفوفة JSON، وليست بايتات) |

`§` (U+00A7) هو ثقب قالب في صيغ Unicode (`"`، `«`، `` ` ``). لا يمكن أن يظهر في حرفيات `أسكي`.

**القوالب المعروضة** (`نص`): `"..."(...)` و `«...»(...)` تخفض إلى `حرر("...", args...)`.

**القوالب الملتقطة** (`forma`): `` `...`(args) `` يلتقط نص القالب ومعاملاته بدون عرض. آمن لحمولات SQL/URL المرتبطة؛ لا تستخدم `«...»(...)` لهذه المهمة.

كتلة `نص` تستخدم guillemets `«...»`. زوج علامات الاقتباس الثقيلة متقاعد (قريب بصرياً جداً من `"` في العديد من الخطوط).

حالة التنفيذ (2026-06-30):

- تم الشحن: `"..."`، `«...»` كتلة `نص`، `'...'` → `أسكي`، `` `...` `` → `forma`، `|...|` → `بايتات`، `{ ... }` → `جسون`، و text/ascii `↦ تعبير`.
- قيد التسليم: حرفيات regex بشرطة مائلة `/.../`.

مثال كتلة مضمنة:

```fab
ثابت _ tag ← «inline»
```

مثال كتلة متعددة الأسطر (سطر جديد بعد `«` الافتتاحية):

```fab
ثابت _ blob ← «
    select id, email
    from accounts
»
```

مثال قالب ملتقط:

```fab
ثابت _ q ← `select * from accounts where id = §`(accountId)
```

مثال حرفية بايتات ست عشرية:

```fab
ثابت _ sig ← |de ad be ef|
ثابت _ hello ← |48 65 6c 6c 6f|
```

### تطبيق قالب التنسيق

صيغة استدعاء حرفية السلسلة هي الصيغة المصدرية القانونية لتطبيق قالب التنسيق:

```fab
"status: § (§)"(sample_status(), "ok")
"status: §1 (§0)"("ok", sample_status())
```

هذا يخفض إلى صيغة المصرّف `حرر("...", args...)`. استخدم صيغة قالب السلسلة في المصدر العادي؛ احجز `حرر(...)` لأمثلة إزالة الاختزال الصريحة وتوثيق واجهة المصرّف.

لـ `نص`، فهرسة الأقواس مبنية على Unicode scalar:

```fab
"Salve, §!"[7]            # "§"
"hello world"[0‥5]        # "hello"
"hello world"[0 حتى 10] # "hello world"
"abcdef"[0‥6 كل 2]      # "ace"
```

شرائح النص تقبل صيغة النطاق الكاملة، بما في ذلك `كل`.

لـ `قائمة<T>`، فهرسة الأقواس هي وصول لعنصر واحد. يجب أن يكون الفهرس عدداً صحيحاً واحداً؛ شرائح النطاق غير مقبولة (استخدم `sectio(start, end)` لنطاق منسوخ):

```fab
xs[i]        # element at position i
xs[i] ← v    # write element at position i
```

وصول قوس قائمة هو **عادي**، وليس قابلاً للإبطال: يعيد العنصر المجرد `T` ويوقع على الخروج عن الحدود. هذا يختلف عن `tensor`، الذي تكون قراءة قوسه اختزال `accipe` وتعيد `T ∪ لاشيء`. لوصول القائمة القابل للإبطال، استخدم `xs.accipe(i) → T ∪ لاشيء` مع `عوض`.

لـ `tensor<T, Figura>`، فهرسة الأقواس هي اختزال على سطح الموتر الجوهري:

```fab
vector[id]        # vector.accipe([id])
vector[id] ← v    # vector.ponde([id], v)
grid[[r, c]]      # grid.accipe([r, c])
grid[[r, c]] ← v  # grid.ponde([r, c], v)
```

القراءات تعيد `T ∪ لاشيء`، مطابقة لـ `accipe`؛ استخدم `عوض` أو صيغة معالجة خيار عادية أخرى قبل الحساب. موترات الرتبة-1 تقبل فهارس عددية صحيحة تناسب حد زمن التشغيل `i64` للموتر (`u64` مرفوض). موترات الرتبة-N تستخدم تعبير فهرس بشكل قائمة مثل `[[r, c]]` أو قيمة `قائمة<integer>` مرتبطة. `grid[r, c]` ليس صياغة؛ `memberSuffix` لا يزال يحتوي على `expression` واحد بالضبط بين الأقواس.

`بايتات` هو أولي مخزن بايتات مؤقت، وليس مصفوفة، لذا فهرسة الأقواس غير مقبولة عليه (قراءة أو كتابة). وصول البايت معتمد على الطرق:

```fab
buf.accipe(i)      # → عدد<u8> ∪ لاشيء (nullable; safe on out-of-bounds)
buf.appende(b)     # append one byte in place
buf.longitudo      # byte length
```

هذا مقصود. `بايتات` هو المخزن المؤقت للبايتات الحدودي المعتم المستخدم بواسطة HAL، والتشفير، وحرفيات `|hex|`؛ قراءاته قابلة للإبطال افتراضياً، وصياغة الأقواس محجوزة لنموذج الوصول الموقع. للفهرسة الثقيلة بالبايتات، استخدم `قائمة<عدد<u8>>` داخلياً (قراءة/كتابة بالأقواس، توقع على الخروج عن الحدود) وأبق `بايتات` عند الحدود.

### التعابير الأولية

`فارغ` هو علامة مجموعة فارغة سياقية (صيغة معرف، وليس كلمة مفتاحية محجوزة). استخدمها مع نوع مجموعة صريح: `ثابت قائمة<عدد> xs ← فارغ` أو `ثابت tensor<كسر<f32>, []> t ← فارغ`.

```ebnf
primary := IDENTIFIER | NUMBER | STRING | ASCII_STRING | BACKTICK_STRING
         | 'ذات' | 'صواب' | 'خطأ' | 'لاشيء'
         | 'فارغ' | arrayLiteral | jsonLiteral | typedConstructor
         | adExpr
         | '(' expression ')'
adExpr    := 'اتصل' asciiLiteral adOpener?
arrayLiteral := '[' argumentList? ']'
# `{ ... }` المجرد هو حرفية مستند JSON. المفاتيح هي سلاسل JSON مقتبسة مفصولة
# بـ `:`؛ القيم هي ثوابت JSON. كائنات Faber المجهولة (`{ key = expr }`)
# متقاعدة (literal-family Stage 6). إنشاء صنف يستخدم `typedConstructor`.
jsonLiteral := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonMember  := STRING ':' jsonValue
typedConstructor := typeAnnotation '{' fieldList? '}'
fieldList := fieldInit (',' fieldInit)*
fieldInit := ('انشر' expression) | (fieldKey '=' expression) | IDENTIFIER
fieldKey := IDENTIFIER | STRING | '[' expression ']'
# قيم JSON: ثوابت فقط (لا تعابير Faber، لا مراجع متغيرات).
jsonValue := jsonObject | jsonArray | jsonString | jsonNumber | 'true' | 'false' | 'null'
jsonObject := '{' (jsonMember (',' jsonMember)* ','?)? '}'
jsonArray  := '[' (jsonValue (',' jsonValue)* ','?)? ']'
jsonString := STRING
jsonNumber := NUMBER                       # عدد عندما لا يكون هناك '.'/e/E، وإلا كسر
```

`STRING` تشمل السلاسل القصيرة المحددة بـ `"` وسلاسل الكتلة المحددة بـ `«` و `»`. `'...'` (`أسكي`) والاقتباس الخلفي `` `...` `` (`forma`) هما صيغ حرفية منفصلة (انظر حرفيات السلاسل والقوالب أعلاه).

`{ ... }` المجرد ينتج الآن مستند JSON كائني الجذر من النوع `جسون`: `{ "name": "Alice", "age": 30, "active": true }`. المفاتيح هي سلاسل JSON مقتبسة مفصولة بـ `:`؛ القيم هي ثوابت JSON فقط. المفاتيح المكررة خطأ (التكرار الثاني). الإسناد إلى `جدول<K,V>` يخفض خريطة ثابتة حقيقية. استخدم `↦ قيمة` للتوسيع الصريح إلى الحامل الديناميكي العريض. إنشاء صنف/variant `Type { field = expr }` يستخدم قواعد Faber `=` بدون تغيير.

### التعابير الخاصة

```ebnf
// verte (∷) هو لاحق — محلل في إنتاج cast أعلاه
fingeExpr     := 'أنشئ' IDENTIFIER ('{' fieldList '}')? ('∷' IDENTIFIER)?
praefixumExpr := 'بادئة' (blockStmt | '(' expression ')')
formatStringExpr := STRING '(' argumentList ')'                # يعرض نص عبر حرر
formaTemplateExpr := BACKTICK_STRING '(' argumentList ')'      # يلتقط forma
scriptumExpr  := 'حرر' '(' STRING (',' expression)* ')'   # صيغة صريحة/مزالة الاختزال
legeExpr      := 'اقرأ' 'سطرا'?
regexFromText   := (STRING | ASCII_STRING) '↦' 'تعبير'
# حرفيات regex بشرطة مائلة ليست قواعد نشطة بعد. `/` يحلل كعامل قسمة،
# بينما `//` و `/* ... */` مرفوضان كتعليقات غير صالحة.
# 'sed' STRING [IDENT] القديم أزيل؛ استخدم "..." ↦ تعبير.
```

---

## الأنماط

```ebnf
objectPattern  := '{' patternProperty (',' patternProperty)* '}'
patternProperty := 'باقي'? IDENTIFIER ('كـ' IDENTIFIER)?
arrayPattern   := '[' arrayPatternElement (',' arrayPatternElement)* ']'
arrayPatternElement := '_' | 'باقي'? IDENTIFIER
```

---

## التشخيصات

```ebnf
outputStmt := ('اعرض' | 'شاهد' | 'نبه' | 'اكتب') expression (',' expression)*
```

- `اعرض` = ملاحظة تشخيصية محايدة، `شاهد` = تصحيح/فحص، `نبه` = تحذير
- `اكتب` هو تهجئة قناة تشخيصية؛ استخدم طرق المكتبة القياسية الحالية للإخراج الحقيقي

### التعليقات

يقبل Faber **تعليقات السطر فقط**: `#` حتى نهاية السطر. يجب أن يكون `#` أول رمز غير مسافة بيضاء على السطر المنطقي (مسافات ASCII أو ألسنة بادئة اختيارية فقط — فواصل المسافات Unicode الأخرى لا يتخطاها المحلل المعجمي). `#` الذي يتبع أي رمز آخر على نفس السطر هو **خطأ معجمي** برسالة `# comments must start a line; move this comment above the code`.

التعليقات الصالحة في بداية السطر ترتبط للأمام كـ `leading_trivia` على العبارة أو التصريح التالي (انظر الحفاظ على التعليقات). `#` داخل حرفيات السلاسل، حرفيات `أسكي`، قوالب `forma`، والحرفيات المحددة الأخرى **ليس** تعليقاً.

---

## نقاط الدخول

```ebnf
incipitStmt  := 'بداية' blockStmt
incipietStmt := 'استهلال' blockStmt
```

- `بداية` = دخول متزامن، `استهلال` = دخول غير متزامن

---

## الاختبار

```ebnf
probandumDecl := 'مختبر' STRING probaModifier* '{' probandumBody '}'
probandumBody := (praeparaBlock | probandumDecl | probaStmt)*
probaStmt     := 'اختبر' STRING probaModifier* blockStmt
probaModifier := 'أهمل' STRING | 'مستقبلي' STRING | 'فقط' | 'وسم' STRING
              | 'زمني' NUMBER | 'قس' | 'معاد' NUMBER | 'هش' NUMBER
              | 'يتطلب' STRING | 'حصري' STRING
praeparaBlock := ('جهز' | 'سيهيئ' | 'لاحق' | 'سيلحق') 'جميع'? blockStmt
```

---

## إطار عمل CLI

```ebnf
cliDecl       := annotation* (incipitStmt | funcDecl)
cliAnnotation := cliProgramAnnotation | imperiumAnnotation | optioAnnotation | operandusAnnotation
```

يدعم Faber بناء تطبيقات CLI مع تحليل آلي للوسائط وتوليد المساعدة.

### نقطة دخول CLI

```fab
@ cli "faber"
@ optio verbose longum "verbose" نمط منطقي
بداية وسائط args {
    # CLI framework automatically parses arguments
}
```

### خيارات ووسائط CLI

```fab
@ imperium "deploy"
@ optio target brevis "t" longum "target" نمط نص descriptio "Deployment target"
@ optio verbose brevis "v" longum "verbose" نمط منطقي descriptio "Enable verbose output"
@ operandus نص file descriptio "File to deploy"
دالة deploy() وسائط args {
    # Arguments automatically parsed and passed
}
```

---

## استدعاءات القدرة

صيغة التعبير `اتصل` هي سطح `اتصل` الوحيد المدعوم. `اتصل "route" (args) → T { }` المنمط القديم وتدفقات مستوى العبارة `اتصل 'route' { meus/tuus … }` مرفوضة زمن التحليل.

```ebnf
adExpr        := 'اتصل' asciiLiteral adOpener?
adOpener      := '(' expression ')'
conversioExpr := expr '↦' typeAnnotation
```

- المسار: `asciiLiteral` (`'solum:lege'`)، وليس `STRING` باقتباس مزدوج.
- الفاتح: `expression` واحد اختياري → بيانات الطلب كـ `قيمة`.
- **تعبير `اتصل`**: بدون كتلة؛ يُقيّم إلى مقبض محادثة `sermo`. استخدم اللاحق `↦ T` (تجسيد)، أو أسند إلى `sermo`، أو افتح عروضاً اتجاهية حية: `s.meus<T>()` (صادر `da` / `fini`) و `s.tuus<T>()` (وارد `accipe` / `مؤشر` / `exhauri` / `fini`). كرر إطارات المحتوى الوارد بـ `s.tuus<T>().مؤشر()`، وليس `كرر من s.tuus<T>()` المباشر.
- **أزيل (خطأ تحليل):** `اتصل "route"` المنمط القديم، أذرع `meus`/`tuus` الكتلية، و `أرسل` مستوى العبارة.
- الأنواع: `scrinium` مملوك للمصرّف، `status`؛ `sermo` معتم كمقبض محادثة.
- `sermo ↦ T` يجسد الإطارات الواردة إلى قيمة واحدة من النوع `T` باستخدام المجمع الموجه بالنوع لـ `T`.

انظر [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md) و [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md).

---

## عمليات المجموعات

خط أنابيب المجموعات `ab` السابق متقاعد. تصفية المجموعات، تقطيعها، وتجميعها يُعبّر عنها عبر طرق `نص`/`قائمة`/`جدول`/`مجموعة` العادية والإغلاقات بدلاً من تعبير استعلام مستوى القواعد. `نص`، `عدد`، `كسر`، `قائمة<T>`، `جدول<K,V>`، و `مجموعة<T>` هي أنواع أساسية مملوكة للمصرّف؛ أسطح طرقها القانونية متتبعة في `docs/design/textus-intrinsics.md`، `docs/design/numerus-intrinsics.md`، `docs/design/fractus-intrinsics.md`، `docs/design/lista-intrinsics.md`، `docs/design/tabula-intrinsics.md`، و `docs/design/copia-intrinsics.md`، وليس في تصريحات Norma.

`prima` و `ultima` هما اسما طريقة عاديان، وليسا كلمتين تحويليتين مفتاحيتين. `ubi` ليس صياغة مجموعة نشطة.

`من` يُستخدم للتكرار (`كرر من items ثابت x`) والاستيرادات (`استورد من "path"`).

---

## كتلة Fac

```ebnf
facBlockStmt := 'افعل' blockStmt catchClause? ('طالما' expression)?
```

- `افعل { ... }` ينفذ الكتلة المحدودة مرة واحدة.
- `افعل { ... } التقط err { ... }` هو حد الخطأ المحلي القابل للاسترداد القانوني.
- `افعل { ... } طالما condition` هو صيغة حلقة ما بعد الاختبار؛ `طالما` اللاحق يرتبط فقط بـ `افعل`، وليس بالكتل السابقة الاعتباطية.

---

## دعم الأهداف

دعم الأهداف **ليس** جزءاً من القواعد — هذا الملف يعرّف اللغة فقط. لأي قواعد يخفضها كل هدف تصريف، وسياسة زمن التشغيل حولها، انظر:

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — مصفوفة قابلية تخفيض القواعد×الهدف المولّدة (الصفوف الرسمية).
- [`docs/design/target-capability-matrix.md`](docs/design/target-capability-matrix.md) — سياسة زمن التشغيل/العقد (مسح/تحذير/تأجيل)، توجيه خط الأنابيب، عقود لكل هدف.

---

## مرجع الكلمات المفتاحية

| الفئة              | Faber                         | المعنى              |
| ------------------ | ----------------------------- | ------------------- |
| **التصريحات**      | `تمايز`                   | اتحاد موسوم         |
|                    | `ثابت`                       | const               |
|                    | `دالة`                     | function            |
|                    | `صنف`                       | class               |
|                    | `عقد`                   | interface contract  |
|                    | `حجم`                   | size/index generic parameter (في قوائم `<>`) |
|                    | `ترتيب`                        | enum                |
|                    | `ليكن`                         | inferred immutable local |
|                    | `اختياري`                      | optional declaration slot (post-name) |
|                    | `نمط`                       | type alias          |
|                    | `فارغ`                       | contextual empty collection marker |
|                    | `متغير`                       | let                 |
| **تدفق التحكم**    | `إذا` / `وإلاإذا` / `وإلا`        | if / else-if / else |
|                    | `احرس`                     | guard               |
|                    | `طابق`                    | pattern match       |
|                    | `طالما`                         | while               |
|                    | `اختر` / `حالة`              | switch / case       |
|                    | `افعل`                         | scoped block / local error boundary |
|                    | `كرر من...ثابت`            | for-of (values)     |
|                    | `كرر عن...ثابت`            | for-in (keys)       |
|                    | `كرر ab...ثابت`            | range iteration     |
|                    | `تابع`                       | continue            |
|                    | `أعد`                       | return              |
|                    | `اكسر`                       | break               |
|                    | `صمت`                       | no-op (silence)     |
|                    | `إذن`                        | compact one-statement body joint |
|                    | `∴`                           | compact clausura joint only |
| **معالجة الأخطاء** | `التقط`                        | structured local handler |
|                    | `أكد`                     | assert              |
|                    | `يتطلب`                   | require (recoverable) |
|                    | `ارم`                        | throw               |
|                    | `يرمي`                       | throws modifier     |
|                    | `انهر`                        | panic               |
|                    | `افعل` / `التقط`                | local recoverable-error boundary |
| **غير متزامن**     | `@ مستقبلي`                    | async annotation    |
|                    | `@ مؤشر`                    | generator annotation |
|                    | `سلم`                        | await/yield by context |
| **النقاط الطرفية** | `اتصل`                          | capability call expression |
|                    | `أرسل`                      | retired statement-level frame emit |
| **منطقي**          | `صواب`                       | true                |
|                    | `أو`                         | or                  |
|                    | `و`                          | and                 |
|                    | `خطأ`                      | false               |
|                    | `ليس`                         | not                 |
|                    | `عوض`                         | local nullable defaulting |
| **الكائنات**       | `ذات`                         | this/self           |
|                    | `أنشئ`                       | construct variant   |
| **شكل النوع**      | `∷` | static type ascription / compile-time cast |
| **تحويل النوع**    | `↦ target`                    | runtime value conversion |
|                    | `↦ T ⇥ expr`                  | conversio with inline recovery of type `T` |
|                    | `↦ عدد`                   | parse to integer    |
|                    | `↦ كسر`                   | parse to float      |
|                    | `↦ نص`                    | convert to string   |
|                    | `↦ منطقي`                  | convert to boolean  |
| **بتي**            | `∧` / `∨` / `⊻` / `¬`         | and/or/xor/not      |
|                    | `⇐` / `⇒`                     | left/right shift    |
| **تشخيصات**        | `اعرض`                        | neutral note        |
|                    | `نبه`                        | warn                |
|                    | `اكتب`                      | diagnostic channel  |
|                    | `شاهد`                        | debug/inspect       |

---

## قواعد نحوية حرجة

1. **معاملات النوع أولاً**: `دالة f(عدد x)` وليس `دالة f(x: عدد)`
2. **تصريحات النوع أولاً**: `ثابت نص name` وليس `ثابت name: نص`
3. **حلقات التكرار**: `كرر من/عن collection ثابت/متغير item { }` أو `كرر ab range ثابت/متغير item { }` (الفعل أولاً، المصدر، ثم الربط)
4. **الأقواس حول الشروط صالحة لكنها غير اصطلاحية**: فضّل `إذا x > 0 { }` أو `إذا flag هو صواب { }` على `إذا (x > 0) { }`
5. **كلمات التشخيص المفتاحية هي عبارات**، وليست دوال — `اعرض x` تعمل، `اعرض(x)` تعمل أيضاً (الأقواس تجمع التعبير)، لكن `اعرض` ليست قيمة قابلة للاستدعاء

---

## قائمة مصطلحات حزمة القارئ (استخراج آلي)

### الكلمات المفتاحية

| لاتيني | معرّب |
|---|---|
| `abstractus` | مجرد |
| `ad` | اتصل |
| `adfirma` | أكد |
| `ante` | قبل |
| `argumenta` | وسائط |
| `aut` | أو |
| `cape` | التقط |
| `casu` | حالة |
| `cede` | سلم |
| `ceteri` | باقي |
| `ceterum` | افتراضي |
| `clausura` | إغلاق |
| `cura` | اعتن |
| `curata` | مخصص |
| `custodi` | احرس |
| `de` | عن |
| `discerne` | طابق |
| `discretio` | تمايز |
| `dum` | طالما |
| `ego` | ذات |
| `elige` | اختر |
| `ergo` | إذن |
| `errata` | مخطئ |
| `est` | هو |
| `et` | و |
| `ex` | من |
| `exitus` | مخرج |
| `fac` | افعل |
| `falsum` | خطأ |
| `finge` | أنشئ |
| `fixum` | ثابت |
| `fragilis` | هش |
| `functio` | دالة |
| `futurum` | مستقبلي |
| `generis` | سكوني |
| `genus` | صنف |
| `iace` | ارم |
| `iacit` | يرمي |
| `immutata` | ثابتة |
| `interna` | داخلي |
| `implet` | حقق |
| `implendum` | عقد |
| `importa` | استورد |
| `in` | في |
| `incipiet` | استهلال |
| `incipit` | بداية |
| `inter` | بين |
| `intra` | ضمن |
| `itera` | كرر |
| `lege` | اقرأ |
| `lineam` | سطرا |
| `magnitudo` | حجم |
| `metior` | قس |
| `modulus` | حلقة |
| `mone` | نبه |
| `mori` | انهر |
| `negativum` | سلبي |
| `nexum` | مرتبط |
| `nihil` | لاشيء |
| `non` | ليس |
| `nonnihil` | موجود |
| `nonnulla` | مليء |
| `nota` | اعرض |
| `nulla` | خال |
| `omitte` | أهمل |
| `omnia` | جميع |
| `optiones` | خيارات |
| `ordo` | ترتيب |
| `per` | كل |
| `perge` | تابع |
| `positivum` | موجب |
| `postpara` | لاحق |
| `postparabit` | سيلحق |
| `prae` | سابق |
| `praefixum` | بادئة |
| `praepara` | جهز |
| `praeparabit` | سيهيئ |
| `privata` | خاص |
| `proba` | اختبر |
| `probandum` | مختبر |
| `protecta` | محمي |
| `publica` | عام |
| `redde` | أعد |
| `repete` | معاد |
| `requirit` | يتطلب |
| `rumpe` | اكسر |
| `scribe` | اكتب |
| `scriptum` | حرر |
| `secus` | وإلا |
| `si` | إذا |
| `sic` | فإذا |
| `sin` | وإلاإذا |
| `sit` | ليكن |
| `solum` | فقط |
| `solum_in` | حصري |
| `sparge` | انشر |
| `sponte` | اختياري |
| `sub` | امتد |
| `tacet` | صمت |
| `tag` | وسم |
| `temporis` | زمني |
| `typus` | نمط |
| `usque` | حتى |
| `ut` | كـ |
| `varia` | متغير |
| `vel` | عوض |
| `verum` | صواب |
| `vide` | شاهد |

### الأنواع

| لاتيني | معرّب |
|---|---|
| `ascii` | أسكي |
| `bivalens` | منطقي |
| `copia` | مجموعة |
| `cursor` | مؤشر |
| `fractus` | كسر |
| `ignotum` | مجهول |
| `instans` | لحظة |
| `json` | جسون |
| `lista` | قائمة |
| `modulus` | حلقة |
| `nihil` | لاشيء |
| `numerus` | عدد |
| `numquam` | أبدا |
| `objectum` | كائن |
| `octeti` | بايتات |
| `promissum` | وعد |
| `quidlibet` | مهما |
| `regex` | تعبير |
| `tabula` | جدول |
| `textus` | نص |
| `vacuum` | فراغ |
| `valor` | قيمة |

### تغييرات القائمة مقابل الحزمة الحالية

| لاتيني | قديم في الحزمة | جديد (هذا EBNF) | السبب |
|---|---|---|---|
| `genus` | نوع | صنف | فك التعارض مع `typus` = "نمط"؛ التشخيصات تستخدم "صنف" باستمرار؛ "نوع" أقرب إلى type منه إلى class |
| `incipiet` | *(غير موجود)* | استهلال | كلمة جديدة — دخول غير متزامن، مميزة عن `incipit` = "بداية" |
| `sit` | *(غير موجود)* | ليكن | كلمة جديدة — محلي غير قابل للتغيير مستنتج |
| `typus` | *(غير موجود)* | نمط | كلمة جديدة — اسم نوع مستعار |
| `discretio` | *(غير موجود)* | تمايز | كلمة جديدة — اتحاد موسوم |
| `implendum` | *(غير موجود)* | عقد | كلمة جديدة — عقد واجهة |
| `modulus` | *(غير موجود)* | حلقة | كلمة جديدة — نوع كلمة حلقية |
| `ordo` | *(غير موجود)* | ترتيب | كلمة جديدة — تعداد |
| `abstractus` | *(غير موجود)* | مجرد | كلمة جديدة — معدل صنف |
| `ceteri` | *(غير موجود)* | باقي | كلمة جديدة — معامل باقي |
| `curata` | *(غير موجود)* | مخصص | كلمة جديدة — متطلب مخصص |
| `errata` | *(غير موجود)* | مخطئ | كلمة جديدة — معدل خطأ |
| `exitus` | *(غير موجود)* | مخرج | كلمة جديدة — معدل خروج |
| `generis` | *(غير موجود)* | سكوني | كلمة جديدة — static |
| `iacit` | *(غير موجود)* | يرمي | كلمة جديدة — معدل throws |
| `immutata` | *(غير موجود)* | ثابتة | كلمة جديدة — معدل غير قابل للتغيير |
| `magnitudo` | *(غير موجود)* | حجم | كلمة جديدة — معامل حجم عام |
| `nexum` | *(غير موجود)* | مرتبط | كلمة جديدة — bound/property |
| `optiones` | *(غير موجود)* | خيارات | كلمة جديدة — معدل options |
| `prae` | *(غير موجود)* | سابق | كلمة جديدة — كلمة قديمة/أزيلت |
| `privata` | *(غير موجود)* | خاص | كلمة جديدة — رؤية خاصة |
| `protecta` | *(غير موجود)* | محمي | كلمة جديدة — رؤية محجوزة |
| `publica` | *(غير موجود)* | عام | كلمة جديدة — رؤية عامة |
| `sponte` | *(غير موجود)* | اختياري | كلمة جديدة — فتحة اختيارية |
| `custodi` | *(غير موجود)* | احرس | كلمة جديدة — guard |
| `ergo` | *(غير موجود)* | إذن | كلمة جديدة — رابط جسم عبارة مدمج |
| `sic` | *(غير موجود)* | فإذا | كلمة جديدة — ternary then |
| `adfirma` | *(غير موجود)* | أكد | كلمة جديدة — assert |
| `iace` | *(غير موجود)* | ارم | كلمة جديدة — throw |
| `mori` | *(غير موجود)* | انهر | كلمة جديدة — panic |
| `clausura` | *(غير موجود)* | إغلاق | كلمة جديدة — closure |
| `vel` | *(غير موجود)* | عوض | كلمة جديدة — افتراضي محلي قابل للإبطال |
| `ego` | *(غير موجود)* | ذات | كلمة جديدة — self/this |
| `finge` | *(غير موجود)* | أنشئ | كلمة جديدة — إنشاء متغير |
| `implet` | *(غير موجود)* | حقق | كلمة جديدة — implements |
| `sub` | *(غير موجود)* | امتد | كلمة جديدة — extends |
| `cura` | *(غير موجود)* | اعتن | كلمة جديدة — إدارة موارد |
| `cede` | *(غير موجود)* | سلم | كلمة جديدة — await/yield |
| `lege` | *(غير موجود)* | اقرأ | كلمة جديدة — read |
| `lineam` | *(غير موجود)* | سطرا | كلمة جديدة — line |
| `omnia` | *(غير موجود)* | جميع | كلمة جديدة — wildcard |
| `praefixum` | *(غير موجود)* | بادئة | كلمة جديدة — prefix expression |
| `scriptum` | *(غير موجود)* | حرر | كلمة جديدة — تحرير/عرض قالب |
| `sparge` | *(غير موجود)* | انشر | كلمة جديدة — spread |
| `ante` | *(غير موجود)* | قبل | كلمة جديدة — قبل في النطاق |
| `inter` | *(غير موجود)* | بين | كلمة جديدة — بين (نطاق حصري) |
| `intra` | *(غير موجود)* | ضمن | كلمة جديدة — ضمن (نطاق/عضوية شامل) |
| `per` | *(غير موجود)* | كل | كلمة جديدة — خطوة في النطاق |
| `usque` | *(غير موجود)* | حتى | كلمة جديدة — حتى (حد نطاق شامل) |
| `fragilis` | *(غير موجود)* | هش | كلمة جديدة — معدل اختبار fragile |
| `futurum` | *(غير موجود)* | مستقبلي | كلمة جديدة — معدل اختبار مستقبلي |
| `metior` | *(غير موجود)* | قس | كلمة جديدة — معدل اختبار قياس |
| `omitte` | *(غير موجود)* | أهمل | كلمة جديدة — معدل اختبار skip |
| `postpara` | *(غير موجود)* | لاحق | كلمة جديدة — post-prepare |
| `postparabit` | *(غير موجود)* | سيلحق | كلمة جديدة — async post-prepare |
| `praepara` | *(غير موجود)* | جهز | كلمة جديدة — prepare |
| `praeparabit` | *(غير موجود)* | سيهيئ | كلمة جديدة — async prepare |
| `proba` | *(غير موجود)* | اختبر | كلمة جديدة — test |
| `probandum` | *(غير موجود)* | مختبر | كلمة جديدة — test suite |
| `repete` | *(غير موجود)* | معاد | كلمة جديدة — معدل اختبار repeat |
| `requirit` | *(غير موجود)* | يتطلب | كلمة جديدة — عبارة require قابلة للاسترداد |
| `solum` | *(غير موجود)* | فقط | كلمة جديدة — معدل اختبار only |
| `solum_in` | *(غير موجود)* | حصري | كلمة جديدة — معدل اختبار only-in |
| `tag` | *(غير موجود)* | وسم | كلمة جديدة — معدل اختبار tag |
| `temporis` | *(غير موجود)* | زمني | كلمة جديدة — معدل اختبار time |
| `negativum` | *(غير موجود)* | سلبي | كلمة جديدة — مسند سالب |
| `nonnihil` | *(غير موجود)* | موجود | كلمة جديدة — مسند غير لاشيء |
| `nonnulla` | *(غير موجود)* | مليء | كلمة جديدة — مسند غير فارغ |
| `nulla` | *(غير موجود)* | خال | كلمة جديدة — مسند فارغ |
| `positivum` | *(غير موجود)* | موجب | كلمة جديدة — مسند موجب |
| *(أنواع)* `ascii` | *(غير موجود)* | أسكي | نوع جديد — سلسلة ASCII |
| *(أنواع)* `modulus` | *(غير موجود)* | حلقة | نوع جديد — كلمة حلقية |
| *(أنواع)* `octeti` | *(غير موجود)* | بايتات | نوع جديد — بايتات |
| *(أنواع)* `regex` | *(غير موجود)* | تعبير | نوع جديد — نمط مجمع |
| *(أنواع)* `json` | *(غير موجود)* | جسون | نوع جديد — مستند JSON |
| *(أنواع)* `valor` | *(غير موجود)* | قيمة | نوع جديد — قيمة ديناميكية |
| *(أنواع)* `instans` | *(غير موجود)* | لحظة | نوع جديد — طابع زمني |
| *(أنواع)* `objectum` | *(غير موجود)* | كائن | نوع جديد — كائن |
| *(أنواع)* `quidlibet` | *(غير موجود)* | مهما | نوع جديد — أي نوع |
| *(أنواع)* `promissum` | *(غير موجود)* | وعد | نوع جديد — promise |
| *(أنواع)* `numquam` | *(غير موجود)* | أبدا | نوع جديد — never |
