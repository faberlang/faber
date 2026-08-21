# Faber Language Specification

This file is generated from `docs/grammar/source.fg` and `docs/grammar/glossary.vi.toml`;
hand edits fail the locale-render gate. Production IDs are the grammar's stable
snake_case spine and their anchors are derived from those IDs.

## Grammar {#grammar}

The grammar below is the identity rendering of the validated source. Normative detail is kept in this English sidecar and rendered as documentation; the source remains the syntax authority.

```ebnf
# formerly: fabFile
# [001] fab_file
fab_file ::= frontmatter? program
# [002] frontmatter
frontmatter ::= FRONTMATTER_DELIMITER NEWLINE TOML_LINES FRONTMATTER_DELIMITER NEWLINE?
# [003] program
program ::= statement*
# [004] statement
statement ::= annotation* statement_core
# formerly: statementCore
# [005] statement_core
statement_core ::= importa_decl | binding_decl | functio_decl | genus_decl | implendum_decl | typus_decl | ordo_decl | discretio_decl | si_stmt | dum_stmt | itera_stmt | elige_stmt | discerne_stmt | custodi_stmt | cura_stmt | fac_stmt | redde_stmt | reddet_stmt | tacebit_stmt | cede_stmt | rumpe_stmt | perge_stmt | tacet_stmt | iace_stmt | adfirma_stmt | requirit_stmt | nota_stmt | incipit_stmt | incipiet_stmt | ex_stmt | probandum_decl | proba_stmt | block_stmt | inc_dec_stmt | expr_stmt
# formerly: bindingDecl
# [006] binding_decl
binding_decl ::= fixum_decl | sit_decl | array_destruct | object_destruct | figendum_decl
# formerly: exprStmt
# [007] expr_stmt
expr_stmt ::= expression
# formerly: blockStmt
# [008] block_stmt
block_stmt ::= '{' statement* '}'
# formerly: varDecl
# [009] fixum_decl
fixum_decl ::= ('hằng' | 'biến') type_annotation IDENTIFIER (('←' expression) | ('↤' assignment inline_recovery?))?
# formerly: awaitVarDecl
# [010] figendum_decl
figendum_decl ::= ('đợi_hằng' | 'đợi_biến') type_annotation IDENTIFIER '←' expression
# formerly: sitDecl
# [011] sit_decl
sit_decl ::= 'đặt' IDENTIFIER ('←' expression)?
# formerly: arrayDestruct
# [012] array_destruct
array_destruct ::= ('hằng' | 'biến') array_pattern '←' expression
# formerly: objectDestruct
# [013] object_destruct
object_destruct ::= ('hằng' | 'biến') object_pattern '←' expression
# formerly: funcDecl
# [014] functio_decl
functio_decl ::= 'hàm' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# formerly: paramList
# [015] param_list
param_list ::= (parameter (',' parameter)*)?
# formerly: genericParams
# [016] generic_params
generic_params ::= '<' generic_param (',' generic_param)* '>'
# formerly: genericParam
# [017] generic_param
generic_param ::= IDENTIFIER | 'kích_thước' IDENTIFIER
# formerly: callTypeArgs
# [018] call_type_args
call_type_args ::= '<' type_annotation (',' type_annotation)* '>'
# [019] parameter
parameter ::= 'còn_lại'? type_annotation IDENTIFIER 'tự_nguyện'? ('như' IDENTIFIER)? ('hoặc_nếu_rỗng' expression)?
# formerly: funcModifier
# [020] func_modifier
func_modifier ::= 'đối_số' IDENTIFIER | 'được_sửa' IDENTIFIER ('như' IDENTIFIER)? | 'lỗi' IDENTIFIER | 'thoát' (IDENTIFIER | NUMBER) | 'bất_biến' | 'ném_lỗi' | 'lựa_chọn' IDENTIFIER
# formerly: callablePosture
# [021] callable_posture
callable_posture ::= 'async' | 'sinh' | 'async_sinh'
# formerly: returnClause
# [022] return_clause
return_clause ::= '→' type_annotation
# formerly: alternateExitClause
# [023] alternate_exit_clause
alternate_exit_clause ::= '⇥' type_annotation
# formerly: stmtBodyJoint
# [024] ergo_joint
ergo_joint ::= 'do_đó'
# formerly: clausuraJoint
# [025] clausura_joint
clausura_joint ::= '∴'
# formerly: clausuraExpr
# [026] clausura_expr
clausura_expr ::= compact_clausura_expr | clausura_legacy_expr
# formerly: compactClausuraExpr
# [027] compact_clausura_expr
compact_clausura_expr ::= clausura_signature clausura_joint (expression | fac_block)
# formerly: clausuraSignature
# [028] clausura_signature
clausura_signature ::= (clausura_param | '(' clausura_params? ')') return_clause? alternate_exit_clause?
# formerly: closureFacBlock
# [029] fac_block
fac_block ::= 'làm' block_stmt cape_clause?
# formerly: legacyClausuraExpr
# [030] clausura_legacy_expr
clausura_legacy_expr ::= 'đóng' clausura_params? ('→' type_annotation)? (':' expression | block_stmt)
# formerly: clausuraParams
# [031] clausura_params
clausura_params ::= clausura_param (',' clausura_param)*
# formerly: clausuraParam
# [032] clausura_param
clausura_param ::= type_annotation IDENTIFIER
# formerly: genusDecl
# [033] genus_decl
genus_decl ::= 'trừu_tượng'? 'kiểu' IDENTIFIER generic_params? ('dưới' IDENTIFIER)? ('thực_thi' IDENTIFIER (',' IDENTIFIER)*)? '{' genus_member* '}'
# formerly: genusMember
# [034] genus_member
genus_member ::= annotation* (field_decl | functio_method_decl)
# formerly: fieldDecl
# [035] field_decl
field_decl ::= 'tĩnh'? 'ràng_buộc'? type_annotation IDENTIFIER 'tự_nguyện'? ('=' expression)?
# formerly: methodDecl
# [036] functio_method_decl
functio_method_decl ::= 'hàm' IDENTIFIER generic_params? '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause? block_stmt
# [037] annotation
annotation ::= nucleum_annotation | braced_annotation | annotation_sugar
# formerly: annotationName
# [038] annotation_name
annotation_name ::= ANNOTATION_NAME
# formerly: bracedAnnotation
# [039] braced_annotation
braced_annotation ::= '@' annotation_name '{' annotation_field_list? '}'
# formerly: annotationFieldList
# [040] annotation_field_list
annotation_field_list ::= annotation_field (',' annotation_field)*
# formerly: annotationField
# [041] annotation_field
annotation_field ::= ANNOTATION_FIELD_NAME '=' (expression | type_annotation)
# formerly: annotationSugar
# [042] annotation_sugar
annotation_sugar ::= '@' annotation_name NON_NEWLINE_TOKEN* NEWLINE
# formerly: nucleumAnnotation
# [043] nucleum_annotation
nucleum_annotation ::= nucleum_sugar | nucleum_braced
# formerly: nucleumSugar
# [044] nucleum_sugar
nucleum_sugar ::= '@' 'hạt_nhân' nucleum_modifier? NEWLINE
# formerly: nucleumBraced
# [045] nucleum_braced
nucleum_braced ::= '@' 'hạt_nhân' '{' nucleum_field_list? '}'
# formerly: nucleumModifier
# [046] nucleum_modifier
nucleum_modifier ::= 'mảnh'
# formerly: nucleumFieldList
# [047] nucleum_field_list
nucleum_field_list ::= nucleum_field (',' nucleum_field)*
# formerly: nucleumField
# [048] nucleum_field
nucleum_field ::= 'mảnh' '=' ('đúng' | 'sai')
# formerly: implendumDecl
# [049] implendum_decl
implendum_decl ::= 'giao_ước' IDENTIFIER generic_params? '{' implendum_method_decl* '}'
# formerly: implendumMethod
# [050] implendum_method_decl
implendum_method_decl ::= annotation* 'hàm' IDENTIFIER '(' param_list ')' func_modifier* callable_posture? return_clause? alternate_exit_clause?
# formerly: typeAliasDecl
# [051] typus_decl
typus_decl ::= 'kiểu_tên' IDENTIFIER generic_params? '=' type_annotation
# formerly: enumDecl
# [052] ordo_decl
ordo_decl ::= 'liệt_kê' IDENTIFIER '{' enum_member (',' enum_member)* '}'
# formerly: enumMember
# [053] enum_member
enum_member ::= IDENTIFIER ('=' ('-'? NUMBER | STRING))?
# formerly: discretioDecl
# [054] discretio_decl
discretio_decl ::= 'hợp_nhất' IDENTIFIER generic_params? '{' variant (',' variant)* '}'
# [055] variant
variant ::= IDENTIFIER ('{' variant_fields '}')?
# formerly: variantFields
# [056] variant_fields
variant_fields ::= (type_annotation IDENTIFIER)*
# formerly: importDecl
# [057] importa_decl
importa_decl ::= importa_record | importa_sugar
# formerly: importRecord
# [058] importa_record
importa_record ::= 'nhập' '{' import_field_list? '}'
# formerly: importFieldList
# [059] import_field_list
import_field_list ::= import_field (',' import_field)*
# formerly: importField
# [060] import_field
import_field ::= ex_field | visibilitas_field | nomen_field | ut_field | omnia_field
# formerly: importSourceField
# [061] ex_field
ex_field ::= 'từ' '=' STRING
# formerly: importVisibilityField
# [062] visibilitas_field
visibilitas_field ::= 'visibilitas' '=' publica
# formerly: importNameField
# [063] nomen_field
nomen_field ::= 'tên' '=' IDENTIFIER
# formerly: importAliasField
# [064] ut_field
ut_field ::= 'như' '=' IDENTIFIER
# formerly: importWildcardField
# [065] omnia_field
omnia_field ::= 'mọi' '=' IDENTIFIER
# formerly: importSugar
# [066] importa_sugar
importa_sugar ::= 'nhập' 'từ' STRING publica? (named_import | wildcard_import)?
# formerly: visibility
# [067] publica
publica ::= 'công_khai'
# formerly: namedImport
# [068] named_import
named_import ::= IDENTIFIER ('như' IDENTIFIER)?
# formerly: wildcardImport
# [069] wildcard_import
wildcard_import ::= '*' 'như' IDENTIFIER
# formerly: typeAnnotation
# [070] type_annotation
type_annotation ::= owned_type ('∪' owned_type)*
# formerly: ownedType
# [071] owned_type
owned_type ::= ('ra' | 'vào' | 'sở_hữu' | 'sao_chép')? base_type
# formerly: baseType
# [072] base_type
base_type ::= hole_type | function_type | width_type_sugar | qualified_type type_arguments? | '(' type_annotation ')'
# formerly: holeType
# [073] hole_type
hole_type ::= '_' | '∪'
# formerly: qualifiedType
# [074] qualified_type
qualified_type ::= IDENTIFIER ('.' IDENTIFIER)*
# formerly: typeArguments
# [075] type_arguments
type_arguments ::= '<' type_argument (',' type_argument)* '>'
# formerly: typeArgument
# [076] type_argument
type_argument ::= labeled_type_argument | type_annotation | NATURAL | '[' figura_list? ']'
# formerly: labeledTypeArgument
# [077] labeled_type_argument
labeled_type_argument ::= IDENTIFIER ':' type_annotation
# formerly: widthTypeSugar
# [078] width_type_sugar
width_type_sugar ::= WIDTH_MARKER | LISTA_WIDTH_SUGAR | (TENSOR_WIDTH_SUGAR | SPARSA_WIDTH_SUGAR | VECTOR_WIDTH_SUGAR) shape_suffix? | MATRIX_WIDTH_SUGAR shape_suffix
# formerly: shapeSuffix
# [079] shape_suffix
shape_suffix ::= '[' figura_list? ']'
# [080] figura
figura ::= '_' | NATURAL | IDENTIFIER | '[' figura_list? ']'
# formerly: figuraList
# [081] figura_list
figura_list ::= figura (',' figura)*
# formerly: functionType
# [082] function_type
function_type ::= '(' type_list? ')' '→' type_annotation alternate_exit_clause?
# formerly: typeList
# [083] type_list
type_list ::= type_annotation (',' type_annotation)*
# formerly: ifStmt
# [084] si_stmt
si_stmt ::= 'nếu' expression arm ('nếukhôngthì' si_stmt | secus_clause)?
# formerly: elseClause
# [085] secus_clause
secus_clause ::= 'khác' else_arm
# [086] arm
arm ::= (block_stmt | ergo_joint statement) cape_clause?
# formerly: elseArm
# [087] else_arm
else_arm ::= (block_stmt | ergo_joint statement) cape_clause?
# formerly: whileStmt
# [088] dum_stmt
dum_stmt ::= 'trong_khi' expression (block_stmt | ergo_joint statement) cape_clause?
# formerly: iteraStmt
# [089] itera_stmt
itera_stmt ::= 'lặp' (('từ' | 'ra') expression | 'khoảng' expression) ('hằng' | 'biến') IDENTIFIER (block_stmt | ergo_joint statement) cape_clause?
# formerly: eligeStmt
# [090] elige_stmt
elige_stmt ::= 'chọn' expression '{' casu_elige_clause* ceterum_clause? '}' cape_clause?
# formerly: eligeCase
# [091] casu_elige_clause
casu_elige_clause ::= 'trường_hợp' expression (block_stmt | ergo_joint statement)
# formerly: defaultCase
# [092] ceterum_clause
ceterum_clause ::= 'mặc_định' (block_stmt | ergo_joint statement)
# formerly: discerneStmt
# [093] discerne_stmt
discerne_stmt ::= 'phân_tích' 'mọi'? discriminants '{' casu_variant_clause* ceterum_clause? '}'
# [094] discriminants
discriminants ::= expression (',' expression)*
# formerly: variantCase
# [095] casu_variant_clause
casu_variant_clause ::= 'trường_hợp' patterns (block_stmt | ergo_joint statement)
# [096] patterns
patterns ::= pattern ((',' | 'và') pattern)*
# [097] pattern
pattern ::= '_' | literal | (IDENTIFIER ut_pattern?)
# formerly: patternBind
# [098] ut_pattern
ut_pattern ::= ('như' IDENTIFIER) | (('hằng' | 'biến') pattern_binding (',' pattern_binding)*)
# formerly: patternBinding
# [099] pattern_binding
pattern_binding ::= IDENTIFIER ('như' IDENTIFIER)?
# formerly: guardStmt
# [100] custodi_stmt
custodi_stmt ::= 'canh_gác' '{' si_guard_clause+ '}'
# formerly: guardClause
# [101] si_guard_clause
si_guard_clause ::= 'nếu' expression (block_stmt | ergo_joint statement)
# formerly: curaStmt
# [102] cura_stmt
cura_stmt ::= 'chăm_sóc' STRING ('hằng' | 'biến') type_annotation IDENTIFIER block_stmt cape_clause?
# formerly: extractStmt
# [103] ex_stmt
ex_stmt ::= 'từ' expression ('hằng' | 'biến') extract_fields
# formerly: extractFields
# [104] extract_fields
extract_fields ::= extract_field (',' extract_field)* (',' ceteri_field)? | ceteri_field
# formerly: extractField
# [105] extract_field
extract_field ::= IDENTIFIER ('như' IDENTIFIER)?
# formerly: restField
# [106] ceteri_field
ceteri_field ::= 'còn_lại' IDENTIFIER
# formerly: returnStmt
# [107] redde_stmt
redde_stmt ::= 'trả' expression?
# formerly: returnAwaitStmt
# [108] reddet_stmt
reddet_stmt ::= 'đợi_trả' expression
# formerly: awaitDiscardStmt
# [109] tacebit_stmt
tacebit_stmt ::= 'đợi_bỏ' expression
# formerly: yieldStmt
# [110] cede_stmt
cede_stmt ::= 'nhường' expression
# formerly: breakStmt
# [111] rumpe_stmt
rumpe_stmt ::= 'dừng'
# formerly: continueStmt
# [112] perge_stmt
perge_stmt ::= 'tiếp'
# formerly: noopStmt
# [113] tacet_stmt
tacet_stmt ::= 'im_lặng'
# formerly: throwStmt
# [114] iace_stmt
iace_stmt ::= iace_expr | iace_guarded_expr
# formerly: bareThrow
# [115] iace_expr
iace_expr ::= ('ném' | 'chết') expression
# formerly: guardedThrowSugar
# [116] iace_guarded_expr
iace_guarded_expr ::= ('ném' | 'chết') expression NO_NEWLINE 'nếu' expression
# formerly: catchClause
# [117] cape_clause
cape_clause ::= 'bắt' IDENTIFIER block_stmt
# formerly: assertStmt
# [118] adfirma_stmt
adfirma_stmt ::= 'khẳng_định' expression ('chết' expression)?
# formerly: requiritStmt
# [119] requirit_stmt
requirit_stmt ::= 'yêu_cầu' expression 'ném' expression
# [120] expression
expression ::= assignment
# [121] assignment
assignment ::= ternary ('←' assignment | '↤' assignment inline_recovery?)?
# formerly: incDecStmt
# [122] inc_dec_stmt
inc_dec_stmt ::= place ('↑' | '↓')
# [123] place
place ::= call_expr
# [124] ternary
ternary ::= aut_expr (('?' expression ':' | 'thế' expression 'khác') ternary)?
# formerly: or
# [125] aut_expr
aut_expr ::= et_expr (('hoặc') et_expr)*
# formerly: and
# [126] et_expr
et_expr ::= equality (('và') equality)*
# [127] equality
equality ::= comparison equality_tail*
# formerly: equalityTail
# [128] equality_tail
equality_tail ::= ('≡' | '≠' | '≈' | '≉' | 'là' | 'không' 'là') comparison
# [129] comparison
comparison ::= bitwise_or_expr (('≺' | '≻' | '≤' | '≥' | 'trong' | 'giữa') bitwise_or_expr)*
# formerly: bitwiseOr
# [130] bitwise_or_expr
bitwise_or_expr ::= bitwise_xor_expr ('∨' bitwise_xor_expr)*
# formerly: bitwiseXor
# [131] bitwise_xor_expr
bitwise_xor_expr ::= bitwise_and_expr ('⊻' bitwise_and_expr)*
# formerly: bitwiseAnd
# [132] bitwise_and_expr
bitwise_and_expr ::= shift_expr ('∧' shift_expr)*
# formerly: shift
# [133] shift_expr
shift_expr ::= range_expr (('⇐' | '⇒') range_expr)*
# formerly: range
# [134] range_expr
range_expr ::= additive_expr range_tail?
# formerly: rangeTail
# [135] range_tail
range_tail ::= ('‥' | '…' | 'trước' | 'tới') additive_expr ('qua' additive_expr)?
# formerly: additive
# [136] additive_expr
additive_expr ::= multiplicative_expr (('+' | '-') multiplicative_expr)*
# formerly: multiplicative
# [137] multiplicative_expr
multiplicative_expr ::= vel_expr (('*' | '/' | '%' | '·' | '×' | '⊗' | '⊙') vel_expr)*
# formerly: coalesce
# [138] vel_expr
vel_expr ::= unary_expr ('hoặc_nếu_rỗng' vel_rhs)*
# formerly: velRhs
# [139] vel_rhs
vel_rhs ::= unary_expr vel_range_tail?
# formerly: velRangeTail
# [140] vel_range_tail
vel_range_tail ::= ('‥' | '…' | 'trước' | 'tới') unary_expr ('qua' unary_expr)?
# formerly: unary
# [141] unary_expr
unary_expr ::= ('-' | '¬' | 'không') unary_expr | finge_expr | cast_expr
# formerly: gradientExpr
# [142] gradient_expr
gradient_expr ::= call_expr ('∇' gradient_selection?)?
# formerly: gradientSelection
# [143] gradient_selection
gradient_selection ::= '[' gradient_place (',' gradient_place)* ']'
# formerly: gradientPlace
# [144] gradient_place
gradient_place ::= expression
# formerly: cast
# [145] cast_expr
cast_expr ::= gradient_expr ('∷' type_annotation | conversio_expr)*
# formerly: conversio
# [146] conversio_expr
conversio_expr ::= '↦' type_annotation inline_recovery?
# formerly: inlineRecovery
# [147] inline_recovery
inline_recovery ::= '⇥' unary_expr
# formerly: call
# [148] call_expr
call_expr ::= primary (call_suffix | member_suffix | optional_suffix | non_null_suffix)*
# formerly: callSuffix
# [149] call_suffix
call_suffix ::= call_type_args? '(' argument_list ')'
# formerly: memberSuffix
# [150] member_suffix
member_suffix ::= '.' IDENTIFIER | '[' expression ']'
# formerly: optionalSuffix
# [151] optional_suffix
optional_suffix ::= '?.' IDENTIFIER | '?[' expression ']' | '?(' argument_list ')'
# formerly: nonNullSuffix
# [152] non_null_suffix
non_null_suffix ::= '!.' IDENTIFIER | '![' expression ']' | '!(' argument_list ')'
# formerly: argumentList
# [153] argument_list
argument_list ::= (argument (',' argument)*)?
# [154] argument
argument ::= template_argument | 'rải'? expression
# formerly: templateArgument
# [155] template_argument
template_argument ::= 'rải'? IDENTIFIER ':' expression
# [156] literal
literal ::= NUMBER | STRING | ASCII_STRING | BACKTICK_STRING | OCTETI_STRING | 'đúng' | 'sai' | 'rỗng'
# [157] primary
primary ::= IDENTIFIER | literal | 'tôi' | array_literal | json_literal | typed_constructor | iuncta_expr | ad_expr | clausura_expr | praefixum_expr | scriptum_expr | lege_expr | '(' expression ')'
# formerly: adExpr
# [158] ad_expr
ad_expr ::= 'gọi' ASCII_STRING ad_opener?
# formerly: adOpener
# [159] ad_opener
ad_opener ::= '(' expression ')'
# formerly: arrayLiteral
# [160] array_literal
array_literal ::= '[' argument_list? ']'
# formerly: iunctaExpr
# [161] iuncta_expr
iuncta_expr ::= 'bộ' type_arguments '[' argument_list? ']'
# formerly: jsonLiteral
# [162] json_literal
json_literal ::= '{' (json_member (',' json_member)*)? '}'
# formerly: jsonMember
# [163] json_member
json_member ::= STRING ':' json_value
# formerly: typedConstructor
# [164] typed_constructor
typed_constructor ::= type_annotation '{' field_list? '}'
# formerly: fieldList
# [165] field_list
field_list ::= field_init (',' field_init)*
# formerly: fieldInit
# [166] field_init
field_init ::= ('rải' expression) | (field_key '=' expression) | IDENTIFIER
# formerly: fieldKey
# [167] field_key
field_key ::= IDENTIFIER | STRING | '[' expression ']'
# formerly: jsonValue
# [168] json_value
json_value ::= json_object | json_array | json_string | json_number | 'true' | 'false' | 'null'
# formerly: jsonObject
# [169] json_object
json_object ::= '{' (json_member (',' json_member)*)? '}'
# formerly: jsonArray
# [170] json_array
json_array ::= '[' (json_value (',' json_value)*)? ']'
# formerly: jsonString
# [171] json_string
json_string ::= STRING
# formerly: jsonNumber
# [172] json_number
json_number ::= NUMBER
# formerly: fingeExpr
# [173] finge_expr
finge_expr ::= 'tạo' qualified_ident ('{' field_list '}')? ('∷' type_annotation)?
# formerly: qualifiedIdent
# [174] qualified_ident
qualified_ident ::= IDENTIFIER ('.' IDENTIFIER)*
# formerly: praefixumExpr
# [175] praefixum_expr
praefixum_expr ::= 'tiền_tố' (block_stmt | '(' expression ')')
# formerly: scriptumExpr
# [176] scriptum_expr
scriptum_expr ::= 'văn_bản_hóa' '(' STRING (',' expression)* ')'
# formerly: legeExpr
# [177] lege_expr
lege_expr ::= 'đọc' 'dòng'?
# formerly: objectPattern
# [178] object_pattern
object_pattern ::= '{' pattern_property (',' pattern_property)* '}'
# formerly: patternProperty
# [179] pattern_property
pattern_property ::= 'còn_lại'? IDENTIFIER ('như' IDENTIFIER)?
# formerly: arrayPattern
# [180] array_pattern
array_pattern ::= '[' array_pattern_element (',' array_pattern_element)* ']'
# formerly: arrayPatternElement
# [181] array_pattern_element
array_pattern_element ::= '_' | 'còn_lại'? IDENTIFIER
# formerly: outputStmt
# [182] nota_stmt
nota_stmt ::= ('ghi_chú' | 'xem' | 'cảnh_báo' | 'viết') expression (',' expression)*
# formerly: entryHeader
# [183] entry_header
entry_header ::= ('đối_số' IDENTIFIER)? ('thoát' expression)?
# formerly: incipitStmt
# [184] incipit_stmt
incipit_stmt ::= 'bắt_đầu' entry_header block_stmt
# formerly: incipietStmt
# [185] incipiet_stmt
incipiet_stmt ::= 'bắt_đầu_bất_đồng_bộ' entry_header block_stmt
# formerly: probandumDecl
# [186] probandum_decl
probandum_decl ::= 'đối_tượng_kiểm_thử' STRING proba_modifier* '{' probandum_body '}'
# formerly: probandumBody
# [187] probandum_body
probandum_body ::= (praepara_block | probandum_decl | proba_stmt)*
# formerly: probaStmt
# [188] proba_stmt
proba_stmt ::= 'kiểm_thử' STRING proba_modifier* block_stmt
# formerly: probaModifier
# [189] proba_modifier
proba_modifier ::= 'bỏ_qua' STRING | 'việc_cần_làm' STRING | 'chỉ' | 'nhãn' STRING | 'thời_gian' NUMBER | 'đo_lường' | 'lặp_lại' NUMBER | 'mong_manh' NUMBER | 'chỉ_trong' STRING
# formerly: praeparaBlock
# [190] praepara_block
praepara_block ::= ('chuẩn_bị' | 'sẽ_chuẩn_bị' | 'sau_chuẩn_bị' | 'sẽ_sau_chuẩn_bị') 'mọi'? block_stmt
# formerly: facBlockStmt
# [191] fac_stmt
fac_stmt ::= 'làm' block_stmt cape_clause? ('trong_khi' expression)?
# [192] IDENTIFIER
IDENTIFIER ::=
# [193] NUMBER
NUMBER ::=
# [194] NATURAL
NATURAL ::=
# [195] STRING
STRING ::=
# [196] ASCII_STRING
ASCII_STRING ::=
# [197] BACKTICK_STRING
BACKTICK_STRING ::=
# [198] OCTETI_STRING
OCTETI_STRING ::=
# [199] NEWLINE
NEWLINE ::=
# [200] WIDTH_MARKER
WIDTH_MARKER ::=
# [201] LISTA_WIDTH_SUGAR
LISTA_WIDTH_SUGAR ::=
# [202] TENSOR_WIDTH_SUGAR
TENSOR_WIDTH_SUGAR ::=
# [203] SPARSA_WIDTH_SUGAR
SPARSA_WIDTH_SUGAR ::=
# [204] VECTOR_WIDTH_SUGAR
VECTOR_WIDTH_SUGAR ::=
# [205] MATRIX_WIDTH_SUGAR
MATRIX_WIDTH_SUGAR ::=
# [206] FRONTMATTER_DELIMITER
FRONTMATTER_DELIMITER ::=
# [207] TOML_LINES
TOML_LINES ::=
# [208] ANNOTATION_NAME
ANNOTATION_NAME ::=
# [209] ANNOTATION_FIELD_NAME
ANNOTATION_FIELD_NAME ::=
# [210] NON_NEWLINE_TOKEN
NON_NEWLINE_TOKEN ::=
# [211] NO_NEWLINE
NO_NEWLINE ::=
```

## Production Index {#production-index}

| ID | Anchor | Status | Former names |
|---|---|---|---|
| [`IDENTIFIER`](#identifier) | `#identifier` | capture-pending | — |
| [`NUMBER`](#number) | `#number` | capture-pending | — |
| [`NATURAL`](#natural) | `#natural` | capture-pending | — |
| [`STRING`](#string) | `#string` | capture-pending | — |
| [`ASCII_STRING`](#ascii-string) | `#ascii-string` | capture-pending | — |
| [`BACKTICK_STRING`](#backtick-string) | `#backtick-string` | capture-pending | — |
| [`OCTETI_STRING`](#octeti-string) | `#octeti-string` | capture-pending | — |
| [`NEWLINE`](#newline) | `#newline` | capture-pending | — |
| [`WIDTH_MARKER`](#width-marker) | `#width-marker` | capture-pending | — |
| [`LISTA_WIDTH_SUGAR`](#lista-width-sugar) | `#lista-width-sugar` | capture-pending | — |
| [`TENSOR_WIDTH_SUGAR`](#tensor-width-sugar) | `#tensor-width-sugar` | capture-pending | — |
| [`SPARSA_WIDTH_SUGAR`](#sparsa-width-sugar) | `#sparsa-width-sugar` | capture-pending | — |
| [`VECTOR_WIDTH_SUGAR`](#vector-width-sugar) | `#vector-width-sugar` | capture-pending | — |
| [`MATRIX_WIDTH_SUGAR`](#matrix-width-sugar) | `#matrix-width-sugar` | capture-pending | — |
| [`FRONTMATTER_DELIMITER`](#frontmatter-delimiter) | `#frontmatter-delimiter` | capture-pending | — |
| [`TOML_LINES`](#toml-lines) | `#toml-lines` | capture-pending | — |
| [`ANNOTATION_NAME`](#annotation-name) | `#annotation-name` | capture-pending | — |
| [`ANNOTATION_FIELD_NAME`](#annotation-field-name) | `#annotation-field-name` | capture-pending | — |
| [`NON_NEWLINE_TOKEN`](#non-newline-token) | `#không-newline-token` | capture-pending | — |
| [`NO_NEWLINE`](#no-newline) | `#no-newline` | capture-pending | — |
| [`fab_file`](#fab-file) | `#fab-file` | live | fabFile |
| [`frontmatter`](#frontmatter) | `#frontmatter` | live | — |
| [`program`](#program) | `#program` | live | — |
| [`statement`](#statement) | `#statement` | live | — |
| [`statement_core`](#statement-core) | `#statement-core` | live | statementCore |
| [`binding_decl`](#binding-decl) | `#binding-decl` | live | bindingDecl |
| [`expr_stmt`](#expr-stmt) | `#expr-stmt` | live | exprStmt |
| [`block_stmt`](#block-stmt) | `#block-stmt` | live | blockStmt |
| [`fixum_decl`](#fixum-decl) | `#hằng-decl` | live | varDecl |
| [`figendum_decl`](#figendum-decl) | `#đợi_hằng-decl` | live | awaitVarDecl |
| [`sit_decl`](#sit-decl) | `#đặt-decl` | live | sitDecl |
| [`array_destruct`](#array-destruct) | `#array-destruct` | live | arrayDestruct |
| [`object_destruct`](#object-destruct) | `#object-destruct` | live | objectDestruct |
| [`functio_decl`](#functio-decl) | `#hàm-decl` | live | funcDecl |
| [`param_list`](#param-list) | `#param-list` | live | paramList |
| [`generic_params`](#generic-params) | `#generic-params` | live | genericParams |
| [`generic_param`](#generic-param) | `#generic-param` | live | genericParam |
| [`call_type_args`](#call-type-args) | `#call-type-args` | live | callTypeArgs |
| [`parameter`](#parameter) | `#parameter` | live | — |
| [`func_modifier`](#func-modifier) | `#func-modifier` | live | funcModifier |
| [`callable_posture`](#callable-posture) | `#callable-posture` | live | callablePosture |
| [`return_clause`](#return-clause) | `#return-clause` | live | returnClause |
| [`alternate_exit_clause`](#alternate-exit-clause) | `#alternate-exit-clause` | live | alternateExitClause |
| [`ergo_joint`](#ergo-joint) | `#do_đó-joint` | live | stmtBodyJoint |
| [`clausura_joint`](#clausura-joint) | `#đóng-joint` | live | clausuraJoint |
| [`clausura_expr`](#clausura-expr) | `#đóng-expr` | live | clausuraExpr |
| [`compact_clausura_expr`](#compact-clausura-expr) | `#compact-đóng-expr` | live | compactClausuraExpr |
| [`clausura_signature`](#clausura-signature) | `#đóng-signature` | live | clausuraSignature |
| [`fac_block`](#fac-block) | `#làm-block` | live | closureFacBlock |
| [`clausura_legacy_expr`](#clausura-legacy-expr) | `#đóng-legacy-expr` | live | legacyClausuraExpr |
| [`clausura_params`](#clausura-params) | `#đóng-params` | live | clausuraParams |
| [`clausura_param`](#clausura-param) | `#đóng-param` | live | clausuraParam |
| [`genus_decl`](#genus-decl) | `#kiểu-decl` | live | genusDecl |
| [`genus_member`](#genus-member) | `#kiểu-member` | live | genusMember |
| [`field_decl`](#field-decl) | `#field-decl` | live | fieldDecl |
| [`functio_method_decl`](#functio-method-decl) | `#hàm-method-decl` | live | methodDecl |
| [`annotation`](#annotation) | `#annotation` | live | — |
| [`annotation_name`](#annotation-name) | `#annotation-name` | live | annotationName |
| [`braced_annotation`](#braced-annotation) | `#braced-annotation` | live | bracedAnnotation |
| [`annotation_field_list`](#annotation-field-list) | `#annotation-field-list` | live | annotationFieldList |
| [`annotation_field`](#annotation-field) | `#annotation-field` | live | annotationField |
| [`annotation_sugar`](#annotation-sugar) | `#annotation-sugar` | live | annotationSugar |
| [`nucleum_annotation`](#nucleum-annotation) | `#hạt_nhân-annotation` | live | nucleumAnnotation |
| [`nucleum_sugar`](#nucleum-sugar) | `#hạt_nhân-sugar` | live | nucleumSugar |
| [`nucleum_braced`](#nucleum-braced) | `#hạt_nhân-braced` | live | nucleumBraced |
| [`nucleum_modifier`](#nucleum-modifier) | `#hạt_nhân-modifier` | live | nucleumModifier |
| [`nucleum_field_list`](#nucleum-field-list) | `#hạt_nhân-field-list` | live | nucleumFieldList |
| [`nucleum_field`](#nucleum-field) | `#hạt_nhân-field` | live | nucleumField |
| [`implendum_decl`](#implendum-decl) | `#giao_ước-decl` | live | implendumDecl |
| [`implendum_method_decl`](#implendum-method-decl) | `#giao_ước-method-decl` | live | implendumMethod |
| [`typus_decl`](#typus-decl) | `#kiểu_tên-decl` | live | typeAliasDecl |
| [`ordo_decl`](#ordo-decl) | `#liệt_kê-decl` | live | enumDecl |
| [`enum_member`](#enum-member) | `#enum-member` | live | enumMember |
| [`discretio_decl`](#discretio-decl) | `#hợp_nhất-decl` | live | discretioDecl |
| [`variant`](#variant) | `#variant` | live | — |
| [`variant_fields`](#variant-fields) | `#variant-fields` | live | variantFields |
| [`importa_decl`](#importa-decl) | `#nhập-decl` | live | importDecl |
| [`importa_record`](#importa-record) | `#nhập-record` | live | importRecord |
| [`import_field_list`](#import-field-list) | `#import-field-list` | live | importFieldList |
| [`import_field`](#import-field) | `#import-field` | live | importField |
| [`ex_field`](#ex-field) | `#từ-field` | live | importSourceField |
| [`visibilitas_field`](#visibilitas-field) | `#visibilitas-field` | live | importVisibilityField |
| [`nomen_field`](#nomen-field) | `#tên-field` | live | importNameField |
| [`ut_field`](#ut-field) | `#như-field` | live | importAliasField |
| [`omnia_field`](#omnia-field) | `#mọi-field` | live | importWildcardField |
| [`importa_sugar`](#importa-sugar) | `#nhập-sugar` | live | importSugar |
| [`công_khai`](#publica) | `#công_khai` | live | visibility |
| [`named_import`](#named-import) | `#named-import` | live | namedImport |
| [`wildcard_import`](#wildcard-import) | `#wildcard-import` | live | wildcardImport |
| [`type_annotation`](#type-annotation) | `#type-annotation` | live | typeAnnotation |
| [`owned_type`](#owned-type) | `#owned-type` | live | ownedType |
| [`base_type`](#base-type) | `#base-type` | live | baseType |
| [`hole_type`](#hole-type) | `#hole-type` | live | holeType |
| [`qualified_type`](#qualified-type) | `#qualified-type` | live | qualifiedType |
| [`type_arguments`](#type-arguments) | `#type-arguments` | live | typeArguments |
| [`type_argument`](#type-argument) | `#type-argument` | live | typeArgument |
| [`labeled_type_argument`](#labeled-type-argument) | `#labeled-type-argument` | live | labeledTypeArgument |
| [`width_type_sugar`](#width-type-sugar) | `#width-type-sugar` | live | widthTypeSugar |
| [`shape_suffix`](#shape-suffix) | `#shape-suffix` | live | shapeSuffix |
| [`figura`](#figura) | `#figura` | live | — |
| [`figura_list`](#figura-list) | `#figura-list` | live | figuraList |
| [`function_type`](#function-type) | `#function-type` | live | functionType |
| [`type_list`](#type-list) | `#type-list` | live | typeList |
| [`si_stmt`](#si-stmt) | `#nếu-stmt` | live | ifStmt |
| [`secus_clause`](#secus-clause) | `#khác-clause` | live | elseClause |
| [`arm`](#arm) | `#arm` | live | — |
| [`else_arm`](#else-arm) | `#else-arm` | live | elseArm |
| [`dum_stmt`](#dum-stmt) | `#trong_khi-stmt` | live | whileStmt |
| [`itera_stmt`](#itera-stmt) | `#lặp-stmt` | live | iteraStmt |
| [`elige_stmt`](#elige-stmt) | `#chọn-stmt` | live | eligeStmt |
| [`casu_elige_clause`](#casu-elige-clause) | `#trường_hợp-chọn-clause` | live | eligeCase |
| [`ceterum_clause`](#ceterum-clause) | `#mặc_định-clause` | live | defaultCase |
| [`discerne_stmt`](#discerne-stmt) | `#phân_tích-stmt` | live | discerneStmt |
| [`discriminants`](#discriminants) | `#discriminants` | live | — |
| [`casu_variant_clause`](#casu-variant-clause) | `#trường_hợp-variant-clause` | live | variantCase |
| [`patterns`](#patterns) | `#patterns` | live | — |
| [`pattern`](#pattern) | `#pattern` | live | — |
| [`ut_pattern`](#ut-pattern) | `#như-pattern` | live | patternBind |
| [`pattern_binding`](#pattern-binding) | `#pattern-binding` | live | patternBinding |
| [`custodi_stmt`](#custodi-stmt) | `#canh_gác-stmt` | live | guardStmt |
| [`si_guard_clause`](#si-guard-clause) | `#nếu-guard-clause` | live | guardClause |
| [`cura_stmt`](#cura-stmt) | `#chăm_sóc-stmt` | live | curaStmt |
| [`ex_stmt`](#ex-stmt) | `#từ-stmt` | live | extractStmt |
| [`extract_fields`](#extract-fields) | `#extract-fields` | live | extractFields |
| [`extract_field`](#extract-field) | `#extract-field` | live | extractField |
| [`ceteri_field`](#ceteri-field) | `#còn_lại-field` | live | restField |
| [`redde_stmt`](#redde-stmt) | `#trả-stmt` | live | returnStmt |
| [`reddet_stmt`](#reddet-stmt) | `#đợi_trả-stmt` | live | returnAwaitStmt |
| [`tacebit_stmt`](#tacebit-stmt) | `#đợi_bỏ-stmt` | live | awaitDiscardStmt |
| [`cede_stmt`](#cede-stmt) | `#nhường-stmt` | live | yieldStmt |
| [`rumpe_stmt`](#rumpe-stmt) | `#dừng-stmt` | live | breakStmt |
| [`perge_stmt`](#perge-stmt) | `#tiếp-stmt` | live | continueStmt |
| [`tacet_stmt`](#tacet-stmt) | `#im_lặng-stmt` | live | noopStmt |
| [`iace_stmt`](#iace-stmt) | `#ném-stmt` | live | throwStmt |
| [`iace_expr`](#iace-expr) | `#ném-expr` | live | bareThrow |
| [`iace_guarded_expr`](#iace-guarded-expr) | `#ném-guarded-expr` | live | guardedThrowSugar |
| [`cape_clause`](#cape-clause) | `#bắt-clause` | live | catchClause |
| [`adfirma_stmt`](#adfirma-stmt) | `#khẳng_định-stmt` | live | assertStmt |
| [`requirit_stmt`](#requirit-stmt) | `#yêu_cầu-stmt` | live | requiritStmt |
| [`expression`](#expression) | `#expression` | live | — |
| [`assignment`](#assignment) | `#assignment` | live | — |
| [`inc_dec_stmt`](#inc-dec-stmt) | `#inc-dec-stmt` | live | incDecStmt |
| [`place`](#place) | `#place` | live | — |
| [`ternary`](#ternary) | `#ternary` | live | — |
| [`aut_expr`](#aut-expr) | `#hoặc-expr` | live | or |
| [`et_expr`](#et-expr) | `#và-expr` | live | and |
| [`equality`](#equality) | `#equality` | live | — |
| [`equality_tail`](#equality-tail) | `#equality-tail` | live | equalityTail |
| [`comparison`](#comparison) | `#comparison` | live | — |
| [`bitwise_or_expr`](#bitwise-or-expr) | `#bitwise-or-expr` | live | bitwiseOr |
| [`bitwise_xor_expr`](#bitwise-xor-expr) | `#bitwise-xor-expr` | live | bitwiseXor |
| [`bitwise_and_expr`](#bitwise-and-expr) | `#bitwise-and-expr` | live | bitwiseAnd |
| [`shift_expr`](#shift-expr) | `#shift-expr` | live | shift |
| [`range_expr`](#range-expr) | `#range-expr` | live | range |
| [`range_tail`](#range-tail) | `#range-tail` | live | rangeTail |
| [`additive_expr`](#additive-expr) | `#additive-expr` | live | additive |
| [`multiplicative_expr`](#multiplicative-expr) | `#multiplicative-expr` | live | multiplicative |
| [`vel_expr`](#vel-expr) | `#hoặc_nếu_rỗng-expr` | live | coalesce |
| [`vel_rhs`](#vel-rhs) | `#hoặc_nếu_rỗng-rhs` | live | velRhs |
| [`vel_range_tail`](#vel-range-tail) | `#hoặc_nếu_rỗng-range-tail` | live | velRangeTail |
| [`unary_expr`](#unary-expr) | `#unary-expr` | live | unary |
| [`gradient_expr`](#gradient-expr) | `#gradient-expr` | live | gradientExpr |
| [`gradient_selection`](#gradient-selection) | `#gradient-selection` | live | gradientSelection |
| [`gradient_place`](#gradient-place) | `#gradient-place` | live | gradientPlace |
| [`cast_expr`](#cast-expr) | `#cast-expr` | live | cast |
| [`conversio_expr`](#conversio-expr) | `#conversio-expr` | live | conversio |
| [`inline_recovery`](#inline-recovery) | `#inline-recovery` | live | inlineRecovery |
| [`call_expr`](#call-expr) | `#call-expr` | live | call |
| [`call_suffix`](#call-suffix) | `#call-suffix` | live | callSuffix |
| [`member_suffix`](#member-suffix) | `#member-suffix` | live | memberSuffix |
| [`optional_suffix`](#optional-suffix) | `#optional-suffix` | live | optionalSuffix |
| [`non_null_suffix`](#non-null-suffix) | `#không-null-suffix` | live | nonNullSuffix |
| [`argument_list`](#argument-list) | `#argument-list` | live | argumentList |
| [`argument`](#argument) | `#argument` | live | — |
| [`template_argument`](#template-argument) | `#template-argument` | live | templateArgument |
| [`literal`](#literal) | `#literal` | live | — |
| [`primary`](#primary) | `#primary` | live | — |
| [`ad_expr`](#ad-expr) | `#gọi-expr` | live | adExpr |
| [`ad_opener`](#ad-opener) | `#gọi-opener` | live | adOpener |
| [`array_literal`](#array-literal) | `#array-literal` | live | arrayLiteral |
| [`iuncta_expr`](#iuncta-expr) | `#bộ-expr` | live | iunctaExpr |
| [`json_literal`](#json-literal) | `#json-literal` | live | jsonLiteral |
| [`json_member`](#json-member) | `#json-member` | live | jsonMember |
| [`typed_constructor`](#typed-constructor) | `#typed-constructor` | live | typedConstructor |
| [`field_list`](#field-list) | `#field-list` | live | fieldList |
| [`field_init`](#field-init) | `#field-init` | live | fieldInit |
| [`field_key`](#field-key) | `#field-key` | live | fieldKey |
| [`json_value`](#json-value) | `#json-value` | live | jsonValue |
| [`json_object`](#json-object) | `#json-object` | live | jsonObject |
| [`json_array`](#json-array) | `#json-array` | live | jsonArray |
| [`json_string`](#json-string) | `#json-string` | live | jsonString |
| [`json_number`](#json-number) | `#json-number` | live | jsonNumber |
| [`finge_expr`](#finge-expr) | `#tạo-expr` | live | fingeExpr |
| [`qualified_ident`](#qualified-ident) | `#qualified-ident` | live | qualifiedIdent |
| [`praefixum_expr`](#praefixum-expr) | `#tiền_tố-expr` | live | praefixumExpr |
| [`scriptum_expr`](#scriptum-expr) | `#văn_bản_hóa-expr` | live | scriptumExpr |
| [`lege_expr`](#lege-expr) | `#đọc-expr` | live | legeExpr |
| [`object_pattern`](#object-pattern) | `#object-pattern` | live | objectPattern |
| [`pattern_property`](#pattern-property) | `#pattern-property` | live | patternProperty |
| [`array_pattern`](#array-pattern) | `#array-pattern` | live | arrayPattern |
| [`array_pattern_element`](#array-pattern-element) | `#array-pattern-element` | live | arrayPatternElement |
| [`nota_stmt`](#nota-stmt) | `#ghi_chú-stmt` | live | outputStmt |
| [`entry_header`](#entry-header) | `#entry-header` | live | entryHeader |
| [`incipit_stmt`](#incipit-stmt) | `#bắt_đầu-stmt` | live | incipitStmt |
| [`incipiet_stmt`](#incipiet-stmt) | `#bắt_đầu_bất_đồng_bộ-stmt` | live | incipietStmt |
| [`probandum_decl`](#probandum-decl) | `#đối_tượng_kiểm_thử-decl` | live | probandumDecl |
| [`probandum_body`](#probandum-body) | `#đối_tượng_kiểm_thử-body` | live | probandumBody |
| [`proba_stmt`](#proba-stmt) | `#kiểm_thử-stmt` | live | probaStmt |
| [`proba_modifier`](#proba-modifier) | `#kiểm_thử-modifier` | live | probaModifier |
| [`praepara_block`](#praepara-block) | `#chuẩn_bị-block` | live | praeparaBlock |
| [`fac_stmt`](#fac-stmt) | `#làm-stmt` | live | facBlockStmt |

## Lexicon Appendix {#lexicon}

The lexical tier is descriptive and remains owned by the live lexer and
driver. `capture-pending` rows intentionally carry no invented token shape.

| Terminal | Status | Capture notes |
|---|---|---|
| `IDENTIFIER` | `capture-pending` | Lexical tier. Empty RHS; status is capture-pending. radix-lexer / driver / parser is the authority (crates/radix-lexer/src/). Not a second lexer spec. scan.rs scan_identifier; Unicode XID_Start or '_' then XID_Continue or '_'; NFKC intern; TokenKind::Ident (keywords also lex as identifiers) |
| `NUMBER` | `capture-pending` | scan.rs scan_number; decimal/hex/bin/oct integers and floats with '_' separators; TokenKind::Integer(u64) or Float(f64) |
| `NATURAL` | `capture-pending` | not a distinct lexer token; type-position TokenKind::Integer used as magnitudo capacity (no fraction/exponent) |
| `STRING` | `capture-pending` | scan.rs scan_string / scan_guillemet_block_string; double-quoted or guillemet block; TokenKind::String |
| `ASCII_STRING` | `capture-pending` | scan.rs scan_ascii_string; single-quoted; TokenKind::AsciiString |
| `BACKTICK_STRING` | `capture-pending` | scan.rs scan_backtick_string; backtick forma template; TokenKind::BacktickString |
| `OCTETI_STRING` | `capture-pending` | scan.rs scan_octeti_string; pipe-delimited hex; TokenKind::OctetiString |
| `NEWLINE` | `capture-pending` | scan.rs scan_line_break; LF or CRLF; TokenKind::Newline |
| `WIDTH_MARKER` | `capture-pending` | parser type-position identifier i8/i16/i32/i64/u8/u16/u32/u64/f16/f32/f64; not a lexer token |
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
| Iteration | `khoảng` | range iteration |
| Declarations | `trừu_tượng` | abstract genus modifier |
| Endpoints | `gọi` | capability call |
| Error | `khẳng_định` | assert |
| Iteration | `trước` | range until exclusive |
| Params | `đối_số` | CLI arguments modifier |
| Boolean | `hoặc` | or |
| Error | `bắt` | local handler |
| Control | `trường_hợp` | case |
| Async | `nhường` | yield |
| Params | `còn_lại` | rest |
| Control | `mặc_định` | default case |
| Objects | `đóng` | legacy closure |
| Type | `sao_chép` | copy ownership |
| Objects | `chăm_sóc` | with-resource |
| Params | `được_sửa` | curated options |
| Control | `canh_gác` | guard |
| Type | `ra` | borrow / for-in keys |
| Control | `phân_tích` | pattern match |
| Declarations | `hợp_nhất` | tagged union |
| Control | `trong_khi` | while / postfix until |
| Objects | `tôi` | self |
| Control | `chọn` | switch |
| Control | `do_đó` | compact statement-body joint |
| Params | `lỗi` | error channel |
| Boolean | `là` | is / equality |
| Boolean | `và` | and |
| Iteration | `từ` | for-of / import from |
| Params | `thoát` | exit code |
| Control | `làm` | do block / post-test loop |
| JSON | `false` | JSON false |
| Boolean | `sai` | false |
| Async | `async_sinh` | async stream posture |
| Async | `async` | async finite posture |
| Async | `đợi_hằng` | await-bind immutable |
| Objects | `tạo` | construct variant |
| Async | `sinh` | sync stream posture |
| Declarations | `hằng` | immutable binding |
| Testing | `mong_manh` | flaky |
| Annotation | `mảnh` | nucleum fragment |
| Declarations | `hàm` | function |
| Testing | `việc_cần_làm` | future |
| Genus | `tĩnh` | static member |
| Declarations | `kiểu` | class |
| Error | `ném` | throw |
| Error | `ném_lỗi` | throws marker |
| Params | `bất_biến` | immutable modifier |
| Declarations | `giao_ước` | interface contract |
| Genus | `thực_thi` | implements |
| Declarations | `nhập` | import |
| Type | `vào` | ownership in |
| Declarations | `bắt_đầu_bất_đồng_bộ` | async entrypoint |
| Declarations | `bắt_đầu` | entrypoint |
| Iteration | `giữa` | between |
| Iteration | `trong` | membership |
| Control | `lặp` | for |
| Objects | `bộ` | tuple type/constructor |
| Builtin | `đọc` | read |
| Builtin | `dòng` | line |
| Declarations | `kích_thước` | size/index generic parameter |
| Testing | `đo_lường` | benchmark |
| Diagnostics | `cảnh_báo` | warn |
| Error | `chết` | panic |
| Genus | `ràng_buộc` | link field |
| Literals | `rỗng` | none |
| Declarations | `tên` | import binding name |
| Boolean | `không` | not |
| Diagnostics | `ghi_chú` | note |
| Annotation | `hạt_nhân` | kernel annotation |
| JSON | `null` | JSON null |
| Testing | `bỏ_qua` | skip |
| Params | `mọi` | all / glob |
| Params | `lựa_chọn` | options modifier |
| Declarations | `liệt_kê` | enum |
| Type | `sở_hữu` | owned |
| Iteration | `qua` | range step |
| Control | `tiếp` | continue |
| Testing | `sau_chuẩn_bị` | teardown |
| Testing | `sẽ_sau_chuẩn_bị` | async teardown |
| Objects | `tiền_tố` | prefix expression |
| Testing | `chuẩn_bị` | setup |
| Testing | `sẽ_chuẩn_bị` | async setup |
| Testing | `kiểm_thử` | test |
| Testing | `đối_tượng_kiểm_thử` | test suite |
| Declarations | `công_khai` | public visibility |
| Control | `trả` | return |
| Async | `đợi_trả` | await-return |
| Testing | `lặp_lại` | repeat |
| Error | `yêu_cầu` | require |
| Control | `dừng` | break |
| Diagnostics | `viết` | diagnostic channel |
| Builtin | `văn_bản_hóa` | write |
| Control | `khác` | else |
| Control | `nếu` | if |
| Control | `thế` | then (ternary) |
| Control | `nếukhôngthì` | else-if |
| Declarations | `đặt` | inferred immutable local |
| Testing | `chỉ` | only |
| Testing | `chỉ_trong` | only-in |
| Params | `rải` | spread |
| Declarations | `tự_nguyện` | optional declaration slot |
| Genus | `dưới` | extends |
| Async | `đợi_bỏ` | await-discard |
| Control | `im_lặng` | no-op |
| Testing | `nhãn` | tag |
| Testing | `thời_gian` | timeout |
| JSON | `true` | JSON true |
| Declarations | `kiểu_tên` | type alias |
| Iteration | `tới` | range until inclusive |
| Params | `như` | as / alias |
| Declarations | `biến` | mutable binding |
| Async | `đợi_biến` | await-bind mutable |
| Boolean | `hoặc_nếu_rỗng` | nullable default |
| Boolean | `đúng` | true |
| Diagnostics | `xem` | debug |
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
lists, `liệt_kê` members, `hợp_nhất` variant lists, JSON members and array
elements, annotation / import / nucleum fields, output statement lists) —
require a comma between adjacent items and forbid one after the last.

**Declaration blocks** — self-annotating declarations (statements, `kiểu`
members, `giao_ước` methods, `hợp_nhất` payload fields) — contain no commas.
Entries are trivia-delimited.

---

## Declarations

### Variables


- `hằng` = immutable binding (write-once): it may be declared without an
  initializer and assigned exactly once later, then frozen. `biến` = mutable
  binding (reassignable), like `let`.
- `đợi_hằng` / `đợi_biến` await a `promissum<T>` or `promissum<T ⇥ E>`, bind
  the resolved `T`, and propagate a compatible alternate `E`.
- Use `_` as the type annotation when the initializer determines the type: `hằng _ name ← value`
- `đặt name ← value` is sugar for `hằng _ name ← value` (inferred immutable local)
- `đặt name` (no initializer) is sugar for `hằng _ name` — the inferred deferred
  immutable. Assign exactly once before any read.
- Typed `hằng`/`biến` initializers accept `↤` (`hằng numerus x ↤ "42"`):
  the written type is the conversion destination, then the binding is
  initialized. `đợi_hằng`/`đợi_biến` keep `←`; `hằng _`, `đặt`, and untyped
  destructuring reject `↤` (no concrete destination type).
- Deferred init: `hằng numerus x` or `đặt x` declares an uninitialized immutable
  slot that must be assigned exactly once before any read; a second assignment is
  rejected. The definite-assignment pass (semantic Phase 3a) enforces this.

### Functions


- Return syntax: `→` declares the normal success type. A bodyful function with no `→` is effect-only (`vacuum`) and must not contain `trả`. A statement-bodied closure (`làm { ... }` or legacy block body) must also spell `→ T` before it can use `trả`; expression-bodied closures may infer their result from the expression.
- Recoverable alternate-exit syntax: `⇥` declares the error-channel type. It can appear after `→ T` or alone on an effect-only failable function or closure. A closure body that uses an escaping `ném` must declare its own `⇥ E`; it cannot inherit the enclosing function's error channel. A local `làm { ... } bắt err { ... }` may catch `ném` without an enclosing `⇥`. A failable function call (`→ T ⇥ E`) inside a `⇥`-declaring function propagates to the function's alternate exit without a `làm`/`bắt` wrapper, mirroring how bare `↦` conversio and `ném` throws already behave; the call lowers to Rust `?`. A closure must still declare its own `⇥` to propagate a failable call — the enclosing function's error channel does not cross the closure boundary.
- Parameter access markers live in the type position: `ra`/`ref` (read), `vào`/`mut` (mutate), `sở_hữu` (consume), and `sao_chép` (duplicate then own). The retired parameter-prefix slot is not part of the grammar; `từ`/`from` remains the import/iteration/extraction token identity.
- Post-name marker: `tự_nguyện` (voluntary/optional provision)
- `còn_lại` marks rest parameter
- `được_sửa NAME ('như' LOCAL)?` declares an allocator requirement; `LOCAL` is the function-body alias.
- Ordinary `hàm` declarations and genus methods require bodies. Signature-only methods belong in `giao_ước`.
- `lỗi NAME` is a legacy runtime-injected `ignotum` local, and `ném_lỗi` is a legacy marker with no current semantic effect. Neither declares the typed alternate-exit contract. New failable APIs should use `⇥ E`; whether either legacy modifier should survive is unresolved.
- `do_đó` is the compact **statement-body** joint only (one-statement `nếu`/`trong_khi`/`trường_hợp`/… arms).
- `∴` is the compact **clausura** joint only. The two are not aliases.
- Compact closure block bodies must use `làm { ... }`; a closure-local `làm` body may attach `bắt`, but cannot use postfix `trong_khi`.

### Classes


### Annotations


`@ hạt_nhân mảnh` is a modifier on the `hạt_nhân` annotation (sugar or
braced `mảnh = đúng` / `sai`), not a fused annotation name and not the
graphics `@ mảnh` stage. Standalone `@ mảnh` is unchanged.

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

**Annotation contracts:** `@ annotatio` (optionally `@ annotatio { target = hàm }`)
marks a top-level `kiểu` as a compile-time annotation contract. Ordinary genera
are not annotation schemas. Applications use `@ ContractName { field = constant }`
and resolve through local declarations or imported file-interface exports.
Resolved applications lower to `HirAnnotation` with `contract_id: Some(DefId)`
and constant field values. v1 attachment target is `hàm` only; payload
scalars are `textus`, `numerus`, `fractus`, and `bivalens` (optional via
`tự_nguyện` or `T ∪ rỗng`). No compiler-owned `@ web` / controller / route families.

**JSON genera:** `@ json` on a `kiểu` is a compiler-owned data-model contract,
not a generic annotation schema. Fields must be JSON-safe (`textus`, `ascii`,
`numerus`, `fractus`, `bivalens`, `instans`, `rỗng`, `lista<T>`,
`tabula<textus, T>`, nullable `T ∪ rỗng`, or another `@ json kiểu`). Field
metadata `@ json { tên = "wire_name" }` changes the emitted object key used by
`value ↦ valor`, `value ↦ json`, and `json ↦ Genus`; JSON text remains a Norma
wire operation such as `json.pange(value ↦ json)`.

- `@ radix` is reserved for compiler-owned metadata. The historical
  morphology-stem meaning is retired; morphology remains a source naming
  discipline, not compiler-generated conjugation. Accepted directive forms are
  `@ radix lane "air"` / `"mir"` / `"hir-direct"` on top-level functions for
  explicit compiler-lane routing; unsupported lane/target combinations reject
  with diagnostics instead of being ignored.
- `@ verte` defines codegen transformation (method name or template)
- `@ nondum [TARGET] ["REASON"]` marks a declaration as present in an interface but unavailable for the target
- `@ cli "NAME"` marks an `bắt_đầu` entry as a CLI program
- `@ imperium "NAME"` marks a function as a CLI command entry point
- `@ optio NAME ...` defines a CLI option; use `kiểu_tên bivalens` for boolean flags
- `@ operandus [còn_lại] TYPE NAME ...` defines a CLI positional argument
- `@ futura` marks a function as async (legacy — prefer `async` posture word)
- `@ cursor` marks a function as generator (legacy — prefer `sinh` posture word)
- Callable posture words (`async`/`sinh`/`async_sinh`) are recognized in the signature
  slot after modifiers and before `→`/`⇥`/body; bare means synchronous finite
- `@ công_khai` marks a declaration for the file's importable (export) surface; `@ interna` marks it package-internal (same-package importable only); `@ privata` is an explicit module-private marker. Unmarked top-level declarations are module-private by default; a declaration mixing distinct visibility tiers is rejected with `SEM019` (`conflicting_visibility`)
- `@ protecta` is reserved and rejected with a semantic diagnostic; it has no package, subclass, or sibling-file visibility meaning

- `dưới` = extends, `thực_thi` = implements
- `tĩnh` = static, `ràng_buộc` = bound/property

### Interfaces


`giao_ước` is the **contract** construct: signature-only methods for `thực_thi`
(gerundive of *implere* — that which must be fulfilled). Import namespaces are
`.fab` file boundaries; exported declarations live at file top level.

### Type Aliases


### Enums


### Tagged Unions


Variant lists are an item list: comma required between variants, forbidden
after the last. Payload fields inside a variant are a declaration block
(genus-style, no commas).

### Identifier Naming

Faber has no globally reserved words. Keyword ownership is contextual per
spelling: a keyword claims only its owning grammar slot. Every user-chosen
name slot accepts every keyword spelling — declaration names, parameters,
members, binding targets (`hằng`/`biến`/`đặt` patterns and captures),
import aliases, and loop/iteration bindings. Type-name slots stay out.

Outside a spelling's owning contexts, that spelling may be an `IDENTIFIER`.
An owning context may itself be effectively global when its production
applies everywhere a statement or expression may begin. Builtin claims
(`đọc`/`dòng`/`văn_bản_hóa`/`vacua`, and the scribe family in
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
```

The `privata` import marker was removed (VM-U3); an import without a marker
does not re-export, and `công_khai` is the re-export marker. Missing named binding
defaults to the
last import path segment when it is a valid, non-conflicting identifier. If the
inferred name is invalid or collides with an existing top-level binding, spell an
explicit `tên` or `như` binding.

`nhập từ "faber:*" faber` is kernel-specific sugar: the glob lives
inside the import path string and expands the released binary's kernel manifest
into `faber.<module>.<verb>` calls. It is not a wildcard re-export and does not create a runtime aggregate value.

---

## Types


- Declaration parameters (`genericParams`) and applied arguments (`typeArguments`) are distinct grammar categories. Applied arguments admit nested types and static `figura` values. `typeArguments` still admits `NATURAL`.
- Applied `NATURAL` arguments are `kích_thước` capacity facts, not width markers. Proposed (not shipped) bounded forms use that slot: `lista<T, N>`, `textus<N>`, `ascii<N>`, `octeti<N>`. Width-marker families such as `numerus<i32>` stay the separate `widthTypeSugar` production below.
- A second applied argument on a `↦` target (`numerus<W, Hex>`, `numerus<W, Be>`) is a convert-slot hint, not a type identity, not a width marker, and not a keyword. Live text-parse hints are `Hex` / `Bin` / `Oct`. `Be` / `Le` occupy that same Hex slot for endian unpack. `typeArguments` is unchanged: these are ordinary `IDENTIFIER` arguments interpreted by conversio, not new `baseType` productions.
- Type arguments admit the hole forms: `lista<∪>` infers a heterogeneous element union and `tabula<K, ∪>` a heterogeneous value union; `lista<_>` keeps the monomorphic single-inhabitant hole.
- Explicit generic call-site lists use the same `typeArguments` production: `id<_>(x)` is a type hole (equivalent to omitted `id(x)` for a one-param callee), and mixed lists such as `both<_, textus>(a, b)` are legal. Arity stays exact (`both<_>` is still one argument). `∪` in that list is rejected (`explicit_union_type_arg_unsupported`): a callee type param is a monomorphic witness slot.
- `labeledTypeArgument` is the optional label prefix on `bộ` type arguments only (`bộ<gx: f32, T>`; mixed labeled/unlabeled legal). A label in a non-`bộ` list (`f<gx: T>(x)`, `lista<gx: T>`) is a parse error. Absence is the only unlabeled form; there is no `_: T` spelling. Keyword spellings are legal labels under the contextual law (`bộ<hằng: A>`).
- Labels are unique within one tuple type.
- Labels are erased from type identity: `bộ<gx: A, B> ≡ bộ<A, B>` for assignment, `≡`/`↦`, unify, and every emitter.
- Bracket index on a tuple requires a literal integer (`i[0]`); every element is reachable by position, labeled or not. Non-literal index expressions stay rejected. Positions are brackets only — no `.0`.
- Member-by-label (`i.gx`) requires that label to be present on the receiver's `bộ` annotation.
- `bộ` element slots admit `_` (monomorphic hole, solved element-wise from the single position witness) and reject `∪`. A wanted union element is declared with binary cup (`bộ<f32, textus ∪ rỗng>`). `lista<∪>` / `tabula<K, ∪>` keep heterogeneous-union behavior. Labels compose with holes (`bộ<loss: _, T>`).
- Arrays are written `lista<T>` (unbounded, shipped). Postfix `T[]` is not accepted. `lista<T, N>` is a proposed (not shipped) bounded form; see Generic Collections.
- `ra`/`vào` mark ownership (borrow/mut-borrow) on the immediately following union member. Parenthesize when grouping must be explicit.
- Two hole kinds share the `holeType` production. `_` is the monomorphic hole ("infer exactly one inhabitant type"); the standalone `∪` is the union hole ("infer a finite multi-member union"). Both are legal wherever a base type is: bindings, returns, params, fields, and type arguments (`lista<∪>`, `tabula<K, ∪>`, `→ ∪`).
- **Lone-`∪` rule:** a `∪` hole consumes the whole type expression — any following `∪` is a parse error (`A ∪ ∪`, `∪ B` rejected, issue `unexpected_cup_after_union_hole`). `_` keeps today's behavior and may still appear as a binary-cup member (`_ ∪ B`).
- **Binary-cup disambiguation:** `∪` between two non-hole types remains the inline value-union operator (`A ∪ B`, nullable `T ∪ rỗng`); the hole reading applies only when `∪` stands alone in a base-type position.
- Inline union `T ∪ U` (cup) for ad-hoc value unions; `T ∪ rỗng` is the canonical nullable type form (lowers to Option<T>).
- Unions are parsed as a flat member list; duplicates and `rỗng`-only cases are diagnosed in semantic lowering.
- `tự_nguyện` is a declaration marker (post-name on params/fields), never a prefix on types.
- Qualified type paths such as `terminus.Terminus` name a type through an
  imported namespace binding. The prefix must resolve to a namespace; the final
  segment must resolve to a type-bearing declaration.

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
| `textus<N>` | proposed — not shipped; bounded Unicode string; `N` is a `kích_thước` / `NATURAL` capacity, not a width marker. `textus<_>` is the capacity hole (infer `N`). |
| `ascii`    | ASCII-only string |
| `ascii<N>` | proposed — not shipped; bounded ASCII string; `N` is a `kích_thước` / `NATURAL` capacity, not a width marker. `ascii<_>` is the capacity hole (infer `N`). |
| `forma`    | captured template + params |
| `numerus`  | integer (default `i64`) |
| `modulus<W>` | unsigned modular word; arithmetic wraps modulo 2^W |
| `fractus`  | float (default `f64`) |
| `bivalens` | boolean |
| `rỗng`    | null |
| `vacuum`   | void |
| `numquam`  | never |
| `ignotum`  | unknown |
| `octeti`   | bytes |
| `octeti<N>` | proposed — not shipped; bounded byte buffer; `N` is a `kích_thước` / `NATURAL` capacity, not a width marker. `octeti<_>` is the capacity hole (infer `N`). |

Bare `textus` / `ascii` / `octeti` remain the unbounded productions. The
proposed (not shipped) forms `textus<N>`, `ascii<N>`, and `octeti<N>` take
one `kích_thước` / `NATURAL` applied argument. That `N` is capacity, not a
width marker and not a language-wide default. `_` in that slot (`ascii<_>`,
`textus<_>`, `octeti<_>`, `lista<T, _>`) is a capacity hole: the form stays
bounded, and `N` is inferred from a same-family bounded witness. Bare
`ascii` is not a hole.

Sized primitives accept one optional **width marker** (not a user type parameter):

| Family | Markers | Invalid example |
| ------ | ------- | --------------- |
| `numerus<W>` | `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64` | `numerus<f32>` → use `fractus<f32>` |
| `fractus<W>` | `f16`, `f32`, `f64` | `fractus<i32>` → use `numerus<i32>`; `bf16` is deferred |
| `modulus<W>` | `u8`, `u16`, `u32`, `u64` | `modulus<i32>` → signed widths are not modular words |

Bare `numerus` / `fractus` remain shorthand for `numerus<i64>` / `fractus<f64>`.
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

### Generic Collections

| Faber          | Meaning  |
| -------------- | -------- |
| `lista<T>`     | array    |
| `lista<T, N>`  | proposed — not shipped; bounded array; `N` is a `kích_thước` / `NATURAL` capacity, not a width marker. `lista<T, _>` is the capacity hole (infer `N`). |
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

Value unions use inline `T ∪ U` (nullable: `T ∪ rỗng`). The standalone `∪` hole infers a multi-member union; `_` infers a single inhabitant (see `docs/design/type-hole-union.md`). Tagged unions use `hợp_nhất`.
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


- `nếu` = if, `nếukhôngthì` = else-if, `khác` = else
- `do_đó` for one-statement bodies, including `do_đó trả`, `do_đó ném`, `do_đó chết`, and `do_đó im_lặng` (`∴` is not accepted here)
- `im_lặng` for explicit no-op (from musical notation: "it is silent")

### Loops


- `trong_khi` = while
- `lặp từ...hằng`/`lặp từ...biến` = for-of (values)
- `lặp ra...hằng`/`lặp ra...biến` = for-in (keys)
- `lặp khoảng range hằng/biến i` = range iteration (e.g. `lặp khoảng 0‥10 qua 2 hằng i { ghi_chú i }`; `qua` belongs to the range expression)

### Switch/Match


### Pattern Matching


### Guards


### Resource Management


### Destructuring Extraction


### Control Transfer


- `đợi_trả` awaits a compatible promise and returns its success value from a
  `async` function.
- `đợi_bỏ` awaits a compatible promise to completion and discards any success
  value.
- `nhường` is statement-initial yield from `sinh` / `async_sinh`; it is not an
  expression-form await.

---

## Error Handling


- `bắt` attaches to the structured forms whose productions name `catchClause`: conditional arms, `trong_khi`, `lặp`, `chọn`, `chăm_sóc`, and `làm`. It does not attach to arbitrary bare blocks.
- Use the explicit do block when a standalone block needs a handler: `làm { ... } bắt err { ... }`.
- `ném` = throw (recoverable), `chết` = panic (fatal).
- A same-line `nếu <expr>` guard on `ném` and `chết` is line-sensitive parser sugar: `ném val nếu cond` desugars to `nếu cond { ném val }` at parse time. Its canonical, compression-safe spelling is the expanded `nếu` block. A source compressor must expand this sugar before removing line breaks; the guarded shorthand remains under language review.
- `khẳng_định` is a runtime invariant check. It desugars conceptually to `chết "msg" nếu !cond`, with the positive condition kept in source form and the inversion applied during lowering. The optional particle is `chết` (en `panic`): `khẳng_định cond chết msg` / `assert cond panic msg`. Bare `khẳng_định cond` stays legal. An `khẳng_định` failure is fatal and uncatchable by `bắt` (it lowers to a panic, not a `Result`-channel error); in test context the harness isolates each `kiểm_thử` so a failed assertion ends that test without ending the suite.
- `yêu_cầu` is the recoverable require statement (en surface `require … throw …`), the typed-error-channel twin of `khẳng_định`. `yêu_cầu cond ném err` desugars to `nếu không (cond) { ném err }` at lowering; the thrown value enters the function's `⇥ E` channel and is catchable by `bắt`/`làm`, unlike `khẳng_định` (fatal). A `yêu_cầu` statement in a `⇥`-less function is a compile error, same as `ném`. The particle is `ném` (en `throw`) and is required.

---

## Expressions

### Operators (by precedence, lowest to highest)


**Conversion-directed assignment (`↤` / conversio-assign):** `place ↤ value`
evaluates the right side, converts it to the statically known type of the left
place through the existing `↦` route, then assigns. It binds at the same
precedence as `←` and is right-associative; `⇥ inlineRecovery` is **legal only
on `↤`** — a `⇥` recovery after ordinary `←` is rejected, and in a
right-associated `↤` chain the recovery attaches to the nearest `↤`. The
operator is preserved verbatim through syntax and emission; it is never
rewritten to `←` or `↦`. Typed `hằng`/`biến` initializers accept `↤`
(convert to the written type, then initialize); `hằng _`, `đặt`, and untyped
destructuring have no concrete destination and are rejected.

`là` and `không là` inspect an existing value; they never convert it. Core type
spellings on the right perform runtime variant/type tests, while `rỗng`,
`đúng`, `sai`, and ordinary value expressions use the value-test path. Radix
currently recognizes type targets through a fixed core-type vocabulary. Extending
that recognition to arbitrary declared types is a separate language decision.
Use `≡` / `≠` for structural value equality and `↦` for runtime conversion.

Retired predicate keywords are not prefix unary syntax. Use `expr là đúng`,
`expr là sai`, `expr là rỗng`, `expr không là rỗng`, `expr ≺ 0`, or
`expr ≻ 0`.

**Static type ascription (`∷` / verte):**

The `∷` glyph (U+2237, "proportion") explicitly ascribes a target type to an expression. Use it when the source expression already exists and the compiler needs a static target shape:

- Primitive/alias → cast (no runtime effect): `data ∷ textus` → TypeScript: `(data as string)`
- Built-in collection → target-shaped collection value: `[1, 2, 3] ∷ lista<numerus>`
- Variant expression → enum/interface target ascription: `tạo Click { x = 10 } ∷ Event`

Prefer typed construction for ordinary `kiểu` values and `vacua` for ordinary empty collection values:

```fab
fixum _ point ← Point { x = 10 }
fixum lista<numerus> xs ← vacua
```

Only the `∷` glyph is accepted as the postfix static type-ascription operator. The Latin forms `qua`, `innatum`, and `novum` were aliases and have been removed (see verte-alias-clean-break).

**Runtime conversion (`↦` / conversio):**

The `↦` glyph (U+21A6, "rightwards arrow from bar") is the runtime value conversion operator. Unlike `∷` (compile-time cast), this performs actual parsing/conversion that can fail:

- `"22" ↦ numerus` → Rust: `"22".parse::<i64>().unwrap()`
- `"bad" ↦ numerus ⇥ 0` → Rust: `"bad".parse::<i64>().unwrap_or(0)`
- `42 ↦ textus` → Rust: `42.to_string()`

The second type argument of a `↦` target is the convert-hint slot. `Hex` / `Bin` / `Oct` / `Be` / `Le` are convert hints in that slot, not keywords and not new `baseType` productions. Target support is not a grammar production (see Target Support).

- `"ff" ↦ numerus<i32, Hex>` — shipped; text parse at radix 16 (`Bin` = 2, `Oct` = 8). Hex/Bin/Oct text parse is unchanged by endian hints.
- `octeti[lo‥hi] ↦ numerus<W, Be>` / `… ↦ numerus<W, Le>` — endian unpack of an exact-width window (`W` is `i16` / `i32` / `i64` / `u16` / `u32` / `u64`; window length 2 / 4 / 8). Shipped on rust, the MIR runner, Go, and TypeScript. TypeScript `i64`/`u64` stay fail-closed (JS number is not exact). English `int<W, Be>` is the same form. `octeti` itself has no endian; `bytes ↦ numerus<u32>` without `Be`/`Le` stays rejected. A short window fails (no pad).
- `n ↦ octeti<N, Be>` / `… ↦ octeti<N, Le>` — proposed (not shipped); write convert after `octeti<N>` (`N` ∈ {2, 4, 8}). `Be`/`Le` stay Hex-slot hints, not a second capacity.

Inline failure recovery uses `⇥` immediately after the conversio target (`↦ T ⇥ recovery-expr`). The unparenthesized recovery operand is a unary-precedence expression; parenthesize arithmetic, coalescing, ternary, or assignment recovery expressions. The recovery value must have type `T`.

Using `hoặc_nếu_rỗng` as conversio recovery is rejected with a migration diagnostic. `hoặc_nếu_rỗng` is local nullable elimination only (`x hoặc_nếu_rỗng y`, parameter defaults) — not logical `hoặc`. A parenthesized conversio result may still combine with `hoặc_nếu_rỗng` as ordinary defaulting.

### Call and Member Access


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
`văn_bản_hóa("...", args...)`.

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
`văn_bản_hóa("§ world", "salve")` form.

This lowers to the compiler's `văn_bản_hóa("...", args...)` form. Use the string-template form in ordinary source; reserve `văn_bản_hóa(...)` for explicit desugaring examples and compiler-facing documentation.

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

Text slices accept the full range form, including `qua`.

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
is `accipe` sugar and returns `T ∪ rỗng`. For nullable list access, use
`xs.accipe(i) → T ∪ rỗng` with `hoặc_nếu_rỗng`.

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

Reads return `T ∪ rỗng`, matching `accipe`; use `hoặc_nếu_rỗng` or another ordinary
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
index on an octeti literal (`|ra gọi be ef|[0‥5]`) is a structured reject.
Runtime out-of-bounds traps — the same trapping model as lista bracket access,
not textus short-slice. Lista `[lo‥hi]` stays rejected.

`octeti` is the endian host. Parse byte windows on the buffer
(`buf[lo‥hi] ↦ numerus<W, Be|Le>`). Cross to a list once, for element work,
via `octeti ↦ lista<numerus<u8>>` (representation change only; other element
types fail closed). The reverse `lista<numerus<u8>> ↦ octeti` is live. Do not
detour through `valor`. Lists stay for element work, not endian windows.

### Primary Expressions

`vacua` is a contextual empty-collection marker (identifier form, not a reserved keyword).
Use it with an explicit collection type: `hằng lista<numerus> xs ← vacua` or `hằng tensor<fractus<f32>, []> t ← vacua`.


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

### Special Expressions


`văn_bản_hóa` and `đọc`/`dòng` are builtin claims that resolve to a user binding
when the surface spelling is bound in scope (parameter, local, function, or any
in-scope definition); otherwise they are the builtin. The same binding-wins rule
applies to `văn_bản_hóa`'s paren-claimed form and to the `vacua` empty-collection
marker: builtin claims are defaults, not reservations.

`tạo` variant construction accepts a qualified variant path
(`tạo pkg.Bonum { … }`), so an imported union's variants construct through
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


The scribe family (`ghi_chú`/`xem`/`cảnh_báo`/`viết` — en `print`/`debug`/`warn`/`write`)
claims the statement-initial position only when **not** immediately followed by
`(`. `ghi_chú expr` is the output statement; a statement-initial `ghi_chú(...)` is an
expression statement whose callee is the identifier `ghi_chú` — a user function
call, never the intrinsic.

- `ghi_chú` = neutral diagnostic note, `xem` = debug/inspect, `cảnh_báo` = warn
- `viết` is a diagnostic channel spelling; use current stdlib methods for real output

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


- `bắt_đầu` = sync entry, `bắt_đầu_bất_đồng_bộ` = async entry.
- `đối_số` binds parsed command-line arguments; `thoát` supplies the process exit expression. Their order is fixed by `entryHeader`.

---

## Testing


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

Expression-form `gọi` is the only supported `gọi` surface. Legacy typed
`gọi "route" (args) → T { }` and statement-level stream blocks
`gọi 'route' { meus/tuus … }` are rejected at parse time.

The active `adExpr` production is defined under **Primary Expressions**. Its
ordinary postfix `conversio` materializes the resulting conversation handle.

- Route: `ASCII_STRING` (`'chỉ:đọc'`), not double-quoted `STRING`.
- Opener: optional single `expression` → Request `data` as `valor`.
- **Expression `gọi`**: blockless; evaluates to a `sermo` conversation handle.
  Use postfix `↦ T` (materialization), assign to `sermo`, or open live directional
  views: `s.meus<T>()` (outbound `da` / `fini`) and `s.tuus<T>()` (inbound
  `accipe` / `cursor` / `exhauri` / `fini`). Iterate inbound content frames with
  `s.tuus<T>().cursor()`, not direct `lặp từ s.tuus<T>()`.
- **Removed (parse error):** legacy typed `gọi "route"` and block `meus`/`tuus` arms.
- Types: compiler-owned `scrinium`, `status`; opaque `sermo` conversation handle.
- `sermo ↦ T` materializes inbound frames into one value of type `T` using
  the type-directed collector for `T`.

See [`docs/design/frame-stream-types.md`](docs/design/frame-stream-types.md).

---

## Collection Operations

The former `khoảng` collection pipeline DSL is retired. Collection filtering,
slicing, and aggregation are expressed through ordinary
`textus`/`lista`/`tabula`/`copia` methods and closures instead of a
grammar-level query expression. `textus`, `numerus`, `fractus`, `lista<T>`,
`tabula<K,V>`, and `copia<T>` are compiler-owned core types; their method
surfaces are not Norma declarations.

`prima` and `ultima` are ordinary method names, not transform keywords. `ubi` is
not active collection syntax.

`từ` is used for iteration (`lặp từ items hằng x`) and imports (`nhập từ "path"`).

---

## Fac Block


- `làm { ... }` is the explicit `do` block and executes its body once.
- `làm { ... } trong_khi condition` is the post-test loop form; postfix `trong_khi` attaches only to `làm`, not arbitrary preceding blocks.
- `bắt` is an attachment shared by several structured forms, not a semantic mode owned by `làm`. A plain `làm` is often used when an otherwise unattached block needs a local handler: `làm { ... } bắt err { ... }`.

---

## Target Support

Target support is **not** part of the grammar — this file defines only the
language. For which grammar each compilation target lowers, and the runtime
policy around it, see:

- [`EBNF_MATRIX.md`](EBNF_MATRIX.md) — generated grammar×target lowerability matrix (the official rows).
- [`docs/design/target-capability-matrix.md`](docs/design/target-capability-matrix.md) — runtime/contract policy (erase/warn/defer), pipeline routing, per-target contracts.

---

## Critical Syntax Rules

1. **Type-first parameters**: `hàm f(numerus x)` NOT `hàm f(x: numerus)`
2. **Type-first declarations**: `hằng textus name` NOT `hằng name: textus`
3. **Iteration loops**: `lặp từ/ra collection hằng/biến item { }` or `lặp khoảng range hằng/biến item { }` (verb-first, source, then binding)
4. **Parentheses around conditions are valid but not idiomatic**: prefer `nếu x ≻ 0 { }` or `nếu flag là đúng { }` over `nếu (x ≻ 0) { }`
5. **Scribe-family keywords claim statement-initial position only when not followed by `(`** — `ghi_chú x` is the output statement; a statement-initial `ghi_chú(x)` is a call to the identifier `ghi_chú`
