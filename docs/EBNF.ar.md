# Faber Language Specification

This file is generated from `docs/grammar/source.fg` and `docs/grammar/glossary.ar.toml`;
hand edits fail the locale-render gate. Production IDs are the grammar's stable
snake_case spine and their anchors are derived from those IDs.

## Grammar {#grammar}

The grammar below is the identity rendering of the validated source. Normative detail is kept in this English sidecar and rendered as documentation; the source remains the syntax authority.

```ebnf
# [001] fab_file
fab_file ::= frontmatter? program
# [002] frontmatter
frontmatter ::= FRONTMATTER_DELIMITER NEWLINE TOML_LINES FRONTMATTER_DELIMITER NEWLINE?
# [003] program
program ::= statement*
# [004] statement
statement ::= annotation* statement_core
# [005] statement_core
statement_core ::= importa_decl | binding_decl | functio_decl | genus_decl | implendum_decl | typus_decl | ordo_decl | discretio_decl | schema_decl | si_stmt | dum_stmt | itera_stmt | elige_stmt | discerne_stmt | custodi_stmt | fac_stmt | redde_stmt | reddet_stmt | tacebit_stmt | cede_stmt | rumpe_stmt | perge_stmt | tacet_stmt | iace_stmt | adfirma_stmt | requirit_stmt | reice_stmt | nota_stmt | incipit_stmt | incipiet_stmt | ex_stmt | probandum_decl | proba_stmt | block_stmt | inc_dec_stmt | expr_stmt
# [006] binding_decl
binding_decl ::= fixum_decl | sit_decl | array_destruct | object_destruct | figendum_decl
# [007] expr_stmt
expr_stmt ::= expression
# [008] block_stmt
block_stmt ::= '{' statement* '}'
# [009] fixum_decl
fixum_decl ::= ('ثابت' | 'متغير') type_annotation IDENTIFIER (('←' expression) | ('↤' assignment inline_default?) | ('↢' expression))?
# [010] figendum_decl
figendum_decl ::= ('انتظر_ثابت' | 'انتظر_متغير') type_annotation IDENTIFIER '←' expression
# [011] sit_decl
sit_decl ::= 'ليكن' IDENTIFIER (('←' | '↢') expression)?
# [012] array_destruct
array_destruct ::= ('ثابت' | 'متغير') array_pattern '←' expression
# [013] object_destruct
object_destruct ::= ('ثابت' | 'متغير') object_pattern '←' expression
# [014] functio_decl
functio_decl ::= 'دالة' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [015] param_list
param_list ::= (parameter (',' parameter)*)?
# [016] generic_params
generic_params ::= '<' generic_param (',' generic_param)* '>'
# [017] generic_param
generic_param ::= IDENTIFIER generic_type_default? | 'حجم' IDENTIFIER generic_size_default?
# [018] generic_type_default
generic_type_default ::= '=' type_annotation
# [019] generic_size_default
generic_size_default ::= '=' NATURAL
# [020] call_type_args
call_type_args ::= '<' type_annotation (',' type_annotation)* '>'
# [021] parameter
parameter ::= 'باقي'? type_annotation IDENTIFIER 'اختياري'? ('كـ' IDENTIFIER)? ('عوض' expression)?
# [022] func_modifier
func_modifier ::= 'وسائط' IDENTIFIER | 'مخطئ' IDENTIFIER | 'مخرج' (IDENTIFIER | NUMBER) | 'ثابتة' | 'يرمي' | 'خيارات' IDENTIFIER
# [023] callable_posture
callable_posture ::= 'غيرمتزامن' | 'مولد' | 'مولد_غيرمتزامن'
# [024] return_clause
return_clause ::= '→' type_annotation
# [025] alternate_exit_clause
alternate_exit_clause ::= '⇥' type_annotation
# [026] ergo_joint
ergo_joint ::= 'إذن'
# [027] clausura_joint
clausura_joint ::= '∴'
# [028] clausura_expr
clausura_expr ::= compact_clausura_expr | clausura_legacy_expr
# [029] compact_clausura_expr
compact_clausura_expr ::= clausura_signature clausura_joint (expression | fac_block)
# [030] clausura_signature
clausura_signature ::= (clausura_param | '(' clausura_params? ')') closure_modifier? return_clause? alternate_exit_clause?
# [031] closure_modifier
closure_modifier ::= 'حر' | 'نواة'
# [032] fac_block
fac_block ::= 'افعل' block_stmt cape_clause?
# [033] clausura_legacy_expr
clausura_legacy_expr ::= 'إغلاق' clausura_params? closure_modifier? ('→' type_annotation)? (':' expression | block_stmt)
# [034] clausura_params
clausura_params ::= clausura_param (',' clausura_param)*
# [035] clausura_param
clausura_param ::= type_annotation IDENTIFIER
# [036] genus_decl
genus_decl ::= 'صنف' IDENTIFIER generic_params? ('حقق' IDENTIFIER ((',' | '∩') IDENTIFIER)*)? '{' genus_member* '}'
# [037] genus_member
genus_member ::= annotation* (field_decl | functio_method_decl)
# [038] field_decl
field_decl ::= 'سكوني'? type_annotation IDENTIFIER 'اختياري'? ('=' expression)?
# [039] functio_method_decl
functio_method_decl ::= 'دالة' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [040] annotation
annotation ::= nucleum_annotation | radix_annotation | ad_annotation | braced_annotation | annotation_sugar
# [041] annotation_name
annotation_name ::= ANNOTATION_NAME
# [042] braced_annotation
braced_annotation ::= '@' annotation_name '{' annotation_field_list? '}'
# [043] annotation_field_list
annotation_field_list ::= annotation_field (',' annotation_field)*
# [044] annotation_field
annotation_field ::= ANNOTATION_FIELD_NAME '=' (expression | type_annotation)
# [045] annotation_sugar
annotation_sugar ::= '@' annotation_name NON_NEWLINE_TOKEN* NEWLINE
# [046] nucleum_annotation
nucleum_annotation ::= nucleum_sugar | nucleum_braced
# [047] nucleum_sugar
nucleum_sugar ::= '@' 'نواة' nucleum_modifier? NEWLINE
# [048] nucleum_braced
nucleum_braced ::= '@' 'نواة' '{' nucleum_field_list? '}'
# [049] nucleum_modifier
nucleum_modifier ::= 'جزء'
# [050] nucleum_field_list
nucleum_field_list ::= nucleum_field (',' nucleum_field)*
# [051] nucleum_field
nucleum_field ::= 'جزء' '=' ('صواب' | 'خطأ')
# [052] radix_annotation
radix_annotation ::= '@' 'radix' radix_directive NEWLINE
# [053] radix_directive
radix_directive ::= 'مسار' STRING | 'backward' STRING | 'نمط' IDENTIFIER 'في' type_annotation+
# [054] ad_annotation
ad_annotation ::= '@' 'اتصل' ASCII_STRING NEWLINE
# [055] implendum_decl
implendum_decl ::= 'عقد' IDENTIFIER generic_params? '{' implendum_method_decl* '}'
# [056] implendum_method_decl
implendum_method_decl ::= annotation* 'دالة' IDENTIFIER '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# [057] typus_decl
typus_decl ::= 'نمط' IDENTIFIER generic_params? '=' type_annotation
# [058] ordo_decl
ordo_decl ::= 'ترتيب' IDENTIFIER '{' enum_member (',' enum_member)* '}'
# [059] enum_member
enum_member ::= IDENTIFIER ('=' ('-'? NUMBER | STRING))?
# [060] discretio_decl
discretio_decl ::= 'تمايز' IDENTIFIER generic_params? '{' union_member* variant (',' variant)* '}'
# [061] union_member
union_member ::= annotation* field_decl
# [062] variant
variant ::= IDENTIFIER ('{' variant_fields '}')?
# [063] variant_fields
variant_fields ::= (type_annotation IDENTIFIER)*
# [064] schema_decl
schema_decl ::= 'مخطط' IDENTIFIER '{' schema_column* '}'
# [065] schema_column
schema_column ::= 'عمود' type_annotation IDENTIFIER (':' IDENTIFIER)?
# [066] importa_decl
importa_decl ::= importa_record | importa_sugar
# [067] importa_record
importa_record ::= 'استورد' '{' import_field_list? '}'
# [068] import_field_list
import_field_list ::= import_field (',' import_field)*
# [069] import_field
import_field ::= ex_field | visibilitas_field | nomen_field | ut_field | omnia_field
# [070] ex_field
ex_field ::= 'من' '=' STRING
# [071] visibilitas_field
visibilitas_field ::= 'visibilitas' '=' publica
# [072] nomen_field
nomen_field ::= 'اسم' '=' IDENTIFIER
# [073] ut_field
ut_field ::= 'كـ' '=' IDENTIFIER
# [074] omnia_field
omnia_field ::= 'جميع' '=' IDENTIFIER
# [075] importa_sugar
importa_sugar ::= 'استورد' 'من' STRING publica? (named_import | wildcard_import | selective_import)?
# [076] publica
publica ::= 'عام'
# [077] named_import
named_import ::= IDENTIFIER ('كـ' IDENTIFIER)?
# [078] wildcard_import
wildcard_import ::= '*' 'كـ' IDENTIFIER
# [079] selective_import
selective_import ::= 'ثابت' import_value_binding (',' import_value_binding)*
# [080] import_value_binding
import_value_binding ::= IDENTIFIER ('كـ' IDENTIFIER)?
# [081] type_annotation
type_annotation ::= intersection_type ('∪' intersection_type)*
# [082] intersection_type
intersection_type ::= owned_type ('∩' owned_type)*
# [083] owned_type
owned_type ::= ('عن' | 'في' | 'ملك' | 'نسخة')? base_type
# [084] base_type
base_type ::= hole_type | function_type | width_type_sugar | ratio_type | qualified_type type_arguments? | '(' type_annotation ')'
# [085] ratio_type
ratio_type ::= 'ratio' '<' labeled_type_argument (',' labeled_type_argument)* '>'
# [086] hole_type
hole_type ::= '_' | '∪'
# [087] qualified_type
qualified_type ::= IDENTIFIER ('.' IDENTIFIER)*
# [088] type_arguments
type_arguments ::= '<' type_argument (',' type_argument)* '>'
# [089] type_argument
type_argument ::= labeled_type_argument | type_annotation | NATURAL | '[' figura_list? ']'
# [090] labeled_type_argument
labeled_type_argument ::= IDENTIFIER ':' type_annotation
# [091] width_type_sugar
width_type_sugar ::= WIDTH_MARKER | LISTA_WIDTH_SUGAR | (TENSOR_WIDTH_SUGAR | SPARSA_WIDTH_SUGAR | VECTOR_WIDTH_SUGAR) shape_suffix? | MATRIX_WIDTH_SUGAR shape_suffix
# [092] shape_suffix
shape_suffix ::= '[' figura_list? ']'
# [093] figura
figura ::= '_' | NATURAL | IDENTIFIER | '[' figura_list? ']'
# [094] figura_list
figura_list ::= figura (',' figura)*
# [095] function_type
function_type ::= '(' type_list? ')' '→' type_annotation alternate_exit_clause?
# [096] type_list
type_list ::= type_annotation (',' type_annotation)*
# [097] si_stmt
si_stmt ::= 'إذا' expression arm ('وإلاإذا' si_stmt | secus_clause)?
# [098] secus_clause
secus_clause ::= 'وإلا' else_arm
# [099] arm
arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [100] else_arm
else_arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [101] dum_stmt
dum_stmt ::= 'طالما' expression (block_stmt | ergo_joint statement) cape_clause?
# [102] itera_stmt
itera_stmt ::= 'كرر' ('من' expression (',' expression)* | 'عن' expression | 'نطاق' expression (',' expression)*) apud_clause? ('ثابت' | 'متغير') itera_binding (block_stmt | ergo_joint statement) cape_clause?
# [103] itera_binding
itera_binding ::= array_pattern | object_pattern | IDENTIFIER (',' IDENTIFIER)*
# [104] apud_clause
apud_clause ::= 'عند' '[' IDENTIFIER (',' IDENTIFIER)* ']'
# [105] elige_stmt
elige_stmt ::= 'اختر' expression '{' casu_elige_clause* ceterum_clause? '}' cape_clause?
# [106] casu_elige_clause
casu_elige_clause ::= 'حالة' expression (block_stmt | ergo_joint statement)
# [107] ceterum_clause
ceterum_clause ::= 'افتراضي' (block_stmt | ergo_joint statement)
# [108] discerne_stmt
discerne_stmt ::= 'طابق' 'جميع'? discriminants '{' casu_variant_clause* ceterum_clause? '}'
# [109] discriminants
discriminants ::= expression (',' expression)*
# [110] casu_variant_clause
casu_variant_clause ::= 'حالة' patterns (block_stmt | ergo_joint statement)
# [111] patterns
patterns ::= pattern ((',' | 'و') pattern)*
# [112] pattern
pattern ::= '_' | negated_number | literal | type_pattern | (IDENTIFIER ut_pattern?)
# [113] negated_number
negated_number ::= '-' NUMBER
# [114] type_pattern
type_pattern ::= IDENTIFIER type_arguments? ut_pattern?
# [115] ut_pattern
ut_pattern ::= ('كـ' IDENTIFIER) | (('ثابت' | 'متغير') pattern_binding (',' pattern_binding)*)
# [116] pattern_binding
pattern_binding ::= IDENTIFIER ('كـ' IDENTIFIER)?
# [117] custodi_stmt
custodi_stmt ::= 'احرس' '{' si_guard_clause+ '}'
# [118] si_guard_clause
si_guard_clause ::= 'إذا' expression (block_stmt | ergo_joint statement)
# [119] ex_stmt
ex_stmt ::= 'من' expression ('ثابت' | 'متغير') extract_fields
# [120] extract_fields
extract_fields ::= extract_field (',' extract_field)* (',' ceteri_field)? | ceteri_field
# [121] extract_field
extract_field ::= IDENTIFIER ('كـ' IDENTIFIER)?
# [122] ceteri_field
ceteri_field ::= 'باقي' IDENTIFIER
# [123] redde_stmt
redde_stmt ::= 'أعد' expression?
# [124] reddet_stmt
reddet_stmt ::= 'أعد_منتظرا' expression
# [125] tacebit_stmt
tacebit_stmt ::= 'انتظر' expression
# [126] cede_stmt
cede_stmt ::= 'سلم' expression
# [127] rumpe_stmt
rumpe_stmt ::= 'اكسر'
# [128] perge_stmt
perge_stmt ::= 'تابع'
# [129] tacet_stmt
tacet_stmt ::= 'صمت'
# [130] iace_stmt
iace_stmt ::= iace_expr | iace_guarded_expr
# [131] iace_expr
iace_expr ::= ('ارم' | 'انهر') expression
# [132] iace_guarded_expr
iace_guarded_expr ::= ('ارم' | 'انهر') expression NO_NEWLINE 'إذا' expression
# [133] cape_clause
cape_clause ::= 'التقط' IDENTIFIER block_stmt
# [134] adfirma_stmt
adfirma_stmt ::= 'أكد' expression ('انهر' expression)?
# [135] requirit_stmt
requirit_stmt ::= 'يتطلب' expression 'ارم' expression
# [136] reice_stmt
reice_stmt ::= 'ارفض' expression 'ارم' expression
# [137] expression
expression ::= assignment
# [138] transfer
transfer ::= ternary ('⇇' ternary)*
# [139] assignment
assignment ::= transfer ('←' assignment | '↤' assignment inline_default?)?
# [140] inc_dec_stmt
inc_dec_stmt ::= place ('↑' | '↓')
# [141] place
place ::= call_expr
# [142] ternary
ternary ::= aut_expr (('?' expression ':' | 'فإذا' expression 'وإلا') ternary)?
# [143] aut_expr
aut_expr ::= et_expr (('أو') et_expr)*
# [144] et_expr
et_expr ::= equality (('و') equality)*
# [145] equality
equality ::= comparison equality_tail*
# [146] equality_tail
equality_tail ::= ('≡' | '≢' | '≠' | '≅' | '≇' | '≈' | '≉' | 'هو' | 'ليس' 'هو') comparison
# [147] comparison
comparison ::= bitwise_or_expr (('≺' | '≻' | '≤' | '≥' | 'ضمن' | 'بين') bitwise_or_expr)*
# [148] bitwise_or_expr
bitwise_or_expr ::= bitwise_xor_expr ('∨' bitwise_xor_expr)*
# [149] bitwise_xor_expr
bitwise_xor_expr ::= bitwise_and_expr ('⊻' bitwise_and_expr)*
# [150] bitwise_and_expr
bitwise_and_expr ::= shift_expr ('∧' shift_expr)*
# [151] shift_expr
shift_expr ::= range_expr (('⇐' | '⇒') range_expr)*
# [152] range_expr
range_expr ::= additive_expr range_tail?
# [153] range_tail
range_tail ::= ('‥' | '…' | 'قبل' | 'حتى') additive_expr ('كل' additive_expr)?
# [154] additive_expr
additive_expr ::= multiplicative_expr (('+' | '-' | '⤒' | '⤓') multiplicative_expr)*
# [155] multiplicative_expr
multiplicative_expr ::= vel_expr (('*' | '/' | '%' | '·' | '×' | '⊗' | '⊙' | '⊘') vel_expr)*
# [156] vel_expr
vel_expr ::= unary_expr ('عوض' vel_rhs)*
# [157] vel_rhs
vel_rhs ::= unary_expr vel_range_tail?
# [158] vel_range_tail
vel_range_tail ::= ('‥' | '…' | 'قبل' | 'حتى') unary_expr ('كل' unary_expr)?
# [159] unary_expr
unary_expr ::= ('-' | '¬' | 'ليس') unary_expr | finge_expr | cast_expr
# [160] gradient_expr
gradient_expr ::= call_expr ('∇' gradient_selection?)?
# [161] gradient_selection
gradient_selection ::= '[' gradient_place (',' gradient_place)* ']'
# [162] gradient_place
gradient_place ::= expression
# [163] cast_expr
cast_expr ::= gradient_expr ('∷' type_annotation | conversio_expr)*
# [164] conversio_expr
conversio_expr ::= '↦' type_annotation inline_default?
# [165] inline_default
inline_default ::= '⊥' unary_expr
# [166] call_expr
call_expr ::= primary (call_suffix | member_suffix | transpose_suffix | optional_suffix | non_null_suffix)*
# [167] call_suffix
call_suffix ::= call_type_args? '(' argument_list ')'
# [168] member_suffix
member_suffix ::= '.' IDENTIFIER | '[' expression ']'
# [169] transpose_suffix
transpose_suffix ::= 'ᵀ'
# [170] optional_suffix
optional_suffix ::= '?.' IDENTIFIER | '?[' expression ']' | '?(' argument_list ')'
# [171] non_null_suffix
non_null_suffix ::= '!.' IDENTIFIER | '![' expression ']' | '!(' argument_list ')'
# [172] argument_list
argument_list ::= (argument (',' argument)*)?
# [173] argument
argument ::= template_argument | 'انشر'? expression
# [174] template_argument
template_argument ::= 'انشر'? IDENTIFIER ':' expression
# [175] literal
literal ::= NUMBER | STRING | ASCII_STRING | BACKTICK_STRING | OCTETI_STRING | 'صواب' | 'خطأ' | 'لاشيء' | '∞' | 'nan'
# [176] primary
primary ::= IDENTIFIER | literal | 'ذات' | array_literal | json_literal | typed_constructor | iuncta_expr | ad_expr | clausura_expr | praefixum_expr | scriptum_expr | lege_expr | first_match_expr | summa_expr | capta_expr | '(' expression ')'
# [177] ad_expr
ad_expr ::= 'اتصل' ASCII_STRING ad_opener?
# [178] ad_opener
ad_opener ::= '(' expression ')'
# [179] array_literal
array_literal ::= '[' argument_list? ']'
# [180] iuncta_expr
iuncta_expr ::= 'توبل' type_arguments '[' argument_list? ']'
# [181] json_literal
json_literal ::= '{' (json_member (',' json_member)*)? '}'
# [182] json_member
json_member ::= STRING ':' json_value
# [183] typed_constructor
typed_constructor ::= type_annotation '{' field_list? '}'
# [184] field_list
field_list ::= field_init (',' field_init)*
# [185] field_init
field_init ::= (field_key '=' expression) | IDENTIFIER
# [186] field_key
field_key ::= IDENTIFIER | STRING | '[' expression ']'
# [187] json_value
json_value ::= json_object | json_array | json_string | json_number | 'true' | 'false' | 'null'
# [188] json_object
json_object ::= '{' (json_member (',' json_member)*)? '}'
# [189] json_array
json_array ::= '[' (json_value (',' json_value)*)? ']'
# [190] json_string
json_string ::= STRING
# [191] json_number
json_number ::= NUMBER
# [192] finge_expr
finge_expr ::= 'أنشئ' qualified_ident ('{' field_list '}')? ('∷' type_annotation)?
# [193] qualified_ident
qualified_ident ::= IDENTIFIER ('.' IDENTIFIER)*
# [194] praefixum_expr
praefixum_expr ::= 'بادئة' (block_stmt | '(' expression ')')
# [195] scriptum_expr
scriptum_expr ::= 'حرر' '(' STRING (',' expression)* ')'
# [196] lege_expr
lege_expr ::= 'اقرأ' 'سطرا'?
# [197] first_match_expr
first_match_expr ::= 'أول_مطابقة' '(' expression apud_clause? ',' 'حيث' IDENTIFIER block_stmt ')'
# [198] summa_expr
summa_expr ::= 'مجموع' 'من' expression apud_clause? filum_clause? ('ثابت' | 'متغير') IDENTIFIER block_stmt
# [199] filum_clause
filum_clause ::= 'خيط' IDENTIFIER
# [200] capta_expr
capta_expr ::= 'فخ' block_stmt
# [201] object_pattern
object_pattern ::= '{' pattern_property (',' pattern_property)* '}'
# [202] pattern_property
pattern_property ::= 'باقي'? IDENTIFIER ('كـ' IDENTIFIER)?
# [203] array_pattern
array_pattern ::= '[' array_pattern_element (',' array_pattern_element)* ']'
# [204] array_pattern_element
array_pattern_element ::= '_' | 'باقي'? IDENTIFIER
# [205] nota_stmt
nota_stmt ::= ('اعرض' | 'شاهد' | 'نبه' | 'اكتب') expression (',' expression)*
# [206] entry_header
entry_header ::= ('وسائط' IDENTIFIER)? ('مخرج' expression)?
# [207] incipit_stmt
incipit_stmt ::= 'بداية' entry_header block_stmt
# [208] incipiet_stmt
incipiet_stmt ::= 'استهلال' entry_header block_stmt
# [209] probandum_decl
probandum_decl ::= 'مختبر' STRING proba_modifier* '{' probandum_body '}'
# [210] probandum_body
probandum_body ::= (praepara_block | probandum_decl | proba_stmt)*
# [211] proba_stmt
proba_stmt ::= 'اختبر' STRING proba_modifier* block_stmt
# [212] proba_modifier
proba_modifier ::= 'توقع_الفشل' | 'أهمل' STRING | 'مستقبلي' STRING | 'فقط' | 'وسم' STRING | 'زمني' NUMBER | 'قس' | 'معاد' NUMBER | 'هش' NUMBER | 'حصري' STRING
# [213] praepara_block
praepara_block ::= ('جهز' | 'سيهيئ' | 'لاحق' | 'سيلحق') 'جميع'? block_stmt
# [214] fac_stmt
fac_stmt ::= 'افعل' block_stmt cape_clause? ('طالما' expression)?
# [215] IDENTIFIER
IDENTIFIER ::=
# [216] NUMBER
NUMBER ::=
# [217] NATURAL
NATURAL ::=
# [218] STRING
STRING ::=
# [219] ASCII_STRING
ASCII_STRING ::=
# [220] BACKTICK_STRING
BACKTICK_STRING ::=
# [221] OCTETI_STRING
OCTETI_STRING ::=
# [222] NEWLINE
NEWLINE ::=
# [223] WIDTH_MARKER
WIDTH_MARKER ::=
# [224] LISTA_WIDTH_SUGAR
LISTA_WIDTH_SUGAR ::=
# [225] TENSOR_WIDTH_SUGAR
TENSOR_WIDTH_SUGAR ::=
# [226] SPARSA_WIDTH_SUGAR
SPARSA_WIDTH_SUGAR ::=
# [227] VECTOR_WIDTH_SUGAR
VECTOR_WIDTH_SUGAR ::=
# [228] MATRIX_WIDTH_SUGAR
MATRIX_WIDTH_SUGAR ::=
# [229] FRONTMATTER_DELIMITER
FRONTMATTER_DELIMITER ::=
# [230] TOML_LINES
TOML_LINES ::=
# [231] ANNOTATION_NAME
ANNOTATION_NAME ::=
# [232] ANNOTATION_FIELD_NAME
ANNOTATION_FIELD_NAME ::=
# [233] NON_NEWLINE_TOKEN
NON_NEWLINE_TOKEN ::=
# [234] NO_NEWLINE
NO_NEWLINE ::=
```

## Production Index {#production-index}

| ID | Anchor | Status |
|---|---|---|
| [`IDENTIFIER`](#identifier) | `#identifier` | capture-pending |
| [`NUMBER`](#number) | `#number` | capture-pending |
| [`NATURAL`](#natural) | `#natural` | capture-pending |
| [`STRING`](#string) | `#string` | capture-pending |
| [`ASCII_STRING`](#ascii-string) | `#ascii-string` | capture-pending |
| [`BACKTICK_STRING`](#backtick-string) | `#backtick-string` | capture-pending |
| [`OCTETI_STRING`](#octeti-string) | `#octeti-string` | capture-pending |
| [`NEWLINE`](#newline) | `#newline` | capture-pending |
| [`WIDTH_MARKER`](#width-marker) | `#width-marker` | capture-pending |
| [`LISTA_WIDTH_SUGAR`](#lista-width-sugar) | `#lista-width-sugar` | capture-pending |
| [`TENSOR_WIDTH_SUGAR`](#tensor-width-sugar) | `#tensor-width-sugar` | capture-pending |
| [`SPARSA_WIDTH_SUGAR`](#sparsa-width-sugar) | `#sparsa-width-sugar` | capture-pending |
| [`VECTOR_WIDTH_SUGAR`](#vector-width-sugar) | `#vector-width-sugar` | capture-pending |
| [`MATRIX_WIDTH_SUGAR`](#matrix-width-sugar) | `#matrix-width-sugar` | capture-pending |
| [`FRONTMATTER_DELIMITER`](#frontmatter-delimiter) | `#frontmatter-delimiter` | capture-pending |
| [`TOML_LINES`](#toml-lines) | `#toml-lines` | capture-pending |
| [`ANNOTATION_NAME`](#annotation-name) | `#annotation-name` | capture-pending |
| [`ANNOTATION_FIELD_NAME`](#annotation-field-name) | `#annotation-field-name` | capture-pending |
| [`NON_NEWLINE_TOKEN`](#non-newline-token) | `#ليس-newline-token` | capture-pending |
| [`NO_NEWLINE`](#no-newline) | `#no-newline` | capture-pending |
| [`fab_file`](#fab-file) | `#fab-file` | live |
| [`frontmatter`](#frontmatter) | `#frontmatter` | live |
| [`program`](#program) | `#program` | live |
| [`statement`](#statement) | `#statement` | live |
| [`statement_core`](#statement-core) | `#statement-core` | live |
| [`binding_decl`](#binding-decl) | `#binding-decl` | live |
| [`expr_stmt`](#expr-stmt) | `#expr-stmt` | live |
| [`block_stmt`](#block-stmt) | `#block-stmt` | live |
| [`fixum_decl`](#fixum-decl) | `#ثابت-decl` | live |
| [`figendum_decl`](#figendum-decl) | `#انتظر_ثابت-decl` | live |
| [`sit_decl`](#sit-decl) | `#ليكن-decl` | live |
| [`array_destruct`](#array-destruct) | `#array-destruct` | live |
| [`object_destruct`](#object-destruct) | `#object-destruct` | live |
| [`functio_decl`](#functio-decl) | `#دالة-decl` | live |
| [`param_list`](#param-list) | `#param-list` | live |
| [`generic_params`](#generic-params) | `#generic-params` | live |
| [`generic_param`](#generic-param) | `#generic-param` | live |
| [`generic_type_default`](#generic-type-default) | `#generic-type-default` | live |
| [`generic_size_default`](#generic-size-default) | `#generic-size-default` | live |
| [`call_type_args`](#call-type-args) | `#call-type-args` | live |
| [`parameter`](#parameter) | `#parameter` | live |
| [`func_modifier`](#func-modifier) | `#func-modifier` | live |
| [`callable_posture`](#callable-posture) | `#callable-posture` | live |
| [`return_clause`](#return-clause) | `#return-clause` | live |
| [`alternate_exit_clause`](#alternate-exit-clause) | `#alternate-exit-clause` | live |
| [`ergo_joint`](#ergo-joint) | `#إذن-joint` | live |
| [`clausura_joint`](#clausura-joint) | `#إغلاق-joint` | live |
| [`clausura_expr`](#clausura-expr) | `#إغلاق-expr` | live |
| [`compact_clausura_expr`](#compact-clausura-expr) | `#compact-إغلاق-expr` | live |
| [`clausura_signature`](#clausura-signature) | `#إغلاق-signature` | live |
| [`closure_modifier`](#closure-modifier) | `#closure-modifier` | live |
| [`fac_block`](#fac-block) | `#افعل-block` | live |
| [`clausura_legacy_expr`](#clausura-legacy-expr) | `#إغلاق-legacy-expr` | live |
| [`clausura_params`](#clausura-params) | `#إغلاق-params` | live |
| [`clausura_param`](#clausura-param) | `#إغلاق-param` | live |
| [`genus_decl`](#genus-decl) | `#صنف-decl` | live |
| [`genus_member`](#genus-member) | `#صنف-member` | live |
| [`field_decl`](#field-decl) | `#field-decl` | live |
| [`functio_method_decl`](#functio-method-decl) | `#دالة-method-decl` | live |
| [`annotation`](#annotation) | `#annotation` | live |
| [`annotation_name`](#annotation-name) | `#annotation-name` | live |
| [`braced_annotation`](#braced-annotation) | `#braced-annotation` | live |
| [`annotation_field_list`](#annotation-field-list) | `#annotation-field-list` | live |
| [`annotation_field`](#annotation-field) | `#annotation-field` | live |
| [`annotation_sugar`](#annotation-sugar) | `#annotation-sugar` | live |
| [`nucleum_annotation`](#nucleum-annotation) | `#نواة-annotation` | live |
| [`nucleum_sugar`](#nucleum-sugar) | `#نواة-sugar` | live |
| [`nucleum_braced`](#nucleum-braced) | `#نواة-braced` | live |
| [`nucleum_modifier`](#nucleum-modifier) | `#نواة-modifier` | live |
| [`nucleum_field_list`](#nucleum-field-list) | `#نواة-field-list` | live |
| [`nucleum_field`](#nucleum-field) | `#نواة-field` | live |
| [`radix_annotation`](#radix-annotation) | `#radix-annotation` | live |
| [`radix_directive`](#radix-directive) | `#radix-directive` | live |
| [`ad_annotation`](#ad-annotation) | `#اتصل-annotation` | live |
| [`implendum_decl`](#implendum-decl) | `#عقد-decl` | live |
| [`implendum_method_decl`](#implendum-method-decl) | `#عقد-method-decl` | live |
| [`typus_decl`](#typus-decl) | `#نمط-decl` | live |
| [`ordo_decl`](#ordo-decl) | `#ترتيب-decl` | live |
| [`enum_member`](#enum-member) | `#enum-member` | live |
| [`discretio_decl`](#discretio-decl) | `#تمايز-decl` | live |
| [`union_member`](#union-member) | `#union-member` | live |
| [`variant`](#variant) | `#variant` | live |
| [`variant_fields`](#variant-fields) | `#variant-fields` | live |
| [`schema_decl`](#schema-decl) | `#مخطط-decl` | live |
| [`schema_column`](#schema-column) | `#مخطط-column` | live |
| [`importa_decl`](#importa-decl) | `#استورد-decl` | live |
| [`importa_record`](#importa-record) | `#استورد-record` | live |
| [`import_field_list`](#import-field-list) | `#import-field-list` | live |
| [`import_field`](#import-field) | `#import-field` | live |
| [`ex_field`](#ex-field) | `#من-field` | live |
| [`visibilitas_field`](#visibilitas-field) | `#visibilitas-field` | live |
| [`nomen_field`](#nomen-field) | `#اسم-field` | live |
| [`ut_field`](#ut-field) | `#كـ-field` | live |
| [`omnia_field`](#omnia-field) | `#جميع-field` | live |
| [`importa_sugar`](#importa-sugar) | `#استورد-sugar` | live |
| [`عام`](#publica) | `#عام` | live |
| [`named_import`](#named-import) | `#named-import` | live |
| [`wildcard_import`](#wildcard-import) | `#wildcard-import` | live |
| [`selective_import`](#selective-import) | `#selective-import` | live |
| [`import_value_binding`](#import-value-binding) | `#import-value-binding` | live |
| [`type_annotation`](#type-annotation) | `#type-annotation` | live |
| [`intersection_type`](#intersection-type) | `#intersection-type` | live |
| [`owned_type`](#owned-type) | `#owned-type` | live |
| [`base_type`](#base-type) | `#base-type` | live |
| [`ratio_type`](#ratio-type) | `#ratio-type` | live |
| [`hole_type`](#hole-type) | `#hole-type` | live |
| [`qualified_type`](#qualified-type) | `#qualified-type` | live |
| [`type_arguments`](#type-arguments) | `#type-arguments` | live |
| [`type_argument`](#type-argument) | `#type-argument` | live |
| [`labeled_type_argument`](#labeled-type-argument) | `#labeled-type-argument` | live |
| [`width_type_sugar`](#width-type-sugar) | `#width-type-sugar` | live |
| [`shape_suffix`](#shape-suffix) | `#shape-suffix` | live |
| [`figura`](#figura) | `#figura` | live |
| [`figura_list`](#figura-list) | `#figura-list` | live |
| [`function_type`](#function-type) | `#function-type` | live |
| [`type_list`](#type-list) | `#type-list` | live |
| [`si_stmt`](#si-stmt) | `#إذا-stmt` | live |
| [`secus_clause`](#secus-clause) | `#وإلا-clause` | live |
| [`arm`](#arm) | `#arm` | live |
| [`else_arm`](#else-arm) | `#else-arm` | live |
| [`dum_stmt`](#dum-stmt) | `#طالما-stmt` | live |
| [`itera_stmt`](#itera-stmt) | `#كرر-stmt` | live |
| [`itera_binding`](#itera-binding) | `#كرر-binding` | live |
| [`apud_clause`](#apud-clause) | `#عند-clause` | live |
| [`elige_stmt`](#elige-stmt) | `#اختر-stmt` | live |
| [`casu_elige_clause`](#casu-elige-clause) | `#حالة-اختر-clause` | live |
| [`ceterum_clause`](#ceterum-clause) | `#افتراضي-clause` | live |
| [`discerne_stmt`](#discerne-stmt) | `#طابق-stmt` | live |
| [`discriminants`](#discriminants) | `#discriminants` | live |
| [`casu_variant_clause`](#casu-variant-clause) | `#حالة-variant-clause` | live |
| [`patterns`](#patterns) | `#patterns` | live |
| [`pattern`](#pattern) | `#pattern` | live |
| [`negated_number`](#negated-number) | `#negated-number` | live |
| [`type_pattern`](#type-pattern) | `#type-pattern` | live |
| [`ut_pattern`](#ut-pattern) | `#كـ-pattern` | live |
| [`pattern_binding`](#pattern-binding) | `#pattern-binding` | live |
| [`custodi_stmt`](#custodi-stmt) | `#احرس-stmt` | live |
| [`si_guard_clause`](#si-guard-clause) | `#إذا-guard-clause` | live |
| [`ex_stmt`](#ex-stmt) | `#من-stmt` | live |
| [`extract_fields`](#extract-fields) | `#extract-fields` | live |
| [`extract_field`](#extract-field) | `#extract-field` | live |
| [`ceteri_field`](#ceteri-field) | `#باقي-field` | live |
| [`redde_stmt`](#redde-stmt) | `#أعد-stmt` | live |
| [`reddet_stmt`](#reddet-stmt) | `#أعد_منتظرا-stmt` | live |
| [`tacebit_stmt`](#tacebit-stmt) | `#انتظر-stmt` | live |
| [`cede_stmt`](#cede-stmt) | `#سلم-stmt` | live |
| [`rumpe_stmt`](#rumpe-stmt) | `#اكسر-stmt` | live |
| [`perge_stmt`](#perge-stmt) | `#تابع-stmt` | live |
| [`tacet_stmt`](#tacet-stmt) | `#صمت-stmt` | live |
| [`iace_stmt`](#iace-stmt) | `#ارم-stmt` | live |
| [`iace_expr`](#iace-expr) | `#ارم-expr` | live |
| [`iace_guarded_expr`](#iace-guarded-expr) | `#ارم-guarded-expr` | live |
| [`cape_clause`](#cape-clause) | `#التقط-clause` | live |
| [`adfirma_stmt`](#adfirma-stmt) | `#أكد-stmt` | live |
| [`requirit_stmt`](#requirit-stmt) | `#يتطلب-stmt` | live |
| [`reice_stmt`](#reice-stmt) | `#ارفض-stmt` | live |
| [`expression`](#expression) | `#expression` | live |
| [`transfer`](#transfer) | `#transfer` | live |
| [`assignment`](#assignment) | `#assignment` | live |
| [`inc_dec_stmt`](#inc-dec-stmt) | `#inc-dec-stmt` | live |
| [`place`](#place) | `#place` | live |
| [`ternary`](#ternary) | `#ternary` | live |
| [`aut_expr`](#aut-expr) | `#أو-expr` | live |
| [`et_expr`](#et-expr) | `#و-expr` | live |
| [`equality`](#equality) | `#equality` | live |
| [`equality_tail`](#equality-tail) | `#equality-tail` | live |
| [`comparison`](#comparison) | `#comparison` | live |
| [`bitwise_or_expr`](#bitwise-or-expr) | `#bitwise-or-expr` | live |
| [`bitwise_xor_expr`](#bitwise-xor-expr) | `#bitwise-xor-expr` | live |
| [`bitwise_and_expr`](#bitwise-and-expr) | `#bitwise-and-expr` | live |
| [`shift_expr`](#shift-expr) | `#shift-expr` | live |
| [`range_expr`](#range-expr) | `#range-expr` | live |
| [`range_tail`](#range-tail) | `#range-tail` | live |
| [`additive_expr`](#additive-expr) | `#additive-expr` | live |
| [`multiplicative_expr`](#multiplicative-expr) | `#multiplicative-expr` | live |
| [`vel_expr`](#vel-expr) | `#عوض-expr` | live |
| [`vel_rhs`](#vel-rhs) | `#عوض-rhs` | live |
| [`vel_range_tail`](#vel-range-tail) | `#عوض-range-tail` | live |
| [`unary_expr`](#unary-expr) | `#unary-expr` | live |
| [`gradient_expr`](#gradient-expr) | `#gradient-expr` | live |
| [`gradient_selection`](#gradient-selection) | `#gradient-selection` | live |
| [`gradient_place`](#gradient-place) | `#gradient-place` | live |
| [`cast_expr`](#cast-expr) | `#cast-expr` | live |
| [`conversio_expr`](#conversio-expr) | `#conversio-expr` | live |
| [`inline_default`](#inline-default) | `#inline-default` | live |
| [`call_expr`](#call-expr) | `#call-expr` | live |
| [`call_suffix`](#call-suffix) | `#call-suffix` | live |
| [`member_suffix`](#member-suffix) | `#member-suffix` | live |
| [`transpose_suffix`](#transpose-suffix) | `#transpose-suffix` | live |
| [`optional_suffix`](#optional-suffix) | `#optional-suffix` | live |
| [`non_null_suffix`](#non-null-suffix) | `#ليس-null-suffix` | live |
| [`argument_list`](#argument-list) | `#argument-list` | live |
| [`argument`](#argument) | `#argument` | live |
| [`template_argument`](#template-argument) | `#template-argument` | live |
| [`literal`](#literal) | `#literal` | live |
| [`primary`](#primary) | `#primary` | live |
| [`ad_expr`](#ad-expr) | `#اتصل-expr` | live |
| [`ad_opener`](#ad-opener) | `#اتصل-opener` | live |
| [`array_literal`](#array-literal) | `#array-literal` | live |
| [`iuncta_expr`](#iuncta-expr) | `#توبل-expr` | live |
| [`json_literal`](#json-literal) | `#json-literal` | live |
| [`json_member`](#json-member) | `#json-member` | live |
| [`typed_constructor`](#typed-constructor) | `#typed-constructor` | live |
| [`field_list`](#field-list) | `#field-list` | live |
| [`field_init`](#field-init) | `#field-init` | live |
| [`field_key`](#field-key) | `#field-key` | live |
| [`json_value`](#json-value) | `#json-value` | live |
| [`json_object`](#json-object) | `#json-object` | live |
| [`json_array`](#json-array) | `#json-array` | live |
| [`json_string`](#json-string) | `#json-string` | live |
| [`json_number`](#json-number) | `#json-number` | live |
| [`finge_expr`](#finge-expr) | `#أنشئ-expr` | live |
| [`qualified_ident`](#qualified-ident) | `#qualified-ident` | live |
| [`praefixum_expr`](#praefixum-expr) | `#بادئة-expr` | live |
| [`scriptum_expr`](#scriptum-expr) | `#حرر-expr` | live |
| [`lege_expr`](#lege-expr) | `#اقرأ-expr` | live |
| [`first_match_expr`](#first-match-expr) | `#first-match-expr` | live |
| [`summa_expr`](#summa-expr) | `#مجموع-expr` | live |
| [`filum_clause`](#filum-clause) | `#خيط-clause` | live |
| [`capta_expr`](#capta-expr) | `#فخ-expr` | live |
| [`object_pattern`](#object-pattern) | `#object-pattern` | live |
| [`pattern_property`](#pattern-property) | `#pattern-property` | live |
| [`array_pattern`](#array-pattern) | `#array-pattern` | live |
| [`array_pattern_element`](#array-pattern-element) | `#array-pattern-element` | live |
| [`nota_stmt`](#nota-stmt) | `#اعرض-stmt` | live |
| [`entry_header`](#entry-header) | `#entry-header` | live |
| [`incipit_stmt`](#incipit-stmt) | `#بداية-stmt` | live |
| [`incipiet_stmt`](#incipiet-stmt) | `#استهلال-stmt` | live |
| [`probandum_decl`](#probandum-decl) | `#مختبر-decl` | live |
| [`probandum_body`](#probandum-body) | `#مختبر-body` | live |
| [`proba_stmt`](#proba-stmt) | `#اختبر-stmt` | live |
| [`proba_modifier`](#proba-modifier) | `#اختبر-modifier` | live |
| [`praepara_block`](#praepara-block) | `#جهز-block` | live |
| [`fac_stmt`](#fac-stmt) | `#افعل-stmt` | live |

## Lexicon Appendix {#lexicon}

The lexical tier is descriptive and remains owned by the live lexer and
driver. `capture-pending` rows intentionally carry no invented token shape.

| Terminal | Status | Capture notes |
|---|---|---|
| `IDENTIFIER` | `capture-pending` | Lexical tier. Empty RHS; status is capture-pending. radix-lexer / driver / parser is the authority (crates/radix-lexer/src/). Not a second lexer spec. scan.rs scan_identifier; Unicode XID_Start or '_' then XID_Continue or '_'; NFKC intern; TokenKind::Ident (keywords also lex as identifiers) |
| `NUMBER` | `capture-pending` | scan.rs scan_number; decimal/hex/bin/oct integers and floats with '_' separators; TokenKind::Integer(u64) or Float(f64); scan.rs also lexes the glyph '∞' as Float(+inf) |
| `NATURAL` | `capture-pending` | not a distinct lexer token; type-position TokenKind::Integer used as magnitudo capacity (no fraction/exponent) |
| `STRING` | `capture-pending` | scan.rs scan_string / scan_guillemet_block_string; double-quoted or guillemet block; TokenKind::String |
| `ASCII_STRING` | `capture-pending` | scan.rs scan_ascii_string; single-quoted; TokenKind::AsciiString |
| `BACKTICK_STRING` | `capture-pending` | scan.rs scan_backtick_string; backtick forma template; TokenKind::BacktickString |
| `OCTETI_STRING` | `capture-pending` | scan.rs scan_octeti_string; pipe-delimited hex; TokenKind::OctetiString |
| `NEWLINE` | `capture-pending` | scan.rs scan_line_break; LF or CRLF; TokenKind::Newline |
| `WIDTH_MARKER` | `capture-pending` | parser type-position identifier i8/i16/i32/i64/u8/u16/u32/u64 and decimal d32/d64 (numerus only), f16/bf16/f32/f64 (fractus only); not a lexer token |
| `LISTA_WIDTH_SUGAR` | `capture-pending` | parser type-position l + WIDTH_MARKER; not a lexer token |
| `TENSOR_WIDTH_SUGAR` | `capture-pending` | parser type-position t + WIDTH_MARKER; not a lexer token |
| `SPARSA_WIDTH_SUGAR` | `capture-pending` | parser type-position s + WIDTH_MARKER; not a lexer token |
| `VECTOR_WIDTH_SUGAR` | `capture-pending` | parser type-position v + WIDTH_MARKER; not a lexer token |
| `MATRIX_WIDTH_SUGAR` | `capture-pending` | parser type-position m + WIDTH_MARKER; not a lexer token |
| `FRONTMATTER_DELIMITER` | `capture-pending` | driver peels a line whose trimmed content is exactly +++ before lexing |
| `TOML_LINES` | `capture-pending` | driver; TOML body between FRONTMATTER_DELIMITER lines |
| `ANNOTATION_NAME` | `capture-pending` | parser; identifier spelling after @, including keyword spellings |
| `ANNOTATION_FIELD_NAME` | `capture-pending` | parser; identifier spelling in annotation field position |
| `NON_NEWLINE_TOKEN` | `capture-pending` | parser; one ordinary token other than TokenKind::Newline |
| `NO_NEWLINE` | `capture-pending` | parser zero-width constraint: adjacent parts stay on the same logical line |

## Keyword Reference {#keyword-reference}

This table is derived from the quoted Latin literals in the source
productions. It is not a second keyword authority.

| Category | Faber | Meaning |
|---|---|---|
| Iteration | `نطاق` | range iteration |
| Endpoints | `اتصل` | capability call |
| Error | `أكد` | assert |
| Iteration | `قبل` | range until exclusive |
| Grammar | `عند` | keyword literal derived from the production |
| Params | `وسائط` | CLI arguments modifier |
| Boolean | `أو` | or |
| Annotation | `backward` | `@ radix` gradient-companion directive |
| Error | `التقط` | local handler |
| Error | `فخ` | capture boundary (error channel reified as a value) |
| Control | `حالة` | case |
| Async | `سلم` | yield |
| Params | `باقي` | rest |
| Control | `افتراضي` | default case |
| Objects | `إغلاق` | legacy closure |
| Declarations | `عمود` | relational column (experimental; census-types) |
| Type | `نسخة` | copy ownership |
| Control | `احرس` | guard |
| Type | `عن` | borrow / for-in keys |
| Control | `طابق` | pattern match |
| Declarations | `تمايز` | tagged union |
| Control | `طالما` | while / postfix until |
| Objects | `ذات` | self |
| Control | `اختر` | switch |
| Control | `إذن` | compact statement-body joint |
| Params | `مخطئ` | error channel |
| Testing | `توقع_الفشل` | expect failure |
| Boolean | `هو` | is / equality |
| Boolean | `و` | and |
| Iteration | `من` | for-of / import from |
| Params | `مخرج` | exit code |
| Control | `افعل` | do block / post-test loop |
| JSON | `false` | JSON false |
| Boolean | `خطأ` | false |
| Async | `مولد_غيرمتزامن` | async stream posture |
| Async | `غيرمتزامن` | async finite posture |
| Async | `انتظر_ثابت` | await-bind immutable |
| Grammar | `خيط` | keyword literal derived from the production |
| Objects | `أنشئ` | construct variant |
| Async | `مولد` | sync stream posture |
| Declarations | `ثابت` | immutable binding |
| Testing | `هش` | flaky |
| Annotation | `جزء` | nucleum fragment |
| Declarations | `دالة` | function |
| Testing | `مستقبلي` | future |
| Genus | `سكوني` | static member |
| Declarations | `صنف` | class |
| Error | `ارم` | throw |
| Error | `يرمي` | throws marker |
| Params | `ثابتة` | immutable modifier |
| Declarations | `عقد` | interface contract |
| Genus | `حقق` | implements |
| Declarations | `استورد` | import |
| Type | `في` | ownership in |
| Declarations | `استهلال` | async entrypoint |
| Declarations | `بداية` | entrypoint |
| Iteration | `بين` | between |
| Iteration | `ضمن` | membership |
| Control | `كرر` | for |
| Objects | `توبل` | tuple type/constructor |
| Annotation | `مسار` | `@ radix` compiler-lane directive |
| Builtin | `اقرأ` | read |
| Objects | `حر` | capture-free closure modifier |
| Builtin | `سطرا` | line |
| Declarations | `حجم` | size/index generic parameter |
| Testing | `قس` | benchmark |
| Diagnostics | `نبه` | warn |
| Error | `انهر` | panic |
| Literals | `لاشيء` | none |
| Declarations | `اسم` | import binding name |
| Boolean | `ليس` | not |
| Literals | `nan` | named NaN literal (`nan` outside the Latin pack) |
| Diagnostics | `اعرض` | note |
| Annotation | `نواة` | kernel annotation; kernel closure modifier |
| JSON | `null` | JSON null |
| Testing | `أهمل` | skip |
| Params | `جميع` | all / glob |
| Params | `خيارات` | options modifier |
| Declarations | `ترتيب` | enum |
| Type | `ملك` | owned |
| Iteration | `كل` | range step |
| Control | `تابع` | continue |
| Testing | `لاحق` | teardown |
| Testing | `سيلحق` | async teardown |
| Objects | `بادئة` | prefix expression |
| Testing | `جهز` | setup |
| Testing | `سيهيئ` | async setup |
| Grammar | `أول_مطابقة` | first-match selection head |
| Testing | `اختبر` | test |
| Testing | `مختبر` | test suite |
| Declarations | `عام` | public visibility |
| Annotation | `radix` | compiler-reserved annotation family |
| Objects | `ratio` | named-field aggregate type/constructor |
| Control | `أعد` | return |
| Async | `أعد_منتظرا` | await-return |
| Error | `ارفض` | reject |
| Testing | `معاد` | repeat |
| Error | `يتطلب` | require |
| Control | `اكسر` | break |
| Declarations | `مخطط` | relational heading (experimental; census-types) |
| Diagnostics | `اكتب` | diagnostic channel |
| Builtin | `حرر` | write |
| Control | `وإلا` | else |
| Control | `إذا` | if |
| Control | `فإذا` | then (ternary) |
| Control | `وإلاإذا` | else-if |
| Declarations | `ليكن` | inferred immutable local |
| Testing | `فقط` | only |
| Testing | `حصري` | only-in |
| Params | `انشر` | spread |
| Declarations | `اختياري` | optional declaration slot |
| Grammar | `مجموع` | keyword literal derived from the production |
| Async | `انتظر` | await-discard |
| Control | `صمت` | no-op |
| Testing | `وسم` | tag |
| Testing | `زمني` | timeout |
| JSON | `true` | JSON true |
| Declarations | `نمط` | type alias |
| Grammar | `حيث` | first-match predicate tail |
| Iteration | `حتى` | range until inclusive |
| Params | `كـ` | as / alias |
| Declarations | `متغير` | mutable binding |
| Async | `انتظر_متغير` | await-bind mutable |
| Boolean | `عوض` | nullable default |
| Boolean | `صواب` | true |
| Diagnostics | `شاهد` | debug |
| Declarations | `visibilitas` | visibility field |

## Comma Separator Table {#comma-separator-law}

Optional commas are forbidden. The source currently has no `','?`
positions; every comma-bearing production is either required or absent.

| Production | Source row |
|---|---|
| — | no optional comma positions |

## Normative Language Notes {#normative-language-notes}

Formal grammar for the Faber programming language. This file is the canonical
grammar and spec-commentary surface for the public language; the compiler
(Radix) implements it. The rendered, localized grammar is published on
[the documentation site](https://faberlang.dev/en-US/reference/grammar.html).

Documentation contract: runnable language reference programs live in the public
sibling [`examples/corpus/`](../../examples/corpus/) with optional `+++`
frontmatter (`term`, `syntax`, `related`, …); the generated manifest is
[`examples/corpus/index.toml`](../../examples/corpus/index.toml). `faber
explain` loads the exempla reference pack from disk. Prefer the language corpus
+ EBNF for new reference work.

---

## Program Structure

Faber source files are raw text peeled by the driver before lexing. Optional TOML
frontmatter is not part of the token grammar. Within Faber syntax, spaces,
tabs, and newlines are trivia unless a production explicitly names `NEWLINE`.
Canonical forms are safe to compress onto one line. Any line-sensitive syntax is
explicitly sugar; a compressor must expand it when a lossless canonical mapping
exists, and otherwise preserve its boundary or reject compression. Line comments
remain line-oriented trivia and must be removed or relocated safely by a compressor.


Uppercase names are lexical terminals. `FRONTMATTER_DELIMITER` is a line whose
trimmed content is exactly `+++`; `TOML_LINES` is the possibly empty sequence of
complete TOML lines before the closing delimiter. `NON_NEWLINE_TOKEN` means one
ordinary source token other than a newline. `ANNOTATION_NAME` and
`ANNOTATION_FIELD_NAME` are identifier spellings in annotation-owned contexts;
they include spellings that are keywords in other contexts. `NO_NEWLINE` is a
zero-width constraint requiring adjacent grammar parts to remain on the same
logical line.

### File frontmatter (`+++`)

When present, frontmatter must open on **line 1** with exactly `+++`. A later line
that trims to exactly `+++` ends the block. Bytes after the closing delimiter are
the Faber `program`. An empty body (whitespace only) is a valid empty program.

Frontmatter is parsed as a generic TOML document in the compiler driver — not
parsed as Faber statements. Authors may attach arbitrary metadata keys; tooling
reads known keys such as `group`, `sectio`, and `[probanda]` via accessors.
`faber` package tooling consumes those package keys. Package authority for
`[package]`, `[paths]`, and `[build]` remains `faber.toml`; conflicting
frontmatter values are rejected in package mode.

Example:

```fab
+++
group = "exempla.directiva"
sectio = "smoke"
+++

incipit {}
```

Line-start `§` file directives were removed. Put file metadata in `+++`
frontmatter instead. Inside quoted strings, `§` remains the string-template hole
(see **Call and Member Access** below).

### Comma separator law

Every comma position is either required or forbidden. Optional commas do not
exist.

**Item lists** — homogeneous entries inside a bounded header (`lista` literals,
call arguments, parameters, type argument lists, figura lists, field-init
lists, `ترتيب` members, `تمايز` variant lists, JSON members and array
elements, annotation / import / nucleum fields, output statement lists) —
require a comma between adjacent items and forbid one after the last.

**Declaration blocks** — self-annotating declarations (statements, `صنف`
members, `عقد` methods, `تمايز` payload fields) — contain no commas.
Entries are trivia-delimited.

---

## Declarations

Declarations are top-level. A `دالة` and the type declarations (`صنف`,
`عقد`, `نمط`, `ترتيب`, `تمايز`, `مخطط`) may not appear inside a
block; the parser rejects them there (`declaration_not_top_level`). Methods
live in `صنف` bodies. For a local function, bind a closure; for recursion,
use a top-level function.

### Variables


- `ثابت` = immutable binding (write-once): it may be declared without an
  initializer and assigned exactly once later, then frozen. `متغير` = mutable
  binding (reassignable), like `let`.
- `انتظر_ثابت` / `انتظر_متغير` await a `promissum<T>` or `promissum<T ⇥ E>`, bind
  the resolved `T`, and propagate a compatible alternate `E`.
- `↢` is the await-directed initializer for an ordinary declaration:
  `ثابت T name ↢ future`, `متغير T name ↢ future`, or `ليكن name ↢ future`.
  It has the same await and alternate-propagation semantics as
  `انتظر_ثابت T name ← future`, but it is not a general expression operator and
  cannot target an existing place.
- Use `_` as the type annotation when the initializer determines the type: `ثابت _ name ← value`
- `ليكن name ← value` is sugar for `ثابت _ name ← value` (inferred immutable local)
- `ليكن name` (no initializer) is sugar for `ثابت _ name` — the inferred deferred
  immutable. Assign exactly once before any read.
- Typed `ثابت`/`متغير` initializers accept `↤` (`ثابت numerus x ↤ "42"`):
  the written type is the conversion destination, then the binding is
  initialized. `انتظر_ثابت`/`انتظر_متغير` keep `←`; `ثابت _`, `ليكن`, and untyped
  destructuring reject `↤` (no concrete destination type).
- Deferred init: `ثابت numerus x` or `ليكن x` declares an uninitialized immutable
  slot that must be assigned exactly once before any read; a second assignment is
  rejected. The definite-assignment pass (semantic Phase 3a) enforces this.

### Functions


### Capture-free closures


`حر` is the canonical Latin spelling of the `closure_modifier`; the English reader spelling is `free`. The modifier follows the parameter list in both compact and legacy `إغلاق` forms, before any `→` return or `⇥` alternate-exit clause. It declares a checked capture-free contract: the closure may use its own parameters, body locals, and module-level items, but it must not reference a local or parameter from an enclosing function. Such a capture is rejected by the compiler.

```fab
sit summa ← (numerus a, numerus b) libera ∴ a + b
clausura numerus x libera: x * 2
```

`نواة` is the second spelling of the `closure_modifier`; the English reader spelling is `kernel`. The alternative is locale-sealed and singular: at most one modifier may occupy the slot, each reader pack admits only its declared spelling, and stacked spellings such as `free kernel` are rejected as a duplicate modifier. A `kernel` closure requires everything `free` requires — no reference to an enclosing function's local or parameter, while its own parameters, body locals, and module-level items stay legal — plus the device-safe subset used by kernel functions: typed tensors and scalars, glyphs, structured control, and calls to other device functions. Host allocation, I/O, bags, dynamic calls, `⇥` clauses, `ارم` throws, and `التقط` recovery are rejected in the kernel contract; `أعد` returns only the closure's own `→` result. Declaration annotations `@ نواة` (`@ kernel` in the English reader) are unchanged: they remain the role marker for named functions, and the closure modifier is their expression-form twin.

The body joint keeps the existing closure law: `∴` followed by one expression, or `∴ افعل { ... }` (`do` in the English reader); bare `{ ... }` is not a closure body. A kernel closure is usable only as a local immutable binding in its enclosing function and only called there, or invoked immediately in the same expression; it is not a first-class value and cannot escape into a field, list element, return value, or ordinary-function argument. The compiler lowers it to a private synthetic kernel with a stable identity: one launch when its host caller invokes it, direct composition with no surviving device-to-device runtime call when a kernel caller invokes it, and never a public launch entry or ABI row. The modifier does not request fusion; two local kernel closures remain two launches unless a later cross-launch pass fuses them.

```fab
fixum _ duplica ← (tensor<f32, [8]> x) nucleum ∴ x + x
fixum _ dup ← duplica(xs)
```

- Return syntax: `→` declares the normal success type. A bodyful function with no `→` is effect-only (`vacuum`) and must not contain `أعد`. A statement-bodied closure (`افعل { ... }` or legacy block body) must also spell `→ T` before it can use `أعد`; expression-bodied closures may infer their result from the expression.
- Recoverable alternate-exit syntax: `⇥` declares the error-channel type. It can appear after `→ T` or alone on an effect-only failable function or closure. A closure body that uses an escaping `ارم` must declare its own `⇥ E`; it cannot inherit the enclosing function's error channel. A local `افعل { ... } التقط err { ... }` may catch `ارم` without an enclosing `⇥`. A failable function call (`→ T ⇥ E`) inside a `⇥`-declaring function propagates to the function's alternate exit without a `افعل`/`التقط` wrapper, mirroring how bare `↦` conversio and `ارم` throws already behave; the call lowers to Rust `?`. A closure must still declare its own `⇥` to propagate a failable call — the enclosing function's error channel does not cross the closure boundary.
- In a signature, `⇥` only ever names an error type (`→ T ⇥ E`). It never carries a value.
- Parameter access markers live in the type position: `عن`/`ref` (read), `في`/`mut` (mutate), `ملك` (consume), and `نسخة` (duplicate then own). The retired parameter-prefix slot is not part of the grammar; `من`/`from` remains the import/iteration/extraction token identity.
- Post-name marker: `اختياري` (voluntary/optional provision)
- `باقي` marks rest parameter
- Ordinary `دالة` declarations and genus methods require bodies. Signature-only methods belong in `عقد`.
- `مخطئ NAME` is a legacy runtime-injected `ignotum` local, and `يرمي` is a legacy marker with no current semantic effect. Neither declares the typed alternate-exit contract. New failable APIs should use `⇥ E`; whether either legacy modifier should survive is unresolved.
- `إذن` is the compact **statement-body** joint only (one-statement `إذا`/`طالما`/`حالة`/… arms).
- `∴` is the compact **clausura** joint only. The two are not aliases.
- Compact closure block bodies must use `افعل { ... }`; a closure-local `افعل` body may attach `التقط`, but cannot use postfix `طالما`.

### Classes

A `صنف` is a struct with methods. It holds data, its methods act on that
data, and it satisfies contracts through `حقق`. It is not a self-contained
object that owns its own construction and process: a value is built with a
construction literal (`Genus { field = value }`).

- **No class inheritance.** Inheritance was removed: there is no `sub`
  (extends) clause and no `abstractus` genus. Shared behaviour comes from
  contracts (`عقد` + `حقق`) and from composition — a field holding
  another value. The old spellings are rejected with a migration diagnostic.

- **No static methods.** A `صنف` declares instance methods only. A function
  about a type is a top-level function in the type's file, reached through the
  import alias. `سكوني` marks a type-level field, never a method.
- **A newtype is a one-field `صنف`.** There is no separate newtype
  declaration. Units that need arithmetic wait on operator overloading.
- **No macros and no user derive.** What you read is what runs. Code
  generation, when a project needs it, is an external step before the build.
- **No extension methods and no retroactive conformance, for now.** A type's
  methods and its `حقق` contracts are declared on the type itself. Code
  elsewhere cannot add either. Allowing it would need coherence rules, and is
  revisited together with the contract features that are deferred.

### Annotations


`@ نواة جزء` is a modifier on the `نواة` annotation (sugar or
braced `جزء = صواب` / `خطأ`), not a fused annotation name and not the
graphics `@ جزء` stage. Standalone `@ جزء` is unchanged.

Braced annotation records (`@ futura { }`, `@ optio { binding = verbose, ... }`)
are canonical and compression-safe. Unbraced annotations are line-sensitive,
non-compression-safe sugar that consumes through `NEWLINE`; the newline is part
of this sugar grammar, not a general Faber statement separator. A compressor may
rewrite promoted families only when their named-field mapping is known. It must
otherwise preserve the line break or reject compression. Promoted sugar and
braced forms lower to the same `HirAnnotation` records. Unpromoted positional
families preserve raw arguments and do not yet have a lossless braced expansion.

The current Radix parser still accepts only a fixed token subset in unbraced
payloads and ends them with declaration-boundary heuristics rather than `NEWLINE`.
Those are implementation mismatches with this specification, not alternate
language rules.

**Annotation contracts:** `@ annotatio` (optionally `@ annotatio { target = دالة }`)
marks a top-level `صنف` as a compile-time annotation contract. Ordinary genera
are not annotation schemas. Applications use `@ ContractName { field = constant }`
and resolve through local declarations or imported file-interface exports.
Resolved applications lower to `HirAnnotation` with `contract_id: Some(DefId)`
and constant field values. v1 attachment target is `دالة` only; payload
scalars are `textus`, `numerus`, `fractus`, and `bivalens` (optional via
`اختياري` or `T ∪ لاشيء`). Web, HTTP, controller, and framework route families
are not compiler-owned; they are built as libraries, from annotation contracts
or on top of `@ اتصل`. The one exception is `@ اتصل` itself: it is the
compiler-owned serving half of `اتصل` (see Capability Calls).

User annotations are metadata. Their consumers are tools, such as product
packaging. They never change compilation, and Faber code never reads them at
run time. An annotation that changes compilation is compiler-owned (`@ json`,
`@ اتصل`, `@ radix`).

**JSON genera:** `@ json` on a `صنف` is a compiler-owned data-model contract,
not a generic annotation schema. Fields must be JSON-safe (`textus`, `ascii`,
`numerus`, `fractus`, `bivalens`, `instans`, `لاشيء`, `lista<T>`,
`tabula<textus, T>`, nullable `T ∪ لاشيء`, or another `@ json صنف`). Field
metadata `@ json { اسم = "wire_name" }` changes the emitted object key used by
`value ↦ valor`, `value ↦ json`, and `json ↦ Genus`; JSON text remains a Norma
wire operation such as `json.pange(value ↦ json)`.

- `@ radix` is **compiler-reserved**: every form under it is compiler-owned
  metadata, not an application surface, and may change with the compiler.
  The historical morphology-stem meaning is retired; morphology remains a
  source naming discipline, not compiler-generated conjugation. The family
  (`radix_annotation` plus the braced records) is:
  - `@ radix مسار "air"` / `"mir"` / `"hir-direct"` (braced
    `@ radix { مسار = "air" }`) on top-level functions for explicit
    compiler-lane routing; unsupported lane/target combinations reject with
    diagnostics instead of being ignored.
  - `@ radix backward "name"` on an `air`-lane function names the generated
    reverse-mode gradient companion; it is valid only paired with
    `مسار "air"`.
  - `@ radix نمط T في A B …` (braced `@ radix { param = T, allowed = A, … }`)
    restricts the type parameter `T` of the annotated declaration to the listed
    domain.
  Any other directive after `@ radix` is rejected (`unknown_directive`).
- `@ verte` defines codegen transformation (method name or template)
- `@ nondum [TARGET] ["REASON"]` marks a declaration as present in an interface but unavailable for the target
- `@ cli "NAME"` marks an `بداية` entry as a CLI program
- `@ imperium "NAME"` marks a function as a CLI command entry point
- `@ optio NAME ...` defines a CLI option; use `نمط bivalens` for boolean flags
- `@ operandus [باقي] TYPE NAME ...` defines a CLI positional argument
- `@ futura` marks a function as async (legacy — prefer `غيرمتزامن` posture word)
- `@ cursor` marks a function as generator (legacy — prefer `مولد` posture word)
- Callable posture words (`غيرمتزامن`/`مولد`/`مولد_غيرمتزامن`) are recognized in the signature
  slot after modifiers and before `→`/`⇥`/body; bare means synchronous finite
- `@ عام` marks a declaration for the file's importable (export) surface; `@ interna` marks it package-internal (same-package importable only); `@ privata` is an explicit module-private marker. Unmarked top-level declarations are module-private by default; a declaration mixing distinct visibility tiers is rejected with `SEM019` (`conflicting_visibility`)
- `@ protecta` is reserved and rejected with a semantic diagnostic; it has no package, subclass, or sibling-file visibility meaning
- `@ doc` is not an annotation. Comments are the documentation: a line comment attaches forward to the declaration it precedes, and there is no doc marker.

- `حقق` = implements (conformance to an `عقد` contract)
- `سكوني` = static (type-level) field. A field has no other modifier; the
  former `nexum` field modifier is removed and rejected with a migration
  diagnostic.
- Members are public by default. Until member privacy is enforced,
  `@ privata` or `@ interna` on a `صنف` member is an error
  (`member_privacy_unenforced`), because it promises privacy nothing checks;
  `@ عام` on a member is a redundant-annotation warning
  (`redundant_member_publica`), an error when warnings are denied.
- A type may refer to itself: `تمايز Expr { Adde { Expr sinister, Expr dexter } }`
  and `صنف Nodus { Nodus ∪ لاشيء next }` need no keyword and no box type.
  Values have reference semantics, so the indirection is implied; a backend
  that stores fields inline inserts it on the fields that close a type cycle.

### Interfaces


`عقد` is the **contract** construct: signature-only methods for `حقق`
(gerundive of *implere* — that which must be fulfilled). Import namespaces are
`.fab` file boundaries; exported declarations live at file top level.

A contract has no default method bodies. Default bodies would make a contract
an abstract base class without fields. Behaviour shared by every implementer
is a top-level function that takes the contract type. Contract inheritance (a
contract that requires another), associated types, and retroactive
conformance are deferred.

### Type Aliases


### Enums


### Tagged Unions


Variant lists are an item list: comma required between variants, forbidden
after the last. Payload fields inside a variant are a declaration block
(genus-style, no commas).

### Relational Schemas (experimental)

**Experimental** — owned by the `census-types` goal; the surface may change.
`مخطط Name { عمود T name … }` declares an application-owned relational
heading for database results. It names only the columns the application reads;
extra source columns stay invisible. Each `عمود` row takes a type (use
`T ∪ لاشيء` for a nullable column) and a name, with an optional
`: sourceName` alias mapping the public column to a source column (absent means
identity). Column rows are a declaration block (no commas). A schema has no
methods (`schema_method`), no `حقق`
(`schema_inheritance`), and no nested columns (`schema_nested_column`); each is
rejected at parse time.

### Identifier Naming

Faber has no globally reserved words. Keyword ownership is contextual per
spelling: a keyword claims only its owning grammar slot. Every user-chosen
name slot accepts every keyword spelling — declaration names, parameters,
members, binding targets (`ثابت`/`متغير`/`ليكن` patterns and captures),
import aliases, and loop/iteration bindings. Type-name slots stay out.

Outside a spelling's owning contexts, that spelling may be an `IDENTIFIER`.
An owning context may itself be effectively global when its production
applies everywhere a statement or expression may begin. Builtin claims
(`اقرأ`/`سطرا`/`حرر`/`vacua`, and the scribe family in
statement-initial position) are defaults, not reservations: a user binding
of the same surface spelling wins.

Radix still emits globally reserved tokens for some spellings and selectively
reinterprets them as identifiers. That is transitional implementation behavior;
it does not replace the contextual language rule above.

Mixed-case lower-initial names are syntactically accepted but not
Faber-preferred for language, stdlib, host routes, or compiler-owned intrinsic APIs.
Prefer one word. If one word cannot carry the meaning, use snake_case only in
rare cases. If neither shape works, the method probably does not belong in the
core surface unless it is critical. Stdlib encode/decode uses the
mechanical verb trio `pange` / `solve` / `tempta` across modules — see
`docs/stdlib/stdlib-mechanical-verbs.md`. The public text library is
`norma:chorda` — see `docs/stdlib/chorda-methods.md`.

### Imports


Example:

```fab
importa ex "hono" Hono
importa ex "hono" Context
# No marker: no re-export.
importa ex "norma:chorda"
importa { ex = "norma:json/solve", ut = solve_mod }
importa ex "norma:consolum" consolum
# Kernel manifest glob.
importa ex "faber:*" faber
importa ex "lodash" * ut _
# Re-export.
importa ex "./types" publica User
# Selective imports (values and types).
importa ex "norma:consolum" fixum dic ut output
```

The `privata` import marker was removed (VM-U3); an import without a marker
does not re-export, and `عام` is the re-export marker. Missing named binding
defaults to the
last import path segment when it is a valid, non-conflicting identifier. If the
inferred name is invalid or collides with an existing top-level binding, spell an
explicit `اسم` or `كـ` binding.

**Selective imports** create ordinary immutable local bindings: `استورد من "norma:consolum" ثابت dic كـ output, funde كـ output_bytes` imports one exported member per `ثابت` local. The pre-`كـ` identifier names an exported member in the imported file; the post-`كـ` identifier is the caller-owned local binding; the imported file interface supplies the complete type. A member may be a value (a function or constant) or a type declaration; the syntax is the same for both. The bindings obey ordinary local-binding rules (duplicates, shadowing, lints), are locale-resolved through the imported module, and are never re-exports. Wildcard members cannot mix into the list. The current parser tolerates one trailing comma after the final member; the canonical spine keeps every comma required.

`استورد من "faber:*" faber` is kernel-specific sugar: the glob lives
inside the import path string and expands the released binary's kernel manifest
into `faber.<module>.<verb>` calls. It is not a wildcard re-export and does not create a runtime aggregate value.

---

## Types


- Declaration parameters (`genericParams`) and applied arguments (`typeArguments`) are distinct grammar categories. Applied arguments admit nested types and static `figura` values. `typeArguments` still admits `NATURAL`.
- Applied `NATURAL` arguments are `حجم` capacity facts, not width markers. Shipped bounded forms use that slot: `lista<T, N>`, `queue<T, N>`, `stack<T, N>`, `textus<N>`, `ascii<N>`, `octeti<N>`. Width-marker families such as `numerus<i32>` stay the separate `widthTypeSugar` production below.
- A second applied argument on a `↦` target (`numerus<W, Hex>`, `numerus<W, Be>`) is a convert-slot hint, not a type identity, not a width marker, and not a keyword. Live text-parse hints are `Hex` / `Bin` / `Oct`. `Be` / `Le` occupy that same Hex slot for endian unpack — both integer (`octeti[lo‥hi] ↦ numerus<W, Be|Le>`) and float windows (`octeti[lo‥hi] ↦ fractus<f32|f64, Be|Le>`, window 4/8, same fail rules as the integer rows). `Bits` occupies the same slot as an exact-width bitcast hint (reinterpretation, not value conversion; never a base). `typeArguments` is unchanged: these are ordinary `IDENTIFIER` arguments interpreted by conversio, not new `baseType` productions.
- Type arguments admit the hole forms: `lista<∪>` infers a heterogeneous element union and `tabula<K, ∪>` a heterogeneous value union; `lista<_>` keeps the monomorphic single-inhabitant hole.
- Explicit generic call-site lists use the same `typeArguments` production: `id<_>(x)` is a type hole (equivalent to omitted `id(x)` for a one-param callee), and mixed lists such as `both<_, textus>(a, b)` are legal. Arity stays exact (`both<_>` is still one argument). `∪` in that list is rejected (`explicit_union_type_arg_unsupported`): a callee type param is a monomorphic witness slot.
- `labeledTypeArgument` is the optional label prefix on `توبل` type arguments only (`توبل<gx: f32, T>`; mixed labeled/unlabeled legal). A label in a non-`توبل` list (`f<gx: T>(x)`, `lista<gx: T>`) is a parse error. Absence is the only unlabeled form; there is no `_: T` spelling. Keyword spellings are legal labels under the contextual law (`توبل<ثابت: A>`).
- Labels are unique within one tuple type.
- The tuple type is spelled `توبل<…>`, not `(K1, K2)`. Parentheses already
  mean grouping, function types, parameters, and calls. Every other compound
  type is `name<args>`, and tuple labels come from the same type-argument
  machinery.
- Labels are erased from type identity: `توبل<gx: A, B> ≡ توبل<A, B>` for assignment, `≡`/`↦`, unify, and every emitter.
- Bracket index on a tuple requires a literal integer (`i[0]`); every element is reachable by position, labeled or not. Non-literal index expressions stay rejected. Positions are brackets only — no `.0`.
- Member-by-label (`i.gx`) requires that label to be present on the receiver's `توبل` annotation.
- `توبل` element slots admit `_` (monomorphic hole, solved element-wise from the single position witness) and reject `∪`. A wanted union element is declared with binary cup (`توبل<f32, textus ∪ لاشيء>`). `lista<∪>` / `tabula<K, ∪>` keep heterogeneous-union behavior. Labels compose with holes (`توبل<loss: _, T>`).
- `ratio` type arguments require a label for every element, labels are unique, `_` is admitted as a monomorphic element hole, and `∪` is rejected in an element slot. A `ratio` has no positional or bracket access, and it has no structural equivalence with another ratio or a genus; fields are accessed by label only.
- Arrays are written `lista<T>` (unbounded, shipped). Postfix `T[]` is not accepted. `lista<T, N>` is the shipped bounded form; see Generic Collections.
- `عن`/`في` mark ownership (borrow/mut-borrow) on the immediately following union member. Parenthesize when grouping must be explicit.
- Two hole kinds share the `holeType` production. `_` is the monomorphic hole ("infer exactly one inhabitant type"); the standalone `∪` is the union hole ("infer a finite multi-member union"). Both are legal wherever a base type is: bindings, returns, params, fields, and type arguments (`lista<∪>`, `tabula<K, ∪>`, `→ ∪`).
- **Lone-`∪` rule:** a `∪` hole consumes the whole type expression — any following `∪` is a parse error (`A ∪ ∪`, `∪ B` rejected, issue `unexpected_cup_after_union_hole`). `_` keeps today's behavior and may still appear as a binary-cup member (`_ ∪ B`).
- **Binary-cup disambiguation:** `∪` between two non-hole types remains the inline value-union operator (`A ∪ B`, nullable `T ∪ لاشيء`); the hole reading applies only when `∪` stands alone in a base-type position.
- Inline union `T ∪ U` (cup) for ad-hoc value unions; `T ∪ لاشيء` is the canonical nullable type form (lowers to Option<T>).
- Inline intersection `T ∩ U` (cap) is the nominal type intersection: `type Reversible = Readable ∩ Seekable` names the conjunction, and the implements clause accepts `∩` as the same separator as the comma (`class A implements Readable ∩ Seekable` ≡ the comma list). `∩` binds tighter than `∪` (`A ∩ B ∪ C` is `(A ∩ B) ∪ C`); nested intersections flatten like unions. Intersection operands are nominal-only (interfaces/structs; aliases resolve through) — primitive operands are rejected at lowering. Implements slots admit `∩` only: `∪` or a hole in an implements position is a parse error (disjunctive conformance is not a checkable contract).
- Signature clauses stay explicit: `_` and a standalone `∪` are rejected in return (`→ _`) and error-channel (`⇥ _`) positions; both holes stay legal in local binding slots (`const _ v`, `const ∪ v`).
- Unions are parsed as a flat member list; duplicates and `لاشيء`-only cases are diagnosed in semantic lowering.
- `اختياري` is a declaration marker (post-name on params/fields), never a prefix on types.
- Qualified type paths such as `terminus.Terminus` name a type through an
  imported namespace binding. The prefix must resolve to a namespace; the final
  segment must resolve to a type-bearing declaration.
- There is no runtime reflection. Types are compile-time facts. Serialization
  goes through conversion (`↦ json`, `↦ valor`).

Function types enable higher-order function signatures:

```fab
functio filtrata((T) → bivalens pred) → lista<T>
functio compose((A) → B f, (B) → C g) → (A) → C
functio apply((numerus) → numerus ⇥ textus op, numerus n) → numerus ⇥ textus
```

### Primitive Types

| Faber      | Meaning |
| ---------- | ------- |
| `textus`   | Unicode string |
| `textus<N>` | shipped; bounded Unicode string; `N` is a `حجم` / `NATURAL` capacity, not a width marker. `textus<_>` is the capacity hole (infer `N`). |
| `ascii`    | ASCII-only string |
| `ascii<N>` | shipped; bounded ASCII string; `N` is a `حجم` / `NATURAL` capacity, not a width marker. `ascii<_>` is the capacity hole (infer `N`). |
| `forma`    | captured template + params |
| `numerus`  | integer (default `i64`) |
| `modulus<W>` | unsigned modular word; arithmetic wraps modulo 2^W |
| `fractus`  | float (default `f64`) |
| `bivalens` | boolean |
| `لاشيء`    | null |
| `vacuum`   | void |
| `numquam`  | never |
| `ignotum`  | unknown |
| `octeti`   | bytes |
| `octeti<N>` | shipped; bounded byte buffer; `N` is a `حجم` / `NATURAL` capacity, not a width marker. `octeti<_>` is the capacity hole (infer `N`). |

Bare `textus` / `ascii` / `octeti` remain the unbounded productions. The
shipped forms `textus<N>`, `ascii<N>`, and `octeti<N>` take
one `حجم` / `NATURAL` applied argument. That `N` is capacity, not a
width marker and not a language-wide default. `_` in that slot (`ascii<_>`,
`textus<_>`, `octeti<_>`, `lista<T, _>`) is a capacity hole: the form stays
bounded, and `N` is inferred from a same-family bounded witness. Bare
`ascii` is not a hole.

Sized primitives accept one optional **width marker** (not a user type parameter):

| Family | Markers | Invalid example |
| ------ | ------- | --------------- |
| `numerus<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `d32`, `d64` | `numerus<f32>` → use `fractus<f32>` |
| `fractus<W>` | `f16`, `bf16`, `f32`, `f64` | `fractus<i32>` → use `numerus<i32>` |
| `modulus<W>` | `u8`, `u16`, `u32`, `u64` | `modulus<i32>` → signed widths are not modular words |

Bare `numerus` / `fractus` remain shorthand for `numerus<i64>` / `fractus<f64>`.

`numerus<d32>` and `numerus<d64>` are exact **decimal** widths: a decimal
literal in a decimal context (`numerus<d32> a ← 4.2`) keeps its digit text, and
arithmetic runs on a scaled-integer carrier (`d32` scale 10⁷, `d64` scale 10⁹)
with round-half-even reductions, so `4.2 + 0.1` is exactly `4.3`. The `d`
markers are valid only on `numerus` (`fractus<d32>` is rejected). Integer
literals in a decimal context are rejected (`decimal_integer_literal_rejected`);
write `1.0` or convert explicitly with `↦`.
`numerus<_>`, `fractus<_>`, `modulus<_>`, and `instans<_>` are marker holes:
the family stays identity and only the width/precision is inferred from a
same-family witness (exact marker, no lattice widening). Unsolved `_` is an
error, never the bare default. Convert-hint holes (`numerus<u32, _>`) are
not this form.

`modulus<W>` is a distinct semantic family: arithmetic does not mix implicitly
with `numerus<W>`, while explicit same-width conversion remains available.
Literals must be in `0..=2^W-1` (for `modulus<u64>` up to
`18446744073709551615`). Shift counts are themselves modular: `x ⇐ W` is a
full wrap. Cross-width modular arithmetic is rejected.

Conversion is the deliberate complement to the checked arithmetic policy:
`fractus ↦ numerus<W>` saturates at the target width — NaN converts to `0`,
and an out-of-range value clamps to the width's bounds (the cross-tier Rust
`as` status quo). The `∷` ascription surface follows the same saturation when
it crosses numeric families. Integer `numerus<W>` arithmetic errors on
overflow while float→integer conversion clamps; `modulus<W>` stays the only
wrapping family (FORK-2, operator mail 2fb79900). Runner cast-path alignment
is tracked as want 34821b73.

Overflow policy lives in the type, read once at the declaration. There are no
per-operation checked, wrapping, or saturating method families. To ask "does
this fit?" of untrusted input, convert it to the narrow type with `↦` and
handle the failure through the error channel.

### Generic Collections

| Faber          | Meaning  |
| -------------- | -------- |
| `lista<T>`     | array    |
| `lista<T, N>`  | shipped; bounded array; `N` is a `حجم` / `NATURAL` capacity, not a width marker. `lista<T, _>` is the capacity hole (infer `N`). |
| `queue<T>`     | shipped; unbounded FIFO queue |
| `queue<T, N>`  | shipped; bounded FIFO queue; `N` is a `حجم` / `NATURAL` capacity, not a width marker. `queue<T, _>` is the capacity hole (infer `N`). |
| `stack<T>`     | shipped; unbounded LIFO stack |
| `stack<T, N>`  | shipped; bounded LIFO stack; `N` is a `حجم` / `NATURAL` capacity, not a width marker. `stack<T, _>` is the capacity hole (infer `N`). |
| `tabula<K,V>`  | map      |
| `copia<T>`     | set      |
| `promissum<T>` | promise  |
| `cursor<T>`    | iterator |
| `tensor<T, Figura>` | dense homogeneous buffer with static shape `Figura`; numeric methods require numeric element types |
| `vector<T, N>` | register-class numeric vector with static width `N` (single dimension, not buffer-backed) |
| `matrix<T, [R, C]>` | register-class numeric matrix with exactly two static dimensions (not buffer-backed and not a tensor alias) |
| `atomic<T>` | storage-sensitive atomic cell; v1 accepts `i32` / `u32` elements only and access must go through atomic methods |
| `sparsa<T, Figura>` | sparse homogeneous buffer with static shape `Figura`; omitted coordinates equal zero; numeric methods require numeric element types |

A `figura` is `_`, a natural number, a size identifier, or a bracketed list of nested figura values; empty `[]` is rank-0. Bare `tensor<T>` is incomplete — use `tensor<T, []>` for rank-0 or `tensor<T, _>` to infer shape.

`vacua` for `tensor<T, []>` produces a rank-0 tensor (one default-initialized element slot).
`vacua` for `sparsa<T, Figura>` (any shape) produces an all-zero sparse tensor with no stored entries.
`matrix<T, Figura>` requires exactly two dimensions; bare `matrix<T>` and one- or three-axis matrix shapes are rejected.
`atomic<T>` requires `T` to be `i32` or `u32` in v1. Atomic cells are not interchangeable with their element type; use `load`, `store`, `exchange`, and `compare_exchange` receiver methods.
Construct multi-dimensional tensors via `crea` / `structa` / `↦`.
`Type(...)` is not a construction form: `vector<f32, 4>(...)`, `matrix<f32, [2, 2]>(...)`, `tensor<f32, [2, 2]>(...)`, and scalar forms such as `numerus("42")` are rejected. Use `value ↦ Type`, named library constructors, or `Genus { field = value }` records.

Tensor index/shape intrinsic slots (`accipe`, `ponde`, `forma`, `crea`, `structa`) accept integer lists that fit the canonical `lista<numerus>` / `&[i64]` runtime boundary at call sites (e.g. `lista<u32>` for GPU thread ids; not `lista<u64>`). This is a structural exception scoped to those slots — it does not widen the signed↔unsigned numeric lattice (see Index vector parameter policy in `tensor-intrinsics.md`).

Value unions use inline `T ∪ U` (nullable: `T ∪ لاشيء`). The standalone `∪` hole infers a multi-member union; `_` infers a single inhabitant (see `docs/design/type-hole-union.md`). Tagged unions use `تمايز`.
`copia.unio()` is a set method, not a type constructor.

### Type Sugar

Explicit long forms such as `numerus<u32>` and `lista<numerus<u32>>` are the
canonical spellings. Type sugar is an ergonomic alternate spelling for numeric
and collection types. It is **type-position only** and **semantically identical**
to the long form — the compiler treats both the same. This is the single
canonical reference for sugar; the rest of the specification uses long form.

Sugar combines a width marker with an optional one-letter family prefix. Width
markers are `i8`/`i16`/`i32`/`i64` (signed), `u8`/`u16`/`u32`/`u64` (unsigned),
and `f16`/`f32`/`f64` (float). A bare width marker (no prefix) sugars the scalar
numeric type; a family prefix sugars a collection of that width. In the grammar,
`WIDTH_MARKER` is a bare marker; `LISTA_WIDTH_SUGAR`, `TENSOR_WIDTH_SUGAR`,
`SPARSA_WIDTH_SUGAR`, `VECTOR_WIDTH_SUGAR`, and `MATRIX_WIDTH_SUGAR` are that
marker prefixed with `l`, `t`, `s`, `v`, and `m`, respectively.

| Sugar | Long form | Bracket rule |
| ----- | --------- | ------------ |
| `i8` … `u64`, `f16`/`f32`/`f64` | `numerus<W>`, `fractus<W>` | none (bare marker) |
| `lf32`, `lu32`, `li64`, … | `lista<f32>`, `lista<u32>`, `lista<i64>`, … | none |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | optional `Figura` |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | optional `Figura` |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 4>`, `vector<u32, 3>` | optional single width |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **required**, two dimensions |

Bracket shapes: `[]` is rank-0, `[2, 3]` is a fixed shape, and no bracket infers
the shape (`_`). Matrix requires exactly two dimensions. Sugar never uses `<>`.
For non-width element types (e.g. `tensor<textus, [3]>`), use the full form.

Sugar is reserved in type syntax only — value identifiers named `tf32`, `lf32`,
etc. are unchanged.

`modulus<W>` has no sugar; write `modulus<u32>` in full.

**Spelling preference (author convention, not grammar):** general Faber code
tends toward long form for readability; numeric/tensor-primary modules may
prefer sugar. Choose per module or file.

---

## Control Flow

### Conditionals


- `إذا` = if, `وإلاإذا` = else-if, `وإلا` = else
- `إذن` for one-statement bodies, including `إذن أعد`, `إذن ارم`, `إذن انهر`, and `إذن صمت` (`∴` is not accepted here)
- `صمت` for explicit no-op (from musical notation: "it is silent")

### Loops


- `طالما` = while
- `كرر من...ثابت`/`كرر من...متغير` = for-of (values)
- `كرر عن...ثابت`/`كرر عن...متغير` = for-in (keys)
- `كرر نطاق range ثابت/متغير i` = range iteration (e.g. `كرر نطاق 0‥10 كل 2 ثابت i { اعرض i }`; `كل` belongs to the range expression)

**Iteration order.** A type whose order is part of its value iterates in that
order. `lista` iterates by index. `textus` iterates its characters in order.
`tensor`, `vector`, and `matrix` iterate by index, outer axis first
(row-major). Two equal values always iterate identically.

`copia` and `tabula` iterate in unspecified order. The order is not promised
and not deliberately random; backends may differ. When order matters, sort
explicitly. `≡` on these types stays structural and does not depend on order.
A map or set that promises an order is a separate library type, not a mode of
`tabula` or `copia`.

There is no iteration interface. `كرر من` works on the built-in iterable
types and on cursors. A user type that should be iterable exposes an ordinary
method that returns a cursor (`كرر من arbor.nodi() ثابت n`); nothing is
called implicitly.

### Switch/Match

`طابق` is a statement, not an expression. A value chosen by a match comes
from a function whose arms each `أعد`. The compiler checks exhaustiveness
and definite return, and the function can be tested on its own.

Coverage is checked as a pattern matrix. Each scrutinee has a space: the
variants of an `ترتيب` or `تمايز`, the members of a union, and `bivalens`
as the closed set `{صواب, خطأ}`. A match over several scrutinees is
checked over their product, so `طابق a, b` over two `bivalens` values
needs all four combinations or a `افتراضي`. A missing variant or combination is
an error that names one uncovered case. Open types (`numerus`, `textus`, …)
are complete only with a catch-all arm. When coverage cannot be computed for a
pattern kind, the compiler warns that it was not checked; it is never silent.
`اختر` keeps its switch meaning: over an open domain, a missing `افتراضي` is
an implicit no-op default, while a closed domain is checked.

### Pattern Matching

Patterns are flat. A `حالة` arm names one variant and binds its fields, or names one literal
value; it does not match inside those fields. Nested patterns are left out for
simplicity, not because they cannot be checked: a `طابق` inside an arm is
two flat exhaustive switches.

A negative number pattern is written with a leading minus (`حالة -1`,
`حالة -∞`). The lexer never signs a number, so the pattern claims the sign;
`-` before anything else is not pattern syntax.

There are no range patterns (`حالة 1‥5`). Test the range with `إذا` inside the
arm.

A NaN pattern is rejected. NaN never equals itself, so it could never match;
test for NaN with `إذا` instead.

### Guards

Match arms have no guards. `طابق` is one arm per variant, and a guard
would split one variant's logic across several arms. Nest a `إذا` in the arm
instead.

### Destructuring Extraction

Destructuring is flat. A nested pattern such as `[[a, b], c]` is rejected;
destructure the outer value, then the inner one on another line.

Parameters are not destructured. A pattern in a parameter slot would hide the
parameter's type from a type-first signature. Destructure in the body.

### Control Transfer

`اكسر` and `تابع` take no label. They apply to the nearest enclosing loop.
A nested search that needs an early exit from an outer loop becomes a
function that `أعد`s.

- `أعد_منتظرا` awaits a compatible promise and returns its success value from a
  `غيرمتزامن` function.
- `انتظر` awaits a compatible promise to completion and discards any success
  value.
- `سلم` is statement-initial yield from `مولد` / `مولد_غيرمتزامن`; it is not an
  expression-form await.

---

## Error Handling


- `التقط` attaches to the structured forms whose productions name `catchClause`: conditional arms, `طالما`, `كرر`, `اختر`, and `افعل`. It does not attach to arbitrary bare blocks.
- Use the explicit do block when a standalone block needs a handler: `افعل { ... } التقط err { ... }`.
- `ارم` = throw (recoverable), `انهر` = panic (fatal).
- A same-line `إذا <expr>` guard on `ارم` and `انهر` is line-sensitive parser sugar: `ارم val إذا cond` desugars to `إذا cond { ارم val }` at parse time. Its canonical, compression-safe spelling is the expanded `إذا` block. A source compressor must expand this sugar before removing line breaks; the guarded shorthand remains under language review.
- `أكد` is a runtime invariant check. It desugars conceptually to `انهر "msg" إذا !cond`, with the positive condition kept in source form and the inversion applied during lowering. The optional particle is `انهر` (en `panic`): `أكد cond انهر msg` / `assert cond panic msg`. Bare `أكد cond` stays legal. An `أكد` failure is fatal and uncatchable by `التقط` (it lowers to a panic, not a `Result`-channel error); in test context the harness isolates each `اختبر` so a failed assertion ends that test without ending the suite.
- `يتطلب` is the recoverable require statement (en surface `require … throw …`), the typed-error-channel twin of `أكد`. `يتطلب cond ارم err` desugars to `إذا ليس (cond) { ارم err }` at lowering; the thrown value enters the function's `⇥ E` channel and is catchable by `التقط`/`افعل`, unlike `أكد` (fatal). A `يتطلب` statement in a `⇥`-less function is a compile error, same as `ارم`. The particle is `ارم` (en `throw`) and is required.

- `ارفض` is the reject statement (en surface `reject … throw …`), the boolean opposite of `يتطلب`. `ارفض cond ارم err` desugars to `إذا (cond) { ارم err }` at lowering — it throws when the condition holds, where `يتطلب` throws when it fails. The thrown value enters the function's `⇥ E` channel and is catchable by `التقط`/`افعل`. A `ارفض` statement in a `⇥`-less function is a compile error, same as `ارم`. The particle is `ارم` (en `throw`) and is required.
- `@ conversio` (en `@ conversion`) on a top-level `دالة` declares an admitted error conversion: the parameter's type is the source error, the return type is the destination, and the compiler enrolls that ordered pair so a propagating `⇥ E` failure converts at the boundary instead of needing a per-caller wrapper. The marker is bare and the conversion is an ordinary function outside any union body; only a direct (source, destination) row is admitted — a missing row fails closed and is never auto-composed into a chain. The earlier union-arm form (the marker carrying a payload inside a `تمايز` body) is retracted.
---

## Expressions

### Operators (by precedence, lowest to highest)


**Postfix tensor transpose (`ᵀ`, U+1D40):** `valueᵀ` is rank-2-only
sugar for the existing `transpone` intrinsic and `Transpose` plan entry. It
maps `[M,N]` to `[N,M]`; rank-1 is a permanent decline because there is no
row/column distinction, while rank-3+ waits for a batched-transpose consumer.
The precedence interaction with parse-only gradient selection is settled law,
not an open fork: `a · bᵀ ∇ [x]` parses `(a · bᵀ) ∇ [x]`, so the transpose
suffix is consumed before the selection suffix. `⊤` remains unspent.

**Hadamard divide (`⊘`):** `a ⊘ b` is element-wise division, the divide
companion of `⊙`. It binds at the multiplicative tier with `*` and the other
glyph products, left-associative.

**Extrema (`⤒` / `⤓`):** `a ⤒ b` is the maximum and `a ⤓ b` the minimum of
two values. They are pure arithmetic operators at the additive tier with `+`
and `-`, left-associative: `a ⤒ b ⤓ c` is `(a ⤒ b) ⤓ c`.

**Exact-output transfer (`⇇`):** `sink ⇇ payload` invokes a callable sink value — one argument, `vacuum` result — once per payload. The operator performs no formatting, adds no separators or terminator, selects no channel, and runs no conversions: the bound value owns destination and behavior, and the compiler holds no console knowledge. A chain `sink ⇇ a ⇇ b` evaluates the sink expression once, each payload once left-to-right, and invokes the sink once per payload left-to-right; the chain result is `vacuum`. `⇇` binds above assignment and below ternary, so postfix calls, conversions, and string-constructor applications finish before transfer; formatting is explicit on the right (`output ⇇ "§ §
"(a, b)`). Combined with selective value imports it replaces compiler-owned output statements with ordinary typed values.

**Conversion-directed assignment (`↤` / conversio-assign):** `place ↤ value`
evaluates the right side, converts it to the statically known type of the left
place through the existing `↦` route, then assigns. It binds at the same
precedence as `←` and is right-associative; the `⊥` default (`inline_default`)
is **legal only on `↤`** — a `⊥` after ordinary `←` is rejected, and in a
right-associated `↤` chain the default attaches to the nearest `↤`. The
operator is preserved verbatim through syntax and emission; it is never
rewritten to `←` or `↦`. Typed `ثابت`/`متغير` initializers accept `↤`
(convert to the written type, then initialize); `ثابت _`, `ليكن`, and untyped
destructuring have no concrete destination and are rejected.

`هو` and `ليس هو` inspect an existing value; they never convert it. Core type
spellings on the right perform runtime variant/type tests, while `لاشيء`,
`صواب`, `خطأ`, and ordinary value expressions use the value-test path. Radix
currently recognizes type targets through a fixed core-type vocabulary. Extending
that recognition to arbitrary declared types is a separate language decision.
Use `≡` / `≠` (or `≢`) for structural value equality, `≅` / `≇` for promoted exact equality (same value after numeric widths join), `≈` / `≉` for fuzzy equality (tolerance match with Python-isclose defaults: rel_tol 1e-09, abs_tol 0.0), and `↦` for runtime conversion.

Retired predicate keywords are not prefix unary syntax. Use `expr هو صواب`,
`expr هو خطأ`, `expr هو لاشيء`, `expr ليس هو لاشيء`, `expr ≺ 0`, or
`expr ≻ 0`.

The legacy ASCII spellings `<` and `>` are not productions of this grammar — both remain generic delimiters — though the shipped parser still accepts them as comparisons during the glyph migration; prefer the canonical `≺` and `≻`.

Ordering comparisons (`≺`, `≻`, `≤`, `≥`) between two `textus` values compare
the whole strings in Unicode code-point order. They do not use locale
collation.

**Static type ascription (`∷` / verte):**

The `∷` glyph (U+2237, "proportion") explicitly ascribes a target type to an expression. Use it when the source expression already exists and the compiler needs a static target shape:

- Primitive/alias → cast (no runtime effect): `data ∷ textus` → TypeScript: `(data as string)`
- Built-in collection → target-shaped collection value: `[1, 2, 3] ∷ lista<numerus>`
- Variant expression → enum/interface target ascription: `أنشئ Click { x = 10 } ∷ Event`

Prefer typed construction for ordinary `صنف` values and `vacua` for ordinary empty collection values:

```fab
fixum _ point ← Point { x = 10 }
fixum lista<numerus> xs ← vacua
```

Only the `∷` glyph is accepted as the postfix static type-ascription operator. The Latin forms `qua`, `innatum`, and `novum` were aliases and have been removed (see verte-alias-clean-break).

**Runtime conversion (`↦` / conversio):**

The `↦` glyph (U+21A6, "rightwards arrow from bar") is the runtime value conversion operator. Unlike `∷` (compile-time cast), this performs actual parsing/conversion that can fail:

- `"22" ↦ numerus` → Rust: `"22".parse::<i64>().unwrap()`
- `"bad" ↦ numerus ⊥ 0` → Rust: `"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ textus` → Rust: `42.to_string()`
- `n ↦ ascii<N, Hex|Bin|Oct>` — shipped; fixed-width lowercase digits, zero-padded to `N`, with overflow and negative sources rejected.
- `n ↦ ascii<_, Hex|Bin|Oct>` — shipped for const-foldable numerus sources; the hole is solved to the source digit count. Runtime sources leave the hole unsolved and require explicit `N`.

The second type argument of a `↦` target is the convert-hint slot. `Hex` / `Bin` / `Oct` / `Be` / `Le` / `Bits` are convert hints in that slot, not keywords and not new `baseType` productions. For ascii output, `Hex` / `Bin` / `Oct` select the lowercase fixed-width digit pack; the hint is not part of type identity. Target support is not a grammar production (see Target Support).

- `"ff" ↦ numerus<i32, Hex>` — shipped; text parse at radix 16 (`Bin` = 2, `Oct` = 8). Hex/Bin/Oct text parse is unchanged by endian hints.
- `octeti[lo‥hi] ↦ numerus<W, Be>` / `… ↦ numerus<W, Le>` — endian unpack of an exact-width window (`W` is `i16` / `i32` / `i64` / `u16` / `u32` / `u64`; window length 2 / 4 / 8). Shipped on rust, the MIR runner, Go, and TypeScript. TypeScript `i64`/`u64` stay fail-closed (JS number is not exact). English `int<W, Be>` is the same form. `octeti` itself has no endian; `bytes ↦ numerus<u32>` without `Be`/`Le` stays rejected. A short window fails (no pad).
- `octeti[lo‥hi] ↦ fractus<f32, Be|Le>` / `… ↦ fractus<f64, Be|Le>` — shipped alongside the integer rows (float endian unpack of an exact-width window, 4 / 8 bytes; same fail rules: exact window required, a short window fails, `Be`/`Le` mandatory).
- `n ↦ numerus<u32, Bits>` / `n ↦ numerus<u64, Bits>` / `n ↦ fractus<f32, Bits>` / `n ↦ fractus<f64, Bits>` / `n ↦ fractus<f16, Bits>` — shipped; the `Bits` hint reinterprets between exact-width integer/float pairs (u32↔f32, u64↔f64, u16↔f16, u16↔bf16) bit-identically. It is reinterpretation, not value conversion; wrong-pair rows reject with the structured issue, and `Bits` is never a base or an ascii format hint. `Bits` is a convert-slot hint in the same Hex slot, not a keyword and not a `baseType` production.
- `n ↦ octeti<N, Be>` / `… ↦ octeti<N, Le>` — proposed (not shipped); write convert after `octeti<N>` (`N` ∈ {2, 4, 8}). `Be`/`Le` stay Hex-slot hints, not a second capacity.

Explicit integer narrowing is magnitude-checked on every backend:
`n ↦ numerus<u8>` converts a value that fits unchanged, and a value out of the
target's range fails — it never wraps and never relabels. The failure takes the
error channel, or the `⊥` default when one is written. Use `modulus<W>` for
wrapping arithmetic.

**Default channel (`⊥`):** `⊥` (U+22A5 UP TACK) supplies a value when a
conversion fails: `ثابت numerus n ← "abc" ↦ numerus ⊥ 0`. It is written
immediately after the conversio target (`↦ T ⊥ default`) or after the value of
a `↤` assignment.

- `⊥` catches only the `⇥` error channel. It never catches `انهر` or traps
  (for example integer overflow).
- The default is evaluated only on failure.
- The default must type-check as the success type `T`.
- One expression either propagates (`⇥ E`) or defaults (`⊥ v`), never both;
  `⊥ v ⇥ …` is rejected.
- `⊥` binds looser than `↦ T`: `x ↦ numerus ⊥ 0` is `(x ↦ numerus) ⊥ 0`. The
  unparenthesized default is a unary-precedence expression; parenthesize
  arithmetic, coalescing, ternary, or assignment defaults.
- Today `⊥` is legal only on a conversion (`↦ T`, `↤`); after any other
  expression it is rejected.
- `⊥` is an operator between a failable expression and a value. It is not the
  type-theory "never" type (that is `numquam`).
- The glyph is the same in every locale. The look-alike `⟂` (U+27C2) is
  rejected with a "did you mean `⊥`?" hint.

`⇥` only ever names an error type. The retired inline recovery `↦ T ⇥ value`
(and `↤ … ⇥ value`) is rejected with a migration diagnostic pointing at `⊥`.

Using `عوض` as a conversio default is rejected with a migration diagnostic. `عوض` is local nullable elimination only (`x عوض y`, parameter defaults) — not logical `أو`. A parenthesized conversio result may still combine with `عوض` as ordinary defaulting.

### Call and Member Access

A `call_expr` may continue with the zero-argument `transpose_suffix` `ᵀ`
(U+1D40) after its ordinary primary/member/index chain. This is postfix
source sugar, not a method spelling: semantic analysis applies the rank-2-only
law and lowers the admitted form through the existing `transpone`/
`Transpose` plan entry. `a · bᵀ ∇ [x]` is settled as `(a · bᵀ) ∇ [x]`.

### String And Template Literals

Faber uses **delimiter semantics**: each quote form means a different source shape.
They are not interchangeable synonyms.

| Form | Type | Role |
| --- | --- | --- |
| `'...'` | `ascii` | fixed machine tokens; no `§`; no `(...)` |
| `"..."` | `textus` | short Unicode line strings; `(...)` renders |
| `«...»` | `textus` | block/multiline Unicode; `(...)` renders |
| `` `...` `` | `forma` | captured templates; `(...)` captures |
| `{ ... }` | `json` | compile-time object-rooted JSON document (`:` inside) |
| `\|...\|` | `octeti` | compile-time hex bytes |
| `"..." ↦ regex` | `regex` | compiled pattern from text conversion |
| `[ ... ]` | `lista<T>` | Faber list (not JSON array, not bytes) |

`§` (U+00A7) is a template hole in Unicode forms (`"`, `«`, `` ` ``).
§{label} names a hole with an identifier label; the label is unique within
its template and may use a keyword spelling under the contextual law. Named
holes are not available in `ascii` literals, where `§` remains forbidden.

**Rendered templates** (`textus`): `"..."(...)` and `«...»(...)` lower to
`حرر("...", args...)`.

**Captured templates** (`forma`): `` `...`(args) `` captures template text and
parameters without rendering. Safe for bound SQL/URL payloads; do not use
`«...»(...)` for that job.

Block `textus` uses guillemets `«...»`. The heavy quotation-mark
pair is retired (too visually close to `"` in many fonts).

Implementation status (2026-06-30):

- Shipped: `"..."`, `«...»` block `textus`, `'...'` → `ascii`, `` `...` `` → `forma`, `|...|` → `octeti`, `{ ... }` → `json`, and text/ascii `↦ regex`.
- Pending factory delivery: slash-delimited `/.../` regex literals.

Inline block example:

```fab
fixum _ tag ← «inline»
```

Multiline block example (newline after opening `«`):

```fab
fixum _ blob ← «
    select id, email
    from accounts
»
```

Captured template example:

```fab
fixum _ q ← `select * from accounts where id = §`(accountId)
```

Octeti hex literal example:

```fab
fixum _ sig ← |de ad be ef|
fixum _ hello ← |48 65 6c 6c 6f|
```

### Format-Template Application

String literal call syntax is the canonical source form for format-template application:

```fab
"§{greet} world"(greet: "salve")
"status: § (§)"(sample_status(), "ok")
"status: §1 (§0)"("ok", sample_status())
```

The position law counts named and anonymous holes together in order of
appearance: "§{greet} §" = `[greet: 0, anonymous: 1]`. Named labels are
erased at lowering, so "§{greet} world"(greet: "salve") lowers identically
to the positional form `"§ world"("salve")` and its canonical
`حرر("§ world", "salve")` form.

This lowers to the compiler's `حرر("...", args...)` form. Use the string-template form in ordinary source; reserve `حرر(...)` for explicit desugaring examples and compiler-facing documentation.

For `textus`, bracket indexing is Unicode-scalar based:

```fab
# Produces "§".
"Salve, §!"[7]
# Produces "hello".
"hello world"[0‥5]
# Produces "hello world".
"hello world"[0 usque 10]
# Produces "ace".
"abcdef"[0‥6 per 2]
```

Text slices accept the full range form, including `كل`.

For `lista<T>`, bracket indexing is a single-element access. The index must be
one integer; range slices are not accepted (use `sectio(start, end)` for a
copied range):

```fab
# Element at position i.
xs[i]
# Write element at position i.
xs[i] ← v
```

Lista bracket access is **plain**, not nullable: it returns the bare element
`T` and traps on out-of-bounds. This differs from `tensor`, whose bracket read
is `accipe` sugar and returns `T ∪ لاشيء`. For nullable list access, use
`xs.accipe(i) → T ∪ لاشيء` with `عوض`.

For `tensor<T, Figura>`, bracket indexing is sugar over the tensor intrinsic
surface:

```fab
# vector.accipe([id])
vector[id]
# vector.ponde([id], v)
vector[id] ← v
# grid.accipe([r, c])
grid[[r, c]]
# grid.ponde([r, c], v)
grid[[r, c]] ← v
```

Reads return `T ∪ لاشيء`, matching `accipe`; use `عوض` or another ordinary
option-handling form before arithmetic. Rank-1 tensors accept scalar integer
indices that fit the tensor `i64` runtime boundary (`u64` is rejected).
Rank-N tensors use a list-shaped index expression such as `[[r, c]]` or a
bound `lista<integer>` value. `grid[r, c]` is not syntax; `memberSuffix` still
contains exactly one `expression` between brackets.

For `octeti`, bracket indexing is a byte or an exclusive window:

```fab
# One byte → numerus<u8>. O(1). Traps on out-of-bounds.
buf[i]
# Exclusive window → octeti. Fully in bounds or fail (no short slice, no pad).
buf[lo‥hi]
```

The index must be an integer or a range. A compile-time-provable out-of-range
index on an octeti literal (`|عن اتصل be ef|[0‥5]`) is a structured reject.
Runtime out-of-bounds traps — the same trapping model as lista bracket access,
not textus short-slice. Lista `[lo‥hi]` stays rejected.

`octeti` is the endian host. Parse byte windows on the buffer
(`buf[lo‥hi] ↦ numerus<W, Be|Le>`). Cross to a list once, for element work,
via `octeti ↦ lista<numerus<u8>>` (representation change only; other element
types fail closed). The reverse `lista<numerus<u8>> ↦ octeti` is live. Do not
detour through `valor`. Lists stay for element work, not endian windows.

### Primary Expressions

Non-finite literals are contextual floating-point values: `∞` is positive
infinity and `nan` is NaN. The named form is `nan` in the
Latin (`la`) pack and `nan` in every other shipped pack; it is claimed only in
the literal slot, so a following `(` keeps an ordinary `nan(...)` call. Their
width follows a surrounding `f32` or `f64` context when present; bare `fractus`
remains unsized, and neither form has a width suffix. A leading `-` is supplied
by `unary_expr`, so `-∞` is unary negation of `∞`, not a separate token. A
`numerus` context rejects both forms (fail-closed); neither maps to an integer.

**Capture boundary (`فخ`):** `فخ { … }` (en `trap`) is an expression
that runs its block and reifies the error channel into a value. The block's
trailing expression is the success value; the result type is the union of the
success type and every error type that can escape the body (failable calls
and `ارم` payloads), so a failure inside the block becomes a value instead of
propagating. When the success and error types coincide the union cannot tell
them apart, and the form is rejected. `فخ` claims its spelling only in expression-primary position
directly followed by `{`, so `فخ(…)` calls and bare identifier uses keep
their ordinary meaning. No `التقط` clause, `طالما` tail, or early-success form
attaches to it — those belong to `افعل`.

`vacua` is a contextual empty-collection marker (identifier form, not a reserved keyword).
Use it with an explicit collection type: `ثابت lista<numerus> xs ← vacua` or `ثابت tensor<fractus<f32>, []> t ← vacua`.


`STRING` includes short strings delimited by `"` and block strings delimited by
`«` and `»`. `'...'` (`ascii`) and backtick
`` `...` `` (`forma`) are separate literal forms (see String And Template
Literals above).

A bare `{ ... }` now produces an object-rooted JSON document of type `json`:
`{ "name": "Alice", "age": 30, "active": true }`. Keys are quoted JSON strings
separated by `:`; values are JSON constants only. Duplicate keys are an error
(second occurrence). Ascribing to `tabula<K,V>` lowers a real constant map.
Use `↦ valor` for explicit widening to the broad dynamic carrier. Genus/variant
construction `Type { field = expr }` uses the Faber `=` grammar unchanged.
Construction literals do not spread: `انشر` is not a field initializer
(`Genus { انشر other }` is rejected). `انشر` stays for list literals and
call arguments. Copy-with-changes is planned as `Genus { … } من source`.

- Ratio construction uses `ratioType '{' fieldInit (',' fieldInit)* '}'` through `typedConstructor`; every field initializer is named, and the resulting fields remain accessible only by label.

### Special Expressions


`أول_مطابقة(source, حيث binder { predicate })` is the dedicated first-match
selection expression over a statically bounded source: the predicate is
evaluated for every candidate lane (total evaluation, no early exit), the
first live match is selected, and a no-match or empty source yields `لاشيء`
(the result type is `T ∪ لاشيء`). The `حيث` predicate tail is owned by this
head and never shares the reduce/scan `ثابت`/`متغير` binder tail.
`أول_مطابقة` claims only the expression-head position immediately followed
by `(`; elsewhere the spelling stays an ordinary identifier. An optional
`عند` coordinate clause binds per-axis indices as in `كرر من`.

`حرر` and `اقرأ`/`سطرا` are builtin claims that resolve to a user binding
when the surface spelling is bound in scope (parameter, local, function, or any
in-scope definition); otherwise they are the builtin. The same binding-wins rule
applies to `حرر`'s paren-claimed form and to the `vacua` empty-collection
marker: builtin claims are defaults, not reservations.

`أنشئ` variant construction accepts a qualified variant path
(`أنشئ pkg.Bonum { … }`), so an imported union's variants construct through
the import alias, and the `∷` cast is a full type annotation
(`∷ pkg.Exitus`) exactly as the general postfix ascription (uvf-u3).

`∷` remains the general postfix ascription in `cast`. Rendered text templates
(`STRING '(' argumentList ')'`) and captured `forma` templates
(`BACKTICK_STRING '(' argumentList ')'`) use the ordinary call suffix. Regex
construction uses the ordinary conversio grammar: `(STRING | ASCII_STRING) '↦'
'regex'`.

Slash-delimited regex literals are not active grammar yet. `/` lexes as the
division operator, while `//` and `/* ... */` are rejected as invalid comments.
Use `"..." ↦ regex` for compiled regex values.

---

## Patterns


---

## Diagnostics


The scribe family (`اعرض`/`شاهد`/`نبه`/`اكتب` — en `print`/`debug`/`warn`/`write`)
claims the statement-initial position only when **not** immediately followed by
`(`. `اعرض expr` is the output statement; a statement-initial `اعرض(...)` is an
expression statement whose callee is the identifier `اعرض` — a user function
call, never the intrinsic.

- `اعرض` = neutral diagnostic note, `شاهد` = debug/inspect, `نبه` = warn
- `اكتب` is a diagnostic channel spelling; use current stdlib methods for real output

### Comments

Faber accepts **line comments only**: `#` through end of line. The `#` must be the
first non-whitespace token on the logical line (optional leading ASCII spaces or
tabs only — other Unicode space separators are not skipped by the lexer).
A `#` that follows any other token on the same line is a **lex error** with the
message `# comments must start a line; move this comment above the code`.

Valid line-start comments attach forward as `leading_trivia` on the following
statement or declaration (see comment-preservation). `#` inside string literals,
`ascii` literals, `forma` templates, and other delimited literals is **not** a
comment.

---

## Entry Points


- `بداية` = sync entry, `استهلال` = async entry.
- `وسائط` binds parsed command-line arguments; `مخرج` supplies the process exit expression. Their order is fixed by `entryHeader`.

---

## Testing

`اختبر` modifiers include `توقع_الفشل` (en `expect_failure`): the case passes only
when its body escapes through the error channel, and a case that completes
cleanly fails (strict expected-failure). The other modifiers are `أهمل`,
`مستقبلي`, `فقط`, `حصري`, `وسم`, `زمني`, `قس`, `معاد`, and
`هش`.

---

## CLI Framework

CLI metadata uses the ordinary reachable `annotation* statementCore` grammar.
The promoted `cli`, `imperium`, `optio`, and `operandus` families validate their
own named-field schemas after parsing.

Faber supports building CLI applications with automatic argument parsing and help generation.

### CLI Entry Point

```fab
@ cli "faber"
@ optio verbose longum "verbose" typus bivalens
incipit argumenta args {
    # CLI framework automatically parses arguments
}
```

### CLI Options and Arguments

```fab
@ imperium "deploy"
@ optio target brevis "t" longum "target" typus textus descriptio "Deployment target"
@ optio verbose brevis "v" longum "verbose" typus bivalens descriptio "Enable verbose output"
@ operandus textus file descriptio "File to deploy"
functio deploy() argumenta args {
    # Arguments automatically parsed and passed
}
```

---

## Capability Calls

Expression-form `اتصل` is the only supported `اتصل` surface. Legacy typed
`اتصل "route" (args) → T { }` and statement-level stream blocks
`اتصل 'route' { meus/tuus … }` are rejected at parse time.

The active `adExpr` production is defined under **Primary Expressions**. Its
ordinary postfix `conversio` materializes the resulting conversation handle.

- Route: `ASCII_STRING` (`'فقط:اقرأ'`), not double-quoted `STRING`.
- Opener: optional single `expression` → Request `data` as `valor`.
- **Expression `اتصل`**: blockless; evaluates to a `sermo` conversation handle.
  Use postfix `↦ T` (materialization), assign to `sermo`, or open live directional
  views: `s.meus<T>()` (outbound `da` / `fini`) and `s.tuus<T>()` (inbound
  `accipe` / `cursor` / `exhauri` / `fini`). Iterate inbound content frames with
  `s.tuus<T>().cursor()`, not direct `كرر من s.tuus<T>()`.
- **Removed (parse error):** legacy typed `اتصل "route"` and block `meus`/`tuus` arms.
- Types: compiler-owned `scrinium`, `status`; opaque `sermo` conversation handle.
- English reader spellings: `sermo` is `channel`, `scrinium` is `frame`, and
  the views `meus<T>` / `tuus<T>` are `send<T>` / `recv<T>` (`s.send<T>()`,
  `s.recv<T>()`). The Latin spellings are unchanged.
- `sermo ↦ T` materializes inbound frames into one value of type `T` using
  the type-directed collector for `T`.

See [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md).

**Concurrency is conversations.** Concurrent work is an `اتصل` conversation with
a route. There is no separate spawn, thread, or lock primitive family.
Handlers that share nothing and exchange only frames are free of data races by
construction.

Every `اتصل` pays the conversation cost. It goes through the router with frames,
even when both ends are local; there is no hidden fast path. The light path is
an ordinary function call, and a swappable light path is a contract passed as a
parameter.

`اتصل` is the effect boundary. Effects reach the outside world through `اتصل`
conversations, which stay portable across backends.

`@ اتصل` on a function is the compiler-owned serving half of `اتصل`: it lets
Faber code answer a route. `@ اتصل 'prefix:name'` (en `@ call`) on a top-level,
non-generic, bodied `دالة` serves that route.

- Routes are exact: `prefix:name` or `prefix/name`. Pattern routes are deferred.
- The handler takes zero or one parameter; the one parameter is the opener
  value of the calling `اتصل`.
- A handler serves one route. Reserved prefixes (such as `runtime:`) and
  builtin routes cannot be served.
- Routes form one program-wide static table built from every module in the
  program, including imported libraries. Two definitions of the same route are
  a compile error.
- Parsing, checking, and the route table exist today; backend serving is in
  progress.

Web, HTTP, and framework routing stay libraries (see Annotations).

---

## Collection Operations

The former `نطاق` collection pipeline DSL is retired. Collection filtering,
slicing, and aggregation are expressed through ordinary
`textus`/`lista`/`tabula`/`copia` methods and closures instead of a
grammar-level query expression. `textus`, `numerus`, `fractus`, `lista<T>`,
`tabula<K,V>`, and `copia<T>` are compiler-owned core types; their method
surfaces are not Norma declarations.

`prima` and `ultima` are ordinary method names, not transform keywords. `حيث` is
the owned predicate-tail introducer of the `أول_مطابقة` first-match expression
(see Special Expressions), not collection syntax.

`من` is used for iteration (`كرر من items ثابت x`) and imports (`استورد من "path"`).

### Iteration coordinates (`عند`)

The optional `عند` coordinate clause binds per-axis indices for an `كرر من`
loop: `كرر من grid عند [r, c] ثابت cell { … }`. The en reader spelling is
"at" — `كرر من grid عند [r, c]` reads as iterating `grid` at coordinates
`[r, c]`.

- **First bound name = outer axis.** The first identifier in the bracket group
  walks the first (outermost) axis; later names walk successively inner axes.
- **Bracket-group convention.** The coordinate group follows the tensor
  bracket-index convention `grid[[r, c]]`: one bracketed group, comma-separated
  coordinate names, in axis order.
- **Arity == rank.** The number of coordinate names must equal the tensor rank.
  Fewer or more names is a structured reject (arity mismatch).
- **`عند` requires `من`.** The coordinate clause is only valid on `كرر من`
  (element iteration); `كرر نطاق` range loops and `كرر عن` reject it.
- The coordinate names are immutable index bindings scoped to the loop body,
  distinct from the element binder that follows the clause.

---

## Fac Block


- `افعل { ... }` is the explicit `do` block and executes its body once.
- `افعل { ... } طالما condition` is the post-test loop form; postfix `طالما` attaches only to `افعل`, not arbitrary preceding blocks.
- `التقط` is an attachment shared by several structured forms, not a semantic mode owned by `افعل`. A plain `افعل` is often used when an otherwise unattached block needs a local handler: `افعل { ... } التقط err { ... }`.

---

## Target Support

Target support is **not** part of the grammar — this file defines only the
language. For which grammar each compilation target lowers, and the runtime
policy around it, see:

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — generated grammar×target lowerability matrix (the official rows).
- [`docs/design/target-capability-matrix.md`](docs/design/target-capability-matrix.md) — runtime/contract policy (erase/warn/defer), pipeline routing, per-target contracts.

**Conditional compilation is package-granular.** A package's `faber.toml`
declares its target or targets (`[build] target = "ts"`, or
`targets = ["rust", "ts"]`). There are no conditionals inside a package: no
`#if`, no in-body `cfg`, and no per-file target selection.

A multi-target package stays target-neutral. Its per-target parts live in the
per-target manifest sections (`[target.ts]`). Code that needs a genuinely
different implementation per target is split into separate packages, and the
consumer chooses one.

Feature flags (`@ feature`, `[features]`) belong to the visibility model and
are unchanged. `@ nondum` stays the marker for "not implemented on this target
yet".

There is no `unsafe`. Faber code is always checked. Code that must step
outside the checker is foreign code, written outside Faber.

---

## Critical Syntax Rules

1. **Type-first parameters**: `دالة f(numerus x)` NOT `دالة f(x: numerus)`
2. **Type-first declarations**: `ثابت textus name` NOT `ثابت name: textus`
3. **Iteration loops**: `كرر من/عن collection ثابت/متغير item { }` or `كرر نطاق range ثابت/متغير item { }` (verb-first, source, then binding)
4. **Parentheses around conditions are valid but not idiomatic**: prefer `إذا x ≻ 0 { }` or `إذا flag هو صواب { }` over `إذا (x ≻ 0) { }`
5. **Scribe-family keywords claim statement-initial position only when not followed by `(`** — `اعرض x` is the output statement; a statement-initial `اعرض(x)` is a call to the identifier `اعرض`
