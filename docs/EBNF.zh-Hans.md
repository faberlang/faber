# Faber Language Specification

This file is generated from `docs/grammar/source.fg` and `docs/grammar/glossary.zh-Hans.toml`;
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
statement_core ::= importa_decl | binding_decl | functio_decl | genus_decl | implendum_decl | typus_decl | ordo_decl | discretio_decl | schema_decl | static_decl | si_stmt | dum_stmt | itera_stmt | elige_stmt | discerne_stmt | custodi_stmt | fac_stmt | redde_stmt | reddet_stmt | tacebit_stmt | cede_stmt | rumpe_stmt | perge_stmt | tacet_stmt | iace_stmt | adfirma_stmt | requirit_stmt | reice_stmt | nota_stmt | incipit_stmt | incipiet_stmt | ex_stmt | probandum_decl | proba_stmt | block_stmt | inc_dec_stmt | expr_stmt
# [006] binding_decl
binding_decl ::= fixum_decl | sit_decl | array_destruct | object_destruct | figendum_decl
# [007] expr_stmt
expr_stmt ::= expression
# [008] block_stmt
block_stmt ::= '{' statement* '}'
# [009] static_decl
static_decl ::= '静态' type_annotation IDENTIFIER '=' expression
# [010] fixum_decl
fixum_decl ::= ('常量' | '变量') type_annotation IDENTIFIER (('←' expression) | ('↤' assignment inline_default?) | ('↢' expression))?
# [011] figendum_decl
figendum_decl ::= ('等定' | '等变') type_annotation IDENTIFIER '←' expression
# [012] sit_decl
sit_decl ::= '设' IDENTIFIER (('←' | '↢') expression)?
# [013] array_destruct
array_destruct ::= ('常量' | '变量') array_pattern '←' expression
# [014] object_destruct
object_destruct ::= ('常量' | '变量') object_pattern '←' expression
# [015] functio_decl
functio_decl ::= '函数' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [016] param_list
param_list ::= (parameter (',' parameter)*)?
# [017] generic_params
generic_params ::= '<' generic_param (',' generic_param)* '>'
# [018] generic_param
generic_param ::= IDENTIFIER generic_type_default? | '维度' IDENTIFIER generic_size_default?
# [019] generic_type_default
generic_type_default ::= '=' type_annotation
# [020] generic_size_default
generic_size_default ::= '=' NATURAL
# [021] call_type_args
call_type_args ::= '<' type_annotation (',' type_annotation)* '>'
# [022] parameter
parameter ::= '其余'? type_annotation IDENTIFIER '可选'? ('作为' IDENTIFIER)? ('兜底' expression)?
# [023] func_modifier
func_modifier ::= '参数' IDENTIFIER | '勘误' IDENTIFIER | '退出' (IDENTIFIER | NUMBER) | '不变' | '可抛' | '可选项' IDENTIFIER
# [024] callable_posture
callable_posture ::= '异步' | '流' | '异流'
# [025] return_clause
return_clause ::= '→' type_annotation
# [026] alternate_exit_clause
alternate_exit_clause ::= '⇥' type_annotation
# [027] ergo_joint
ergo_joint ::= '则'
# [028] clausura_joint
clausura_joint ::= '∴'
# [029] clausura_expr
clausura_expr ::= compact_clausura_expr | clausura_legacy_expr
# [030] compact_clausura_expr
compact_clausura_expr ::= clausura_signature clausura_joint (expression | fac_block)
# [031] clausura_signature
clausura_signature ::= (clausura_param | '(' clausura_params? ')') closure_modifier? return_clause? alternate_exit_clause?
# [032] closure_modifier
closure_modifier ::= '自由' | '内核'
# [033] fac_block
fac_block ::= '执行' block_stmt cape_clause?
# [034] clausura_legacy_expr
clausura_legacy_expr ::= '闭包' clausura_params? closure_modifier? ('→' type_annotation)? (':' expression | block_stmt)
# [035] clausura_params
clausura_params ::= clausura_param (',' clausura_param)*
# [036] clausura_param
clausura_param ::= type_annotation IDENTIFIER
# [037] genus_decl
genus_decl ::= '类' IDENTIFIER generic_params? ('实现' IDENTIFIER ((',' | '∩') IDENTIFIER)*)? '{' genus_member* '}'
# [038] genus_member
genus_member ::= annotation* (field_decl | functio_method_decl)
# [039] field_decl
field_decl ::= '静态'? type_annotation IDENTIFIER '可选'? ('=' expression)?
# [040] functio_method_decl
functio_method_decl ::= '函数' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [041] annotation
annotation ::= nucleum_annotation | radix_annotation | ad_annotation | braced_annotation | annotation_sugar
# [042] annotation_name
annotation_name ::= ANNOTATION_NAME
# [043] braced_annotation
braced_annotation ::= '@' annotation_name '{' annotation_field_list? '}'
# [044] annotation_field_list
annotation_field_list ::= annotation_field (',' annotation_field)*
# [045] annotation_field
annotation_field ::= ANNOTATION_FIELD_NAME '=' (expression | type_annotation)
# [046] annotation_sugar
annotation_sugar ::= '@' annotation_name NON_NEWLINE_TOKEN* NEWLINE
# [047] nucleum_annotation
nucleum_annotation ::= nucleum_sugar | nucleum_braced
# [048] nucleum_sugar
nucleum_sugar ::= '@' '内核' nucleum_modifier? NEWLINE
# [049] nucleum_braced
nucleum_braced ::= '@' '内核' '{' nucleum_field_list? '}'
# [050] nucleum_modifier
nucleum_modifier ::= '片段'
# [051] nucleum_field_list
nucleum_field_list ::= nucleum_field (',' nucleum_field)*
# [052] nucleum_field
nucleum_field ::= '片段' '=' ('真' | '假')
# [053] radix_annotation
radix_annotation ::= '@' 'radix' radix_directive NEWLINE
# [054] radix_directive
radix_directive ::= '车道' STRING | 'backward' STRING | '类型' IDENTIFIER '传入' type_annotation+
# [055] ad_annotation
ad_annotation ::= '@' '调用' ASCII_STRING NEWLINE
# [056] implendum_decl
implendum_decl ::= '契约' IDENTIFIER generic_params? '{' implendum_method_decl* '}'
# [057] implendum_method_decl
implendum_method_decl ::= annotation* '函数' IDENTIFIER '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# [058] typus_decl
typus_decl ::= '类型' IDENTIFIER generic_params? '=' type_annotation
# [059] ordo_decl
ordo_decl ::= '枚举' IDENTIFIER '{' enum_member (',' enum_member)* '}'
# [060] enum_member
enum_member ::= IDENTIFIER ('=' ('-'? NUMBER | STRING))?
# [061] discretio_decl
discretio_decl ::= '判别' IDENTIFIER generic_params? '{' union_member* variant (',' variant)* '}'
# [062] union_member
union_member ::= annotation* field_decl
# [063] variant
variant ::= IDENTIFIER ('{' variant_fields '}')?
# [064] variant_fields
variant_fields ::= (type_annotation IDENTIFIER)*
# [065] schema_decl
schema_decl ::= '架构' IDENTIFIER '{' schema_column* '}'
# [066] schema_column
schema_column ::= '列' type_annotation IDENTIFIER (':' IDENTIFIER)?
# [067] importa_decl
importa_decl ::= importa_record | importa_sugar
# [068] importa_record
importa_record ::= '导入' '{' import_field_list? '}'
# [069] import_field_list
import_field_list ::= import_field (',' import_field)*
# [070] import_field
import_field ::= ex_field | visibilitas_field | nomen_field | ut_field | omnia_field
# [071] ex_field
ex_field ::= '取自' '=' STRING
# [072] visibilitas_field
visibilitas_field ::= 'visibilitas' '=' publica
# [073] nomen_field
nomen_field ::= '名称' '=' IDENTIFIER
# [074] ut_field
ut_field ::= '作为' '=' IDENTIFIER
# [075] omnia_field
omnia_field ::= '全部' '=' IDENTIFIER
# [076] importa_sugar
importa_sugar ::= '导入' '取自' STRING publica? (named_import | wildcard_import | selective_import)?
# [077] publica
publica ::= '公开'
# [078] named_import
named_import ::= IDENTIFIER ('作为' IDENTIFIER)?
# [079] wildcard_import
wildcard_import ::= '*' '作为' IDENTIFIER
# [080] selective_import
selective_import ::= '常量' import_value_binding (',' import_value_binding)*
# [081] import_value_binding
import_value_binding ::= IDENTIFIER ('作为' IDENTIFIER)?
# [082] type_annotation
type_annotation ::= intersection_type ('∪' intersection_type)*
# [083] intersection_type
intersection_type ::= owned_type ('∩' owned_type)*
# [084] owned_type
owned_type ::= ('借自' | '传入' | '拥有' | '拷贝')? base_type
# [085] base_type
base_type ::= hole_type | function_type | width_type_sugar | ratio_type | qualified_type type_arguments? | '(' type_annotation ')'
# [086] ratio_type
ratio_type ::= 'ratio' '<' labeled_type_argument (',' labeled_type_argument)* '>'
# [087] hole_type
hole_type ::= '_' | '∪'
# [088] qualified_type
qualified_type ::= IDENTIFIER ('.' IDENTIFIER)*
# [089] type_arguments
type_arguments ::= '<' type_argument (',' type_argument)* '>'
# [090] type_argument
type_argument ::= labeled_type_argument | type_annotation | NATURAL | '[' figura_list? ']'
# [091] labeled_type_argument
labeled_type_argument ::= IDENTIFIER ':' type_annotation
# [092] width_type_sugar
width_type_sugar ::= WIDTH_MARKER | LISTA_WIDTH_SUGAR | (TENSOR_WIDTH_SUGAR | SPARSA_WIDTH_SUGAR | VECTOR_WIDTH_SUGAR) shape_suffix? | MATRIX_WIDTH_SUGAR shape_suffix
# [093] shape_suffix
shape_suffix ::= '[' figura_list? ']'
# [094] figura
figura ::= '_' | NATURAL | IDENTIFIER | '[' figura_list? ']'
# [095] figura_list
figura_list ::= figura (',' figura)*
# [096] function_type
function_type ::= '(' type_list? ')' '→' type_annotation alternate_exit_clause?
# [097] type_list
type_list ::= type_annotation (',' type_annotation)*
# [098] si_stmt
si_stmt ::= '如果' expression arm ('否则如果' si_stmt | secus_clause)?
# [099] secus_clause
secus_clause ::= '否则' else_arm
# [100] arm
arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [101] else_arm
else_arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [102] dum_stmt
dum_stmt ::= '当' expression (block_stmt | ergo_joint statement) cape_clause?
# [103] itera_stmt
itera_stmt ::= '遍历' ('取自' expression (',' expression)* | '借自' expression | '范围' expression (',' expression)*) apud_clause? ('常量' | '变量') itera_binding (block_stmt | ergo_joint statement) cape_clause?
# [104] itera_binding
itera_binding ::= array_pattern | object_pattern | IDENTIFIER (',' IDENTIFIER)*
# [105] apud_clause
apud_clause ::= '于' '[' IDENTIFIER (',' IDENTIFIER)* ']'
# [106] elige_stmt
elige_stmt ::= '选择' expression '{' casu_elige_clause* ceterum_clause? '}' cape_clause?
# [107] casu_elige_clause
casu_elige_clause ::= '情况' expression (block_stmt | ergo_joint statement)
# [108] ceterum_clause
ceterum_clause ::= '默认' (block_stmt | ergo_joint statement)
# [109] discerne_stmt
discerne_stmt ::= '匹配' '全部'? discriminants '{' casu_variant_clause* ceterum_clause? '}'
# [110] discriminants
discriminants ::= expression (',' expression)*
# [111] casu_variant_clause
casu_variant_clause ::= '情况' patterns (block_stmt | ergo_joint statement)
# [112] patterns
patterns ::= pattern ((',' | '且') pattern)*
# [113] pattern
pattern ::= '_' | negated_number | literal | type_pattern | (IDENTIFIER ut_pattern?)
# [114] negated_number
negated_number ::= '-' NUMBER
# [115] type_pattern
type_pattern ::= IDENTIFIER type_arguments? ut_pattern?
# [116] ut_pattern
ut_pattern ::= ('作为' IDENTIFIER) | (('常量' | '变量') pattern_binding (',' pattern_binding)*)
# [117] pattern_binding
pattern_binding ::= IDENTIFIER ('作为' IDENTIFIER)?
# [118] custodi_stmt
custodi_stmt ::= '守护' '{' si_guard_clause+ '}'
# [119] si_guard_clause
si_guard_clause ::= '如果' expression (block_stmt | ergo_joint statement)
# [120] ex_stmt
ex_stmt ::= '取自' expression ('常量' | '变量') extract_fields
# [121] extract_fields
extract_fields ::= extract_field (',' extract_field)* (',' ceteri_field)? | ceteri_field
# [122] extract_field
extract_field ::= IDENTIFIER ('作为' IDENTIFIER)?
# [123] ceteri_field
ceteri_field ::= '其余' IDENTIFIER
# [124] redde_stmt
redde_stmt ::= '返回' expression?
# [125] reddet_stmt
reddet_stmt ::= '等返' expression
# [126] tacebit_stmt
tacebit_stmt ::= '等弃' expression
# [127] cede_stmt
cede_stmt ::= '让出' expression
# [128] rumpe_stmt
rumpe_stmt ::= '中断'
# [129] perge_stmt
perge_stmt ::= '继续'
# [130] tacet_stmt
tacet_stmt ::= '静默'
# [131] iace_stmt
iace_stmt ::= iace_expr | iace_guarded_expr
# [132] iace_expr
iace_expr ::= ('抛错' | '崩溃') expression
# [133] iace_guarded_expr
iace_guarded_expr ::= ('抛错' | '崩溃') expression NO_NEWLINE '如果' expression
# [134] cape_clause
cape_clause ::= '捕获' IDENTIFIER block_stmt
# [135] adfirma_stmt
adfirma_stmt ::= '断言' expression ('崩溃' expression)?
# [136] requirit_stmt
requirit_stmt ::= '需求' expression '抛错' expression
# [137] reice_stmt
reice_stmt ::= '拒绝' expression '抛错' expression
# [138] expression
expression ::= assignment
# [139] transfer
transfer ::= ternary ('⇇' ternary)*
# [140] assignment
assignment ::= transfer ('←' assignment | '↤' assignment inline_default?)?
# [141] inc_dec_stmt
inc_dec_stmt ::= place ('↑' | '↓')
# [142] place
place ::= call_expr
# [143] ternary
ternary ::= aut_expr ('✓' expression '✗' aut_expr)?
# [144] aut_expr
aut_expr ::= et_expr (('或') et_expr)*
# [145] et_expr
et_expr ::= equality (('且') equality)*
# [146] equality
equality ::= comparison equality_tail*
# [147] equality_tail
equality_tail ::= ('≡' | '≢' | '≠' | '≅' | '≇' | '≈' | '≉' | '是' | '非' '是') comparison
# [148] comparison
comparison ::= format_expr (('≺' | '≻' | '≤' | '≥' | '内' | '间') format_expr)*
# [149] format_expr
format_expr ::= bitwise_or_expr ('¶' STRING)?
# [150] bitwise_or_expr
bitwise_or_expr ::= bitwise_xor_expr ('∨' bitwise_xor_expr)*
# [151] bitwise_xor_expr
bitwise_xor_expr ::= bitwise_and_expr ('⊻' bitwise_and_expr)*
# [152] bitwise_and_expr
bitwise_and_expr ::= shift_expr ('∧' shift_expr)*
# [153] shift_expr
shift_expr ::= range_expr (('⇐' | '⇒') range_expr)*
# [154] range_expr
range_expr ::= additive_expr range_tail?
# [155] range_tail
range_tail ::= ('‥' | '…' | '迄' | '到') additive_expr ('步' additive_expr)?
# [156] additive_expr
additive_expr ::= multiplicative_expr (('+' | '-' | '⤒' | '⤓') multiplicative_expr)*
# [157] multiplicative_expr
multiplicative_expr ::= vel_expr (('*' | '/' | '%' | '·' | '×' | '⊗' | '⊙' | '⊘') vel_expr)*
# [158] vel_expr
vel_expr ::= unary_expr ('兜底' vel_rhs)*
# [159] vel_rhs
vel_rhs ::= unary_expr vel_range_tail?
# [160] vel_range_tail
vel_range_tail ::= ('‥' | '…' | '迄' | '到') unary_expr ('步' unary_expr)?
# [161] unary_expr
unary_expr ::= ('-' | '¬' | '非') unary_expr | finge_expr | cast_expr
# [162] gradient_expr
gradient_expr ::= call_expr ('∇' gradient_selection?)?
# [163] gradient_selection
gradient_selection ::= '[' gradient_place (',' gradient_place)* ']'
# [164] gradient_place
gradient_place ::= expression
# [165] cast_expr
cast_expr ::= gradient_expr ('∷' type_annotation | conversio_expr)*
# [166] conversio_expr
conversio_expr ::= '↦' type_annotation inline_default?
# [167] inline_default
inline_default ::= '⊥' unary_expr
# [168] call_expr
call_expr ::= primary (call_suffix | member_suffix | transpose_suffix | optional_suffix | non_null_suffix)*
# [169] call_suffix
call_suffix ::= call_type_args? '(' argument_list ')'
# [170] member_suffix
member_suffix ::= '.' IDENTIFIER | '[' expression ']'
# [171] transpose_suffix
transpose_suffix ::= 'ᵀ'
# [172] optional_suffix
optional_suffix ::= '?.' IDENTIFIER | '?[' expression ']' | '?(' argument_list ')'
# [173] non_null_suffix
non_null_suffix ::= '!.' IDENTIFIER | '![' expression ']' | '!(' argument_list ')'
# [174] argument_list
argument_list ::= (argument (',' argument)*)?
# [175] argument
argument ::= template_argument | '展开'? expression
# [176] template_argument
template_argument ::= '展开'? IDENTIFIER ':' expression
# [177] literal
literal ::= NUMBER | STRING | ASCII_STRING | BACKTICK_STRING | OCTETI_STRING | '真' | '假' | '空' | '∞' | 'nan'
# [178] primary
primary ::= IDENTIFIER | literal | '自身' | array_literal | json_literal | typed_constructor | iuncta_expr | ad_expr | clausura_expr | praefixum_expr | scriptum_expr | lege_expr | first_match_expr | summa_expr | capta_expr | '(' expression ')'
# [179] ad_expr
ad_expr ::= '调用' ASCII_STRING ad_opener?
# [180] ad_opener
ad_opener ::= '(' expression ')'
# [181] array_literal
array_literal ::= '[' argument_list? ']'
# [182] iuncta_expr
iuncta_expr ::= '元组' type_arguments '[' argument_list? ']'
# [183] json_literal
json_literal ::= '{' (json_member (',' json_member)*)? '}'
# [184] json_member
json_member ::= STRING ':' json_value
# [185] typed_constructor
typed_constructor ::= type_annotation '{' field_list? '}'
# [186] field_list
field_list ::= field_init (',' field_init)*
# [187] field_init
field_init ::= (field_key '=' expression) | IDENTIFIER
# [188] field_key
field_key ::= IDENTIFIER | STRING | '[' expression ']'
# [189] json_value
json_value ::= json_object | json_array | json_string | json_number | 'true' | 'false' | 'null'
# [190] json_object
json_object ::= '{' (json_member (',' json_member)*)? '}'
# [191] json_array
json_array ::= '[' (json_value (',' json_value)*)? ']'
# [192] json_string
json_string ::= STRING
# [193] json_number
json_number ::= NUMBER
# [194] finge_expr
finge_expr ::= '构造' qualified_ident ('{' field_list '}')? ('∷' type_annotation)?
# [195] qualified_ident
qualified_ident ::= IDENTIFIER ('.' IDENTIFIER)*
# [196] praefixum_expr
praefixum_expr ::= '前缀' (block_stmt | '(' expression ')')
# [197] scriptum_expr
scriptum_expr ::= '格式化' '(' STRING (',' expression)* ')'
# [198] lege_expr
lege_expr ::= '读取' '行'?
# [199] first_match_expr
first_match_expr ::= '首个匹配' '(' expression apud_clause? ',' '其中' IDENTIFIER block_stmt ')'
# [200] summa_expr
summa_expr ::= '求和' '取自' expression apud_clause? filum_clause? ('常量' | '变量') IDENTIFIER block_stmt
# [201] filum_clause
filum_clause ::= '线程' IDENTIFIER
# [202] capta_expr
capta_expr ::= '陷阱' block_stmt
# [203] object_pattern
object_pattern ::= '{' pattern_property (',' pattern_property)* '}'
# [204] pattern_property
pattern_property ::= '其余'? IDENTIFIER ('作为' IDENTIFIER)?
# [205] array_pattern
array_pattern ::= '[' array_pattern_element (',' array_pattern_element)* ']'
# [206] array_pattern_element
array_pattern_element ::= '_' | '其余'? IDENTIFIER
# [207] nota_stmt
nota_stmt ::= ('显示' | '查看' | '警告' | '写入') expression (',' expression)*
# [208] entry_header
entry_header ::= ('参数' IDENTIFIER)? ('退出' expression)?
# [209] incipit_stmt
incipit_stmt ::= '入口' entry_header block_stmt
# [210] incipiet_stmt
incipiet_stmt ::= '异步入口' entry_header block_stmt
# [211] probandum_decl
probandum_decl ::= '验题' STRING proba_modifier* '{' probandum_body '}'
# [212] probandum_body
probandum_body ::= (praepara_block | probandum_decl | proba_stmt)*
# [213] proba_stmt
proba_stmt ::= '测试' STRING proba_modifier* block_stmt
# [214] proba_modifier
proba_modifier ::= '预期失败' | '跳过' STRING | '预期' STRING | '仅' | '标签' STRING | '时限' NUMBER | '计量' | '重复' NUMBER | '易碎' NUMBER | '仅于' STRING
# [215] praepara_block
praepara_block ::= ('备置' | '异步备置' | '收尾' | '异步收尾') '全部'? block_stmt
# [216] fac_stmt
fac_stmt ::= '执行' block_stmt cape_clause? ('当' expression)?
# [217] IDENTIFIER
IDENTIFIER ::=
# [218] NUMBER
NUMBER ::=
# [219] NATURAL
NATURAL ::=
# [220] STRING
STRING ::=
# [221] ASCII_STRING
ASCII_STRING ::=
# [222] BACKTICK_STRING
BACKTICK_STRING ::=
# [223] OCTETI_STRING
OCTETI_STRING ::=
# [224] NEWLINE
NEWLINE ::=
# [225] WIDTH_MARKER
WIDTH_MARKER ::=
# [226] LISTA_WIDTH_SUGAR
LISTA_WIDTH_SUGAR ::=
# [227] TENSOR_WIDTH_SUGAR
TENSOR_WIDTH_SUGAR ::=
# [228] SPARSA_WIDTH_SUGAR
SPARSA_WIDTH_SUGAR ::=
# [229] VECTOR_WIDTH_SUGAR
VECTOR_WIDTH_SUGAR ::=
# [230] MATRIX_WIDTH_SUGAR
MATRIX_WIDTH_SUGAR ::=
# [231] FRONTMATTER_DELIMITER
FRONTMATTER_DELIMITER ::=
# [232] TOML_LINES
TOML_LINES ::=
# [233] ANNOTATION_NAME
ANNOTATION_NAME ::=
# [234] ANNOTATION_FIELD_NAME
ANNOTATION_FIELD_NAME ::=
# [235] NON_NEWLINE_TOKEN
NON_NEWLINE_TOKEN ::=
# [236] NO_NEWLINE
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
| [`NON_NEWLINE_TOKEN`](#non-newline-token) | `#非-newline-token` | capture-pending |
| [`NO_NEWLINE`](#no-newline) | `#no-newline` | capture-pending |
| [`fab_file`](#fab-file) | `#fab-file` | live |
| [`frontmatter`](#frontmatter) | `#frontmatter` | live |
| [`program`](#program) | `#program` | live |
| [`statement`](#statement) | `#statement` | live |
| [`statement_core`](#statement-core) | `#statement-core` | live |
| [`binding_decl`](#binding-decl) | `#binding-decl` | live |
| [`expr_stmt`](#expr-stmt) | `#expr-stmt` | live |
| [`block_stmt`](#block-stmt) | `#block-stmt` | live |
| [`static_decl`](#static-decl) | `#static-decl` | live |
| [`fixum_decl`](#fixum-decl) | `#常量-decl` | live |
| [`figendum_decl`](#figendum-decl) | `#等定-decl` | live |
| [`sit_decl`](#sit-decl) | `#设-decl` | live |
| [`array_destruct`](#array-destruct) | `#array-destruct` | live |
| [`object_destruct`](#object-destruct) | `#object-destruct` | live |
| [`functio_decl`](#functio-decl) | `#函数-decl` | live |
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
| [`ergo_joint`](#ergo-joint) | `#则-joint` | live |
| [`clausura_joint`](#clausura-joint) | `#闭包-joint` | live |
| [`clausura_expr`](#clausura-expr) | `#闭包-expr` | live |
| [`compact_clausura_expr`](#compact-clausura-expr) | `#compact-闭包-expr` | live |
| [`clausura_signature`](#clausura-signature) | `#闭包-signature` | live |
| [`closure_modifier`](#closure-modifier) | `#closure-modifier` | live |
| [`fac_block`](#fac-block) | `#执行-block` | live |
| [`clausura_legacy_expr`](#clausura-legacy-expr) | `#闭包-legacy-expr` | live |
| [`clausura_params`](#clausura-params) | `#闭包-params` | live |
| [`clausura_param`](#clausura-param) | `#闭包-param` | live |
| [`genus_decl`](#genus-decl) | `#类-decl` | live |
| [`genus_member`](#genus-member) | `#类-member` | live |
| [`field_decl`](#field-decl) | `#field-decl` | live |
| [`functio_method_decl`](#functio-method-decl) | `#函数-method-decl` | live |
| [`annotation`](#annotation) | `#annotation` | live |
| [`annotation_name`](#annotation-name) | `#annotation-name` | live |
| [`braced_annotation`](#braced-annotation) | `#braced-annotation` | live |
| [`annotation_field_list`](#annotation-field-list) | `#annotation-field-list` | live |
| [`annotation_field`](#annotation-field) | `#annotation-field` | live |
| [`annotation_sugar`](#annotation-sugar) | `#annotation-sugar` | live |
| [`nucleum_annotation`](#nucleum-annotation) | `#内核-annotation` | live |
| [`nucleum_sugar`](#nucleum-sugar) | `#内核-sugar` | live |
| [`nucleum_braced`](#nucleum-braced) | `#内核-braced` | live |
| [`nucleum_modifier`](#nucleum-modifier) | `#内核-modifier` | live |
| [`nucleum_field_list`](#nucleum-field-list) | `#内核-field-list` | live |
| [`nucleum_field`](#nucleum-field) | `#内核-field` | live |
| [`radix_annotation`](#radix-annotation) | `#radix-annotation` | live |
| [`radix_directive`](#radix-directive) | `#radix-directive` | live |
| [`ad_annotation`](#ad-annotation) | `#调用-annotation` | live |
| [`implendum_decl`](#implendum-decl) | `#契约-decl` | live |
| [`implendum_method_decl`](#implendum-method-decl) | `#契约-method-decl` | live |
| [`typus_decl`](#typus-decl) | `#类型-decl` | live |
| [`ordo_decl`](#ordo-decl) | `#枚举-decl` | live |
| [`enum_member`](#enum-member) | `#enum-member` | live |
| [`discretio_decl`](#discretio-decl) | `#判别-decl` | live |
| [`union_member`](#union-member) | `#union-member` | live |
| [`variant`](#variant) | `#variant` | live |
| [`variant_fields`](#variant-fields) | `#variant-fields` | live |
| [`schema_decl`](#schema-decl) | `#架构-decl` | live |
| [`schema_column`](#schema-column) | `#架构-column` | live |
| [`importa_decl`](#importa-decl) | `#导入-decl` | live |
| [`importa_record`](#importa-record) | `#导入-record` | live |
| [`import_field_list`](#import-field-list) | `#import-field-list` | live |
| [`import_field`](#import-field) | `#import-field` | live |
| [`ex_field`](#ex-field) | `#取自-field` | live |
| [`visibilitas_field`](#visibilitas-field) | `#visibilitas-field` | live |
| [`nomen_field`](#nomen-field) | `#名称-field` | live |
| [`ut_field`](#ut-field) | `#作为-field` | live |
| [`omnia_field`](#omnia-field) | `#全部-field` | live |
| [`importa_sugar`](#importa-sugar) | `#导入-sugar` | live |
| [`公开`](#publica) | `#公开` | live |
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
| [`si_stmt`](#si-stmt) | `#如果-stmt` | live |
| [`secus_clause`](#secus-clause) | `#否则-clause` | live |
| [`arm`](#arm) | `#arm` | live |
| [`else_arm`](#else-arm) | `#else-arm` | live |
| [`dum_stmt`](#dum-stmt) | `#当-stmt` | live |
| [`itera_stmt`](#itera-stmt) | `#遍历-stmt` | live |
| [`itera_binding`](#itera-binding) | `#遍历-binding` | live |
| [`apud_clause`](#apud-clause) | `#于-clause` | live |
| [`elige_stmt`](#elige-stmt) | `#选择-stmt` | live |
| [`casu_elige_clause`](#casu-elige-clause) | `#情况-选择-clause` | live |
| [`ceterum_clause`](#ceterum-clause) | `#默认-clause` | live |
| [`discerne_stmt`](#discerne-stmt) | `#匹配-stmt` | live |
| [`discriminants`](#discriminants) | `#discriminants` | live |
| [`casu_variant_clause`](#casu-variant-clause) | `#情况-variant-clause` | live |
| [`patterns`](#patterns) | `#patterns` | live |
| [`pattern`](#pattern) | `#pattern` | live |
| [`negated_number`](#negated-number) | `#negated-number` | live |
| [`type_pattern`](#type-pattern) | `#type-pattern` | live |
| [`ut_pattern`](#ut-pattern) | `#作为-pattern` | live |
| [`pattern_binding`](#pattern-binding) | `#pattern-binding` | live |
| [`custodi_stmt`](#custodi-stmt) | `#守护-stmt` | live |
| [`si_guard_clause`](#si-guard-clause) | `#如果-guard-clause` | live |
| [`ex_stmt`](#ex-stmt) | `#取自-stmt` | live |
| [`extract_fields`](#extract-fields) | `#extract-fields` | live |
| [`extract_field`](#extract-field) | `#extract-field` | live |
| [`ceteri_field`](#ceteri-field) | `#其余-field` | live |
| [`redde_stmt`](#redde-stmt) | `#返回-stmt` | live |
| [`reddet_stmt`](#reddet-stmt) | `#等返-stmt` | live |
| [`tacebit_stmt`](#tacebit-stmt) | `#等弃-stmt` | live |
| [`cede_stmt`](#cede-stmt) | `#让出-stmt` | live |
| [`rumpe_stmt`](#rumpe-stmt) | `#中断-stmt` | live |
| [`perge_stmt`](#perge-stmt) | `#继续-stmt` | live |
| [`tacet_stmt`](#tacet-stmt) | `#静默-stmt` | live |
| [`iace_stmt`](#iace-stmt) | `#抛错-stmt` | live |
| [`iace_expr`](#iace-expr) | `#抛错-expr` | live |
| [`iace_guarded_expr`](#iace-guarded-expr) | `#抛错-guarded-expr` | live |
| [`cape_clause`](#cape-clause) | `#捕获-clause` | live |
| [`adfirma_stmt`](#adfirma-stmt) | `#断言-stmt` | live |
| [`requirit_stmt`](#requirit-stmt) | `#需求-stmt` | live |
| [`reice_stmt`](#reice-stmt) | `#拒绝-stmt` | live |
| [`expression`](#expression) | `#expression` | live |
| [`transfer`](#transfer) | `#transfer` | live |
| [`assignment`](#assignment) | `#assignment` | live |
| [`inc_dec_stmt`](#inc-dec-stmt) | `#inc-dec-stmt` | live |
| [`place`](#place) | `#place` | live |
| [`ternary`](#ternary) | `#ternary` | live |
| [`aut_expr`](#aut-expr) | `#或-expr` | live |
| [`et_expr`](#et-expr) | `#且-expr` | live |
| [`equality`](#equality) | `#equality` | live |
| [`equality_tail`](#equality-tail) | `#equality-tail` | live |
| [`comparison`](#comparison) | `#comparison` | live |
| [`format_expr`](#format-expr) | `#format-expr` | live |
| [`bitwise_or_expr`](#bitwise-or-expr) | `#bitwise-or-expr` | live |
| [`bitwise_xor_expr`](#bitwise-xor-expr) | `#bitwise-xor-expr` | live |
| [`bitwise_and_expr`](#bitwise-and-expr) | `#bitwise-and-expr` | live |
| [`shift_expr`](#shift-expr) | `#shift-expr` | live |
| [`range_expr`](#range-expr) | `#range-expr` | live |
| [`range_tail`](#range-tail) | `#range-tail` | live |
| [`additive_expr`](#additive-expr) | `#additive-expr` | live |
| [`multiplicative_expr`](#multiplicative-expr) | `#multiplicative-expr` | live |
| [`vel_expr`](#vel-expr) | `#兜底-expr` | live |
| [`vel_rhs`](#vel-rhs) | `#兜底-rhs` | live |
| [`vel_range_tail`](#vel-range-tail) | `#兜底-range-tail` | live |
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
| [`non_null_suffix`](#non-null-suffix) | `#非-null-suffix` | live |
| [`argument_list`](#argument-list) | `#argument-list` | live |
| [`argument`](#argument) | `#argument` | live |
| [`template_argument`](#template-argument) | `#template-argument` | live |
| [`literal`](#literal) | `#literal` | live |
| [`primary`](#primary) | `#primary` | live |
| [`ad_expr`](#ad-expr) | `#调用-expr` | live |
| [`ad_opener`](#ad-opener) | `#调用-opener` | live |
| [`array_literal`](#array-literal) | `#array-literal` | live |
| [`iuncta_expr`](#iuncta-expr) | `#元组-expr` | live |
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
| [`finge_expr`](#finge-expr) | `#构造-expr` | live |
| [`qualified_ident`](#qualified-ident) | `#qualified-ident` | live |
| [`praefixum_expr`](#praefixum-expr) | `#前缀-expr` | live |
| [`scriptum_expr`](#scriptum-expr) | `#格式化-expr` | live |
| [`lege_expr`](#lege-expr) | `#读取-expr` | live |
| [`first_match_expr`](#first-match-expr) | `#first-match-expr` | live |
| [`summa_expr`](#summa-expr) | `#求和-expr` | live |
| [`filum_clause`](#filum-clause) | `#线程-clause` | live |
| [`capta_expr`](#capta-expr) | `#陷阱-expr` | live |
| [`object_pattern`](#object-pattern) | `#object-pattern` | live |
| [`pattern_property`](#pattern-property) | `#pattern-property` | live |
| [`array_pattern`](#array-pattern) | `#array-pattern` | live |
| [`array_pattern_element`](#array-pattern-element) | `#array-pattern-element` | live |
| [`nota_stmt`](#nota-stmt) | `#显示-stmt` | live |
| [`entry_header`](#entry-header) | `#entry-header` | live |
| [`incipit_stmt`](#incipit-stmt) | `#入口-stmt` | live |
| [`incipiet_stmt`](#incipiet-stmt) | `#异步入口-stmt` | live |
| [`probandum_decl`](#probandum-decl) | `#验题-decl` | live |
| [`probandum_body`](#probandum-body) | `#验题-body` | live |
| [`proba_stmt`](#proba-stmt) | `#测试-stmt` | live |
| [`proba_modifier`](#proba-modifier) | `#测试-modifier` | live |
| [`praepara_block`](#praepara-block) | `#备置-block` | live |
| [`fac_stmt`](#fac-stmt) | `#执行-stmt` | live |

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
| Iteration | `范围` | range iteration |
| Endpoints | `调用` | capability call |
| Error | `断言` | assert |
| Iteration | `迄` | range until exclusive |
| Grammar | `于` | keyword literal derived from the production |
| Params | `参数` | CLI arguments modifier |
| Boolean | `或` | or |
| Annotation | `backward` | `@ radix` gradient-companion directive |
| Error | `捕获` | local handler |
| Error | `陷阱` | capture boundary (error channel reified as a value) |
| Control | `情况` | case |
| Async | `让出` | yield |
| Params | `其余` | rest |
| Control | `默认` | default case |
| Objects | `闭包` | legacy closure |
| Declarations | `列` | relational column (experimental; census-types) |
| Type | `拷贝` | copy ownership |
| Control | `守护` | guard |
| Type | `借自` | borrow / for-in keys |
| Control | `匹配` | pattern match |
| Declarations | `判别` | tagged union |
| Control | `当` | while / postfix until |
| Objects | `自身` | self |
| Control | `选择` | switch |
| Control | `则` | compact statement-body joint |
| Params | `勘误` | error channel |
| Testing | `预期失败` | expect failure |
| Boolean | `是` | is / equality |
| Boolean | `且` | and |
| Iteration | `取自` | for-of / import from |
| Params | `退出` | exit code |
| Control | `执行` | do block / post-test loop |
| JSON | `false` | JSON false |
| Boolean | `假` | false |
| Async | `异流` | async stream posture |
| Async | `异步` | async finite posture |
| Async | `等定` | await-bind immutable |
| Grammar | `线程` | keyword literal derived from the production |
| Objects | `构造` | construct variant |
| Async | `流` | sync stream posture |
| Declarations | `常量` | immutable binding |
| Testing | `易碎` | flaky |
| Annotation | `片段` | nucleum fragment |
| Declarations | `函数` | function |
| Testing | `预期` | future |
| Genus | `静态` | static member |
| Declarations | `类` | class |
| Error | `抛错` | throw |
| Error | `可抛` | throws marker |
| Params | `不变` | immutable modifier |
| Declarations | `契约` | interface contract |
| Genus | `实现` | implements |
| Declarations | `导入` | import |
| Type | `传入` | ownership in |
| Declarations | `异步入口` | async entrypoint |
| Declarations | `入口` | entrypoint |
| Iteration | `间` | between |
| Iteration | `内` | membership |
| Control | `遍历` | for |
| Objects | `元组` | tuple type/constructor |
| Annotation | `车道` | `@ radix` compiler-lane directive |
| Builtin | `读取` | read |
| Objects | `自由` | capture-free closure modifier |
| Builtin | `行` | line |
| Declarations | `维度` | size/index generic parameter |
| Testing | `计量` | benchmark |
| Diagnostics | `警告` | warn |
| Error | `崩溃` | panic |
| Literals | `空` | none |
| Declarations | `名称` | import binding name |
| Boolean | `非` | not |
| Literals | `nan` | named NaN literal (`nan` outside the Latin pack) |
| Diagnostics | `显示` | note |
| Annotation | `内核` | kernel annotation; kernel closure modifier |
| JSON | `null` | JSON null |
| Testing | `跳过` | skip |
| Params | `全部` | all / glob |
| Params | `可选项` | options modifier |
| Declarations | `枚举` | enum |
| Type | `拥有` | owned |
| Iteration | `步` | range step |
| Control | `继续` | continue |
| Testing | `收尾` | teardown |
| Testing | `异步收尾` | async teardown |
| Objects | `前缀` | prefix expression |
| Testing | `备置` | setup |
| Testing | `异步备置` | async setup |
| Grammar | `首个匹配` | first-match selection head |
| Testing | `测试` | test |
| Testing | `验题` | test suite |
| Declarations | `公开` | public visibility |
| Annotation | `radix` | compiler-reserved annotation family |
| Objects | `ratio` | named-field aggregate type/constructor |
| Control | `返回` | return |
| Async | `等返` | await-return |
| Error | `拒绝` | reject |
| Testing | `重复` | repeat |
| Error | `需求` | require |
| Control | `中断` | break |
| Declarations | `架构` | relational heading (experimental; census-types) |
| Diagnostics | `写入` | diagnostic channel |
| Builtin | `格式化` | write |
| Control | `否则` | else |
| Control | `如果` | if |
| Control | `否则如果` | else-if |
| Declarations | `设` | inferred immutable local |
| Testing | `仅` | only |
| Testing | `仅于` | only-in |
| Params | `展开` | spread |
| Declarations | `可选` | optional declaration slot |
| Grammar | `求和` | keyword literal derived from the production |
| Async | `等弃` | await-discard |
| Control | `静默` | no-op |
| Testing | `标签` | tag |
| Testing | `时限` | timeout |
| JSON | `true` | JSON true |
| Declarations | `类型` | type alias |
| Grammar | `其中` | first-match predicate tail |
| Iteration | `到` | range until inclusive |
| Params | `作为` | as / alias |
| Declarations | `变量` | mutable binding |
| Async | `等变` | await-bind mutable |
| Boolean | `兜底` | nullable default |
| Boolean | `真` | true |
| Diagnostics | `查看` | debug |
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
lists, `枚举` members, `判别` variant lists, JSON members and array
elements, annotation / import / nucleum fields, output statement lists) —
require a comma between adjacent items and forbid one after the last.

**Declaration blocks** — self-annotating declarations (statements, `类`
members, `契约` methods, `判别` payload fields) — contain no commas.
Entries are trivia-delimited.

---

## Declarations

Declarations are top-level. A `函数` and the type declarations (`类`,
`契约`, `类型`, `枚举`, `判别`, `架构`) may not appear inside a
block; the parser rejects them there (`declaration_not_top_level`). Methods
live in `类` bodies. For a local function, bind a closure; for recursion,
use a top-level function.

### Variables


- `常量` = immutable binding (write-once): it may be declared without an
  initializer and assigned exactly once later, then frozen. `变量` = mutable
  binding (reassignable), like `let`.
- `等定` / `等变` await a `promissum<T>` or `promissum<T ⇥ E>`, bind
  the resolved `T`, and propagate a compatible alternate `E`.
- `↢` is the await-directed initializer for an ordinary declaration:
  `常量 T name ↢ future`, `变量 T name ↢ future`, or `设 name ↢ future`.
  It has the same await and alternate-propagation semantics as
  `等定 T name ← future`, but it is not a general expression operator and
  cannot target an existing place.
- Use `_` as the type annotation when the initializer determines the type: `常量 _ name ← value`
- `设 name ← value` is sugar for `常量 _ name ← value` (inferred immutable local)
- `设 name` (no initializer) is sugar for `常量 _ name` — the inferred deferred
  immutable. Assign exactly once before any read.
- Typed `常量`/`变量` initializers accept `↤` (`常量 numerus x ↤ "42"`):
  the written type is the conversion destination, then the binding is
  initialized. `等定`/`等变` keep `←`; `常量 _`, `设`, and untyped
  destructuring reject `↤` (no concrete destination type).
- Deferred init: `常量 numerus x` or `设 x` declares an uninitialized immutable
  slot that must be assigned exactly once before any read; a second assignment is
  rejected. The definite-assignment pass (semantic Phase 3a) enforces this.

### Top-level statics


`常量` and `变量` are not allowed at top level (D5.7): module-level mutable
state does not exist. A top-level `常量`/`变量` binding is a migration
warning (`top_level_binding`) until the library sweep lands, when it becomes
an error.

The top-level static is `静态` (en `static`): `静态 numerus LIMES = 4096`
(D5.8) — the same production as a `类` static field, used in a second,
top-level-only scope. It is the only top-level value declaration; declaring
one inside a block is a parse error (`static_not_top_level`), the same
enforcement shape as `函数`/`类`/`枚举`/`判别` at non-top level.

Statics are **immutable and initialized with `=` only** (D5.9), never `←`; a
missing initializer or an initializer spelled with `←` is a named parse
error. The initializer must be evaluable at compile time: literals;
arithmetic, comparison, bit, and logical operators on `numerus`, `fractus`,
and `bivalens` scalars, plus `textus` concatenation; references to other
statics (evaluated in dependency order — a cycle is `static_cycle`); and
collection literals (`lista`, tuples, map construction) whose elements are
constants (only their scalar leaves fold). Anything else is
`static_initializer_not_constant`. Decimal widths and `modulus<W>` values are
not folded, so arithmetic on them is not a compile-time constant today.
Compile-time integer arithmetic is checked (overflow and division by zero are
compile errors), matching the runner's checked runtime semantics.

### Functions


### Capture-free closures


`自由` is the canonical Latin spelling of the `closure_modifier`; the English reader spelling is `free`. The modifier follows the parameter list in both compact and legacy `闭包` forms, before any `→` return or `⇥` alternate-exit clause. It declares a checked capture-free contract: the closure may use its own parameters, body locals, and module-level items, but it must not reference a local or parameter from an enclosing function. Such a capture is rejected by the compiler.

```fab
sit summa ← (numerus a, numerus b) libera ∴ a + b
clausura numerus x libera: x * 2
```

`内核` is the second spelling of the `closure_modifier`; the English reader spelling is `kernel`. The alternative is locale-sealed and singular: at most one modifier may occupy the slot, each reader pack admits only its declared spelling, and stacked spellings such as `free kernel` are rejected as a duplicate modifier. A `kernel` closure requires everything `free` requires — no reference to an enclosing function's local or parameter, while its own parameters, body locals, and module-level items stay legal — plus the device-safe subset used by kernel functions: typed tensors and scalars, glyphs, structured control, and calls to other device functions. Host allocation, I/O, bags, dynamic calls, `⇥` clauses, `抛错` throws, and `捕获` recovery are rejected in the kernel contract; `返回` returns only the closure's own `→` result. Declaration annotations `@ 内核` (`@ kernel` in the English reader) are unchanged: they remain the role marker for named functions, and the closure modifier is their expression-form twin.

The body joint keeps the existing closure law: `∴` followed by one expression, or `∴ 执行 { ... }` (`do` in the English reader); bare `{ ... }` is not a closure body. A kernel closure is usable only as a local immutable binding in its enclosing function and only called there, or invoked immediately in the same expression; it is not a first-class value and cannot escape into a field, list element, return value, or ordinary-function argument. The compiler lowers it to a private synthetic kernel with a stable identity: one launch when its host caller invokes it, direct composition with no surviving device-to-device runtime call when a kernel caller invokes it, and never a public launch entry or ABI row. The modifier does not request fusion; two local kernel closures remain two launches unless a later cross-launch pass fuses them.

```fab
fixum _ duplica ← (tensor<f32, [8]> x) nucleum ∴ x + x
fixum _ dup ← duplica(xs)
```

- Return syntax: `→` declares the normal success type. A bodyful function with no `→` is effect-only (`vacuum`) and must not contain `返回`. A statement-bodied closure (`执行 { ... }` or legacy block body) must also spell `→ T` before it can use `返回`; expression-bodied closures may infer their result from the expression.
- Recoverable alternate-exit syntax: `⇥` declares the error-channel type. It can appear after `→ T` or alone on an effect-only failable function or closure. A closure body that uses an escaping `抛错` must declare its own `⇥ E`; it cannot inherit the enclosing function's error channel. A local `执行 { ... } 捕获 err { ... }` may catch `抛错` without an enclosing `⇥`. A failable function call (`→ T ⇥ E`) inside a `⇥`-declaring function propagates to the function's alternate exit without a `执行`/`捕获` wrapper, mirroring how bare `↦` conversio and `抛错` throws already behave; the call lowers to Rust `?`. A closure must still declare its own `⇥` to propagate a failable call — the enclosing function's error channel does not cross the closure boundary.
- In a signature, `⇥` only ever names an error type (`→ T ⇥ E`). It never carries a value.
- Parameter access markers live in the type position: `借自`/`ref` (read), `传入`/`mut` (mutate), `拥有` (consume), and `拷贝` (duplicate then own). The retired parameter-prefix slot is not part of the grammar; `取自`/`from` remains the import/iteration/extraction token identity.
- Post-name marker: `可选` (voluntary/optional provision)
- `其余` marks rest parameter
- Ordinary `函数` declarations and genus methods require bodies. Signature-only methods belong in `契约`.
- `勘误 NAME` is a legacy runtime-injected `ignotum` local, and `可抛` is a legacy marker with no current semantic effect. Neither declares the typed alternate-exit contract. New failable APIs should use `⇥ E`; whether either legacy modifier should survive is unresolved.
- `则` is the compact **statement-body** joint only (one-statement `如果`/`当`/`情况`/… arms).
- `∴` is the compact **clausura** joint only. The two are not aliases.
- Compact closure block bodies must use `执行 { ... }`; a closure-local `执行` body may attach `捕获`, but cannot use postfix `当`.

### Classes

A `类` is a struct with methods. It holds data, its methods act on that
data, and it satisfies contracts through `实现`. It is not a self-contained
object that owns its own construction and process: a value is built with a
construction literal (`Genus { field = value }`).

- **No class inheritance.** Inheritance was removed: there is no `sub`
  (extends) clause and no `abstractus` genus. Shared behaviour comes from
  contracts (`契约` + `实现`) and from composition — a field holding
  another value. The old spellings are rejected with a migration diagnostic.

- **No static methods.** A `类` declares instance methods only. A function
  about a type is a top-level function in the type's file, reached through the
  import alias. `静态` marks a type-level field, never a method.
- **A newtype is a one-field `类`.** There is no separate newtype
  declaration. Units that need arithmetic wait on operator overloading.
- **No macros and no user derive.** What you read is what runs. Code
  generation, when a project needs it, is an external step before the build.
- **No extension methods and no retroactive conformance, for now.** A type's
  methods and its `实现` contracts are declared on the type itself. Code
  elsewhere cannot add either. Allowing it would need coherence rules, and is
  revisited together with the contract features that are deferred.

### Annotations


`@ 内核 片段` is a modifier on the `内核` annotation (sugar or
braced `片段 = 真` / `假`), not a fused annotation name and not the
graphics `@ 片段` stage. Standalone `@ 片段` is unchanged.

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

**Annotation contracts:** `@ annotatio` (optionally `@ annotatio { target = 函数 }`)
marks a top-level `类` as a compile-time annotation contract. Ordinary genera
are not annotation schemas. Applications use `@ ContractName { field = constant }`
and resolve through local declarations or imported file-interface exports.
Resolved applications lower to `HirAnnotation` with `contract_id: Some(DefId)`
and constant field values. v1 attachment target is `函数` only; payload
scalars are `textus`, `numerus`, `fractus`, and `bivalens` (optional via
`可选` or `T ∪ 空`). Web, HTTP, controller, and framework route families
are not compiler-owned; they are built as libraries, from annotation contracts
or on top of `@ 调用`. The one exception is `@ 调用` itself: it is the
compiler-owned serving half of `调用` (see Capability Calls).

User annotations are metadata. Their consumers are tools, such as product
packaging. They never change compilation, and Faber code never reads them at
run time. An annotation that changes compilation is compiler-owned (`@ json`,
`@ 调用`, `@ radix`).

**JSON genera:** `@ json` on a `类` is a compiler-owned data-model contract,
not a generic annotation schema. Fields must be JSON-safe (`textus`, `ascii`,
`numerus`, `fractus`, `bivalens`, `instans`, `空`, `lista<T>`,
`tabula<textus, T>`, nullable `T ∪ 空`, or another `@ json 类`). Field
metadata `@ json { 名称 = "wire_name" }` changes the emitted object key used by
`value ↦ valor`, `value ↦ json`, and `json ↦ Genus`; JSON text remains a Norma
wire operation such as `json.pange(value ↦ json)`.

- `@ radix` is **compiler-reserved**: every form under it is compiler-owned
  metadata, not an application surface, and may change with the compiler.
  The historical morphology-stem meaning is retired; morphology remains a
  source naming discipline, not compiler-generated conjugation. The family
  (`radix_annotation` plus the braced records) is:
  - `@ radix 车道 "air"` / `"mir"` / `"hir-direct"` (braced
    `@ radix { 车道 = "air" }`) on top-level functions for explicit
    compiler-lane routing; unsupported lane/target combinations reject with
    diagnostics instead of being ignored.
  - `@ radix backward "name"` on an `air`-lane function names the generated
    reverse-mode gradient companion; it is valid only paired with
    `车道 "air"`.
  - `@ radix 类型 T 传入 A B …` (braced `@ radix { param = T, allowed = A, … }`)
    restricts the type parameter `T` of the annotated declaration to the listed
    domain.
  Any other directive after `@ radix` is rejected (`unknown_directive`).
- `@ verte` defines codegen transformation (method name or template)
- `@ nondum [TARGET] ["REASON"]` marks a declaration as present in an interface but unavailable for the target
- `@ cli "NAME"` marks an `入口` entry as a CLI program
- `@ imperium "NAME"` marks a function as a CLI command entry point
- `@ optio NAME ...` defines a CLI option; use `类型 bivalens` for boolean flags
- `@ operandus [其余] TYPE NAME ...` defines a CLI positional argument
- `@ futura` marks a function as async (legacy — prefer `异步` posture word)
- `@ cursor` marks a function as generator (legacy — prefer `流` posture word)
- Callable posture words (`异步`/`流`/`异流`) are recognized in the signature
  slot after modifiers and before `→`/`⇥`/body; bare means synchronous finite
- `@ 公开` marks a declaration for the file's importable (export) surface; `@ interna` marks it package-internal (same-package importable only); `@ privata` is an explicit module-private marker. Unmarked top-level declarations are module-private by default; a declaration mixing distinct visibility tiers is rejected with `SEM019` (`conflicting_visibility`)
- `@ protecta` is reserved and rejected with a semantic diagnostic; it has no package, subclass, or sibling-file visibility meaning
- `@ doc` is not an annotation. Comments are the documentation: a line comment attaches forward to the declaration it precedes, and there is no doc marker.

- `实现` = implements (conformance to an `契约` contract)
- `静态` = static (type-level) field. A field has no other modifier; the
  former `nexum` field modifier is removed and rejected with a migration
  diagnostic.
- Members are public by default. Until member privacy is enforced,
  `@ privata` or `@ interna` on a `类` member is an error
  (`member_privacy_unenforced`), because it promises privacy nothing checks;
  `@ 公开` on a member is a redundant-annotation warning
  (`redundant_member_publica`), an error when warnings are denied.
- A type may refer to itself: `判别 Expr { Adde { Expr sinister, Expr dexter } }`
  and `类 Nodus { Nodus ∪ 空 next }` need no keyword and no box type.
  Values have reference semantics, so the indirection is implied; a backend
  that stores fields inline inserts it on the fields that close a type cycle.

### Interfaces


`契约` is the **contract** construct: signature-only methods for `实现`
(gerundive of *implere* — that which must be fulfilled). Import namespaces are
`.fab` file boundaries; exported declarations live at file top level.

A contract has no default method bodies. Default bodies would make a contract
an abstract base class without fields. Behaviour shared by every implementer
is a top-level function that takes the contract type. Contract inheritance (a
contract that requires another), associated types, and retroactive
conformance are deferred.

### Type Aliases


### Enums


`枚举` (an enum) and `判别` (a tagged union) are **data only** (D9.1): a
`函数` member inside either body is a parse error (`sum_type_function`,
recovered so parsing resumes at the next member), and an `实现` clause on
either header is a parse error (`sum_type_implements`) before the body is even
read. Shared behavior over an `枚举`/`判别` value is an ordinary
top-level function that takes the type, the same posture `契约` already
uses for contract default bodies.

An `枚举` converts without user code (D9.4). A member's discriminant is the
authored number, or the previous member's number plus one; the first member
defaults to `0`. A string-valued member has no discriminant.

- `Ordo ↦ numerus` — the member's discriminant; infallible.
- `numerus ↦ Ordo` — the first member whose discriminant equals the value;
  failable when none matches (`⊥` default, or `textus` propagation).
- `Ordo ↦ textus` — the member's name; infallible.

Other conversion pairs involving an `枚举` fall through to the ordinary
`unsupported_conversio` rejection.

A registered `@ conversio (A, B)` also serves `a ↦ B` for a program's own
error types (see Annotations): a direct (source, destination) pair only, never
auto-composed into a chain, and a missing row fails closed.

### Tagged Unions


Variant lists are an item list: comma required between variants, forbidden
after the last. Payload fields inside a variant are a declaration block
(genus-style, no commas).

**Union overlap access (D9.2):** a call, read, or write on a field/method name
through a union (`判别` or `∪`) value type-checks when **every**
constituent exposes it with the **same declared type**, then dispatches per
the value's actual member at runtime — access is not restricted to a common
supertype shape. A constituent that lacks the name is `union_member_not_common`;
when every constituent has it but the declared types disagree, it is
`union_member_differs` (each constituent's type is named in the diagnostic).

### Relational Schemas (experimental)

**Experimental** — owned by the `census-types` goal; the surface may change.
`架构 Name { 列 T name … }` declares an application-owned relational
heading for database results. It names only the columns the application reads;
extra source columns stay invisible. Each `列` row takes a type (use
`T ∪ 空` for a nullable column) and a name, with an optional
`: sourceName` alias mapping the public column to a source column (absent means
identity). Column rows are a declaration block (no commas). A schema has no
methods (`schema_method`), no `实现`
(`schema_inheritance`), and no nested columns (`schema_nested_column`); each is
rejected at parse time.

### Identifier Naming

Faber has no globally reserved words. Keyword ownership is contextual per
spelling: a keyword claims only its owning grammar slot. Every user-chosen
name slot accepts every keyword spelling — declaration names, parameters,
members, binding targets (`常量`/`变量`/`设` patterns and captures),
import aliases, and loop/iteration bindings. Type-name slots stay out.

Outside a spelling's owning contexts, that spelling may be an `IDENTIFIER`.
An owning context may itself be effectively global when its production
applies everywhere a statement or expression may begin. Builtin claims
(`读取`/`行`/`格式化`/`vacua`, and the scribe family in
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
does not re-export, and `公开` is the re-export marker. Missing named binding
defaults to the
last import path segment when it is a valid, non-conflicting identifier. If the
inferred name is invalid or collides with an existing top-level binding, spell an
explicit `名称` or `作为` binding.

**Selective imports** create ordinary immutable local bindings: `导入 取自 "norma:consolum" 常量 dic 作为 output, funde 作为 output_bytes` imports one exported member per `常量` local. The pre-`作为` identifier names an exported member in the imported file; the post-`作为` identifier is the caller-owned local binding; the imported file interface supplies the complete type. A member may be a value (a function or constant) or a type declaration; the syntax is the same for both. The bindings obey ordinary local-binding rules (duplicates, shadowing, lints), are locale-resolved through the imported module, and are never re-exports. Wildcard members cannot mix into the list. The current parser tolerates one trailing comma after the final member; the canonical spine keeps every comma required.

`导入 取自 "faber:*" faber` is kernel-specific sugar: the glob lives
inside the import path string and expands the released binary's kernel manifest
into `faber.<module>.<verb>` calls. It is not a wildcard re-export and does not create a runtime aggregate value.

---

## Types


- Declaration parameters (`genericParams`) and applied arguments (`typeArguments`) are distinct grammar categories. Applied arguments admit nested types and static `figura` values. `typeArguments` still admits `NATURAL`.
- Applied `NATURAL` arguments are `维度` capacity facts, not width markers. Shipped bounded forms use that slot: `lista<T, N>`, `queue<T, N>`, `stack<T, N>`, `textus<N>`, `ascii<N>`, `octeti<N>`. Width-marker families such as `numerus<i32>` stay the separate `widthTypeSugar` production below.
- A second applied argument on a `↦` target (`numerus<W, Hex>`, `numerus<W, Be>`) is a convert-slot hint, not a type identity, not a width marker, and not a keyword. Live text-parse hints are `Hex` / `Bin` / `Oct`. `Be` / `Le` occupy that same Hex slot for endian unpack — both integer (`octeti[lo‥hi] ↦ numerus<W, Be|Le>`) and float windows (`octeti[lo‥hi] ↦ fractus<f32|f64, Be|Le>`, window 4/8, same fail rules as the integer rows). `Bits` occupies the same slot as an exact-width bitcast hint (reinterpretation, not value conversion; never a base). `typeArguments` is unchanged: these are ordinary `IDENTIFIER` arguments interpreted by conversio, not new `baseType` productions.
- Type arguments admit the hole forms: `lista<∪>` infers a heterogeneous element union and `tabula<K, ∪>` a heterogeneous value union; `lista<_>` keeps the monomorphic single-inhabitant hole.
- Explicit generic call-site lists use the same `typeArguments` production: `id<_>(x)` is a type hole (equivalent to omitted `id(x)` for a one-param callee), and mixed lists such as `both<_, textus>(a, b)` are legal. Arity stays exact (`both<_>` is still one argument). `∪` in that list is rejected (`explicit_union_type_arg_unsupported`): a callee type param is a monomorphic witness slot.
- `labeledTypeArgument` is the optional label prefix on `元组` type arguments only (`元组<gx: f32, T>`; mixed labeled/unlabeled legal). A label in a non-`元组` list (`f<gx: T>(x)`, `lista<gx: T>`) is a parse error. Absence is the only unlabeled form; there is no `_: T` spelling. Keyword spellings are legal labels under the contextual law (`元组<常量: A>`).
- Labels are unique within one tuple type.
- The tuple type is spelled `元组<…>`, not `(K1, K2)`. Parentheses already
  mean grouping, function types, parameters, and calls. Every other compound
  type is `name<args>`, and tuple labels come from the same type-argument
  machinery.
- Labels are erased from type identity: `元组<gx: A, B> ≡ 元组<A, B>` for assignment, `≡`/`↦`, unify, and every emitter.
- Bracket index on a tuple requires a literal integer (`i[0]`); every element is reachable by position, labeled or not. Non-literal index expressions stay rejected. Positions are brackets only — no `.0`.
- Member-by-label (`i.gx`) requires that label to be present on the receiver's `元组` annotation.
- `元组` element slots admit `_` (monomorphic hole, solved element-wise from the single position witness) and reject `∪`. A wanted union element is declared with binary cup (`元组<f32, textus ∪ 空>`). `lista<∪>` / `tabula<K, ∪>` keep heterogeneous-union behavior. Labels compose with holes (`元组<loss: _, T>`).
- `ratio` type arguments require a label for every element, labels are unique, `_` is admitted as a monomorphic element hole, and `∪` is rejected in an element slot. A `ratio` has no positional or bracket access, and it has no structural equivalence with another ratio or a genus; fields are accessed by label only.
- Arrays are written `lista<T>` (unbounded, shipped). Postfix `T[]` is not accepted. `lista<T, N>` is the shipped bounded form; see Generic Collections.
- `借自`/`传入` mark ownership (borrow/mut-borrow) on the immediately following union member. Parenthesize when grouping must be explicit.
- Two hole kinds share the `holeType` production. `_` is the monomorphic hole ("infer exactly one inhabitant type"); the standalone `∪` is the union hole ("infer a finite multi-member union"). Both are legal wherever a base type is: bindings, returns, params, fields, and type arguments (`lista<∪>`, `tabula<K, ∪>`, `→ ∪`).
- **Lone-`∪` rule:** a `∪` hole consumes the whole type expression — any following `∪` is a parse error (`A ∪ ∪`, `∪ B` rejected, issue `unexpected_cup_after_union_hole`). `_` keeps today's behavior and may still appear as a binary-cup member (`_ ∪ B`).
- **Binary-cup disambiguation:** `∪` between two non-hole types remains the inline value-union operator (`A ∪ B`, nullable `T ∪ 空`); the hole reading applies only when `∪` stands alone in a base-type position.
- Inline union `T ∪ U` (cup) for ad-hoc value unions; `T ∪ 空` is the canonical nullable type form (lowers to Option<T>).
- Inline intersection `T ∩ U` (cap) is the nominal type intersection: `type Reversible = Readable ∩ Seekable` names the conjunction, and the implements clause accepts `∩` as the same separator as the comma (`class A implements Readable ∩ Seekable` ≡ the comma list). `∩` binds tighter than `∪` (`A ∩ B ∪ C` is `(A ∩ B) ∪ C`); nested intersections flatten like unions. Intersection operands are nominal-only (interfaces/structs; aliases resolve through) — primitive operands are rejected at lowering. Implements slots admit `∩` only: `∪` or a hole in an implements position is a parse error (disjunctive conformance is not a checkable contract).
- Signature clauses stay explicit: `_` and a standalone `∪` are rejected in return (`→ _`) and error-channel (`⇥ _`) positions; both holes stay legal in local binding slots (`const _ v`, `const ∪ v`).
- Unions are parsed as a flat member list; duplicates and `空`-only cases are diagnosed in semantic lowering.
- `可选` is a declaration marker (post-name on params/fields), never a prefix on types.
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
| `textus<N>` | shipped; bounded Unicode string; `N` is a `维度` / `NATURAL` capacity, not a width marker. `textus<_>` is the capacity hole (infer `N`). |
| `ascii`    | ASCII-only string |
| `ascii<N>` | shipped; bounded ASCII string; `N` is a `维度` / `NATURAL` capacity, not a width marker. `ascii<_>` is the capacity hole (infer `N`). |
| `littera`  | en `char`; one Unicode scalar value (D10.1–10.2): a 4-byte value that never allocates (Rust `char`, Go `rune`). Element of `textus` / `ascii` iteration and of `textus[i]` / `ascii[i]` indexing. Grapheme clusters are norma library work, not this type. |
| `forma`    | captured template + params |
| `numerus`  | integer (default `i64`) |
| `modulus<W>` | unsigned modular word; arithmetic wraps modulo 2^W |
| `fractus`  | float (default `f64`) |
| `bivalens` | boolean |
| `空`    | null |
| `vacuum`   | void |
| `numquam`  | never |
| `ignotum`  | unknown |
| `octeti`   | bytes |
| `octeti<N>` | shipped; bounded byte buffer; `N` is a `维度` / `NATURAL` capacity, not a width marker. `octeti<_>` is the capacity hole (infer `N`). |
| `octetus`  | en `byte`; an exact alias of `numerus<u8>` (D10.4) — arithmetic and `0x0A` comparisons use it directly. Fixed-width; rejects applied parameters. |

Bare `textus` / `ascii` / `octeti` remain the unbounded productions. The
shipped forms `textus<N>`, `ascii<N>`, and `octeti<N>` take
one `维度` / `NATURAL` applied argument. That `N` is capacity, not a
width marker and not a language-wide default. `_` in that slot (`ascii<_>`,
`textus<_>`, `octeti<_>`, `lista<T, _>`) is a capacity hole: the form stays
bounded, and `N` is inferred from a same-family bounded witness. Bare
`ascii` is not a hole.

**`octeti ≡ lista<octetus>` is a type-identity fact (D10.4), not mutual
assignability**: the two names denote the same type for checking, `↦`, and
every emitter, while `octeti` keeps its byte-buffer runtime representation
(no element-boxing regression). `ascii<1>` is an ordinary ASCII string of
length one (the type of `'x'`), not a separate character type; it widens
implicitly `ascii<1> → littera → textus` (D10.5), so `s[i] ≡ '\n'` keeps
working across the chain.

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
| `lista<T, N>`  | shipped; bounded array; `N` is a `维度` / `NATURAL` capacity, not a width marker. `lista<T, _>` is the capacity hole (infer `N`). |
| `queue<T>`     | shipped; unbounded FIFO queue |
| `queue<T, N>`  | shipped; bounded FIFO queue; `N` is a `维度` / `NATURAL` capacity, not a width marker. `queue<T, _>` is the capacity hole (infer `N`). |
| `stack<T>`     | shipped; unbounded LIFO stack |
| `stack<T, N>`  | shipped; bounded LIFO stack; `N` is a `维度` / `NATURAL` capacity, not a width marker. `stack<T, _>` is the capacity hole (infer `N`). |
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

Value unions use inline `T ∪ U` (nullable: `T ∪ 空`). The standalone `∪` hole infers a multi-member union; `_` infers a single inhabitant (see `docs/design/type-hole-union.md`). Tagged unions use `判别`.
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


- `如果` = if, `否则如果` = else-if, `否则` = else
- `c ✓ a ✗ b` is the one value conditional: `a` when `c` holds, else `b`.
  `✓` (U+2713 CHECK MARK) and `✗` (U+2717 BALLOT X) are the same in every
  locale and have no word twin. It is one level only: a `✓ ✗` inside the
  condition or either branch is rejected (`conditional_nested`); choose among
  more values with a function whose `如果` arms each `返回`. The branches narrow
  exactly like `如果` branches (after `r 是 numerus`, `r` is `numerus` in the
  `✓` branch).
- `c ? a : b` and `c sic a 否则 b` (en `c yields a else b`) were removed and
  are rejected with a migration diagnostic; write `c ✓ a ✗ b`. `sic` stays a
  reserved word only to carry that diagnostic. The look-alikes `✔` and `✘` are
  rejected with a "did you mean" hint.
- `则` for one-statement bodies, including `则 返回`, `则 抛错`, `则 崩溃`, and `则 静默` (`∴` is not accepted here)
- `静默` for explicit no-op (from musical notation: "it is silent")

### Loops


- `当` = while
- `遍历 取自...常量`/`遍历 取自...变量` = for-of (values)
- `遍历 借自...常量`/`遍历 借自...变量` = for-in (keys)
- `遍历 范围 range 常量/变量 i` = range iteration (e.g. `遍历 范围 0‥10 步 2 常量 i { 显示 i }`; `步` belongs to the range expression)

**Iteration order.** A type whose order is part of its value iterates in that
order. `lista` iterates by index. `textus` iterates its characters in order.
`tensor`, `vector`, and `matrix` iterate by index, outer axis first
(row-major). Two equal values always iterate identically.

`copia` and `tabula` iterate in unspecified order. The order is not promised
and not deliberately random; backends may differ. When order matters, sort
explicitly. `≡` on these types stays structural and does not depend on order.
A map or set that promises an order is a separate library type, not a mode of
`tabula` or `copia`.

There is no iteration interface. `遍历 取自` works on the built-in iterable
types and on cursors. A user type that should be iterable exposes an ordinary
method that returns a cursor (`遍历 取自 arbor.nodi() 常量 n`); nothing is
called implicitly.

### Switch/Match

`匹配` is a statement, not an expression. A value chosen by a match comes
from a function whose arms each `返回`. The compiler checks exhaustiveness
and definite return, and the function can be tested on its own.

Coverage is checked as a pattern matrix. Each scrutinee has a space: the
variants of an `枚举` or `判别`, the members of a union, and `bivalens`
as the closed set `{真, 假}`. A match over several scrutinees is
checked over their product, so `匹配 a, b` over two `bivalens` values
needs all four combinations or a `默认`. A missing variant or combination is
an error that names one uncovered case. Open types (`numerus`, `textus`, …)
are complete only with a catch-all arm. When coverage cannot be computed for a
pattern kind, the compiler warns that it was not checked; it is never silent.
`选择` keeps its switch meaning: over an open domain, a missing `默认` is
an implicit no-op default, while a closed domain is checked.

### Pattern Matching

Patterns are flat. A `情况` arm names one variant and binds its fields, or names one literal
value; it does not match inside those fields. Nested patterns are left out for
simplicity, not because they cannot be checked: a `匹配` inside an arm is
two flat exhaustive switches.

A negative number pattern is written with a leading minus (`情况 -1`,
`情况 -∞`). The lexer never signs a number, so the pattern claims the sign;
`-` before anything else is not pattern syntax.

There are no range patterns (`情况 1‥5`). Test the range with `如果` inside the
arm.

A NaN pattern is rejected. NaN never equals itself, so it could never match;
test for NaN with `如果` instead.

### Guards

Match arms have no guards. `匹配` is one arm per variant, and a guard
would split one variant's logic across several arms. Nest a `如果` in the arm
instead.

### Destructuring Extraction

Destructuring is flat. A nested pattern such as `[[a, b], c]` is rejected;
destructure the outer value, then the inner one on another line.

Parameters are not destructured. A pattern in a parameter slot would hide the
parameter's type from a type-first signature. Destructure in the body.

### Control Transfer

`中断` and `继续` take no label. They apply to the nearest enclosing loop.
A nested search that needs an early exit from an outer loop becomes a
function that `返回`s.

- `等返` awaits a compatible promise and returns its success value from a
  `异步` function.
- `等弃` awaits a compatible promise to completion and discards any success
  value.
- `让出` is statement-initial yield from `流` / `异流`; it is not an
  expression-form await.

---

## Error Handling


- `捕获` attaches to the structured forms whose productions name `catchClause`: conditional arms, `当`, `遍历`, `选择`, and `执行`. It does not attach to arbitrary bare blocks.
- Use the explicit do block when a standalone block needs a handler: `执行 { ... } 捕获 err { ... }`.
- `抛错` = throw (recoverable), `崩溃` = panic (fatal).
- A same-line `如果 <expr>` guard on `抛错` and `崩溃` is line-sensitive parser sugar: `抛错 val 如果 cond` desugars to `如果 cond { 抛错 val }` at parse time. Its canonical, compression-safe spelling is the expanded `如果` block. A source compressor must expand this sugar before removing line breaks; the guarded shorthand remains under language review.
- `断言` is a runtime invariant check. It desugars conceptually to `崩溃 "msg" 如果 !cond`, with the positive condition kept in source form and the inversion applied during lowering. The optional particle is `崩溃` (en `panic`): `断言 cond 崩溃 msg` / `assert cond panic msg`. Bare `断言 cond` stays legal. An `断言` failure is fatal and uncatchable by `捕获` (it lowers to a panic, not a `Result`-channel error); in test context the harness isolates each `测试` so a failed assertion ends that test without ending the suite.
- `需求` is the recoverable require statement (en surface `require … throw …`), the typed-error-channel twin of `断言`. `需求 cond 抛错 err` desugars to `如果 非 (cond) { 抛错 err }` at lowering; the thrown value enters the function's `⇥ E` channel and is catchable by `捕获`/`执行`, unlike `断言` (fatal). A `需求` statement in a `⇥`-less function is a compile error, same as `抛错`. The particle is `抛错` (en `throw`) and is required.

- `拒绝` is the reject statement (en surface `reject … throw …`), the boolean opposite of `需求`. `拒绝 cond 抛错 err` desugars to `如果 (cond) { 抛错 err }` at lowering — it throws when the condition holds, where `需求` throws when it fails. The thrown value enters the function's `⇥ E` channel and is catchable by `捕获`/`执行`. A `拒绝` statement in a `⇥`-less function is a compile error, same as `抛错`. The particle is `抛错` (en `throw`) and is required.
- `@ conversio` (en `@ conversion`) on a top-level `函数` declares an admitted error conversion: the parameter's type is the source error, the return type is the destination, and the compiler enrolls that ordered pair so a propagating `⇥ E` failure converts at the boundary instead of needing a per-caller wrapper. The marker is bare and the conversion is an ordinary function outside any union body; only a direct (source, destination) row is admitted — a missing row fails closed and is never auto-composed into a chain. The earlier union-arm form (the marker carrying a payload inside a `判别` body) is retracted.
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
rewritten to `←` or `↦`. Typed `常量`/`变量` initializers accept `↤`
(convert to the written type, then initialize); `常量 _`, `设`, and untyped
destructuring have no concrete destination and are rejected.

`是` and `非 是` inspect an existing value; they never convert it. Core type
spellings on the right perform runtime variant/type tests, while `空`,
`真`, `假`, and ordinary value expressions use the value-test path. Radix
currently recognizes type targets through a fixed core-type vocabulary. Extending
that recognition to arbitrary declared types is a separate language decision.
Use `≡` / `≠` (or `≢`) for structural value equality, `≅` / `≇` for promoted exact equality (same value after numeric widths join), `≈` / `≉` for fuzzy equality (tolerance match with Python-isclose defaults: rel_tol 1e-09, abs_tol 0.0), and `↦` for runtime conversion.

Retired predicate keywords are not prefix unary syntax. Use `expr 是 真`,
`expr 是 假`, `expr 是 空`, `expr 非 是 空`, `expr ≺ 0`, or
`expr ≻ 0`.

The legacy ASCII spellings `<` and `>` are not productions of this grammar — both remain generic delimiters — though the shipped parser still accepts them as comparisons during the glyph migration; prefer the canonical `≺` and `≻`.

Ordering comparisons (`≺`, `≻`, `≤`, `≥`) between two `textus` values compare
the whole strings in Unicode code-point order. They do not use locale
collation.

**Format operator (`¶`, U+00B6, D2.1–D2.5, D2.7):** `value ¶ "spec"` renders a
built-in value as `textus`. It pairs with `§`: `§` marks *where* a value
goes, `¶` says *how* it is shown — `"Summa: §"(pretium ¶ ".2")`. `¶` is an
**operator, not an arrow**, because it cannot fail (D2.4): it is a pure
computation like `+` or `≡`, with no state change, no control flow, and no
failure path. A malformed spec, or a spec that does not fit the left side's
type, is a compile error (pass 1 checks only that a literal is present; pass
2 validates the spec against the left side's type) — a computed spec is
rejected. `¶` binds looser than arithmetic and tighter than comparison
(`a + b ¶ ".2" ≤ 100 ¶ ".2"` is `(a + b ¶ ".2") ≤ (100 ¶ ".2")`) and does not
chain (a second `¶` is `format_chained`). `¶` stays closed to built-in types
(numbers, `textus`, `instans`); a user type formats through an ordinary
function. Holes (`§`, `§N`, and the named form) stay pure substitution and
gain no spec slot.

The spec vocabulary is one fixed pattern for every type, each type accepting
only the parts that make sense: `[fill][align][sign][0][width][.precision][kind]`.

- **Numbers:** `.2` precision (`12.50`; integers pad too, so `42 ¶ ".2"` is
  `42.00` and integers/floats line up in one column); width (`"5"` →
  `   42`, right-aligned by default); `0` zero-pad (`"05"` → `00042`); `<`
  `>` `^` align, with an optional fill character before the align (`"*^7"` →
  `**42***`); `+` always shows the sign; kinds `x` `b` `o` (hex, binary,
  octal) and `e` (scientific); combinable (`"08x"`).
- **`textus`:** fill, align, width, and `.N` — **truncate to N characters**
  (`littera`), following C `%.3s` / Python `{:.3}` / Rust `{:.3}`
  (`"Aurelia" ¶ ".3"` = `Aur`). `.N` is precision on numbers, maximum length
  on text — the same split those languages use.
- **`instans`:** named presets only (`iso`, `date`, `time`); no
  strftime-style patterns (norma work, if ever).
- **Left out on purpose:** thousands separators (country-aware, so library
  work, not this operator) and computed specs (D2.2).
- **Split from `↦`:** `↦ ascii<N, Hex>` is exact conversion — fixed width,
  fails if the value does not fit; `¶` is display — width is a minimum that
  grows to fit, and never fails.
- **No word twin:** `¶` is the same glyph in every locale, like `✓ ✗`.
- Decimal types (`d32`/`d64`) are not yet supported by `¶` (display pending a
  scale-preserving representation; held).

**Edge-case outputs (D2.7):** `NaN` / `∞` / `-∞` print as `NaN`, `∞`, `-∞`
(precision does not apply); a negative number in hex/bin/oct prints sign plus
digits (`-42 ¶ "x"` = `-2a`), not two's complement (`↦ ascii<N, Hex>` stays
the strict tool and rejects negatives); `textus` width counts `littera`
(characters), not screen columns (an emoji with a skin-tone modifier counts as
2; screen-width alignment is library work); `instans` outside years 0–9999
with `"iso"` uses ISO 8601's extended form (`+10000-01-01`).

**Static type ascription (`∷` / verte):**

The `∷` glyph (U+2237, "proportion") explicitly ascribes a target type to an expression. Use it when the source expression already exists and the compiler needs a static target shape:

- Primitive/alias → cast (no runtime effect): `data ∷ textus` → TypeScript: `(data as string)`
- Built-in collection → target-shaped collection value: `[1, 2, 3] ∷ lista<numerus>`
- Variant expression → enum/interface target ascription: `构造 Click { x = 10 } ∷ Event`

Prefer typed construction for ordinary `类` values and `vacua` for ordinary empty collection values:

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
conversion fails: `常量 numerus n ← "abc" ↦ numerus ⊥ 0`. It is written
immediately after the conversio target (`↦ T ⊥ default`) or after the value of
a `↤` assignment.

- `⊥` catches only the `⇥` error channel. It never catches `崩溃` or traps
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

Using `兜底` as a conversio default is rejected with a migration diagnostic. `兜底` is local nullable elimination only (`x 兜底 y`, parameter defaults) — not logical `或`. A parenthesized conversio result may still combine with `兜底` as ordinary defaulting.

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
`格式化("...", args...)`.

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
`格式化("§ world", "salve")` form.

This lowers to the compiler's `格式化("...", args...)` form. Use the string-template form in ordinary source; reserve `格式化(...)` for explicit desugaring examples and compiler-facing documentation.

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

Text slices accept the full range form, including `步`.

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
is `accipe` sugar and returns `T ∪ 空`. For nullable list access, use
`xs.accipe(i) → T ∪ 空` with `兜底`.

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

Reads return `T ∪ 空`, matching `accipe`; use `兜底` or another ordinary
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
index on an octeti literal (`|借自 调用 be ef|[0‥5]`) is a structured reject.
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

**Capture boundary (`陷阱`):** `陷阱 { … }` (en `trap`) is an expression
that runs its block and reifies the error channel into a value. The block's
trailing expression is the success value; the result type is the union of the
success type and every error type that can escape the body (failable calls
and `抛错` payloads), so a failure inside the block becomes a value instead of
propagating. When the success and error types coincide the union cannot tell
them apart, and the form is rejected. `陷阱` claims its spelling only in expression-primary position
directly followed by `{`, so `陷阱(…)` calls and bare identifier uses keep
their ordinary meaning. No `捕获` clause, `当` tail, or early-success form
attaches to it — those belong to `执行`.

`vacua` is a contextual empty-collection marker (identifier form, not a reserved keyword).
Use it with an explicit collection type: `常量 lista<numerus> xs ← vacua` or `常量 tensor<fractus<f32>, []> t ← vacua`.


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
Construction literals do not spread: `展开` is not a field initializer
(`Genus { 展开 other }` is rejected). `展开` stays for list literals and
call arguments. Copy-with-changes is planned as `Genus { … } 取自 source`.

- Ratio construction uses `ratioType '{' fieldInit (',' fieldInit)* '}'` through `typedConstructor`; every field initializer is named, and the resulting fields remain accessible only by label.

### Special Expressions


`首个匹配(source, 其中 binder { predicate })` is the dedicated first-match
selection expression over a statically bounded source: the predicate is
evaluated for every candidate lane (total evaluation, no early exit), the
first live match is selected, and a no-match or empty source yields `空`
(the result type is `T ∪ 空`). The `其中` predicate tail is owned by this
head and never shares the reduce/scan `常量`/`变量` binder tail.
`首个匹配` claims only the expression-head position immediately followed
by `(`; elsewhere the spelling stays an ordinary identifier. An optional
`于` coordinate clause binds per-axis indices as in `遍历 取自`.

`格式化` and `读取`/`行` are builtin claims that resolve to a user binding
when the surface spelling is bound in scope (parameter, local, function, or any
in-scope definition); otherwise they are the builtin. The same binding-wins rule
applies to `格式化`'s paren-claimed form and to the `vacua` empty-collection
marker: builtin claims are defaults, not reservations.

`构造` variant construction accepts a qualified variant path
(`构造 pkg.Bonum { … }`), so an imported union's variants construct through
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


The scribe family (`显示`/`查看`/`警告`/`写入` — en `print`/`debug`/`warn`/`write`)
claims the statement-initial position only when **not** immediately followed by
`(`. `显示 expr` is the output statement; a statement-initial `显示(...)` is an
expression statement whose callee is the identifier `显示` — a user function
call, never the intrinsic.

- `显示` = neutral diagnostic note, `查看` = debug/inspect, `警告` = warn
- `写入` is a diagnostic channel spelling; use current stdlib methods for real output

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


- `入口` = sync entry, `异步入口` = async entry.
- `参数` binds parsed command-line arguments; `退出` supplies the process exit expression. Their order is fixed by `entryHeader`.

---

## Testing

`测试` modifiers include `预期失败` (en `expect_failure`): the case passes only
when its body escapes through the error channel, and a case that completes
cleanly fails (strict expected-failure). The other modifiers are `跳过`,
`预期`, `仅`, `仅于`, `标签`, `时限`, `计量`, `重复`, and
`易碎`.

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

Expression-form `调用` is the only supported `调用` surface. Legacy typed
`调用 "route" (args) → T { }` and statement-level stream blocks
`调用 'route' { meus/tuus … }` are rejected at parse time.

The active `adExpr` production is defined under **Primary Expressions**. Its
ordinary postfix `conversio` materializes the resulting conversation handle.

- Route: `ASCII_STRING` (`'仅:读取'`), not double-quoted `STRING`.
- Opener: optional single `expression` → Request `data` as `valor`.
- **Expression `调用`**: blockless; evaluates to a `sermo` conversation handle.
  Use postfix `↦ T` (materialization), assign to `sermo`, or open live directional
  views: `s.meus<T>()` (outbound `da` / `fini`) and `s.tuus<T>()` (inbound
  `accipe` / `cursor` / `exhauri` / `fini`). Iterate inbound content frames with
  `s.tuus<T>().cursor()`, not direct `遍历 取自 s.tuus<T>()`.
- **Removed (parse error):** legacy typed `调用 "route"` and block `meus`/`tuus` arms.
- Types: compiler-owned `scrinium`, `status`; opaque `sermo` conversation handle.
- English reader spellings: `sermo` is `channel`, `scrinium` is `frame`, and
  the views `meus<T>` / `tuus<T>` are `send<T>` / `recv<T>` (`s.send<T>()`,
  `s.recv<T>()`). The Latin spellings are unchanged.
- `sermo ↦ T` materializes inbound frames into one value of type `T` using
  the type-directed collector for `T`.

See [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md).

**Concurrency is conversations.** Concurrent work is an `调用` conversation with
a route. There is no separate spawn, thread, or lock primitive family.
Handlers that share nothing and exchange only frames are free of data races by
construction.

Every `调用` pays the conversation cost. It goes through the router with frames,
even when both ends are local; there is no hidden fast path. The light path is
an ordinary function call, and a swappable light path is a contract passed as a
parameter.

`调用` is the effect boundary. Effects reach the outside world through `调用`
conversations, which stay portable across backends.

`@ 调用` on a function is the compiler-owned serving half of `调用`: it lets
Faber code answer a route. `@ 调用 'prefix:name'` (en `@ call`) on a top-level,
non-generic, bodied `函数` serves that route.

- Routes are exact: `prefix:name` or `prefix/name`. Pattern routes are deferred.
- The handler takes zero or one parameter; the one parameter is the opener
  value of the calling `调用`.
- A handler serves one route. Reserved prefixes (such as `runtime:`) and
  builtin routes cannot be served.
- Routes form one program-wide static table built from every module in the
  program, including imported libraries. Two definitions of the same route are
  a compile error.
- Parsing, checking, and the route table exist today; serving is implemented
  on Rust, Go, and TypeScript. The MIR runner does not serve `@ 调用` yet.

Web, HTTP, and framework routing stay libraries (see Annotations).

---

## Collection Operations

The former `范围` collection pipeline DSL is retired. Collection filtering,
slicing, and aggregation are expressed through ordinary
`textus`/`lista`/`tabula`/`copia` methods and closures instead of a
grammar-level query expression. `textus`, `numerus`, `fractus`, `lista<T>`,
`tabula<K,V>`, and `copia<T>` are compiler-owned core types; their method
surfaces are not Norma declarations.

`prima` and `ultima` are ordinary method names, not transform keywords. `其中` is
the owned predicate-tail introducer of the `首个匹配` first-match expression
(see Special Expressions), not collection syntax.

`ordina(key)` (D1.7) sorts a `lista` in place by a key selector; `ordinata(key)`
returns a new sorted `lista` and leaves the receiver untouched. The zero-argument
forms `ordina()` / `ordinata()` sort by the element's natural order. Both are a
**stable** sort. The key selector's result must be a number or `textus`; other
key types are rejected.

`取自` is used for iteration (`遍历 取自 items 常量 x`) and imports (`导入 取自 "path"`).

### Iteration coordinates (`于`)

The optional `于` coordinate clause binds per-axis indices for an `遍历 取自`
loop: `遍历 取自 grid 于 [r, c] 常量 cell { … }`. The en reader spelling is
"at" — `遍历 取自 grid 于 [r, c]` reads as iterating `grid` at coordinates
`[r, c]`.

- **First bound name = outer axis.** The first identifier in the bracket group
  walks the first (outermost) axis; later names walk successively inner axes.
- **Bracket-group convention.** The coordinate group follows the tensor
  bracket-index convention `grid[[r, c]]`: one bracketed group, comma-separated
  coordinate names, in axis order.
- **Arity == rank.** The number of coordinate names must equal the tensor rank.
  Fewer or more names is a structured reject (arity mismatch).
- **`于` requires `取自`.** The coordinate clause is only valid on `遍历 取自`
  (element iteration); `遍历 范围` range loops and `遍历 借自` reject it.
- The coordinate names are immutable index bindings scoped to the loop body,
  distinct from the element binder that follows the clause.

---

## Fac Block


- `执行 { ... }` is the explicit `do` block and executes its body once.
- `执行 { ... } 当 condition` is the post-test loop form; postfix `当` attaches only to `执行`, not arbitrary preceding blocks.
- `捕获` is an attachment shared by several structured forms, not a semantic mode owned by `执行`. A plain `执行` is often used when an otherwise unattached block needs a local handler: `执行 { ... } 捕获 err { ... }`.

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

1. **Type-first parameters**: `函数 f(numerus x)` NOT `函数 f(x: numerus)`
2. **Type-first declarations**: `常量 textus name` NOT `常量 name: textus`
3. **Iteration loops**: `遍历 取自/借自 collection 常量/变量 item { }` or `遍历 范围 range 常量/变量 item { }` (verb-first, source, then binding)
4. **Parentheses around conditions are valid but not idiomatic**: prefer `如果 x ≻ 0 { }` or `如果 flag 是 真 { }` over `如果 (x ≻ 0) { }`
5. **Scribe-family keywords claim statement-initial position only when not followed by `(`** — `显示 x` is the output statement; a statement-initial `显示(x)` is a call to the identifier `显示`
