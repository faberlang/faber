# Faber Language Specification

This file is generated from `grammar/sources/source.fg`, its `sidecar.en.toml` and its `prose.en.md`, and
`grammar/sources/glossary.zh-Hant.toml`; edit those, not this file (`scripta/check-grammar` fails on drift).
Production IDs are the grammar's stable snake_case spine and their
anchors are derived from those IDs.

## Grammar {#grammar}

The grammar below is the identity rendering of the validated source. Normative detail is kept in `prose.en.md` beside this file and rendered as documentation; the source remains the syntax authority.

```ebnf
# [001] fab_file
fab_file ::= frontmatter? program
# [002] frontmatter
frontmatter ::= FRONTMATTER_DELIMITER NEWLINE TOML_LINES FRONTMATTER_DELIMITER NEWLINE?
# [003] program
program ::= regio_decl? statement*
# [004] regio_decl
regio_decl ::= '模組' IDENTIFIER
# [005] statement
statement ::= annotation* statement_core | ad_handler_decl | nondum_functio_decl
# [006] ad_handler_decl
ad_handler_decl ::= annotation* ad_annotation annotation* functio_decl
# [007] nondum_functio_decl
nondum_functio_decl ::= annotation* nondum_annotation annotation* '函式' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# [008] statement_core
statement_core ::= importa_decl | binding_decl | functio_decl | genus_decl | implendum_decl | typus_decl | ordo_decl | discretio_decl | schema_decl | si_stmt | dum_stmt | itera_stmt | elige_stmt | discerne_stmt | custodi_stmt | fac_stmt | redde_stmt | reddet_stmt | tacebit_stmt | cede_stmt | rumpe_stmt | perge_stmt | tacet_stmt | iace_stmt | adfirma_stmt | requirit_stmt | reice_stmt | nota_stmt | incipit_stmt | incipiet_stmt | ex_stmt | probandum_decl | proba_stmt | block_stmt | inc_dec_stmt | expr_stmt
# [009] binding_decl
binding_decl ::= fixum_decl | sit_decl | ref_decl | array_destruct | object_destruct | figendum_decl
# [010] expr_stmt
expr_stmt ::= expression
# [011] block_stmt
block_stmt ::= '{' statement* '}'
# [012] const_init
const_init ::= insere_expr | expression
# [013] insere_expr
insere_expr ::= '嵌入' STRING
# [014] fixum_decl
fixum_decl ::= ('定值' type_annotation IDENTIFIER '=' const_init) | ('變值' type_annotation IDENTIFIER (('←' value_marker? expression) | ('↤' assignment inline_default?) | ('↢' expression))?)
# [015] figendum_decl
figendum_decl ::= ('等定' | '等變') type_annotation IDENTIFIER '←' expression
# [016] sit_decl
sit_decl ::= '設為' type_annotation IDENTIFIER (('←' value_marker? expression) | ('↤' assignment inline_default?) | ('↢' expression))?
# [017] ref_decl
ref_decl ::= '從' concrete_type IDENTIFIER '←' expression
# [018] array_destruct
array_destruct ::= ('設為' | '變值') array_pattern '←' expression
# [019] object_destruct
object_destruct ::= ('設為' | '變值') object_pattern '←' expression
# [020] functio_decl
functio_decl ::= '函式' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [021] param_list
param_list ::= (parameter (',' parameter)*)?
# [022] generic_params
generic_params ::= '<' (type_param_list (',' size_param_list)? | size_param_list) '>'
# [023] type_param_list
type_param_list ::= generic_param (',' generic_param)*
# [024] size_param_list
size_param_list ::= size_param (',' size_param)*
# [025] generic_param
generic_param ::= IDENTIFIER generic_bound? generic_type_default?
# [026] size_param
size_param ::= '尺寸' IDENTIFIER generic_size_default?
# [027] generic_bound
generic_bound ::= '實作' contract_ref ('∩' contract_ref)*
# [028] contract_ref
contract_ref ::= IDENTIFIER ('<' type_annotation (',' type_annotation)* '>')?
# [029] generic_type_default
generic_type_default ::= '=' type_annotation
# [030] generic_size_default
generic_size_default ::= '=' NATURAL
# [031] call_type_args
call_type_args ::= '<' call_type_arg (',' call_type_arg)* '>'
# [032] call_type_arg
call_type_arg ::= type_annotation | call_size_arg
# [033] call_size_arg
call_size_arg ::= NATURAL dimension_tail? | IDENTIFIER dimension_tail | spelled_lift | '[' figura_list? ']'
# [034] dimension_tail
dimension_tail ::= ('/' NATURAL)+ (('+' | '-') NATURAL)* | (('+' | '-') NATURAL)+
# [035] parameter
parameter ::= '其餘'? '軸'? type_annotation IDENTIFIER '可選'? ('作為' IDENTIFIER)? ('或取' expression)?
# [036] func_modifier
func_modifier ::= '引數' IDENTIFIER | '錯誤' IDENTIFIER | '出口' (IDENTIFIER | NATURAL) | '不變' | '可拋' | '可選項' IDENTIFIER
# [037] callable_posture
callable_posture ::= '異步' | '流' | '異流'
# [038] return_clause
return_clause ::= '→' type_annotation
# [039] alternate_exit_clause
alternate_exit_clause ::= '⇥' type_annotation
# [040] ergo_joint
ergo_joint ::= '則'
# [041] clausura_joint
clausura_joint ::= '∴'
# [042] clausura_expr
clausura_expr ::= compact_clausura_expr | clausura_legacy_expr
# [043] compact_clausura_expr
compact_clausura_expr ::= clausura_signature clausura_joint (expression | fac_block)
# [044] clausura_signature
clausura_signature ::= (clausura_param | '(' clausura_params? ')') return_clause? alternate_exit_clause?
# [045] fac_block
fac_block ::= '執行' fac_modifier? block_stmt cape_clause?
# [046] clausura_legacy_expr
clausura_legacy_expr ::= '閉包' clausura_params? ('→' type_annotation)? (':' expression | block_stmt)
# [047] clausura_params
clausura_params ::= clausura_param (',' clausura_param)*
# [048] clausura_param
clausura_param ::= type_annotation IDENTIFIER
# [049] genus_decl
genus_decl ::= '類型' IDENTIFIER generic_params? ('實作' contract_ref ((',' | '∩') contract_ref)*)? '{' genus_member* '}'
# [050] genus_member
genus_member ::= annotation* (genus_field_decl | functio_method_decl) | nondum_method_decl
# [051] nondum_method_decl
nondum_method_decl ::= annotation* nondum_annotation annotation* '函式' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# [052] genus_field_decl
genus_field_decl ::= (('設為' | '變值') type_annotation IDENTIFIER '可選'? ('=' const_init)?) | genus_const_decl
# [053] genus_const_decl
genus_const_decl ::= '定值' type_annotation IDENTIFIER '=' const_init
# [054] field_decl
field_decl ::= (('設為' | '變值')? type_annotation IDENTIFIER '可選'? ('=' const_init)?) | genus_const_decl
# [055] functio_method_decl
functio_method_decl ::= '函式' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [056] annotation
annotation ::= nucleum_annotation | radix_annotation | braced_annotation | annotation_sugar
# [057] annotation_name
annotation_name ::= ANNOTATION_NAME
# [058] braced_annotation
braced_annotation ::= '@' annotation_name '{' annotation_field_list? '}'
# [059] annotation_field_list
annotation_field_list ::= annotation_field (',' annotation_field)*
# [060] annotation_field
annotation_field ::= ANNOTATION_FIELD_NAME '=' (expression | concrete_type)
# [061] annotation_sugar
annotation_sugar ::= '@' annotation_name NON_NEWLINE_TOKEN* NEWLINE
# [062] nucleum_annotation
nucleum_annotation ::= nucleum_sugar | nucleum_braced
# [063] nucleum_sugar
nucleum_sugar ::= '@' '內核' nucleum_modifier? NEWLINE
# [064] nucleum_braced
nucleum_braced ::= '@' '內核' '{' nucleum_field_list? '}'
# [065] nucleum_modifier
nucleum_modifier ::= '片段' | nucleum_tier
# [066] nucleum_tier
nucleum_tier ::= '寬鬆' | '不安全'
# [067] nucleum_field_list
nucleum_field_list ::= nucleum_field (',' nucleum_field)*
# [068] nucleum_field
nucleum_field ::= '片段' '=' ('真' | '假')
# [069] radix_annotation
radix_annotation ::= '@' 'radix' radix_directive NEWLINE
# [070] radix_directive
radix_directive ::= '車道' STRING | 'backward' STRING | 'contract' STRING | '型別' IDENTIFIER '傳入' concrete_type+
# [071] ad_annotation
ad_annotation ::= '@' '端點' ASCII_STRING NEWLINE
# [072] nondum_annotation
nondum_annotation ::= '@' '不穩定' NON_NEWLINE_TOKEN* NEWLINE | '@' '不穩定' '{' annotation_field_list? '}'
# [073] implendum_decl
implendum_decl ::= '待實作介面' IDENTIFIER generic_params? '{' implendum_method_decl* '}'
# [074] implendum_method_decl
implendum_method_decl ::= annotation* '函式' IDENTIFIER '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# [075] typus_decl
typus_decl ::= '型別' IDENTIFIER generic_params? '=' type_annotation
# [076] ordo_decl
ordo_decl ::= '列舉' IDENTIFIER '{' enum_member (',' enum_member)* '}'
# [077] enum_member
enum_member ::= IDENTIFIER ('=' ('-'? NUMBER | STRING))?
# [078] discretio_decl
discretio_decl ::= '分支聯集' IDENTIFIER generic_params? '{' union_fields? variant (',' variant)* '}'
# [079] union_fields
union_fields ::= annotation+ field_decl union_member*
# [080] union_member
union_member ::= annotation* field_decl
# [081] variant
variant ::= IDENTIFIER ('{' variant_fields '}')?
# [082] variant_fields
variant_fields ::= (type_annotation IDENTIFIER)*
# [083] schema_decl
schema_decl ::= '結構' IDENTIFIER '{' (schema_column (NEWLINE schema_column)*)? '}'
# [084] schema_column
schema_column ::= '欄位' type_annotation IDENTIFIER (':' IDENTIFIER)?
# [085] importa_decl
importa_decl ::= importa_record | importa_sugar
# [086] importa_record
importa_record ::= '匯入' '{' import_field_list '}'
# [087] import_field_list
import_field_list ::= import_field (',' import_field)*
# [088] import_field
import_field ::= ex_field | visibilitas_field | nomen_field | ut_field | omnia_field
# [089] ex_field
ex_field ::= '取自' '=' STRING
# [090] visibilitas_field
visibilitas_field ::= '可見性' '=' publica
# [091] nomen_field
nomen_field ::= '名稱' '=' IDENTIFIER
# [092] ut_field
ut_field ::= '作為' '=' IDENTIFIER
# [093] omnia_field
omnia_field ::= '全部' '=' IDENTIFIER
# [094] importa_sugar
importa_sugar ::= '匯入' ('取自' | import_target) STRING publica? (named_import | wildcard_import | selective_import)?
# [095] import_target
import_target ::= IDENTIFIER
# [096] publica
publica ::= '公開'
# [097] named_import
named_import ::= IDENTIFIER ('作為' IDENTIFIER)?
# [098] wildcard_import
wildcard_import ::= '*' '作為' IDENTIFIER
# [099] selective_import
selective_import ::= '定值' import_value_binding (',' import_value_binding)*
# [100] import_value_binding
import_value_binding ::= IDENTIFIER ('作為' IDENTIFIER)?
# [101] type_annotation
type_annotation ::= union_hole_type | concrete_type
# [102] concrete_type
concrete_type ::= intersection_type ('∪' intersection_type)*
# [103] union_hole_type
union_hole_type ::= ('從' | '變值' | '擁有' | '拷貝')? '∪'
# [104] intersection_type
intersection_type ::= owned_type ('∩' owned_type)*
# [105] owned_type
owned_type ::= ('從' | '變值' | '擁有' | '拷貝')? base_type
# [106] base_type
base_type ::= hole_type | function_type | width_type_sugar | ratio_type | failable_promissum_type | qualified_type type_arguments?
# [107] failable_promissum_type
failable_promissum_type ::= IDENTIFIER '<' type_annotation alternate_exit_clause '>'
# [108] ratio_type
ratio_type ::= 'ratio' '<' ('*' | labeled_type_argument (',' labeled_type_argument)*) '>'
# [109] hole_type
hole_type ::= '_'
# [110] qualified_type
qualified_type ::= type_head ('.' IDENTIFIER)*
# [111] type_head
type_head ::= IDENTIFIER | '模數'
# [112] type_arguments
type_arguments ::= '<' type_argument (',' type_argument)* '>'
# [113] type_argument
type_argument ::= labeled_type_argument | range_argument | sized_range_argument | spelled_lift | type_annotation | NATURAL | '[' figura_list? ']'
# [114] labeled_type_argument
labeled_type_argument ::= IDENTIFIER ':' type_annotation
# [115] range_argument
range_argument ::= range_bound range_glyph range_bound
# [116] sized_range_argument
sized_range_argument ::= '尺寸' range_argument
# [117] range_glyph
range_glyph ::= '‥' | '…'
# [118] range_bound
range_bound ::= '-'? NATURAL | '-'? '∞' | '尺寸'? IDENTIFIER
# [119] width_type_sugar
width_type_sugar ::= WIDTH_MARKER | LISTA_WIDTH_SUGAR | (TENSOR_WIDTH_SUGAR | SPARSA_WIDTH_SUGAR | VECTOR_WIDTH_SUGAR) shape_suffix? | MATRIX_WIDTH_SUGAR shape_suffix
# [120] shape_suffix
shape_suffix ::= '[' figura_list? ']'
# [121] figura
figura ::= spelled_lift | figura_product (('+' | '-') NATURAL)*
# [122] spelled_lift
spelled_lift ::= '尺寸' lift_extent
# [123] lift_extent
lift_extent ::= NATURAL | '-' NATURAL | '(' figura ')' | IDENTIFIER | '[' figura_list? ']'
# [124] figura_product
figura_product ::= figura_primary ('/' NATURAL)*
# [125] figura_primary
figura_primary ::= '_' | NATURAL | IDENTIFIER | '[' figura_list? ']' | '(' figura ')'
# [126] figura_list
figura_list ::= figura (',' figura)*
# [127] function_type
function_type ::= '(' type_list? ')' '→' type_annotation alternate_exit_clause?
# [128] type_list
type_list ::= type_annotation (',' type_annotation)*
# [129] si_stmt
si_stmt ::= '若' si_tail
# [130] si_tail
si_tail ::= expression arm ('否則若' si_tail | secus_clause)?
# [131] secus_clause
secus_clause ::= '否則' else_arm
# [132] arm
arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [133] else_arm
else_arm ::= (block_stmt | ergo_joint statement) cape_clause?
# [134] dum_stmt
dum_stmt ::= '當' expression (block_stmt | ergo_joint statement) cape_clause?
# [135] itera_stmt
itera_stmt ::= '遍歷' ('取自' expression (',' expression)* | '從' expression | '範圍' expression (',' expression)*) apud_clause? filum_clause? ('設為' | '變值') itera_binding (block_stmt | ergo_joint statement) cape_clause?
# [136] itera_binding
itera_binding ::= array_pattern | object_pattern | IDENTIFIER (',' IDENTIFIER)*
# [137] apud_clause
apud_clause ::= '在' '[' IDENTIFIER (',' IDENTIFIER)* ']'
# [138] elige_stmt
elige_stmt ::= '選擇' expression '{' casu_elige_clause* ceterum_clause? '}' cape_clause?
# [139] casu_elige_clause
casu_elige_clause ::= '分支' expression (block_stmt | ergo_joint statement)
# [140] ceterum_clause
ceterum_clause ::= '預設' (block_stmt | ergo_joint statement)
# [141] discerne_stmt
discerne_stmt ::= '比對' '全部'? discriminants '{' casu_variant_clause* ceterum_clause? '}'
# [142] discriminants
discriminants ::= subject_path ('且' subject_path)*
# [143] subject_path
subject_path ::= IDENTIFIER ('.' IDENTIFIER)*
# [144] casu_variant_clause
casu_variant_clause ::= '分支' patterns (block_stmt | ergo_joint statement)
# [145] patterns
patterns ::= pattern ('且' pattern)*
# [146] pattern
pattern ::= pattern_atom ('或' pattern_atom)*
# [147] pattern_atom
pattern_atom ::= '_' | negated_number | literal | type_pattern | (IDENTIFIER ut_pattern?)
# [148] negated_number
negated_number ::= '-' NUMBER
# [149] type_pattern
type_pattern ::= IDENTIFIER type_arguments? ut_pattern?
# [150] ut_pattern
ut_pattern ::= ('作為' IDENTIFIER) | (('設為' | '變值') pattern_binding (',' pattern_binding)*)
# [151] pattern_binding
pattern_binding ::= IDENTIFIER ('作為' IDENTIFIER)?
# [152] custodi_stmt
custodi_stmt ::= '守衛' '{' si_guard_clause+ '}'
# [153] si_guard_clause
si_guard_clause ::= '若' expression (block_stmt | ergo_joint statement)
# [154] ex_stmt
ex_stmt ::= '取自' expression ('設為' | '變值') extract_fields
# [155] extract_fields
extract_fields ::= extract_field (',' extract_field)* (',' ceteri_field)? | ceteri_field
# [156] extract_field
extract_field ::= IDENTIFIER ('作為' IDENTIFIER)?
# [157] ceteri_field
ceteri_field ::= '其餘' IDENTIFIER
# [158] redde_stmt
redde_stmt ::= '傳回' ('執行緒' expression? | expression?)
# [159] reddet_stmt
reddet_stmt ::= '等返' expression
# [160] tacebit_stmt
tacebit_stmt ::= '等棄' expression
# [161] cede_stmt
cede_stmt ::= '讓出' expression
# [162] rumpe_stmt
rumpe_stmt ::= '中斷'
# [163] perge_stmt
perge_stmt ::= '繼續'
# [164] tacet_stmt
tacet_stmt ::= '靜默'
# [165] iace_stmt
iace_stmt ::= iace_expr | iace_guarded_expr
# [166] iace_expr
iace_expr ::= ('拋出' | '崩潰') expression
# [167] iace_guarded_expr
iace_guarded_expr ::= ('拋出' | '崩潰') expression NO_NEWLINE '若' expression
# [168] cape_clause
cape_clause ::= '捕捉' IDENTIFIER block_stmt
# [169] adfirma_stmt
adfirma_stmt ::= '斷言' expression ('崩潰' expression)?
# [170] requirit_stmt
requirit_stmt ::= '需要' expression '拋出' expression
# [171] reice_stmt
reice_stmt ::= '拒絕' expression '拋出' expression
# [172] expression
expression ::= assignment
# [173] transfer
transfer ::= ternary ('⇇' ternary)?
# [174] sink_into
sink_into ::= transfer ('⇜' ternary)?
# [175] value_marker
value_marker ::= '擁有' | '拷貝'
# [176] assignment
assignment ::= sink_into ('←' '拷貝'? assignment | '↤' assignment via_clause? inline_default?)?
# [177] inc_dec_stmt
inc_dec_stmt ::= place ('↑' | '↓')
# [178] place
place ::= call_expr
# [179] ternary
ternary ::= aut_expr ('✓' expression '✗' aut_expr)?
# [180] aut_expr
aut_expr ::= et_expr (('或') et_expr)*
# [181] et_expr
et_expr ::= equality (('且') equality)*
# [182] equality
equality ::= comparison equality_tail*
# [183] equality_tail
equality_tail ::= ('≡' | '≢' | '≅' | '≇' | '≈' | '≉') comparison | ('是' | '非' '是') type_annotation
# [184] comparison
comparison ::= format_expr (('≺' | '≻' | '≤' | '≥' | '∈' | '∉') format_expr)*
# [185] format_expr
format_expr ::= bitwise_or_expr ('¶' STRING)?
# [186] bitwise_or_expr
bitwise_or_expr ::= bitwise_xor_expr ('∨' bitwise_xor_expr)*
# [187] bitwise_xor_expr
bitwise_xor_expr ::= bitwise_and_expr ('⊻' bitwise_and_expr)*
# [188] bitwise_and_expr
bitwise_and_expr ::= shift_expr ('∧' shift_expr)*
# [189] shift_expr
shift_expr ::= range_expr (('⇐' | '⇒') range_expr)*
# [190] range_expr
range_expr ::= additive_expr range_tail?
# [191] range_tail
range_tail ::= ('‥' | '…' | '之前' | '直到') additive_expr ('每' additive_expr)?
# [192] additive_expr
additive_expr ::= multiplicative_expr (('+' | '-' | '⤒' | '⤓') multiplicative_expr)*
# [193] multiplicative_expr
multiplicative_expr ::= vel_expr (('*' | '/' | '÷' | '%' | '·' | '×' | '⊗' | '⊙' | '⊘') vel_expr)*
# [194] vel_expr
vel_expr ::= unary_expr ('或取' vel_rhs)*
# [195] vel_rhs
vel_rhs ::= unary_expr vel_range_tail?
# [196] vel_range_tail
vel_range_tail ::= ('‥' | '…' | '之前' | '直到') unary_expr ('每' unary_expr)?
# [197] unary_expr
unary_expr ::= ('-' | '¬' | '非' | '√' | '∛' | '∜') unary_expr | rounding_expr | finge_expr | cast_expr
# [198] rounding_expr
rounding_expr ::= '⌊' expression ('⌋' | '⌉') | '⌈' expression '⌉'
# [199] gradient_expr
gradient_expr ::= call_expr ('∇' gradient_selection?)?
# [200] gradient_selection
gradient_selection ::= '[' gradient_place (',' gradient_place)* ']'
# [201] gradient_place
gradient_place ::= expression
# [202] cast_expr
cast_expr ::= gradient_expr ('∷' type_annotation | conversio_expr)* inline_default?
# [203] conversio_expr
conversio_expr ::= '↦' (type_annotation | interval_target) via_clause? inline_default?
# [204] interval_target
interval_target ::= range_expr
# [205] via_clause
via_clause ::= '經由' IDENTIFIER
# [206] inline_default
inline_default ::= '⊥' unary_expr
# [207] call_expr
call_expr ::= primary (call_suffix | member_suffix | transpose_suffix | optional_suffix | non_null_suffix)*
# [208] call_suffix
call_suffix ::= call_type_args? '(' argument_list ')'
# [209] member_suffix
member_suffix ::= '.' IDENTIFIER | '[' expression ']'
# [210] transpose_suffix
transpose_suffix ::= 'ᵀ'
# [211] optional_suffix
optional_suffix ::= '?.' IDENTIFIER | '?[' expression ']' | '?(' argument_list ')'
# [212] non_null_suffix
non_null_suffix ::= '!.' IDENTIFIER | '![' expression ']' | '!(' argument_list ')'
# [213] argument_list
argument_list ::= (argument (',' argument)*)?
# [214] argument
argument ::= template_argument | (call_marker | '軸')? '展開'? expression
# [215] call_marker
call_marker ::= '從' | '變值' | '擁有' | '拷貝'
# [216] template_argument
template_argument ::= '展開'? IDENTIFIER ':' expression
# [217] literal
literal ::= NUMBER | STRING | ASCII_STRING | BACKTICK_STRING | OCTETI_STRING | '真' | '假' | '缺失' | '∞' | 'nan'
# [218] regex_literal
regex_literal ::= REGEX_TYPE_WORD REGEX_BODY | '_' REGEX_BODY
# [219] primary
primary ::= IDENTIFIER | literal | regex_literal | '自身' | array_literal | json_literal | typed_constructor | iuncta_expr | ad_expr | clausura_expr | praefixum_expr | scriptum_expr | lege_expr | reducta_expr | scan_expr | sort_expr | capta_expr | '(' expression ')'
# [220] ad_expr
ad_expr ::= '端點' ASCII_STRING ad_opener?
# [221] ad_opener
ad_opener ::= '(' expression ')'
# [222] array_literal
array_literal ::= '[' array_element_list? ']'
# [223] array_element_list
array_element_list ::= array_element (',' array_element)*
# [224] array_element
array_element ::= argument | '_'
# [225] iuncta_expr
iuncta_expr ::= '元組' type_arguments '[' argument_list? ']'
# [226] json_literal
json_literal ::= '{' (json_member (',' json_member)*)? '}'
# [227] json_member
json_member ::= STRING ':' json_value
# [228] typed_constructor
typed_constructor ::= type_annotation '{' field_list? '}' construction_source? | hole_type '{' field_list? '}' construction_source? | type_annotation json_literal | hole_type json_literal | '{' field_list '}' construction_source? | type_annotation '[' array_element_list? ']'
# [229] field_list
field_list ::= field_init (',' field_init)*
# [230] field_init
field_init ::= (field_key '=' expression) | IDENTIFIER
# [231] field_key
field_key ::= IDENTIFIER | STRING | '[' expression ']'
# [232] construction_source
construction_source ::= '取自' call_expr
# [233] json_value
json_value ::= json_object | json_array | json_string | json_number | 'true' | 'false' | 'null'
# [234] json_object
json_object ::= '{' (json_member (',' json_member)*)? '}'
# [235] json_array
json_array ::= '[' (json_value (',' json_value)*)? ']'
# [236] json_string
json_string ::= STRING
# [237] json_number
json_number ::= NUMBER
# [238] finge_expr
finge_expr ::= '虛構' qualified_ident ('{' field_list? '}')? ('∷' type_annotation)?
# [239] qualified_ident
qualified_ident ::= IDENTIFIER ('.' IDENTIFIER)*
# [240] praefixum_expr
praefixum_expr ::= '前綴' block_stmt
# [241] scriptum_expr
scriptum_expr ::= '格式文字' '(' STRING (',' expression)* ')'
# [242] lege_expr
lege_expr ::= '讀取' '行'?
# [243] reducta_expr
reducta_expr ::= '歸約' '經由' reducta_op '取自' expression apud_clause? reducta_group? (filum_clause? reducta_body)? reducta_identity?
# [244] reducta_op
reducta_op ::= '求和' | '求積' | '最大' | '最小' | '最大值索引' | '最小值索引' | '全部' | '任一' | '計數' | '第一個'
# [245] reducta_group
reducta_group ::= '按' (group_key | '[' expression (',' expression)* ']')
# [246] group_key
group_key ::= unary_expr
# [247] reducta_body
reducta_body ::= ('設為' | '變值') IDENTIFIER block_stmt
# [248] reducta_identity
reducta_identity ::= '或取' expression
# [249] scan_expr
scan_expr ::= 'scan' '經由' scan_op ('total' ('inclusive' | 'exclusive'))? '取自' expression apud_clause scan_group? scan_order_clause? filum_clause '設為' IDENTIFIER block_stmt
# [250] scan_op
scan_op ::= '求和' | '求積' | '最大' | '最小' | '全部' | '任一' | '計數'
# [251] scan_group
scan_group ::= '按' expression
# [252] scan_order_clause
scan_order_clause ::= 'order' ('升序' | '降序')
# [253] sort_expr
sort_expr ::= '排序' '經由' sort_direction ('取自' | '從') expression sort_body?
# [254] sort_direction
sort_direction ::= '升序' | '降序'
# [255] sort_body
sort_body ::= ('設為' | '變值') IDENTIFIER block_stmt
# [256] filum_clause
filum_clause ::= '執行緒' IDENTIFIER
# [257] capta_expr
capta_expr ::= '陷阱' block_stmt
# [258] object_pattern
object_pattern ::= '{' pattern_property (',' pattern_property)* '}'
# [259] pattern_property
pattern_property ::= '其餘'? IDENTIFIER ('作為' IDENTIFIER)?
# [260] array_pattern
array_pattern ::= '[' array_pattern_element (',' array_pattern_element)* ']'
# [261] array_pattern_element
array_pattern_element ::= '_' | '其餘'? IDENTIFIER
# [262] nota_stmt
nota_stmt ::= ('註記' | '檢視' | '警告' | '寫出') expression (',' expression)*
# [263] entry_header
entry_header ::= ('引數' IDENTIFIER)? ('出口' expression)?
# [264] incipit_stmt
incipit_stmt ::= '入口' entry_header block_stmt
# [265] incipiet_stmt
incipiet_stmt ::= '非同步入口' entry_header block_stmt
# [266] probandum_decl
probandum_decl ::= '測試規格' STRING proba_modifier* '{' probandum_body '}'
# [267] probandum_body
probandum_body ::= (praepara_block | probandum_decl | proba_stmt)*
# [268] proba_stmt
proba_stmt ::= '測試' STRING proba_modifier* block_stmt
# [269] proba_modifier
proba_modifier ::= '預期失敗' | '略過' STRING | '預期' STRING | '僅限' | '標籤' STRING | '時限' NATURAL | '測量' | '重複' NATURAL | '脆弱' NATURAL | '僅限於' STRING
# [270] praepara_block
praepara_block ::= ('準備' | '準備非同步' | '後置準備' | '後置準備非同步') '全部'? block_stmt
# [271] fac_stmt
fac_stmt ::= '執行' fac_modifier? block_stmt cape_clause? ('當' expression)?
# [272] fac_modifier
fac_modifier ::= '全部' | '裝置'
# [273] IDENTIFIER
IDENTIFIER ::=
# [274] NUMBER
NUMBER ::=
# [275] NATURAL
NATURAL ::=
# [276] STRING
STRING ::=
# [277] ASCII_STRING
ASCII_STRING ::=
# [278] BACKTICK_STRING
BACKTICK_STRING ::=
# [279] OCTETI_STRING
OCTETI_STRING ::=
# [280] REGEX_TYPE_WORD
REGEX_TYPE_WORD ::=
# [281] REGEX_BODY
REGEX_BODY ::=
# [282] NEWLINE
NEWLINE ::=
# [283] WIDTH_MARKER
WIDTH_MARKER ::=
# [284] LISTA_WIDTH_SUGAR
LISTA_WIDTH_SUGAR ::=
# [285] TENSOR_WIDTH_SUGAR
TENSOR_WIDTH_SUGAR ::=
# [286] SPARSA_WIDTH_SUGAR
SPARSA_WIDTH_SUGAR ::=
# [287] VECTOR_WIDTH_SUGAR
VECTOR_WIDTH_SUGAR ::=
# [288] MATRIX_WIDTH_SUGAR
MATRIX_WIDTH_SUGAR ::=
# [289] FRONTMATTER_DELIMITER
FRONTMATTER_DELIMITER ::=
# [290] TOML_LINES
TOML_LINES ::=
# [291] ANNOTATION_NAME
ANNOTATION_NAME ::=
# [292] ANNOTATION_FIELD_NAME
ANNOTATION_FIELD_NAME ::=
# [293] NON_NEWLINE_TOKEN
NON_NEWLINE_TOKEN ::=
# [294] NO_NEWLINE
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
| [`REGEX_TYPE_WORD`](#regex-type-word) | `#regex-type-word` | capture-pending |
| [`REGEX_BODY`](#regex-body) | `#regex-body` | capture-pending |
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
| [`regio_decl`](#regio-decl) | `#模組-decl` | live |
| [`statement`](#statement) | `#statement` | live |
| [`ad_handler_decl`](#ad-handler-decl) | `#端點-handler-decl` | live |
| [`nondum_functio_decl`](#nondum-functio-decl) | `#不穩定-函式-decl` | live |
| [`statement_core`](#statement-core) | `#statement-core` | live |
| [`binding_decl`](#binding-decl) | `#binding-decl` | live |
| [`expr_stmt`](#expr-stmt) | `#expr-stmt` | live |
| [`block_stmt`](#block-stmt) | `#block-stmt` | live |
| [`const_init`](#const-init) | `#const-init` | live |
| [`insere_expr`](#insere-expr) | `#嵌入-expr` | live |
| [`fixum_decl`](#fixum-decl) | `#定值-decl` | live |
| [`figendum_decl`](#figendum-decl) | `#等定-decl` | live |
| [`sit_decl`](#sit-decl) | `#設為-decl` | live |
| [`ref_decl`](#ref-decl) | `#ref-decl` | live |
| [`array_destruct`](#array-destruct) | `#array-destruct` | live |
| [`object_destruct`](#object-destruct) | `#object-destruct` | live |
| [`functio_decl`](#functio-decl) | `#函式-decl` | live |
| [`param_list`](#param-list) | `#param-list` | live |
| [`generic_params`](#generic-params) | `#generic-params` | live |
| [`type_param_list`](#type-param-list) | `#type-param-list` | live |
| [`size_param_list`](#size-param-list) | `#size-param-list` | live |
| [`generic_param`](#generic-param) | `#generic-param` | live |
| [`size_param`](#size-param) | `#size-param` | live |
| [`generic_bound`](#generic-bound) | `#generic-bound` | live |
| [`contract_ref`](#contract-ref) | `#contract-ref` | live |
| [`generic_type_default`](#generic-type-default) | `#generic-type-default` | live |
| [`generic_size_default`](#generic-size-default) | `#generic-size-default` | live |
| [`call_type_args`](#call-type-args) | `#call-type-args` | live |
| [`call_type_arg`](#call-type-arg) | `#call-type-arg` | live |
| [`call_size_arg`](#call-size-arg) | `#call-size-arg` | live |
| [`dimension_tail`](#dimension-tail) | `#dimension-tail` | live |
| [`parameter`](#parameter) | `#parameter` | live |
| [`func_modifier`](#func-modifier) | `#func-modifier` | live |
| [`callable_posture`](#callable-posture) | `#callable-posture` | live |
| [`return_clause`](#return-clause) | `#return-clause` | live |
| [`alternate_exit_clause`](#alternate-exit-clause) | `#alternate-exit-clause` | live |
| [`ergo_joint`](#ergo-joint) | `#則-joint` | live |
| [`clausura_joint`](#clausura-joint) | `#閉包-joint` | live |
| [`clausura_expr`](#clausura-expr) | `#閉包-expr` | live |
| [`compact_clausura_expr`](#compact-clausura-expr) | `#compact-閉包-expr` | live |
| [`clausura_signature`](#clausura-signature) | `#閉包-signature` | live |
| [`fac_block`](#fac-block) | `#執行-block` | live |
| [`clausura_legacy_expr`](#clausura-legacy-expr) | `#閉包-legacy-expr` | live |
| [`clausura_params`](#clausura-params) | `#閉包-params` | live |
| [`clausura_param`](#clausura-param) | `#閉包-param` | live |
| [`genus_decl`](#genus-decl) | `#類型-decl` | live |
| [`genus_member`](#genus-member) | `#類型-member` | live |
| [`nondum_method_decl`](#nondum-method-decl) | `#不穩定-method-decl` | live |
| [`genus_field_decl`](#genus-field-decl) | `#類型-field-decl` | live |
| [`genus_const_decl`](#genus-const-decl) | `#類型-const-decl` | live |
| [`field_decl`](#field-decl) | `#field-decl` | live |
| [`functio_method_decl`](#functio-method-decl) | `#函式-method-decl` | live |
| [`annotation`](#annotation) | `#annotation` | live |
| [`annotation_name`](#annotation-name) | `#annotation-name` | live |
| [`braced_annotation`](#braced-annotation) | `#braced-annotation` | live |
| [`annotation_field_list`](#annotation-field-list) | `#annotation-field-list` | live |
| [`annotation_field`](#annotation-field) | `#annotation-field` | live |
| [`annotation_sugar`](#annotation-sugar) | `#annotation-sugar` | live |
| [`nucleum_annotation`](#nucleum-annotation) | `#內核-annotation` | live |
| [`nucleum_sugar`](#nucleum-sugar) | `#內核-sugar` | live |
| [`nucleum_braced`](#nucleum-braced) | `#內核-braced` | live |
| [`nucleum_modifier`](#nucleum-modifier) | `#內核-modifier` | live |
| [`nucleum_tier`](#nucleum-tier) | `#內核-tier` | live |
| [`nucleum_field_list`](#nucleum-field-list) | `#內核-field-list` | live |
| [`nucleum_field`](#nucleum-field) | `#內核-field` | live |
| [`radix_annotation`](#radix-annotation) | `#radix-annotation` | live |
| [`radix_directive`](#radix-directive) | `#radix-directive` | live |
| [`ad_annotation`](#ad-annotation) | `#端點-annotation` | live |
| [`nondum_annotation`](#nondum-annotation) | `#不穩定-annotation` | live |
| [`implendum_decl`](#implendum-decl) | `#待實作介面-decl` | live |
| [`implendum_method_decl`](#implendum-method-decl) | `#待實作介面-method-decl` | live |
| [`typus_decl`](#typus-decl) | `#型別-decl` | live |
| [`ordo_decl`](#ordo-decl) | `#列舉-decl` | live |
| [`enum_member`](#enum-member) | `#enum-member` | live |
| [`discretio_decl`](#discretio-decl) | `#分支聯集-decl` | live |
| [`union_fields`](#union-fields) | `#union-fields` | live |
| [`union_member`](#union-member) | `#union-member` | live |
| [`variant`](#variant) | `#variant` | live |
| [`variant_fields`](#variant-fields) | `#variant-fields` | live |
| [`schema_decl`](#schema-decl) | `#結構-decl` | live |
| [`schema_column`](#schema-column) | `#結構-column` | live |
| [`importa_decl`](#importa-decl) | `#匯入-decl` | live |
| [`importa_record`](#importa-record) | `#匯入-record` | live |
| [`import_field_list`](#import-field-list) | `#import-field-list` | live |
| [`import_field`](#import-field) | `#import-field` | live |
| [`ex_field`](#ex-field) | `#取自-field` | live |
| [`visibilitas_field`](#visibilitas-field) | `#可見性-field` | live |
| [`nomen_field`](#nomen-field) | `#名稱-field` | live |
| [`ut_field`](#ut-field) | `#作為-field` | live |
| [`omnia_field`](#omnia-field) | `#全部-field` | live |
| [`importa_sugar`](#importa-sugar) | `#匯入-sugar` | live |
| [`import_target`](#import-target) | `#import-target` | live |
| [`公開`](#publica) | `#公開` | live |
| [`named_import`](#named-import) | `#named-import` | live |
| [`wildcard_import`](#wildcard-import) | `#wildcard-import` | live |
| [`selective_import`](#selective-import) | `#selective-import` | live |
| [`import_value_binding`](#import-value-binding) | `#import-value-binding` | live |
| [`type_annotation`](#type-annotation) | `#type-annotation` | live |
| [`concrete_type`](#concrete-type) | `#concrete-type` | live |
| [`union_hole_type`](#union-hole-type) | `#union-hole-type` | live |
| [`intersection_type`](#intersection-type) | `#intersection-type` | live |
| [`owned_type`](#owned-type) | `#owned-type` | live |
| [`base_type`](#base-type) | `#base-type` | live |
| [`failable_promissum_type`](#failable-promissum-type) | `#failable-promissum-type` | live |
| [`ratio_type`](#ratio-type) | `#ratio-type` | live |
| [`hole_type`](#hole-type) | `#hole-type` | live |
| [`qualified_type`](#qualified-type) | `#qualified-type` | live |
| [`type_head`](#type-head) | `#type-head` | live |
| [`type_arguments`](#type-arguments) | `#type-arguments` | live |
| [`type_argument`](#type-argument) | `#type-argument` | live |
| [`labeled_type_argument`](#labeled-type-argument) | `#labeled-type-argument` | live |
| [`range_argument`](#range-argument) | `#range-argument` | live |
| [`sized_range_argument`](#sized-range-argument) | `#sized-range-argument` | live |
| [`range_glyph`](#range-glyph) | `#range-glyph` | live |
| [`range_bound`](#range-bound) | `#range-bound` | live |
| [`width_type_sugar`](#width-type-sugar) | `#width-type-sugar` | live |
| [`shape_suffix`](#shape-suffix) | `#shape-suffix` | live |
| [`figura`](#figura) | `#figura` | live |
| [`spelled_lift`](#spelled-lift) | `#spelled-lift` | live |
| [`lift_extent`](#lift-extent) | `#lift-extent` | live |
| [`figura_product`](#figura-product) | `#figura-product` | live |
| [`figura_primary`](#figura-primary) | `#figura-primary` | live |
| [`figura_list`](#figura-list) | `#figura-list` | live |
| [`function_type`](#function-type) | `#function-type` | live |
| [`type_list`](#type-list) | `#type-list` | live |
| [`si_stmt`](#si-stmt) | `#若-stmt` | live |
| [`si_tail`](#si-tail) | `#若-tail` | live |
| [`secus_clause`](#secus-clause) | `#否則-clause` | live |
| [`arm`](#arm) | `#arm` | live |
| [`else_arm`](#else-arm) | `#else-arm` | live |
| [`dum_stmt`](#dum-stmt) | `#當-stmt` | live |
| [`itera_stmt`](#itera-stmt) | `#遍歷-stmt` | live |
| [`itera_binding`](#itera-binding) | `#遍歷-binding` | live |
| [`apud_clause`](#apud-clause) | `#在-clause` | live |
| [`elige_stmt`](#elige-stmt) | `#選擇-stmt` | live |
| [`casu_elige_clause`](#casu-elige-clause) | `#分支-選擇-clause` | live |
| [`ceterum_clause`](#ceterum-clause) | `#預設-clause` | live |
| [`discerne_stmt`](#discerne-stmt) | `#比對-stmt` | live |
| [`discriminants`](#discriminants) | `#discriminants` | live |
| [`subject_path`](#subject-path) | `#subject-path` | live |
| [`casu_variant_clause`](#casu-variant-clause) | `#分支-variant-clause` | live |
| [`patterns`](#patterns) | `#patterns` | live |
| [`pattern`](#pattern) | `#pattern` | live |
| [`pattern_atom`](#pattern-atom) | `#pattern-atom` | live |
| [`negated_number`](#negated-number) | `#negated-number` | live |
| [`type_pattern`](#type-pattern) | `#type-pattern` | live |
| [`ut_pattern`](#ut-pattern) | `#作為-pattern` | live |
| [`pattern_binding`](#pattern-binding) | `#pattern-binding` | live |
| [`custodi_stmt`](#custodi-stmt) | `#守衛-stmt` | live |
| [`si_guard_clause`](#si-guard-clause) | `#若-guard-clause` | live |
| [`ex_stmt`](#ex-stmt) | `#取自-stmt` | live |
| [`extract_fields`](#extract-fields) | `#extract-fields` | live |
| [`extract_field`](#extract-field) | `#extract-field` | live |
| [`ceteri_field`](#ceteri-field) | `#其餘-field` | live |
| [`redde_stmt`](#redde-stmt) | `#傳回-stmt` | live |
| [`reddet_stmt`](#reddet-stmt) | `#等返-stmt` | live |
| [`tacebit_stmt`](#tacebit-stmt) | `#等棄-stmt` | live |
| [`cede_stmt`](#cede-stmt) | `#讓出-stmt` | live |
| [`rumpe_stmt`](#rumpe-stmt) | `#中斷-stmt` | live |
| [`perge_stmt`](#perge-stmt) | `#繼續-stmt` | live |
| [`tacet_stmt`](#tacet-stmt) | `#靜默-stmt` | live |
| [`iace_stmt`](#iace-stmt) | `#拋出-stmt` | live |
| [`iace_expr`](#iace-expr) | `#拋出-expr` | live |
| [`iace_guarded_expr`](#iace-guarded-expr) | `#拋出-guarded-expr` | live |
| [`cape_clause`](#cape-clause) | `#捕捉-clause` | live |
| [`adfirma_stmt`](#adfirma-stmt) | `#斷言-stmt` | live |
| [`requirit_stmt`](#requirit-stmt) | `#需要-stmt` | live |
| [`reice_stmt`](#reice-stmt) | `#拒絕-stmt` | live |
| [`expression`](#expression) | `#expression` | live |
| [`transfer`](#transfer) | `#transfer` | live |
| [`sink_into`](#sink-into) | `#sink-into` | live |
| [`value_marker`](#value-marker) | `#value-marker` | live |
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
| [`vel_expr`](#vel-expr) | `#或取-expr` | live |
| [`vel_rhs`](#vel-rhs) | `#或取-rhs` | live |
| [`vel_range_tail`](#vel-range-tail) | `#或取-range-tail` | live |
| [`unary_expr`](#unary-expr) | `#unary-expr` | live |
| [`rounding_expr`](#rounding-expr) | `#rounding-expr` | live |
| [`gradient_expr`](#gradient-expr) | `#gradient-expr` | live |
| [`gradient_selection`](#gradient-selection) | `#gradient-selection` | live |
| [`gradient_place`](#gradient-place) | `#gradient-place` | live |
| [`cast_expr`](#cast-expr) | `#cast-expr` | live |
| [`conversio_expr`](#conversio-expr) | `#conversio-expr` | live |
| [`interval_target`](#interval-target) | `#interval-target` | live |
| [`via_clause`](#via-clause) | `#經由-clause` | live |
| [`inline_default`](#inline-default) | `#inline-default` | live |
| [`call_expr`](#call-expr) | `#call-expr` | live |
| [`call_suffix`](#call-suffix) | `#call-suffix` | live |
| [`member_suffix`](#member-suffix) | `#member-suffix` | live |
| [`transpose_suffix`](#transpose-suffix) | `#transpose-suffix` | live |
| [`optional_suffix`](#optional-suffix) | `#optional-suffix` | live |
| [`non_null_suffix`](#non-null-suffix) | `#非-null-suffix` | live |
| [`argument_list`](#argument-list) | `#argument-list` | live |
| [`argument`](#argument) | `#argument` | live |
| [`call_marker`](#call-marker) | `#call-marker` | live |
| [`template_argument`](#template-argument) | `#template-argument` | live |
| [`literal`](#literal) | `#literal` | live |
| [`regex_literal`](#regex-literal) | `#regex-literal` | live |
| [`primary`](#primary) | `#primary` | live |
| [`ad_expr`](#ad-expr) | `#端點-expr` | live |
| [`ad_opener`](#ad-opener) | `#端點-opener` | live |
| [`array_literal`](#array-literal) | `#array-literal` | live |
| [`array_element_list`](#array-element-list) | `#array-element-list` | live |
| [`array_element`](#array-element) | `#array-element` | live |
| [`iuncta_expr`](#iuncta-expr) | `#元組-expr` | live |
| [`json_literal`](#json-literal) | `#json-literal` | live |
| [`json_member`](#json-member) | `#json-member` | live |
| [`typed_constructor`](#typed-constructor) | `#typed-constructor` | live |
| [`field_list`](#field-list) | `#field-list` | live |
| [`field_init`](#field-init) | `#field-init` | live |
| [`field_key`](#field-key) | `#field-key` | live |
| [`construction_source`](#construction-source) | `#construction-source` | live |
| [`json_value`](#json-value) | `#json-value` | live |
| [`json_object`](#json-object) | `#json-object` | live |
| [`json_array`](#json-array) | `#json-array` | live |
| [`json_string`](#json-string) | `#json-string` | live |
| [`json_number`](#json-number) | `#json-number` | live |
| [`finge_expr`](#finge-expr) | `#虛構-expr` | live |
| [`qualified_ident`](#qualified-ident) | `#qualified-ident` | live |
| [`praefixum_expr`](#praefixum-expr) | `#前綴-expr` | live |
| [`scriptum_expr`](#scriptum-expr) | `#格式文字-expr` | live |
| [`lege_expr`](#lege-expr) | `#讀取-expr` | live |
| [`reducta_expr`](#reducta-expr) | `#歸約-expr` | live |
| [`reducta_op`](#reducta-op) | `#歸約-op` | live |
| [`reducta_group`](#reducta-group) | `#歸約-group` | live |
| [`group_key`](#group-key) | `#group-key` | live |
| [`reducta_body`](#reducta-body) | `#歸約-body` | live |
| [`reducta_identity`](#reducta-identity) | `#歸約-identity` | live |
| [`scan_expr`](#scan-expr) | `#scan-expr` | live |
| [`scan_op`](#scan-op) | `#scan-op` | live |
| [`scan_group`](#scan-group) | `#scan-group` | live |
| [`scan_order_clause`](#scan-order-clause) | `#scan-order-clause` | live |
| [`sort_expr`](#sort-expr) | `#sort-expr` | live |
| [`sort_direction`](#sort-direction) | `#sort-direction` | live |
| [`sort_body`](#sort-body) | `#sort-body` | live |
| [`filum_clause`](#filum-clause) | `#執行緒-clause` | live |
| [`capta_expr`](#capta-expr) | `#陷阱-expr` | live |
| [`object_pattern`](#object-pattern) | `#object-pattern` | live |
| [`pattern_property`](#pattern-property) | `#pattern-property` | live |
| [`array_pattern`](#array-pattern) | `#array-pattern` | live |
| [`array_pattern_element`](#array-pattern-element) | `#array-pattern-element` | live |
| [`nota_stmt`](#nota-stmt) | `#註記-stmt` | live |
| [`entry_header`](#entry-header) | `#entry-header` | live |
| [`incipit_stmt`](#incipit-stmt) | `#入口-stmt` | live |
| [`incipiet_stmt`](#incipiet-stmt) | `#非同步入口-stmt` | live |
| [`probandum_decl`](#probandum-decl) | `#測試規格-decl` | live |
| [`probandum_body`](#probandum-body) | `#測試規格-body` | live |
| [`proba_stmt`](#proba-stmt) | `#測試-stmt` | live |
| [`proba_modifier`](#proba-modifier) | `#測試-modifier` | live |
| [`praepara_block`](#praepara-block) | `#準備-block` | live |
| [`fac_stmt`](#fac-stmt) | `#執行-stmt` | live |
| [`fac_modifier`](#fac-modifier) | `#執行-modifier` | live |

## Lexicon Appendix {#lexicon}

The lexical tier is descriptive and remains owned by the live lexer and
driver. `capture-pending` rows intentionally carry no invented token shape.

| Terminal | Status | Capture notes |
|---|---|---|
| `IDENTIFIER` | `capture-pending` | Lexical tier. Empty RHS; status is capture-pending. radix-lexer / driver / parser is the authority (crates/radix-lexer/src/). Not a second lexer spec. scan.rs scan_identifier; Unicode XID_Start or '_' then XID_Continue or '_'; NFKC intern; TokenKind::Ident (keywords also lex as identifiers) |
| `NUMBER` | `capture-pending` | scan.rs scan_number; decimal/hex/bin/oct integers and floats with '_' separators; TokenKind::Integer(u64) when the value fits u64, TokenKind::BigInteger(text) when an integer literal is longer (no upper bound on length; inf track, F9 ruling 34) or Float(f64); a BigInteger is legal only where an expression literal or a `分支` constant pattern stands (its value must then fit the receiving slot: always an `inf` slot, otherwise the slot's range) and is a parse error in a NATURAL, enum-member or JSON-literal position (a JSON integer keeps the signed 64-bit wire range: `json_integer_overflow` / `json_integer_underflow`); scan.rs also lexes the glyph '∞' as Float(+inf), never an `inf` value |
| `NATURAL` | `capture-pending` | not a distinct lexer token; TokenKind::Integer (so at most u64::MAX; a BigInteger here is a parse error) used as magnitudo capacity in type position, as the count of a `測試` modifier (`時限`, `重複`, `脆弱`; a float is `test_modifier_integer`), and as a function's `出口` code (no fraction/exponent) |
| `STRING` | `capture-pending` | scan.rs scan_string / scan_guillemet_block_string; double-quoted or guillemet block; TokenKind::String |
| `ASCII_STRING` | `capture-pending` | scan.rs scan_ascii_string; single-quoted; TokenKind::AsciiString |
| `BACKTICK_STRING` | `capture-pending` | scan.rs scan_backtick_string; backtick forma template; TokenKind::BacktickString |
| `OCTETI_STRING` | `capture-pending` | scan.rs scan_octeti_string; pipe-delimited hex; TokenKind::OctetiString |
| `REGEX_TYPE_WORD` | `capture-pending` | the file locale's regex type word (English `regex`); the lexer opens a raw body at `/` only after this spelling or `_`; not a lexer token |
| `REGEX_BODY` | `capture-pending` | scan.rs scan_slash; raw body between slashes, only after the regex type word or `_`; TokenKind::RegexString |
| `NEWLINE` | `capture-pending` | scan.rs scan_line_break; LF or CRLF; TokenKind::Newline |
| `WIDTH_MARKER` | `capture-pending` | parser type-position identifier i8/i16/i32/i64/u8/u16/u32/u64 and decimal d64 (numerus and exactus), f16/bf16/f32/f64 (fractus only); every integer width i8…u64 (modulus; no d64, no float); every integer width i8…u64 and float width f16…f64 (saturatus; no d64); integer widths, d64 and float widths (exactus; a float cell traps only when a non-finite value is stored or converted into it); the unbounded integer marker `inf` (numerus, exactus, modulus and saturatus all name the same type; locale-invariant, not a keyword; never prefixed sugar; not a float width; not a tensor, sparsa, vector or matrix element; host-only, so no kernel or AIR-lane position takes it); not a lexer token |
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
| Iteration | `範圍` | range iteration |
| Endpoints | `端點` | capability call |
| Error | `斷言` | assert |
| Iteration | `之前` | range until exclusive |
| Grammar | `在` | keyword literal derived from the production |
| Expression | `最大值索引` | argmax reduction operator (en `argmax`) |
| Expression | `最小值索引` | argmin reduction operator (en `argmin`) |
| Params | `引數` | CLI arguments modifier |
| Expression | `升序` | ascending sort direction (en `asc`; contextual) |
| Boolean | `或` | or |
| Params | `軸` | axis marker word: a parameter declares it (`軸 int a`), a call echoes it (`t.mean(軸 1)`); shared source spelling; contextual |
| Annotation | `backward` | `@ radix` gradient-companion directive |
| Error | `捕捉` | local handler |
| Error | `陷阱` | capture boundary (error channel reified as a value) |
| Control | `分支` | case |
| Async | `讓出` | yield |
| Params | `其餘` | rest |
| Control | `預設` | default case |
| Objects | `閉包` | legacy closure |
| Declarations | `欄位` | relational column (experimental; census-types) |
| Annotation | `contract` | `@ radix` contract-role mark |
| Type | `拷貝` | copy ownership |
| Control | `守衛` | guard |
| Type | `從` | borrow / for-in keys |
| Expression | `降序` | descending sort direction (en `desc`; contextual) |
| Control | `比對` | pattern match |
| Declarations | `分支聯集` | tagged union |
| Control | `當` | while / postfix until |
| Objects | `自身` | self |
| Control | `選擇` | switch |
| Control | `則` | compact statement-body joint |
| Params | `錯誤` | error channel |
| Testing | `預期失敗` | expect failure |
| Boolean | `是` | is / type test |
| Boolean | `且` | and |
| Iteration | `取自` | for-of / import from / reduction source |
| Params | `出口` | exit code |
| Control | `執行` | do block / post-test loop |
| Expression | `求積` | product reduction operator (en `product`) |
| JSON | `false` | JSON false |
| Boolean | `假` | false |
| Async | `異流` | async stream posture |
| Async | `異步` | async finite posture |
| Async | `等定` | await-bind immutable |
| Grammar | `執行緒` | keyword literal derived from the production |
| Objects | `虛構` | construct variant |
| Async | `流` | sync stream posture |
| Declarations | `定值` | immutable binding |
| Testing | `脆弱` | flaky |
| Annotation | `片段` | nucleum fragment |
| Declarations | `函式` | function |
| Testing | `預期` | future |
| Declarations | `類型` | class |
| Error | `拋出` | throw |
| Error | `可拋` | throws marker |
| Params | `不變` | immutable modifier |
| Declarations | `待實作介面` | interface contract |
| Genus | `實作` | implements |
| Declarations | `匯入` | import |
| Type | `傳入` | ownership in |
| Declarations | `非同步入口` | async entrypoint |
| Declarations | `入口` | entrypoint |
| Comptime | `嵌入` | build-time file embed |
| Annotation | `不安全` | kernel tier `@ 內核 不安全` (en `unsafe`); contextual |
| Control | `遍歷` | for |
| Objects | `元組` | tuple type/constructor |
| Annotation | `車道` | `@ radix` compiler-lane directive |
| Annotation | `寬鬆` | kernel tier `@ 內核 寬鬆` (en `relaxed`); contextual |
| Builtin | `讀取` | read |
| Builtin | `行` | line |
| Control | `裝置` | device region modifier after `執行` (en `device`); contextual |
| Declarations | `尺寸` | size/index generic parameter |
| Expression | `最大` | maximum reduction operator (en `max`) |
| Testing | `測量` | benchmark |
| Expression | `最小` | minimum reduction operator (en `min`) |
| Type | `模數` | modular-word policy type head (en `wrapping`) |
| Diagnostics | `警告` | warn |
| Error | `崩潰` | panic |
| Declarations | `名稱` | import binding name |
| Boolean | `非` | not |
| Annotation | `不穩定` | availability marker `@ 不穩定` (en `@ unstable`) |
| Literals | `nan` | named NaN literal (`nan` outside the Latin pack) |
| Diagnostics | `註記` | note |
| Annotation | `內核` | kernel annotation |
| JSON | `null` | JSON null |
| Literals | `缺失` | null |
| Expression | `計數` | count reduction operator (en `count`) |
| Testing | `略過` | skip |
| Params | `全部` | all reduction operator / glob |
| Params | `可選項` | options modifier |
| Expression | `排序` | sort expression head (en `sort`; contextual) |
| Declarations | `列舉` | enum |
| Type | `擁有` | owned |
| Iteration | `每` | range step |
| Control | `繼續` | continue |
| Testing | `後置準備` | teardown |
| Testing | `後置準備非同步` | async teardown |
| Objects | `前綴` | prefix expression |
| Testing | `準備` | setup |
| Testing | `準備非同步` | async setup |
| Expression | `第一個` | first-match reduction operator (en `first`; contextual) |
| Expression | `按` | grouped reduction clause (en `每`; contextual) |
| Testing | `測試` | test |
| Testing | `測試規格` | test suite |
| Declarations | `公開` | public visibility |
| Expression | `任一` | any reduction operator (en `any`; contextual) |
| Annotation | `radix` | compiler-reserved annotation family |
| Objects | `ratio` | named-field aggregate type/constructor |
| Control | `傳回` | return |
| Async | `等返` | await-return |
| Expression | `歸約` | reduction expression head (en `reduce`; contextual) |
| Declarations | `模組` | file module name (contextual) |
| Error | `拒絕` | reject |
| Testing | `重複` | repeat |
| Error | `需要` | require |
| Control | `中斷` | break |
| Expression | `scan` | ordered prefix-scan head (shared source spelling; contextual) |
| Expression | `exclusive` | exclusive prefix-scan mode (shared source spelling; contextual) |
| Expression | `inclusive` | inclusive prefix-scan mode (shared source spelling; contextual) |
| Expression | `order` | scan order-clause introducer (shared source spelling; contextual) |
| Expression | `total` | prefix-scan total-mode introducer (shared source spelling; contextual) |
| Declarations | `結構` | relational heading (experimental; census-types) |
| Diagnostics | `寫出` | diagnostic channel |
| Builtin | `格式文字` | write |
| Control | `否則` | else |
| Control | `若` | if |
| Control | `否則若` | else-if |
| Declarations | `設為` | write-once local |
| Testing | `僅限` | only |
| Testing | `僅限於` | only-in |
| Params | `展開` | spread |
| Declarations | `可選` | optional declaration slot |
| Expression | `求和` | sum reduction operator (en `sum`) |
| Async | `等棄` | await-discard |
| Control | `靜默` | no-op |
| Testing | `標籤` | tag |
| Testing | `時限` | timeout |
| JSON | `true` | JSON true |
| Declarations | `型別` | type alias |
| Iteration | `直到` | range until inclusive |
| Params | `作為` | as / alias |
| Declarations | `變值` | mutable binding |
| Async | `等變` | await-bind mutable |
| Boolean | `或取` | nullable default / explicit reduction identity |
| Boolean | `真` | true |
| Conversion | `經由` | convert-hint clause, reduction operator clause, scan operator clause or sort direction clause (contextual) |
| Diagnostics | `檢視` | debug |
| Declarations | `可見性` | visibility field |

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

Documentation contract: runnable language reference programs live in the
language corpus, [`corpus/`](../corpus/), with optional `+++` frontmatter
(`term`, `syntax`, `related`, …). `faber explain` loads the exempla reference
pack from disk. Prefer the language corpus + EBNF for new reference work.

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
lists, `列舉` members, `分支聯集` variant lists, JSON members and array
elements, annotation / import / nucleum fields, output statement lists) —
require a comma between adjacent items and forbid one after the last.

**Declaration blocks** — self-annotating declarations (statements, `類型`
members, `待實作介面` methods, `分支聯集` payload fields) — contain no commas.
Entries are trivia-delimited.

---

## Declarations

Declarations are top-level. A `函式` and the type declarations (`類型`,
`待實作介面`, `型別`, `列舉`, `分支聯集`, `結構`) may not appear inside a
block; the parser rejects them there (`declaration_not_top_level`). Methods
live in `類型` bodies. For a local function, bind a closure; for recursion,
use a top-level function.

### Variables


- `設為` (en `let`) = run-time name, write-once: `設為 T name ← e` states the
  type, `設為 _ name ← e` infers it. Every declaration writes its type slot;
  `_` is the explicit request to infer, and the type-omitted `設為 name ← e`
  (likewise `變值 name`, `從 name`, `定值 name`) is `PARSE010`
  `binding_type_slot_required`. It may be declared without an initializer and
  assigned exactly once later, then frozen. `變值` (en `mut`) = writable name
  (reassignable). `定值` (en `const`) = compile-time constant only, and it goes
  with `=`: a run-time `定值 T name ← e` is `PARSE010` `runtime_const_retired`
  (write `設為`), and the old English spelling `var` is `PARSE010` `var_retired`
  (write `mut`).
- `從 T name ← place` (en `ref T name ← place`) is a re-pointable name that
  reads through to a place: the name may be bound to another place, a write
  through it is `SEM056` `assign_through_ref`, a scalar type takes no marker
  (`SEM008` `scalar_access_marker`), and `拷貝`/`擁有` do not belong on it
  (`PARSE010` `ref_local_value_marker`).
- `等定` / `等變` await a `promissum<T>` or `promissum<T ⇥ E>`, bind
  the resolved `T`, and propagate a compatible alternate `E`.
- `↢` is the await-directed initializer for an ordinary declaration:
  `定值 T name ↢ future`, `變值 T name ↢ future`, or `設為 _ name ↢ future`.
  It has the same await and alternate-propagation semantics as
  `等定 T name ← future`, but it is not a general expression operator and
  cannot target an existing place.
- Use `_` as the type annotation when the initializer determines the type: `變值 _ name ← value`
- `設為 _ name ← value` is the inferred write-once local; `設為 T name ← value` is the typed one
- `設為 _ name` (no initializer) is the inferred deferred
  immutable. Assign exactly once before any read.
- Typed `設為`/`變值` initializers accept `↤` (`設為 numerus x ↤ "42"`):
  the written type is the conversion destination, then the binding is
  initialized. `等定`/`等變` keep `←`; `變值 _`, `設為 _`, and untyped
  destructuring reject `↤` (no concrete destination type).
- `定值 T x = e` (D5.10) declares a typed **constant**; at the top of a file
  the same declaration is the module-level constant (next section). `=` states a
  compile-time fact, so `e` is evaluated while compiling (literals, arithmetic
  and the other operators on scalars, module constants, earlier constants) and
  must fit `T` whatever `T`'s overflow policy: `定值 u8 d = 300` is a compile
  error even for `saturating<u8>`. `定值 _ x = 10` infers `int`. The result is
  an ordinary immutable local of type `T`. `變值` never takes `=`
  (`varia_compile_time_initializer`), and a value that is not known at compile
  time is stored with `←` (`constant_initializer_not_constant`, SEM060).
- Deferred init: `設為 numerus x` or `設為 x` declares an uninitialized immutable
  slot that must be assigned exactly once before any read; a second assignment is
  rejected. The definite-assignment pass (semantic Phase 3a) enforces this.

#### Sharing on bind (`const` is deep)

A plain `←` bind into a writable name shares the source value; `拷貝` and
`擁有` are the two legal ways to detach it. Binding a shareable payload — a
collection or a genus value, including an `Option` or alias of one — from a
read-only source (a `定值` local or a module constant) into a writable
`變值` name is `SEM065` `const_shared_into_var`; scalars bind by value and
are always legal.

| Binding (target `變值 b`) | `定值` / module-constant source | `變值` source |
| -------------------------- | -------------------------------- | -------------- |
| `變值 T b ← a` | SEM065 for a shareable payload; scalars bind by value | accepted — `b` shares `a`; writes through `b` land in `a` |
| `變值 T b ← 拷貝 a` (en `拷貝`) | accepted — `b` is an independent value | accepted — independent value |
| `變值 T b ← 擁有 a` (en `擁有`) | accepted as a takeover; a later use of `a` rejects the bind with SEM057 `unmarked_into_own` | same rule |
| `取自 a 變值 x, 其餘 r` (en `from a var x, rest r`) | per binder, the plain-share rule applies to the field's payload: a shareable field of a read-only source is SEM065, scalar fields bind by value; the `其餘` binder shares the whole source, so a shareable source is SEM065 too | accepted — each binder shares its field, `其餘` shares the whole source; the source evaluates once |

- An immutable target never triggers SEM065: `定值 T b ← a` is accepted for
  every source.
- An unmarked or `從`/`ref` parameter source is outside SEM065; the borrow
  pass tracks the alias and refuses in-place mutation through it (for example
  `append` on such an alias is SEM020
  `imperativus_operator_method_requires_varia_receiver`). Rebinding the alias
  with `←` is always allowed — `←` rebinds a name.
- `擁有` moves an exclusive source immediately (a later use is SEM050
  `use_after_move`); an unmarked or read-only source is tentatively accepted
  as provably dead, and the bind is rejected if the source is used later.

### Module-level constants

A module declares values only as compile-time constants: `定值 T X = e` at the
top of a file (en `const T X = e`), for example `定值 numerus LIMES = 4096`
(D5.8, st1 R1). It is the same declaration as the block-level constant above,
with `=` stating a compile-time fact, and it is the only top-level value
declaration. Module-level mutable state does not exist (D5.7): a top-level
`變值`, `設為`, destructuring, or a runtime initializer (`←`, `↤`, `↢`) is a
compile error, SEM062 `top_level_binding` — a runtime value belongs in a
function. `變值 T X = e` stays `varia_compile_time_initializer`, and a
top-level declaration with no initializer is SEM008 `top_level_initializer`.
`定值 _ X = 10` infers `int` as it does for a local.

**Retired spelling.** The module-level static `generis T X = e` (en
`static T X = e`) no longer exists. A statement-initial `generis` followed by an
identifier or `(`, at the top level or in a block, parses as the old
declaration whole and is reported as `PARSE010 static_decl_retired` (args
`keyword`, the spelling written, and `name`; the help names the `定值`
spelling); there is no alias period, and the diagnostic stays. `generis`
followed by anything else is an ordinary identifier. In a `類型` body the
same word is the separate `PARSE010 static_member_retired`; the type-level
member is the constant `定值 T X = e` (see Classes).

Constants are **immutable and initialized with `=` only** (D5.9), never `←`.
The initializer must be evaluable at compile time: literals;
arithmetic, comparison, bit, and logical operators on `numerus`, `fractus`,
and `bivalens` scalars, plus `textus` concatenation; references to other
constants (evaluated in dependency order, so a constant may be used before its
declaration — a cycle is `constant_cycle`, SEM007); and
collection literals (`lista`, tuples, map construction) whose elements are
constants (only their scalar leaves fold). Anything else is
`constant_initializer_not_constant` (SEM060). Decimal widths and `模數<W>`/`saturatus<W>` values are
not folded, so arithmetic on them is not a compile-time constant today.
Compile-time integer arithmetic is checked (overflow and division by zero are
compile errors: `constant_arithmetic_overflow`, `constant_division_by_zero`),
matching the runner's checked runtime semantics. A value that needs
computation takes a `前綴 { … }` block (en `comptime { … }`), which runs
during the build; it is legal as the whole initializer of an `=` constant, at
module level or at block level (inside any function, method, closure or entry
body), and as a `類型` field default under any modifier. Anywhere else it is
`SEM064` `praefixum_outside_constant`. The body stands alone: it may read module
constants but not a parameter or local of the enclosing body
(`praefixum_captures_local`), and inside a generic function, method or genus it
may not mention a type parameter (`praefixum_type_parameter`). A block-level body
is evaluated like a module constant; an inner `前綴` constant runs before
the one that contains it, and a cycle through a site is `constant_cycle`.

**Build-time file embed, `嵌入` (en `embed`, D8.10).** An `=` constant at
module level or block level, or a `類型` field default under any modifier
(`定值` constant member, `設為`, `變值`), may take `嵌入 "path"` as its initializer
instead of an ordinary expression:
`定值 textus LICENSE = 嵌入 "LICENSE.txt"`. `嵌入` is contextual (the
parser claims it in expression position when directly followed by a string
literal; any other use of the spelling is an ordinary identifier). Lowering
admits it only as the whole initializer of such a constant or field default;
anywhere else (`x ← 嵌入 "p"`, a call argument, a `傳回` value) it is `SEM061`
`insere_outside_constant`. The path is package-relative, resolved against the nearest ancestor `faber.toml` (or the source file's own directory when none exists); an absolute path or a `..` escape is rejected, and a missing file is a compile error. The file is read once, at build time — it is a build input, like the source itself. The declared type decides how the bytes land: `textus` requires valid UTF-8 and fails to build otherwise; `lista<b8>` reads the raw bytes unconditionally. The result type follows the slot: a `_` slot, or no slot, is `SEM061` `insere_type_required`, and any other concrete slot type is `insere_type_invalid`.

### Functions

- Generic parameter lists put type parameters first and `尺寸` (en `size`) parameters after them (`<T, U, 尺寸 N>`); a type parameter after a size parameter is `type_param_after_magnitudo`. Once one parameter has a default (`= numerus`, `尺寸 N = 3`), every later parameter needs one (`generic_default_not_trailing`).
- The `出口` function modifier takes an identifier or a non-negative integer literal; the entry-point `出口` (below) takes an expression.

### Closures

The body joint keeps the existing closure law: `∴` followed by one expression, or `∴ 執行 { ... }` (`do` in the English reader); bare `{ ... }` is not a closure body.

- Return syntax: `→` declares the normal success type. A bodyful function with no `→` is effect-only (`vacuum`) and must not contain `傳回`. A statement-bodied closure (`執行 { ... }` or legacy block body) must also spell `→ T` before it can use `傳回`; expression-bodied closures may infer their result from the expression.
- Recoverable alternate-exit syntax: `⇥` declares the error-channel type. It can appear after `→ T` or alone on an effect-only failable function or closure. A closure body that uses an escaping `拋出` must declare its own `⇥ E`; it cannot inherit the enclosing function's error channel. A local `執行 { ... } 捕捉 err { ... }` may catch `拋出` without an enclosing `⇥`. A failable function call (`→ T ⇥ E`) inside a `⇥`-declaring function propagates to the function's alternate exit without a `執行`/`捕捉` wrapper, mirroring how bare `↦` conversio and `拋出` throws already behave; the call lowers to Rust `?`. A closure must still declare its own `⇥` to propagate a failable call — the enclosing function's error channel does not cross the closure boundary.
- In a signature, `⇥` only ever names an error type (`→ T ⇥ E`). It never carries a value. The error type named after `⇥` is the payload; the value that actually travels is the compiler-owned carrier `erratum<T>` (en `error<T>`), a builtin generic genus with one field `payload: T`, in the same wrapper family as `promissum<T>` and `cursor<T>`. `⇥ T` means the channel carries `erratum<T>`, and `⇥ erratum<T>` is written only for a nested carrier (mechanical expansion, no special diagnostic). `捕捉` (en `catch`) binds the payload `T`, never the carrier, and a form that binds the carrier is not built. The catch-all channel is `ignotum` (en `unknown`), which accepts every error with no `@ conversio` row and needs the ordinary narrowing to read back, while `任一` (en `any`) is the reduction keyword and stays a non-canonical alias of the same primitive, never the catch-all spelling. An `@ conversio` row is written on the payload types and is one direct (source, destination) pair.
- Parameter access markers live in the type position: `從`/`ref` (read), `傳入`/`mut` (mutate), `擁有` (consume), and `拷貝` (duplicate then own). The retired parameter-prefix slot is not part of the grammar; `取自`/`from` remains the import/iteration/extraction token identity.
- Call-site access markers: an argument passed to a parameter declared `mut` / `擁有` / `拷貝` carries the same marker at the call (en `mut` `擁有` `拷貝`; la `傳入` `擁有` `拷貝`), as in `bump(mut a)`, `eat(擁有 a)`, `dup(拷貝 a)`; the read marker `ref` (la `從`) is optional and may be written explicitly. This applies to every call of a function with a source signature, top-level or in a class/genus, and to a method's arguments (`obj.run(mut src)`); the receiver carries no marker. Calls through function values and closures are checked against the modes in the function type. Built-in, intrinsic, and provider methods are exempt. The one exception is the `軸` marker: the built-in reductions and row operations (`t.sum(軸 n)`, `t.mean(軸 n)`, `rms_norm(軸 n, ...)`, `laminatio(軸 n, ...)`, `softmax(軸 n)`, last axis only) require it, and a bare integer in that position is `SEM057 axis_marker_required`, an `軸` before any other argument is `axis_marker_unexpected`. `軸` is a second kind of marker beside those four: they say how a value is passed, this one says what a number means, and the kind is kept to two words: `軸` for a call's arguments and `size` (la `尺寸`) for the explicit arguments of a call's generic list (`mk<int, size 4>(xs)`), which says an argument is a size and not a type. `size` follows the same echo rule as `軸`: a callee that declares a `size` parameter is called with the `size` marker, a bare natural, shape or size name in that list is `SEM057 size_marker_required`, and `size` on a callee that declares no `size` parameter is `size_marker_unexpected`. A parameter declares it before its type (`軸 int a`), the call echoes it (`t.mean(軸 1)`), and inside the callee the parameter is an ordinary `int`; `軸 n` names the axis that is reduced over. The explicit form `reduce 經由 sum from t at [i, j] 每 i` names the axis that is kept. A missing marker is `SEM057 call_marker_missing`, a different marker is `call_marker_mismatch`, and a marker on a read-only parameter (other than `ref`) is `call_marker_on_read_only`. A temporary passed to a `mut` parameter is `SEM020 mut_argument_not_writable`; a fresh value is accepted for `擁有` and `拷貝` with the matching marker. Call-site `拷貝` is a plain echo of the parameter marker. Parameters take no `var`/`const`; the marker says what the callee may do. An unmarked or `ref` parameter is read-only, a `mut` parameter writes the caller's value (a `mut` tensor parameter may also be rebound whole by `←` on the host; inside a kernel use `⇇`), and an `擁有` or `拷貝` parameter is always writable by the callee (method call, field store, reassignment, element store; an `擁有` tensor parameter may be rebound whole). Only a whole local binding may be passed as `擁有`: a field or element, an unmarked parameter, a module constant, or an `遍歷` binder is `SEM057 own_argument_not_local_binding` / `own_argument_unmarked_parameter` / `own_argument_module_constant` / `own_argument_loop_binder` (`拷貝 b.xs` is fine and does not kill `b`). After `eat(擁有 a)` the name `a` is dead: a later use is `SEM050 use_after_own`, blamed on the call argument (an unmarked source gets a copy suggestion); `moved_in_loop` and `moved_name_captured_by_closure` cover loops and closures; an exclusive (`擁有`/`拷貝`) source keeps `use_after_move` at the later read; a fresh value (`eat(擁有 make())`) is always fine.
- Post-name marker: `可選` (voluntary/optional provision)
- `其餘` marks rest parameter
- Ordinary `函式` declarations and genus methods require bodies. Signature-only methods belong in `待實作介面`. The one exception is a top-level `函式` or a genus method marked `@ 不穩定`, which may omit its block (see `nondum_functio_decl`, `nondum_method_decl`).
- `錯誤 NAME` is a legacy runtime-injected `ignotum` local, and `可拋` is a legacy marker with no current semantic effect. Neither declares the typed alternate-exit contract. New failable APIs should use `⇥ E`; whether either legacy modifier should survive is unresolved.
- `則` is the compact **statement-body** joint only (one-statement `若`/`當`/`分支`/… arms).
- `∴` is the compact **clausura** joint only. The two are not aliases.
- Compact closure block bodies must use `執行 { ... }`; a closure-local `執行` body may attach `捕捉`, but cannot use postfix `當`.

### Classes

A `類型` is a struct with methods. It holds data, its methods act on that
data, and it satisfies contracts through `實作`. It is not a self-contained
object that owns its own construction and process: a value is built with a
construction literal (`Genus { field = value }`).

- **No class inheritance.** Inheritance was removed: there is no `sub`
  (extends) clause and no `abstractus` genus. Shared behaviour comes from
  contracts (`待實作介面` + `實作`) and from composition — a field holding
  another value. The old spellings are rejected with a migration diagnostic.

- **No static methods.** A `類型` declares instance methods only. A function
  about a type is a top-level function in the type's file, reached through the
  import alias. A type-level member is a `定值` constant, never a method.
- **A newtype is a one-field `類型`.** There is no separate newtype
  declaration. Units that need arithmetic wait on operator overloading.
- **No macros and no user derive.** What you read is what runs. Code
  generation, when a project needs it, is an external step before the build.
- **No extension methods and no retroactive conformance, for now.** A type's
  methods and its `實作` contracts are declared on the type itself. Code
  elsewhere cannot add either. Allowing it would need coherence rules, and is
  revisited together with the contract features that are deferred.
- **Contract bounds on type parameters (D1.1-D1.3).** `函式 maior<T 實作 Orderable<T>>(T a, T b) → T`
  bounds a *callable's* type parameter to witnesses that declare that
  contract. Several bounds on one parameter join with `∩` only
  (`<T 實作 Orderable<T> ∩ Equatable<T>>` — never a comma there; a comma
  starts the next parameter). The bound is checked, and its methods become
  callable inside the bounded body, only on a `函式`/method type parameter
  (`generic_bound`); the same clause parses on a `類型`/`型別`/`分支聯集`/
  `待實作介面` type parameter but is rejected there
  (`implet_bound_on_type_declaration`) — those declarations state contracts
  through the genus's own `實作` clause instead (below). Every generic
  contract is written with its type arguments in full — `Orderable<Persona>`,
  `Orderable<T>` — never a bare name (`implet_contract_arity` on a mismatched
  count). Satisfaction stays nominal (D1.3): a witness must declare the bound
  itself.

- **Copy with changes (D15.1-D15.3, D6).** `Genus { field = value, … } 取自 source` builds a new value: the braced fields override, and every other field copies shallowly from `source` (a collection field is shared with the source, not deep-cloned; private fields copy across too). `取自` must start on the closing `}`'s line — a line-leading `取自` is instead the extraction statement (`取自 p 設為 x, y`). Exactly one source is legal (`construction_source_repeated` on a second same-line `取自`); the source must be the same genus type as the constructor. `展開` was removed from construction literals (D15.4); it stays for lists and calls.

### Annotations


`@ 內核 片段` is a modifier on the `內核` annotation (sugar or
braced `片段 = 真` / `假`), not a fused annotation name and not the
graphics `@ 片段` stage. Standalone `@ 片段` is unchanged.

A kernel runs at one of three ordered safety tiers (operator ruling 2026-10-28, `docs/factory/faber-language-decisions/kernel-math-tiers-ruling-2026-10-28.md`). `@ 內核` is the default tier. `@ 內核 寬鬆` (en `@ kernel relaxed`) adds approximate `/` and `sqrt`, fast transcendentals, flush-to-zero and float reassociation. `@ 內核 不安全` (en `@ kernel unsafe`) includes `寬鬆` and also drops the index checks the compiler cannot prove, so an out-of-range access is undefined behaviour. The words are contextual and claimed only after `@ 內核`, so `relaxed` and `unsafe` stay ordinary identifiers elsewhere. A second tier word is `duplicate_nucleum_tier`, an unknown word `unknown_nucleum_tier`, a tier on a `片段` `nucleum_tier_on_fragment` (a fragment uses the tier of the entry that composes it), and a tier on anything but a kernel function `nucleum_tier_non_kernel`. The tier is sugar only; the braced form carries `片段` alone. `faber run --safe`, `faber build --safe` and `safe = true` under `[device]` in `faber.toml` run every kernel at the default tier; they can only lower a tier and no environment variable controls them.

The `車道` clause of the `內核` annotation (`@ 內核 車道 "x"`, braced `@ 內核 { 車道 = "x" }`) was removed (K7): the compiler rejects it with `nucleum_lane_removed`, and `片段` is the only modifier or field. `@ radix 車道` is a different annotation and is unaffected.

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

**Annotation contracts:** `@ annotation` (la `@ annotatio`; optionally `@ annotation { target = fn }`)
marks a top-level `類型` as a compile-time annotation contract. Ordinary genera
are not annotation schemas. Applications use `@ ContractName { field = constant }`
and resolve through local declarations or imported file-interface exports.
Resolved applications lower to `HirAnnotation` with `contract_id: Some(DefId)`
and constant field values. v1 attachment target is `函式` only; payload
scalars are `textus`, `numerus`, `fractus`, and `bivalens` (optional via
`可選` or `T ∪ nihil`). Web, HTTP, controller, and framework route families
are not compiler-owned; they are built as libraries, from annotation contracts
or on top of `@ 端點`. The one exception is `@ 端點` itself: it is the
compiler-owned serving half of `端點` (see Capability Calls).

User annotations are metadata. Their consumers are tools, such as product
packaging. They never change compilation, and Faber code never reads them at
run time. An annotation that changes compilation is compiler-owned (`@ json`,
`@ 端點`, `@ radix`).

**JSON genera:** `@ json` on a `類型` is a compiler-owned data-model contract,
not a generic annotation schema. Fields must be JSON-safe (`textus`, `ascii`,
`numerus`, `fractus`, `bivalens`, `instans`, `nihil`, `lista<T>`,
`tabula<textus, T>`, nullable `T ∪ nihil`, or another `@ json 類型`). Field
metadata `@ json { 名稱 = "wire_name" }` changes the emitted object key used by
`value ↦ valor`, `value ↦ json`, and `json ↦ Genus`; JSON text remains a Norma
wire operation such as `json.pange(value ↦ json)`.

- `@ radix` is **compiler-reserved**: every form under it is compiler-owned
  metadata, not an application surface, and may change with the compiler.
  The historical morphology-stem meaning is retired; morphology remains a
  source naming discipline, not compiler-generated conjugation. The family
  (`radix_annotation` plus the braced records) is:
  - `@ radix 車道 "air"` / `"mir"` / `"hir-direct"` (braced
    `@ radix { 車道 = "air" }`) on top-level functions for explicit
    compiler-lane routing; unsupported lane/target combinations reject with
    diagnostics instead of being ignored.
  - `@ radix backward "name"` on an `air`-lane function names the generated
    reverse-mode gradient companion; it is valid only paired with
    `車道 "air"`.
  - `@ radix 型別 T 傳入 A B …` (braced `@ radix { param = T, allowed = A, … }`)
    restricts the type parameter `T` of the annotated declaration to the listed
    domain.
  Any other directive after `@ radix` is rejected (`unknown_directive`).
- `@ verte` defines codegen transformation (method name or template)
- `@ 不穩定 [TARGET] ["REASON"]` marks a declaration as present in an interface but unavailable for the target; without a `TARGET` it is unavailable on every target. On a top-level `函式` or a genus method the block may be omitted (`@ 不穩定 "pending host dispatch"` then the signature alone): the function is declared, exported and re-exported, any use of it (a call, a call through an imported namespace or name, or the function taken as a value; for a method, a call through an instance, `self`, or an imported genus) is refused with `SEM017 nondum_function_for_target`, and no backend emits it; the genus's other methods are unaffected. A signature-only `函式` without `@ 不穩定` is `PARSE012`
- `@ cli "NAME"` marks an `入口` entry as a CLI program
- `@ imperium "NAME"` marks a function as a CLI command entry point
- `@ optio NAME ...` defines a CLI option; use `型別 bivalens` for boolean flags
- `@ operandus [其餘] TYPE NAME ...` defines a CLI positional argument
- `@ futura` marks a function as async (legacy — prefer `異步` posture word)
- `@ cursor` marks a function as generator (legacy — prefer `流` posture word)
- Callable posture words (`異步`/`流`/`異流`) are recognized in the signature
  slot after modifiers and before `→`/`⇥`/body; bare means synchronous finite
  (`流 T` is a synchronous generator: a call to it has type `cursor<T>`, not
  `lista<T>`; collect with `gen() ↦ lista<T>`)
- `@ 公開` marks a declaration for the file's importable (export) surface; `@ interna` marks it package-internal (same-package importable only); `@ privata` is an explicit module-private marker. Unmarked top-level declarations are module-private by default; a declaration mixing distinct visibility tiers is rejected with `SEM019` (`conflicting_visibility`)
- `@ protecta` is reserved and rejected with a semantic diagnostic; it has no package, subclass, or sibling-file visibility meaning
- `@ doc` is not an annotation. Comments are the documentation: a line comment attaches forward to the declaration it precedes, and there is no doc marker.

- `實作` = implements (conformance to an `待實作介面` contract), written
  with the contract's type arguments in full
  (`類型 Persona 實作 Orderable<Persona>`, D1.2).
- Every `類型` member declares exactly one of `設為` (en `let`) / `變值` (en `mut`)
  / `定值` (en `const`) (D16.1); there is no default — an unmarked field is a parse error: PARSE010
  `field_modifier_missing` (D5c). The `分支聯集` shared-field position
  (`union_member`) keeps today's unmarked form (fork F7 held).
  `設為 T x`: per instance, set only in
  a construction literal (`Genus { field = value }`), never reassigned;
  `Genus { … } 取自 p` copies it unchanged (D16.3), independent of visibility
  (`@ privata` + `設為` is legal). `變值 T x`: per instance, reassignable.
  `定值 T X = …` (en `const T X = …`): the type-level constant, one per type,
  compile-time and read-only, with the same `=` initializer as a module or
  block constant (foldable expressions, `前綴`, `嵌入`). It is read
  through the type name (`Tab.X`); a store to it, through the type name, `自身`
  or an instance, is `SEM020` (`assignment_to_const_member`), a missing `=` is
  `PARSE010` `const_member_requires_initializer` and `←` is
  `const_member_assign_initializer`. A write to a
  `設為` field outside a construction literal is `SEM020`
  (`assignment_to_fixum_field`). The retired `generis` (en `static`) member
  word is `PARSE010 static_member_retired` (there is no alias) and `從` (en
  `ref`) is `ref_field_refused`. The former `nexum` field modifier is removed
  and rejected with a migration diagnostic.
- `類型` members are public by default (D5.2). `@ privata` on a member restricts it to the type's own methods: only code inside the type's own function bodies may read, write, or call it (D5.3); `@ interna` restricts it to code in the declaring package. A construction literal may still set a private field, from any file, and `Genus { … } 取自 p` copies it unchanged (D5.4). Reading, writing, or calling an inaccessible member from outside its allowed scope is `SEM063` (`member_private_read`/`_write`/`_call`, or `member_interna_read`/`_write`/`_call`); `@ 公開` on a member is a redundant-annotation warning `WARN028` (`redundant_member_publica`), an error when warnings are denied.
- A type may refer to itself: `分支聯集 Expr { Adde { Expr sinister, Expr dexter } }`
  and `類型 Nodus { Nodus ∪ nihil next }` need no keyword and no box type.
  Values have reference semantics, so the indirection is implied; a backend
  that stores fields inline inserts it on the fields that close a type cycle.

### Interfaces


`待實作介面` is the **contract** construct: signature-only methods for `實作`
(gerundive of *implere* — that which must be fulfilled). Import namespaces are
`.fab` file boundaries; exported declarations live at file top level.

A contract has no default method bodies. Default bodies would make a contract
an abstract base class without fields. Behaviour shared by every implementer
is a top-level function that takes the contract type. Contract inheritance (a
contract that requires another), associated types, and retroactive
conformance are deferred.

**The one ordering contract, `Orderable<T>` (D1.4).** Norma declares it (`norma:order`) as an ordinary `待實作介面` with one method, `compare(T other) → numerus`: negative, zero, or positive when `self` sorts before, with, or after `other`. A `類型` opts in by naming itself (`實作 Orderable<Persona>`, D1.1-D1.3); satisfaction stays nominal. The compiler recognizes the contract by a mark on its declaration, never by its name: `@ radix contract "ordering"` (C2). That mark is what lets the contract drive language-level behaviour a plain `待實作介面` cannot: **`≺ ≻ ≤ ≥` on a conforming type call its one `compare`**, so the glyphs and `compare` can never disagree; **`numerus`, `fractus`, `textus`, and `instans` conform without any code** (integers by value, floats by IEEE 754 totalOrder so NaN sorts above every number — the bare comparison glyphs on `fractus` stay IEEE, where NaN compares `假`; text by Unicode code point; instants by time); and **tuples order lexicographically** when every element conforms. There is no contract tower and no default method (D1.10): a bound generic uses the contract the same way, `函式 maior<T 實作 Orderable<T>>(T a, T b) → T`. `@ radix` stays reserved for compiler-owned metadata; an application must not write it, and today `"ordering"` is the only recognized role.

### Type Aliases


### Enums


`列舉` (an enum) and `分支聯集` (a tagged union) are **data only** (D9.1): a
`函式` member inside either body is a parse error (`sum_type_function`,
recovered so parsing resumes at the next member), and an `實作` clause on
either header is a parse error (`sum_type_implements`) before the body is even
read. Shared behavior over an `列舉`/`分支聯集` value is an ordinary
top-level function that takes the type, the same posture `待實作介面` already
uses for contract default bodies.

An `列舉` converts without user code (D9.4). A member's discriminant is the
authored number, or the previous member's number plus one; the first member
defaults to `0`. A string-valued member has no discriminant.

- `Ordo ↦ numerus` — the member's discriminant; infallible.
- `numerus ↦ Ordo` — the first member whose discriminant equals the value;
  failable when none matches (`⊥` default, or `textus` propagation).
- `Ordo ↦ textus` — the member's name; infallible.

Other conversion pairs involving an `列舉` fall through to the ordinary
`unsupported_conversio` rejection.

A registered `@ conversio (A, B)` also serves `a ↦ B` for a program's own
error types (see Annotations): a direct (source, destination) pair only, never
auto-composed into a chain, and a missing row fails closed.

### Tagged Unions


Shared fields come first, before every variant. The first shared field must open
with an annotation — `@ commune` (en `@ shared`) in practice — and the fields
after it join the same region with or without one; a bare `T name` before any
annotation reads as a variant. A variant may not redeclare a shared field
(`union_variant_redeclares_shared_field`).

Variant lists are an item list: comma required between variants, forbidden
after the last. Payload fields inside a variant are a declaration block
(genus-style, no commas).

**Union overlap access (D9.2):** a call, read, or write on a field/method name
through a union (`分支聯集` or `∪`) value type-checks when **every**
constituent exposes it with the **same declared type**, then dispatches per
the value's actual member at runtime — access is not restricted to a common
supertype shape. A constituent that lacks the name is `union_member_not_common`;
when every constituent has it but the declared types disagree, it is
`union_member_differs` (each constituent's type is named in the diagnostic).

### Relational Schemas (experimental)

**Experimental** — owned by the `census-types` goal; the surface may change.
`結構 Name { 欄位 T name … }` declares an application-owned relational
heading for database results. It names only the columns the application reads;
extra source columns stay invisible. Each `欄位` row takes a type (use
`T ∪ nihil` for a nullable column) and a name, with an optional
`: sourceName` alias mapping the public column to a source column (absent means
identity). Column rows are a declaration block (no commas), and each row starts on its own line (a second `欄位` on the same line is `schema_nested_column`). A schema has no
methods (`schema_method`), no `實作`
(`schema_inheritance`), and no nested columns (`schema_nested_column`); each is
rejected at parse time.

### Identifier Naming

Faber has no globally reserved words. Keyword ownership is contextual per
spelling: a keyword claims only its owning grammar slot. Every user-chosen
name slot accepts every keyword spelling — declaration names, parameters,
members, binding targets (`定值`/`變值`/`設為` patterns and captures),
import aliases, and loop/iteration bindings. Type-name slots stay out.

Outside a spelling's owning contexts, that spelling may be an `IDENTIFIER`.
An owning context may itself be effectively global when its production
applies everywhere a statement or expression may begin. Builtin claims
(`讀取`/`行`/`格式文字`/`vacua`, and the scribe family in
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

### Modules (`模組`)


`模組 NAME` (en `module NAME`, D7.7) optionally names the file. It is legal only as the file's very first declaration, before any import or other statement, and at most once (a second `模組` is `module_declaration_duplicate`; one that is not first is `module_declaration_misplaced`). The spelling is contextual: `模組` is claimed only in that leading, statement-initial position immediately followed by an identifier, so it stays an ordinary identifier everywhere else (a field, a local, a parameter named `模組`).

The declared name does two jobs. It is the file's **default import name**: `匯入 取自 "library:geo"` binds `geometria` when that file declares `模組 geometria`, instead of the last path segment. Two imports that would default to the same name are a compile error; alias one with `作為`. There is no warning when the declared name differs from the file's own name — the name is never visible on the import line — but an explicit alias (`匯入 取自 "library:geo" geo`) is always available.

It is also the **module doc anchor** (D7.3, D7.6): the comment block directly above `模組` (with no blank line between) is the file's module documentation, replacing the older "first block in the file" rule. A file without `模組` keeps today's behaviour on both counts: the default import name is the last path segment, and the leading comment block attaches forward to whatever follows it.

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

A record import needs its `取自 = "…"` source (`missing_import_source`), and `全部` cannot be combined with `名稱` or `作為` (`mixed_wildcard_and_named_import`).

The five record field names are reader-pack keywords like the rest of the surface: the `en` reader spells `取自`, `可見性`, `名稱`, `作為` and `全部` as `from`, `visibility`, `name`, `as` and `all` (`import { from = "norma:text", name = text, as = t }`), and every other pack spells them with its own words. A field word from another locale is `PARSE050 wrong_locale_import_field`, not `unknown_import_field`.

The `privata` import marker was removed (VM-U3); an import without a marker
does not re-export, and `公開` is the re-export marker. Missing named binding
defaults to the
last import path segment when it is a valid, non-conflicting identifier. If the
inferred name is invalid or collides with an existing top-level binding, spell an
explicit `名稱` or `作為` binding.

**Selective imports** create ordinary immutable local bindings: `匯入 取自 "norma:consolum" 定值 dic 作為 output, funde 作為 output_bytes` imports one exported member per `定值` local. The pre-`作為` identifier names an exported member in the imported file; the post-`作為` identifier is the caller-owned local binding; the imported file interface supplies the complete type. A member may be a value (a function or constant) or a type declaration; the syntax is the same for both. The bindings obey ordinary local-binding rules (duplicates, shadowing, lints), are locale-resolved through the imported module, and are never re-exports. Wildcard members cannot mix into the list. The current parser tolerates one trailing comma after the final member; the canonical spine keeps every comma required.

`匯入 取自 "faber:*" faber` is kernel-specific sugar: the glob lives
inside the import path string and expands the released binary's kernel manifest
into `faber.<module>.<verb>` calls. It is not a wildcard re-export and does not create a runtime aggregate value.

---

## Types


- Declaration parameters (`genericParams`) and applied arguments (`typeArguments`) are distinct grammar categories. Applied arguments admit nested types and static `figura` values. `typeArguments` still admits `NATURAL`.
- A call-site argument list (`callTypeArgs`) carries each explicit size argument behind the `size` marker (la `尺寸`), the way the call echoes `軸` and `mut`/`擁有`/`拷貝`: `take<size 2, size 2>(a)`, `construct<int, size [4]>(xs)`, `f<size n>(x)`. The grammar still parses the bare forms (a natural, a bracketed shape, a dimension term such as `n + 1`) so the checker can name the missing marker: a bare natural, shape, dimension term or caller `size` name in a call list is `SEM057 size_marker_required`, and a `size` argument on a callee that declares no `size` parameter is `SEM057 size_marker_unexpected` (one past the declared `size` parameters is an arity miss, `wrong_number_of_type_arguments`). The list binds by marker: a `size` argument binds the next `size` parameter of the callee and an unmarked argument the next type parameter, the two cursors advancing independently, so `mk<int, size 4>(xs)` binds `fn mk<T, size N>` and a list of only `size` arguments (`construct<size [4]>(xs)`) leaves every type to inference. The operand decides the binding time: `size 4` and `size [2, 3]` are compile-time witnesses and a shape travels as one argument (`tensor.construct<int, size [4]>(xs)`, checked against its store as a scalar size is: `tensor<int, [5]>` is `SEM010 incompatible_tensor_index`); `size n` of an in-scope integer stays the run-time extent described above, checked at its store; `size D` of a caller `size` parameter forwards it. A shape inside a call list holds natural literals only: a `_` hole, a name, a run-time lift or a nested term in it is `size_shape_dimension_unsupported`, and `size [..]` outside a call's list is `size_shape_outside_call_args`. An unmarked name or `_` stays a type there; the callee decides which type parameter it binds.
- Applied `NATURAL` arguments are `尺寸` capacity facts, not width markers. Shipped bounded forms use that slot: `lista<T, 尺寸 N>`, `queue<T, 尺寸 N>`, `stack<T, 尺寸 N>`, `textus<尺寸 N>`, `ascii<尺寸 N>`. The `尺寸` marker is required on all five: a bare `queue<T, N>`, `stack<T, N>`, `textus<N>` or `ascii<N>` is retired by name (`SEM008 capacity_needs_size`; a bare list count is `SEM008 list_count_needs_size`). The `_` hole stays bare. Width markers such as `i32` and `f32` stay the separate `widthTypeSugar` production below.
- **Convert hints are not type arguments (D11.9).** A hint (`Hex` / `Bin` / `Oct` / `Be` / `Le` / `Code`) is a `經由` clause on the `↦` conversion, never a further argument of the target type (see Runtime conversion). The retired spellings are parse errors with a pointer at the clause: a hint as a further type argument of a scalar head (`ascii<N, Hex>`, `littera<Code>`; the wrapped numeric heads `numerus<W, Hex>` and `fractus<f64, Be>` are rejected whole as `numeric_wrapper_retired`, see Sized primitives) is `conversio_hint_type_argument`, and a bracketed hint tail after the target (`lista<b8, 16><Le>`, `vector<u32, 尺寸 4><Be>`) is `conversio_hint_tail_argument`. Only scalar heads are checked, so a user type named like a hint stays a legal argument of a collection target (`↦ lista<Code>`).
- Type arguments admit the hole forms: `lista<∪>` infers a heterogeneous element union and `tabula<K, ∪>` a heterogeneous value union; `lista<_>` keeps the monomorphic single-inhabitant hole.
- Explicit generic call-site lists use the same `typeArguments` production: `id<_>(x)` is a type hole (equivalent to omitted `id(x)` for a one-param callee), and mixed lists such as `both<_, textus>(a, b)` are legal. Arity stays exact (`both<_>` is still one argument). `∪` in that list is rejected (`explicit_union_type_arg_unsupported`): a callee type param is a monomorphic witness slot.
- `labeledTypeArgument` is the optional label prefix on `元組` type arguments only (`元組<gx: f32, T>`; mixed labeled/unlabeled legal). A label in a non-`元組` list (`f<gx: T>(x)`, `lista<gx: T>`) is a parse error. Absence is the only unlabeled form; there is no `_: T` spelling. Keyword spellings are legal labels under the contextual law (`元組<定值: A>`).
- Labels are unique within one tuple type.
- The tuple type is spelled `元組<…>`, not `(K1, K2)`. Parentheses already
  mean grouping, function types, parameters, and calls. Every other compound
  type is `name<args>`, and tuple labels come from the same type-argument
  machinery.
- Labels are erased from type identity: `元組<gx: A, B> ≡ 元組<A, B>` for assignment, `≡`/`↦`, unify, and every emitter.
- Bracket index on a tuple requires a literal integer (`i[0]`); every element is reachable by position, labeled or not. Non-literal index expressions stay rejected. Positions are brackets only — no `.0`.
- Member-by-label (`i.gx`) requires that label to be present on the receiver's `元組` annotation.
- `元組` element slots admit `_` (monomorphic hole, solved element-wise from the single position witness) and reject `∪`. A wanted union element is declared with binary cup (`元組<f32, textus ∪ nihil>`). `lista<∪>` / `tabula<K, ∪>` keep heterogeneous-union behavior. Labels compose with holes (`元組<loss: _, T>`).
- `ratio` type arguments require a label for every element, labels are unique, `_` is admitted as a monomorphic element hole, and `∪` is rejected in an element slot. A `ratio` has no positional or bracket access, and it has no structural equivalence with another ratio or a genus; fields are accessed by label only.
- Arrays are written `lista<T>` (unbounded, shipped). Postfix `T[]` is not accepted. `lista<T, 尺寸 N>` is the shipped bounded form; see Generic Collections.
- `從`/`傳入`/`擁有`/`拷貝` mark ownership on the type they prefix: one union member, or a standalone `∪` hole. There is no grouping parenthesis in type position — `(` opens a function type and nothing else, so `(A ∪ B)` is a parse error (`PARSE001`); write the marker on the member (`從 A ∪ B`).
- Two hole kinds share the `holeType` production. `_` is the monomorphic hole ("infer exactly one inhabitant type"); the standalone `∪` is the union hole ("infer a finite multi-member union"). Both are legal wherever a base type is: bindings, returns, params, fields, and type arguments (`lista<∪>`, `tabula<K, ∪>`, `→ ∪`).
- **Lone-`∪` rule:** a `∪` hole consumes the whole type expression — any following `∪` is a parse error (`A ∪ ∪`, `∪ B` rejected, issue `unexpected_cup_after_union_hole`). `_` keeps today's behavior and may still appear as a binary-cup member (`_ ∪ B`).
- **Binary-cup disambiguation:** `∪` between two non-hole types remains the inline value-union operator (`A ∪ B`, nullable `T ∪ nihil`); the hole reading applies only when `∪` stands alone in a base-type position.
- Inline union `T ∪ U` (cup) for ad-hoc value unions; `T ∪ nihil` is the canonical nullable type form (lowers to Option<T>).
- Inline intersection `T ∩ U` (cap) is the nominal type intersection: `type Reversible = Readable ∩ Seekable` names the conjunction, and the implements clause accepts `∩` as the same separator as the comma (`class A implements Readable ∩ Seekable` ≡ the comma list). `∩` binds tighter than `∪` (`A ∩ B ∪ C` is `(A ∩ B) ∪ C`); nested intersections flatten like unions. Intersection operands are nominal-only (interfaces/structs; aliases resolve through) — primitive operands are rejected at lowering. Implements slots admit `∩` only: `∪` or a hole in an implements position is a parse error (disjunctive conformance is not a checkable contract).
- Signature clauses stay explicit: `_` and a standalone `∪` are rejected in return (`→ _`) and error-channel (`⇥ _`) positions; both holes stay legal in local binding slots (`const _ v`, `const ∪ v`).
- Unions are parsed as a flat member list; duplicates and `nihil`-only cases are diagnosed in semantic lowering.
- `可選` is a declaration marker (post-name on params/fields), never a prefix on types.
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
| `textus<尺寸 N>` | shipped; bounded Unicode string; `N` is a `尺寸` / `NATURAL` capacity, not a width marker. `textus<_>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `ascii`    | ASCII-only string |
| `ascii<尺寸 N>` | shipped; bounded ASCII string; `N` is a `尺寸` / `NATURAL` capacity, not a width marker. `ascii<_>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `littera`  | en `char`; one Unicode scalar value (D10.1–10.2): a 4-byte value that never allocates (Rust `char`, Go `rune`). Element of `textus` / `ascii` iteration and of `textus[i]` / `ascii[i]` indexing. Grapheme clusters are norma library work, not this type. |
| `forma`    | captured template + params |
| `numerus`  | integer (default `i64`) |
| `模數<W>` | en `wrapping<W>`; modular word, signed or unsigned (N7e); a store reduces modulo 2^W |
| `saturatus<W>` | en `saturating<W>`; saturating integer; a store clamps at both ends of W |
| `exactus<W>` | en `trapping<W>`; the trapping policy spelled out (D11.8, N7a): the same type as the bare marker `W`, and a store traps when the value does not fit |
| `inf` | the unbounded integer (D11.5): a width marker in the `numerus` family with no upper or lower bound, spelled `inf` in every locale (no keyword). `inf`, `exactus<inf>`, `模數<inf>` and `saturatus<inf>` (en `trapping<inf>`, `wrapping<inf>`, `saturating<inf>`) all name this one type; the wrapped `inf` is retired. **Shipped:** the type, big literals, the join, store and conversion rules, exact run-time arithmetic, and the host-only rejections, on the MIR runner, Rust, TypeScript, Go and Python, and in part on the Racket (`sexp`) target. A target with no unbounded carrier (Swift, Haskell, LLVM, Wasm) fails closed with a named diagnostic, and Metal, WGSL and AIR never carry it; see The unbounded integer `inf`. |
| `fractus`  | float (default `f64`) |
| `bivalens` | boolean |
| `nihil`    | null |
| `vacuum`   | void |
| `numquam`  | never |
| `ignotum`  | unknown |

Bare `textus` / `ascii` remain the unbounded productions. The
shipped forms `textus<尺寸 N>` and `ascii<尺寸 N>` take
one `尺寸` / `NATURAL` applied argument. That `N` is capacity, not a
width marker and not a language-wide default. `_` in that slot (`ascii<_>`,
`textus<_>`, `lista<T, _>`) is a capacity hole: the form stays
bounded, and `N` is inferred from a same-family bounded witness. Bare
`ascii` is not a hole.

Capacities and extents are buffer bounds, so a capacity or extent value may arrive at compile time or at run time (`尺寸` means one
thing everywhere; gpu-reset rule 11). **Admitted, scheduled (FLD K14), not shipped:** today every capacity and extent must be a
compile-time value or inferred from a witness. Under K14 the same syntax accepts a run-time-origin size, a `_` in a capacity or extent
position means inferred if possible and otherwise bound at run time, and a size relation that cannot be proven statically is checked at
the call boundary as a recoverable error, never a silent reshape. Type parameters, element types, numeric widths, tensor rank and
layout, `vector` and `matrix` register shapes, and `atomic<T>` stay compile-time; a whole-shape `_` must still resolve its rank at
compile time.

**A byte string is `lista<b8>`**: the list of raw
bytes; it is not a list of numbers, so `lista<b8>` converts to `lista<u8>`
only with an explicit `↦` and never widens to `lista<int>` or `lista<b16>`.
`ascii<尺寸 1>` is an ordinary ASCII string of
length one (the type of `'x'`), not a separate character type; it widens
implicitly `ascii<尺寸 1> → littera → textus` (D10.5), so `s[i] ≡ '\n'` keeps
working across the chain.

A sized numeric type is written as its **bare width marker** (not a user type parameter): `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `d64`, `inf` (the integer family) and `f16`, `bf16`, `f32`, `f64` (the float family). The three policy words wrap a marker and keep their `<W>` argument:

| Family | Markers | Invalid example |
| ------ | ------- | --------------- |
| `模數<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, and `inf` (the same type as `inf`) | `模數<f32>` or `模數<d64>` → a modular word takes an integer width |
| `saturatus<W>` | the same eight integer widths, the float widths `f16`, `bf16`, `f32` and `f64`, and `inf` (the same type as `inf`) | `saturatus<d64>` → a decimal takes only the trapping policy |
| `exactus<W>` | the eight integer widths, `d64`, the float widths `f16`, `bf16`, `f32` and `f64`, and `inf` | none: every width marker is valid; a float cell checks only where a non-finite value is stored or converted into it |

Bare `numerus` / `fractus` remain shorthand for `i64` / `f64`.
The bare marker (`i32`, `f32`, `d64`, `inf`) is the canonical spelling of a sized
numeric type. The wrapped spelling `numerus<W>` / `fractus<W>` (en `int<W>` /
`float<W>`, and the same words of every locale pack) is **rejected** at parse
time with `numeric_wrapper_retired` (PARSE040), which names the form written and
the bare replacement. This covers family-correct forms (`numerus<i32>`),
wrong-family forms (`numerus<f32>`, `fractus<i32>`), `numerus<d64>` and
`numerus<inf>`, the removed `numerus<d32>`, extra arguments, and the marker holes
`numerus<_>` / `fractus<_>` (write bare `numerus` / `fractus`, or a bare marker).
`inf` is the one marker with no range: it is integer-only (not a float width),
and an unbounded integer has no word to wrap or clamp at, so `模數<inf>` and
`saturatus<inf>` are accepted and change nothing.

`d64` is the one **decimal** width, for money and accounting
(there is no narrower decimal width). A decimal literal in a decimal context (`d64 a ←
4.2`) keeps its digit text, and `d64` is the scaled integer `i64` × 10⁻⁸: eight
fraction digits and a range of ±92,233,720,368.54775807, so `4.2 + 0.1` is
exactly `4.3`. Arithmetic is exact until the store, the same model as integers:
`+` and `-` are exact, `*` is exact and its scale grows (scale 8 × scale 8 is
exact at scale 16), and `/` rounds half-even to the larger operand scale, all
in a wide intermediate bounded by a 128-bit carrier at its scale (past it the
operation traps). The `d64` slot applies its policy where the value lands: it
rounds **half-even to scale 8** and traps when the value leaves the range, so
`amount * rate * (1 + tax)` rounds once, at the store; per-step rounding is
written as separate stores. `d64` takes only the trapping policy
(`saturating<d64>` and `wrapping<d64>` are rejected: a clamped money amount is
silently wrong). The `d` marker is integer-family only: `d64` is not a float
width. Integer literals in a decimal context are rejected
(`decimal_integer_literal_rejected`); write `1.0` or convert explicitly with
`↦`, as for every crossing between number families. A decimal literal with more
than eight fraction digits into `d64` is a compile error: a written literal is
never silently changed, while a computed value is rounded by the slot. Display
(D2.6): with a `¶` spec the value prints exactly as the spec says (`12.5 ¶
".2"` is `12.50`, rounding half-even when the spec cuts digits); without one
(`print`, `§` holes) it prints the shortest form with trailing zeros dropped,
`12.5` and `12`, never `12.50` or `12.0`. A decimal stores its value only, with
no per-value scale.
`模數<_>`, `saturatus<_>`, and `instans<_>` are marker holes:
the family stays identity and only the width/precision is inferred from a
same-family witness (exact marker, no lattice widening). Unsolved `_` is an
error, never the bare default. The wrapped holes `numerus<_>` and `fractus<_>`
are retired with the wrapped numeric spelling (`numeric_wrapper_retired`). A
convert hint is never a type argument, so there is no hint hole; hints are
`經由` clauses.

### Numeric model

The numeric rules below are D11.1–D11.8 and the operator rulings of
2026-09-29/30 (delivery spec `d11-6-widening-delivery.md` §3). They apply to
scalars on the host; tensors and kernels follow the same store rule
per element, with the device profile of ruling 18.

**Exact values, checked stores.** Integer arithmetic computes the exact
mathematical result; an expression is a number, not a container. Every
intermediate of bounded operands must lie in one 64-bit range, [−2⁶³, 2⁶⁴ − 1]
(it fits some 64-bit integer, signed or unsigned); outside it the operation
traps. The only way past that cap is an operand typed `inf`, the opt-in
unbounded integer (see The unbounded integer `inf`). Overflow is therefore
observed only where a value **lands in a
typed slot**, and every such store applies the slot's policy: declaration,
assignment, `↑`/`↓`, field, argument, `傳回`, `讓出`, collection element, and
the other store positions of the spec (a `print`, a `§` hole, a `¶`, a
comparison or a condition has no slot and never traps for size). `x * 3 / 2`
with `x: u8 = 100` computes 150 and fits; with 200 it computes 300, which traps
at the store, not at the multiply. A check is omitted only where the compiler
proves the value fits. A value known at compile time is checked at compile
time.

**Slot policies.** The policy lives in the type, read once at the declaration:

| Family | Policy at the store | Use |
| ------ | ------------------- | --- |
| bare marker `W` (default) | **traps** if the value does not fit | counts, sizes, money, indices |
| `模數<W>` (en `wrapping<W>`) | **reduces** modulo 2^W | hashes, checksums |
| `saturatus<W>` (en `saturating<W>`) | **clamps** to W's bounds, once, at the store | pixels, audio, levels |

`saturating<u8>` with `x = 250` and `x + 200 - 100` stores 255, not the 155 that
clamping each step would give; per-step clamping is written as separate stores
into `saturating` slots. This departs from Rust `Saturating<T>` deliberately.
`模數` reduces only at the store too (operator ruling 2026-10-02: math
happens in the ether): `(a + b) / 2` with `wrapping<u8>` 200 and 100 is
`300 / 2 = 150`, `a + b ≡ 44` is falsum and `print a + b` prints `300`. For
`+ - * ⇐ ∧ ∨ ⊻ ¬` that feed a store directly, reducing once at the end equals
reducing each step, so, for number types, a backend may keep per-operation
modular arithmetic there, where no one can observe the difference (raw bits
`b8`..`b64` are not numbers and act at their width, `docs/design/numeric-model.md`
section 8); the operand of `⇒`, `/`, `%`
and a comparison is read, so it is exact. Ported hash and crypto code keeps its
results by storing into a `wrapping<W>` slot before dividing, shifting right or
comparing. Within one policy
family a store into a narrower width applies the slot's policy
(`wrapping<u32>` into `wrapping<u8>` reduces); crossing policy families needs
`↦` (superseded 2026-10-27 by ruling 51, ruled and not yet built: a value read
from any cell is a number and the destination's policy applies to the exact
value whatever the source's policy; `docs/design/numeric-model.md` section 8,
*Store policy belongs to the destination*; until it is built the compiler still
refuses a store from another policy). A constant stored with `←` follows the slot's policy
(`saturating<u8> w ← 300` is 255, `wrapping<u8> w ← -1` is 255, and a trapping
slot's certain trap is a compile error); a constant in an `=` position
(a `定值 T X = e` constant at module level, in a block or as a `類型` member, field
default, enum member) must fit `W` whatever the policy; a construction field
written with `=` is the same fact (ruling 51, not yet built for named constants
and construction fields). Under ruling 51 a compile-time-known value (a literal,
a named constant or an expression made only of those) is checked
at the store at compile time: a misfit into a trapping cell is a compile error and
a `模數<W>` or `saturatus<W>` cell applies its policy.
The unbounded integer `inf` (D11.5) is a type (see its subsection below), so a
bounded expression still obeys the 64-bit range above and an `inf` slot never
applies a size policy.

The D11.8 naming frame puts the policy outside and the representation inside:
en `trapping<W>`, `wrapping<W>`, `saturating<W>`; la `exactus<W>`, `模數<W>`,
`saturatus<W>`. A bare marker takes its domain's default policy (`u8` is
`trapping<u8>`; integers and `d64` trap, floats follow IEEE).

**Shipped (N7a, N7e):** the trapping policy word (`exactus<W>` / en
`trapping<W>`, integer widths and `d64`), bare markers in every type position,
and signed widths on `模數<W>` — `wrapping<i8>` reduces into the signed
range, so `100 + 100` stored into it is −56. **Shipped (float cells,
2026-10-01):** `saturating<F>` and `trapping<F>` (la `saturatus<F>`,
`exactus<F>`) over `f16`, `bf16`, `f32` and `f64`, each its own type. A float
cell follows its container: the policy applies where a value lands in the cell,
never inside an expression, and no operation traps. A bare float and
`saturating<F>` behave alike under IEEE: an overflow gives ±infinity, NaN
propagates, and nothing clamps to the largest finite value. Only `trapping<F>`
errors, and only when a non-finite value is stored or converted into it.
`模數<F>` (en `wrapping<F>`) is still rejected: a modular word takes an
integer width. The trapping float cell is not available on device targets.

**Ranged store policies.** A store policy takes a range as a
second argument after the width: `saturatus<u32, 0‥8>` (en
`saturating<u32, 0‥8>`) stores into `u32` and clamps into `[0, 8)`;
`exactus<i32, -2‥3>` (en `trapping<i32, -2‥3>`) traps outside `[-2, 3)`;
`模數<u32, 0‥8>` (en `wrapping<u32, 0‥8>`) reduces modulo the range's
length, offset by its low end, so `10` stored in `wrapping<u32, 0‥8>` is `2`.
The range is its own type-argument slot (`range_argument`), told apart from a
type or a `size` argument by the range glyph after its first bound.

- **The width stays explicit.** The representation, the ABI and the kernel index
  envelope are the width's; the range refines inside it and must fit it. Both
  ends are always written (`0…` alone is `PARSE040 range_bound_required`).
  `‥` is half-open (`0‥8` holds `0` through `7`) and `…` is inclusive (`0…7`).
  `∞` and `-∞` mean no bound on that side (`saturating<i32, -∞‥8>`); `∞` stays
  illegal as an integer value. Unsigned widths need a low end of at least 0;
  signed widths take negative ends (`-2‥3`). A range the width cannot hold, an
  empty range, a named or `size` end (not built: `ranged_policy_bound_not_constant`)
  and a float, decimal or `inf` width are named `SEM008` refusals.
- **The policy word is mandatory.** `u8<0‥8>` is
  `SEM008 ranged_policy_word_required`; the edge behaviour is always visible.
- **Each existing form is sugar for one explicit form.** `saturating<u8>` is
  `saturating<u8, 0…255>`, a bare `u8` is `trapping<u8, 0…255>` and `inf` is
  `trapping<inf, -∞…∞>`. A range that is the whole width is the plain form:
  the two spellings are one type, and no existing program changes.
- **Arithmetic stays unbounded; the store applies the range.** A value read from
  a ranged cell computes as its width, so `cx + 1` is exact and
  `saturating<u32, 0‥8>` mixes with any other `saturating<u32, ..>` value in
  arithmetic. Only a store into a ranged cell (declaration, assignment, argument,
  return, field, element, `↑` `↓`) applies the policy once; storing a value of
  another range is a store. A constant stored with `←` follows the policy, and a
  trapping cell's certain trap is a compile error. The policy families do not
  mix implicitly: a plain integer reaches a `saturating` or `wrapping` ranged
  cell through `↦`, which reduces or clamps and cannot fail, exactly as for the
  unranged words; `↦` into a `trapping` ranged cell is a checked narrowing and is
  failable. (Superseded 2026-10-27 by ruling 51, ruled and not yet built: a value
  of any policy stores into a ranged cell and the destination range applies, so
  `↦` is not needed there, though it stays legal.)
- **Where it is built.** The MIR runner and the Rust target apply a range;
  the Faber source emitter keeps the authored form (`faber format` prints a
  range half-open); every other target and every kernel refuse the type by name
  (`SEM010 ranged_policy_target_unsupported`, `SEM008 nucleum_ranged_policy_not_built`).
  Float ranges, index containment and kernel facts are the later parts of the
  series.

**List size ranges.** After the `尺寸` marker (en
`size`) the same range slot is the second argument of `lista`:
`lista<numerus, 尺寸 4‥8>` (en `list<int, size 4‥8>`) holds four to seven
elements and `lista<numerus, 尺寸 1…∞>` is never empty. The marker says the
range bounds a count: a bare range bounds a value (`exactus<u8, 0…255>`), and
`lista<T, 4>` or `lista<T, 4‥8>` is retired by name
(`SEM008 list_count_needs_size`). A list range is
trapping only and takes no policy word: `saturatus<lista<T>, …>` and the other
policy words around a list are `SEM008 ranged_list_policy_word`. `lista<T>` is
`尺寸 0…∞` and `lista<T, 尺寸 N>` is `尺寸 0…N`, one type each. A push past the maximum and a
removal below the minimum report on `⇥`, a `↦` into a range checks the length
and takes `⊥`, and `primus` / `ultimus` of a range with a floor of one return
`T`. Both ends are integer literals or `∞` (`ranged_list_bound_not_constant`),
non-negative (`ranged_list_bound_negative`) and the range holds a length
(`ranged_list_range_empty`); a literal outside it is
`SEM011 ranged_list_length_out_of_range`; a target that does not lower it is
`SEM010 ranged_list_target_unsupported`.
**Shipped (N7c/N7d):** the
retirement of the long forms: the canonical emitter writes the bare marker and
the parser rejects `numerus<W>`/`fractus<W>` (en `int<W>`/`float<W>`) with
`numeric_wrapper_retired`; the policy words keep their `<W>`.

**Implicit and explicit failure differ.** A failed implicit store is a trap of
its own identity: it never enters the `⇥` channel, even inside `執行 … 捕捉`,
and its message names the value, the destination type and the slot (for an
inferred slot, the expression the type came from). Only an explicit `↦` is
recoverable (`⇥`, `⊥`, `陷阱`). `⊥` never catches a trap.

**Expression types: the range rule.** The type of a trapping integer
expression is the smallest integer type that holds every possible result,
computed by interval arithmetic from the operands' declared types and never
from the destination. With `u8` operands `a + b` and `a * b` are `u16`, `a - b`,
`-a` and `¬a` are `i16`, and `a / b`, `a % b`, `a ⇒ n`, `a ∧ b` and `a ∨ b` are
`u8`. Only trapping types grow. A `模數<W>` or `saturatus<W>`
operand takes part by its declared width and gives the same range-rule type: the
word reduces or clamps only where a value is stored into a slot, never
mid-expression (number types only: raw bits `b8`..`b64` act at their width,
`docs/design/numeric-model.md` section 8). Growth stops at the 64-bit containers: past them the
type keeps the sign of the range (`i64` if it can be negative, else `u64`), so
`u64 - u64` is `i64` (operator ruling 2026-09-30: it does not become `inf`;
write `a ↦ inf - b` for the exact difference). `_` slots take the expression's
type (`定值 _ t ← a + b` with `u8` operands is `u16`); a collection literal
with no declared element type, a `✓ ✗` conditional and `求和` take theirs from
the same rule. The one exception to the growth cap is an operand typed `inf`:
see The unbounded integer `inf`.

**Untyped constants.** A literal, or an expression made only of literals, is
an exact number with no type. Beside a typed operand its value joins that
operand's range; in an annotated slot it takes the slot's type and must fit at
compile time (`定值 u8 d ← 10 - 100` is a compile error); otherwise it
defaults to `int`. A constant of any length is an
exact number: an integer literal has no upper bound (see The unbounded integer
`inf`), and where it may land is decided by the slot. Beside a float operand it is checked once: an integer
constant must be exactly representable (`x + 1` with `x: f64` is legal, 2⁵³ + 1
is a compile error), a constant beyond the float's finite range is a compile
error, and a decimal literal rounds to the nearest float.

**Implicit widening is lossless only.** Integer widenings that hold every value
stay implicit (`u8 → i16`); `u64` has no bounded target and requires `↦`, and
its one implicit target is `inf` (every integer width widens into `inf`, which
widens into nothing). Crossing number
families (integer, `d64`, float) always needs `↦`, in arithmetic and at stores:
`定值 fractus f ← n` with `n: i32` needs `n ↦ f64`. `u64` with a typed signed
operand is a compile error in every join (arithmetic, `✓ ✗` branches, `∧ ∨ ⊻`,
collection literals, `求和`): `u64_signed_arithmetic_requires_conversion`,
fixed with `↦` (to `i64` or to `inf`). Untyped constants are exempt (`x - 1` with `x: u64` is fine).

**Division.** `/` is the programmer's division and `÷` the mathematician's. On
integers `a / b` is ⌊a / b⌋ and `a % b` is `a − b·⌊a / b⌋`, which takes the
**divisor's** sign: `7 / 2` is 3, `-7 / 2` is −4, `-7 % 2` is 1, `7 % -2` is
−1. The only failure is a zero divisor. Floor is the mathematical division
(`x % 2 ≡ 1` holds for every odd `x`, and `/` agrees with `⇒`); code ported from
C, Java, Rust or Go changes its results on negative operands. `/` on floats is
IEEE division. An operation's type is fixed by its operands, never by the
destination: `定值 fractus avg ← a / b` with integer operands is a compile
error (`integer_quotient_to_float_requires_true_division`) whose help points at
`÷`.

`a ÷ b` is real division and never yields an integer, including between
constants. On floats and `d64` it equals `/`. On integers the result is the
smallest float that represents every value of both operand types exactly,
never below `f32`:

| Widest integer operand | `÷` result |
| ---------------------- | ---------- |
| `i8`, `u8`, `i16`, `u16` | `f32` |
| `i32`, `u32`, `i64`, `u64`, default `int` | `f64` |

Mixed widths use the wider operand (`i8 ÷ i32` is `f64`). Operand types are the
range-rule types (`(a + b) ÷ c` with `u8` operands keys on `u16`); an untyped
constant joins by value (`u8 ÷ 2` is `f32`) or defaults to `int` alone (`7 ÷ 2`
is `f64`, 3.5). `f16` is never chosen implicitly. `÷` has `/`'s precedence and
associativity and the same glyph in every locale. It is not exact: `1 ÷ 3`
rounds, and `i64`/`u64` values above 2⁵³ round even in `f64`. An integer zero
divisor traps; float operands keep IEEE (`x ÷ 0.0` is ∞). It has no method
twin. The same result type applies per element on tensors.

**Bit operations and shifts are pure math.** `∧ ∨ ⊻ ¬` and unary `-` compute the
exact value on infinite two's-complement integers, so `¬x` is `-x - 1` (`¬250`
is −251, which traps when stored into an unsigned slot; `flags ∧ ¬mask` still
works). Fixed-width complement is what `wrapping<W>` is for (`¬x` on
`wrapping<u8>` 250, stored into a `wrapping<u8>` slot, is 5). `x ⇐ n` is `x * 2ⁿ` and `x ⇒ n` is `⌊x / 2ⁿ⌋`. The
count is not masked to a receiver width: `x ⇒ n` past the value's size is 0 (or
−1 for a negative `x`) and never traps, `x ⇐ n` traps only past the 64-bit
range (never on an `inf` operand), on `wrapping<W>` it wraps at the store, and a
negative count is an error (a compile error for a constant). The count may be
any integer type.

**Comparisons are exact across families.** `≺ ≻ ≤ ≥ ≅ ≇` accept operands from
different number families with no `↦` and compare the true mathematical values
(`i64 ≺ f64` is exact even above 2⁵³; NaN compares false). `≈`/`≉` compute in
the float operand's width. `≡`/`≢` stay structural and exact-type, so
`1 ≡ 1.0` is rejected. (Ruling 51, 2026-10-27, ruled and not yet built: between
integers of any width and policy `≡`/`≢` compare by exact value, as `≺` does;
across number families they are unchanged.) A comparison stores nothing, so the family-crossing rule
does not reach it.

**Conversion.** `↦` is the checked, recoverable form (D1.11: `∷` states only
what the compiler can prove, and `↦` is a check). Into a trapping integer type
it is a magnitude-checked narrowing that fails through `⇥`, `⊥` or `陷阱`. Into
a `wrapping<W>` type it reduces the exact source value modulo 2^W, and into a
`saturating<W>` type it clamps it; neither can fail and neither takes a `⊥`
(integer and `d64` sources). `fractus ↦` an integer width `W` saturates at the target
width, NaN converting to `0` (the cross-tier Rust `as` status quo); integer
`W` arithmetic traps on overflow while float→integer conversion
clamps. Overflow policy lives in the type. For number types there are no per-operation
checked, wrapping, or saturating method families (raw bits `b8`..`b64` act at
their width, `docs/design/numeric-model.md` section 8). To ask "does this fit?" of untrusted
input, convert it to the narrow type with `↦` and handle the failure through the
error channel. The `inf` rows are in The unbounded integer `inf`.

**AIR.** AIR (`@ radix 車道 "air"`) has no representation for a trap, so in an
AIR-lane function an integer store is admitted only when the range rule proves
it fits, and an operation whose exact intermediate could leave the 64-bit range
is rejected the same way. A store that would need a runtime check is a compile
error naming the store; declare a wider slot, or write `↦` with a `⊥` default.
There is no exemption. An `inf` type is rejected in an AIR-lane function
outright (`air_unbounded_integer`): AIR has no representation for a heap value.

**The unbounded integer `inf` (D11.5; F9 rulings 32–50, operator-ruled
2026-09-30).** `inf` is the opt-in integer with no range: every integer is a
value, ∞ and NaN are not (`inf` has no upper bound; ∞ is not one of its
values). It is never a default and is never inferred from bounded operands; an
author writes `inf` in a slot or converts with `↦ inf`. Its rules in full:

- **Spelling.** `inf` is a width marker in the `numerus` family, written the
  same in every locale: it is not a keyword and has no glossary word, and, like
  `u8`, it is reserved in type position only. `inf`, `trapping<inf>`,
  `wrapping<inf>` and `saturating<inf>` (la `exactus<inf>`, `模數<inf>`,
  `saturatus<inf>`) are one type; the policy words are accepted and never
  produce a wrapping or saturating word. `faber format` keeps the author's
  spelling among them. `∞` remains the IEEE float literal and is never an `inf`
  value (`定值 inf x ← ∞` is a compile error); a float ∞ prints as `inf`, the
  same three letters, by the long-standing float print rule.
- **Literals.** An integer literal may have any number of digits in decimal,
  `0x`, `0o` and `0b` forms. A literal, or an expression made only of literals,
  is an exact untyped constant whatever its size, folded exactly. It lands
  where its exact value fits: in an `inf` slot, or beside an `inf` operand,
  always; in a bounded slot, beside a bounded operand, or as the default `int`,
  only if it fits that range, else `numerus_literal_out_of_range` (so
  `定值 _ x ← 18446744073709551616` is a compile error and
  `定值 inf x ← 18446744073709551616` is legal). A `分支` constant pattern on
  an `inf` subject takes a big literal. A position that names a size or a
  code rather than a value (capacity, tensor extent, `出口` code, `測試`
  count, enum member value) keeps the `u64` range: a longer literal there is a
  parse error.
- **Join.** An operand typed `inf` makes the result `inf` for every integer
  operator (`+ - * / % ⇐ ⇒ ∧ ∨ ⊻`, unary `-` `¬` `√ ∛ ∜`, `potentia`, `求和`, `✓ ✗`
  branches, collection literals). An untyped constant beside an `inf` operand
  joins by exact value. Nothing else changes: bounded operands keep the 64-bit
  cap, and `u64 - u64` stays `i64` (it does not become `inf`).
- **Widening.** Every integer width, `u64` included, widens implicitly into
  `inf` (`定值 inf x ← u` needs no `↦`); `inf` widens into nothing. Crossing
  families (float, `d64`) still needs `↦`.
- **Arithmetic.** Exact and never a size trap: `+ - *` do not trap; `/` is
  floor and `%` the floor remainder; `∧ ∨ ⊻ ¬` act on infinite two's
  complement; `x ⇐ n` is `x · 2ⁿ` and `x ⇒ n` is `⌊x / 2ⁿ⌋` with no cap;
  `potentia` is exact; `÷` is true division in `f64`. The only failures are a
  zero divisor, a negative shift count or exponent (the existing traps), and
  exhaustion of memory, which is a resource fault: fatal, never the `⇥` channel,
  never caught by `捕捉`. The language sets no upper bound; an implementation
  may (the MIR runner has a configurable bit-length ceiling).
- **Comparison and keys.** `≺ ≻ ≤ ≥ ≅ ≇` compare exact mathematical values
  against any integer width, `d64` or float (±∞ order beyond every integer);
  `≡ ≢` stay exact-type (`inf ≡ i64` is rejected; superseded for integers by
  ruling 51, ruled and not yet built: they compare by exact value). An `inf` value is hashable
  and totally ordered, so it is a valid `tabula` key and `copia` element.
- **Stores.** A store into an `inf` slot is total and emits no check. A store
  from an `inf` value into a bounded trapping slot is an implicit checked
  narrowing: it traps (`implicit_store_out_of_range`, never `⇥`), unless a
  constant is proven to fit. `inf` is in the trapping family, so a store into a
  `wrapping<W>` or `saturating<W>` slot needs `↦`, which reduces or clamps the
  exact value and cannot fail (superseded 2026-10-27 by ruling 51, ruled and not
  yet built: an `inf` value is a number and a `wrapping<W>` or `saturating<W>`
  slot reduces or clamps it with no `↦`; `↦` stays legal). `saturating<u64> hi; hi ↑` at the bound still
  traps; the clamp is written `((hi ↦ inf) + 1) ↦ saturating<u64>`.
- **Conversion `↦`.** Any bounded integer, including a word, converts to `inf`
  and never fails. `inf ↦` a trapping width is a magnitude-checked narrowing and
  is failable (a handler is required except for a proven constant); into
  `wrapping<W>` / `saturating<W>` it reduces / clamps and cannot fail. `inf ↦`
  a float rounds to nearest-even and yields ±∞ beyond the float's finite range
  (a constant beyond it is a compile error); a float or `d64` `↦ inf` needs a
  rounding choice, `經由 Floor|Ceil|HalfEven|Trunc` (toward −∞, toward +∞,
  nearest with ties to the even neighbour, toward zero); the bare conversion is
  `SEM016 float_to_integer_requires_rounding`. The conversion is failable and
  reports on the error channel for NaN and ±∞ (a handler or `⊥ default` applies,
  as for other failable numeric conversions). `inf ↦ d64` is range-checked and
  failable. `textus`/`ascii ↦ inf` accepts an
  optional sign and digits of any length (failable on malformed input; `經由
  Hex|Bin|Oct` as for other integers); `inf ↦ textus` writes the decimal digits.
  `inf ↔ lista<b8> 經由 Be|Le` is the minimal two's-complement encoding and its
  exact inverse. `inf ↦ bN` is rejected (no fixed width), and
  `inf ↦ littera 經由 Code` is not a row (write `x ↦ u32 ↦ littera 經由 Code`).
  `inf ↔ valor`/`json` carries the integer exactly.
- **Host only.** `inf` has no device layout. `tensor`, `sparsa`, `vector` and
  `matrix` reject an `inf` element (`tensor_element_unbounded`; use
  `lista<inf>`), a kernel rejects an `inf` parameter, return, local or field
  (`nucleum_host_type`), and an AIR-lane function rejects every `inf` type
  (`air_unbounded_integer`).
- **Collections and loops.** `lista<inf>`, `tabula<inf, V>`, `copia<inf>`,
  tuples, `inf ∪ nihil`, genus fields, variant payloads and generic
  instantiation at `inf` are ordinary. In `遍歷 範圍 a‥b` the binder takes the
  join of the bounds (an `inf` bound gives an `inf` binder).
- **Display.** `print`, a `§` hole in a template and a composite print show the
  decimal digits with a leading `-` for a negative, with no grouping or suffix;
  the `¶` integer specs apply as for `int`.

**What is shipped.** `inf` is shipped; every rule above is checked at compile
time and computed exactly at run time. The front end accepts the type in all
four spellings, the host-only rejections, big literals and their slot rule, the
join, widening, store and conversion typing rules, and comparisons. Literal-only
float expressions fold exactly before the slot rounds them. Run-time
semantics are carried per target, on an unbounded integer of the target's own:

- **Supported.** The MIR runner (the oracle), Rust, TypeScript, Go and Python
  run the arithmetic, comparison, conversion and display rules above; the
  runner also runs the `遍歷` binder over `inf` bounds. The runner, Rust and TypeScript also carry
  the `lista<b8> 經由 Be|Le` and `valor`/`json` rows with every digit; Go carries
  `valor ↦ inf` and `lista<b8> 經由 Be|Le`. The Racket (`sexp`) target carries the
  arithmetic and comparison rows.
- **Named gaps.** Python has no `↦ valor` and no `lista<b8>` route; the Racket
  target lacks formatted display, genus printing and `↦ valor` (as it does for
  every type); Go fails closed on a few container and intrinsic constructs
  holding an `inf`; TypeScript keeps bounded `int` and `u64` as numbers, so a
  bounded `u64` slot past 2⁵³ still traps there. Each gap is a named
  compile-time diagnostic or a documented trap, never a bounded substitute.
- **Fail closed.** Swift, Haskell, LLVM and Wasm have no unbounded carrier and
  reject `inf` with a named diagnostic (`inf_target_unsupported` on Swift and
  Haskell, `llvm_target_inf_unsupported`, `mir_wasm_unsupported`). The language
  does not change to fit them. Metal, WGSL and AIR never carry `inf`: it is host
  only, rejected by language rule before emission.

The per-target rows with their open gaps are kept in the target capability
matrix and the numeric model; this file states only the language.

### Generic Collections

| Faber          | Meaning  |
| -------------- | -------- |
| `lista<T>`     | array    |
| `lista<T, 尺寸 N>`  | shipped; bounded array; `N` is a `尺寸` / `NATURAL` capacity, not a width marker (a bare `lista<T, N>` is `SEM008 list_count_needs_size`). `lista<T, _>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `queue<T>`     | shipped; unbounded FIFO queue |
| `queue<T, 尺寸 N>`  | shipped; bounded FIFO queue; `N` is a `尺寸` / `NATURAL` capacity, not a width marker. `queue<T, _>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `stack<T>`     | shipped; unbounded LIFO stack |
| `stack<T, 尺寸 N>`  | shipped; bounded LIFO stack; `N` is a `尺寸` / `NATURAL` capacity, not a width marker. `stack<T, _>` is the capacity hole (infer `N`; otherwise run-time bound — admitted, scheduled (K14), not shipped). |
| `tabula<K,V>`  | map      |
| `tabulae<K,V>` | ledger: a map that keeps its entries in insertion order (en `ledger`); `tabula` and `tabulae` sit one letter apart on purpose, "two shades of the same thing" (Roman account books were `tabulae`). Parses and type-checks in every type slot a map is accepted; every operation on a ledger value is refused (`ledger_operation_not_implemented`) until the operations land. |
| `copia<T>`     | set      |
| `promissum<T>` | promise  |
| `cursor<T>`    | iterator |
| `tensor<T, Figura>` | dense homogeneous buffer whose shape `Figura` is part of the type: element type and rank are static, and each extent is a size that is a compile-time value today (shipped) and may be bound at run time once K14 lands (admitted, scheduled, not shipped); numeric methods require numeric element types |
| `vector<T, 尺寸 N>` | register-class numeric vector with static width `N` (single dimension, not buffer-backed). The long form writes its lane count after the `尺寸` marker, as a list writes its count: a bare `vector<T, 4>` or `vector<T, N>` is retired by name (`SEM008 vector_width_needs_size`, a clean break with no alias). The `vf32[4]` shorthand, a bracketed `vector<T, [4]>` and the `_` hole are unchanged: the brackets already mark an extent |
| `matrix<T, [R, C]>` | register-class numeric matrix with exactly two static dimensions (not buffer-backed and not a tensor alias) |
| `atomic<T>` | storage-sensitive atomic cell; v1 accepts `i32` / `u32` elements only and access must go through atomic methods |
| `sparsa<T, Figura>` | sparse homogeneous buffer whose shape `Figura` is part of the type (element type and rank static; extents compile-time today, run-time-bindable once K14 lands — admitted, scheduled, not shipped); omitted coordinates equal zero; numeric methods require numeric element types |
| `trapped<R, T>` | built-in discriminated union of a `陷阱` capture boundary: `returned` carries `value: R`, the block's trailing expression, and `thrown` carries `error: T`, the error type or union of error types that can escape; `R` and `T` are separate slots, so a `string` success with a `string` error is legal; read it with an ordinary `比對` (en `match`) |
| `erratum<T>` | en `error<T>`; the compiler-owned error carrier: one field `payload: T` and nothing else (no message, origin or cause). It is what `⇥ T` means underneath and what `捕捉` binds around; a nested `⇥ erratum<T>` is mechanical expansion. Reader spellings come from the locale packs (`error` in en, `erratum` in la). |

A `figura` is `_`, a natural number, a size identifier, or a bracketed list of nested figura values; empty `[]` is rank-0. Dimension arithmetic admits addition or subtraction of natural constants and division by positive natural constants: `[N-1]`, `[D/2]`, and `[(D-2)/3]`. Division binds more tightly than addition or subtraction. Size division is exact, so a negative extent, zero divisor, or nonintegral result is invalid; it does not use ordinary integer floor division. Bare `tensor<T>` is incomplete — use `tensor<T, []>` for rank-0 or `tensor<T, _>` to infer shape. A spelled lift marks a run-time extent: `size N` (la `尺寸 N`) takes a name, a natural, or a parenthesized figura term (`size (n - 1)`); the extent binds at run time and its value is checked at the store that observes it.

An expression shape list can contain `_`, as in `row.expanded([_, 64])`. These structural holes are valid in tensor shape arguments only. A shape witness or the expected tensor type must supply their extents; they do not introduce a scalar placeholder value.

Extents follow the same binding-time rule as capacities (see the capacity paragraph above): an unmarked extent is a compile-time value and a `_` extent infers from a witness; a spelled lift (`size N`, the `spelled_lift` rule) marks a run-time extent, bound at run time and checked at its store. Rank and layout stay static.

`vacua` is the default empty instance of the declared type on the left, whatever that type is. A list, map, or set gets the empty collection. A dense `tensor<T, Figura>`, `vector<T, 尺寸 N>`, or `matrix<T, [R, C]>` gets its zero, so `tf32[2, 3] blank ← vacua` means exactly `tf32[2, 3] blank ← tf32[2, 3] {}`: the shape is the declared shape and every element is zero. That holds wherever the type is known (a declaration, an assignment target, a parameter, a return). Every extent must be resolved, as for the typed zero: a `_` extent, a non-literal vector width or matrix extent, or an affine extent (`[N+1]`) leaves nothing to build and is refused with `SEM011 vacua_requires_resolved_extents`. `vacua` for `tensor<T, []>` produces a rank-0 tensor (one default-initialized element slot, its zero). `vacua` for `sparsa<T, Figura>` (any shape) produces an all-zero sparse tensor with no stored entries.
`matrix<T, Figura>` requires exactly two dimensions; bare `matrix<T>` and one- or three-axis matrix shapes are rejected.
`atomic<T>` requires `T` to be `i32` or `u32` in v1. Atomic cells are not interchangeable with their element type; use `load`, `store`, `exchange`, and `compare_exchange` receiver methods.
Construct multi-dimensional tensors via `structa` / `↦`, or build the typed zero with `Type {}` (below).

**The typed zero `Type {}`.** The zero of a type is written as the type followed by empty braces; it is the `typed_constructor` rule (`Genus { field = value }`) applied to every type: `{}` means build a value and override nothing, so every part takes its default. It is legal for `tensor<T, Figura>`, `vector<T, 尺寸 N>`, `matrix<T, [R, C]>` and their prefixed sugar (`tf32[2, 3] {}`, `vf32[4] {}`, `mf32[2, 2] {}`), including a tensor whose element is a vector (`tensor<vector<f32, 尺寸 4>, [H, W]> {}`) and extents that are size parameters; a vector width and a matrix extent must be literals (`constructor_register_extent_requires_literal`). It is a value expression, so it can be a receiver (`tf32[4] {}.reple(1.5)`). Where the type is already declared, `vacua` is sugar for it: `T x ← vacua` and `T x ← T {}` are the same value for a tensor, vector, or matrix `T`. The bare type as a value (`← tf32[2, 3]`) is retired and refused with `PARSE030 constructor_requires_empty_braces`; `Type []` is refused by the same diagnostic; `lista<…> {}` is refused by name (`constructor_type_not_claimed`); a call-form constructor is refused (`constructor_store_position_only`). The stated bracket constructor `lista<T>[1, 2]` builds a list whose element type is `T` and may leave a type-argument slot as a hole its elements solve (`lista<_>[1, 2]` infers the element, `元組<_, string>[1, "x"]` the first tuple slot); a slot the elements cannot solve is a named error (`cannot_infer_empty_list_element_type` for `lista<_>[]`, `expression_type_mismatch` for `元組<_, string>[1, 2]`). Like every constructor it is a store-position form, so `f(lista<int>[1, 2])` is `constructor_store_position_only` and the value must be bound to a name first, while `lista<…> {}` stays refused by name. A bracket literal is the same constructor with the type left out: the bare `[1, 2]` is a hole with a default, so in a typed cell it takes the cell's bracket type — `const lista<int> xs ← [1, 2]`, `const copia<int> s ← [1, 2]` (en `set<int>`) and `const 元組<int, int> t ← [1, 2]` build the list, the set and the tuple the cell declares; a set collapses its duplicates, and `_ [1, 2]` is the hole with no default that must be solved from context (`const copia<int> s ← _ [1, 2]`). There is no fill form: data enters a typed value by conversion (`↦`), never by the constructor. **The inferred constructor `_ { … }`.** With the hole type `_` before the braces the constructor builds the expected type, so the type is not written; it takes fields (`_ { name = value }`), entries (`_ { "key": value }`, the `json_literal` members) or the empty `{}`, and without an expected type it is `SEM016 inferred_constructor_no_expected_type`. The braces alone carry the same meaning: `{ field = value }` leaves the type slot out, which implies the hole, and the cell supplies the type; the bare form claims as a constructor only when its head member is a `field = value` init, while `{"key": value}` stays a JSON literal. An empty `{}` or `[]` has no contents to borrow a default from and needs a typed cell (`const copia<int> s ← []`, `const lista<int> xs ← []`, `const json j ← {}`); with no typed cell it is `SEM016 inferred_constructor_no_expected_type`, and `vacua` (en `empty`) is the contents-free spelling of the same empty value wherever the type is declared. A stated type before a keyed literal builds that type (`json { … }`, `map<K, V> { … }`), a bare keyed literal needs a typed cell. `*` is the field-list hole, legal only as the sole argument of `record<...>` (la `ratio<...>`): `record<*>` is the whole field list, solved from the shape, `record<g: f32, *>` is `ratio_field_list_hole_not_sole`, and `record<_>` stays `ratio_type_arg_label_required`; a bare `{ field = value }` with no typed cell is `record<*>` (a record), while `_ { … }` has no default.
`Type(...)` is not a construction form: `vector<f32, 尺寸 4>(...)`, `matrix<f32, [2, 2]>(...)`, `tensor<f32, [2, 2]>(...)`, and scalar forms such as `numerus("42")` are rejected. Use `value ↦ Type`, named library constructors, or `Genus { field = value }` records.

Tensor index/shape intrinsic slots (`accipe`, `ponde`, `forma`, `crea`, `structa`) accept integer lists that fit the canonical `lista<numerus>` / `&[i64]` runtime boundary at call sites (e.g. `lista<u32>` for GPU thread ids; not `lista<u64>`). This is a structural exception scoped to those slots — it does not widen the signed↔unsigned numeric lattice (see Index vector parameter policy in `tensor-intrinsics.md`).

Value unions use inline `T ∪ U` (nullable: `T ∪ nihil`). The standalone `∪` hole infers a multi-member union; `_` infers a single inhabitant (see `docs/design/type-hole-union.md`). Tagged unions use `分支聯集`.
`copia.unio()` is a set method, not a type constructor.

### Type Sugar

The bare width marker (`u32`, `f32`, `d64`, `inf`) is the canonical spelling of a
sized numeric type, and `lista<u32>` is the canonical collection form. The wrapped
`numerus<W>` / `fractus<W>` form is rejected (`numeric_wrapper_retired`). Type
sugar (`lu32`, `tf32`, …) is an ergonomic alternate spelling for collection
types. It is **type-position only** and **semantically identical** to the long
form — the compiler treats both the same. This is the single canonical
reference for sugar; the rest of the specification uses the long collection form.

Sugar combines a width marker with an optional one-letter family prefix. Width
markers are `i8`/`i16`/`i32`/`i64` (signed), `u8`/`u16`/`u32`/`u64` (unsigned),
and `f16`/`f32`/`f64` (float); `inf` (the unbounded integer) is a bare marker
only. A bare width marker (no prefix) sugars the scalar
numeric type; a family prefix sugars a collection of that width. In the grammar,
`WIDTH_MARKER` is a bare marker; `LISTA_WIDTH_SUGAR`, `TENSOR_WIDTH_SUGAR`,
`SPARSA_WIDTH_SUGAR`, `VECTOR_WIDTH_SUGAR`, and `MATRIX_WIDTH_SUGAR` are that
marker prefixed with `l`, `t`, `s`, `v`, and `m`, respectively.

| Sugar | Long form | Bracket rule |
| ----- | --------- | ------------ |
| `i8` … `u64`, `f16`/`f32`/`f64`, `d64`, `inf` | none: the bare marker is the type (the wrapped `numerus<W>` / `fractus<W>` long form is rejected, `numeric_wrapper_retired`) | none (bare marker) |
| `lf32`, `lu32`, `li64`, … | `lista<f32>`, `lista<u32>`, `lista<i64>`, … | none |
| `tf32`, `tf32[2, 3]`, `ti64[N]` | `tensor<f32, _>`, `tensor<f32, [2, 3]>`, `tensor<i64, [N]>` | optional `Figura` |
| `sf32`, `sf32[2, 3]`, `si64[N]` | `sparsa<f32, _>`, `sparsa<f32, [2, 3]>`, `sparsa<i64, [N]>` | optional `Figura` |
| `vf32`, `vf32[4]`, `vu32[3]` | `vector<f32, _>`, `vector<f32, 尺寸 4>`, `vector<u32, 尺寸 3>` | optional single width |
| `mf32[4, 4]`, `mf16[2, 2]`, `mu32[3, 3]` | `matrix<f32, [4, 4]>`, `matrix<f16, [2, 2]>`, `matrix<u32, [3, 3]>` | **required**, two dimensions |

Bracket shapes: `[]` is rank-0, `[2, 3]` is a fixed shape, and no bracket infers
the shape (`_`). Matrix requires exactly two dimensions. Sugar never uses `<>`.
For non-width element types (e.g. `tensor<textus, [3]>`), use the full form.

Sugar is reserved in type syntax only — value identifiers named `tf32`, `lf32`,
etc. are unchanged. `inf` takes no prefix: `linf`, `tinf`, `sinf`, `vinf` and
`minf` are not sugar and stay ordinary identifiers (`sinf` and `linf` are
common names, and a tensor, sparsa, vector or matrix element may not be `inf`).

`模數<W>`, `saturatus<W>` and `exactus<W>` have no sugar; write
`模數<u32>` / `saturatus<i16>` / `exactus<u8>` in full (the bare marker `u8`
already is the trapping `u8`).

**Spelling preference (author convention, not grammar):** general Faber code
tends toward the long collection form (`lista<u32>`) for readability; numeric/tensor-primary modules may
prefer sugar. Choose per module or file.

---

## Control Flow

### Conditionals


- `若` = if, `否則若` = else-if, `否則` = else. `否則若` takes its condition
  directly (`若 a { … } 否則若 b { … } 否則 { … }`); `否則若 若 b` and `否則 若 b`
  are parse errors.
- `c ✓ a ✗ b` is the one value conditional: `a` when `c` holds, else `b`.
  `✓` (U+2713 CHECK MARK) and `✗` (U+2717 BALLOT X) are the same in every
  locale and have no word twin. It is one level only: a `✓ ✗` inside the
  condition or either branch is rejected (`conditional_nested`); choose among
  more values with a function whose `若` arms each `傳回`. The branches narrow
  exactly like `若` branches (after `r 是 numerus`, `r` is `numerus` in the
  `✓` branch).
- `c ? a : b` and `c sic a 否則 b` (en `c yields a else b`) were removed and
  are rejected with a migration diagnostic; write `c ✓ a ✗ b`. `sic` stays a
  reserved word only to carry that diagnostic. The look-alikes `✔` and `✘` are
  rejected with a "did you mean" hint.
- `則` for one-statement bodies, including `則 傳回`, `則 拋出`, `則 崩潰`, and `則 靜默` (`∴` is not accepted here)
- `靜默` for explicit no-op (from musical notation: "it is silent")

### Loops


- `當` = while
- `遍歷 取自...設為`/`遍歷 取自...變值` = for-of (values)
- `遍歷 從...設為`/`遍歷 從...變值` = for-in (keys)
- `遍歷 範圍 range 設為/變值 i` = range iteration (e.g. `遍歷 範圍 0‥10 每 2 設為 i { 註記 i }`; `每` belongs to the range expression)
- In an `遍歷 ... 執行緒` lane body, `傳回 執行緒` (en `return thread`) ends only the current lane from any depth, including inside nested ordinary loops. Lanes are independent, so plain `傳回` stays forbidden there; `傳回 執行緒` is forbidden outside a lane and inside a nested closure or reduction body. It takes no value: lanes publish results through their stores. `繼續` (en `continue`) also ends the current lane when used at that lane's top level.

**Range step and direction (`range_tail`, `每`).** The bounds alone pick the
direction of a range: `a‥b` and `a…b` count up when `a <= b` and count down
when `a > b`. The optional `每` step is a *positive stride* applied in
whatever direction the range moves, so `10‥0 每 2` yields `10 8 6 4 2`, `0‥10
每 2` yields `0 2 4 6 8`, and `10…0 每 5` yields `10 5 0`. A step is never
signed: a zero or negative step is an error, a compile error
(`range_step_not_positive`) when the step is a literal and a run-time trap
otherwise. Equal bounds walk the ascending way (`5‥5` is empty, `5…5` is the
single value `5`). The step never changes which endpoint a range includes:
`…` includes its end only when the progression reaches it.

A range value carries its step in its type: `range<T>` has step 1, and
`range<T, S>` carries the compile-time constant `S > 0`, as in
`let range<int, 2> evens ← 0…10 step 2`. A step in a value position must be a
positive integer literal; a run-time step remains inline in `for range` and
bracket windows and is rejected elsewhere (`intervallum_value_step_unsupported`).
A stepped range takes integer bounds only; an `instans` bound takes no step
(`intervallum_step_bound_not_integer`). The grammar admits a stepped range as
a clamp target (`↦` or `coercere`), in `inter` and `union`, and in equality
across different steps, but these are named rejections, never silent drops of
the step.

A range binder declared `變值` is a fresh per-iteration copy of the walk's
counter. A write to it inside the body (`遍歷 範圍 0‥6 變值 i { i ← i + 1 }`)
changes only the body's copy and never steers the loop, so the example visits
`0 1 2 3 4 5`. In an `遍歷 範圍` product each binder is refreshed once per
iteration of its own axis.

**Iteration order.** A type whose order is part of its value iterates in that
order. `lista` iterates by index. `textus` iterates its characters in order.
`tabulae` (the ledger) iterates its entries in insertion order; updating the
value of an existing key keeps the key's position, and removing a key then
adding it again appends it. `tensor`, `vector`, and `matrix` iterate by index,
outer axis first (row-major). Two equal values always iterate identically; for
a `tabulae` the order is part of its value, so two ledgers with the same
entries in a different order are not equal.

`copia` and `tabula` iterate in unspecified order. The order is not promised
and not deliberately random; backends may differ. When order matters, sort
explicitly with the sort expression (`排序 經由 升序 取自 xs`; en `sort 經由 asc from xs`, and `sort 經由 asc ref m` for a map's keys), or use a `tabulae`. `≡` on these types stays structural and does
not depend on order.

**Set display.** `print`, a `§` hole in a template and a composite print show a
`copia` in the list shape: `[3, 7]` for a `copia<numerus>` and `["x"]` for a
`copia<textus>`. Each element renders exactly as it renders inside a `lista`,
through the display tokens of the code locale (`true`/`false`/`none`/`tuple`
in `en`, `真`/`假`/`nihil`/`元組` in `la`); an empty set prints `[]`.
The element order is the set's iteration order above: unspecified, not
promised, and not sorted by the print. A program that needs a stable printed
set sorts a list first (`s ↦ lista<T>`, then `.sort()`). A conforming target
prints the same list shape up to element order.

There is no iteration interface. `遍歷 取自` works on the built-in iterable
types and on cursors. A user type that should be iterable exposes an ordinary
method that returns a cursor (`遍歷 取自 arbor.nodi() 設為 n`); nothing is
called implicitly.

**Changing the collection inside its own loop.** The rule is about the shape of
the collection being walked (how many entries, in what order), not about what
is inside an entry.

- A method that changes the shape or order of the collection, called directly
  on that collection inside its own loop, is rejected
  (`SEM011 loop_collection_shape_change`). That covers `lista` `appende`,
  `extende`, `decapita`, `detrahe`, `inverte` and `ordina`, `copia` `adde` and `dele`, and
  `tabula` `pone` (even when it would only update a value) and `dele`. A
  worklist that must see its own additions uses a queue with `當`.
- A bracket store is not checked at compile time. `xs[i] ← v` and `m[k] ← v`
  are accepted inside the loop. A bracket store that adds a key to a `tabula`
  or `tabulae` during a loop over it traps with `map changed size during
  iteration`, the same on every target.
- A change through another name for the same collection, through a method on
  an object that holds the collection, or through a function the collection is
  passed to is not guarded at compile time or at run time. Behaviour may differ
  between targets: on an append, some targets cover the entries present when
  the loop started and others also visit the new entries; on a removal, a
  target may skip an element or trap. Do not file such a difference as a
  defect; it is a known, accepted gap that may be closed later.
- Updating the elements themselves is never covered by the rule. A loop over a
  `lista` of objects that passes each object to a function as `mut` keeps
  working.

### Switch/Match

`比對` is a statement, not an expression. A value chosen by a match comes
from a function whose arms each `傳回`. The compiler checks exhaustiveness
and definite return, and the function can be tested on its own.

Coverage is checked as a pattern matrix. Each scrutinee has a space: the
variants of an `列舉` or `分支聯集`, the members of a union, and `bivalens`
as the closed set `{真, 假}`. A match over several scrutinees is
checked over their product, so `比對 a 且 b` over two `bivalens` values
needs all four combinations or a `預設`. A missing variant or combination is
an error that names one uncovered case. The multi-subject form parses today —
subjects are comma-separated, and an arm's patterns are separated by `,` or
`且` (`分支 真 且 假`) — and its coverage is checked over the product,
but its lowering is **admitted, not shipped** (D22.4, the `dms` unit): the Rust
emitter lowers it, while the MIR runner, TypeScript, Go and Haskell reject it (for example `unsupported MIR lowering: multi-subject 比對 before
switch MIR lowering`). Open types (`numerus`, `textus`, …)
are complete only with a catch-all arm. When coverage cannot be computed for a
pattern kind, the compiler warns that it was not checked; it is never silent.
`選擇` keeps its switch meaning: over an open domain, a missing `預設` is
an implicit no-op default, while a closed domain is checked.

### Pattern Matching

Patterns are flat. A `分支` arm names one variant and binds its fields, or names one literal
value; it does not match inside those fields. Nested patterns are left out for
simplicity, not because they cannot be checked: a `比對` inside an arm is
two flat exhaustive switches.

An arm names a member of the scrutinee's closed set. A bare name that is not a
member is rejected (`SEM010 discerne_arm_not_member`); it never becomes a
binding, and the diagnostic names the set's members and the nearest member
spelling when one is close. `_` matches anything, and `預設` is the default
arm; those are the only two wildcards. How the parser classifies an arm name
internally (a type name, a variant name, or a plain identifier) is not part of
the language.

A negative number pattern is written with a leading minus (`分支 -1`,
`分支 -∞`). The lexer never signs a number, so the pattern claims the sign;
`-` before anything else is not pattern syntax.

`比對` matches a closed set and nothing else: the variants of an `列舉` or
`分支聯集`, or the members of a union (`分支 numerus 設為 n` over
`numerus ∪ textus`). It is not a generic "match this thing" keyword. A type
pattern that is not a member of the scrutinee's closed set is rejected
(`SEM010 discerne_pattern_not_in_closed_set`). That covers numeric-width
patterns (`分支 u32` over a `numerus`) and length-shaped patterns (`分支
lista<numerus, 尺寸 4>` over a `lista<numerus>`; bounded `textus` and `ascii`; tensor figures). Ask an integer's width or range with an `是` test,
and ask a length with `.longitudo()` in a `若`.

There are no range patterns (`分支 1‥5`). Test the range with `若` inside the
arm.

A NaN pattern is rejected. NaN never equals itself, so it could never match;
test for NaN with `若` instead.

### Guards

Match arms have no guards. `比對` is one arm per variant, and a guard
would split one variant's logic across several arms. Nest a `若` in the arm
instead.

### Destructuring Extraction

Destructuring is flat. A nested pattern such as `[[a, b], c]` is rejected;
destructure the outer value, then the inner one on another line.

Parameters are not destructured. A pattern in a parameter slot would hide the
parameter's type from a type-first signature. Destructure in the body.

### Control Transfer

`中斷` and `繼續` take no label. They apply to the nearest enclosing loop.
A nested search that needs an early exit from an outer loop becomes a
function that `傳回`s.

- `等返` awaits a compatible promise and returns its success value from a
  `異步` function.
- `等棄` awaits a compatible promise to completion and discards any success
  value.
- `讓出` is statement-initial yield from `流` / `異流`; it is not an
  expression-form await.

---

## Error Handling


- `捕捉` attaches to the structured forms whose productions name `catchClause`: conditional arms, `當`, `遍歷`, `選擇`, and `執行`. It does not attach to arbitrary bare blocks.
- Use the explicit do block when a standalone block needs a handler: `執行 { ... } 捕捉 err { ... }`.
- `拋出` = throw (recoverable), `崩潰` = panic (fatal).
- A same-line `若 <expr>` guard on `拋出` and `崩潰` is line-sensitive parser sugar: `拋出 val 若 cond` desugars to `若 cond { 拋出 val }` at parse time. Its canonical, compression-safe spelling is the expanded `若` block. A source compressor must expand this sugar before removing line breaks; the guarded shorthand remains under language review.
- `斷言` is a runtime invariant check. It desugars conceptually to `崩潰 "msg" 若 !cond`, with the positive condition kept in source form and the inversion applied during lowering. The optional particle is `崩潰` (en `panic`): `斷言 cond 崩潰 msg` / `assert cond panic msg`. Bare `斷言 cond` stays legal. An `斷言` failure is fatal and uncatchable by `捕捉` (it lowers to a panic, not a `Result`-channel error); in test context the harness isolates each `測試` so a failed assertion ends that test without ending the suite.
- `需要` is the recoverable require statement (en surface `require … throw …`), the typed-error-channel twin of `斷言`. `需要 cond 拋出 err` desugars to `若 非 (cond) { 拋出 err }` at lowering; the thrown value enters the function's `⇥ E` channel and is catchable by `捕捉`/`執行`, unlike `斷言` (fatal). A `需要` statement in a `⇥`-less function is a compile error, same as `拋出`. The particle is `拋出` (en `throw`) and is required.

- `拒絕` is the reject statement (en surface `reject … throw …`), the boolean opposite of `需要`. `拒絕 cond 拋出 err` desugars to `若 (cond) { 拋出 err }` at lowering — it throws when the condition holds, where `需要` throws when it fails. The thrown value enters the function's `⇥ E` channel and is catchable by `捕捉`/`執行`. A `拒絕` statement in a `⇥`-less function is a compile error, same as `拋出`. The particle is `拋出` (en `throw`) and is required.
- `@ conversio` (en `@ conversion`) on a top-level `函式` declares an admitted error conversion: the parameter's type is the source error, the return type is the destination, and the compiler enrolls that ordered pair so a propagating `⇥ E` failure converts at the boundary instead of needing a per-caller wrapper. The marker is bare and the conversion is an ordinary function outside any union body; only a direct (source, destination) row is admitted — a missing row fails closed and is never auto-composed into a chain. The earlier union-arm form (the marker carrying a payload inside a `分支聯集` body) is retracted.
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

**Radical prefixes (`√` `∛` `∜`):** the radical glyphs are spaced prefix sugar
and bind like unary `-` / `¬`, so `√ x + 1` is `(√ x) + 1`. `√` is sugar for
`.sqrt()` exactly where that method is legal (floats and float tensors; an
integer operand is the `.sqrt()` error), `∛` is the real cube root of the new
`.cbrt()` (`∛ -8.0` is `-2.0`), and `∜` is `.sqrt().sqrt()` (NaN for negatives,
as `sqrt`). They are locale-invariant math glyphs with no word twin.

**Tensor lifting (FLD K4, K5):** the scalar operators lift to tensors elementwise with no grammar change. Shipped: `+` and `-` (binary and unary) against a scalar or an equal-shape tensor, `*` by a scalar, `/` and `%` by a scalar, `÷` on any shape (with the per-element result widths of the [Numeric model](#numeric-model)), `⤒`/`⤓` tensor against tensor, the comparisons `≺ ≻ ≤ ≥ ≅ ≇` (each yields a `tensor<bivalens>`; `≡` and `≢` compare whole tensors and yield one `bivalens`), the logic words `且` / `或` / `非` on `tensor<bivalens>`, the `✓ ✗` select with a `tensor<bivalens>` condition, and `或取` when the elements are nullable (`tensor_coalesce_element_nullable_required` otherwise). The math methods `abs sqrt cbrt exp ln log10 否則若 cos tan` (Latin `absolutum radix radix_cubica exponentia logarithmus logarithmus_decimalis sinus cosinus tangens`) lift the same way; the float functions need float elements, and a user function is never lifted (`tensor_function_not_lifted`). Tensor `≈`/`≉` are deferred (`tensor_approx_comparison_deferred`), and `*` or `/` between two tensors (or any two aggregates) is rejected naming the glyph (`tensor_multiply_requires_glyph`, `elementwise_division_requires_glyph`): write `⊙ ⊘` per element or `·` for contraction. Lifting runs on the MIR runner (the math methods also lower on Rust); every other emitter fails closed (`tensor_lift_unsupported_on_target`). A numeric `list` takes the same per-element comparisons `≺ ≻ ≤ ≥ ≅ ≇` against a list of equal length or a scalar and yields a `list<bivalens>`; `≡` and `≢` stay whole-value, and a list of bits has whole-value equality only (`lista_bits_whole_equality_only`).

**Division (`/` and `÷`):** both bind at the multiplicative tier with `*`,
left-associative. `/` floors on integers and `%` takes the divisor's sign; `÷`
is true division and yields a float (`f32` for 8- and 16-bit integer operands,
`f64` otherwise). See [Numeric model](#numeric-model).

**Extrema (`⤒` / `⤓`):** `a ⤒ b` is the maximum and `a ⤓ b` the minimum of
two values. They are pure arithmetic operators at the additive tier with `+`
and `-`, left-associative: `a ⤒ b ⤓ c` is `(a ⤒ b) ⤓ c`.

**Copy-into (`⇇`):** `target ⇇ value` copies every value of `value` into the existing storage of `target`. The storage of `target` keeps its identity: `←` only ever rebinds a name, and `⇇` is the one way to write into a tensor that already exists (a `mut` parameter, a `var` local, a field of a writable root). `⇇` binds above assignment and below ternary, so every binary operator, postfix call, and conversion on the right finishes first: `output ⇇ (q · kt) ⊙ s` copies the whole product. It is a statement and its result is `vacuum`, so it cannot be chained or used as a value (`a ⇇ b ⇇ c` is `copy_into_chain`; `x ← a ⇇ b` is `copy_into_value_used`). The left side must be writable tensor storage: a `const` or a non-`mut` parameter is `copy_into_target_immutable`; a scalar, list, matrix, or vector is `copy_into_target_not_tensor` (a value type is written by `←`); a view, including `out.sectio(…)`, is `copy_into_target_view` (a view target is a future question, not an admitted form). Both sides have the same element type and shape. A static mismatch is a compile error (`copy_into_type_mismatch` for the element type, `incompatible_tensor_index` for the shape); extents known only at run time are a recoverable runtime error value, never a kernel-level trap, and a generic shape compares in declared-symbol space. The right side may be any tensor expression, including a view, but it must not alias the left: a view of the target, the target itself, or a `←` alias of either is `copy_into_alias`. A host function may rebind a `mut` tensor parameter whole with a plain `←`; that is the ordinary rebind. Inside a kernel a parameter is a device buffer with no binding to rebind, so a plain `←` of a whole tensor into a `mut` tensor parameter is rejected there (`mut_tensor_param_rebind`) and `⇇` is the one write. The earlier callable-sink form `sink ⇇ payload` is retired: a callable on the left is `transfer_sink_retired`.

**Sink-into (`⇜`):** `s ⇜ x` adds one item, `x`, into the sink `s`, and is the operator form of the method call `s.sink(x)`. They differ when the write fails: the method checks and reports, while the operator dies. The item must be exactly the sink's element type `T`, with no implicit conversion, so `console.out ⇜ "§"(n)` is legal and `console.out ⇜ n` is not. A sink writes exactly what it is given; no sink adds a newline or any other framing. The operator is a statement with no value, so using it as a value is `sink_into_value_used` and chaining it (`a ⇜ b ⇜ c`) is `sink_into_chain`. A `⇜` into a sink reached through a `let` name is refused, exactly as a mutating method call on that name is; a `⇜` into a collection inside a loop over that same collection is `loop_collection_shape_change`, the same as `append`. A left operand that is not a `Sink<T>` — tensors, vectors, and matrices included, whose write stays `⇇` — is `sink_into_receiver_not_sink`. A channel's send view implements `Sink<T>`, so `m ⇜ "ping"` is legal.

**Conversion-directed assignment (`↤` / conversio-assign):** `place ↤ value`
evaluates the right side, converts it to the statically known type of the left
place through the existing `↦` route, then assigns. It binds at the same
precedence as `←` and is right-associative; the `⊥` default (`inline_default`)
is **legal only on `↤`** — a `⊥` after ordinary `←` is rejected, and in a
right-associated `↤` chain the default attaches to the nearest `↤`. The
operator is preserved verbatim through syntax and emission; it is never
rewritten to `←` or `↦`. Typed `定值`/`變值` initializers accept `↤`
(convert to the written type, then initialize); `定值 _`, `設為`, and untyped
destructuring have no concrete destination and are rejected.

The rounding hint on `↤` takes the same `經由` word as `↦` (`經由 Floor|Ceil|HalfEven|Trunc`): `a ↤ x 經由 Trunc ⊥ 0` converts `x` to the type of `a` with the hint, and the clause binds tighter than the `⊥` default.

`是` and `非 是` are a **type test**: the right-hand side is always a type —
including a declared or imported one — and the result is a runtime variant/type
test on the value. They never convert and never compare values; a value spelling
on the right is rejected in the reader's own words (`SEM011:est_value_rhs`),
pointing at the equality family. The null type is the one type spelling that also
names a literal slot: `x 是 nihil` tests the null *type*, while the null *value*
is `缺失` (`null` in the English reader).
Use `≡` / `≢` for structural value equality, `≅` / `≇` for promoted exact equality (same value after numeric widths join), `≈` / `≉` for fuzzy equality (tolerance match with Python-isclose defaults: rel_tol 1e-09, abs_tol 0.0), and `↦` for runtime conversion.

Retired predicate keywords are not prefix unary syntax. Use `expr ≡ 真`,
`expr ≡ 假`, `expr ≡ 缺失`, `expr 是 nihil` (the null *type* test),
`expr ≺ 0`, or `expr ≻ 0`.

The ASCII spellings `<` and `>` are not comparison operators — they are generic delimiters only. The parser rejects one in comparison position with `PARSE030 comparison_ascii_retired` (operator ruling TZ-2); write `≺` and `≻`.

Ordering comparisons (`≺`, `≻`, `≤`, `≥`) between two `textus` values compare
the whole strings in Unicode code-point order. They do not use locale
collation.

**Membership (`∈`, `∉`).** `x ∈ xs` tests whether `x` is an element of the
right operand and `x ∉ xs` is its first-class negation (not sugar over `非`);
both sit in the comparison tier with `≺ ≻ ≤ ≥`. One operator covers two
meanings, chosen by the type of the right operand: a collection (key
membership for a `tabula`) or a range. The glyphs have no ASCII spelling, and
they never apply to text: a `textus` right operand is rejected with a
diagnostic that points to the `contains` method. The former keywords `intra`
and `inter` are retired from the grammar.

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
- **Split from `↦`:** `↦ ascii<尺寸 N> 經由 Hex` is exact conversion — fixed width,
  fails if the value does not fit; `¶` is display — width is a minimum that
  grows to fit, and never fails.
- **No word twin:** `¶` is the same glyph in every locale, like `✓ ✗`.
- `d64` decimals print as decimal numbers (D2.6): with a spec, exactly what the
  spec says (`12.5 ¶ ".2"` is `12.50`, digits cut below the carrier's scale
  round half-even); without one, the shortest form with trailing zeros dropped
  (`12.5`, `12`).

**Edge-case outputs (D2.7):** `NaN` / `∞` / `-∞` print as `NaN`, `∞`, `-∞`
(precision does not apply); a negative number in hex/bin/oct prints sign plus
digits (`-42 ¶ "x"` = `-2a`), not two's complement (`↦ ascii<尺寸 N> 經由 Hex` stays
the strict tool and rejects negatives); `textus` width counts `littera`
(characters), not screen columns (an emoji with a skin-tone modifier counts as
2; screen-width alignment is library work); `instans` outside years 0–9999
with `"iso"` uses ISO 8601's extended form (`+10000-01-01`).

**Static type ascription (`∷` / verte):**

The `∷` glyph (U+2237, "proportion") explicitly ascribes a target type to an expression. Use it when the source expression already exists and the compiler needs a static target shape:

- Primitive/alias → cast (no runtime effect): `data ∷ textus` → TypeScript: `(data as string)`
- Built-in collection → target-shaped collection value: `[1, 2, 3] ∷ lista<numerus>`
- Variant expression → enum/interface target ascription: `虛構 Click { x = 10 } ∷ Event`

Prefer typed construction for ordinary `類型` values and `vacua` for ordinary empty collection values:

```fab
sit _ point ← Point { x = 10 }
sit lista<numerus> xs ← vacua
```

Only the `∷` glyph is accepted as the postfix static type-ascription operator. The Latin forms `qua`, `innatum`, and `novum` were aliases and have been removed (see verte-alias-clean-break).

**Runtime conversion (`↦` / conversio):**

The `↦` glyph (U+21A6, "rightwards arrow from bar") is the runtime value conversion operator. Unlike `∷` (compile-time cast), this performs actual parsing/conversion that can fail:

- `"22" ↦ numerus` → Rust: `"22".parse::<i64>().unwrap()`
- `"bad" ↦ numerus ⊥ 0` → Rust: `"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ textus` → Rust: `42.to_string()`
- `n ↦ ascii<尺寸 N> 經由 Hex|Bin|Oct` — shipped; fixed-width lowercase digits, zero-padded to `N`, with overflow and negative sources rejected.
- `n ↦ ascii<_> 經由 Hex|Bin|Oct` — shipped for const-foldable numerus sources; the hole is solved to the source digit count. Runtime sources leave the hole unsolved and require explicit `N`.

**The `經由` clause (D11.9).** A convert hint is a clause on the conversion, not a type argument: `"ff" ↦ i32 經由 Hex`, `65 ↦ littera 經由 Code`, `lista<b8>[0‥2] ↦ u16 經由 Le ↦ b16 ↦ f16 ↦ f32`. The grammar is `conversio_expr := '↦' type_annotation via_clause? inline_default?` and `via_clause := '經由' IDENTIFIER`.

- `經由` is contextual: it is claimed only on the conversion's own line, immediately after the target type. Everywhere else it is an ordinary identifier (radix corpora contain 186 real uses of `經由` as an identifier: gradus 129, examples 29, inferentia 26, norma 2).
- The hint (`Hex`, `Bin`, `Oct`, `Be`, `Le`, `Code`) is a compile-time identifier that selects the conversion row. It is not part of the target type and it is not a keyword. The set is exactly those six (there is no `Radix` hint; `Bits` retired, see below). Hint spellings are the same short English identifiers in every locale; the word `經由` itself is per-locale (`經由` in en and la).
- The clause binds tighter than the `⊥` default: `x ↦ u32 經由 Hex ⊥ 0` is `(x ↦ u32 經由 Hex) ⊥ 0`. Conversions chain, each hop with its own clause.
- Whether a hint is known, and whether the target takes one, is semantic (lowering), not grammar.

**Retired spellings.** Before D11.9 a hint was written as the second type argument of the `↦` target (`ascii<N, Hex>`, `littera<Code>`) or as a bracketed tail (`lista<b8, 16><Le>`). Both are rejected at parse time (`conversio_hint_type_argument`, `conversio_hint_tail_argument`); the `經由` clause is the only spelling.

The hint selects the conversion row. `Hex` / `Bin` / `Oct` / `Be` / `Le` / `Code` are convert hints in the `經由` clause, not keywords and not new `baseType` productions. For ascii output, `Hex` / `Bin` / `Oct` select the lowercase fixed-width digit pack; the hint is not part of type identity. Target support is not a grammar production (see Target Support).

- `"ff" ↦ i32 經由 Hex` — shipped; text parse at radix 16 (`Bin` = 2, `Oct` = 8). Hex/Bin/Oct text parse is unchanged by endian hints.
- `lista<b8>[lo‥hi] ↦ W 經由 Be` / `… ↦ W 經由 Le` — endian unpack of an exact-width window (`W` is `i16` / `i32` / `i64` / `u16` / `u32` / `u64`; window length 2 / 4 / 8). Shipped on rust, the MIR runner, Go, and TypeScript. TypeScript `i64`/`u64` stay fail-closed (JS number is not exact). A `lista<b8>` itself has no endian; `lista<b8> ↦ u32` without `經由 Be` / `經由 Le` stays rejected. A short window fails (no pad).
- `lista<b8>[lo‥hi] ↦ f32 經由 Be|Le` / `… ↦ f64 經由 Be|Le` — shipped alongside the integer rows (float endian unpack of an exact-width window, 4 / 8 bytes; same fail rules: exact window required, a short window fails, `經由 Be` / `經由 Le` mandatory).
- `lista<b8> ↦ tensor<E, [d1, …, dk]> 經由 Le` / `… 經由 Be` — bulk faithful reinterpretation of a byte string as a typed tensor (shipped on the checker and the MIR runner). The source is `lista<b8>` or the bounded `lista<b8, 尺寸 N>`; each target extent is a literal or a run-time size read from an in-scope binding (`size n`, la `尺寸 n`); `E` is `u8` / `i8` / `i16` / `u16` / `i32` / `u32` / `i64` / `u64` / `f32` / `f64`, or a genus with a fixed byte layout (a record row, below). The `經由 Le` / `經由 Be` hint is required for every element wider than 8 bits (no hint, or any other hint, is `SEM016` `list_b8_to_tensor_unspecified`; an element outside the set is `list_b8_to_tensor_element_unsupported`) and is forbidden on an 8-bit element (`u8`, `i8`), which has no byte order (a clause written on one is `SEM016` `bits_via_clause_not_allowed`). Element `i` occupies bytes `[i·w, (i+1)·w)` for element width `w`, in row-major order (last index fastest), read in the byte order of the hint. It is pure bit reinterpretation with no numeric conversion: NaN payloads, `-0.0` and subnormals survive exactly, as in the scalar window rows. The byte length must be exactly `product(shape) · w`, with no truncation and no padding; a mismatch is a recoverable error (`捕捉`, `⊥` and `⇥` apply, and outside `main` a bare `↦` needs a handler), and a compile error (`SEM016` `list_b8_to_tensor_length_mismatch`) when the source length is statically known (a `|...|` literal or a constant window). A run-time extent (`tensor<f32, [2, size n]>`, with `n` an in-scope binding) is checked when the conversion runs: the byte length must equal `product(shape) · w` for the extent values at that point, with the same recoverable error on a mismatch as for literal extents. On Rust a `size` parameter or an expression extent (`size (n - 1)`) is still declined by name (`rust_list_b8_to_tensor_runtime_extent_unsupported`); read a local binding instead. The reverse is the whole-tensor byte encode, `tensor<E, shape> ↦ lista<b8> 經由 Be|Le`: every element, in row-major order, becomes its `E`-width bytes in the byte order of the hint, so the byte count is the element count times the width and a round trip returns the tensor (shipped on the MIR runner and Rust; Go, TypeScript, Swift and several MIR targets decline it by name). The list form `lista<E> ↦ lista<b8> 經由 Be|Le` below is its list counterpart. A genus element (`tensor<Block, [n]>`, where `Block` is a non-generic genus whose fields all have a fixed size: sized numerics, register vectors and fixed-size `lista<…, 尺寸 N>`; a nested genus, text or a nullable field has no fixed layout) is a record row: it takes `經由 Le` only, because the row's byte image is the little-endian layout of its fields (`經由 Be` is `SEM016` `list_b8_to_tensor_record_big_endian`, and no hint is `list_b8_to_tensor_unspecified`). The row width comes from that layout, so the byte length must be a whole number of rows and exactly `product(shape) · row width`; the checker cannot see the layout, so every length mismatch is a recoverable run-time failure with a stable issue (`runner_list_b8_to_tensor_record_truncated`, `…_trailing_bytes`, `…_misaligned`, `…_overflow`), and a genus with no fixed layout is refused by name (`list_b8_to_tensor_record_layout_refused`). The runner keeps the rows as the bytes and decodes one only when an element is read; a field store through an element (`t[[i]].f ← v`, also `↑`/`↓`) decodes the row, changes the field and stores the whole element back into the tensor, so the write is never lost. Worked example: `|00 00 80 3f 00 00 00 40 00 00 40 40 00 00 80 40 00 00 a0 40 00 00 c0 40| ↦ tensor<f32, [2, 3]> 經由 Le` prints `[1.0, 2.0, 3.0, 4.0, 5.0, 6.0]`, and `|01 02 03 04| ↦ tensor<u16, [2]> 經由 Be` prints `[258, 772]` where `經由 Le` prints `[513, 1027]`.
- `lista<bN> ↦ lista<E>` / `lista<E> ↦ lista<bN>` — bulk conversion between a raw-bits list and a numeric list (a `經由 Le` / `經由 Be` clause exactly when the bit width changes, and never when it stays the same). A same-width conversion (`lista<b8> ↦ lista<u8>`, `lista<b16> ↦ lista<u16>`, the signed twins, and back) takes no clause at any width, and a clause written on one is `SEM016` `bits_via_clause_not_allowed`. A width change (`lista<b8> ↦ lista<u16>`, `lista<u16> ↦ lista<b8>`, `lista<b8> ↦ lista<b16>`, `lista<b8> ↦ lista<f32>`) requires the clause (`SEM016` `bits_via_clause_required`): each source element is written as width/8 bytes in the byte order of the clause and the byte stream is regrouped at the target width, so `lista<b8> ↦ lista<u16> 經由 Le` of `[1, 2, 3, 4]` is `[513, 1027]` and `經由 Be` is `[258, 772]`. A byte total that is not a whole number of target elements is a recoverable run-time error (`bits list length mismatch`). Shipped on the checker and the MIR runner; every emitter refuses the width-changing form by name. The `lista<b8> ↦ tensor<E, [shape]>` row above follows the same rule (no clause on an 8-bit element).
- `經由 Bits` — **retired**. A bit pattern is a `bN` value (`b8`, `b16`, `b32`, `b64`), and the one rule is: from a number, `↦` converts the value; from bits, `↦` to a same-width type reads those bits as that type. A float to its pattern is `x ↦ b32` (b16 for f16 and bf16, b64 for f64); a pattern to a float is `p ↦ f32`; a number that holds a pattern says so in one step, `u16 ↦ b16 ↦ f16`. `n ↦ f32` on a `u32` stays the value conversion. Every `經由 Bits` spelling is `SEM016` (`conversio_via_bits_retired`), whose help names the `↦ bN` replacement.
- `W ↦ lista<b8, 尺寸 N> 經由 Be` / `… ↦ lista<b8, 尺寸 N> 經由 Le` — shipped on the checker, the MIR runner and Rust, for a sized numeric source `W` (`i16` / `u16` / `i32` / `u32` / `i64` / `u64` / `f32` / `f64`) and a target `lista<b8, 尺寸 N>` whose `N` equals the source element's byte width. The `經由 Le` / `經由 Be` hint is required: no hint, or any other hint, is `SEM016` `numeric_to_octeti_unspecified`, and a wrong `N` is `SEM016` `octeti_endian_width_mismatch`. The row cannot fail — no recovery channel, no `⊥` — and it is bit-exact: two's-complement integers and IEEE-754 bits, so NaN payloads and `-0.0` survive the round trip. An unsized `int` source, an 8-bit source (write `x ↦ b8`, with no clause; there is no `lista<b8, 尺寸 1>` row) and an unsized `lista<b8>` target keep `numeric_to_octeti_unspecified`. Go declines the row by name (`go_scalar_octeti_conversio_unsupported`), and the other targets refuse bounded `lista<b8, 尺寸 N>`. Register targets take the clause today: `v ↦ lista<b8, 尺寸 16> 經由 Le`, `corpus[0‥16] ↦ vector<u32, 尺寸 4> 經由 Be`.
- `'A' ↦ u32 經由 Code` — shipped; the code point as a `u32` (`u32` holds every code point, as Rust's `char as u32`); the source must be `littera`. `65 ↦ littera 經由 Code` — shipped; builds the character for that code point, failing above U+10FFFF and on a surrogate. `Code` is a `經由` hint like `Hex`/`Le`; any other hint on these targets, or a source/target type other than `littera`/`u32`, is `SEM016` (`code_hint_pair_mismatch`).
- `n ↦ textus` / `n ↦ ascii` / `n ↦ littera` — a number's digits (D10.6): `7 ↦ textus` = `"7"`, `7 ↦ ascii` = `"7"`, `7 ↦ littera` = `'7'`; `littera` fails outside 0–9 (`42 ↦ littera` fails, two letters).
- `littera ↦ numerus` — parses the digit, failing otherwise (as `"22" ↦ numerus` parses).
- `littera ↦ textus` — the one-letter string; never fails.
- `textus ↦ littera` — the only letter; fails unless the text is exactly one letter.
- `lista<b8> ↦ textus` — UTF-8 decode; can fail. `lista<b8> ↦ ascii` — checks every byte is below 128, same bytes; can fail. `lista<b8>[i‥i+1] ↦ ascii` — one byte through a window (mirrors `lista<b8>[lo‥hi] ↦ W 經由 Be`).

Explicit integer narrowing is magnitude-checked on every backend:
`n ↦ u8` converts a value that fits unchanged, and a value out of the
target's range fails — it never wraps and never relabels. The failure takes the
error channel, or the `⊥` default when one is written. Into `模數<W>` and
`saturatus<W>` targets `↦` reduces or clamps and cannot fail. Use `模數<W>`
for wrapping arithmetic.

**Interval clamp (`↦ lo‥hi`).** When the target of `↦` is a range instead of a type, the conversion clamps a number into that interval: `15 ↦ 0‥10` is 9 (the half-open `‥` excludes its end), `15 ↦ 0…10` is 10 (`…` includes it), `wide ↦ 10…50` clamps one `intervallum` value into another range, and a stored `intervallum` value is a legal target too (`x ↦ fines`). The grammar production is `conversio_expr := '↦' (type_annotation | interval_target) via_clause? inline_default?` with `interval_target := range_expr`. The parser reads the operand as an interval, not a type, when it opens with a number literal or a non-type identifier; a capitalized name, a known type word, or a qualified `ns.Type` stays a type. A clamp is total, so it takes no `經由` hint (`conversio_via_target_takes_no_hint`), no `⊥` default (`intervallum_clamp_recovery_unsupported`) and no `每` step (`intervallum_value_step_unsupported`); these are semantic rejections of a shape the grammar still admits.

**Default channel (`⊥`):** `⊥` (U+22A5 UP TACK) supplies a value when a
conversion or a failable call fails: `定值 numerus n ← "abc" ↦ numerus ⊥ 0`,
or `定值 numerus n ← risum() ⊥ 0` (X3, D17.7) when `risum` is failable. On a
conversion it is written immediately after the conversio target (`↦ T ⊥
default`) or after the value of a `↤` assignment; on a call it is written
immediately after the complete call chain (`f(x).m() ⊥ default`).

- `⊥` catches only the `⇥` error channel. It never catches `崩潰` or traps
  (for example integer overflow).
- The default is evaluated only on failure.
- The default must type-check as the success type `T`.
- One expression either propagates (`⇥ E`) or defaults (`⊥ v`), never both;
  `⊥ v ⇥ …` is rejected.
- `⊥` binds looser than `↦ T`: `x ↦ numerus ⊥ 0` is `(x ↦ numerus) ⊥ 0`. The
  unparenthesized default is a unary-precedence expression; parenthesize
  arithmetic, coalescing, ternary, or assignment defaults.
- `⊥` is legal on a conversion (`↦ T`, `↤`) or on a call whose last
  postfix step is a call suffix (X3): `f() ⊥ 0`, `f() ⊥ 0 + 1` parses as
  `(f() ⊥ 0) + 1` — the default binds at the same postfix tier as the
  call. After any other expression — a bare identifier, a member or
  index access, a cast (`∷`), or a second `⊥` on the same expression
  (`f() ⊥ 0 ⊥ 1`) — it is rejected (`default_requires_failable`); `⊥` is
  not a general postfix operator.
- `⊥` is an operator between a failable expression and a value. It is not the
  type-theory "never" type (that is `numquam`).
- The glyph is the same in every locale. The look-alike `⟂` (U+27C2) is
  rejected with a "did you mean `⊥`?" hint.

`⇥` only ever names an error type. The retired inline recovery `↦ T ⇥ value`
(and `↤ … ⇥ value`) is rejected with a migration diagnostic pointing at `⊥`.

Using `或取` as a conversio default is rejected with a migration diagnostic. `或取` is local nullable elimination only (`x 或取 y`, parameter defaults) — not logical `或`. A parenthesized conversio result may still combine with `或取` as ordinary defaulting.

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
| `\|...\|` | `lista<b8>` | compile-time hex bytes |
| `"..." ↦ regex` | `regex` | compiled pattern from dynamic text (conversion) |
| `regex /.../` | `regex` | inline raw pattern literal (locale regex type word) |
| `_ /.../` | `regex` | inferred inline raw pattern literal |
| `[ ... ]` | `lista<T>` | Faber list (not JSON array, not bytes) |

`§` (U+00A7) is a template hole in Unicode forms (`"`, `«`, `` ` ``).
§{label} names a hole with an identifier label; the label may repeat within
its template (see the hole rules below) and may use a keyword spelling under
the contextual law. Named holes are not available in `ascii` literals, where
`§` remains forbidden.

Hole rules (P1-P5):

- **P1, numbered holes select.** A numbered hole `§N` means argument N; the
  same number may appear any number of times and one value serves all of
  them. A template needs max(N)+1 values, so the count of holes does not
  matter, and a value past the highest number is
  `template_application_arity_mismatch`.
- **P2, no gaps.** Every number below the highest must be read by some hole.
  `"§0 and §2"` leaves argument 1 unread and is `template_hole_index_gap`,
  whether or not the value count happens to match.
- **P3, forma follows.** A backtick `forma` template obeys the same rules; its
  parameter list holds one entry per distinct position.
- **P4, a label is one position.** A repeated label reads the position its
  first occurrence was assigned, takes one value, and consumes no extra
  position (see Format-Template Application). The argument is evaluated once, however many
  holes read it.
- **P5, mixing warns.** Plain holes (an unnumbered `§`, named or not) count
  from 0 in their own order and ignore numbered holes, so `"§0 and §"("a")` is `a and a` and
  `"§ § §0"("a", "b")` is `a b a`. A literal that holds both kinds is the
  warning `WARN035` (`mixed_plain_numbered_template_holes`); it names the
  argument each plain hole reads and suggests numbering every hole. The
  numbering itself does not change.

Plain `§` holes stay positional, one value each.

**Rendered templates** (`textus`): `"..."(...)` and `«...»(...)` lower to
`格式文字("...", args...)`.

**Captured templates** (`forma`): `` `...`(args) `` captures template text and
parameters without rendering. Safe for bound SQL/URL payloads; do not use
`«...»(...)` for that job.

Block `textus` uses guillemets `«...»`. The heavy quotation-mark
pair is retired (too visually close to `"` in many fonts).

#### Escapes

Every literal form accepts exactly these escapes after a backslash, and
anything else is a compile error:

| Form | Own-delimiter escape | Shared escapes |
| --- | --- | --- |
| `"..."` | `\"` | `\\`, `\n`, `\t`, `\r`, `\u{H…}` |
| `«...»` | `\»` | `\\`, `\n`, `\t`, `\r`, `\u{H…}` |
| `` `...` `` | `` \` `` | `\\`, `\n`, `\t`, `\r`, `\u{H…}` |
| `'...'` | `\'` | `\\`, `\n`, `\t`, `\r`, `\u{H…}` (`≤ 7F`) |

`\u{H…}` is 1–6 hex digits naming a Unicode scalar; inside `'...'` it is
limited to `7F`. Anything else after a backslash is a compile error (LEX003
`unknown_escape` for `"..."`, `«...»` and backtick forms, SEM011
`ascii_literal_invalid_escape` for `'...'`). A literal escapes its own
delimiter and not another literal's; `\0` and `\xHH` do not exist (`\u{0}`
writes NUL). A regex pattern in `"..."` doubles its backslashes
(`"\\d+" ↦ regex`).

Implementation status (2026-10-11):

- Shipped: `"..."`, `«...»` block `textus`, `'...'` → `ascii`, `` `...` `` → `forma`, `|...|` → `lista<b8>`, `{ ... }` → `json`, text/ascii `↦ regex`, and the inline regex literals `regex /.../` and `_ /.../`.

Inline block example:

```fab
sit _ tag ← «inline»
```

Multiline block example (newline after opening `«`):

```fab
sit _ blob ← «
    select id, email
    from accounts
»
```

Captured template example:

```fab
sit _ q ← `select * from accounts where id = §`(accountId)
```

Octeti hex literal example:

```fab
sit _ sig ← |de ad be ef|
sit _ hello ← |48 65 6c 6c 6f|
```

### Format-Template Application

String literal call syntax is the canonical source form for format-template application:

```fab
"§{greet} world"(greet: "salve")
"status: § (§)"(sample_status(), "ok")
"status: §1 (§0)"("ok", sample_status())
# Renders "her, her, her, and me".
"§0, §0, §0, and §1"("her", "me")
```

The position law counts named and anonymous holes together in order of
appearance: "§{greet} §" = `[greet: 0, anonymous: 1]`. Named labels are
erased at lowering, so "§{greet} world"(greet: "salve") lowers identically
to the positional form `"§ world"("salve")` and its canonical
`格式文字("§ world", "salve")` form.

The hole rules P1-P5 (stated with the template literal forms above) in use:

```fab
# P1: one value serves every occurrence of a number; renders "her, her, her, and me".
"§0, §0, §0, and §1"("her", "me")
# P2: rejected, template_hole_index_gap (argument 1 is never read).
"§0 and §2"("a", "b", "c")
# P4: a repeated label is one position and one value; renders "x and x".
"§{n} and §{n}"(n: "x")
# P5: accepted with WARN035; the plain hole reads argument 0, so this renders "a and a".
"§0 and §"("a")
```

This lowers to the compiler's `格式文字("...", args...)` form. Use the string-template form in ordinary source; reserve `格式文字(...)` for explicit desugaring examples and compiler-facing documentation.

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

Text slices accept the full range form, including `每`.

For `lista<T>`, bracket indexing is a single element or an exclusive window.
A single-element index must be one integer. A window `xs[lo‥hi]` takes the full
range form (`‥`, `…`, and `每`), and the window is a **new list of the same
elements**: the copy is shallow, so reference-type elements are shared and
`拷貝` is not read as deep. The range must lie fully inside `0‥length` and run
forward; otherwise the run dies. An empty window at a valid position
(`xs[0‥0]`, `xs[n‥n]`) is `[]`.

```fab
# Element at position i.
xs[i]
# Write element at position i.
xs[i] ← v
# Exclusive window → a new lista<T>. Fully in bounds or the run dies.
xs[lo‥hi]
```

The copy-out methods report on the error channel instead of trimming:
`xs.sectio(start, end)` (en `slice`) and `xs.第一個(n)` (en `take`) are failable
(`⇥`; a `⊥` default or a handler takes the fault), and the fault carries the
requested bounds or count and the list length as data.

Lista bracket access is **plain**, not nullable: it returns the bare element
`T` and traps on out-of-bounds. A `tensor` bracket read is plain in the same
way. For nullable list access, use `xs.accipe(i) → T ∪ nihil` with `或取`.

For `tensor<T, Figura>`, a bracket read returns the bare element `T` and traps
on a bad index, like a list index; a literal index that is provably out of
range is a compile error. Bracket indexing is sugar over the tensor intrinsic
surface (the nullable read is the `accipe` method, not the bracket):

```fab
# trapping vector.accipe([id])
vector[id]
# vector.ponde([id], v)
vector[id] ← v
# trapping grid.accipe([r, c])
grid[[r, c]]
# grid.ponde([r, c], v)
grid[[r, c]] ← v
```

Reads return `T` (no `或取` is needed); use the `accipe` method for a nullable
read. Rank-1 tensors accept scalar integer
indices that fit the tensor `i64` runtime boundary (`u64` is rejected).
Rank-N tensors use a list-shaped index expression such as `[[r, c]]` or a
bound `lista<integer>` value. `grid[r, c]` is not syntax; `memberSuffix` still
contains exactly one `expression` between brackets.

For a `lista<b8>` byte string, bracket indexing is a byte or an exclusive window:

```fab
# One byte → u8. O(1). Traps on out-of-bounds.
buf[i]
# Exclusive window → lista<b8>. Fully in bounds or fail (no short slice, no pad).
buf[lo‥hi]
```

The index must be an integer or a range. A compile-time-provable out-of-range
index on a `|...|` literal (`|從 端點 be ef|[0‥5]`) is a structured reject.
Runtime out-of-bounds traps — the same trapping model as
lista bracket access, not textus short-slice. A lista `[lo‥hi]` is a live window
(a new list of the same elements), fully in bounds or the run dies.

A `lista<b8>` is the endian host. Parse byte windows on the buffer
(`buf[lo‥hi] ↦ W 經由 Be|Le`). Cross to a numeric list once, for element work,
via `lista<b8> ↦ lista<u8>` (representation change only; other element
types fail closed). The reverse `lista<u8> ↦ lista<b8>` is live. Do not
detour through `valor`. Lists stay for element work, not endian windows.
Decode a whole buffer at once with `lista<b8> ↦ tensor<E, [shape]> 經由 Le|Be`
(see Runtime conversion), not element by element through a list.

### Primary Expressions

Non-finite literals are contextual floating-point values: `∞` is positive
infinity and `nan` is NaN. The named form is `nan` in the
Latin (`la`) pack and `nan` in every other shipped pack; it is claimed only in
the literal slot, so a following `(` keeps an ordinary `nan(...)` call. Their
width follows a surrounding `f32` or `f64` context when present; bare `fractus`
remains unsized, and neither form has a width suffix. A leading `-` is supplied
by `unary_expr`, so `-∞` is unary negation of `∞`, not a separate token. A
`numerus` context, `inf` included, rejects both forms (fail-closed); neither
maps to an integer.

**Capture boundary (`陷阱`):** `陷阱 { … }` (en `trap`) is an expression
that runs its block and reifies the error channel into a value. The block's
trailing expression is the success value `R`; the error type `T` is the error
type, or the union of error types, that can escape the body (failable calls
and `拋出` payloads). The result is `trapped<R, T>`, a built-in discriminated
union with two variants: `returned`, whose field `value` holds `R`, and
`thrown`, whose field `error` holds `T`. It is read with an ordinary
`比對` (en `match`); `R` and `T` are separate slots, so a `string`
success with a `string` error is legal, and the earlier bare-union result form
is gone. `捕捉` (en `catch`) is unchanged and binds the payload. `陷阱` claims its spelling only in expression-primary position
directly followed by `{`, so `陷阱(…)` calls and bare identifier uses keep
their ordinary meaning. No `捕捉` clause, `當` tail, or early-success form
attaches to it — those belong to `執行`.

`vacua` is a contextual empty-collection marker (identifier form, not a reserved keyword).
Use it with an explicit declared type: `定值 lista<numerus> xs ← vacua`, `定值 tensor<f32, []> t ← vacua`, or `定值 tf32[2, 3] z ← vacua` (the typed zero `tf32[2, 3] {}`).


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
Construction literals do not spread: `展開` is not a field initializer
(`Genus { 展開 other }` is rejected). `展開` stays for list literals and
call arguments. Copy-with-changes is planned as `Genus { … } 取自 source`.

- Ratio construction uses `ratioType '{' fieldInit (',' fieldInit)* '}'` through `typedConstructor`; every field initializer is named, and the resulting fields remain accessible only by label.

### Special Expressions


`歸約 經由 <op> 取自 source [在 [coords]] [按 key | 按 [key, …]] ([執行緒 f] (定值 | 變值) x { 傳回 term })? [或取 identity]?` is the general reduction form. The optional body may carry an optional `執行緒`; the optional identity follows the body or, when there is no body, the source. Without a body, the source elements are the terms. `經由` is mandatory and selects exactly one of `求和`, `求積`, `最大`, `最小`, `最大值索引`, `最小值索引`, `全部`, `任一`, `計數`, or `第一個`. Each operator uses its locale pack's existing method spelling, or its keyword spelling where there is no method row. The operator word is claimed only after `經由` and remains an ordinary identifier elsewhere. This does not change list folds such as `xs.歸約(f)` (en `xs.reduce(f)`).

The source is a shaped tensor or matrix; lists keep their methods. An optional `在 [coords]` binds one integer coordinate per source axis. A body is claimed only when `(定值 | 變值) IDENTIFIER '{'` follows, so a declaration such as `定值 numerus x ← 1` on the next line remains a separate statement. The body must be pure and yield exactly one `傳回` term per element. Without a body, each source element is the term. `執行緒 f` requires a body and a kernel context; it retains the existing sum path, while other `執行緒` operators fail closed by name until their lowering is implemented. Without `執行緒`, the reduction is sequential in row-major order. The numeric store rule still applies: width policies are observed at stores or conversions, never in the middle of a reduction expression.

| Operators | Term | Result | Empty source |
| --- | --- | --- | --- |
| `求和` | numeric scalar | integer terms use the signed or unsigned 64-bit cap by the range rule outside device functions; otherwise the term type | identity `0` |
| `求積` | numeric scalar | term type | identity `1` |
| `最大`, `最小` | numeric scalar | `T` when statically non-empty; otherwise `T ∪ nihil`; `T` with `或取` | `nihil` or the supplied identity |
| `最大值索引`, `最小值索引` | numeric scalar | rank-one coordinate `numerus`; higher-rank `元組<numerus, …>` with one coordinate per axis; `∪ nihil` unless statically non-empty; plain coordinate result with `或取` | `nihil` or the supplied coordinate identity |
| `全部`, `任一` | `bivalens` | `bivalens` | `真`, `假` |
| `計數` | `bivalens` | `numerus`, the number of true terms | `0` |
| `第一個` | `bivalens` predicate, required body | the first element whose predicate holds: `T ∪ nihil`, always optional | `nihil` |

`第一個` (en `first`) takes a required body that is the predicate over the element and yields the first element in row-major order for which it holds, or `nihil` for an empty source or no match. Evaluation is total: the predicate runs on every element, including those after the first match, and a predicate error is never turned into `nihil`. It takes no `按` group (`reducta_group_op_unsupported`) and no `或取` identity, and a source that is not a tensor or matrix, like every reduction source, is refused; first-match over lists is not a reduction source. `第一個` is the one reduction operator whose result is always optional, even over a statically non-empty source. On `@ radix 車道 "air"` it is admitted over a statically bounded source only (`reducta_first_unbounded_source`), forward-only: it never becomes an AIR tensor op, so reverse AD stays closed.

**List predicate methods.** `xs.all(pred)`, `xs.any(pred)` and `xs.find(pred)` (la `全部`, `任一`, `reperta`) are list methods that take a closure, not reduction operators. They evaluate the predicate left to right in list order, once per element, and stop at the first decisive element: the first false for `all`, the first true for `any`, the first true for `find`. No element after the decisive one is visited, so a predicate side effect or error after it never runs; this is the opposite of the tensor `reduce 經由 first`, which runs the predicate on every element. `all` is `true` and `any` is `false` for an empty list. `find` returns `T ∪ none` (la `T ∪ nihil`): the first element for which the predicate holds, or `none` when no element matches or the list is empty. The result stays optional even for a bounded list whose minimum size is at least one, because a match is not guaranteed. `xs.first()` (la `primus`) is unchanged, and `find` on a regex (la `inventa`) is a different method found by the receiver type.

**List join.** `xs.extend(ys)` (la `extende`) appends every element of the list `ys` to `xs`, in `ys` order, in place, and yields nothing. `ys` is a list of the receiver's element type: a plain `lista<T>`, or a fixed-capacity or length-ranged list of `T`; a bounded byte buffer `lista<b8, 尺寸 N>` joins a `lista<b8>`, which is how per-element sized encodes (`x ↦ lista<b8, 尺寸 4> 經由 Le`) are chained into one byte string. The receiver changes, so it must be `mut`; the argument is only read, and its length is read once before the first element is appended, so `xs.extend(xs)` appends the original elements exactly once. The receiver must be a plain `lista<T>`: a bounded or length-ranged receiver is refused (`SEM011` `extend_requires_plain_list`), because a push past its capacity is the `appende` failure that reports on `⇥`. Inside a loop over `xs`, `xs.extend(…)` is the same shape change as `xs.append(…)` (`loop_collection_shape_change`). The Swift and Python targets decline it by name.

For arg-extrema, ties choose the lowest coordinate in row-major order. A NaN never wins while a number participates; an all-NaN source keeps its first coordinate. Runner sum and product visit terms sequentially in row-major order. Device reassociation and its numeric tolerance belong to the device contract, not this grammar.

`按 key` or `按 [key, …]` groups terms into a new result tensor; it does not mutate the source or key tensor. A bare key is one key, while the bracketed form requires a non-empty expression list, with one key per axis of the result. One key (`按 k` or `按 [k]`) keeps the rank-one result `tensor<R, [K]>`; a bracketed list of N keys gives a result of rank N, `tensor<R, [K0, …, KN-1]>`, where `R` is the operator's result type. A bare key stops before `coalesce` (la `或取`), so a following identity belongs to the reduction. Each key is evaluated per element with the coordinates in scope and must be integral. The bucket extents come from the expected result type, so the key count must equal its rank (`reducta_group_rank_mismatch`), and a term lands in the row-major bucket its keys name. A multi-key list must be written as a literal `[k0, k1, …]` of plain keys: a key list held in a tensor or list value, an empty list, or a list with a spread element is refused (`reducta_group_keys_not_literal`). Inside the list a key is one scalar expression: `ka[i, j]` is `PARSE001` and `ka[i][j]` on a rank-two key tensor is `SEM011`, so read one rank-one key tensor per key, as in `按 [rows[i], cols[i]]`. A key outside its own extent uses the existing tensor-write failure. Grouped `求和`, `求積`, `計數`, `全部`, and `任一` have empty-bucket identities; grouped `最大` and `最小` require `或取 identity`. Grouped `最大值索引` and `最小值索引` are unsupported by name because their result is a coordinate, not a tensor element. The Rust, TypeScript, and Go targets emit the multi-key group and the MIR runner runs it; no other target is claimed for it here.

The axis sugar `t.求和(軸 n)` (en `t.sum(軸 n)`) on a tensor of literal rank 2 to 8 is the grouped sum whose key list is the coordinates other than `n`, in order. `n` is a compile-time constant in `0..rank`, and the result is the receiver shape with axis `n` removed.

The old `求和 取自`, `最大 取自`, and `最小 取自` forms (English `sum from`, `max from`, and `min from`) are retired. The parser reports `PARSE010` for them. Use `歸約 經由 求和`, `歸約 經由 最大`, or `歸約 經由 最小`; the operator words remain ordinary identifiers outside the `經由` clause.

The tensor method `.media()` (en `.mean()`) is not a reduction operator and has no `經由` form. It is sugar over the sum reduction: `t.media()` is `t.求和() / count`, where `count` is the element count of `t` (en `t.mean()` is `t.sum() / count`). The grouped form `t.media(軸 n)` is the grouped sum divided by the grouped count, elementwise. A mean is not a true reduction (two partial means do not combine; sums and counts do), so the reduction operator set stays the set of real reductions. An empty source is `0 / 0`, which follows the float rules and is not-a-number (NaN); it is not a no-value result and has no fallback.

`排序 經由 <direction> (取自 | 從) source [(定值 | 變值) x { 傳回 key }]` is the sort expression (en `sort 經由 asc from xs`). `經由` is mandatory and selects `升序` or `降序` (en `asc`, `desc`). `取自` (en `from`) orders a list, a set, or a map's values; `從` (en `ref`) orders a map's keys. The optional body is the reduce body form and returns the sort key; without a body the source elements are the keys. The result is always a new list and the source is not changed. The sort is stable. `排序`, `升序`, and `降序` are claimed only in their slots and remain ordinary identifiers elsewhere; the list method `.ordina()` (en `.sort()`) is unchanged. The checker types it (`sort_source_unordered` refuses a source with no natural order, such as a nested list with no body key; `sort_source_not_collection` refuses a non-collection; `sort_ref_requires_map` refuses a list or set under `從`/`ref`; `sort_tensor_unsupported` refuses a tensor; `sort_key_unordered` refuses an unordered key; `sort_body_requires_redde` refuses a body with no `傳回`); a map under `從`/`ref` yields its keys as `lista<K>`, a map or set under `取自`/`from` its values; the list method `.sorted(key)` is retired (`sorted_key_retired`) in favour of this expression. It runs on the MIR runner and on the Rust, Go, TypeScript, Swift, LLVM, Wasm and Python emitters.

`格式文字` and `讀取`/`行` are builtin claims that resolve to a user binding
when the surface spelling is bound in scope (parameter, local, function, or any
in-scope definition); otherwise they are the builtin. The same binding-wins rule
applies to `格式文字`'s paren-claimed form and to the `vacua` empty-collection
marker: builtin claims are defaults, not reservations.

`虛構` variant construction accepts a qualified variant path
(`虛構 pkg.Bonum { … }`), so an imported union's variants construct through
the import alias, and the `∷` cast is a full type annotation
(`∷ pkg.Exitus`) exactly as the general postfix ascription (uvf-u3). A `{` right after a `虛構` path always opens its field list (empty braces are legal), so a `虛構` condition or scrutinee cannot be directly followed by a block: `若 虛構 A { … }` is a parse error, and `若 (虛構 A) { … }` is the parenthesized form.

`∷` remains the general postfix ascription in `cast`. Rendered text templates
(`STRING '(' argumentList ')'`) and captured `forma` templates
(`BACKTICK_STRING '(' argumentList ')'`) use the ordinary call suffix.

The inline regex literal is the file locale's regex type word followed by a
slash-delimited raw body (`regex /pattern/`), or `_` followed by the body when
the expected slot supplies the type (`_ /pattern/`). A bare `/pattern/` is not
legal: the lexer opens a body only when the previous significant token is the
regex type word or `_`, so any other `/` in operand position stays the division
operator (or a parse error), and a binding named `regex` used as the left
operand of a division is written `(regex) / 2`. The body is raw to the next
unescaped `/` on the same line: `\/` writes a literal slash, Faber processes no
other backslash escape, an empty pattern (`//`) is rejected, and there are no
suffix flags (use the inline `(?i)` form). The literal types as `regex` and its
body is passed to the target's native regex engine unchecked: Faber defines no
pattern subset and performs no compile-time validation of the contents, exactly
as when a pattern was a plain `textus`. `"..." ↦ regex` (and `ascii ↦ regex`)
remains the compiled-pattern conversion for dynamic text, which fails at
runtime on a pattern the target engine rejects.

---

## Patterns


---

## Diagnostics


The scribe family (`註記`/`檢視`/`警告`/`寫出` — en `print`/`debug`/`warn`/`write`)
claims the statement-initial position only when **not** immediately followed by
`(`. `註記 expr` is the output statement; a statement-initial `註記(...)` is an
expression statement whose callee is the identifier `註記` — a user function
call, never the intrinsic.

- `註記` = neutral diagnostic note, `檢視` = debug/inspect, `警告` = warn
- Output channel per level, the same on every target: `註記` (en `print`) and `寫出` (en `write`) write standard output; `檢視` (en `debug`) and `警告` (en `warn`) write standard error, so standard output carries only the program's normal output
- `寫出` is a diagnostic channel spelling; use current stdlib methods for real output

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


- `入口` = sync entry, `非同步入口` = async entry.
- `引數` binds parsed command-line arguments; `出口` supplies the process exit expression. Their order is fixed by `entryHeader`.
- Raw entry arguments never include the program name, on every target: `args[0]` is the first argument the program was given.

---

## Testing

`測試` modifiers include `預期失敗` (en `expect_failure`): the case passes only
when its body escapes through the error channel, and a case that completes
cleanly fails (strict expected-failure). The other modifiers are `略過`,
`預期`, `僅限`, `僅限於`, `標籤`, `時限`, `測量`, `重複`, and
`脆弱`. `重複 N` runs the case N times and passes only if every run
passes; it does not retry until a pass. `脆弱 N` is rejected by `faber test`
because flaky retries are not executed, so the case body does not run. The
counts of `時限`, `重複` and `脆弱` are non-negative integer literals;
a float is `test_modifier_integer`.

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

Expression-form `端點` is the only supported `端點` surface. Legacy typed
`端點 "route" (args) → T { }` and statement-level stream blocks
`端點 'route' { meus/tuus … }` are rejected at parse time.

The active `adExpr` production is defined under **Primary Expressions**. Its
ordinary postfix `conversio` materializes the resulting conversation handle.

- Route: `ASCII_STRING` (`'僅限:讀取'`), not double-quoted `STRING`.
- Opener: optional single `expression` → Request `data` as `valor`.
- **Expression `端點`**: blockless; evaluates to a `sermo` conversation handle.
  Use postfix `↦ T` (materialization), assign to `sermo`, or open live directional
  views: `s.meus<T>()` (outbound `da` / `fini`) and `s.tuus<T>()` (inbound
  `accipe` / `cursor` / `exhauri` / `fini`). Iterate inbound content frames with
  `s.tuus<T>().cursor()`, not direct `遍歷 取自 s.tuus<T>()`.
- **Removed (parse error):** legacy typed `端點 "route"` and block `meus`/`tuus` arms.
- Types: compiler-owned `scrinium`, `status`; opaque `sermo` conversation handle.
- English reader spellings: `sermo` is `channel`, `scrinium` is `frame`, and
  the views `meus<T>` / `tuus<T>` are `send<T>` / `recv<T>` (`s.send<T>()`,
  `s.recv<T>()`), and the view methods `fini` / `accipe` / `exhauri` are
  `finish` / `get` / `drain` (`cursor` and `da` keep their spelling). An en file
  uses the English spellings only; the Latin ones are `unknown_method` there, and
  a `la` file uses the Latin ones only.
- `sermo ↦ T` materializes inbound frames into one value of type `T` using
  the type-directed collector for `T`.
- **`sermo<O, R>` (D6.11, D6.12).** A conversation carries its types: `O` is
  what the caller sends (the opener; `nihil` when the call sends none) and
  `R` is each item frame back. `sermo` (en `channel`) takes zero or exactly
  two type arguments — bare `sermo` means `sermo<valor, valor>`, the same
  rule as bare `numerus` meaning `i64` (any other argument count is
  `sermo_arity`). For a route served by a Faber `@ 端點` handler visible to the
  caller's module (its own handlers plus its imports), the compiler fills
  `O`/`R` from that handler's own signature — its one parameter (or `nihil`)
  and its item type; every other route (a host route, or a handler outside
  that visibility) keeps bare `sermo`. `s.tuus<T>()`, `s.meus<T>()`, and
  postfix `↦ T` are checked against, or infer, `O`/`R`. `sermo<O, R>` assigns
  to bare `sermo`; the reverse is an error. The type arguments are
  compile-time only — the wire is unchanged, and frames still carry loose
  data.

**Concurrency is conversations.** Concurrent work is an `端點` conversation with
a route. There is no separate spawn, thread, or lock primitive family.
Handlers that share nothing and exchange only frames are free of data races by
construction.

Every `端點` pays the conversation cost. It goes through the router with frames,
even when both ends are local; there is no hidden fast path. The light path is
an ordinary function call, and a swappable light path is a contract passed as a
parameter.

`端點` is the effect boundary. Effects reach the outside world through `端點`
conversations, which stay portable across backends.

`@ 端點` on a function is the compiler-owned serving half of `端點`: it lets
Faber code answer a route. `@ 端點 'prefix:name'` (en `@ call`) on a top-level,
non-generic, bodied `函式` serves that route.

- Routes are exact: `prefix:name` or `prefix/name`. Pattern routes are deferred.
- The annotation must be followed — directly, or after further stacked annotations — by a `函式`; before any other declaration it is a parse error (`ad_annotation_requires_functio`), and it is never a `類型` member, `分支聯集` field or `待實作介面` method annotation.
- The handler takes zero or one parameter; the one parameter is the opener
  value of the calling `端點`.
- A handler serves one route. Reserved prefixes (such as `runtime:`) and
  builtin routes cannot be served.
- Routes form one program-wide static table built from every module in the
  program, including imported libraries. Two definitions of the same route are
  a compile error.
- Parsing, checking, and the route table exist today; serving is implemented
  on Rust, Go, TypeScript, and the MIR runner.

Web, HTTP, and framework routing stay libraries (see Annotations).

---

## Collection Operations

The former `範圍` collection pipeline DSL is retired. Collection filtering,
slicing, and aggregation are expressed through ordinary
`textus`/`lista`/`tabula`/`copia` methods and closures instead of a
grammar-level query expression. `textus`, `numerus`, `fractus`, `lista<T>`,
`tabula<K,V>`, and `copia<T>` are compiler-owned core types; their method
surfaces are not Norma declarations.

`第一個` and `ultima` are ordinary method names, not transform keywords.

`ordina(key)` (D1.7) sorts a `lista` in place by a key selector; `排序(key)`
returns a new sorted `lista` and leaves the receiver untouched. The zero-argument
forms `ordina()` / `排序()` sort by the element's natural order. Both are a
**stable** sort. The key selector's result must be a number or `textus`; other
key types are rejected.

`取自` is used for iteration (`遍歷 取自 items 設為 x`) and imports (`匯入 取自 "path"`).

### Iteration coordinates (`在`)

The optional `在` coordinate clause names the index a loop is walking. The
en reader spelling is "at": `遍歷 取自 grid 在 [r, c]` reads as iterating
`grid` at coordinates `[r, c]`.

- **`lista`** (D3.1): one name binds the element's position
  (`遍歷 取自 items 在 [i] 設為 v`).
- **`tabula`** (D3.1-D3.3): one name binds the entry's key
  (`遍歷 取自 m 在 [k] 設為 v`); a composite-key
  `tabula<元組<K1, …, Kn>, V>` takes N names, one per part of the `元組`
  key, in declared part order.
- **Tensor / matrix**: as before — one name per axis, first name = outermost
  axis, and later names walk successively inner axes; arity must equal rank
  (fewer or more names is a structured reject).
- **No index surface, no `在`.** `copia`, cursors, generators, `textus`, and
  `sparsa` have no index to name; `在` on any of them is a structured
  reject (`itera_apud_requires_indexed_iterable`), not a silent no-op.
- **`在` requires `取自`.** The coordinate clause is only valid on `遍歷 取自`
  (element iteration); `遍歷 範圍` range loops and `遍歷 從` reject it.
- The coordinate names are immutable index bindings scoped to the loop body,
  distinct from the element binder that follows the clause.
- **`執行緒` lane binder (FLD K2).** An optional `執行緒 IDENTIFIER` clause
  (`filum_clause`, shared with `歸約`) follows the `在` clause and
  precedes the mutability marker: `遍歷 取自 t 在 [i, j] 執行緒 f 設為 v { … }`.
  The clause also parses without `在` (`遍歷 取自 t 執行緒 f 設為 v { … }`).
  The parser claims `執行緒` only in that slot, so a binder named `執行緒` after
  the mutability marker stays an ordinary name. The binder is declared
  metadata; semantic passes own what it binds.

**Composite-key index (D3.2, D3.3).** The same bracket-list shape indexes a
composite key outside a loop, too: on a `tabula<元組<K1, …, Kn>, V>`,
`m[[k1, …, kn]]` reads or writes the entry keyed by that `元組` — an
ordinary index expression, not a distinct production. A bracket list of the
wrong part count or part type falls through to the ordinary map-index
type-mismatch report.

**Hashable keys and elements (D3.4).** A `tabula` key or `copia` element must
be hashable: no `fractus` of any width (NaN breaks equality; ±0 hash apart on
some targets), no mutable collection (`lista`, `tabula`, `copia`, and the
other reference collections), no `valor`/`json`/`regex`. `元組`, `類型`,
and `分支聯集` keys/elements are hashable when every part is. A non-hashable
map key is `tabula_key_not_hashable`; a non-hashable set element is
`copia_element_not_hashable`. See Loops for map/set iteration order.

---

## Fac Block


- `執行 { ... }` is the explicit `do` block and executes its body once.
- `執行 { ... } 當 condition` is the post-test loop form; postfix `當` attaches only to `執行`, not arbitrary preceding blocks.
- `捕捉` is an attachment shared by several structured forms, not a semantic mode owned by `執行`. A plain `執行` is often used when an otherwise unattached block needs a local handler: `執行 { ... } 捕捉 err { ... }`.
- The modifier slot directly after `執行` holds `全部` (en `do all`) or `裝置` (en `do device`), never both: a second modifier word is `PARSE020 fac_modifier_redundant`. Both words are contextual and claimed only in this slot, so `let int device ← 1` stays legal.
- In runner/device execution, `執行 全部 { ... } 捕捉 err { ... }` defers per-call delivery of recoverable errors from dispatched failable kernels to that handled block's owning handler. A host read that synchronizes device data can deliver an error before block exit; generated checks and block/loop delivery points can also observe it. This is not universal fault capture: fatal device faults are not made catchable, and other executors do not use the runner's device-delivery markers.
- With a `當` tail, delivery occurs per iteration. Because the `捕捉` on `執行 全部 { ... } 捕捉 ... 當 condition` wraps the whole loop, a delivered error handled there exits that loop; a handler inside an outer `當` loop can instead let the outer loop continue.
- `執行 裝置 { ... }` (en `do device`) is a checked device-resident region. Its defined model is the surface of a private kernel-of-kernels function called once under `執行 全部` (gpu-reset rule 12.7: a kernel body may be split into sequential launches at whole-grid dependencies); V0 checks this model but does not synthesize that expansion. Inside the block the existing kernel legality applies to everything between launches: a host call is `nucleum_host_call`, a host effect `nucleum_host_effect`, a host-typed capture `nucleum_host_type`, a handler `nucleum_recovery`, and ordinary `當` / `遍歷 範圍` loops are unchanged. Captures are classified by writes (a captured `傳入` scalar or genus is device state). The block's own `捕捉` stays outside it and receives recoverable kernel errors through the handled-block delivery mechanism, which may observe them before block exit; a `當` tail keeps host loop control with per-iteration delivery. These are refused by name: a nested `執行 裝置` (`fac_machina_nested`), a `傳回` / `中斷` / `繼續` leaving the block (`nucleum_block_exit_crosses_boundary`), a direct `執行緒` loop or `歸約` outside any kernel call (`nucleum_region_lane_loop_not_built`), a tensor allocation inside (`nucleum_region_alloc_not_built`), and a condition, bound, scalar store or kernel scalar argument that reads a value computed on the device (`nucleum_region_device_read_not_built`). `faber check --device` prints each region's inputs, in-outs, launches per iteration and its host-sync count. In V0 the region is checked, not compiled differently: it lowers exactly as `執行 全部`, so every device route is unchanged.

---

## Admitted, Not Shipped

These are ruled or admitted for the language and are **not fully accepted by
the compiler today**. A production may appear in the grammar before its parser,
typing, and lowering work is complete; each row records that state.

| Construct | State |
| --------- | ----- |
| Superscript powers `x²`, `r⁻¹` | planned goal; the lexer rejects the superscript digits (`LEX004`) |
| Multi-subject `比對` lowering | parses and is coverage-checked; lowered only by the Rust emitter |
| Run-time capacities and extents (`[H, W]`, `_`) | admitted (FLD K14); today every extent and capacity is a compile-time value |

---

## Target Support

Target support is **not** part of the grammar — this file defines only the
language. For which grammar each compilation target lowers, and the runtime
policy around it, see:

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — generated grammar×target lowerability matrix (the official rows).
- [Target matrix](https://faberlang.dev/en-US/toolchain/target-matrix.html) on the documentation site — runtime/contract policy (erase/warn/defer), pipeline routing, per-target contracts.

**Conditional compilation is package-granular.** A package's `faber.toml`
declares its target or targets (`[build] target = "ts"`, or
`targets = ["rust", "ts"]`). There are no conditionals inside a package: no
`#if`, no in-body `cfg`, and no per-file target selection.

A multi-target package stays target-neutral. Its per-target parts live in the
per-target manifest sections (`[target.ts]`). Code that needs a genuinely
different implementation per target is split into separate packages, and the
consumer chooses one.

Feature flags (`@ feature`, `[features]`) belong to the visibility model and
are unchanged. `@ 不穩定` stays the marker for "not implemented on this target
yet"; on a top-level `函式` with no `TARGET` it marks a function that is not
implemented on any target, and the block may be left off.

There is no `unsafe`. Faber code is always checked. Code that must step
outside the checker is foreign code, written outside Faber.

---

## Critical Syntax Rules

1. **Type-first parameters**: `函式 f(numerus x)` NOT `函式 f(x: numerus)`
2. **Type-first declarations**: `定值 textus name` NOT `定值 name: textus`
3. **Iteration loops**: `遍歷 取自/從 collection 定值/變值 item { }` or `遍歷 範圍 range 定值/變值 item { }` (verb-first, source, then binding)
4. **Parentheses around conditions are valid but not idiomatic**: prefer `若 x ≻ 0 { }` or `若 flag ≡ 真 { }` over `若 (x ≻ 0) { }`
5. **Scribe-family keywords claim statement-initial position only when not followed by `(`** — `註記 x` is the output statement; a statement-initial `註記(x)` is a call to the identifier `註記`
